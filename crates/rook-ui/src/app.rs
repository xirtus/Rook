//! Main egui application struct — holds shared engine and coordinates panels.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rook_core;
use rook_engine::Engine;
use rook_ipc::server::IpcServer;

use crate::audio::AudioBridge;
use crate::panels::{
    GalleryPanel, InspectorPanel, MarkerListPanel, MulticamPanel, PluginBrowserPanel, PreviewPanel,
    TimelinePanel, VuMeterPanel, import_srt,
};

fn last_valid_timeline_frame(duration: i64) -> i64 {
    duration.saturating_sub(1).max(0)
}

/// Result sent back from a background file-dialog thread.
enum FileDialogResult {
    ImportMedia(Vec<PathBuf>),
    ImportFolder(PathBuf),
    OpenProject(PathBuf),
    SaveAs(PathBuf),
    ImportEdl(PathBuf),
    ImportImovie(PathBuf),
    ImportSrt(PathBuf),
}

// ── osascript-backed file pickers ─────────────────────────────────────────
// rfd (both sync and async) triggers NSOpenPanel inside our process, which
// creates a nested CFRunLoop that panics winit 0.30's control_flow observer.
// Running the dialog via `osascript` in a subprocess keeps NSOpenPanel
// entirely out of our process — winit never sees it.

/// Pick one or more files via osascript. Returns POSIX paths, one per line.
fn osascript_pick_files(type_filter: &str) -> Vec<PathBuf> {
    let script = format!(
        "set sel to choose file of type {{{}}} with multiple selections allowed\n\
         set out to \"\"\n\
         repeat with f in sel\n  set out to out & POSIX path of f & \"\\n\"\nend repeat\n\
         return out",
        type_filter
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok();
    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| PathBuf::from(l.trim()))
                .collect()
        })
        .unwrap_or_default()
}

/// Pick a single file via osascript.
fn osascript_pick_file(type_filter: &str) -> Option<PathBuf> {
    let script = format!("POSIX path of (choose file of type {{{}}})", type_filter);
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// Pick a folder via osascript.
fn osascript_pick_folder() -> Option<PathBuf> {
    let output = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose folder)"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// Pick a save location via osascript.
fn osascript_save_file(default_name: &str) -> Option<PathBuf> {
    let script = format!(
        "POSIX path of (choose file name default name \"{}\")",
        default_name
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// Path to the recent projects file.
fn recent_projects_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Rook").join("recent.json"))
}

/// Path to the auto-save directory for crash recovery.
fn autosave_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Rook").join("autosave"))
}

/// Load recent projects from disk.
fn load_recent_projects() -> Vec<(String, String)> {
    let path = recent_projects_path().unwrap_or_default();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<(String, String)>>(&data) {
                return list;
            }
        }
    }
    Vec::new()
}

/// Save recent projects to disk.
fn save_recent_projects(list: &[(String, String)]) {
    if let Some(path) = recent_projects_path() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(list) {
            std::fs::write(&path, json).ok();
        }
    }
}

/// Add a recently opened project, deduplicating and limiting to 10.
fn add_recent(list: &mut Vec<(String, String)>, name: String, path: String) {
    list.retain(|(_, p)| p != &path);
    list.insert(0, (name, path));
    if list.len() > 10 {
        list.truncate(10);
    }
    save_recent_projects(list);
}

pub struct RookApp {
    engine: Arc<Mutex<Engine>>,
    ipc_server: Option<IpcServer>,

    // Audio subsystem
    audio_bridge: Option<AudioBridge>,

    // Panels
    gallery: GalleryPanel,
    timeline: TimelinePanel,
    preview: PreviewPanel,
    inspector: InspectorPanel,
    markers: MarkerListPanel,
    multicam: MulticamPanel,
    vu_meter: VuMeterPanel,
    plugin_browser: PluginBrowserPanel,

    // Layout
    show_gallery: bool,
    show_inspector: bool,
    show_markers: bool,
    show_multicam: bool,
    show_vu_meter: bool,
    show_export_dialog: bool,
    /// Event library sidebar.
    show_event_library: bool,
    /// Plugin browser sidebar.
    show_plugin_browser: bool,

    // Transport
    playing: bool,
    playhead: i64,
    /// Loop playback between I/O marks.
    looping: bool,

    /// Recent projects (name, path).
    recent_projects: Vec<(String, String)>,

    /// Light mode toggle.
    light_mode: bool,

    /// Accessibility: high contrast mode.
    high_contrast: bool,
    /// Accessibility: larger UI text.
    large_text: bool,
    /// Accessibility: reduce motion/animations.
    reduce_motion: bool,

    /// Accent color (0-255 RGB).
    accent_color: [u8; 3],

    /// Batch export queue — list of (path, preset) tuples.
    batch_queue: Vec<(std::path::PathBuf, String)>,
    /// Whether batch export is in progress.
    batch_exporting: bool,
    /// Timestamp of last auto-save (for crash recovery).
    last_autosave: f64,
    /// Whether to show the crash recovery dialog.
    show_recovery_dialog: bool,
    /// Path to recovered auto-save file.
    recovery_path: Option<PathBuf>,

    /// Wall-clock time of the last playback tick, for frame-rate-correct advancement.
    playback_last_tick: Option<std::time::Instant>,
    /// Sub-frame accumulator so fractional frames don't get lost between repaints.
    playback_frame_accum: f64,
    /// Previous playing state — used to detect play/pause transitions for audio sync.
    prev_playing: bool,

    /// Receives the result of a background file-dialog thread.
    /// Only one dialog can be open at a time.
    file_dialog_rx: Option<std::sync::mpsc::Receiver<FileDialogResult>>,
}

impl RookApp {
    pub fn new(engine: Arc<Mutex<Engine>>, ipc_server: Option<IpcServer>) -> Self {
        let playhead = 0i64; // Always start at frame 0 so clips are immediately visible

        // Check for crash recovery autosave
        let (show_recovery_dialog, recovery_path) = if let Some(adir) = autosave_dir() {
            let autosave_path = adir.join("recovery.rook");
            if autosave_path.exists() {
                (true, Some(autosave_path))
            } else {
                (false, None)
            }
        } else {
            (false, None)
        };

        Self {
            engine,
            ipc_server,
            audio_bridge: {
                match AudioBridge::new() {
                    Ok(bridge) => Some(bridge),
                    Err(e) => {
                        eprintln!("[rook] AudioBridge init FAILED: {e} — audio disabled");
                        None
                    }
                }
            },
            gallery: GalleryPanel::default(),
            timeline: TimelinePanel::default(),
            preview: PreviewPanel::default(),
            inspector: InspectorPanel::default(),
            markers: MarkerListPanel::default(),
            multicam: MulticamPanel::default(),
            vu_meter: VuMeterPanel::default(),
            plugin_browser: PluginBrowserPanel::default(),
            show_gallery: false,
            show_inspector: true,
            show_markers: false,
            show_multicam: false,
            show_vu_meter: true,
            show_export_dialog: false,
            show_event_library: true,
            show_plugin_browser: false,
            playing: false,
            playhead,
            looping: false,
            recent_projects: load_recent_projects(),
            light_mode: false,
            high_contrast: false,
            large_text: false,
            reduce_motion: false,
            accent_color: [80, 140, 220],
            batch_queue: Vec::new(),
            batch_exporting: false,
            last_autosave: 0.0,
            show_recovery_dialog,
            recovery_path,
            playback_last_tick: None,
            playback_frame_accum: 0.0,
            prev_playing: false,
            file_dialog_rx: None,
        }
    }
}

/// Spawn a background thread that runs a blocking file-picker function and
/// sends the result back. The picker function runs in the background thread
/// and must not touch the UI — use the osascript_* helpers above, which
/// open dialogs in a separate process entirely outside the winit event loop.
fn spawn_file_dialog<F>(ctx: &egui::Context, f: F) -> std::sync::mpsc::Receiver<FileDialogResult>
where
    F: FnOnce() -> Option<FileDialogResult> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        if let Some(result) = f() {
            tx.send(result).ok();
        }
        ctx.request_repaint();
    });
    rx
}

