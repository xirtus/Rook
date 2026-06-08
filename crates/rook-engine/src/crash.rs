//! Crash reporter — panic hook + signal handlers.
//!
//! On initialisation the reporter installs:
//! * A `std::panic::set_hook` that writes a crash report *before* the default
//!   handler runs.
//! * (Unix only) Signal handlers for SIGSEGV, SIGABRT, SIGBUS that attempt
//!   to write a best-effort crash report.
//!
//! Crash reports are written to `~/.local/share/Rook/crashes/` with
//! timestamps.  On next launch, call [`has_pending_crash()`] to check
//! for unsent reports and [`recover_last_crash()`] to load the newest one.

use std::fs::{self, File};
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── Types ───────────────────────────────────────────────────────────────

/// A single crash report stored on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    /// UNIX epoch seconds when the crash occurred.
    pub timestamp_secs: u64,
    /// Human-readable timestamp.
    pub timestamp_iso: String,
    /// Panic message / signal description.
    pub message: String,
    /// Best-effort stack backtrace (may be empty for signal crashes).
    pub backtrace: String,
    /// OS thread name that crashed.
    pub thread: String,
    /// Platform string from `std::env::consts::OS`.
    pub platform: String,
    /// Rook version (from `CARGO_PKG_VERSION`).
    pub version: String,
}

/// Handle to the crash-reporting infrastructure.
/// Dropping this unregisters the hooks (useful for testing).
pub struct CrashReporter {
    _private: (),
}

// ── Global state ────────────────────────────────────────────────────────

static CRASH_DIR: OnceLock<PathBuf> = OnceLock::new();

fn crash_dir() -> &'static PathBuf {
    CRASH_DIR.get_or_init(|| {
        let base = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Rook")
            .join("crashes");
        let _ = fs::create_dir_all(&base);
        base
    })
}

// ── Write helpers ───────────────────────────────────────────────────────

fn write_crash_report(message: &str, backtrace: &str, thread: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let report = CrashReport {
        timestamp_secs: now.as_secs(),
        timestamp_iso: {
            // Simple ISO-like format
            let secs = now.as_secs();
            // approximate: just use secs for filename
            format!("{}", secs)
        },
        message: message.to_string(),
        backtrace: backtrace.to_string(),
        thread: thread.to_string(),
        platform: std::env::consts::OS.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    let dir = crash_dir();
    let filename = format!("crash-{}.json", report.timestamp_iso);
    let path = dir.join(&filename);

    if let Ok(mut f) = File::create(&path) {
        let json = serde_json::to_string_pretty(&report)
            .unwrap_or_else(|_| format!("{{ \"message\": {:?} }}", report.message));
        let _ = f.write_all(json.as_bytes());
        let _ = f.flush();
        eprintln!("Rook: crash report written to {}", path.display());
    }
}

// ── Panic hook ──────────────────────────────────────────────────────────

fn panic_hook(info: &PanicHookInfo) {
    let msg = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
        .unwrap_or("<non-string panic payload>");

    let bt = std::backtrace::Backtrace::force_capture();
    let bt_str = format!("{bt}");

    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_string();

    write_crash_report(msg, &bt_str, &thread);
}

// ── Signal handlers (Unix) ──────────────────────────────────────────────

#[cfg(unix)]
mod signals {
    use super::write_crash_report;
    use std::sync::atomic::{AtomicBool, Ordering};

    static SIGNAL_INSTALLED: AtomicBool = AtomicBool::new(false);

    /// Install signal handlers.  Safe to call multiple times (idempotent).
    pub(super) fn install() {
        if SIGNAL_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        // We must use unsafe extern "C" for signal handlers.
        unsafe {
            // SIGSEGV — segmentation fault
            libc::signal(libc::SIGSEGV, signal_handler as libc::sighandler_t);
            // SIGABRT — abort
            libc::signal(libc::SIGABRT, signal_handler as libc::sighandler_t);
            // SIGBUS — bus error
            libc::signal(libc::SIGBUS, signal_handler as libc::sighandler_t);
        }
    }

    unsafe extern "C" fn signal_handler(sig: libc::c_int) {
        let name = match sig {
            libc::SIGSEGV => "SIGSEGV (segmentation fault)",
            libc::SIGABRT => "SIGABRT (abort)",
            libc::SIGBUS => "SIGBUS (bus error)",
            _ => "unknown signal",
        };

        // Best-effort backtrace from this context
        let bt = std::backtrace::Backtrace::force_capture();
        let bt_str = format!("{bt}");

        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();

        write_crash_report(name, &bt_str, &thread);

        // Reset to default and re-raise so the OS can create a core dump
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }
}

#[cfg(not(unix))]
mod signals {
    pub(super) fn install() {
        // Signal handlers not supported on this platform.
        // Windows has its own crash reporting via WerRegisterFile.
    }
}

// ── Public API ──────────────────────────────────────────────────────────

impl CrashReporter {
    /// Install the panic hook and (on Unix) signal handlers.
    pub fn install() -> Self {
        // Make sure the crash directory exists
        let _ = crash_dir();

        // Install panic hook
        let _ = panic::take_hook(); // get the default
        panic::set_hook(Box::new(panic_hook));

        // Install signal handlers
        signals::install();

        CrashReporter { _private: () }
    }
}

impl Drop for CrashReporter {
    fn drop(&mut self) {
        // Restore default panic hook
        let _ = panic::take_hook();
    }
}

// ── Recovery ────────────────────────────────────────────────────────────

/// Returns true if there is at least one pending crash report on disk.
pub fn has_pending_crash() -> bool {
    let dir = crash_dir();
    if !dir.exists() {
        return false;
    }
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        })
        .unwrap_or(false)
}

/// Load the most recent crash report, if any.
pub fn recover_last_crash() -> Option<CrashReport> {
    let dir = crash_dir();
    if !dir.exists() {
        return None;
    }

    let mut reports: Vec<(u64, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("crash-") && name_str.ends_with(".json") {
                let path = entry.path();
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Ok(report) = serde_json::from_str::<CrashReport>(&data) {
                        reports.push((report.timestamp_secs, path));
                    }
                }
            }
        }
    }

    // Sort by timestamp descending, return newest
    reports.sort_by(|a, b| b.0.cmp(&a.0));
    reports.first().and_then(|(_, path)| {
        fs::read_to_string(path)
            .ok()
            .and_then(|data| serde_json::from_str::<CrashReport>(&data).ok())
    })
}

/// Delete all crash reports (call after user acknowledges recovery).
pub fn clear_crash_reports() {
    let dir = crash_dir();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("crash-") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_crash_on_clean_start() {
        // Should not fail when no crashes exist
        assert!(!has_pending_crash() || recover_last_crash().is_some());
    }
}
