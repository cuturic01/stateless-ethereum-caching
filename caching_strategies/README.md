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

## Sweep grid

`3 policies {lru,lfu,arc} × 5 windows {8,16,32,64,128} × 5 capacities
{10,25,50,75,100}% × 3 strata {all,defi,transfer} = 225 result rows.`

Capacity in entries = `pct × window_working_set[N].mean` from
`dataset_stats.json`. Strata only gate *which blocks are counted* (cache state
always advances over every block), so the three strata of a (policy, window,
capacity) cell share **one** cache simulation → 75 simulations, 225 rows.

## Run

```sh
cargo test                 # 29 unit/integration tests
cargo run --release -- verify-load --data ../data   # stream check (50,400 blocks)
cargo run --release -- run \
    --data ../data \
    --stats ../analytics/out/dataset/dataset_stats.json \
    --strata ../analytics/out/dataset/strata.json \
    --out ../results \
    [--bytes-per-key 200] [--verify-sha] [--threads N]
```

## Outputs (`../results/`)

| File | One row per | Key columns |
|---|---|---|
| `runs.parquet` | run (225) | policy, window, capacity_pct, capacity_entries, stratum, overall_hit_rate, overall_compression_ratio, total_{reads,hits,misses,invalidations}, peak_occupancy, mean_survival, survival_b0..b13, bytes_{witnessed,saved,naive} |
| `series/run_<id>.parquet` | in-stratum block | block_number, reads, hits, misses, block_hit_rate, bytes_{witnessed,saved}, invalidations, occupancy |
| `contracts/run_<id>.parquet` | top-50 contract | address, invalidations |

`run_id = sim_index*3 + stratum_index` where stratum order is `[all, defi,
transfer]`.

## Module map

`model` (Key/Address/BlockRecord) · `manifest` + `loader` (streaming
msgpack+zstd, one shard at a time) · `ordered` (O(1) intrusive recency set, the
shared primitive) · `hash` (FxHash — keys are already uniform) · `policy/{lru,
lfu,arc}` · `window` (N-block retention) · `cache` (the driver + multi-sink) ·
`metrics` (survival histogram, contract counters) · `strata` · `config`
(capacity resolution) · `output` (Parquet) · `sweep` (grid + rayon).

## Hypotheses the output supports

- **H1** substantial redundancy: `overall_compression_ratio ≫ 0.10`.
- **H2** window inflection: compression vs `window` at fixed capacity rises fast
  to N≈32 then saturates.
- **H3** invalidation concentration + survival: `contracts/` top-K skew +
  `survival_b*` histogram (expected bimodal: k=1 churn spike + long tail).
- **H4** policy comparison: `arc` vs `lru`/`lfu` at matched capacity < 100%.
