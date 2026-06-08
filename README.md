# Rook

**A proxy-based video editor with AI agent interoperability.**

Rook is a Rust NLE that wraps MLT for its media engine, uses egui for its UI, and exposes a JSON-RPC / MCP API so AI agents can inspect, edit, and export projects programmatically.

```
┌──────────────────────────────────────────────────────┐
│  egui UI ──► rook-engine ──► rook-mlt (MLT C FFI)   │
│                        │                             │
│                   rook-ipc (JSON-RPC / MCP)          │
│                        │                             │
│              AI Agents (Claude, GPT, custom)         │
└──────────────────────────────────────────────────────┘
```

## Status

**Phase 1 — vendored Gausian crates.** 7 crates (10,400 LOC) vendored from
Gausian Native Editor (Apache 2.0). GPU renderer, FCPXML/EDL export, HW decode,
SQLite persistence, and graph-based timeline model are now integrated.

| Category | Crate | LOC | Status |
|----------|-------|-----|--------|
| **Rook native** | `rook-core` | 1,783 | ✅ Data model (cutlass + verbreel + anica) |
| | `rook-mlt` | 647 | ⚠️ MLT bindings stubbed |
| | `rook-engine` | 624 | ⚠️ Engine with edit commands |
| | `rook-decode` | 190 | ⚠️ FFmpeg probe (stub) |
| | `rook-ipc` | 599 | ✅ 30-method agent API |
| | `rook-ui` | 583 | ⚠️ egui shell, 4 panels |
| | `rook-cli` | 140 | ⚠️ CLI skeleton |
| **Vendored** | `rook-timeline` | 920 | ✅ Graph timeline model |
| (Gausian) | `rook-exporters` | 1,324 | ✅ FCPXML 1.9/1.10 + EDL + JSON |
| | `rook-renderer` | 2,425 | ✅ wgpu + 7 WGSL shaders |
| | `rook-decoder-native` | 4,719 | ✅ VideoToolbox + GStreamer |
| | `rook-project-db` | 1,016 | ✅ SQLite persistence |
| | `rook-media-io` | 665 | ✅ Media I/O helpers |
| | `rook-jobs` | 355 | ✅ Job queue |
| **Total** | **14 crates** | **15,970** | |

## Architecture

```
crates/
├── rook-core/           Data model: Project, Timeline, Track, Clip, Asset,
│                        EditCommand, EditHistory, Keyframe, Effect, Transform.
│                        Zero external deps beyond serde + uuid.
│
├── rook-timeline/  (V)  Graph-based timeline: TimelineGraph, ClipNode,
│                        TransitionNode, TimelineCommand, automation lanes.
│
├── rook-mlt/            Safe Rust bindings for MLT 7.x: Producer, Consumer,
│                        Playlist, Tractor, Filter, Transition, Frame.
│
├── rook-decode/         FFmpeg probe (stub). Real decode lives in:
├── rook-decoder-native/ (V) HW-accelerated: VideoToolbox (macOS) + GStreamer.
│                        Native wgpu texture integration.
│
├── rook-engine/         Headless editor: apply EditCommand → validates
│                        → records undo → mutates Project → mirrors to MLT.
│
├── rook-renderer/  (V)  wgpu renderer with 7 WGSL shaders: NV12/P010/RGBA
│                        preview, YUV→RGB, scale, blend, transform.
│
├── rook-project-db/ (V) SQLite persistence: WAL mode, asset CRUD, proxy
│                        tracking, job queue, migrations.
│
├── rook-exporters/ (V)  Professional interchange: FCPXML 1.9/1.10 round-trip,
│                        FCP7 XML, EDL round-trip, JSON. Timecode conversion.
│
├── rook-ipc/            Agent protocol: JSON-RPC 2.0 over stdio/socket/TCP.
│                        30 API methods. Editor→agent event stream.
│
├── rook-ui/             egui desktop shell: Gallery, Timeline, Preview,
│                        Inspector panels. Menu bar, export dialog.
│
└── rook-cli/            CLI: render, export, import, info, serve (IPC/MCP).

(V) = vendored from Gausian Native Editor (Apache 2.0)
```

## Quickstart

```bash
# Clone
git clone https://github.com/rook/rook
cd rook

# Build everything (stub mode — no MLT needed)
cargo build --workspace

# Run the desktop editor
cargo run -p rook-ui

# List available IPC methods
cargo run -p rook-cli -- methods

# Run IPC server in stdio mode
cargo run -p rook-cli -- serve --ipc stdio

# Run tests
cargo test --workspace
```

### With real MLT (future)

```bash
brew install mlt         # macOS
cargo build --workspace --features system-mlt
```

## Agent API

Connect any AI agent via stdio, Unix socket, or TCP. Example:

```bash
# Spawn Rook in headless IPC mode
rook-cli serve --ipc stdio

# Agent sends JSON-RPC requests on stdin, reads responses on stdout
echo '{"jsonrpc":"2.0","id":1,"method":"timeline.get","params":{}}' | rook-cli serve --ipc stdio
```

Full API reference: [`docs/IPC.md`](docs/IPC.md)

## Provenance

Rook's data model is a deliberate merge of three vetted upstreams:

| From | What | License |
|------|------|---------|
| **cutlass** | Project/Timeline/Clip model, EditCommand, EditHistory, ProxyService, FrameCache | MIT / Apache 2.0 |
| **verbreel-engine** | Canvas, Effect, Keyframe, BlendMode, MaskKind, FadeCurve, Asset tagged union | MIT / Apache 2.0 |
| **anica** | TimelineSnapshot API shapes, SemanticClip, AI metadata fields, ACP transport pattern | Apache 2.0 |

See [`docs/SAVED_LOC.md`](docs/SAVED_LOC.md) for the LOC savings analysis.

## License

MIT OR Apache-2.0, at your option.
