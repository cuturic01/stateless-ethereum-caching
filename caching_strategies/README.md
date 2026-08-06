# caching_strategies — Stage 3 (Rust simulation)

Simulates a stateless-Ethereum **witness cache** over the scraped mainnet trace
(`../data/`) and measures cross-block witness-delta compression under three
replacement policies. Inputs and outputs are the contracts described in
`../../implementation-plan.md`.

## What it does

For each block, in order:

1. **Expire** — drop entries whose last witnessing is older than the retention
   window N (H2's "how long must a validator retain witness data").
2. **Invalidate** — evict every resident key written this block. A write changes
   the Verkle commitment up to the state root (EIP-6800), so a cached witness for
   a written key is cryptographically invalid to reuse.
3. **Test + insert** — each read key is a **hit** (compressible, not re-sent) or
   a **miss** (must be witnessed); the replacement policy evicts to stay within
   capacity.
4. **Record** — per-stratum metrics.

Witness bytes use a **structural EIP-4762 Verkle model** (`src/witness.rs`): each key
maps to a Verkle `(stem, suffix)` and a block's witness is accounted as a non-cacheable
per-block IPA proof floor (576 B) + `commitments_by_path` + per-stem scaffolding +
per-leaf values, with a stem dirtied by any miss/write re-shipping its scaffold.
`overall_compression_ratio = bytes_saved / bytes_naive` is therefore **decoupled** from
the hit rate. Output columns: `bytes_sent`, `bytes_saved`, `bytes_naive`, `floor_bytes`,
`cacheable_fraction`.

### Extension-node cache

A second cache, keyed on `StemId`, holds **extension nodes** as first-class
entries (128 B each: the 96 B extension node plus its depth-1
`commitments_by_path` entry). It uses the same policy kind and retention window
as the leaf cache, sized against its own working set
(`window_working_set_stems`). A write to any of a stem's 256 suffixes moves C1
or C2, so the entry is dropped — exact, because the trace carries every state
write. Three per-stem rules are reported side by side:

| Column | The stem's 128 B is free iff |
|---|---|
| `bytes_sent` | every touched leaf resident and unwritten (published, unchanged) |
| `bytes_sent_stem` | the extension node itself is cached and nothing under the stem was written |
| `bytes_sent_stem_opt` | *some* touched leaf resident and nothing written — the published rule minus its pessimism |

`bytes_sent_stem` is a **lower bound** on extension-node caching: a client that
recomputed C1/C2 from the witness would keep entries this model drops.
`bytes_sent_stem_opt ≤ bytes_sent` always; `bytes_sent_stem` is incomparable to
both, and can beat either.

### Cache footprint

`peak_cache_bytes` / `mean_cache_bytes` (split into `peak_leaf_bytes` /
`peak_stem_bytes`) report how much witness material the client holds, at 33 B
per leaf and 128 B per stem — the same bytes the witness model already counts.
This is the **protocol payload**, not a real client's resident set: it excludes
the key index, which is implementation-dependent and deliberately not modelled.

## Sweep grid

`3 policies {lru,lfu,arc} × 5 windows {8,16,32,64,128} × 5 capacities
{10,25,50,75,100}% × 3 strata {all,defi,transfer} = 225 result rows.`

Capacity in entries = `pct × window_working_set[N].mean` from
`dataset_stats.json`; the stem cache gets the same `pct` of
`window_working_set_stems[N].mean`. The grid is unchanged at 225 rows — policy
is shared between the two caches, not a new axis. Strata only gate *which blocks are counted* (cache state
always advances over every block), so the three strata of a (policy, window,
capacity) cell share **one** cache simulation → 75 simulations, 225 rows.

## Run

```sh
cargo test                 # 63 unit/integration tests
cargo run --release -- verify-load --data ../data   # stream check (50,400 blocks)
cargo run --release -- run \
    --data ../data \
    --stats ../analytics/out/dataset/dataset_stats.json \
    --strata ../analytics/out/dataset/strata.json \
    --out ../results \
    [--verify-sha] [--threads N]
```

## Outputs (`../results/`)

| File | One row per | Key columns |
|---|---|---|
| `runs.parquet` | run (225) | policy, window, capacity_pct, capacity_entries, stratum, overall_hit_rate, overall_compression_ratio, total_{reads,hits,misses,invalidations}, peak_occupancy, mean_survival, survival_b0..b13, bytes_{sent,saved,naive}, floor_bytes, cacheable_fraction, {peak,mean}_cache_bytes, peak_{leaf,stem}_bytes, bytes_sent_stem{,_opt}, compression_stem, stem_{capacity_entries,peak_occupancy,mean_survival}, total_stem_invalidations |
| `series/run_<id>.parquet` | in-stratum block | block_number, reads, hits, misses, block_hit_rate, bytes_{sent,saved,naive}, floor_bytes, invalidations, occupancy, cache_bytes, bytes_sent_stem |
| `contracts/run_<id>.parquet` | top-50 contract | address, invalidations |

`run_id = sim_index*3 + stratum_index` where stratum order is `[all, defi,
transfer]`.

## Module map

`model` (Key/Address/BlockRecord) · `manifest` + `loader` (streaming
msgpack+zstd, one shard at a time) · `ordered` (O(1) intrusive recency set, the
shared primitive, generic over the key) · `hash` (FxHash — keys are already
uniform) · `policy/{lru,lfu,arc}` (generic over the key, so one implementation
serves both caches) · `window` (N-block retention) · `cache` (the driver +
multi-sink, holding the leaf and stem caches) · `metrics` (survival histograms,
contract counters, footprint) · `strata` · `config`

`witness` holds the structural size model *and* the stem derivation. That
derivation is mirrored in `analytics_lib/dataio.py`; `fixtures/stem_derivation.json`
is the shared fixture that keeps the two from drifting, checked by
`tests/stem_derivation.rs` and `analytics/tests/test_stem_derivation.py`.
(capacity resolution) · `output` (Parquet) · `sweep` (grid + rayon).

## Hypotheses the output supports

- **H1** substantial redundancy: `overall_compression_ratio ≫ 0.10`.
- **H2** window inflection: compression vs `window` at fixed capacity rises fast
  to N≈32 then saturates.
- **H3** invalidation concentration + survival: `contracts/` top-K skew +
  `survival_b*` histogram (expected bimodal: k=1 churn spike + long tail).
- **H4** policy comparison: `arc` vs `lru`/`lfu` at matched capacity < 100%.
