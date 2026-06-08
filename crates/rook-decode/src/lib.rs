//! # Rook Decode — FFmpeg media decode layer.
//!
//! Thin wrapper around `ffmpeg-next`.  Exposes:
//! * `Decoder` — open a media file, seek to frame, decode
//!   (auto-detects GPU: CUDA/NVDEC → VAAPI → software)
//! * `HwAccelBackend` — which backend was selected
//! * `cuda_available()` / `vaapi_available()` — probe specific backends
//! * `probe()` — extract metadata without full decode
//! * `DecodedFrame` — owned RGBA image + PCM audio
//!
//! ## Stub mode
//!
//! Without `ffmpeg-next` linked (or when FFmpeg is absent), all functions
//! return `DecodeError::Stub`.

mod decoder;
mod error;
mod frame;
mod probe;

pub use decoder::Decoder;
pub use decoder::HwAccelBackend;
pub use decoder::cuda_available;
pub use decoder::vaapi_available;
pub use error::DecodeError;
pub use frame::DecodedFrame;
pub use probe::probe;
