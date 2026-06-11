//! LFU correctness: lowest-frequency victim, with LRU tie-break within a freq.

use caching_strategies::model::{Address, Key, ADDR_LEN};
use caching_strategies::policy::{lfu::Lfu, ReplacementPolicy};

fn k(n: u8) -> Key {
    Key::account(Address([n; ADDR_LEN]))
}

#[test]
fn evicts_least_frequent() {
    let mut c = Lfu::new(2);
    c.access(k(1)); // freq{1:1}
    c.access(k(2)); // freq{1:1,2:1}
    assert!(c.access(k(1)).hit); // freq{1:2,2:1}
    // insert 3 -> evict lowest freq = 2
    let out = c.access(k(3));
    assert_eq!(out.evicted, Some(k(2)));
    assert!(c.contains(&k(1)) && c.contains(&k(3)));
}

#[test]
fn lru_breaks_frequency_ties() {
    let mut c = Lfu::new(2);
    c.access(k(1)); // {1:1}
    c.access(k(2)); // {1:1,2:1}  both freq 1; 1 is LRU
    // insert 3 -> tie at freq 1, evict LRU among them = 1
    let out = c.access(k(3));
    assert_eq!(out.evicted, Some(k(1)));
    assert!(c.contains(&k(2)) && c.contains(&k(3)));
}

#[test]
fn promotion_protects_hot_key() {
    let mut c = Lfu::new(2);
    c.access(k(1));
    c.access(k(1)); // freq 2
    c.access(k(2)); // freq 1 (evicts nothing, capacity 2)
    // 1 is hot (freq 2). Insert 3 -> evict freq-1 key = 2.
    let out = c.access(k(3));
    assert_eq!(out.evicted, Some(k(2)));
    // Now 1 (freq2) and 3 (freq1). Insert 4 -> evict 3.
    let out = c.access(k(4));
    assert_eq!(out.evicted, Some(k(3)));
    assert!(c.contains(&k(1)));
}

#[test]
fn remove_empties_min_bucket_then_recovers() {
    let mut c = Lfu::new(2);
    c.access(k(1)); // {1:f1}
    c.access(k(1)); // {1:f2}
    c.access(k(2)); // {1:f2, 2:f1}, min freq = 1
    // Removing the sole f1 key empties the min bucket.
    assert!(c.remove(&k(2)));
    c.access(k(3)); // {1:f2, 3:f1}, min freq recovered to 1
    // At capacity: evict the min-freq key (3), not the hot f2 key (1).
    let out = c.access(k(4));
    assert_eq!(out.evicted, Some(k(3)));
    assert!(c.contains(&k(1)) && c.contains(&k(4)));
}
