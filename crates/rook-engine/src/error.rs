use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("model error: {0}")]
    Model(#[from] rook_core::ModelError),

    #[error("decode error: {0}")]
    Decode(#[from] rook_decode::DecodeError),

    #[error("MLT error: {0}")]
    Mlt(#[from] rook_mlt::MltError),

    #[error("timeline error: {0}")]
    Timeline(#[from] rook_timeline::TimelineError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unknown media id: {0}")]
    UnknownMedia(rook_core::AssetId),

    #[error("unknown clip id: {0}")]
    UnknownClip(rook_core::ClipId),

    #[error("export in progress")]
    ExportInProgress,

    #[error("export failed: {0}")]
    ExportFailed(String),

    #[error("{0}")]
    Generic(&'static str),
}
