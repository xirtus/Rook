//! Timeline panel — multi-track editing surface with custom-painted canvas.
//!
//! Features:
//! - Zoom-dependent pixel-per-frame rendering
//! - Drag clips between tracks
//! - Trim handles (left/right edge drag)
//! - Playhead with drag-to-scrub
//! - Snap to playhead, clip edges, markers
//! - Keyboard: Space=play, Del=delete, S=split, J/K/L=shuttle
//! - Track headers with mute/solo/lock controls
//! - Context menu on clips (split, delete, ripple delete)

use std::collections::{HashMap, HashSet};

use rook_core::clip::{Clip, ClipId};
use rook_core::ids::{AssetId, MarkerId, TrackId};
use rook_core::marker::Marker;
use rook_core::project::Project;
use rook_core::track::{Track, TrackKind};

use crate::widgets::thumbnail::{ThumbnailCache, thumbs_for_clip};
use crate::widgets::waveform::{WaveformCache, peaks_for_clip};

const TRACK_HEADER_W: f32 = 64.0;
const RULER_H: f32 = 22.0;
const TRACK_H_MINI: f32 = 22.0;
const TRACK_H_SMALL: f32 = 32.0;
const TRACK_H_MEDIUM: f32 = 44.0;
const TRACK_H_LARGE: f32 = 64.0;
const AUDIO_SEPARATOR_H: f32 = 28.0;
const MIN_PX_PER_FRAME: f32 = 0.25;
const MAX_PX_PER_FRAME: f32 = 30.0;
const TRIM_HANDLE_W: f32 = 6.0;
const SNAP_THRESHOLD: f32 = 8.0; // pixels
const PLAYHEAD_COLOR: egui::Color32 = egui::Color32::RED;
const MARKER_COLOR: egui::Color32 = egui::Color32::from_rgb(0, 200, 140);
const RULER_BG: egui::Color32 = egui::Color32::from_gray(30);

fn last_valid_timeline_frame(duration: i64) -> i64 {
    duration.saturating_sub(1).max(0)
}

/// FCP-style tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Arrow — select and move clips. (A)
    Select,
    /// Blade — split clip at click point. (B)
    Blade,
    /// Trim — drag clip edges. (T)
    Trim,
    /// Range Select — drag to select a time range. (R)
    RangeSelect,
    /// Zoom — click to zoom in, Option+click to zoom out. (Z)
    Zoom,
    /// Hand — pan timeline by dragging. (H)
    Hand,
    /// Position — move clip within frame (transform mode). (P)
    Position,
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Tool::Select => "⇱ Select (A)",
            Tool::Blade => "✂ Blade (B)",
            Tool::Trim => "⏩ Trim (T)",
            Tool::RangeSelect => "⏺ Range (R)",
            Tool::Zoom => "🔍 Zoom (Z)",
            Tool::Hand => "✋ Hand (H)",
            Tool::Position => "↕ Position (P)",
        };
        write!(f, "{s}")
    }
}

/// Snapshot of a clip for hit-testing and paint.
#[derive(Clone)]
struct ClipGeom {
    clip_id: ClipId,
    track_id: TrackId,
    asset_id: AssetId,
    /// Left edge in scroll-space pixels.
    x: f32,
    /// Width in pixels (may be < 1 at extreme zoom-out).
    w: f32,
    label: String,
    color: egui::Color32,
    selected: bool,
    track_kind: TrackKind,
    /// Source duration for relative positioning.
    source_duration: i64,
    source_in: i64,
    /// Fade in/out frames for visual indicators.
    fade_in_frames: i64,
    fade_out_frames: i64,
    /// Real waveform peaks (0.0–1.0) for audio clips, empty if unavailable.
    waveform_peaks: Vec<f32>,
    /// Thumbnail indices with x-fractions (0..1) for video clips.
    thumbnail_indices: Vec<(usize, f32)>,
    /// Whether thumbnails are available for this asset.
    has_thumbnails: bool,
    /// Speed multiplier (for badge display).
    speed: f64,
    /// Audio gain in dB (for badge display).
    gain_db: Option<f32>,
    /// Whether this clip is a compound clip (has nested contents).
    is_compound: bool,
    /// Link group ID (for compound clip indicator).
    link_group_id: Option<u64>,
    /// Speed curve points for visual ramp indicators.
    speed_curve_points: Vec<(f32, f64)>, // (x_fraction 0..1, speed_multiplier)
    /// Whether this clip has audio fade-in handles.
    has_audio_fade: bool,
    /// Clip-level audio gain for gain line overlay.
    audio_gain_db: f32,
    /// Volume keyframe positions for gain line dots (local frame, gain_db).
    volume_keyframes: Vec<(i64, f64)>,
}

pub struct TimelinePanel {
    /// Active FCP-style tool.
    pub active_tool: Tool,
    /// Pixels per timeline frame.
    zoom: f32,
    /// Horizontal scroll offset in scroll-space pixels.
    scroll_x: f32,
    /// Vertical scroll offset.
    scroll_y: f32,
    /// Clip being dragged (if any). Stores the clip-id and pointer offset.
    drag_clip: Option<DragState>,
    /// Clip context menu popup position.
    clip_context_popup: Option<(ClipId, egui::Pos2)>,
    /// Track context menu popup position.
    track_context_popup: Option<(TrackId, egui::Pos2)>,
    /// Clip being trimmed (if any). Edge and original range.
    trim_state: Option<TrimState>,
    /// Range-select drag in progress: (start_frame, current_frame).
    range_select: Option<(i64, i64)>,
    /// Hand-tool pan anchor: (scroll_x, scroll_y) at drag start.
    hand_drag_anchor: Option<(f32, f32)>,
    /// Last frame for detecting double-click.
    last_click_frame: Option<(ClipId, f64)>,
    /// Snapping enabled.
    snapping: bool,
    /// Overwrite edit mode — insert at playhead overwrites existing clips.
    pub overwrite_mode: bool,
    /// Audio waveform cache — lazily populated from ffmpeg.
    waveform_cache: WaveformCache,
    /// Assets we've attempted waveform extraction for.
    waveforms_tried: HashSet<AssetId>,
    /// Visual Y positions for tracks (accounts for audio/video split layout).
    visual_track_ys: HashMap<TrackId, f32>,
    /// Video thumbnail cache — lazily populated from ffmpeg.
    /// Current track height (cycles through 4 presets).
    track_h: f32,
    pub thumbnail_cache: ThumbnailCache,
    /// Assets we've attempted thumbnail extraction for.
    thumbnails_tried: HashSet<AssetId>,
    /// Slip trim state (Option+drag inside clip body).
    slip_state: Option<SlipState>,
    /// Track drag reorder state.
    track_drag: Option<TrackDragState>,
    /// Drop target index for track reorder visual feedback.
    track_drop_target: Option<usize>,
    /// Whether the track drag has moved far enough to count as reorder.
    track_drag_moved: bool,
    /// Slide trim state.
    slide_state: Option<SlideState>,
    /// Speed ramp point drag state.
    speed_ramp_drag: Option<SpeedRampDragState>,
    /// Copied clip attributes for paste operations.
    clipboard: Option<ClipAttributes>,
    /// Show the trim edit window.
    show_trim_window: bool,
    /// Marker being edited (clicked marker).
    editing_marker: Option<MarkerId>,
    /// Compound clip navigation stack (breadcrumb).
    /// Each entry is the compound ClipId we've navigated into.
    compound_nav: Vec<ClipId>,
    /// Saved top-level tracks when inside a compound clip.
    /// Restored when navigating back to project level.
    saved_tracks: Vec<Track>,
    /// Saved playhead/in-points etc. from top level.
    saved_playhead: i64,
    saved_in_point: Option<i64>,
    saved_out_point: Option<i64>,
    /// Timeline index bar rect for click-to-jump.
    timeline_index_rect: Option<egui::Rect>,
}

/// Snapshot of clip properties for copy/paste.
#[derive(Clone)]
struct ClipAttributes {
    transform: rook_core::transform::Transform,
    blend_mode: rook_core::clip::BlendMode,
    opacity: f32,
    speed: f64,
    fade: Option<rook_core::clip::Fade>,
    gain_db: Option<f32>,
    mute_audio: bool,
}

#[derive(Clone)]
struct SlipState {
    clip_id: ClipId,
    orig_source_in: i64,
    orig_source_duration: i64,
    /// Frame offset from clip start where drag began.
    drag_start_frame: i64,
}

#[derive(Clone)]
struct TrackDragState {
    track_id: TrackId,
    orig_index: usize,
    /// Screen Y offset from track header top.
    offset_y: f32,
}

/// State for speed ramp point drag.
#[derive(Clone)]
struct SpeedRampDragState {
    clip_id: ClipId,
    /// Index into the speed_curve vec.
    point_index: usize,
    /// Original frame value of the point.
    orig_frame: i64,
}

/// State for slide trim (Cmd+drag clip between adjacent clips).
#[derive(Clone)]
struct SlideState {
    clip_id: ClipId,
    /// Original timeline_in of the slid clip.
    orig_timeline_in: i64,
    /// Left neighbor: original source_duration.
    left_dur: Option<i64>,
    /// Left neighbor clip id.
    left_id: Option<ClipId>,
    /// Right neighbor: original timeline_in.
    right_timeline_in: Option<i64>,
    /// Right neighbor: original source_in.
    right_source_in: Option<i64>,
    /// Right neighbor: original source_duration.
    right_dur: Option<i64>,
    /// Right neighbor clip id.
    right_id: Option<ClipId>,
}

impl Default for TimelinePanel {
    fn default() -> Self {
        Self {
            active_tool: Tool::Select,
            zoom: 15.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            clip_context_popup: None,
            track_context_popup: None,
            drag_clip: None,
            trim_state: None,
            range_select: None,
            hand_drag_anchor: None,
            last_click_frame: None,
            snapping: true,
            overwrite_mode: false,
            waveform_cache: WaveformCache::new(),
            waveforms_tried: HashSet::new(),
            visual_track_ys: HashMap::new(),
            track_h: TRACK_H_MEDIUM,
            thumbnail_cache: ThumbnailCache::new(),
            thumbnails_tried: HashSet::new(),
            slip_state: None,
            track_drag: None,
            track_drop_target: None,
            track_drag_moved: false,
            slide_state: None,
            speed_ramp_drag: None,
            clipboard: None,
            show_trim_window: false,
            editing_marker: None,
            compound_nav: Vec::new(),
            saved_tracks: Vec::new(),
            saved_playhead: 0,
            saved_in_point: None,
            saved_out_point: None,
            timeline_index_rect: None,
        }
    }
}

#[derive(Clone)]
struct DragState {
    clip_id: ClipId,
    orig_track: TrackId,
    orig_pos: i64, // timeline frame
    /// Pointer offset from clip left edge in pixels.
    offset_x: f32,
}

#[derive(Clone)]
struct TrimState {
    clip_id: ClipId,
    edge: TrimEdge,
    orig_in: i64,
    orig_out: i64,
    orig_dur: i64,
    /// Original timeline_in of the clip being trimmed (for delta calculation).
    orig_timeline_in: i64,
    /// For roll trim: the adjacent clip that shares this boundary.
    roll_clip_id: Option<ClipId>,
    roll_orig_in: Option<i64>,
    roll_orig_dur: Option<i64>,
    /// Ripple trim: shift all subsequent clips on this track.
    ripple: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum TrimEdge {
    Left,
    Right,
}

impl TimelinePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, project: &mut Project, playhead: &mut i64) {
        project.timeline.playhead = *playhead;
        let fps = project.timeline.frame_rate.as_f64();
        let total_dur = project.timeline.duration().max(1);

        // ── Debug: always-visible timeline header ─────────────────────
        ui.colored_label(egui::Color32::from_rgb(100, 180, 255), "▬▬ TIMELINE ▬▬");
        ui.separator();

        // Keyboard shortcuts
        self.handle_keys(ui, project, playhead);

