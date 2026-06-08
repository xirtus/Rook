//! Preview panel — video output + transport controls.
//! Uses rook-renderer for GPU compositing and rook-decoder-native for hardware decode.

use rook_core::clip::ClipId;
use rook_core::transform::Transform;
use rook_engine::Engine;
use rook_renderer::compositor::{
    CanvasClearDescriptor, EffectPassDescriptor, EffectUniformValueDescriptor, FrameDescriptor,
    FrameItemDescriptor, LayerDescriptor, QuadTransformDescriptor,
};

use super::timeline::Tool;
use super::video_preview::VideoPreviewBridge;
use crate::widgets::waveform::WaveformCache;

fn last_valid_timeline_frame(duration: i64) -> i64 {
    duration.saturating_sub(1).max(0)
}

/// Build a `FrameDescriptor` from the project state at the given frame.
///
/// Handles transitions between clips by adding overlapping layers with
/// opacity ramps during the transition window.
pub fn build_frame_descriptor(
    project: &rook_core::project::Project,
    frame: i64,
    canvas_w: u32,
    canvas_h: u32,
) -> FrameDescriptor {
    let mut items = Vec::new();
    let mut text_items = Vec::new(); // Subtitle/text layers rendered on top
    let fps = project.frame_rate.as_f64();

    for track in &project.timeline.tracks {
        if !track.visible || track.disabled {
            continue;
        }

        // Text track layers render on top
        let target_items: &mut Vec<FrameItemDescriptor> =
            if track.kind == rook_core::track::TrackKind::Text {
                &mut text_items
            } else {
                &mut items
            };

        // Pre-scan: find clips with transitions and their previous neighbors
        let mut transition_pairs: Vec<(usize, usize, i64, rook_core::clip::TransitionKind)> =
            Vec::new();
        for i in 0..track.clips.len() {
            if let Some(ref transition) = track.clips[i].transition {
                // Look for previous clip on same track that ends where this clip starts
                let clip_start = track.clips[i].timeline_in;
                for j in 0..i {
                    let prev_end = track.clips[j].timeline_in + track.clips[j].duration();
                    if prev_end == clip_start {
                        transition_pairs.push((j, i, transition.duration_frames, transition.kind));
                    }
                }
            }
        }

        for (clip_idx, clip) in track.clips.iter().enumerate() {
            if !clip.covers(frame) {
                continue;
            }

            let transform = QuadTransformDescriptor {
                center_x: clip.transform.position.x
                    + clip.transform.scale.x * canvas_w as f32 / 2.0,
                center_y: clip.transform.position.y
                    + clip.transform.scale.y * canvas_h as f32 / 2.0,
                width: clip.transform.scale.x * canvas_w as f32,
                height: clip.transform.scale.y * canvas_h as f32,
                rotation_degrees: clip.transform.rotation_deg,
                flip_x: clip.transform.flip_h,
                flip_y: clip.transform.flip_v,
            };

            let blend = map_blend_to_compositor(clip.blend_mode);

            // Apply fade-in/out opacity ramp
            let mut opacity = clip.transform.opacity;
            if let Some(ref fade) = clip.fade {
                let frame_in_clip = frame - clip.timeline_in;
                let clip_dur = clip.duration().max(1);
                if fade.in_frames > 0 && frame_in_clip < fade.in_frames {
                    opacity *= frame_in_clip as f32 / fade.in_frames as f32;
                }
                if fade.out_frames > 0 && frame_in_clip > clip_dur - fade.out_frames {
                    let frames_from_end = clip_dur - frame_in_clip;
                    opacity *= frames_from_end as f32 / fade.out_frames as f32;
                }
            }

            // Check if this clip is in a transition as the incoming clip
            for &(prev_idx, this_idx, dur_frames, kind) in &transition_pairs {
                if this_idx == clip_idx {
                    let transition_start = clip.timeline_in;
                    let transition_end = transition_start + dur_frames;
                    if frame >= transition_start && frame < transition_end {
                        let progress = (frame - transition_start) as f32 / dur_frames as f32;
                        // Fade in the current clip during transition
                        match kind {
                            rook_core::clip::TransitionKind::CrossDissolve
                            | rook_core::clip::TransitionKind::Dissolve => {
                                opacity *= progress;
                            }
                            rook_core::clip::TransitionKind::Wipe => {
                                // Wipe: clip becomes fully visible after wipe passes
                                opacity *= if progress > 0.5 { 1.0 } else { 0.0 };
                            }
                            rook_core::clip::TransitionKind::Slide => {
                                // Slide: clip slides in from left
                                opacity *= (progress * 2.0).min(1.0);
                            }
                        }
                    }
                }
            }

            // ── Build effect pass groups from clip filters ────────────
            let effect_passes: Vec<Vec<EffectPassDescriptor>> = if clip.filters.is_empty() {
                vec![]
            } else {
                let passes: Vec<EffectPassDescriptor> = clip
                    .filters
                    .iter()
                    .filter(|f| f.enabled)
                    .filter_map(|f| effect_to_pass(f))
                    .collect();
                if passes.is_empty() {
                    vec![]
                } else {
                    vec![passes]
                }
            };

            target_items.push(FrameItemDescriptor::Layer(LayerDescriptor {
                texture_id: format!("clip_{}", clip.id.0),
                transform,
                opacity,
                blend_mode: blend,
                effect_pass_groups: effect_passes,
                mask: None,
            }));
        }

        // ── Transition layers: previous clip extended into transition window ──
        for &(prev_idx, this_idx, dur_frames, kind) in &transition_pairs {
            let prev_clip = &track.clips[prev_idx];
            let this_clip = &track.clips[this_idx];
            let transition_start = this_clip.timeline_in;
            let transition_end = transition_start + dur_frames;

            if frame >= transition_start && frame < transition_end {
                // Extend the previous clip into the transition
                let progress = (frame - transition_start) as f32 / dur_frames as f32;
                let prev_frame_in_clip = frame - prev_clip.timeline_in;
                let prev_clip_dur = prev_clip.duration().max(1);

                // Compute opacity for previous clip during transition
                let mut prev_opacity = prev_clip.transform.opacity;
                // Apply any existing fade-out on the previous clip
                if let Some(ref fade) = prev_clip.fade {
                    if fade.out_frames > 0 && prev_frame_in_clip > prev_clip_dur - fade.out_frames {
                        let frames_from_end = prev_clip_dur - prev_frame_in_clip;
                        prev_opacity *= frames_from_end as f32 / fade.out_frames as f32;
                    }
                }
                // Transition fade-out
                match kind {
                    rook_core::clip::TransitionKind::CrossDissolve
                    | rook_core::clip::TransitionKind::Dissolve => {
                        prev_opacity *= 1.0 - progress;
                    }
                    rook_core::clip::TransitionKind::Wipe => {
                        prev_opacity *= if progress > 0.5 { 0.0 } else { 1.0 };
                    }
                    rook_core::clip::TransitionKind::Slide => {
                        prev_opacity *= (1.0 - progress * 2.0).max(0.0);
                    }
                }

                if prev_opacity > 0.001 {
                    let prev_transform = QuadTransformDescriptor {
                        center_x: prev_clip.transform.position.x
                            + prev_clip.transform.scale.x * canvas_w as f32 / 2.0,
                        center_y: prev_clip.transform.position.y
                            + prev_clip.transform.scale.y * canvas_h as f32 / 2.0,
                        width: prev_clip.transform.scale.x * canvas_w as f32,
                        height: prev_clip.transform.scale.y * canvas_h as f32,
                        rotation_degrees: prev_clip.transform.rotation_deg,
                        flip_x: prev_clip.transform.flip_h,
                        flip_y: prev_clip.transform.flip_v,
                    };

                    // Apply slide offset for Slide transition
                    let slide_transform = match kind {
                        rook_core::clip::TransitionKind::Slide => {
                            let sx = (1.0 - progress) * canvas_w as f32;
                            QuadTransformDescriptor {
                                center_x: prev_transform.center_x - sx,
                                ..prev_transform
                            }
                        }
                        _ => prev_transform,
                    };

                    // Build effect passes for previous clip too
                    let prev_passes: Vec<Vec<EffectPassDescriptor>> =
                        if prev_clip.filters.is_empty() {
                            vec![]
                        } else {
                            let p: Vec<EffectPassDescriptor> = prev_clip
                                .filters
                                .iter()
                                .filter(|f| f.enabled)
                                .filter_map(|f| effect_to_pass(f))
                                .collect();
                            if p.is_empty() { vec![] } else { vec![p] }
                        };

                    target_items.push(FrameItemDescriptor::Layer(LayerDescriptor {
                        texture_id: format!("clip_{}", prev_clip.id.0),
                        transform: slide_transform,
                        opacity: prev_opacity,
                        blend_mode: map_blend_to_compositor(prev_clip.blend_mode),
                        effect_pass_groups: prev_passes,
                        mask: None,
                    }));
                }
            }
        }
    }

    // Append subtitle/text layers on top
    items.append(&mut text_items);

    FrameDescriptor {
        width: canvas_w,
        height: canvas_h,
        clear: CanvasClearDescriptor {
            color: [0.0, 0.0, 0.0, 1.0],
        },
        items,
    }
}

/// Convert a clip effect instance to a compositor effect pass descriptor.
fn effect_to_pass(effect: &rook_core::effect::EffectInstance) -> Option<EffectPassDescriptor> {
    let mut uniforms = std::collections::HashMap::new();
    let params = &effect.params;

    let shader = match &effect.kind {
        rook_core::effect::EffectKind::Brightness => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "brightness"
        }
        rook_core::effect::EffectKind::Contrast => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "contrast"
        }
        rook_core::effect::EffectKind::Saturation => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "saturation"
        }
        rook_core::effect::EffectKind::Exposure => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "exposure"
        }
        rook_core::effect::EffectKind::HueRotate => {
            let deg = params
                .get("degrees")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            uniforms.insert("degrees".into(), EffectUniformValueDescriptor::Number(deg));
            "hue-rotate"
        }
        rook_core::effect::EffectKind::ColorBalance => {
            let r = params.get("red").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let g = params.get("green").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let b = params.get("blue").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            uniforms.insert("red".into(), EffectUniformValueDescriptor::Number(r));
            uniforms.insert("green".into(), EffectUniformValueDescriptor::Number(g));
            uniforms.insert("blue".into(), EffectUniformValueDescriptor::Number(b));
            "color-balance"
        }
        rook_core::effect::EffectKind::GaussianBlur => {
            let sigma = params.get("sigma").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
            let dir = params
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("both");
            uniforms.insert("sigma".into(), EffectUniformValueDescriptor::Number(sigma));
            uniforms.insert(
                "direction".into(),
                EffectUniformValueDescriptor::Vector(vec![
                    if dir == "horizontal" || dir == "both" {
                        1.0
                    } else {
                        0.0
                    },
                    if dir == "vertical" || dir == "both" {
                        1.0
                    } else {
                        0.0
                    },
                ]),
            );
            "gaussian-blur"
        }
        rook_core::effect::EffectKind::Sharpen => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "sharpen"
        }
        rook_core::effect::EffectKind::Vignette => {
            let strength = params
                .get("strength")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5) as f32;
            uniforms.insert(
                "strength".into(),
                EffectUniformValueDescriptor::Number(strength),
            );
            "vignette"
        }
        rook_core::effect::EffectKind::FilmGrain => {
            let amt = params.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.1) as f32;
            uniforms.insert("amount".into(), EffectUniformValueDescriptor::Number(amt));
            "film-grain"
        }
        rook_core::effect::EffectKind::ChromaKey => {
            let hue = params.get("hue").and_then(|v| v.as_f64()).unwrap_or(120.0) as f32;
            let tol = params
                .get("tolerance")
                .and_then(|v| v.as_f64())
                .unwrap_or(30.0) as f32;
            uniforms.insert("key_hue".into(), EffectUniformValueDescriptor::Number(hue));
            uniforms.insert(
                "tolerance".into(),
                EffectUniformValueDescriptor::Number(tol),
            );
            "chroma-key"
        }
        _ => return None, // Unsupported effect kind for CPU path
    };

    Some(EffectPassDescriptor {
        shader: shader.to_string(),
        uniforms,
    })
}

