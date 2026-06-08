//! Method dispatch table — maps verb names to handler functions.
//! Each handler receives `params` JSON and a reference to the Engine.

use rook_core::commands::EditCommand;
use rook_core::ids::TrackId;
use rook_core::track::TrackKind;
use rook_engine::Engine;
use crate::protocol::Response;
use crate::types::*;

/// Registry of all supported API methods.
pub static METHOD_REGISTRY: &[(&str, &str)] = &[
    ("project.get",        "Get full project state"),
    ("project.create",     "Create a new project"),
    ("project.open",       "Open project from file"),
    ("project.save",       "Save project to file"),
    ("project.export",     "Start export job"),
    ("gallery.import",     "Import media files"),
    ("gallery.list",       "List all assets"),
    ("gallery.probe",      "Probe a file for metadata"),
    ("gallery.annotate",   "Set AI labels/description on asset"),
    ("gallery.remove",     "Remove asset from pool"),
    ("timeline.get",          "Get full timeline state"),
    ("timeline.insert_clip",  "Insert clip on track"),
    ("timeline.remove_clip",  "Remove clip"),
    ("timeline.move_clip",    "Move clip to new track/position"),
    ("timeline.trim_clip",    "Trim clip in/out"),
    ("timeline.split_clip",   "Split clip at frame"),
    ("timeline.ripple_delete","Ripple delete clip"),
    ("timeline.add_track",    "Add new track"),
    ("timeline.remove_track", "Remove track"),
    ("timeline.set_playhead", "Move playhead"),
    ("timeline.add_filter",   "Add filter to clip"),
    ("timeline.add_keyframe", "Add keyframe to clip"),
    ("preview.get_frame",   "Get composited frame as base64 JPEG"),
    ("preview.get_waveform","Get audio waveform data"),
    ("undo.undo",   "Undo last edit"),
    ("undo.redo",   "Redo last undone edit"),
    ("undo.history","Get undo/redo history"),
    ("batch.execute", "Execute multiple commands atomically"),
    ("query.search_clips",   "Semantic search over clips"),
    ("query.find_gaps",      "Find timeline gaps"),
    ("query.analyze_pacing", "Analyze clip pacing"),
];

