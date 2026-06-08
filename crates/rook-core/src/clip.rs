//! Clip — the atomic unit on a track.  Merged from cutlass-models
//! (deterministic edit model) with verbreel-state additions (blend mode,
//! mask, fade, speed curve) and anica additions (semantic clips for AI).

use serde::{Deserialize, Serialize};

use crate::effect::EffectInstance;
pub use crate::ids::{AssetId, ClipId};
use crate::keyframe::Keyframe;
use crate::time::TimeRange;
use crate::transform::Transform;

// ── Blend mode ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    PlusLighter,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl Default for BlendMode {
    fn default() -> Self { Self::Normal }
}

/// How a clip's source media is conformed to the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialConform {
    /// Fit within canvas, preserving aspect ratio (letterbox/pillarbox).
    Fit,
    /// Fill canvas, cropping excess.
    Fill,
    /// No scaling — use source dimensions as-is.
    None,
}

// ── Mask ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskKind {
    Rectangle,
    Ellipse,
    Freehand,
}

/// A mask applied to a clip (position, size, feather).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipMask {
    pub kind: MaskKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Feather radius in pixels.
    #[serde(default)]
    pub feather: f32,
    /// Invert the mask.
    #[serde(default)]
    pub invert: bool,
}

// ── Fade ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    #[default]
    Linear,
    Ease,
    EaseIn,
    EaseOut,
}

/// Fade-in / fade-out durations in frames.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fade {
    pub in_frames: i64,
    pub out_frames: i64,
    #[serde(default)]
    pub curve: FadeCurve,
}

// ── Transitions ─────────────────────────────────────────────────────────

/// The kind of video transition between clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// Cross-dissolve (fade one clip into the next).
    CrossDissolve,
    /// Simple opacity fade (same as cross-dissolve, simpler name).
    Dissolve,
    /// Wipe left-to-right.
    Wipe,
    /// Slide left-to-right.
    Slide,
}

/// A transition applied at the start (or end) of a clip.
///
/// Transitions overlap two adjacent clips: the outgoing clip's tail and the
/// incoming clip's head overlap for `duration_frames`, during which the
/// transition effect is applied.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// The kind of transition effect.
    pub kind: TransitionKind,
    /// Duration of the transition in frames.
    pub duration_frames: i64,
    /// Whether the transition plays forward (default) or reversed.
    #[serde(default)]
    pub reversed: bool,
    /// Optional easing curve.
    #[serde(default)]
    pub curve: FadeCurve,
}

// ── Speed curve ─────────────────────────────────────────────────────────

/// A control point on a speed-ramp curve (time → speed multiplier).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedCurvePoint {
    /// Time in frames from the start of the clip.
    pub frame: i64,
    /// Speed multiplier at this point.
    pub speed: f64,
}

// ── Generators ────────────────────────────────────────────────────────────

/// A generated clip (no media asset needed).
/// Renders patterns, solids, text, etc. directly in the compositor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Generator {
    /// Solid color fill.
    Solid {
        /// RGBA color, 0.0–1.0 per channel.
        color: [f32; 4],
    },
    /// Text overlay (placeholder — renders placeholder text).
    Text {
        /// The text content.
        content: String,
        /// Font size in points.
        #[serde(default = "default_text_size")]
        font_size: f32,
        /// Text color.
        #[serde(default = "default_text_color")]
        color: [f32; 4],
    },
    /// Scrolling credits (text that scrolls upward).
    Credits {
        /// Multi-line text (each line is a credit entry).
        content: String,
        /// Font size in points.
        #[serde(default = "default_text_size")]
        font_size: f32,
        /// Text color.
        #[serde(default = "default_text_color")]
        color: [f32; 4],
        /// Scroll speed — pixels per second.
        #[serde(default = "default_scroll_speed")]
        scroll_speed: f32,
    },
    /// Custom placeholder — for future generators.
    #[serde(other)]
    Custom,
}

fn default_scroll_speed() -> f32 { 60.0 }

fn default_text_size() -> f32 { 48.0 }
fn default_text_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }

impl Generator {
    /// Whether this generator needs an asset (always false).
    pub fn needs_asset(&self) -> bool { false }

    pub fn label(&self) -> &str {
        match self {
            Self::Solid { .. } => "Color",
            Self::Text { content, .. } => {
                let trimmed = content.trim();
                if trimmed.len() > 20 { &trimmed[..20] } else { trimmed }
            }
            Self::Credits { content, .. } => {
                let first_line = content.lines().next().unwrap_or("Credits");
                if first_line.len() > 20 { &first_line[..20] } else { first_line }
            }
            Self::Custom => "Generator",
        }
    }
}

