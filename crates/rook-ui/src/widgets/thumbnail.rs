//! Video thumbnail extraction and rendering for timeline clip blocks.
//!
//! Uses ffmpeg-next to decode video frames at regular intervals, scale them
//! down to small thumbnail strips, and cache them per asset.
//!
//! Extraction runs on background threads so the UI never freezes.
//! `get_or_extract()` starts a background job if data isn't cached;
//! `get()` returns cached data only.  Callers in the render path should
//! always use `get()`.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;

use rook_core::ids::AssetId;

// ── Thumbnail strip data ─────────────────────────────────────────────────

/// A single thumbnail image (RGBA, small resolution).
#[derive(Clone)]
pub struct Thumbnail {
    /// RGBA pixel data.
    pub rgba: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The frame number this thumbnail represents (source frame).
    pub source_frame: i64,
}

/// Pre-computed thumbnail strip for one asset.
#[derive(Clone)]
pub struct ThumbnailStrip {
    /// Individual thumbnail frames, evenly spaced through the asset.
    pub thumbs: Vec<Thumbnail>,
    /// Duration in seconds covered by this strip.
    pub duration_secs: f64,
    /// Spacing between thumbnails in seconds.
    pub interval_secs: f64,
}

// ── Thumbnail cache ──────────────────────────────────────────────────────

