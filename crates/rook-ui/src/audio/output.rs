//! Audio output via cpal.
//!
//! Creates a platform audio stream fed by a lock-free ring buffer.
//! The ring buffer is sized to match the **device's** actual sample rate
//! so the cpal callback reads at the correct speed with no resampling.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Ring buffer for passing audio samples from UI thread to audio thread.
/// Uses atomic read/write indices for lock-free operation.
/// Uses UnsafeCell for interior mutability (safe because atomic indices
/// guarantee no concurrent access to the same element).
pub struct AudioRingBuffer {
    /// Interleaved f32 samples. UnsafeCell allows mutation through &self.
    data: UnsafeCell<Vec<f32>>,
    /// Capacity in frames (samples / channels).
    capacity: usize,
    /// Write index (UI thread writes here).
    write: AtomicU64,
    /// Read index (audio thread reads here).
    read: AtomicU64,
}

// AudioRingBuffer is Send+Sync because access is coordinated by atomics
unsafe impl Send for AudioRingBuffer {}
unsafe impl Sync for AudioRingBuffer {}

impl AudioRingBuffer {
    /// Create a new ring buffer with given capacity in frames.
    pub fn new(capacity_frames: usize, channels: usize) -> Self {
        Self {
            data: UnsafeCell::new(vec![0.0f32; capacity_frames * channels]),
            capacity: capacity_frames,
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
        }
    }

    /// Number of frames available to read.
    pub fn available(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire);
        ((w.wrapping_sub(r)) as usize).min(self.capacity)
    }

    /// Number of frames that can be written.
    pub fn free(&self) -> usize {
        self.capacity.saturating_sub(self.available())
    }

    /// Write interleaved samples to the ring buffer.
    /// Returns number of frames actually written.
    pub fn write_samples(&self, samples: &[f32], channels: usize) -> usize {
        let frames = samples.len() / channels;
        let to_write = frames.min(self.free());
        if to_write == 0 || frames == 0 {
            return 0;
        }

        let data = unsafe { &mut *self.data.get() };
        let w = self.write.load(Ordering::Acquire) as usize % self.capacity;
        let samples_per_chunk = to_write * channels;
        let data_len = data.len();

        let first_part = samples_per_chunk.min(data_len - w * channels);
        data[w * channels..w * channels + first_part].copy_from_slice(&samples[..first_part]);

        if first_part < samples_per_chunk {
            let remaining = samples_per_chunk - first_part;
            data[..remaining].copy_from_slice(&samples[first_part..samples_per_chunk]);
        }

        self.write
            .store(self.write.load(Ordering::Acquire).wrapping_add(to_write as u64), Ordering::Release);
        to_write
    }

    /// Read frames from the ring buffer into output.
    /// Returns number of frames actually read.
    pub fn read_samples(&self, output: &mut [f32], channels: usize) -> usize {
        let frames_wanted = output.len() / channels;
        let available = self.available();
        let to_read = frames_wanted.min(available);

        if to_read == 0 {
            // Output silence
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return 0;
        }

        let data = unsafe { &*self.data.get() };
        let r = self.read.load(Ordering::Acquire) as usize % self.capacity;
        let samples_per_chunk = to_read * channels;
        let data_len = data.len();

        let first_part = samples_per_chunk.min(data_len - r * channels);
        output[..first_part].copy_from_slice(&data[r * channels..r * channels + first_part]);

        if first_part < samples_per_chunk {
            let remaining = samples_per_chunk - first_part;
            output[first_part..samples_per_chunk].copy_from_slice(&data[..remaining]);
        }

        // Zero out remaining output
        for i in samples_per_chunk..output.len() {
            output[i] = 0.0;
        }

        self.read
            .store(self.read.load(Ordering::Acquire).wrapping_add(to_read as u64), Ordering::Release);
        to_read
    }

    /// Clear all data in the ring buffer.
    pub fn clear(&self) {
        let w = self.write.load(Ordering::Acquire);
        self.read.store(w, Ordering::Release);
    }

    /// Get the current read position in frames (total frames consumed).
    pub fn read_position(&self) -> u64 {
        self.read.load(Ordering::Acquire)
    }

    /// Get the current write position in frames (total frames written).
    pub fn write_position(&self) -> u64 {
        self.write.load(Ordering::Acquire)
    }
}

