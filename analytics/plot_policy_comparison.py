"""H4 — headline ARC vs LFU vs LRU comparison at a canonical window.

Two panels at window N=32 (the H2 operating point), stratum=all:
  (left)  grouped bars: compression per policy at each capacity — shows
          ARC >= LFU > LRU below 100% and convergence at 100%.
  (right) ARC and LFU advantage over LRU, in compression points, vs capacity.

ARC results use the corrected algorithm (Stage 3 fixed a collapse under external
write-invalidation; regression-tested).
"""

from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from rich.console import Console  # noqa: E402

from analytics_lib.resultio import CAPS, POLICIES, load_runs, select  # noqa: E402

console = Console()

CANONICAL_WINDOW = 32


def _comp(runs, policy: str, cap: int, window: int) -> float:
    mask = select(runs, policy=policy, capacity_pct=cap, window=window, stratum="all")
    return float(runs["overall_compression_ratio"][mask][0]) if mask.any() else np.nan


def make_plots(runs, out: Path, window: int = CANONICAL_WINDOW) -> None:
    by_policy = {p: np.asarray([_comp(runs, p, c, window) for c in CAPS]) for p in POLICIES}

    fig, (axbar, axadv) = plt.subplots(1, 2, figsize=(12, 5))

    x = np.arange(len(CAPS))
    width = 0.25
    for i, pol in enumerate(POLICIES):
        axbar.bar(x + (i - 1) * width, by_policy[pol], width, label=pol.upper())
    axbar.set_xticks(x)
    axbar.set_xticklabels([f"{c}%" for c in CAPS])
    axbar.set_xlabel("capacity (% of working set)")
    axbar.set_ylabel("compression (= hit rate)")
    axbar.set_title(f"Policy compression (N = {window})")
    axbar.legend(fontsize=8)

    lru = by_policy["lru"]
    axadv.plot(CAPS, by_policy["arc"] - lru, "o-", label="ARC − LRU")
    axadv.plot(CAPS, by_policy["lfu"] - lru, "s-", label="LFU − LRU")
    axadv.axhline(0, color="grey", lw=0.8)
    axadv.set_xlabel("capacity (% of working set)")
    axadv.set_ylabel("compression advantage over LRU")
    axadv.set_title("Adaptive/frequency gain over recency")
    axadv.set_xticks(CAPS)
    axadv.legend(fontsize=8)

    fig.suptitle("ARC vs LFU vs LRU (H4)")
    fig.tight_layout()
    fig.savefig(out / "policy_comparison.png", dpi=120)
    plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot ARC vs LFU vs LRU comparison (H4)")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    parser.add_argument("--window", type=int, default=CANONICAL_WINDOW)
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    runs = load_runs(Path(args.results_dir))
    make_plots(runs, out, window=args.window)
    console.print(f"[green]Wrote policy_comparison.png to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
