//! Timeline markers — named bookmarks at specific frames.

use serde::{Deserialize, Serialize};

use crate::ids::MarkerId;

/// A marker on the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    pub label: String,
    pub frame: i64,
    /// Optional colour hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    /// Free-form notes attached to this marker.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

impl Marker {
    pub fn new(label: impl Into<String>, frame: i64) -> Self {
        Self {
            id: MarkerId::next(),
            label: label.into(),
            frame,
            color: None,
            notes: String::new(),
        }
    }
}
