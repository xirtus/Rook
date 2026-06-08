//! Decoded frame — owned RGBA pixel data + optional audio.

/// A single decoded frame (video + optional audio).
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data, row-major, `width * height * 4` bytes.
    pub data: Vec<u8>,
    /// Presentation timestamp in the source's time base.
    pub pts: i64,
    /// Optional audio samples interleaved with this frame.
    pub audio: Option<AudioFrame>,
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u8,
}
