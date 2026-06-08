# Rook IPC — Agent API Reference

Rook exposes a JSON-RPC 2.0 API over stdio, Unix socket, or TCP.  
AI agents connect, inspect the project, execute edits, and trigger exports.

## Transport

| Mode | Connection | Use-case |
|---|---|---|
| stdio | Spawn `rook-cli serve --ipc stdio` as subprocess | Local agent in same process tree |
| Unix socket | Connect to `~/.rook/ipc.sock` | Local agent (separate process) |
| TCP | Connect to `127.0.0.1:9123` | Remote agent or web dashboard |

## Protocol

Newline-delimited JSON.  Every message is exactly one line.

```json
{"jsonrpc":"2.0","id":1,"method":"project.get","params":{}}
```

Response:
```json
{"jsonrpc":"2.0","id":1,"result":{"snapshot":{...}}}
```

Error:
```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found: project.frob"}}
```

## Methods

### project.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `project.get` | `{}` | `{snapshot: ProjectSnapshot}` | Full project state |
| `project.create` | `{name, canvas?, fps?}` | `{snapshot: ProjectSnapshot}` | New project |
| `project.open` | `{path}` | `{snapshot: ProjectSnapshot}` | Open from file |
| `project.save` | `{path?}` | `{path}` | Save to file |
| `project.export` | `{output_path, format, preset?}` | `{job_id}` | Start export job |

### gallery.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `gallery.import` | `{paths: [string]}` | `{assets: [Asset]}` | Import media files |
| `gallery.list` | `{}` | `{assets: [Asset]}` | List all assets |
| `gallery.probe` | `{path}` | `{metadata: AssetMetadata}` | Probe file metadata |
| `gallery.annotate` | `{asset_id, description?, labels?}` | `{}` | Set AI labels |
| `gallery.remove` | `{asset_id}` | `{}` | Remove asset |

### timeline.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `timeline.get` | `{include_subtitles?, include_semantic?}` | `{snapshot}` | Full timeline |
| `timeline.insert_clip` | `{asset_id, track_id, position, source_in, source_out?}` | `{clip_id}` | Place clip |
| `timeline.remove_clip` | `{clip_id}` | `{}` | Remove clip |
| `timeline.move_clip` | `{clip_id, new_track_id, new_position}` | `{}` | Move clip |
| `timeline.trim_clip` | `{clip_id, new_source_range}` | `{}` | Trim in/out |
| `timeline.split_clip` | `{clip_id, at_frame}` | `{clip_a, clip_b}` | Split at frame |
| `timeline.ripple_delete` | `{clip_id}` | `{}` | Ripple delete |
| `timeline.add_track` | `{kind, name, index?}` | `{track_id}` | Add track |
| `timeline.remove_track` | `{track_id}` | `{}` | Remove track |
| `timeline.set_playhead` | `{frame}` | `{frame}` | Move playhead |
| `timeline.add_filter` | `{clip_id, filter}` | `{}` | Add filter |
| `timeline.add_keyframe` | `{clip_id, keyframe}` | `{}` | Add keyframe |

### preview.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `preview.get_frame` | `{frame?}` | `{frame, image_base64, width, height}` | Composited frame |
| `preview.get_waveform` | `{clip_id}` | `{samples: [f32], sample_rate}` | Audio waveform |

### undo.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `undo.undo` | `{}` | `{undone, can_undo, can_redo}` | Undo last edit |
| `undo.redo` | `{}` | `{undone, can_undo, can_redo}` | Redo |
| `undo.history` | `{}` | `{undo_stack, redo_stack}` | History |

### batch.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `batch.execute` | `{commands: [EditCommand]}` | `{results: [{ok, error?}]}` | Atomic batch |

### query.*

| Method | Params | Returns | Description |
|---|---|---|---|
| `query.search_clips` | `{query, min_duration_frames?}` | `{clips: [{clip_id, score}]}` | Semantic search |
| `query.find_gaps` | `{}` | `{gaps: [[start, end]]}` | Find timeline gaps |
| `query.analyze_pacing` | `{track_id?}` | `{pacing_data}` | Clip pacing |

## Events (Editor → Agent)

The server pushes these notifications (no `id`, no response expected):

```json
{"jsonrpc":"2.0","method":"project.changed","params":{"dirty":true}}
{"jsonrpc":"2.0","method":"playhead.moved","params":{"frame":240}}
{"jsonrpc":"2.0","method":"export.progress","params":{"job_id":"j1","percent":67.3}}
{"jsonrpc":"2.0","method":"proxy.status","params":{"asset_id":"a1","status":"ready"}}
{"jsonrpc":"2.0","method":"selection.changed","params":{"clip_ids":["c1","c2"]}}
```

## Example: Agent Script

```python
import json, subprocess

proc = subprocess.Popen(
    ["rook-cli", "serve", "--ipc", "stdio"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
)

def call(method, params=None, req_id=1):
    req = {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params or {}}
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())

# Import footage
assets = call("gallery.import", {"paths": ["clip1.mp4", "clip2.mp4"]})

# Get timeline
tl = call("timeline.get")

# Insert clip
clip = call("timeline.insert_clip", {
    "asset_id": assets["result"]["assets"][0]["id"],
    "track_id": tl["result"]["tracks"][0]["id"],
    "position": 0,
    "source_in": 0,
    "source_out": 120
})

# Export
call("project.export", {
    "output_path": "output.mp4",
    "format": "h264",
    "preset": "youtube"
})
```
