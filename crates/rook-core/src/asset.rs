//! Assets — media files in the project pool.  Tagged-union design from
//! verbreel-state, augmented with AI annotation fields from anica.

use serde::{Deserialize, Serialize};

pub use crate::ids::AssetId;

/// Top-level asset enum — one variant per media type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Asset {
    Video(VideoAsset),
    Audio(AudioAsset),
    Image(ImageAsset),
    Subtitle(SubtitleAsset),
}

impl Asset {
    pub fn id(&self) -> AssetId {
        match self {
            Self::Video(a) => a.id,
            Self::Audio(a) => a.id,
            Self::Image(a) => a.id,
            Self::Subtitle(a) => a.id,
        }
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Video(a) => &a.path,
            Self::Audio(a) => &a.path,
            Self::Image(a) => &a.path,
            Self::Subtitle(a) => &a.path,
        }
    }

    /// Set the file path (for relinking moved media).
    pub fn set_path(&mut self, new_path: String) {
        match self {
            Self::Video(a) => a.path = new_path,
            Self::Audio(a) => a.path = new_path,
            Self::Image(a) => a.path = new_path,
            Self::Subtitle(a) => a.path = new_path,
        }
    }

    pub fn filename_stem(&self) -> &str {
        // Cheap: split on last '/', then on last '.'
        self.path().rsplit('/').next().unwrap_or("").rsplit('.').next().unwrap_or("")
    }

    pub fn metadata(&self) -> &AssetMetadata {
        match self {
            Self::Video(a) => &a.metadata,
            Self::Audio(a) => &a.metadata,
            Self::Image(a) => &a.metadata,
            Self::Subtitle(a) => &a.metadata,
        }
    }

    /// Mutable access to metadata (for AI annotation and updates).
    pub fn metadata_mut(&mut self) -> &mut AssetMetadata {
        match self {
            Self::Video(a) => &mut a.metadata,
            Self::Audio(a) => &mut a.metadata,
            Self::Image(a) => &mut a.metadata,
            Self::Subtitle(a) => &mut a.metadata,
        }
    }
}

/// Metadata shared by all asset types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetMetadata {
    /// Duration in frames (0 for images, subtitles may have intrinsic timing).
    #[serde(default)]
    pub duration_frames: Option<i64>,
    /// File size in bytes.
    #[serde(default)]
    pub file_size_bytes: Option<u64>,
    /// SHA-256 of the file (for caching / dedup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,

    // ── Video-specific ──────────────────────────────────────────────────
    #[serde(default)]
    pub video: Option<VideoMetadata>,
    // ── Audio-specific ──────────────────────────────────────────────────
    #[serde(default)]
    pub audio: Option<AudioMetadata>,
    // ── Image-specific ──────────────────────────────────────────────────
    #[serde(default)]
    pub image: Option<ImageMetadata>,

    // ── AI annotations (from anica) ─────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ai_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub fps: f64,
    pub bitrate_bps: u64,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoAsset {
    pub id: AssetId,
    pub path: String,
    #[serde(default)]
    pub metadata: AssetMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_path: Option<String>,
    /// Proxy generation status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_status: Option<ProxyStatus>,
    /// Media fingerprint for cache invalidation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAsset {
    pub id: AssetId,
    pub path: String,
    #[serde(default)]
    pub metadata: AssetMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAsset {
    pub id: AssetId,
    pub path: String,
    #[serde(default)]
    pub metadata: AssetMetadata,
    /// Duration in frames this image should appear on the timeline.
    #[serde(default = "default_image_duration")]
    pub default_duration_frames: i64,
}

fn default_image_duration() -> i64 { 120 } // 5 s at 24 fps

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleAsset {
    pub id: AssetId,
    pub path: String,
    #[serde(default)]
    pub metadata: AssetMetadata,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub size: u64,
    pub mtime_ns: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyStatus {
    Pending,
    Building { progress: f32 },
    Ready,
    Failed { reason: String },
}
