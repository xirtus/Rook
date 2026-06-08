//! Media pool — asset open/close + frame decode.
//!
//! On macOS: uses VideoToolbox hardware decode via `rook-decoder-native`.
//! On other platforms: falls back to stub decode (FFmpeg-next planned).
//! Frame data flows as YUV planes; RGBA conversion happens in `frame_at`.

use crate::cache::{CacheConfig, FrameCache};
use parking_lot::Mutex;
use rook_core::asset::{
    Asset, AssetId, AssetMetadata, AudioMetadata, ImageMetadata, VideoMetadata,
};
use rook_decode::{DecodeError, DecodedFrame};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Configuration for the media pool.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Frame cache configuration.
    pub cache: CacheConfig,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            cache: CacheConfig::default(),
        }
    }
}

impl PoolConfig {
    /// Reduced-footprint config for low-memory / proxy workflows.
    pub fn low_memory() -> Self {
        Self {
            cache: CacheConfig::low_memory(),
        }
    }
}

/// Owns open decoders and the shared frame cache.
///
/// Decoders are opened lazily: `open()` only stores the file path,
/// and the actual AVFoundation/ffmpeg decoder is created on the
/// first `get_frame()` call.  This prevents the import-time beachball
/// that occurred when `create_decoder()` blocked for 1-5s per file.
pub struct MediaPool {
    cache: Arc<FrameCache>,
    /// Native decoders — opened lazily on first get_frame().
    decoders: Mutex<HashMap<AssetId, NativeDecoder>>,
    /// File paths for assets that haven't had a decoder opened yet.
    /// Populated by open(); consumed by the lazy-open in get_frame().
    pending_paths: Mutex<HashMap<AssetId, std::path::PathBuf>>,
}

/// A native video decoder handle (platform-specific internals).
#[cfg(feature = "native-decode")]
struct NativeDecoder {
    inner: Box<dyn rook_decoder_native::NativeVideoDecoder>,
    props: rook_decoder_native::VideoProperties,
}

#[cfg(feature = "native-decode")]
impl NativeDecoder {
    fn open(path: &Path) -> Result<Self, DecodeError> {
        let config = rook_decoder_native::DecoderConfig {
            hardware_acceleration: true,
            preferred_format: Some(rook_decoder_native::YuvPixFmt::Nv12),
            zero_copy: false,
        };
        let inner = rook_decoder_native::create_decoder(path, config)
            .map_err(|e| DecodeError::Ffmpeg(format!("native decoder: {e}")))?;
        let props = inner.get_properties();
        Ok(Self { inner, props })
    }

    fn decode(&mut self, frame: i64) -> Result<Option<DecodedFrame>, DecodeError> {
        let fps = self.props.frame_rate;
        let timestamp = if fps > 0.0 {
            frame as f64 / fps
        } else {
            frame as f64 / 24.0
        };
        self.inner
            .seek_to(timestamp)
            .map_err(|e| DecodeError::SeekFailed(format!("{e}")))?;

        let vid_frame =
            self.inner
                .decode_frame(timestamp)
                .map_err(|e| DecodeError::DecodeFailed {
                    frame,
                    reason: format!("{e}"),
                })?;

        match vid_frame {
            Some(vf) => {
                let rgba = convert_nv12_to_rgba(
                    &vf.y_plane,
                    &vf.uv_plane,
                    vf.width as usize,
                    vf.height as usize,
                );
                Ok(Some(DecodedFrame {
                    width: vf.width,
                    height: vf.height,
                    data: rgba,
                    pts: frame,
                    audio: None,
                }))
            }
            None => Ok(None),
        }
    }
}

/// Fallback decoder using ffmpeg-next when native-decode is not enabled.
#[cfg(not(feature = "native-decode"))]
struct NativeDecoder {
    inner: rook_decode::Decoder,
    w: u32,
    h: u32,
}

