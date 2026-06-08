//! Edit commands — the closed set of structured, undoable mutations.
//! From cutlass-engines `EditCommand`, expanded with verbreel verb set.

use serde::{Deserialize, Serialize};

use crate::asset::AssetId;
use crate::clip::ClipId;
use crate::effect::EffectInstance;
use crate::ids::{TrackId, AngleId, PluginId};
use crate::plugin::PluginManifest;
use crate::time::TimeRange;
use crate::track::TrackKind;

/// Every mutation to a [`Project`](crate::Project) goes through an
/// [`EditCommand`].  Commands are serializable, deterministic, and
/// undoable — a UI turns gestures into commands; an AI agent turns
/// a natural-language prompt into the same commands.
///
/// Every variant includes all the data needed to apply *and* invert
/// the edit, so undo is simply: pop the command, apply its inverse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum EditCommand {
    // ── Clip operations ─────────────────────────────────────────────────

    /// Place a trimmed range of media onto a track.
    InsertClip {
        asset_id: AssetId,
        track_id: TrackId,
        position: i64,          // timeline frame
        source_in: i64,         // source in-point
        source_out: i64,        // source out-point
        /// Optional link group for audio/video sync editing.
        /// Clips sharing the same link_group_id are treated as a single unit.
        #[serde(default)]
        link_group_id: Option<u64>,
    },
    /// Remove a clip, leaving a gap.
    RemoveClip {
        clip_id: ClipId,
    },
    /// Remove a clip and slide later clips left to close the gap.
    RippleDelete {
        clip_id: ClipId,
    },
    /// Move a clip to a different track or position.
    MoveClip {
        clip_id: ClipId,
        new_track_id: TrackId,
        new_position: i64,
    },
    /// Change a clip's source in/out points (trim).
    TrimClip {
        clip_id: ClipId,
        new_source_range: TimeRange,
    },
    /// Slip the source range without moving the clip on the timeline.
    SlipClip {
        clip_id: ClipId,
        delta: i64,             // frames to slip source_in by
    },
    /// Split a clip at `frame` into two abutting clips.
    SplitClip {
        clip_id: ClipId,
        at_frame: i64,
    },
    /// Set clip speed.
    SetClipSpeed {
        clip_id: ClipId,
        speed: f64,
    },
    /// Set the transform on a clip.
    SetClipTransform {
        clip_id: ClipId,
        transform: crate::transform::Transform,
    },
    /// Toggle mute on a clip.
    ToggleClipMute {
        clip_id: ClipId,
    },
    /// Set clip gain in dB.
    SetClipGain {
        clip_id: ClipId,
        gain_db: f32,
    },
    /// Set fade-in / fade-out on a clip.
    SetClipFade {
        clip_id: ClipId,
        fade: Option<crate::clip::Fade>,
    },
    /// Set the clip blend mode.
    SetClipBlendMode {
        clip_id: ClipId,
        blend_mode: crate::clip::BlendMode,
    },
    /// Set the clip label.
    SetClipLabel {
        clip_id: ClipId,
        label: String,
    },
    /// Set a transition on a clip.
    SetClipTransition {
        clip_id: ClipId,
        transition: Option<crate::clip::Transition>,
    },
    /// Reverse the clip playback direction.
    SetClipReverse {
        clip_id: ClipId,
        reverse: bool,
    },
    /// Freeze the clip on a specific source frame.
    SetClipFreezeFrame {
        clip_id: ClipId,
        /// Source frame offset (relative to source_in) to freeze on.
        /// None = remove freeze.
        freeze_frame: Option<i64>,
    },
    /// Toggle frame blending for smooth speed changes.
    SetClipFrameBlending {
        clip_id: ClipId,
        enabled: bool,
    },
    /// Set spatial conform mode for a clip.
    SetClipSpatialConform {
        clip_id: ClipId,
        conform: Option<crate::clip::SpatialConform>,
    },
    /// Normalize audio gain — compute peak and adjust to target dB.
    NormalizeClipAudio {
        clip_id: ClipId,
        target_peak_db: f32,
    },

    // ── Track operations ────────────────────────────────────────────────

    /// Add a new track.
    AddTrack {
        kind: TrackKind,
        name: String,
        /// Insert at this index (defaults to end).
        index: Option<usize>,
    },
    /// Remove a track and all its clips.
    RemoveTrack {
        track_id: TrackId,
    },
    /// Rename a track.
    RenameTrack {
        track_id: TrackId,
        new_name: String,
    },
    /// Move a track to a new index.
    MoveTrack {
        track_id: TrackId,
        new_index: usize,
    },
    /// Toggle track state.
    ToggleTrackMute { track_id: TrackId },
    ToggleTrackLock { track_id: TrackId },
    ToggleTrackVisibility { track_id: TrackId },
    /// Toggle track disable (exclude from export).
    ToggleTrackDisable { track_id: TrackId },
    /// Set track color label.
    SetTrackColor {
        track_id: TrackId,
        color: Option<crate::track::TrackColor>,
    },
    /// Set primary storyline track.
    SetPrimaryTrack {
        track_id: TrackId,
    },

    // ── Effect operations ───────────────────────────────────────────────

    /// Add a filter to a clip.
    AddFilter {
        clip_id: ClipId,
        filter: EffectInstance,
    },
    /// Remove a filter from a clip.
    RemoveFilter {
        clip_id: ClipId,
        filter_id: crate::ids::EffectId,
    },
    /// Set a parameter on a filter.
    SetFilterParam {
        clip_id: ClipId,
        filter_id: crate::ids::EffectId,
        param_name: String,
        value: serde_json::Value,
    },

    // ── Keyframe operations ─────────────────────────────────────────────

    AddKeyframe {
        clip_id: ClipId,
        keyframe: crate::keyframe::Keyframe,
    },
    RemoveKeyframe {
        clip_id: ClipId,
        keyframe_id: crate::ids::KeyframeId,
    },
    MoveKeyframe {
        clip_id: ClipId,
        keyframe_id: crate::ids::KeyframeId,
        new_frame: i64,
    },

    // ── Marker operations ───────────────────────────────────────────────

    AddMarker {
        label: String,
        frame: i64,
    },
    RemoveMarker {
        marker_id: crate::ids::MarkerId,
    },
    MoveMarker {
        marker_id: crate::ids::MarkerId,
        new_frame: i64,
    },

    // ── Asset operations ────────────────────────────────────────────────

    ImportAssets {
        paths: Vec<String>,
    },
    RemoveAsset {
        asset_id: AssetId,
    },
    TagAsset {
        asset_id: AssetId,
        tags: Vec<String>,
    },
    /// Set AI-generated labels / description on an asset.
    AnnotateAsset {
        asset_id: AssetId,
        description: Option<String>,
        labels: Vec<String>,
    },
    /// Add an AI semantic region.
    AddSemanticClip {
        semantic_clip: crate::clip::SemanticClip,
    },

    // ── Bulk / batch ────────────────────────────────────────────────────

    /// Apply a batch of commands atomically (one undo entry).
    Batch {
        label: String,
        commands: Vec<EditCommand>,
    },

    // ── Multicam operations ──────────────────────────────────────────────

    /// Create a multicam clip from selected clips.
    CreateMulticam {
        /// The IDs of the clips to group into a multicam clip.
        clip_ids: Vec<ClipId>,
        /// Label for the multicam clip.
        label: String,
        /// Sync method (waveform, timecode, or manual).
        sync_method: crate::multicam::MulticamSyncMethod,
        /// Position on the timeline.
        position: i64,
        /// Track to place the multicam on.
        track_id: TrackId,
    },
    /// Switch the active angle of a multicam clip.
    SwitchMulticamAngle {
        clip_id: ClipId,
        /// Index of the new active angle.
        angle_index: usize,
    },
    /// Add an angle to an existing multicam clip.
    AddMulticamAngle {
        clip_id: ClipId,
        /// The backing asset for the new angle.
        asset_id: AssetId,
        /// Label for the angle.
        label: String,
        /// Sync offset in frames.
        offset_frames: i64,
    },
    /// Remove an angle from a multicam clip.
    RemoveMulticamAngle {
        clip_id: ClipId,
        angle_id: AngleId,
    },
    /// Set the audio policy for a multicam clip.
    SetMulticamAudioPolicy {
        clip_id: ClipId,
        policy: crate::multicam::MulticamAudioPolicy,
    },
    /// Collapse a multicam clip to a regular clip (keeps active angle).
    CollapseMulticam {
        clip_id: ClipId,
    },
    /// Set separate audio clip for a multicam clip (for AudioPolicy::Separate).
    SetMulticamSeparateAudio {
        clip_id: ClipId,
        /// The audio clip id, or None to clear.
        audio_clip_id: Option<ClipId>,
    },

    // ── Plugin operations ───────────────────────────────────────────────────

    /// Register a plugin manifest in the project's plugin cache.
    LoadPlugin {
        manifest: PluginManifest,
    },
    /// Remove a plugin from the project's cache (does not delete the file).
    UnloadPlugin {
        plugin_id: PluginId,
    },
    /// Apply a plugin as an effect on a clip (uses `EffectKind::Plugin`).
    ApplyPlugin {
        clip_id: ClipId,
        plugin_id: PluginId,
    },
    /// Remove a plugin effect from a clip.
    RemovePlugin {
        clip_id: ClipId,
        /// The `EffectInstance.id` that wraps the plugin.
        effect_id: crate::ids::EffectId,
    },
    /// Set a single parameter on a plugin effect instance.
    SetPluginParam {
        clip_id: ClipId,
        effect_id: crate::ids::EffectId,
        param_name: String,
        value: serde_json::Value,
    },
    /// Re-scan the plugins directory and refresh the discovered manifest cache.
    RefreshPluginCache,
}

