//! 2D transform for clips — position, scale, rotation, anchor point.
//! Adapted from verbreel-state.

use serde::{Deserialize, Serialize};

/// Where the anchor sits inside the clip rect, normalised 0–1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnchorPoint {
    pub x: f32,
    pub y: f32,
}

impl Default for AnchorPoint {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

/// Clip transform: position, scale, rotation, cropping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Position in canvas pixels (top-left of the anchor).
    #[serde(default)]
    pub position: Position,
    /// Scale multipliers (1.0 = 100 %).
    #[serde(default = "default_scale")]
    pub scale: Scale,
    /// Rotation in degrees clockwise.
    #[serde(default)]
    pub rotation_deg: f32,
    /// Anchor point (normalised 0–1 within the clip rect).
    #[serde(default)]
    pub anchor: AnchorPoint,
    /// Crop margins in pixels (top, right, bottom, left).
    #[serde(default)]
    pub crop: Crop,
    /// Opacity 0.0–1.0.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Flip horizontal / vertical.
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

fn default_scale() -> Scale { Scale { x: 1.0, y: 1.0 } }
fn default_opacity() -> f32 { 1.0 }

impl Default for Position {
    fn default() -> Self { Self { x: 0.0, y: 0.0 } }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Position::default(),
            scale: default_scale(),
            rotation_deg: 0.0,
            anchor: AnchorPoint::default(),
            crop: Crop::default(),
            opacity: 1.0,
            flip_h: false,
            flip_v: false,
        }
    }
}
