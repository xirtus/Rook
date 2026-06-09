//! Video preview bridge — decode media frames and convert to RGBA for display.
//!
//! On macOS, uses VideoToolbox hardware decoding via `rook-decoder-native`.
//! On other platforms, falls back to checkerboard pattern.
//!
//! Decoder opening is **fully async**: `create_decoder()` (1-5s AVFoundation init)
//! runs on a background thread.  The UI thread never blocks — if a decoder isn't
//! ready yet, `decode_frame_rgba()` returns `None` and the preview shows a
//! checkerboard until the background thread completes.  No freeze on import OR play.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use rook_core::ids::AssetId;
use rook_renderer::compositor::FrameDescriptor;

/// Per-asset video decode state.
struct DecodeState {
    /// None while a background seek is in flight.
    decoder: Option<Box<dyn rook_decoder_native::NativeVideoDecoder>>,
    width: u32,
    height: u32,
    fps: f64,
    duration_secs: f64,
    /// Cached last frame number to avoid redundant decodes.
    last_frame: i64,
    last_rgba: Vec<u8>,
}

type SeekResult = (Box<dyn rook_decoder_native::NativeVideoDecoder>, Vec<u8>, i64);

/// Manages video decode for the preview panel.
///
/// Decoders are opened asynchronously on background threads.
/// Seeks are also asynchronous: when a non-sequential frame is requested the
/// decoder is moved to a background thread which performs seek+decode, while
/// the UI thread immediately returns the last cached frame.
pub struct VideoPreviewBridge {
    /// Open decoders keyed by asset ID.
    states: HashMap<AssetId, DecodeState>,
    /// Assets that failed to open — never retry.
    failed: HashSet<AssetId>,
    /// File paths for assets that haven't been opened yet.
    known_paths: HashMap<AssetId, PathBuf>,
    /// Assets currently being opened on background threads.
    opening: HashSet<AssetId>,
    /// Channels for receiving decoder-open results from background threads.
    open_channels: HashMap<AssetId, mpsc::Receiver<Option<DecodeState>>>,
    /// Channels for receiving async seek+decode results.
    seek_rx: HashMap<AssetId, mpsc::Receiver<SeekResult>>,
}

