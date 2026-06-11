use crate::model::Key;

pub mod arc;
pub mod lfu;
pub mod lru;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessOutcome {
    pub hit: bool,
    pub evicted: Option<Key>,
}

pub trait ReplacementPolicy: Send + Sync {
    fn access(&mut self, key: Key) -> AccessOutcome;

   fn remove(&mut self, key: &Key) -> bool;

    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn capacity(&self) -> usize;
    fn name(&self) -> &'static str;

    fn contains(&self, key: &Key) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyKind {
    Lru,
    Lfu,
    Arc,
}

impl PolicyKind {
    pub fn build(self, capacity: usize) -> Box<dyn ReplacementPolicy> {
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
