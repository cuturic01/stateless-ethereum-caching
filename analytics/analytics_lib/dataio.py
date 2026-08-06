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


# --- EIP-6800 stem derivation -------------------------------------------------
# Must agree exactly with key_to_leaf in caching_strategies/src/witness.rs. The
# shared fixture at fixtures/stem_derivation.json is what stops the two drifting.

HEADER_STORAGE_OFFSET = 64
_ZERO31 = bytes(31)

Stem = tuple[bytes, bool, bytes]


def key_to_stem(addr: bytes, slot: bytes | None) -> Stem:
    """The stem a (address, slot) key lives under.

    The account header and storage slots 0..63 share one stem; slots >= 64 group
    into stems of 256 consecutive slots. Note the header test needs the *whole*
    slot below 64, not just its low byte: slot 261 has a low byte of 5 but is
    main storage.
    """
    if slot is None:
        return (addr, False, _ZERO31)
    high, low = slot[:31], slot[31]
    if high == _ZERO31 and low < HEADER_STORAGE_OFFSET:
        return (addr, False, _ZERO31)
    return (addr, True, high)


def key_to_leaf(addr: bytes, slot: bytes | None) -> tuple[Stem, int]:
    """The stem plus the suffix within it (0..255)."""
    stem = key_to_stem(addr, slot)
    if slot is None:
        return stem, 0
    if not stem[1]:
        return stem, HEADER_STORAGE_OFFSET + slot[31]
    return stem, slot[31]
