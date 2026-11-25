//! Deformed glyph strokes kept between layouts. The editor lays the whole document out again on
//! every keystroke, and 字符变形 dominates that cost (M9). A deformed glyph depends only on the
//! glyph, its deformation stream (seed + char id), its vertical scale and whether it is CJK, so
//! a cached entry is exact for the same fonts and deformation settings.
//!
//! Contract: one cache per font set. The settings are fingerprinted and a change clears the cache;
//! the caller makes a new cache when the fonts change (glyphs are keyed by address).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

type Strokes = Arc<Vec<Vec<(f32, f32)>>>;

/// (glyph address, deformation stream id, vertical scale bits, CJK).
pub(super) type Key = (usize, u64, u32, bool);

/// Entries kept before the cache starts over (deleted text leaves stale entries behind).
const MAX_ENTRIES: usize = 200_000;

#[derive(Default)]
pub struct DeformCache {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for DeformCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeformCache({} entries)", self.len())
    }
}

#[derive(Default)]
struct Inner {
    fingerprint: u64,
    map: HashMap<Key, Strokes>,
}

impl DeformCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Entries cached now.
    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |i| i.map.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear the cache when the deformation settings (Chinese, English, digits) or the seed
    /// differ from the ones it was filled with.
    pub(super) fn begin(&self, p: &super::PageParams) {
        let mut h = DefaultHasher::new();
        format!("{:?}{:?}{:?}{:?}", p.deformation, p.style.english.deformation, p.style.number.deformation, p.style.cursive).hash(&mut h);
        p.style.seed.hash(&mut h);
        let fp = h.finish();
        if let Ok(mut i) = self.inner.lock()
            && i.fingerprint != fp
        {
            i.fingerprint = fp;
            i.map.clear();
        }
    }

    pub(super) fn get_or(&self, key: Key, make: impl FnOnce() -> Vec<Vec<(f32, f32)>>) -> Strokes {
        let Ok(mut i) = self.inner.lock() else { return Arc::new(make()) };
        if let Some(s) = i.map.get(&key) {
            return s.clone();
        }
        if i.map.len() >= MAX_ENTRIES {
            i.map.clear();
        }
        let s = Arc::new(make());
        i.map.insert(key, s.clone());
        s
    }
}
