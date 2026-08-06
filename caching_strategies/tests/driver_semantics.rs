//! End-to-end driver semantics on small synthetic traces: cross-block hits,
//! write invalidation, read+write in the same block, window expiry, survival
//! ages, per-contract counters, and stratum gating.

use caching_strategies::cache::WitnessCache;
use caching_strategies::metrics::RunMetrics;
use caching_strategies::model::{Address, BlockRecord, Key, ADDR_LEN};
use caching_strategies::policy::PolicyKind;
use caching_strategies::witness::{LEAF_ENTRY_BYTES, STEM_ENTRY_BYTES};

fn addr(n: u8) -> Address {
    Address([n; ADDR_LEN])
}
fn ak(n: u8) -> Key {
    Key::account(addr(n))
}
/// Storage key for slot `slot` of contract `n`.
fn sk(n: u8, slot: u64) -> Key {
    let mut s = [0u8; 32];
    s[24..].copy_from_slice(&slot.to_be_bytes());
    Key::storage(addr(n), s)
}

fn block(num: u64, reads: Vec<Key>, writes: Vec<Key>) -> BlockRecord {
    BlockRecord { block_number: num, timestamp: num, reads, writes }
}

/// Build a cache with a generous window so expiry doesn't interfere unless tested.
/// The stem cache gets the same capacity as the leaf cache, which is ample: a
/// block never has more stems than leaves.
fn cache(policy: PolicyKind, cap: usize, window: u64) -> WitnessCache {
    WitnessCache::new(policy.build(cap), policy.build(cap), window)
}

/// A cache whose leaf and stem capacities differ, for split-budget cases.
fn cache_split(policy: PolicyKind, cap: usize, stem_cap: usize, window: u64) -> WitnessCache {
    WitnessCache::new(policy.build(cap), policy.build(stem_cap), window)
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

// ---- cache footprint (extention-plan §5) ----

#[test]
fn empty_cache_holds_no_bytes() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![], vec![]), true, &mut m);
    assert_eq!(m.peak_cache_bytes, 0);
    assert_eq!(m.mean_cache_bytes(), 0.0);
}

#[test]
fn footprint_charges_33_per_leaf_and_128_per_stem() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    // Three leaves under a single storage stem (slots 300/400/500 all sit in
    // chunk 1), so the leaf and stem counts differ and the split is meaningful.
    c.process_counting(
        &block(1, vec![sk(1, 300), sk(1, 400), sk(1, 500)], vec![]),
        true,
        &mut m,
    );
    assert_eq!(c.occupancy(), 3);
    assert_eq!(c.stem_occupancy(), 1);
    assert_eq!(m.peak_leaf_bytes, 3 * LEAF_ENTRY_BYTES);
    assert_eq!(m.peak_stem_bytes, STEM_ENTRY_BYTES);
    assert_eq!(m.peak_cache_bytes, 3 * LEAF_ENTRY_BYTES + STEM_ENTRY_BYTES);
}

#[test]
fn footprint_never_exceeds_the_capacity_bound() {
    // Leaf capacity 5, stem capacity 2, but 40 distinct keys over 8 blocks: the
    // bound must hold at every block, not just on average.
    let (cap, stem_cap) = (5, 2);
    let mut c = cache_split(PolicyKind::Lru, cap, stem_cap, 100);
    let mut m = RunMetrics::default();
    for b in 1..=8u64 {
        let reads: Vec<Key> = (0..5).map(|i| ak((b as u8) * 5 + i)).collect();
        c.process_counting(&block(b, reads, vec![]), true, &mut m);
    }
    let bound = cap as u64 * LEAF_ENTRY_BYTES + stem_cap as u64 * STEM_ENTRY_BYTES;
    assert!(m.peak_cache_bytes <= bound, "{} > {bound}", m.peak_cache_bytes);
    assert!(m.series.iter().all(|s| s.cache_bytes <= bound));
}

