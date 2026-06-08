//! The engine: owns Project + MediaPool + MLT bridge + TimelineGraph.
//! Adapted from cutlass-engines `Engine`, augmented with Gausian timeline graph.

use std::collections::HashMap;
use std::path::PathBuf;

use rook_core::{
    asset::AssetId,
    canvas::Canvas,
    clip::ClipId,
    commands::EditCommand,
    history::EditHistory,
    ids::TrackId,
    project::Project,
    time::Rational,
    track::{Track, TrackKind},
};

use crate::error::EngineError;
use crate::pool::{MediaPool, PoolConfig};
use crate::proxy::ProxyService;
use crate::resolve::{RenderedLayer, resolve_frame};
use rook_plugin_host::PluginHost;

const DEFAULT_HISTORY_LIMIT: usize = 128;

/// The headless editing session.
pub struct Engine {
    project: Project,
    pool: MediaPool,
    history: EditHistory,
    proxy: ProxyService,
    /// The graph-model view of the timeline, synced on every mutation.
    graph: rook_timeline::TimelineGraph,
    /// SQLite database for persistence (optional — lazy init on first save).
    db: Option<rook_project_db::ProjectDb>,
    /// Current project file path (for save).
    current_path: Option<PathBuf>,
    /// Whether MLT is initialized and the tractor is live.
    mlt_live: bool,
    // MLT objects (only valid when mlt_live)
    mlt_profile: Option<rook_mlt::profile::Profile>,
    mlt_tractor: Option<rook_mlt::tractor::Tractor>,
    /// Per-track playlists, keyed by TrackId.
    mlt_playlists: HashMap<TrackId, rook_mlt::playlist::Playlist>,
    /// Per-clip producers, keyed by ClipId.
    mlt_producers: HashMap<ClipId, rook_mlt::producer::Producer>,
    /// Named project snapshots (save points the user can revert to).
    snapshots: Vec<(String, rook_core::project::Project)>,
    /// Plugin host — routes WASM and OFX plugins to their respective sandboxes.
    plugin_host: PluginHost,
}

impl Engine {
    /// Create an empty engine for a new project.
    pub fn new(name: impl Into<String>, canvas: Canvas, frame_rate: Rational) -> Self {
        let project = Project::new(name, canvas, frame_rate);
        Self {
            project,
            pool: MediaPool::new(PoolConfig::default()),
            history: EditHistory::new(DEFAULT_HISTORY_LIMIT),
            proxy: ProxyService::new(),
            graph: rook_timeline::TimelineGraph::default(),
            db: None,
            current_path: None,
            mlt_live: false,
            mlt_profile: None,
            mlt_tractor: None,
            mlt_playlists: HashMap::new(),
            mlt_producers: HashMap::new(),
            snapshots: Vec::new(),
            plugin_host: PluginHost::new(),
        }
    }

    /// Initialise the SQLite database (lazy — called on first save).
    pub fn init_db(&mut self, db_path: &PathBuf) -> Result<(), EngineError> {
        if self.db.is_some() {
            return Ok(());
        }
        let db = rook_project_db::ProjectDb::open_or_create(db_path)
            .map_err(|e| EngineError::Generic("failed to open database"))?;
        let project_id = self.project.id.to_string();
        db.ensure_project(&project_id, &self.project.name, None)
            .map_err(|e| EngineError::Generic("failed to ensure project"))?;
        self.db = Some(db);
        self.current_path = Some(db_path.clone());
        Ok(())
    }

    /// Get the database path (for UI display).
    pub fn db_path(&self) -> Option<&PathBuf> {
        self.current_path.as_ref()
    }

    /// Initialize MLT and build the tractor from the current project state.
    pub fn init_mlt(&mut self) -> Result<(), EngineError> {
        if self.mlt_live {
            return Ok(());
        }
        rook_mlt::init()?;
        let profile = rook_mlt::profile::Profile::from_preset("hd1080_24")?;
        let tractor = rook_mlt::tractor::Tractor::new(&profile)?;
        self.mlt_profile = Some(profile);
        self.mlt_tractor = Some(tractor);
        self.mlt_live = true;
        self.rebuild_mlt()?;
        Ok(())
    }

    /// Rebuild the entire MLT timeline from the current project state.
    /// Called after `init_mlt()` and on project open.
    fn rebuild_mlt(&mut self) -> Result<(), EngineError> {
        if !self.mlt_live {
            return Ok(());
        }

        let profile = self.mlt_profile.as_ref().unwrap();
        let tractor = self.mlt_tractor.as_ref().unwrap();

        self.mlt_playlists.clear();
        self.mlt_producers.clear();

        for (ti, track) in self.project.timeline.tracks.iter().enumerate() {
            let playlist = match rook_mlt::playlist::Playlist::new(profile) {
                Ok(p) => p,
                Err(_) => continue,
            };

            for clip in &track.clips {
                let asset_path = self
                    .project
                    .asset(clip.asset_id)
                    .map(|a| std::path::PathBuf::from(a.path()));
                if let Some(ref path) = asset_path {
                    if let Ok(producer) = rook_mlt::producer::Producer::from_file(profile, path) {
                        producer.seek(clip.source_in);
                        let _ = playlist.insert_at(
                            playlist.count(),
                            &producer,
                            clip.source_in,
                            clip.source_in + clip.source_duration,
                        );
                        self.mlt_producers.insert(clip.id, producer);
                    }
                }
            }

            tractor.connect(&playlist, ti as i32);
            self.mlt_playlists.insert(track.id, playlist);
        }

        tracing::info!(tracks = self.mlt_playlists.len(), "MLT timeline rebuilt");
        Ok(())
    }

    // ── Read access ─────────────────────────────────────────────────────

    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn project_mut(&mut self) -> &mut Project {
        &mut self.project
    }
    /// The graph-model view of the timeline (synced on every mutation).
    pub fn graph(&self) -> &rook_timeline::TimelineGraph {
        &self.graph
    }
    pub fn pool(&self) -> &MediaPool {
        &self.pool
    }
    pub fn proxy(&self) -> &ProxyService {
        &self.proxy
    }
    pub fn current_path(&self) -> Option<&PathBuf> {
        self.current_path.as_ref()
    }
    pub fn plugin_host(&self) -> &PluginHost {
        &self.plugin_host
    }
    pub fn plugin_host_mut(&mut self) -> &mut PluginHost {
        &mut self.plugin_host
    }

    // ── Apply an edit command ───────────────────────────────────────────

    pub fn apply(&mut self, cmd: EditCommand) -> Result<(), EngineError> {
        let label = cmd.label().to_string();
        self.history.record_labeled(&label, self.project.clone());
        self.apply_to_model(&cmd)?;
        self.project.timeline.rebuild_index();
        self.sync_to_graph();
        self.project.updated_at = now_secs();
        if self.mlt_live {
            self.mirror_to_mlt(&cmd)?;
        }
        tracing::debug!(command = %label, "applied");
        Ok(())
    }

