use std::ops::{Add, Div, Mul, Neg, Sub};
use num_traits::ToPrimitive;

use crate::TICKS_PER_SECOND;
use crate::frame_rate::FrameRate;

/// `MediaTime` is the canonical time representation in Rook.
///
/// It wraps an `i64` count of **ticks** at 120,000 ticks-per-second,
/// giving sub-frame precision (0.0083 ms at 120 fps) while staying in
/// the cheap integer domain.
///
/// # Comparison to `Rational`
///
/// Rook originally used `Rational { num, den }` for time. `MediaTime`
/// replaces that for all *instant in time* calculations (playhead,
/// in/out points, keyframe positions). `Rational` is retained for
/// *frame rate descriptions* only.
///
/// # Guarantees
///
/// * Addition/subtraction is never lossy (integer arithmetic).
/// * Conversions to/from frames use explicit rounding policies
///   (`round`, `floor`, `ceil` are all available).
/// * `clamp`/`min`/`max` are panic-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct MediaTime(i64);

impl MediaTime {
    // ── Constants ────────────────────────────────────────────────────────

    /// Zero time.
    pub const ZERO: Self = Self(0);

    /// One tick (8.33 µs).
    pub const ONE_TICK: Self = Self(1);

    // ── Construction ─────────────────────────────────────────────────────

    /// Create from a raw tick count.
    #[inline]
    pub const fn from_ticks(ticks: i64) -> Self {
        Self(ticks)
    }

    /// Return the raw tick count.
    #[inline]
    pub const fn as_ticks(self) -> i64 {
        self.0
    }

    /// Create from seconds as an `f64`. Returns `None` for non-finite values
    /// or overflows.
    ///
    /// ```
    /// use rook_time::MediaTime;
    /// assert_eq!(MediaTime::from_seconds_f64(1.5).unwrap().to_seconds_f64(), 1.5);
    /// ```
    pub fn from_seconds_f64(seconds: f64) -> Option<Self> {
        if !seconds.is_finite() {
            return None;
        }
        let ticks = (seconds * TICKS_PER_SECOND as f64).round().to_i64()?;
        Some(Self(ticks))
    }

    /// Convert to seconds as `f64`.
    #[inline]
    pub fn to_seconds_f64(self) -> f64 {
        self.0.to_f64().unwrap_or(0.0) / TICKS_PER_SECOND as f64
    }

    // ── Frame conversions ────────────────────────────────────────────────

    /// Create from a frame number and rate.
    ///
    /// Returns `None` if the rate is invalid or the tick count overflows.
    ///
    /// ```
    /// use rook_time::{MediaTime, FrameRate};
    /// let t = MediaTime::from_frame(5, FrameRate::FPS_30).unwrap();
    /// assert_eq!(t.to_frame_round(FrameRate::FPS_30), Some(5));
    /// ```
    pub fn from_frame(frame: i64, rate: FrameRate) -> Option<Self> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        Some(Self(frame.checked_mul(ticks_per_frame)?))
    }

    /// Convert to frame number, rounding to the nearest frame boundary.
    ///
    /// Ties (exact half-way between frames) round away from zero.
    pub fn to_frame_round(self, rate: FrameRate) -> Option<i64> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        let remainder = self.0.rem_euclid(ticks_per_frame);
        let floor = self.0.div_euclid(ticks_per_frame);
        if remainder * 2 >= ticks_per_frame {
            Some(floor + 1)
        } else {
            Some(floor)
        }
    }

    /// Convert to frame number, rounding down (floor).
    ///
    /// This returns the frame index whose start is ≤ this time.
    pub fn to_frame_floor(self, rate: FrameRate) -> Option<i64> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        Some(self.0.div_euclid(ticks_per_frame))
    }

    /// Convert to frame number, rounding up (ceil).
    pub fn to_frame_ceil(self, rate: FrameRate) -> Option<i64> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        let remainder = self.0.rem_euclid(ticks_per_frame);
        let floor = self.0.div_euclid(ticks_per_frame);
        if remainder > 0 {
            Some(floor + 1)
        } else {
            Some(floor)
        }
    }

    // ── Frame alignment ──────────────────────────────────────────────────

    /// Snap this time to the nearest frame boundary.
    pub fn round_to_frame(self, rate: FrameRate) -> Option<Self> {
        Self::from_frame(self.to_frame_round(rate)?, rate)
    }

    /// Snap down to the nearest frame boundary.
    pub fn floor_to_frame(self, rate: FrameRate) -> Option<Self> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        Some(Self(self.0.div_euclid(ticks_per_frame) * ticks_per_frame))
    }

    /// Is this time exactly on a frame boundary?
    pub fn is_frame_aligned(self, rate: FrameRate) -> Option<bool> {
        let ticks_per_frame = rate.ticks_per_frame()?;
        Some(self.0.rem_euclid(ticks_per_frame) == 0)
    }

    /// Compute the last valid playable frame time given a duration.
    ///
    /// For a timeline of `duration` ticks, the last frame that can be
    /// played (the frame starting at the last frame boundary before
    /// `duration`) is returned.
    ///
    /// ```
    /// use rook_time::{MediaTime, FrameRate};
    /// let dur = MediaTime::from_seconds_f64(10.0).unwrap();
    /// let last = dur.last_frame_time(FrameRate::new(5, 1)).unwrap();
    /// // At 5 fps, last frame starts at 9.8s
    /// assert_eq!(last.to_seconds_f64(), 9.8);
    /// ```
    pub fn last_frame_time(self, rate: FrameRate) -> Option<Self> {
        if self <= Self::ZERO {
            return Some(Self::ZERO);
        }
        let last_inclusive_tick = self.0.checked_sub(1).unwrap_or(0);
        Self::from_ticks(last_inclusive_tick).floor_to_frame(rate)
    }

    /// Snap a seek time to the nearest frame, clamped to [0, duration].
    pub fn snapped_seek_time(self, duration: Self, rate: FrameRate) -> Option<Self> {
        let snapped = self.round_to_frame(rate)?;
        Some(snapped.clamp(Self::ZERO, duration))
    }

    // ── Bounds ───────────────────────────────────────────────────────────

    #[inline]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    // ── Duration helpers ──────────────────────────────────────────────────

    /// Duration between two times. Always non-negative.
    #[inline]
    pub fn duration_since(self, earlier: Self) -> Self {
        if self.0 >= earlier.0 {
            Self(self.0 - earlier.0)
        } else {
            Self::ZERO
        }
    }

    /// Add a duration to this time.
    #[inline]
    pub fn add_duration(self, duration: Self) -> Self {
        Self(self.0 + duration.0)
    }

    /// Subtract a duration from this time.
    /// Returns `None` if the result would be negative.
    #[inline]
    pub fn sub_duration(self, duration: Self) -> Option<Self> {
        self.0.checked_sub(duration.0).filter(|&v| v >= 0).map(Self)
    }
}

