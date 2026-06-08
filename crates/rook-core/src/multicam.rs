//! Multicam — multi-angle clip support.
//!
//! A [`MulticamClip`] groups multiple video clips (angles) that were recorded
//! simultaneously (e.g. a 3‑camera interview). Each angle is a [`MulticamAngle`]
//! referencing a real asset. The multicam clip appears as a single clip on the
//! timeline; the user switches the active angle in real time via the angle
//! viewer or keyboard shortcuts.
//!
//! ## Sync methods
//!
//! * **Waveform** — cross‑correlate audio waveforms (same algorithm as `SyncAudio`).
//! * **Timecode** — match embedded SMPTE timecodes.
//! * **Manual** — user‑entered offset in frames.

use serde::{Deserialize, Serialize};

use crate::clip::ClipId;
use crate::ids::{AngleId, AssetId};

// ── Id ──────────────────────────────────────────────────────────────────

/// How to synchronise the angles of a multicam clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MulticamSyncMethod {
    /// Use audio waveform cross-correlation.
    Waveform,
    /// Use embedded SMPTE timecode.
    Timecode,
    /// Angles are manually aligned (user supplies offsets).
    Manual,
}

/// How the multicam clip handles audio when switching angles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MulticamAudioPolicy {
    /// Audio switches to the newly‑selected angle's audio track.
    FollowVideo,
    /// Audio stays on the master (first) angle regardless of the active video.
    MasterOnly,
    /// Audio is separated — the multicam clip uses a dedicated audio clip
    /// (stored in `separate_audio_clip_id`).
    Separate,
}

// ── Angle ───────────────────────────────────────────────────────────────

/// A single camera angle inside a multicam clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticamAngle {
    /// Unique id for this angle.
    pub id: AngleId,
    /// Label shown in the angle viewer (e.g. "Cam A", "Wide").
    pub label: String,
    /// The backing asset for this angle.
    pub asset_id: AssetId,
    /// Offset in frames from the multicam clip's timeline start.
    /// Determined by the sync method — e.g. if Cam B started recording
    /// 12 frames later, its offset will be -12.
    pub offset_frames: i64,
    /// Source in‑point within the asset (where playback starts).
    #[serde(default)]
    pub source_in: i64,
    /// Whether this angle is enabled (visible in the angle viewer grid).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Audio gain offset relative to clip's gain (for matching levels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_trim_db: Option<f32>,
}

fn default_true() -> bool { true }

impl MulticamAngle {
    pub fn new(asset_id: AssetId, label: impl Into<String>) -> Self {
        Self {
            id: AngleId::next(),
            label: label.into(),
            asset_id,
            offset_frames: 0,
            source_in: 0,
            enabled: true,
            gain_trim_db: None,
        }
    }
}

// ── Multicam Clip ──────────────────────────────────────────────────────

/// A multicam clip — appears as a single clip on the timeline, but holds
/// multiple synchronised angles. The active angle determines which source
/// is rendered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulticamClip {
    /// The clip id on the timeline (the multicam "wrapper" clip).
    pub clip_id: ClipId,
    /// Human‑readable label (e.g. "Interview MC").
    pub label: String,
    /// The angles (sources) for this multicam clip.
    pub angles: Vec<MulticamAngle>,
    /// Which angle is currently active (index into `angles`).
    #[serde(default)]
    pub active_angle_index: usize,
    /// How the angles were synchronised.
    pub sync_method: MulticamSyncMethod,
    /// How audio behaves when switching angles.
    #[serde(default)]
    pub audio_policy: MulticamAudioPolicy,
    /// Optional separate audio clip id (used with `AudioPolicy::Separate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub separate_audio_clip_id: Option<ClipId>,
    /// Frame on the timeline where this multicam clip starts.
    pub timeline_in: i64,
    /// Duration in frames (computed from angles and sync offsets).
    pub duration_frames: i64,
}

impl Default for MulticamAudioPolicy {
    fn default() -> Self { Self::FollowVideo }
}

impl MulticamClip {
    /// Create a new multicam clip from a list of angles.
    pub fn new(
        clip_id: ClipId,
        label: impl Into<String>,
        angles: Vec<MulticamAngle>,
        sync_method: MulticamSyncMethod,
        timeline_in: i64,
    ) -> Self {
        let duration = angles.iter()
            .map(|a| a.offset_frames)
            .max()
            .unwrap_or(0)
            + 300; // fallback duration
        Self {
            clip_id,
            label: label.into(),
            angles,
            active_angle_index: 0,
            sync_method,
            audio_policy: MulticamAudioPolicy::default(),
            separate_audio_clip_id: None,
            timeline_in,
            duration_frames: duration,
        }
    }

