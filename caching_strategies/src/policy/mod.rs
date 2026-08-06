use std::hash::Hash;

pub mod arc;
pub mod lfu;
pub mod lru;

pub trait CacheKey: Copy + Eq + Hash + Send + Sync + 'static {}
impl<T: Copy + Eq + Hash + Send + Sync + 'static> CacheKey for T {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessOutcome<K> {
    pub hit: bool,
    pub evicted: Option<K>,
}

pub trait ReplacementPolicy<K>: Send + Sync {
    fn access(&mut self, key: K) -> AccessOutcome<K>;

    fn remove(&mut self, key: &K) -> bool;

    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn capacity(&self) -> usize;
    fn name(&self) -> &'static str;

    fn contains(&self, key: &K) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    Lru,
    Lfu,
    Arc,
}

impl PolicyKind {
    pub fn build<K: CacheKey>(self, capacity: usize) -> Box<dyn ReplacementPolicy<K>> {
        let capacity = capacity.max(1);
        match self {
            PolicyKind::Lru => Box::new(lru::Lru::new(capacity)),
            PolicyKind::Lfu => Box::new(lfu::Lfu::new(capacity)),
            PolicyKind::Arc => Box::new(arc::Arc::new(capacity)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PolicyKind::Lru => "lru",
            PolicyKind::Lfu => "lfu",
            PolicyKind::Arc => "arc",
        }
    }

    pub const ALL: [PolicyKind; 3] = [PolicyKind::Lru, PolicyKind::Lfu, PolicyKind::Arc];
}
