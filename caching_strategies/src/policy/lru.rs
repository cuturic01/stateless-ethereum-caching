use crate::ordered::OrderedMap;

use super::{AccessOutcome, CacheKey, ReplacementPolicy};

pub struct Lru<K> {
    order: OrderedMap<K, ()>,
    capacity: usize,
}

impl<K: CacheKey> Lru<K> {
    pub fn new(capacity: usize) -> Self {
        Lru { order: OrderedMap::new(), capacity }
    }
}

impl<K: CacheKey> ReplacementPolicy<K> for Lru<K> {
    fn access(&mut self, key: K) -> AccessOutcome<K> {
        if self.order.touch_back(&key, ()) {
            return AccessOutcome { hit: true, evicted: None };
        }
        let evicted = if self.order.len() >= self.capacity {
            self.order.pop_front().map(|(k, _)| k)
        } else {
            None
        };
        self.order.push_back(key, ());
        AccessOutcome { hit: false, evicted }
    }

    fn remove(&mut self, key: &K) -> bool {
        self.order.remove(key).is_some()
    }

    fn len(&self) -> usize {
        self.order.len()
    }
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn name(&self) -> &'static str {
        "lru"
    }
    fn contains(&self, key: &K) -> bool {
        self.order.contains(key)
    }
}