/// Map the core blend mode enum to the compositor enum.
fn map_blend_to_compositor(
    mode: rook_core::clip::BlendMode,
) -> rook_renderer::compositor::BlendMode {
    match mode {
        rook_core::clip::BlendMode::Normal => rook_renderer::compositor::BlendMode::Normal,
        rook_core::clip::BlendMode::Darken => rook_renderer::compositor::BlendMode::Darken,
        rook_core::clip::BlendMode::Multiply => rook_renderer::compositor::BlendMode::Multiply,
        rook_core::clip::BlendMode::ColorBurn => rook_renderer::compositor::BlendMode::ColorBurn,
        rook_core::clip::BlendMode::Lighten => rook_renderer::compositor::BlendMode::Lighten,
        rook_core::clip::BlendMode::Screen => rook_renderer::compositor::BlendMode::Screen,
        rook_core::clip::BlendMode::PlusLighter => {
            rook_renderer::compositor::BlendMode::PlusLighter
        }
        rook_core::clip::BlendMode::ColorDodge => rook_renderer::compositor::BlendMode::ColorDodge,
        rook_core::clip::BlendMode::Overlay => rook_renderer::compositor::BlendMode::Overlay,
        rook_core::clip::BlendMode::SoftLight => rook_renderer::compositor::BlendMode::SoftLight,
        rook_core::clip::BlendMode::HardLight => rook_renderer::compositor::BlendMode::HardLight,
        rook_core::clip::BlendMode::Difference => rook_renderer::compositor::BlendMode::Difference,
        rook_core::clip::BlendMode::Exclusion => rook_renderer::compositor::BlendMode::Exclusion,
        rook_core::clip::BlendMode::Hue => rook_renderer::compositor::BlendMode::Hue,
        rook_core::clip::BlendMode::Saturation => rook_renderer::compositor::BlendMode::Saturation,
        rook_core::clip::BlendMode::Color => rook_renderer::compositor::BlendMode::Color,
        rook_core::clip::BlendMode::Luminosity => rook_renderer::compositor::BlendMode::Luminosity,
    }
}

/// Generate a solid-color RGBA texture.
fn solid_color_rgba(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
    // Small 2×2 tile — compositor scales it
    vec![r, g, b, a, r, g, b, a, r, g, b, a, r, g, b, a]
}

/// Generate a placeholder text image with background pill and character outline.
/// Renders text centered on a semi-transparent dark background with a 1px outline
/// for readability against any video content.
fn placeholder_text_rgba(
    content: &str,
    font_size: f32,
    color: &[f32; 4],
    w: u32,
    h: u32,
) -> Vec<u8> {
    let cw = w as usize;
    let ch = h as usize;
    let mut rgba = vec![0u8; cw * ch * 4]; // transparent background

    let r = (color[0] * 255.0) as u8;
    let g = (color[1] * 255.0) as u8;
    let b = (color[2] * 255.0) as u8;
    let a = (color[3] * 255.0) as u8;

    // Outline color: black with alpha matching text
    let outline_r = 0u8;
    let outline_g = 0u8;
    let outline_b = 0u8;
    let outline_a = (a as f32 * 0.85) as u8;

    // Background pill color: dark with alpha
    let bg_r = 0u8;
    let bg_g = 0u8;
    let bg_b = 0u8;
    let bg_a = 160u8;

    // Character rendering dimensions
    let char_w = 10;
    let char_h = 16;
    let text_cols = content.len();
    let total_text_w = (text_cols * char_w).min(cw.saturating_sub(40));
    let pill_w = total_text_w + 40;
    let pill_h = char_h + 24;
    let pill_x = ((cw as i32 - pill_w as i32) / 2).max(0) as usize;
    let pill_y = ((ch as i32 - pill_h as i32) / 2).max(0) as usize;

    // Draw background pill (rounded via corner omission)
    let radius = 12usize;
    for py in pill_y..(pill_y + pill_h).min(ch) {
        for px in pill_x..(pill_x + pill_w).min(cw) {
            // Simple rounded corner check
            let rel_x = px - pill_x;
            let rel_y = py - pill_y;
            let in_corner = |dx: usize, dy: usize| -> bool {
                let cx = if dx < radius {
                    radius - dx
                } else if dx >= pill_w.saturating_sub(radius) {
                    dx - (pill_w.saturating_sub(radius)) + 1
                } else {
                    0
                };
                let cy = if dy < radius {
                    radius - dy
                } else if dy >= pill_h.saturating_sub(radius) {
                    dy - (pill_h.saturating_sub(radius)) + 1
                } else {
                    0
                };
                cx * cx + cy * cy > radius * radius
            };
            if in_corner(rel_x, rel_y) {
                continue;
            }
            let idx = (py * cw + px) * 4;
            rgba[idx] = bg_r;
            rgba[idx + 1] = bg_g;
            rgba[idx + 2] = bg_b;
            rgba[idx + 3] = bg_a;
        }
    }

    let text_x = pill_x + 20;
    let text_y = pill_y + 12;

    // Helper to test if a character pixel is "on"
    let char_on = |chr: char, dx: usize, dy: usize| -> bool {
        match chr {
            'A'..='Z' | 'a'..='z' => {
                (dx > 1 && dx < 6 && dy > 2 && dy < 14) || (dy >= 6 && dy <= 10)
            }
            '0'..='9' => dx > 1 && dx < 6 && dy > 2 && dy < 14,
            ' ' => false,
            _ => dx > 2 && dx < 5 && dy > 4 && dy < 12,
        }
    };

    // First pass: draw outline (1px in all 8 directions)
    for (ci, chr) in content.chars().enumerate() {
        let cx = text_x as i32 + ci as i32 * char_w as i32;
        for dy in 0i32..char_h as i32 {
            for dx in 0i32..8 {
                if !char_on(chr, dx as usize, dy as usize) {
                    continue;
                }
                let px = cx + dx;
                let py = text_y as i32 + dy;
                // Check 8 neighbor positions
                for &(ox, oy) in &[
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let nx = px + ox;
                    let ny = py + oy;
                    if nx >= 0 && ny >= 0 && (nx as usize) < cw && (ny as usize) < ch {
                        let idx = ((ny as usize) * cw + (nx as usize)) * 4;
                        if rgba[idx + 3] < outline_a {
                            rgba[idx] = outline_r;
                            rgba[idx + 1] = outline_g;
                            rgba[idx + 2] = outline_b;
                            rgba[idx + 3] = outline_a;
                        }
                    }
                }
            }
        }
    }

    // Second pass: draw main text on top
    for (ci, chr) in content.chars().enumerate() {
        let cx = text_x as i32 + ci as i32 * char_w as i32;
        for dy in 0i32..char_h as i32 {
            for dx in 0i32..8 {
                if !char_on(chr, dx as usize, dy as usize) {
                    continue;
                }
                let px = (cx + dx) as usize;
                let py = (text_y as i32 + dy) as usize;
                if px < cw && py < ch {
                    let idx = (py * cw + px) * 4;
                    rgba[idx] = r;
                    rgba[idx + 1] = g;
                    rgba[idx + 2] = b;
                    rgba[idx + 3] = a;
                }
            }
        }
    }

    rgba
}

