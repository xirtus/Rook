//! CPU compositor — mirrors the GPU shader logic for quad transforms,
//! 17 blend modes, and alpha compositing.
//!
//! Uses the same `FrameDescriptor` / `LayerDescriptor` types as the GPU
//! compositor. When wgpu device access is wired into the eframe paint
//! callback, this module can be replaced with the GPU pipeline.

use rook_renderer::compositor::{
    BlendMode, EffectPassDescriptor, EffectUniformValueDescriptor, FrameDescriptor,
    FrameItemDescriptor, LayerDescriptor,
};

/// Composite a frame descriptor into a single RGBA buffer (CPU).
///
/// Returns RGBA bytes in row-major order (width × height × 4).
pub fn composite_frame_cpu(frame: &FrameDescriptor, textures: &TextureBank) -> Vec<u8> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let size = w * h * 4;
    // Scene buffer: premultiplied RGBA, starting with clear color
    let mut scene = vec![0u8; size];

    // Fill with clear color
    let [cr, cg, cb, ca] = frame.clear.color;
    let cr8 = (cr * 255.0).clamp(0.0, 255.0) as u8;
    let cg8 = (cg * 255.0).clamp(0.0, 255.0) as u8;
    let cb8 = (cb * 255.0).clamp(0.0, 255.0) as u8;
    let ca8 = (ca * 255.0).clamp(0.0, 255.0) as u8;
    for i in (0..size).step_by(4) {
        scene[i] = cr8;
        scene[i + 1] = cg8;
        scene[i + 2] = cb8;
        scene[i + 3] = ca8;
    }

    for item in &frame.items {
        match item {
            FrameItemDescriptor::Layer(layer) => {
                let layer_rgba = render_layer_cpu(layer, textures, w, h);
                blend_onto_cpu(&mut scene, &layer_rgba, layer.blend_mode, w, h);
            }
            FrameItemDescriptor::SceneEffect { .. } => {
                // Scene effects not yet implemented in CPU path
            }
        }
    }

    scene
}

/// Renders a single layer: fetches source texture, applies quad transform + effects + opacity.
fn render_layer_cpu(
    layer: &LayerDescriptor,
    textures: &TextureBank,
    canvas_w: usize,
    canvas_h: usize,
) -> Vec<u8> {
    let src = match textures.get(&layer.texture_id) {
        Some(tex) => {
            // eprintln!("[compositor] layer '{}' texture found: {}x{} opacity={}", ...);
            tex
        }
        None => {
            eprintln!(
                "[compositor] layer '{}' texture NOT FOUND — rendering transparent",
                layer.texture_id
            );
            return vec![0u8; canvas_w * canvas_h * 4];
        }
    };

    let mut out = vec![0u8; canvas_w * canvas_h * 4];
    let opacity = layer.opacity.clamp(0.0, 1.0);

    // Precompute inverse transform parameters
    let cx = layer.transform.center_x as f32;
    let cy = layer.transform.center_y as f32;
    let size_w = layer.transform.width;
    let size_h = layer.transform.height;
    let rot_rad = layer.transform.rotation_degrees.to_radians();
    let cos_r = rot_rad.cos();
    let sin_r = rot_rad.sin();
    let flip_x = layer.transform.flip_x;
    let flip_y = layer.transform.flip_y;

    for py in 0..canvas_h {
        for px in 0..canvas_w {
            let idx = (py * canvas_w + px) * 4;

            // Map output pixel to source space (inverse quad transform)
            let dx = px as f32 - cx;
            let dy = py as f32 - cy;

            // Inverse rotation
            let rx = dx * cos_r + dy * sin_r;
            let ry = -dx * sin_r + dy * cos_r;

            // Inverse scale — map from output quad size to source texture size
            let u = if size_w > 0.0 { rx / size_w + 0.5 } else { 0.5 };
            let v = if size_h > 0.0 { ry / size_h + 0.5 } else { 0.5 };

            // Flip
            let u = if flip_x { 1.0 - u } else { u };
            let v = if flip_y { 1.0 - v } else { v };

            // Out of bounds → transparent
            if u < 0.0 || u >= 1.0 || v < 0.0 || v >= 1.0 {
                out[idx] = 0;
                out[idx + 1] = 0;
                out[idx + 2] = 0;
                out[idx + 3] = 0;
                continue;
            }

            // Sample source texture — pass normalized [0,1) coords directly.
            // sample_bilinear internally multiplies by texture width/height.
            let (r, g, b, a) = sample_bilinear(src, u, v);

            out[idx] = r;
            out[idx + 1] = g;
            out[idx + 2] = b;
            out[idx + 3] = a;
        }
    }

    // ── Apply effect passes (CPU) ──────────────────────────────────────
    for group in &layer.effect_pass_groups {
        for pass in group {
            apply_effect_cpu(&mut out, pass, canvas_w, canvas_h);
        }
    }

    // ── Apply opacity ──────────────────────────────────────────────────
    if opacity < 1.0 {
        for i in (0..canvas_w * canvas_h * 4).step_by(4) {
            let a = out[i + 3] as f32 * opacity;
            out[i + 3] = a as u8;
        }
    }

    out
}

