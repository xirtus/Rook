# Rook — Feature Parity Spec (FCP + Shotcut)

> Target: match Final Cut Pro's editing model + Shotcut's flexibility.
> Format: checkbox list for an agent to implement in priority order.

---

## 1. TOOLBAR & TOOLS

### 1.1 Tool Palette (FCP-style tool strip)
- [ ] **Select (A)** — default arrow tool, click to select clips, drag to move
- [ ] **Blade (B)** — split clip at playhead or click point
- [ ] **Trim (T)** — trim edge without entering trim mode
- [ ] **Range Select (R)** — drag to select a time range across tracks
- [ ] **Zoom (Z)** — click to zoom in, Option+click to zoom out
- [ ] **Hand (H)** — pan timeline by dragging
- [ ] **Position (P)** — move clip within frame (transform mode)

### 1.2 Toolbar Buttons (top bar, left-to-right)
- [ ] **New Project** — create from template or blank
- [ ] **Open Library / Project** — file browser
- [ ] **Save** — ⌘S
- [ ] **Undo / Redo** — ⌘Z / ⇧⌘Z with history dropdown
- [ ] **Import Media** — ⌘I, opens file dialog
- [ ] **Export / Share** — ⌘E, opens export panel
- [ ] **Snapping Toggle** — N key, visual indicator in toolbar
- [ ] **Magnetic Timeline Toggle** — (FCP-specific, P2)
- [ ] **Solo / Mute selected** — audio monitoring
- [ ] **Playback controls** — ⏮ ⏵ ⏸ ⏭ (already done)
- [ ] **Loop playback** — toggle
- [ ] **Fullscreen preview** — ⌘⇧F

---

## 2. TIMELINE EDITING OPERATIONS

### 2.1 Core Edits
- [ ] **Insert Clip** — place at playhead, ripple existing clips right
- [ ] **Overwrite Clip** — place at playhead, overwrite what's there
- [ ] **Append Clip** — place at end of timeline
- [ ] **Connect Clip** — place as connected clip above primary storyline
- [ ] **Lift** — remove clip, leave gap (⌘⌫ or Delete)
- [ ] **Ripple Delete** — remove clip, close gap (⇧⌫)
- [ ] **Replace** — replace clip keeping duration
- [ ] **Replace from Start** — replace keeping start frame
- [ ] **Overwrite to Primary Storyline** — move connected clip down

### 2.2 Markers & Selection
- [ ] **Set In Point (I)** — mark selection start on clip or timeline
- [ ] **Set Out Point (O)** — mark selection end
- [ ] **Clear In/Out (⌥I / ⌥O)** — remove marks
- [ ] **Select Between In/Out** — selects range
- [ ] **Markers (M)** — place named marker at playhead
- [ ] **Marker List** — panel showing all markers with names + timecode
- [ ] **To-Do Markers (⇧M)** — task markers
- [ ] **Chapter Markers** — for DVD/Blu-ray export

### 2.3 Trimming
- [ ] **Trim Start** — drag left edge (already done)
- [ ] **Trim End** — drag right edge (already done)
- [ ] **Ripple Trim** — trim + shift adjacent clips
- [ ] **Roll Trim** — move edit point, one clip shortens as other lengthens
- [ ] **Slip** — change in/out of clip without moving it on timeline
- [ ] **Slide** — move clip, trimming adjacent clips to compensate
- [ ] **Trim Edit Window** — precision trim panel with JKL control
- [ ] **Trim to Selection** — ⌥\

### 2.4 Blade / Split
- [ ] **Blade at Playhead** — ⌘B or S (already done)
- [ ] **Blade All Tracks** — ⇧⌘B, split every track at playhead
- [ ] **Blade at Skimmer** — click with blade tool active

### 2.5 Compound & Nesting
- [ ] **New Compound Clip** — ⌥G, group selected clips into a compound
- [ ] **Break Apart Compound** — ⇧⌥G
- [ ] **Nested Sequences** — reference another project/sequence as a clip
- [ ] **Open in Timeline** — double-click compound to edit contents