/// Thread-safe cache of thumbnail strips keyed by AssetId.
///
/// Extraction runs on background threads via `get_or_extract()`, which
/// returns `None` immediately if data isn't cached and starts a background
/// job.  Call `get()` for non-blocking cache reads on the render path.
pub struct ThumbnailCache {
    data: Arc<Mutex<HashMap<AssetId, ThumbnailStrip>>>,
    /// egui texture handles for uploaded thumbnails.
    ///
    /// Keep the handles alive for as long as the cache lives. Caching only
    /// TextureId lets egui drop the texture after the temporary handle is
    /// released, which makes thumbnails flash for a frame and then vanish.
    textures: Arc<Mutex<HashMap<(AssetId, usize), egui::TextureHandle>>>,
    /// Assets currently being extracted in background threads.
    pending: Arc<Mutex<HashSet<AssetId>>>,
    /// Receivers for in-flight background extractions.
    extract_rx: Arc<Mutex<HashMap<AssetId, mpsc::Receiver<Option<ThumbnailStrip>>>>>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            textures: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashSet::new())),
            extract_rx: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get cached thumbnail strip (never blocks).
    pub fn get(&self, id: AssetId) -> Option<ThumbnailStrip> {
        self.data.lock().get(&id).cloned()
    }

    /// Get thumbnail strip from cache, or start a background extraction.
    ///
    /// ⚠️ Never blocks — if data isn't cached, spawns a background thread
    /// and returns `None`.  On subsequent frames, `get()` will return the
    /// cached data once the background thread finishes.
    pub fn get_or_extract(&self, id: AssetId, path: &Path, fps: f64) -> Option<ThumbnailStrip> {
        // Poll completed extractions first (non-blocking)
        self.poll_completed();

        // Check cache
        {
            let cache = self.data.lock();
            if let Some(strip) = cache.get(&id) {
                return Some(strip.clone());
            }
        }

        // Already pending?
        if self.pending.lock().contains(&id) {
            return None;
        }

        // No extraction started yet — spawn background thread
        let data_clone = Arc::clone(&self.data);
        let pending_clone = Arc::clone(&self.pending);
        let id_copy = id;
        let path_buf = path.to_path_buf();

        self.pending.lock().insert(id);

        let (tx, rx) = mpsc::channel();
        self.extract_rx.lock().insert(id, rx);

        thread::spawn(move || {
            let result = extract_thumbnails(&path_buf, fps);
            match result {
                Ok(strip) => {
                    data_clone.lock().insert(id_copy, strip.clone());
                    let _ = tx.send(Some(strip));
                }
                Err(e) => {
                    tracing::warn!(?e, asset_id = %id_copy.0, "thumbnail extraction failed");
                    let _ = tx.send(None);
                }
            }
        });

        None
    }

    /// Poll for completed background extractions (non-blocking).
    pub fn poll_completed(&self) {
        let mut rx_map = self.extract_rx.lock();
        let mut completed = Vec::new();

        for (&asset_id, rx) in rx_map.iter() {
            match rx.try_recv() {
                Ok(_) => {
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still running
                }
            }
        }

        for id in &completed {
            rx_map.remove(id);
            self.pending.lock().remove(id);
        }
    }

    /// Get or upload an egui texture for a specific thumbnail.
    /// Returns the TextureId for painting.
    pub fn texture(
        &self,
        ctx: &egui::Context,
        id: AssetId,
        thumb_idx: usize,
        thumb: &Thumbnail,
    ) -> egui::TextureId {
        let key = (id, thumb_idx);
        {
            let cache = self.textures.lock();
            if let Some(handle) = cache.get(&key) {
                return handle.id();
            }
        }
        // Upload
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [thumb.width as usize, thumb.height as usize],
            &thumb.rgba,
        );
        let handle = ctx.load_texture(
            format!("thumb_{}_{}", id.0, thumb_idx),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        let mut cache = self.textures.lock();
        let tex_id = handle.id();
        cache.insert(key, handle);
        tex_id
    }

    /// Check if we have thumbnails for an asset.
    pub fn has(&self, id: AssetId) -> bool {
        self.data.lock().contains_key(&id)
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── Thumbnail extraction via ffmpeg ──────────────────────────────────────

/// Extract thumbnail frames from a video file at regular intervals.
///
/// Opens the file with ffmpeg, locates the first video stream, and decodes
/// frames at `interval_secs` spacing. Each frame is scaled to thumbnail size.
///
/// Target: ~1 thumbnail per 0.5–2 seconds of video, at ~160×90 resolution.
pub fn extract_thumbnails(path: &Path, fps: f64) -> Result<ThumbnailStrip, anyhow::Error> {
    use ffmpeg_next::format;
    use ffmpeg_next::media::Type;
    use ffmpeg_next::software::scaling;
    use ffmpeg_next::util::frame::Video;

    ffmpeg_next::init().ok();

    let mut ictx = format::input(&path).map_err(|e| anyhow::anyhow!("ffmpeg open failed: {e}"))?;

    let video_stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or_else(|| anyhow::anyhow!("no video stream in file"))?;

    let video_idx = video_stream.index();
    let params = video_stream.parameters();
    let time_base = video_stream.time_base();
    let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;

    let decoder = ffmpeg_next::codec::context::Context::from_parameters(params.clone())
        .map_err(|e| anyhow::anyhow!("video codec context: {e}"))?;
    let mut decoder = decoder
        .decoder()
        .video()
        .map_err(|e| anyhow::anyhow!("video decoder: {e}"))?;

    let src_width = decoder.width();
    let src_height = decoder.height();

    // Determine duration
    let duration_secs = if video_stream.duration() > 0 {
        video_stream.duration() as f64 * time_base_f64
    } else if ictx.duration() >= 0 {
        ictx.duration() as f64 / ffmpeg_next::ffi::AV_TIME_BASE as f64
    } else {
        0.0
    };

    // Thumbnail size: scale to ~160px wide, maintain aspect
    let thumb_w: u32 = 160;
    let thumb_h: u32 = (thumb_w as f64 * src_height as f64 / src_width as f64) as u32;
    let thumb_h = thumb_h.max(1);

    // Determine interval: aim for ~120 thumbnails or one every 0.5s, whichever is fewer
    let interval_secs = if duration_secs > 0.0 {
        let interval_by_count = duration_secs / 120.0;
        interval_by_count.max(0.5).min(2.0)
    } else {
        1.0
    };

    let total_thumbs = ((duration_secs / interval_secs).ceil() as usize).max(1);

    // Set up scaler
    let mut scaler = scaling::Context::get(
        decoder.format(),
        src_width,
        src_height,
        ffmpeg_next::format::Pixel::RGBA,
        thumb_w,
        thumb_h,
        scaling::Flags::BILINEAR,
    )
    .map_err(|e| anyhow::anyhow!("video scaler: {e}"))?;

    let mut thumbs: Vec<Thumbnail> = Vec::with_capacity(total_thumbs);
    let mut current_target_idx: usize = 0;
    let mut decode_frame = Video::empty();
    let mut last_pts: Option<i64> = None;

    for (stream, packet) in ictx.packets() {
        if stream.index() != video_idx {
            continue;
        }

        decoder
            .send_packet(&packet)
            .map_err(|e| anyhow::anyhow!("send packet: {e}"))?;

        while decoder.receive_frame(&mut decode_frame).is_ok() {
            let pts = decode_frame.pts().unwrap_or(0);
            let frame_secs = pts as f64 * time_base_f64;

            // Check if we should capture this frame
            let target_secs = current_target_idx as f64 * interval_secs;
            if frame_secs >= target_secs && current_target_idx < total_thumbs {
                // Scale to thumbnail size
                let mut scaled = Video::empty();
                scaler
                    .run(&decode_frame, &mut scaled)
                    .map_err(|e| anyhow::anyhow!("scale: {e}"))?;

                // Copy RGBA data
                let mut rgba = vec![0u8; thumb_w as usize * thumb_h as usize * 4];
                let stride = scaled.stride(0);
                let data = scaled.data(0);
                let h = scaled.height() as usize;
                let src_w = scaled.width() as usize;
                for y in 0..h {
                    let src_offset = y * stride;
                    let dst_offset = y * thumb_w as usize * 4;
                    let row_len = (src_w * 4).min(stride);
                    if src_offset + row_len <= data.len() && dst_offset + row_len <= rgba.len() {
                        rgba[dst_offset..dst_offset + row_len]
                            .copy_from_slice(&data[src_offset..src_offset + row_len]);
                    }
                }

                thumbs.push(Thumbnail {
                    rgba,
                    width: thumb_w,
                    height: thumb_h,
                    source_frame: (frame_secs * fps).round() as i64,
                });

                current_target_idx += 1;
                if current_target_idx >= total_thumbs {
                    break;
                }
            }
            last_pts = Some(pts);
        }

        if current_target_idx >= total_thumbs {
            break;
        }
    }

    Ok(ThumbnailStrip {
        thumbs,
        duration_secs,
        interval_secs,
    })
}

/// Given a thumbnail strip and clip parameters, return the subset of thumbnails
/// visible on this clip, with their relative x-offsets (0.0–1.0 across clip width).
pub fn thumbs_for_clip(
    strip: &ThumbnailStrip,
    clip_source_start_secs: f64,
    clip_source_end_secs: f64,
) -> Vec<(usize, f32)> {
    // (thumb_index, x_fraction 0..1)
    let mut result = Vec::new();
    for (i, thumb) in strip.thumbs.iter().enumerate() {
        let t = i as f64 * strip.interval_secs;
        if t >= clip_source_start_secs && t < clip_source_end_secs {
            let frac = ((t - clip_source_start_secs)
                / (clip_source_end_secs - clip_source_start_secs)) as f32;
            result.push((i, frac.clamp(0.0, 1.0)));
        }
    }
    result
}
