use crate::ids::{AssetId, ClipId, TrackId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("unknown media id: {0}")]
    UnknownMedia(AssetId),

    #[error("media {0} is still referenced by at least one clip")]
    MediaReferenced(AssetId),

    #[error("unknown track id: {0}")]
    UnknownTrack(TrackId),

    #[error("unknown clip id: {0}")]
    UnknownClip(ClipId),

    #[error("source range {start}..{end} exceeds media bounds (0..{bound})")]
    SourceOutOfBounds {
        start: i64,
        end: i64,
        bound: i64,
    },

    #[error("clip placement [{start}, {end}) overlaps existing clip at [{existing_start}, {existing_end}) on track {track}")]
    Overlap {
        track: TrackId,
        start: i64,
        end: i64,
        existing_start: i64,
        existing_end: i64,
    },

    #[error("track {0} is locked")]
    TrackLocked(TrackId),

    #[error("unexpected track kind: expected {expected:?}, got {got:?}")]
    TrackKindMismatch {
        expected: crate::track::TrackKind,
        got: crate::track::TrackKind,
    },

    #[error("invalid frame rate: {0:?}")]
    InvalidFrameRate(crate::time::Rational),

    #[error("{0}")]
    Generic(String),
}