/// Dispatch a method call. Returns a JSON-RPC Response.
pub fn dispatch(
    id: Option<u64>,
    method: &str,
    params: Option<serde_json::Value>,
    engine: &Engine,
) -> Response {
    let result: Option<serde_json::Value> = match method {
        // ── Project ─────────────────────────────────────────────────────
        "project.get" => {
            let snapshot = engine.project().to_snapshot();
            serde_json::to_value(ProjectGetResponse { snapshot }).ok()
        }
        "project.save" => {
            // Try to save; engine owns current_path
            let path_param = params.as_ref()
                .and_then(|p| p.get("path"))
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            // Note: save is &mut, but we're & — for IPC we'd need interior mutability.
            // For now, return the current path as informational.
            let path = engine.current_path().map(|p| p.to_string_lossy().to_string());
            Some(serde_json::json!({"saved": false, "path": path, "note": "save requires mutable engine"}))
        }
        "project.export" => {
            let p = params.as_ref();
            let output = p.and_then(|p| p.get("output_path")?.as_str()).unwrap_or("export.mp4");
            let format = p.and_then(|p| p.get("format")?.as_str()).unwrap_or("mp4");
            Some(serde_json::json!({"export_status": "not_implemented",
                "output": output, "format": format}))
        }

        // ── Gallery ────────────────────────────────────────────────────
        "gallery.list" => {
            let resp = GalleryListResponse { assets: engine.project().assets.clone() };
            serde_json::to_value(resp).ok()
        }
        "gallery.import" => {
            // Import requires &mut Engine — return metadata about what would happen
            let paths: Vec<String> = params.as_ref()
                .and_then(|p| p.get("paths"))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(serde_json::json!({
                "imported": 0,
                "requested": paths.len(),
                "note": "import requires mutable engine — use batch API or direct engine call"
            }))
        }
        "gallery.probe" => {
            let path = params.as_ref().and_then(|p| p.get("path")?.as_str());
            Some(serde_json::json!({"path": path, "probe": "not_implemented"}))
        }
        "gallery.annotate" => {
            let p = params.as_ref();
            let asset_id: Option<u64> = p.and_then(|p| p.get("asset_id")?.as_u64());
            let description = p.and_then(|p| p.get("description")?.as_str()).map(String::from);
            let labels: Vec<String> = p.and_then(|p| p.get("labels")?.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(serde_json::json!({
                "asset_id": asset_id, "description": description, "labels": labels,
                "note": "annotation requires mutable engine"
            }))
        }
        "gallery.remove" => {
            let asset_id = params.as_ref().and_then(|p| p.get("asset_id")?.as_u64());
            Some(serde_json::json!({"asset_id": asset_id, "note": "requires mutable engine"}))
        }

        // ── Timeline ───────────────────────────────────────────────────
        "timeline.get" => {
            let snapshot = engine.project().to_snapshot();
            serde_json::to_value(TimelineGetResponse { snapshot }).ok()
        }
        "timeline.insert_clip" => {
            let p = params.as_ref();
            if let (Some(asset_id), Some(track_id), Some(position)) = (
                p.and_then(|p| p.get("asset_id")?.as_u64()),
                p.and_then(|p| p.get("track_id")?.as_u64()),
                p.and_then(|p| p.get("position")?.as_i64()),
            ) {
                let source_in = p.and_then(|p| p.get("source_in")?.as_i64()).unwrap_or(0);
                let source_out = p.and_then(|p| p.get("source_out")?.as_i64()).unwrap_or(300);
                let cmd = EditCommand::InsertClip {
                    asset_id: rook_core::AssetId::from_u64(asset_id),
                    track_id: TrackId::from_u64(track_id),
                    position,
                    source_in,
                    source_out,
                    link_group_id: None,
                };
                Some(serde_json::json!({
                    "command": cmd.label(),
                    "asset_id": asset_id, "track_id": track_id, "position": position,
                    "note": "apply requires mutable engine — use batch.execute for atomic apply"
                }))
            } else {
                Some(serde_json::json!({"error": "missing required params: asset_id, track_id, position"}))
            }
        }
        "timeline.remove_clip" => {
            let clip_id = params.as_ref().and_then(|p| p.get("clip_id")?.as_u64());
            Some(serde_json::json!({"clip_id": clip_id, "note": "requires mutable engine"}))
        }
        "timeline.move_clip" => {
            let p = params.as_ref();
            let clip_id = p.and_then(|p| p.get("clip_id")?.as_u64());
            let new_track = p.and_then(|p| p.get("new_track_id")?.as_u64());
            let new_pos = p.and_then(|p| p.get("new_position")?.as_i64());
            Some(serde_json::json!({"clip_id": clip_id, "new_track_id": new_track, "new_position": new_pos, "note": "requires mutable engine"}))
        }
        "timeline.trim_clip" => {
            let p = params.as_ref();
            let clip_id = p.and_then(|p| p.get("clip_id")?.as_u64());
            let new_in = p.and_then(|p| p.get("source_in")?.as_i64());
            let new_out = p.and_then(|p| p.get("source_out")?.as_i64());
            Some(serde_json::json!({"clip_id": clip_id, "source_in": new_in, "source_out": new_out, "note": "requires mutable engine"}))
        }
        "timeline.split_clip" => {
            let p = params.as_ref();
            let clip_id = p.and_then(|p| p.get("clip_id")?.as_u64());
            let at_frame = p.and_then(|p| p.get("at_frame")?.as_i64());
            Some(serde_json::json!({"clip_id": clip_id, "at_frame": at_frame, "note": "requires mutable engine"}))
        }
        "timeline.ripple_delete" => {
            let clip_id = params.as_ref().and_then(|p| p.get("clip_id")?.as_u64());
            Some(serde_json::json!({"clip_id": clip_id, "note": "requires mutable engine"}))
        }
        "timeline.add_track" => {
            let p = params.as_ref();
            let kind_str = p.and_then(|p| p.get("kind")?.as_str()).unwrap_or("video");
            let name = p.and_then(|p| p.get("name")?.as_str()).unwrap_or("New Track");
            let kind = match kind_str {
                "audio" => TrackKind::Audio,
                "text" => TrackKind::Text,
                "effect" => TrackKind::Effect,
                _ => TrackKind::Video,
            };
            // Read-only: return what would be created
            let next_idx = engine.project().timeline.tracks_of_kind(kind).len();
            Some(serde_json::json!({
                "kind": format!("{:?}", kind), "name": name, "index": next_idx,
                "note": "requires mutable engine"
            }))
        }
        "timeline.remove_track" => {
            let track_id = params.as_ref().and_then(|p| p.get("track_id")?.as_u64());
            Some(serde_json::json!({"track_id": track_id, "note": "requires mutable engine"}))
        }
        "timeline.set_playhead" => {
            // Read-only on the engine reference — playhead lives in project
            let frame = params.as_ref().and_then(|p| p.get("frame")?.as_i64()).unwrap_or(0);
            let current = engine.project().timeline.playhead;
            Some(serde_json::json!({"frame": frame, "previous": current, "note": "requires mutable engine to set"}))
        }
        "timeline.add_filter" => {
            let p = params.as_ref();
            let clip_id = p.and_then(|p| p.get("clip_id")?.as_u64());
            let filter_type = p.and_then(|p| p.get("type")?.as_str()).unwrap_or("unknown");
            Some(serde_json::json!({"clip_id": clip_id, "filter_type": filter_type, "note": "requires mutable engine"}))
        }
        "timeline.add_keyframe" => {
            let p = params.as_ref();
            let clip_id = p.and_then(|p| p.get("clip_id")?.as_u64());
            let frame = p.and_then(|p| p.get("frame")?.as_i64());
            let property = p.and_then(|p| p.get("property")?.as_str());
            Some(serde_json::json!({"clip_id": clip_id, "frame": frame, "property": property, "note": "requires mutable engine"}))
        }

        // ── Preview ────────────────────────────────────────────────────
        "preview.get_frame" => {
            let frame = params.as_ref()
                .and_then(|p| p.get("frame")?.as_i64())
                .unwrap_or(engine.project().timeline.playhead);
            let layers = engine.frame_at(frame);
            let canvas = &engine.project().canvas;
            Some(serde_json::json!({
                "frame": frame,
                "image_base64": "",
                "width": canvas.width,
                "height": canvas.height,
                "layers": layers.len(),
                "note": "GPU render not yet wired — returns metadata only"
            }))
        }
        "preview.get_waveform" => {
            let clip_id = params.as_ref().and_then(|p| p.get("clip_id")?.as_u64());
            Some(serde_json::json!({"clip_id": clip_id, "samples": [], "note": "audio decode not yet implemented"}))
        }

        // ── Undo ───────────────────────────────────────────────────────
        "undo.undo" => {
            Some(serde_json::json!({
                "can_undo": engine.can_undo(),
                "can_redo": engine.can_redo(),
                "undo_label": engine.undo_label(),
                "redo_label": engine.redo_label(),
                "note": "requires mutable engine to execute"
            }))
        }
        "undo.redo" => {
            Some(serde_json::json!({
                "can_undo": engine.can_undo(),
                "can_redo": engine.can_redo(),
                "note": "requires mutable engine to execute"
            }))
        }
        "undo.history" => {
            Some(serde_json::json!({
                "can_undo": engine.can_undo(),
                "can_redo": engine.can_redo(),
                "undo_label": engine.undo_label(),
                "redo_label": engine.redo_label(),
            }))
        }

        // ── Batch ──────────────────────────────────────────────────────
        "batch.execute" => {
            let commands: Vec<EditCommand> = params.as_ref()
                .and_then(|p| p.get("commands"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let count = commands.len();
            Some(serde_json::json!({
                "results": [],
                "command_count": count,
                "note": "batch requires mutable engine — use engine.apply_batch()"
            }))
        }

        // ── Query ──────────────────────────────────────────────────────
        "query.find_gaps" => {
            let track_id = params.as_ref()
                .and_then(|p| p.get("track_id")?.as_u64())
                .map(TrackId::from_u64)
                .unwrap_or_else(|| {
                    engine.project().timeline.tracks.first()
                        .map(|t| t.id)
                        .unwrap_or_else(TrackId::nil)
                });
            let gaps = engine.find_gaps(track_id);
            Some(serde_json::json!({"gaps": gaps, "track_id": track_id.0}))
        }
        "query.search_clips" => {
            let query = params.as_ref()
                .and_then(|p| p.get("query")?.as_str())
                .unwrap_or("");
            let min_dur = params.as_ref()
                .and_then(|p| p.get("min_duration_frames")?.as_i64());
            let matches: Vec<serde_json::Value> = engine.search_clips(query, min_dur)
                .iter()
                .map(|m| serde_json::json!({
                    "clip_id": m.clip_id,
                    "label": m.label,
                    "asset_id": m.asset_id,
                }))
                .collect();
            Some(serde_json::json!({"clips": matches, "query": query}))
        }
        "query.analyze_pacing" => {
            let track_id = params.as_ref()
                .and_then(|p| p.get("track_id")?.as_u64())
                .map(TrackId::from_u64);
            // Compute simple pacing: duration of each clip on the first video track
            let track = track_id
                .and_then(|tid| engine.project().timeline.track(tid))
                .or_else(|| engine.project().timeline.tracks.first());
            let rhythms: Vec<serde_json::Value> = track
                .map(|t| t.clips.iter().map(|c| serde_json::json!({
                    "clip_id": c.id,
                    "label": c.label,
                    "position": c.timeline_in,
                    "duration": c.duration(),
                    "speed": c.speed,
                })).collect())
                .unwrap_or_default();
            Some(serde_json::json!({"pacing": rhythms}))
        }

        // ── Unknown ────────────────────────────────────────────────────
        _ => {
            return Response::error(id, -32601, format!("Method not found: {method}"));
        }
    };

    match result {
        Some(data) => Response::result(id, data),
        None => Response::error(id, -32602, "Invalid params"),
    }
}
