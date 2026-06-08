//! Canvas — the output frame dimensions and composition space.
//! Adapted from verbreel-state's `Canvas` type + OpenReelio's canvas config.

use serde::{Deserialize, Serialize};

/// The composition canvas: output resolution, background, and pixel aspect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    /// Background colour when no clips cover a region (sRGBA, 0–255).
    #[serde(default = "default_background")]
    pub background: [u8; 4],
    /// Pixel aspect ratio (1.0 = square pixels).
    #[serde(default = "default_par")]
    pub pixel_aspect: f32,
}

fn default_background() -> [u8; 4] {
    [0, 0, 0, 255]
}

fn default_par() -> f32 {
    1.0
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            background: [0, 0, 0, 255],
            pixel_aspect: 1.0,
        }
    }
}

impl Canvas {
    pub const HD_720P: Self = Self { width: 1280, height: 720, background: [0, 0, 0, 255], pixel_aspect: 1.0 };
    pub const HD_1080P: Self = Self { width: 1920, height: 1080, background: [0, 0, 0, 255], pixel_aspect: 1.0 };
    pub const UHD_4K: Self = Self { width: 3840, height: 2160, background: [0, 0, 0, 255], pixel_aspect: 1.0 };
    pub const VERTICAL_9x16: Self = Self { width: 1080, height: 1920, background: [0, 0, 0, 255], pixel_aspect: 1.0 };
    pub const SQUARE: Self = Self { width: 1080, height: 1080, background: [0, 0, 0, 255], pixel_aspect: 1.0 };

    /// Proxy resolution: roughly 1/3 linear dimension, keeping aspect.
    pub fn proxy(&self) -> Self {
        let scale = 360.0 / self.height.min(self.width) as f32;
        Self {
            width: (self.width as f32 * scale).round() as u32 & !1,  // even
            height: (self.height as f32 * scale).round() as u32 & !1,
            ..*self
        }
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}