impl eframe::App for RookApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Process any completed file dialog ──────────────────────────────
        if let Some(rx) = &self.file_dialog_rx {
            if let Ok(result) = rx.try_recv() {
                self.file_dialog_rx = None;
                let mut engine = self.engine.lock().unwrap();
                let t_dlg = std::time::Instant::now();
                match result {
                    FileDialogResult::ImportMedia(files) => {
                        import_files(&mut engine, &files);
                        eprintln!("[update] import_files total took {:?}", t_dlg.elapsed());
                        self.playhead = 0;
                        engine.project_mut().timeline.playhead = 0;
                    }
                    FileDialogResult::ImportFolder(folder) => {
                        let exts = [
                            "mp4", "mov", "m4v", "mkv", "webm", "mp3", "wav", "aac", "flac", "jpg",
                            "jpeg", "png", "bmp", "tiff", "srt", "vtt",
                        ];
                        let mut files = Vec::new();
                        if let Ok(entries) = std::fs::read_dir(&folder) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_file() {
                                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                        if exts.contains(&ext.to_lowercase().as_str()) {
                                            files.push(path);
                                        }
                                    }
                                }
                            }
                        }
                        if !files.is_empty() {
                            import_files(&mut engine, &files);
                            self.playhead = 0;
                            engine.project_mut().timeline.playhead = 0;
                        }
                    }
                    FileDialogResult::OpenProject(path) => {
                        drop(engine);
                        match rook_engine::Engine::open_project(&path) {
                            Ok(e) => {
                                let path_str = path.display().to_string();
                                let name = e.project().name.clone();
                                // Reset playhead to 0 so we start at the beginning of the content
                                self.playhead = 0;
                                *self.engine.lock().unwrap() = e;
                                add_recent(&mut self.recent_projects, name, path_str);
                            }
                            Err(err) => tracing::error!(?err, "open failed"),
                        }
                    }
                    FileDialogResult::SaveAs(path) => match engine.save_project(Some(&path)) {
                        Ok(saved) => {
                            let path_str = saved.display().to_string();
                            let name = engine.project().name.clone();
                            add_recent(&mut self.recent_projects, name, path_str);
                        }
                        Err(err) => tracing::error!(?err, "save as failed"),
                    },
                    FileDialogResult::ImportEdl(path) => {
                        match import_edl_file(&mut engine, &path) {
                            Ok(count) => tracing::info!(count, ?path, "EDL imported"),
                            Err(err) => tracing::error!(?err, "EDL import failed"),
                        }
                    }
                    FileDialogResult::ImportImovie(path) => {
                        match import_imovie_file(&mut engine, &path) {
                            Ok(count) => tracing::info!(count, ?path, "iMovie imported"),
                            Err(err) => tracing::error!(?err, "iMovie import failed"),
                        }
                    }
                    FileDialogResult::ImportSrt(path) => {
                        let fps = engine.project().frame_rate.as_f64();
                        match import_srt(&path, fps) {
                            Ok((_entries, clips)) => {
                                if engine
                                    .project()
                                    .timeline
                                    .tracks_of_kind(rook_core::track::TrackKind::Text)
                                    .is_empty()
                                {
                                    engine.project_mut().add_text_track("Subtitles 1");
                                }
                                if let Some(track_id) = engine
                                    .project()
                                    .timeline
                                    .tracks
                                    .iter()
                                    .find(|t| t.kind == rook_core::track::TrackKind::Text)
                                    .map(|t| t.id)
                                {
                                    for (_frame, clip) in clips {
                                        if let Some(track) =
                                            engine.project_mut().timeline.track_mut(track_id)
                                        {
                                            let _ = track.insert_clip(clip);
                                        }
                                    }
                                }
                                tracing::info!(?path, "SRT imported");
                            }
                            Err(err) => tracing::error!(?err, "SRT import failed"),
                        }
                    }
                }
            }
        }

        // Apply light/dark mode with accessibility modifications
        let mut visuals = if self.light_mode {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        if self.high_contrast {
            // Boost contrast: darker darks, brighter brights, thicker outlines
            if self.light_mode {
                visuals.widgets.noninteractive.bg_fill = egui::Color32::WHITE;
                visuals.widgets.inactive.bg_fill = egui::Color32::from_gray(240);
                visuals.widgets.inactive.fg_stroke = egui::Stroke::new(2.0, egui::Color32::BLACK);
                visuals.widgets.active.fg_stroke = egui::Stroke::new(2.0, egui::Color32::BLACK);
            } else {
                visuals.widgets.noninteractive.bg_fill = egui::Color32::BLACK;
                visuals.widgets.inactive.bg_fill = egui::Color32::from_gray(30);
                visuals.widgets.inactive.fg_stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
                visuals.widgets.active.fg_stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
            }
        }
        if self.large_text {
            // Scale UI by setting a global zoom factor
            ctx.set_pixels_per_point(1.5);
        }
        ctx.set_visuals(visuals);

        // Reduce motion: skip animation effects
        if self.reduce_motion {
            // egui animations are built-in; we request no continuous repaint
        }

        // Lock engine for this frame
        let mut engine = self.engine.lock().unwrap();

        // Poll proxy builds and IPC
        engine.proxy().tick();
        drop(engine); // release lock for IPC poll
        if let Some(ref mut ipc) = self.ipc_server {
            ipc.poll();
        }
        let mut engine = self.engine.lock().unwrap();

        // Playback: advance playhead using real elapsed time so speed is
        // correct at any fps regardless of the egui repaint rate.
        if self.playing {
            let fps = engine.project().frame_rate.as_f64();
            let duration = engine.project().timeline.duration();
            let max_frame = last_valid_timeline_frame(duration);
            let now = std::time::Instant::now();

            // If this is the first tick after play started, advance by
            // at least 1 frame immediately so the user sees a response.
            // Without this, the accumulator needs 2-3 repaints (~50ms) to
            // accumulate enough fractional time for the first frame.
            let first_tick = self.playback_last_tick.is_none();
            if first_tick {
                self.playback_last_tick = Some(now);
                let next = self.playhead + 1;
                if self.looping {
                    let loop_start = engine.project().timeline.in_point.unwrap_or(0);
                    let loop_end = engine.project().timeline.out_point.unwrap_or(duration);
                    if loop_end > loop_start && next >= loop_end {
                        self.playhead = loop_start;
                    } else {
                        self.playhead = next.min(max_frame);
                    }
                } else {
                    self.playhead = next.min(max_frame);
                    if self.playhead >= max_frame {
                        self.playing = false;
                    }
                }
                engine.project_mut().timeline.playhead = self.playhead;
            } else {
                let elapsed = now
                    .duration_since(self.playback_last_tick.unwrap())
                    .as_secs_f64();
                self.playback_last_tick = Some(now);

                self.playback_frame_accum += elapsed * fps;
                let advance = self.playback_frame_accum.floor() as i64;
                self.playback_frame_accum -= advance as f64;

                let next = self.playhead + advance;

                // Loop: wrap around when playhead reaches out-point
                if self.looping {
                    let loop_start = engine.project().timeline.in_point.unwrap_or(0);
                    let loop_end = engine.project().timeline.out_point.unwrap_or(duration);
                    if loop_end > loop_start {
                        if next >= loop_end {
                            let loop_len = (loop_end - loop_start).max(1);
                            self.playhead = loop_start + ((next - loop_start).rem_euclid(loop_len));
                        } else {
                            self.playhead = next.clamp(loop_start, max_frame);
                        }
                    } else {
                        self.playhead = next.min(max_frame);
                    }
                } else {
                    self.playhead = next.min(max_frame);
                    if self.playhead >= max_frame {
                        self.playing = false;
                    }
                }
                engine.project_mut().timeline.playhead = self.playhead;
            }
        } else {
            self.playback_last_tick = None;
            self.playback_frame_accum = 0.0;
        }

        // ── Audio sync ───────────────────────────────────────────────
        if let Some(ref mut audio) = self.audio_bridge {
            let fps = engine.project().frame_rate.as_f64();
            audio.set_fps(fps);

            // ── Probe audio assets (fast — metadata only, no PCM decode) ──
            // Safe to call every frame; returns immediately for already-probed assets.
            for track in &engine.project().timeline.tracks {
                if track.kind != rook_core::track::TrackKind::Audio {
                    continue;
                }
                for clip in &track.clips {
                    if !audio.has_asset(clip.asset_id) {
                        if let Some(asset) = engine.project().asset(clip.asset_id) {
                            let path = std::path::PathBuf::from(asset.path());
                            if path.exists() {
                                let _ = audio.probe(clip.asset_id, &path);
                            }
                        }
                    }
                }
            }

            // ── Play/pause transitions ──────────────────────────────
            if self.playing && !self.prev_playing {
                // Just started — clear buffer and begin output
                audio.set_playhead_frame(self.playhead);
                audio.play();
            } else if !self.playing && self.prev_playing {
                audio.pause();
            }

            // ── Continuous feeding during playback ──────────────────
            if self.playing {
                audio.set_playhead_frame(self.playhead);
                let playhead_secs = self.playhead as f64 / fps;

                // Feed audio from the first active audio clip at the playhead
                for track in &engine.project().timeline.tracks {
                    if track.kind != rook_core::track::TrackKind::Audio {
                        continue;
                    }
                    if track.muted || !track.visible {
                        continue;
                    }
                    for clip in &track.clips {
                        let clip_end = clip.timeline_in + clip.duration();
                        if clip.timeline_in <= self.playhead && self.playhead < clip_end {
                            if audio.has_asset(clip.asset_id) {
                                // Map timeline position to source position
                                let source_frame = clip.timeline_to_source(self.playhead).unwrap_or(0);
                                let source_secs = source_frame as f64 / fps;
                                audio.feed_audio_at(clip.asset_id, source_secs);
                            }
                            break; // Only feed the first matching clip per frame
                        }
                    }
                }
            }

            self.prev_playing = self.playing;
        }

        // ── App-level keyboard shortcuts ───────────────────────────
        let app_input = ctx.input(|i| i.clone());
        let cmd = app_input.modifiers.command || app_input.modifiers.ctrl;
        if app_input.key_pressed(egui::Key::L) && cmd {
            self.looping = !self.looping;
        }
        if app_input.key_pressed(egui::Key::F) && cmd && app_input.modifiers.shift {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        // Panel toggles: Cmd+Shift+1/2/3
        if app_input.key_pressed(egui::Key::Num1) && cmd && app_input.modifiers.shift {
            self.show_gallery = !self.show_gallery;
        }
        if app_input.key_pressed(egui::Key::Num2) && cmd && app_input.modifiers.shift {
            self.show_inspector = !self.show_inspector;
        }
        if app_input.key_pressed(egui::Key::Num3) && cmd && app_input.modifiers.shift {
            self.show_markers = !self.show_markers;
        }
        // Cmd+E = export
        if app_input.key_pressed(egui::Key::E) && cmd && !app_input.modifiers.shift {
            self.show_export_dialog = true;
        }
        // Cmd+0 = toggle viewer 100% zoom
        if app_input.key_pressed(egui::Key::Num0) && cmd {
            self.preview.toggle_fit_vs_100();
        }
        // / key = Play Selected — jump to selected clip start and play
        if app_input.key_pressed(egui::Key::Slash) && !cmd {
            if let Some((start, _end)) = self.preview.play_selected(&engine) {
                self.playhead = start;
                self.playing = true;
                engine.project_mut().timeline.playhead = start;
            }
        }
        // Shift+Space = Play from Start
        if app_input.key_pressed(egui::Key::Space) && app_input.modifiers.shift && !cmd {
            self.playhead = 0;
            self.playing = true;
            engine.project_mut().timeline.playhead = 0;
        }
        // Multicam angle switching with 1-9 keys
        let mc_keys = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ];
        for (i, key) in mc_keys.iter().enumerate() {
            if app_input.key_pressed(*key) && !cmd {
                let selected = engine.project().timeline.selected_clip_ids.clone();
                if let Some(cid) = selected.first() {
                    if engine.project().multicam_for_clip(*cid).is_some() {
                        let cmd_mc = rook_core::commands::EditCommand::SwitchMulticamAngle {
                            clip_id: *cid,
                            angle_index: i,
                        };
                        engine.apply(cmd_mc).ok();
                    }
                }
            }
        }
        // Multicam next/prev angle: ⌘→ / ⌘←
        if app_input.key_pressed(egui::Key::ArrowRight) && cmd && !app_input.modifiers.shift {
            let selected = engine.project().timeline.selected_clip_ids.clone();
            if let Some(cid) = selected.first() {
                if let Some(mc) = engine.project().multicam_for_clip(*cid) {
                    let next = (mc.active_angle_index + 1) % mc.angle_count().max(1);
                    let cmd_mc = rook_core::commands::EditCommand::SwitchMulticamAngle {
                        clip_id: *cid,
                        angle_index: next,
                    };
                    engine.apply(cmd_mc).ok();
                }
            }
        }
        if app_input.key_pressed(egui::Key::ArrowLeft) && cmd && !app_input.modifiers.shift {
            let selected = engine.project().timeline.selected_clip_ids.clone();
            if let Some(cid) = selected.first() {
                if let Some(mc) = engine.project().multicam_for_clip(*cid) {
                    let prev = if mc.active_angle_index == 0 {
                        mc.angle_count().saturating_sub(1)
                    } else {
                        mc.active_angle_index - 1
                    };
                    let cmd_mc = rook_core::commands::EditCommand::SwitchMulticamAngle {
                        clip_id: *cid,
                        angle_index: prev,
                    };
                    engine.apply(cmd_mc).ok();
                }
            }
        }

        // Undo/Redo keyboard shortcuts (engine already locked above)
        if app_input.key_pressed(egui::Key::Z) && cmd && app_input.modifiers.shift {
            engine.redo();
        } else if app_input.key_pressed(egui::Key::Z) && cmd {
            engine.undo();
        }

        // ── Periodic auto-save for crash recovery ─────────────────────
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        if now_secs - self.last_autosave > 60.0 && engine.db_path().is_some() {
            self.last_autosave = now_secs;
            if let Some(autosave_dir) = autosave_dir() {
                std::fs::create_dir_all(&autosave_dir).ok();
                let autosave_path = autosave_dir.join("recovery.rook");
                // Clone the current db to autosave location
                if let Some(db_path) = engine.db_path() {
                    if db_path.exists() {
                        let _ = std::fs::copy(db_path, &autosave_path);
                    }
                }
                // Also write a lightweight timestamp file
                let _ = std::fs::write(
                    autosave_dir.join("recovery.meta"),
                    format!("{}\n{}", engine.project().name, now_secs as i64),
                );
            }
        }

        // Snapshot state needed by menu bar (read-only, before releasing lock)
        let db_path_info = engine.db_path().cloned();
        let project_name = engine.project().name.clone();
        let can_undo = engine.can_undo();
        let can_redo = engine.can_redo();
        drop(engine); // release lock for menu bar closures

        // ── Handle file drops (drag & drop from Finder) ─────────────────
        let drops: Vec<PathBuf> = ctx
            .input(|i| i.raw.dropped_files.clone())
            .into_iter()
            .map(|f| f.path.clone())
            .filter(|p| p.is_some())
            .map(|p| p.unwrap())
            .collect();
        if !drops.is_empty() {
            let mut e = self.engine.lock().unwrap();
            let exts = [
                "mp4", "mov", "m4v", "mkv", "webm", "mp3", "wav", "aac", "flac", "jpg", "jpeg",
                "png", "bmp", "tiff", "srt", "vtt", "rook",
            ];
            let mut media_files = Vec::new();
            let mut rook_files = Vec::new();
            for path in &drops {
                let is_media = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| exts.contains(&ext.to_lowercase().as_str()) && ext != "rook")
                    .unwrap_or(false);
                let is_rook = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| ext == "rook")
                    .unwrap_or(false);
                if is_rook {
                    rook_files.push(path.clone());
                } else if is_media {
                    media_files.push(path.clone());
                }
            }
            if !media_files.is_empty() {
                import_files(&mut e, &media_files);
                // Reset playhead to 0 so user sees the start of imported content
                e.project_mut().timeline.playhead = 0;
                self.playhead = 0;
                tracing::info!("drag-dropped {} files onto timeline", media_files.len());
            }
            drop(e);
            // Open first .rook file dropped
            if !rook_files.is_empty() {
                if let Some(path) = rook_files.first() {
                    match Engine::open_project(path) {
                        Ok(new_e) => {
                            let path_str = path.display().to_string();
                            let name = new_e.project().name.clone();
                            *self.engine.lock().unwrap() = new_e;
                            add_recent(&mut self.recent_projects, name, path_str);
                            tracing::info!(?path, "project opened via drag-drop");
                        }
                        Err(err) => tracing::error!(?err, "drag-drop open failed"),
                    }
                }
            }
        }

        // ── Menu bar ────────────────────────────────────────────────────
        let engine_arc = self.engine.clone();
        let mut open_export = false;
        // Capture recents as Rc<RefCell<...>> for interior mutability inside menu closures
        let recents_rc = std::rc::Rc::new(std::cell::RefCell::new(self.recent_projects.clone()));
        // Channel receiver produced by any dialog spawned from the File menu.
        type DialogRx = std::sync::mpsc::Receiver<FileDialogResult>;
        let dialog_rx_cell: std::rc::Rc<std::cell::RefCell<Option<DialogRx>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                {
                    let eng = engine_arc.clone();
                    let recents = recents_rc.clone();
                    let ctx_ref = ui.ctx().clone();
                    let dialog_cell = dialog_rx_cell.clone();
                    ui.menu_button("File", move |ui| {
                        // Helper used in this closure to set the pending dialog receiver.
                        let mut self_file_dialog_rx: Option<DialogRx> = None;
                        let ctx_ref = &ctx_ref;
                        if ui.button("🆕 New Project").clicked() {
                            let mut e = eng.lock().unwrap();
                            let canvas = rook_core::canvas::Canvas::default();
                            let fps = rook_core::time::Rational::FPS_24;
                            *e = Engine::new("Untitled", canvas, fps);
                            e.project_mut().timeline.playhead = 0;
                            let mut r = recents.borrow_mut();
                            r.clear();
                            save_recent_projects(&r);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("📁 Import Media...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                let files = osascript_pick_files(
                                    "\"public.movie\", \"public.audio\", \"com.apple.m4v-video\"",
                                );
                                if files.is_empty() {
                                    None
                                } else {
                                    Some(FileDialogResult::ImportMedia(files))
                                }
                            }));
                            ui.close_menu();
                        }
                        if ui.button("📂 Import Folder...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                osascript_pick_folder().map(FileDialogResult::ImportFolder)
                            }));
                            ui.close_menu();
                        }
                        if ui.button("📂 Open...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                // No type filter — osascript shows all files so user can pick .rook
                                let output = std::process::Command::new("osascript")
                                    .args(["-e", "POSIX path of (choose file)"])
                                    .output()
                                    .ok()?;
                                let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                                if s.is_empty() {
                                    None
                                } else {
                                    Some(FileDialogResult::OpenProject(PathBuf::from(s)))
                                }
                            }));
                            ui.close_menu();
                        }
                        // ── Recent Projects ────────────────────────────
                        {
                            // Clone out before rendering to avoid borrow-mut conflict inside loop.
                            let recent_items: Vec<(String, String)> = recents.borrow().clone();
                            if !recent_items.is_empty() {
                                ui.separator();
                                ui.label(
                                    egui::RichText::new("Recent Projects")
                                        .size(11.0)
                                        .color(egui::Color32::from_gray(160)),
                                );
                                for (name, path_str) in &recent_items {
                                    let p = PathBuf::from(path_str);
                                    let exists = p.exists();
                                    if ui
                                        .button(format!(
                                            "{} {}",
                                            if exists { "📄" } else { "❌" },
                                            name
                                        ))
                                        .on_hover_text(path_str)
                                        .clicked()
                                    {
                                        if exists {
                                            match Engine::open_project(&p) {
                                                Ok(e) => {
                                                    let ps = p.display().to_string();
                                                    let n = e.project().name.clone();
                                                    *eng.lock().unwrap() = e;
                                                    add_recent(&mut recents.borrow_mut(), n, ps);
                                                    tracing::info!(
                                                        ?p,
                                                        "project opened from recent"
                                                    );
                                                }
                                                Err(err) => {
                                                    tracing::error!(?err, "open recent failed")
                                                }
                                            }
                                        }
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button("🗑 Clear Recent").clicked() {
                                    let mut r = recents.borrow_mut();
                                    r.clear();
                                    save_recent_projects(&r);
                                    ui.close_menu();
                                }
                            }
                        }
                        if ui.button("💾 Save").clicked() {
                            let mut e = eng.lock().unwrap();
                            if e.db_path().is_none() {
                                let app_dir = dirs::data_local_dir()
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join("Rook");
                                std::fs::create_dir_all(&app_dir).ok();
                                let db_path = app_dir.join(format!("{}.rook", e.project().name));
                                if let Err(err) = e.init_db(&db_path) {
                                    tracing::error!(?err, "failed to init database");
                                }
                            }
                            match e.save_project(None) {
                                Ok(path) => {
                                    let path_str = path.display().to_string();
                                    let name = e.project().name.clone();
                                    tracing::info!(?path, "project saved");
                                    add_recent(&mut recents.borrow_mut(), name, path_str);
                                }
                                Err(err) => tracing::error!(?err, "save failed"),
                            }
                            ui.close_menu();
                        }
                        // ── Save As ───────────────────────────────────
                        if ui.button("💾 Save As...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                osascript_save_file("Untitled.rook").map(FileDialogResult::SaveAs)
                            }));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("📦 Consolidate Project").clicked() {
                            let mut e = eng.lock().unwrap();
                            match e.consolidate_project() {
                                Ok(count) => {
                                    tracing::info!(count, "project consolidated");
                                }
                                Err(err) => {
                                    tracing::error!(?err, "consolidate failed");
                                }
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("📋 Import EDL...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                osascript_pick_file("\"public.data\"")
                                    .map(FileDialogResult::ImportEdl)
                            }));
                            ui.close_menu();
                        }
                        if ui.button("🎬 Import iMovie Project...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                osascript_pick_file("\"public.xml\"")
                                    .map(FileDialogResult::ImportImovie)
                            }));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("📝 Import SRT...").clicked() {
                            self_file_dialog_rx = Some(spawn_file_dialog(ctx_ref, || {
                                osascript_pick_file("\"public.data\"")
                                    .map(FileDialogResult::ImportSrt)
                            }));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("📤 Export...").clicked() {
                            open_export = true;
                            ui.close_menu();
                        }
                        // Write any spawned dialog receiver back out through the cell.
                        if self_file_dialog_rx.is_some() {
                            *dialog_cell.borrow_mut() = self_file_dialog_rx;
                        }
                    });
                }
                {
                    let eng = engine_arc.clone();
                    ui.menu_button("Edit", move |ui| {
                        // ── Undo with history dropdown ─────────────────
                        let (can_undo, undo_label, undo_labels) = {
                            let e = eng.lock().unwrap();
                            (
                                e.can_undo(),
                                e.undo_label().map(|s| s.to_string()),
                                e.history().undo_labels(),
                            )
                        };
                        if can_undo {
                            let label = undo_label.as_deref().unwrap_or("Undo");
                            ui.menu_button(format!("↩ Undo {}", label), |ui| {
                                for (i, lbl) in undo_labels.iter().enumerate() {
                                    if ui
                                        .button(format!(
                                            "{}↩ {}",
                                            if i == 0 { "┌ " } else { "├ " },
                                            lbl
                                        ))
                                        .clicked()
                                    {
                                        let mut e = eng.lock().unwrap();
                                        for _ in 0..=i {
                                            e.undo();
                                        }
                                        ui.close_menu();
                                    }
                                }
                            });
                        } else {
                            ui.add_enabled(false, egui::Button::new("↩ Undo"));
                        }

                        // ── Redo with history dropdown ─────────────────
                        let (can_redo, redo_label, redo_labels) = {
                            let e = eng.lock().unwrap();
                            (
                                e.can_redo(),
                                e.redo_label().map(|s| s.to_string()),
                                e.history().redo_labels(),
                            )
                        };
                        if can_redo {
                            let label = redo_label.as_deref().unwrap_or("Redo");
                            ui.menu_button(format!("↪ Redo {}", label), |ui| {
                                for (i, lbl) in redo_labels.iter().enumerate() {
                                    if ui
                                        .button(format!(
                                            "{}↪ {}",
                                            if i == 0 { "┌ " } else { "├ " },
                                            lbl
                                        ))
                                        .clicked()
                                    {
                                        let mut e = eng.lock().unwrap();
                                        for _ in 0..=i {
                                            e.redo();
                                        }
                                        ui.close_menu();
                                    }
                                }
                            });
                        } else {
                            ui.add_enabled(false, egui::Button::new("↪ Redo"));
                        }

                        ui.separator();
                        // ── Project Snapshots ─────────────────────────
                        ui.menu_button("📸 Snapshots", |ui| {
                            let (snapshot_names, current_name) = {
                                let e = eng.lock().unwrap();
                                (e.snapshot_names(), e.project().name.clone())
                            };
                            if snapshot_names.is_empty() {
                                ui.label("No snapshots yet");
                            } else {
                                for name in &snapshot_names {
                                    if ui.button(format!("📸 {}", name)).clicked() {
                                        let mut e = eng.lock().unwrap();
                                        e.restore_snapshot(name);
                                        tracing::info!(%name, "snapshot restored");
                                        ui.close_menu();
                                    }
                                }
                            }
                            ui.separator();
                            let mut snap_name = current_name.clone();
                            ui.horizontal(|ui| {
                                ui.label("Name:");
                                ui.text_edit_singleline(&mut snap_name);
                            });
                            if ui.button("💾 Save Snapshot").clicked() {
                                let mut e = eng.lock().unwrap();
                                e.save_snapshot(snap_name.clone());
                                tracing::info!(%snap_name, "snapshot saved");
                                ui.close_menu();
                            }
                        });
                        ui.separator();
                        if ui.button("🎬 Add Video Track").clicked() {
                            let mut e = eng.lock().unwrap();
                            let count = e
                                .project()
                                .timeline
                                .tracks_of_kind(rook_core::track::TrackKind::Video)
                                .len()
                                + 1;
                            e.project_mut().add_video_track(format!("V{}", count));
                            ui.close_menu();
                        }
                        if ui.button("🔊 Add Audio Track").clicked() {
                            let mut e = eng.lock().unwrap();
                            let count = e
                                .project()
                                .timeline
                                .tracks_of_kind(rook_core::track::TrackKind::Audio)
                                .len()
                                + 1;
                            e.project_mut().add_audio_track(format!("A{}", count));
                            ui.close_menu();
                        }
                        ui.separator();
                        // ── Multicam ────────────────────────────────────
                        let selected_count = {
                            let e = eng.lock().unwrap();
                            e.project().timeline.selected_clip_ids.len()
                        };
                        if selected_count >= 2 {
                            if ui.button("🎬 Create Multicam Clip").clicked() {
                                let (clip_ids, tid, pos) = {
                                    let e = eng.lock().unwrap();
                                    let ids = e.project().timeline.selected_clip_ids.clone();
                                    let t = e
                                        .project()
                                        .timeline
                                        .tracks
                                        .iter()
                                        .find(|t| t.kind == rook_core::track::TrackKind::Video)
                                        .map(|t| t.id);
                                    let p = ids
                                        .first()
                                        .and_then(|cid| e.project().timeline.clip(*cid))
                                        .map(|c| c.timeline_in)
                                        .unwrap_or(0);
                                    (ids, t, p)
                                };
                                if let Some(tid) = tid {
                                    let mut e = eng.lock().unwrap();
                                    let cmd = rook_core::commands::EditCommand::CreateMulticam {
                                        clip_ids,
                                        label: "Multicam 1".to_string(),
                                        sync_method:
                                            rook_core::multicam::MulticamSyncMethod::Waveform,
                                        position: pos,
                                        track_id: tid,
                                    };
                                    e.apply(cmd).ok();
                                }
                                ui.close_menu();
                            }
                        } else {
                            ui.add_enabled(
                                false,
                                egui::Button::new("🎬 Create Multicam Clip (select 2+ clips)"),
                            )
                            .on_disabled_hover_text("Select at least 2 clips on the timeline");
                        }
                    });
                }
                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_gallery, "Gallery");
                    ui.checkbox(&mut self.show_inspector, "Inspector");
                    ui.checkbox(&mut self.show_markers, "📍 Markers");
                    ui.checkbox(&mut self.show_multicam, "📷 Multicam Angle Viewer");
                    ui.checkbox(&mut self.show_vu_meter, "🔊 VU Meter");
                    ui.checkbox(&mut self.show_event_library, "📚 Event Library");
                    ui.checkbox(&mut self.show_plugin_browser, "🔌 Plugin Browser");
                    ui.separator();
                    let light_label = if self.light_mode {
                        "🌙 Dark Mode"
                    } else {
                        "☀️ Light Mode"
                    };
                    if ui.button(light_label).clicked() {
                        self.light_mode = !self.light_mode;
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label("Workspace:");
                    if ui.button("📐 Default").clicked() {
                        self.show_gallery = false;
                        self.show_inspector = true;
                        self.show_markers = false;
                        self.show_vu_meter = false;
                        self.show_event_library = true;
                        self.show_plugin_browser = false;
                        ui.close_menu();
                    }
                    if ui.button("🎨 Color & Effects").clicked() {
                        self.show_gallery = false;
                        self.show_inspector = true;
                        self.show_markers = false;
                        self.show_vu_meter = false;
                        self.show_event_library = false;
                        self.show_plugin_browser = false;
                        ui.close_menu();
                    }
                    if ui.button("🔊 Audio").clicked() {
                        self.show_gallery = false;
                        self.show_inspector = false;
                        self.show_markers = false;
                        self.show_vu_meter = true;
                        self.show_event_library = false;
                        self.show_plugin_browser = false;
                        ui.close_menu();
                    }
                    if ui.button("📋 Logging").clicked() {
                        self.show_gallery = false;
                        self.show_inspector = false;
                        self.show_markers = true;
                        self.show_vu_meter = false;
                        self.show_event_library = true;
                        self.show_plugin_browser = false;
                        ui.close_menu();
                    }
                    if ui.button("🔄 Reset Workspace").clicked() {
                        self.show_gallery = false;
                        self.show_inspector = true;
                        self.show_markers = false;
                        self.show_vu_meter = false;
                        self.show_event_library = true;
                        self.show_plugin_browser = false;
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label("Accent Color:");
                    ui.color_edit_button_srgb(&mut self.accent_color);
                    ui.separator();
                    ui.label("Proxy Resolution:");
                    let mut scale = {
                        let eng = engine_arc.lock().unwrap();
                        eng.proxy().proxy_scale()
                    };
                    let prev_scale = scale;
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label((scale - 0.25).abs() < 0.01, "¼")
                            .clicked()
                        {
                            scale = 0.25;
                        }
                        if ui
                            .selectable_label((scale - 0.5).abs() < 0.01, "½")
                            .clicked()
                        {
                            scale = 0.5;
                        }
                        if ui
                            .selectable_label((scale - 1.0).abs() < 0.01, "Full")
                            .clicked()
                        {
                            scale = 1.0;
                        }
                    });
                    if (scale - prev_scale).abs() > 0.01 {
                        let eng = engine_arc.lock().unwrap();
                        eng.proxy().set_proxy_scale(scale);
                    }
                    ui.separator();
                    ui.label("Accessibility:");
                    ui.checkbox(&mut self.high_contrast, "♿ High Contrast");
                    ui.checkbox(&mut self.large_text, "🔤 Larger Text");
                    ui.checkbox(&mut self.reduce_motion, "🔄 Reduce Motion");
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let connected = self
                        .ipc_server
                        .as_ref()
                        .map(|s| s.is_connected())
                        .unwrap_or(false);
                    let (icon, label) = if connected {
                        ("🟢", "Agent")
                    } else {
                        ("⚫", "Agent")
                    };
                    ui.label(format!("{icon} {label}"));
                    ui.separator();
                    if let Some(ref db_path) = db_path_info {
                        ui.label(
                            egui::RichText::new(format!("📁 {}", db_path.display()))
                                .size(10.0)
                                .color(egui::Color32::from_gray(140)),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("💾 Unsaved")
                                .size(10.0)
                                .color(egui::Color32::from_rgb(200, 160, 40)),
                        );
                    }
                    ui.separator();
                    ui.label(format!(
                        "↩{} ↪{}",
                        if can_undo { "✓" } else { "—" },
                        if can_redo { "✓" } else { "—" },
                    ));
                });
            });
        });

        if open_export {
            self.show_export_dialog = true;
        }

        // Sync recent projects back from RefCell
        self.recent_projects = recents_rc.take();
        // Pick up any dialog receiver spawned from the File menu this frame.
        if self.file_dialog_rx.is_none() {
            if let Some(rx) = dialog_rx_cell.borrow_mut().take() {
                self.file_dialog_rx = Some(rx);
            }
        }

        let mut engine = self.engine.lock().unwrap();

        // Pick up playhead changes made by menu actions (e.g. New Project resets to 0).
        // Only sync when not playing so we don't fight the playback clock.
        if !self.playing {
            self.playhead = engine.project().timeline.playhead;
        }

        // ── Bottom: Timeline — full width, declared before sidepanels ────
        egui::TopBottomPanel::bottom("timeline")
            .resizable(true)
            .default_height(240.0)
            .height_range(120.0..=400.0)
            .show_separator_line(true)
            .show(ctx, |ui| {
                ui.style_mut().visuals.widgets.noninteractive.bg_fill =
                    egui::Color32::from_rgb(28, 30, 38);
                ui.painter().rect_filled(
                    ui.max_rect(),
                    0.0,
                    egui::Color32::from_rgb(28, 30, 38),
                );
                self.timeline
                    .show(ui, engine.project_mut(), &mut self.playhead);
            });

        // ── Bottom: Marker list (above timeline) ──────────────────────
        if self.show_markers {
            egui::TopBottomPanel::bottom("markers")
                .default_height(100.0)
                .resizable(true)
                .show(ctx, |ui| {
                    self.markers
                        .show(ui, engine.project_mut(), &mut self.playhead);
                });
        }

        // ── Right: Inspector ────────────────────────────────────────────
        if self.show_inspector {
            egui::SidePanel::right("inspector")
                .default_width(260.0)
                .resizable(true)
                .show(ctx, |ui| {
                    self.inspector
                        .show(ui, engine.project_mut(), &self.playhead);
                });
        }

        // ── Right: VU Meter ──────────────────────────────────────────────
        if self.show_vu_meter {
            egui::SidePanel::right("vu_meter")
                .default_width(160.0)
                .resizable(true)
                .show(ctx, |ui| {
                    self.vu_meter.show(ui, engine.project(), self.playhead);
                });
        }

        // ── Right: Multicam Angle Viewer (shown as right panel) ──────────
        if self.show_multicam {
            let selected = engine.project().timeline.selected_clip_ids.clone();
            egui::SidePanel::right("multicam_panel")
                .default_width(260.0)
                .resizable(true)
                .show(ctx, |ui| {
                    if let Some(switched_idx) = self.multicam.show(ui, &mut engine, &selected) {
                        if let Some(cid) = selected.first() {
                            let cmd = rook_core::commands::EditCommand::SwitchMulticamAngle {
                                clip_id: *cid,
                                angle_index: switched_idx,
                            };
                            engine.apply(cmd).ok();
                        }
                    }
                });
        }

        // ── Right: Library Panel (tabbed: Gallery | Event Library | Plugin Browser) ──
        let show_library = self.show_event_library
            || self.show_gallery
            || self.show_plugin_browser;
        if show_library {
            egui::SidePanel::right("library_panel")
                .default_width(280.0)
                .min_width(160.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.show_event_library, "📚 Events")
                            .clicked()
                        {
                            self.show_event_library = !self.show_event_library;
                        }
                        if ui
                            .selectable_label(self.show_gallery, "🖼 Gallery")
                            .clicked()
                        {
                            self.show_gallery = !self.show_gallery;
                        }
                        if ui
                            .selectable_label(self.show_plugin_browser, "🔌 Plugins")
                            .clicked()
                        {
                            self.show_plugin_browser = !self.show_plugin_browser;
                        }
                    });
                    ui.separator();

                    if self.show_event_library {
                        ui.heading("📚 Event Library");
                        ui.separator();
                        let events = [
                            (
                                "🎬 Video Clips",
                                engine
                                    .project()
                                    .timeline
                                    .tracks
                                    .iter()
                                    .filter(|t| t.kind == rook_core::track::TrackKind::Video)
                                    .flat_map(|t| t.clips.iter())
                                    .count(),
                            ),
                            (
                                "🔊 Audio Clips",
                                engine
                                    .project()
                                    .timeline
                                    .tracks
                                    .iter()
                                    .filter(|t| t.kind == rook_core::track::TrackKind::Audio)
                                    .flat_map(|t| t.clips.iter())
                                    .count(),
                            ),
                            (
                                "📝 Text/Subtitles",
                                engine
                                    .project()
                                    .timeline
                                    .tracks
                                    .iter()
                                    .filter(|t| t.kind == rook_core::track::TrackKind::Text)
                                    .flat_map(|t| t.clips.iter())
                                    .count(),
                            ),
                        ];
                        for (label, count) in &events {
                            ui.label(format!("{}: {}", label, count));
                        }
                        ui.separator();
                        ui.label("📍 Markers:");
                        for m in &engine.project().timeline.markers {
                            let fps = engine.project().frame_rate.as_f64();
                            let secs = m.frame as f64 / fps;
                            ui.label(format!(
                                "  {} — {}:{:05.2}",
                                m.label,
                                (secs / 60.0) as i64,
                                secs % 60.0
                            ));
                        }
                        ui.separator();
                        ui.label("📦 Compound Clips:");
                        let compound_count = engine.project().timeline.compound_contents.len();
                        ui.label(format!("  {} compound clips", compound_count));
                        ui.separator();
                        ui.label(format!("🗂 {} assets total", engine.project().assets.len()));
                        let on_timeline: usize = engine
                            .project()
                            .timeline
                            .tracks
                            .iter()
                            .flat_map(|t| t.clips.iter())
                            .map(|c| c.asset_id)
                            .collect::<std::collections::HashSet<_>>()
                            .len();
                        ui.label(format!("  {} used on timeline", on_timeline));
                    }

                    if self.show_gallery {
                        self.gallery.show(
                            ui,
                            &mut engine,
                            &self.timeline.thumbnail_cache,
                        );
                    }

                    if self.show_plugin_browser {
                        self.plugin_browser.show(ui, &mut engine);
                    }
                });
        }

        // ── Center: Preview monitor ─────────────────────────────────────
        // Set preview quality based on playback state (outside egui render pass)
        self.preview.set_playing(self.playing);
        let active_tool = self.timeline.active_tool;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.colored_label(
                egui::Color32::from_rgb(255, 200, 100),
                "▬▬ PREVIEW — click File → Import Media to load video ▬▬",
            );
            ui.separator();
            self.preview.show(
                ui,
                &mut engine,
                active_tool,
                &mut self.playing,
                &mut self.playhead,
            );
        });

        // ── Export dialog ───────────────────────────────────────────────
        if self.show_export_dialog {
            let mut export_codec: String = "h264".into();
            let mut export_container: String = "mp4".into();
            let mut export_range: u8 = 0; // 0=full, 1=IO marks
            let mut export_audio_only = false;
            let mut export_still_frame = false;
            let mut export_alpha = false;
            let mut exporting = false;
            let mut export_progress: f32 = 0.0;
            let mut export_eta: f64 = 0.0;
            // Custom resolution / framerate
            let mut export_custom_res = false;
            let mut export_width: u32 = 1920;
            let mut export_height: u32 = 1080;
            let mut export_fps: f64 = 24.0;
            // Bitrate / quality
            let mut export_bitrate_mbps: f32 = 15.0;
            // Audio settings
            let mut export_sample_rate: u32 = 48000;
            let mut export_audio_channels: u8 = 2;

            // Snapshot range info
            let (total_frames, range_start, range_end, canvas_w, canvas_h, fps_val) = {
                let proj = engine.project();
                let total = proj.timeline.duration().max(1);
                let (rs, re) = if export_range == 1
                    && proj.timeline.in_point.is_some()
                    && proj.timeline.out_point.is_some()
                {
                    let s = proj.timeline.in_point.unwrap();
                    let e = proj.timeline.out_point.unwrap();
                    (s, if e > s { e } else { total })
                } else {
                    (0i64, total)
                };
                // Initialize custom res from project
                if !export_custom_res {
                    export_width = proj.canvas.width;
                    export_height = proj.canvas.height;
                    export_fps = proj.frame_rate.as_f64();
                }
                (
                    total,
                    rs,
                    re,
                    proj.canvas.width,
                    proj.canvas.height,
                    proj.frame_rate.as_f64(),
                )
            };

            let range_dur = range_end - range_start;
            let range_secs = range_dur as f64 / fps_val;

            egui::Window::new("Export")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("📤 Export");
                    ui.separator();

                    // ── Format ──────────────────────────────────────────
                    ui.collapsing("Format", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Codec:");
                            egui::ComboBox::from_id_salt("export_codec")
                                .selected_text(match export_codec.as_str() {
                                    "h264" => "H.264",
                                    "h265" => "H.265 / HEVC",
                                    "prores" => "ProRes 422",
                                    "mp3" => "MP3",
                                    "wav" => "WAV",
                                    _ => &export_codec,
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut export_codec, "h264".into(), "H.264");
                                    ui.selectable_value(
                                        &mut export_codec,
                                        "h265".into(),
                                        "H.265 / HEVC",
                                    );
                                    ui.selectable_value(
                                        &mut export_codec,
                                        "prores".into(),
                                        "ProRes 422",
                                    );
                                    ui.separator();
                                    ui.selectable_value(
                                        &mut export_codec,
                                        "mp3".into(),
                                        "MP3 (audio)",
                                    );
                                    ui.selectable_value(
                                        &mut export_codec,
                                        "wav".into(),
                                        "WAV (audio)",
                                    );
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Container:");
                            ui.selectable_value(&mut export_container, "mp4".into(), "MP4");
                            ui.selectable_value(&mut export_container, "mov".into(), "MOV");
                            ui.selectable_value(&mut export_container, "mkv".into(), "MKV");
                        });
                    });

                    // ── Range ───────────────────────────────────────────
                    ui.collapsing("Range", |ui| {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut export_range, 0, "Full Timeline");
                            ui.radio_value(&mut export_range, 1, "I/O Marks");
                        });

                        if export_range == 1 {
                            let has_io = engine.project().timeline.in_point.is_some()
                                && engine.project().timeline.out_point.is_some();
                            if !has_io {
                                ui.label(
                                    egui::RichText::new(
                                        "No I/O marks set — use I/O keys on timeline",
                                    )
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(200, 160, 40)),
                                );
                            } else {
                                ui.label(format!(
                                    "In: frame {}  Out: frame {}",
                                    engine.project().timeline.in_point.unwrap_or(0),
                                    engine.project().timeline.out_point.unwrap_or(0)
                                ));
                            }
                        }

                        let dur = engine.project().frame_rate.as_f64();
                        let total_sec = total_frames as f64 / dur;
                        ui.label(format!(
                            "Total: {}f ({:.1}s) → Range: {}f ({:.1}s)",
                            total_frames, total_sec, range_dur, range_secs
                        ));
                    });

                    // ── Resolution & Frame Rate ────────────────────────
                    ui.collapsing("Resolution & Frame Rate", |ui| {
                        ui.checkbox(&mut export_custom_res, "Custom settings");
                        if export_custom_res {
                            ui.horizontal(|ui| {
                                ui.label("Resolution:");
                                ui.add(
                                    egui::DragValue::new(&mut export_width)
                                        .clamp_range(64..=8192)
                                        .suffix(" px"),
                                );
                                ui.label("×");
                                ui.add(
                                    egui::DragValue::new(&mut export_height)
                                        .clamp_range(64..=8192)
                                        .suffix(" px"),
                                );
                            });
                            // Preset buttons
                            ui.horizontal(|ui| {
                                for (label, w, h) in [
                                    ("4K", 3840, 2160),
                                    ("1080p", 1920, 1080),
                                    ("720p", 1280, 720),
                                    ("480p", 854, 480),
                                ] {
                                    if ui.small_button(label).clicked() {
                                        export_width = w;
                                        export_height = h;
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Frame Rate:");
                                egui::ComboBox::from_id_salt("export_fps")
                                    .selected_text(format!("{:.0} fps", export_fps))
                                    .show_ui(ui, |ui| {
                                        for &val in
                                            &[23.976, 24.0, 25.0, 29.97, 30.0, 50.0, 59.94, 60.0]
                                        {
                                            ui.selectable_value(
                                                &mut export_fps,
                                                val,
                                                format!("{:.2} fps", val),
                                            );
                                        }
                                    });
                            });
                        } else {
                            ui.label(format!(
                                "Project: {}×{} @ {:.2} fps",
                                canvas_w, canvas_h, fps_val
                            ));
                        }
                    });

                    // ── Quality / Bitrate ──────────────────────────────
                    ui.collapsing("Quality", |ui| {
                        if export_codec == "h264" || export_codec == "h265" {
                            ui.horizontal(|ui| {
                                ui.label("Bitrate:");
                                ui.add(
                                    egui::Slider::new(&mut export_bitrate_mbps, 0.5..=200.0)
                                        .logarithmic(true)
                                        .text("Mbps"),
                                );
                            });
                            ui.label(
                                egui::RichText::new(format!(
                                    "~{} MB for this export",
                                    (export_bitrate_mbps * range_secs as f32 / 8.0) as u32
                                ))
                                .size(10.0)
                                .color(egui::Color32::from_gray(140)),
                            );
                        } else {
                            ui.label("Bitrate not configurable for this codec");
                        }
                    });

                    // ── Audio Settings ─────────────────────────────────
                    ui.collapsing("Audio Settings", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Sample Rate:");
                            egui::ComboBox::from_id_salt("export_sr")
                                .selected_text(format!("{} Hz", export_sample_rate))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut export_sample_rate, 44100, "44100 Hz");
                                    ui.selectable_value(&mut export_sample_rate, 48000, "48000 Hz");
                                    ui.selectable_value(&mut export_sample_rate, 96000, "96000 Hz");
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Channels:");
                            ui.selectable_value(&mut export_audio_channels, 1, "Mono");
                            ui.selectable_value(&mut export_audio_channels, 2, "Stereo");
                        });
                    });

                    // ── Extras ──────────────────────────────────────────
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut export_audio_only, "Audio Only");
                        ui.checkbox(&mut export_still_frame, "Still Frame");
                    });
                    ui.checkbox(&mut export_alpha, "🔲 Export Alpha (ProRes 4444)")
                        .on_hover_text("Include alpha channel — forces ProRes 4444 codec");

                    if export_still_frame {
                        ui.horizontal(|ui| {
                            ui.label("Export frame at playhead:");
                            ui.label(format!("{}", self.playhead));
                        });
                    }

                    if exporting {
                        ui.add_space(8.0);
                        ui.label(format!("Exporting… {:.0}%", export_progress));
                        ui.add(
                            egui::ProgressBar::new(export_progress / 100.0)
                                .desired_width(ui.available_width())
                                .animate(true),
                        );
                        if export_eta > 0.0 {
                            ui.label(format!("ETA: {:.0}s", export_eta));
                        }
                        ctx.request_repaint();
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.show_export_dialog = false;
                        }
                        if ui.button("📥 Add to Batch").clicked() {
                            let ext = if export_alpha {
                                "mov"
                            } else {
                                export_container.as_str()
                            };
                            let path = if export_still_frame {
                                std::path::PathBuf::from("frame.png")
                            } else {
                                std::path::PathBuf::from(format!("export.{}", ext))
                            };
                            let preset = if export_audio_only {
                                "mp3".to_string()
                            } else if export_alpha {
                                "prores_4444".to_string()
                            } else {
                                export_codec.clone()
                            };
                            self.batch_queue.push((path, preset));
                            self.show_export_dialog = false;
                        }
                        if ui
                            .button(if exporting {
                                "⏳ Exporting…"
                            } else {
                                "📤 Export"
                            })
                            .clicked()
                            && !exporting
                        {
                            exporting = true;
                            let ext = if export_alpha {
                                "mov"
                            } else {
                                export_container.as_str()
                            };
                            let path = if export_still_frame {
                                std::path::PathBuf::from("frame.png")
                            } else {
                                std::path::PathBuf::from(format!("export.{}", ext))
                            };
                            let preset = if export_audio_only {
                                "mp3".to_string()
                            } else if export_alpha {
                                "prores_4444".to_string()
                            } else {
                                export_codec.clone()
                            };
                            match engine.export_with_progress(&path, &preset, |pct, eta| {
                                let _ = (pct, eta);
                            }) {
                                Ok(()) => tracing::info!("export complete: {:?}", path),
                                Err(e) => tracing::error!(?e, "export failed"),
                            }
                            self.show_export_dialog = false;
                        }
                    });
                });

            // ── Batch Queue display ──────────────────────────────────────
            if !self.batch_queue.is_empty() {
                egui::Window::new("Export Batch")
                    .collapsible(true)
                    .show(ctx, |ui| {
                        ui.heading("📥 Batch Export Queue");
                        ui.label(format!("{} jobs queued", self.batch_queue.len()));
                        ui.separator();
                        let mut to_remove = Vec::new();
                        for (i, (path, preset)) in self.batch_queue.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}. {}", i + 1, path.display()));
                                ui.label(format!("[{}]", preset));
                                if ui.button("✕").clicked() {
                                    to_remove.push(i);
                                }
                            });
                        }
                        for i in to_remove.iter().rev() {
                            self.batch_queue.remove(*i);
                        }
                        ui.separator();
                        if !self.batch_queue.is_empty() {
                            let queue: Vec<_> = self.batch_queue.clone();
                            if ui.button("▶ Export All").clicked() {
                                self.batch_exporting = true;
                                let mut e = self.engine.lock().unwrap();
                                for (path, preset) in &queue {
                                    match e.export_with_progress(path, preset, |pct, eta| {
                                        let _ = (pct, eta);
                                    }) {
                                        Ok(()) => tracing::info!("batch export ok: {:?}", path),
                                        Err(err) => {
                                            tracing::error!(?err, "batch export failed: {:?}", path)
                                        }
                                    }
                                }
                                self.batch_queue.clear();
                                self.batch_exporting = false;
                            }
                        }
                    });
            }
        }

        // Release engine lock before potential repaint
        drop(engine);

        // ── Crash Recovery Dialog ────────────────────────────────────
        if self.show_recovery_dialog {
            let recovery_path = self.recovery_path.clone();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            egui::Window::new("🔄 Crash Recovery")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("Recover unsaved work?");
                    ui.label("Rook detected an auto-save from a previous session.");
                    ui.label("You can recover it or discard it.");
                    ui.separator();
                    if let Some(ref path) = recovery_path {
                        ui.label(format!("Recovery file: {}", path.display()));
                        // Show metadata if available
                        if let Some(meta_path) = autosave_dir().map(|d| d.join("recovery.meta")) {
                            if meta_path.exists() {
                                if let Ok(meta) = std::fs::read_to_string(&meta_path) {
                                    let lines: Vec<&str> = meta.lines().collect();
                                    if let Some(name) = lines.first() {
                                        ui.label(format!("Project: {}", name));
                                    }
                                    if let Some(ts) =
                                        lines.get(1).and_then(|s| s.parse::<i64>().ok())
                                    {
                                        let age = now - ts;
                                        if age < 120 {
                                            ui.label(format!("Saved: {} seconds ago", age));
                                        } else {
                                            ui.label(format!("Saved: {} minutes ago", age / 60));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("🗑 Discard").clicked() {
                            // Clean up autosave files
                            if let Some(adir) = autosave_dir() {
                                let _ = std::fs::remove_file(adir.join("recovery.rook"));
                                let _ = std::fs::remove_file(adir.join("recovery.meta"));
                            }
                            self.show_recovery_dialog = false;
                            self.recovery_path = None;
                        }
                        if ui.button("✅ Recover").clicked() {
                            if let Some(ref path) = recovery_path {
                                match Engine::open_project(path) {
                                    Ok(recovered) => {
                                        let path_str = path.display().to_string();
                                        let name = recovered.project().name.clone();
                                        *self.engine.lock().unwrap() = recovered;
                                        add_recent(&mut self.recent_projects, name, path_str);
                                        // Clean up autosave so it doesn't trigger again
                                        if let Some(adir) = autosave_dir() {
                                            let _ =
                                                std::fs::remove_file(adir.join("recovery.rook"));
                                            let _ =
                                                std::fs::remove_file(adir.join("recovery.meta"));
                                        }
                                        tracing::info!("crash recovery: project restored");
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            ?e,
                                            "crash recovery: failed to open autosave"
                                        );
                                    }
                                }
                            }
                            self.show_recovery_dialog = false;
                            self.recovery_path = None;
                        }
                    });
                });
        }

        // Continuous repaint during playback
        if self.playing {
            ctx.request_repaint();
        }
    }
}

