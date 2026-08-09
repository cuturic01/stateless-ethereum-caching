use crate::hash::FastSet;
use crate::metrics::{BlockOutcome, RunMetrics};
use crate::model::{BlockRecord, Key};
use crate::policy::ReplacementPolicy;
use crate::window::RetentionWindow;
use crate::witness::{key_to_leaf, seal, BlockWitnessAccum, CacheFootprint, StemId};

pub struct MetricSink<'a> {
    pub metrics: &'a mut RunMetrics,
    pub active: bool,
}

pub struct WitnessCache {
    policy: Box<dyn ReplacementPolicy<Key>>,
    window: RetentionWindow<Key>,
    stem_policy: Box<dyn ReplacementPolicy<StemId>>,
    stem_window: RetentionWindow<StemId>,
    block_stems: Vec<StemId>,
    block_stems_seen: FastSet<StemId>,
    written_resident: FastSet<Key>,
}

impl WitnessCache {
    pub fn new(
        policy: Box<dyn ReplacementPolicy<Key>>,
        stem_policy: Box<dyn ReplacementPolicy<StemId>>,
        window_n: u64,
    ) -> Self {
        WitnessCache {
            policy,
            window: RetentionWindow::new(window_n),
            stem_policy,
            stem_window: RetentionWindow::new(window_n),
            block_stems: Vec::new(),
            block_stems_seen: FastSet::default(),
            written_resident: FastSet::default(),
        }
    }

    pub fn occupancy(&self) -> usize {
        self.policy.len()
    }

    pub fn stem_occupancy(&self) -> usize {
        self.stem_policy.len()
    }

    pub fn footprint(&self) -> CacheFootprint {
        CacheFootprint {
            leaf_entries: self.policy.len() as u64,
            stem_entries: self.stem_policy.len() as u64,
        }
    }

    pub fn process_counting(&mut self, rec: &BlockRecord, counting: bool, m: &mut RunMetrics) {
        let mut sinks = [MetricSink { metrics: m, active: counting }];
        self.process_block(rec, &mut sinks);
    }

    pub fn process_block(&mut self, rec: &BlockRecord, sinks: &mut [MetricSink<'_>]) {
        let now = rec.block_number;

        for (key, last_seen) in self.window.drain_expired(now) {
            self.policy.remove(&key);
            let age = now.saturating_sub(last_seen);
            for s in sinks.iter_mut() {
                if s.active {
                    s.metrics.record_survival(age);
                }
            }
        }

        for (id, last_seen) in self.stem_window.drain_expired(now) {
            self.stem_policy.remove(&id);
            let age = now.saturating_sub(last_seen);
            for s in sinks.iter_mut() {
                if s.active {
                    s.metrics.record_stem_survival(age);
                }
            }
        }

        let mut wacc = BlockWitnessAccum::default();

        self.written_resident.clear();
        for w in &rec.writes {
            if self.policy.contains(w) {
                self.written_resident.insert(*w);
            }
        }

        let mut invalidations: u32 = 0;
        let mut stem_invalidations: u32 = 0;
        for w in &rec.writes {
            wacc.record_leaf(w, self.written_resident.contains(w), true);
            if self.policy.remove(w) {
                let last_seen = self.window.remove(w).unwrap_or(now);
                let age = now.saturating_sub(last_seen);
                invalidations += 1;
                for s in sinks.iter_mut() {
                    if s.active {
                        s.metrics.record_survival(age);
                        s.metrics.record_invalidation(w.addr);
                    }
                }
            }
            let (stem, _) = key_to_leaf(w);
            if self.stem_policy.remove(&stem) {
                let last_seen = self.stem_window.remove(&stem).unwrap_or(now);
                let age = now.saturating_sub(last_seen);
                stem_invalidations += 1;
                for s in sinks.iter_mut() {
                    if s.active {
                        s.metrics.record_stem_survival(age);
                    }
                }
            }
        }

        self.block_stems.clear();
        self.block_stems_seen.clear();
        let mut hits: u32 = 0;
        let mut misses: u32 = 0;
        for r in &rec.reads {
            let out = self.policy.access(*r);
            // A key this block also writes was evicted above, so `out.hit` is
            // false even though the client held it on arrival; the snapshot is
            // the authority. The hit counter still tracks the cache's own view.
            wacc.record_leaf(r, out.hit || self.written_resident.contains(r), false);
            if let Some(ev) = out.evicted {
                let last_seen = self.window.remove(&ev).unwrap_or(now);
                let age = now.saturating_sub(last_seen);
                for s in sinks.iter_mut() {
                    if s.active {
                        s.metrics.record_survival(age);
                    }
                }
            }
            self.window.touch(*r, now);
            if out.hit {
                hits += 1;
            } else {
                misses += 1;
            }
            let stem = key_to_leaf(r).0;
            if self.block_stems_seen.insert(stem) {
                self.block_stems.push(stem);
            }
        }

        for i in 0..self.block_stems.len() {
            let id = self.block_stems[i];
            let out = self.stem_policy.access(id);
            wacc.record_stem(id, out.hit);
            if let Some(ev) = out.evicted {
                let last_seen = self.stem_window.remove(&ev).unwrap_or(now);
                let age = now.saturating_sub(last_seen);
                for s in sinks.iter_mut() {
                    if s.active {
                        s.metrics.record_stem_survival(age);
                    }
                }
            }
            self.stem_window.touch(id, now);
        }

        let wb = seal(&wacc);
        let reads = rec.reads.len() as u32;
        let occ = self.occupancy() as u64;
        let stem_occ = self.stem_occupancy() as u64;
        let fp = self.footprint();
        debug_assert!(
            fp.leaf_entries <= self.policy.capacity() as u64
                && fp.stem_entries <= self.stem_policy.capacity() as u64,
            "occupancy above capacity"
        );
        for s in sinks.iter_mut() {
            if s.active {
                s.metrics.record_block(BlockOutcome {
                    block_number: now,
                    reads,
                    hits,
                    misses,
                    invalidations,
                    stem_invalidations,
                    occupancy: occ,
                    stem_occupancy: stem_occ,
                    witness: wb,
                    footprint: fp,
                });
            }
        }
    }
}
