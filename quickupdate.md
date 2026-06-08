

<><><>><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><><>
______________________________________________________________________________________________
below is our log of session completions, ignore unless you need to research execution history:

status:

Here's the complete remaining feature list, ranked easiest → hardest:

┌────┬──────────────────────────────────────┬──────────┬─────────────────────────────────────────────────────────────────┐
│  # │               Feature                │  Effort  │                          What It Takes                           │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│  1 │ Export Alpha (ProRes 4444)           │  ●○○○○   │ Add "Include Alpha" checkbox + ProRes 4444 variant in           │
│    │                                      │  ~10 min │ export dialog. Already have ProRes path, just add codec         │
│    │                                      │          │ option + alpha flag.                                            │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│  2 │ Crash Recovery from Auto-save        │  ●○○○○   │ On project open, check for newer auto-save in                   │
│    │                                      │  ~15 min │ ~/.local/share/Rook/autosave/, offer "Recover?" dialog.         │
│    │                                      │          │ SQLite auto-save already exists — just detect & restore.        │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│  3 │ Consolidate Project                  │  ●○○○○   │ "Consolidate" menu item → copy all referenced media files       │
│    │                                      │  ~15 min │ into a project bundle folder next to .rook file.                │
│    │                                      │          │ Simple fs::copy + path rewrite.
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│  4 │ Custom Workspace saveSerialize panel visibilitybooleans to JSON in                  │
│    │~/.local/share/Rook/workspaces/. Save/Load/Reset                │
│    │                      buttons in View menu. Just 5bools + a name string.             │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│  5 │ Distort / Corner Pin Add 4 `CornerOffset` fieldsto Transform. Extend                │
│    │                      draw/handle methods for 4independent corner drags.             │
│    │                                      │          │ Bilinear warp math in compositor.                               │
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│  6 │ Replace with Source R3-point edit: user marks I/Oon source clip, then              │
│    │                                      │  ~30 min │ replaces selected timeline clip range. Already have             │
│    │                      TrimClip/SlipClip commands —just wire the UI.                  │
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│  7 │ Filmstrip View (hover-scrub)         │  ●●○○○   │ In gallery, on thumbnail hover, show filmstrip with             │
│    │                      frame-at-cursor. Use existing ThumbnailCache to seek            │                                                  │    │                      through frames on mouse Xposition.                             │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤                                                 │  8 │ Proxy Generate + Pref"Generate Proxy" button perasset → spawn ffmpeg to
│    │                                      │  ~60 min │ create ProRes Proxy / H.264 low-res. ProxyService               │
│    │                                      │          │ skeleton exists. Add
resolution picker + progress
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│  9 │ Frame Blending Rendering             │  ●●●○○   │ When frame_blending=true, crossfade adjacent decoded            │
│    │                      frames in the compositor.
Already have the toggle —
│    │                                      │          │ just need the blend logic in frame_rgba.                        │
├────┼──────────────────────────────────────┼──────────┼──────────────────────────────
────────────────────────────
│ 10 │ CEA-608 Closed Captions              │  ●●●○○   │ Parse CEA-608 data from video tracks (line 21 data).            │
│    │                                      │  ~90 min │ Map to subtitle track on
import. Format-specific byte
│    │                                      │          │ parsing from video elementary streams.                          │
├────┼──────────────────────────────────────┼──────────┼──────────────────────────────
────────────────────────────
│ 11 │ Sync Audio (waveform alignment)      │  ●●●●○   │ Select video clip + external audio → cross-correlate           │
│    │                                      │ ~120 min │ waveforms to find offset.
Already have waveform data,
│    │                                      │          │ just need correlation math + UI to adjust sync.                 │
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│ 12 │ Color Curves Visual EDraw spline curve on a256×256 canvas. Drag control             │
│    │                                      │ ~120 min │ points. Map to RGB curve LUT. egui custom paint with            │                                          │    │                      cubic bezier interpolation.                                   │
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────
│ 13 │ Color Wheels Visual Editor           │  ●●●●○   │ HSL color wheel widgshadows/midtones/highlights
│    │                                      │ ~120 min │ offsets. Render radigradient. Map to lift/gamma/
│    │                                      │          │ color matrix for com├────┼───────────────────────────────────────────────────────────────────────────────│ 14 │ Multicam (6 sub-featuSync 2-16 angles bywaveform/TC, angle viewer gr
│    │                                      │ ~8 hrs   │ switching with number keys,
audio-follows-video logic,
│    │                                      │          │ separate master audio, collapse multicam to single track.       │
├────┼──────────────────────────────────────┼──────────┼──────────────────────────────
────────────────────────────
│ 15 │ Import: Camera / iMovie / EDL / AAF  │  ●●●●●   │ Camera: AVFoundation/MTP
device discovery. iMovie: .f
│    │                                      │ ~6 hrs   │ variant parser. EDL: CMX 3600 parser. AAF: structured           │
│    │                                      │          │ storage format (complex
binary). 4 separate parsers.
├────┼──────────────────────────────────────┼──────────┼──────────────────────────────
────────────────────────────
│ 16 │ Plugins: WASM + OFX + Python         │  ●●●●●   │ WASM host (wasmtime runtime + sandbox), plugin browser UI,       │
│    │                                      │ ~12 hrs  │ dynamic parameter mapping to
inspector. OFX host (C ABI).
│    │                                      │          │ Python scripting (pyo3
embedded interpreter).
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│ 17 │ NVDEC / VAAPI (Linux GPU decode)     │  ●●●●●   │ FFmpeg hardware decode
context setup. NVDEC = CUDA/
│    │                                      │  ~4 hrs  │ API, VAAPI = libva. Need to
plumb through decode pipelin
│    │                                      │          │ Platform-specific, needs Linux test environment.                │
├────┼──────────────────────────────────────┼──────────┼──────────────────────────────
────────────────────────────
│ 18 │ Stabilization        Feature point tracking(KLT/Harris), motion estimation,         │
│    │                                      │  ~8 hrs  │ inverse transform per frame. Requires computer vision           │
│    │                      crate (e.g., opencv-rust) or
custom implementation.
├────┼──────────────────────────────────────┼──────────┼─────────────────────────────────────────────────────────────────┤
│ 19 │ Low Memory Mode + Crash Reporter     │  ●●●●●   │ Low memory: half-res
textures, aggressive LRU evi
│    │                      streaming decode. Crashreporter: native signal handler         │
│    │                                      │          │ + minidump upload (sentry/breakpad). Platform-specific.         │
├────┼──────────────────────────────────────────────────────────────────────────────────────┤
│ 20 │ VoiceOver accessibility              │  ●●●●●   │ macOS NSAccessibility protocol integration. Requires            │
│    │                      Objective-C bindings, customegui accessibility backend,        │
│    │                                      │          │ or platform-layer annotations. macOS-only.                      │
└────┴──────────────────────────────────────────────────────────────────────────────────────┘

Triage summary:

┌───────────────────┬───────┬─────────────────────────────────────────────────────┐
│       Tier        │ Count │                        Items                        │
├───────────────────┼────────────────────────────────┤
│ 🟢 Quick wins     │ 3     │ Export Alpha, Crash Recovery, Consolidate           │
│ (●○○○○)           │       │                                                     │
├───────────────────┼────────────────────────────────┤
│ 🟢 Small features │ 4     │ Custom Workspace, Distort, Replace Range, Filmstrip │
│  (●●○○○)          │       │                                                     │
├───────────────────┼────────────────────────────────┤
│ 🟡 Medium (●●●○○) │ 6     │ Proxy Gen, Frame Blending, CEA-608, Sync Audio,     │
│                   │       │ Color Curves, Color Wheels                          │
├───────────────────┼────────────────────────────────┤
│ 🔴 Heavy (●●●●○)  │ 0     │ —                                                   │
├───────────────────┼────────────────────────────────┤
│ 🔴 Major (●●●●●)  │ 7     │ Multicam, Import formats, Plugins, NVDEC,           │
│                   │       h, VoiceOver             │
└───────────────────┴───────┴─────────────────────────────────────────────────────┘

Want me to start knocking oues for 3 features that wouldpush the project to ~97%.




✅ Batch 5 — Accessibility + Track Mgmt + Project Snapshots + Viewer Polish

Progress: 92% → 95% (266/280 done, 14 remaining)

Accessibility: 0% → 80% (4 of 5 done)

┌──────────────────┬──────────────────────────────────────────────────────────────┐
│     Feature      │                        Implementation                        │
├──────────────────┼──────────────────────────────────────────────────────────────┤
│ High Contrast    │ View menu toggle, modifies widget colors for high contrast   │
├──────────────────┼──────────────────────────────────────────────────────────────┤
│ Larger Text      │ View menu toggle, sets 1.5× pixel scaling via                │
│                  │ ctx.set_pixels_per_point()                                   │
├──────────────────┼─────────────────────────────────┤

Accessibility: 0% → 80% (4 o

┌──────────────────┬─────────────────────────────────┐
│     Feature      │                        Implementation                        │
├──────────────────┼──────────────────────────────────────────────────────────────┤
│ High Contrast    │ View menu toggle, modifies widget colors for high contrast   │
├──────────────────┼─────────────────────────────────┤
│ Larger Text      │ View menu toggle, sets 1.5× pixel scaling via                │
│                  │ ctx.set                         │
├──────────────────┼──────────────────────────────────────────────────────────────┤
│ Reduce Motion    │ View menu toggle, disables unnecessary animations            │
├──────────────────┼──────────────────────────────────────────────────────────────┤
│ Keyboard         │ Alreadye shortcut system        │
│ Navigation       │                                                              │
├──────────────────┼─────────────────────────────────┤
│ VoiceOver        │ Remaining (platform-level feature)                           │
└──────────────────┴──────────────────────────────────────────────────────────────┘

Track Management: 85% → 93%

┌────────────────────────────────────────────────────┐
│           Feature            │                  Implementation                  │
├──────────────────────────────┼──────────────────────────────────────────────────┤
│ Primary Storyline            │ is_primary: bool on Track, right-click header →  │
│                            others)                 │
├──────────────────────────────┼──────────────────────────────────────────────────┤
│ SetPrimaryTrack command +                          │
│ engine handler               │                                                  │
└──────────────────────────────┴──────────────────────────────────────────────────┘

Project Management: 70% → 83

┌────────────────────────────────────────────────────┐
│                Feature                │             Implementation              │
├───────────────────────────────────────┼─────────────────────────────────────────┤
│                                       │ Named save points stored in Engine,     │
│ Project Snapshots         s menu                   │
│                                       │ (save/restore/list)                     │
├────────────────────────────────────────────────────┤
│ Engine methods: save_snapshot(),      │                                         │
│ restore_snapshot(), snapshot_names()  │                                         │
└───────────────────────────────────────┴─────────────────────────────────────────┘

UI Layout: 80% → 90%

┌────────────┬────────────────────────────────────────────────────────────────────┐
│  Feature   │                           Implementation                           │
├────────────┼────────────────────────────────────────────────────────────────────┤
│ Event      │ View → 📚 Evets by type, markers      │
│ Library    │ list, compound clips, asset usage stats                            │
└────────────┴───────────────────────────────────────┘

Viewer/Canvas: 78% → 89%

┌─────────────────┬───────────────────────────────────┐
│     Feature     │         n                         │
├─────────────────┼────────────────────────────────────────────────────────────────┤
│ Canvas Overlays │ 💬 toggle: timecode at bottom + selected clip name at top-left │
├─────────────────┼────────────────────────────────────────────────────────────────┤
│ Quality Toggle  │ ⚡/🐢 toty mode                   │
└─────────────────┴───────────────────────────────────┘

Audio: 78% → 83%

┌─────────────┬──────────────────────────────────────┐
│   Feature   │                                      │
├─────────────┼───────────────────────────────────────────────────────────────────┤
│ Gain        │ Vol quick-add button in keyframe editor, uses existing            │
│ Keyframe    │ KeyframeProperty::Volume                                          │
└─────────────┴──────────────────────────────────────┘

Effects Library: Color Curves/Wheels/LUT all have enum variants + editors

What Remains (14 items):

┌───────────────┬───────┬──────────────────────────────────────────────────────────┐
│   Category    │ Count │                          Items                           │
├───────────────┼───────┼─────────────────────────────┤
│ Multicam      │ 6     │ Sync, angle viewer, live switching, audio follows,       │
│               │       │ separate audio, collapse                                 │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Plugins       │ 5     │ WA, Python scripting        │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Import        │ 5     │ Filmstrip, proxy, EDL/AAF, Camera, iMovie                │
├───────────────┼───────┼─────────────────────────────┤
│ Performance   │ 3     │ NVDEC/VAAPI, low mem, crash reporter                     │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Audio         │ 2     │ Sy                          │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Project Mgmt  │ 2     │ Consolidate, crash recovery                              │
├───────────────┼───────┼─────────────────────────────┤
│ Subtitles     │ 2     │ BG/outline, CEA-608                                      │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Effects       │ 2     │ Cu                          │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Clip Props    │ 2     │ Dion                        │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ Export        │ 2     │ Yo                          │
├───────────────┼───────┼──────────────────────────────────────────────────────────┤
│ UI Layout     │ 2     │ Cuce                        │
├───────────────┼───────┼─────────────────────────────┤
│ Viewer        │ 1     │ Pi                          │
├───────────────┼───────┼─────────────────────────────┤
│ Ruler         │ 1     │ Cl                          │
├───────────────┼───────┼─────────────────────────────┤
│ Toolbar       │ 1     │ New project button                                       │
├───────────────┼───────┼─────────────────────────────┤
│ Titles        │ 1     │ Mo                          │
├───────────────┼───────┼─────────────────────────────┤
│ Timeline      │ 1     │ Re                          │
├───────────────┼───────┼─────────────────────────────┤
│ Retiming      │ 1     │ Frame blending rendering                                 │
├───────────────┼───────┼─────────────────────────────┤
│ Accessibility │ 1     │ VoiceOver                                                │
└───────────────┴───────┴─────────────────────────────┘

Categories at 100%:

Shortcuts · Color Correction


 ✅ Batch 4 — Effects Library + Timeline Editing + UI Polish

  Progress: 87% → 92% (257/280 done, 23 remaining)

  Effects Library: 53% → 84% (15 new effects)

  ┌──────────────┬──────────────────────────────────────────┬─────────┐
  │    Effect    │                Parameters                │ Status  │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ EQ           │ Low/Mid/High gain sliders (-24..+12 dB)  │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Compressor   │ Threshold / Ratio / Makeup gain          │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Limiter      │ Ceiling / Release ms                     │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Reverb       │ Wet mix / Decay                          │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Delay/Echo   │ Delay time / Feedback                    │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Pitch Shift  │ Semitone slider (-12..+12)               │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Glow         │ Radius / Intensity                       │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Luma Key     │ Threshold / Tolerance                    │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Distort      │ Barrel / Pincushion                      │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Noise        │ Amount / Monochrome toggle               │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Timecode     │ Font size / Position (top/center/bottom) │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ LUT Loader   │ .cube file picker                        │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Text Overlay │ Text content + font size                 │ ✅ Done │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Color Curves │ Placeholder (UI planned)                 │ ⬜ Stub │
  ├──────────────┼──────────────────────────────────────────┼─────────┤
  │ Color Wheels │ Placeholder (UI planned)                 │ ⬜ Stub │
  └──────────────┴──────────────────────────────────────────┴─────────┘

  EffectKind enum grew from 21 to 31 variants. All new effects appear in categorized dropdown (Video/Audio sections) with proper parameter editors. Remaining: visual curves/wheels editors.

  Timeline Editing: 86% → 91%

  ┌────────────┬────────────────────────────────────────────────────────────────────┐
  │  Feature   │                           Implementation                           │
  ├────────────┼────────────────────────────────────────────────────────────────────┤
  │ Connect    │ Right-click → creates connected clip on new track above            │
  │ Clip       │                                                                    │
  ├────────────┼────────────────────────────────────────────────────────────────────┤
  │ Trim Edit  │ Togglable ⏩ Trim Window panel with frame-accurate                 │
  │ Window     │ source_in/duration/timeline_in drag editors + JKL-style nudge      │
  │            │ buttons (◁◁ ◁ ▷ ▷▷)                                                │
  └────────────┴────────────────────────────────────────────────────────────────────┘

Export: 78% → 89%

┌──────────────┬──────────────────────────────────────────────────────────────────┐
│   Feature    │                          Implementation                          │
├──────────────┼──────────────────────────────────────────────────────────────────┤
│ Batch Export │ "Add to Batch" button in export dialog, persistent queue window  │
│  Queue       │ with per-job remove, "Export All" to process sequentially        │
└──────────────┴──────────────────────────────────────────────────────────────────┘

UI Layout: 70% → 80%

┌───────────┬─────────────────────────────────────────────────────────────────────┐
│  Feature  │                           Implementation                            │
├───────────┼─────────────────────────────────────────────────────────────────────┤
│ Light     │ View → ☀️ Light Mode / 🌙 Dark Mode toggle, applies                 │
│ Mode      │ egui::Visuals::light()/dark()                                       │
└───────────┴─────────────────────────────────────────────────────────────────────┘

Import/Media: 62% (Relink, Smart Collections, Used/Unused, Tags done)

What's Left (23 items):

┌─────────────────┬───────┬───────────────────────────────────────────────────────┐
│    Category     │ Count │                        Details                        │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Multicam        │ 6     │ All at 0% — sync, angle viewer, live switching        │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Plugins         │ 5     │ All at 0% — WASM, OFX, Python                         │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Accessibility   │ 5     │ All at 0% — VoiceOver, contrast, keyboard nav         │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Import          │ 5     │ Filmstrip, proxy, EDL/AAF, Camera, iMovie             │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ UI Layout       │ 4     │ Event library, custom workspace, reset, accent color  │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Performance     │ 3     │ NVDEC/VAAPI, low mem, crash reporter                  │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Project Mgmt    │ 3     │ Snapshots, consolidate, crash recovery                │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Audio           │ 3     │ Gain keyframes, sync, -12dB normalize                 │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Viewer/Canvas   │ 3     │ Quality toggle, overlays, rulers                      │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Effects         │ 2     │ Curves/wheels visual editors                          │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Export          │ 2     │ YouTube/Vimeo, Alpha                                  │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Timeline        │ 2     │ Connected storyline indicator, replace with source    │
│ Editing         │       │ range                                                 │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Subtitles       │ 2     │ BG/outline, CEA-608                                   │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Clip Props      │ 2     │ Distort/corner pin, stabilization                     │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Track Mgmt      │ 1     │ Primary storyline                                     │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Retiming        │ 1     │ Frame blending rendering                              │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Titles          │ 1     │ More L3 presets                                       │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Ruler           │ 1     │ Click-on-index to jump                                │
├─────────────────┼───────┼───────────────────────────────────────────────────────┤
│ Toolbar         │ 1     │ New project button                                    │
└─────────────────┴───────┴───────────────────────────────────────────────────────┘


---



### P2 — Big Features (0% done)
- [ ] **Multicam** (0/6)
- [ ] **Plugin system** (0/5) — WASM host, OFX
- [ ] **Accessibility** (0/5) — VoiceOver, high contrast, full keyboard nav
- [ ] **HDR** — Rec.2020, HLG, PQ
- [ ] **Auto-generated captions** — Whisper STT
- [ ] **Optical Flow** — smooth slow-mo

---

## Architecture

```
rook-ui (egui app) ──┐
                     ├── rook-engine (orchestrator + undo)
rook-cli ────────────┘    ├── rook-core (model: clips/tracks/commands)
                          ├── rook-mlt (MLT 7.x FFI bridge)
                          ├── rook-decode (FFmpeg-next)
                          ├── rook-renderer (wgpu compositor + effects)
                          └── rook-timeline (graph model)
         rook-ipc ────────┘ (JSON-RPC server for AI agents)
```

**Key crates**: 18 total
| Crate | Purpose | LOC |
|-------|---------|-----|
| `rook-core` | Data model, EditCommand enum, serialization | 2,010 |
| `rook-ui` | egui panels (timeline, inspector, preview) | 6,576 |
| `rook-renderer` | wgpu compositor + CPU fallback + WGSL shaders | 2,459 |
| `rook-engine` | Orchestrator, undo/redo, MLT sync | ~500 |
| `rook-mlt` | MLT 7.x FFI wrappers | ~600 |

---

## Next Steps (recommended order)

1. **Ripple/Roll Trim UI** — wire the existing model to timeline gestures
2. **Marker List panel** — show all markers in a sortable list
3. **Keyframe editor** — diamond buttons + curve editor in inspector
4. **VU Meter** — audio metering overlay
5. **Media browser upgrade** — search, filmstrip, favorites
6. **Export polish** — range selection, image export, audio-only

---

*Last assessed: 2026-06-06. Project at ~64% FCP parity across ~280 features.*
