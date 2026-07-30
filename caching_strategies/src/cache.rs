use crate::metrics::RunMetrics;
use crate::model::BlockRecord;
use crate::policy::ReplacementPolicy;
use crate::window::RetentionWindow;
use crate::witness::{seal, BlockWitnessAccum};

pub struct MetricSink<'a> {
    pub metrics: &'a mut RunMetrics,
    pub active: bool,
}

pub struct WitnessCache {
    policy: Box<dyn ReplacementPolicy>,
    window: RetentionWindow,
}

impl WitnessCache {
    pub fn new(policy: Box<dyn ReplacementPolicy>, window_n: u64) -> Self {
        WitnessCache { policy, window: RetentionWindow::new(window_n) }
    }

    pub fn occupancy(&self) -> usize {
        self.policy.len()
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

        let mut wacc = BlockWitnessAccum::default();

        let mut invalidations: u32 = 0;
        for w in &rec.writes {
            wacc.record_leaf(w, false, true);
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
        }

        let mut hits: u32 = 0;
        let mut misses: u32 = 0;
        for r in &rec.reads {
            let out = self.policy.access(*r);
            wacc.record_leaf(r, out.hit, false);
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
        }

       let wb = seal(&wacc);
        let reads = rec.reads.len() as u32;
        let occ = self.occupancy() as u64;
        for s in sinks.iter_mut() {
            if s.active {
                s.metrics.record_block(now, reads, hits, misses, invalidations, occ, wb);
            }
        }
    }
}
