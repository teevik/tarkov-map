//! Budgeted texture residency tracking.
//!
//! Keeps the set of retained map textures within an explicit byte budget,
//! measured in decoded pixel cost (width × height × 4), not compressed file
//! size. The policy mirrors handle-style asset servers: a texture referenced
//! during the current frame is resident and never evicted; everything else is
//! evicted least-recently-used-first whenever the total exceeds the budget.

use std::collections::HashMap;

struct Entry {
    bytes: usize,
    last_used: u64,
}

/// Tracks retained textures by path, evicting least-recently-used entries
/// once the total decoded byte cost exceeds the budget.
///
/// Driven by the frame loop: [`begin_frame`] advances the clock, uploads
/// [`insert`] and the render pass [`touch`]es the texture it draws, then
/// [`take_evictions`] runs at the end of the frame — so the budget holds
/// within the frame and whatever was just displayed is protected.
///
/// [`take_evictions`]: TextureResidency::take_evictions
/// [`begin_frame`]: TextureResidency::begin_frame
/// [`insert`]: TextureResidency::insert
/// [`touch`]: TextureResidency::touch
pub struct TextureResidency {
    budget_bytes: usize,
    tick: u64,
    entries: HashMap<String, Entry>,
}

impl TextureResidency {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    /// Advances the frame clock. Entries not touched again before the next
    /// [`take_evictions`] call become eligible for eviction.
    ///
    /// [`take_evictions`]: TextureResidency::take_evictions
    pub fn begin_frame(&mut self) {
        self.tick += 1;
    }

