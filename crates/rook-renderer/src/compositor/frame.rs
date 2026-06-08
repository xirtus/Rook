//! Frame descriptor types for the GPU compositor.
//!
//! These are the data types that flow from the timeline engine to the
//! wgpu compositor. They describe *what* to render, not *how* (that's
//! in the WGSL shaders and render pipeline code).
//!
//! Vendored and adapted from koughen/Editor (MIT).

use crate::compositor::BlendMode;
use std::collections::HashMap;

/// A complete frame to render — a canvas with layers and effects.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameDescriptor {
    pub width: u32,
    pub height: u32,
    pub clear: CanvasClearDescriptor,
    pub items: Vec<FrameItemDescriptor>,
}

/// Background clear color for the canvas.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasClearDescriptor {
    /// RGBA color [r, g, b, a] in 0..1 range.
    pub color: [f32; 4],
}

/// An item on the frame — either a layer or a scene-wide effect group.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FrameItemDescriptor {
    /// A renderable layer (video clip, text, shape, etc.).
    Layer(LayerDescriptor),
    /// Scene-wide effect passes applied to the accumulated canvas.
    SceneEffect {
        /// Groups of effect passes. Passes within a group are sequential;
        /// groups are parallelizable.
        effect_pass_groups: Vec<Vec<EffectPassDescriptor>>,
    },
}

/// Describes a single layer in the compositor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerDescriptor {
    /// The texture ID (looked up in the TextureStore).
    pub texture_id: String,
    /// Position, scale, rotation, flip.
    pub transform: QuadTransformDescriptor,
    /// Overall layer opacity (0.0 = invisible, 1.0 = opaque).
    pub opacity: f32,
    /// How this layer blends with the scene.
    pub blend_mode: BlendMode,
    /// Effect passes applied to this layer before blending.
    #[serde(default)]
    pub effect_pass_groups: Vec<Vec<EffectPassDescriptor>>,
    /// Optional mask applied to this layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<LayerMaskDescriptor>,
}

/// 2D quad transform (position, scale, rotation, flip).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuadTransformDescriptor {
    /// Center X position in canvas pixels.
    pub center_x: f32,
    /// Center Y position in canvas pixels.
    pub center_y: f32,
    /// Display width in canvas pixels.
    pub width: f32,
    /// Display height in canvas pixels.
    pub height: f32,
    /// Rotation in degrees.
    pub rotation_degrees: f32,
    /// Flip horizontally.
    #[serde(default)]
    pub flip_x: bool,
    /// Flip vertically.
    #[serde(default)]
    pub flip_y: bool,
}

impl Default for QuadTransformDescriptor {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            width: 100.0,
            height: 100.0,
            rotation_degrees: 0.0,
            flip_x: false,
            flip_y: false,
        }
    }
}

/// A mask applied to a layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerMaskDescriptor {
    /// Texture ID of the mask source.
    pub texture_id: String,
    /// Feather radius in pixels.
    pub feather: f32,
    /// Whether to invert the mask.
    #[serde(default)]
    pub inverted: bool,
}

/// A single effect pass within a group.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectPassDescriptor {
    /// Shader identifier (e.g., "gaussian-blur").
    pub shader: String,
    /// Uniform values for this pass.
    pub uniforms: HashMap<String, EffectUniformValueDescriptor>,
}

/// Uniform value descriptor (before converting to GPU data).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum EffectUniformValueDescriptor {
    /// A single float value.
    Number(f32),
    /// A vector of floats.
    Vector(Vec<f32>),
}
