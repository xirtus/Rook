//! Transport layer — stdin/stdout, Unix socket, TCP.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Which transport is active.
#[derive(Debug, Clone)]
pub enum Transport {
    /// Subprocess stdio (editor spawns agent, or agent spawns editor).
    Stdio,
    /// Unix domain socket.
    UnixSocket { path: PathBuf },
    /// TCP listener.
    Tcp { addr: String, port: u16 },
}

impl Transport {
    pub fn name(&self) -> &str {
        match self {
            Self::Stdio => "stdio",
            Self::UnixSocket { .. } => "unix",
            Self::Tcp { .. } => "tcp",
        }
    }
}

/// Read a newline-delimited JSON frame from stdin.
pub fn read_stdio_frame() -> Option<String> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => None,      // EOF
        Ok(_) => Some(line.trim().to_string()),
        Err(_) => None,
    }
}

/// Write a newline-delimited JSON frame to stdout.
pub fn write_stdio_frame(frame: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", frame)?;
    stdout.flush()
}
