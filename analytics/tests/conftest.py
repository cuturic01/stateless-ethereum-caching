"""Synthetic dataset fixture mirroring the block_scraper on-disk format."""

import hashlib
import json

import msgpack
import pytest
import zstandard as zstd


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