    /// Get the currently active angle.
    pub fn active_angle(&self) -> Option<&MulticamAngle> {
        self.angles.get(self.active_angle_index)
    }

    /// Switch to the given angle index.
    pub fn switch_to(&mut self, index: usize) -> Option<&MulticamAngle> {
        if index < self.angles.len() {
            self.active_angle_index = index;
            self.angles.get(index)
        } else {
            None
        }
    }

    /// Switch to the next angle (wraps around).
    pub fn next_angle(&mut self) -> Option<&MulticamAngle> {
        let next = (self.active_angle_index + 1) % self.angles.len().max(1);
        self.switch_to(next)
    }

    /// Switch to the previous angle (wraps around).
    pub fn prev_angle(&mut self) -> Option<&MulticamAngle> {
        let prev = if self.active_angle_index == 0 {
            self.angles.len().saturating_sub(1)
        } else {
            self.active_angle_index - 1
        };
        self.switch_to(prev)
    }

    /// Add a new angle to the multicam clip.
    pub fn add_angle(&mut self, angle: MulticamAngle) {
        self.angles.push(angle);
    }

    /// Remove an angle by id.
    pub fn remove_angle(&mut self, angle_id: AngleId) -> Option<MulticamAngle> {
        if let Some(idx) = self.angles.iter().position(|a| a.id == angle_id) {
            let removed = self.angles.remove(idx);
            if self.active_angle_index >= self.angles.len() {
                self.active_angle_index = self.angles.len().saturating_sub(1);
            }
            Some(removed)
        } else {
            None
        }
    }

    /// Whether this multicam clip has enough angles to be functional.
    pub fn is_valid(&self) -> bool {
        self.angles.len() >= 2
    }

    /// Number of angles.
    pub fn angle_count(&self) -> usize {
        self.angles.len()
    }

    /// Collapse to a single clip — keeps only the active angle and
    /// returns the angle data (so the caller can replace the multicam
    /// clip with a regular clip using the active angle's asset).
    pub fn collapse(&self) -> Option<&MulticamAngle> {
        self.active_angle()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{AngleId, AssetId, ClipId};

    fn mk_angle(label: &str) -> MulticamAngle {
        MulticamAngle {
            id: AngleId::next(),
            label: label.to_string(),
            asset_id: AssetId::next(),
            offset_frames: 0,
            source_in: 0,
            enabled: true,
            gain_trim_db: None,
        }
    }

    #[test]
    fn test_angle_switching() {
        let clip_id = ClipId::next();
        let angles = vec![
            mk_angle("Cam A"),
            mk_angle("Cam B"),
            mk_angle("Cam C"),
        ];
        let mut mc = MulticamClip::new(
            clip_id, "Test MC", angles,
            MulticamSyncMethod::Manual, 0,
        );

        assert_eq!(mc.active_angle_index, 0);
        assert_eq!(mc.active_angle().unwrap().label, "Cam A");

        mc.next_angle();
        assert_eq!(mc.active_angle_index, 1);
        assert_eq!(mc.active_angle().unwrap().label, "Cam B");

        mc.next_angle();
        assert_eq!(mc.active_angle_index, 2);

        mc.next_angle(); // wraps
        assert_eq!(mc.active_angle_index, 0);

        mc.prev_angle(); // wraps to last
        assert_eq!(mc.active_angle_index, 2);
    }

    #[test]
    fn test_remove_angle() {
        let clip_id = ClipId::next();
        let angles = vec![mk_angle("A"), mk_angle("B"), mk_angle("C")];
        let mut mc = MulticamClip::new(clip_id, "MC", angles, MulticamSyncMethod::Manual, 0);
        mc.switch_to(2);

        let b_id = mc.angles[1].id;
        mc.remove_angle(b_id);
        assert_eq!(mc.angle_count(), 2);
        // active index should clamp to last valid (1)
        assert_eq!(mc.active_angle_index, 1);

        assert!(mc.is_valid());
    }

    #[test]
    fn test_audio_policy_default() {
        let clip_id = ClipId::next();
        let mc = MulticamClip::new(
            clip_id, "MC", vec![mk_angle("A"), mk_angle("B")],
            MulticamSyncMethod::Timecode, 0,
        );
        assert_eq!(mc.audio_policy, MulticamAudioPolicy::FollowVideo);
    }

    #[test]
    fn test_collapse() {
        let clip_id = ClipId::next();
        let angles = vec![mk_angle("A"), mk_angle("B")];
        let mut mc = MulticamClip::new(clip_id, "MC", angles, MulticamSyncMethod::Manual, 0);
        mc.switch_to(1);

        let collapsed = mc.collapse().unwrap();
        assert_eq!(collapsed.label, "B");
    }
}