impl EditCommand {
    /// Human-readable label for this command (for undo/redo menus).
    pub fn label(&self) -> &str {
        match self {
            Self::InsertClip { .. } => "Insert clip",
            Self::RemoveClip { .. } => "Remove clip",
            Self::RippleDelete { .. } => "Ripple delete",
            Self::MoveClip { .. } => "Move clip",
            Self::TrimClip { .. } => "Trim clip",
            Self::SlipClip { .. } => "Slip clip",
            Self::SplitClip { .. } => "Split clip",
            Self::SetClipSpeed { .. } => "Set speed",
            Self::SetClipTransform { .. } => "Set transform",
            Self::ToggleClipMute { .. } => "Toggle mute",
            Self::SetClipGain { .. } => "Set gain",
            Self::SetClipFade { .. } => "Set fade",
            Self::SetClipBlendMode { .. } => "Set blend mode",
            Self::SetClipLabel { .. } => "Rename clip",
            Self::SetClipTransition { .. } => "Set transition",
            Self::SetClipReverse { .. } => "Reverse clip",
            Self::SetClipFreezeFrame { .. } => "Freeze frame",
            Self::SetClipFrameBlending { .. } => "Frame blending",
            Self::SetClipSpatialConform { .. } => "Spatial conform",
            Self::NormalizeClipAudio { .. } => "Normalize audio",
            Self::AddTrack { .. } => "Add track",
            Self::RemoveTrack { .. } => "Remove track",
            Self::RenameTrack { .. } => "Rename track",
            Self::MoveTrack { .. } => "Move track",
            Self::ToggleTrackMute { .. } => "Toggle track mute",
            Self::ToggleTrackLock { .. } => "Toggle track lock",
            Self::ToggleTrackVisibility { .. } => "Toggle track visibility",
            Self::ToggleTrackDisable { .. } => "Toggle track disable",
            Self::SetTrackColor { .. } => "Set track color",
            Self::SetPrimaryTrack { .. } => "Set primary track",
            Self::AddFilter { .. } => "Add filter",
            Self::RemoveFilter { .. } => "Remove filter",
            Self::SetFilterParam { .. } => "Set filter param",
            Self::AddKeyframe { .. } => "Add keyframe",
            Self::RemoveKeyframe { .. } => "Remove keyframe",
            Self::MoveKeyframe { .. } => "Move keyframe",
            Self::AddMarker { .. } => "Add marker",
            Self::RemoveMarker { .. } => "Remove marker",
            Self::MoveMarker { .. } => "Move marker",
            Self::ImportAssets { .. } => "Import assets",
            Self::RemoveAsset { .. } => "Remove asset",
            Self::TagAsset { .. } => "Tag asset",
            Self::AnnotateAsset { .. } => "Annotate asset",
            Self::AddSemanticClip { .. } => "Add semantic clip",
            Self::Batch { label, .. } => label,
            Self::CreateMulticam { .. } => "Create multicam",
            Self::SwitchMulticamAngle { .. } => "Switch multicam angle",
            Self::AddMulticamAngle { .. } => "Add multicam angle",
            Self::RemoveMulticamAngle { .. } => "Remove multicam angle",
            Self::SetMulticamAudioPolicy { .. } => "Set audio policy",
            Self::CollapseMulticam { .. } => "Collapse multicam",
            Self::SetMulticamSeparateAudio { .. } => "Set separate audio",
            Self::LoadPlugin { .. } => "Load plugin",
            Self::UnloadPlugin { .. } => "Unload plugin",
            Self::ApplyPlugin { .. } => "Apply plugin",
            Self::RemovePlugin { .. } => "Remove plugin",
            Self::SetPluginParam { .. } => "Set plugin param",
            Self::RefreshPluginCache => "Refresh plugin cache",
        }
    }
}
