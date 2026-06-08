//! API types — the request/response shapes agents see.
//! Adapted from Anica's `api/timeline/` + Verbreel's verb schemas.

use serde::{Deserialize, Serialize};
use rook_core::{
    asset::{Asset, AssetId},
    canvas::Canvas,
    clip::{ClipId, SemanticClip},
    commands::EditCommand,
    effect::EffectInstance,
    ids::TrackId,
    keyframe::Keyframe,
    marker::Marker,
    project::Project,
    time::Rational,
    track::{TrackKind, TrackSnapshot},
};

// ── Project ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGetResponse {
    pub snapshot: rook_core::ProjectSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectExportRequest {
    pub output_path: String,
    pub format: String,
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectExportResponse {
    pub job_id: String,
}

// ── Gallery ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryImportRequest {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryImportResponse {
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryListResponse {
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryAnnotateRequest {
    pub asset_id: AssetId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
}

// ── Timeline ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineGetRequest {
    #[serde(default = "default_true")]
    pub include_subtitles: bool,
    #[serde(default)]
    pub include_semantic: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineGetResponse {
    pub snapshot: rook_core::ProjectSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineInsertClipRequest {
    pub asset_id: AssetId,
    pub track_id: TrackId,
    pub position: i64,
    #[serde(default)]
    pub source_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_out: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineInsertClipResponse {
    pub clip_id: ClipId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineRemoveClipRequest {
    pub clip_id: ClipId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSplitClipRequest {
    pub clip_id: ClipId,
    pub at_frame: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSplitClipResponse {
    pub clip_a: ClipId,
    pub clip_b: ClipId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineAddTrackRequest {
    pub kind: TrackKind,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineAddTrackResponse {
    pub track_id: TrackId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSetPlayheadRequest {
    pub frame: i64,
}

// ── Preview ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewGetFrameRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewGetFrameResponse {
    pub frame: i64,
    /// Base64-encoded JPEG of the composited frame.
    pub image_base64: String,
    pub width: u32,
    pub height: u32,
}

// ── Undo ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoResponse {
    pub undone: Option<String>,  // command label that was undone
    pub can_undo: bool,
    pub can_redo: bool,
}

// ── Batch ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecuteRequest {
    pub commands: Vec<EditCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecuteResponse {
    pub results: Vec<BatchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Query ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchClipsRequest {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_duration_frames: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchClipsResponse {
    pub clips: Vec<ClipMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipMatch {
    pub clip_id: ClipId,
    pub label: String,
    pub asset_id: AssetId,
    pub score: f64,
}

// ── Timeline Snapshot (agent-friendly) — re-exported from rook_core ────

pub use rook_core::snapshot::{
    TimelineSnapshot, TimelineClipView, TimelineTrackView,
    TimelineSemanticView, LinkGroupView, TimelineMarkerView,
};

// ── Agent Edit Plan ─────────────────────────────────────────────────────

/// An edit plan submitted by an agent — a list of proposed operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEditPlanRequest {
    pub edits: Vec<EditOperation>,
    /// Optional human-readable explanation of the plan.
    #[serde(default)]
    pub description: Option<String>,
}

/// A single edit operation in an agent plan.
/// These map 1:1 to `EditCommand` variants but use agent-friendly field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOperation {
    Cut {
        clip_ids: Vec<String>,
        #[serde(default)]
        description: Option<String>,
    },
    Trim {
        clip_id: String,
        new_start_ms: Option<f64>,
        new_end_ms: Option<f64>,
    },
    Move {
        clip_id: String,
        new_track: Option<String>,
        new_start_ms: f64,
    },
    AddFilter {
        clip_id: String,
        filter_type: String,
        params: serde_json::Value,
    },
    SetSpeed {
        clip_id: String,
        speed: f64,
    },
    InsertClip {
        asset_id: String,
        track_name: String,
        position_ms: f64,
        source_in_ms: Option<f64>,
        source_out_ms: Option<f64>,
    },
    RemoveClip {
        clip_id: String,
    },
    RippleDelete {
        clip_id: String,
    },
    SplitClip {
        clip_id: String,
        at_frame: i64,
    },
    /// Batch label — used for macro operations.
    Label(String),
}

/// Validation response from the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEditValidationResponse {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Agent protocol: silence detection request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSilenceMapRequest {
    /// RMS threshold in dB. Silences below this are detected.
    pub rms_threshold_db: f64,
    /// Minimum silence duration in milliseconds.
    pub min_silence_ms: f64,
    /// Pad silence regions by this many milliseconds on each side.
    #[serde(default)]
    pub pad_ms: f64,
}

/// A detected silence region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SilenceRegion {
    pub start_ms: f64,
    pub end_ms: f64,
    pub duration_ms: f64,
    pub level_db: f64,
}

/// Response to a silence map request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSilenceMapResponse {
    pub clip_id: String,
    pub silences: Vec<SilenceRegion>,
}

// ── Server events (editor → agent) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EditorEvent {
    ProjectChanged { dirty: bool },
    PlayheadMoved { frame: i64 },
    ExportProgress { job_id: String, percent: f32 },
    ProxyStatus { asset_id: AssetId, status: String },
    SelectionChanged { clip_ids: Vec<ClipId> },
}
