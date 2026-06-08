//! IPC server — listens for agent requests, dispatches, responds.
//! Architecture adapted from Anica's `transport_acp.rs`.

use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use rook_engine::Engine;

use crate::methods;
use crate::protocol::{Request, Response};
use crate::transport::Transport;
use crate::types::EditorEvent;

/// The IPC server runs in its own thread, reading requests from the
/// transport and dispatching them against the engine state.
pub struct IpcServer {
    engine: Arc<Mutex<Engine>>,
    tx_out: Sender<String>,
    rx_in: Receiver<String>,
    transport: Transport,
    connected: bool,
    /// Subscribed agents get async events (playhead moved, etc.).
    subscribers: Vec<Sender<EditorEvent>>,
}

impl IpcServer {
    /// Start in stdio mode — reads stdin, writes stdout.
    pub fn start_stdio(engine: Arc<Mutex<Engine>>) -> Self {
        let (tx_out, rx_out) = crossbeam_channel::unbounded::<String>();
        let (tx_in, rx_in) = crossbeam_channel::unbounded::<String>();

        // Reader thread: stdin → rx_in
        std::thread::spawn(move || {
            loop {
                match crate::transport::read_stdio_frame() {
                    Some(line) if !line.is_empty() => {
                        if tx_in.send(line).is_err() { break; }
                    }
                    None => break,
                    _ => {}
                }
            }
        });

        // Writer thread: rx_out → stdout
        std::thread::spawn(move || {
            loop {
                match rx_out.recv() {
                    Ok(msg) => {
                        if crate::transport::write_stdio_frame(&msg).is_err() { break; }
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            engine,
            tx_out,
            rx_in,
            transport: Transport::Stdio,
            connected: true,
            subscribers: Vec::new(),
        }
    }

    /// Create an IPC server wrapping an existing engine (no streaming transport).
    /// Use `execute()` for direct method calls.
    pub fn new(engine: Arc<Mutex<Engine>>) -> Self {
        let (tx_out, _rx_out) = crossbeam_channel::unbounded::<String>();
        let (_tx_in, rx_in) = crossbeam_channel::unbounded::<String>();
        Self {
            engine,
            tx_out,
            rx_in,
            transport: Transport::Stdio,
            connected: false,
            subscribers: Vec::new(),
        }
    }

    /// Poll for incoming messages. Call each frame from the egui update loop.
    pub fn poll(&mut self) {
        loop {
            match self.rx_in.try_recv() {
                Ok(raw) => {
                    let response = self.handle_message(&raw);
                    if let Some(resp) = response {
                        let json = serde_json::to_string(&resp).unwrap_or_default();
                        let _ = self.tx_out.send(json);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.connected = false;
                    break;
                }
            }
        }
    }

    /// Execute a method call directly (for programmatic use, not streaming).
    pub fn execute(&self, method: &str, params: Option<serde_json::Value>) -> Response {
        let engine = self.engine.lock().unwrap();
        methods::dispatch(None, method, params, &engine)
    }

    fn handle_message(&self, raw: &str) -> Option<Response> {
        let req: Request = match serde_json::from_str(raw) {
            Ok(r) => r,
            Err(e) => {
                return Some(Response::error(None, -32700, format!("Parse error: {e}")));
            }
        };

        // Notifications don't get responses
        let id = req.id?;

        let engine = self.engine.lock().unwrap();
        Some(methods::dispatch(Some(id), &req.method, req.params, &engine))
    }

    /// Broadcast an event to all subscribers.
    pub fn emit(&self, event: EditorEvent) {
        for sub in &self.subscribers {
            let _ = sub.send(event.clone());
        }
    }

    /// Subscribe to editor events.
    pub fn subscribe(&mut self) -> Receiver<EditorEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.subscribers.push(tx);
        rx
    }

    pub fn is_connected(&self) -> bool { self.connected }
    pub fn transport(&self) -> &Transport { &self.transport }
}
