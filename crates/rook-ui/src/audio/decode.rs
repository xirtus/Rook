//! FFmpeg-based audio PCM decoder.
//!
//! Two-phase design:
//! 1. `probe()` — fast: opens file, reads metadata (duration, rate, channels).
//! 2. `decode()` / `spawn_decode()` — slow: decodes full file to PCM.
//!    `spawn_decode()` runs in a background thread so it never blocks the UI.
//!
//! This prevents UI freezes during media import AND during first-playback decode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use anyhow::Context;
use ffmpeg_next::format;
use ffmpeg_next::media::Type;
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::channel_layout::ChannelLayout;
use ffmpeg_next::util::frame::Audio as AudioFrame;
use parking_lot::Mutex;
use rook_core::ids::AssetId;

/// State for a probed (not yet decoded) audio asset.
#[derive(Clone)]
struct ProbeState {
    /// File path for re-opening during decode.
    path: PathBuf,
    /// Duration in seconds.
    duration_secs: f64,
    /// Source sample rate.
    source_rate: u32,
    /// Source channels.
    source_channels: u16,
}

/// State for a fully decoded audio asset.
struct DecodedState {
    /// Duration in seconds.
    duration_secs: f64,
    /// Full decoded PCM buffer (interleaved f32, target rate/channels).
    pcm: Vec<f32>,
}

/// Receiver for a background decode that is in-flight.
pub type DecodeReceiver = mpsc::Receiver<Option<Vec<f32>>>;

/// Thread-safe audio decode manager.
pub struct AudioDecoder {
    /// Probed assets (fast — metadata only, no PCM yet).
    probed: Mutex<HashMap<AssetId, ProbeState>>,
    /// Decoded assets (has PCM data).
    decoded: Mutex<HashMap<AssetId, DecodedState>>,
    /// Target output format.
    target_sample_rate: u32,
    target_channels: u16,
}

impl AudioDecoder {
    pub fn new(target_sample_rate: u32, target_channels: u16) -> Self {
        ffmpeg_next::init().ok();
        Self {
            probed: Mutex::new(HashMap::new()),
            decoded: Mutex::new(HashMap::new()),
            target_sample_rate,
            target_channels,
        }
    }

    /// Quick probe — opens the file, reads audio metadata. Does NOT decode PCM.
    ///
    /// Uses raw FFI to skip `avformat_find_stream_info` (the hidden 1-5s
    /// block that opens every codec and decodes sample frames).  Reads
    /// audio parameters directly from `AVCodecParameters` — for MP4/MOV
    /// containers these are in the moov atom and require zero codec init.
    ///
    /// Returns immediately (~1ms for most files).
    pub fn probe(&self, asset_id: AssetId, path: &Path) -> anyhow::Result<()> {
        {
            let probed = self.probed.lock();
            if probed.contains_key(&asset_id) {
                return Ok(());
            }
        }
        {
            let decoded = self.decoded.lock();
            if decoded.contains_key(&asset_id) {
                return Ok(());
            }
        }

        let probe = probe_audio_fast(path)?;
        let source_rate = probe.sample_rate;
        let source_channels = probe.channels;

        eprintln!(
            "[audio_decode] probed asset {}: {}Hz {}ch dur={:.2}s path={}",
            asset_id.0, source_rate, source_channels, probe.duration_secs, path.display()
        );

        let mut probed = self.probed.lock();
        probed.insert(asset_id, ProbeState {
            path: path.to_path_buf(),
            duration_secs: probe.duration_secs,
            source_rate,
            source_channels,
        });

        Ok(())
    }

    /// Decode the full audio file to PCM synchronously.
    /// ⚠️  This blocks the calling thread — prefer `spawn_decode()` for UI use.
    pub fn decode(&self, asset_id: AssetId) -> Option<Vec<f32>> {
        // Already decoded?
        {
            let decoded = self.decoded.lock();
            if let Some(state) = decoded.get(&asset_id) {
                return Some(state.pcm.clone());
            }
        }

        let pcm = self.decode_inner(asset_id)?;

        let mut decoded = self.decoded.lock();
        decoded.insert(asset_id, DecodedState {
            duration_secs: pcm.len() as f64 / (self.target_sample_rate as f64 * self.target_channels as f64),
            pcm: pcm.clone(),
        });

        // Clean up probe entry
        self.probed.lock().remove(&asset_id);

        Some(pcm)
    }

