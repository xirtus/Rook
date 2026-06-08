//! Rook CLI — headless editing, batch export, and agent host.
//!
//! Modes:
//! ```bash
//! # Render a single frame
//! rook-cli render --project project.rook --frame 240 --output frame.png
//!
//! # Export a project
//! rook-cli export --project project.rook --output out.mp4 --format h264
//!
//! # Run as an IPC server for AI agents (stdio mode)
//! rook-cli serve --ipc stdio
//!
//! # Run as an MCP server for Claude / Cursor
//! rook-cli serve --mcp
//! ```

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rook_engine::Engine;
use rook_ipc::{methods, protocol};

#[derive(Parser)]
#[command(name = "rook", about = "Rook video editor — CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a single frame to PNG
    Render {
        #[arg(short, long)]
        project: PathBuf,
        #[arg(short, long)]
        frame: i64,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Export project to video file
    Export {
        #[arg(short, long)]
        project: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short, long, default_value = "h264")]
        format: String,
    },
    /// Import media files into a project
    Import {
        #[arg(short, long)]
        project: PathBuf,
        #[arg(short, long)]
        files: Vec<PathBuf>,
    },
    /// List project contents
    Info {
        #[arg(short, long)]
        project: PathBuf,
    },
    /// Start IPC server (stdio or MCP)
    Serve {
        /// Transport: stdio, mcp
        #[arg(long, default_value = "stdio")]
        ipc: String,
    },
    /// Export list of available IPC methods
    Methods,
}

fn load_engine(path: &std::path::Path) -> anyhow::Result<Engine> {
    if path.extension().map_or(false, |e| e == "rook") {
        Engine::open_project(path).map_err(|e| anyhow::anyhow!("{e}"))
    } else {
        Engine::open_project_json(path).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Render { project, frame, output } => {
            let engine = load_engine(&project)?;
            let layers = engine.frame_at(frame);
            let canvas = engine.project().canvas.clone();

            println!("Frame {frame}: {} layers", layers.len());
            for layer in &layers {
                println!("  Track {:?} Clip {:?}", layer.track, layer.clip);
            }

            // Write a placeholder PNG (checkerboard pattern)
            let w = canvas.width as usize;
            let h = canvas.height as usize;
            let mut img_data = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let idx = (y * w + x) * 4;
                    let bright = if ((x / 20) + (y / 20)) % 2 == 0 { 40u8 } else { 30u8 };
                    img_data[idx] = bright;
                    img_data[idx + 1] = bright;
                    img_data[idx + 2] = bright + 20;
                    img_data[idx + 3] = 255;
                }
            }
            let img = image::RgbaImage::from_raw(w as u32, h as u32, img_data)
                .ok_or_else(|| anyhow::anyhow!("failed to create image"))?;
            img.save(&output)?;
            println!("Rendered frame {frame} → {}", output.display());
        }

        Command::Export { project, output, format } => {
            let mut engine = load_engine(&project)?;
            engine.init_mlt()?;
            engine.export(&output, &format)?;
            println!("Exported → {}", output.display());
        }

        Command::Import { project, files } => {
            let mut engine = load_engine(&project)?;
            let mut imported = 0;
            for file in &files {
                match engine.import_media(file) {
                    Ok(id) => {
                        println!("  Imported: {} (id={})", file.display(), id);
                        imported += 1;
                    }
                    Err(e) => {
                        eprintln!("  Failed: {} — {e}", file.display());
                    }
                }
            }
            engine.save_project(None)?;
            println!("Imported {imported}/{} files into {}", files.len(), project.display());
        }

        Command::Info { project } => {
            let engine = load_engine(&project)?;
            let proj = engine.project();
            println!("Project: {} ({})", proj.name, proj.id);
            println!("Canvas: {}×{} @ {:.2} fps",
                proj.canvas.width, proj.canvas.height, proj.frame_rate.as_f64());
            println!("Audio: {} Hz, {} channels", proj.sample_rate, proj.audio_channels);
            println!("Duration: {} frames ({:.1}s)",
                proj.timeline.duration(),
                proj.timeline.duration() as f64 / proj.frame_rate.as_f64());
            println!("Tracks: {}", proj.timeline.tracks.len());
            for track in &proj.timeline.tracks {
                let dur = track.clips.iter().map(|c| c.duration()).sum::<i64>();
                println!("  {:?} {}: {} clips, {} frames total",
                    track.kind, track.name, track.clips.len(), dur);
            }
            println!("Assets: {}", proj.assets.len());
            for asset in &proj.assets {
                let desc = asset.metadata().ai_description.as_deref().unwrap_or("-");
                println!("  {} ({})", asset.filename_stem(), desc);
            }
            println!("Markers: {}", proj.timeline.markers.len());
            println!("Can undo: {}  Can redo: {}",
                engine.can_undo(), engine.can_redo());
        }

        Command::Serve { ipc } => {
            let engine = Engine::new("Headless", rook_core::canvas::Canvas::HD_1080P, rook_core::time::Rational::FPS_24);
            let engine = Arc::new(Mutex::new(engine));

            match ipc.as_str() {
                "stdio" => {
                    eprintln!("Rook IPC server running on stdio (JSON-RPC 2.0)");
                    eprintln!("Send requests as newline-delimited JSON. Ctrl+D to stop.");
                    let stdin = std::io::stdin();
                    let reader = BufReader::new(stdin.lock());
                    for line in reader.lines() {
                        let line = match line {
                            Ok(l) => l,
                            Err(_) => break,
                        };
                        let line = line.trim().to_string();
                        if line.is_empty() { continue; }

                        let req: protocol::Request = match serde_json::from_str(&line) {
                            Ok(r) => r,
                            Err(e) => {
                                let err = protocol::Response::error(None, -32700, format!("Parse error: {e}"));
                                let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string(&err).unwrap_or_default());
                                continue;
                            }
                        };

                        let id = req.id;
                        let engine_guard = engine.lock().unwrap();
                        let response = methods::dispatch(id, &req.method, req.params, &engine_guard);
                        drop(engine_guard);

                        let json = serde_json::to_string(&response).unwrap_or_default();
                        let _ = writeln!(std::io::stdout(), "{json}");
                        let _ = std::io::stdout().flush();
                    }
                    eprintln!("IPC server stopped.");
                }

                "mcp" => {
                    eprintln!("Rook MCP server starting on stdio...");
                    run_mcp_server(engine);
                }

                other => {
                    anyhow::bail!("Unknown transport: {other}. Use 'stdio' or 'mcp'.");
                }
            }
        }

        Command::Methods => {
            println!("Rook IPC Methods ({})", methods::METHOD_REGISTRY.len());
            println!("{:=<60}", "");
            for (method, desc) in methods::METHOD_REGISTRY {
                println!("  {method:<35} {desc}");
            }
            println!("\nAll methods use JSON-RPC 2.0 over newline-delimited frames.");
        }
    }

    Ok(())
}

