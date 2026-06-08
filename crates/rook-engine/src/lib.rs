//! # Rook Engine — headless editing session.
//!
//! [`Engine`] is the single owner of a [`Project`] and the bridge to MLT.
//! Every mutation flows through [`Engine::apply`], which validates the
//! command against the model, records undo state, and (when MLT is live)
//! mirrors the edit into the MLT tractor so the timeline is always in sync.
//!
//! ## Architecture (from cutlass-engines)
//!
//! ```text
//! UI / Agent ──EditCommand──► Engine::apply()
//!                                │
//!                    ┌───────────┴───────────┐
//!                    │  Validate (no overlap, │
//!                    │  track exists, etc.)   │
//!                    └───────────┬───────────┘
//!                                │
//!                    ┌───────────┴───────────┐
//!                    │  Record undo snapshot  │
//!                    └───────────┬───────────┘
//!                                │
//!                    ┌───────────┴───────────┐
//!                    │  Mutate Project model  │
//!                    └───────────┬───────────┘
//!                                │
//!                    ┌───────────┴───────────┐
//!                    │  Mirror to MLT tractor │
//!                    │  (if MLT is live)      │
//!                    └───────────────────────┘
//! ```

mod cache;
mod crash;
mod engine;
mod error;
mod pool;
mod proxy;
mod resolve;

pub use cache::{CacheConfig, CacheStats, FrameCache};
pub use crash::{CrashReport, CrashReporter, has_pending_crash, recover_last_crash};
pub use engine::{ClipMatch, Engine, build_graph_from_project};
pub use error::EngineError;
pub use pool::{MediaPool, PoolConfig};
pub use proxy::{ProxyService, ProxyStatus};
pub use resolve::{RenderedContent, RenderedLayer};
