//! FFmpeg decoder using ffmpeg-next 8.1 — linear decode with hardware acceleration.
//!
//! At `open()` time the decoder probes for GPU backends in priority order:
//! **CUDA/NVDEC** → **VAAPI** (Intel/AMD) → **software**.
//! When a GPU is detected, FFmpeg automatically selects the matching
//! hardware codec (`h264_cuvid`, `h264_vaapi`, etc.).
//! Decoded frames are transferred from GPU → CPU, then scaled to RGBA.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use crate::{DecodeError, DecodedFrame};
use ffmpeg_next::format::{input, Pixel};
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context, Flags};
use ffmpeg_next::util::frame::video::Video;

use ffmpeg_next::ffi;

// ── Hardware backend enumeration ────────────────────────────────────────

/// Which hardware-acceleration backend is active for a decode session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwAccelBackend {
    /// NVIDIA NVDEC via CUDA (`h264_cuvid` / `hevc_cuvid`).
    Cuda,
    /// Intel / AMD via VA-API (`h264_vaapi` / `hevc_vaapi`).
    Vaapi,
    /// No hardware acceleration — software decode only.
    Software,
}

impl HwAccelBackend {
    /// Human-readable label (e.g. for HUD / settings display).
    pub fn label(&self) -> &'static str {
        match self {
            HwAccelBackend::Cuda => "NVIDIA NVDEC (CUDA)",
            HwAccelBackend::Vaapi => "VA-API (Intel/AMD)",
            HwAccelBackend::Software => "Software (libavcodec)",
        }
    }

    fn device_type_name(&self) -> Option<&'static str> {
        match self {
            HwAccelBackend::Cuda => Some("cuda"),
            HwAccelBackend::Vaapi => Some("vaapi"),
            HwAccelBackend::Software => None,
        }
    }

    /// Optional device path override (e.g. `/dev/dri/renderD128` for VAAPI).
    /// `None` means "use the default device".
    fn device_path(&self) -> Option<&'static str> {
        match self {
            HwAccelBackend::Vaapi => {
                // Try the first available render node.
                // FFmpeg also accepts a DRM device path here.
                if std::path::Path::new("/dev/dri/renderD128").exists() {
                    Some("/dev/dri/renderD128")
                } else if std::path::Path::new("/dev/dri/renderD129").exists() {
                    Some("/dev/dri/renderD129")
                } else {
                    None // let FFmpeg try the default
                }
            }
            _ => None,
        }
    }
}

// ── Availability probes ─────────────────────────────────────────────────

/// Returns true when NVIDIA NVDEC (CUDA) hardware decode is available.
pub fn cuda_available() -> bool {
    probe_hw_backend(&HwAccelBackend::Cuda)
}

/// Returns true when VA-API hardware decode is available.
pub fn vaapi_available() -> bool {
    probe_hw_backend(&HwAccelBackend::Vaapi)
}

fn probe_hw_backend(backend: &HwAccelBackend) -> bool {
    let name_str = match backend.device_type_name() {
        Some(n) => n,
        None => return false,
    };
    let name = match CString::new(name_str) {
        Ok(n) => n,
        Err(_) => return false,
    };
    unsafe {
        let hw_type = ffi::av_hwdevice_find_type_by_name(name.as_ptr());
        hw_type != ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE
    }
}

// ── Device creation ─────────────────────────────────────────────────────

/// Backends to probe, in priority order.
const HW_BACKEND_PRIORITY: &[HwAccelBackend] = &[HwAccelBackend::Cuda, HwAccelBackend::Vaapi];

/// Try to create a hardware device context and return the `AVBufferRef*`
/// along with which backend succeeded.
/// Returns `None` when no hardware backend is available.
fn try_create_hw_device() -> Option<(*mut ffi::AVBufferRef, HwAccelBackend)> {
    for backend in HW_BACKEND_PRIORITY {
        let dev = try_create_device_for_backend(backend);
        if dev.is_some() {
            return dev.map(|d| (d, *backend));
        }
    }
    None
}