    /// Apply a batch directly (no undo on each sub-command).
    pub fn apply_batch(&mut self, commands: Vec<EditCommand>) -> Result<(), EngineError> {
        let label = if commands.len() == 1 {
            commands[0].label().to_string()
        } else {
            format!("{} edits", commands.len())
        };
        self.history.record_labeled(&label, self.project.clone());
        for cmd in &commands {
            self.apply_to_model(cmd)?;
        }
        self.project.timeline.rebuild_index();
        self.sync_to_graph();
        self.project.updated_at = now_secs();
        Ok(())
    }

    fn apply_to_model(&mut self, cmd: &EditCommand) -> Result<(), EngineError> {
        match cmd {
            EditCommand::InsertClip {
                asset_id,
                track_id,
                position,
                source_in,
                source_out,
                link_group_id,
            } => {
                let source_duration = source_out - source_in;
                let label = self
                    .project
                    .asset(*asset_id)
                    .map(|a| a.filename_stem().to_string())
                    .unwrap_or_else(|| "clip".to_string());
                let track = self
                    .project
                    .timeline
                    .track_mut(*track_id)
                    .ok_or(EngineError::Generic("track not found"))?;
                let clip = rook_core::clip::Clip {
                    id: ClipId::next(),
                    label,
                    asset_id: *asset_id,
                    timeline_in: *position,
                    source_in: *source_in,
                    source_duration,
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
                    link_group_id: *link_group_id,
                    generator: None,
                };
                track.insert_clip(clip)?;
            }
            EditCommand::RemoveClip { clip_id } => {
                let track_id = self
                    .project
                    .timeline
                    .clip_track_id(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                let track = self.project.timeline.track_mut(track_id).unwrap();
                track.remove_clip(*clip_id);
            }
            EditCommand::RippleDelete { clip_id } => {
                let track_id = self
                    .project
                    .timeline
                    .clip_track_id(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                let track = self.project.timeline.track_mut(track_id).unwrap();
                if let Some(removed) = track.remove_clip(*clip_id) {
                    let gap = removed.duration();
                    for clip in &mut track.clips {
                        if clip.timeline_in > removed.timeline_in {
                            clip.timeline_in -= gap;
                        }
                    }
                }
            }
            EditCommand::MoveClip {
                clip_id,
                new_track_id,
                new_position,
            } => {
                let old_track_id = self
                    .project
                    .timeline
                    .clip_track_id(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                let mut clip = self
                    .project
                    .timeline
                    .track_mut(old_track_id)
                    .unwrap()
                    .remove_clip(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.timeline_in = *new_position;
                self.project
                    .timeline
                    .track_mut(*new_track_id)
                    .ok_or(EngineError::Generic("target track not found"))?
                    .insert_clip(clip)?;
            }
            EditCommand::SplitClip { clip_id, at_frame } => {
                let track_id = self
                    .project
                    .timeline
                    .clip_track_id(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                let track = self.project.timeline.track_mut(track_id).unwrap();
                let original = track
                    .remove_clip(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                let split_point = at_frame - original.timeline_in;
                let left = rook_core::clip::Clip {
                    id: ClipId::next(),
                    label: format!("{} (A)", original.label),
                    asset_id: original.asset_id,
                    timeline_in: original.timeline_in,
                    source_in: original.source_in,
                    source_duration: split_point,
                    transform: original.transform.clone(),
                    blend_mode: original.blend_mode,
                    mask: original.mask.clone(),
                    fade: original.fade,
                    transition: None,
                    speed: original.speed,
                    speed_curve: original.speed_curve.clone(),
                    reverse: original.reverse,
                    freeze_frame: original.freeze_frame,
                    frame_blending: original.frame_blending,
                    spatial_conform: original.spatial_conform,
                    gain_db: original.gain_db,
                    volume_keyframes: original.volume_keyframes.clone(),
                    mute_audio: original.mute_audio,
                    filters: original.filters.clone(),
                    keyframes: original.keyframes.clone(),
                    link_group_id: original.link_group_id,
                    generator: original.generator.clone(),
                };
                let right = rook_core::clip::Clip {
                    id: ClipId::next(),
                    label: format!("{} (B)", original.label),
                    asset_id: original.asset_id,
                    timeline_in: *at_frame,
                    source_in: original.source_in + split_point,
                    source_duration: original.source_duration - split_point,
                    transform: original.transform,
                    blend_mode: original.blend_mode,
                    mask: original.mask,
                    fade: original.fade,
                    transition: None,
                    speed: original.speed,
                    speed_curve: original.speed_curve,
                    reverse: original.reverse,
                    freeze_frame: original.freeze_frame,
                    frame_blending: original.frame_blending,
                    spatial_conform: original.spatial_conform,
                    gain_db: original.gain_db,
                    volume_keyframes: original.volume_keyframes,
                    mute_audio: original.mute_audio,
                    filters: original.filters,
                    keyframes: original.keyframes,
                    link_group_id: original.link_group_id,
                    generator: original.generator,
                };
                track.insert_clip(left)?;
                track.insert_clip(right)?;
            }
            EditCommand::AddTrack { kind, name, index } => {
                let idx = index.unwrap_or(self.project.timeline.tracks_of_kind(*kind).len());
                let track = Track::new(*kind, name.clone(), idx);
                self.project.timeline.add_track(track);
            }
            EditCommand::RemoveTrack { track_id } => {
                self.project.timeline.remove_track(*track_id);
            }
            EditCommand::SetClipSpeed { clip_id, speed } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.speed = *speed;
            }
            EditCommand::SetClipTransform { clip_id, transform } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.transform = transform.clone();
            }
            EditCommand::TrimClip {
                clip_id,
                new_source_range,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.source_in = new_source_range.start;
                clip.source_duration = new_source_range.end - new_source_range.start;
            }
            EditCommand::SlipClip { clip_id, delta } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.source_in = (clip.source_in + delta).max(0);
            }
            EditCommand::AddFilter { clip_id, filter } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.filters.push(filter.clone());
            }
            EditCommand::RemoveFilter { clip_id, filter_id } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.filters.retain(|f| f.id() != *filter_id);
            }
            EditCommand::SetFilterParam {
                clip_id,
                filter_id,
                param_name,
                value,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                if let Some(filter) = clip.filters.iter_mut().find(|f| f.id() == *filter_id) {
                    filter.set_param(param_name, value.clone());
                }
            }
            EditCommand::AddKeyframe { clip_id, keyframe } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.keyframes.retain(|k| k.id != keyframe.id);
                clip.keyframes.push(keyframe.clone());
            }
            EditCommand::RemoveKeyframe {
                clip_id,
                keyframe_id,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.keyframes.retain(|k| k.id != *keyframe_id);
            }
            EditCommand::MoveKeyframe {
                clip_id,
                keyframe_id,
                new_frame,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                if let Some(kf) = clip.keyframes.iter_mut().find(|k| k.id == *keyframe_id) {
                    kf.at_frame = *new_frame;
                }
            }
            EditCommand::AddMarker { label, frame } => {
                let marker = rook_core::marker::Marker::new(label.clone(), *frame);
                self.project.timeline.markers.push(marker);
            }
            EditCommand::RemoveMarker { marker_id } => {
                self.project.timeline.markers.retain(|m| m.id != *marker_id);
            }
            EditCommand::MoveMarker {
                marker_id,
                new_frame,
            } => {
                if let Some(m) = self
                    .project
                    .timeline
                    .markers
                    .iter_mut()
                    .find(|m| m.id == *marker_id)
                {
                    m.frame = *new_frame;
                }
            }
            EditCommand::ToggleClipMute { clip_id } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.mute_audio = !clip.mute_audio;
            }
            EditCommand::SetClipGain { clip_id, gain_db } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.gain_db = Some(*gain_db);
            }
            EditCommand::SetClipFade { clip_id, fade } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.fade = fade.clone();
            }
            EditCommand::SetClipBlendMode {
                clip_id,
                blend_mode,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.blend_mode = *blend_mode;
            }
            EditCommand::SetClipLabel { clip_id, label } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.label = label.clone();
            }
            EditCommand::SetClipTransition {
                clip_id,
                transition,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.transition = transition.clone();
            }
            EditCommand::SetClipReverse { clip_id, reverse } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.reverse = *reverse;
            }
            EditCommand::SetClipFreezeFrame {
                clip_id,
                freeze_frame,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.freeze_frame = *freeze_frame;
            }
            EditCommand::SetClipFrameBlending { clip_id, enabled } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.frame_blending = *enabled;
            }
            EditCommand::SetClipSpatialConform { clip_id, conform } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.spatial_conform = *conform;
            }
            EditCommand::NormalizeClipAudio {
                clip_id,
                target_peak_db,
            } => {
                // Normalize is handled in the UI layer with waveform data;
                // here we just mark it as processed. The UI computes the
                // gain adjustment and applies SetClipGain separately.
                let _ = (clip_id, target_peak_db);
                tracing::debug!(?clip_id, target_peak_db, "audio normalize (UI-handled)");
            }
            EditCommand::ToggleTrackMute { track_id } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.muted = !t.muted;
                }
            }
            EditCommand::ToggleTrackLock { track_id } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.locked = !t.locked;
                }
            }
            EditCommand::ToggleTrackVisibility { track_id } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.visible = !t.visible;
                }
            }
            EditCommand::ToggleTrackDisable { track_id } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.disabled = !t.disabled;
                }
            }
            EditCommand::SetTrackColor { track_id, color } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.color = *color;
                }
            }
            EditCommand::SetPrimaryTrack { track_id } => {
                // Clear primary on all tracks, then set on the target
                for track in &mut self.project.timeline.tracks {
                    track.is_primary = false;
                }
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.is_primary = true;
                }
            }
            EditCommand::RenameTrack { track_id, new_name } => {
                if let Some(t) = self.project.timeline.track_mut(*track_id) {
                    t.name = new_name.clone();
                }
            }
            EditCommand::MoveTrack {
                track_id,
                new_index,
            } => {
                if let Some(idx) = self.project.timeline.track_index(*track_id) {
                    if idx != *new_index && *new_index < self.project.timeline.tracks.len() {
                        let track = self.project.timeline.tracks.remove(idx);
                        self.project.timeline.tracks.insert(*new_index, track);
                    }
                }
            }
            EditCommand::AnnotateAsset {
                asset_id,
                description,
                labels,
            } => {
                if let Some(asset) = self.project.asset_mut(*asset_id) {
                    if let Some(desc) = description {
                        asset.metadata_mut().ai_description = Some(desc.clone());
                    }
                    asset.metadata_mut().ai_labels = labels.clone();
                }
            }
            EditCommand::AddSemanticClip { semantic_clip } => {
                self.project
                    .timeline
                    .semantic_clips
                    .push(semantic_clip.clone());
            }
            EditCommand::TagAsset { asset_id, tags } => {
                if let Some(asset) = self.project.asset_mut(*asset_id) {
                    asset.metadata_mut().ai_labels = tags.clone();
                }
            }
            EditCommand::RemoveAsset { asset_id } => {
                self.project.remove_asset(*asset_id).ok();
            }
            EditCommand::Batch { commands, .. } => {
                for sub in commands {
                    self.apply_to_model(sub)?;
                }
            }
            // ── Multicam ─────────────────────────────────────────────────
            EditCommand::CreateMulticam {
                clip_ids,
                label,
                sync_method,
                position,
                track_id,
            } => {
                let mut angles = Vec::new();
                for cid in clip_ids {
                    let track_id_for_clip = self.project.timeline.clip_track_id(*cid);
                    let clip = track_id_for_clip.and_then(|tid| {
                        self.project
                            .timeline
                            .track(tid)
                            .and_then(|t| t.clip(*cid).cloned())
                    });
                    if let Some(clip) = clip {
                        angles.push(rook_core::multicam::MulticamAngle {
                            id: rook_core::ids::AngleId::next(),
                            label: clip.label.clone(),
                            asset_id: clip.asset_id,
                            offset_frames: clip.timeline_in - position,
                            source_in: clip.source_in,
                            enabled: true,
                            gain_trim_db: clip.gain_db,
                        });
                    }
                }
                // Remove original clips from tracks
                for cid in clip_ids {
                    if let Some(tid) = self.project.timeline.clip_track_id(*cid) {
                        if let Some(track) = self.project.timeline.track_mut(tid) {
                            track.remove_clip(*cid);
                        }
                    }
                }
                // Create a wrapper clip
                let wrapper_id = ClipId::next();
                let first_asset = angles.first().map(|a| a.asset_id).unwrap_or_default();
                let wrapper_clip = rook_core::clip::Clip {
                    id: wrapper_id,
                    label: label.clone(),
                    asset_id: first_asset,
                    timeline_in: *position,
                    source_in: 0,
                    source_duration: 300, // will be set from angles
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
                if let Some(track) = self.project.timeline.track_mut(*track_id) {
                    track.insert_clip(wrapper_clip)?;
                }
                let mc = rook_core::multicam::MulticamClip::new(
                    wrapper_id,
                    label.clone(),
                    angles,
                    *sync_method,
                    *position,
                );
                self.project.add_multicam(mc);
            }
            EditCommand::SwitchMulticamAngle {
                clip_id,
                angle_index,
            } => {
                if let Some(mc) = self.project.multicam_for_clip_mut(*clip_id) {
                    mc.switch_to(*angle_index);
                }
            }
            EditCommand::AddMulticamAngle {
                clip_id,
                asset_id,
                label,
                offset_frames,
            } => {
                if let Some(mc) = self.project.multicam_for_clip_mut(*clip_id) {
                    let angle = rook_core::multicam::MulticamAngle {
                        id: rook_core::ids::AngleId::next(),
                        label: label.clone(),
                        asset_id: *asset_id,
                        offset_frames: *offset_frames,
                        source_in: 0,
                        enabled: true,
                        gain_trim_db: None,
                    };
                    mc.add_angle(angle);
                }
            }
            EditCommand::RemoveMulticamAngle { clip_id, angle_id } => {
                if let Some(mc) = self.project.multicam_for_clip_mut(*clip_id) {
                    mc.remove_angle(*angle_id);
                }
            }
            EditCommand::SetMulticamAudioPolicy { clip_id, policy } => {
                if let Some(mc) = self.project.multicam_for_clip_mut(*clip_id) {
                    mc.audio_policy = *policy;
                }
            }
            EditCommand::CollapseMulticam { clip_id } => {
                // Clone needed data before mutating
                let collapse_info = {
                    self.project.multicam_for_clip(*clip_id).and_then(|mc| {
                        mc.active_angle().map(|angle| {
                            (
                                angle.label.clone(),
                                angle.asset_id,
                                angle.source_in,
                                mc.clip_id,
                            )
                        })
                    })
                };
                if let Some((label, asset_id, source_in, mc_clip_id)) = collapse_info {
                    let tid = self.project.timeline.clip_track_id(mc_clip_id);
                    if let Some(tid) = tid {
                        if let Some(track) = self.project.timeline.track_mut(tid) {
                            if let Some(wrapper) = track.remove_clip(mc_clip_id) {
                                let plain_clip = rook_core::clip::Clip {
                                    id: ClipId::next(),
                                    label,
                                    asset_id,
                                    timeline_in: wrapper.timeline_in,
                                    source_in,
                                    source_duration: wrapper.source_duration,
                                    ..wrapper
                                };
                                let _ = track.insert_clip(plain_clip);
                            }
                        }
                    }
                }
                self.project.remove_multicam(*clip_id);
            }
            EditCommand::SetMulticamSeparateAudio {
                clip_id,
                audio_clip_id,
            } => {
                if let Some(mc) = self.project.multicam_for_clip_mut(*clip_id) {
                    mc.separate_audio_clip_id = *audio_clip_id;
                }
            }
            // ── Plugin commands ──────────────────────────────────────────
            EditCommand::LoadPlugin { manifest } => {
                self.project.add_plugin(manifest.clone());
            }
            EditCommand::UnloadPlugin { plugin_id } => {
                self.project.remove_plugin(*plugin_id);
            }
            EditCommand::ApplyPlugin { clip_id, plugin_id } => {
                let manifest = self.project.plugin(*plugin_id).cloned();
                if let Some(manifest) = manifest {
                    let clip = self
                        .project
                        .timeline
                        .clip_mut(*clip_id)
                        .ok_or(EngineError::UnknownClip(*clip_id))?;
                    let mut effect = rook_core::effect::EffectInstance::new(
                        rook_core::effect::EffectKind::Plugin(*plugin_id),
                    );
                    // Seed params with manifest defaults
                    effect.params = manifest.default_params();
                    clip.filters.push(effect);
                } else {
                    tracing::warn!(?plugin_id, "ApplyPlugin: plugin not found in project");
                }
            }
            EditCommand::RemovePlugin { clip_id, effect_id } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                clip.filters.retain(|f| f.id() != *effect_id);
            }
            EditCommand::SetPluginParam {
                clip_id,
                effect_id,
                param_name,
                value,
            } => {
                let clip = self
                    .project
                    .timeline
                    .clip_mut(*clip_id)
                    .ok_or(EngineError::UnknownClip(*clip_id))?;
                if let Some(effect) = clip.filters.iter_mut().find(|f| f.id() == *effect_id) {
                    effect.set_param(param_name, value.clone());
                }
            }
            EditCommand::RefreshPluginCache => {
                self.plugin_host.refresh_cache();
                // Sync newly discovered manifests into the project registry
                for m in self.plugin_host.discovered().to_vec() {
                    self.project.add_plugin(m);
                }
            }
            _ => {
                tracing::warn!("Unimplemented command: {:?}", cmd);
            }
        }
        Ok(())
    }

    /// Mirror a single edit command to the MLT engine.
    /// When MLT isn't live, gracefully no-ops.
    fn mirror_to_mlt(&mut self, cmd: &EditCommand) -> Result<(), EngineError> {
        if !self.mlt_live {
            return Ok(());
        }

        let profile = self.mlt_profile.as_ref().unwrap();

        match cmd {
            EditCommand::InsertClip {
                asset_id,
                track_id,
                position,
                source_in,
                source_out,
                ..
            } => {
                let asset_path = self
                    .project
                    .asset(*asset_id)
                    .map(|a| std::path::PathBuf::from(a.path()));
                if let Some(ref path) = asset_path {
                    if let Ok(producer) = rook_mlt::producer::Producer::from_file(profile, path) {
                        if let Some(playlist) = self.mlt_playlists.get(track_id) {
                            let idx = self
                                .project
                                .timeline
                                .track(*track_id)
                                .map(|t| {
                                    t.clips.iter().filter(|c| c.timeline_in < *position).count()
                                        as i32
                                })
                                .unwrap_or(playlist.count());
                            let _ = playlist.insert_at(idx, &producer, *source_in, *source_out);
                        }
                    }
                }
            }
            EditCommand::RemoveClip { clip_id } => {
                let tid = self.project.timeline.clip_track_id(*clip_id);
                if let Some(tid) = tid {
                    if let Some(playlist) = self.mlt_playlists.get(&tid) {
                        if let Some(track) = self.project.timeline.track(tid) {
                            if let Some(idx) = track.clip_index(*clip_id) {
                                let _ = playlist.remove_at(idx as i32);
                            }
                        }
                    }
                }
                self.mlt_producers.remove(clip_id);
            }
            EditCommand::RippleDelete { clip_id } => {
                // Remove + rebuild the track
                let tid = self.project.timeline.clip_track_id(*clip_id);
                if let Some(tid) = tid {
                    self.rebuild_one_track(tid)?;
                }
            }
            EditCommand::MoveClip { .. }
            | EditCommand::AddTrack { .. }
            | EditCommand::RemoveTrack { .. }
            | EditCommand::SplitClip { .. }
            | EditCommand::TrimClip { .. } => {
                // Rebuild relevant tracks
                self.rebuild_all_tracks()?;
            }
            EditCommand::AddFilter { clip_id, filter } => {
                if let Some(producer) = self.mlt_producers.get(clip_id) {
                    tracing::debug!(
                        ?filter,
                        "MLT filter attach — not yet mapped to MLT filter types"
                    );
                }
            }
            _ => {
                tracing::debug!(?cmd, "MLT mirror: unhandled, rebuilding all tracks");
                self.rebuild_all_tracks()?;
            }
        }
        Ok(())
    }

    fn rebuild_one_track(&mut self, track_id: TrackId) -> Result<(), EngineError> {
        if !self.mlt_live {
            return Ok(());
        }
        let profile = self.mlt_profile.as_ref().unwrap();
        let tractor = self.mlt_tractor.as_ref().unwrap();

        self.mlt_playlists.remove(&track_id);
        if let Some(track) = self.project.timeline.track(track_id) {
            let clip_ids: Vec<_> = track.clips.iter().map(|c| c.id).collect();
            for cid in &clip_ids {
                self.mlt_producers.remove(cid);
            }

            if let Ok(playlist) = rook_mlt::playlist::Playlist::new(profile) {
                for clip in &track.clips {
                    let path = self
                        .project
                        .asset(clip.asset_id)
                        .map(|a| std::path::PathBuf::from(a.path()));
                    if let Some(ref p) = path {
                        if let Ok(prod) = rook_mlt::producer::Producer::from_file(profile, p) {
                            let _ = playlist.insert_at(
                                playlist.count(),
                                &prod,
                                clip.source_in,
                                clip.source_in + clip.source_duration,
                            );
                            self.mlt_producers.insert(clip.id, prod);
                        }
                    }
                }
                let ti = self.project.timeline.track_index(track_id).unwrap_or(0);
                tractor.connect(&playlist, ti as i32);
                self.mlt_playlists.insert(track_id, playlist);
            }
        }
        Ok(())
    }

    fn rebuild_all_tracks(&mut self) -> Result<(), EngineError> {
        if !self.mlt_live {
            return Ok(());
        }
        let profile = self.mlt_profile.as_ref().unwrap();
        let tractor = self.mlt_tractor.as_ref().unwrap();
        self.mlt_playlists.clear();
        self.mlt_producers.clear();

        for (ti, track) in self.project.timeline.tracks.iter().enumerate() {
            if let Ok(playlist) = rook_mlt::playlist::Playlist::new(profile) {
                for clip in &track.clips {
                    let path = self
                        .project
                        .asset(clip.asset_id)
                        .map(|a| std::path::PathBuf::from(a.path()));
                    if let Some(ref p) = path {
                        if let Ok(prod) = rook_mlt::producer::Producer::from_file(profile, p) {
                            let _ = playlist.insert_at(
                                playlist.count(),
                                &prod,
                                clip.source_in,
                                clip.source_in + clip.source_duration,
                            );
                            self.mlt_producers.insert(clip.id, prod);
                        }
                    }
                }
                tractor.connect(&playlist, ti as i32);
                self.mlt_playlists.insert(track.id, playlist);
            }
        }
        Ok(())
    }

    // ── TimelineGraph sync ──────────────────────────────────────────────

    /// Rebuild the graph from the current project state.
    /// Called after every mutation so the graph stays in sync.
    pub fn sync_to_graph(&mut self) {
        self.graph = build_graph_from_project(&self.project);
    }

    // ── Undo / Redo ─────────────────────────────────────────────────────

    pub fn undo(&mut self) -> Option<&Project> {
        let current = self.project.clone();
        if let Some(prev) = self.history.undo(current) {
            self.project = prev;
            self.sync_to_graph();
            Some(&self.project)
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<&Project> {
        let current = self.project.clone();
        if let Some(next) = self.history.redo(current) {
            self.project = next;
            self.sync_to_graph();
            Some(&self.project)
        } else {
            None
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    pub fn undo_label(&self) -> Option<String> {
        self.history.undo_label().map(|s| s.to_string())
    }
    pub fn redo_label(&self) -> Option<String> {
        self.history.redo_label().map(|s| s.to_string())
    }
    /// Access the edit history for UI display (undo/redo labels).
    pub fn history(&self) -> &EditHistory {
        &self.history
    }

    // ── Project Snapshots ───────────────────────────────────────────────

    /// Save a named snapshot of the current project state.
    pub fn save_snapshot(&mut self, name: String) {
        self.snapshots.retain(|(n, _)| n != &name);
        self.snapshots.push((name, self.project.clone()));
        if self.snapshots.len() > 20 {
            self.snapshots.remove(0);
        }
        tracing::info!(snapshot = %self.snapshots.last().unwrap().0, "snapshot saved");
    }

    /// Restore a named snapshot.
    pub fn restore_snapshot(&mut self, name: &str) -> bool {
        if let Some(idx) = self.snapshots.iter().position(|(n, _)| n == name) {
            let (_, project) = self.snapshots.remove(idx);
            self.project = project;
            self.sync_to_graph();
            tracing::info!(%name, "snapshot restored");
            true
        } else {
            false
        }
    }

    /// List available snapshot names.
    pub fn snapshot_names(&self) -> Vec<String> {
        self.snapshots.iter().map(|(n, _)| n.clone()).collect()
    }

    // ── Frame resolution ────────────────────────────────────────────────

    pub fn frame_at(&self, frame: i64) -> Vec<RenderedLayer> {
        resolve_frame(&self.project, &self.pool, frame)
    }

    // ── Import media ────────────────────────────────────────────────────

    pub fn import_media(&mut self, path: &std::path::Path) -> Result<AssetId, EngineError> {
        let t0 = std::time::Instant::now();
        let asset = crate::pool::probe_asset(path)?;
        eprintln!("[import_media] probe_asset took {:?}", t0.elapsed());
        let id = asset.id();
        let t1 = std::time::Instant::now();
        self.project.add_asset(asset);
        self.pool.open(id, path)?;
        eprintln!("[import_media] pool.open + add_asset took {:?}", t1.elapsed());
        let t2 = std::time::Instant::now();
        self.proxy.request_proxy(id, path);
        eprintln!("[import_media] request_proxy took {:?}", t2.elapsed());
        Ok(id)
    }

    pub fn import_media_multi(
        &mut self,
        paths: &[std::path::PathBuf],
    ) -> Vec<Result<AssetId, EngineError>> {
        paths.iter().map(|p| self.import_media(p)).collect()
    }

    // ── Save / Load ─────────────────────────────────────────────────────

    /// Save the project to SQLite (or JSON fallback).
    ///
    /// If a database has been initialised, stores project settings + timeline
    /// as JSON blobs in the SQLite database.  Otherwise falls back to a
    /// plain JSON file at `path`.
    pub fn save_project(&mut self, path: Option<&std::path::Path>) -> Result<PathBuf, EngineError> {
        // If we have a database, use it
        if let Some(ref db) = self.db {
            let project_id = self.project.id.to_string();
            let settings =
                serde_json::to_value(&self.project).map_err(|e| EngineError::Serialization(e))?;
            db.update_project_settings_json(&project_id, &settings)
                .map_err(|e| EngineError::Generic("db save failed"))?;
            // Also store the graph as timeline JSON
            let graph_json =
                serde_json::to_string(&self.graph).map_err(|e| EngineError::Serialization(e))?;
            db.upsert_project_timeline_json(&project_id, &graph_json)
                .map_err(|e| EngineError::Generic("db timeline save failed"))?;
            tracing::info!(path = %db.path().display(), "project saved to SQLite");
            return Ok(db.path().to_path_buf());
        }

        // JSON fallback
        let save_path = match path.or(self.current_path.as_deref()) {
            Some(p) => p.to_path_buf(),
            None => {
                return Err(EngineError::Generic(
                    "No save path specified — call init_db() first or provide a path",
                ));
            }
        };
        let json = serde_json::to_string_pretty(&self.project)
            .map_err(|e| EngineError::Serialization(e))?;
        std::fs::write(&save_path, json)?;
        self.current_path = Some(save_path.clone());
        tracing::info!(path = %save_path.display(), "project saved to JSON");
        Ok(save_path)
    }

    /// Open a project from a SQLite database.
    pub fn open_project(path: &std::path::Path) -> Result<Self, EngineError> {
        // Detect JSON files by reading the first byte — fall back to legacy JSON loader.
        let is_json = std::fs::read(path)
            .ok()
            .and_then(|b| b.first().copied())
            .map(|b| b == b'{' || b == b'[')
            .unwrap_or(false);
        if is_json {
            return Self::open_project_json(path);
        }

        let db = rook_project_db::ProjectDb::open_or_create(path)
            .map_err(|e| EngineError::Generic("failed to open database"))?;

        // Try to load project from SQLite
        let projects = db
            .list_projects()
            .map_err(|e| EngineError::Generic("db list failed"))?;

        let (project, graph) = if let Some(info) = projects.first() {
            let settings = db
                .get_project_settings_json(&info.id)
                .map_err(|e| EngineError::Generic("db settings read failed"))?;
            let project: Project =
                serde_json::from_value(settings).map_err(|e| EngineError::Serialization(e))?;

            let graph = db
                .get_project_timeline_json(&info.id)
                .ok()
                .flatten()
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_else(|| build_graph_from_project(&project));

            (project, graph)
        } else {
            // Empty database — create new project
            let project = Project::new("Untitled", Canvas::default(), Rational::FPS_24);
            let graph = rook_timeline::TimelineGraph::default();
            (project, graph)
        };

        let mut engine = Self {
            project,
            graph,
            pool: MediaPool::new(PoolConfig::default()),
            history: EditHistory::new(DEFAULT_HISTORY_LIMIT),
            proxy: ProxyService::new(),
            db: Some(db),
            current_path: Some(path.to_path_buf()),
            mlt_live: false,
            mlt_profile: None,
            mlt_tractor: None,
            mlt_playlists: HashMap::new(),
            mlt_producers: HashMap::new(),
            snapshots: Vec::new(),
            plugin_host: PluginHost::new(),
        };
        engine.project.timeline.rebuild_index();
        engine.clamp_loaded_playhead();
        engine.repair_loaded_project();
        Ok(engine)
    }

    /// Open a project from a JSON file (legacy format).
    pub fn open_project_json(path: &std::path::Path) -> Result<Self, EngineError> {
        let json = std::fs::read_to_string(path)?;
        let project: Project =
            serde_json::from_str(&json).map_err(|e| EngineError::Serialization(e))?;
        let graph = build_graph_from_project(&project);
        let mut engine = Self {
            project,
            graph,
            pool: MediaPool::new(PoolConfig::default()),
            history: EditHistory::new(DEFAULT_HISTORY_LIMIT),
            proxy: ProxyService::new(),
            db: None,
            current_path: Some(path.to_path_buf()),
            mlt_live: false,
            mlt_profile: None,
            mlt_tractor: None,
            mlt_playlists: HashMap::new(),
            mlt_producers: HashMap::new(),
            snapshots: Vec::new(),
            plugin_host: PluginHost::new(),
        };
        engine.project.timeline.rebuild_index();
        engine.clamp_loaded_playhead();
        engine.repair_loaded_project();
        Ok(engine)
    }

    /// After loading a project from disk, ensure audio clips exist for any assets
    /// that have audio but no corresponding audio clip on an audio track.
    ///
    /// This repairs projects saved before audio-clip auto-creation was added,
    /// and projects where audio tracks were manually added without clips.
    fn repair_loaded_project(&mut self) {
        use rook_core::clip::Clip;
        use rook_core::ids::TrackId;
        use rook_core::track::TrackKind;
        use std::collections::HashSet;

        // ── Fix duplicate track IDs ────────────────────────────────────
        // If two tracks share the same ID, reassign the second one so
        // track lookups work correctly.
        {
            let mut seen: HashSet<TrackId> = HashSet::new();
            for track in &mut self.project.timeline.tracks {
                if !seen.insert(track.id) {
                    let old_id = track.id;
                    track.id = TrackId::next();
                    eprintln!(
                        "[repair] reassigned duplicate track ID {:?} → {:?} ({})",
                        old_id, track.id, track.name
                    );
                }
            }
        }

        let mut repairs: Vec<(AssetId, i64, i64, Option<u64>)> = Vec::new();

        // Find assets that have audio but no audio clip
        for asset in &self.project.assets {
            let asset_id = asset.id();
            let dur = asset.metadata().duration_frames.unwrap_or(300);
            let has_audio = match asset {
                rook_core::asset::Asset::Video(v) => {
                    v.metadata.video.as_ref().map(|vm| vm.has_audio).unwrap_or(false)
                        || v.metadata.audio.is_some()
                }
                rook_core::asset::Asset::Audio(_) => true,
                _ => false,
            };

            if !has_audio {
                continue;
            }

            // Check if this asset already has a clip on an audio track
            let already_has_audio_clip = self
                .project
                .timeline
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Audio)
                .any(|t| t.clips.iter().any(|c| c.asset_id == asset_id));

            if already_has_audio_clip {
                continue;
            }

            // Find matching video clip for link_group_id + position
            let mut link_group: Option<u64> = None;
            let mut position: i64 = self.project.timeline.duration();

            for track in &self.project.timeline.tracks {
                if track.kind == TrackKind::Video {
                    for clip in &track.clips {
                        if clip.asset_id == asset_id {
                            link_group = clip.link_group_id;
                            position = clip.timeline_in;
                            break;
                        }
                    }
                }
            }

            repairs.push((asset_id, position, dur, link_group));
        }

        if repairs.is_empty() {
            return;
        }

        eprintln!(
            "[repair] found {} assets with audio but no audio clip — auto-creating",
            repairs.len()
        );

        // Create audio track if needed
        if self
            .project
            .timeline
            .tracks_of_kind(TrackKind::Audio)
            .is_empty()
        {
            self.project_mut().add_audio_track("A1".to_string());
            eprintln!("[repair] created audio track A1");
        }

        // Get or create audio track
        let audio_track_id = self
            .project
            .timeline
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Audio)
            .map(|t| t.id);

        if let Some(atid) = audio_track_id {
            for (asset_id, position, dur, link_group) in &repairs {
                // Skip if the asset has no duration
                if *dur <= 0 {
                    continue;
                }

                // Create and insert clip directly (not via EditCommand to avoid history pollution)
                let label = self
                    .project
                    .asset(*asset_id)
                    .map(|a| a.filename_stem().to_string())
                    .unwrap_or_else(|| "audio".to_string());

                let clip = Clip {
                    id: ClipId::next(),
                    label,
                    asset_id: *asset_id,
                    timeline_in: *position,
                    source_in: 0,
                    source_duration: *dur,
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
                    link_group_id: *link_group,
                    generator: None,
                };

                if let Some(track) = self.project.timeline.track_mut(atid) {
                    match track.insert_clip(clip) {
                        Ok(()) => {
                            eprintln!(
                                "[repair] inserted audio clip for asset {} on track {:?} at pos {} dur {}",
                                asset_id.0, atid, position, dur
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "[repair] failed to insert audio clip for asset {}: {e}",
                                asset_id.0
                            );
                        }
                    }
                }
            }

            self.project.timeline.rebuild_index();
        }

        // Sync graph to include repaired clips
        if !repairs.is_empty() {
            self.sync_to_graph();
        }
    }

    /// Keep a loaded project playhead inside the valid frame range.
    ///
    /// Saved projects can land exactly on `duration()`, which is the first
    /// frame outside the half-open clip ranges used by `Clip::covers()`.
    /// That makes the preview look black and playback appear stuck.
    fn clamp_loaded_playhead(&mut self) {
        let max_frame = self.project.timeline.duration().saturating_sub(1).max(0);
        self.project.timeline.playhead = self.project.timeline.playhead.clamp(0, max_frame);
    }

    /// Relink an asset to a new file path.
    pub fn relink_asset(
        &mut self,
        asset_id: AssetId,
        new_path: &std::path::Path,
    ) -> Result<(), EngineError> {
        let asset = self
            .project
            .asset_mut(asset_id)
            .ok_or(EngineError::Generic("asset not found"))?;
        let old_path = asset.path().to_string();
        asset.set_path(new_path.to_string_lossy().to_string());
        // Re-open in media pool
        self.pool.close(asset_id);
        self.pool.open(asset_id, new_path)?;
        tracing::info!(%old_path, new_path = %new_path.display(), "asset relinked");
        Ok(())
    }

    /// Consolidate project: copy all referenced media files into a
    /// "Media" subdirectory next to the project file, and update asset paths.
    /// Returns the number of files copied.
    pub fn consolidate_project(&mut self) -> Result<usize, EngineError> {
        let project_dir = match self.current_path.as_ref().and_then(|p| p.parent()) {
            Some(dir) => dir.to_path_buf(),
            None => {
                return Err(EngineError::Generic(
                    "Save project first before consolidating",
                ));
            }
        };
        let media_dir = project_dir.join("Media");
        std::fs::create_dir_all(&media_dir).map_err(|e| EngineError::Io(e))?;

        let mut copied = 0usize;
        // Collect asset info first to avoid borrow conflict
        let asset_info: Vec<_> = self
            .project
            .assets
            .iter()
            .map(|a| (a.id(), std::path::PathBuf::from(a.path())))
            .collect();

        for (asset_id, src) in &asset_info {
            if !src.exists() {
                tracing::warn!(path = %src.display(), "consolidate: source file missing, skipping");
                continue;
            }
            let fname = src
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));
            let dst = media_dir.join(fname);

            // Skip if already in the media directory
            if src.starts_with(&media_dir) {
                continue;
            }

            // Copy if not already there
            if !dst.exists() {
                std::fs::copy(src, &dst).map_err(|e| EngineError::Io(e))?;
                copied += 1;
            }

            // Update asset path
            if let Some(asset_mut) = self.project.asset_mut(*asset_id) {
                asset_mut.set_path(dst.to_string_lossy().to_string());
            }
        }

        if copied > 0 {
            // Save the project with updated paths
            self.save_project(None)?;
        }

        tracing::info!(copied, dir = %media_dir.display(), "project consolidated");
        Ok(copied)
    }

    /// Find gaps on a track (for query.find_gaps).
    pub fn find_gaps(&self, track_id: TrackId) -> Vec<(i64, i64)> {
        self.project.timeline.find_gaps(track_id)
    }

    // ── Crash recovery ─────────────────────────────────────────────────

    /// Check whether the last session crashed, and if so return the
    /// crash report and the path to the auto-saved project for recovery.
    pub fn check_crash_recovery(&self) -> Option<crate::crash::CrashReport> {
        if !crate::crash::has_pending_crash() {
            return None;
        }
        crate::crash::recover_last_crash()
    }

    /// Clear all pending crash reports (call after successful recovery).
    pub fn clear_crash_recovery() {
        crate::crash::clear_crash_reports();
    }

    /// Search clips by query string (simple substring match on label + AI description).
    pub fn search_clips(&self, query: &str, min_duration: Option<i64>) -> Vec<ClipMatch> {
        let mut results = Vec::new();
        let q = query.to_lowercase();
        for track in &self.project.timeline.tracks {
            for clip in &track.clips {
                if let Some(min_dur) = min_duration {
                    if clip.duration() < min_dur {
                        continue;
                    }
                }
                let asset = self.project.asset(clip.asset_id);
                let haystack = format!(
                    "{} {} {}",
                    clip.label.to_lowercase(),
                    asset
                        .and_then(|a| a.metadata().ai_description.as_deref())
                        .unwrap_or(""),
                    asset
                        .map(|a| a.metadata().ai_labels.join(" "))
                        .unwrap_or_default(),
                );
                if q.is_empty() || haystack.contains(&q) {
                    results.push(ClipMatch {
                        clip_id: clip.id,
                        label: clip.label.clone(),
                        asset_id: clip.asset_id,
                    });
                }
            }
        }
        results
    }

    // ── Export ──────────────────────────────────────────────────────────

    /// Export the project to a video file with progress callback.
    ///
    /// `preset` is one of: "h264", "h265", "prores", or a raw MLT format string
    /// like "avformat-x264" etc.
    /// `on_progress` is called with (percent_complete, eta_seconds).
    pub fn export_with_progress(
        &self,
        output_path: &std::path::Path,
        preset: &str,
        mut on_progress: impl FnMut(f32, f64),
    ) -> Result<(), EngineError> {
        if !self.mlt_live {
            return Err(EngineError::Generic("MLT not initialized — cannot export"));
        }

        let format = Self::resolve_export_format(preset);
        let profile = self.mlt_profile.as_ref().unwrap();
        let consumer = rook_mlt::consumer::Consumer::new(
            profile,
            rook_mlt::consumer::ConsumerKind::Avformat {
                path: output_path.to_path_buf(),
                format: format.clone(),
            },
        )?;

        let tractor = self.mlt_tractor.as_ref().unwrap();
        consumer.connect(tractor.as_service_ptr() as *mut _)?;
        consumer.start()?;

        // Get total duration for progress calculation
        let total_duration = self.project.timeline.duration();
        let total_seconds = if total_duration > 0 {
            total_duration as f64 / self.project.frame_rate.as_f64()
        } else {
            10.0 // fallback
        };

        let start = std::time::Instant::now();
        while !consumer.is_stopped() {
            let elapsed = start.elapsed().as_secs_f64();
            let estimated_progress = (elapsed / total_seconds).min(0.99);
            let eta = if estimated_progress > 0.0 {
                (elapsed / estimated_progress - elapsed).max(0.0)
            } else {
                total_seconds
            };
            on_progress(estimated_progress as f32 * 100.0, eta);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        on_progress(100.0, 0.0);
        Ok(())
    }

    /// Simple export without progress.
    pub fn export(&self, output_path: &std::path::Path, format: &str) -> Result<(), EngineError> {
        self.export_with_progress(output_path, format, |_, _| {})
    }

    /// Resolve a user-friendly preset name to an MLT format string.
    fn resolve_export_format(preset: &str) -> String {
        match preset.to_lowercase().as_str() {
            "h264" | "h.264" | "x264" => "avformat-x264".to_string(),
            "h265" | "h.265" | "hevc" | "x265" => "avformat-x265".to_string(),
            "prores" | "prores_ks" | "prores422" => "avformat-prores_ks".to_string(),
            "prores_4444" | "prores4444" => "avformat-prores_ks".to_string(), // ProRes 4444 with alpha
            "vp9" | "webm" => "avformat-libvpx-vp9".to_string(),
            // Pass through raw format strings
            other => other.to_string(),
        }
    }

    /// Return the total frame count of the project timeline.
    pub fn total_frames(&self) -> i64 {
        self.project.timeline.duration()
    }

    /// Build a TimelineSnapshot — the full agent-friendly view of the project.
    /// This is the primary data structure AI agents receive to understand the edit.
    pub fn build_timeline_snapshot(&self) -> rook_core::snapshot::TimelineSnapshot {
        let project = &self.project;
        let fps = project.frame_rate.as_f64();
        let duration_ms = if project.timeline.duration() > 0 {
            project.timeline.duration() as f64 / fps * 1000.0
        } else {
            0.0
        };

        let frame_to_ms = |frame: i64| -> f64 { frame as f64 / fps * 1000.0 };

        let clip_to_view = |clip: &rook_core::clip::Clip| -> rook_core::snapshot::TimelineClipView {
            let media_dur = project
                .asset(clip.asset_id)
                .and_then(|a| a.metadata().duration_frames)
                .map(|f| f as f64 / fps * 1000.0);
            let file_path = project.asset(clip.asset_id).map(|a| a.path().to_string());
            let effects: Vec<String> = clip
                .filters
                .iter()
                .map(|f| format!("{:?}", f.kind))
                .collect();

            rook_core::snapshot::TimelineClipView {
                clip_id: format!("{}", clip.id.0),
                label: clip.label.clone(),
                file_path,
                start_ms: frame_to_ms(clip.timeline_in),
                duration_ms: frame_to_ms(clip.duration()),
                source_in_ms: frame_to_ms(clip.source_in),
                media_duration_ms: media_dur,
                muted: clip.mute_audio,
                gain_db: clip.gain_db,
                link_group_id: clip.link_group_id,
                effects,
            }
        };

        let track_to_view =
            |track: &rook_core::track::Track| -> rook_core::snapshot::TimelineTrackView {
                rook_core::snapshot::TimelineTrackView {
                    track_name: track.name.clone(),
                    track_kind: format!("{:?}", track.kind),
                    muted: track.muted,
                    locked: track.locked,
                    visible: track.visible,
                    clips: track.clips.iter().map(clip_to_view).collect(),
                }
            };

        let mut v1 = Vec::new();
        let mut video_tracks = Vec::new();
        let mut audio_tracks = Vec::new();

        for (i, track) in project.timeline.tracks.iter().enumerate() {
            let view = track_to_view(track);
            match track.kind {
                rook_core::track::TrackKind::Video => {
                    if i == 0 {
                        v1 = view.clips.clone();
                    }
                    video_tracks.push(view);
                }
                rook_core::track::TrackKind::Audio => {
                    audio_tracks.push(view);
                }
                _ => {} // Text/Effect tracks not yet handled
            }
        }

        // Semantic clips — placeholder (populated by AI annotation pipeline)
        let semantic_clips: Vec<rook_core::snapshot::TimelineSemanticView> = Vec::new();

        let markers: Vec<rook_core::snapshot::TimelineMarkerView> = project
            .timeline
            .markers
            .iter()
            .map(|m| rook_core::snapshot::TimelineMarkerView {
                label: m.label.clone(),
                frame: m.frame,
                time_ms: frame_to_ms(m.frame),
            })
            .collect();

        let mut link_groups: std::collections::HashMap<u64, Vec<String>> =
            std::collections::HashMap::new();
        for track in &project.timeline.tracks {
            for clip in &track.clips {
                if let Some(gid) = clip.link_group_id {
                    link_groups
                        .entry(gid)
                        .or_default()
                        .push(format!("{}", clip.id.0));
                }
            }
        }
        let link_groups: Vec<rook_core::snapshot::LinkGroupView> = link_groups
            .into_iter()
            .map(|(gid, cids)| rook_core::snapshot::LinkGroupView {
                group_id: gid,
                clip_ids: cids,
            })
            .collect();

        rook_core::snapshot::TimelineSnapshot {
            fps_num: project.frame_rate.num as i32,
            fps_den: project.frame_rate.den as i32,
            duration_ms,
            canvas_width: project.canvas.width,
            canvas_height: project.canvas.height,
            v1,
            video_tracks,
            audio_tracks,
            subtitle_tracks: vec![],
            semantic_clips,
            link_groups,
            markers,
        }
    }

    /// Get the project snapshot as a JSON string (for IPC/methods).
    pub fn snapshot_json(&self) -> Result<String, EngineError> {
        let snap = self.build_timeline_snapshot();
        serde_json::to_string(&snap).map_err(|_| EngineError::Generic("serialization error"))
    }

    // ── Proxy status ────────────────────────────────────────────────────

    pub fn proxy_status(&self, asset_id: AssetId) -> Option<crate::proxy::ProxyStatus> {
        self.proxy.status(asset_id)
    }
}

