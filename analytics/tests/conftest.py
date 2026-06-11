"""Synthetic fixtures mirroring the on-disk formats Stage 4 consumes.

``synthetic_data`` mimics the block_scraper shards (Stage 2 input);
``synthetic_results`` mimics the Rust sweep output in ``results/`` (Stage 4 input).
"""

import hashlib
import json

import msgpack
import pyarrow as pa
import pyarrow.parquet as pq
import pytest
import zstandard as zstd

from analytics_lib.resultio import (
    CAPS,
    POLICIES,
    STRATA,
    SURVIVAL_BUCKETS,
    WINDOWS,
    run_id_for,
)


def _addr(i: int) -> bytes:
    return i.to_bytes(20, "big")


def _slot(i: int) -> bytes:
    return i.to_bytes(32, "big")


@pytest.fixture
def synthetic_data(tmp_path):
    """Build a tiny but realistic data/ dir: 20 blocks across 2 shards.

    Contracts 1 and 2 are "hot" (touched by most blocks); blocks alternate
    between storage-heavy (defi-like) and transfer-like (account-only).
    """
    data_dir = tmp_path / "data"
    (data_dir / "shards").mkdir(parents=True)

    start, end, shard_size = 1000, 1019, 10
    records_by_shard: dict[int, list] = {0: [], 1: []}
    for bn in range(start, end + 1):
        reads = [[_addr(1), None], [_addr(1), _slot(0)], [_addr(2), _slot(7)]]
        writes = []
        if bn % 2 == 0:  # storage-heavy block hitting hot contracts
            reads += [[_addr(1), _slot(bn % 5)], [_addr(2), _slot(bn % 3)]]
            writes = [[_addr(2), _slot(7)]]
        else:  # transfer-like: just account headers
            reads = [[_addr(100 + bn), None], [_addr(1), None]]
        rec = [bn, 1_700_000_000 + (bn - start) * 12, reads, writes]
        records_by_shard[(bn - start) // shard_size].append(rec)

    shards = []
    cctx = zstd.ZstdCompressor()
    for idx, recs in records_by_shard.items():
        blob = cctx.compress(msgpack.packb(recs, use_bin_type=True))
        fname = f"shards/blocks_{idx:05d}.msgpack.zst"
        (data_dir / fname).write_bytes(blob)
        shards.append({
            "file": fname,
            "first_block": recs[0][0],
            "last_block": recs[-1][0],
            "count": len(recs),
            "sha256": hashlib.sha256(blob).hexdigest(),
        })

    manifest = {
        "network": "eth-mainnet",
        "start_block": start,
        "end_block": end,
        "shard_size": shard_size,
        "schema_version": 1,
        "shards": shards,
    }
    (data_dir / "manifest.json").write_text(json.dumps(manifest))
    return data_dir


@pytest.fixture
def synthetic_results(tmp_path):
    """Build a tiny but complete ``results/`` tree mirroring output.rs's schema.

    Writes all 225 runs.parquet rows (deterministic values, ARC>=LFU>=LRU below
    100% capacity) plus a couple of series/ and contracts/ files, so every
    plot/table script's column filters resolve.
    """
    results = tmp_path / "results"
    (results / "series").mkdir(parents=True)
    (results / "contracts").mkdir(parents=True)

    rows: dict[str, list] = {c: [] for c in (
        "run_id", "policy", "window", "capacity_pct", "capacity_entries", "stratum",
        "bytes_per_key", "blocks_counted", "total_reads", "total_hits", "total_misses",
        "overall_hit_rate", "overall_compression_ratio", "total_invalidations",
        "peak_occupancy", "mean_survival",
        *(f"survival_b{i}" for i in range(SURVIVAL_BUCKETS)),
        "bytes_witnessed", "bytes_saved", "bytes_naive",
    )}
    pol_bonus = {"lru": 0.0, "lfu": 0.03, "arc": 0.05}
    for pol in POLICIES:
        for w in WINDOWS:
            for cap in CAPS:
                for stratum in STRATA:
                    rid = run_id_for(pol, w, cap, stratum)
                    # diminishing-by-capacity, recency<freq<adaptive below 100%.
                    bonus = 0.0 if cap == 100 else pol_bonus[pol] * (100 - cap) / 100
                    comp = min(0.10 + 0.0015 * w + bonus, 0.40)
                    reads, hits = 10_000, int(10_000 * comp)
                    rows["run_id"].append(rid)
                    rows["policy"].append(pol)
                    rows["window"].append(w)
                    rows["capacity_pct"].append(cap)
                    rows["capacity_entries"].append(w * 100 * cap)
                    rows["stratum"].append(stratum)
                    rows["bytes_per_key"].append(200)
                    rows["blocks_counted"].append(20)
                    rows["total_reads"].append(reads)
                    rows["total_hits"].append(hits)
                    rows["total_misses"].append(reads - hits)
                    rows["overall_hit_rate"].append(comp)
                    rows["overall_compression_ratio"].append(comp)
                    rows["total_invalidations"].append(1000 + rid)
                    rows["peak_occupancy"].append(w * 100)
                    rows["mean_survival"].append(8.0 + 0.1 * w)
                    for i in range(SURVIVAL_BUCKETS):
                        rows[f"survival_b{i}"].append(100 - i if i < 14 else 0)
                    rows["bytes_witnessed"].append((reads - hits) * 200)
                    rows["bytes_saved"].append(hits * 200)
                    rows["bytes_naive"].append(reads * 200)
    pq.write_table(pa.table(rows), results / "runs.parquet")

    # A couple of series + contracts files (canonical run + run 0).
    canonical = run_id_for("lru", 32, 100, "all")
    for rid in {0, canonical}:
        pq.write_table(pa.table({
            "block_number": [1000, 1001, 1002],
            "reads": [10, 12, 8], "hits": [3, 5, 2], "misses": [7, 7, 6],
            "block_hit_rate": [0.3, 0.41, 0.25],
            "bytes_witnessed": [1400, 1400, 1200], "bytes_saved": [600, 1000, 400],
            "invalidations": [2, 1, 3], "occupancy": [8, 11, 10],
        }), results / "series" / f"run_{rid}.parquet")
        pq.write_table(pa.table({
            "address": ["0xdac17f958d2ee523a2206206994597c13d831ec7",
                        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"],
            "invalidations": [500, 300, 100],
        }), results / "contracts" / f"run_{rid}.parquet")

    # Minimal dataset_stats.json so rank-labelling has something to match.
    dataset_dir = tmp_path / "out" / "dataset"
    dataset_dir.mkdir(parents=True)
    (dataset_dir / "dataset_stats.json").write_text(json.dumps({
        "top_contracts": [
            {"address": "0xdac17f958d2ee523a2206206994597c13d831ec7", "storage_accesses": 9},
            {"address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", "storage_accesses": 8},
        ]
    }))
    return results, dataset_dir
