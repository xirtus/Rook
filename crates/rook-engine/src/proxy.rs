//! Proxy pipeline — background ffmpeg transcode for smooth scrubbing.
//! Generates half-resolution ProRes Proxy files for video assets.

use parking_lot::Mutex;
use rook_core::AssetId;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ProxyStatus {
    None,
    Building { progress: f32 },
    Ready(PathBuf),
    Failed(String),
}

/// Active proxy build: the child process + progress reading.
struct ActiveBuild {
    child: Child,
    progress: f32,
    /// Stderr lines we've read so far (for progress parsing).
    stderr_lines: Vec<String>,
    /// Proxy output path.
    output_path: PathBuf,
    /// Partial stdout buffer for non-blocking line extraction.
    /// Bytes are appended by each tick() call and consumed when
    /// complete lines (ending with '\n') are found.
    stdout_buf: Vec<u8>,
}

pub struct ProxyService {
    statuses: Mutex<HashMap<AssetId, ProxyStatus>>,
    /// Active builds — checked on each tick.
    builds: Mutex<HashMap<AssetId, ActiveBuild>>,
    proxy_dir: PathBuf,
    /// Default proxy resolution: fraction of original (0.5 = half).
    proxy_scale: Mutex<f32>,
}

impl ProxyService {
    pub fn new() -> Self {
        let proxy_dir = dirs_next()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rook")
            .join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();
        Self {
            statuses: Mutex::new(HashMap::new()),
            builds: Mutex::new(HashMap::new()),
            proxy_dir,
            proxy_scale: Mutex::new(0.5),
        }
    }

    /// Set the proxy resolution scale (0.25 = quarter, 0.5 = half, 1.0 = full).
    pub fn set_proxy_scale(&self, scale: f32) {
        *self.proxy_scale.lock() = scale.clamp(0.125, 1.0);
    }

    /// Get the current proxy resolution scale.
    pub fn proxy_scale(&self) -> f32 {
        *self.proxy_scale.lock()
    }

    /// Request proxy generation for an asset. Spawns ffmpeg in the background.
    pub fn request_proxy(&self, asset_id: AssetId, source_path: &Path) {
        let output_path = self.proxy_dir.join(format!("proxy_{}.mov", asset_id.0));
        let scale = *self.proxy_scale.lock();

        // Build ffmpeg command: half-res ProRes Proxy, no audio
        let scale_filter = format!("scale=iw*{}:ih*{}", scale, scale);
        let cmd = Command::new("ffmpeg")
            .args([
                "-y",
                "-i",
                source_path.to_str().unwrap_or(""),
                "-vf",
                &scale_filter,
                "-c:v",
                "prores_ks",
                "-profile:v",
                "0",   // Proxy profile
                "-an", // no audio
                "-progress",
                "pipe:1", // machine-readable progress on stdout
                "-nostats",
                output_path.to_str().unwrap_or(""),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        match cmd {
            Ok(mut child) => {
                // Set stdout to non-blocking so tick() never blocks the UI thread.
                if let Some(ref stdout) = child.stdout {
                    let fd = stdout.as_raw_fd();
                    unsafe {
                        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
                        if flags >= 0 {
                            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                        }
                    }
                }
                self.builds.lock().insert(
                    asset_id,
                    ActiveBuild {
                        child,
                        progress: 0.0,
                        stderr_lines: Vec::new(),
                        output_path: output_path.clone(),
                        stdout_buf: Vec::new(),
                    },
                );
                self.statuses
                    .lock()
                    .insert(asset_id, ProxyStatus::Building { progress: 0.0 });
                tracing::info!(?asset_id, src = %source_path.display(), "proxy build started");
            }
            Err(e) => {
                self.statuses.lock().insert(
                    asset_id,
                    ProxyStatus::Failed(format!("ffmpeg spawn: {}", e)),
                );
                tracing::error!(?asset_id, err = %e, "proxy build failed to spawn");
            }
        }
    }

    /// Poll active builds — call periodically from the main thread.
    /// Updates statuses map as builds complete or make progress.
    ///
    /// **Never blocks** — stdout reads are non-blocking per-file-descriptor
    /// flags set in `request_proxy()`.  Partial output is buffered per-build
    /// and lines are extracted incrementally.
    pub fn tick(&self) {
        let mut builds = self.builds.lock();
        let mut statuses = self.statuses.lock();
        let mut finished: Vec<(AssetId, ProxyStatus)> = Vec::new();

        for (asset_id, build) in builds.iter_mut() {
            // ── Non-blocking stdout read ──────────────────────────────
            if let Some(ref mut stdout) = build.child.stdout {
                let mut chunk = [0u8; 4096];
                loop {
                    match stdout.read(&mut chunk) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            build.stdout_buf.extend_from_slice(&chunk[..n]);
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            break; // No more data available
                        }
                        Err(_) => {
                            // Pipe broken — stop reading
                            break;
                        }
                    }
                }
            }

            // ── Extract complete lines from buffer ───────────────────
            while let Some(newline_pos) = build.stdout_buf.iter().position(|&b| b == b'\n') {
                let line_bytes = build.stdout_buf.drain(..=newline_pos).collect::<Vec<u8>>();
                // Drop the trailing newline
                let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len().saturating_sub(1)])
                    .to_string();

                if line.starts_with("progress=") {
                    let val = line.trim_start_matches("progress=").trim();
                    if val == "end" {
                        build.progress = 1.0;
                    }
                    // "continue" lines are informational — ignore
                }
            }

            // ── Check if process exited ──────────────────────────────
            match build.child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        finished.push((*asset_id, ProxyStatus::Ready(build.output_path.clone())));
                        tracing::info!(?asset_id, path = %build.output_path.display(), "proxy build complete");
                    } else {
                        finished.push((
                            *asset_id,
                            ProxyStatus::Failed(format!("ffmpeg exit code: {:?}", status.code())),
                        ));
                        tracing::error!(?asset_id, code = ?status.code(), "proxy build failed");
                    }
                }
                Ok(None) => {
                    // Still running — update progress
                    statuses.insert(
                        *asset_id,
                        ProxyStatus::Building {
                            progress: build.progress,
                        },
                    );
                }
                Err(e) => {
                    finished.push((*asset_id, ProxyStatus::Failed(format!("wait error: {}", e))));
                }
            }
        }

        // Move finished builds to statuses
        for (asset_id, status) in finished {
            builds.remove(&asset_id);
            statuses.insert(asset_id, status);
        }
    }

    pub fn status(&self, asset_id: AssetId) -> Option<ProxyStatus> {
        self.statuses.lock().get(&asset_id).cloned()
    }

    pub fn proxy_dir(&self) -> &Path {
        &self.proxy_dir
    }
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