#[cfg(not(feature = "native-decode"))]
impl NativeDecoder {
    fn open(path: &Path) -> Result<Self, DecodeError> {
        let inner = rook_decode::Decoder::open(path)?;
        let (w, h) = inner.dimensions();
        Ok(Self { inner, w, h })
    }
    fn decode(&mut self, frame: i64) -> Result<Option<DecodedFrame>, DecodeError> {
        self.inner.decode_frame(frame).map(Some)
    }
}

/// Fast NV12 → RGBA conversion on CPU.
/// NV12 layout: Y plane (w×h), then interleaved UV plane (w/2 × h/2 × 2).
/// Uses BT.709 coefficients for colour accuracy.
fn convert_nv12_to_rgba(y: &[u8], uv: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y_val = y[row * w + col] as f32;
            let uv_row = row / 2;
            let uv_col = (col / 2) * 2;
            let u_val = uv[uv_row * w + uv_col] as f32 - 128.0;
            let v_val = uv[uv_row * w + uv_col + 1] as f32 - 128.0;

            // BT.709 YUV → RGB
            let r = (y_val + 1.402 * v_val).clamp(0.0, 255.0) as u8;
            let g = (y_val - 0.344136 * u_val - 0.714136 * v_val).clamp(0.0, 255.0) as u8;
            let b = (y_val + 1.772 * u_val).clamp(0.0, 255.0) as u8;

            let idx = (row * w + col) * 4;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }
    rgba
}

impl MediaPool {
    pub fn new(config: PoolConfig) -> Self {
        Self {
            cache: Arc::new(FrameCache::new(config.cache)),
            decoders: Mutex::new(HashMap::new()),
            pending_paths: Mutex::new(HashMap::new()),
        }
    }

    /// Create a pool with conservative memory limits for low-RAM systems.
    pub fn new_low_memory() -> Self {
        Self::new(PoolConfig::low_memory())
    }

    /// Resize the frame cache budget at runtime (e.g. toggling low-memory mode).
    pub fn set_low_memory_mode(&self, enabled: bool) {
        let config = if enabled {
            CacheConfig::low_memory()
        } else {
            CacheConfig::default()
        };
        self.cache.resize(config);
    }

    /// Register an asset for lazy decoding.
    ///
    /// Does NOT create a decoder — only stores the path (fast, no AVFoundation).
    /// The actual decoder is created on the first `get_frame()` call.
    pub fn open(&self, id: AssetId, path: &Path) -> Result<(), DecodeError> {
        // If already have a decoder, nothing to do
        if self.decoders.lock().contains_key(&id) {
            return Ok(());
        }
        self.pending_paths.lock().insert(id, path.to_path_buf());
        Ok(())
    }

    /// Close a decoder and remove from cache.
    pub fn close(&self, id: AssetId) {
        self.decoders.lock().remove(&id);
        // Cache entries for this asset will naturally expire via LRU
    }

    /// Get a decoded frame as RGBA, hitting the cache first.
    /// Opens the decoder lazily if this is the first access for the asset.
    pub fn get_frame(&self, id: AssetId, frame: i64) -> Option<Arc<DecodedFrame>> {
        // Check cache
        if let Some(cached) = self.cache.get(id, frame) {
            return Some(cached);
        }

        // Lazy-open the decoder if we have a pending path
        {
            let mut paths = self.pending_paths.lock();
            if let Some(path) = paths.remove(&id) {
                match NativeDecoder::open(&path) {
                    Ok(decoder) => {
                        eprintln!("[pool] lazy-opened decoder for asset {} on first get_frame()", id.0);
                        self.decoders.lock().insert(id, decoder);
                    }
                    Err(e) => {
                        eprintln!("[pool] lazy-open FAILED for asset {}: {e}", id.0);
                        return None;
                    }
                }
            }
        }

        // Decode
        let mut decoders = self.decoders.lock();
        let decoder = decoders.get_mut(&id)?;
        match decoder.decode(frame) {
            Ok(Some(decoded)) => {
                let frame = Arc::new(decoded);
                drop(decoders);
                self.cache.insert(id, frame.pts, frame.clone());
                Some(frame)
            }
            _ => None,
        }
    }

    pub fn cache(&self) -> &Arc<FrameCache> {
        &self.cache
    }
}