impl VideoPreviewBridge {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            failed: HashSet::new(),
            known_paths: HashMap::new(),
            opening: HashSet::new(),
            open_channels: HashMap::new(),
            seek_rx: HashMap::new(),
        }
    }

    /// Poll for completed background decode results. Returns true if any new
    /// frame data arrived — callers should re-composite when this is true.
    pub fn poll_new_frames(&mut self) -> bool {
        self.poll_open_results();
        self.poll_seek_results()
    }

    /// True if any background decode or open operations are still in flight.
    pub fn has_pending_decodes(&self) -> bool {
        !self.seek_rx.is_empty() || !self.open_channels.is_empty()
    }

    /// Register a file path for an asset so the decoder can be opened lazily.
    /// Call this every frame for all video assets in the project.
    /// Fast — just a HashMap insert, no ffmpeg/AVFoundation calls.
    pub fn register_path(&mut self, asset_id: AssetId, path: PathBuf) {
        if self.states.contains_key(&asset_id) || self.failed.contains(&asset_id) {
            return;
        }
        self.known_paths.insert(asset_id, path);
    }

    /// Open a video file for decoding (synchronous — blocks for 1-5s on AVFoundation init).
    /// Prefer lazy open via `decode_frame_rgba()` instead of calling this directly.
    pub fn open(&mut self, asset_id: AssetId, path: &std::path::Path) -> anyhow::Result<()> {
        if self.failed.contains(&asset_id) {
            return Err(anyhow::anyhow!("asset previously failed to open, skipping"));
        }
        if self.states.contains_key(&asset_id) {
            return Ok(()); // already open
        }

        let config = rook_decoder_native::DecoderConfig {
            hardware_acceleration: true,
            preferred_format: Some(rook_decoder_native::YuvPixFmt::Nv12),
            zero_copy: false,
        };

        let decoder = match rook_decoder_native::create_decoder(path, config) {
            Ok(d) => d,
            Err(e) => {
                self.failed.insert(asset_id);
                self.known_paths.remove(&asset_id);
                return Err(e);
            }
        };
        let props = decoder.get_properties();

        eprintln!(
            "[video_bridge] opened asset {}: {}x{} @ {:.2}fps dur={:.2}s path={}",
            asset_id.0,
            props.width,
            props.height,
            props.frame_rate,
            props.duration,
            path.display()
        );

        self.states.insert(
            asset_id,
            DecodeState {
                decoder: Some(decoder),
                width: props.width,
                height: props.height,
                fps: props.frame_rate,
                duration_secs: props.duration,
                last_frame: -1,
                last_rgba: {
                    let w = props.width as usize;
                    let h = props.height as usize;
                    let sz = w * h * 4;
                    let mut v = vec![0u8; sz];
                    let sq = 32usize;
                    for y in 0..h {
                        for x in 0..w {
                            let idx = (y * w + x) * 4;
                            let dark = ((x / sq) + (y / sq)) % 2 == 0;
                            if dark {
                                v[idx] = 30;
                                v[idx + 1] = 30;
                                v[idx + 2] = 35;
                            } else {
                                v[idx] = 22;
                                v[idx + 1] = 22;
                                v[idx + 2] = 27;
                            }
                            v[idx + 3] = 255;
                        }
                    }
                    v
                },
            },
        );

        // Clean up — no longer needed in known_paths
        self.known_paths.remove(&asset_id);

        Ok(())
    }

    /// Check if we have a decoder for this asset (or know it failed).
    pub fn has_asset(&self, asset_id: AssetId) -> bool {
        self.states.contains_key(&asset_id) || self.failed.contains(&asset_id)
    }

    /// Poll for completed background decoder opens.
    /// Call this at the start of every `decode_frame_rgba()` call.
    fn poll_open_results(&mut self) {
        if self.open_channels.is_empty() {
            return;
        }
        let mut completed = Vec::new();
        for (&asset_id, rx) in &self.open_channels {
            match rx.try_recv() {
                Ok(Some(state)) => {
                    eprintln!(
                        "[video_bridge] background open complete for asset {} — {}x{}",
                        asset_id.0, state.width, state.height
                    );
                    self.states.insert(asset_id, state);
                    completed.push(asset_id);
                }
                Ok(None) => {
                    eprintln!("[video_bridge] background open FAILED for asset {}", asset_id.0);
                    self.failed.insert(asset_id);
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    eprintln!("[video_bridge] background open thread died for asset {}", asset_id.0);
                    self.failed.insert(asset_id);
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still opening — nothing to do
                }
            }
        }
        for id in &completed {
            self.open_channels.remove(id);
            self.opening.remove(id);
        }
    }

    /// Poll for completed background seek+decode results.
    /// Returns true if at least one new frame arrived (caller should re-composite).
    fn poll_seek_results(&mut self) -> bool {
        if self.seek_rx.is_empty() {
            return false;
        }
        let mut done = Vec::new();
        let mut got_new = false;
        for (&asset_id, rx) in &self.seek_rx {
            match rx.try_recv() {
                Ok((decoder, rgba, frame_num)) => {
                    if let Some(state) = self.states.get_mut(&asset_id) {
                        state.decoder = Some(decoder);
                        if !rgba.is_empty() {
                            state.last_rgba = rgba;
                            got_new = true;
                        }
                        state.last_frame = frame_num;
                    }
                    done.push(asset_id);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    done.push(asset_id);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        for id in &done {
            self.seek_rx.remove(id);
        }
        got_new
    }

    /// Ensure a decoder is open for the asset.
    ///
    /// If no decoder exists yet and we have a known path, spawns a background
    /// thread for the AVFoundation `create_decoder()` call (1-5s). Returns
    /// `true` if a decoder is ready **right now**, `false` if we need to wait.
    ///
    /// Never blocks the UI thread.
    fn ensure_open(&mut self, asset_id: AssetId) -> bool {
        // Already have a decoder?
        if self.states.contains_key(&asset_id) {
            return true;
        }
        // Known failure?
        if self.failed.contains(&asset_id) {
            return false;
        }
        // Already opening in background?
        if self.opening.contains(&asset_id) {
            return false;
        }

        // Spawn background thread for the blocking create_decoder call
        if let Some(path) = self.known_paths.remove(&asset_id) {
            self.opening.insert(asset_id);
            let (tx, rx) = mpsc::channel();
            self.open_channels.insert(asset_id, rx);

            eprintln!(
                "[video_bridge] spawning background open for asset {}: {}",
                asset_id.0, path.display()
            );

            std::thread::spawn(move || {
                let result = Self::open_sync(&path);
                let _ = tx.send(result);
            });
        }

        false // Not ready yet — checkerboard fallback
    }

    /// Synchronous decoder open — runs on a background thread.
    /// Returns Some(DecodeState) on success, None on failure.
    fn open_sync(path: &std::path::Path) -> Option<DecodeState> {
        let config = rook_decoder_native::DecoderConfig {
            hardware_acceleration: true,
            preferred_format: Some(rook_decoder_native::YuvPixFmt::Nv12),
            zero_copy: false,
        };

        let decoder = match rook_decoder_native::create_decoder(path, config) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[video_bridge] open_sync FAILED for {}: {e}", path.display());
                return None;
            }
        };
        let props = decoder.get_properties();

        eprintln!(
            "[video_bridge] open_sync OK: {}x{} @ {:.2}fps dur={:.2}s path={}",
            props.width, props.height, props.frame_rate, props.duration, path.display()
        );

        let w = props.width as usize;
        let h = props.height as usize;
        let sz = w * h * 4;
        let mut checkerboard = vec![0u8; sz];
        let sq = 32usize;
        for y in 0..h {
            for x in 0..w {
                let idx = (y * w + x) * 4;
                let dark = ((x / sq) + (y / sq)) % 2 == 0;
                if dark {
                    checkerboard[idx] = 30;
                    checkerboard[idx + 1] = 30;
                    checkerboard[idx + 2] = 35;
                } else {
                    checkerboard[idx] = 22;
                    checkerboard[idx + 1] = 22;
                    checkerboard[idx + 2] = 27;
                }
                checkerboard[idx + 3] = 255;
            }
        }

        Some(DecodeState {
            decoder: Some(decoder),
            width: props.width,
            height: props.height,
            fps: props.frame_rate,
            duration_secs: props.duration,
            last_frame: -1,
            last_rgba: checkerboard,
        })
    }

    /// Decode a frame and return RGBA bytes. Never blocks the UI thread.
    ///
    /// Every decode (seek or sequential) is dispatched to a background thread.
    /// The last cached frame is returned immediately while decoding runs.
    /// The cache is updated once the background thread finishes.
    pub fn decode_frame_rgba(
        &mut self,
        asset_id: AssetId,
        source_frame: i64,
        fps: f64,
    ) -> Option<(Vec<u8>, u32, u32)> {
        self.poll_open_results();
        self.poll_seek_results(); // ignore return here — caller polls separately

        if !self.ensure_open(asset_id) {
            return None;
        }

        let state = self.states.get_mut(&asset_id)?;

        // Cache hit: same frame, return immediately
        if state.last_frame == source_frame && !state.last_rgba.is_empty() {
            return Some((state.last_rgba.clone(), state.width, state.height));
        }

        // Background decode already in flight — return cached frame while we wait
        if self.seek_rx.contains_key(&asset_id) || state.decoder.is_none() {
            return Some((state.last_rgba.clone(), state.width, state.height));
        }

        let asset_fps = if state.fps.is_finite() && state.fps > 0.0 {
            state.fps
        } else {
            fps
        };
        let timestamp = source_frame as f64 / asset_fps;
        let max_timestamp = (state.duration_secs - (0.5 / asset_fps)).max(0.0);
        let timestamp = timestamp.clamp(0.0, max_timestamp);

        let need_seek = (state.last_frame < 0 && source_frame > 0)
            || (state.last_frame >= 0 && source_frame != state.last_frame + 1);

        // Move decoder to background thread for decode (seek + decode if needed,
        // or just decode for sequential frames). UI thread never calls decode_frame.
        let mut decoder = state.decoder.take().unwrap();
        let (tx, rx) = mpsc::channel();
        self.seek_rx.insert(asset_id, rx);

        std::thread::spawn(move || {
            let result: anyhow::Result<(Vec<u8>, i64)> = (|| {
                if need_seek {
                    decoder.seek_to(timestamp)?;
                }
                let frame = decoder
                    .decode_frame(timestamp)?
                    .ok_or_else(|| anyhow::anyhow!("no frame"))?;
                let rgba = match frame.format {
                    rook_decoder_native::YuvPixFmt::Nv12 => nv12_to_rgba(
                        &frame.y_plane,
                        &frame.uv_plane,
                        frame.width as usize,
                        frame.height as usize,
                    ),
                    rook_decoder_native::YuvPixFmt::P010 => p010_to_rgba(
                        &frame.y_plane,
                        &frame.uv_plane,
                        frame.width as usize,
                        frame.height as usize,
                    ),
                };
                Ok((rgba, source_frame))
            })();
            let (rgba, frame_num) = result.unwrap_or_else(|e| {
                eprintln!("[decode_frame_rgba] bg decode FAIL frame={} ts={:.4}: {:?}", source_frame, timestamp, e);
                (vec![], -1)
            });
            let _ = tx.send((decoder, rgba, frame_num));
        });

        // Return last cached frame while background decode runs
        Some((state.last_rgba.clone(), state.width, state.height))
    }

    /// Get dimensions for an asset.
    pub fn dimensions(&self, asset_id: AssetId) -> Option<(u32, u32)> {
        self.states.get(&asset_id).map(|s| (s.width, s.height))
    }

    /// Build a compositor FrameDescriptor for a specific frame,
    /// using decoded video frames as layer textures.
    ///
    /// This is the bridge between the editor model and the GPU compositor.
    /// For now it returns a list of layers that the compositor can render.
    /// The actual GPU texture upload happens in a separate step.
    pub fn build_frame_layers(
        &mut self,
        project: &rook_core::project::Project,
        frame: i64,
        canvas_w: u32,
        canvas_h: u32,
    ) -> (Vec<DecodedLayer>, FrameDescriptor) {
        let mut layers = Vec::new();
        let mut frame_items = Vec::new();

        for track in &project.timeline.tracks {
            if !track.visible {
                continue;
            }
            for clip in &track.clips {
                if !clip.covers(frame) {
                    continue;
                }
                let source_frame = clip.timeline_to_source(frame).unwrap_or(0);

                // Decode the frame
                let (rgba, clip_w, clip_h) = self
                    .decode_frame_rgba(clip.asset_id, source_frame, project.frame_rate.as_f64())
                    .unwrap_or_else(|| {
                        generate_checkerboard(clip_w_default(&project), canvas_w, canvas_h, frame)
                    });

                let texture_id = format!("clip_{}", clip.id.0);

                layers.push(DecodedLayer {
                    texture_id: texture_id.clone(),
                    rgba,
                    width: clip_w,
                    height: clip_h,
                });

                // Build compositor layer descriptor
                let transform = rook_renderer::compositor::QuadTransformDescriptor {
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

                let blend = map_blend_mode(clip.blend_mode);

                frame_items.push(rook_renderer::compositor::FrameItemDescriptor::Layer(
                    rook_renderer::compositor::LayerDescriptor {
                        texture_id,
                        transform,
                        opacity: clip.transform.opacity,
                        blend_mode: blend,
                        effect_pass_groups: vec![],
                        mask: None,
                    },
                ));
            }
        }

        let descriptor = FrameDescriptor {
            width: canvas_w,
            height: canvas_h,
            clear: rook_renderer::compositor::CanvasClearDescriptor {
                color: [0.0, 0.0, 0.0, 1.0],
            },
            items: frame_items,
        };

        (layers, descriptor)
    }
}

/// A decoded layer ready for GPU upload.
pub struct DecodedLayer {
    pub texture_id: String,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Default for VideoPreviewBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ── NV12 → RGBA conversion (BT.709 limited-range, CPU) ───────────────────

/// Convert NV12 YCbCr to RGBA using BT.709 limited-range (video-range) coefficients.
///
/// NV12 layout: Y plane (W×H) followed by interleaved UV plane (W/2 × H/2).
fn nv12_to_rgba(y_plane: &[u8], uv_plane: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let y_idx = y * width + x;
            // BT.709 limited-range (video-range): Y in [16,235], Cb/Cr in [16,240].
            // VideoToolbox outputs kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange ('420v').
            let yy = (y_plane[y_idx] as f32 - 16.0) * 1.16438;

            // UV plane: half resolution, interleaved
            let uv_x = x / 2;
            let uv_y = y / 2;
            let uv_idx = (uv_y * (width / 2) + uv_x) * 2;
            let uu = uv_plane.get(uv_idx).copied().unwrap_or(128) as f32 - 128.0;
            let vv = uv_plane.get(uv_idx + 1).copied().unwrap_or(128) as f32 - 128.0;

            // BT.709 limited-range YCbCr → RGB
            let r = (yy + 1.79274 * vv).clamp(0.0, 255.0) as u8;
            let g = (yy - 0.21325 * uu - 0.53291 * vv).clamp(0.0, 255.0) as u8;
            let b = (yy + 2.11240 * uu).clamp(0.0, 255.0) as u8;

            let rgba_idx = y_idx * 4;
            rgba[rgba_idx] = r;
            rgba[rgba_idx + 1] = g;
            rgba[rgba_idx + 2] = b;
            rgba[rgba_idx + 3] = 255;
        }
    }

    rgba
}