---

## 3. TRACK MANAGEMENT

- [ ] **Add Video Track** — ⌘⇧V (already partial)
- [ ] **Add Audio Track** — ⌘⇧A
- [ ] **Add Title Track** — for text overlays
- [ ] **Add Subtitle Track** — SRT import/export
- [ ] **Track Height** — 4 sizes: mini, small, medium, large
- [ ] **Track Headers** — name, mute, solo, lock, visibility (✓ mute/lock done)
- [ ] **Disable Track** — exclude from export
- [ ] **Track Color Labels** — assign colors to tracks
- [ ] **Track Focus** — solo track for trimming
- [ ] **Reorder Tracks** — drag track header to reorder
- [ ] **Primary Storyline** — FCP's magnetic track concept (P2)

---

## 4. CLIP PROPERTIES & INSPECTOR

### 4.1 Video Inspector
- [ ] **Transform** — position X/Y, scale X/Y, rotation (°), anchor point
- [ ] **Crop** — left, right, top, bottom (pixels or %)
- [ ] **Distort** — corner pin (4-point warp, P2)
- [ ] **Opacity** — 0–100% slider
- [ ] **Blend Mode** — dropdown with 17 modes (✓ types defined in compositor)
- [ ] **Speed** — constant speed %, reverse checkbox
- [ ] **Speed Ramp** — keyframe-able speed curve
- [ ] **Stabilization** — (P2, requires analysis)
- [ ] **Spatial Conform** — fit/fill/none for mismatched aspect ratios

### 4.2 Audio Inspector
- [ ] **Volume** — dB slider
- [ ] **Pan** — L/R slider
- [ ] **Audio Enhancements** — EQ, compressor, limiter (P2)
- [ ] **Fade In / Fade Out** — duration in seconds
- [ ] **Audio Channels** — stereo/mono config
- [ ] **Noise Removal** — (P2)

### 4.3 Effects Inspector
- [ ] **Effects Stack** — ordered list with enable/disable toggle
- [ ] **Add Effect** — searchable dropdown
- [ ] **Effect Parameters** — per-effect property controls
- [ ] **Keyframe Editor** — diamond buttons, curve editor
- [ ] **Reset Parameter** — reset to default
- [ ] **Copy/Paste Effects** — copy effects from one clip to another
- [ ] **Save Effect Preset** — save + name effect chains

### 4.4 Info Inspector
- [ ] **Clip Name** — editable
- [ ] **Source Media** — path, codec, resolution, fps, duration
- [ ] **Timeline Position** — in frame, duration, out frame
- [ ] **Notes** — free-text notes field
- [ ] **Tags / Keywords** — user tags
- [ ] **Roles** — video/audio/titles/dialogue/music/effects

---

## 5. EFFECTS LIBRARY

### 5.1 Video Effects (priority: high)
- [ ] **Blur** — gaussian, directional, zoom (✓ gaussian shader done)
- [ ] **Sharpen** — unsharp mask
- [ ] **Color Board** — exposure, contrast, saturation, tint, temperature
- [ ] **Color Curves** — RGB curves editor
- [ ] **Color Wheels** — shadows/midtones/highlights
- [ ] **LUT** — load .cube LUT files
- [ ] **Chroma Key** — green/blue screen keyer with spill suppression
- [ ] **Luma Key** — key on brightness
- [ ] **Mask** — rectangle, ellipse, free-draw, feather, invert (✓ types done)
- [ ] **Transform** — position, scale, rotation, crop (per-effect version)
- [ ] **Distort** — perspective, lens correction
- [ ] **Noise / Grain** — film grain simulation
- [ ] **Glow** — bloom effect
- [ ] **Vignette** — darken edges
- [ ] **Text** — basic title generator (✓ in compositor types)
- [ ] **Timecode** — burn-in timecode overlay

