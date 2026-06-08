//! # Rook Core — shared data model for the Rook video editor.
//!
//! This crate defines the canonical project state: timeline, clips, tracks,
//! assets, effects, keyframes, markers, and edit commands.  It has **zero**
//! external dependencies beyond serde + uuid — no FFmpeg, no MLT, no GPU.
//! Every struct round-trips through serde and is the single source of truth
//! for the engine ([`rook-engine`]), the IPC layer ([`rook-ipc`]), the desktop
//! UI ([`rook-ui`]), and external AI agents.
//!
//! ## Provenance
//!
//! The model is a deliberate merge of three upstreams:
//!
//! * **cutlass-models** (MIT/Apache-2.0) — `Project`, `Timeline`, `Track`,
//!   `Clip`, `EditCommand`, `EditHistory`.  Battle-tested in a working NLE
//!   with ~8 kLOC of surrounding engine + decode code.
//! * **verbreel-state** (MIT/Apache-2.0) — `Canvas`, `Effect`, `Keyframe`,
//!   `Transform`, `BlendMode`, `MaskKind`, `FadeCurve`, `Asset` tagged union.
//!   The most comprehensive Rust video-editor type system published.
//! * **anica** (Apache-2.0) — `TimelineSnapshot`, `SemanticClip`, AI-labelled
//!   metadata fields (`ai_labels`, `ai_description`).  Shapes an agent needs
//!   to reason about a project.
//!
//! ## Crate boundary
//!
//! `rook-core` never touches a file system, spawns a process, or links
//! against a C library.  It is the "data" half; `rook-engine` is the "do"
//! half.

pub mod asset;
pub mod canvas;
pub mod clip;
pub mod commands;
pub mod effect;
pub mod error;
pub mod history;
pub mod ids;
pub mod keyframe;
pub mod marker;
pub mod multicam;
pub mod plugin;
pub mod project;
pub mod snapshot;
pub mod timeline;
pub mod time;
pub mod track;
pub mod transform;

// ── Re-exports ──────────────────────────────────────────────────────────

pub use asset::{Asset, AssetId, AssetMetadata, AudioAsset, ImageAsset, SubtitleAsset, VideoAsset};
pub use canvas::Canvas;
pub use clip::{BlendMode, Clip, ClipId, ClipMask, FadeCurve, MaskKind, SemanticClip, SpatialConform};
pub use commands::EditCommand;
pub use effect::{Effect, EffectInstance, EffectKind};
pub use error::ModelError;
pub use history::EditHistory;
pub use ids::{ProjectId, TrackId, AngleId, MulticamId, PluginId};
pub use keyframe::{Easing, Keyframe, KeyframeProperty};
pub use marker::Marker;
pub use project::Project;
pub use time::Rational;
pub use timeline::Timeline;
pub use multicam::{MulticamClip, MulticamAngle, MulticamSyncMethod, MulticamAudioPolicy};
pub use plugin::{PluginCategory, PluginManifest, PluginParamDef, PluginParamKind, PluginSource};
pub use track::{Track, TrackColor, TrackKind};
pub use transform::Transform;

/// Serde-able snapshot of the full project for agent consumption.
///
/// This is the shape that flows over IPC when an AI agent requests
/// `project.get` or `timeline.get`.  It intentionally flattens internal
/// references (clips carry their media metadata inline) so the agent
/// never needs a second round-trip.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSnapshot {
    pub project_id: ProjectId,
    pub name: String,
    pub canvas: Canvas,
    pub fps: Rational,
    pub sample_rate: u32,
    pub duration_frames: i64,
    pub tracks: Vec<track::TrackSnapshot>,
    pub markers: Vec<Marker>,
    pub assets: Vec<Asset>,
    pub proxy_dir: String,
}

/// Per-track snapshot for agents.
impl track::Track {
    pub fn to_snapshot(&self, clips: &[TrackClipView]) -> track::TrackSnapshot {
        track::TrackSnapshot {
            id: self.id,
            index: self.index,
            name: self.name.clone(),
            kind: self.kind,
            visible: self.visible,
            locked: self.locked,
            muted: self.muted,
            solo: self.solo,
            clips: clips.to_vec(),
        }
    }
}

/// A clip as seen from outside (agent or UI), with media metadata inlined.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrackClipView {
    pub clip_id: ClipId,
    pub label: String,
    pub file_path: String,
    pub timeline_in_frames: i64,
    pub duration_frames: i64,
    pub source_in_frames: i64,
    pub media_duration_frames: i64,
    pub track_id: TrackId,
    pub link_group_id: Option<u64>,
    pub speed: f64,
    pub filters: Vec<EffectInstance>,
}
