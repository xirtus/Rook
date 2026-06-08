//! Audio subsystem: FFmpeg PCM decode + cpal output + ring buffer.
//!
//! Architecture (revised — decode is now async):
//! ```
//! UI Thread                            Background Thread(s)        Audio Thread
//! ─────────                            ────────────────────        ────────────
//! AudioBridge
//!   ├─ AudioDecoder                     decode_file_sync() → mpsc   cpal callback
//!   │    spawn_decode() → Receiver             ↓                       ↓
//!   │       ↓                            tx.send(pcm)          AudioRingBuffer.read()
//!   │  pending_decodes[asset] ← rx           │                       ↓
//!   │       ↓                                 │                  device output
//!   │  poll_completed() ← rx.try_recv() ←────┘
//!   │       ↓
//!   │  pcm_cache[asset] = pcm
//!   │       ↓
//!   │  AudioRingBuffer.write(chunk)
//!   └─ AudioOutput.play/pause
//! ```
//!
//! Key design decisions:
//! - `spawn_decode()` starts a background thread that never touches shared state.
//!   It sends PCM back via a oneshot mpsc channel.
//! - `poll_completed()` drains the channel on the UI thread (non-blocking).
//!   Stores PCM directly in the bridge's `pcm_cache`.
//! - `feed_audio_at()` polls completed decodes at the top, then feeds from cache.
//!   If PCM isn't ready yet, it returns immediately — no freeze.
//! - The ring buffer is created at the **device's** sample rate so the cpal
//!   callback reads at the correct speed. Decoded PCM is resampled to match.

pub mod decode;
pub mod output;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{mpsc, Arc};

use decode::{AudioDecoder, DecodeReceiver};
use output::AudioOutput;
use rook_core::ids::AssetId;

/// Bridge between the UI and the audio subsystem.
///
/// Caches fully-decoded PCM per asset, then feeds chunks into the
/// ring buffer each frame during playback so audio stays continuous.
///
/// All heavy work (ffmpeg decode) runs in background threads.
/// The UI thread only does O(1) ring-buffer writes.
pub struct AudioBridge {
    /// FFmpeg-based PCM decoder (shared, with parking_lot Mutex internals).
    decoder: AudioDecoder,
    /// cpal audio output with ring buffer (10 seconds capacity).
    output: Arc<AudioOutput>,
    /// Assets that have been probed.
    opened: HashSet<AssetId>,
    /// Assets that failed to open (don't retry).
    failed: HashSet<AssetId>,
    /// Background decode receivers that haven't completed yet.
    /// Keyed by asset ID; polled each frame.
    pending_decodes: HashMap<AssetId, DecodeReceiver>,
    /// Cached full PCM buffers per asset (interleaved f32, at device sample rate).
    pcm_cache: HashMap<AssetId, Vec<f32>>,
    /// Sample rate of **decoded** PCM (matches the device output rate).
    cached_sample_rate: u32,
    /// Number of channels in cache (matches the device output channels).
    cached_channels: u16,
    /// Last feed position per asset (sample index) to avoid re-feeding.
    last_feed_pos: HashMap<AssetId, usize>,
    /// Frames per second for time calculations.
    fps: f64,
    /// Whether we're waiting for decode to complete (shows "loading audio..." UX).
    pub decoding_in_progress: bool,
}

impl AudioBridge {
    /// Create a new audio bridge.
    ///
    /// The decoder and ring buffer are both configured to match the
    /// **device's** actual sample rate, not a hardcoded 48 kHz.  This
    /// avoids pitch-shift and buffer-underflow when the hardware runs at
    /// e.g. 44.1 or 96 kHz.
    pub fn new() -> anyhow::Result<Self> {
        let output = AudioOutput::new_with_capacity(10)?;
        let output = Arc::new(output);

        // The decoder must produce PCM at the device's sample rate
        // so the cpal callback reads at the correct speed.
        let sample_rate = output.sample_rate;
        let channels = output.channels;
        let decoder = AudioDecoder::new(sample_rate, channels);

        Ok(Self {
            decoder,
            output,
            opened: HashSet::new(),
            failed: HashSet::new(),
            pending_decodes: HashMap::new(),
            pcm_cache: HashMap::new(),
            cached_sample_rate: sample_rate,
            cached_channels: channels,
            last_feed_pos: HashMap::new(),
            fps: 30.0,
            decoding_in_progress: false,
        })
    }

    // ── Asset lifecycle ─────────────────────────────────────────────

    /// Probe an audio file — fast, reads metadata only (duration, rate, channels).
    /// Does NOT decode PCM. Safe to call during import without freezing.
    pub fn probe(&mut self, asset_id: AssetId, path: &Path) -> anyhow::Result<()> {
        if self.failed.contains(&asset_id) {
            return Err(anyhow::anyhow!("asset previously failed to open, skipping"));
        }
        if self.opened.contains(&asset_id) {
            return Ok(());
        }

        match self.decoder.probe(asset_id, path) {
            Ok(()) => {
                self.opened.insert(asset_id);
                Ok(())
            }
            Err(e) => {
                self.failed.insert(asset_id);
                Err(e)
            }
        }
    }

    /// Check if an asset's audio has been probed.
    pub fn has_asset(&self, asset_id: AssetId) -> bool {
        self.opened.contains(&asset_id)
    }

    /// Check if decoded PCM is available for this asset.
    pub fn has_decoded(&self, asset_id: AssetId) -> bool {
        self.pcm_cache.contains_key(&asset_id)
    }

    // ── Async decode management ─────────────────────────────────────

