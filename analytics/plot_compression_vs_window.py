"""H1 / H2 — compression vs retention window N.

H1: cross-block redundancy is substantial (compression >> 0.10).
H2: compression rises fast toward N~32 then gains diminish. We also plot the
marginal gain Δcompression/ΔN to make the inflection visible numerically.

Witness size is 200 B/item (Oberst 2025 + EIP-6800); compression here is the hit
rate = bytes_saved / bytes_naive.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from rich.console import Console  # noqa: E402

from analytics_lib.resultio import CAPS, POLICIES, WINDOWS, load_runs, select  # noqa: E402

console = Console()


def _comp_by_window(runs, **filters) -> np.ndarray:
    """compression at each window in WINDOWS for the given filters (NaN if absent)."""
    out = []
    for w in WINDOWS:
        mask = select(runs, window=w, **filters)
        out.append(float(runs["overall_compression_ratio"][mask][0]) if mask.any() else np.nan)
    return np.asarray(out)


def make_plots(runs, out: Path) -> None:
    # Panel A: one line per policy at cap=100%, stratum=all (H1 best case).
    # Panel B: one line per capacity at policy=lru, stratum=all (capacity sensitivity).
    fig, (axp, axc) = plt.subplots(1, 2, figsize=(12, 5), sharey=True)

    for pol in POLICIES:
        axp.plot(WINDOWS, _comp_by_window(runs, policy=pol, capacity_pct=100, stratum="all"),
                 "o-", label=pol.upper())
    axp.axhline(0.10, ls="--", color="grey", lw=1, label="H1 threshold (0.10)")
    axp.axvline(32, ls=":", color="red", lw=1, label="H2 inflection (N=32)")
    axp.set_xlabel("retention window N (blocks)")
    axp.set_ylabel("compression (= hit rate)")
    axp.set_title("By policy (capacity = 100%)")
    axp.set_xscale("log", base=2)
    axp.set_xticks(WINDOWS)
    axp.get_xaxis().set_major_formatter(plt.ScalarFormatter())
    axp.legend(fontsize=8)

    for cap in CAPS:
        axc.plot(WINDOWS, _comp_by_window(runs, policy="lru", capacity_pct=cap, stratum="all"),
                 "o-", label=f"{cap}%")
    axc.axvline(32, ls=":", color="red", lw=1)
    axc.set_xlabel("retention window N (blocks)")
    axc.set_title("By capacity (policy = LRU)")
    axc.set_xscale("log", base=2)
    axc.set_xticks(WINDOWS)
    axc.get_xaxis().set_major_formatter(plt.ScalarFormatter())
    axc.legend(title="capacity", fontsize=8)

    fig.suptitle("Compression vs retention window")
    fig.tight_layout()
    fig.savefig(out / "compression_vs_window.png", dpi=120)
    plt.close(fig)

    # Marginal gain Δcompression/ΔN per policy at cap=100% (H2 inflection numerically).
    fig, ax = plt.subplots(figsize=(8, 4))
    for pol in POLICIES:
        comp = _comp_by_window(runs, policy=pol, capacity_pct=100, stratum="all")
        dn = np.diff(WINDOWS)
        marginal = np.diff(comp) / dn
        mids = [(WINDOWS[i] + WINDOWS[i + 1]) / 2 for i in range(len(WINDOWS) - 1)]
        ax.plot(mids, marginal, "o-", label=pol.upper())
    ax.axhline(0, color="grey", lw=0.8)
    ax.set_xlabel("window N (midpoint of step)")
    ax.set_ylabel("marginal gain Δcompression / ΔN")
    ax.set_title("Diminishing returns of a larger window")
    ax.set_xscale("log", base=2)
    ax.legend(fontsize=8)
    fig.tight_layout()
    fig.savefig(out / "marginal_gain_vs_window.png", dpi=120)
    plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot compression vs retention window (H1/H2)")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    runs = load_runs(Path(args.results_dir))
    make_plots(runs, out)
    console.print(f"[green]Wrote compression_vs_window.png, marginal_gain_vs_window.png to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