### 5.2 Audio Effects (priority: medium)
- [ ] **EQ** — 10-band parametric
- [ ] **Compressor** — threshold, ratio, attack, release
- [ ] **Limiter** — ceiling, release
- [ ] **Reverb** — room size, damping
- [ ] **Delay / Echo** — time, feedback, mix
- [ ] **Noise Gate** — threshold, reduction
- [ ] **Pitch** — semitone shift, formant preservation
- [ ] **DeEsser** — frequency, threshold
- [ ] **Gain** — simple volume adjust

### 5.3 Transitions
- [ ] **Cross Dissolve** — default fade transition
- [ ] **Fade to Color** — fade to/from black/white
- [ ] **Wipe** — directional wipe (left, right, up, down)
- [ ] **Slide** — push slide
- [ ] **Zoom** — zoom in/out transition
- [ ] **Flash** — white flash transition
- [ ] **Transition Duration** — editable in timeline (drag edge)
- [ ] **Transition Inspector** — parameters per transition type

---

## 6. VIEWER / CANVAS

### 6.1 Preview Controls
- [ ] **Play/Pause** — Space (✓ done)
- [ ] **JKL Shuttle** — J=reverse, K=stop, L=forward (✓ done)
- [ ] **Frame Step** — ← → arrow keys (✓ done)
- [ ] **Go to Start** — Home, End (✓ done)
- [ ] **Loop Playback** — ⌘L
- [ ] **Play Around** — play a few seconds around playhead
- [ ] **Play from Start** — play from project beginning
- [ ] **Play Selected** — / key, play only selected clip

### 6.2 Viewing Options
- [ ] **Zoom to Fit** — ⇧Z
- [ ] **100% (Actual Size)** — ⌘0
- [ ] **Quality** — better performance / better quality toggle
- [ ] **Show Title Safe** — action/title safe guides
- [ ] **Show Horizon** — rule of thirds grid
- [ ] **Show Overlays** — timecode, clip name on canvas
- [ ] **Background** — checkerboard / black / white
- [ ] **Show Rulers** — pixel rulers on canvas edges
- [ ] **Canvas Size** — scrub percentage slider

### 6.3 Transform Controls (on-canvas)
- [ ] **Drag to Reposition** — directly on canvas
- [ ] **Corner Handles** — scale with corner drag
- [ ] **Rotation Handle** — rotate with handle drag
- [ ] **Crop Handles** — crop edges with handles
- [ ] **Anchor Point** — drag anchor crosshair
- [ ] **Snap to Guides** — magnetic alignment
- [ ] **Nudge** — arrow keys for 1px moves

---

## 7. TIMELINE RULER & NAVIGATION

- [ ] **Timecode Ruler** — HH:MM:SS:FF at top (✓ timecode format done)
- [ ] **Ruler Markings** — frame ticks, second ticks, labels at zoom thresholds
- [ ] **Snap Indicators** — yellow lines when snapping (✓ done)
- [ ] **Zoom Slider** — at bottom of timeline (✓ done)
- [ ] **Zoom to Fit** — ⇧Z, fit entire project
- [ ] **Zoom to Selection** — ⌥⇧Z
- [ ] **Scroll to Playhead** — F key
- [ ] **Timeline Index** — colored bar showing which part you're looking at
- [ ] **Duration Display** — total project duration in toolbar

---

## 8. AUDIO

### 8.1 Waveforms
- [ ] **Audio Waveforms on Clips** — visual waveform inside clip blocks
- [ ] **Waveform Zoom** — scale with clip height
- [ ] **Waveform Color** — configurable (green/blue/white)
- [ ] **Regenerate Waveforms** — recompute from source

### 8.2 Audio Meters
- [ ] **VU Meter** — per-track or master, peak + RMS
- [ ] **Peak Indicator** — red clip indicator, click to reset
- [ ] **dB Scale** — -60 to 0 dB markings

### 8.3 Audio Editing
- [ ] **Fade Handles** — drag from clip edge to add fade in/out
- [ ] **Crossfade** — overlap two audio clips for crossfade
- [ ] **Audio Gain Line** — horizontal line, drag up/down for volume
- [ ] **Keyframe Points** — ⌥+click on gain line to add points
- [ ] **Normalize Audio** — set peak to -6 dB or -12 dB
- [ ] **Solo Track** — mute all other tracks (S key)
- [ ] **Detach Audio** — separate audio from video clip
- [ ] **Sync Audio** — sync external audio to video waveform

