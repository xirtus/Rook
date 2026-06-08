# Rook Architecture

## Crate dependency graph

```
rook-ui ──────┐
              ├──► rook-engine ──┬──► rook-core
rook-cli ─────┘                 ├──► rook-mlt
                                └──► rook-decode
                  rook-ipc ─────┘
```

- `rook-core` — zero external deps (serde + uuid only)
- `rook-mlt` — optional system lib (MLT 7.x via FFI)
- `rook-decode` — optional system lib (FFmpeg via ffmpeg-next)
- `rook-engine` — orchestrates core + mlt + decode
- `rook-ipc` — JSON-RPC server (axum + rmcp)
- `rook-ui` — egui desktop application
- `rook-cli` — clap CLI

## Data flow

```
User gesture (UI) ──► EditCommand ──► Engine::apply()
                                          │
                              ┌───────────┼───────────┐
                              ▼           ▼           ▼
                         Project      Undo        MLT Tractor
                         (model)    (snapshot)   (live engine)
                              │
                              ▼
                       Engine::frame_at(frame)
                              │
                    ┌─────────┼─────────┐
                    ▼         ▼         ▼
               MediaPool   FrameCache   MLT Producer
               (decoders)  (LRU cache)  (seek+decode)
```

## Edit lifecycle

1. UI or agent constructs an `EditCommand`
2. `Engine::apply(command)` is called
3. Engine clones `Project` → pushes to `EditHistory::undo_stack`
4. Engine validates the command against the model (overlap check, track existence)
5. Engine mutates the `Project` model
6. Engine mirrors the edit to the MLT tractor (if MLT is initialized)
7. UI receives the updated project and re-renders

## Agent integration

```
┌──────────┐  JSON-RPC  ┌──────────────┐
│ AI Agent │ ◄────────► │ rook-ipc     │
│ (Claude, │  stdio/     │ server       │
│  GPT,    │  socket/TCP │              │
│  custom) │            │ methods::     │
│          │            │ dispatch()    │
└──────────┘            └──────┬───────┘
                               │
                        ┌──────┴───────┐
                        │ rook-engine  │
                        │ Engine       │
                        └──────────────┘
```

The agent sees a stable, typed API. It never touches the file system
or the MLT tractor directly. All mutations flow through `Engine::apply()`,
which means undo/redo and validation are always enforced.