#[test]
fn mean_footprint_never_exceeds_peak() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    for b in 1..=6u64 {
        let reads: Vec<Key> = (0..b as u8).map(ak).collect();
        c.process_counting(&block(b, reads, vec![]), true, &mut m);
    }
    assert!(m.mean_cache_bytes() <= m.peak_cache_bytes as f64);
    assert!(m.mean_cache_bytes() > 0.0);
    // The peak split always reconstructs the peak total.
    assert_eq!(m.peak_leaf_bytes + m.peak_stem_bytes, m.peak_cache_bytes);
}

#[test]
fn footprint_shrinks_when_writes_invalidate() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![ak(1), ak(2), ak(3)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![], vec![ak(1), ak(2)]), true, &mut m);
    // Each account header is its own stem, so the writes drop 2 leaves and the
    // 2 extension nodes above them.
    assert_eq!(c.occupancy(), 1);
    assert_eq!(c.stem_occupancy(), 1);
    assert_eq!(m.series[1].cache_bytes, LEAF_ENTRY_BYTES + STEM_ENTRY_BYTES);
    assert_eq!(
        m.peak_cache_bytes,
        3 * (LEAF_ENTRY_BYTES + STEM_ENTRY_BYTES),
        "peak keeps block 1's high-water mark"
    );
}

// ---- extension-node cache (extention-plan §3, §6) ----

const FLOOR: u64 = 576; // witness::IPA_FLOOR_BYTES

#[test]
fn resident_stem_is_charged_once_across_blocks() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![sk(1, 300)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![sk(1, 300)], vec![]), true, &mut m);
    // Block 1 is cold: floor + the stem's 128 B + the leaf's 33 B.
    assert_eq!(m.series[0].bytes_sent_stem, FLOOR + STEM_ENTRY_BYTES + LEAF_ENTRY_BYTES);
    // Block 2 re-reads it: both the leaf and its extension node are held.
    assert_eq!(m.series[1].bytes_sent_stem, FLOOR);
}

#[test]
fn a_write_to_any_suffix_invalidates_the_whole_stem() {
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    // Slots 300 and 380 share a stem (both in the 256..511 chunk).
    c.process_counting(&block(1, vec![sk(1, 300)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![], vec![sk(1, 380)]), true, &mut m);
    c.process_counting(&block(3, vec![sk(1, 300)], vec![]), true, &mut m);

    assert_eq!(m.total_stem_invalidations, 1);
    // The leaf at slot 300 was never written, so it is still resident and its
    // 33 B is still saved — but the extension node above it is stale.
    assert_eq!(m.series[2].bytes_sent_stem, FLOOR + STEM_ENTRY_BYTES);

    // The published pessimistic rule disagrees here: it only inspects leaves
    // touched *this* block, so it forgets block 2's write and elides the stem.
    // Left as-is deliberately — `sent` is the published model and must not move
    // (extention-plan §3.5) — but it means sent_stem can exceed sent even at
    // full capacity, which §6.3 flags as expected-not-guaranteed.
    assert_eq!(m.series[2].witness_sent, FLOOR);
    assert!(m.series[2].bytes_sent_stem > m.series[2].witness_sent);
}

#[test]
fn stem_evicted_under_capacity_pressure_is_recharged() {
    // Leaf cache ample, stem cache holds one entry.
    let mut c = cache_split(PolicyKind::Lru, 100, 1, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![sk(1, 300)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![sk(2, 300)], vec![]), true, &mut m); // evicts stem 1
    c.process_counting(&block(3, vec![sk(1, 300)], vec![]), true, &mut m);
    // The leaf is still cached (its own cache is ample) so its 33 B is saved,
    // but the evicted extension node must be re-sent.
    assert_eq!(m.series[2].bytes_sent_stem, FLOOR + STEM_ENTRY_BYTES);
}

#[test]
fn stem_is_accessed_once_per_block_not_once_per_leaf() {
    // The discriminating case. Stem X is read via 5 leaves in block 1; stem Y
    // via 1. With a correct once-per-block access both reach frequency 1 and
    // LFU evicts the older (X) when Z arrives. Accessed once per leaf, X would
    // reach frequency 5 and Y would be evicted instead.
    let mut c = cache_split(PolicyKind::Lfu, 100, 2, 1000);
    let mut m = RunMetrics::default();

    let x_leaves = vec![sk(1, 300), sk(1, 320), sk(1, 340), sk(1, 360), sk(1, 380)];
    let mut reads = x_leaves.clone();
    reads.push(sk(2, 300)); // stem Y
    c.process_counting(&block(1, reads, vec![]), true, &mut m);
    assert_eq!(c.stem_occupancy(), 2, "one entry per distinct stem, not per leaf");

    c.process_counting(&block(2, vec![sk(3, 300)], vec![]), true, &mut m); // stem Z
    c.process_counting(&block(3, vec![sk(1, 300)], vec![]), true, &mut m); // re-read X

    // X was the eviction victim, so its extension node is re-sent. Its leaf is
    // still resident, so only the stem's 128 B is charged.
    assert_eq!(
        m.series[2].bytes_sent_stem,
        FLOOR + STEM_ENTRY_BYTES,
        "stem X should have been evicted; if it survived, it was accessed once per leaf"
    );
}

#[test]
fn explicit_stem_cache_can_beat_the_leaf_only_bound() {
    // extention-plan §6.3 asserts sent_stem >= sent_stem_opt as a hard
    // invariant. It does not hold, and this is the counterexample: block 2
    // touches a *different* leaf under the stem read in block 1. That leaf is a
    // cold miss, so the leaf-only rule has nothing resident to justify the stem
    // and charges its 128 B — while the explicit cache still holds the
    // extension node itself and elides it.
    let mut c = cache(PolicyKind::Lru, 10_000, 1000);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![sk(1, 300)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![sk(1, 380)], vec![]), true, &mut m);

    let b2 = &m.series[1];
    assert_eq!(b2.bytes_sent_stem, FLOOR + LEAF_ENTRY_BYTES, "stem held, only the leaf is sent");
    assert_eq!(b2.witness_sent, FLOOR + STEM_ENTRY_BYTES + LEAF_ENTRY_BYTES);
    assert!(b2.bytes_sent_stem < b2.witness_sent, "explicit cache beats the leaf-only rule");
    assert!(m.total_witness_bytes_sent_stem < m.total_witness_bytes_sent_stem_opt);
}

