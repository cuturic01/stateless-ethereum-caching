from __future__ import annotations

import hashlib
import math
import os
from pathlib import Path

import msgpack
import zstandard as zstd

from .checkpoint import Checkpoint, Manifest, ShardEntry
from .models import BlockRecord

SHARDS_SUBDIR = "shards"


def shard_filename(index: int) -> str:
    return f"blocks_{index:05d}.msgpack.zst"


class ShardStore:
    def __init__(self, data_dir: Path, manifest: Manifest, *, zstd_level: int = 10):
        self.data_dir = data_dir
        self.manifest = manifest
        self.checkpoint = Checkpoint.load(data_dir)
        self._cctx = zstd.ZstdCompressor(level=zstd_level)
        self._buffer: dict[int, dict[int, BlockRecord]] = {}
        (data_dir / SHARDS_SUBDIR).mkdir(parents=True, exist_ok=True)

    # ---- shard geometry ----
    @property
    def n_shards(self) -> int:
        span = self.manifest.end_block - self.manifest.start_block + 1
        return math.ceil(span / self.manifest.shard_size)

    def shard_index(self, block_number: int) -> int:
        return (block_number - self.manifest.start_block) // self.manifest.shard_size

    def shard_range(self, index: int) -> tuple[int, int]:
        first = self.manifest.start_block + index * self.manifest.shard_size
        last = min(first + self.manifest.shard_size - 1, self.manifest.end_block)
        return first, last

    def shard_expected_count(self, index: int) -> int:
        first, last = self.shard_range(index)
        return last - first + 1

    def completed_block_numbers(self) -> set[int]:
        done: set[int] = set()
        for idx in self.checkpoint.completed_shards:
            first, last = self.shard_range(idx)
            done.update(range(first, last + 1))
        return done

    def pending_block_numbers(self) -> list[int]:
        done = self.completed_block_numbers()
        return [
            bn
            for bn in range(self.manifest.start_block, self.manifest.end_block + 1)
            if bn not in done
        ]

    def add(self, record: BlockRecord) -> int | None:
        idx = self.shard_index(record.block_number)
        if idx in self.checkpoint.completed_shards:
            return None
        self._buffer.setdefault(idx, {})[record.block_number] = record
        if len(self._buffer[idx]) >= self.shard_expected_count(idx):
            self._flush(idx)
            return idx
        return None

    def _flush(self, index: int) -> None:
        records = sorted(self._buffer[index].values(), key=lambda r: r.block_number)
        first, last = records[0].block_number, records[-1].block_number
        packed = msgpack.packb([r.to_wire() for r in records], use_bin_type=True)
        blob = self._cctx.compress(packed)

        fname = shard_filename(index)
        out = self.data_dir / SHARDS_SUBDIR / fname
        tmp = out.with_suffix(out.suffix + ".tmp")
        tmp.write_bytes(blob)
        os.replace(tmp, out)

        entry = ShardEntry(
            file=f"{SHARDS_SUBDIR}/{fname}",
            first_block=first,
            last_block=last,
            count=len(records),
            sha256=hashlib.sha256(blob).hexdigest(),
        )
        self.manifest.upsert_shard(entry)
        self.manifest.save(self.data_dir)

        if index not in self.checkpoint.completed_shards:
            self.checkpoint.completed_shards.append(index)
            self.checkpoint.completed_shards.sort()
        self.checkpoint.highest_contiguous_block = self._highest_contiguous()
        self.checkpoint.save(self.data_dir)

        del self._buffer[index]

    def _highest_contiguous(self) -> int:
        block = self.manifest.start_block - 1
        done = set(self.checkpoint.completed_shards)
        for idx in range(self.n_shards):
            if idx in done:
                block = self.shard_range(idx)[1]
            else:
                break
        return block

    def flush_complete(self) -> list[int]:
        flushed = []
        for idx in list(self._buffer.keys()):
            if len(self._buffer[idx]) >= self.shard_expected_count(idx):
                self._flush(idx)
                flushed.append(idx)
        return flushed

    def incomplete_shards(self) -> dict[int, int]:
        return {
            idx: len(buf)
            for idx, buf in self._buffer.items()
            if 0 < len(buf) < self.shard_expected_count(idx)
        }
