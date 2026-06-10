"""End-to-end test of dataio + characterize on a synthetic dataset."""

import json

import characterize
from analytics_lib.dataio import block_count, iter_blocks


def test_dataio_reads_in_order(synthetic_data):
    blocks = list(iter_blocks(synthetic_data))
    assert block_count(synthetic_data) == 20
    assert [b.number for b in blocks] == list(range(1000, 1020))
    # Keys decode back to (bytes addr, bytes|None slot).
    addr, slot = blocks[0].read_set[0]
    assert isinstance(addr, bytes) and len(addr) == 20
    assert slot is None or len(slot) == 32


def test_characterize_outputs(synthetic_data, tmp_path):
    out = tmp_path / "out"
    rc = characterize.main(["--data-dir", str(synthetic_data), "--out", str(out)])
    assert rc == 0

    stats = json.loads((out / "dataset_stats.json").read_text())
    assert stats["total_blocks"] == 20
    assert stats["block_range"] == [1000, 1019]
    assert stats["global_unique_keys"] > 0
    assert stats["reads_per_block"]["count"] == 20
    # Hot contracts 1 and 2 should top the storage-access ranking.
    top_addrs = {c["address"] for c in stats["top_contracts"][:2]}
    assert ("0x" + (1).to_bytes(20, "big").hex()) in top_addrs
    assert ("0x" + (2).to_bytes(20, "big").hex()) in top_addrs

    strata = json.loads((out / "strata.json").read_text())
    assert strata["counts"]["all"] == 20
    # Odd blocks are transfer-like (no storage reads) -> transfer_only.
    assert strata["counts"]["transfer_only"] >= 1
    assert len(strata["defi_contracts"]) >= 2

    assert (out / "summary.md").exists()
    assert (out / "hist_reads_writes.png").exists()
    assert (out / "working_set_vs_window.png").exists()
