//! GPU compositor module — layer composition, blend modes, and frame descriptors.
//!
//! Takes a `FrameDescriptor` (a structured list of layers with transforms,
//! blend modes, effects, and masks) and composites them into a single
//! output texture using wgpu render passes.
//!
//! ## Architecture
//!
//! ```text
//! FrameDescriptor → Compositor::render_frame()
//!   ├─ Clear texture (background color)
//!   ├─ For each FrameItem:
//!   │   ├─ Layer → render_layer()
//!   │   │   ├─ Render source texture with quad transform (layer.wgsl)
//!   │   │   ├─ Apply effect pass groups (gaussian_blur.wgsl, etc.)
//!   │   │   ├─ Apply mask (mask.wgsl)
//!   │   │   └─ Blend onto scene (blend.wgsl — 17 blend modes)
//!   │   └─ SceneEffect → apply effects to entire scene
//!   └─ Blit scene to surface / render target
//! ```
//!
//! Vendored and adapted from koughen/Editor (MIT).

pub mod blend_mode;
pub mod frame;
// Pipeline requires wgpu >= 29 (enabled via "compositor-pipeline" feature).
#[cfg(feature = "compositor-pipeline")]
pub mod pipeline;

pub use blend_mode::BlendMode;
pub use frame::{
    CanvasClearDescriptor, EffectPassDescriptor, EffectUniformValueDescriptor,
    FrameDescriptor, FrameItemDescriptor, LayerDescriptor, LayerMaskDescriptor,
    QuadTransformDescriptor,
};
#[cfg(feature = "compositor-pipeline")]
pub use pipeline::{Compositor, CompositorError, TextureStore};

pub mod shaders {
    //! WGSL shader sources embedded at compile time.
    pub const LAYER_SHADER: &str = include_str!("layer.wgsl");
    pub const BLEND_SHADER: &str = include_str!("blend.wgsl");
    pub const MASK_SHADER: &str = include_str!("mask.wgsl");
}