    /// Spawn a background thread to decode the full audio file to PCM.
    /// Returns a receiver that will receive the PCM data when decoding completes.
    /// Does NOT block the calling thread — UI stays responsive.
    pub fn spawn_decode(&self, asset_id: AssetId) -> DecodeReceiver {
        let (tx, rx) = mpsc::channel();

        // Already decoded? Send immediately without spawning a thread.
        {
            let decoded = self.decoded.lock();
            if let Some(state) = decoded.get(&asset_id) {
                let pcm = state.pcm.clone();
                drop(decoded);
                let _ = tx.send(Some(pcm));
                return rx;
            }
        }

        // Check if we have probe info to decode
        let probe = {
            let probed = self.probed.lock();
            probed.get(&asset_id).cloned()
        };

        if probe.is_none() {
            // Nothing to decode — send None immediately
            let _ = tx.send(None);
            return rx;
        }

        // Spawn background thread for the heavy ffmpeg work.
        // We clone the probe + target params so the thread owns its own data.
        let target_rate = self.target_sample_rate;
        let target_ch = self.target_channels;

        thread::spawn(move || {
            let pcm = Self::decode_file_sync(&probe.unwrap(), target_rate, target_ch);
            let _ = tx.send(pcm);
        });

        rx
    }

    /// Internal: run the ffmpeg decode loop (does NOT touch self.decoded).
    fn decode_inner(&self, asset_id: AssetId) -> Option<Vec<f32>> {
        let probe = {
            let probed = self.probed.lock();
            probed.get(&asset_id).cloned()
        }?;
        Self::decode_file_sync(&probe, self.target_sample_rate, self.target_channels)
    }

    /// Pure-function decode: takes probe + target params, returns PCM or None.
    /// Safe to call from any thread — no shared state.
    fn decode_file_sync(probe: &ProbeState, target_rate: u32, target_channels: u16) -> Option<Vec<f32>> {
        let path = &probe.path;

        eprintln!(
            "[audio_decode] decoding asset ({}Hz {}ch {:.2}s) → {}Hz {}ch...",
            probe.source_rate, probe.source_channels, probe.duration_secs,
            target_rate, target_channels
        );

        let start = std::time::Instant::now();

        let mut ictx = match format::input(path) {
            Ok(ctx) => ctx,
            Err(e) => {
                eprintln!("[audio_decode] re-open failed: {e}");
                return None;
            }
        };

        let audio_stream = match ictx.streams().best(Type::Audio) {
            Some(s) => s,
            None => return None,
        };

        let stream_idx = audio_stream.index();
        let params = audio_stream.parameters();

        let codec_ctx = match ffmpeg_next::codec::context::Context::from_parameters(params.clone()) {
            Ok(c) => c,
            Err(_) => return None,
        };
        let mut decoder = match codec_ctx.decoder().audio() {
            Ok(d) => d,
            Err(_) => return None,
        };

        let source_layout = decoder.channel_layout();
        let target_layout = if target_channels == 1 {
            ChannelLayout::MONO
        } else {
            ChannelLayout::STEREO
        };

        let mut resampler = match resampling::Context::get(
            decoder.format(),
            source_layout,
            decoder.rate(),
            ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
            target_layout,
            target_rate,
        ) {
            Ok(r) => r,
            Err(_) => return None,
        };

        let est_samples =
            ((probe.duration_secs + 1.0) * target_rate as f64).ceil() as usize;
        let mut pcm = Vec::with_capacity(est_samples * target_channels as usize);
        let mut decode_frame = AudioFrame::empty();
        let channels = target_channels as usize;

        for (stream, packet) in ictx.packets() {
            if stream.index() != stream_idx {
                continue;
            }
            if decoder.send_packet(&packet).is_err() {
                continue;
            }
            while decoder.receive_frame(&mut decode_frame).is_ok() {
                let mut resampled = AudioFrame::empty();
                if resampler.run(&decode_frame, &mut resampled).is_ok() {
                    let n = resampled.samples();
                    for si in 0..n {
                        for ch in 0..channels {
                            let plane = resampled.plane::<f32>(ch);
                            pcm.push(plane.get(si).copied().unwrap_or(0.0));
                        }
                    }
                }
            }
        }

        // Flush
        decoder.send_eof().ok();
        while decoder.receive_frame(&mut decode_frame).is_ok() {
            let mut resampled = AudioFrame::empty();
            if resampler.run(&decode_frame, &mut resampled).is_ok() {
                let n = resampled.samples();
                for si in 0..n {
                    for ch in 0..channels {
                        let plane = resampled.plane::<f32>(ch);
                        pcm.push(plane.get(si).copied().unwrap_or(0.0));
                    }
                }
            }
        }

        let elapsed = start.elapsed();
        let actual_dur = pcm.len() as f64 / (target_rate as f64 * channels as f64);
        eprintln!(
            "[audio_decode] decoded {} samples ({:.2}s) in {:.1}ms",
            pcm.len(), actual_dur, elapsed.as_secs_f64() * 1000.0
        );

        Some(pcm)
    }

    /// Check if asset has been probed or decoded.
    pub fn has_asset(&self, asset_id: AssetId) -> bool {
        self.probed.lock().contains_key(&asset_id)
            || self.decoded.lock().contains_key(&asset_id)
    }