---

## 9. IMPORT & MEDIA MANAGEMENT

### 9.1 Import
- [ ] **Import Media** — ⌘I, file dialog with filters (✓ done)
- [ ] **Import Folder** — batch import folder contents
- [ ] **Drag & Drop from Finder** — drag files directly onto timeline or media bin
- [ ] **Import from Camera** — capture from connected device (P2)
- [ ] **Import iMovie Project** — import .imovieproject (P2)
- [ ] **Import FCPXML** — import Final Cut Pro XML (✓ format support in exporters)
- [ ] **Import EDL** — import CMX 3600 EDL
- [ ] **Import AAF** — Advanced Authoring Format

### 9.2 Media Browser
- [ ] **List View** — filename, duration, resolution, fps, codec
- [ ] **Icon View** — thumbnail grid (✓ partial)
- [ ] **Filmstrip View** — hover to scrub through clip
- [ ] **Sort** — by name, date, duration, kind, tags
- [ ] **Search / Filter** — text search across all metadata
- [ ] **Favorite / Reject** — F = favorite, ⌫ = reject
- [ ] **Keyword Tags** — assign tags to clips
- [ ] **Smart Collections** — saved search filters
- [ ] **Used / Unused** — show only clips on timeline
- [ ] **Relink Media** — reconnect moved/renamed source files

### 9.3 Proxies
- [ ] **Generate Proxy** — create low-res proxy for selected (✓ proxy service exists)
- [ ] **Proxy Preference** — use original / proxy / proxy only
- [ ] **Proxy Progress** — background queue with progress bar

---

## 10. EXPORT

### 10.1 Export Dialog
- [ ] **Format Presets** — H.264, H.265, ProRes, DNxHR, MP3, WAV
- [ ] **Resolution** — 720p, 1080p, 4K, custom
- [ ] **Frame Rate** — source, 24, 25, 30, 60
- [ ] **Bitrate** — quality slider or custom Mbps
- [ ] **Audio Settings** — codec, sample rate, bitrate, channels
- [ ] **Range** — entire project / in-out range / selection
- [ ] **Include Markers** — chapters, to-do, all
- [ ] **Export Alpha** — ProRes 4444 with alpha
- [ ] **HDR Export** — Rec.2020 HLG / PQ (P2)

### 10.2 Share Destinations
- [ ] **Export File** — save to disk
- [ ] **Export Master File** — full-quality master
- [ ] **YouTube / Vimeo** — direct upload (P2)
- [ ] **Export Image** — still frame at playhead as PNG/JPEG
- [ ] **Export Audio Only** — WAV, MP3, AAC
- [ ] **Batch Export** — queue multiple exports

### 10.3 Export Formats (✓ FCPXML + EDL in rook-exporters)
- [ ] **FCPXML** — round-trip Final Cut Pro
- [ ] **EDL** — CMX 3600
- [ ] **JSON** — project archive
- [ ] **MLT XML** — Shotcut/Kdenlive interchange

---

## 11. KEYBOARD SHORTCUTS (FCP default set)

### 11.1 Playback & Navigation
- [ ] **Space** — Play/Pause (✓)
- [ ] **J / K / L** — Reverse / Stop / Forward (✓)
- [ ] **← / →** — Frame back/forward (✓)
- [ ] **⇧← / ⇧→** — 10 frames back/forward
- [ ] **Home / End** — Go to start/end (✓)
- [ ] **↑ / ↓** — Jump to previous/next edit point
- [ ] **F** — Scroll to playhead

### 11.2 Editing
- [ ] **⌘B** — Blade at playhead (✓ S key)
- [ ] **⇧⌘B** — Blade all tracks
- [ ] **⌫** — Lift (leave gap) (✓ Delete)
- [ ] **⇧⌫** — Ripple delete
- [ ] **I / O** — Set In / Out point
- [ ] **⌥I / ⌥O** — Clear In / Out
- [ ] **⌥[ / ⌥]** — Trim start/end to playhead
- [ ] **⌥\** — Trim to selection
- [ ] **⌘R** — Show retime editor
- [ ] **⌥G** — Compound clip
- [ ] **⇧⌥G** — Break apart compound

