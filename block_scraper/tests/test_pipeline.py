"""Offline end-to-end test of run_scrape using a fake RPC (no network)."""

import asyncio

import msgpack
import zstandard as zstd

from scraper.checkpoint import Manifest
from scraper.models import BlockRecord, addr_to_bytes
from scraper.pipeline import run_scrape
from scraper.writer import ShardStore


class FakeRPC:
    """Stand-in for AlchemyRPC: deterministic synthetic traces per block."""

    def __init__(self):
        self.cu_spent = 0
        self.retry_count = 0

    def _addr(self, bn: int) -> str:
        return "0x" + f"{bn:040x}"

    async def trace_block(self, bn: int, *, diff: bool):
        self.cu_spent += 40
        await asyncio.sleep(0)
        if diff:
            # block bn writes slot 0 of its own address
            return [{"result": {"pre": {self._addr(bn): {"storage": {"0x0": "0x1"}}},
                                "post": {self._addr(bn): {"storage": {"0x0": "0x2"}}}}}]
        # reads its own account header + slot 0
        return [{"result": {self._addr(bn): {"balance": "0x1", "storage": {"0x0": "0x2"}}}}]

    async def get_block_timestamp(self, bn: int) -> int:
        self.cu_spent += 20
        await asyncio.sleep(0)
        return 1_700_000_000 + bn


def test_run_scrape_end_to_end(tmp_path):
    start, end, shard_size = 0, 9, 5
    manifest = Manifest(network="eth-mainnet", start_block=start, end_block=end,
                        shard_size=shard_size)
    manifest.save(tmp_path)
    store = ShardStore(tmp_path, manifest)
    rpc = FakeRPC()

    asyncio.run(run_scrape(rpc, store, n_workers=4))

    # Two full shards written, run complete.
    assert store.checkpoint.completed_shards == [0, 1]
    assert store.checkpoint.highest_contiguous_block == 9
    assert len(store.manifest.shards) == 2

    # Decode a shard and check parsed read/write sets.
    blob = (tmp_path / "shards" / "blocks_00000.msgpack.zst").read_bytes()
    records = [BlockRecord.from_wire(r)
               for r in msgpack.unpackb(zstd.ZstdDecompressor().decompress(blob), raw=False)]
    assert [r.block_number for r in records] == [0, 1, 2, 3, 4]
    rec = records[3]
    a = addr_to_bytes("0x" + f"{3:040x}")
    assert set(rec.read_set) == {(a, None), (a, b"\x00" * 32)}
    assert set(rec.write_set) == {(a, b"\x00" * 32)}


def test_resume_after_partial(tmp_path):
    manifest = Manifest(network="eth-mainnet", start_block=0, end_block=9, shard_size=5)
    manifest.save(tmp_path)

    # First pass: only feed shard 0's blocks, simulating an interruption.
    store = ShardStore(tmp_path, manifest)
    rpc = FakeRPC()

    async def partial():
        from scraper.pipeline import _fetch_block
        for bn in range(5):
            store.add(await _fetch_block(rpc, bn))
    asyncio.run(partial())
    assert store.checkpoint.completed_shards == [0]

    # Resume: a fresh store sees only blocks 5..9 pending, then completes.
    store2 = ShardStore(tmp_path, Manifest.load(tmp_path))
    assert store2.pending_block_numbers() == list(range(5, 10))
    asyncio.run(run_scrape(FakeRPC(), store2, n_workers=4))
    assert store2.checkpoint.completed_shards == [0, 1]