/// Convert P010 YCbCr to RGBA using BT.709 limited-range coefficients.
///
/// P010 stores 10-bit samples in 16-bit words. The decoder copies the raw
/// little-endian words into byte buffers, so we read u16 values and shift off
/// the low padding bits before conversion.
fn p010_to_rgba(y_plane: &[u8], uv_plane: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let y_idx = y * width + x;
            let y10 = read_u16_le(y_plane, y_idx * 2).unwrap_or(64) >> 6;
            let yy = (y10 as f32 - 64.0) * (255.0 / 876.0);

            let uv_x = x / 2;
            let uv_y = y / 2;
            let uv_idx = (uv_y * (width / 2) + uv_x) * 2;
            let uu10 = read_u16_le(uv_plane, uv_idx * 2).unwrap_or(512) >> 6;
            let vv10 = read_u16_le(uv_plane, uv_idx * 2 + 2).unwrap_or(512) >> 6;
            let uu = uu10 as f32 - 512.0;
            let vv = vv10 as f32 - 512.0;

            let r = (yy + 0.448_914 * vv).clamp(0.0, 255.0) as u8;
            let g = (yy - 0.054_321 * uu - 0.133_789 * vv).clamp(0.0, 255.0) as u8;
            let b = (yy + 0.563_608 * uu).clamp(0.0, 255.0) as u8;

            let rgba_idx = y_idx * 4;
            rgba[rgba_idx] = r;
            rgba[rgba_idx + 1] = g;
            rgba[rgba_idx + 2] = b;
            rgba[rgba_idx + 3] = 255;
        }
    }

    rgba
}