/// Import a batch of files into the engine, auto-creating tracks and inserting clips.
fn import_files(engine: &mut Engine, files: &[std::path::PathBuf]) {
    let t0 = std::time::Instant::now();
    for path in files {
        let t1 = std::time::Instant::now();
        if let Ok(asset_id) = engine.import_media(path) {
            eprintln!("[import_files] import_media took {:?} for {}", t1.elapsed(), path.display());

            // ── Gather asset metadata before any mutation (avoids borrow conflicts) ──
            let is_video = engine
                .project()
                .asset(asset_id)
                .map(|a| {
                    matches!(a, rook_core::asset::Asset::Video(v) if v.metadata.video.is_some())
                })
                .unwrap_or(false);
            let has_audio = engine
                .project()
                .asset(asset_id)
                .map(|a| match a {
                    rook_core::asset::Asset::Video(v) => {
                        // Check the has_audio flag on the video metadata, OR check
                        // if this is an audio-only file (meta.audio populated but
                        // no video stream found).
                        v.metadata.video.as_ref().map(|vm| vm.has_audio).unwrap_or(false)
                            || v.metadata.audio.is_some()
                    }
                    rook_core::asset::Asset::Audio(_) => true,
                    _ => false,
                })
                .unwrap_or(false);
            let dur = engine
                .project()
                .asset(asset_id)
                .and_then(|a| a.metadata().duration_frames)
                .unwrap_or(300);
            let position = engine.project().timeline.duration();

            // Generate link group if we're creating both video + audio clips
            let link_group: Option<u64> = if is_video && has_audio {
                Some(next_link_group_id())
            } else {
                None
            };

            eprintln!(
                "[import_files] {} is_video={is_video} has_audio={has_audio} dur={dur} pos={position} link_group={link_group:?}",
                path.display()
            );

            // ── Video track + clip ──────────────────────────────────────
            if is_video {
                if engine
                    .project()
                    .timeline
                    .tracks_of_kind(rook_core::track::TrackKind::Video)
                    .is_empty()
                {
                    engine.project_mut().add_video_track("V1".to_string());
                }
                if let Some(track) = engine
                    .project()
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == rook_core::track::TrackKind::Video)
                {
                    let tid = track.id;
                    let cmd = rook_core::commands::EditCommand::InsertClip {
                        asset_id,
                        track_id: tid,
                        position,
                        source_in: 0,
                        source_out: dur,
                        link_group_id: link_group,
                    };
                    match engine.apply(cmd) {
                        Ok(()) => eprintln!("[import_files] inserted video clip on track {:?}", tid),
                        Err(e) => eprintln!("[import_files] FAILED to insert video clip: {e}"),
                    }
                }
            }

            // ── Audio track + clip ──────────────────────────────────────
            if has_audio {
                if engine
                    .project()
                    .timeline
                    .tracks_of_kind(rook_core::track::TrackKind::Audio)
                    .is_empty()
                {
                    let atid = engine.project_mut().add_audio_track("A1".to_string());
                    eprintln!("[import_files] created audio track {:?}", atid);
                }
                if let Some(track) = engine
                    .project()
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == rook_core::track::TrackKind::Audio)
                {
                    let atid = track.id;
                    let cmd = rook_core::commands::EditCommand::InsertClip {
                        asset_id,
                        track_id: atid,
                        position,
                        source_in: 0,
                        source_out: dur,
                        link_group_id: link_group,
                    };
                    match engine.apply(cmd) {
                        Ok(()) => eprintln!("[import_files] inserted audio clip on track {:?}", atid),
                        Err(e) => eprintln!("[import_files] FAILED to insert audio clip: {e}"),
                    }
                }
            }
        }
    }
    eprintln!("[import_files] batch of {} files done in {:?}", files.len(), t0.elapsed());
}