/// Shared state between UI thread and audio thread.
pub struct AudioOutput {
    /// Ring buffer for PCM data.
    pub ring: Arc<AudioRingBuffer>,
    /// cpal stream handle (held to keep the stream alive).
    stream: Option<cpal::Stream>,
    /// Sample rate of the output device.
    pub sample_rate: u32,
    /// Number of output channels.
    pub channels: u16,
    /// Whether audio is actively playing.
    pub playing: Arc<AtomicBool>,
    /// Current playhead frame (for sync with video).
    pub playhead_frame: Arc<AtomicI64>,
    /// Frames per second (timeline frame rate).
    pub frames_per_second: Arc<AtomicU64>,
}

impl AudioOutput {
    /// Create a new audio output with a ring buffer of `capacity_secs`.
    ///
    /// The ring buffer is sized at the device's actual sample rate so the
    /// cpal callback reads at the correct speed.  Callers must ensure that
    /// decoded PCM matches `self.sample_rate` and `self.channels`.
    pub fn new_with_capacity(capacity_secs: usize) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no default audio output device"))?;

        let default_config = device.default_output_config()
            .map_err(|e| anyhow::anyhow!("default output config error: {e}"))?;

        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();

        eprintln!(
            "[audio_output] device: {} | {}Hz {}ch | ring buffer: {}s",
            device.name().unwrap_or_default(),
            sample_rate,
            channels,
            capacity_secs,
        );

        // Ring buffer sized at device sample rate for correct playback speed
        let ring_capacity = (sample_rate as usize * capacity_secs).max(48000);
        let ring = Arc::new(AudioRingBuffer::new(ring_capacity, channels as usize));

        Self::build(sample_rate, channels, ring, device, default_config)
    }

    /// Create a new audio output with a 2-second ring buffer. (legacy)
    pub fn new() -> anyhow::Result<Self> {
        Self::new_with_capacity(2)
    }

    fn build(
        sample_rate: u32,
        channels: u16,
        ring: Arc<AudioRingBuffer>,
        device: cpal::Device,
        default_config: cpal::SupportedStreamConfig,
    ) -> anyhow::Result<Self> {
        let config: cpal::StreamConfig = default_config.into();

        let ring_clone = Arc::clone(&ring);
        let playing = Arc::new(AtomicBool::new(false));
        let playing_clone = Arc::clone(&playing);

        // Build the output stream
        let err_fn = |err| eprintln!("[audio_output] stream error: {err}");

        let stream = device.build_output_stream(
            &config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if playing_clone.load(Ordering::Acquire) {
                    ring_clone.read_samples(output, channels as usize);
                } else {
                    // Output silence
                    for s in output.iter_mut() {
                        *s = 0.0;
                    }
                }
            },
            err_fn,
            Some(Duration::from_secs(1)),
        )
        .map_err(|e| anyhow::anyhow!("build output stream: {e}"))?;

        stream.play().map_err(|e| anyhow::anyhow!("play stream: {e}"))?;

        Ok(Self {
            ring,
            stream: Some(stream),
            sample_rate,
            channels,
            playing,
            playhead_frame: Arc::new(AtomicI64::new(0)),
            frames_per_second: Arc::new(AtomicU64::new(30)),
        })
    }

    /// Start audio playback (unmute the stream).
    pub fn play(&self) {
        self.playing.store(true, Ordering::Release);
    }

    /// Pause audio playback (output silence).
    pub fn pause(&self) {
        self.playing.store(false, Ordering::Release);
    }

    /// Stop playback and clear the ring buffer.
    pub fn stop(&self) {
        self.playing.store(false, Ordering::Release);
        self.ring.clear();
    }

    /// Set the current playhead frame for sync.
    pub fn set_playhead(&self, frame: i64) {
        self.playhead_frame.store(frame, Ordering::Release);
    }

    /// Set the timeline frame rate for time calculations.
    pub fn set_fps(&self, fps: f64) {
        self.frames_per_second.store(fps as u64, Ordering::Release);
    }

    /// Get current playhead position in seconds (computed from audio frames consumed).
    pub fn current_time_secs(&self) -> f64 {
        self.ring.read_position() as f64 / self.sample_rate as f64
    }

    /// How many frames of audio are buffered.
    pub fn buffered_frames(&self) -> usize {
        self.ring.available()
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.playing.store(false, Ordering::Release);
        // Stream is dropped when self.stream is dropped
    }
}
