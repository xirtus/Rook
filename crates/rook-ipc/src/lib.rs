//! # Rook IPC — Agent Interoperability Protocol.
//!
//! Three transport surfaces, one API:
//!
//! | Transport  | Use-case                          |
//! |------------|-----------------------------------|
//! | stdio      | Subprocess agent (spawn & talk)   |
//! | Unix socket| Local agent on same machine        |
//! | HTTP       | Remote agent or web dashboard      |
//! | MCP        | Model Context Protocol (Claude/etc)|
//!
//! ## API design (from Anica + Verbreel)
//!
//! Every method takes typed request → typed response.  The agent sees a
//! stable, versioned JSON-RPC 2.0 surface.  Events flow async from editor
//! → agent (playhead moved, project changed, export progress).
//!
//! ## Verb set
//!
//! ```text
//! project.{get, create, open, save, export}
//! gallery.{import, list, probe, tag, annotate}
//! timeline.{get, insert_clip, remove_clip, move_clip, trim_clip,
//!          split_clip, ripple_delete, add_track, remove_track,
//!          set_playhead, add_filter, set_keyframe}
//! preview.{get_frame, get_waveform}
//! undo.{undo, redo, history}
//! batch.execute
//! query.{search_clips, find_gaps, analyze_pacing}
//! ```

pub mod methods;
pub mod protocol;
pub mod server;
pub mod transport;
pub mod types;

pub use protocol::{Request, Response, Notification};
pub use server::IpcServer;
pub use transport::Transport;
pub use types::*;
