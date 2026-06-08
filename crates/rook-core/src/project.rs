//! Project — the aggregate root.  From cutlass-models.

use serde::{Deserialize, Serialize};

use crate::asset::{Asset, AssetId};
use crate::canvas::Canvas;
use crate::ids::ProjectId;
use crate::ids::PluginId;
use crate::multicam::MulticamClip;
use crate::plugin::PluginManifest;
use crate::time::Rational;
use crate::timeline::Timeline;
use crate::track::TrackKind;

/// Top-level project: media pool + timeline + metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// Version of the project format (for migrations).
    #[serde(default = "default_version")]
    pub version: u32,
    pub canvas: Canvas,
    pub frame_rate: Rational,
    pub sample_rate: u32,
    /// Audio channels (1 = mono, 2 = stereo, 6 = 5.1).
    #[serde(default = "default_channels")]
    pub audio_channels: u8,
    /// Assets in the media pool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<Asset>,
    /// The timeline.
    #[serde(default)]
    pub timeline: Timeline,
    /// Timestamp when the project was created (unix seconds).
    #[serde(default)]
    pub created_at: i64,
    /// Timestamp of last save (unix seconds).
    #[serde(default)]
    pub updated_at: i64,
    /// Multicam clips stored in this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub multicam_clips: Vec<MulticamClip>,
    /// Plugin manifests registered with this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginManifest>,
}

fn default_version() -> u32 { 1 }
fn default_channels() -> u8 { 2 }

impl Project {
    pub fn new(name: impl Into<String>, canvas: Canvas, frame_rate: Rational) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        Self {
            id: ProjectId::next(),
            name: name.into(),
            version: default_version(),
            canvas,
            frame_rate,
            sample_rate: 48000,
            audio_channels: 2,
            assets: Vec::new(),
            timeline: Timeline::new(frame_rate),
            created_at: now,
            updated_at: now,
            multicam_clips: Vec::new(),
            plugins: Vec::new(),
        }
    }

    // ── Asset pool ──────────────────────────────────────────────────────

    pub fn add_asset(&mut self, asset: Asset) -> &Asset {
        self.assets.push(asset);
        self.assets.last().unwrap()
    }

    pub fn asset(&self, id: AssetId) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id() == id)
    }

    pub fn asset_mut(&mut self, id: AssetId) -> Option<&mut Asset> {
        self.assets.iter_mut().find(|a| a.id() == id)
    }

    pub fn remove_asset(&mut self, id: AssetId) -> Result<Asset, crate::error::ModelError> {
        // Check no clip references this asset
        let referenced = self.timeline.tracks.iter()
            .flat_map(|t| &t.clips)
            .any(|c| c.asset_id == id);
        if referenced {
            return Err(crate::error::ModelError::MediaReferenced(id));
        }
        let idx = self.assets.iter().position(|a| a.id() == id)
            .ok_or(crate::error::ModelError::UnknownMedia(id))?;
        Ok(self.assets.remove(idx))
    }

    // ── Track helpers ───────────────────────────────────────────────────

    pub fn add_video_track(&mut self, name: impl Into<String>) -> crate::ids::TrackId {
        let idx = self.timeline.tracks_of_kind(TrackKind::Video).len();
        let track = crate::track::Track::new(TrackKind::Video, name, idx);
        self.timeline.add_track(track)
    }

    pub fn add_audio_track(&mut self, name: impl Into<String>) -> crate::ids::TrackId {
        let idx = self.timeline.tracks_of_kind(TrackKind::Audio).len();
        let track = crate::track::Track::new(TrackKind::Audio, name, idx);
        self.timeline.add_track(track)
    }

    pub fn add_text_track(&mut self, name: impl Into<String>) -> crate::ids::TrackId {
        let idx = self.timeline.tracks_of_kind(TrackKind::Text).len();
        let track = crate::track::Track::new(TrackKind::Text, name, idx);
        self.timeline.add_track(track)
    }

    // ── Multicam helpers ────────────────────────────────────────────────

    /// Find a multicam clip by the timeline clip id.
    pub fn multicam_for_clip(&self, clip_id: crate::clip::ClipId) -> Option<&MulticamClip> {
        self.multicam_clips.iter().find(|mc| mc.clip_id == clip_id)
    }

    /// Find a multicam clip by the timeline clip id (mutable).
    pub fn multicam_for_clip_mut(&mut self, clip_id: crate::clip::ClipId) -> Option<&mut MulticamClip> {
        self.multicam_clips.iter_mut().find(|mc| mc.clip_id == clip_id)
    }

    /// Add a multicam clip.
    pub fn add_multicam(&mut self, mc: MulticamClip) {
        self.multicam_clips.push(mc);
    }

    /// Remove a multicam clip.
    pub fn remove_multicam(&mut self, clip_id: crate::clip::ClipId) -> Option<MulticamClip> {
        if let Some(idx) = self.multicam_clips.iter().position(|mc| mc.clip_id == clip_id) {
            Some(self.multicam_clips.remove(idx))
        } else {
            None
        }
    }

    // ── Plugin registry ─────────────────────────────────────────────────

    pub fn add_plugin(&mut self, manifest: PluginManifest) {
        self.plugins.retain(|p| p.id != manifest.id);
        self.plugins.push(manifest);
    }

    pub fn remove_plugin(&mut self, id: PluginId) {
        self.plugins.retain(|p| p.id != id);
    }

    pub fn plugin(&self, id: PluginId) -> Option<&PluginManifest> {
        self.plugins.iter().find(|p| p.id == id)
    }

    pub fn plugin_mut(&mut self, id: PluginId) -> Option<&mut PluginManifest> {
        self.plugins.iter_mut().find(|p| p.id == id)
    }

    // ── Snapshots ───────────────────────────────────────────────────────

    pub fn to_snapshot(&self) -> crate::ProjectSnapshot {
        let clips: Vec<_> = self.timeline.tracks.iter()
            .flat_map(|t| t.clips.iter().map(|c| {
                let asset = self.asset(c.asset_id);
                crate::TrackClipView {
                    clip_id: c.id,
                    label: c.label.clone(),
                    file_path: asset.map(|a| a.path().to_string()).unwrap_or_default(),
                    timeline_in_frames: c.timeline_in,
                    duration_frames: c.duration(),
                    source_in_frames: c.source_in,
                    media_duration_frames: asset.and_then(|a| a.metadata().duration_frames).unwrap_or(0),
                    track_id: t.id,
                    link_group_id: c.link_group_id,
                    speed: c.speed,
                    filters: c.filters.clone(),
                }
            }))
            .collect();

        crate::ProjectSnapshot {
            project_id: self.id,
            name: self.name.clone(),
            canvas: self.canvas.clone(),
            fps: self.frame_rate,
            sample_rate: self.sample_rate,
            duration_frames: self.timeline.duration(),
            tracks: self.timeline.tracks.iter().map(|t| t.to_snapshot(&clips)).collect(),
            markers: self.timeline.markers.clone(),
            assets: self.assets.clone(),
            proxy_dir: String::new(), // set by engine
        }
    }
}
