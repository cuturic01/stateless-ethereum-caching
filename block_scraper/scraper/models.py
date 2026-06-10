from __future__ import annotations

from dataclasses import dataclass

Key = tuple[bytes, bytes | None]

ADDR_LEN = 20
SLOT_LEN = 32


def addr_to_bytes(addr_hex: str) -> bytes:
    h = addr_hex[2:] if addr_hex.startswith(("0x", "0X")) else addr_hex
    b = bytes.fromhex(h.rjust(ADDR_LEN * 2, "0"))
    if len(b) != ADDR_LEN:
        raise ValueError(f"address must be {ADDR_LEN} bytes, got {len(b)}: {addr_hex!r}")
    return b


def slot_to_bytes(slot_hex: str) -> bytes:
    h = slot_hex[2:] if slot_hex.startswith(("0x", "0X")) else slot_hex
    b = bytes.fromhex(h.rjust(SLOT_LEN * 2, "0"))
    if len(b) != SLOT_LEN:
        raise ValueError(f"slot must be {SLOT_LEN} bytes, got {len(b)}: {slot_hex!r}")
    return b


@dataclass(slots=True)
class BlockRecord:
    block_number: int
    timestamp: int
    read_set: list[Key]
    write_set: list[Key]

    def to_wire(self) -> list:
        return [
            self.block_number,
            self.timestamp,
            [[a, s] for (a, s) in self.read_set],
            [[a, s] for (a, s) in self.write_set],
        ]

    @classmethod
    def from_wire(cls, obj: list) -> BlockRecord:
        bn, ts, reads, writes = obj
        return cls(
            block_number=bn,
            timestamp=ts,
            read_set=[(a, s) for (a, s) in reads],
            write_set=[(a, s) for (a, s) in writes],
        )