/// Atomic counter for link group IDs.
fn next_link_group_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Import a CMX 3600 EDL file into the project.
/// Parses edit decision lines and creates clips on video/audio tracks.
fn import_edl_file(engine: &mut Engine, path: &std::path::Path) -> Result<usize, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read: {}", e))?;
    let fps = engine.project().frame_rate.as_f64();
    let mut imported = 0usize;

    // Ensure we have tracks
    if engine
        .project()
        .timeline
        .tracks_of_kind(rook_core::track::TrackKind::Video)
        .is_empty()
    {
        engine.project_mut().add_video_track("V1".to_string());
    }
    let v_track_id = engine
        .project()
        .timeline
        .tracks
        .iter()
        .find(|t| t.kind == rook_core::track::TrackKind::Video)
        .map(|t| t.id)
        .ok_or("no video track")?;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("TITLE:") || line.starts_with("FCM:") {
            continue;
        }
        // CMX 3600 format: EDIT# REEL TRACK TRANSITION SRC_IN SRC_OUT REC_IN REC_OUT
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }

        let reel = parts[1];
        let track_type = parts[2]; // V, A, A2, etc.
        let src_in_tc = parts[4];
        let src_out_tc = parts[5];
        let rec_in_tc = parts[6];
        let rec_out_tc = parts[7];

        // Parse timecodes (HH:MM:SS:FF)
        let parse_tc = |tc: &str| -> Result<i64, String> {
            let p: Vec<&str> = tc.split(':').collect();
            if p.len() != 4 {
                return Err(format!("bad TC: {}", tc));
            }
            let h: i64 = p[0].parse().map_err(|_| "bad hours")?;
            let m: i64 = p[1].parse().map_err(|_| "bad mins")?;
            let s: i64 = p[2].parse().map_err(|_| "bad secs")?;
            let f: i64 = p[3].parse().map_err(|_| "bad frames")?;
            Ok(((h * 3600 + m * 60 + s) as f64 * fps).round() as i64 + f)
        };

        let rec_in = parse_tc(rec_in_tc)?;
        let rec_out = parse_tc(rec_out_tc)?;
        let src_in = parse_tc(src_in_tc)?;
        let src_out = parse_tc(src_out_tc)?;
        let dur = (rec_out - rec_in).max(1);

        // Try to find the reel file next to the EDL
        let edl_dir = path.parent().unwrap_or(std::path::Path::new("."));
        let reel_path = find_reel_file(edl_dir, reel);

        let asset_id = if reel_path.exists() {
            engine.import_media(&reel_path).ok()
        } else {
            None
        };

        if let Some(aid) = asset_id {
            let tid = if track_type.starts_with('V') {
                v_track_id
            } else {
                // Ensure audio track exists
                if engine
                    .project()
                    .timeline
                    .tracks_of_kind(rook_core::track::TrackKind::Audio)
                    .is_empty()
                {
                    engine.project_mut().add_audio_track("A1".to_string());
                }
                engine
                    .project()
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == rook_core::track::TrackKind::Audio)
                    .map(|t| t.id)
                    .unwrap_or(v_track_id)
            };

            let cmd = rook_core::commands::EditCommand::InsertClip {
                asset_id: aid,
                track_id: tid,
                position: rec_in,
                source_in: src_in,
                source_out: src_out,
                link_group_id: None,
            };
            engine.apply(cmd).ok();
            imported += 1;
        }
    }

    Ok(imported)
}