    /// Marks `path` as displayed this frame, protecting it from eviction and
    /// refreshing its recency. A path not being tracked is ignored.
    pub fn touch(&mut self, path: &str) {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.last_used = self.tick;
        }
    }

    /// Starts tracking `path` at the given decoded byte cost, marked as used
    /// this frame. Re-inserting an evicted or existing path replaces its cost.
    pub fn insert(&mut self, path: &str, bytes: usize) {
        self.entries.insert(
            path.to_string(),
            Entry {
                bytes,
                last_used: self.tick,
            },
        );
    }

    /// Total decoded byte cost of all tracked textures.
    pub fn total_bytes(&self) -> usize {
        self.entries.values().map(|entry| entry.bytes).sum()
    }

    /// Evicts least-recently-used entries until the total is within budget,
    /// returning the evicted paths so the caller can drop the matching
    /// textures. Entries used at the current tick (i.e. displayed this frame)
    /// are never evicted, even if they alone exceed the budget.
    pub fn take_evictions(&mut self) -> Vec<String> {
        let mut evicted = Vec::new();

        while self.total_bytes() > self.budget_bytes {
            // Oldest first; ties broken by path for deterministic behavior.
            let candidate = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.last_used < self.tick)
                .min_by(|(path_a, a), (path_b, b)| {
                    a.last_used.cmp(&b.last_used).then(path_a.cmp(path_b))
                })
                .map(|(path, _)| path.clone());

            let Some(path) = candidate else {
                break; // Everything left is displayed this frame.
            };
            self.entries.remove(&path);
            evicted.push(path);
        }

        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: usize = 1024 * 1024;

    #[test]
    fn under_budget_retains_everything() {
        let mut residency = TextureResidency::new(3 * MB);
        residency.insert("a", MB);
        residency.insert("b", MB);

        residency.begin_frame();

        assert!(residency.take_evictions().is_empty());
        assert_eq!(residency.total_bytes(), 2 * MB);
    }

    #[test]
    fn over_budget_evicts_least_recently_used_first() {
        let mut residency = TextureResidency::new(2 * MB);
        residency.insert("oldest", MB);
        residency.begin_frame();
        residency.insert("middle", MB);
        residency.begin_frame();
        residency.insert("newest", MB);
        residency.begin_frame();

        assert_eq!(residency.take_evictions(), vec!["oldest".to_string()]);
        assert_eq!(residency.total_bytes(), 2 * MB);
    }

    #[test]
    fn texture_displayed_this_frame_is_never_evicted() {
        let mut residency = TextureResidency::new(MB);
        residency.insert("active", 4 * MB); // alone exceeds the budget
        residency.begin_frame();
        residency.touch("active"); // displayed this frame

        assert!(residency.take_evictions().is_empty());
        assert_eq!(residency.total_bytes(), 4 * MB);
    }

    #[test]
    fn touch_refreshes_recency() {
        let mut residency = TextureResidency::new(2 * MB);
        residency.insert("first", MB);
        residency.begin_frame();
        residency.insert("second", MB);
        residency.begin_frame();
        residency.touch("first"); // "first" is now more recent than "second"
        residency.begin_frame();
        residency.insert("third", MB);
        residency.begin_frame();

        assert_eq!(residency.take_evictions(), vec!["second".to_string()]);
    }

    #[test]
    fn evicted_texture_can_be_reinserted() {
        let mut residency = TextureResidency::new(MB);
        residency.insert("a", MB);
        residency.begin_frame();
        residency.insert("b", MB);
        residency.begin_frame();
        assert_eq!(residency.take_evictions(), vec!["a".to_string()]);

        residency.insert("a", MB); // reload after eviction
        assert_eq!(residency.total_bytes(), 2 * MB);
        residency.begin_frame();
        residency.touch("a");
        assert_eq!(residency.take_evictions(), vec!["b".to_string()]);
    }

    #[test]
    fn touching_an_untracked_path_is_ignored() {
        let mut residency = TextureResidency::new(MB);
        residency.touch("never-inserted");
        assert_eq!(residency.total_bytes(), 0);
        assert!(residency.take_evictions().is_empty());
    }

    /// Budget-boundary check: cycles through every bundled map and floor
    /// twice via the real cache + residency pipeline, asserting memory
    /// plateaus at the budget and evicted images reload when revisited.
    #[test]
    fn cycling_every_map_and_floor_plateaus_at_the_budget() {
        use crate::assets::AssetCache;
        use crate::constants::TEXTURE_BUDGET_BYTES;
        use std::sync::mpsc;

        // Worst-case decoded cost of the largest bundled images (4096² RGBA).
        const IMAGE_BYTES: usize = 64 * MB;

        let maps = crate::assets::load_maps().expect("map fixtures should load");
        let paths: Vec<String> = maps
            .iter()
            .flat_map(|map| {
                std::iter::once(map.image_path.clone()).chain(
                    map.layers
                        .iter()
                        .flatten()
                        .filter_map(|layer| layer.image_path.clone()),
                )
            })
            .collect();
        assert!(
            paths.len() * IMAGE_BYTES > TEXTURE_BUDGET_BYTES,
            "fixture set must exceed the budget for this test to mean anything"
        );

        let mut cache = AssetCache::new();
        let mut residency = TextureResidency::new(TEXTURE_BUDGET_BYTES);
        let mut peak = 0;

        // Two passes: the second revisits paths evicted during the first.
        for pass in 0..2 {
            for path in &paths {
                // One simulated frame per selected image, mirroring the app's
                // frame order: advance clock, load/upload, display (touch),
                // then evict over budget at the end of the frame.
                residency.begin_frame();

                let started = cache.request(path, || {
                    let (tx, rx) = mpsc::channel();
                    tx.send(Ok(crate::assets::DecodedImage {
                        pixels: vec![0; 4],
                        width: 1,
                        height: 1,
                    }))
                    .unwrap();
                    rx
                });
                if started {
                    cache.poll();
                    let taken = cache.take_decoded(path);
                    assert!(taken.is_some(), "pass {pass}: {path} should decode");
                    residency.insert(path, IMAGE_BYTES);
                }
                residency.touch(path);

                for path in residency.take_evictions() {
                    assert!(cache.evict(&path), "evicted textures leave the cache");
                }

                peak = peak.max(residency.total_bytes());
                assert!(
                    residency.total_bytes() <= TEXTURE_BUDGET_BYTES,
                    "pass {pass}: retained bytes exceeded the budget at {path}"
                );
            }
        }

        assert!(
            peak > TEXTURE_BUDGET_BYTES - IMAGE_BYTES,
            "the cache should fill up to the budget, not stay far below it \
             (peak was {peak} of {TEXTURE_BUDGET_BYTES})"
        );
    }
}