/// A clip match from semantic search.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClipMatch {
    pub clip_id: ClipId,
    pub label: String,
    pub asset_id: AssetId,
}

/// Build a `TimelineGraph` from a flat `Project` timeline.
/// Maps rook_core's Track/Clip model onto rook_timeline's node/edge graph.
pub fn build_graph_from_project(project: &Project) -> rook_timeline::TimelineGraph {
    use rook_timeline::{
        ClipNode, EdgeKind, FrameRange, NodeId, TimelineEdge, TimelineNode, TimelineNodeKind,
        TrackBinding, TrackKind as GraphTrackKind,
    };

    let mut graph = rook_timeline::TimelineGraph::default();
    graph.version = 1;

    for track in &project.timeline.tracks {
        let track_id = rook_timeline::TrackId::new();
        let kind = match track.kind {
            TrackKind::Video => GraphTrackKind::Video,
            TrackKind::Audio => GraphTrackKind::Audio,
            TrackKind::Text | TrackKind::Effect => {
                GraphTrackKind::Custom(format!("{:?}", track.kind))
            }
        };
        let mut binding = TrackBinding {
            id: track_id,
            name: track.name.clone(),
            kind,
            node_ids: Vec::new(),
        };

        let mut prev_node_id: Option<NodeId> = None;

        for clip in &track.clips {
            let node_id = NodeId::new();
            let clip_node = ClipNode {
                asset_id: Some(clip.asset_id.to_string()),
                media_range: FrameRange::new(clip.source_in, clip.source_duration),
                timeline_range: FrameRange::new(clip.timeline_in, clip.duration()),
                playback_rate: clip.speed as f32,
                reverse: clip.reverse,
                metadata: serde_json::json!({
                    "label": clip.label,
                    "link_group_id": clip.link_group_id,
                    "freeze_frame": clip.freeze_frame,
                }),
            };
            let node = TimelineNode {
                id: node_id,
                label: Some(clip.label.clone()),
                kind: TimelineNodeKind::Clip(clip_node),
                locked: false,
                metadata: serde_json::Value::Null,
            };
            graph.nodes.insert(node_id, node);
            binding.node_ids.push(node_id);

            // Add sequential edge from previous node
            if let Some(prev) = prev_node_id {
                graph.edges.push(TimelineEdge {
                    from: prev,
                    to: node_id,
                    kind: EdgeKind::Sequential,
                });
            }
            prev_node_id = Some(node_id);
        }

        graph.tracks.push(binding);
    }

    graph
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