#[test]
fn relaxed_rule_never_sends_more_than_the_published_rule() {
    // The invariant that *does* hold: `!dirty` implies `!written`, so every
    // stem the published rule elides the relaxed rule elides too.
    let mut c = cache(PolicyKind::Lru, 10_000, 64);
    let mut m = RunMetrics::default();
    for b in 1..=40u64 {
        let reads: Vec<Key> = (0..12)
            .map(|i| sk(((b + i) % 5) as u8 + 1, 300 + (b * 7 + i) % 200))
            .collect();
        let writes: Vec<Key> = if b % 4 == 0 { vec![sk((b % 5) as u8 + 1, 305)] } else { vec![] };
        c.process_counting(&block(b, reads, writes), true, &mut m);
    }
    assert!(m.total_witness_bytes_sent_stem_opt <= m.total_witness_bytes_sent);
    for s in &m.series {
        assert!(s.bytes_sent_stem >= FLOOR);
        assert!(s.bytes_sent_stem <= s.witness_naive, "sent_stem above naive");
    }
    assert!(m.stem_compression_ratio() > 0.0);
}

#[test]
fn stem_invalidations_stay_out_of_the_contract_histogram() {
    // The per-contract histogram backs invalidation_by_contract.png and counts
    // leaf invalidations only.
    let mut c = cache(PolicyKind::Lru, 100, 100);
    let mut m = RunMetrics::default();
    c.process_counting(&block(1, vec![sk(1, 300)], vec![]), true, &mut m);
    c.process_counting(&block(2, vec![], vec![sk(1, 380)]), true, &mut m);
    // Leaf 380 was never resident, so no leaf invalidation; the stem was.
    assert_eq!(m.total_invalidations, 0);
    assert_eq!(m.total_stem_invalidations, 1);
    assert!(m.contracts.counts.is_empty());
}