    /// Check if asset has been fully decoded (PCM available).
    pub fn is_decoded(&self, asset_id: AssetId) -> bool {
        self.decoded.lock().contains_key(&asset_id)
    }

    /// Get cached PCM without decoding.
    pub fn get_pcm(&self, asset_id: AssetId) -> Option<Vec<f32>> {
        self.decoded.lock().get(&asset_id).map(|s| s.pcm.clone())
    }

    pub fn duration_secs(&self, asset_id: AssetId) -> Option<f64> {
        if let Some(s) = self.decoded.lock().get(&asset_id) {
            return Some(s.duration_secs);
        }
        self.probed.lock().get(&asset_id).map(|s| s.duration_secs)
    }
}

// ── Fast audio probe (raw FFI, skips avformat_find_stream_info) ─────────

/// Minimal audio metadata from container headers only — no codec init.
struct AudioProbeResult {
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
}

/// Fast container-level audio probe using raw FFmpeg FFI.
///
/// Calls `avformat_open_input` but deliberately skips
/// `avformat_find_stream_info` — the latter opens every codec and decodes
/// sample frames (1-5s per stream), which is exactly the freeze we're
/// eliminating.  Reads sample_rate/channels from `AVCodecParameters` and
/// duration from `AVFormatContext`/`AVStream`.
///
/// Takes ~1ms regardless of file size or codec complexity.
fn probe_audio_fast(path: &Path) -> anyhow::Result<AudioProbeResult> {
    use std::ffi::CString;
    use ffmpeg_next::ffi;

    let cpath = CString::new(path.to_string_lossy().as_bytes())
        .context("null byte in path")?;

    let mut fmt_ctx: *mut ffi::AVFormatContext = std::ptr::null_mut();

    // Open with tiny probe size — we only need container/stream headers.
    let mut opts: *mut ffi::AVDictionary = std::ptr::null_mut();
    unsafe {
        ffi::av_dict_set(&mut opts, b"probesize\0".as_ptr() as *const i8, b"500000\0".as_ptr() as *const i8, 0);
        ffi::av_dict_set(&mut opts, b"analyzeduration\0".as_ptr() as *const i8, b"0\0".as_ptr() as *const i8, 0);

        let ret = ffi::avformat_open_input(&mut fmt_ctx, cpath.as_ptr(), std::ptr::null_mut(), &mut opts);
        ffi::av_dict_free(&mut opts);

        if ret < 0 || fmt_ctx.is_null() {
            if !fmt_ctx.is_null() {
                ffi::avformat_close_input(&mut fmt_ctx);
            }
            anyhow::bail!("avformat_open_input failed: {ret}");
        }
    }

    // NOTE: We deliberately do NOT call avformat_find_stream_info.
    // For MP4/MOV files the moov atom already has codec parameters.

    let mut sample_rate: u32 = 44100;
    let mut channels: u16 = 2;
    let mut duration_secs: f64 = 0.0;
    let mut found_audio = false;

    unsafe {
        let ctx = &*fmt_ctx;

        // Container duration (available after open for most formats)
        if ctx.duration > 0 {
            duration_secs = ctx.duration as f64 / ffi::AV_TIME_BASE as f64;
        }

        // Walk streams — read audio params directly from codecpar
        for i in 0..ctx.nb_streams as usize {
            let stream = *ctx.streams.add(i);
            if stream.is_null() {
                continue;
            }
            let codecpar = &*(*stream).codecpar;
            if codecpar.codec_type != ffi::AVMediaType::AVMEDIA_TYPE_AUDIO {
                continue;
            }

            found_audio = true;

            // Sample rate from codecpar (available without codec init)
            if codecpar.sample_rate > 0 {
                sample_rate = codecpar.sample_rate as u32;
            }

            // Channel count from ch_layout
            let ch_layout = codecpar.ch_layout;
            if ch_layout.nb_channels > 0 {
                channels = ch_layout.nb_channels as u16;
            }

            // Stream duration (more precise than container duration)
            let time_base = (*stream).time_base;
            if (*stream).duration > 0 && time_base.den > 0 {
                let stream_dur = (*stream).duration as f64 * time_base.num as f64 / time_base.den as f64;
                if stream_dur > duration_secs {
                    duration_secs = stream_dur;
                }
            }

            break; // Only need first audio stream
        }

        ffi::avformat_close_input(&mut fmt_ctx);
    }

    if !found_audio {
        anyhow::bail!("no audio stream in {}", path.display());
    }

    eprintln!(
        "[audio_decode] fast-probe {}: {}Hz {}ch {:.2}s",
        path.display(), sample_rate, channels, duration_secs
    );

    Ok(AudioProbeResult { sample_rate, channels, duration_secs })
}
