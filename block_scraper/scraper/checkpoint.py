from __future__ import annotations

import json
import os
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
from pathlib import Path

from . import SCHEMA_VERSION

KEY_ENCODING = "raw-bytes:addr20+slot32-or-nil"


def _atomic_write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)


def _atomic_write_json(path: Path, obj: dict) -> None:
    _atomic_write_bytes(path, json.dumps(obj, indent=2).encode())


@dataclass
class ShardEntry:
    file: str
    first_block: int
    last_block: int
    count: int
    sha256: str


@dataclass
class Manifest:
    network: str
    start_block: int
    end_block: int
    shard_size: int
    created_at: str = ""
    schema_version: int = SCHEMA_VERSION
    key_encoding: str = KEY_ENCODING
    shards: list[ShardEntry] = field(default_factory=list)

    # ---- io ----
    @classmethod
    def path(cls, data_dir: Path) -> Path:
        return data_dir / "manifest.json"

    @classmethod
    def load(cls, data_dir: Path) -> Manifest | None:
        p = cls.path(data_dir)
        if not p.exists():
            return None
        raw = json.loads(p.read_text())
        shards = [ShardEntry(**s) for s in raw.pop("shards", [])]
        return cls(**raw, shards=shards)

    def save(self, data_dir: Path) -> None:
        if not self.created_at:
            self.created_at = datetime.now(UTC).isoformat()
        obj = asdict(self)
        _atomic_write_json(self.path(data_dir), obj)

    def upsert_shard(self, entry: ShardEntry) -> None:
        for i, s in enumerate(self.shards):
            if s.file == entry.file:
                self.shards[i] = entry
                return
        self.shards.append(entry)
        self.shards.sort(key=lambda s: s.first_block)


@dataclass
class Checkpoint:
    completed_shards: list[int] = field(default_factory=list)
    highest_contiguous_block: int = -1

    @classmethod
    def path(cls, data_dir: Path) -> Path:
        return data_dir / "checkpoint.json"

    @classmethod
    def load(cls, data_dir: Path) -> Checkpoint:
        p = cls.path(data_dir)
        if not p.exists():
            return cls()
        raw = json.loads(p.read_text())
        return cls(**raw)

    def save(self, data_dir: Path) -> None:
        _atomic_write_json(self.path(data_dir), asdict(self))
