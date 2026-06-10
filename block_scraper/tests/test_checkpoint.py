"""Round-trip tests for sharded writing, manifest, checkpoint, and resume."""

import msgpack
import zstandard as zstd

from scraper.checkpoint import Manifest
from scraper.models import BlockRecord, addr_to_bytes
from scraper.writer import ShardStore

A = addr_to_bytes("0x" + "11" * 20)


def make_record(bn: int) -> BlockRecord:
    return BlockRecord(block_number=bn, timestamp=1000 + bn, read_set=[(A, None)], write_set=[])


def new_store(tmp_path, start, end, shard_size):
    manifest = Manifest(
        network="eth-mainnet", start_block=start, end_block=end, shard_size=shard_size
    )
    manifest.save(tmp_path)
    return ShardStore(tmp_path, manifest)


def test_shard_flushes_when_full(tmp_path):
    store = new_store(tmp_path, 100, 109, shard_size=5)  # 2 shards of 5
    flushed = [store.add(make_record(bn)) for bn in range(100, 105)]
    # First four buffered (None), fifth triggers flush of shard 0.
    assert flushed[:4] == [None, None, None, None]
    assert flushed[4] == 0
    assert store.checkpoint.completed_shards == [0]
    assert store.checkpoint.highest_contiguous_block == 104
    assert (tmp_path / "shards" / "blocks_00000.msgpack.zst").exists()


def test_shard_contents_roundtrip(tmp_path):
    store = new_store(tmp_path, 0, 2, shard_size=3)
    for bn in range(3):
        store.add(make_record(bn))
    blob = (tmp_path / "shards" / "blocks_00000.msgpack.zst").read_bytes()
    records = msgpack.unpackb(zstd.ZstdDecompressor().decompress(blob), raw=False)
    parsed = [BlockRecord.from_wire(r) for r in records]
    assert [p.block_number for p in parsed] == [0, 1, 2]
    assert parsed[0].read_set == [(A, None)]


def test_out_of_order_arrival(tmp_path):
    store = new_store(tmp_path, 10, 12, shard_size=3)
    for bn in (12, 10, 11):  # arrive out of order
        store.add(make_record(bn))
    blob = (tmp_path / "shards" / "blocks_00000.msgpack.zst").read_bytes()
    records = msgpack.unpackb(zstd.ZstdDecompressor().decompress(blob), raw=False)
    assert [r[0] for r in records] == [10, 11, 12]  # sorted on flush


def test_resume_skips_completed_shards(tmp_path):
    store = new_store(tmp_path, 0, 9, shard_size=5)
    for bn in range(5):  # complete shard 0 only
        store.add(make_record(bn))
    assert store.checkpoint.completed_shards == [0]

    # Reopen as if resuming: pending should be the second shard only.
    manifest = Manifest.load(tmp_path)
    store2 = ShardStore(tmp_path, manifest)
    assert store2.pending_block_numbers() == list(range(5, 10))
    assert store2.completed_block_numbers() == set(range(0, 5))


def test_final_partial_shard_flushes(tmp_path):
    # Range 0..6 with shard_size 5 => shard 0 (0..4) full, shard 1 (5..6) is partial=2.
    store = new_store(tmp_path, 0, 6, shard_size=5)
    for bn in range(7):
        store.add(make_record(bn))
    # Shard 1 expected count is 2, so it auto-flushes once both present.
    assert store.checkpoint.completed_shards == [0, 1]
    assert store.checkpoint.highest_contiguous_block == 6


def test_incomplete_shard_not_marked_done(tmp_path):
    store = new_store(tmp_path, 0, 9, shard_size=5)
    store.add(make_record(0))  # only one of five in shard 0
    assert store.flush_complete() == []
    assert store.incomplete_shards() == {0: 1}
    assert store.checkpoint.completed_shards == []