/// Search for a reel file near the EDL directory.
fn find_reel_file(dir: &std::path::Path, reel: &str) -> std::path::PathBuf {
    let exts = ["mov", "mp4", "mxf", "m4v", "mkv", "avi"];
    // Try exact match first
    for ext in &exts {
        let p = dir.join(format!("{}.{}", reel, ext));
        if p.exists() {
            return p;
        }
    }
    // Try case-insensitive
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let reel_lower = reel.to_lowercase();
            if name.starts_with(&reel_lower) {
                for ext in &exts {
                    if name.ends_with(ext) {
                        return entry.path();
                    }
                }
            }
        }
    }
    // Default guess
    dir.join(format!("{}.mov", reel))
}

/// Import an iMovie project (FCPXML format).
/// iMovie exports use a subset of FCPXML with some project metadata differences.
fn import_imovie_file(engine: &mut Engine, path: &std::path::Path) -> Result<usize, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read: {}", e))?;

    // Detect iMovie format
    let is_imovie = content.contains("iMovie")
        || content.contains("imovie")
        || path
            .file_stem()
            .map(|s| s.to_string_lossy().contains("iMovie"))
            .unwrap_or(false);

    if !content.contains("fcpxml") && !content.contains("<project ") && !is_imovie {
        return Err("Not a valid FCPXML or iMovie project file".to_string());
    }

    // Parse basic FCPXML structure for clips
    let fps = engine.project().frame_rate.as_f64();
    let mut imported = 0usize;
    let project_dir = path.parent().unwrap_or(std::path::Path::new("."));

    // Simple XML parsing: extract <asset> and <clip> elements
    // Look for asset references with file paths
    let mut asset_paths: Vec<(String, String)> = Vec::new(); // (id, path)
    let mut clips: Vec<(String, i64, i64, i64)> = Vec::new(); // (asset_ref, start_frames, duration, offset)

    // Parse asset elements — clone strings to avoid borrow issues
    for line in content.lines() {
        let line_s = line.trim().to_string();
        // <asset id="r1" src="file:///path/to/media.mov" ...>
        if line_s.contains("<asset ") && line_s.contains("src=") {
            let id = extract_attr(&line_s, "id").map(|s| s.to_string());
            let src =
                extract_attr(&line_s, "src").map(|s| s.trim_start_matches("file://").to_string());
            if let (Some(id), Some(src)) = (id, src) {
                asset_paths.push((id, src));
            }
        }
        // <clip name="..." offset="..." duration="..." start="..." ...>
        if line_s.contains("<clip ") {
            let asset_ref = extract_attr(&line_s, "ref")
                .or_else(|| extract_attr(&line_s, "assetRef"))
                .map(|s| s.to_string());
            if let Some(asset_ref) = asset_ref {
                let offset = extract_attr(&line_s, "offset")
                    .and_then(|s| parse_fcpxml_time(s, fps))
                    .unwrap_or(0i64);
                let duration = extract_attr(&line_s, "duration")
                    .and_then(|s| parse_fcpxml_time(s, fps))
                    .unwrap_or(300i64);
                let start = extract_attr(&line_s, "start")
                    .and_then(|s| parse_fcpxml_time(s, fps))
                    .unwrap_or(0i64);
                clips.push((asset_ref, offset, duration, start));
            }
        }
        // <asset-clip ref="r1" offset="..." ...> (FCPXML 1.6+)
        if line_s.contains("<asset-clip ") {
            let asset_ref = extract_attr(&line_s, "ref").map(|s| s.to_string());
            if let Some(asset_ref) = asset_ref {
                let offset = extract_attr(&line_s, "offset")
                    .and_then(|s| parse_fcpxml_time(s, fps))
                    .unwrap_or(0i64);
                let duration = extract_attr(&line_s, "duration")
                    .and_then(|s| parse_fcpxml_time(s, fps))
                    .unwrap_or(300i64);
                clips.push((asset_ref, offset, duration, 0i64));
            }
        }
    }

    if asset_paths.is_empty() && clips.is_empty() {
        return Err("No clips found in the project file".to_string());
    }

    // Ensure tracks exist
    if engine
        .project()
        .timeline
        .tracks_of_kind(rook_core::track::TrackKind::Video)
        .is_empty()
    {
        engine.project_mut().add_video_track("V1".to_string());
    }
    let v_track_id = engine
        .project()
        .timeline
        .tracks
        .iter()
        .find(|t| t.kind == rook_core::track::TrackKind::Video)
        .map(|t| t.id)
        .ok_or("no video track")?;

    // Import each clip
    for (asset_ref, offset, duration, start) in &clips {
        // Find the asset path
        let media_path = asset_paths
            .iter()
            .find(|(id, _)| id == asset_ref)
            .map(|(_, p)| {
                let p = std::path::PathBuf::from(p);
                if p.is_absolute() && p.exists() {
                    p
                } else {
                    // Try relative to project
                    let fname = p.file_name().unwrap_or_default();
                    project_dir.join(fname)
                }
            });

        if let Some(ref media_path) = media_path {
            if media_path.exists() {
                if let Ok(asset_id) = engine.import_media(media_path) {
                    let cmd = rook_core::commands::EditCommand::InsertClip {
                        asset_id,
                        track_id: v_track_id,
                        position: *offset,
                        source_in: *start,
                        source_out: start + duration,
                        link_group_id: None,
                    };
                    engine.apply(cmd).ok();
                    imported += 1;
                }
            }
        }
    }

    Ok(imported)
}

/// Extract an XML attribute value from a string like `attr="value"`.
fn extract_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let prefix = format!("{}=", attr);
    let start = line.find(&prefix)? + prefix.len();
    let quote = line.as_bytes().get(start)?;
    let end = if *quote == b'"' || *quote == b'\'' {
        line[start + 1..].find(*quote as char)? + start + 1
    } else {
        line[start..].find(' ').unwrap_or(line.len() - start) + start
    };
    Some(&line[start + 1..end])
}

/// Parse a time value from FCPXML (e.g., "120/24s" or "5s" or "3600/1s").
fn parse_fcpxml_time(s: &str, fps: f64) -> Option<i64> {
    if let Some(rest) = s.strip_suffix('s') {
        if let Some(slash_pos) = rest.find('/') {
            let num: f64 = rest[..slash_pos].parse().ok()?;
            let den: f64 = rest[slash_pos + 1..].parse().ok()?;
            if den > 0.0 {
                Some((num / den * fps).round() as i64)
            } else {
                None
            }
        } else {
            let secs: f64 = rest.parse().ok()?;
            Some((secs * fps).round() as i64)
        }
    } else {
        None
    }
}