fn try_create_device_for_backend(
    backend: &HwAccelBackend,
) -> Option<*mut ffi::AVBufferRef> {
    let name_str = backend.device_type_name()?;
    let name = CString::new(name_str).ok()?;
    let hw_type = unsafe { ffi::av_hwdevice_find_type_by_name(name.as_ptr()) };
    if hw_type == ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
        tracing::debug!(
            backend = backend.label(),
            "HW device type not found in FFmpeg build"
        );
        return None;
    }

    // Build the device path CString if one is configured
    let dev_path = backend.device_path();
    let dev_path_cstr = dev_path.map(|p| CString::new(p).ok()).flatten();
    let dev_path_ptr = dev_path_cstr
        .as_ref()
        .map(|c| c.as_ptr())
        .unwrap_or(ptr::null());

    let mut device_ref: *mut ffi::AVBufferRef = ptr::null_mut();
    let ret = unsafe {
        ffi::av_hwdevice_ctx_create(
            &mut device_ref,
            hw_type,
            dev_path_ptr,    // device path (null = default)
            ptr::null_mut(), // options
            0,               // flags
        )
    };
    if ret < 0 {
        tracing::warn!(
            backend = backend.label(),
            code = ret,
            "av_hwdevice_ctx_create failed"
        );
        return None;
    }

    tracing::info!(
        backend = backend.label(),
        dev = dev_path.unwrap_or("default"),
        "hardware device created successfully"
    );
    Some(device_ref)
}

/// Check whether `pix_fmt` is a hardware-accelerated pixel format (GPU memory).
fn is_hw_pixel_format(pix_fmt: Pixel) -> bool {
    let raw: ffi::AVPixelFormat = pix_fmt.into();
    unsafe {
        let desc = ffi::av_pix_fmt_desc_get(raw);
        if desc.is_null() {
            return false;
        }
        ((*desc).flags & ffi::AV_PIX_FMT_FLAG_HWACCEL as u64) != 0
    }
}

// ── Decoder ──────────────────────────────────────────────────────────────

pub struct Decoder {
    ictx: ffmpeg_next::format::context::Input,
    video_idx: usize,
    decoder: ffmpeg_next::decoder::Video,
    scaler: Context,
    width: u32,
    height: u32,
    fps: f64,
    frame_count: i64,
    decoded_frames: Vec<DecodedFrame>,
    current_idx: usize,

    /// Owned reference to the `AVHWDeviceContext`.  Freed in `Drop`.
    hw_device_ref: Option<*mut ffi::AVBufferRef>,

    /// Which hardware backend is active (or `Software`).
    hw_backend: HwAccelBackend,

    /// The native (software) pixel format the codec uses — used as the
    /// destination format when transferring frames from GPU to CPU.
    sw_pix_fmt: Pixel,
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        ffmpeg_next::init().map_err(|e| DecodeError::Ffmpeg(format!("init: {e}")))?;
        let mut ictx = input(&path).map_err(|e| DecodeError::Ffmpeg(format!("open: {e}")))?;

        let input = &mut ictx;
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or_else(|| DecodeError::NoVideoStream(path.to_path_buf()))?;
        let video_idx = stream.index();

        let mut context = ffmpeg_next::codec::context::Context::from_parameters(
            stream.parameters().clone(),
        )
        .map_err(|e| DecodeError::Ffmpeg(format!("params: {e}")))?;

        // ── Probe hardware backends: CUDA → VAAPI → software ──────────
        let (hw_device_ref, hw_backend) = match try_create_hw_device() {
            Some((mut dev_ref, backend)) => {
                unsafe {
                    let ctx_dev_ref = ffi::av_buffer_ref(dev_ref);
                    if ctx_dev_ref.is_null() {
                        tracing::warn!(
                            backend = backend.label(),
                            "av_buffer_ref failed — falling back to software"
                        );
                        ffi::av_buffer_unref(&mut dev_ref as *mut *mut ffi::AVBufferRef);
                        (None, HwAccelBackend::Software)
                    } else {
                        (*context.as_mut_ptr()).hw_device_ctx = ctx_dev_ref;
                        tracing::info!(
                            backend = backend.label(),
                            codec = ?stream.parameters().id(),
                            "hw_device_ctx set on decoder context"
                        );
                        (Some(dev_ref), backend)
                    }
                }
            }
            None => (None, HwAccelBackend::Software),
        };

        // Save the native pixel format before opening the decoder.
        // After open, the decoder may negotiate a hwaccel format internally.
        let sw_pix_fmt = {
            unsafe { Pixel::from((*context.as_ptr()).pix_fmt) }
        };

        let decoder = context
            .decoder()
            .video()
            .map_err(|e| DecodeError::Ffmpeg(format!("decoder: {e}")))?;

        let width = decoder.width();
        let height = decoder.height();
        let rate = stream.rate();
        let fps = rate.numerator() as f64 / rate.denominator() as f64;
        let frame_count = stream.frames() as i64;