/// Bilinear sample from a texture (preserves alpha).
fn sample_bilinear(tex: &StoredTexture, u: f32, v: f32) -> (u8, u8, u8, u8) {
    let w = tex.width as f32;
    let h = tex.height as f32;
    let x = u * w - 0.5;
    let y = v * h - 0.5;
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let get = |px: i32, py: i32| -> (f32, f32, f32, f32) {
        let px = px.clamp(0, tex.width as i32 - 1) as usize;
        let py = py.clamp(0, tex.height as i32 - 1) as usize;
        let si = (py * tex.width as usize + px) * 4;
        if si + 3 < tex.data.len() {
            (
                tex.data[si] as f32,
                tex.data[si + 1] as f32,
                tex.data[si + 2] as f32,
                tex.data[si + 3] as f32,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        }
    };

    let (r00, g00, b00, a00) = get(x0, y0);
    let (r10, g10, b10, a10) = get(x0 + 1, y0);
    let (r01, g01, b01, a01) = get(x0, y0 + 1);
    let (r11, g11, b11, a11) = get(x0 + 1, y0 + 1);

    let r = (r00 * (1.0 - fx) * (1.0 - fy)
        + r10 * fx * (1.0 - fy)
        + r01 * (1.0 - fx) * fy
        + r11 * fx * fy) as u8;
    let g = (g00 * (1.0 - fx) * (1.0 - fy)
        + g10 * fx * (1.0 - fy)
        + g01 * (1.0 - fx) * fy
        + g11 * fx * fy) as u8;
    let b = (b00 * (1.0 - fx) * (1.0 - fy)
        + b10 * fx * (1.0 - fy)
        + b01 * (1.0 - fx) * fy
        + b11 * fx * fy) as u8;
    let a = (a00 * (1.0 - fx) * (1.0 - fy)
        + a10 * fx * (1.0 - fy)
        + a01 * (1.0 - fx) * fy
        + a11 * fx * fy) as u8;

    (r, g, b, a)
}

/// Blend a layer onto the scene using the specified blend mode.
/// Both buffers are in straight (non-premultiplied) RGBA.
fn blend_onto_cpu(scene: &mut [u8], layer: &[u8], mode: BlendMode, w: usize, h: usize) {
    for i in (0..w * h * 4).step_by(4) {
        let lr = layer[i] as f32 / 255.0;
        let lg = layer[i + 1] as f32 / 255.0;
        let lb = layer[i + 2] as f32 / 255.0;
        let la = layer[i + 3] as f32 / 255.0;

        let sr = scene[i] as f32 / 255.0;
        let sg = scene[i + 1] as f32 / 255.0;
        let sb = scene[i + 2] as f32 / 255.0;
        let sa = scene[i + 3] as f32 / 255.0;

        // Blend RGB using the selected mode
        let blended_rgb = blend_rgb(mode, lr, lg, lb, sr, sg, sb);

        // Alpha compositing: over operator
        let out_a = la + sa * (1.0 - la);
        let out_r = if out_a > 0.0 {
            (blended_rgb.0 * la + sr * sa * (1.0 - la)) / out_a
        } else {
            0.0
        };
        let out_g = if out_a > 0.0 {
            (blended_rgb.1 * la + sg * sa * (1.0 - la)) / out_a
        } else {
            0.0
        };
        let out_b = if out_a > 0.0 {
            (blended_rgb.2 * la + sb * sa * (1.0 - la)) / out_a
        } else {
            0.0
        };

        scene[i] = (out_r * 255.0).clamp(0.0, 255.0) as u8;
        scene[i + 1] = (out_g * 255.0).clamp(0.0, 255.0) as u8;
        scene[i + 2] = (out_b * 255.0).clamp(0.0, 255.0) as u8;
        scene[i + 3] = (out_a * 255.0).clamp(0.0, 255.0) as u8;
    }
}

/// Blend two RGB values using the specified blend mode.
/// Returns blended (r, g, b).
fn blend_rgb(
    mode: BlendMode,
    lr: f32,
    lg: f32,
    lb: f32,
    sr: f32,
    sg: f32,
    sb: f32,
) -> (f32, f32, f32) {
    match mode {
        BlendMode::Normal => (lr, lg, lb),
        BlendMode::Darken => (lr.min(sr), lg.min(sg), lb.min(sb)),
        BlendMode::Multiply => (lr * sr, lg * sg, lb * sb),
        BlendMode::ColorBurn => {
            let r = if lr > 0.0 {
                1.0 - ((1.0 - sr) / lr).min(1.0)
            } else {
                0.0
            };
            let g = if lg > 0.0 {
                1.0 - ((1.0 - sg) / lg).min(1.0)
            } else {
                0.0
            };
            let b = if lb > 0.0 {
                1.0 - ((1.0 - sb) / lb).min(1.0)
            } else {
                0.0
            };
            (r, g, b)
        }
        BlendMode::Lighten => (lr.max(sr), lg.max(sg), lb.max(sb)),
        BlendMode::Screen => (
            1.0 - (1.0 - lr) * (1.0 - sr),
            1.0 - (1.0 - lg) * (1.0 - sg),
            1.0 - (1.0 - lb) * (1.0 - sb),
        ),
        BlendMode::PlusLighter => ((lr + sr).min(1.0), (lg + sg).min(1.0), (lb + sb).min(1.0)),
        BlendMode::ColorDodge => {
            let r = if lr < 1.0 {
                (sr / (1.0 - lr)).min(1.0)
            } else {
                1.0
            };
            let g = if lg < 1.0 {
                (sg / (1.0 - lg)).min(1.0)
            } else {
                1.0
            };
            let b = if lb < 1.0 {
                (sb / (1.0 - lb)).min(1.0)
            } else {
                1.0
            };
            (r, g, b)
        }
        BlendMode::Overlay => {
            let r = if sr < 0.5 {
                2.0 * lr * sr
            } else {
                1.0 - 2.0 * (1.0 - lr) * (1.0 - sr)
            };
            let g = if sg < 0.5 {
                2.0 * lg * sg
            } else {
                1.0 - 2.0 * (1.0 - lg) * (1.0 - sg)
            };
            let b = if sb < 0.5 {
                2.0 * lb * sb
            } else {
                1.0 - 2.0 * (1.0 - lb) * (1.0 - sb)
            };
            (r, g, b)
        }
        BlendMode::SoftLight => {
            let soft = |a: f32, b: f32| {
                if a < 0.5 {
                    b - (1.0 - 2.0 * a) * b * (1.0 - b)
                } else {
                    b + (2.0 * a - 1.0)
                        * ((if b < 0.25 {
                            ((16.0 * b - 12.0) * b + 4.0) * b
                        } else {
                            b.sqrt()
                        }) - b)
                }
            };
            (soft(lr, sr), soft(lg, sg), soft(lb, sb))
        }
        BlendMode::HardLight => {
            let hard = |a: f32, b: f32| {
                if a < 0.5 {
                    2.0 * a * b
                } else {
                    1.0 - 2.0 * (1.0 - a) * (1.0 - b)
                }
            };
            (hard(lr, sr), hard(lg, sg), hard(lb, sb))
        }
        BlendMode::Difference => ((lr - sr).abs(), (lg - sg).abs(), (lb - sb).abs()),
        BlendMode::Exclusion => (
            sr + lr - 2.0 * sr * lr,
            sg + lg - 2.0 * sg * lg,
            sb + lb - 2.0 * sb * lb,
        ),
        BlendMode::Hue => {
            let (lh, ls, ll) = rgb_to_hsl(lr, lg, lb);
            let (_, sb_sat, sb_lum) = rgb_to_hsl(sr, sg, sb);
            hsl_to_rgb(lh, sb_sat, sb_lum)
        }
        BlendMode::Saturation => {
            let (lh, ls, ll) = rgb_to_hsl(lr, lg, lb);
            let (sb_h, _, sb_l) = rgb_to_hsl(sr, sg, sb);
            hsl_to_rgb(sb_h, ls, sb_l)
        }
        BlendMode::Color => {
            let (lh, ls, ll) = rgb_to_hsl(lr, lg, lb);
            let (_, _, sb_l) = rgb_to_hsl(sr, sg, sb);
            hsl_to_rgb(lh, ls, sb_l)
        }
        BlendMode::Luminosity => {
            let (lh, ls, ll) = rgb_to_hsl(lr, lg, lb);
            let (sb_h, sb_s, _) = rgb_to_hsl(sr, sg, sb);
            hsl_to_rgb(sb_h, sb_s, ll)
        }
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < 0.0001 {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if max == r {
        (g - b) / d + (if g < b { 6.0 } else { 0.0 })
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;

    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let hue_to_rgb = |p: f32, q: f32, t: f32| -> f32 {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };

    (
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    )
}

// ── Texture bank ──────────────────────────────────────────────────

/// Stores decoded RGBA textures keyed by ID string.
pub struct TextureBank {
    textures: std::collections::HashMap<String, StoredTexture>,
}

pub struct StoredTexture {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl TextureBank {
    pub fn new() -> Self {
        Self {
            textures: std::collections::HashMap::new(),
        }
    }

    pub fn upsert(&mut self, id: String, data: Vec<u8>, width: u32, height: u32) {
        self.textures.insert(
            id,
            StoredTexture {
                data,
                width,
                height,
            },
        );
    }

    pub fn get(&self, id: &str) -> Option<&StoredTexture> {
        self.textures.get(id)
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

impl Default for TextureBank {
    fn default() -> Self {
        Self::new()
    }
}

// ── CPU effect application ────────────────────────────────────────────────

/// Apply a single effect pass to an RGBA pixel buffer in-place.
fn apply_effect_cpu(pixels: &mut [u8], pass: &EffectPassDescriptor, w: usize, h: usize) {
    match pass.shader.as_str() {
        "brightness" => {
            let amt = get_uniform_f32(pass, "amount", 0.0);
            apply_brightness(pixels, amt, w, h);
        }
        "contrast" => {
            let amt = get_uniform_f32(pass, "amount", 0.0);
            apply_contrast(pixels, amt, w, h);
        }
        "saturation" => {
            let amt = get_uniform_f32(pass, "amount", 0.0);
            apply_saturation(pixels, amt, w, h);
        }
        "exposure" => {
            let amt = get_uniform_f32(pass, "amount", 0.0);
            apply_exposure(pixels, amt, w, h);
        }
        "hue-rotate" => {
            let deg = get_uniform_f32(pass, "degrees", 0.0);
            apply_hue_rotate(pixels, deg, w, h);
        }
        "color-balance" => {
            let r = get_uniform_f32(pass, "red", 0.0);
            let g = get_uniform_f32(pass, "green", 0.0);
            let b = get_uniform_f32(pass, "blue", 0.0);
            apply_color_balance(pixels, r, g, b, w, h);
        }
        "gaussian-blur" => {
            let sigma = get_uniform_f32(pass, "sigma", 5.0);
            let dir = get_uniform_vec2_or(pass, "direction", [1.0, 1.0]);
            apply_gaussian_blur(pixels, sigma, dir[0] > 0.5, dir[1] > 0.5, w, h);
        }
        "sharpen" => {
            let amt = get_uniform_f32(pass, "amount", 0.5);
            apply_sharpen(pixels, amt, w, h);
        }
        "vignette" => {
            let strength = get_uniform_f32(pass, "strength", 0.5);
            apply_vignette(pixels, strength, w, h);
        }
        "film-grain" => {
            let amt = get_uniform_f32(pass, "amount", 0.1);
            apply_film_grain(pixels, amt, w, h);
        }
        "chroma-key" => {
            let key_hue = get_uniform_f32(pass, "key_hue", 120.0);
            let tolerance = get_uniform_f32(pass, "tolerance", 30.0);
            apply_chroma_key(pixels, key_hue, tolerance, w, h);
        }
        _ => {} // Unknown shader — no-op
    }
}

fn get_uniform_f32(pass: &EffectPassDescriptor, key: &str, default: f32) -> f32 {
    pass.uniforms
        .get(key)
        .and_then(|v| match v {
            EffectUniformValueDescriptor::Number(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(default)
}

fn get_uniform_vec2_or(pass: &EffectPassDescriptor, key: &str, default: [f32; 2]) -> [f32; 2] {
    pass.uniforms
        .get(key)
        .and_then(|v| match v {
            EffectUniformValueDescriptor::Vector(vec) if vec.len() >= 2 => Some([vec[0], vec[1]]),
            _ => None,
        })
        .unwrap_or(default)
}

// ── Individual effect implementations ─────────────────────────────────────

/// Brightness: add `amount` to each channel. Range -1..1 → -255..255.
fn apply_brightness(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    let adj = (amount * 255.0) as i16;
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        for ch in 0..3 {
            let val = pixels[i + ch] as i16 + adj;
            pixels[i + ch] = val.clamp(0, 255) as u8;
        }
    }
}

/// Contrast: scale around 128. amount -1..1 → 0..2× contrast.
fn apply_contrast(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    let factor = 1.0 + amount; // 0=1×, 1=2×, -1=0×
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        for ch in 0..3 {
            let val = (pixels[i + ch] as f32 - 128.0) * factor + 128.0;
            pixels[i + ch] = val.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Saturation: amount -1=grayscale, 0=normal, 1=2× saturation.
fn apply_saturation(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    let factor = 1.0 + amount;
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        let r = pixels[i] as f32;
        let g = pixels[i + 1] as f32;
        let b = pixels[i + 2] as f32;
        let gray = 0.299 * r + 0.587 * g + 0.114 * b;
        pixels[i] = (gray + (r - gray) * factor).clamp(0.0, 255.0) as u8;
        pixels[i + 1] = (gray + (g - gray) * factor).clamp(0.0, 255.0) as u8;
        pixels[i + 2] = (gray + (b - gray) * factor).clamp(0.0, 255.0) as u8;
    }
}

/// Exposure: 2^amount multiplier. -5..5 stops.
fn apply_exposure(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    let factor = 2.0f32.powf(amount);
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        for ch in 0..3 {
            let val = pixels[i + ch] as f32 * factor;
            pixels[i + ch] = val.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Hue rotate: shift hue by `degrees` in HSL space.
fn apply_hue_rotate(pixels: &mut [u8], degrees: f32, w: usize, h: usize) {
    if degrees.abs() < 0.01 {
        return;
    }
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        let r = pixels[i] as f32 / 255.0;
        let g = pixels[i + 1] as f32 / 255.0;
        let b = pixels[i + 2] as f32 / 255.0;
        let (h, s, l) = rgb_to_hsl(r, g, b);
        let nh = (h * 360.0 + degrees) % 360.0;
        let nh = if nh < 0.0 { nh + 360.0 } else { nh } / 360.0;
        let (nr, ng, nb) = hsl_to_rgb(nh, s, l);
        pixels[i] = (nr * 255.0).clamp(0.0, 255.0) as u8;
        pixels[i + 1] = (ng * 255.0).clamp(0.0, 255.0) as u8;
        pixels[i + 2] = (nb * 255.0).clamp(0.0, 255.0) as u8;
    }
}

/// Color balance: add `r`, `g`, `b` to each channel. Range -1..1 mapped to -255..255.
fn apply_color_balance(pixels: &mut [u8], r_adj: f32, g_adj: f32, b_adj: f32, w: usize, h: usize) {
    let adj = [
        (r_adj * 255.0) as i16,
        (g_adj * 255.0) as i16,
        (b_adj * 255.0) as i16,
    ];
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        for ch in 0..3 {
            let val = pixels[i + ch] as i16 + adj[ch];
            pixels[i + ch] = val.clamp(0, 255) as u8;
        }
    }
}

/// Gaussian blur: separable box-blur approximation.
fn apply_gaussian_blur(
    pixels: &mut [u8],
    sigma: f32,
    horizontal: bool,
    vertical: bool,
    w: usize,
    h: usize,
) {
    if sigma < 0.5 || (!horizontal && !vertical) {
        return;
    }
    let radius = (sigma * 3.0f32).ceil() as usize;
    let tmp = if horizontal && vertical {
        Some(pixels.to_vec())
    } else {
        None
    };

    if horizontal {
        blur_pass(pixels, &tmp, radius, w, h, true);
    }
    if vertical {
        if horizontal {
            if let Some(ref t) = tmp {
                pixels.copy_from_slice(t);
            }
            blur_pass(pixels, &None, radius, w, h, false);
        } else {
            blur_pass(pixels, &None, radius, w, h, false);
        }
    }
}

fn blur_pass(
    pixels: &mut [u8],
    tmp: &Option<Vec<u8>>,
    radius: usize,
    w: usize,
    h: usize,
    horiz: bool,
) {
    let tmp_slice: Option<&[u8]> = tmp.as_ref().map(|v| v.as_slice());
    let src: &[u8] = match tmp_slice {
        Some(s) => s,
        None => pixels,
    };
    let len = w * h * 4;
    let mut out = vec![0u8; len];
    let kernel_size = radius * 2 + 1;

    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) * 4;
            if horiz {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for k in 0..kernel_size {
                    let sx = (col + k).wrapping_sub(radius);
                    if sx < w {
                        let si = (row * w + sx) * 4;
                        if si + 3 < len {
                            for ch in 0..4 {
                                sum[ch] += src[si + ch] as u32;
                            }
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    for ch in 0..4 {
                        out[idx + ch] = (sum[ch] / count) as u8;
                    }
                }
            } else {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for k in 0..kernel_size {
                    let sy = (row + k).wrapping_sub(radius);
                    if sy < h {
                        let si = (sy * w + col) * 4;
                        if si + 3 < len {
                            for ch in 0..4 {
                                sum[ch] += src[si + ch] as u32;
                            }
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    for ch in 0..4 {
                        out[idx + ch] = (sum[ch] / count) as u8;
                    }
                }
            }
        }
    }
    pixels.copy_from_slice(&out);
}

/// Sharpen: unsharp mask. amount 0..1 controls intensity.
fn apply_sharpen(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    if amount < 0.01 {
        return;
    }
    let orig = pixels.to_vec();
    // First blur the original
    let mut blurred = orig.clone();
    let sigma = 1.5f32;
    let radius = (sigma * 3.0f32).ceil() as usize;
    blur_pass(&mut blurred, &None, radius, w, h, true);
    blur_pass(&mut blurred, &None, radius, w, h, false);

    for i in (0..w * h * 4).step_by(4) {
        if orig[i + 3] == 0 {
            continue;
        }
        for ch in 0..3 {
            let orig_val = orig[i + ch] as f32;
            let blur_val = blurred[i + ch] as f32;
            let sharp = orig_val + (orig_val - blur_val) * amount;
            pixels[i + ch] = sharp.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Vignette: darken edges. strength 0..1.
fn apply_vignette(pixels: &mut [u8], strength: f32, w: usize, h: usize) {
    if strength < 0.01 {
        return;
    }
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            if pixels[idx + 3] == 0 {
                continue;
            }
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt() / max_r;
            let vignette = 1.0 - (dist * strength).min(1.0);
            for ch in 0..3 {
                pixels[idx + ch] = (pixels[idx + ch] as f32 * vignette).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Film grain: random noise overlay. amount 0..1.
fn apply_film_grain(pixels: &mut [u8], amount: f32, w: usize, h: usize) {
    if amount < 0.001 {
        return;
    }
    // Simple deterministic pseudo-random per pixel
    let mut seed: u32 = 0xDEADBEEF;
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let noise = ((seed >> 16) as f32 / 65535.0 - 0.5) * 2.0 * amount * 255.0;
        for ch in 0..3 {
            let val = pixels[i + ch] as f32 + noise;
            pixels[i + ch] = val.clamp(0.0, 255.0) as u8;
        }
    }
}

/// Chroma key: key out a specific hue range. key_hue 0..360, tolerance degrees.
fn apply_chroma_key(pixels: &mut [u8], key_hue: f32, tolerance: f32, w: usize, h: usize) {
    let half_tol = tolerance / 2.0;
    for i in (0..w * h * 4).step_by(4) {
        if pixels[i + 3] == 0 {
            continue;
        }
        let r = pixels[i] as f32 / 255.0;
        let g = pixels[i + 1] as f32 / 255.0;
        let b = pixels[i + 2] as f32 / 255.0;
        let (h, _, _) = rgb_to_hsl(r, g, b);
        let hue_deg = h * 360.0;
        // Check if hue is within tolerance of key_hue
        let hue_diff = (hue_deg - key_hue).abs();
        let hue_diff = hue_diff.min(360.0 - hue_diff);
        if hue_diff < half_tol {
            // Feather alpha based on distance to tolerance edge
            let alpha = ((hue_diff / half_tol).clamp(0.0, 1.0)) as u8;
            pixels[i + 3] = (pixels[i + 3] as f32 * alpha as f32 / 255.0) as u8;
            // Also desaturate in the key region
            if hue_diff < half_tol * 0.5 {
                let gray = (0.299 * r + 0.587 * g + 0.114 * b) * 255.0;
                pixels[i] = gray as u8;
                pixels[i + 1] = gray as u8;
                pixels[i + 2] = gray as u8;
            }
        }
    }
}
