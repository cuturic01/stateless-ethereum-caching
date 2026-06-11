//! End-to-end driver semantics on small synthetic traces: cross-block hits,
//! write invalidation, read+write in the same block, window expiry, survival
//! ages, per-contract counters, and stratum gating.

use caching_strategies::cache::WitnessCache;
use caching_strategies::metrics::RunMetrics;
use caching_strategies::model::{Address, BlockRecord, Key, ADDR_LEN};
use caching_strategies::policy::PolicyKind;

fn addr(n: u8) -> Address {
    Address([n; ADDR_LEN])
}
fn ak(n: u8) -> Key {
    Key::account(addr(n))
}

fn block(num: u64, reads: Vec<Key>, writes: Vec<Key>) -> BlockRecord {
    BlockRecord { block_number: num, timestamp: num, reads, writes }
}

/// Build a cache with a generous window so expiry doesn't interfere unless tested.
fn cache(policy: PolicyKind, cap: usize, window: u64) -> WitnessCache {
    WitnessCache::new(policy.build(cap), window)
}

#[test]
fn cross_block_reread_is_a_hit() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![ak(1)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![ak(1)], vec![]), true, &mut m);
    assert_eq!(m.total_reads, 2);
    assert_eq!(m.total_hits, 1); // block 2's read hits
    assert_eq!(m.total_misses, 1); // block 1's read misses
}

#[test]
fn write_between_reads_invalidates() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![ak(1)], vec![]), true, &mut m); // miss, resident
    c.process_counting(&block(2, vec![], vec![ak(1)]), true, &mut m); // invalidated
    c.process_counting(&block(3, vec![ak(1)], vec![]), true, &mut m); // miss again
    assert_eq!(m.total_hits, 0);
    assert_eq!(m.total_misses, 2);
    assert_eq!(m.total_invalidations, 1);
    assert_eq!(m.contracts.counts.get(&addr(1)), Some(&1));
}

#[test]
fn read_and_write_same_block_is_invalidation_then_miss() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![ak(1)], vec![]), true, &mut m); // resident
    // block 2 writes then reads the same key: invalidate (step 2) then miss (step 3)
    c.process_counting(&block(2, vec![ak(1)], vec![ak(1)]), true, &mut m);
    assert_eq!(m.total_invalidations, 1);
    assert_eq!(m.total_hits, 0);
    assert_eq!(m.total_misses, 2); // block1 miss + block2 miss
}

#[test]
fn window_expiry_drops_stale_entries() {
    let mut c = cache(PolicyKind::Lru, 100, 4); // window N=4
    let mut m = RunMetrics::default();
    c.process_counting(&block(100, vec![ak(1)], vec![]), true, &mut m); // last_seen 100
    // not re-witnessed; by block 104 it should have expired (100 + 4 <= 104)
    c.process_counting(&block(104, vec![ak(1)], vec![]), true, &mut m); // expired pre-read -> miss
    assert_eq!(m.total_hits, 0);
    assert_eq!(m.total_misses, 2);
    // one survival sample from the expiry, age = 104 - 100 = 4
    assert_eq!(m.survival.n, 1);
    assert_eq!(m.survival.buckets[4], 1);
}

#[test]
fn survival_age_recorded_on_invalidation() {
    let mut c = cache(PolicyKind::Lru, 100, 1000);
    let mut m = RunMetrics::default();
    c.process_counting(&block(10, vec![ak(1)], vec![]), true, &mut m); // insert at 10
    c.process_counting(&block(17, vec![], vec![ak(1)]), true, &mut m); // invalidate at 17 -> age 7
    assert_eq!(m.survival.n, 1);
    assert_eq!(m.survival.buckets[7], 1);
}

#[test]
fn stratum_gating_advances_cache_without_counting() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    // block 1 NOT counted, but its read must still populate the cache
    c.process_counting(&block(1, vec![ak(1)], vec![]), false, &mut m);
    // block 2 counted: the read hits thanks to block 1's (uncounted) insert
    c.process_counting(&block(2, vec![ak(1)], vec![]), true, &mut m);
    assert_eq!(m.blocks_counted, 1);
    assert_eq!(m.total_reads, 1);
    assert_eq!(m.total_hits, 1);
}

#[test]
fn multi_sink_shares_cache_across_strata() {
    // One cache, two sinks: an "all" sink (always active) and a "selective" sink
    // active only on block 2. Both must see the same cache, but count differently.
    use caching_strategies::cache::MetricSink;
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut all = RunMetrics::default();
    let mut sel = RunMetrics::default();

    let b1 = block(1, vec![ak(1)], vec![]);
    {
        let mut sinks = [
            MetricSink { metrics: &mut all, active: true },
            MetricSink { metrics: &mut sel, active: false },
        ];
        c.process_block(&b1, &mut sinks);
    }
    let b2 = block(2, vec![ak(1)], vec![]); // hit (shared cache from b1)
    {
        let mut sinks = [
            MetricSink { metrics: &mut all, active: true },
            MetricSink { metrics: &mut sel, active: true },
        ];
        c.process_block(&b2, &mut sinks);
    }

    assert_eq!(all.total_reads, 2);
    assert_eq!(all.total_hits, 1);
    // selective only counted block 2, which was a hit thanks to the shared cache
    assert_eq!(sel.total_reads, 1);
    assert_eq!(sel.total_hits, 1);
    assert_eq!(sel.blocks_counted, 1);
}

#[test]
fn all_policies_agree_when_capacity_is_ample() {
    // With capacity >> working set and no writes, hit counts are policy-independent.
    let trace = vec![
        block(1, vec![ak(1), ak(2)], vec![]),
        block(2, vec![ak(1), ak(3)], vec![]),
        block(3, vec![ak(2), ak(3)], vec![]),
    ];
    let mut hits = vec![];
    for p in PolicyKind::ALL {
        let mut c = cache(p, 1000, 1000);
        let mut m = RunMetrics::default();
        for b in &trace {
            c.process_counting(b, true, &mut m);
        }
        hits.push(m.total_hits);
    }
    assert!(hits.iter().all(|&h| h == hits[0]), "policies disagree: {hits:?}");
    // reads: 2+2+2=6; uniques first-seen: 1,2 (blk1 miss x2), 3 (blk2 miss), blk2 ak1 hit,
    // blk3 ak2 hit, ak3 hit => 3 hits.
    assert_eq!(hits[0], 3);
}
