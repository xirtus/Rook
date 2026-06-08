//! Media probe — extract metadata without full decode.
//! Uses `ffprobe` via subprocess for now; direct FFmpeg API later.

use std::path::Path;
use std::process::Command;
use rook_core::asset::{AssetMetadata, AudioMetadata, VideoMetadata};
use crate::DecodeError;

/// Probe a media file and return its metadata.
pub fn probe(path: &Path) -> Result<AssetMetadata, DecodeError> {
    // Stub: use ffprobe to extract metadata
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            &path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| DecodeError::Io(e))?;

    if !output.status.success() {
        return Err(DecodeError::Ffmpeg(
            String::from_utf8_lossy(&output.stderr).to_string()
        ));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| DecodeError::Ffmpeg(format!("ffprobe JSON parse: {e}")))?;

    let mut meta = AssetMetadata::default();

    // Parse format duration
    if let Some(dur) = json["format"]["duration"].as_str() {
        if let Ok(secs) = dur.parse::<f64>() {
            meta.duration_frames = Some((secs * 24.0) as i64); // assuming 24fps
        }
    }

    // Parse streams
    if let Some(streams) = json["streams"].as_array() {
        for stream in streams {
            let codec_type = stream["codec_type"].as_str().unwrap_or("");
            match codec_type {
                "video" => {
                    let w = stream["width"].as_u64().unwrap_or(0) as u32;
                    let h = stream["height"].as_u64().unwrap_or(0) as u32;
                    let (num, den) = parse_r_frame_rate(stream);
                    meta.video = Some(VideoMetadata {
                        width: w,
                        height: h,
                        codec: stream["codec_name"].as_str().unwrap_or("?").to_string(),
                        fps: if den > 0 { num as f64 / den as f64 } else { 24.0 },
                        bitrate_bps: 0,
                        has_audio: false,
                    });
                }
                "audio" => {
                    meta.audio = Some(AudioMetadata {
                        codec: stream["codec_name"].as_str().unwrap_or("?").to_string(),
                        sample_rate: stream["sample_rate"].as_str().and_then(|s| s.parse().ok()).unwrap_or(48000),
                        channels: stream["channels"].as_u64().unwrap_or(2) as u8,
                        bitrate_bps: 0,
                    });
                    if let Some(ref mut v) = meta.video {
                        v.has_audio = true;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(meta)
}

fn parse_r_frame_rate(stream: &serde_json::Value) -> (i64, i64) {
    let s = stream["r_frame_rate"].as_str().unwrap_or("24/1");
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        (parts[0].parse().unwrap_or(24), parts[1].parse().unwrap_or(1))
    } else {
        (24, 1)
    }
}
