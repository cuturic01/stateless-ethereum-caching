"""Golden tests for prestateTracer parsing into read/write key sets."""

from scraper.models import addr_to_bytes, slot_to_bytes
from scraper.tracer import parse_read_set, parse_write_set

A = "0x" + "11" * 20  # an account
B = "0x" + "22" * 20  # another account
S0 = "0x" + "00" * 32
S1 = "0x" + "00" * 31 + "01"


def key(addr, slot=None):
    return (addr_to_bytes(addr), None if slot is None else slot_to_bytes(slot))


# ---- read set (default-mode prestate) ----

def test_read_set_account_and_storage():
    # One tx touching account A (header + two slots) and account B (header only).
    trace = [
        {
            "result": {
                A: {"balance": "0x1", "nonce": "0x0", "storage": {S0: "0xaa", S1: "0xbb"}},
                B: {"balance": "0x5"},
            }
        }
    ]
    reads = set(parse_read_set(trace))
    assert reads == {key(A), key(A, S0), key(A, S1), key(B)}


def test_read_set_unions_across_txs_and_dedups():
    trace = [
        {"result": {A: {"balance": "0x1", "storage": {S0: "0x1"}}}},
        {"result": {A: {"balance": "0x1", "storage": {S0: "0x1", S1: "0x2"}}}},
    ]
    reads = set(parse_read_set(trace))
    assert reads == {key(A), key(A, S0), key(A, S1)}


def test_read_set_tolerates_unwrapped_entries_and_empty():
    trace = [{A: {"balance": "0x1"}}, {"result": None}, {}]
    assert set(parse_read_set(trace)) == {key(A)}


# ---- write set (diffMode) ----

def test_write_set_storage_and_header_change():
    # A: slot S0 changed value, balance changed (header write). S1 unchanged -> not a write.
    trace = [
        {
            "result": {
                "pre": {A: {"balance": "0x1", "storage": {S0: "0x1", S1: "0x9"}}},
                "post": {A: {"balance": "0x2", "storage": {S0: "0x2", S1: "0x9"}}},
            }
        }
    ]
    writes = set(parse_write_set(trace))
    assert writes == {key(A), key(A, S0)}


def test_write_set_selfdestruct_is_invalidation():
    # B exists in pre, absent from post (self-destruct): header + all its slots invalidated.
    trace = [
        {
            "result": {
                "pre": {B: {"balance": "0x5", "storage": {S0: "0x7", S1: "0x8"}}},
                "post": {},
            }
        }
    ]
    writes = set(parse_write_set(trace))
    assert writes == {key(B), key(B, S0), key(B, S1)}


def test_write_set_creation_is_write():
    # A absent in pre, present in post (account creation).
    trace = [{"result": {"pre": {}, "post": {A: {"balance": "0x3", "storage": {S0: "0x1"}}}}}]
    writes = set(parse_write_set(trace))
    assert writes == {key(A), key(A, S0)}


def test_write_set_slot_zeroed_only_in_pre():
    # Slot S1 set to zero: appears in pre but not post -> still a write. Balance unchanged.
    trace = [
        {
            "result": {
                "pre": {A: {"balance": "0x1", "storage": {S1: "0x4"}}},
                "post": {A: {"balance": "0x1", "storage": {}}},
            }
        }
    ]
    writes = set(parse_write_set(trace))
    assert writes == {key(A, S1)}


def test_write_set_empty_diff_has_no_writes():
    assert parse_write_set([{"result": {"pre": {}, "post": {}}}]) == []
