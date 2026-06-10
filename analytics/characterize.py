from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from rich.console import Console  # noqa: E402

from analytics_lib.dataio import Block, iter_blocks, load_manifest  # noqa: E402

console = Console()

WINDOWS = [8, 16, 32, 64, 128]  # block-window sizes the Rust sweep also uses
N_TOP_CONTRACTS = 30  # size of the "defi" contract set
GROWTH_SAMPLES = 500  # data points in the cumulative-unique-keys curve


def _key_hash(addr: bytes, slot: bytes | None) -> int:
    return hash(addr if slot is None else addr + slot)


class Pass1:
    def __init__(self, total_blocks: int):
        self.total_blocks = total_blocks
        self.sample_every = max(1, total_blocks // GROWTH_SAMPLES)

        # Per-block scalars.
        self.reads: list[int] = []
        self.writes: list[int] = []
        self.read_storage: list[int] = []
        self.write_storage: list[int] = []
        self.timestamps: list[int] = []
        self.numbers: list[int] = []

        # Global distinct keys (hashed) + growth curve.
        self.global_keys: set[int] = set()
        self.growth: list[tuple[int, int]] = []  # (block_index, cumulative_unique)

        # Per-window non-overlapping working sets.
        self._acc: dict[int, set[int]] = {n: set() for n in WINDOWS}
        self._cnt: dict[int, int] = {n: 0 for n in WINDOWS}
        self.window_sizes: dict[int, list[int]] = {n: [] for n in WINDOWS}

        # Storage access counts per contract address.
        self.addr_storage_reads: Counter[bytes] = Counter()
        self.addr_storage_writes: Counter[bytes] = Counter()

    def feed(self, i: int, b: Block) -> None:
        self.numbers.append(b.number)
        self.timestamps.append(b.timestamp)
        self.reads.append(len(b.read_set))
        self.writes.append(len(b.write_set))

        rs = 0
        block_hashes: set[int] = set()
        for addr, slot in b.read_set:
            block_hashes.add(_key_hash(addr, slot))
            if slot is not None:
                rs += 1
                self.addr_storage_reads[addr] += 1
        self.read_storage.append(rs)

        ws = 0
        for addr, slot in b.write_set:
            if slot is not None:
                ws += 1
                self.addr_storage_writes[addr] += 1
        self.write_storage.append(ws)

        self.global_keys |= block_hashes
        if i % self.sample_every == 0 or i == self.total_blocks - 1:
            self.growth.append((i, len(self.global_keys)))

        for n in WINDOWS:
            self._acc[n] |= block_hashes
            self._cnt[n] += 1
            if self._cnt[n] == n:
                self.window_sizes[n].append(len(self._acc[n]))
                self._acc[n] = set()
                self._cnt[n] = 0

    def finalize(self) -> None:
        # Flush trailing partial windows so short datasets still report something.
        for n in WINDOWS:
            if self._cnt[n] > 0:
                self.window_sizes[n].append(len(self._acc[n]))


def _stats(arr: list[int]) -> dict:
    a = np.asarray(arr, dtype=np.int64)
    if a.size == 0:
        return {"count": 0}
    return {
        "count": int(a.size),
        "min": int(a.min()),
        "max": int(a.max()),
        "mean": float(a.mean()),
        "median": float(np.median(a)),
        "p95": float(np.percentile(a, 95)),
        "p99": float(np.percentile(a, 99)),
        "total": int(a.sum()),
    }


def run_pass1(data_dir: Path, total_blocks: int) -> Pass1:
    p1 = Pass1(total_blocks)
    with console.status("Pass 1/2: scanning blocks..."):
        for i, b in enumerate(iter_blocks(data_dir)):
            p1.feed(i, b)
    p1.finalize()
    return p1


def run_pass2(data_dir: Path, defi_set: set[bytes], thresholds: dict) -> dict:
    transfer_max = thresholds["transfer_storage_max"]
    defi_min = thresholds["defi_storage_min"]
    defi_heavy: list[int] = []
    transfer_only: list[int] = []
    with console.status("Pass 2/2: classifying strata..."):
        for b in iter_blocks(data_dir):
            storage_reads = 0
            defi_hits = 0
            for addr, slot in b.read_set:
                if slot is not None:
                    storage_reads += 1
                    if addr in defi_set:
                        defi_hits += 1
            if storage_reads <= transfer_max:
                transfer_only.append(b.number)
            defi_frac = (defi_hits / storage_reads) if storage_reads else 0.0
            if defi_frac >= 0.5 and storage_reads >= defi_min:
                defi_heavy.append(b.number)
    return {"defi_heavy": defi_heavy, "transfer_only": transfer_only}


def make_plots(p1: Pass1, out: Path) -> None:
    # Reads vs writes per block.
    fig, ax = plt.subplots(figsize=(8, 4))
    ax.hist(p1.reads, bins=60, alpha=0.6, label="reads/block")
    ax.hist(p1.writes, bins=60, alpha=0.6, label="writes/block")
    ax.set_xlabel("keys per block")
    ax.set_ylabel("# blocks")
    ax.set_title("Read / write set size per block")
    ax.legend()
    fig.tight_layout()
    fig.savefig(out / "hist_reads_writes.png", dpi=120)
    plt.close(fig)

    # Cumulative unique keys.
    if p1.growth:
        xs, ys = zip(*p1.growth, strict=True)
        fig, ax = plt.subplots(figsize=(8, 4))
        ax.plot(xs, ys)
        ax.set_xlabel("block index")
        ax.set_ylabel("cumulative unique keys")
        ax.set_title("Working-set growth")
        fig.tight_layout()
        fig.savefig(out / "cumulative_unique_keys.png", dpi=120)
        plt.close(fig)

    # Working set vs window size.
    means = [float(np.mean(p1.window_sizes[n])) if p1.window_sizes[n] else 0 for n in WINDOWS]
    maxes = [float(np.max(p1.window_sizes[n])) if p1.window_sizes[n] else 0 for n in WINDOWS]
    fig, ax = plt.subplots(figsize=(8, 4))
    ax.plot(WINDOWS, means, "o-", label="mean")
    ax.plot(WINDOWS, maxes, "s--", label="max")
    ax.set_xlabel("window size (blocks)")
    ax.set_ylabel("distinct keys in window")
    ax.set_title("Working set vs window size")
    ax.legend()
    fig.tight_layout()
    fig.savefig(out / "working_set_vs_window.png", dpi=120)
    plt.close(fig)

    # Top contracts by storage access.
    top = (p1.addr_storage_reads + p1.addr_storage_writes).most_common(20)
    if top:
        labels = ["0x" + a.hex()[:8] for a, _ in top]
        vals = [c for _, c in top]
        fig, ax = plt.subplots(figsize=(8, 5))
        ax.barh(range(len(vals)), vals)
        ax.set_yticks(range(len(labels)))
        ax.set_yticklabels(labels, fontsize=7)
        ax.invert_yaxis()
        ax.set_xlabel("storage accesses (read+write)")
        ax.set_title("Top 20 contracts by storage access")
        fig.tight_layout()
        fig.savefig(out / "top_contracts.png", dpi=120)
        plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Characterize the scraped block dataset")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--data-dir", default=str(repo_root / "data"))
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "dataset"))
    args = parser.parse_args(argv)

    data_dir = Path(args.data_dir)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    manifest = load_manifest(data_dir)
    total_blocks = sum(s["count"] for s in manifest["shards"])
    if total_blocks == 0:
        console.print("[red]No blocks in dataset. Run the scraper first.")
        return 1

    p1 = run_pass1(data_dir, total_blocks)

    # Define the "defi" contract set and stratification thresholds.
    combined = p1.addr_storage_reads + p1.addr_storage_writes
    defi_contracts = [a for a, _ in combined.most_common(N_TOP_CONTRACTS)]
    defi_set = set(defi_contracts)
    storage_arr = np.asarray(p1.read_storage, dtype=np.int64)
    thresholds = {
        "transfer_storage_max": int(np.percentile(storage_arr, 25)) if storage_arr.size else 0,
        "defi_storage_min": int(np.median(storage_arr)) if storage_arr.size else 0,
    }

    strata_blocks = run_pass2(data_dir, defi_set, thresholds)

    # ---- assemble outputs ----
    timespan_s = (max(p1.timestamps) - min(p1.timestamps)) if p1.timestamps else 0
    window_working_set = {
        str(n): _stats(p1.window_sizes[n]) for n in WINDOWS
    }
    stats = {
        "network": manifest["network"],
        "block_range": [manifest["start_block"], manifest["end_block"]],
        "total_blocks": total_blocks,
        "timespan_seconds": timespan_s,
        "timespan_hours": round(timespan_s / 3600, 2),
        "global_unique_keys": len(p1.global_keys),
        "reads_per_block": _stats(p1.reads),
        "writes_per_block": _stats(p1.writes),
        "read_storage_per_block": _stats(p1.read_storage),
        "write_storage_per_block": _stats(p1.write_storage),
        "read_write_ratio": (
            sum(p1.reads) / sum(p1.writes) if sum(p1.writes) else None
        ),
        "window_working_set": window_working_set,
        "top_contracts": [
            {"address": "0x" + a.hex(), "storage_accesses": int(c)}
            for a, c in combined.most_common(N_TOP_CONTRACTS)
        ],
        "memory_projection": {
            "key_count": len(p1.global_keys),
            "bytes_per_entry_estimate": 72,  # 32B verkle key + 32B value + metadata
            "projected_cache_bytes_full_workingset": len(p1.global_keys) * 72,
        },
    }
    (out / "dataset_stats.json").write_text(json.dumps(stats, indent=2))

    strata = {
        "thresholds": thresholds,
        "defi_contracts": ["0x" + a.hex() for a in defi_contracts],
        "counts": {
            "all": total_blocks,
            "defi_heavy": len(strata_blocks["defi_heavy"]),
            "transfer_only": len(strata_blocks["transfer_only"]),
        },
        "blocks": strata_blocks,
    }
    (out / "strata.json").write_text(json.dumps(strata, indent=2))

    make_plots(p1, out)
    _write_summary(out, stats, strata)

    console.print(f"[green]Wrote dataset_stats.json, strata.json, summary.md, and plots to {out}")
    console.print(
        f"  blocks={total_blocks}  unique_keys={len(p1.global_keys):,}  "
        f"defi_heavy={len(strata_blocks['defi_heavy'])}  "
        f"transfer_only={len(strata_blocks['transfer_only'])}"
    )
    return 0


