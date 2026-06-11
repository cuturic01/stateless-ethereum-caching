use crate::model::Key;
use crate::ordered::OrderedMap;

use super::{AccessOutcome, ReplacementPolicy};

pub struct Arc {
    c: usize,
    p: usize,
    t1: OrderedMap<()>,
    t2: OrderedMap<()>,
    b1: OrderedMap<()>,
    b2: OrderedMap<()>,
}

impl Arc {
    pub fn new(capacity: usize) -> Self {
        Arc {
            c: capacity.max(1),
            p: 0,
            t1: OrderedMap::new(),
            t2: OrderedMap::new(),
            b1: OrderedMap::new(),
            b2: OrderedMap::new(),
        }
    }

    pub fn target_p(&self) -> usize {
        self.p
    }

    pub fn list_sizes(&self) -> (usize, usize, usize, usize) {
        (self.t1.len(), self.t2.len(), self.b1.len(), self.b2.len())
    }

    fn replace(&mut self, x_in_b2: bool) -> Option<Key> {
        let t1_len = self.t1.len();
        if t1_len > 0 && (t1_len > self.p || (x_in_b2 && t1_len == self.p)) {
            // demote LRU of t1 -> MRU of b1
            let (k, _) = self.t1.pop_front()?;
            self.b1.push_back(k, ());
            Some(k)
        } else {
            // demote LRU of t2 -> MRU of b2
            let (k, _) = self.t2.pop_front()?;
            self.b2.push_back(k, ());
            Some(k)
        }
    }
}

impl ReplacementPolicy for Arc {
    fn access(&mut self, key: Key) -> AccessOutcome {
        if self.t1.remove(&key).is_some() || self.t2.remove(&key).is_some() {
            self.t2.push_back(key, ());
            return AccessOutcome { hit: true, evicted: None };
        }

        let in_b1 = self.b1.contains(&key);
        let in_b2 = self.b2.contains(&key);

        if in_b1 {
            let delta = (self.b2.len() / self.b1.len().max(1)).max(1);
            self.p = (self.p + delta).min(self.c);
        } else if in_b2 {
            let delta = (self.b1.len() / self.b2.len().max(1)).max(1);
            self.p = self.p.saturating_sub(delta);
        }

        let evicted = if self.t1.len() + self.t2.len() >= self.c {
            self.replace(in_b2)
        } else {
            None
        };

        if in_b1 {
            self.b1.remove(&key);
            self.t2.push_back(key, ());
        } else if in_b2 {
            self.b2.remove(&key);
            self.t2.push_back(key, ());
        } else {
            self.t1.push_back(key, ());
        }

        while self.t1.len() + self.b1.len() > self.c && !self.b1.is_empty() {
            self.b1.pop_front();
        }
        while self.t1.len() + self.t2.len() + self.b1.len() + self.b2.len() > 2 * self.c
            && !self.b2.is_empty()
        {
            self.b2.pop_front();
        }

        AccessOutcome { hit: false, evicted }
    }

    fn remove(&mut self, key: &Key) -> bool {
        let resident = self.t1.remove(key).is_some() || self.t2.remove(key).is_some();
        self.b1.remove(key);
        self.b2.remove(key);
        resident
    }

    fn len(&self) -> usize {
        self.t1.len() + self.t2.len()
    }
    fn capacity(&self) -> usize {
        self.c
    }
    fn name(&self) -> &'static str {
        "arc"
    }
    fn contains(&self, key: &Key) -> bool {
        self.t1.contains(key) || self.t2.contains(key)
    }
}
