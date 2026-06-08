use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("no video stream found in {0}")]
    NoVideoStream(PathBuf),

    #[error("decode failed at frame {frame}: {reason}")]
    DecodeFailed { frame: i64, reason: String },

    #[error("seek failed: {0}")]
    SeekFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),

    #[error("module is a stub — real decode not linked")]
    Stub,
}