        let scaler = Context::get(
            sw_pix_fmt,
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            Flags::BILINEAR,
        )
        .map_err(|e| DecodeError::Ffmpeg(format!("scaler: {e}")))?;

        tracing::info!(
            width,
            height,
            fps,
            backend = hw_backend.label(),
            sw_fmt = ?sw_pix_fmt,
            "decoder opened"
        );

        Ok(Self {
            ictx,
            video_idx,
            decoder,
            scaler,
            width,
            height,
            fps: if fps > 0.0 { fps } else { 24.0 },
            frame_count,
            decoded_frames: Vec::new(),
            current_idx: 0,
            hw_device_ref,
            hw_backend,
            sw_pix_fmt,
        })
    }

    /// Which hardware-acceleration backend is active for this session.
    pub fn hw_backend(&self) -> HwAccelBackend {
        self.hw_backend
    }

    /// Convenience: returns true when any hardware backend is active.
    pub fn is_hwaccel_active(&self) -> bool {
        self.hw_backend != HwAccelBackend::Software
    }

    /// Transfer a GPU-decoded frame to a software frame (CPU memory).
    /// This is a freestanding function (not `&self`) so it can be called
    /// while `self.ictx` is borrowed by `self.ictx.packets()`.
    fn transfer_to_sw(hw_frame: &Video, sw_pix_fmt: Pixel) -> Result<Video, DecodeError> {
        let mut sw_frame = Video::empty();
        sw_frame.set_format(sw_pix_fmt);

        let ret = unsafe {
            ffi::av_hwframe_transfer_data(
                sw_frame.as_mut_ptr(),
                hw_frame.as_ptr() as *mut ffi::AVFrame,
                0,
            )
        };
        if ret < 0 {
            return Err(DecodeError::Ffmpeg(format!(
                "av_hwframe_transfer_data failed (code {ret})"
            )));
        }

        // Copy PTS from hw frame to sw frame
        if let Some(pts) = hw_frame.pts() {
            unsafe {
                (*sw_frame.as_mut_ptr()).pts = pts;
            }
        }

        Ok(sw_frame)
    }

    /// Decode ALL frames into memory (simple approach, works for short clips).
    fn ensure_decoded(&mut self) -> Result<(), DecodeError> {
        if !self.decoded_frames.is_empty() {
            return Ok(());
        }

        for (s, pkt) in self.ictx.packets() {
            if s.index() != self.video_idx {
                continue;
            }
            if let Err(e) = self.decoder.send_packet(&pkt) {
                return Err(DecodeError::Ffmpeg(format!("send: {e}")));
            }
            let mut d = Video::empty();
            while self.decoder.receive_frame(&mut d).is_ok() {
                // Determine which frame to scale: if the decoded frame is on
                // the GPU, transfer it to system memory first.
                let source_frame: Video;
                let hwaccel = self.hw_backend != HwAccelBackend::Software;
                let frame_to_scale: &Video = if hwaccel && is_hw_pixel_format(d.format()) {
                    source_frame = Self::transfer_to_sw(&d, self.sw_pix_fmt)?;
                    &source_frame
                } else {
                    &d
                };

                let mut r = Video::empty();
                if let Err(e) = self.scaler.run(frame_to_scale, &mut r) {
                    return Err(DecodeError::Ffmpeg(format!("scale: {e}")));
                }
                self.decoded_frames.push(DecodedFrame {
                    width: self.width,
                    height: self.height,
                    data: r.data(0).to_vec(),
                    pts: d.pts().unwrap_or(0),
                    audio: None,
                });
            }
        }
        Ok(())
    }

    pub fn decode_frame(&mut self, frame: i64) -> Result<DecodedFrame, DecodeError> {
        self.ensure_decoded()?;
        let idx = frame as usize % self.decoded_frames.len().max(1);
        self.decoded_frames
            .get(idx)
            .cloned()
            .ok_or(DecodeError::DecodeFailed {
                frame,
                reason: "out of range".into(),
            })
    }

    pub fn frame_count(&self) -> i64 {
        self.frame_count
    }
    pub fn fps(&self) -> f64 {
        self.fps
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if let Some(dev_ref) = self.hw_device_ref {
            unsafe {
                let mut ptr: *mut ffi::AVBufferRef = dev_ref;
                ffi::av_buffer_unref(&mut ptr as *mut *mut ffi::AVBufferRef);
            }
            tracing::debug!(
                backend = self.hw_backend.label(),
                "released hardware device context"
            );
        }
    }
}