        // ── Breadcrumb for compound clips ────────────────────────────
        if !self.compound_nav.is_empty() {
            let nav_clone = self.compound_nav.clone();
            ui.horizontal(|ui| {
                ui.label("📁");
                if ui
                    .button("⏶ Project")
                    .on_hover_text("Back to project level")
                    .clicked()
                {
                    self.exit_all_compounds(project);
                }
                for (i, &cid) in nav_clone.iter().enumerate() {
                    ui.label("▸");
                    let label = project
                        .timeline
                        .clip(cid)
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| format!("Clip {}", cid.0));
                    if ui.button(format!("📦 {}", label)).clicked() {
                        // Navigate back to this level
                        while self.compound_nav.len() > i + 1 {
                            self.exit_compound(project);
                        }
                    }
                }
                if ui
                    .button("↩ Exit")
                    .on_hover_text("Exit compound clip")
                    .clicked()
                {
                    self.exit_compound(project);
                }
                ui.separator();
            });
        }

        // ── Toolbar ─────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            // ── Tool strip (FCP-style) ───────────────────────────────────
            for tool in &[
                Tool::Select,
                Tool::Blade,
                Tool::Trim,
                Tool::RangeSelect,
                Tool::Zoom,
                Tool::Hand,
                Tool::Position,
            ] {
                let selected = self.active_tool == *tool;
                let label = match tool {
                    Tool::Select => "⇱",
                    Tool::Blade => "✂",
                    Tool::Trim => "⏩",
                    Tool::RangeSelect => "⏺",
                    Tool::Zoom => "🔍",
                    Tool::Hand => "✋",
                    Tool::Position => "↕",
                };
                let btn = egui::Button::new(egui::RichText::new(label).size(14.0))
                    .fill(if selected {
                        egui::Color32::from_rgb(80, 140, 220)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .min_size(egui::vec2(28.0, 24.0));
                if ui.add(btn).on_hover_text(tool.to_string()).clicked() {
                    self.active_tool = *tool;
                }
            }

            ui.separator();

            // Overwrite toggle
            let ow_btn = egui::Button::new(
                egui::RichText::new(if self.overwrite_mode { "↯" } else { "↯" }).size(14.0),
            )
            .fill(if self.overwrite_mode {
                egui::Color32::from_rgb(220, 80, 40)
            } else {
                egui::Color32::TRANSPARENT
            })
            .min_size(egui::vec2(28.0, 24.0));
            if ui.add(ow_btn).on_hover_text("Overwrite Mode").clicked() {
                self.overwrite_mode = !self.overwrite_mode;
            }

            // Snapping toggle
            let snap_btn = egui::Button::new(
                egui::RichText::new(if self.snapping { "🧲" } else { "🧲✗" }).size(14.0),
            )
            .fill(if self.snapping {
                egui::Color32::from_rgb(80, 140, 220)
            } else {
                egui::Color32::TRANSPARENT
            })
            .min_size(egui::vec2(28.0, 24.0));
            if ui.add(snap_btn).on_hover_text("Snapping (N)").clicked() {
                self.snapping = !self.snapping;
            }

            ui.separator();

            let secs = *playhead as f64 / fps;
            let total_secs = total_dur as f64 / fps;
            ui.label(format!(
                "{:02}:{:05.2} / {:02}:{:05.2}",
                (secs / 60.0) as i64,
                secs % 60.0,
                (total_secs / 60.0) as i64,
                total_secs % 60.0
            ));

            ui.separator();
            ui.label("Zoom:");
            if ui
                .add(
                    egui::Slider::new(&mut self.zoom, MIN_PX_PER_FRAME..=MAX_PX_PER_FRAME)
                        .logarithmic(true)
                        .text("px/f"),
                )
                .changed()
            {}

            ui.separator();
            if ui.button("Fit").clicked() {
                let av_w = ui.available_width().max(100.0);
                let dur = project.timeline.duration().max(1) as f32;
                self.zoom = (av_w / dur).clamp(MIN_PX_PER_FRAME, MAX_PX_PER_FRAME);
                self.scroll_x = 0.0;
                self.scroll_y = 0.0;
            }

            ui.separator();
            // Track height cycle button
            let size_label = if self.track_h == TRACK_H_MINI {
                "📏 Mini"
            } else if self.track_h == TRACK_H_SMALL {
                "📏 Small"
            } else if self.track_h == TRACK_H_LARGE {
                "📏 Large"
            } else {
                "📏 Med"
            };
            if ui.button(size_label).clicked() {
                self.track_h = if self.track_h == TRACK_H_MINI {
                    TRACK_H_SMALL
                } else if self.track_h == TRACK_H_SMALL {
                    TRACK_H_MEDIUM
                } else if self.track_h == TRACK_H_MEDIUM {
                    TRACK_H_LARGE
                } else {
                    TRACK_H_MINI
                };
            }

            ui.separator();
            let nav_prefix = if self.inside_compound() { "📦 " } else { "" };
            ui.label(format!(
                "{}{}trk {}clips | Tool: {}",
                nav_prefix,
                project.timeline.tracks.len(),
                project
                    .timeline
                    .tracks
                    .iter()
                    .map(|t| t.clips.len())
                    .sum::<usize>(),
                self.active_tool,
            ));
        });

        // ── Timeline canvas ─────────────────────────────────────────────
        let total_frames = project.timeline.duration().max(1);
        let total_tracks = project.timeline.tracks.len();
        let canvas_w = total_frames as f32 * self.zoom + 200.0; // pad right
        let canvas_h = self.visual_canvas_h(project);

        // Scroll area — fill available vertical space, scroll horizontally freely
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .scroll_offset(egui::vec2(self.scroll_x, self.scroll_y))
            .show(ui, |ui| {
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(canvas_w, canvas_h),
                    egui::Sense::click_and_drag(),
                );
                let clip_rect = response.rect;

                // Compute visual track Y positions now that we know clip_rect.top()
                self.compute_visual_ys(project, clip_rect.top());

                // ── Canvas background — clearly distinguishable from panel fill ──
                painter.rect_filled(
                    clip_rect,
                    0.0,
                    egui::Color32::from_rgb(32, 36, 42),
                );

                // ── Navigation shortcuts (need clip_rect) ────────────
                let input_state = ui.input(|i| i.clone());
                // F = scroll to playhead
                if input_state.key_pressed(egui::Key::F)
                    && !input_state.modifiers.command
                    && !input_state.modifiers.ctrl
                    && self.active_tool != Tool::Position
                {
                    let view_w = ui.clip_rect().width().max(100.0);
                    let playhead_px = *playhead as f32 * self.zoom + TRACK_HEADER_W;
                    self.scroll_x = (playhead_px - view_w / 2.0).max(0.0);
                }
                // Shift+Z = zoom to fit timeline
                if input_state.key_pressed(egui::Key::Z)
                    && input_state.modifiers.shift
                    && !input_state.modifiers.command
                    && !input_state.modifiers.alt
                {
                    let dur = total_frames.max(1) as f32;
                    let avail_w = ui.clip_rect().width().max(200.0) - TRACK_HEADER_W;
                    self.zoom = (avail_w / dur).clamp(MIN_PX_PER_FRAME, MAX_PX_PER_FRAME);
                    self.scroll_x = 0.0;
                    self.scroll_y = 0.0;
                }
                // Opt+Shift+Z = zoom to selection
                if input_state.key_pressed(egui::Key::Z)
                    && input_state.modifiers.shift
                    && input_state.modifiers.alt
                {
                    if !project.timeline.selected_clip_ids.is_empty() {
                        let min_in = project
                            .timeline
                            .selected_clip_ids
                            .iter()
                            .filter_map(|&cid| project.timeline.clip(cid))
                            .map(|c| c.timeline_in)
                            .min()
                            .unwrap_or(0);
                        let max_out = project
                            .timeline
                            .selected_clip_ids
                            .iter()
                            .filter_map(|&cid| project.timeline.clip(cid))
                            .map(|c| c.timeline_in + c.duration())
                            .max()
                            .unwrap_or(1);
                        let sel_dur = (max_out - min_in).max(1) as f32;
                        let avail_w = ui.clip_rect().width().max(200.0) - TRACK_HEADER_W;
                        self.zoom = (avail_w / sel_dur).clamp(MIN_PX_PER_FRAME, MAX_PX_PER_FRAME);
                        self.scroll_x = (min_in as f32 * self.zoom).max(0.0);
                    }
                }
                // Cmd+= / Cmd+- = zoom in/out
                if input_state.key_pressed(egui::Key::Equals)
                    && (input_state.modifiers.command || input_state.modifiers.ctrl)
                {
                    self.zoom = (self.zoom * 1.5).min(MAX_PX_PER_FRAME);
                }
                if input_state.key_pressed(egui::Key::Minus)
                    && (input_state.modifiers.command || input_state.modifiers.ctrl)
                {
                    self.zoom = (self.zoom / 1.5).max(MIN_PX_PER_FRAME);
                }

                // Track scroll offset after painting
                if input_state.key_pressed(egui::Key::Space) {
                    // handled by key handler
                }

                // Build clip geometries
                let clips_geom = self.build_clip_geoms(project, clip_rect.left());

                // ── Empty state message ──────────────────────────────────
                if total_tracks == 0 {
                    let cx = clip_rect.center().x;
                    let cy = clip_rect.top() + RULER_H + 40.0;
                    painter.text(
                        egui::pos2(cx, cy),
                        egui::Align2::CENTER_CENTER,
                        "No tracks yet — File → Import Media to start",
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_gray(140),
                    );
                    painter.text(
                        egui::pos2(cx, cy + 20.0),
                        egui::Align2::CENTER_CENTER,
                        "or drag & drop media files here",
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_gray(100),
                    );
                }

                // ── Paint ruler ─────────────────────────────────────────
                self.paint_ruler(&painter, clip_rect, total_frames, fps, project);

                // ── Track drag drop indicator ──────────────────────────
                if let Some(drop_idx) = self.track_drop_target {
                    let drop_y = if drop_idx < project.timeline.tracks.len() {
                        self.visual_track_ys
                            .get(&project.timeline.tracks[drop_idx].id)
                            .copied()
                            .unwrap_or(clip_rect.top() + RULER_H + drop_idx as f32 * self.track_h)
                    } else {
                        // Dropping past the last track
                        let last_track = project.timeline.tracks.last();
                        if let Some(lt) = last_track {
                            self.visual_track_ys
                                .get(&lt.id)
                                .copied()
                                .unwrap_or(0.0)
                                + self.track_h
                        } else {
                            clip_rect.top() + RULER_H
                        }
                    };
                    let drop_rect = egui::Rect::from_min_size(
                        egui::pos2(clip_rect.left() + TRACK_HEADER_W, drop_y),
                        egui::vec2(canvas_w - TRACK_HEADER_W, 3.0),
                    );
                    painter.rect_filled(drop_rect, 1.0, egui::Color32::from_rgb(80, 180, 255));
                }

                // ── Paint track backgrounds ─────────────────────────────
                let track_count = project.timeline.tracks.len();
                let mut tracks_to_mute: Vec<TrackId> = Vec::new();
                let mut tracks_to_solo: Vec<TrackId> = Vec::new();
                let mut vis_idx = 0usize;
                let video_count = video_vis_count(project);
                let audio_count = audio_vis_count(project);

                for track in &project.timeline.tracks {
                    let y = self
                        .visual_track_ys
                        .get(&track.id)
                        .copied()
                        .unwrap_or(clip_rect.top() + RULER_H);

                    // Track header
                    let hdr_rect = egui::Rect::from_min_size(
                        egui::pos2(clip_rect.left(), y),
                        egui::vec2(TRACK_HEADER_W, self.track_h),
                    );
                    let hdr_color = track_header_color(track);
                    painter.rect_filled(hdr_rect, 0.0, hdr_color);

                    if track.kind.is_audio() && self.track_h >= TRACK_H_SMALL {
                        // ── Audio track header: M/S buttons + gain ────────
                        let btn_w = 20.0;
                        let btn_h = 14.0;
                        let pad = 3.0;
                        let left_x = hdr_rect.left() + pad;

                        // Mute button
                        let mute_rect = egui::Rect::from_min_size(
                            egui::pos2(left_x, hdr_rect.top() + pad),
                            egui::vec2(btn_w, btn_h),
                        );
                        let mute_color = if track.muted {
                            egui::Color32::from_rgb(220, 80, 60)
                        } else {
                            egui::Color32::from_gray(60)
                        };
                        painter.rect_filled(mute_rect, egui::CornerRadius::same(3), mute_color);
                        painter.text(
                            mute_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "M",
                            egui::FontId::proportional(9.0),
                            if track.muted { egui::Color32::WHITE } else { egui::Color32::from_gray(160) },
                        );
                        let mute_resp = ui.interact(mute_rect, ui.next_auto_id(), egui::Sense::click());
                        if mute_resp.clicked() {
                            tracks_to_mute.push(track.id);
                        }

                        // Solo button
                        let solo_rect = egui::Rect::from_min_size(
                            egui::pos2(left_x + btn_w + 2.0, hdr_rect.top() + pad),
                            egui::vec2(btn_w, btn_h),
                        );
                        let solo_color = if track.solo {
                            egui::Color32::from_rgb(220, 180, 40)
                        } else {
                            egui::Color32::from_gray(60)
                        };
                        painter.rect_filled(solo_rect, egui::CornerRadius::same(3), solo_color);
                        painter.text(
                            solo_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "S",
                            egui::FontId::proportional(9.0),
                            if track.solo { egui::Color32::BLACK } else { egui::Color32::from_gray(160) },
                        );
                        let solo_resp = ui.interact(solo_rect, ui.next_auto_id(), egui::Sense::click());
                        if solo_resp.clicked() {
                            tracks_to_solo.push(track.id);
                        }

                        // dB gain display
                        let gain_text = if let Some(db) = track.gain_db {
                            format!("{:.0}dB", db)
                        } else {
                            "0dB".to_string()
                        };
                        painter.text(
                            egui::pos2(hdr_rect.right() - 2.0, hdr_rect.top() + pad),
                            egui::Align2::RIGHT_TOP,
                            gain_text,
                            egui::FontId::proportional(8.0),
                            egui::Color32::from_rgb(140, 200, 140),
                        );

                        // Track name (abbreviated)
                        painter.text(
                            egui::pos2(hdr_rect.left() + 2.0, hdr_rect.bottom() - 2.0),
                            egui::Align2::LEFT_BOTTOM,
                            &track.name[..track.name.len().min(5)],
                            egui::FontId::proportional(7.0),
                            egui::Color32::from_gray(160),
                        );
                    } else {
                        // ── Standard track header (video/text/effect) ────
                        let icons = format!(
                            "{}{}{}{}{}",
                            if track.muted { "🔇" } else { "" },
                            if track.solo { "🟡" } else { "" },
                            if track.locked { "🔒" } else { "" },
                            if !track.visible { "👻" } else { "" },
                            if track.disabled { "🚫" } else { "" },
                        );
                        painter.text(
                            egui::pos2(hdr_rect.left() + 2.0, hdr_rect.top() + 2.0),
                            egui::Align2::LEFT_TOP,
                            &track.name[..track.name.len().min(5)],
                            egui::FontId::proportional(8.0),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            egui::pos2(hdr_rect.left() + 2.0, hdr_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            icons,
                            egui::FontId::proportional(7.0),
                            egui::Color32::from_gray(180),
                        );

                        // Delete indicator (X) for empty tracks
                        if track.clips.is_empty() {
                            painter.text(
                                egui::pos2(hdr_rect.right() - 10.0, hdr_rect.top() + 2.0),
                                egui::Align2::RIGHT_TOP,
                                "✕",
                                egui::FontId::proportional(9.0),
                                egui::Color32::from_gray(120),
                            );
                        }

                        // Click header to toggle mute/lock (deferred)
                        let hdr_resp = ui.interact(hdr_rect, ui.next_auto_id(), egui::Sense::click());
                        if hdr_resp.double_clicked() {
                            tracks_to_mute.push(track.id);
                        }
                    }

                    // Track strip background
                    let strip_rect = egui::Rect::from_min_size(
                        egui::pos2(clip_rect.left() + TRACK_HEADER_W, y),
                        egui::vec2(canvas_w - TRACK_HEADER_W, self.track_h),
                    );
                    let zebra = if track.kind.is_audio() {
                        // Audio tracks: slightly different tint
                        if vis_idx % 2 == 0 {
                            egui::Color32::from_rgb(26, 32, 28) // green-tinted dark
                        } else {
                            egui::Color32::from_rgb(22, 28, 24)
                        }
                    } else {
                        if vis_idx % 2 == 0 {
                            egui::Color32::from_gray(28)
                        } else {
                            egui::Color32::from_gray(24)
                        }
                    };
                    painter.rect_filled(strip_rect, 0.0, zebra);

                    // Track divider line
                    painter.line_segment(
                        [
                            egui::pos2(strip_rect.left(), strip_rect.bottom()),
                            egui::pos2(strip_rect.right(), strip_rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_gray(40)),
                    );

                    vis_idx += 1;
                }

                // ── Audio separator bar ────────────────────────────────────
                if video_count > 0 && audio_count > 0 {
                    let sep_y = self.audio_separator_y(project, clip_rect.top());
                    let sep_rect = egui::Rect::from_min_size(
                        egui::pos2(clip_rect.left(), sep_y),
                        egui::vec2(canvas_w, AUDIO_SEPARATOR_H),
                    );
                    // Dark background for separator
                    painter.rect_filled(
                        sep_rect,
                        0.0,
                        egui::Color32::from_rgb(24, 28, 26),
                    );
                    // Label
                    painter.text(
                        egui::pos2(clip_rect.left() + TRACK_HEADER_W + 4.0, sep_y + AUDIO_SEPARATOR_H * 0.5),
                        egui::Align2::LEFT_CENTER,
                        "🎵 AUDIO",
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_rgb(140, 200, 140),
                    );
                    // Audio time ruler below video, above audio tracks
                    let audio_ruler_rect = egui::Rect::from_min_size(
                        egui::pos2(sep_rect.left() + TRACK_HEADER_W, sep_y + 2.0),
                        egui::vec2(canvas_w - TRACK_HEADER_W, AUDIO_SEPARATOR_H - 4.0),
                    );
                    self.paint_ruler(&painter, audio_ruler_rect, total_frames, fps, project);
                    // Top and bottom border lines
                    painter.line_segment(
                        [
                            egui::pos2(clip_rect.left() + TRACK_HEADER_W, sep_y),
                            egui::pos2(clip_rect.right().max(clip_rect.left() + canvas_w), sep_y),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 80, 60)),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(clip_rect.left() + TRACK_HEADER_W, sep_y + AUDIO_SEPARATOR_H),
                            egui::pos2(clip_rect.right().max(clip_rect.left() + canvas_w), sep_y + AUDIO_SEPARATOR_H),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 80, 60)),
                    );
                }

                // Apply deferred track toggles
                for tid in &tracks_to_mute {
                    if let Some(t) = project.timeline.track_mut(*tid) {
                        t.muted = !t.muted;
                    }
                }
                for tid in &tracks_to_solo {
                    if let Some(t) = project.timeline.track_mut(*tid) {
                        t.solo = !t.solo;
                    }
                }

                // ── Paint clips ─────────────────────────────────────────
                for cg in &clips_geom {
                    let y = self.track_y(cg.track_id, project, clip_rect.top());
                    let clip_r = egui::Rect::from_min_size(
                        egui::pos2(cg.x, y + 3.0),
                        egui::vec2(cg.w.max(2.0), self.track_h - 6.0),
                    );

                    // Clip body
                    painter.rect_filled(clip_r, egui::CornerRadius::same(3), cg.color);

                    // ── Thumbnail strips / waveform ──────────────────────
                    if cg.w > 20.0 && clip_r.height() > 10.0 {
                        match cg.track_kind {
                            TrackKind::Video => {
                                // Render real thumbnails if available, fall back to pseudo-strips
                                if cg.has_thumbnails {
                                    let inner_h = clip_r.height() * 0.55;
                                    let inner_y = clip_r.center().y - inner_h / 2.0;
                                    // Paint each thumbnail at its x-fraction position
                                    for &(thumb_idx, frac) in &cg.thumbnail_indices {
                                        // Get thumbnail strip to look up size info
                                        if let Some(strip) = self.thumbnail_cache.get(cg.asset_id) {
                                            if let Some(thumb) = strip.thumbs.get(thumb_idx) {
                                                let tex_id = self.thumbnail_cache.texture(
                                                    ui.ctx(),
                                                    cg.asset_id,
                                                    thumb_idx,
                                                    thumb,
                                                );
                                                let thumb_aspect =
                                                    thumb.width as f32 / thumb.height as f32;
                                                let thumb_h = inner_h;
                                                let thumb_w = thumb_h * thumb_aspect;
                                                let tx =
                                                    clip_r.left() + frac * cg.w - thumb_w / 2.0;
                                                let thumb_rect = egui::Rect::from_min_size(
                                                    egui::pos2(tx, inner_y),
                                                    egui::vec2(thumb_w, thumb_h),
                                                );
                                                // Clip to clip bounds
                                                if thumb_rect.intersects(clip_r) {
                                                    let uv = egui::Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    );
                                                    painter.image(
                                                        tex_id,
                                                        thumb_rect,
                                                        uv,
                                                        egui::Color32::WHITE,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // Fallback: pseudo-thumbnail strips
                                    let strip_count = (cg.w / 8.0).clamp(1.0, 40.0) as usize;
                                    let seg_w = cg.w / strip_count as f32;
                                    let inner_h = clip_r.height() * 0.55;
                                    let inner_y = clip_r.center().y - inner_h / 2.0;
                                    for i in 0..strip_count {
                                        let sx = clip_r.left() + i as f32 * seg_w;
                                        let hue = ((cg.asset_id.0.wrapping_add(i as u64) as f32
                                            * 0.17)
                                            .fract()
                                            * 0.4
                                            + 0.55)
                                            .min(1.0);
                                        let sat = 0.3 + (i % 3) as f32 * 0.1;
                                        let val = 0.3 + (i % 5) as f32 * 0.05;
                                        let (r, g, b) = hsv_to_rgb(hue, sat, val);
                                        let seg_color = egui::Color32::from_rgb(
                                            (r * 255.0) as u8,
                                            (g * 255.0) as u8,
                                            (b * 255.0) as u8,
                                        );
                                        let seg_r = egui::Rect::from_min_size(
                                            egui::pos2(sx, inner_y),
                                            egui::vec2(seg_w.max(1.0), inner_h),
                                        );
                                        painter.rect_filled(
                                            seg_r,
                                            egui::CornerRadius::same(1),
                                            seg_color,
                                        );
                                    }
                                }
                                // ── Waveform bar under video clip ──────────────
                                if !cg.waveform_peaks.is_empty() {
                                    let bar_count = cg.waveform_peaks.len();
                                    let bar_w = (cg.w / bar_count as f32 - 0.5).max(0.5);
                                    let bar_h = (clip_r.height() * 0.12).max(2.0);
                                    let bar_y = clip_r.bottom() - bar_h - 1.0;
                                    let mid_y = bar_y + bar_h / 2.0;
                                    let half_h = bar_h / 2.0;
                                    for (i, peak) in cg.waveform_peaks.iter().enumerate() {
                                        let h = (*peak * half_h).max(0.5);
                                        let bar_r = egui::Rect::from_min_size(
                                            egui::pos2(
                                                clip_r.left() + i as f32 * (bar_w + 0.5),
                                                mid_y - h,
                                            ),
                                            egui::vec2(bar_w.max(0.5), h * 2.0),
                                        );
                                        // Green waveform bar, semi-transparent
                                        let alpha = if *peak > 0.7 {
                                            220u8
                                        } else if *peak > 0.3 {
                                            160u8
                                        } else {
                                            100u8
                                        };
                                        painter.rect_filled(
                                            bar_r,
                                            egui::CornerRadius::same(0),
                                            egui::Color32::from_rgba_premultiplied(
                                                100, 220, 100, alpha,
                                            ),
                                        );
                                    }
                                }
                            }
                            TrackKind::Audio => {
                                // Use real waveform data if available, fall back to pseudo-random
                                if !cg.waveform_peaks.is_empty() {
                                    let bar_count = cg.waveform_peaks.len();
                                    let bar_w = (cg.w / bar_count as f32 - 1.0).max(1.0);
                                    let mid_y = clip_r.center().y;
                                    let max_h = clip_r.height() * 0.38;
                                    for (i, peak) in cg.waveform_peaks.iter().enumerate() {
                                        let h = (*peak * max_h).max(0.5);
                                        let bar_r = egui::Rect::from_min_size(
                                            egui::pos2(
                                                clip_r.left() + i as f32 * (bar_w + 1.0),
                                                mid_y - h,
                                            ),
                                            egui::vec2(bar_w.max(0.5), h * 2.0),
                                        );
                                        painter.rect_filled(
                                            bar_r,
                                            egui::CornerRadius::same(1),
                                            egui::Color32::from_rgba_premultiplied(
                                                160, 220, 120, 180,
                                            ),
                                        );
                                    }
                                } else {
                                    // Fallback: pseudo-random bars (same as before)
                                    let bar_count = (cg.w / 3.0).clamp(2.0, 120.0) as usize;
                                    let bar_w = (cg.w / bar_count as f32 - 1.0).max(1.0);
                                    let mid_y = clip_r.center().y;
                                    let max_h = clip_r.height() * 0.40;
                                    let seed = cg.asset_id.0.wrapping_mul(0x45d9f3b);
                                    for i in 0..bar_count {
                                        let h =
                                            (pseudo_random(seed, i as u64) * max_h as f64) as f32;
                                        let bar_r = egui::Rect::from_min_size(
                                            egui::pos2(
                                                clip_r.left() + i as f32 * (bar_w + 1.0),
                                                mid_y - h,
                                            ),
                                            egui::vec2(bar_w, h * 2.0),
                                        );
                                        let alpha =
                                            ((i as f32 / bar_count as f32) * 100.0 + 100.0) as u8;
                                        painter.rect_filled(
                                            bar_r,
                                            egui::CornerRadius::same(1),
                                            egui::Color32::from_rgba_premultiplied(
                                                180, 220, 140, alpha,
                                            ),
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    // ── Transition indicator ──────────────────────────────
                    let transition_info = project.timeline.clip(cg.clip_id).map(|c| c.transition);
                    if let Some(Some(trans)) = transition_info {
                        let tri_w = 10.0;
                        let tri = [
                            egui::pos2(clip_r.left(), clip_r.top()),
                            egui::pos2(clip_r.left() + tri_w, clip_r.top()),
                            egui::pos2(clip_r.left(), clip_r.top() + tri_w),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            tri.to_vec(),
                            egui::Color32::from_rgba_premultiplied(100, 200, 255, 200),
                            egui::Stroke::NONE,
                        ));
                        // Transition duration label
                        let fps = project.frame_rate.as_f64();
                        let dur_secs = trans.duration_frames as f64 / fps;
                        painter.text(
                            egui::pos2(clip_r.left() + tri_w + 2.0, clip_r.top() + 1.0),
                            egui::Align2::LEFT_TOP,
                            format!("{:0.1}s", dur_secs),
                            egui::FontId::proportional(7.0),
                            egui::Color32::from_rgba_premultiplied(100, 200, 255, 180),
                        );
                    }

                    // ── Compound clip border ──────────────────────────────
                    if cg.is_compound {
                        // Double border for compound clips
                        painter.rect_stroke(
                            clip_r.expand(1.0),
                            egui::CornerRadius::same(4),
                            egui::Stroke::new(
                                2.0,
                                egui::Color32::from_rgba_premultiplied(140, 160, 220, 180),
                            ),
                            egui::StrokeKind::Inside,
                        );
                        // Folder badge
                        let badge_x = clip_r.right() - 24.0;
                        let badge_y = clip_r.top() - 6.0;
                        painter.text(
                            egui::pos2(badge_x, badge_y),
                            egui::Align2::LEFT_TOP,
                            "📦",
                            egui::FontId::proportional(11.0),
                            egui::Color32::WHITE,
                        );
                    }

                    // ── Link group indicator ──────────────────────────────
                    if cg.link_group_id.is_some() && !cg.is_compound {
                        let chain_x = clip_r.right() - 14.0;
                        let chain_y = clip_r.top() + 1.0;
                        painter.text(
                            egui::pos2(chain_x, chain_y),
                            egui::Align2::LEFT_TOP,
                            "🔗",
                            egui::FontId::proportional(9.0),
                            egui::Color32::from_gray(180),
                        );
                    }

                    // ── Fade indicators ──────────────────────────────────
                    if cg.fade_in_frames > 0 {
                        // Fade-in triangle at left edge
                        let fade_w = (cg.fade_in_frames as f32 * self.zoom).min(cg.w * 0.4);
                        let tri = [
                            egui::pos2(clip_r.left(), clip_r.bottom()),
                            egui::pos2(clip_r.left() + fade_w, clip_r.bottom()),
                            egui::pos2(clip_r.left(), clip_r.top()),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            tri.to_vec(),
                            egui::Color32::from_black_alpha(120),
                            egui::Stroke::NONE,
                        ));
                    }
                    if cg.fade_out_frames > 0 {
                        // Fade-out triangle at right edge
                        let fade_w = (cg.fade_out_frames as f32 * self.zoom).min(cg.w * 0.4);
                        let tri = [
                            egui::pos2(clip_r.right(), clip_r.bottom()),
                            egui::pos2(clip_r.right() - fade_w, clip_r.bottom()),
                            egui::pos2(clip_r.right(), clip_r.top()),
                        ];
                        painter.add(egui::Shape::convex_polygon(
                            tri.to_vec(),
                            egui::Color32::from_black_alpha(120),
                            egui::Stroke::NONE,
                        ));
                    }

                    // Border on selected
                    if cg.selected {
                        painter.rect_stroke(
                            clip_r,
                            egui::CornerRadius::same(3),
                            egui::Stroke::new(2.0, egui::Color32::WHITE),
                            egui::StrokeKind::Inside,
                        );
                    }

                    // Label (with speed badge if not 1×)
                    if cg.w > 40.0 {
                        let label_y = clip_r.center().y;
                        painter.text(
                            egui::pos2(clip_r.center().x, label_y),
                            egui::Align2::CENTER_CENTER,
                            &cg.label,
                            egui::FontId::proportional(9.0),
                            egui::Color32::WHITE,
                        );

                        // Speed badge (top-right corner)
                        if cg.speed != 1.0 {
                            let speed_text = if cg.speed == (cg.speed as i64) as f64 {
                                format!("{}×", cg.speed as i64)
                            } else {
                                format!("{:.1}×", cg.speed)
                            };
                            let badge_w = 30.0;
                            let badge_h = 12.0;
                            let badge_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_r.right() - badge_w - 2.0, clip_r.top() + 1.0),
                                egui::vec2(badge_w, badge_h),
                            );
                            painter.rect_filled(
                                badge_rect,
                                egui::CornerRadius::same(3),
                                egui::Color32::from_rgba_premultiplied(40, 40, 40, 200),
                            );
                            painter.text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                speed_text,
                                egui::FontId::proportional(8.0),
                                egui::Color32::from_rgb(255, 220, 100),
                            );
                        }

                        // Gain badge for audio clips (bottom-left)
                        if cg.track_kind == TrackKind::Audio && cg.gain_db.is_some() {
                            let db = cg.gain_db.unwrap_or(0.0);
                            let gain_text = format!("{:.0}dB", db);
                            let badge_w = 30.0;
                            let badge_h = 11.0;
                            let badge_rect = egui::Rect::from_min_size(
                                egui::pos2(clip_r.left() + 2.0, clip_r.bottom() - badge_h - 1.0),
                                egui::vec2(badge_w, badge_h),
                            );
                            painter.rect_filled(
                                badge_rect,
                                egui::CornerRadius::same(2),
                                egui::Color32::from_rgba_premultiplied(40, 40, 40, 180),
                            );
                            painter.text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                gain_text,
                                egui::FontId::proportional(7.0),
                                if db > 0.0 {
                                    egui::Color32::from_rgb(255, 160, 60)
                                } else {
                                    egui::Color32::from_gray(160)
                                },
                            );
                        }
                    }

                    // ── Speed ramp diamonds ─────────────────────────────
                    if !cg.speed_curve_points.is_empty() && cg.w > 20.0 {
                        for &(frac, speed_val) in &cg.speed_curve_points {
                            let sx = clip_r.left() + frac * cg.w;
                            if sx >= clip_r.left() && sx <= clip_r.right() {
                                let sy = clip_r.top() + 6.0;
                                let d = 4.0;
                                let diamond = vec![
                                    egui::pos2(sx, sy - d),
                                    egui::pos2(sx + d, sy),
                                    egui::pos2(sx, sy + d),
                                    egui::pos2(sx - d, sy),
                                ];
                                let diamond_color = if speed_val > 1.0 {
                                    egui::Color32::from_rgb(255, 180, 60)
                                } else if speed_val < 1.0 {
                                    egui::Color32::from_rgb(100, 200, 255)
                                } else {
                                    egui::Color32::from_gray(180)
                                };
                                painter.add(egui::Shape::convex_polygon(
                                    diamond,
                                    diamond_color,
                                    egui::Stroke::new(1.0, egui::Color32::from_black_alpha(120)),
                                ));
                                // Speed label below diamond
                                painter.text(
                                    egui::pos2(sx, sy + d + 2.0),
                                    egui::Align2::CENTER_TOP,
                                    format!("{:.1}×", speed_val),
                                    egui::FontId::proportional(7.0),
                                    diamond_color,
                                );
                            }
                        }
                    }

                    // ── Audio gain line (on waveform) ────────────────────
                    if cg.track_kind == TrackKind::Audio && cg.w > 30.0 && cg.audio_gain_db != 0.0 {
                        // Map dB to vertical position: 0dB → center, +12dB → top, -96dB → bottom
                        let db_norm = (cg.audio_gain_db / 24.0).clamp(-1.0, 1.0);
                        let mid_y = clip_r.center().y;
                        let gain_y = mid_y - db_norm * clip_r.height() * 0.35;
                        // Dashed line across the clip
                        let dash_len = 6.0;
                        let gap_len = 4.0;
                        let mut x = clip_r.left() + 4.0;
                        let gain_color = if cg.audio_gain_db > 0.0 {
                            egui::Color32::from_rgba_premultiplied(255, 180, 60, 140)
                        } else {
                            egui::Color32::from_rgba_premultiplied(160, 200, 255, 120)
                        };
                        while x < clip_r.right() - 4.0 {
                            let end_x = (x + dash_len).min(clip_r.right() - 4.0);
                            painter.line_segment(
                                [egui::pos2(x, gain_y), egui::pos2(end_x, gain_y)],
                                egui::Stroke::new(1.0, gain_color),
                            );
                            x += dash_len + gap_len;
                        }

                        // ── Volume keyframe dots on the gain line ────────
                        let dur_frames = cg.w / self.zoom; // approximate duration in frames
                        for &(local_frame, value) in &cg.volume_keyframes {
                            if dur_frames > 0.0 {
                                let frac = local_frame as f32 / dur_frames;
                                let kx = clip_r.left() + frac * cg.w;
                                let kf_db_norm: f32 = ((value as f32) / 24.0).clamp(-1.0, 1.0);
                                let ky = mid_y - kf_db_norm * clip_r.height() * 0.35;
                                let dot_r = 4.0;
                                let gain_val = value as f32;
                                let dot_color = if gain_val > cg.audio_gain_db {
                                    egui::Color32::from_rgba_premultiplied(255, 200, 60, 220)
                                } else if gain_val < cg.audio_gain_db {
                                    egui::Color32::from_rgba_premultiplied(100, 200, 255, 220)
                                } else {
                                    egui::Color32::from_rgba_premultiplied(200, 200, 200, 200)
                                };
                                painter.circle_filled(egui::pos2(kx, ky), dot_r, dot_color);
                                painter.circle_stroke(
                                    egui::pos2(kx, ky),
                                    dot_r,
                                    egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
                                );
                            }
                        }
                    }

                    // ── Audio fade handles (dots at edges) ────────────────
                    if cg.has_audio_fade && cg.w > 30.0 {
                        // Fade-in dot at left edge
                        if cg.fade_in_frames > 0 {
                            let fade_x = clip_r.left()
                                + (cg.fade_in_frames as f32 * self.zoom).min(cg.w * 0.5);
                            let fy = clip_r.center().y;
                            let dot_r = 3.5;
                            painter.circle_filled(
                                egui::pos2(fade_x, fy),
                                dot_r,
                                egui::Color32::from_rgba_premultiplied(140, 220, 140, 200),
                            );
                            painter.circle_stroke(
                                egui::pos2(fade_x, fy),
                                dot_r,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                            );
                        }
                        // Fade-out dot at right edge
                        if cg.fade_out_frames > 0 {
                            let fade_x = clip_r.right()
                                - (cg.fade_out_frames as f32 * self.zoom).min(cg.w * 0.5);
                            let fy = clip_r.center().y;
                            let dot_r = 3.5;
                            painter.circle_filled(
                                egui::pos2(fade_x, fy),
                                dot_r,
                                egui::Color32::from_rgba_premultiplied(220, 140, 140, 200),
                            );
                            painter.circle_stroke(
                                egui::pos2(fade_x, fy),
                                dot_r,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
                            );
                        }
                    }

                    // Trim handles (subtle L/R zones)
                    if cg.w > 12.0 {
                        let lh = egui::Rect::from_min_size(
                            clip_r.min,
                            egui::vec2(TRIM_HANDLE_W, clip_r.height()),
                        );
                        let rh = egui::Rect::from_min_size(
                            egui::pos2(clip_r.max.x - TRIM_HANDLE_W, clip_r.min.y),
                            egui::vec2(TRIM_HANDLE_W, clip_r.height()),
                        );
                        painter.rect_filled(lh, 0.0, egui::Color32::from_white_alpha(40));
                        painter.rect_filled(rh, 0.0, egui::Color32::from_white_alpha(40));
                    }
                }

                // ── Snap indicators ─────────────────────────────────────
                let mut snap_points: Vec<i64> = Vec::new();
                snap_points.push(*playhead);
                for cg in &clips_geom {
                    let clip = project.timeline.clip(cg.clip_id);
                    if let Some(c) = clip {
                        snap_points.push(c.timeline_in);
                        snap_points.push(c.timeline_in + c.duration());
                    }
                }
                for marker in &project.timeline.markers {
                    snap_points.push(marker.frame);
                }
                // Draw subtle snap lines
                for sp in &snap_points {
                    let sx = clip_rect.left() + *sp as f32 * self.zoom;
                    if sx >= clip_rect.left() && sx <= clip_rect.right() {
                        painter.line_segment(
                            [
                                egui::pos2(sx, clip_rect.top()),
                                egui::pos2(sx, clip_rect.bottom()),
                            ],
                            egui::Stroke::new(0.5, egui::Color32::from_gray(60)),
                        );
                    }
                }

                // ── Playhead ────────────────────────────────────────────
                let px = clip_rect.left() + *playhead as f32 * self.zoom;
                painter.line_segment(
                    [
                        egui::pos2(px, clip_rect.top()),
                        egui::pos2(px, clip_rect.bottom()),
                    ],
                    egui::Stroke::new(2.0, PLAYHEAD_COLOR),
                );
                // Red triangle in ruler area
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(px, clip_rect.top()),
                        egui::pos2(px - 5.0, clip_rect.top() + RULER_H),
                        egui::pos2(px + 5.0, clip_rect.top() + RULER_H),
                    ],
                    PLAYHEAD_COLOR,
                    egui::Stroke::NONE,
                ));

                // ── I/O marks ────────────────────────────────────────────
                let in_color = egui::Color32::from_rgb(0, 200, 80);
                let out_color = egui::Color32::from_rgb(200, 80, 40);
                if let Some(in_pt) = project.timeline.in_point {
                    let ix = clip_rect.left() + in_pt as f32 * self.zoom;
                    painter.line_segment(
                        [
                            egui::pos2(ix, clip_rect.top()),
                            egui::pos2(ix, clip_rect.bottom()),
                        ],
                        egui::Stroke::new(1.5, in_color),
                    );
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(ix, clip_rect.top()),
                            egui::pos2(ix - 5.0, clip_rect.top() + 8.0),
                            egui::pos2(ix + 5.0, clip_rect.top() + 8.0),
                        ],
                        in_color,
                        egui::Stroke::NONE,
                    ));
                }
                if let Some(out_pt) = project.timeline.out_point {
                    let ox = clip_rect.left() + out_pt as f32 * self.zoom;
                    painter.line_segment(
                        [
                            egui::pos2(ox, clip_rect.top()),
                            egui::pos2(ox, clip_rect.bottom()),
                        ],
                        egui::Stroke::new(1.5, out_color),
                    );
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(ox, clip_rect.top()),
                            egui::pos2(ox - 5.0, clip_rect.top() + 8.0),
                            egui::pos2(ox + 5.0, clip_rect.top() + 8.0),
                        ],
                        out_color,
                        egui::Stroke::NONE,
                    ));
                }
                // Highlight range between in/out if both set
                if let (Some(in_pt), Some(out_pt)) =
                    (project.timeline.in_point, project.timeline.out_point)
                {
                    let (start, end) = if in_pt <= out_pt {
                        (in_pt, out_pt)
                    } else {
                        (out_pt, in_pt)
                    };
                    let rx = clip_rect.left() + start as f32 * self.zoom;
                    let rw = ((end - start) as f32 * self.zoom).max(1.0);
                    let range_rect = egui::Rect::from_min_size(
                        egui::pos2(rx, clip_rect.top()),
                        egui::vec2(rw, clip_rect.height()),
                    );
                    painter.rect_filled(
                        range_rect,
                        0.0,
                        egui::Color32::from_rgba_premultiplied(100, 100, 0, 30),
                    );
                }

                // ── Live range-select drag highlight ───────────────────────
                if let Some((rs_start, rs_end)) = self.range_select {
                    let (r_start, r_end) = if rs_start <= rs_end {
                        (rs_start, rs_end)
                    } else {
                        (rs_end, rs_start)
                    };
                    let rx = clip_rect.left() + r_start as f32 * self.zoom;
                    let rw = ((r_end - r_start) as f32 * self.zoom).max(1.0);
                    let range_rect = egui::Rect::from_min_size(
                        egui::pos2(rx, clip_rect.top()),
                        egui::vec2(rw, clip_rect.height()),
                    );
                    painter.rect_filled(
                        range_rect,
                        0.0,
                        egui::Color32::from_rgba_premultiplied(60, 120, 200, 30),
                    );
                    // Range start/end lines
                    for &fr in &[r_start, r_end] {
                        let fx = clip_rect.left() + fr as f32 * self.zoom;
                        painter.line_segment(
                            [
                                egui::pos2(fx, clip_rect.top()),
                                egui::pos2(fx, clip_rect.bottom()),
                            ],
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 140, 220)),
                        );
                    }
                }

                // ── Handle input ────────────────────────────────────────
                self.handle_input(ui, &response, project, &clips_geom, playhead, clip_rect);

                // Save horizontal scroll state only (vertical stays 0 unless hand-tool panning)
                let viewport_rect = ui.clip_rect();
                self.scroll_x = (viewport_rect.min.x - clip_rect.min.x).max(0.0);
                // Don't auto-track scroll_y — prevents vertical drift from viewport/content mismatch
                // Keep scroll_y clamped to prevent negative values
                self.scroll_y = self.scroll_y.max(0.0);
            });

        // ── Clip context menu popup ────────────────────────────────────
        if let Some((cid, popup_pos)) = self.clip_context_popup.take() {
            egui::Area::new("clip_context_menu".into())
                .fixed_pos(popup_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(130.0);
                        if ui.button("✂ Split at Playhead").clicked() {
                            self.split_clip_at(project, cid, project.timeline.playhead);
                            ui.close_menu();
                        }
                        if ui.button("🗑 Delete").clicked() {
                            let tid = project.timeline.clip_track_id(cid);
                            if let Some(tid) = tid {
                                if let Some(track) = project.timeline.track_mut(tid) {
                                    track.remove_clip(cid);
                                }
                            }
                            ui.close_menu();
                        }
                        if ui.button("🌊 Ripple Delete").clicked() {
                            let tid = project.timeline.clip_track_id(cid);
                            if let Some(tid) = tid {
                                if let Some(track) = project.timeline.track_mut(tid) {
                                    if let Some(removed) = track.remove_clip(cid) {
                                        let gap = removed.duration();
                                        for clip in &mut track.clips {
                                            if clip.timeline_in > removed.timeline_in {
                                                clip.timeline_in -= gap;
                                            }
                                        }
                                    }
                                }
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        let is_muted = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.mute_audio)
                            .unwrap_or(false);
                        if ui
                            .button(if is_muted { "🔊 Unmute" } else { "🔇 Mute" })
                            .clicked()
                        {
                            if let Some(clip) = project.timeline.clip_mut(cid) {
                                clip.mute_audio = !clip.mute_audio;
                            }
                            ui.close_menu();
                        }
                        // Detach audio — snapshot clip info first to avoid borrow issues
                        let maybe_detach = {
                            let clip = project.timeline.clip(cid);
                            clip.and_then(|c| {
                                project.asset(c.asset_id).and_then(|a| {
                                    if let rook_core::asset::Asset::Video(v) = a {
                                        if v.metadata
                                            .video
                                            .as_ref()
                                            .map(|vm| vm.has_audio)
                                            .unwrap_or(false)
                                        {
                                            Some((
                                                c.asset_id,
                                                c.timeline_in,
                                                c.source_in,
                                                c.source_duration,
                                                c.speed,
                                                c.speed_curve.clone(),
                                                c.gain_db,
                                                c.mute_audio,
                                                c.filters.clone(),
                                                c.keyframes.clone(),
                                                c.link_group_id,
                                                c.label.clone(),
                                            ))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                            })
                        };
                        if maybe_detach.is_some() {
                            if ui.button("🔊 Detach Audio").clicked() {
                                if let Some((
                                    asset_id,
                                    tl_in,
                                    src_in,
                                    src_dur,
                                    speed,
                                    speed_curve,
                                    gain_db,
                                    mute_audio,
                                    filters,
                                    keyframes,
                                    link_group,
                                    label,
                                )) = maybe_detach
                                {
                                    if project
                                        .timeline
                                        .tracks_of_kind(rook_core::track::TrackKind::Audio)
                                        .is_empty()
                                    {
                                        project.add_audio_track("A1".to_string());
                                    }
                                    if let Some(audio_track) = project
                                        .timeline
                                        .tracks
                                        .iter()
                                        .find(|t| t.kind == rook_core::track::TrackKind::Audio)
                                        .map(|t| t.id)
                                    {
                                        let audio_clip = rook_core::clip::Clip {
                                            id: rook_core::ids::ClipId::next(),
                                            label: format!("{} (audio)", label),
                                            asset_id,
                                            timeline_in: tl_in,
                                            source_in: src_in,
                                            source_duration: src_dur,
                                            transform: rook_core::transform::Transform::default(),
                                            blend_mode: rook_core::clip::BlendMode::Normal,
                                            mask: None,
                                            fade: None,
                                            transition: None,
                                            speed,
                                            speed_curve,
                                            reverse: false,
                                            freeze_frame: None,
                                            frame_blending: false,
                                            spatial_conform: None,
                                            gain_db,
                                            mute_audio,
                                            volume_keyframes: None,
                                            filters,
                                            keyframes,
                                            link_group_id: link_group,
                                            generator: None,
                                        };
                                        if let Some(track) = project.timeline.track_mut(audio_track)
                                        {
                                            track.insert_clip(audio_clip).ok();
                                        }
                                    }
                                }
                                ui.close_menu();
                            }
                        }

                        let has_transition = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.transition.is_some())
                            .unwrap_or(false);
                        if !has_transition {
                            if ui.button("↔ Add Transition").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    clip.transition = Some(rook_core::clip::Transition {
                                        kind: rook_core::clip::TransitionKind::CrossDissolve,
                                        duration_frames: 24,
                                        reversed: false,
                                        curve: rook_core::clip::FadeCurve::Linear,
                                    });
                                }
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        // ── Reverse / Freeze ──────────────────────────
                        let is_reversed = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.reverse)
                            .unwrap_or(false);
                        if ui
                            .button(if is_reversed {
                                "↩ Un-reverse"
                            } else {
                                "↩ Reverse Clip"
                            })
                            .clicked()
                        {
                            if let Some(clip) = project.timeline.clip_mut(cid) {
                                clip.reverse = !clip.reverse;
                            }
                            ui.close_menu();
                        }
                        let is_frozen = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.freeze_frame.is_some())
                            .unwrap_or(false);
                        if is_frozen {
                            if ui.button("🧊 Un-freeze Frame").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    clip.freeze_frame = None;
                                }
                                ui.close_menu();
                            }
                        } else {
                            if ui
                                .button("🧊 Freeze Frame")
                                .on_hover_text("Hold current frame at playhead")
                                .clicked()
                            {
                                let ph = project.timeline.playhead;
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    let local = ph - clip.timeline_in;
                                    clip.freeze_frame = Some(local.max(0));
                                }
                                ui.close_menu();
                            }
                        }
                        // ── Spatial Conform ──────────────────────────
                        let conform = project.timeline.clip(cid).and_then(|c| c.spatial_conform);
                        if ui
                            .button(match conform {
                                Some(rook_core::clip::SpatialConform::Fit) => "📐 Conform: Fit",
                                Some(rook_core::clip::SpatialConform::Fill) => "📐 Conform: Fill",
                                Some(rook_core::clip::SpatialConform::None) => "📐 Conform: None",
                                None => "📐 Conform: Default",
                            })
                            .clicked()
                        {
                            if let Some(clip) = project.timeline.clip_mut(cid) {
                                clip.spatial_conform = match clip.spatial_conform {
                                    None => Some(rook_core::clip::SpatialConform::Fit),
                                    Some(rook_core::clip::SpatialConform::Fit) => {
                                        Some(rook_core::clip::SpatialConform::Fill)
                                    }
                                    Some(rook_core::clip::SpatialConform::Fill) => {
                                        Some(rook_core::clip::SpatialConform::None)
                                    }
                                    Some(rook_core::clip::SpatialConform::None) => None,
                                };
                            }
                            ui.close_menu();
                        }
                        // ── Frame Blending toggle ─────────────────────
                        let has_blending = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.frame_blending)
                            .unwrap_or(false);
                        if ui
                            .button(if has_blending {
                                "🎞 Frame Blending: ON"
                            } else {
                                "🎞 Frame Blending: OFF"
                            })
                            .clicked()
                        {
                            if let Some(clip) = project.timeline.clip_mut(cid) {
                                clip.frame_blending = !clip.frame_blending;
                            }
                            ui.close_menu();
                        }
                        // ── Audio Normalize (audio clips only) ─────────
                        let is_audio = project
                            .timeline
                            .clip_track_id(cid)
                            .and_then(|tid| project.timeline.track(tid))
                            .map(|t| t.kind == TrackKind::Audio)
                            .unwrap_or(false);
                        if is_audio {
                            let norm_data = project.timeline.clip(cid).and_then(|clip| {
                                let asset_path = project
                                    .asset(clip.asset_id)
                                    .map(|a| a.path().to_string())
                                    .unwrap_or_default();
                                let path = std::path::PathBuf::from(&asset_path);
                                let waveform =
                                    self.waveform_cache.get_or_extract(clip.asset_id, &path);
                                let peak = waveform
                                    .as_ref()
                                    .map(|w| w.peaks.iter().cloned().fold(0.0f32, f32::max))
                                    .unwrap_or(0.5);
                                if peak > 0.001 {
                                    let current_db = 20.0 * (peak.max(0.0001)).log10();
                                    let current_gain = clip.gain_db.unwrap_or(0.0);
                                    Some((current_db, current_gain))
                                } else {
                                    None
                                }
                            });
                            if let Some((current_db, current_gain)) = norm_data {
                                // Normalize to -6dB (broadcast standard)
                                let delta_6 = -6.0 - current_db;
                                if ui
                                    .button(format!("🔊 Normalize to -6dB ({:+.0} dB)", delta_6))
                                    .clicked()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(cid) {
                                        clip.gain_db = Some(current_gain + delta_6);
                                    }
                                    ui.close_menu();
                                }
                                // Normalize to -12dB (streaming/YouTube standard)
                                let delta_12 = -12.0 - current_db;
                                if ui
                                    .button(format!("🔉 Normalize to -12dB ({:+.0} dB)", delta_12))
                                    .clicked()
                                {
                                    if let Some(clip) = project.timeline.clip_mut(cid) {
                                        clip.gain_db = Some(current_gain + delta_12);
                                    }
                                    ui.close_menu();
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new("🔇 No waveform data"));
                            }
                        }
                        ui.separator();
                        // ── Compound clip ──────────────────────────────
                        let is_compound = self.is_compound(&project.timeline, cid);
                        if is_compound {
                            if ui.button("📂 Open Compound").clicked() {
                                self.enter_compound(project, cid);
                                ui.close_menu();
                            }
                            if ui.button("💥 Break Apart").clicked() {
                                self.break_apart_compound(project, cid);
                                ui.close_menu();
                            }
                        } else {
                            if project.timeline.selected_clip_ids.len() > 1 {
                                if ui.button("📦 Create Compound Clip").clicked() {
                                    self.create_compound_clip(project);
                                    ui.close_menu();
                                }
                            }
                        }
                        ui.separator();
                        // ── Speed Ramp ──────────────────────────────────
                        let has_speed_ramp = project
                            .timeline
                            .clip(cid)
                            .map(|c| !c.speed_curve.is_empty())
                            .unwrap_or(false);
                        if !has_speed_ramp {
                            if ui.button("⏩ Add Speed Ramp").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    let dur = clip.duration().max(1);
                                    clip.speed_curve.push(rook_core::clip::SpeedCurvePoint {
                                        frame: 0,
                                        speed: clip.speed,
                                    });
                                    clip.speed_curve.push(rook_core::clip::SpeedCurvePoint {
                                        frame: dur,
                                        speed: clip.speed,
                                    });
                                    clip.speed_curve.sort_by_key(|p| p.frame);
                                }
                                ui.close_menu();
                            }
                        } else {
                            let ph = project.timeline.playhead;
                            let sp_at_playhead = project
                                .timeline
                                .clip(cid)
                                .map(|c| {
                                    let local = ph - c.timeline_in;
                                    c.speed_curve.iter().position(|p| p.frame == local)
                                })
                                .unwrap_or(None);
                            if let Some(_idx) = sp_at_playhead {
                                if ui.button("🗑 Remove Speed Point Here").clicked() {
                                    if let Some(clip) = project.timeline.clip_mut(cid) {
                                        let local = ph - clip.timeline_in;
                                        clip.speed_curve.retain(|p| p.frame != local);
                                    }
                                    ui.close_menu();
                                }
                            }
                            if ui.button("➕ Add Speed Point at Playhead").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    let local =
                                        (ph - clip.timeline_in).max(0).min(clip.duration().max(1));
                                    if !clip.speed_curve.iter().any(|p| p.frame == local) {
                                        clip.speed_curve.push(rook_core::clip::SpeedCurvePoint {
                                            frame: local,
                                            speed: 1.0,
                                        });
                                        clip.speed_curve.sort_by_key(|p| p.frame);
                                    }
                                }
                                ui.close_menu();
                            }
                            if ui.button("🧹 Clear Speed Ramp").clicked() {
                                if let Some(clip) = project.timeline.clip_mut(cid) {
                                    clip.speed_curve.clear();
                                }
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        // ── Replace with Source Range ────────────────────
                        let has_io = project.timeline.in_point.is_some()
                            && project.timeline.out_point.is_some();
                        if has_io {
                            if ui
                                .button("↩ Replace with Source Range")
                                .on_hover_text(
                                    "Replace clip content with the I/O-marked source range",
                                )
                                .clicked()
                            {
                                if let (Some(in_pt), Some(out_pt)) =
                                    (project.timeline.in_point, project.timeline.out_point)
                                {
                                    if out_pt > in_pt {
                                        if let Some(clip) = project.timeline.clip_mut(cid) {
                                            clip.source_in = in_pt.max(0);
                                            clip.source_duration = (out_pt - in_pt).max(1);
                                        }
                                    }
                                }
                                ui.close_menu();
                            }
                        }
                        // ── Connect Clip ─────────────────────────────────
                        if ui
                            .button("🔗 Connect Clip")
                            .on_hover_text("Create connected clip above")
                            .clicked()
                        {
                            // Snapshot clip data first
                            let conn_data = project.timeline.clip(cid).map(|clip| {
                                (
                                    clip.asset_id,
                                    clip.timeline_in,
                                    clip.source_in,
                                    clip.source_duration,
                                    clip.transform.clone(),
                                    clip.blend_mode,
                                    clip.mask.clone(),
                                    clip.fade,
                                    clip.speed,
                                    clip.speed_curve.clone(),
                                    clip.reverse,
                                    clip.freeze_frame,
                                    clip.frame_blending,
                                    clip.spatial_conform,
                                    clip.gain_db,
                                    clip.filters.clone(),
                                    clip.keyframes.clone(),
                                    clip.link_group_id,
                                    clip.generator.clone(),
                                    clip.label.clone(),
                                )
                            });
                            if let Some((
                                asset_id,
                                tl_in,
                                src_in,
                                dur,
                                transform,
                                blend_mode,
                                mask,
                                fade,
                                speed,
                                speed_curve,
                                reverse,
                                freeze_frame,
                                frame_blending,
                                spatial_conform,
                                gain_db,
                                filters,
                                keyframes,
                                link_group_id,
                                generator,
                                label,
                            )) = conn_data
                            {
                                let count =
                                    project.timeline.tracks_of_kind(TrackKind::Video).len() + 1;
                                project.add_video_track(format!("V{} Connected", count));
                                if let Some(track_id) = project
                                    .timeline
                                    .tracks
                                    .iter()
                                    .find(|t| t.name.contains("Connected"))
                                    .map(|t| t.id)
                                {
                                    let conn = Clip {
                                        id: ClipId::next(),
                                        label: format!("{} (connected)", label),
                                        asset_id,
                                        timeline_in: tl_in,
                                        source_in: src_in,
                                        source_duration: dur,
                                        transform,
                                        blend_mode,
                                        mask,
                                        fade,
                                        transition: None,
                                        speed,
                                        speed_curve,
                                        reverse,
                                        freeze_frame,
                                        frame_blending,
                                        spatial_conform,
                                        gain_db,
                                        volume_keyframes: None,
                                        mute_audio: true,
                                        filters,
                                        keyframes,
                                        link_group_id,
                                        generator,
                                    };
                                    if let Some(track) = project.timeline.track_mut(track_id) {
                                        track.insert_clip(conn).ok();
                                    }
                                }
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        // ── Group / Ungroup ──────────────────────────────
                        let linked = project.timeline.clip(cid).and_then(|c| c.link_group_id);
                        if linked.is_some() {
                            if ui.button("💔 Ungroup").clicked() {
                                let group_id = linked;
                                for track in &mut project.timeline.tracks {
                                    for clip in &mut track.clips {
                                        if clip.link_group_id == group_id {
                                            clip.link_group_id = None;
                                        }
                                    }
                                }
                                ui.close_menu();
                            }
                        } else {
                            // Check if multiple clips selected
                            if project.timeline.selected_clip_ids.len() > 1 {
                                if ui.button("🔗 Group Selected").clicked() {
                                    let group_id = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_nanos()
                                        as u64;
                                    for cid2 in project.timeline.selected_clip_ids.clone() {
                                        if let Some(clip) = project.timeline.clip_mut(cid2) {
                                            clip.link_group_id = Some(group_id);
                                        }
                                    }
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                });
        }

        // ── Track context menu popup ────────────────────────────────────
        if let Some((tid, popup_pos)) = self.track_context_popup.take() {
            egui::Area::new("track_context_menu".into())
                .fixed_pos(popup_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(120.0);
                        let track_name = project
                            .timeline
                            .track(tid)
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        ui.label(egui::RichText::new(format!("Track: {}", track_name)).strong());
                        ui.separator();
                        // Mute toggle
                        let muted = project
                            .timeline
                            .track(tid)
                            .map(|t| t.muted)
                            .unwrap_or(false);
                        if ui
                            .button(if muted { "🔊 Unmute" } else { "🔇 Mute" })
                            .clicked()
                        {
                            if let Some(t) = project.timeline.track_mut(tid) {
                                t.muted = !t.muted;
                            }
                            ui.close_menu();
                        }
                        // Solo toggle
                        let soloed = project.timeline.track(tid).map(|t| t.solo).unwrap_or(false);
                        if ui
                            .button(if soloed { "🔉 Un-solo" } else { "🟡 Solo" })
                            .clicked()
                        {
                            if let Some(t) = project.timeline.track_mut(tid) {
                                t.solo = !t.solo;
                            }
                            ui.close_menu();
                        }
                        // Disable toggle
                        let disabled = project
                            .timeline
                            .track(tid)
                            .map(|t| t.disabled)
                            .unwrap_or(false);
                        if ui
                            .button(if disabled {
                                "✅ Enable Track"
                            } else {
                                "🚫 Disable Track"
                            })
                            .clicked()
                        {
                            if let Some(t) = project.timeline.track_mut(tid) {
                                t.disabled = !t.disabled;
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        // ── Color labels ────────────────────────────────
                        ui.label("Color Label:");
                        use rook_core::TrackColor;
                        let current_color = project.timeline.track(tid).and_then(|t| t.color);
                        for color in &[
                            TrackColor::Red,
                            TrackColor::Orange,
                            TrackColor::Yellow,
                            TrackColor::Green,
                            TrackColor::Blue,
                            TrackColor::Purple,
                            TrackColor::Pink,
                            TrackColor::Gray,
                        ] {
                            let is_current = current_color == Some(*color);
                            let label = if is_current {
                                format!("● {} ✓", color.label())
                            } else {
                                format!("● {}", color.label())
                            };
                            let text_color = track_color_to_egui(color);
                            if ui
                                .button(egui::RichText::new(label).color(text_color))
                                .clicked()
                            {
                                if let Some(t) = project.timeline.track_mut(tid) {
                                    t.color = if is_current { None } else { Some(*color) };
                                }
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        // Primary storyline
                        let is_primary = project
                            .timeline
                            .track(tid)
                            .map(|t| t.is_primary)
                            .unwrap_or(false);
                        if ui
                            .button(if is_primary {
                                "⭐ Primary Storyline ✓"
                            } else {
                                "☆ Set as Primary"
                            })
                            .clicked()
                        {
                            for track in &mut project.timeline.tracks {
                                track.is_primary = false;
                            }
                            if let Some(t) = project.timeline.track_mut(tid) {
                                t.is_primary = true;
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        // Rename
                        if ui.button("✏ Rename Track").clicked() {
                            // Placeholder — full rename would need text input
                            ui.close_menu();
                        }
                        // Delete track
                        if ui.button("🗑 Delete Track").clicked() {
                            project.timeline.remove_track(tid);
                            ui.close_menu();
                        }
                    });
                });
        }

        // ── Add track buttons ───────────────────────────────────────────
        ui.horizontal(|ui| {
            if ui.button("+ V").on_hover_text("Add Video Track").clicked() {
                let n = project.timeline.tracks_of_kind(TrackKind::Video).len() + 1;
                project.add_video_track(format!("V{n}"));
            }
            if ui.button("+ A").on_hover_text("Add Audio Track").clicked() {
                let n = project.timeline.tracks_of_kind(TrackKind::Audio).len() + 1;
                project.add_audio_track(format!("A{n}"));
            }
            if ui
                .button("+ T")
                .on_hover_text("Add Subtitle Track")
                .clicked()
            {
                let n = project.timeline.tracks_of_kind(TrackKind::Text).len() + 1;
                project.add_text_track(format!("Subtitles {n}"));
            }
            ui.separator();
            if ui
                .button("📝 Title")
                .on_hover_text("Add text title at playhead")
                .clicked()
            {
                // Ensure a text track exists
                if project.timeline.tracks_of_kind(TrackKind::Text).is_empty() {
                    project.add_text_track("Titles 1".to_string());
                }
                if let Some(track_id) = project
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == TrackKind::Text)
                    .map(|t| t.id)
                {
                    let title_clip = Clip {
                        id: ClipId::next(),
                        label: "Title".to_string(),
                        asset_id: AssetId(0), // generator clips don't need real assets
                        timeline_in: project.timeline.playhead,
                        source_in: 0,
                        source_duration: (project.timeline.frame_rate.as_f64() * 5.0) as i64, // 5 seconds
                        transform: rook_core::transform::Transform::default(),
                        blend_mode: rook_core::clip::BlendMode::Normal,
                        mask: None,
                        fade: None,
                        transition: None,
                        speed: 1.0,
                        speed_curve: vec![],
                        reverse: false,
                        freeze_frame: None,
                        frame_blending: false,
                        spatial_conform: None,
                        gain_db: None,
                        volume_keyframes: None,
                        mute_audio: true,
                        filters: vec![],
                        keyframes: vec![],
                        link_group_id: None,
                        generator: Some(rook_core::clip::Generator::Text {
                            content: "Title".to_string(),
                            font_size: 64.0,
                            color: [1.0, 1.0, 1.0, 1.0],
                        }),
                    };
                    if let Some(track) = project.timeline.track_mut(track_id) {
                        track.insert_clip(title_clip).ok();
                    }
                }
            }
            // ── Lower Third presets ────────────────────────────────────
            let l3_presets: &[(&str, &str, rook_core::transform::Position, f32, [f32; 4])] = &[
                (
                    "📺 L3 Classic",
                    "Lower Third",
                    rook_core::transform::Position { x: 180.0, y: 820.0 },
                    0.6,
                    [1.0, 0.95, 0.8, 1.0],
                ),
                (
                    "📺 L3 Bold",
                    "Lower Third",
                    rook_core::transform::Position { x: 80.0, y: 780.0 },
                    0.75,
                    [1.0, 1.0, 1.0, 1.0],
                ),
                (
                    "📺 L3 Minimal",
                    "Lower Third",
                    rook_core::transform::Position { x: 60.0, y: 860.0 },
                    0.35,
                    [0.85, 0.9, 1.0, 0.9],
                ),
            ];
            for (btn_label, clip_label, pos, scale_x, color) in l3_presets {
                if ui
                    .button(*btn_label)
                    .on_hover_text(format!("Add {} at playhead", clip_label))
                    .clicked()
                {
                    if project.timeline.tracks_of_kind(TrackKind::Text).is_empty() {
                        project.add_text_track("Titles 1".to_string());
                    }
                    if let Some(track_id) = project
                        .timeline
                        .tracks
                        .iter()
                        .find(|t| t.kind == TrackKind::Text)
                        .map(|t| t.id)
                    {
                        let l3_clip = Clip {
                            id: ClipId::next(),
                            label: clip_label.to_string(),
                            asset_id: AssetId(0),
                            timeline_in: project.timeline.playhead,
                            source_in: 0,
                            source_duration: (project.timeline.frame_rate.as_f64() * 3.0) as i64,
                            transform: rook_core::transform::Transform {
                                position: *pos,
                                scale: rook_core::transform::Scale {
                                    x: *scale_x,
                                    y: 0.06,
                                },
                                anchor: rook_core::transform::AnchorPoint { x: 0.0, y: 1.0 },
                                ..Default::default()
                            },
                            blend_mode: rook_core::clip::BlendMode::Normal,
                            mask: None,
                            fade: Some(rook_core::clip::Fade {
                                in_frames: 8,
                                out_frames: 8,
                                curve: rook_core::clip::FadeCurve::Ease,
                            }),
                            transition: None,
                            speed: 1.0,
                            speed_curve: vec![],
                            reverse: false,
                            freeze_frame: None,
                            frame_blending: false,
                            spatial_conform: None,
                            gain_db: None,
                            volume_keyframes: None,
                            mute_audio: true,
                            filters: vec![],
                            keyframes: vec![],
                            link_group_id: None,
                            generator: Some(rook_core::clip::Generator::Text {
                                content: "Name • Title".to_string(),
                                font_size: 28.0,
                                color: *color,
                            }),
                        };
                        if let Some(track) = project.timeline.track_mut(track_id) {
                            track.insert_clip(l3_clip).ok();
                        }
                    }
                }
            }
            if ui
                .button("🎬 Credits")
                .on_hover_text("Add scrolling credits at playhead")
                .clicked()
            {
                if project.timeline.tracks_of_kind(TrackKind::Text).is_empty() {
                    project.add_text_track("Titles 1".to_string());
                }
                if let Some(track_id) = project
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == TrackKind::Text)
                    .map(|t| t.id)
                {
                    let credits_clip = Clip {
                        id: ClipId::next(),
                        label: "Credits".to_string(),
                        asset_id: AssetId(0),
                        timeline_in: project.timeline.playhead,
                        source_in: 0,
                        source_duration: (project.timeline.frame_rate.as_f64() * 10.0) as i64,
                        transform: rook_core::transform::Transform {
                            position: rook_core::transform::Position { x: 0.0, y: 0.0 },
                            scale: rook_core::transform::Scale { x: 1.0, y: 1.0 },
                            anchor: rook_core::transform::AnchorPoint { x: 0.5, y: 0.5 },
                            ..Default::default()
                        },
                        blend_mode: rook_core::clip::BlendMode::Normal,
                        mask: None,
                        fade: None,
                        transition: Some(rook_core::clip::Transition {
                            kind: rook_core::clip::TransitionKind::Dissolve,
                            duration_frames: 24,
                            reversed: false,
                            curve: rook_core::clip::FadeCurve::Linear,
                        }),
                        speed: 1.0,
                        speed_curve: vec![],
                        reverse: false,
                        freeze_frame: None,
                        frame_blending: false,
                        spatial_conform: None,
                        gain_db: None,
                        volume_keyframes: None,
                        mute_audio: true,
                        filters: vec![],
                        keyframes: vec![],
                        link_group_id: None,
                        generator: Some(rook_core::clip::Generator::Credits {
                            content:
                                "Directed by\n\nWritten by\n\nProduced by\n\nMusic by\n\nEditor\n"
                                    .to_string(),
                            font_size: 32.0,
                            color: [1.0, 0.95, 0.85, 1.0],
                            scroll_speed: 60.0,
                        }),
                    };
                    if let Some(track) = project.timeline.track_mut(track_id) {
                        track.insert_clip(credits_clip).ok();
                    }
                }
            }
            if ui
                .button("🎨 Solid")
                .on_hover_text("Add solid color clip at playhead")
                .clicked()
            {
                if project.timeline.tracks_of_kind(TrackKind::Video).is_empty() {
                    project.add_video_track("V1".to_string());
                }
                if let Some(track_id) = project
                    .timeline
                    .tracks
                    .iter()
                    .find(|t| t.kind == TrackKind::Video)
                    .map(|t| t.id)
                {
                    let solid_clip = Clip {
                        id: ClipId::next(),
                        label: "Color".to_string(),
                        asset_id: AssetId(0),
                        timeline_in: project.timeline.playhead,
                        source_in: 0,
                        source_duration: (project.timeline.frame_rate.as_f64() * 3.0) as i64, // 3 seconds
                        transform: rook_core::transform::Transform::default(),
                        blend_mode: rook_core::clip::BlendMode::Normal,
                        mask: None,
                        fade: None,
                        transition: None,
                        speed: 1.0,
                        speed_curve: vec![],
                        reverse: false,
                        freeze_frame: None,
                        frame_blending: false,
                        spatial_conform: None,
                        gain_db: None,
                        volume_keyframes: None,
                        mute_audio: true,
                        filters: vec![],
                        keyframes: vec![],
                        link_group_id: None,
                        generator: Some(rook_core::clip::Generator::Solid {
                            color: [0.2, 0.3, 0.5, 1.0],
                        }),
                    };
                    if let Some(track) = project.timeline.track_mut(track_id) {
                        track.insert_clip(solid_clip).ok();
                    }
                }
            }
            // ── Sync Audio button (when video + audio clip selected) ──
            let sel_count = project.timeline.selected_clip_ids.len();
            if sel_count == 2 {
                let sel_clips: Vec<ClipId> = project.timeline.selected_clip_ids.clone();
                let sel_tracks: Vec<(ClipId, TrackKind)> = sel_clips
                    .iter()
                    .filter_map(|&cid| {
                        project
                            .timeline
                            .clip_track_id(cid)
                            .and_then(|tid| project.timeline.track(tid))
                            .map(|t| (cid, t.kind))
                    })
                    .collect();
                let has_video = sel_tracks.iter().any(|(_, k)| *k == TrackKind::Video);
                let has_audio = sel_tracks.iter().any(|(_, k)| *k == TrackKind::Audio);
                if has_video && has_audio {
                    if ui
                        .button("🔗 Sync Audio")
                        .on_hover_text("Cross-correlate waveforms and align audio to video")
                        .clicked()
                    {
                        let video_cid = sel_tracks
                            .iter()
                            .find(|(_, k)| *k == TrackKind::Video)
                            .map(|(c, _)| *c);
                        let audio_cid = sel_tracks
                            .iter()
                            .find(|(_, k)| *k == TrackKind::Audio)
                            .map(|(c, _)| *c);
                        if let (Some(vid), Some(aid)) = (video_cid, audio_cid) {
                            self.sync_audio_clips(project, vid, aid);
                        }
                    }
                    ui.separator();
                }
            }

            // Status bar info
            let duration = project.timeline.duration();
            let fps = project.frame_rate.as_f64();
            ui.label(format!(
                "Dur: {:.1}s | {} fps | {} clips | {} markers",
                duration as f64 / fps,
                fps as i64,
                project
                    .timeline
                    .tracks
                    .iter()
                    .map(|t| t.clips.len())
                    .sum::<usize>(),
                project.timeline.markers.len(),
            ));
            ui.separator();
            // Trim window toggle
            if ui
                .selectable_label(self.show_trim_window, "⏩ Trim Window")
                .clicked()
            {
                self.show_trim_window = !self.show_trim_window;
            }
        });

        // ── Trim Edit Window ────────────────────────────────────────────
        if self.show_trim_window {
            let selected_clip = project.timeline.selected_clip_ids.first().copied();
            if let Some(cid) = selected_clip {
                if let Some(clip) = project.timeline.clip(cid) {
                    let clip_info = (
                        clip.timeline_in,
                        clip.source_in,
                        clip.source_duration,
                        clip.duration(),
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("⏩ Trim Editor");
                        ui.separator();
                        let (tl_in, src_in, src_dur, dur) = clip_info;
                        let mut new_src_in = src_in;
                        let mut new_tl_in = tl_in;
                        let mut new_dur_frames = src_dur;

                        ui.label("Source In:");
                        if ui
                            .add(egui::DragValue::new(&mut new_src_in).speed(1.0))
                            .changed()
                        {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                let delta = new_src_in - c.source_in;
                                c.source_in = new_src_in;
                                c.source_duration = (c.source_duration - delta).max(1);
                            }
                        }
                        ui.separator();
                        ui.label("Duration:");
                        if ui
                            .add(egui::DragValue::new(&mut new_dur_frames).speed(1.0))
                            .changed()
                        {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.source_duration = new_dur_frames.max(1);
                            }
                        }
                        ui.separator();
                        ui.label("Timeline In:");
                        if ui
                            .add(egui::DragValue::new(&mut new_tl_in).speed(1.0))
                            .changed()
                        {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.timeline_in = new_tl_in.max(0);
                            }
                        }
                        ui.separator();
                        // JKL nudge buttons
                        if ui.button("◁◁").on_hover_text("Nudge -10 frames").clicked() {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.timeline_in = (c.timeline_in - 10).max(0);
                            }
                        }
                        if ui.button("◁").on_hover_text("Nudge -1 frame").clicked() {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.timeline_in = (c.timeline_in - 1).max(0);
                            }
                        }
                        let dur_frames = project
                            .timeline
                            .clip(cid)
                            .map(|c| c.duration())
                            .unwrap_or(1);
                        ui.label(format!(
                            "End: {}f ({:.1}s)",
                            tl_in + dur_frames,
                            (tl_in + dur_frames) as f64 / fps
                        ));
                        if ui.button("▷").on_hover_text("Nudge +1 frame").clicked() {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.timeline_in += 1;
                            }
                        }
                        if ui.button("▷▷").on_hover_text("Nudge +10 frames").clicked() {
                            if let Some(c) = project.timeline.clip_mut(cid) {
                                c.timeline_in += 10;
                            }
                        }
                    });
                }
            } else {
                ui.label(
                    egui::RichText::new("Select a clip to trim")
                        .size(11.0)
                        .color(egui::Color32::from_gray(140)),
                );
            }
        }

        // Sync playhead back
        *playhead = project.timeline.playhead;
    }

    // ── Input handling ──────────────────────────────────────────────────

    fn handle_keys(&mut self, ui: &egui::Ui, project: &mut Project, playhead: &mut i64) {
        let input = ui.input(|i| i.clone());
        let fps = project.timeline.frame_rate.as_f64();

        // ── Tool shortcuts ─────────────────────────────────────────────
        if input.key_pressed(egui::Key::A) && !input.modifiers.command {
            self.active_tool = Tool::Select;
        }
        if input.key_pressed(egui::Key::B) && !input.modifiers.command {
            self.active_tool = Tool::Blade;
        }
        if input.key_pressed(egui::Key::T) && !input.modifiers.command {
            self.active_tool = Tool::Trim;
        }
        if input.key_pressed(egui::Key::R) && !input.modifiers.command {
            self.active_tool = Tool::RangeSelect;
        }
        if input.key_pressed(egui::Key::Z) && !input.modifiers.command && !input.modifiers.ctrl {
            self.active_tool = Tool::Zoom;
        }
        if input.key_pressed(egui::Key::H) && !input.modifiers.command {
            self.active_tool = Tool::Hand;
        }
        if input.key_pressed(egui::Key::P) && !input.modifiers.command {
            self.active_tool = Tool::Position;
        }
        // N = toggle snapping
        if input.key_pressed(egui::Key::N) && !input.modifiers.command {
            self.snapping = !self.snapping;
        }
        // Esc = back to Select
        if input.key_pressed(egui::Key::Escape) {
            self.active_tool = Tool::Select;
        }

        // ── Editing ────────────────────────────────────────────────────

        if input.key_pressed(egui::Key::Space) {
            // Play/pause — handled by app
        }
        if input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace) {
            let shift = input.modifiers.shift;
            let to_delete: Vec<ClipId> = project.timeline.selected_clip_ids.drain(..).collect();
            for cid in to_delete {
                if let Some(tid) = project.timeline.clip_track_id(cid) {
                    if let Some(track) = project.timeline.track_mut(tid) {
                        if shift {
                            // Ripple delete: remove clip and close the gap
                            if let Some(removed) = track.remove_clip(cid) {
                                let gap = removed.duration();
                                for clip in &mut track.clips {
                                    if clip.timeline_in > removed.timeline_in {
                                        clip.timeline_in -= gap;
                                    }
                                }
                            }
                        } else {
                            // Normal delete (lift): remove clip, leave gap
                            track.remove_clip(cid);
                        }
                    }
                }
            }
        }
        // I / O — set in/out point at playhead
        if input.key_pressed(egui::Key::I) && !input.modifiers.command && !input.modifiers.alt {
            project.timeline.in_point = Some(*playhead);
        }
        if input.key_pressed(egui::Key::O) && !input.modifiers.command && !input.modifiers.alt {
            project.timeline.out_point = Some(*playhead);
        }
        // Option+I / Option+O — clear in/out point
        if input.key_pressed(egui::Key::I) && input.modifiers.alt {
            project.timeline.in_point = None;
        }
        if input.key_pressed(egui::Key::O) && input.modifiers.alt {
            project.timeline.out_point = None;
        }
        // J/K/L shuttle (only when no modifiers held)
        if input.key_down(egui::Key::J) && !input.modifiers.command && !input.modifiers.ctrl {
            *playhead = (*playhead - (fps * 2.0) as i64).max(0);
            project.timeline.playhead = *playhead;
        }
        if input.key_down(egui::Key::K) && !input.modifiers.command && !input.modifiers.ctrl {
            // Stop (handled by app)
        }
        if input.key_down(egui::Key::L) && !input.modifiers.command && !input.modifiers.ctrl {
            let dur = last_valid_timeline_frame(project.timeline.duration());
            *playhead = (*playhead + (fps * 2.0) as i64).min(dur);
            project.timeline.playhead = *playhead;
        }
        // Arrow keys: frame step / 10-frame jump with Shift
        // (skipped in Position mode — arrow keys nudge the clip transform instead)
        if self.active_tool != Tool::Position {
            if input.key_pressed(egui::Key::ArrowLeft) {
                let step = if input.modifiers.shift { 10 } else { 1 };
                *playhead = (*playhead - step).max(0);
                project.timeline.playhead = *playhead;
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                let step = if input.modifiers.shift { 10 } else { 1 };
                let dur = last_valid_timeline_frame(project.timeline.duration());
                *playhead = (*playhead + step).min(dur);
                project.timeline.playhead = *playhead;
            }
            // Up/Down: jump to prev/next edit point
            if input.key_pressed(egui::Key::ArrowUp) {
                let mut candidates: Vec<i64> = vec![0];
                for track in &project.timeline.tracks {
                    for clip in &track.clips {
                        if clip.timeline_in < *playhead {
                            candidates.push(clip.timeline_in);
                        }
                        let out = clip.timeline_in + clip.duration();
                        if out < *playhead {
                            candidates.push(out);
                        }
                    }
                }
                candidates.sort();
                candidates.dedup();
                *playhead = candidates.last().copied().unwrap_or(0);
                project.timeline.playhead = *playhead;
            }
            if input.key_pressed(egui::Key::ArrowDown) {
                let mut candidates: Vec<i64> = Vec::new();
                for track in &project.timeline.tracks {
                    for clip in &track.clips {
                        if clip.timeline_in > *playhead {
                            candidates.push(clip.timeline_in);
                        }
                        let out = clip.timeline_in + clip.duration();
                        if out > *playhead {
                            candidates.push(out);
                        }
                    }
                }
                candidates.sort();
                let dur = last_valid_timeline_frame(project.timeline.duration());
                *playhead = candidates.first().copied().unwrap_or(dur);
                project.timeline.playhead = *playhead;
            }
        }
        // Home/End
        if input.key_pressed(egui::Key::Home) {
            *playhead = 0;
            project.timeline.playhead = *playhead;
        }
        if input.key_pressed(egui::Key::End) {
            *playhead = last_valid_timeline_frame(project.timeline.duration());
            project.timeline.playhead = *playhead;
        }
        // S = solo selected clip's track (when no modifiers)
        // If S key pressed alone (not Shift+Cmd+B for blade-all, not Cmd+S for save)
        if input.key_pressed(egui::Key::S)
            && !input.modifiers.command
            && !input.modifiers.ctrl
            && !input.modifiers.shift
            && self.active_tool != Tool::Blade
        {
            // Already used by S=split above. S-solo only works when no clip selected.
            if project.timeline.selected_clip_ids.is_empty() {
                // Toggle solo on first audio track
                if let Some(track) = project
                    .timeline
                    .tracks
                    .iter_mut()
                    .find(|t| t.kind == TrackKind::Audio)
                {
                    track.solo = !track.solo;
                }
            }
        }
        // M = add/remove marker at playhead
        if input.key_pressed(egui::Key::M) && !input.modifiers.command && !input.modifiers.ctrl {
            // Toggle marker at current playhead position
            let frame = *playhead;
            if let Some(existing) = project
                .timeline
                .markers
                .iter()
                .position(|m| m.frame == frame)
            {
                project.timeline.markers.remove(existing);
            } else {
                let marker = Marker::new(
                    format!("Marker {}", project.timeline.markers.len() + 1),
                    frame,
                );
                project.timeline.markers.push(marker);
            }
        }
        // Cmd+M = add named marker
        if input.key_pressed(egui::Key::M) && (input.modifiers.command || input.modifiers.ctrl) {
            let frame = *playhead;
            let marker = Marker::new(
                format!(
                    "M {:02}:{:02.0}",
                    (*playhead as f64 / fps / 60.0) as i64,
                    *playhead as f64 / fps % 60.0
                ),
                frame,
            );
            project.timeline.markers.push(marker);
        }
        // Cmd+Backspace = delete selected track
        if input.key_pressed(egui::Key::Backspace)
            && (input.modifiers.command || input.modifiers.ctrl)
        {
            // Find the track of the first selected clip
            if let Some(cid) = project.timeline.selected_clip_ids.first().copied() {
                if let Some(tid) = project.timeline.clip_track_id(cid) {
                    // Only delete if track has no other clips (or confirm)
                    if let Some(track) = project.timeline.track(tid) {
                        if track.clips.len() <= 1 {
                            project.timeline.remove_track(tid);
                        }
                    }
                }
            }
        }
        // Cmd+C = copy clip attributes
        if input.key_pressed(egui::Key::C)
            && (input.modifiers.command || input.modifiers.ctrl)
            && !input.modifiers.shift
        {
            if let Some(cid) = project.timeline.selected_clip_ids.first().copied() {
                if let Some(clip) = project.timeline.clip(cid) {
                    self.clipboard = Some(ClipAttributes {
                        transform: clip.transform.clone(),
                        blend_mode: clip.blend_mode,
                        opacity: clip.transform.opacity,
                        speed: clip.speed,
                        fade: clip.fade,
                        gain_db: clip.gain_db,
                        mute_audio: clip.mute_audio,
                    });
                }
            }
        }
        // Cmd+G = group selected clips (shared link_group_id)
        if input.key_pressed(egui::Key::G)
            && (input.modifiers.command || input.modifiers.ctrl)
            && !input.modifiers.shift
        {
            let group_id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;
            for cid in &project.timeline.selected_clip_ids.clone() {
                if let Some(clip) = project.timeline.clip_mut(*cid) {
                    clip.link_group_id = Some(group_id);
                }
            }
        }
        // Cmd+Shift+G = ungroup selected clips
        if input.key_pressed(egui::Key::G)
            && (input.modifiers.command || input.modifiers.ctrl)
            && input.modifiers.shift
        {
            for cid in &project.timeline.selected_clip_ids.clone() {
                if let Some(clip) = project.timeline.clip_mut(*cid) {
                    clip.link_group_id = None;
                }
            }
        }

        // Cmd+Opt+G = create compound clip
        if input.key_pressed(egui::Key::G)
            && (input.modifiers.command || input.modifiers.ctrl)
            && input.modifiers.alt
            && !input.modifiers.shift
        {
            self.create_compound_clip(project);
        }

        // Cmd+Opt+Shift+G = break apart compound clip
        if input.key_pressed(egui::Key::G)
            && (input.modifiers.command || input.modifiers.ctrl)
            && input.modifiers.alt
            && input.modifiers.shift
        {
            if let Some(&cid) = project.timeline.selected_clip_ids.first() {
                self.break_apart_compound(project, cid);
            }
        }

        // Enter = enter selected compound clip
        if input.key_pressed(egui::Key::Enter) && !input.modifiers.command && !input.modifiers.ctrl
        {
            if let Some(&cid) = project.timeline.selected_clip_ids.first() {
                if self.is_compound(&project.timeline, cid) {
                    self.enter_compound(project, cid);
                }
            }
        }

        // Backspace at top level (with no selected clips) when inside compound = exit
        // (regular Backspace with selection = delete, already handled above)
        if input.key_pressed(egui::Key::Backspace)
            && project.timeline.selected_clip_ids.is_empty()
            && self.inside_compound()
            && !input.modifiers.command
            && !input.modifiers.ctrl
        {
            self.exit_compound(project);
        }

        // Cmd+T = quick add transition to selected clip
        if input.key_pressed(egui::Key::T)
            && (input.modifiers.command || input.modifiers.ctrl)
            && !input.modifiers.shift
        {
            for &cid in &project.timeline.selected_clip_ids.clone() {
                if let Some(clip) = project.timeline.clip_mut(cid) {
                    clip.transition = Some(rook_core::clip::Transition {
                        kind: rook_core::clip::TransitionKind::CrossDissolve,
                        duration_frames: 24,
                        reversed: false,
                        curve: rook_core::clip::FadeCurve::Linear,
                    });
                }
            }
        }

        // Option+Left/Right = nudge selected clip position by 1 frame
        if input.key_pressed(egui::Key::ArrowLeft)
            && input.modifiers.alt
            && self.active_tool != Tool::Position
        {
            for &cid in &project.timeline.selected_clip_ids.clone() {
                if let Some(clip) = project.timeline.clip_mut(cid) {
                    clip.timeline_in = (clip.timeline_in - 1).max(0);
                }
            }
        }
        if input.key_pressed(egui::Key::ArrowRight)
            && input.modifiers.alt
            && self.active_tool != Tool::Position
        {
            for &cid in &project.timeline.selected_clip_ids.clone() {
                if let Some(clip) = project.timeline.clip_mut(cid) {
                    clip.timeline_in += 1;
                }
            }
        }

        // Cmd+Shift+V = paste clip attributes
        if input.key_pressed(egui::Key::V)
            && (input.modifiers.command || input.modifiers.ctrl)
            && input.modifiers.shift
        {
            if let Some(ref attrs) = self.clipboard.clone() {
                for cid in &project.timeline.selected_clip_ids.clone() {
                    if let Some(clip) = project.timeline.clip_mut(*cid) {
                        clip.transform = attrs.transform.clone();
                        clip.blend_mode = attrs.blend_mode;
                        clip.transform.opacity = attrs.opacity;
                        clip.speed = attrs.speed;
                        clip.fade = attrs.fade;
                        clip.gain_db = attrs.gain_db;
                        clip.mute_audio = attrs.mute_audio;
                    }
                }
            }
        }

        // Shift+[ = select all clips between I/O marks
        if input.key_pressed(egui::Key::OpenBracket)
            && input.modifiers.shift
            && !input.modifiers.alt
        {
            if let (Some(in_pt), Some(out_pt)) =
                (project.timeline.in_point, project.timeline.out_point)
            {
                let (start, end) = if in_pt <= out_pt {
                    (in_pt, out_pt)
                } else {
                    (out_pt, in_pt)
                };
                project.timeline.selected_clip_ids.clear();
                for track in &project.timeline.tracks {
                    for clip in &track.clips {
                        let clip_end = clip.timeline_in + clip.duration();
                        if clip.timeline_in < end && clip_end > start {
                            project.timeline.selected_clip_ids.push(clip.id);
                        }
                    }
                }
            }
        }
        // S = split at playhead
        if input.key_pressed(egui::Key::S) && !input.modifiers.ctrl && !input.modifiers.command {
            if let Some(cid) = project.timeline.selected_clip_ids.first().copied() {
                self.split_clip_at(project, cid, *playhead);
            }
        }
        // Shift+Cmd+B = blade all tracks at playhead
        if input.key_pressed(egui::Key::B) && input.modifiers.command && input.modifiers.shift {
            self.blade_all_tracks(project, *playhead);
        }
        // Option+[ / Option+] — trim start/end of selected clip to playhead
        if input.key_pressed(egui::Key::OpenBracket) && input.modifiers.alt {
            self.trim_selected_start_to(project, *playhead);
        }
        if input.key_pressed(egui::Key::CloseBracket) && input.modifiers.alt {
            self.trim_selected_end_to(project, *playhead);
        }
        // Option+\ — trim selected clip to I/O mark range
        if input.key_pressed(egui::Key::Backslash) && input.modifiers.alt {
            self.trim_selected_to_io_range(project);
        }
        // ⌘R — show retime editor (toggle speed ramp on selected clip)
        if input.key_pressed(egui::Key::R)
            && (input.modifiers.command || input.modifiers.ctrl)
            && !input.modifiers.shift
        {
            if let Some(cid) = project.timeline.selected_clip_ids.first().copied() {
                if let Some(clip) = project.timeline.clip_mut(cid) {
                    if clip.speed_curve.is_empty() {
                        // Add default speed ramp
                        let dur = clip.duration().max(1);
                        clip.speed_curve.push(rook_core::clip::SpeedCurvePoint {
                            frame: 0,
                            speed: clip.speed,
                        });
                        clip.speed_curve.push(rook_core::clip::SpeedCurvePoint {
                            frame: dur,
                            speed: clip.speed,
                        });
                        clip.speed_curve.sort_by_key(|p| p.frame);
                    } else {
                        // Clear speed ramp
                        clip.speed_curve.clear();
                    }
                }
            }
        }

        // Ctrl+Z / Ctrl+Shift+Z for undo/redo
        if input.key_pressed(egui::Key::Z) && (input.modifiers.ctrl || input.modifiers.command) {
            if input.modifiers.shift {
                // Redo — handled by app
            } else {
                // Undo — handled by app
            }
        }
    }

    fn handle_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        project: &mut Project,
        clips: &[ClipGeom],
        playhead: &mut i64,
        clip_rect: egui::Rect,
    ) {
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let ctrl = ui.input(|i| i.modifiers.ctrl);

        // Ctrl+Scroll = zoom
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if ctrl && scroll.y != 0.0 {
            self.zoom *= 1.0 + scroll.y * 0.005;
            self.zoom = self.zoom.clamp(MIN_PX_PER_FRAME, MAX_PX_PER_FRAME);
        }

        // ── Hand tool: start pan ──────────────────────────────────────────
        if response.clicked() && self.active_tool == Tool::Hand {
            // Save current scroll position as anchor for the drag
            let cur_sx = ui.clip_rect().min.x - clip_rect.min.x;
            let cur_sy = ui.clip_rect().min.y - clip_rect.min.y;
            self.hand_drag_anchor = Some((cur_sx, cur_sy));
            return;
        }

        // ── Hand tool: drag to pan ────────────────────────────────────────
        if let Some((anchor_sx, anchor_sy)) = self.hand_drag_anchor {
            let still_hand = self.active_tool == Tool::Hand;
            let still_dragging = response.dragged_by(egui::PointerButton::Primary);
            if still_hand && still_dragging {
                let delta = response.drag_delta();
                self.scroll_x = anchor_sx - delta.x;
                self.scroll_y = (anchor_sy - delta.y).max(0.0);
                return; // suppress normal click/drag while actively panning
            }
            // Drag ended or tool switched — clear anchor
            self.hand_drag_anchor = None;
            // If the drag just ended, suppress the release from triggering
            // a click/seek on the canvas below.
            if still_hand {
                return;
            }
            // Otherwise (tool switched mid-drag), fall through to normal handling
        }

        // Right-click selects clip and shows context actions via keyboard
        if response.secondary_clicked() {
            if let Some(pos) = pointer {
                // Check if clicking track header first
                let track_idx = self.pixel_to_track_idx(pos.y - clip_rect.top(), project);
                if let Some(ti) = track_idx {
                    if ti < project.timeline.tracks.len()
                        && pos.x < clip_rect.left() + TRACK_HEADER_W
                    {
                        let tid = project.timeline.tracks[ti].id;
                        self.clip_context_popup = None; // clear any clip popup
                        self.track_context_popup = Some((tid, pos));
                    } else {
                        self.track_context_popup = None;
                        let mut hit_clip: Option<ClipGeom> = None;
                        for cg in clips.iter().rev() {
                            let y = self.track_y(cg.track_id, project, clip_rect.top());
                            let cr = egui::Rect::from_min_size(
                                egui::pos2(cg.x, y + 3.0),
                                egui::vec2(cg.w.max(12.0), self.track_h - 6.0),
                            );
                            if cr.contains(pos) {
                                hit_clip = Some(cg.clone());
                                break;
                            }
                        }
                        if let Some(cg) = hit_clip {
                            project.timeline.select(cg.clip_id);
                            self.clip_context_popup = Some((cg.clip_id, pos));
                        } else {
                            self.clip_context_popup = None;
                        }
                    }
                } else {
                    self.track_context_popup = None;
                    let mut hit_clip: Option<ClipGeom> = None;
                    for cg in clips.iter().rev() {
                        let y = self.track_y(cg.track_id, project, clip_rect.top());
                        let cr = egui::Rect::from_min_size(
                            egui::pos2(cg.x, y + 3.0),
                            egui::vec2(cg.w.max(12.0), self.track_h - 6.0),
                        );
                        if cr.contains(pos) {
                            hit_clip = Some(cg.clone());
                            break;
                        }
                    }
                    if let Some(cg) = hit_clip {
                        project.timeline.select(cg.clip_id);
                        // Show a small popup for context actions
                        self.clip_context_popup = Some((cg.clip_id, pos));
                    } else {
                        self.clip_context_popup = None;
                    }
                }
            }
        }

        // Mouse press on canvas
        if response.clicked() {
            if let Some(pos) = pointer {
                let frame = self.pixel_to_frame(pos.x - clip_rect.left());

                // ── Marker interaction in ruler area ────────────────────
                if pos.y < clip_rect.top() + RULER_H {
                    let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);

                    // ── Timeline index bar click-to-jump ───────────────
                    if let Some(idx_rect) = self.timeline_index_rect {
                        if idx_rect.contains(pos) {
                            // Compute which frame the click corresponds to
                            let click_x = pos.x - clip_rect.left();
                            let total_dur = last_valid_timeline_frame(project.timeline.duration());
                            let target = ((click_x / self.zoom) as i64).clamp(0, total_dur);
                            *playhead = target;
                            project.timeline.playhead = target;
                            return;
                        }
                    }

                    let mut hit_marker: Option<usize> = None;
                    for (i, marker) in project.timeline.markers.iter().enumerate() {
                        let mx = clip_rect.left() + marker.frame as f32 * self.zoom;
                        if (pos.x - mx).abs() < 8.0 {
                            hit_marker = Some(i);
                            break;
                        }
                    }
                    if let Some(idx) = hit_marker {
                        if cmd {
                            project.timeline.markers.remove(idx);
                        } else {
                            let marker_frame = project.timeline.markers[idx].frame;
                            *playhead = marker_frame;
                            project.timeline.playhead = marker_frame;
                        }
                        return;
                    }
                    // Click on ruler → seek
                    *playhead =
                        frame.clamp(0, last_valid_timeline_frame(project.timeline.duration()));
                    project.timeline.playhead = *playhead;
                    return;
                }

                // ── Track header interaction ────────────────────────────
                let track_idx_opt = self.pixel_to_track_idx(pos.y - clip_rect.top(), project);
                if let Some(track_idx) = track_idx_opt {
                    if track_idx < project.timeline.tracks.len()
                        && pos.x < clip_rect.left() + TRACK_HEADER_W
                    {
                        let track_y = self.visual_track_ys
                            .get(&project.timeline.tracks[track_idx].id)
                            .copied()
                            .unwrap_or(clip_rect.top() + RULER_H);
                        // Start a track drag — will be handled as click if no movement
                        self.track_drag = Some(TrackDragState {
                            track_id: project.timeline.tracks[track_idx].id,
                            orig_index: track_idx,
                            offset_y: pos.y - track_y,
                        });
                        return;
                    }
                }

                // ── Clip hit-test ───────────────────────────────────────

                // Check if we clicked a clip
                let mut hit_clip = None;
                for cg in clips.iter().rev() {
                    let y = self.track_y(cg.track_id, project, clip_rect.top());
                    let cr = egui::Rect::from_min_size(
                        egui::pos2(cg.x, y + 3.0),
                        egui::vec2(cg.w.max(2.0), self.track_h - 6.0),
                    );
                    if cr.contains(pos) {
                        hit_clip = Some(cg.clone());
                        break;
                    }
                }

                if let Some(cg) = hit_clip {
                    // ── Alt+click: add audio gain keyframe point ──────────
                    let alt_held = ui.input(|i| i.modifiers.alt);
                    if alt_held && cg.track_kind == TrackKind::Audio {
                        // Compute click geometry first (avoid borrowing project while
                        // we still need it for track_y).
                        let track_y = self.track_y(cg.track_id, project, clip_rect.top());
                        let cr = egui::Rect::from_min_size(
                            egui::pos2(cg.x, track_y + 3.0),
                            egui::vec2(cg.w.max(2.0), self.track_h - 6.0),
                        );
                        let mid_y = cr.center().y;
                        let gain_y = mid_y
                            - (cg.audio_gain_db as f32 / 24.0f32).clamp(-1.0, 1.0)
                                * cr.height()
                                * 0.35;
                        let local_frame = self.pixel_to_frame(pos.x - clip_rect.left());

                        if (pos.y - gain_y).abs() < 12.0 && local_frame >= 0 {
                            let db = ((mid_y - pos.y) / (cr.height() * 0.35) * 24.0)
                                .clamp(-96.0, 24.0) as f64;
                            // Now mutate the clip model
                            if let Some(clip) = project.timeline.clip_mut(cg.clip_id) {
                                let source_offset = local_frame - clip.source_in;
                                clip.volume_keyframes
                                    .get_or_insert_with(Vec::new)
                                    .push((source_offset, db));
                                clip.volume_keyframes
                                    .as_mut()
                                    .map(|kf| kf.sort_by_key(|(f, _)| *f));
                            }
                        }
                        return;
                    }

                    // ── Double-click: enter compound clip ────────────────
                    if response.double_clicked() && self.is_compound(&project.timeline, cg.clip_id)
                    {
                        self.enter_compound(project, cg.clip_id);
                        return;
                    }

                    // ── Blade tool: split clip at click point ─────────────
                    if self.active_tool == Tool::Blade {
                        let click_frame = self.pixel_to_frame(pos.x - clip_rect.left());
                        if project
                            .timeline
                            .clip(cg.clip_id)
                            .map(|c| c.covers(click_frame))
                            .unwrap_or(false)
                        {
                            self.split_clip_at(project, cg.clip_id, click_frame);
                            *playhead = click_frame;
                            project.timeline.playhead = *playhead;
                        }
                        return;
                    }

                    // ── Zoom tool: zoom in/out at click ───────────────────
                    if self.active_tool == Tool::Zoom {
                        let opt = ui.input(|i| i.modifiers.alt);
                        if opt {
                            self.zoom = (self.zoom * 0.5).max(MIN_PX_PER_FRAME);
                        } else {
                            self.zoom = (self.zoom * 2.0).min(MAX_PX_PER_FRAME);
                        }
                        return;
                    }

                    // ── Range Select on clip: select it ──────────────────
                    if self.active_tool == Tool::RangeSelect {
                        if !ui.input(|i| i.modifiers.shift) {
                            project.timeline.select(cg.clip_id);
                        } else {
                            project.timeline.toggle_select(cg.clip_id);
                        }
                        *playhead = self.pixel_to_frame(cg.x - clip_rect.left());
                        project.timeline.playhead = *playhead;
                        return;
                    }

                    // Check trim handle zones
                    let y = self.track_y(cg.track_id, project, clip_rect.top());
                    let cr = egui::Rect::from_min_size(
                        egui::pos2(cg.x, y + 3.0),
                        egui::vec2(cg.w.max(12.0), self.track_h - 6.0),
                    );
                    let in_left_handle = (pos.x - cr.left()).abs() < TRIM_HANDLE_W + 4.0;
                    let in_right_handle = (cr.right() - pos.x).abs() < TRIM_HANDLE_W + 4.0;

                    let shift = ui.input(|i| i.modifiers.shift);
                    if in_left_handle && cg.w > 12.0 {
                        // Begin left trim — check for adjacent clip (roll trim)
                        if let Some(clip) = project.timeline.clip(cg.clip_id) {
                            let (roll_id, roll_in, roll_dur) = if !shift {
                                self.find_adjacent_clip(
                                    project,
                                    cg.clip_id,
                                    cg.track_id,
                                    clip.timeline_in,
                                    true,
                                )
                            } else {
                                (None, None, None) // ripple trim: don't roll
                            };
                            self.trim_state = Some(TrimState {
                                clip_id: cg.clip_id,
                                edge: TrimEdge::Left,
                                orig_in: clip.source_in,
                                orig_out: clip.source_in + clip.source_duration,
                                orig_dur: clip.source_duration,
                                orig_timeline_in: clip.timeline_in,
                                roll_clip_id: roll_id,
                                roll_orig_in: roll_in,
                                roll_orig_dur: roll_dur,
                                ripple: shift,
                            });
                        }
                    } else if in_right_handle && cg.w > 12.0 {
                        // Begin right trim — check for adjacent clip (roll trim)
                        if let Some(clip) = project.timeline.clip(cg.clip_id) {
                            let right_edge = clip.timeline_in + clip.duration();
                            let (roll_id, roll_in, roll_dur) = if !shift {
                                self.find_adjacent_clip(
                                    project,
                                    cg.clip_id,
                                    cg.track_id,
                                    right_edge,
                                    false,
                                )
                            } else {
                                (None, None, None) // ripple trim: don't roll
                            };
                            self.trim_state = Some(TrimState {
                                clip_id: cg.clip_id,
                                edge: TrimEdge::Right,
                                orig_in: clip.source_in,
                                orig_out: clip.source_in + clip.source_duration,
                                orig_dur: clip.source_duration,
                                orig_timeline_in: clip.timeline_in,
                                roll_clip_id: roll_id,
                                roll_orig_in: roll_in,
                                roll_orig_dur: roll_dur,
                                ripple: shift,
                            });
                        }
                    } else if ui.input(|i| i.modifiers.alt) && cg.w > 12.0 {
                        // ── Option+click on audio gain line → add volume keyframe ──
                        if cg.track_kind == TrackKind::Audio {
                            let y = self.track_y(cg.track_id, project, clip_rect.top());
                            let cr = egui::Rect::from_min_size(
                                egui::pos2(cg.x, y + 3.0),
                                egui::vec2(cg.w.max(12.0), self.track_h - 6.0),
                            );
                            let mid_y = cr.center().y;
                            let db_norm = (cg.audio_gain_db / 24.0).clamp(-1.0, 1.0);
                            let gain_y = mid_y - db_norm * cr.height() * 0.35;
                            let hit_radius = 8.0; // generous hit area for the gain line
                            if (pos.y - gain_y).abs() < hit_radius {
                                // Compute local frame and gain value from click position
                                let local_frame = ((pos.x - cg.x) / self.zoom) as i64;
                                let click_db_norm = (mid_y - pos.y) / (cr.height() * 0.35);
                                let click_gain_db = (click_db_norm * 24.0).clamp(-96.0, 24.0);
                                if let Some(clip) = project.timeline.clip_mut(cg.clip_id) {
                                    let dur = clip.duration().max(1);
                                    let local = local_frame.clamp(0, dur);
                                    let kf = rook_core::keyframe::Keyframe::new(
                                        local,
                                        rook_core::keyframe::KeyframeProperty::Volume,
                                        click_gain_db as f64,
                                    );
                                    clip.keyframes.retain(|k| {
                                        !(k.property
                                            == rook_core::keyframe::KeyframeProperty::Volume
                                            && k.at_frame == local)
                                    });
                                    clip.keyframes.push(kf);
                                    clip.keyframes.sort_by_key(|k| k.at_frame);
                                }
                                project.timeline.select(cg.clip_id);
                                return;
                            }
                        }
                        // Option+click inside clip → slip trim
                        if let Some(clip) = project.timeline.clip(cg.clip_id) {
                            self.slip_state = Some(SlipState {
                                clip_id: cg.clip_id,
                                orig_source_in: clip.source_in,
                                orig_source_duration: clip.source_duration,
                                drag_start_frame: frame,
                            });
                        }
                        project.timeline.select(cg.clip_id);
                    } else if ui.input(|i| i.modifiers.command || ui.input(|i| i.modifiers.ctrl))
                        && cg.w > 12.0
                    {
                        // Cmd+click inside clip → slide trim
                        if let Some(clip) = project.timeline.clip(cg.clip_id) {
                            let (left_id, _, left_dur) = self.find_adjacent_clip(
                                project,
                                cg.clip_id,
                                cg.track_id,
                                clip.timeline_in,
                                true,
                            );
                            let right_edge = clip.timeline_in + clip.duration();
                            let (right_id, right_in, right_dur) = self.find_adjacent_clip(
                                project,
                                cg.clip_id,
                                cg.track_id,
                                right_edge,
                                false,
                            );
                            self.slide_state = Some(SlideState {
                                clip_id: cg.clip_id,
                                orig_timeline_in: clip.timeline_in,
                                left_id,
                                left_dur,
                                right_id,
                                right_timeline_in: right_id
                                    .map(|_| clip.timeline_in + clip.duration()),
                                right_source_in: right_in,
                                right_dur,
                            });
                        }
                        project.timeline.select(cg.clip_id);
                    } else if !cg.speed_curve_points.is_empty() && cg.w > 20.0 {
                        // Check if clicking near a speed ramp diamond
                        let diamond_y = cr.top() + 6.0;
                        let mut hit_speed_point = None;
                        for (i, &(frac, _)) in cg.speed_curve_points.iter().enumerate() {
                            let sx = cr.left() + frac * cg.w;
                            if (pos.x - sx).abs() < 10.0 && (pos.y - diamond_y).abs() < 12.0 {
                                hit_speed_point = Some(i);
                                break;
                            }
                        }
                        if let Some(point_idx) = hit_speed_point {
                            // Begin speed ramp point drag
                            if let Some(clip) = project.timeline.clip(cg.clip_id) {
                                if point_idx < clip.speed_curve.len() {
                                    self.speed_ramp_drag = Some(SpeedRampDragState {
                                        clip_id: cg.clip_id,
                                        point_index: point_idx,
                                        orig_frame: clip.speed_curve[point_idx].frame,
                                    });
                                    project.timeline.select(cg.clip_id);
                                }
                            }
                        } else {
                            // Begin normal drag
                            self.drag_clip = Some(DragState {
                                clip_id: cg.clip_id,
                                orig_track: cg.track_id,
                                orig_pos: self.pixel_to_frame(cg.x - clip_rect.left()),
                                offset_x: pos.x - cg.x,
                            });
                            let link_group = project
                                .timeline
                                .clip(cg.clip_id)
                                .and_then(|c| c.link_group_id);
                            let shift_held = ui.input(|i| i.modifiers.shift);
                            if let Some(lg) = link_group {
                                if !shift_held {
                                    project.timeline.clear_selection();
                                    for track in &project.timeline.tracks {
                                        for clip in &track.clips {
                                            if clip.link_group_id == Some(lg) {
                                                project.timeline.selected_clip_ids.push(clip.id);
                                            }
                                        }
                                    }
                                }
                            } else if !shift_held {
                                project.timeline.select(cg.clip_id);
                            } else {
                                project.timeline.toggle_select(cg.clip_id);
                            }
                            *playhead = self.pixel_to_frame(cg.x - clip_rect.left());
                            project.timeline.playhead = *playhead;
                        }
                    } else {
                        // Begin drag
                        self.drag_clip = Some(DragState {
                            clip_id: cg.clip_id,
                            orig_track: cg.track_id,
                            orig_pos: self.pixel_to_frame(cg.x - clip_rect.left()),
                            offset_x: pos.x - cg.x,
                        });
                        let link_group = project
                            .timeline
                            .clip(cg.clip_id)
                            .and_then(|c| c.link_group_id);
                        let shift_held = ui.input(|i| i.modifiers.shift);
                        if let Some(lg) = link_group {
                            if !shift_held {
                                project.timeline.clear_selection();
                                for track in &project.timeline.tracks {
                                    for clip in &track.clips {
                                        if clip.link_group_id == Some(lg) {
                                            project.timeline.selected_clip_ids.push(clip.id);
                                        }
                                    }
                                }
                            }
                        } else if !shift_held {
                            project.timeline.select(cg.clip_id);
                        } else {
                            project.timeline.toggle_select(cg.clip_id);
                        }
                        *playhead = self.pixel_to_frame(cg.x - clip_rect.left());
                        project.timeline.playhead = *playhead;
                    }
                } else {
                    // Clicked empty space
                    if self.active_tool == Tool::Zoom {
                        let opt = ui.input(|i| i.modifiers.alt);
                        if opt {
                            self.zoom = (self.zoom * 0.5).max(MIN_PX_PER_FRAME);
                        } else {
                            self.zoom = (self.zoom * 2.0).min(MAX_PX_PER_FRAME);
                        }
                    } else if self.active_tool == Tool::RangeSelect {
                        // Start range selection drag
                        self.range_select = Some((frame, frame));
                    } else if self.active_tool == Tool::Blade {
                        // Blade on empty space does nothing
                    } else {
                        // Default: seek
                        *playhead =
                            frame.clamp(0, last_valid_timeline_frame(project.timeline.duration()));
                        project.timeline.playhead = *playhead;
                        project.timeline.clear_selection();
                    }
                }
            }
        }

        // Drag in progress
        if response.dragged_by(egui::PointerButton::Primary) {
            if let Some(pos) = pointer {
                let frame = self.pixel_to_frame(pos.x - clip_rect.left()).max(0);

                // ── Track drag (reorder) ───────────────────────────────
                if let Some(ref td) = self.track_drag {
                    // Use visual pixel-to-track mapping for accurate track targeting
                    let target_idx = self.pixel_to_track_idx(pos.y - clip_rect.top(), project);
                    if let Some(target_idx) = target_idx {
                        if target_idx != td.orig_index {
                            self.track_drop_target = Some(target_idx);
                            self.track_drag_moved = true;
                        } else {
                            self.track_drop_target = None;
                        }
                    }
                }

                // ── Slide drag ─────────────────────────────────────────
                // ── Speed ramp drag ─────────────────────────────────────
                if let Some(ref ramp) = self.speed_ramp_drag {
                    let new_frame = frame.max(0);
                    if let Some(clip) = project.timeline.clip_mut(ramp.clip_id) {
                        let max_frame = clip.duration().max(1);
                        let clamped = new_frame.min(max_frame);
                        if ramp.point_index < clip.speed_curve.len() {
                            clip.speed_curve[ramp.point_index].frame = clamped;
                            // Keep sorted by frame
                            clip.speed_curve.sort_by_key(|p| p.frame);
                        }
                    }
                }

                if let Some(ref slide) = self.slide_state {
                    let delta = frame - slide.orig_timeline_in;
                    if let Some(clip) = project.timeline.clip_mut(slide.clip_id) {
                        clip.timeline_in = (slide.orig_timeline_in + delta).max(0);
                    }
                    // Adjust left neighbor
                    if let (Some(lid), Some(ld)) = (slide.left_id, slide.left_dur) {
                        if let Some(left) = project.timeline.clip_mut(lid) {
                            left.source_duration = (ld - delta).max(1);
                        }
                    }
                    // Adjust right neighbor
                    if let (Some(rid), Some(rti), Some(rsi), Some(rd)) = (
                        slide.right_id,
                        slide.right_timeline_in,
                        slide.right_source_in,
                        slide.right_dur,
                    ) {
                        if let Some(right) = project.timeline.clip_mut(rid) {
                            right.timeline_in = (rti + delta).max(0);
                            right.source_in = (rsi + delta).max(0);
                            right.source_duration = (rd - delta).max(1);
                        }
                    }
                }

                // ── Slip drag ──────────────────────────────────────────
                if let Some(ref slip) = self.slip_state {
                    let delta = frame - slip.drag_start_frame;
                    if let Some(clip) = project.timeline.clip_mut(slip.clip_id) {
                        clip.source_in = (slip.orig_source_in + delta).max(0);
                    }
                }

                if let Some(ref drag) = self.drag_clip {
                    let new_frame = frame - (drag.offset_x / self.zoom) as i64;
                    let new_frame = new_frame.max(0);
                    // Snap
                    let snapped = self.snap_frame(new_frame, clips, playhead);
                    // Move clip to new position
                    if let Some(track_idx) =
                        self.pixel_to_track_idx(pos.y - clip_rect.top(), project)
                    {
                        if let Some(track) = project.timeline.tracks.get(track_idx) {
                            let new_tid = track.id;
                            if new_tid != drag.orig_track || snapped != drag.orig_pos {
                                if let Some(clip) = project.timeline.clip_mut(drag.clip_id) {
                                    clip.timeline_in = snapped;
                                }
                                // Move between tracks
                                if new_tid != drag.orig_track {
                                    let old_tid = drag.orig_track;
                                    if let Some(old_track) = project.timeline.track_mut(old_tid) {
                                        if let Some(mut clip) = old_track.remove_clip(drag.clip_id)
                                        {
                                            clip.timeline_in = snapped;
                                            if let Some(new_track) =
                                                project.timeline.track_mut(new_tid)
                                            {
                                                new_track.insert_clip(clip).ok();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(ref trim) = self.trim_state {
                    // Compute delta: how far the pointer has moved from the original boundary.
                    let delta_frames = match trim.edge {
                        TrimEdge::Left => frame - trim.orig_timeline_in,
                        TrimEdge::Right => frame - (trim.orig_timeline_in + trim.orig_dur),
                    };

                    let orig_right_edge = trim.orig_timeline_in + trim.orig_dur;
                    let tid = project.timeline.clip_track_id(trim.clip_id);
                    let ripple_enabled = trim.ripple;

                    // ── Apply trim through the track (avoids double borrow) ──
                    let new_right_edge = if let Some(tid) = tid {
                        if let Some(track) = project.timeline.track_mut(tid) {
                            // Find both clips by index in the track
                            let (our_idx, roll_idx) = {
                                let our = track.clips.iter().position(|c| c.id == trim.clip_id);
                                let roll = trim
                                    .roll_clip_id
                                    .and_then(|rid| track.clips.iter().position(|c| c.id == rid));
                                (our, roll)
                            };

                            let mut result = None;
                            if let Some(idx) = our_idx {
                                let clip = &mut track.clips[idx];
                                match trim.edge {
                                    TrimEdge::Left => {
                                        let new_src_in = (trim.orig_in + delta_frames).max(0);
                                        let new_dur = (trim.orig_dur - delta_frames).max(1);
                                        clip.source_in = new_src_in;
                                        clip.source_duration = new_dur;
                                        clip.timeline_in =
                                            (trim.orig_timeline_in + delta_frames).max(0);
                                    }
                                    TrimEdge::Right => {
                                        let new_dur = (trim.orig_dur + delta_frames).max(1);
                                        clip.source_duration = new_dur;
                                    }
                                }
                                result = Some(clip.timeline_in + clip.duration());
                            }

                            // Roll trim: adjust adjacent clip
                            if let (Some(ridx), Some(ri), Some(rd)) =
                                (roll_idx, trim.roll_orig_in, trim.roll_orig_dur)
                            {
                                let roll = &mut track.clips[ridx];
                                if roll.id != trim.clip_id {
                                    match trim.edge {
                                        TrimEdge::Left => {
                                            roll.source_duration = (rd - delta_frames).max(1);
                                        }
                                        TrimEdge::Right => {
                                            roll.source_in = (ri + delta_frames).max(0);
                                            roll.source_duration = (rd - delta_frames).max(1);
                                            roll.timeline_in =
                                                (roll.timeline_in + delta_frames).max(0);
                                        }
                                    }
                                }
                            }
                            result
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // ── Ripple: shift all subsequent clips ───────────────
                    if ripple_enabled {
                        if let Some(new_re) = new_right_edge {
                            let right_edge_delta = new_re - orig_right_edge;
                            if right_edge_delta != 0 {
                                if let Some(tid) = tid {
                                    if let Some(track) = project.timeline.track_mut(tid) {
                                        for c in &mut track.clips {
                                            if c.id != trim.clip_id
                                                && c.timeline_in >= orig_right_edge
                                            {
                                                c.timeline_in =
                                                    (c.timeline_in + right_edge_delta).max(0);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    *playhead = frame;
                    project.timeline.playhead = *playhead;
                }

                // Range Select drag: update end frame
                if let Some((start, _)) = self.range_select {
                    self.range_select = Some((start, frame));
                }
            }
        }

        // Drag ended
        if response.drag_stopped() {
            // Range Select: select clips in range
            if let Some((start, end)) = self.range_select.take() {
                let (r_start, r_end) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                let dur = project.timeline.duration().max(1);
                let r_start = r_start.clamp(0, dur);
                let r_end = r_end.clamp(0, dur);
                if r_end > r_start {
                    project.timeline.clear_selection();
                    for track in &project.timeline.tracks {
                        for clip in &track.clips {
                            if clip.timeline_in < r_end
                                && clip.timeline_in + clip.duration() > r_start
                            {
                                project.timeline.selected_clip_ids.push(clip.id);
                            }
                        }
                    }
                    // Set I/O points to the range
                    project.timeline.in_point = Some(r_start);
                    project.timeline.out_point = Some(r_end);
                }
            }

            // ── Track drag release ────────────────────────────────────
            if let Some(td) = self.track_drag.take() {
                if self.track_drag_moved {
                    // Reorder: move track to drop target
                    if let Some(target) = self.track_drop_target.take() {
                        if target != td.orig_index && target < project.timeline.tracks.len() {
                            let track = project.timeline.tracks.remove(td.orig_index);
                            project.timeline.tracks.insert(target, track);
                        }
                    }
                    self.track_drag_moved = false;
                } else {
                    // It was a click — apply mute/solo/lock/delete based on position
                    let opt = ui.input(|i| i.modifiers.alt);
                    let x_in_header = pointer.map(|p| p.x - clip_rect.left()).unwrap_or(0.0);
                    let tid = td.track_id;
                    if opt {
                        if let Some(t) = project.timeline.track_mut(tid) {
                            t.locked = !t.locked;
                        }
                    } else if x_in_header < TRACK_HEADER_W * 0.4 {
                        if let Some(t) = project.timeline.track_mut(tid) {
                            t.muted = !t.muted;
                        }
                    } else if x_in_header < TRACK_HEADER_W * 0.7 {
                        if let Some(t) = project.timeline.track_mut(tid) {
                            t.solo = !t.solo;
                        }
                    } else {
                        // Delete track
                        let can_delete = project
                            .timeline
                            .track(tid)
                            .map(|t| t.clips.is_empty())
                            .unwrap_or(false);
                        if can_delete {
                            project.timeline.remove_track(tid);
                        }
                    }
                }
            }
            self.track_drop_target = None;

            self.drag_clip = None;
            self.trim_state = None;
            self.slip_state = None;
            self.slide_state = None;
            self.speed_ramp_drag = None;
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Check if a clip is a compound clip (has children).
    fn is_compound(&self, timeline: &rook_core::timeline::Timeline, clip_id: ClipId) -> bool {
        timeline.compound_contents.contains_key(&clip_id)
    }

    /// Check if we're currently inside a compound clip.
    fn inside_compound(&self) -> bool {
        !self.compound_nav.is_empty()
    }

    /// Enter a compound clip — swap timeline tracks with compound's nested tracks.
    fn enter_compound(&mut self, project: &mut Project, compound_id: ClipId) {
        // Save current tracks back to their source before entering
        if let Some(&current_compound) = self.compound_nav.last() {
            // We're already inside a compound — save changes back
            let current_tracks = std::mem::take(&mut project.timeline.tracks);
            project
                .timeline
                .compound_contents
                .insert(current_compound, current_tracks);
        } else {
            // We're at top level
            self.saved_tracks = std::mem::take(&mut project.timeline.tracks);
            self.saved_playhead = project.timeline.playhead;
            self.saved_in_point = project.timeline.in_point;
            self.saved_out_point = project.timeline.out_point;
        }

        let Some(nested) = project
            .timeline
            .compound_contents
            .get(&compound_id)
            .cloned()
        else {
            // Restore if compound not found
            if self.compound_nav.is_empty() {
                project.timeline.tracks = std::mem::take(&mut self.saved_tracks);
                project.timeline.playhead = self.saved_playhead;
                project.timeline.in_point = self.saved_in_point;
                project.timeline.out_point = self.saved_out_point;
            }
            return;
        };

        // Swap in compound's nested tracks
        project.timeline.tracks = nested;
        project.timeline.playhead = 0;
        project.timeline.in_point = None;
        project.timeline.out_point = None;
        project.timeline.clear_selection();

        self.compound_nav.push(compound_id);
    }

    /// Exit current compound clip — save changes and restore parent tracks.
    fn exit_compound(&mut self, project: &mut Project) {
        if self.compound_nav.is_empty() {
            return;
        }

        // Save current tracks back to the compound
        let current_id = self.compound_nav.pop().unwrap();
        let current_tracks = std::mem::take(&mut project.timeline.tracks);
        project
            .timeline
            .compound_contents
            .insert(current_id, current_tracks);

        if self.compound_nav.is_empty() {
            // Back to top level
            project.timeline.tracks = std::mem::take(&mut self.saved_tracks);
            project.timeline.playhead = self.saved_playhead;
            project.timeline.in_point = self.saved_in_point;
            project.timeline.out_point = self.saved_out_point;
        } else {
            // Back to parent compound
            if let Some(&parent_id) = self.compound_nav.last() {
                if let Some(parent_tracks) =
                    project.timeline.compound_contents.get(&parent_id).cloned()
                {
                    project.timeline.tracks = parent_tracks;
                }
            }
            project.timeline.playhead = 0;
            project.timeline.in_point = None;
            project.timeline.out_point = None;
        }
        project.timeline.clear_selection();
    }

    /// Navigate all the way back to project level.
    fn exit_all_compounds(&mut self, project: &mut Project) {
        while self.inside_compound() {
            self.exit_compound(project);
        }
    }

    /// Create a compound clip from selected clips.
    fn create_compound_clip(&mut self, project: &mut Project) {
        let selected: Vec<ClipId> = project.timeline.selected_clip_ids.clone();
        if selected.len() < 2 {
            return;
        }

        // Collect all selected clips
        let mut entries: Vec<(TrackId, Clip)> = Vec::new();
        for &cid in &selected {
            if let Some(tid) = project.timeline.clip_track_id(cid) {
                if let Some(track) = project.timeline.track_mut(tid) {
                    if let Some(clip) = track.remove_clip(cid) {
                        entries.push((tid, clip));
                    }
                }
            }
        }

        if entries.is_empty() {
            return;
        }

        let min_in = entries
            .iter()
            .map(|(_, c)| c.timeline_in)
            .min()
            .unwrap_or(0);
        let max_out = entries
            .iter()
            .map(|(_, c)| c.timeline_in + c.duration())
            .max()
            .unwrap_or(0);
        let compound_dur = max_out - min_in;
        let compound_id = ClipId::next();

        // Create nested tracks: one video track containing all clips (offset to 0)
        let mut nested_clips: Vec<Clip> = Vec::new();
        for (_, mut clip) in entries {
            clip.timeline_in -= min_in;
            nested_clips.push(clip);
        }
        nested_clips.sort_by_key(|c| c.timeline_in);

        let mut nested_track = Track::new(TrackKind::Video, "Nested", 0);
        nested_track.clips = nested_clips;
        project
            .timeline
            .compound_contents
            .insert(compound_id, vec![nested_track]);

        // Create compound clip on first selected clip's track
        let first_tid = project
            .timeline
            .selected_clip_ids
            .first()
            .and_then(|&cid| project.timeline.clip_track_id(cid));
        if let Some(tid) = first_tid {
            if let Some(track) = project.timeline.track_mut(tid) {
                let compound_clip = Clip {
                    id: compound_id,
                    label: "Compound Clip".to_string(),
                    asset_id: AssetId::nil(),
                    timeline_in: min_in,
                    source_in: 0,
                    source_duration: compound_dur,
                    transform: Default::default(),
                    blend_mode: Default::default(),
                    mask: None,
                    fade: None,
                    transition: None,
                    speed: 1.0,
                    speed_curve: vec![],
                    reverse: false,
                    freeze_frame: None,
                    frame_blending: false,
                    spatial_conform: None,
                    gain_db: None,
                    volume_keyframes: None,
                    mute_audio: false,
                    filters: vec![],
                    keyframes: vec![],
                    link_group_id: None,
                    generator: None,
                };
                let _ = track.insert_clip(compound_clip);
                project.timeline.select(compound_id);
            }
        }
    }

    /// Break apart a compound clip, restoring its children to the timeline.
    fn break_apart_compound(&mut self, project: &mut Project, compound_id: ClipId) {
        let Some(nested) = project.timeline.compound_contents.remove(&compound_id) else {
            return;
        };

        // Find and remove the compound clip
        let tid = match project.timeline.clip_track_id(compound_id) {
            Some(t) => t,
            None => {
                project
                    .timeline
                    .compound_contents
                    .insert(compound_id, nested);
                return;
            }
        };
        let compound = {
            let track = match project.timeline.track_mut(tid) {
                Some(t) => t,
                None => {
                    project
                        .timeline
                        .compound_contents
                        .insert(compound_id, nested);
                    return;
                }
            };
            match track.remove_clip(compound_id) {
                Some(c) => c,
                None => {
                    project
                        .timeline
                        .compound_contents
                        .insert(compound_id, nested);
                    return;
                }
            }
        };

        // Restore nested clips with new IDs, offset by compound's timeline_in
        let base = compound.timeline_in;
        let mut new_clips: Vec<(TrackKind, Clip)> = Vec::new();
        for nested_track in &nested {
            for clip in &nested_track.clips {
                let mut c = clip.clone();
                c.timeline_in += base;
                c.id = ClipId::next();
                new_clips.push((nested_track.kind, c));
            }
        }

        // Insert clips into tracks
        for (kind, clip) in &new_clips {
            // Find or create a track of this kind
            let target_tid = project
                .timeline
                .tracks
                .iter()
                .find(|t| t.kind == *kind)
                .map(|t| t.id)
                .unwrap_or_else(|| {
                    let name = match kind {
                        TrackKind::Video => "V1",
                        TrackKind::Audio => "A1",
                        TrackKind::Text => "T1",
                        TrackKind::Effect => "FX1",
                    };
                    let track = Track::new(*kind, name.to_string(), 0);
                    project.timeline.add_track(track)
                });

            if let Some(track) = project.timeline.track_mut(target_tid) {
                let _ = track.insert_clip(clip.clone());
            }
        }

        project.timeline.clear_selection();

        // Exit compound nav if we were inside this compound
        if self.compound_nav.last() == Some(&compound_id) {
            self.compound_nav.pop();
        }
    }

    fn build_clip_geoms(&mut self, project: &Project, left: f32) -> Vec<ClipGeom> {
        let fps = project.frame_rate.as_f64();

        // Collect any completed background waveform/thumbnail extractions.
        // Must be called every frame so results from background threads are
        // moved into the cache even when we don't trigger new extractions.
        self.waveform_cache.poll_completed();
        self.thumbnail_cache.poll_completed();

        // Lazily start background extractions for assets we haven't seen yet.
        // get_or_extract() is non-blocking — spawns a background thread if
        // data isn't cached, returns None immediately.  The `tried` guard
        // prevents infinite retries for assets that fail extraction.
        for asset in &project.assets {
            let asset_id = asset.id();
            if self.waveforms_tried.insert(asset_id) {
                let path = std::path::PathBuf::from(asset.path());
                if path.exists() {
                    let _ = self.waveform_cache.get_or_extract(asset_id, &path);
                }
            }
        }

        for asset in &project.assets {
            let asset_id = asset.id();
            if self.thumbnails_tried.insert(asset_id) {
                let path = std::path::PathBuf::from(asset.path());
                if path.exists() {
                    let _ = self.thumbnail_cache.get_or_extract(asset_id, &path, fps);
                }
            }
        }

        let mut geoms = Vec::new();

        for track in &project.timeline.tracks {
            for clip in &track.clips {
                let x = left + clip.timeline_in as f32 * self.zoom;
                let w = clip.duration() as f32 * self.zoom;
                let asset_name = if let Some(ref generator) = clip.generator {
                    generator.label().to_string()
                } else {
                    project
                        .asset(clip.asset_id)
                        .map(|a| a.filename_stem().to_string())
                        .unwrap_or_else(|| format!("Clip {}", clip.id.0))
                };
                let label = format!("{} ({:.1}s)", asset_name, clip.duration() as f64 / fps);
                let selected = project.timeline.selected_clip_ids.contains(&clip.id);
                let compound = project.timeline.compound_contents.contains_key(&clip.id);
                let color = clip_color(track, selected, clip.generator.is_some(), compound);
                let fade_in = clip.fade.as_ref().map(|f| f.in_frames).unwrap_or(0);
                let fade_out = clip.fade.as_ref().map(|f| f.out_frames).unwrap_or(0);

                // Fetch waveform peaks for audio AND video clips (audio clips get full height,
                // video clips get a thin bar at the bottom)
                let waveform_peaks = if track.kind == TrackKind::Audio || track.kind == TrackKind::Video {
                    if let Some(wf_data) = self.waveform_cache.get(clip.asset_id) {
                        let clip_dur_secs = clip.source_duration as f64 / fps;
                        let src_in_secs = clip.source_in as f64 / fps;
                        let max_bars = if track.kind == TrackKind::Audio {
                            (w.max(20.0) / 3.0).clamp(2.0, 180.0) as usize
                        } else {
                            // Video clips: more bars for a denser waveform
                            (w.max(20.0) / 2.0).clamp(4.0, 300.0) as usize
                        };
                        peaks_for_clip(&wf_data, clip_dur_secs, src_in_secs, max_bars)
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                // Fetch thumbnail indices for video clips
                let (thumbnail_indices, has_thumbnails) = if track.kind == TrackKind::Video {
                    if let Some(strip) = self.thumbnail_cache.get(clip.asset_id) {
                        let start_secs = clip.source_in as f64 / fps;
                        let end_secs = (clip.source_in + clip.source_duration) as f64 / fps;
                        let indices = thumbs_for_clip(&strip, start_secs, end_secs);
                        let has = !indices.is_empty();
                        (indices, has)
                    } else {
                        (Vec::new(), false)
                    }
                } else {
                    (Vec::new(), false)
                };

                geoms.push(ClipGeom {
                    clip_id: clip.id,
                    track_id: track.id,
                    asset_id: clip.asset_id,
                    x,
                    w,
                    label,
                    color,
                    selected,
                    track_kind: track.kind,
                    source_duration: clip.source_duration,
                    source_in: clip.source_in,
                    fade_in_frames: fade_in,
                    fade_out_frames: fade_out,
                    waveform_peaks,
                    thumbnail_indices,
                    has_thumbnails,
                    speed: clip.speed,
                    gain_db: clip.gain_db,
                    link_group_id: clip.link_group_id,
                    speed_curve_points: {
                        let dur = clip.duration().max(1);
                        clip.speed_curve
                            .iter()
                            .map(|p| (p.frame as f32 / dur as f32, p.speed))
                            .collect()
                    },
                    has_audio_fade: clip.fade.is_some() && track.kind == TrackKind::Audio,
                    audio_gain_db: clip.gain_db.unwrap_or(0.0),
                    volume_keyframes: {
                        let mut kfs: Vec<(i64, f64)> = clip
                            .keyframes
                            .iter()
                            .filter(|k| k.property == rook_core::keyframe::KeyframeProperty::Volume)
                            .map(|k| (k.at_frame, k.value))
                            .collect();
                        // Also include direct volume keyframes from clip model
                        if let Some(ref vkfs) = clip.volume_keyframes {
                            kfs.extend(vkfs.iter().copied());
                            kfs.sort_by_key(|(f, _)| *f);
                            kfs.dedup_by_key(|(f, _)| *f);
                        }
                        kfs
                    },
                    is_compound: project.timeline.compound_contents.contains_key(&clip.id),
                });
            }
        }
        geoms
    }

    /// Compute visual Y positions for all tracks, accounting for
    /// the audio/video split layout. Audio tracks are displayed below
    /// video/text/effect tracks with a separator between them.
    fn compute_visual_ys(&mut self, project: &Project, top: f32) {
        self.visual_track_ys.clear();
        let mut video_vis_idx: usize = 0;
        let mut audio_vis_idx: usize = 0;
        let has_video = video_vis_count(project) > 0;

        for track in &project.timeline.tracks {
            if track.kind.is_audio() {
                if has_video {
                    let y = top + RULER_H
                        + (video_vis_count(project) as f32 * self.track_h)
                        + AUDIO_SEPARATOR_H
                        + audio_vis_idx as f32 * self.track_h;
                    self.visual_track_ys.insert(track.id, y);
                } else {
                    let y = top + RULER_H + audio_vis_idx as f32 * self.track_h;
                    self.visual_track_ys.insert(track.id, y);
                }
                audio_vis_idx += 1;
            } else {
                let y = top + RULER_H + video_vis_idx as f32 * self.track_h;
                self.visual_track_ys.insert(track.id, y);
                video_vis_idx += 1;
            }
        }
    }

    fn track_y(&self, track_id: TrackId, _project: &Project, _top: f32) -> f32 {
        self.visual_track_ys
            .get(&track_id)
            .copied()
            .unwrap_or(_top + RULER_H)
    }

    fn pixel_to_frame(&self, px: f32) -> i64 {
        (px / self.zoom).round() as i64
    }

    fn pixel_to_track_idx(&self, py: f32, project: &Project) -> Option<usize> {
        // Iterate through tracks in order and check visual Y ranges
        for (i, track) in project.timeline.tracks.iter().enumerate() {
            if let Some(&ty) = self.visual_track_ys.get(&track.id) {
                if py >= ty && py < ty + self.track_h {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Total visual canvas height including separator.
    fn visual_canvas_h(&self, project: &Project) -> f32 {
        let video = video_vis_count(project);
        let audio = audio_vis_count(project);
        let mut h = RULER_H
            + video as f32 * self.track_h
            + audio as f32 * self.track_h
            + 8.0;
        if video > 0 && audio > 0 {
            h += AUDIO_SEPARATOR_H;
        }
        h.max(100.0)
    }

    /// Y position of the audio separator (top of separator bar).
    fn audio_separator_y(&self, project: &Project, top: f32) -> f32 {
        let video = video_vis_count(project);
        top + RULER_H + video as f32 * self.track_h
    }

    /// Find a clip adjacent to the given boundary on the same track.
    /// Returns (clip_id, source_in, source_duration) if an abutting clip exists.
    fn find_adjacent_clip(
        &self,
        project: &Project,
        this_clip_id: ClipId,
        track_id: TrackId,
        boundary_frame: i64,
        look_left: bool,
    ) -> (Option<ClipId>, Option<i64>, Option<i64>) {
        let track = match project.timeline.track(track_id) {
            Some(t) => t,
            None => return (None, None, None),
        };
        for clip in &track.clips {
            if clip.id == this_clip_id {
                continue;
            }
            if look_left {
                // Looking for a clip whose RIGHT edge abuts our LEFT edge
                if clip.timeline_in + clip.duration() == boundary_frame {
                    return (
                        Some(clip.id),
                        Some(clip.source_in),
                        Some(clip.source_duration),
                    );
                }
            } else {
                // Looking for a clip whose LEFT edge abuts our RIGHT edge
                if clip.timeline_in == boundary_frame {
                    return (
                        Some(clip.id),
                        Some(clip.source_in),
                        Some(clip.source_duration),
                    );
                }
            }
        }
        (None, None, None)
    }

    fn snap_frame(&self, frame: i64, clips: &[ClipGeom], playhead: &i64) -> i64 {
        let mut candidates = vec![*playhead];
        for cg in clips {
            let clip_frame = (cg.w / self.zoom).round() as i64;
            if clip_frame > 0 {
                candidates.push(self.pixel_to_frame(cg.x));
                candidates.push(self.pixel_to_frame(cg.x + cg.w));
            }
        }
        candidates.sort();
        candidates.dedup();

        let snap_px = SNAP_THRESHOLD / self.zoom; // snap threshold in frames
        for c in &candidates {
            if (frame - c).abs() <= snap_px as i64 {
                return *c;
            }
        }
        frame
    }

    /// Split a single clip at the given timeline frame.
    fn split_clip_at(&mut self, project: &mut Project, clip_id: ClipId, at_frame: i64) {
        if let Some(clip) = project.timeline.clip(clip_id) {
            if !clip.covers(at_frame) {
                return;
            }
        } else {
            return;
        }
        if let Some(tid) = project.timeline.clip_track_id(clip_id) {
            if let Some(track) = project.timeline.track_mut(tid) {
                let original = track.remove_clip(clip_id);
                if let Some(orig) = original {
                    let split_pt = at_frame - orig.timeline_in;
                    let left = rook_core::clip::Clip {
                        id: ClipId::next(),
                        label: format!("{} A", orig.label),
                        asset_id: orig.asset_id,
                        timeline_in: orig.timeline_in,
                        source_in: orig.source_in,
                        source_duration: split_pt,
                        transform: orig.transform.clone(),
                        blend_mode: orig.blend_mode,
                        mask: orig.mask.clone(),
                        fade: orig.fade,
                        transition: None,
                        speed: orig.speed,
                        speed_curve: orig.speed_curve.clone(),
                        reverse: orig.reverse,
                        freeze_frame: orig.freeze_frame,
                        frame_blending: orig.frame_blending,
                        spatial_conform: orig.spatial_conform,
                        gain_db: orig.gain_db,
                        volume_keyframes: orig.volume_keyframes.clone(),
                        mute_audio: orig.mute_audio,
                        filters: orig.filters.clone(),
                        keyframes: orig.keyframes.clone(),
                        link_group_id: orig.link_group_id,
                        generator: orig.generator.clone(),
                    };
                    let right = rook_core::clip::Clip {
                        id: ClipId::next(),
                        label: format!("{} B", orig.label),
                        asset_id: orig.asset_id,
                        timeline_in: at_frame,
                        source_in: orig.source_in + split_pt,
                        source_duration: orig.source_duration - split_pt,
                        transform: orig.transform,
                        blend_mode: orig.blend_mode,
                        mask: orig.mask,
                        fade: orig.fade,
                        transition: None,
                        speed: orig.speed,
                        speed_curve: orig.speed_curve,
                        reverse: orig.reverse,
                        freeze_frame: orig.freeze_frame,
                        frame_blending: orig.frame_blending,
                        spatial_conform: orig.spatial_conform,
                        gain_db: orig.gain_db,
                        volume_keyframes: orig.volume_keyframes.clone(),
                        mute_audio: orig.mute_audio,
                        filters: orig.filters,
                        keyframes: orig.keyframes,
                        link_group_id: orig.link_group_id,
                        generator: orig.generator,
                    };
                    let right_id = right.id;
                    track.insert_clip(left).ok();
                    track.insert_clip(right).ok();
                    project.timeline.select(right_id);
                }
            }
        }
    }

    /// Blade all tracks at the playhead.
    fn blade_all_tracks(&mut self, project: &mut Project, at_frame: i64) {
        let track_count = project.timeline.tracks.len();
        let mut all_clips: Vec<Vec<ClipId>> = Vec::new();
        // Collect all clip IDs first to avoid borrow conflicts
        for i in 0..track_count {
            let track = &project.timeline.tracks[i];
            let clip_ids: Vec<ClipId> = track
                .clips
                .iter()
                .filter(|c| c.covers(at_frame))
                .map(|c| c.id)
                .collect();
            all_clips.push(clip_ids);
        }
        // Then split them
        for clip_ids in all_clips {
            for cid in clip_ids {
                self.split_clip_at(project, cid, at_frame);
            }
        }
    }

    /// Trim the start of the selected clip to the given frame (Option+[).
    fn trim_selected_start_to(&mut self, project: &mut Project, at_frame: i64) {
        let Some(cid) = project.timeline.selected_clip_ids.first().copied() else {
            return;
        };
        let Some(clip) = project.timeline.clip(cid) else {
            return;
        };
        if !clip.covers(at_frame) {
            return;
        }
        let offset = at_frame - clip.timeline_in;
        let Some(tid) = project.timeline.clip_track_id(cid) else {
            return;
        };
        let Some(track) = project.timeline.track_mut(tid) else {
            return;
        };
        let Some(clip) = track.clip_mut(cid) else {
            return;
        };
        clip.timeline_in = at_frame;
        clip.source_in += offset;
        clip.source_duration = (clip.source_duration - offset).max(1);
    }

    /// Trim the end of the selected clip to the given frame (Option+]).
    fn trim_selected_end_to(&mut self, project: &mut Project, at_frame: i64) {
        let Some(cid) = project.timeline.selected_clip_ids.first().copied() else {
            return;
        };
        let Some(clip) = project.timeline.clip(cid) else {
            return;
        };
        if !clip.covers(at_frame) {
            return;
        }
        let offset = at_frame - clip.timeline_in;
        let Some(tid) = project.timeline.clip_track_id(cid) else {
            return;
        };
        let Some(track) = project.timeline.track_mut(tid) else {
            return;
        };
        let Some(clip) = track.clip_mut(cid) else {
            return;
        };
        clip.source_duration = offset.max(1);
    }

    /// Trim the selected clip to the I/O mark range (Option+\).
    fn trim_selected_to_io_range(&mut self, project: &mut Project) {
        let Some(in_pt) = project.timeline.in_point else {
            return;
        };
        let Some(out_pt) = project.timeline.out_point else {
            return;
        };
        if out_pt <= in_pt {
            return;
        }
        let Some(cid) = project.timeline.selected_clip_ids.first().copied() else {
            return;
        };
        let Some(clip) = project.timeline.clip(cid) else {
            return;
        };
        // The clip must at least partially overlap the I/O range
        if clip.timeline_in + clip.duration() <= in_pt || clip.timeline_in >= out_pt {
            return;
        }
        let offset = in_pt - clip.timeline_in;
        let Some(tid) = project.timeline.clip_track_id(cid) else {
            return;
        };
        let Some(track) = project.timeline.track_mut(tid) else {
            return;
        };
        let Some(clip) = track.clip_mut(cid) else {
            return;
        };
        clip.timeline_in = in_pt;
        clip.source_in += offset;
        clip.source_duration = (out_pt - in_pt).max(1);
    }

    // ── Ruler painting ─────────────────────────────────────────────────

    fn paint_ruler(
        &mut self,
        painter: &egui::Painter,
        clip_rect: egui::Rect,
        total_frames: i64,
        fps: f64,
        project: &Project,
    ) {
        let ruler_rect =
            egui::Rect::from_min_size(clip_rect.left_top(), egui::vec2(clip_rect.width(), RULER_H));
        painter.rect_filled(ruler_rect, 0.0, RULER_BG);

        let font = egui::FontId::proportional(9.0);
        let text_color = egui::Color32::from_gray(160);
        let tick_color = egui::Color32::from_gray(60);

        // Determine tick spacing based on zoom
        let px_per_sec = self.zoom * fps as f32;
        let (major_interval, minor_interval) = if px_per_sec > 200.0 {
            (1.0f64, 0.2) // 1-second major, frame minor
        } else if px_per_sec > 60.0 {
            (1.0, 0.5)
        } else if px_per_sec > 20.0 {
            (5.0, 1.0)
        } else if px_per_sec > 5.0 {
            (30.0, 5.0)
        } else {
            (60.0, 10.0)
        };

        // Major ticks with time labels
        let major_frames = (major_interval * fps as f64) as i64;
        let minor_frames = (minor_interval * fps as f64) as i64;

        let start_frame = ((-clip_rect.left()) / self.zoom).max(0.0) as i64;
        let end_frame = ((clip_rect.right() - clip_rect.left()) / self.zoom).ceil() as i64 + 1;

        let mut t = (start_frame / minor_frames.max(1)) * minor_frames.max(1);
        while t <= end_frame {
            let sx = clip_rect.left() + t as f32 * self.zoom;
            if sx >= clip_rect.left() && sx <= clip_rect.right() {
                let is_major = t % major_frames.max(1) == 0;
                let tick_h = if is_major { 10.0 } else { 4.0 };
                let tick_y = ruler_rect.bottom() - tick_h;
                painter.line_segment(
                    [egui::pos2(sx, tick_y), egui::pos2(sx, ruler_rect.bottom())],
                    egui::Stroke::new(if is_major { 1.0 } else { 0.5 }, tick_color),
                );

                if is_major {
                    let secs = t as f64 / fps as f64;
                    let label = if secs < 60.0 {
                        format!("{:.0}s", secs)
                    } else {
                        format!("{}:{:02.0}", (secs / 60.0) as i64, secs % 60.0)
                    };
                    painter.text(
                        egui::pos2(sx + 2.0, ruler_rect.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        font.clone(),
                        text_color,
                    );
                }
            }
            t += minor_frames.max(1);
        }

        // Draw markers as diamonds
        for marker in &project.timeline.markers {
            let mx = clip_rect.left() + marker.frame as f32 * self.zoom;
            if mx >= clip_rect.left() && mx <= clip_rect.right() {
                let my = ruler_rect.center().y;
                let color = marker
                    .color
                    .map(|c| egui::Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]))
                    .unwrap_or(MARKER_COLOR);
                // Diamond shape
                let d = 5.0;
                let diamond = vec![
                    egui::pos2(mx, my - d),
                    egui::pos2(mx + d, my),
                    egui::pos2(mx, my + d),
                    egui::pos2(mx - d, my),
                ];
                painter.add(egui::Shape::convex_polygon(
                    diamond,
                    color,
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                ));
                // Label below diamond
                if !marker.label.is_empty() {
                    let lbl = if marker.label.len() > 8 {
                        format!("{}…", &marker.label[..8])
                    } else {
                        marker.label.clone()
                    };
                    painter.text(
                        egui::pos2(mx, ruler_rect.bottom() - 2.0),
                        egui::Align2::CENTER_BOTTOM,
                        lbl,
                        egui::FontId::proportional(8.0),
                        color,
                    );
                }
            }
        }

        // ── Timeline Index (colored bar showing visible portion) ───────
        let total_dur = project.timeline.duration().max(1) as f32;
        if total_dur > 0.0 {
            let total_px = total_dur * self.zoom;
            let index_h = 3.0;
            let index_y = ruler_rect.bottom() - index_h - 1.0;
            let clickable_index_h = index_h + 6.0; // wider hit area for clicking
            // Full timeline background strip
            if total_px > 0.0 {
                let full_rect = egui::Rect::from_min_size(
                    egui::pos2(clip_rect.left(), index_y),
                    egui::vec2(total_px.min(clip_rect.width()), index_h),
                );
                painter.rect_filled(full_rect, 0.0, egui::Color32::from_gray(25));
                // Store the clickable rect for hit-testing (wider than visual)
                self.timeline_index_rect = Some(egui::Rect::from_min_size(
                    egui::pos2(clip_rect.left(), index_y - 3.0),
                    egui::vec2(total_px.min(clip_rect.width()), clickable_index_h),
                ));
            }
            // Visible portion highlight
            let view_left = self.scroll_x.max(0.0);
            let view_right = (view_left + (clip_rect.width() - TRACK_HEADER_W)).min(total_px);
            if view_right > view_left {
                let vis_rect = egui::Rect::from_min_size(
                    egui::pos2(clip_rect.left() + view_left, index_y),
                    egui::vec2((view_right - view_left).max(1.0), index_h),
                );
                painter.rect_filled(vis_rect, 0.0, egui::Color32::from_rgb(80, 140, 220));
            }
        } else {
            self.timeline_index_rect = None;
        }

        // Ruler bottom border
        painter.line_segment(
            [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
            egui::Stroke::new(1.0, egui::Color32::from_gray(40)),
        );
    }

    /// Sync an audio clip to a video clip by cross-correlating their waveforms.
    fn sync_audio_clips(&mut self, project: &mut Project, video_cid: ClipId, audio_cid: ClipId) {
        let fps = project.frame_rate.as_f64();
        let (video_peaks, audio_peaks) = {
            let v_clip = match project.timeline.clip(video_cid) {
                Some(c) => c,
                None => return,
            };
            let a_clip = match project.timeline.clip(audio_cid) {
                Some(c) => c,
                None => return,
            };
            let v_path = project
                .asset(v_clip.asset_id)
                .map(|a| std::path::PathBuf::from(a.path()))
                .unwrap_or_default();
            let a_path = project
                .asset(a_clip.asset_id)
                .map(|a| std::path::PathBuf::from(a.path()))
                .unwrap_or_default();
            let v_wf = self.waveform_cache.get_or_extract(v_clip.asset_id, &v_path);
            let a_wf = self.waveform_cache.get_or_extract(a_clip.asset_id, &a_path);
            let dur_secs = |clip: &Clip| clip.source_duration as f64 / fps;
            let src_secs = |clip: &Clip| clip.source_in as f64 / fps;
            let v_p = v_wf
                .as_ref()
                .map(|w| {
                    crate::widgets::waveform::peaks_for_clip(
                        w,
                        dur_secs(v_clip),
                        src_secs(v_clip),
                        480,
                    )
                })
                .unwrap_or_default();
            let a_p = a_wf
                .as_ref()
                .map(|w| {
                    crate::widgets::waveform::peaks_for_clip(
                        w,
                        dur_secs(a_clip),
                        src_secs(a_clip),
                        480,
                    )
                })
                .unwrap_or_default();
            (v_p, a_p)
        };
        if video_peaks.is_empty() || audio_peaks.is_empty() {
            tracing::warn!("sync audio: no waveform data");
            return;
        }
        // Cross-correlate
        let max_offset = (video_peaks.len().min(audio_peaks.len()) / 2).max(1);
        let mut best_offset: isize = 0;
        let mut best_corr = f32::NEG_INFINITY;
        for offset in -(max_offset as isize)..(max_offset as isize) {
            let mut corr = 0.0f32;
            let mut count = 0usize;
            for i in 0..video_peaks.len() {
                let j = i as isize + offset;
                if j >= 0 && (j as usize) < audio_peaks.len() {
                    corr += video_peaks[i] * audio_peaks[j as usize];
                    count += 1;
                }
            }
            if count > 0 {
                corr /= count as f32;
            }
            if corr > best_corr {
                best_corr = corr;
                best_offset = offset;
            }
        }
        // Convert to frame offset (approximate: bars map to time via duration ratio)
        let video_dur_frames = project
            .timeline
            .clip(video_cid)
            .map(|c| c.duration())
            .unwrap_or(1);
        let bars_count = video_peaks.len().max(1) as f64;
        let frames_per_bar = video_dur_frames as f64 / bars_count;
        let frame_offset = (best_offset as f64 * frames_per_bar).round() as i64;
        // Move audio clip
        if let Some(v_clip) = project.timeline.clip(video_cid) {
            let new_pos = (v_clip.timeline_in + frame_offset).max(0);
            if let Some(tid) = project.timeline.clip_track_id(audio_cid) {
                if let Some(track) = project.timeline.track_mut(tid) {
                    if let Some(clip) = track.clip_mut(audio_cid) {
                        clip.timeline_in = new_pos;
                        tracing::info!(video=%video_cid.0, audio=%audio_cid.0, offset=frame_offset, "audio synced");
                    }
                }
            }
        }
    }
}

fn track_header_color(track: &Track) -> egui::Color32 {
    if track.muted {
        egui::Color32::from_gray(30)
    } else if track.disabled {
        egui::Color32::from_gray(24)
    } else if let Some(ref color) = track.color {
        track_color_to_egui(color)
    } else {
        match track.kind {
            TrackKind::Video => egui::Color32::from_rgb(35, 70, 110),
            TrackKind::Audio => egui::Color32::from_rgb(35, 100, 70),
            TrackKind::Text => egui::Color32::from_rgb(110, 70, 35),
            TrackKind::Effect => egui::Color32::from_rgb(70, 35, 70),
        }
    }
}

fn track_color_to_egui(color: &rook_core::TrackColor) -> egui::Color32 {
    match color {
        rook_core::TrackColor::Red => egui::Color32::from_rgb(231, 76, 60),
        rook_core::TrackColor::Orange => egui::Color32::from_rgb(230, 126, 34),
        rook_core::TrackColor::Yellow => egui::Color32::from_rgb(241, 196, 15),
        rook_core::TrackColor::Green => egui::Color32::from_rgb(46, 204, 113),
        rook_core::TrackColor::Blue => egui::Color32::from_rgb(52, 152, 219),
        rook_core::TrackColor::Purple => egui::Color32::from_rgb(155, 89, 182),
        rook_core::TrackColor::Pink => egui::Color32::from_rgb(233, 30, 144),
        rook_core::TrackColor::Gray => egui::Color32::from_rgb(127, 140, 141),
    }
}

fn clip_color(
    track: &Track,
    selected: bool,
    is_generator: bool,
    is_compound: bool,
) -> egui::Color32 {
    if selected {
        egui::Color32::from_rgb(255, 200, 80)
    } else if is_compound {
        egui::Color32::from_rgb(160, 140, 220)
    } else if is_generator {
        egui::Color32::from_rgb(120, 200, 150)
    } else {
        match track.kind {
            TrackKind::Video => egui::Color32::from_rgb(100, 160, 240),
            TrackKind::Audio => egui::Color32::from_rgb(100, 220, 140),
            TrackKind::Text => egui::Color32::from_rgb(220, 160, 100),
            TrackKind::Effect => egui::Color32::from_rgb(170, 110, 220),
        }
    }
}

// ── Helpers for thumbnail/waveform rendering ──────────────────────────

/// Convert HSV to RGB (all components 0.0–1.0).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h * 6.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

/// Simple pseudo-random float 0.0–1.0 from a seed and index.
fn pseudo_random(seed: u64, idx: u64) -> f64 {
    let hash = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(idx);
    let mix = hash ^ (hash >> 30);
    let mixed = mix.wrapping_mul(0xbf58476d1ce4e5b9);
    let final_hash = mixed ^ (mixed >> 27);
    let val = final_hash.wrapping_mul(0x94d049bb133111eb);
    (val as f64 / u64::MAX as f64).abs()
}

/// Count non-audio tracks (Video, Text, Effect).
fn video_vis_count(project: &Project) -> usize {
    project
        .timeline
        .tracks
        .iter()
        .filter(|t| !t.kind.is_audio())
        .count()
}

/// Count audio tracks.
fn audio_vis_count(project: &Project) -> usize {
    project
        .timeline
        .tracks
        .iter()
        .filter(|t| t.kind.is_audio())
        .count()
}