/// Generates a checkerboard test pattern as RGBA bytes.
fn checkerboard_rgba(w: u32, h: u32, frame: i64) -> Vec<u8> {
    let (cw, ch) = (w as usize, h as usize);
    let mut rgba = vec![0u8; cw * ch * 4];
    let sq = 80usize;
    for y in 0..ch {
        for x in 0..cw {
            let idx = (y * cw + x) * 4;
            let dark = (x / sq + y / sq) % 2 == 0;
            if dark {
                rgba[idx] = 100;
                rgba[idx + 1] = 40;
                rgba[idx + 2] = 100;
            } else {
                rgba[idx] = 50;
                rgba[idx + 1] = 50;
                rgba[idx + 2] = 55;
            }
            rgba[idx + 3] = 255;
        }
    }
    // Centered "No Media" message — clearly visible white text on dark checkerboard
    let msg = "No Media";
    let font_w = 8usize;
    let font_h = 14usize;
    let msg_px_w = msg.len() * font_w;
    let start_x = cw.saturating_sub(msg_px_w) / 2;
    let start_y = ch.saturating_sub(font_h) / 2;
    for (ci, chr) in msg.chars().enumerate() {
        let fx = start_x + ci * font_w;
        for dy in 0..font_h {
            for dx in 0..font_w {
                let px = fx + dx;
                let py = start_y + dy;
                if px < cw && py < ch {
                    let idx = (py * cw + px) * 4;
                    rgba[idx] = 220;
                    rgba[idx + 1] = 220;
                    rgba[idx + 2] = 220;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
    // Frame counter in bottom-left corner
    let label = format!("Frame {}", frame);
    for (ci, chr) in label.chars().enumerate() {
        let fx = 16 + ci * 9;
        for dy in 0..10 {
            for dx in 0..7 {
                let px = fx + dx;
                let py = ch.saturating_sub(24) + dy;
                if px < cw && py < ch {
                    let idx = (py * cw + px) * 4;
                    let b: u8 = 160;
                    rgba[idx] = b;
                    rgba[idx + 1] = b;
                    rgba[idx + 2] = if chr.is_ascii_digit() { 100 } else { b };
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
    rgba
}

pub struct PreviewPanel {
    renderer: PreviewRenderer,
    preview_texture: Option<egui::TextureHandle>,
    /// Video decode bridge — opens media files and decodes frames.
    video_bridge: VideoPreviewBridge,
    /// Transform drag state for on-canvas Position tool.
    transform_drag: Option<TransformDragState>,
    /// Audio waveform cache for VU meter.
    waveform_cache: WaveformCache,
    /// Whether we tried to load waveforms.
    waveforms_loaded: bool,
    /// Viewer zoom mode.
    viewer_zoom: ViewerZoom,
    /// Show color scopes overlay.
    show_scopes: bool,
    /// Which scope to display.
    scope_mode: ScopeMode,
    /// Show rule-of-thirds grid.
    show_grid: bool,
    /// Show title-safe zone overlay.
    show_title_safe: bool,
    /// Show canvas overlays (timecode, clip name).
    show_overlays: bool,
    /// Quality mode: false = performance (lower res), true = quality (full res).
    quality_mode: bool,
    /// Show pixel rulers on canvas edges.
    show_rulers: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ViewerZoom {
    Fit,
    Pct100,
    Pct200,
    Pct50,
}

impl ViewerZoom {
    fn label(&self) -> &str {
        match self {
            Self::Fit => "Fit",
            Self::Pct100 => "100%",
            Self::Pct200 => "200%",
            Self::Pct50 => "50%",
        }
    }
    fn next(&self) -> Self {
        match self {
            Self::Fit => Self::Pct100,
            Self::Pct100 => Self::Pct200,
            Self::Pct200 => Self::Pct50,
            Self::Pct50 => Self::Fit,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ScopeMode {
    Waveform,
    Vectorscope,
    Histogram,
    Parade,
}

impl ScopeMode {
    fn label(&self) -> &str {
        match self {
            Self::Waveform => "WFM",
            Self::Vectorscope => "Vec",
            Self::Histogram => "Hist",
            Self::Parade => "RGB",
        }
    }
    fn next(&self) -> Self {
        match self {
            Self::Waveform => Self::Vectorscope,
            Self::Vectorscope => Self::Histogram,
            Self::Histogram => Self::Parade,
            Self::Parade => Self::Waveform,
        }
    }
}

impl Default for PreviewPanel {
    fn default() -> Self {
        Self {
            renderer: PreviewRenderer::default(),
            preview_texture: None,
            video_bridge: VideoPreviewBridge::new(),
            transform_drag: None,
            waveform_cache: WaveformCache::new(),
            waveforms_loaded: false,
            viewer_zoom: ViewerZoom::Fit,
            show_scopes: false,
            scope_mode: ScopeMode::Waveform,
            show_grid: false,
            show_title_safe: false,
            show_overlays: false,
            quality_mode: true,
            show_rulers: false,
        }
    }
}

/// State for an on-canvas transform drag operation.
struct TransformDragState {
    clip_id: ClipId,
    handle: HandleKind,
    /// Screen position where drag started.
    start_screen: egui::Pos2,
    /// Transform when drag started.
    start_transform: Transform,
}

#[derive(Clone, Copy, PartialEq)]
enum HandleKind {
    Center,
    Corner(CornerPos),
    Rotate,
}

#[derive(Clone, Copy, PartialEq)]
enum CornerPos {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

impl PreviewPanel {
    /// Toggle between Fit and 100% zoom mode.
    pub fn toggle_fit_vs_100(&mut self) {
        self.viewer_zoom = match self.viewer_zoom {
            ViewerZoom::Fit => ViewerZoom::Pct100,
            _ => ViewerZoom::Fit,
        };
    }

    /// Set quality mode based on playback state — called outside the egui
    /// rendering pass to avoid winit draw_rect panics.
    pub fn set_playing(&mut self, playing: bool) {
        let effective_low_quality = if playing {
            true
        } else {
            !self.quality_mode
        };
        self.renderer.set_low_quality(effective_low_quality);
    }

    /// Set playhead to the start of the first selected clip.
    pub fn play_selected(&mut self, engine: &Engine) -> Option<(i64, i64)> {
        let cid = engine.project().timeline.selected_clip_ids.first()?;
        let clip = engine.project().timeline.clip(*cid)?;
        Some((clip.timeline_in, clip.timeline_in + clip.duration()))
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        engine: &mut Engine,
        active_tool: Tool,
        playing: &mut bool,
        playhead: &mut i64,
    ) {
        // Collect completed background waveform extractions (every frame).
        self.waveform_cache.poll_completed();

        // Start background waveform extractions for assets we haven't seen yet.
        // get_or_extract() is non-blocking — spawns a background thread if
        // data isn't cached, returns None immediately.
        if !self.waveforms_loaded {
            self.waveforms_loaded = true;
            for asset in &engine.project().assets {
                let path = std::path::PathBuf::from(asset.path());
                if path.exists() {
                    let _ = self.waveform_cache.get_or_extract(asset.id(), &path);
                }
            }
        }

        let available = ui.available_size();
        let (rw, rh) = self.renderer.dimensions();
        let aspect = rw as f32 / rh as f32;

        // Compute display size based on zoom mode.
        // Reserve 42px at bottom for transport bar; clamp to avoid negative sizes.
        let avail_for_video = (available.y - 42.0).max(1.0);
        let avail_w = available.x.max(1.0);
        let (display_w, display_h) = match self.viewer_zoom {
            ViewerZoom::Fit => {
                let h = (avail_w / aspect).min(avail_for_video).max(1.0);
                ((h * aspect).max(1.0), h)
            }
            ViewerZoom::Pct100 => {
                let h = rh as f32;
                let w = rw as f32;
                if h > avail_for_video || w > avail_w {
                    let scale = (avail_for_video / h).min(avail_w / w).max(0.0001);
                    ((w * scale).max(1.0), (h * scale).max(1.0))
                } else {
                    (w.max(1.0), h.max(1.0))
                }
            }
            ViewerZoom::Pct200 => {
                let w = (rw as f32 * 2.0).min(avail_w).max(1.0);
                let h = (rh as f32 * 2.0).min(avail_for_video).max(1.0);
                (w, h)
            }
            ViewerZoom::Pct50 => {
                let w = (rw as f32 * 0.5).max(1.0);
                let h = (rh as f32 * 0.5).max(1.0);
                (w, h)
            }
        };

        // Try to decode real video; fall back to checkerboard
        let rgba_vec = {
            let rgba_ref = self
                .renderer
                .frame_rgba(*playhead, engine, &mut self.video_bridge);
            rgba_ref.to_vec()
        }; // mutable borrow released here

        // Re-read dimensions AFTER frame_rgba — quality switching may have
        // changed the composite resolution (e.g. 1920×1080 → 480×270).
        let (rw, rh) = self.renderer.dimensions();

        // Allocate space with click and drag sense
        let sense = if active_tool == Tool::Position {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click_and_drag()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(display_w, display_h), sense);

        // Check if there are any clips on the timeline
        let has_clips = engine
            .project()
            .timeline
            .tracks
            .iter()
            .any(|t| !t.clips.is_empty());

        // Paint black background
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_gray(16));

        // Compute paint_rect (where the video is actually painted with aspect ratio)
        let img_aspect = rw as f32 / rh as f32;
        let rect_aspect = rect.width() / rect.height();
        let paint_rect = if img_aspect > rect_aspect {
            let h = rect.width() / img_aspect;
            let y = rect.center().y - h / 2.0;
            egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(rect.width(), h))
        } else {
            let w = rect.height() * img_aspect;
            let x = rect.center().x - w / 2.0;
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height()))
        };

        if has_clips {
            // Upload as egui texture
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([rw as usize, rh as usize], &rgba_vec);
            let texture = self.preview_texture.get_or_insert_with(|| {
                ui.ctx().load_texture(
                    "rook_preview_frame",
                    color_image.clone(),
                    egui::TextureOptions::LINEAR,
                )
            });
            texture.set(color_image, egui::TextureOptions::LINEAR);

            // Paint the video frame
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter()
                .image(texture.id(), paint_rect, uv, egui::Color32::WHITE);

            // ── Frame status indicator ──────────────────────────────────
            // Shows pipeline state at top-center of paint_rect.
            let ren = &self.renderer;
            let (indicator, ind_color, ind_bg) = if ren.last_had_decode {
                let all_black = rgba_vec.len() >= 4 && rgba_vec.chunks(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0);
                if all_black {
                    ("BLACK FRAME", egui::Color32::from_rgb(255, 180, 40), egui::Color32::from_black_alpha(200))
                } else {
                    ("DECODED", egui::Color32::from_rgb(40, 240, 60), egui::Color32::from_black_alpha(200))
                }
            } else {
                ("NO DECODE", egui::Color32::from_rgb(240, 80, 200), egui::Color32::from_black_alpha(200))
            };
            let info = format!(
                "{} | frame#{} cover={} tex={}",
                indicator, ren.frame_count, ren.covering_clips, ren.texture_count
            );
            let ind_size = egui::vec2(320.0, 28.0);
            let ind_pos = egui::pos2(paint_rect.center().x - ind_size.x / 2.0, paint_rect.top() + 8.0);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(ind_pos, ind_size),
                6.0,
                ind_bg,
            );
            ui.painter().text(
                ind_pos + egui::vec2(ind_size.x / 2.0, ind_size.y / 2.0),
                egui::Align2::CENTER_CENTER,
                info,
                egui::FontId::proportional(12.0),
                ind_color,
            );

            // Show asset-open errors below
            for (i, err) in ren.asset_errors.iter().enumerate() {
                let truncated: String = if err.len() > 70 {
                    format!("{}…", &err[..67])
                } else {
                    err.clone()
                };
                ui.painter().text(
                    egui::pos2(paint_rect.center().x, paint_rect.top() + 42.0 + i as f32 * 16.0),
                    egui::Align2::CENTER_TOP,
                    truncated,
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_rgb(255, 100, 60),
                );
            }
        } else {
            // Drop-zone placeholder: guide user to import media
            let center = rect.center();
            ui.painter().text(
                egui::pos2(center.x, center.y - 24.0),
                egui::Align2::CENTER_CENTER,
                "🎬 Drop media here",
                egui::FontId::proportional(20.0),
                egui::Color32::from_gray(120),
            );
            ui.painter().text(
                egui::pos2(center.x, center.y + 4.0),
                egui::Align2::CENTER_CENTER,
                "or File → Import Media",
                egui::FontId::proportional(14.0),
                egui::Color32::from_gray(80),
            );
        }

        // ── Canvas pixel rulers ───────────────────────────────────────────
        if self.show_rulers {
            self.draw_rulers(ui, paint_rect, rw as f32, rh as f32);
        }

        // ── Canvas overlays (timecode, clip name) ────────────────────────
        if self.show_overlays {
            let fps = engine.project().frame_rate.as_f64();
            let secs = *playhead as f64 / fps;
            let tc = format!(
                "{:02}:{:02}:{:02}:{:02}",
                (secs / 3600.0) as i64,
                (secs / 60.0) as i64 % 60,
                secs as i64 % 60,
                *playhead as i64 % fps.round() as i64
            );
            // Timecode at bottom center
            ui.painter().text(
                egui::pos2(paint_rect.center().x, paint_rect.bottom() - 8.0),
                egui::Align2::CENTER_BOTTOM,
                &tc,
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgba_premultiplied(255, 255, 255, 200),
            );
            // Selected clip name at top-left
            if let Some(&cid) = engine.project().timeline.selected_clip_ids.first() {
                if let Some(clip) = engine.project().timeline.clip(cid) {
                    if clip.covers(*playhead) {
                        ui.painter().text(
                            egui::pos2(paint_rect.left() + 8.0, paint_rect.top() + 8.0),
                            egui::Align2::LEFT_TOP,
                            &clip.label,
                            egui::FontId::proportional(12.0),
                            egui::Color32::from_rgba_premultiplied(255, 255, 255, 180),
                        );
                    }
                }
            }
        }

        // ── Title-safe zone overlay ──────────────────────────────────────
        if self.show_title_safe {
            let margin_x = paint_rect.width() * 0.1;
            let margin_y = paint_rect.height() * 0.1;
            let safe_rect = egui::Rect::from_min_max(
                egui::pos2(paint_rect.left() + margin_x, paint_rect.top() + margin_y),
                egui::pos2(
                    paint_rect.right() - margin_x,
                    paint_rect.bottom() - margin_y,
                ),
            );
            ui.painter().rect_stroke(
                safe_rect,
                0.0,
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, 40),
                ),
                egui::StrokeKind::Inside,
            );
            ui.painter().rect_stroke(
                paint_rect,
                0.0,
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_premultiplied(255, 255, 255, 20),
                ),
                egui::StrokeKind::Inside,
            );
        }

        // ── Rule-of-thirds grid ─────────────────────────────────────────
        if self.show_grid {
            let thirds_x = paint_rect.width() / 3.0;
            let thirds_y = paint_rect.height() / 3.0;
            let grid_color = egui::Color32::from_rgba_premultiplied(255, 255, 255, 30);
            for i in 1..3 {
                let x = paint_rect.left() + i as f32 * thirds_x;
                ui.painter().line_segment(
                    [
                        egui::pos2(x, paint_rect.top()),
                        egui::pos2(x, paint_rect.bottom()),
                    ],
                    egui::Stroke::new(0.5, grid_color),
                );
                let y = paint_rect.top() + i as f32 * thirds_y;
                ui.painter().line_segment(
                    [
                        egui::pos2(paint_rect.left(), y),
                        egui::pos2(paint_rect.right(), y),
                    ],
                    egui::Stroke::new(0.5, grid_color),
                );
            }
        }

        // ── Color scopes overlay ────────────────────────────────────────
        self.draw_scopes(ui, &rgba_vec, rw, rh, paint_rect);

        let canvas_w = rw as f32;
        let canvas_h = rh as f32;
        let scale_x = paint_rect.width() / canvas_w;
        let scale_y = paint_rect.height() / canvas_h;
        let offset = paint_rect.left_top();

        // ── On-canvas transform controls (Position tool) ────────────────
        let mut in_position_mode = false;
        if active_tool == Tool::Position {
            let selected = engine.project().timeline.selected_clip_ids.first().copied();
            if let Some(cid) = selected {
                if let Some(clip) = engine.project().timeline.clip(cid) {
                    if clip.covers(*playhead) {
                        in_position_mode = true;
                        let transform = clip.transform.clone();
                        self.draw_transform_handles(
                            ui, &response, &transform, canvas_w, canvas_h, scale_x, scale_y,
                            offset, paint_rect,
                        );

                        // Handle transform drag (mouse)
                        self.handle_transform_drag(
                            engine,
                            &response,
                            cid,
                            transform.clone(),
                            canvas_w,
                            canvas_h,
                            scale_x,
                            scale_y,
                            offset,
                        );

                        // Handle keyboard shortcuts for transform
                        self.handle_position_keys(ui, engine, cid);

                        // Show transform info overlay
                        self.draw_transform_info(ui, &transform, canvas_w, canvas_h);
                    }
                }
            }
        }

        // Transport overlay (shown in all modes)
        ui.horizontal(|ui| {
            // ⏮ Go to start
            if ui.button("⏮").clicked() {
                *playhead = 0;
            }
            // ⏪ Skip back 2s
            if ui.button("⏪").clicked() {
                let fps = engine.project().frame_rate.as_f64();
                *playhead = (*playhead - (fps * 2.0) as i64).max(0);
            }
            // ⏵ Play/Pause
            if ui.button(if *playing { "⏸" } else { "⏵" }).clicked() {
                // If playhead is at the end of the timeline, wrap to start
                if !*playing {
                    let end = last_valid_timeline_frame(engine.project().timeline.duration());
                    if *playhead >= end {
                        *playhead = 0;
                    }
                }
                *playing = !*playing;
            }
            // ⏩ Skip forward 2s
            if ui.button("⏩").clicked() {
                let fps = engine.project().frame_rate.as_f64();
                let end = last_valid_timeline_frame(engine.project().timeline.duration());
                *playhead = (*playhead + (fps * 2.0) as i64).min(end);
            }
            // ⏭ Go to end
            if ui.button("⏭").clicked() {
                let end = last_valid_timeline_frame(engine.project().timeline.duration());
                *playhead = end;
            }
            ui.separator();

            // Play Around — play 2s before playhead, clamped to valid frame range
            if ui.button("🔄").on_hover_text("Play Around").clicked() {
                let fps = engine.project().frame_rate.as_f64();
                let max_frame = engine.project().timeline.duration().max(1).saturating_sub(1);
                // Clamp playhead to valid range before jumping back
                let safe_playhead = (*playhead).clamp(0, max_frame);
                *playhead = (safe_playhead - (fps * 2.0) as i64).max(0);
                *playing = true;
            }
            // Play Selected — play range of selected clip(s)
            let can_play_sel = !engine.project().timeline.selected_clip_ids.is_empty();
            if ui
                .add_enabled(can_play_sel, egui::Button::new("🎯"))
                .on_hover_text("Play Selected")
                .clicked()
            {
                if let Some(&cid) = engine.project().timeline.selected_clip_ids.first() {
                    if let Some(clip) = engine.project().timeline.clip(cid) {
                        *playhead = clip.timeline_in;
                        *playing = true;
                    }
                }
            }
            // ⌘0 — 100% viewer toggle
            if ui
                .button("🔍")
                .on_hover_text("Toggle 100% / Fit (⌘0)")
                .clicked()
            {
                self.viewer_zoom = match self.viewer_zoom {
                    ViewerZoom::Fit => ViewerZoom::Pct100,
                    _ => ViewerZoom::Fit,
                };
            }

            // Grid overlay toggle
            let grid_btn = if self.show_grid { "#" } else { "#" };
            if ui
                .add(egui::SelectableLabel::new(self.show_grid, grid_btn))
                .on_hover_text("Rule-of-thirds grid")
                .clicked()
            {
                self.show_grid = !self.show_grid;
            }
            // Title-safe toggle
            let ts_btn = if self.show_title_safe { "⊡" } else { "⊡" };
            if ui
                .add(egui::SelectableLabel::new(self.show_title_safe, ts_btn))
                .on_hover_text("Title-safe zone")
                .clicked()
            {
                self.show_title_safe = !self.show_title_safe;
            }
            // Overlays toggle
            let ov_btn = if self.show_overlays { "💬" } else { "💬" };
            if ui
                .add(egui::SelectableLabel::new(self.show_overlays, ov_btn))
                .on_hover_text("Show overlays (timecode, clip name)")
                .clicked()
            {
                self.show_overlays = !self.show_overlays;
            }
            // Rulers toggle
            let ruler_btn = if self.show_rulers { "📏" } else { "📏" };
            if ui
                .add(egui::SelectableLabel::new(self.show_rulers, ruler_btn))
                .on_hover_text("Show pixel rulers")
                .clicked()
            {
                self.show_rulers = !self.show_rulers;
            }
            // Quality toggle
            let q_btn = if self.quality_mode { "⚡" } else { "🐢" };
            if ui
                .add(egui::SelectableLabel::new(self.quality_mode, q_btn))
                .on_hover_text(if self.quality_mode {
                    "Quality mode — switch to performance"
                } else {
                    "Performance mode — switch to quality"
                })
                .clicked()
            {
                self.quality_mode = !self.quality_mode;
            }

            ui.separator();

            // Viewer zoom toggle
            if ui.button(self.viewer_zoom.label()).clicked() {
                self.viewer_zoom = self.viewer_zoom.next();
            }

            ui.separator();

            let fps = engine.project().frame_rate.as_f64();
            let cur_sec = *playhead as f64 / fps;
            let tc = rook_time::format_timecode(
                rook_time::MediaTime::from_seconds_f64(cur_sec)
                    .unwrap_or(rook_time::MediaTime::ZERO),
                Some(rook_time::TimeCodeFormat::HhMmSsFf),
                engine.project().frame_rate.to_time_frame_rate(),
            )
            .unwrap_or_else(|| format!("{:02}:{:05.2}", (cur_sec / 60.0) as i64, cur_sec % 60.0));
            ui.label(tc);

            ui.separator();

            // ── VU meter ──────────────────────────────────────────────
            let vu_level = self.compute_vu_level(engine, *playhead);
            self.draw_vu_meter(ui, vu_level);

            ui.separator();
            // Scopes toggle
            let scopes_label = if self.show_scopes {
                format!("📊 {}", self.scope_mode.label())
            } else {
                "📊".to_string()
            };
            if ui
                .button(scopes_label)
                .on_hover_text("Toggle color scopes (click to cycle modes)")
                .clicked()
            {
                if self.show_scopes {
                    self.scope_mode = self.scope_mode.next();
                } else {
                    self.show_scopes = true;
                }
            }
            if self.show_scopes {
                if ui.button("✕").on_hover_text("Hide scopes").clicked() {
                    self.show_scopes = false;
                }
            }
            ui.separator();
            if in_position_mode {
                ui.label("🎯 Position");
            }
            ui.label(format!(
                "{}×{}",
                engine.project().canvas.width,
                engine.project().canvas.height
            ));
        });

        // Skip click-to-seek in Position mode (handles/interaction already handled)
        if in_position_mode {
            return;
        }

        // Click to seek (non-Position mode)
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if paint_rect.contains(pos) {
                    let frac = ((pos.x - paint_rect.left()) / paint_rect.width()).clamp(0.0, 1.0);
                    let total_dur = last_valid_timeline_frame(engine.project().timeline.duration());
                    *playhead = (frac * total_dur as f32) as i64;
                }
            }
        }
    }

    // ── VU meter ──────────────────────────────────────────────────────

    /// Compute the peak audio level at the current playhead across all visible audio clips.
    fn compute_vu_level(&self, engine: &Engine, frame: i64) -> f32 {
        let project = engine.project();
        let fps = project.frame_rate.as_f64();
        let mut max_peak = 0.0f32;

        for track in &project.timeline.tracks {
            if !track.visible || track.muted {
                continue;
            }
            if track.kind != rook_core::track::TrackKind::Audio {
                continue;
            }
            for clip in &track.clips {
                if clip.mute_audio || !clip.covers(frame) {
                    continue;
                }
                let src_frame = clip.timeline_to_source(frame).unwrap_or(0);
                let src_secs = src_frame as f64 / fps;
                if let Some(wf) = self.waveform_cache.get(clip.asset_id) {
                    let bar = (src_secs * wf.bars_per_second as f64) as usize;
                    let peak = wf.peaks.get(bar).copied().unwrap_or(0.0);
                    // Apply clip gain
                    let gain_linear = 10.0f32.powf(clip.gain_db.unwrap_or(0.0) / 20.0);
                    max_peak = max_peak.max(peak * gain_linear);
                }
            }
        }
        max_peak
    }

    fn draw_vu_meter(&self, ui: &mut egui::Ui, level: f32) {
        let bar_w = 50.0;
        let bar_h = 12.0;
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(bar_w, bar_h), egui::Sense::hover());
        let painter = ui.painter();

        // Background
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));

        // Green zone (0 to -12dB)
        let green_w = rect.width() * 0.6;
        let green_rect = egui::Rect::from_min_size(rect.left_top(), egui::vec2(green_w, bar_h));
        painter.rect_filled(green_rect, 2.0, egui::Color32::from_rgb(30, 140, 50));

        // Yellow zone (-12 to -6dB)
        let yellow_w = rect.width() * 0.2;
        let yellow_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + green_w, rect.top()),
            egui::vec2(yellow_w, bar_h),
        );
        painter.rect_filled(yellow_rect, 0.0, egui::Color32::from_rgb(180, 160, 40));

        // Red zone (-6 to 0dB)
        let red_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + green_w + yellow_w, rect.top()),
            egui::vec2(rect.width() * 0.2, bar_h),
        );
        painter.rect_filled(red_rect, 2.0, egui::Color32::from_rgb(180, 40, 40));

        // Level indicator
        let level_w = (level.clamp(0.0, 1.0) * bar_w).min(bar_w);
        let level_rect = egui::Rect::from_min_size(rect.left_top(), egui::vec2(level_w, bar_h));
        let level_color = if level > 0.8 {
            egui::Color32::from_rgb(255, 60, 40)
        } else if level > 0.6 {
            egui::Color32::from_rgb(220, 200, 40)
        } else {
            egui::Color32::from_rgb(60, 220, 60)
        };
        painter.rect_filled(level_rect, 2.0, level_color);

        // Label
        let db = if level > 0.001 {
            20.0 * (level as f32).log10()
        } else {
            -60.0
        };
        painter.text(
            rect.right_center() + egui::vec2(2.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("{:.0}dB", db),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(180),
        );
    }

    // ── Canvas rulers ───────────────────────────────────────────────────

    /// Draw pixel measurement rulers along the top and left edges of the canvas.
    fn draw_rulers(&self, ui: &egui::Ui, paint_rect: egui::Rect, canvas_w: f32, canvas_h: f32) {
        let painter = ui.painter();
        let ruler_w = 18.0; // width/height of ruler strips
        let bg_color = egui::Color32::from_black_alpha(180);
        let tick_color = egui::Color32::from_gray(180);
        let major_tick_color = egui::Color32::from_gray(220);
        let text_color = egui::Color32::from_gray(210);
        let font = egui::FontId::proportional(8.0);

        let scale_x = paint_rect.width() / canvas_w.max(1.0);
        let scale_y = paint_rect.height() / canvas_h.max(1.0);

        // Determine tick spacing based on display scale
        let (tick_spacing, major_every) = if scale_x > 4.0 {
            (5.0_f32, 10)
        } else if scale_x > 2.0 {
            (10.0, 10)
        } else if scale_x > 0.5 {
            (50.0, 5)
        } else if scale_x > 0.1 {
            (100.0, 5)
        } else {
            (500.0, 4)
        };

        // ── Top ruler ──────────────────────────────────────────────────
        let top_ruler = egui::Rect::from_min_size(
            egui::pos2(paint_rect.left(), paint_rect.top() - ruler_w),
            egui::vec2(paint_rect.width(), ruler_w),
        );
        // Extend slightly left to cover corner
        let top_bg = top_ruler.translate(egui::vec2(-ruler_w, 0.0));
        painter.rect_filled(
            egui::Rect::from_min_size(
                top_bg.left_top(),
                egui::vec2(top_bg.width() + ruler_w, ruler_w),
            ),
            0.0,
            bg_color,
        );

        let mut x: f32 = 0.0;
        let mut tick_idx = 0;
        while x <= canvas_w + tick_spacing {
            let screen_x = paint_rect.left() + x * scale_x;
            if screen_x >= paint_rect.left() && screen_x <= paint_rect.right() {
                let is_major = tick_idx % major_every == 0;
                let tick_h = if is_major {
                    ruler_w * 0.65
                } else {
                    ruler_w * 0.35
                };
                painter.line_segment(
                    [
                        egui::pos2(screen_x, paint_rect.top()),
                        egui::pos2(screen_x, paint_rect.top() - tick_h),
                    ],
                    egui::Stroke::new(
                        if is_major { 1.0 } else { 0.5 },
                        if is_major {
                            major_tick_color
                        } else {
                            tick_color
                        },
                    ),
                );
                if is_major && x >= 0.0 {
                    painter.text(
                        egui::pos2(screen_x + 2.0, paint_rect.top() - ruler_w + 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{:.0}", x),
                        font.clone(),
                        text_color,
                    );
                }
            }
            x += tick_spacing;
            tick_idx += 1;
        }
        // Bottom edge line of top ruler
        painter.line_segment(
            [top_ruler.left_bottom(), top_ruler.right_bottom()],
            egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
        );

        // ── Left ruler ─────────────────────────────────────────────────
        let left_ruler = egui::Rect::from_min_size(
            egui::pos2(paint_rect.left() - ruler_w, paint_rect.top()),
            egui::vec2(ruler_w, paint_rect.height()),
        );
        // Extend slightly up to cover corner
        let left_bg = left_ruler.translate(egui::vec2(0.0, -ruler_w));
        painter.rect_filled(
            egui::Rect::from_min_size(
                left_bg.left_top(),
                egui::vec2(ruler_w, left_bg.height() + ruler_w),
            ),
            0.0,
            bg_color,
        );

        let mut y: f32 = 0.0;
        let mut tick_idx = 0;
        while y <= canvas_h + tick_spacing {
            let screen_y = paint_rect.top() + y * scale_y;
            if screen_y >= paint_rect.top() && screen_y <= paint_rect.bottom() {
                let is_major = tick_idx % major_every == 0;
                let tick_w = if is_major {
                    ruler_w * 0.65
                } else {
                    ruler_w * 0.35
                };
                painter.line_segment(
                    [
                        egui::pos2(paint_rect.left(), screen_y),
                        egui::pos2(paint_rect.left() - tick_w, screen_y),
                    ],
                    egui::Stroke::new(
                        if is_major { 1.0 } else { 0.5 },
                        if is_major {
                            major_tick_color
                        } else {
                            tick_color
                        },
                    ),
                );
                if is_major && y >= 0.0 {
                    painter.text(
                        egui::pos2(paint_rect.left() - ruler_w + 1.0, screen_y + 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{:.0}", y),
                        font.clone(),
                        text_color,
                    );
                }
            }
            y += tick_spacing;
            tick_idx += 1;
        }
        // Right edge line of left ruler
        painter.line_segment(
            [left_ruler.right_top(), left_ruler.right_bottom()],
            egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
        );

        // ── Corner indicator ───────────────────────────────────────────
        let corner = egui::Rect::from_min_size(
            egui::pos2(paint_rect.left() - ruler_w, paint_rect.top() - ruler_w),
            egui::vec2(ruler_w, ruler_w),
        );
        // Small crosshair in corner
        let cx = corner.center().x;
        let cy = corner.center().y;
        painter.line_segment(
            [egui::pos2(cx - 3.0, cy), egui::pos2(cx + 3.0, cy)],
            egui::Stroke::new(1.0, major_tick_color),
        );
        painter.line_segment(
            [egui::pos2(cx, cy - 3.0), egui::pos2(cx, cy + 3.0)],
            egui::Stroke::new(1.0, major_tick_color),
        );
    }

    // ── Color scopes ────────────────────────────────────────────────────

    /// Render the active color scope overlay on the preview area.
    fn draw_scopes(&self, ui: &egui::Ui, rgba: &[u8], w: u32, h: u32, paint_rect: egui::Rect) {
        if !self.show_scopes {
            return;
        }
        if rgba.len() < (w * h * 4) as usize {
            return;
        }

        let painter = ui.painter();
        let scope_w = 200.0;
        let scope_h = 120.0;
        let margin = 8.0;
        let scope_rect = egui::Rect::from_min_size(
            egui::pos2(
                paint_rect.right() - scope_w - margin,
                paint_rect.bottom() - scope_h - margin,
            ),
            egui::vec2(scope_w, scope_h),
        );

        // Semi-transparent background
        painter.rect_filled(
            scope_rect.expand(4.0),
            4.0,
            egui::Color32::from_black_alpha(160),
        );

        match self.scope_mode {
            ScopeMode::Waveform => self.draw_waveform_scope(painter, rgba, w, h, scope_rect),
            ScopeMode::Vectorscope => self.draw_vectorscope(painter, rgba, w, h, scope_rect),
            ScopeMode::Histogram => self.draw_histogram_scope(painter, rgba, w, h, scope_rect),
            ScopeMode::Parade => self.draw_parade_scope(painter, rgba, w, h, scope_rect),
        }

        // Label
        painter.text(
            scope_rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            self.scope_mode.label(),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(180),
        );
    }

    /// Waveform monitor — luminance per column.
    fn draw_waveform_scope(
        &self,
        painter: &egui::Painter,
        rgba: &[u8],
        w: u32,
        h: u32,
        rect: egui::Rect,
    ) {
        let cols = 256usize;
        let mut max_luma = vec![0u8; cols];
        let mut min_luma = vec![255u8; cols];

        for y in 0..h as usize {
            for x in 0..w as usize {
                let idx = (y * w as usize + x) * 4;
                let r = rgba[idx] as f32;
                let g = rgba[idx + 1] as f32;
                let b = rgba[idx + 2] as f32;
                let luma = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
                let col = (x * cols / w as usize).min(cols - 1);
                max_luma[col] = max_luma[col].max(luma);
                min_luma[col] = min_luma[col].min(luma);
            }
        }

        for col in 0..cols {
            let x = rect.left() + col as f32 / cols as f32 * rect.width();
            let y_top = rect.bottom() - max_luma[col] as f32 / 255.0 * rect.height();
            let y_bot = rect.bottom() - min_luma[col] as f32 / 255.0 * rect.height();
            if max_luma[col] > min_luma[col] {
                painter.line_segment(
                    [egui::pos2(x, y_top), egui::pos2(x, y_bot)],
                    egui::Stroke::new(
                        0.8,
                        egui::Color32::from_rgba_premultiplied(80, 220, 100, 180),
                    ),
                );
            } else if max_luma[col] > 0 {
                let y = rect.bottom() - max_luma[col] as f32 / 255.0 * rect.height();
                painter.line_segment(
                    [egui::pos2(x, y), egui::pos2(x, y + 1.0)],
                    egui::Stroke::new(0.8, egui::Color32::from_gray(160)),
                );
            }
        }

        // 0% and 100% reference lines
        let zero_y = rect.bottom();
        let hundred_y = rect.top();
        painter.line_segment(
            [
                egui::pos2(rect.left(), zero_y),
                egui::pos2(rect.right(), zero_y),
            ],
            egui::Stroke::new(0.5, egui::Color32::from_gray(60)),
        );
        painter.line_segment(
            [
                egui::pos2(rect.left(), hundred_y),
                egui::pos2(rect.right(), hundred_y),
            ],
            egui::Stroke::new(0.5, egui::Color32::from_gray(60)),
        );
    }

    /// Vectorscope — color distribution in a circular plot.
    fn draw_vectorscope(
        &self,
        painter: &egui::Painter,
        rgba: &[u8],
        w: u32,
        h: u32,
        rect: egui::Rect,
    ) {
        let cx = rect.center().x;
        let cy = rect.center().y;
        let radius = rect.height().min(rect.width()) / 2.0 - 4.0;

        // Draw graticule circles
        for &r_frac in &[0.25, 0.5, 0.75, 1.0] {
            painter.circle_stroke(
                egui::pos2(cx, cy),
                radius * r_frac,
                egui::Stroke::new(0.5, egui::Color32::from_gray(40)),
            );
        }

        // Draw crosshair
        painter.line_segment(
            [egui::pos2(cx - radius, cy), egui::pos2(cx + radius, cy)],
            egui::Stroke::new(0.5, egui::Color32::from_gray(40)),
        );
        painter.line_segment(
            [egui::pos2(cx, cy - radius), egui::pos2(cx, cy + radius)],
            egui::Stroke::new(0.5, egui::Color32::from_gray(40)),
        );

        // Sample pixels and plot on vectorscope
        // Use stride to reduce pixel count
        let stride = ((w * h) as f32 / 4000.0).max(1.0) as usize;
        for y in (0..h as usize).step_by(stride) {
            for x in (0..w as usize).step_by(stride) {
                let idx = (y * w as usize + x) * 4;
                let r = rgba[idx] as f32 / 255.0;
                let g = rgba[idx + 1] as f32 / 255.0;
                let b = rgba[idx + 2] as f32 / 255.0;

                // Convert to YUV color difference signals (B-Y, R-Y)
                let y_val = 0.299 * r + 0.587 * g + 0.114 * b;
                let u_val = b - y_val; // B-Y
                let v_val = r - y_val; // R-Y

                let sx = cx + u_val * radius * 2.0;
                let sy = cy - v_val * radius * 2.0;

                if sx >= rect.left()
                    && sx <= rect.right()
                    && sy >= rect.top()
                    && sy <= rect.bottom()
                {
                    let alpha = y_val.clamp(0.0, 1.0) as u8;
                    painter.rect_filled(
                        egui::Rect::from_center_size(egui::pos2(sx, sy), egui::vec2(2.0, 2.0)),
                        0.0,
                        egui::Color32::from_rgba_premultiplied(
                            r as u8,
                            g as u8,
                            b as u8,
                            alpha.min(80),
                        ),
                    );
                }
            }
        }

        // Color target markers
        let targets: &[(f32, f32, &str)] = &[
            (0.0, 0.0, ""), // center
        ];
        for &(tu, tv, _label) in targets {
            let tx = cx + tu * radius * 2.0;
            let ty = cy - tv * radius * 2.0;
            painter.circle_stroke(
                egui::pos2(tx, ty),
                3.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
            );
        }
    }

    /// Histogram — RGB + Luma distribution.
    fn draw_histogram_scope(
        &self,
        painter: &egui::Painter,
        rgba: &[u8],
        w: u32,
        h: u32,
        rect: egui::Rect,
    ) {
        let bins = 128usize;
        let mut r_hist = vec![0u32; bins];
        let mut g_hist = vec![0u32; bins];
        let mut b_hist = vec![0u32; bins];

        for y in 0..h as usize {
            for x in 0..w as usize {
                let idx = (y * w as usize + x) * 4;
                let r = rgba[idx] as usize * bins / 256;
                let g = rgba[idx + 1] as usize * bins / 256;
                let b = rgba[idx + 2] as usize * bins / 256;
                r_hist[r.min(bins - 1)] += 1;
                g_hist[g.min(bins - 1)] += 1;
                b_hist[b.min(bins - 1)] += 1;
            }
        }

        let max_count = r_hist
            .iter()
            .chain(&g_hist)
            .chain(&b_hist)
            .max()
            .copied()
            .unwrap_or(1)
            .max(1) as f32;

        // Draw filled histogram for each channel
        let colors = [
            (
                egui::Color32::from_rgba_premultiplied(220, 60, 60, 160),
                &r_hist,
            ),
            (
                egui::Color32::from_rgba_premultiplied(60, 220, 60, 160),
                &g_hist,
            ),
            (
                egui::Color32::from_rgba_premultiplied(60, 100, 240, 160),
                &b_hist,
            ),
        ];

        for (color, hist) in &colors {
            for bin in 0..bins {
                let h_px = hist[bin] as f32 / max_count * rect.height();
                if h_px > 0.5 {
                    let x = rect.left() + bin as f32 / bins as f32 * rect.width();
                    let bar_w = rect.width() / bins as f32;
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x, rect.bottom() - h_px),
                            egui::vec2(bar_w.max(0.5), h_px),
                        ),
                        0.0,
                        *color,
                    );
                }
            }
        }

        // 50% reference line
        let mid_y = rect.center().y;
        painter.line_segment(
            [
                egui::pos2(rect.left(), mid_y),
                egui::pos2(rect.right(), mid_y),
            ],
            egui::Stroke::new(0.5, egui::Color32::from_gray(50)),
        );
    }

    /// RGB Parade — separate waveform for each channel.
    fn draw_parade_scope(
        &self,
        painter: &egui::Painter,
        rgba: &[u8],
        w: u32,
        h: u32,
        rect: egui::Rect,
    ) {
        let cols = 128usize;
        let ch_w = rect.width() / 3.0;
        let ch_h = rect.height();
        let ch_labels = ["R", "G", "B"];
        let ch_colors = [
            egui::Color32::from_rgba_premultiplied(240, 60, 60, 200),
            egui::Color32::from_rgba_premultiplied(60, 240, 60, 200),
            egui::Color32::from_rgba_premultiplied(60, 120, 255, 200),
        ];

        for ch in 0..3 {
            let ch_left = rect.left() + ch as f32 * ch_w;
            let mut max_vals = vec![0u8; cols];
            let mut min_vals = vec![255u8; cols];

            for y in 0..h as usize {
                for x in 0..w as usize {
                    let idx = (y * w as usize + x) * 4;
                    let val = rgba[idx + ch];
                    let col = (x * cols / w as usize).min(cols - 1);
                    max_vals[col] = max_vals[col].max(val);
                    min_vals[col] = min_vals[col].min(val);
                }
            }

            for col in 0..cols {
                let x = ch_left + col as f32 / cols as f32 * ch_w;
                let y_top = rect.bottom() - max_vals[col] as f32 / 255.0 * ch_h;
                let y_bot = rect.bottom() - min_vals[col] as f32 / 255.0 * ch_h;
                if max_vals[col] > min_vals[col] {
                    painter.line_segment(
                        [egui::pos2(x, y_top), egui::pos2(x, y_bot)],
                        egui::Stroke::new(0.8, ch_colors[ch]),
                    );
                }
            }

            // Channel label
            painter.text(
                egui::pos2(ch_left + 4.0, rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                ch_labels[ch],
                egui::FontId::proportional(9.0),
                ch_colors[ch],
            );

            // Separator line between channels
            if ch < 2 {
                painter.line_segment(
                    [
                        egui::pos2(ch_left + ch_w, rect.top()),
                        egui::pos2(ch_left + ch_w, rect.bottom()),
                    ],
                    egui::Stroke::new(0.5, egui::Color32::from_gray(40)),
                );
            }
        }
    }

    // ── Transform handle rendering ──────────────────────────────────────

    fn draw_transform_handles(
        &self,
        ui: &egui::Ui,
        _response: &egui::Response,
        transform: &Transform,
        canvas_w: f32,
        canvas_h: f32,
        scale_x: f32,
        scale_y: f32,
        offset: egui::Pos2,
        _paint_rect: egui::Rect,
    ) {
        let painter = ui.painter();

        let clip_w = transform.scale.x * canvas_w;
        let clip_h = transform.scale.y * canvas_h;
        let center_x = transform.position.x + clip_w / 2.0;
        let center_y = transform.position.y + clip_h / 2.0;

        // 4 corners in canvas space (unrotated)
        let corners = [
            (transform.position.x, transform.position.y), // TL
            (transform.position.x + clip_w, transform.position.y), // TR
            (transform.position.x + clip_w, transform.position.y + clip_h), // BR
            (transform.position.x, transform.position.y + clip_h), // BL
        ];

        // Rotate corners around anchor (center)
        let rot_rad = transform.rotation_deg.to_radians();
        let cos_r = rot_rad.cos();
        let sin_r = rot_rad.sin();
        let anchor_x = center_x; // anchor at 0.5,0.5 = center
        let anchor_y = center_y;

        let rotated_corners: Vec<(f32, f32)> = corners
            .iter()
            .map(|&(cx, cy)| {
                let dx = cx - anchor_x;
                let dy = cy - anchor_y;
                (
                    anchor_x + dx * cos_r - dy * sin_r,
                    anchor_y + dx * sin_r + dy * cos_r,
                )
            })
            .collect();

        // Map to screen
        let screen_corners: Vec<egui::Pos2> = rotated_corners
            .iter()
            .map(|&(cx, cy)| egui::pos2(offset.x + cx * scale_x, offset.y + cy * scale_y))
            .collect();

        let screen_center =
            egui::pos2(offset.x + center_x * scale_x, offset.y + center_y * scale_y);

        // Draw bounding box
        let stroke = egui::Stroke::new(1.5, egui::Color32::WHITE);
        let tl = screen_corners[0];
        let tr = screen_corners[1];
        let br = screen_corners[2];
        let bl = screen_corners[3];
        painter.line_segment([tl, tr], stroke);
        painter.line_segment([tr, br], stroke);
        painter.line_segment([br, bl], stroke);
        painter.line_segment([bl, tl], stroke);

        // Center crosshair
        let ch = 8.0;
        let c_color = egui::Color32::from_rgb(200, 200, 200);
        painter.line_segment(
            [
                egui::pos2(screen_center.x - ch, screen_center.y),
                egui::pos2(screen_center.x + ch, screen_center.y),
            ],
            egui::Stroke::new(1.0, c_color),
        );
        painter.line_segment(
            [
                egui::pos2(screen_center.x, screen_center.y - ch),
                egui::pos2(screen_center.x, screen_center.y + ch),
            ],
            egui::Stroke::new(1.0, c_color),
        );

        // Corner handles (small squares)
        let h_size = 8.0;
        let h_color = egui::Color32::from_rgb(80, 160, 240);
        for corner in &screen_corners {
            let hr = egui::Rect::from_center_size(*corner, egui::vec2(h_size, h_size));
            painter.rect_filled(hr, 2.0, h_color);
            painter.rect_stroke(
                hr,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }

        // Rotation handle — small circle above top-center
        let top_center = egui::pos2((tl.x + tr.x) / 2.0, (tl.y + tr.y) / 2.0);
        let rot_handle = egui::pos2(top_center.x, top_center.y - 20.0);
        painter.line_segment(
            [top_center, rot_handle],
            egui::Stroke::new(1.0, egui::Color32::from_gray(180)),
        );
        painter.circle_filled(rot_handle, 5.0, egui::Color32::from_rgb(240, 180, 60));
        painter.circle_stroke(
            rot_handle,
            5.0,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
    }

    // ── Transform drag handling ─────────────────────────────────────────

    fn handle_transform_drag(
        &mut self,
        engine: &mut Engine,
        response: &egui::Response,
        clip_id: ClipId,
        transform: Transform,
        canvas_w: f32,
        canvas_h: f32,
        scale_x: f32,
        scale_y: f32,
        offset: egui::Pos2,
    ) {
        let pointer = response.hover_pos();
        let clicked = response.clicked();
        let dragging = response.dragged_by(egui::PointerButton::Primary);
        let released = response.drag_stopped();
        let shift = response.ctx.input(|i| i.modifiers.shift);

        let clip_w = transform.scale.x * canvas_w;
        let clip_h = transform.scale.y * canvas_h;
        let center_x = transform.position.x + clip_w / 2.0;
        let center_y = transform.position.y + clip_h / 2.0;

        // Compute corners in screen space
        let corners_canvas = [
            (transform.position.x, transform.position.y), // TL
            (transform.position.x + clip_w, transform.position.y), // TR
            (transform.position.x + clip_w, transform.position.y + clip_h), // BR
            (transform.position.x, transform.position.y + clip_h), // BL
        ];
        let rot_rad = transform.rotation_deg.to_radians();
        let cos_r = rot_rad.cos();
        let sin_r = rot_rad.sin();

        let rotated_corners: Vec<(f32, f32)> = corners_canvas
            .iter()
            .map(|&(cx, cy)| {
                let dx = cx - center_x;
                let dy = cy - center_y;
                (
                    center_x + dx * cos_r - dy * sin_r,
                    center_y + dx * sin_r + dy * cos_r,
                )
            })
            .collect();

        let screen_corners: Vec<egui::Pos2> = rotated_corners
            .iter()
            .map(|&(cx, cy)| egui::pos2(offset.x + cx * scale_x, offset.y + cy * scale_y))
            .collect();

        let top_center = egui::pos2(
            (screen_corners[0].x + screen_corners[1].x) / 2.0,
            (screen_corners[0].y + screen_corners[1].y) / 2.0,
        );
        let rot_handle = egui::pos2(top_center.x, top_center.y - 20.0);

        let h_size = 8.0;
        let handle_radius = h_size + 4.0; // generous hit area

        // Check if clicking on a handle
        if clicked {
            if let Some(pos) = pointer {
                let mut hit: Option<HandleKind> = None;

                // Check rotation handle
                if pos.distance(rot_handle) < 8.0 {
                    hit = Some(HandleKind::Rotate);
                }
                // Check corners
                for (i, corner) in screen_corners.iter().enumerate() {
                    if pos.distance(*corner) < handle_radius {
                        let corner_kind = match i {
                            0 => CornerPos::TopLeft,
                            1 => CornerPos::TopRight,
                            2 => CornerPos::BottomRight,
                            3 => CornerPos::BottomLeft,
                            _ => continue,
                        };
                        hit = Some(HandleKind::Corner(corner_kind));
                        break;
                    }
                }
                // Check center (inside bounding box)
                if hit.is_none() {
                    if point_in_quad(pos, &screen_corners) {
                        hit = Some(HandleKind::Center);
                    }
                }

                if let Some(handle) = hit {
                    self.transform_drag = Some(TransformDragState {
                        clip_id,
                        handle,
                        start_screen: pos,
                        start_transform: transform,
                    });
                    return;
                }
            }
        }

        // Handle drag
        if dragging {
            if let Some(ref drag) = self.transform_drag {
                if let Some(pos) = pointer {
                    let dx = (pos.x - drag.start_screen.x) / scale_x;
                    let dy = (pos.y - drag.start_screen.y) / scale_y;
                    // Rotate delta into clip-local space
                    let rot = drag.start_transform.rotation_deg.to_radians();
                    let cos_r = rot.cos();
                    let sin_r = rot.sin();
                    let lx = dx * cos_r + dy * sin_r;
                    let ly = -dx * sin_r + dy * cos_r;

                    let mut new_transform = drag.start_transform.clone();

                    match drag.handle {
                        HandleKind::Center => {
                            new_transform.position.x = drag.start_transform.position.x + lx;
                            new_transform.position.y = drag.start_transform.position.y + ly;
                        }
                        HandleKind::Corner(corner) => {
                            let sw = drag.start_transform.scale.x * canvas_w;
                            let sh = drag.start_transform.scale.y * canvas_h;
                            let cx = drag.start_transform.position.x;
                            let cy = drag.start_transform.position.y;

                            let (anchor_x, anchor_y) = match corner {
                                CornerPos::TopLeft => (cx + sw, cy + sh),
                                CornerPos::TopRight => (cx, cy + sh),
                                CornerPos::BottomRight => (cx, cy),
                                CornerPos::BottomLeft => (cx + sw, cy),
                            };

                            let new_corner_x = match corner {
                                CornerPos::TopLeft | CornerPos::BottomLeft => cx + lx,
                                CornerPos::TopRight | CornerPos::BottomRight => cx + sw + lx,
                            };
                            let new_corner_y = match corner {
                                CornerPos::TopLeft | CornerPos::TopRight => cy + ly,
                                CornerPos::BottomLeft | CornerPos::BottomRight => cy + sh + ly,
                            };

                            let mut new_w = (new_corner_x - anchor_x).abs().max(1.0);
                            let mut new_h = (new_corner_y - anchor_y).abs().max(1.0);

                            if shift {
                                // Proportional scale
                                let ar = sw / sh.max(1.0);
                                if ar > 0.0 {
                                    let avg_scale = ((new_w / sw) + (new_h / sh)) / 2.0;
                                    new_w = sw * avg_scale;
                                    new_h = sh * avg_scale;
                                }
                            }

                            new_transform.scale.x = new_w / canvas_w;
                            new_transform.scale.y = new_h / canvas_h;

                            // Reposition top-left
                            new_transform.position.x = anchor_x
                                - if matches!(corner, CornerPos::TopLeft | CornerPos::BottomLeft) {
                                    0.0
                                } else {
                                    0.0
                                };
                            // Actually, the position needs to be recalculated based on which corner was dragged
                            // The anchor corner stays fixed; compute new top-left from anchor and size
                            new_transform.position.x = match corner {
                                CornerPos::TopLeft | CornerPos::BottomLeft => new_corner_x,
                                CornerPos::TopRight | CornerPos::BottomRight => anchor_x,
                            };
                            new_transform.position.y = match corner {
                                CornerPos::TopLeft | CornerPos::TopRight => new_corner_y,
                                CornerPos::BottomLeft | CornerPos::BottomRight => anchor_y,
                            };

                            // Clamp minimum size
                            if new_transform.scale.x * canvas_w < 4.0 {
                                new_transform.scale.x = 4.0 / canvas_w;
                            }
                            if new_transform.scale.y * canvas_h < 4.0 {
                                new_transform.scale.y = 4.0 / canvas_h;
                            }
                        }
                        HandleKind::Rotate => {
                            // Compute angle from center to cursor
                            let cx = drag.start_transform.position.x
                                + drag.start_transform.scale.x * canvas_w / 2.0;
                            let cy = drag.start_transform.position.y
                                + drag.start_transform.scale.y * canvas_h / 2.0;

                            let sx = offset.x + cx * scale_x;
                            let sy = offset.y + cy * scale_y;

                            let start_angle =
                                (drag.start_screen.y - sy).atan2(drag.start_screen.x - sx);
                            let current_angle = (pos.y - sy).atan2(pos.x - sx);
                            let delta_deg = (current_angle - start_angle).to_degrees();
                            new_transform.rotation_deg =
                                drag.start_transform.rotation_deg + delta_deg;
                        }
                    }

                    // Apply the transform change
                    let cmd = rook_core::commands::EditCommand::SetClipTransform {
                        clip_id,
                        transform: new_transform.clone(),
                    };
                    let _ = engine.apply(cmd);

                    // Update drag start to track cumulative change
                    // (re-read transform since engine applied it)
                    if let Some(clip) = engine.project().timeline.clip(clip_id) {
                        let mut drag = self.transform_drag.take().unwrap();
                        drag.start_transform = clip.transform.clone();
                        self.transform_drag = Some(drag);
                    }
                }
            }
        }

        // Release drag
        if released {
            self.transform_drag = None;
        }
    }

    // ── Keyboard shortcuts for Position mode ────────────────────────────

    fn handle_position_keys(&self, ui: &egui::Ui, engine: &mut Engine, clip_id: ClipId) {
        let input = ui.input(|i| i.clone());
        let cmd = input.modifiers.command || input.modifiers.ctrl;

        // Read current transform
        let clip = match engine.project().timeline.clip(clip_id) {
            Some(c) => c,
            None => return,
        };
        let mut t = clip.transform.clone();
        let canvas_w = engine.project().canvas.width as f32;
        let canvas_h = engine.project().canvas.height as f32;

        let mut changed = false;

        // ── Arrow keys: nudge position ────────────────────────────────
        let nudge_px: f32 = if input.modifiers.shift { 10.0 } else { 1.0 };
        if input.key_pressed(egui::Key::ArrowLeft) {
            t.position.x -= nudge_px;
            changed = true;
        }
        if input.key_pressed(egui::Key::ArrowRight) {
            t.position.x += nudge_px;
            changed = true;
        }
        if input.key_pressed(egui::Key::ArrowUp) {
            t.position.y -= nudge_px;
            changed = true;
        }
        if input.key_pressed(egui::Key::ArrowDown) {
            t.position.y += nudge_px;
            changed = true;
        }

        // ── Cmd+Arrow: nudge scale ────────────────────────────────────
        if cmd {
            let scale_nudge: f32 = if input.modifiers.shift { 0.01 } else { 0.05 };
            if input.key_pressed(egui::Key::ArrowLeft) {
                t.scale.x = (t.scale.x - scale_nudge).max(0.01);
                changed = true;
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                t.scale.x = (t.scale.x + scale_nudge).min(10.0);
                changed = true;
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                t.scale.y = (t.scale.y + scale_nudge).min(10.0);
                changed = true;
            }
            if input.key_pressed(egui::Key::ArrowDown) {
                t.scale.y = (t.scale.y - scale_nudge).max(0.01);
                changed = true;
            }
        }

        // ── , / . : rotate ±1° (Shift → ±10°) ────────────────────────
        let rot_step: f32 = if input.modifiers.shift { 10.0 } else { 1.0 };
        if input.key_pressed(egui::Key::Comma) {
            t.rotation_deg = (t.rotation_deg - rot_step) % 360.0;
            changed = true;
        }
        if input.key_pressed(egui::Key::Period) {
            t.rotation_deg = (t.rotation_deg + rot_step) % 360.0;
            changed = true;
        }

        // ── Cmd+R : reset transform ───────────────────────────────────
        if cmd && input.key_pressed(egui::Key::R) {
            t.position = rook_core::transform::Position::default();
            t.scale = rook_core::transform::Scale { x: 1.0, y: 1.0 };
            t.rotation_deg = 0.0;
            t.anchor = rook_core::transform::AnchorPoint::default();
            t.flip_h = false;
            t.flip_v = false;
            t.opacity = 1.0;
            changed = true;
        }

        // ── Cmd+Shift+H / Cmd+Shift+V : flip horizontal/vertical ─────
        if cmd && input.modifiers.shift {
            if input.key_pressed(egui::Key::H) {
                t.flip_h = !t.flip_h;
                changed = true;
            }
            if input.key_pressed(egui::Key::V) {
                t.flip_v = !t.flip_v;
                changed = true;
            }
        }

        // ── Cmd+Option+H / Cmd+Option+V : toggle flip (alternative) ──
        if cmd && input.modifiers.alt {
            if input.key_pressed(egui::Key::H) {
                t.flip_h = !t.flip_h;
                changed = true;
            }
            if input.key_pressed(egui::Key::V) {
                t.flip_v = !t.flip_v;
                changed = true;
            }
        }

        // ── Opacity: Cmd+[ / Cmd+] ────────────────────────────────────
        if cmd {
            if input.key_pressed(egui::Key::OpenBracket) {
                t.opacity = (t.opacity - 0.05).max(0.0);
                changed = true;
            }
            if input.key_pressed(egui::Key::CloseBracket) {
                t.opacity = (t.opacity + 0.05).min(1.0);
                changed = true;
            }
        }

        if changed {
            let cmd = rook_core::commands::EditCommand::SetClipTransform {
                clip_id,
                transform: t,
            };
            let _ = engine.apply(cmd);
        }
    }

    // ── Transform info overlay ────────────────────────────────────────

    fn draw_transform_info(
        &self,
        ui: &egui::Ui,
        transform: &Transform,
        canvas_w: f32,
        canvas_h: f32,
    ) {
        let painter = ui.painter();
        let available = ui.available_size();

        // Position the overlay at the top-right of the preview area
        let x = 8.0;
        let y = 8.0;
        let line_h = 14.0;
        let lines = 7;

        // Semi-transparent background
        let bg_rect = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(170.0, line_h * lines as f32 + 8.0),
        );
        painter.rect_filled(bg_rect, 4.0, egui::Color32::from_black_alpha(140));

        let font = egui::FontId::proportional(11.0);
        let text_color = egui::Color32::from_gray(220);
        let label_color = egui::Color32::from_gray(160);

        let clip_w = transform.scale.x * canvas_w;
        let clip_h = transform.scale.y * canvas_h;

        let items: Vec<(&str, String)> = vec![
            (
                "Position",
                format!("{:.0}, {:.0}", transform.position.x, transform.position.y),
            ),
            ("Size", format!("{:.0}×{:.0}", clip_w, clip_h)),
            (
                "Scale",
                format!(
                    "{:.1}% × {:.1}%",
                    transform.scale.x * 100.0,
                    transform.scale.y * 100.0
                ),
            ),
            ("Rotation", format!("{:.1}°", transform.rotation_deg)),
            ("Opacity", format!("{:.0}%", transform.opacity * 100.0)),
            (
                "Flip",
                format!(
                    "{} {}",
                    if transform.flip_h { "H" } else { " " },
                    if transform.flip_v { "V" } else { " " },
                ),
            ),
            (
                "Anchor",
                format!("{:.2}, {:.2}", transform.anchor.x, transform.anchor.y),
            ),
        ];

        for (i, (label, value)) in items.iter().enumerate() {
            let ly = y + 4.0 + i as f32 * line_h;
            painter.text(
                egui::pos2(x + 4.0, ly),
                egui::Align2::LEFT_TOP,
                format!("{}:", label),
                font.clone(),
                label_color,
            );
            painter.text(
                egui::pos2(x + 72.0, ly),
                egui::Align2::LEFT_TOP,
                value,
                font.clone(),
                text_color,
            );
        }
    }
}

/// Detect whether the frame data is a fallback checkerboard pattern
/// (no video decode) vs actual decoded video content.
/// Checks the first ~100 non-transparent pixels for RGB uniformity —
/// video has varied colors; checkerboards are uniform gray blocks.
fn is_checkerboard_fallback(rgba: &[u8], w: u32, h: u32) -> bool {
    let cw = w as usize;
    let ch = h as usize;
    if cw == 0 || ch == 0 || rgba.len() < cw * ch * 4 {
        return true;
    }
    let mut unique_colors = 0u32;
    let mut samples = 0u32;
    // Sample a grid of 20x20 points across the frame
    let step_x = (cw / 20).max(1);
    let step_y = (ch / 20).max(1);
    let mut last_r: u8 = 0;
    let mut last_g: u8 = 0;
    let mut last_b: u8 = 0;
    for y in (0..ch).step_by(step_y) {
        for x in (0..cw).step_by(step_x) {
            let idx = (y * cw + x) * 4;
            if idx + 3 >= rgba.len() {
                continue;
            }
            let r = rgba[idx];
            let g = rgba[idx + 1];
            let b = rgba[idx + 2];
            let a = rgba[idx + 3];
            if a == 0 {
                continue;
            }
            if samples > 0 && (r != last_r || g != last_g || b != last_b) {
                unique_colors += 1;
            }
            last_r = r;
            last_g = g;
            last_b = b;
            samples += 1;
            if unique_colors > 3 {
                return false; // Varied colors = real video
            }
        }
    }
    // If we have very few unique color transitions, it's likely a pattern
    unique_colors <= 3
}

/// Check if a point is inside a convex quad defined by 4 corners in order.
fn point_in_quad(point: egui::Pos2, corners: &[egui::Pos2]) -> bool {
    if corners.len() != 4 {
        return false;
    }
    let signs: Vec<bool> = (0..4)
        .map(|i| {
            let a = corners[i];
            let b = corners[(i + 1) % 4];
            let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
            cross >= 0.0
        })
        .collect();
    signs.iter().all(|&s| s == signs[0])
}

/// Manages the preview render state — decodes frames and composites layers.
pub struct PreviewRenderer {
    frame_data: Vec<u8>,
    width: u32,
    height: u32,
    /// GPU-style texture bank for CPU compositing.
    texture_bank: super::composite_cpu::TextureBank,
    /// When true, render at half resolution for performance.
    low_quality: bool,
    /// True if the last frame_rgba() call actually decoded at least one video frame.
    pub last_had_decode: bool,
    /// Error messages from the most recent ensure_assets_open call.
    pub asset_errors: Vec<String>,
    /// Frame counter — increments every time frame_rgba() is called.
    pub frame_count: u64,
    /// Number of clips found covering the current frame.
    pub covering_clips: usize,
    /// Number of decoded textures in the bank.
    pub texture_count: usize,
}

impl Default for PreviewRenderer {
    fn default() -> Self {
        Self {
            frame_data: vec![0u8; 1920 * 1080 * 4],
            width: 1920,
            height: 1080,
            texture_bank: super::composite_cpu::TextureBank::new(),
            low_quality: false,
            last_had_decode: false,
            asset_errors: Vec::new(),
            frame_count: 0,
            covering_clips: 0,
            texture_count: 0,
        }
    }
}

impl PreviewRenderer {
    /// Set whether to render at low quality (half resolution) for performance.
    pub fn set_low_quality(&mut self, low: bool) {
        self.low_quality = low;
    }

    /// Get the current frame as RGBA bytes — composited from all visible layers.
    /// Decodes video frames, builds a FrameDescriptor, composites on CPU, returns result.
    pub fn frame_rgba(
        &mut self,
        frame: i64,
        engine: &Engine,
        bridge: &mut VideoPreviewBridge,
    ) -> &[u8] {
        let project = engine.project();
        let fps = project.frame_rate.as_f64();
        let canvas_w = project.canvas.width;
        let canvas_h = project.canvas.height;

        // Defensive clamp: never try to render past the end of the timeline.
        // If playhead is out of bounds (e.g. from ruler click on empty canvas
        // or stale state), clamp to the last valid frame so decode doesn't fail.
        let max_frame = project.timeline.duration().max(1).saturating_sub(1);
        let frame = frame.clamp(0, max_frame);

        // In low-quality mode, render at quarter resolution (1/16th pixels)
        // for real-time playback — 480×270 for 1080p, egui scales up smoothly.
        let (composite_w, composite_h) = if self.low_quality {
            ((canvas_w / 4).max(320), (canvas_h / 4).max(180))
        } else {
            (canvas_w, canvas_h)
        };

        self.width = composite_w;
        self.height = composite_h;

        // Resize frame buffer if composite dimensions changed (e.g. quality switch)
        let needed = (composite_w as usize) * (composite_h as usize) * 4;
        if self.frame_data.len() != needed {
            self.frame_data.resize(needed, 0u8);
        }

        // Open decoders for any assets not yet in the bridge (idempotent — skips already-open assets).
        // Called every frame so newly-imported clips get a decoder without requiring restart.
        self.asset_errors = self.ensure_assets_open(engine, bridge);
        self.frame_count += 1;

        // Decode frames for all visible clips at this frame → texture bank
        let mut has_any = false;

        // Collect clips that need decoding: normal clips + transition participants
        let mut clips_to_decode: Vec<(rook_core::ids::ClipId, rook_core::ids::AssetId, i64)> =
            Vec::new();

        let track_count = project.timeline.tracks.len();
        let total_clips: usize = project.timeline.tracks.iter().map(|t| t.clips.len()).sum();
        // Log once per second (every 60 frames at ~60fps) instead of every frame
        if frame == 0 || frame % 60 == 0 {
            eprintln!(
                "[frame_rgba] frame={} canvas={}x{} tracks={} total_clips={} assets={}",
                frame, canvas_w, canvas_h, track_count, total_clips,
                project.assets.len()
            );
        }
        self.covering_clips = 0;

        for track in &project.timeline.tracks {
            if !track.visible {
                continue;
            }
            for (i, clip) in track.clips.iter().enumerate() {
                if clip.covers(frame) {
                    let source_frame = clip.timeline_to_source(frame).unwrap_or(0);
                    clips_to_decode.push((clip.id, clip.asset_id, source_frame));
                    self.covering_clips += 1;
                }

                // Also decode frames for transition participants:
                // If this clip has a transition and frame is in the overlap,
                // decode the previous clip's corresponding frame
                if let Some(ref transition) = clip.transition {
                    let trans_start = clip.timeline_in;
                    let trans_end = trans_start + transition.duration_frames;
                    if frame >= trans_start && frame < trans_end {
                        // Find previous clip on same track
                        for j in 0..i {
                            let prev = &track.clips[j];
                            let prev_end = prev.timeline_in + prev.duration();
                            if prev_end == clip.timeline_in {
                                // Decode the previous clip's extended frame
                                let offset_into_transition = frame - trans_start;
                                let prev_source_frame = prev.source_in + prev.source_duration
                                    - transition.duration_frames
                                    + offset_into_transition;
                                let prev_source_frame = prev_source_frame.max(prev.source_in);
                                clips_to_decode.push((prev.id, prev.asset_id, prev_source_frame));
                                break;
                            }
                        }
                    }
                }
            }
        }

        for (clip_id, asset_id, source_frame) in &clips_to_decode {
            // eprintln!("[frame_rgba] clip={} asset={} src_frame={}", clip_id.0, asset_id.0, source_frame);
            let texture_id = format!("clip_{}", clip_id.0);
            // Check if this clip has frame blending enabled
            let has_blending = project
                .timeline
                .clip(*clip_id)
                .map(|c| c.frame_blending)
                .unwrap_or(false);

            if has_blending {
                // Decode current + next frame and blend them
                let frame_a = bridge.decode_frame_rgba(*asset_id, *source_frame, fps);
                let frame_b = bridge.decode_frame_rgba(*asset_id, source_frame + 1, fps);
                match (frame_a, frame_b) {
                    (Some((rgba_a, w_a, h_a)), Some((rgba_b, _w_b, _h_b))) => {
                        // Blend 50/50
                        let mut blended = rgba_a.clone();
                        for i in (0..blended.len()).step_by(4) {
                            if i + 3 < rgba_b.len() {
                                blended[i] = ((blended[i] as u16 + rgba_b[i] as u16) / 2) as u8;
                                blended[i + 1] =
                                    ((blended[i + 1] as u16 + rgba_b[i + 1] as u16) / 2) as u8;
                                blended[i + 2] =
                                    ((blended[i + 2] as u16 + rgba_b[i + 2] as u16) / 2) as u8;
                            }
                        }
                        self.texture_bank.upsert(texture_id, blended, w_a, h_a);
                        has_any = true;
                    }
                    (Some((rgba, w, h)), None) => {
                        // Only current frame available — use as-is
                        self.texture_bank.upsert(texture_id, rgba, w, h);
                        has_any = true;
                    }
                    _ => {}
                }
            } else {
                if let Some((rgba, w, h)) = bridge.decode_frame_rgba(*asset_id, *source_frame, fps)
                {
                    self.texture_bank.upsert(texture_id, rgba, w, h);
                    has_any = true;
                }
            }
        }

        // ── Generator clips: generate solid-color/text textures ──────────
        for track in &project.timeline.tracks {
            if !track.visible {
                continue;
            }
            for clip in &track.clips {
                if !clip.covers(frame) {
                    continue;
                }
                if let Some(ref generator) = clip.generator {
                    let texture_id = format!("clip_{}", clip.id.0);
                    match generator {
                        rook_core::clip::Generator::Solid { color } => {
                            let rgba = solid_color_rgba(
                                (color[0] * 255.0) as u8,
                                (color[1] * 255.0) as u8,
                                (color[2] * 255.0) as u8,
                                (color[3] * 255.0) as u8,
                            );
                            // Use canvas-sized texture for solid colors (will be composited)
                            self.texture_bank
                                .upsert(texture_id, rgba, canvas_w, canvas_h);
                            has_any = true;
                        }
                        rook_core::clip::Generator::Text {
                            content,
                            font_size,
                            color,
                        } => {
                            let rgba = placeholder_text_rgba(
                                content.as_str(),
                                *font_size,
                                color,
                                canvas_w,
                                canvas_h,
                            );
                            self.texture_bank
                                .upsert(texture_id, rgba, canvas_w, canvas_h);
                            has_any = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        if has_any {
            // Build compositor descriptor and composite all layers
            let desc = build_frame_descriptor(project, frame, composite_w, composite_h);
            let layer_count = desc.items.len();
            self.texture_count = layer_count; // approximate — layers ≈ textures
            if frame == 0 || frame % 60 == 0 {
                eprintln!(
                    "[frame_rgba] compositing {} layers onto {}x{} (covering={})",
                    layer_count, composite_w, composite_h, self.covering_clips
                );
            }
            self.frame_data = super::composite_cpu::composite_frame_cpu(&desc, &self.texture_bank);
            self.last_had_decode = true;
        } else {
            // Fallback: checkerboard at canvas resolution
            self.texture_count = 0;
            if frame == 0 || frame % 60 == 0 {
                eprintln!(
                    "[frame_rgba] no decoded frames — using checkerboard fallback (covering_clips={})",
                    self.covering_clips
                );
            }
            self.frame_data = checkerboard_rgba(composite_w, composite_h, frame);
            self.last_had_decode = false;
        }

        &self.frame_data
    }

    /// Register file paths for all video assets so decoders can be opened
    /// lazily on first decode.  Does NOT open any decoders — zero blocking.
    ///
    /// Returns a list of error messages for assets with missing files.
    fn ensure_assets_open(&mut self, engine: &Engine, bridge: &mut VideoPreviewBridge) -> Vec<String> {
        let mut errors = Vec::new();
        let project = engine.project();

        // ── Fully lazy decoder opening ────────────────────────────────
        // We only register file paths here (HashMap insert, sub-microsecond).
        // The actual AVAssetReader creation (1-5s) happens lazily in
        // VideoPreviewBridge::ensure_open() on the first decode_frame_rgba()
        // call — which only fires when the user presses Play.  No freeze
        // during import, no per-frame throttle needed.
        for asset in &project.assets {
            let path = std::path::PathBuf::from(asset.path());
            if !path.exists() {
                let msg = format!("asset {} path does not exist: {}", asset.id().0, path.display());
                eprintln!("[ensure_assets] {}", msg);
                errors.push(msg);
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "mxf" | "mpg" | "mpeg" | "wmv" | "flv" | "3gp" | "hevc" | "h264") {
                // Register path for lazy open — no AVFoundation call
                bridge.register_path(asset.id(), path);
            }
        }
        errors
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
