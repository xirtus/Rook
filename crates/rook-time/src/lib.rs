//! Ticks-based media time, frame rates, and timecode for the Rook video editor.
//!
//! Adapted from koughen/Editor (MIT). Uses 120,000 ticks-per-second for
//! precise integer arithmetic across all standard frame rates
//! (24, 25, 30, 48, 50, 60, 120 fps + NTSC variants).
//!
//! ## Design Decisions
//!
//! * **120,000 ticks/sec** — the least common multiple of all standard
//!   frame-rate denominators, guaranteeing exact integer tick counts
//!   for every frame at every supported rate.
//! * **No floating-point in core ops** — frame-to-tick and tick-to-frame
//!   conversions use integer arithmetic exclusively, eliminating
//!   rounding drift that accumulates over long timelines.
//! * **Half-away-from-zero rounding** — `to_frame_round()` rounds to the
//!   nearest frame boundary, with ties breaking away from zero (standard
//!   video convention).

mod media_time;
mod frame_rate;
mod timecode;

pub use media_time::MediaTime;
pub use frame_rate::FrameRate;
pub use timecode::{TimeCodeFormat, format_timecode, parse_timecode, guess_timecode_format};

/// Ticks per second — the fundamental time unit of Rook.
pub const TICKS_PER_SECOND: i64 = 120_000;

/// Number of centiseconds per second (for HH:MM:SS:CS display).
const CENTISECONDS_PER_SECOND: i64 = 100;

/// Ticks per centisecond.
const TICKS_PER_CENTISECOND: i64 = TICKS_PER_SECOND / CENTISECONDS_PER_SECOND;

// ── Helper consts for timecode formatting ──
const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_MINUTE: i64 = 60;
