//! Frame resolution — from timeline frame → ordered layers.
//! Stub implementation; real version uses MLT tractor for resolution.

use crate::pool::MediaPool;
use rook_core::ClipId;
use rook_core::ids::TrackId;
use rook_core::project::Project;
use rook_decode::DecodedFrame;
use std::sync::Arc;

/// One fully-resolved layer of a frame.
#[derive(Debug, Clone)]
pub struct RenderedLayer {
    pub track: TrackId,
    pub clip: ClipId,
    pub content: RenderedContent,
}

#[derive(Debug, Clone)]
pub enum RenderedContent {
    Media(Arc<DecodedFrame>),
    /// Generated content (text, color, shape).
    Generated(GeneratorParams),
}

#[derive(Debug, Clone)]
pub struct GeneratorParams {
    pub kind: String,
    pub params: serde_json::Value,
}

/// Resolve the layer stack at `frame`.
/// Returns layers in back-to-front order (paint background first).
pub fn resolve_frame(project: &Project, pool: &MediaPool, frame: i64) -> Vec<RenderedLayer> {
    let mut layers = Vec::new();

    // Iterate tracks in compositing order (video bottom-to-top)
    for track in &project.timeline.tracks {
        if !track.visible {
            continue;
        }
        for clip in &track.clips {
            if clip.covers(frame) {
                let content = match clip.timeline_to_source(frame) {
                    Some(source_frame) => {
                        // Try to get decoded frame
                        pool.get_frame(clip.asset_id, source_frame)
                            .map(RenderedContent::Media)
                            .unwrap_or_else(|| {
                                RenderedContent::Generated(GeneratorParams {
                                    kind: "placeholder".into(),
                                    params: serde_json::json!({"frame": source_frame}),
                                })
                            })
                    }
                    None => continue,
                };
                layers.push(RenderedLayer {
                    track: track.id,
                    clip: clip.id,
                    content,
                });
            }
        }
    }

    layers
}
