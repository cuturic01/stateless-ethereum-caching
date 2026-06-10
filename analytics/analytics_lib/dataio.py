from __future__ import annotations

import json
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

import msgpack
import zstandard as zstd

Key = tuple[bytes, bytes | None]


@dataclass(slots=True)
class Block:
    number: int
    timestamp: int
    read_set: list[Key]
    write_set: list[Key]


def load_manifest(data_dir: Path) -> dict:
    p = data_dir / "manifest.json"
    if not p.exists():
        raise FileNotFoundError(
            f"No manifest at {p}. Run the scraper first (see block_scraper/README.md)."
        )
    return json.loads(p.read_text())


def _decode_key(k: list) -> Key:
    addr, slot = k
    return (addr, slot)


def iter_blocks(data_dir: Path) -> Iterator[Block]:
    """Yield every scraped block in ascending block order."""
    manifest = load_manifest(data_dir)
    dctx = zstd.ZstdDecompressor()
    shards = sorted(manifest["shards"], key=lambda s: s["first_block"])
    for shard in shards:
        blob = (data_dir / shard["file"]).read_bytes()
        records = msgpack.unpackb(dctx.decompress(blob), raw=False)
        for bn, ts, reads, writes in records:
            yield Block(
                number=bn,
                timestamp=ts,
                read_set=[_decode_key(k) for k in reads],
                write_set=[_decode_key(k) for k in writes],
            )


def block_count(data_dir: Path) -> int:
    return sum(s["count"] for s in load_manifest(data_dir)["shards"])
