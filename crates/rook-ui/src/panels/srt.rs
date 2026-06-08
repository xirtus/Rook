//! SRT subtitle file importer.
//!
//! Parses SubRip (.srt) files and returns a list of subtitle entries
//! that can be inserted as clips on a Text track.

use rook_core::clip::{Clip, ClipId, Generator};
use rook_core::ids::AssetId;

/// A parsed subtitle entry with timing and text.
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    /// Index number from the SRT file (1-based).
    pub index: u32,
    /// Start time in seconds.
    pub start_secs: f64,
    /// End time in seconds.
    pub end_secs: f64,
    /// Subtitle text (may span multiple lines in SRT, joined with \n).
    pub text: String,
}

/// Parse an SRT file into subtitle entries.
///
/// SRT format:
/// ```
/// 1
/// 00:00:01,000 --> 00:00:04,000
/// Hello world!
///
/// 2
/// 00:00:05,000 --> 00:00:08,500
/// Second subtitle
/// ```
pub fn parse_srt(content: &str) -> Result<Vec<SubtitleEntry>, String> {
    let mut entries = Vec::new();
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse index
        let index: u32 = trimmed
            .parse()
            .map_err(|_| format!("invalid index: {}", trimmed))?;

        // Parse timestamp line
        let ts_line = lines
            .next()
            .ok_or_else(|| format!("missing timestamp for entry {}", index))?;
        let (start_secs, end_secs) =
            parse_timestamp_line(ts_line).map_err(|e| format!("entry {}: {}", index, e))?;

        // Parse text (may span multiple lines until blank line)
        let mut text = String::new();
        while let Some(text_line) = lines.peek() {
            if text_line.trim().is_empty() {
                lines.next(); // consume blank line
                break;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(text_line);
            lines.next();
        }

        entries.push(SubtitleEntry {
            index,
            start_secs,
            end_secs,
            text,
        });
    }

    Ok(entries)
}

/// Parse a timestamp line like "00:00:01,000 --> 00:00:04,000"
fn parse_timestamp_line(line: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err(format!("invalid timestamp line: {}", line));
    }
    let start = parse_srt_time(parts[0].trim())?;
    let end = parse_srt_time(parts[1].trim())?;
    Ok((start, end))
}

/// Parse SRT time format: HH:MM:SS,mmm or HH:MM:SS.mmm
fn parse_srt_time(s: &str) -> Result<f64, String> {
    // Replace comma with dot for milliseconds
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return Err(format!("invalid time: {}", s));
    }
    let hours: f64 = parts[0]
        .trim()
        .parse()
        .map_err(|_| format!("bad hours: {}", parts[0]))?;
    let mins: f64 = parts[1]
        .trim()
        .parse()
        .map_err(|_| format!("bad mins: {}", parts[1]))?;
    let secs: f64 = parts[2]
        .trim()
        .parse()
        .map_err(|_| format!("bad secs: {}", parts[2]))?;
    Ok(hours * 3600.0 + mins * 60.0 + secs)
}

/// Convert parsed SRT entries to subtitle Clips for a given frame rate.
///
/// Each entry becomes a Generator::Text clip on the timeline.
/// Returns a list of (timeline_start_frame, clip) pairs.
pub fn entries_to_clips(
    entries: &[SubtitleEntry],
    fps: f64,
    dummy_asset_id: AssetId,
) -> Vec<(i64, Clip)> {
    entries
        .iter()
        .map(|entry| {
            let timeline_in = (entry.start_secs * fps).round() as i64;
            let end_frame = (entry.end_secs * fps).round() as i64;
            let source_duration = (end_frame - timeline_in).max(1);

            let clip = Clip {
                id: ClipId::next(),
                label: if entry.text.len() > 25 {
                    format!("{}…", &entry.text[..25])
                } else {
                    entry.text.clone()
                },
                asset_id: dummy_asset_id,
                timeline_in,
                source_in: 0,
                source_duration,
                transform: rook_core::transform::Transform {
                    position: rook_core::transform::Position { x: 0.0, y: 0.0 },
                    scale: rook_core::transform::Scale { x: 0.8, y: 0.08 },
                    anchor: rook_core::transform::AnchorPoint { x: 0.5, y: 0.9 },
                    ..Default::default()
                },
                blend_mode: Default::default(),
                mask: None,
                fade: Some(rook_core::clip::Fade {
                    in_frames: 3,
                    out_frames: 3,
                    curve: rook_core::clip::FadeCurve::Linear,
                }),
                transition: None,
                speed: 1.0,
                speed_curve: vec![],
                reverse: false,
                freeze_frame: None,
                frame_blending: false,
                spatial_conform: None,
                gain_db: None,
                volume_keyframes: None,
                mute_audio: false,
                filters: vec![],
                keyframes: vec![],
                link_group_id: None,
                generator: Some(Generator::Text {
                    content: entry.text.clone(),
                    font_size: 36.0,
                    color: [1.0, 1.0, 1.0, 0.95],
                }),
            };
            (timeline_in, clip)
        })
        .collect()
}

/// Import an SRT file and return subtitle clips.
/// Returns entries that can be inserted into a timeline.
pub fn import_srt(
    path: &std::path::Path,
    fps: f64,
) -> Result<(Vec<SubtitleEntry>, Vec<(i64, Clip)>), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let entries = parse_srt(&content)?;
    let clips = entries_to_clips(&entries, fps, AssetId::nil());
    Ok((entries, clips))
}
