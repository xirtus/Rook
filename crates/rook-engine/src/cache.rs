//! LRU frame cache — byte-aware eviction for low-memory resilience.

use parking_lot::Mutex;
use rook_core::AssetId;
use rook_decode::DecodedFrame;
use std::collections::HashMap;
use std::sync::Arc;

/// Stats for monitoring cache performance.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// Current total bytes of cached frame data (pixel buffers).
    pub total_bytes: u64,
    pub entry_count: usize,
}

/// Configuration for the frame cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of cached frame entries.
    pub max_entries: usize,
    /// Maximum total bytes of pixel data before LRU eviction kicks in.
    /// 0 = unlimited (only `max_entries` governs).
    pub max_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 512,
            max_bytes: 512 * 1024 * 1024, // 512 MB
        }
    }
}

impl CacheConfig {
    /// A reduced-footprint config for low-memory / proxy workflows.
    pub fn low_memory() -> Self {
        Self {
            max_entries: 128,
            max_bytes: 128 * 1024 * 1024, // 128 MB
        }
    }
}

/// An LRU cache of decoded frames, keyed by `(asset_id, frame_number)`.
///
/// Shared behind an `Arc` so the UI (reader) and decode threads (writer)
/// can share without contention on the hot path.
///
/// Eviction happens when *either* `max_entries` or `max_bytes` is exceeded —
/// oldest frames are dropped first.
pub struct FrameCache {
    inner: Mutex<FrameCacheInner>,
}

struct FrameCacheInner {
    map: HashMap<(AssetId, i64), Arc<DecodedFrame>>,
    lru: Vec<(AssetId, i64)>,
    stats: CacheStats,
    max_entries: usize,
    max_bytes: u64,
}

impl FrameCache {
    /// Create a cache with the given config.
    pub fn new(config: CacheConfig) -> Self {
        Self {
            inner: Mutex::new(FrameCacheInner {
                map: HashMap::with_capacity(config.max_entries),
                lru: Vec::with_capacity(config.max_entries),
                stats: CacheStats {
                    total_bytes: 0,
                    ..Default::default()
                },
                max_entries: config.max_entries,
                max_bytes: config.max_bytes,
            }),
        }
    }

    /// Change the budget at runtime (e.g. when toggling low-memory mode).
    /// Any entries over the new budget will be evicted immediately.
    pub fn resize(&self, config: CacheConfig) {
        let mut inner = self.inner.lock();
        inner.max_entries = config.max_entries;
        inner.max_bytes = config.max_bytes;
        let max_entries = inner.max_entries;
        let max_bytes = inner.max_bytes;
        Self::evict_to_budget(&mut inner, max_entries, max_bytes);
    }

    pub fn get(&self, asset: AssetId, frame: i64) -> Option<Arc<DecodedFrame>> {
        let mut inner = self.inner.lock();
        let key = (asset, frame);
        if let Some(f) = inner.map.get(&key) {
            let result = f.clone();
            inner.stats.hits += 1;
            // Touch: move to back of LRU
            if let Some(pos) = inner.lru.iter().position(|k| *k == key) {
                inner.lru.remove(pos);
                inner.lru.push(key);
            }
            Some(result)
        } else {
            inner.stats.misses += 1;
            None
        }
    }

    pub fn insert(&self, asset: AssetId, frame: i64, data: Arc<DecodedFrame>) {
        let mut inner = self.inner.lock();
        let key = (asset, frame);

        // Add byte cost of the new frame
        let frame_bytes = data.data.len() as u64;
        let max_entries = inner.max_entries;
        let max_bytes = inner.max_bytes;

        // Evict oldest until there's room for the new entry in both dimensions
        Self::ensure_room(&mut inner, max_entries, max_bytes, frame_bytes);

        inner.map.insert(key, data);
        inner.lru.push(key);
        inner.stats.total_bytes += frame_bytes;
        inner.stats.entry_count = inner.map.len();
    }

    /// Evict oldest entries until the cache is within budget, accounting for
    /// a pending `incoming_bytes` that will be added.
    fn ensure_room(
        inner: &mut FrameCacheInner,
        max_entries: usize,
        max_bytes: u64,
        incoming_bytes: u64,
    ) {
        // Evict if entry count will exceed
        while inner.map.len() >= max_entries && inner.map.len() > 0 {
            if let Some(oldest) = inner.lru.first().copied() {
                Self::evict_one(inner, &oldest);
            } else {
                break;
            }
        }
        // Evict if byte budget will be exceeded
        if max_bytes > 0 {
            while inner.stats.total_bytes + incoming_bytes > max_bytes && inner.map.len() > 0 {
                if let Some(oldest) = inner.lru.first().copied() {
                    Self::evict_one(inner, &oldest);
                } else {
                    break;
                }
            }
        }
    }

    /// Evict entries until within the given budgets (used after resize).
    fn evict_to_budget(inner: &mut FrameCacheInner, max_entries: usize, max_bytes: u64) {
        while inner.map.len() > max_entries {
            if let Some(oldest) = inner.lru.first().copied() {
                Self::evict_one(inner, &oldest);
            } else {
                break;
            }
        }
        if max_bytes > 0 {
            while inner.stats.total_bytes > max_bytes && inner.map.len() > 0 {
                if let Some(oldest) = inner.lru.first().copied() {
                    Self::evict_one(inner, &oldest);
                } else {
                    break;
                }
            }
        }
    }

    fn evict_one(inner: &mut FrameCacheInner, key: &(AssetId, i64)) {
        if let Some(entry) = inner.map.remove(key) {
            inner.stats.total_bytes -= entry.data.len() as u64;
            inner.stats.evictions += 1;
        }
        if inner.lru.first() == Some(key) {
            inner.lru.remove(0);
        } else if let Some(pos) = inner.lru.iter().position(|k| k == key) {
            inner.lru.remove(pos);
        }
        inner.stats.entry_count = inner.map.len();
    }

    /// Remove all entries belonging to an asset.
    pub fn evict_asset(&self, asset: AssetId) {
        let mut inner = self.inner.lock();
        let keys: Vec<_> = inner
            .map
            .keys()
            .filter(|(a, _)| *a == asset)
            .cloned()
            .collect();
        for key in keys {
            Self::evict_one(&mut inner, &key);
        }
    }

    pub fn stats(&self) -> CacheStats {
        self.inner.lock().stats.clone()
    }
}
