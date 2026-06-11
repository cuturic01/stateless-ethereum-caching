//! LRU correctness on deterministic sequences.

use caching_strategies::model::{Address, Key, ADDR_LEN};
use caching_strategies::policy::{lru::Lru, ReplacementPolicy};

fn k(n: u8) -> Key {
    Key::account(Address([n; ADDR_LEN]))
}

#[test]
fn evicts_least_recently_used() {
    let mut c = Lru::new(2);
    assert_eq!(c.access(k(1)).evicted, None); // [1]
    assert_eq!(c.access(k(2)).evicted, None); // [1,2]
    // touch 1 -> order [2,1]; inserting 3 evicts 2
    assert!(c.access(k(1)).hit);
    let out = c.access(k(3));
    assert!(!out.hit);
    assert_eq!(out.evicted, Some(k(2)));
    assert!(c.contains(&k(1)) && c.contains(&k(3)) && !c.contains(&k(2)));
    assert_eq!(c.len(), 2);
}

#[test]
fn hit_does_not_evict() {
    let mut c = Lru::new(2);
    c.access(k(1));
    c.access(k(2));
    let out = c.access(k(2));
    assert!(out.hit);
    assert_eq!(out.evicted, None);
}

#[test]
fn remove_frees_capacity() {
    let mut c = Lru::new(2);
    c.access(k(1));
    c.access(k(2));
    assert!(c.remove(&k(1)));
    assert!(!c.remove(&k(1))); // already gone
    let out = c.access(k(3)); // room again, no eviction
    assert!(!out.hit);
    assert_eq!(out.evicted, None);
    assert_eq!(c.len(), 2);
}