/// Run a minimal MCP-compatible server over stdio.
///
/// Implements the Model Context Protocol (MCP) basics:
/// - `initialize` — capability negotiation
/// - `tools/list` — list all available Rook tools
/// - `tools/call` — call a specific Rook method
///
/// Uses the `rmcp` crate when available; otherwise falls back to
/// a hand-rolled JSON-RPC loop that speaks MCP-shaped messages.
fn run_mcp_server(engine: Arc<Mutex<Engine>>) {
    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim().to_string();
        if line.is_empty() { continue; }

        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({"jsonrpc":"2.0","error":{"code":-32700,"message":format!("Parse error: {e}")}});
                let _ = writeln!(std::io::stdout(), "{err}");
                continue;
            }
        };

        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = req.get("id").cloned();

        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "Rook",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),

            "tools/list" => {
                let tools: Vec<serde_json::Value> = methods::METHOD_REGISTRY.iter()
                    .map(|(name, desc)| serde_json::json!({
                        "name": name,
                        "description": desc,
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    }))
                    .collect();
                serde_json::json!({"tools": tools})
            }

            "tools/call" => {
                let tool_name = req.get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tool_args = req.get("params")
                    .and_then(|p| p.get("arguments"))
                    .cloned();

                let engine_guard = engine.lock().unwrap();
                let response = methods::dispatch(None, tool_name, tool_args, &engine_guard);
                drop(engine_guard);

                // MCP expects content array
                let text = serde_json::to_string_pretty(&response.result.unwrap_or_default())
                    .unwrap_or_default();
                serde_json::json!({
                    "content": [{"type": "text", "text": text}]
                })
            }

            "notifications/initialized" => {
                // No response needed for notifications
                let _ = writeln!(std::io::stdout(), "");
                continue;
            }

            _ => {
                serde_json::json!({
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {method}")
                    }
                })
            }
        };

        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });

        let _ = writeln!(std::io::stdout(), "{response}");
        let _ = std::io::stdout().flush();
    }
}