/// Probe a file and build an Asset with metadata.
///
/// Uses ffmpeg container-level probing (fast — reads stream headers only,
/// no codec initialization).  Previously this created a full AVFoundation
/// AVAssetReader (1-5s blocking) just to read width/height/duration/fps —
/// that caused a beachball on every import.  Now it's ~10ms.
pub fn probe_asset(path: &Path) -> Result<Asset, DecodeError> {
    let path_str = path.to_string_lossy().to_string();
    let id = AssetId::next();

    // Fast container-level probe using ffmpeg — reads stream parameters
    // without initializing any codecs.  This is exactly what `ffprobe` does
    // but in-process, avoiding both the AVAssetReader block AND the subprocess
    // dependency on a Homebrew install.
    let metadata = probe_ffmpeg_fast(path).unwrap_or_else(|e| {
        eprintln!("[probe_asset] ffmpeg probe failed for {}: {e} — using defaults", path_str);
        AssetMetadata {
            duration_frames: Some(300), // 10s at 30fps fallback
            video: Some(VideoMetadata {
                width: 1920,
                height: 1080,
                codec: "unknown".into(),
                fps: 30.0,
                bitrate_bps: 0,
                has_audio: false,
            }),
            ..AssetMetadata::default()
        }
    });

    Ok(Asset::Video(rook_core::asset::VideoAsset {
        id,
        path: path_str,
        metadata,
        proxy_path: None,
        proxy_status: None,
        fingerprint: None,
    }))
}

/// Fast container-level probe using ffmpeg.
///
/// Uses raw `avformat_open_input` WITHOUT `avformat_find_stream_info` —
/// the latter opens every codec and decodes sample frames (1-5s per stream),
/// which is exactly what we're trying to avoid.  Instead we read whatever
/// metadata the container gives us for free (MP4/MOV moov atoms, etc.) and
/// fall back to sensible defaults for anything missing.
///
/// Takes ~1ms regardless of file size or codec complexity.
fn probe_ffmpeg_fast(path: &Path) -> Result<AssetMetadata, DecodeError> {
    let t0 = std::time::Instant::now();
    ffmpeg_next::init().ok();
    eprintln!("[probe_ffmpeg] init took {:?}", t0.elapsed());

    // Open the container with minimal probe — NO avformat_find_stream_info.
    // That function calls avcodec_open2 + decode for every stream, which is
    // the same AVFoundation/ffmpeg block we're trying to avoid.
    let mut meta = unsafe {
        probe_container_only(path)?
    };
    eprintln!("[probe_ffmpeg] total probe took {:?}", t0.elapsed());
    Ok(meta)
}