### 11.3 Tools
- [ ] **A** — Select (✓ click)
- [ ] **B** — Blade (✓)
- [ ] **T** — Trim
- [ ] **R** — Range select
- [ ] **P** — Position/transform

### 11.4 View
- [ ] **⇧Z** — Zoom to fit timeline
- [ ] **⌘= / ⌘-** — Zoom in/out timeline
- [ ] **⌘⇧1** — Show/hide browser
- [ ] **⌘⇧2** — Show/hide timeline
- [ ] **⌘⇧3** — Show/hide inspector

---

## 12. PROJECT MANAGEMENT

- [ ] **New Project** — create with name, resolution, fps, sample rate
- [ ] **Open Project** — from SQLite database or .rook file
- [ ] **Save Project** — ⌘S (✓)
- [ ] **Save As** — duplicate project
- [ ] **Duplicate Project** — copy with new name
- [ ] **Project Settings** — resolution, fps, color space, audio sample rate
- [ ] **Recent Projects** — list of recently opened
- [ ] **Auto-save** — periodic backup with recovery
- [ ] **Project Snapshots** — named save points you can revert to
- [ ] **Consolidate Project** — copy all media into project bundle

---

## 13. UI LAYOUT

### 13.1 Panels (FCP three-pane layout)
- [ ] **Browser (left)** — media, effects, transitions, titles, generators
- [ ] **Viewer (center-top)** — preview canvas with transform controls (✓)
- [ ] **Inspector (right)** — clip properties, effects, audio, info (✓ basic)
- [ ] **Timeline (center-bottom)** — tracks, clips, transitions (✓)
- [ ] **Audio Meters (right edge)** — floating or docked
- [ ] **Event Library** — project/organization sidebar

### 13.2 Window Management
- [ ] **Show/Hide Panels** — toggle each panel
- [ ] **Go Full Screen** — ⌃⌘F
- [ ] **Workspaces** — default, color & effects, audio, organization
- [ ] **Custom Workspace** — save/load panel layouts
- [ ] **Reset Workspace** — back to default layout

### 13.3 Appearance
- [ ] **Dark Mode** — default Apple Glass aesthetic (✓)
- [ ] **Light Mode** — optional
- [ ] **Highlight Color** — configurable accent color
- [ ] **Timeline Color** — clip colors by role/type
- [ ] **Playhead Color** — red (✓)
- [ ] **Snap Color** — yellow/orange

---

## 14. TITLES & GENERATORS

- [ ] **Basic Title** — text with font, size, color, alignment, background
- [ ] **Lower Third** — name + title bar templates
- [ ] **Credits Roll** — scrolling credits
- [ ] **3D Title** — extruded text (P2)
- [ ] **Custom Title Templates** — user-saved templates
- [ ] **Generators** — solid color, gradient, noise, custom (✓ GeneratorParams exists)
- [ ] **Timecode Generator** — burn-in timecode
- [ ] **Shape Generator** — rectangle, circle, line
- [ ] **Background Generator** — blur background, gradient

---

## 15. COLOR CORRECTION

- [ ] **Color Board** — exposure, saturation, color temp (4-way)
- [ ] **Color Wheels** — lift/gamma/gain with hue offsets
- [ ] **Curves** — RGB parade + luma curve
- [ ] **Hue/Saturation Curves** — selective color
- [ ] **Video Scopes** — waveform, vectorscope, histogram, RGB parade
- [ ] **LUT Browser** — preview LUTs on clip
- [ ] **Match Color** — match one clip's grade to another
- [ ] **HDR Tools** — HLG, PQ, Dolby Vision (P2)
- [ ] **Comparison Viewer** — split screen before/after

---

## 16. RETIMING

