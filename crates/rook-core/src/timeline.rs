//! Timeline — the ordered collection of tracks.  From cutlass-models.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::clip::{Clip, SemanticClip};
use crate::ids::{ClipId, TrackId};
use crate::marker::Marker;
use crate::time::Rational;
use crate::track::{Track, TrackKind};

/// The timeline: tracks, markers, playhead, and selection state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    /// Frame rate this timeline runs at.
    pub frame_rate: Rational,
    /// All tracks, ordered bottom-to-top for compositing.
    /// Index 0 = background-most, last = foreground-most.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<Track>,
    /// Current playhead position in timeline frames.
    #[serde(default)]
    pub playhead: i64,
    /// In-point for range selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_point: Option<i64>,
    /// Out-point for range selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_point: Option<i64>,
    /// Markers placed on the timeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// AI-labelled semantic regions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_clips: Vec<SemanticClip>,
    /// Currently selected clip ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_clip_ids: Vec<ClipId>,

    /// Compound clip contents: compound_clip_id → nested tracks.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub compound_contents: HashMap<ClipId, Vec<Track>>,

    // ── Id counters ────────────────────────────────────────────────────
    #[serde(default)]
    pub next_clip_id: u64,
    #[serde(default)]
    pub next_track_id: u64,

    // ── O(1) lookup indices — not persisted, rebuilt after each mutation ──
    /// clip_id → track_id that owns it.
    #[serde(skip)]
    pub clip_track_index: HashMap<ClipId, TrackId>,
    /// track_id → index in `self.tracks`.
    #[serde(skip)]
    pub track_pos_index: HashMap<TrackId, usize>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            frame_rate: Rational::FPS_24,
            tracks: Vec::new(),
            playhead: 0,
            in_point: None,
            out_point: None,
            markers: Vec::new(),
            semantic_clips: Vec::new(),
            selected_clip_ids: Vec::new(),
            compound_contents: HashMap::new(),
            next_clip_id: 1,
            next_track_id: 1,
            clip_track_index: HashMap::new(),
            track_pos_index: HashMap::new(),
        }
    }
}

impl Timeline {
    pub fn new(frame_rate: Rational) -> Self {
        Self {
            frame_rate,
            tracks: Vec::new(),
            playhead: 0,
            in_point: None,
            out_point: None,
            markers: Vec::new(),
            semantic_clips: Vec::new(),
            selected_clip_ids: Vec::new(),
            compound_contents: HashMap::new(),
            next_clip_id: 1,
            next_track_id: 1,
            clip_track_index: HashMap::new(),
            track_pos_index: HashMap::new(),
        }
    }

    /// Rebuild the O(1) lookup indices from the current track/clip state.
    /// Call after any operation that adds, removes, or moves clips or tracks.
    pub fn rebuild_index(&mut self) {
        self.track_pos_index.clear();
        self.clip_track_index.clear();
        for (i, track) in self.tracks.iter().enumerate() {
            self.track_pos_index.insert(track.id, i);
            for clip in &track.clips {
                self.clip_track_index.insert(clip.id, track.id);
            }
        }
    }

    // ── Track access ────────────────────────────────────────────────────

    pub fn track(&self, track_id: TrackId) -> Option<&Track> {
        if let Some(&idx) = self.track_pos_index.get(&track_id) {
            if let Some(t) = self.tracks.get(idx) {
                if t.id == track_id { return Some(t); }
            }
        }
        self.tracks.iter().find(|t| t.id == track_id)
    }

    pub fn track_mut(&mut self, track_id: TrackId) -> Option<&mut Track> {
        if let Some(&idx) = self.track_pos_index.get(&track_id) {
            if self.tracks.get(idx).map(|t| t.id) == Some(track_id) {
                return self.tracks.get_mut(idx);
            }
        }
        self.tracks.iter_mut().find(|t| t.id == track_id)
    }

