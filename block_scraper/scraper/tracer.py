from __future__ import annotations

from collections.abc import Iterable

from .models import Key, addr_to_bytes, slot_to_bytes

# Account-header fields (everything that is not per-slot storage).
_HEADER_FIELDS = ("balance", "nonce", "code")


def _unwrap_tx_entries(trace: list) -> Iterable[dict]:
    for entry in trace:
        if isinstance(entry, dict) and "result" in entry:
            yield entry["result"]
        else:
            yield entry


def _key(addr: str, slot: str | None) -> Key:
    return (addr_to_bytes(addr), None if slot is None else slot_to_bytes(slot))


def parse_read_set(trace: list) -> list[Key]:
    keys: set[Key] = set()
    for prestate in _unwrap_tx_entries(trace):
        if not prestate:
            continue
        for addr, state in prestate.items():
            keys.add(_key(addr, None))  # account header
            for slot in (state or {}).get("storage", {}) or {}:
                keys.add(_key(addr, slot))
    return list(keys)


def parse_write_set(trace: list) -> list[Key]:
   keys: set[Key] = set()
    for diff in _unwrap_tx_entries(trace):
        if not diff:
            continue
        pre = diff.get("pre", {}) or {}
        post = diff.get("post", {}) or {}
        for addr in set(pre) | set(post):
            pre_a = pre.get(addr, {}) or {}
            post_a = post.get(addr, {}) or {}

            header_changed = (addr in pre) != (addr in post) or any(
                pre_a.get(f) != post_a.get(f) for f in _HEADER_FIELDS
            )
            if header_changed:
                keys.add(_key(addr, None))

            pre_storage = pre_a.get("storage", {}) or {}
            post_storage = post_a.get("storage", {}) or {}
            for slot in set(pre_storage) | set(post_storage):
                if pre_storage.get(slot) != post_storage.get(slot):
                    keys.add(_key(addr, slot))
    return list(keys)
