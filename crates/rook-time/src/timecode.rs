use crate::frame_rate::FrameRate;
use crate::media_time::MediaTime;
use crate::{TICKS_PER_SECOND, TICKS_PER_CENTISECOND, CENTISECONDS_PER_SECOND,
            SECONDS_PER_HOUR, SECONDS_PER_MINUTE};

// ── Timecode format ──────────────────────────────────────────────────────

/// Standard SMPTE timecode display formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TimeCodeFormat {
    /// Minutes:Seconds (e.g. `01:05`)
    #[cfg_attr(feature = "serde", serde(rename = "MM:SS"))]
    MmSs,
    /// Hours:Minutes:Seconds (e.g. `00:01:05`)
    #[cfg_attr(feature = "serde", serde(rename = "HH:MM:SS"))]
    HhMmSs,
    /// Hours:Minutes:Seconds:Centiseconds (e.g. `01:02:03:45`)
    #[cfg_attr(feature = "serde", serde(rename = "HH:MM:SS:CS"))]
    HhMmSsCs,
    /// Hours:Minutes:Seconds:Frames (e.g. `00:00:01:15` at 30 fps)
    #[cfg_attr(feature = "serde", serde(rename = "HH:MM:SS:FF"))]
    HhMmSsFf,
}

// ── Public API ───────────────────────────────────────────────────────────

/// Auto-detect the most likely timecode format from a string.
///
/// ```
/// use rook_time::{guess_timecode_format, TimeCodeFormat};
/// assert_eq!(guess_timecode_format("01:05"), Some(TimeCodeFormat::MmSs));
/// assert_eq!(guess_timecode_format("00:00:01"), Some(TimeCodeFormat::HhMmSs));
/// assert_eq!(guess_timecode_format("00:00:01:15"), Some(TimeCodeFormat::HhMmSsFf));
/// ```
pub fn guess_timecode_format(time_code: &str) -> Option<TimeCodeFormat> {
    if time_code.trim().is_empty() { return None; }
    let part_count = time_code.trim().split(':')
        .try_fold(0usize, |count, part| part.parse::<u32>().ok().map(|_| count + 1))?;
    match part_count {
        2 => Some(TimeCodeFormat::MmSs),
        3 => Some(TimeCodeFormat::HhMmSs),
        4 => Some(TimeCodeFormat::HhMmSsFf),
        _ => None,
    }
}

/// Format a `MediaTime` as a timecode string.
///
/// If `format` is `None`, defaults to `HhMmSsCs` (HH:MM:SS:CS).
/// For `HhMmSsFf`, a `FrameRate` must be provided.
///
/// ```
/// use rook_time::{MediaTime, FrameRate, format_timecode, TimeCodeFormat};
/// let t = MediaTime::from_seconds_f64(3723.45).unwrap();
/// assert_eq!(format_timecode(t, None, None).unwrap(), "01:02:03:45");
///
/// let t = MediaTime::from_seconds_f64(1.5).unwrap();
/// assert_eq!(
///     format_timecode(t, Some(TimeCodeFormat::HhMmSsFf), Some(FrameRate::FPS_30)).unwrap(),
///     "00:00:01:15"
/// );
/// ```
pub fn format_timecode(
    time: MediaTime,
    format: Option<TimeCodeFormat>,
    rate: Option<FrameRate>,
) -> Option<String> {
    let format = format.unwrap_or(TimeCodeFormat::HhMmSsCs);
    let total_ticks = u64::try_from(time.as_ticks().max(0)).ok()?;
    let ticks_per_second = u64::try_from(TICKS_PER_SECOND).ok()?;
    let total_seconds = total_ticks / ticks_per_second;

    let hour_ticks = u64::try_from(SECONDS_PER_HOUR).ok()? * ticks_per_second;
    let minute_ticks = u64::try_from(SECONDS_PER_MINUTE).ok()? * ticks_per_second;
    let seconds_per_minute = u64::try_from(SECONDS_PER_MINUTE).ok()?;
    let ticks_per_centisecond = u64::try_from(TICKS_PER_CENTISECOND).ok()?;

    let hours = total_ticks / hour_ticks;
    let minutes = (total_ticks % hour_ticks) / minute_ticks;
    let seconds = total_seconds % seconds_per_minute;
    let second_ticks = total_ticks % ticks_per_second;
    let centiseconds = second_ticks / ticks_per_centisecond;

    match format {
        TimeCodeFormat::MmSs =>
            Some(format!("{minutes:02}:{seconds:02}")),
        TimeCodeFormat::HhMmSs =>
            Some(format!("{hours:02}:{minutes:02}:{seconds:02}")),
        TimeCodeFormat::HhMmSsCs =>
            Some(format!("{hours:02}:{minutes:02}:{seconds:02}:{centiseconds:02}")),
        TimeCodeFormat::HhMmSsFf => {
            let rate = rate?;
            let ticks_per_frame = rate.ticks_per_frame()?;
            let frames = second_ticks / u64::try_from(ticks_per_frame).ok()?;
            Some(format!("{hours:02}:{minutes:02}:{seconds:02}:{frames:02}"))
        }
    }
}

