//! ARC correctness: capacity, ghost-driven `p` adaptation, and ghost purge on
//! invalidation. Trace hand-derived for capacity c = 2.

use caching_strategies::model::{Address, Key, ADDR_LEN};
use caching_strategies::policy::{arc::Arc, ReplacementPolicy};

fn k(n: u8) -> Key {
    Key::account(Address([n; ADDR_LEN]))
}

/// Drive A,A,B,C so that B is demoted to the b1 ghost list (see module trace).
/// Returns the cache in that state.
fn arc_with_b_ghosted() -> Arc {
    let mut c = Arc::new(2);
    c.access(k(1)); // A miss -> t1=[A]
    assert!(c.access(k(1)).hit); // A hit -> t2=[A]
    c.access(k(2)); // B miss -> t1=[B], t2=[A]
    let out = c.access(k(3)); // C miss -> replace demotes B to b1
    assert!(!out.hit);
    assert_eq!(out.evicted, Some(k(2)));
    let (t1, t2, b1, b2) = c.list_sizes();
    assert_eq!((t1, t2, b1, b2), (1, 1, 1, 0)); // t1=[C], t2=[A], b1=[B]
    c
}

#[test]
fn ghost_hit_in_b1_grows_p_and_readmits() {
    let mut c = arc_with_b_ghosted();
    assert_eq!(c.target_p(), 0);
    assert!(!c.contains(&k(2))); // B is a ghost, not resident

    let out = c.access(k(2)); // ghost hit in b1
    assert!(!out.hit, "ghost hit is a compression miss");
    assert_eq!(out.evicted, Some(k(1))); // A demoted from t2 to b2
    assert!(c.contains(&k(2))); // B re-admitted to t2
    assert!(c.target_p() >= 1, "b1 hit should grow p toward recency");
    assert!(c.len() <= 2);
}

#[test]
fn invalidate_purges_ghost_lists() {
    let mut c = arc_with_b_ghosted(); // b1=[B]
    // B is a ghost (not resident): remove returns false but must purge b1.
    assert!(!c.remove(&k(2)));
    let (_, _, b1, b2) = c.list_sizes();
    assert_eq!((b1, b2), (0, 0), "ghost B purged");

    let p_before = c.target_p();
    // Re-accessing B is now a fresh miss (Case IV), so p must not adapt.
    let out = c.access(k(2));
    assert!(!out.hit);
    assert_eq!(c.target_p(), p_before, "fresh miss must not change p");
}

#[test]
fn invalidate_resident_frees_capacity() {
    let mut c = Arc::new(2);
    c.access(k(1));
    c.access(k(2));
    assert_eq!(c.len(), 2);
    assert!(c.remove(&k(1)), "k1 was resident");
    assert_eq!(c.len(), 1);
    let out = c.access(k(3)); // room available, no resident eviction
    assert!(!out.hit);
    assert_eq!(out.evicted, None);
}

#[test]
fn capacity_never_exceeded() {
    let mut c = Arc::new(4);
    // a churny mixed sequence; resident set must never exceed c
    for round in 0..50u32 {
        for n in 0..10u8 {
            let key = k(((round as u8).wrapping_mul(7)).wrapping_add(n));
            c.access(key);
            assert!(c.len() <= 4, "resident set exceeded capacity");
        }
    }
}