    /// Start background decode for an asset if it hasn't been started yet.
    /// Returns true if PCM is already available (or just-started async),
    /// false if the asset can't be decoded.
    ///
    /// Never blocks — the actual ffmpeg work runs on a background thread.
    fn ensure_decoded_async(&mut self, asset_id: AssetId) -> bool {
        // Already have PCM?
        if self.pcm_cache.contains_key(&asset_id) {
            return true;
        }

        // Already waiting for a background decode?
        if self.pending_decodes.contains_key(&asset_id) {
            return false; // Still in progress
        }

        // Start background decode
        let rx = self.decoder.spawn_decode(asset_id);
        self.pending_decodes.insert(asset_id, rx);
        self.decoding_in_progress = true;
        eprintln!(
            "[audio_bridge] started background decode for asset {}",
            asset_id.0
        );
        false // Not ready yet
    }

    /// Poll all pending background decodes. If any have completed,
    /// store the PCM in the cache. Non-blocking.
    pub fn poll_completed(&mut self) {
        if self.pending_decodes.is_empty() {
            return;
        }

        let mut completed = Vec::new();

        for (&asset_id, rx) in &self.pending_decodes {
            match rx.try_recv() {
                Ok(Some(pcm)) => {
                    let dur = pcm.len() as f64
                        / (self.cached_sample_rate as f64 * self.cached_channels as f64);
                    eprintln!(
                        "[audio_bridge] background decode complete for asset {} — {} samples ({:.1}s)",
                        asset_id.0, pcm.len(), dur
                    );
                    // Store in bridge cache only (single source of truth, no double-storage)
                    self.pcm_cache.insert(asset_id, pcm);
                    completed.push(asset_id);
                }
                Ok(None) => {
                    // Decode failed
                    eprintln!(
                        "[audio_bridge] background decode FAILED for asset {}",
                        asset_id.0
                    );
                    self.failed.insert(asset_id);
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Thread panicked or sender dropped
                    eprintln!(
                        "[audio_bridge] background decode thread died for asset {}",
                        asset_id.0
                    );
                    self.failed.insert(asset_id);
                    completed.push(asset_id);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    // Still running — nothing to do
                }
            }
        }

        for id in &completed {
            self.pending_decodes.remove(id);
        }

        if self.pending_decodes.is_empty() {
            self.decoding_in_progress = false;
        }
    }

    // ── Playback ────────────────────────────────────────────────────

    /// Feed audio from the cached PCM into the ring buffer, starting from
    /// the given source time in seconds.
    ///
    /// If the asset hasn't been decoded yet, starts a background decode
    /// and returns immediately (no blocking). On subsequent frames after
    /// the decode completes, this will begin feeding PCM data.
    pub fn feed_audio_at(&mut self, asset_id: AssetId, source_time_secs: f64) {
        // First, check for completed background decodes
        self.poll_completed();

        // If PCM isn't decoded yet, start async decode and return
        if !self.ensure_decoded_async(asset_id) {
            return; // Decode in progress — no data to feed yet
        }

        let pcm = match self.pcm_cache.get(&asset_id) {
            Some(p) => p,
            None => return,
        };

        if pcm.is_empty() {
            return;
        }

        let sample_rate = self.cached_sample_rate as f64;
        let channels = self.cached_channels as usize;

        // Check if ring buffer needs more data
        let buffered_secs = self.output.buffered_frames() as f64 / sample_rate;
        if buffered_secs > 2.0 {
            return;
        }

        // Calculate sample position from source time, but resume from last_feed_pos
        // if we've already fed past this point (prevents re-feeding the same data).
        let time_sample = ((source_time_secs * sample_rate) as usize * channels)
            .min(pcm.len().saturating_sub(channels));
        let last_pos = self.last_feed_pos.get(&asset_id).copied().unwrap_or(0);
        let start_sample = time_sample.max(last_pos).min(pcm.len().saturating_sub(channels));

        // Feed up to 1 second of audio
        let max_to_feed = (sample_rate * 1.0) as usize * channels;
        let end_sample = (start_sample + max_to_feed).min(pcm.len());

        if start_sample < end_sample {
            let chunk = &pcm[start_sample..end_sample];
            let frames_written = self.output.ring.write_samples(chunk, channels);
            // Advance last_feed_pos by samples actually written (not requested)
            let samples_written = frames_written * channels;
            let new_pos = (start_sample + samples_written).min(pcm.len());
            self.last_feed_pos.insert(asset_id, new_pos);
            eprintln!(
                "[audio_feed] wrote {} frames ({} samples) at pos {}→{} buffered={:.2}s",
                frames_written, samples_written, start_sample, new_pos,
                self.output.buffered_frames() as f64 / sample_rate
            );
        }
    }

    /// Start playback — clear the ring buffer and unmute the stream.
    pub fn play(&mut self) {
        self.output.ring.clear();
        self.last_feed_pos.clear();
        self.output.play();
    }

    /// Pause playback (output silence but keep ring buffer).
    pub fn pause(&self) {
        self.output.pause();
    }

    /// Stop and clear.
    pub fn stop(&mut self) {
        self.output.stop();
        self.last_feed_pos.clear();
    }

    /// Set the timeline frame rate.
    pub fn set_fps(&mut self, fps: f64) {
        self.fps = fps;
        self.output.set_fps(fps);
    }

    /// Set the playhead for sync.
    pub fn set_playhead_frame(&self, frame: i64) {
        self.output.set_playhead(frame);
    }

    /// Get current audio time in seconds (from ring buffer read position).
    pub fn current_time_secs(&self) -> f64 {
        self.output.current_time_secs()
    }

    /// How many seconds are buffered in the ring buffer.
    pub fn buffered_secs(&self) -> f64 {
        self.output.buffered_frames() as f64 / self.cached_sample_rate as f64
    }

    /// Number of pending background decodes.
    pub fn pending_decode_count(&self) -> usize {
        self.pending_decodes.len()
    }
}