/// Parse a timecode string into a `MediaTime`.
///
/// ```
/// use rook_time::{MediaTime, FrameRate, parse_timecode, TimeCodeFormat};
///
/// let t = parse_timecode("01:05", Some(TimeCodeFormat::MmSs), None).unwrap();
/// assert_eq!(t, MediaTime::from_seconds_f64(65.0).unwrap());
///
/// let t = parse_timecode(
///     "00:00:01:15", Some(TimeCodeFormat::HhMmSsFf), Some(FrameRate::FPS_30)
/// ).unwrap();
/// assert_eq!(t, MediaTime::from_seconds_f64(1.5).unwrap());
/// ```
pub fn parse_timecode(
    time_code: &str,
    format: Option<TimeCodeFormat>,
    rate: Option<FrameRate>,
) -> Option<MediaTime> {
    if time_code.trim().is_empty() { return None; }

    let format = format.unwrap_or(TimeCodeFormat::HhMmSsCs);
    let parts = time_code.trim().split(':')
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;

    match format {
        TimeCodeFormat::MmSs => {
            let [minutes, seconds] = parts.as_slice() else { return None; };
            if i64::from(*seconds) >= SECONDS_PER_MINUTE { return None; }
            Some(MediaTime::from_ticks(
                (i64::from(*minutes) * SECONDS_PER_MINUTE + i64::from(*seconds)) * TICKS_PER_SECOND,
            ))
        }
        TimeCodeFormat::HhMmSs => {
            let [hours, minutes, seconds] = parts.as_slice() else { return None; };
            if i64::from(*minutes) >= SECONDS_PER_MINUTE
                || i64::from(*seconds) >= SECONDS_PER_MINUTE { return None; }
            Some(MediaTime::from_ticks(
                (i64::from(*hours) * SECONDS_PER_HOUR
                    + i64::from(*minutes) * SECONDS_PER_MINUTE
                    + i64::from(*seconds)) * TICKS_PER_SECOND,
            ))
        }
        TimeCodeFormat::HhMmSsCs => {
            let [hours, minutes, seconds, centiseconds] = parts.as_slice() else { return None; };
            if i64::from(*minutes) >= SECONDS_PER_MINUTE
                || i64::from(*seconds) >= SECONDS_PER_MINUTE
                || i64::from(*centiseconds) >= CENTISECONDS_PER_SECOND { return None; }
            Some(MediaTime::from_ticks(
                (i64::from(*hours) * SECONDS_PER_HOUR
                    + i64::from(*minutes) * SECONDS_PER_MINUTE
                    + i64::from(*seconds)) * TICKS_PER_SECOND
                    + i64::from(*centiseconds) * TICKS_PER_CENTISECOND,
            ))
        }
        TimeCodeFormat::HhMmSsFf => {
            let rate = rate?;
            let frame_upper_bound = rate.frame_number_upper_bound()?;
            let [hours, minutes, seconds, frames] = parts.as_slice() else { return None; };
            if i64::from(*minutes) >= SECONDS_PER_MINUTE
                || i64::from(*seconds) >= SECONDS_PER_MINUTE
                || *frames >= frame_upper_bound { return None; }
            Some(
                MediaTime::from_ticks(
                    (i64::from(*hours) * SECONDS_PER_HOUR
                        + i64::from(*minutes) * SECONDS_PER_MINUTE
                        + i64::from(*seconds)) * TICKS_PER_SECOND,
                ) + MediaTime::from_frame(i64::from(*frames), rate)?,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_rate::FrameRate;
    use crate::media_time::MediaTime;

    #[test]
    fn formats_default_and_frame_timecodes() {
        assert_eq!(
            format_timecode(
                MediaTime::from_seconds_f64(3723.45).unwrap(),
                None,
                None,
            ).unwrap(),
            "01:02:03:45"
        );
        assert_eq!(
            format_timecode(
                MediaTime::from_seconds_f64(1.5).unwrap(),
                Some(TimeCodeFormat::HhMmSsFf),
                Some(FrameRate::FPS_30),
            ).unwrap(),
            "00:00:01:15"
        );
    }

    #[test]
    fn parses_timecodes() {
        assert_eq!(
            parse_timecode("01:05", Some(TimeCodeFormat::MmSs), None).unwrap(),
            MediaTime::from_seconds_f64(65.0).unwrap()
        );
        assert_eq!(
            parse_timecode(
                "00:00:01:15",
                Some(TimeCodeFormat::HhMmSsFf),
                Some(FrameRate::FPS_30),
            ).unwrap(),
            MediaTime::from_seconds_f64(1.5).unwrap()
        );
        // 30 frames at 30 fps = out of bounds
        assert_eq!(
            parse_timecode(
                "00:00:01:30",
                Some(TimeCodeFormat::HhMmSsFf),
                Some(FrameRate::FPS_30),
            ),
            None
        );
    }

    #[test]
    fn guesses_timecode_formats() {
        assert_eq!(guess_timecode_format("01:05"), Some(TimeCodeFormat::MmSs));
        assert_eq!(guess_timecode_format("00:00:01"), Some(TimeCodeFormat::HhMmSs));
        assert_eq!(guess_timecode_format("00:00:01:15"), Some(TimeCodeFormat::HhMmSsFf));
    }
}
