//! Rational frame rates and time ranges — from cutlass-models.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A rational number (numerator / denominator), used for frame rates.
///
/// `Rational { num: 24, den: 1 }` means 24 fps.
/// `Rational { num: 30000, den: 1001 }` means 29.97 fps (NTSC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    pub num: i64,
    pub den: i64,
}

impl Rational {
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }

    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Frames → seconds.
    pub fn frames_to_seconds(&self, frames: i64) -> f64 {
        frames as f64 / self.as_f64()
    }

    /// Seconds → frames (rounded).
    pub fn seconds_to_frames(&self, seconds: f64) -> i64 {
        (seconds * self.as_f64()).round() as i64
    }

    /// Convert a frame count from `source` rate to `target` rate.
    pub fn convert_frames(frames: i64, source: Rational, target: Rational) -> i64 {
        let secs = frames as f64 / source.as_f64();
        (secs * target.as_f64()).round() as i64
    }

    // ── Common rates ──
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    pub const FPS_60: Self = Self { num: 60, den: 1 };
    pub const NTSC: Self = Self { num: 30000, den: 1001 };

    // ── rook_time::FrameRate conversions ───────────────────────────────────

    /// Convert to `rook_time::FrameRate`.
    ///
    /// Returns `None` if the rational can't be represented as a u32 ratio.
    pub fn to_time_frame_rate(&self) -> Option<rook_time::FrameRate> {
        let num: u32 = self.num.try_into().ok()?;
        let den: u32 = self.den.try_into().ok()?;
        Some(rook_time::FrameRate::new(num, den))
    }
}

impl From<rook_time::FrameRate> for Rational {
    fn from(rate: rook_time::FrameRate) -> Self {
        Self {
            num: rate.numerator as i64,
            den: rate.denominator as i64,
        }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// A half-open range of frames `[start, end)` on a timeline or within a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

impl TimeRange {
    pub fn new(start: i64, end: i64) -> Self {
        debug_assert!(end >= start, "TimeRange: end ({end}) < start ({start})");
        Self { start, end }
    }

    pub fn duration(&self) -> i64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn contains(&self, frame: i64) -> bool {
        frame >= self.start && frame < self.end
    }

    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn intersection(&self, other: &TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then(|| TimeRange::new(start, end))
    }

    /// Shift this range by `offset` frames.
    pub fn offset(&self, offset: i64) -> TimeRange {
        TimeRange::new(self.start + offset, self.end + offset)
    }
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {})", self.start, self.end)
    }
}