    pub fn track_index(&self, track_id: TrackId) -> Option<usize> {
        if let Some(&idx) = self.track_pos_index.get(&track_id) {
            if self.tracks.get(idx).map(|t| t.id) == Some(track_id) {
                return Some(idx);
            }
        }
        self.tracks.iter().position(|t| t.id == track_id)
    }

    pub fn tracks_of_kind(&self, kind: TrackKind) -> Vec<&Track> {
        self.tracks.iter().filter(|t| t.kind == kind).collect()
    }

    pub fn add_track(&mut self, track: Track) -> TrackId {
        let id = track.id;
        self.tracks.push(track);
        id
    }

    pub fn remove_track(&mut self, track_id: TrackId) -> Option<Track> {
        self.track_index(track_id).map(|i| self.tracks.remove(i))
    }

    pub fn tracks_ordered(&self) -> impl Iterator<Item = &Track> + '_ {
        // Video tracks first (bottom-to-top), then audio, then text, then effect
        let mut indices: Vec<(usize, TrackKind)> = self.tracks.iter()
            .enumerate()
            .map(|(i, t)| (i, t.kind))
            .collect();
        indices.sort_by_key(|(_, k)| track_order(*k));
        indices.into_iter().filter_map(move |(i, _)| self.tracks.get(i))
    }

    // ── Clip access ─────────────────────────────────────────────────────

    pub fn clip(&self, clip_id: ClipId) -> Option<&Clip> {
        if let Some(&track_id) = self.clip_track_index.get(&clip_id) {
            if let Some(track) = self.track(track_id) {
                return track.clip(clip_id);
            }
        }
        self.tracks.iter().find_map(|t| t.clip(clip_id))
    }

    pub fn clip_mut(&mut self, clip_id: ClipId) -> Option<&mut Clip> {
        if let Some(&track_id) = self.clip_track_index.get(&clip_id) {
            if let Some(&idx) = self.track_pos_index.get(&track_id) {
                if self.tracks.get(idx).map(|t| t.id) == Some(track_id) {
                    return self.tracks.get_mut(idx)?.clip_mut(clip_id);
                }
            }
        }
        self.tracks.iter_mut().find_map(|t| t.clip_mut(clip_id))
    }

    pub fn clip_track_id(&self, clip_id: ClipId) -> Option<TrackId> {
        if let Some(&track_id) = self.clip_track_index.get(&clip_id) {
            return Some(track_id);
        }
        self.tracks.iter()
            .find(|t| t.clip(clip_id).is_some())
            .map(|t| t.id)
    }

    /// Find gaps — empty timeline ranges between clips on a track.
    pub fn find_gaps(&self, track_id: TrackId) -> Vec<(i64, i64)> {
        let Some(track) = self.track(track_id) else { return vec![]; };
        let mut gaps = Vec::new();
        let mut cursor: i64 = 0;
        for clip in &track.clips {
            if clip.timeline_in > cursor {
                gaps.push((cursor, clip.timeline_in));
            }
            cursor = clip.timeline_in + clip.duration();
        }
        gaps
    }

    // ── Duration ────────────────────────────────────────────────────────

    /// Total timeline duration in frames (end of the latest clip).
    pub fn duration(&self) -> i64 {
        self.tracks.iter()
            .filter_map(|t| t.clips.last().map(|c| c.timeline_in + c.duration()))
            .max()
            .unwrap_or(0)
    }

    // ── Selection ───────────────────────────────────────────────────────

    pub fn select(&mut self, clip_id: ClipId) {
        self.selected_clip_ids = vec![clip_id];
    }

    pub fn toggle_select(&mut self, clip_id: ClipId) {
        if let Some(pos) = self.selected_clip_ids.iter().position(|&id| id == clip_id) {
            self.selected_clip_ids.remove(pos);
        } else {
            self.selected_clip_ids.push(clip_id);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_clip_ids.clear();
    }
}

fn track_order(kind: TrackKind) -> u8 {
    match kind {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
        TrackKind::Text => 2,
        TrackKind::Effect => 3,
    }
}
