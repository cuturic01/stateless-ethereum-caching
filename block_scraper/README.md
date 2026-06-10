# block_scraper

Fetches recent Ethereum mainnet blocks with their per-block **read sets** and
**write sets** (the state each block touched / modified) and serializes them for
the witness-cache simulation. Data source: Alchemy `debug_traceBlockByNumber`
with the `prestateTracer`.

## What it collects

For each block: `block_number`, `timestamp`, `read_set`, `write_set`. A *key* is
either an account-header access `(address, None)` or a storage-slot access
`(address, slot)`. Read set comes from the prestateTracer in default mode (full
prestate = everything touched); write set comes from `diffMode: true` (only what
changed) — both required, so **2 trace calls + 1 timestamp call per block**.

## Setup

Requires [uv](https://docs.astral.sh/uv/) and an Alchemy key on a **Pay-As-You-Go**
(or higher) plan — `debug_*` / trace methods are gated behind paid tiers.

```bash
cd block_scraper
cp .env.example .env        # then edit .env, set ALCHEMY_API_KEY
uv sync                     # create venv + install deps
```

## Usage

```bash
# Small dry run first (~50 blocks, ~$0.001) — validates the whole path.
uv run python -m scraper scrape --blocks 50 --shard-size 25

# Verify shard hashes + contiguous coverage.
uv run python -m scraper verify

# The real run: ~1 week of mainnet (~50,400 blocks).
uv run python -m scraper scrape --blocks 50400

# Pin an exact range for a fully reproducible dataset:
uv run python -m scraper scrape --start 21000000 --end 21050399

# Resume after an interruption (re-reads checkpoint, skips finished shards):
uv run python -m scraper resume
```

Cost is dominated by wall-clock, not money: ~50,400 blocks × (2×40 + 20) CU
≈ 5M CU ≈ **~$2**, but ~100k heavy trace calls take hours — hence the resumable
checkpointing. Tune `--concurrency` if you hit 429s.

## Output (`../data/`)

```
data/
  manifest.json     # run config + per-shard {first/last block, count, sha256}
  checkpoint.json   # completed shards (resume state)
  shards/
    blocks_00000.msgpack.zst   # ~500 blocks/shard, zstd-compressed MessagePack
    ...
```

Each shard is a MessagePack array of records; each record is the positional
array `[block_number, timestamp, read_set, write_set]`, where every key is
`[addr_bytes20, slot_bytes32_or_nil]`. See `scraper/models.py` for the schema —
the Rust simulation (`caching_strategies/`) reads this same format.
```