def _write_summary(out: Path, stats: dict, strata: dict) -> None:
    r = stats["reads_per_block"]
    w = stats["writes_per_block"]
    lines = [
        "# Dataset summary",
        "",
        f"- Network: **{stats['network']}**",
        f"- Block range: `{stats['block_range'][0]}`..`{stats['block_range'][1]}` "
        f"({stats['total_blocks']:,} blocks, ~{stats['timespan_hours']} h)",
        f"- Global unique keys: **{stats['global_unique_keys']:,}**",
        f"- Reads/block: mean {r.get('mean', 0):.1f}, median {r.get('median', 0):.0f}, "
        f"p99 {r.get('p99', 0):.0f}, max {r.get('max', 0)}",
        f"- Writes/block: mean {w.get('mean', 0):.1f}, median {w.get('median', 0):.0f}, "
        f"p99 {w.get('p99', 0):.0f}, max {w.get('max', 0)}",
        f"- Read/write ratio: {stats['read_write_ratio']}",
        "",
        "## Working set vs window (distinct keys, non-overlapping windows)",
        "",
        "| window (blocks) | mean | max |",
        "|---|---|---|",
    ]
    for n in WINDOWS:
        ws = stats["window_working_set"][str(n)]
        lines.append(f"| {n} | {ws.get('mean', 0):.0f} | {ws.get('max', 0)} |")
    lines += [
        "",
        "## Strata",
        "",
        f"- defi-heavy blocks: {strata['counts']['defi_heavy']:,}",
        f"- transfer-only blocks: {strata['counts']['transfer_only']:,}",
        f"- defi contract set size: {len(strata['defi_contracts'])}",
        "",
        "## Top contracts by storage access",
        "",
    ]
    for c in stats["top_contracts"][:10]:
        lines.append(f"- `{c['address']}` — {c['storage_accesses']:,}")
    (out / "summary.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
