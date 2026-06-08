//! GPU effect pipeline — post-processing effects for layers and scenes.
//!
//! Each effect is a series of render passes that transform a source
//! texture into a destination texture. Passes can be chained: the output
//! of pass N becomes the input of pass N+1.
//!
//! ## Adding a new effect
//!
//! 1. Write a WGSL shader in `effects/` (e.g., `sharpen.wgsl`).
//! 2. Register its identifier in the `EffectPipeline` shader map.
//! 3. Define the uniforms the shader expects (mapped in `pack_effect_uniforms`).
//!
//! ## Current shaders
//!
//! | Identifier       | File                          | Description                      |
//! |------------------|-------------------------------|----------------------------------|
//! | `gaussian-blur`  | `effects/gaussian_blur.wgsl`  | Separable Gaussian blur (H + V) |
//!
//! Vendored and adapted from koughen/Editor (MIT).

use crate::compositor::EffectPassDescriptor;
use std::collections::HashMap;

// ── Public types ──────────────────────────────────────────────────────────

/// A resolved effect pass ready for GPU execution.
#[derive(Debug, Clone)]
pub struct EffectPass {
    /// Shader identifier (e.g., "gaussian-blur").
    pub shader: String,
    /// Resolved uniform values.
    pub uniforms: HashMap<String, UniformValue>,
}

/// A uniform value for a WGSL shader.
#[derive(Debug, Clone)]
pub enum UniformValue {
    /// A scalar float (maps to `f32` in WGSL).
    Number(f32),
    /// A float vector (maps to `vecN<f32>` in WGSL).
    Vector(Vec<f32>),
}

/// Shader identifiers for known effects.
pub mod shader_ids {
    pub const GAUSSIAN_BLUR: &str = "gaussian-blur";
}

/// Convert `EffectPassDescriptor`s (from the frame description) into
/// resolved `EffectPass`es.
pub fn resolve_passes(passes: &[EffectPassDescriptor]) -> Vec<EffectPass> {
    passes
        .iter()
        .map(|pass| EffectPass {
            shader: pass.shader.clone(),
            uniforms: pass
                .uniforms
                .iter()
                .map(|(name, value)| {
                    let uniform = match value {
                        crate::compositor::EffectUniformValueDescriptor::Number(n) => {
                            UniformValue::Number(*n)
                        }
                        crate::compositor::EffectUniformValueDescriptor::Vector(v) => {
                            UniformValue::Vector(v.clone())
                        }
                    };
                    (name.clone(), uniform)
                })
                .collect(),
        })
        .collect()
}

/// Gaussian blur effect configuration.
///
/// The blur is applied as two passes (horizontal + vertical) using
/// the separable Gaussian kernel. The `sigma` parameter controls the
/// blur radius, and `step` controls sampling density on the kernel.
#[derive(Debug, Clone, Copy)]
pub struct GaussianBlurConfig {
    /// Standard deviation of the Gaussian kernel.
    pub sigma: f32,
    /// Step size between samples along the kernel (higher = faster but
    /// more approximate). Typical: 1.0–4.0.
    pub step: f32,
}

impl Default for GaussianBlurConfig {
    fn default() -> Self {
        Self { sigma: 2.0, step: 1.0 }
    }
}

impl GaussianBlurConfig {
    /// Build a horizontal + vertical blur pass group.
    ///
    /// Returns a `Vec<EffectPassDescriptor>` that can be placed in a
    /// layer's `effect_pass_groups`.
    pub fn to_pass_group(&self) -> Vec<EffectPassDescriptor> {
        let sigma_uniform = crate::compositor::EffectUniformValueDescriptor::Number(self.sigma);
        let step_uniform = crate::compositor::EffectUniformValueDescriptor::Number(self.step);

        let h_pass = EffectPassDescriptor {
            shader: shader_ids::GAUSSIAN_BLUR.to_string(),
            uniforms: HashMap::from([
                ("u_sigma".to_string(), sigma_uniform.clone()),
                ("u_step".to_string(), step_uniform.clone()),
                ("u_direction".to_string(), crate::compositor::EffectUniformValueDescriptor::Vector(vec![1.0, 0.0])),
            ]),
        };

        let v_pass = EffectPassDescriptor {
            shader: shader_ids::GAUSSIAN_BLUR.to_string(),
            uniforms: HashMap::from([
                ("u_sigma".to_string(), sigma_uniform),
                ("u_step".to_string(), step_uniform),
                ("u_direction".to_string(), crate::compositor::EffectUniformValueDescriptor::Vector(vec![0.0, 1.0])),
            ]),
        };

        vec![h_pass, v_pass]
    }
}

// ── Embedded shader sources ───────────────────────────────────────────────

pub const GAUSSIAN_BLUR_SHADER: &str = include_str!("gaussian_blur.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_blur_pass_group_creates_two_passes() {
        let config = GaussianBlurConfig { sigma: 2.0, step: 1.0 };
        let passes = config.to_pass_group();
        assert_eq!(passes.len(), 2);
        assert_eq!(passes[0].shader, "gaussian-blur");
        assert_eq!(passes[1].shader, "gaussian-blur");

        // H pass has x-direction
        let h_dir = passes[0].uniforms.get("u_direction").unwrap();
        if let crate::compositor::EffectUniformValueDescriptor::Vector(ref v) = h_dir {
            assert_eq!(v, &vec![1.0, 0.0]);
        }

        // V pass has y-direction
        let v_dir = passes[1].uniforms.get("u_direction").unwrap();
        if let crate::compositor::EffectUniformValueDescriptor::Vector(ref v) = v_dir {
            assert_eq!(v, &vec![0.0, 1.0]);
        }
    }
}