fn read_u16_le(buf: &[u8], byte_idx: usize) -> Option<u16> {
    let lo = *buf.get(byte_idx)? as u16;
    let hi = *buf.get(byte_idx + 1)? as u16;
    Some(lo | (hi << 8))
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn clip_w_default(project: &rook_core::project::Project) -> u32 {
    project.canvas.width.max(1920)
}

fn map_blend_mode(mode: rook_core::clip::BlendMode) -> rook_renderer::compositor::BlendMode {
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

/// Generate a fallback checkerboard pattern (same as original test pattern).
fn generate_checkerboard(w: u32, canvas_w: u32, canvas_h: u32, frame: i64) -> (Vec<u8>, u32, u32) {
    let cw = canvas_w as usize;
    let ch = canvas_h as usize;
    let mut rgba = vec![0u8; cw * ch * 4];
    let square = 40usize;

    for y in 0..ch {
        for x in 0..cw {
            let idx = (y * cw + x) * 4;
            let dark = (x / square + y / square) % 2 == 0;
            if dark {
                rgba[idx] = 28;
                rgba[idx + 1] = 28;
                rgba[idx + 2] = 32;
            } else {
                rgba[idx] = 22;
                rgba[idx + 1] = 22;
                rgba[idx + 2] = 26;
            }
            rgba[idx + 3] = 255;
        }
    }

    // Frame number overlay
    let label = format!("Frame: {}", frame);
    for (ci, chr) in label.chars().enumerate() {
        let fx = 12 + ci * 12;
        for dy in 0..12 {
            for dx in 0..8 {
                let px = fx + dx;
                let py = 12 + dy;
                if px < cw && py < ch {
                    let idx = (py * cw + px) * 4;
                    let br = if chr == ':' || chr == ' ' {
                        120u8
                    } else {
                        220u8
                    };
                    rgba[idx] = br;
                    rgba[idx + 1] = br;
                    rgba[idx + 2] = if chr.is_ascii_digit() { 100 } else { 220 };
                }
            }
        }
    }

    (rgba, canvas_w, canvas_h)
}