// ── Clip ────────────────────────────────────────────────────────────────

/// A single clip on a track.  Owns its source reference, timeline placement,
/// transform, effects, and keyframes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    /// Human-readable label (defaults to filename stem).
    pub label: String,
    /// The backing asset.
    pub asset_id: AssetId,

    // ── Timeline placement ──────────────────────────────────────────────
    /// Frame on the timeline where this clip starts.
    pub timeline_in: i64,
    /// Frame within the source where playback starts.
    pub source_in: i64,
    /// Number of source frames to play (before speed adjustment).
    pub source_duration: i64,

    // ── Transform ───────────────────────────────────────────────────────
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<ClipMask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade: Option<Fade>,
    /// Transition at the start of this clip (cross-fade with previous clip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,

    // ── Playback ────────────────────────────────────────────────────────
    /// Playback speed multiplier (1.0 = normal, 2.0 = 2× fast).
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// Speed-ramp curve (overrides `speed` when set).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speed_curve: Vec<SpeedCurvePoint>,
    /// Reverse playback (plays clip backwards).
    #[serde(default)]
    pub reverse: bool,
    /// Freeze frame: if set, hold this source frame for the clip's duration.
    /// The frame is relative to `source_in`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze_frame: Option<i64>,
    /// Frame blending for smooth speed changes (optical flow / frame mix).
    #[serde(default)]
    pub frame_blending: bool,
    /// Spatial conform mode: how source media fits into the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial_conform: Option<SpatialConform>,

    // ── Audio ───────────────────────────────────────────────────────────
    /// Gain adjustment in dB for this clip's audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
    /// Volume keyframe points for gain automation (local_frame, gain_db).
    /// Added via ⌥+click on the gain line in the timeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_keyframes: Option<Vec<(i64, f64)>>,
    /// Mute this clip's audio.
    #[serde(default)]
    pub mute_audio: bool,

    // ── Effects & keyframes ─────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<EffectInstance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyframes: Vec<Keyframe>,

    // ── Linking ─────────────────────────────────────────────────────────
    /// If set, this clip is grouped with others sharing the same link_group_id
    /// (e.g. linked AV pair).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_group_id: Option<u64>,

    // ── Generator ──────────────────────────────────────────────────────
    /// If set, this clip renders a generator instead of media.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
}

fn default_speed() -> f64 { 1.0 }

impl Clip {
    /// Duration of this clip on the timeline, accounting for speed.
    pub fn duration(&self) -> i64 {
        (self.source_duration as f64 / self.speed).round() as i64
    }

    /// The time range this clip occupies on the timeline.
    pub fn timeline_range(&self) -> TimeRange {
        TimeRange::new(self.timeline_in, self.timeline_in + self.duration())
    }

    /// The time range within the source media.
    pub fn source_range(&self) -> TimeRange {
        TimeRange::new(self.source_in, self.source_in + self.source_duration)
    }

    /// Whether this clip covers `frame` on the timeline.
    pub fn covers(&self, frame: i64) -> bool {
        self.timeline_range().contains(frame)
    }

    /// Map a timeline frame to a source frame (accounting for source_in + speed).
    pub fn timeline_to_source(&self, timeline_frame: i64) -> Option<i64> {
        if !self.covers(timeline_frame) { return None; }
        let offset = timeline_frame - self.timeline_in;
        let source_offset = (offset as f64 * self.speed).round() as i64;
        Some(self.source_in + source_offset)
    }

    // ── MediaTime helpers (ticks-based precision) ──────────────────────────

    /// Duration as a `MediaTime`, given the timeline's frame rate.
    pub fn duration_media_time(&self, rate: &rook_time::FrameRate) -> Option<rook_time::MediaTime> {
        let frames = self.duration();
        rook_time::MediaTime::from_frame(frames, *rate)
    }

    /// Timeline start as a `MediaTime`.
    pub fn timeline_in_media_time(&self, rate: &rook_time::FrameRate) -> Option<rook_time::MediaTime> {
        rook_time::MediaTime::from_frame(self.timeline_in, *rate)
    }
}

// ── Semantic clip (AI annotation layer) ─────────────────────────────────

/// A semantic region on the timeline — labelled by AI, not tied to a media
/// clip.  Used for "find all scenes with product X", "this 30 s window is
/// a montage", etc.  Adapted from anica.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticClip {
    pub id: u64,
    pub label: String,
    pub semantic_type: String,
    pub start_frame: i64,
    pub duration_frames: i64,
    /// Arbitrary structured metadata the AI agent attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_schema: Option<serde_json::Value>,
}
