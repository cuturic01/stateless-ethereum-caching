"""The Python stem derivation must match the Rust one exactly.

Both sides consume fixtures/stem_derivation.json. If characterize.py's stem
working set disagreed with witness.rs's, the stem cache would be sized against
the wrong number and nothing would fail loudly.
"""

import json
from pathlib import Path

import pytest

from analytics_lib.dataio import HEADER_STORAGE_OFFSET, key_to_leaf, key_to_stem

FIXTURE = Path(__file__).resolve().parents[2] / "fixtures" / "stem_derivation.json"


def _cases():
    doc = json.loads(FIXTURE.read_text())
    assert doc["header_storage_offset"] == HEADER_STORAGE_OFFSET
    return doc["cases"]


def _unhex(s):
    return bytes.fromhex(s.removeprefix("0x"))


@pytest.mark.parametrize("case", _cases(), ids=lambda c: c["name"])
def test_matches_shared_fixture(case):
    addr = _unhex(case["addr"])
    slot = None if case["slot"] is None else _unhex(case["slot"])
    (got_addr, got_storage, got_chunk), suffix = key_to_leaf(addr, slot)

    assert got_addr == addr
    assert got_storage == case["storage"]
    assert got_chunk.hex() == case["chunk"]
    assert suffix == case["suffix"]


def test_header_and_low_slots_collapse_to_one_stem():
    addr = bytes(20)
    header = key_to_stem(addr, None)
    for n in (0, 1, 63):
        assert key_to_stem(addr, n.to_bytes(32, "big")) == header
    assert key_to_stem(addr, (64).to_bytes(32, "big")) != header


def test_slots_group_in_blocks_of_256():
    addr = bytes(20)

    def stem(n):
        return key_to_stem(addr, n.to_bytes(32, "big"))

    assert stem(300) == stem(400)  # both in 256..511
    assert stem(300) != stem(556)  # 556 is 300 + 256, next chunk
