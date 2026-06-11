//! Steady-state sanity: on a workload with an obvious hot set that fits in
//! capacity, every policy should hit on the hot keys. This would have caught a
//! policy that fails to retain frequently/recently used entries.

use caching_strategies::model::{Address, Key, ADDR_LEN};
use caching_strategies::policy::{PolicyKind, ReplacementPolicy};

fn key(n: u32) -> Key {
    let mut b = [0u8; ADDR_LEN];
    b[..4].copy_from_slice(&n.to_le_bytes());
    Key::account(Address(b))
}

/// Run a hot-set workload through `policy` at `capacity`; return the hit rate.
fn hit_rate(kind: PolicyKind, capacity: usize) -> f64 {
    let mut p = kind.build(capacity);
    let hot = 50u32; // hot keys, all fit in capacity=100
    let rounds = 5000u32;
    let mut cold = 1_000_000u32; // ever-fresh cold keys
    let mut hits = 0u64;
    let mut total = 0u64;
    for _ in 0..rounds {
        // read the whole hot set
        for h in 0..hot {
            if p.access(key(h)).hit {
                hits += 1;
            }
            total += 1;
        }
        // a few unique cold keys (capacity has room: 100 > 50 hot + 5 cold)
        for _ in 0..5 {
            cold += 1;
            p.access(key(cold));
            total += 1;
        }
    }
    hits as f64 / total as f64
}

#[test]
fn all_policies_retain_a_hot_set() {
    let lru = hit_rate(PolicyKind::Lru, 100);
    let lfu = hit_rate(PolicyKind::Lfu, 100);
    let arc = hit_rate(PolicyKind::Arc, 100);
    println!("hot-set hit rates: lru={lru:.3} lfu={lfu:.3} arc={arc:.3}");
    // The 50 hot keys fit; after warmup they should essentially always hit, so
    // hit rate approaches 50/55 ≈ 0.91. Allow generous slack but catch collapse.
    assert!(lru > 0.8, "LRU collapsed: {lru:.3}");
    assert!(lfu > 0.8, "LFU collapsed: {lfu:.3}");
    assert!(arc > 0.8, "ARC collapsed: {arc:.3}");
}

// --- Reproduce the driver-level collapse: WitnessCache with a retention window ---

use caching_strategies::cache::WitnessCache;
use caching_strategies::metrics::RunMetrics;
use caching_strategies::model::BlockRecord;

fn windowed_hit_rate(kind: PolicyKind, capacity: usize, window: u64) -> f64 {
    let mut c = WitnessCache::new(kind.build(capacity), window);
    let mut m = RunMetrics::default();
    // 300 blocks; each block reads a sliding window of 31 keys, so consecutive
    // blocks share 30 keys — strong reuse well within the window.
    for b in 0..300u64 {
        let reads: Vec<Key> = (b..b + 31).map(|n| key(n as u32)).collect();
        let rec = BlockRecord { block_number: b, timestamp: b, reads, writes: vec![] };
        c.process_counting(&rec, true, &mut m);
    }
    m.overall_hit_rate()
}

#[test]
fn windowed_cache_retains_reused_keys() {
    let lru = windowed_hit_rate(PolicyKind::Lru, 100, 32);
    let lfu = windowed_hit_rate(PolicyKind::Lfu, 100, 32);
    let arc = windowed_hit_rate(PolicyKind::Arc, 100, 32);
    println!("windowed hit rates: lru={lru:.3} lfu={lfu:.3} arc={arc:.3}");
    assert!(arc > 0.5, "ARC collapsed under windowed driver: {arc:.3} (lru={lru:.3})");
}

/// Regression for the ARC collapse under heavy write-invalidation: with many
/// resident keys removed each block, ARC's resident set must keep rebuilding
/// rather than getting pinned near-empty by stale ghost lists. Runs at a large
/// capacity (where the original bug manifested) with reuse + churn.
fn windowed_hit_rate_with_writes(kind: PolicyKind, capacity: usize, window: u64) -> (f64, u64) {
    let mut c = WitnessCache::new(kind.build(capacity), window);
    let mut m = RunMetrics::default();
    let pool = 8000u32; // reused key pool, fits well within capacity
    let mut cold = 5_000_000u32;
    for b in 0..400u64 {
        // 2000 reads from the reused pool (sliding) + 100 fresh cold keys
        let base = (b as u32 * 7) % (pool - 2000);
        let mut reads: Vec<Key> = (base..base + 2000).map(key).collect();
        for _ in 0..100 {
            cold += 1;
            reads.push(key(cold));
        }
        // 1000 writes invalidating part of the reused pool each block (heavy churn)
        let wbase = (b as u32 * 13) % (pool - 1000);
        let writes: Vec<Key> = (wbase..wbase + 1000).map(key).collect();
        let rec = BlockRecord { block_number: b, timestamp: b, reads, writes };
        c.process_counting(&rec, true, &mut m);
    }
    (m.overall_hit_rate(), c.occupancy() as u64)
}

#[test]
fn arc_survives_heavy_invalidation() {
    let cap = 10_000;
    let (lru, lru_occ) = windowed_hit_rate_with_writes(PolicyKind::Lru, cap, 64);
    let (arc, arc_occ) = windowed_hit_rate_with_writes(PolicyKind::Arc, cap, 64);
    println!("with writes: lru={lru:.3} (occ {lru_occ})  arc={arc:.3} (occ {arc_occ})");
    // ARC must stay competitive with LRU and keep a populated resident set — the
    // original bug left ARC at occupancy ~8 and hit rate near zero.
    assert!(arc_occ > cap as u64 / 2, "ARC resident set collapsed: occ={arc_occ}");
    assert!(arc > 0.5 * lru, "ARC far below LRU under churn: arc={arc:.3} lru={lru:.3}");
}
