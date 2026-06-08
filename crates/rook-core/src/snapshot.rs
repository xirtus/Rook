//! Timeline snapshot types — agent-friendly project views.
//! Used by rook-ipc for AI agent communication and by rook-engine for export.

use serde::{Deserialize, Serialize};

/// A flat, agent-friendly view of the entire project timeline.
/// Modeled after Anica's `TimelineSnapshotResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSnapshot {
    pub fps_num: i32,
    pub fps_den: i32,
    pub duration_ms: f64,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub v1: Vec<TimelineClipView>,
    pub video_tracks: Vec<TimelineTrackView>,
    pub audio_tracks: Vec<TimelineTrackView>,
    pub subtitle_tracks: Vec<TimelineTrackView>,
    pub semantic_clips: Vec<TimelineSemanticView>,
    pub link_groups: Vec<LinkGroupView>,
    pub markers: Vec<TimelineMarkerView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineClipView {
    pub clip_id: String,
    pub label: String,
    pub file_path: Option<String>,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub source_in_ms: f64,
    pub media_duration_ms: Option<f64>,
    pub muted: bool,
    pub gain_db: Option<f32>,
    pub link_group_id: Option<u64>,
    pub effects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineTrackView {
    pub track_name: String,
    pub track_kind: String,
    pub muted: bool,
    pub locked: bool,
    pub visible: bool,
    pub clips: Vec<TimelineClipView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSemanticView {
    pub label: String,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkGroupView {
    pub group_id: u64,
    pub clip_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineMarkerView {
    pub label: String,
    pub frame: i64,
    pub time_ms: f64,
}
