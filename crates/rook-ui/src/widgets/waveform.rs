//! Real audio waveform extraction and rendering.
//!
//! Uses ffmpeg-next to decode audio from media files, downmix to mono,
//! and compute peak amplitude bars for timeline display.
//!
//! Waveform data is cached per asset. Extraction runs on background threads
//! so the UI never freezes — `get_or_extract()` starts a background job if
//! data isn't cached, and `get()` returns cached data only.  Callers in the
//! render path should always use `get()`.

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::thread;

use rook_core::ids::AssetId;

// ── Waveform data ──────────────────────────────────────────────────────

/// Pre-computed waveform peaks for one asset.
///
/// `peaks` is a vector of peak amplitude samples (0.0–1.0) representing
/// the maximum absolute amplitude in each time window.
#[derive(Clone, Debug)]
pub struct WaveformData {
    /// Peak amplitude per bar, 0.0–1.0.
    pub peaks: Vec<f32>,
    /// Duration in seconds these peaks span.
    #[allow(dead_code)]
    pub duration_secs: f64,
    /// Number of peaks per second of audio.
    pub bars_per_second: usize,
    /// Total number of audio samples processed.
    #[allow(dead_code)]
    pub total_samples: u64,
}

// ── Waveform cache ─────────────────────────────────────────────────────

/// Thread-safe cache of waveform data keyed by AssetId.
///
/// Extraction runs on background threads via `get_or_extract()`, which
/// returns `None` immediately if data isn't cached and starts a background
/// job.  Call `poll_completed()` on the UI thread to collect finished
/// extractions; on subsequent frames `get()` will return cached data.
pub struct WaveformCache {
    data: Arc<Mutex<HashMap<AssetId, WaveformData>>>,
    /// Assets currently being extracted in background threads.
    pending: Arc<Mutex<HashSet<AssetId>>>,
    /// Receivers for in-flight background extractions.
    extract_rx: Arc<Mutex<HashMap<AssetId, mpsc::Receiver<Option<WaveformData>>>>>,
}

impl WaveformCache {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashSet::new())),
            extract_rx: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get cached waveform data (never blocks).
    pub fn get(&self, id: AssetId) -> Option<WaveformData> {
        self.data.lock().get(&id).cloned()
    }

    /// Get waveform data from cache, or start a background extraction.
    ///
    /// ⚠️ Never blocks — if data isn't cached, spawns a background thread
    /// and returns `None`.  On subsequent frames, `get()` will return the
    /// cached data once the background thread finishes.
    pub fn get_or_extract(&self, id: AssetId, path: &Path) -> Option<WaveformData> {
        // Poll completed extractions first (non-blocking)
        self.poll_completed();

        // Check cache
        {
            let cache = self.data.lock();
            if let Some(wf) = cache.get(&id) {
                return Some(wf.clone());
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
            let result = extract_waveform(&path_buf);
            match result {
                Ok(wf) => {
                    data_clone.lock().insert(id_copy, wf.clone());
                    let _ = tx.send(Some(wf));
                }
                Err(e) => {
                    tracing::warn!(?e, asset_id = %id_copy.0, "waveform extraction failed");
                    let _ = tx.send(None);
                }
            }
            // pending flag is cleared by poll_completed when the receiver is drained
        });

        None
    }

    /// Poll for completed background extractions (non-blocking).
    /// Should be called from the UI thread, e.g. in `get_or_extract()`.
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

    /// Check if an asset has waveform data cached.
    pub fn has(&self, id: AssetId) -> bool {
        self.data.lock().contains_key(&id)
    }

    /// Check if an asset's waveform is currently being extracted.
    #[allow(dead_code)]
    pub fn is_pending(&self, id: AssetId) -> bool {
        self.pending.lock().contains(&id)
    }
}

impl Default for WaveformCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── Waveform extraction via ffmpeg ─────────────────────────────────────