// ── Operator impls ───────────────────────────────────────────────────────

impl Add for MediaTime {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for MediaTime {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Neg for MediaTime {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl Mul<i64> for MediaTime {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: i64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Div<i64> for MediaTime {
    type Output = Self;
    #[inline]
    fn div(self, rhs: i64) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl std::fmt::Display for MediaTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let secs = self.to_seconds_f64();
        write!(f, "{:.3}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_rate::FrameRate;

    #[test]
    fn converts_between_seconds_and_ticks() {
        assert_eq!(
            MediaTime::from_seconds_f64(1.5).unwrap(),
            MediaTime::from_ticks(180_000)
        );
        assert_eq!(MediaTime::from_ticks(180_000).to_seconds_f64(), 1.5);
        assert_eq!(crate::TICKS_PER_SECOND, 120_000);
    }

    #[test]
    fn rejects_non_finite_seconds() {
        assert_eq!(MediaTime::from_seconds_f64(f64::NAN), None);
        assert_eq!(MediaTime::from_seconds_f64(f64::INFINITY), None);
        assert_eq!(MediaTime::from_seconds_f64(f64::NEG_INFINITY), None);
    }

    #[test]
    fn snaps_to_the_nearest_frame() {
        let rate = FrameRate::FPS_30;
        let time = MediaTime::from_seconds_f64(1.26).unwrap();
        assert_eq!(time.to_frame_round(rate), Some(38));
        assert_eq!(
            time.round_to_frame(rate).unwrap(),
            MediaTime::from_ticks(152_000)
        );
    }

    #[test]
    fn floors_to_frame() {
        let rate = FrameRate::FPS_30;
        let ticks_per_frame = 4_000;
        let time = MediaTime::from_ticks(ticks_per_frame * 5 + 1);
        assert_eq!(time.to_frame_floor(rate), Some(5));
        assert_eq!(time.to_frame_round(rate), Some(5));

        let almost_next = MediaTime::from_ticks(ticks_per_frame * 5 + ticks_per_frame / 2);
        assert_eq!(almost_next.to_frame_floor(rate), Some(5));
        assert_eq!(almost_next.to_frame_round(rate), Some(6));
    }

    #[test]
    fn computes_last_frame_time_and_snapped_seek_time() {
        let rate = FrameRate::new(5, 1);
        let duration = MediaTime::from_seconds_f64(10.0).unwrap();
        assert_eq!(
            duration.last_frame_time(rate).unwrap(),
            MediaTime::from_seconds_f64(9.8).unwrap()
        );
        assert_eq!(
            MediaTime::from_seconds_f64(10.0)
                .unwrap()
                .snapped_seek_time(duration, rate)
                .unwrap(),
            MediaTime::from_seconds_f64(10.0).unwrap()
        );
    }

    #[test]
    fn duration_since_is_non_negative() {
        let a = MediaTime::from_seconds_f64(1.0).unwrap();
        let b = MediaTime::from_seconds_f64(3.0).unwrap();
        assert_eq!(b.duration_since(a), MediaTime::from_seconds_f64(2.0).unwrap());
        assert_eq!(a.duration_since(b), MediaTime::ZERO);
    }

    #[test]
    fn add_sub_duration() {
        let t = MediaTime::from_seconds_f64(5.0).unwrap();
        let d = MediaTime::from_seconds_f64(2.0).unwrap();
        assert_eq!(t.add_duration(d).to_seconds_f64(), 7.0);
        assert_eq!(t.sub_duration(d).unwrap().to_seconds_f64(), 3.0);
        assert_eq!(MediaTime::ZERO.sub_duration(d), None);
    }
}
