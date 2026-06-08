use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MltError {
    #[error("MLT factory not initialized — call rook_mlt::init() first")]
    NotInitialized,

    #[error("failed to create producer from {0}")]
    ProducerCreationFailed(PathBuf),

    #[error("failed to create consumer")]
    ConsumerCreationFailed,

    #[error("failed to create filter '{0}'")]
    FilterCreationFailed(String),

    #[error("failed to create transition '{0}'")]
    TransitionCreationFailed(String),

    #[error("failed to create playlist")]
    PlaylistCreationFailed,

    #[error("failed to create tractor")]
    TractorCreationFailed,

    #[error("invalid profile: {0}")]
    InvalidProfile(String),

    #[error("MLT internal error: {0}")]
    Internal(String),

    #[error("{0}")]
    Generic(&'static str),
}
