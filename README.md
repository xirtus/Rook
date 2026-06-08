# Rook

**A native video editor written in Rust.**

Rook is a lightweight, fast NLE built with egui for the UI and FFmpeg for decode. Runs entirely on the CPU with quarter-resolution compositing during playback for smooth scrubbing on Apple Silicon.

```
┌─────────────────────────────────────────────────────┐
│  Event Library │      Preview (viewer)      │ Insp. │
│   Media Browser│                            │       │
├────────────────┴────────────────────────────┴───────┤
│                     Timeline                        │
├─────────────────────────────────────────────────────┤
│              VU Strip (optional)                    │
└─────────────────────────────────────────────────────┘
```

## Download

[**Rook 0.1.0 — macOS arm64 (Apple Silicon)**](https://github.com/xirtus/Rook/releases/tag/v0.1.0)

Requires macOS 13 Ventura or later.

## Features

- **FCP-style three-column layout** — viewer is always unobscured by tools
- **FFmpeg decode** — H.264, HEVC, ProRes, and most common containers
- **CPU compositor** — quarter-res during playback, full-res on pause
- **Lock-free audio** — cpal output via ring buffer
- **Multi-track timeline** — blade, lift, overwrite edits
- **Inspector** — per-clip transform, opacity, speed
- **VU meter** — per-track + master level in a bottom strip (never reduces preview width)
- **Workspace presets** — Default, Audio, Logging, Color

## Build from source

Requires Rust 1.78+ and FFmpeg 6+ headers.

```bash
git clone https://github.com/xirtus/Rook
cd Rook
cargo build --release -p rook-ui
./target/release/rook-ui
```

### macOS app bundle

```bash
cargo build --release -p rook-ui
mkdir -p target/Rook.app/Contents/MacOS
cp target/release/rook-ui target/Rook.app/Contents/MacOS/Rook
codesign --force --deep --sign - target/Rook.app
open target/Rook.app
```

## Architecture

```
crates/
├── rook-core/     Data model: Project, Timeline, Track, Clip, Asset
├── rook-decode/   FFmpeg probe + decode pipeline
├── rook-engine/   Edit commands, undo/redo
├── rook-ipc/      JSON-RPC agent API
└── rook-ui/       egui desktop shell
    └── panels/
        ├── preview.rs     Video output + transport bar
        ├── timeline.rs    Multi-track timeline
        ├── inspector.rs   Clip properties
        ├── browser.rs     Media browser / event library
        └── vu_meter.rs    Audio level meters
```

## Changelog

See [quickupdate.html](quickupdate.html) for the full session-by-session log.

| Feature | Status |
|---------|--------|
| FFmpeg decode | ✅ |
| CPU compositor | ✅ |
| Quarter-res playback | ✅ |
| Audio (cpal) | ✅ |
| FCP three-column layout | ✅ |
| VU meter bottom strip | ✅ |
| Timeline editing | ✅ |
| Inspector | ✅ |
| Export | in progress |
| GPU compositor | planned |

## License

MIT OR Apache-2.0
