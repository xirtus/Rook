//! Tracks: ordered containers that hold clips on the timeline.
//! Merged from cutlass-models + verbreel-state (added text/effect track kinds).

use serde::{Deserialize, Serialize};

use crate::clip::Clip;
use crate::ids::TrackId;

/// The kind of media a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    /// Subtitle / caption track.
    Text,
    /// Adjustment / effect layer.
    Effect,
}

impl TrackKind {
    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Audio)
    }
}

/// Track height presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackHeight {
    Mini,
    Small,
    Medium,
    Large,
}

impl TrackHeight {
    pub fn pixels(&self) -> f32 {
        match self {
            TrackHeight::Mini => 22.0,
            TrackHeight::Small => 32.0,
            TrackHeight::Medium => 44.0,
            TrackHeight::Large => 64.0,
        }
    }
    pub fn label(&self) -> &str {
        match self {
            TrackHeight::Mini => "Mini",
            TrackHeight::Small => "Small",
            TrackHeight::Medium => "Medium",
            TrackHeight::Large => "Large",
        }
    }
}

impl Default for TrackHeight {
    fn default() -> Self { TrackHeight::Medium }
}

/// A color label assigned to a track for visual identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Gray,
}

impl TrackColor {
    /// Return the hex color string for this label.
    pub fn hex(&self) -> &str {
        match self {
            TrackColor::Red => "#e74c3c",
            TrackColor::Orange => "#e67e22",
            TrackColor::Yellow => "#f1c40f",
            TrackColor::Green => "#2ecc71",
            TrackColor::Blue => "#3498db",
            TrackColor::Purple => "#9b59b6",
            TrackColor::Pink => "#e91e90",
            TrackColor::Gray => "#7f8c8d",
        }
    }

    /// Return a human-readable label.
    pub fn label(&self) -> &str {
        match self {
            TrackColor::Red => "Red",
            TrackColor::Orange => "Orange",
            TrackColor::Yellow => "Yellow",
            TrackColor::Green => "Green",
            TrackColor::Blue => "Blue",
            TrackColor::Purple => "Purple",
            TrackColor::Pink => "Pink",
            TrackColor::Gray => "Gray",
        }
    }
}

/// A single track on the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    /// 0-based index among tracks of the same kind (V1=0, V2=1, ...).
    pub index: usize,
    pub name: String,
    pub kind: TrackKind,
    pub visible: bool,
    pub locked: bool,
    pub muted: bool,
    pub solo: bool,
    /// Disable track: exclude from export.
    #[serde(default)]
    pub disabled: bool,
    /// Color label for visual identification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TrackColor>,
    /// Primary storyline — the main magnetic track (FCP concept).
    /// Only one track should be primary at a time.
    #[serde(default)]
    pub is_primary: bool,
    /// Gain in dB (audio tracks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
    /// Track display height.
    #[serde(default)]
    pub height: TrackHeight,
    /// Clips on this track, ordered by timeline position.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
    /// Per-track effects (e.g. EQ on an audio track).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<crate::effect::EffectInstance>,
}

impl Track {
    pub fn new(kind: TrackKind, name: impl Into<String>, index: usize) -> Self {
        Self {
            id: TrackId::next(),
            index,
            name: name.into(),
            kind,
            visible: true,
            locked: false,
            muted: false,
            solo: false,
            disabled: false,
            color: None,
            is_primary: false,
            gain_db: None,
            height: TrackHeight::default(),
            clips: Vec::new(),
            effects: Vec::new(),
        }
    }

    pub fn clip(&self, clip_id: crate::ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == clip_id)
    }

    pub fn clip_mut(&mut self, clip_id: crate::ClipId) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == clip_id)
    }

    pub fn clip_index(&self, clip_id: crate::ClipId) -> Option<usize> {
        self.clips.iter().position(|c| c.id == clip_id)
    }

    /// Insert a clip, maintaining timeline-order.
    ///
    /// Audio tracks allow overlapping clips (for crossfades).
    /// All other track kinds reject overlaps.
    pub fn insert_clip(&mut self, clip: Clip) -> Result<(), crate::error::ModelError> {
        let range = clip.timeline_range();
        // Allow overlap on audio tracks (crossfade)
        if self.kind != TrackKind::Audio {
            for existing in &self.clips {
                if existing.timeline_range().overlaps(&range) {
                    return Err(crate::error::ModelError::Overlap {
                        track: self.id,
                        start: range.start,
                        end: range.end,
                        existing_start: existing.timeline_in,
                        existing_end: existing.timeline_in + existing.duration(),
                    });
                }
            }
        }
        let pos = self.clips.iter().position(|c| c.timeline_in > clip.timeline_in)
            .unwrap_or(self.clips.len());
        self.clips.insert(pos, clip);
        Ok(())
    }

    pub fn remove_clip(&mut self, clip_id: crate::ClipId) -> Option<Clip> {
        self.clip_index(clip_id).map(|i| self.clips.remove(i))
    }

    /// Total duration of this track in frames (end of the last clip).
    pub fn duration(&self) -> i64 {
        self.clips.last().map(|c| c.timeline_in + c.duration()).unwrap_or(0)
    }
}

/// Snapshot of a track sent to agents / UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSnapshot {
    pub id: TrackId,
    pub index: usize,
    pub name: String,
    pub kind: TrackKind,
    pub visible: bool,
    pub locked: bool,
    pub muted: bool,
    pub solo: bool,
    pub clips: Vec<super::TrackClipView>,
}
