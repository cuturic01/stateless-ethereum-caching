use crate::hash::FastMap;
use crate::model::Key;
use crate::ordered::OrderedMap;

use super::{AccessOutcome, ReplacementPolicy};

pub struct Lfu {
    capacity: usize,
    freq: FastMap<Key, u64>,
    buckets: FastMap<u64, OrderedMap<()>>,
    min_freq: u64,
}

impl Lfu {
    pub fn new(capacity: usize) -> Self {
        Lfu {
            capacity,
            freq: FastMap::default(),
            buckets: FastMap::default(),
            min_freq: 0,
        }
    }

    fn detach(&mut self, key: &Key, f: u64) {
        if let Some(b) = self.buckets.get_mut(&f) {
            b.remove(key);
            if b.is_empty() {
                self.buckets.remove(&f);
            }
        }
    }

    fn fix_min_freq(&mut self) {
        if self.freq.is_empty() {
            return;
        }
        while !self.buckets.contains_key(&self.min_freq) {
            self.min_freq += 1;
        }
    }
}

impl ReplacementPolicy for Lfu {
    fn access(&mut self, key: Key) -> AccessOutcome {
        if let Some(&f) = self.freq.get(&key) {
            self.detach(&key, f);
            let nf = f + 1;
            self.freq.insert(key, nf);
            self.buckets.entry(nf).or_default().push_back(key, ());
            if self.min_freq == f && !self.buckets.contains_key(&f) {
                self.min_freq = nf;
            }
            return AccessOutcome { hit: true, evicted: None };
        }

        let mut evicted = None;
        if self.freq.len() >= self.capacity {
            self.fix_min_freq();
            if let Some(victim_bucket) = self.buckets.get_mut(&self.min_freq) {
                if let Some((victim, _)) = victim_bucket.pop_front() {
                    if victim_bucket.is_empty() {
                        self.buckets.remove(&self.min_freq);
                    }
                    self.freq.remove(&victim);
                    evicted = Some(victim);
                }
            }
        }
        self.freq.insert(key, 1);
        self.buckets.entry(1).or_default().push_back(key, ());
        self.min_freq = 1;
        AccessOutcome { hit: false, evicted }
    }

    fn remove(&mut self, key: &Key) -> bool {
        if let Some(f) = self.freq.remove(key) {
            self.detach(key, f);
            true
        } else {
            false
        }
    }

    fn len(&self) -> usize {
        self.freq.len()
    }
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn name(&self) -> &'static str {
        "lfu"
    }
    fn contains(&self, key: &Key) -> bool {
        self.freq.contains_key(key)
    }
}