- [ ] **Constant Speed** — 50%, 200%, 400%, etc.
- [ ] **Variable Speed** — speed ramps with keyframable points
- [ ] **Reverse Clip** — play backwards
- [ ] **Freeze Frame** — hold on single frame
- [ ] **Optical Flow** — frame interpolation for smooth slow-mo (P2)
- [ ] **Frame Blending** — blend frames for smoother speed changes
- [ ] **Preserve Pitch** — when retiming audio
- [ ] **Speed Duration Dialog** — ⌘R, precise entry

---

## 17. MULTICAM

- [ ] **Create Multicam Clip** — sync 2–16 angles by timecode/audio
- [ ] **Angle Viewer** — grid of all angles during playback
- [ ] **Angle Editor** — cut between angles with number keys 1–9
- [ ] **Audio Follows Video** — switch audio with angle
- [ ] **Separate Audio** — keep master audio independent
- [ ] **Collapse Multicam** — flatten to single clip (destructive)

---

## 18. SUBTITLES & CAPTIONS

- [ ] **Import SRT / VTT** — load subtitle tracks
- [ ] **Subtitle Track** — dedicated subtitle lane on timeline
- [ ] **Edit Captions** — text, timing, position
- [ ] **Burn-In Captions** — render into video on export
- [ ] **Auto-Generate** — speech-to-text via Whisper (P2)
- [ ] **Export SRT** — save subtitles as file
- [ ] **Subtitle Inspector** — font, size, color, background, position
- [ ] **CEA-608 / CEA-708** — closed caption standards (P2)

---

## 19. PLUGIN SYSTEM

- [ ] **WASM Plugin Host** — load community plugins
- [ ] **Plugin Browser** — list installed plugins
- [ ] **Plugin Parameters** — expose to inspector
- [ ] **OFX / OpenFX** — industry standard plugin API (P2)
- [ ] **Python Scripting** — headless automation (P2)

---

## 20. ACCESSIBILITY

- [ ] **VoiceOver Support** — all UI elements labeled
- [ ] **High Contrast Mode** — increase contrast
- [ ] **Larger Text** — accessibility font sizes
- [ ] **Keyboard Navigation** — full keyboard control without mouse
- [ ] **Reduce Motion** — disable animations

---

## 21. PERFORMANCE & STABILITY

- [ ] **Hardware Decode** — VideoToolbox (macOS), NVDEC (NVIDIA), VAAPI (Linux)
- [ ] **GPU Compositing** — wgpu render pipeline (✓ foundation)
- [ ] **Proxy Workflow** — low-res editing proxies (✓ proxy service)
- [ ] **Background Rendering** — render effects/fusion in background
- [ ] **Auto-save** — recovery from crashes
- [ ] **Crash Reporter** — submit crash logs
- [ ] **Memory Management** — frame cache with LRU eviction (✓)
- [ ] **Low Memory Mode** — reduce cache, use proxies

---

## Priority Summary for Implementation

| Priority | Area | Key Items |
|----------|------|-----------|
| **P0** | Timeline Tools | Blade (⌘B), Trim (T), I/O marks, ripple delete (⇧⌫) |
| **P0** | Inspector | Transform, opacity, speed, full effect params |
| **P0** | Toolbar | Full tool strip with keyboard shortcuts |
| **P1** | Effects | Color board, chroma key, mask editor, transitions |
| **P1** | Audio | Waveforms on clips, fade handles, VU meter, gain line |
| **P1** | Trim | Ripple, roll, slip, slide — all trim modes |
| **P1** | Export | H.264/H.265 presets, range selection, image export |
| **P2** | Multicam | Sync angles, angle viewer, live switching |
| **P2** | Color | Scopes, curves, wheels, LUT browser |
| **P2** | Plugins | WASM host, OFX support |
| **P2** | HDR | Rec.2020, HLG, PQ, Dolby Vision |
| **P3** | Subtitles | SRT import/export, burn-in, auto-gen |
| **P3** | Accessibility | VoiceOver, high contrast, full keyboard nav |

---

*Last updated: 2026-06-04. Total: ~280 individual features across 21 categories.*