/// Open the container, read whatever the muxer gives us for free (no codec open).
unsafe fn probe_container_only(path: &Path) -> Result<AssetMetadata, DecodeError> {
    use std::ffi::CString;
    use ffmpeg_next::ffi;

    let cpath = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| DecodeError::Ffmpeg("null byte in path".into()))?;

    let mut fmt_ctx: *mut ffi::AVFormatContext = std::ptr::null_mut();

    // Open with tiny probe size — we only need container headers.
    let mut opts: *mut ffi::AVDictionary = std::ptr::null_mut();
    ffi::av_dict_set(&mut opts, b"probesize\0".as_ptr() as *const i8, b"500000\0".as_ptr() as *const i8, 0);
    ffi::av_dict_set(&mut opts, b"analyzeduration\0".as_ptr() as *const i8, b"0\0".as_ptr() as *const i8, 0);
    ffi::av_dict_set(&mut opts, b"fpsprobesize\0".as_ptr() as *const i8, b"0\0".as_ptr() as *const i8, 0);

    let ret = ffi::avformat_open_input(&mut fmt_ctx, cpath.as_ptr(), std::ptr::null_mut(), &mut opts);
    ffi::av_dict_free(&mut opts);

    if ret < 0 || fmt_ctx.is_null() {
        if !fmt_ctx.is_null() {
            ffi::avformat_close_input(&mut fmt_ctx);
        }
        return Err(DecodeError::Ffmpeg(format!("avformat_open_input failed: {ret}")));
    }

    // NOTE: We deliberately do NOT call avformat_find_stream_info.
    // For MP4/MOV files the moov atom already has codec parameters.
    // For files where info is missing, we return sensible defaults.

    let mut meta = AssetMetadata::default();

    // Container duration (available after open for most formats)
    let ctx = &*fmt_ctx;
    if ctx.duration > 0 {
        let dur_secs = ctx.duration as f64 / ffi::AV_TIME_BASE as f64;
        meta.duration_frames = Some((dur_secs * 30.0) as i64);
    }

    // Walk streams and extract what's available from codecpar
    for i in 0..ctx.nb_streams as usize {
        let stream = *ctx.streams.add(i);
        if stream.is_null() {
            continue;
        }
        let codecpar = &*(*stream).codecpar;
        let media_type = codecpar.codec_type;

        if media_type == ffi::AVMediaType::AVMEDIA_TYPE_VIDEO && meta.video.is_none() {
            let w = if codecpar.width > 0 { codecpar.width as u32 } else { 1920 };
            let h = if codecpar.height > 0 { codecpar.height as u32 } else { 1080 };

            // Frame rate from stream (may be 0 if not in container headers)
            let r_frame_rate = (*stream).r_frame_rate;
            let avg_frame_rate = (*stream).avg_frame_rate;
            let fps = if r_frame_rate.num > 0 && r_frame_rate.den > 0 {
                r_frame_rate.num as f64 / r_frame_rate.den as f64
            } else if avg_frame_rate.num > 0 && avg_frame_rate.den > 0 {
                avg_frame_rate.num as f64 / avg_frame_rate.den as f64
            } else {
                30.0
            };

            // Codec name
            let codec_name = unsafe {
                let desc = ffi::avcodec_descriptor_get(codecpar.codec_id);
                if desc.is_null() {
                    "unknown".to_string()
                } else {
                    std::ffi::CStr::from_ptr((*desc).name)
                        .to_string_lossy()
                        .to_string()
                }
            };

            // Stream duration if available
            let time_base = (*stream).time_base;
            let stream_dur = if (*stream).duration > 0 {
                (*stream).duration as f64 * time_base.num as f64 / time_base.den as f64
            } else if ctx.duration > 0 {
                ctx.duration as f64 / ffi::AV_TIME_BASE as f64
            } else {
                0.0
            };

            if stream_dur > 0.0 {
                meta.duration_frames = Some((stream_dur * fps) as i64);
            }

            meta.video = Some(VideoMetadata {
                width: w,
                height: h,
                codec: codec_name,
                fps,
                bitrate_bps: 0,
                has_audio: false, // checked below
            });
        }

        if media_type == ffi::AVMediaType::AVMEDIA_TYPE_AUDIO && meta.audio.is_none() {
            let sample_rate = if codecpar.sample_rate > 0 { codecpar.sample_rate as u32 } else { 48000 };
            let channels = if codecpar.ch_layout.nb_channels > 0 {
                codecpar.ch_layout.nb_channels as u8
            } else {
                2
            };

            let codec_name = unsafe {
                let desc = ffi::avcodec_descriptor_get(codecpar.codec_id);
                if desc.is_null() {
                    "unknown".to_string()
                } else {
                    std::ffi::CStr::from_ptr((*desc).name)
                        .to_string_lossy()
                        .to_string()
                }
            };

            meta.audio = Some(AudioMetadata {
                sample_rate,
                channels,
                codec: codec_name,
                bitrate_bps: 0,
            });

            // Mark video stream as having audio
            if let Some(ref mut vid) = meta.video {
                vid.has_audio = true;
            }
        }
    }

    // Post-loop: if audio was found but video stream was iterated later,
    // the has_audio flag might have been missed.  Fix it now.
    if meta.audio.is_some() {
        if let Some(ref mut vid) = meta.video {
            vid.has_audio = true;
        }
    }

    ffi::avformat_close_input(&mut fmt_ctx);
    Ok(meta)
}
