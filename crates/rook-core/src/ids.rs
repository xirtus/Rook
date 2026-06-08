//! Strongly-typed identifiers — no raw u64 confusion.
//!
//! Every entity gets its own newtype so the compiler catches clip-id / track-id
//! mix-ups at compile time.  Default-constructed ids use UUID v7 (time-ordered)
//! so database / log ordering is roughly chronological.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_newtype {
    ($name:ident, $display:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl $name {
            pub fn next() -> Self {
                Self(uuid::Uuid::now_v7().as_u64_pair().0)
            }

            pub fn from_u64(v: u64) -> Self {
                Self(v)
            }

            pub fn nil() -> Self {
                Self(0)
            }

            pub fn is_nil(&self) -> bool {
                self.0 == 0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($display, "({:016x})"), self.0)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::nil()
            }
        }
    };
}

id_newtype!(ProjectId, "project");
id_newtype!(TrackId, "track");
id_newtype!(ClipId, "clip");
id_newtype!(AssetId, "asset");
id_newtype!(MarkerId, "marker");
id_newtype!(EffectId, "effect");
id_newtype!(KeyframeId, "keyframe");
id_newtype!(AngleId, "angle");
id_newtype!(MulticamId, "multicam");
id_newtype!(PluginId, "plugin");