/// Extract waveform peaks from a media file.
///
/// Opens the file with ffmpeg, locates the first audio stream, and computes
/// peak amplitude bars by downmixing to mono and taking the max absolute
/// sample value in each time window.
///
/// Target resolution: ~60 bars per second of audio for smooth display.
pub fn extract_waveform(path: &Path) -> Result<WaveformData, anyhow::Error> {
    use ffmpeg_next::format;
    use ffmpeg_next::media::Type;
    use ffmpeg_next::software::resampling;
    use ffmpeg_next::util::channel_layout::ChannelLayout;
    use ffmpeg_next::util::frame::Audio;

    // Init ffmpeg (idempotent after first call)
    ffmpeg_next::init().ok();

    let mut ictx = format::input(&path).map_err(|e| anyhow::anyhow!("ffmpeg open failed: {e}"))?;

    // Find best audio stream
    let audio_stream = ictx
        .streams()
        .best(Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("no audio stream in file"))?;

    let audio_idx = audio_stream.index();

    // Get stream parameters and time_base for duration calculation
    let params = audio_stream.parameters();
    let time_base = audio_stream.time_base();
    let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;

    let decoder = ffmpeg_next::codec::context::Context::from_parameters(params.clone())
        .map_err(|e| anyhow::anyhow!("audio codec context: {e}"))?;
    let mut decoder = decoder
        .decoder()
        .audio()
        .map_err(|e| anyhow::anyhow!("audio decoder: {e}"))?;

    let sample_rate = decoder.rate();
    let _channels = decoder.channels();
    let channel_layout = decoder.channel_layout();

    // Set up resampler to convert to mono f32 planar
    let mut resampler = resampling::Context::get(
        decoder.format(),
        channel_layout,
        decoder.rate(),
        ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
        ChannelLayout::MONO,
        sample_rate,
    )
    .map_err(|e| anyhow::anyhow!("audio resampler: {e}"))?;

    // Determine duration for sizing using stream time_base
    let duration_secs = if audio_stream.duration() > 0 {
        audio_stream.duration() as f64 * time_base_f64
    } else if ictx.duration() >= 0 {
        // Context duration is in AV_TIME_BASE (1/MICROSECOND actually no — it's in AV_TIME_BASE units = 1/1_000_000 s)
        // In ffmpeg-next 8.x, we can use the rational time base from format context
        let fmt_time_base = ffmpeg_next::ffi::AV_TIME_BASE as f64;
        ictx.duration() as f64 / fmt_time_base
    } else {
        0.0
    };

    let bars_per_second = 60usize;
    let total_bars = ((duration_secs * bars_per_second as f64).ceil() as usize).max(1);
    let samples_per_bar = (sample_rate as f64 / bars_per_second as f64).ceil() as usize;

    // Accumulators
    let mut peaks = vec![0.0f32; total_bars];
    let mut current_bar: usize = 0;
    let mut bar_sample_count: usize = 0;
    let mut bar_peak: f32 = 0.0;
    let mut total_samples: u64 = 0;

    let mut decode_frame = Audio::empty();

    for (stream, packet) in ictx.packets() {
        if stream.index() != audio_idx {
            continue;
        }

        decoder
            .send_packet(&packet)
            .map_err(|e| anyhow::anyhow!("send packet: {e}"))?;

        while decoder.receive_frame(&mut decode_frame).is_ok() {
            // Resample to mono f32
            let mut resampled = Audio::empty();
            resampler
                .run(&decode_frame, &mut resampled)
                .map_err(|e| anyhow::anyhow!("resample: {e}"))?;

            // Read mono f32 planar samples
            let plane = resampled.plane::<f32>(0);
            for &sample in plane {
                let abs_val = sample.abs();
                bar_peak = bar_peak.max(abs_val);
                bar_sample_count += 1;
                total_samples += 1;

                if bar_sample_count >= samples_per_bar {
                    if current_bar < total_bars {
                        peaks[current_bar] = bar_peak;
                    }
                    current_bar += 1;
                    bar_sample_count = 0;
                    bar_peak = 0.0;
                }
            }
        }
    }

    // Flush remaining
    if bar_sample_count > 0 && current_bar < total_bars {
        peaks[current_bar] = bar_peak;
    }

    // Normalise peaks to 0.0–1.0 range (some codecs decode above 1.0)
    let max_peak = peaks.iter().cloned().fold(0.0f32, f32::max).max(0.001);
    for p in &mut peaks {
        *p = (*p / max_peak).min(1.0);
    }

    Ok(WaveformData {
        peaks,
        duration_secs,
        bars_per_second,
        total_samples,
    })
}

/// Compute waveform peaks to display in the timeline clip block.
///
/// Given waveform data and the timeline clip's duration/frame info, returns
/// a vector of peak values (0-1) that can be used to draw bars.
pub fn peaks_for_clip(
    waveform: &WaveformData,
    clip_duration_secs: f64,
    source_in_secs: f64,
    max_bars: usize,
) -> Vec<f32> {
    if waveform.peaks.is_empty() {
        return Vec::new();
    }

    let start_bar = (source_in_secs * waveform.bars_per_second as f64) as usize;
    let end_bar =
        ((source_in_secs + clip_duration_secs) * waveform.bars_per_second as f64).ceil() as usize;
    let end_bar = end_bar.min(waveform.peaks.len());
    let start_bar = start_bar.min(end_bar);

    if start_bar >= end_bar {
        return Vec::new();
    }

    let raw_peaks: Vec<f32> = waveform.peaks[start_bar..end_bar].to_vec();

    // Downsample to max_bars by taking max peak in each bucket
    let raw_len = raw_peaks.len();
    if raw_len <= max_bars {
        raw_peaks
    } else {
        let bucket_size = (raw_len as f64 / max_bars as f64).max(1.0) as usize;
        let mut downsampled = Vec::with_capacity(max_bars);
        for i in 0..max_bars {
            let start = i * bucket_size;
            let end = ((i + 1) * bucket_size).min(raw_len);
            if start < raw_len {
                let max_peak = raw_peaks[start..end].iter().cloned().fold(0.0f32, f32::max);
                downsampled.push(max_peak);
            }
        }
        downsampled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peaks_for_clip_empty() {
        let wf = WaveformData {
            peaks: vec![],
            duration_secs: 0.0,
            bars_per_second: 60,
            total_samples: 0,
        };
        let result = peaks_for_clip(&wf, 1.0, 0.0, 100);
        assert!(result.is_empty());
    }

    #[test]
    fn test_peaks_for_clip_downsample() {
        let wf = WaveformData {
            peaks: (0..600).map(|i| (i % 10) as f32 / 10.0).collect(),
            duration_secs: 10.0,
            bars_per_second: 60,
            total_samples: 0,
        };
        let result = peaks_for_clip(&wf, 5.0, 0.0, 20);
        assert!(!result.is_empty());
        assert!(result.len() <= 20);
    }
}
