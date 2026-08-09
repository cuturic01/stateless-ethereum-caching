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

POLICY_MARKER = "o"

POLICY_PANEL_CAP = 50


def _zoom(ax, *series, pad: float = 0.06) -> None:
    vals = np.concatenate([np.asarray(s, dtype=float) for s in series])
    vals = vals[np.isfinite(vals)]
    lo, hi = float(vals.min()), float(vals.max())
    margin = (hi - lo) * pad
    ax.set_ylim(lo - margin, hi + margin)


def _comp_by_window(runs, **filters) -> np.ndarray:
    """compression at each window in WINDOWS for the given filters (NaN if absent)."""
    out = []
    for w in WINDOWS:
        mask = select(runs, window=w, **filters)
        out.append(float(runs["overall_compression_ratio"][mask][0]) if mask.any() else np.nan)
    return np.asarray(out)


def make_plots(runs, out: Path) -> None:
    fig, (axp, axc) = plt.subplots(1, 2, figsize=(11, 4.5))

    by_policy = []
    for pol in POLICIES:
        comp = _comp_by_window(runs, policy=pol, capacity_pct=POLICY_PANEL_CAP, stratum="all")
        by_policy.append(comp)
        axp.plot(WINDOWS, comp, marker=POLICY_MARKER, ls="-", label=pol.upper())
    _zoom(axp, *by_policy)
    axp.set_xlabel("retention window N (blocks)")
    axp.set_ylabel("compression (bytes saved / naive)")
    axp.set_title(f"By policy (capacity = {POLICY_PANEL_CAP}%)")
    axp.set_xscale("log", base=2)
    axp.set_xticks(WINDOWS)
    axp.get_xaxis().set_major_formatter(plt.ScalarFormatter())
    axp.legend(fontsize=8)

    for cap in CAPS:
        axc.plot(WINDOWS, _comp_by_window(runs, policy="lru", capacity_pct=cap, stratum="all"),
                 "o-", label=f"{cap}%")
    axc.set_xlabel("retention window N (blocks)")
    axc.set_ylabel("compression (bytes saved / naive)")
    axc.set_title("By capacity (policy = LRU)")
    axc.set_xscale("log", base=2)
    axc.set_xticks(WINDOWS)
    axc.get_xaxis().set_major_formatter(plt.ScalarFormatter())
    axc.legend(title="capacity", fontsize=8)

    fig.suptitle("Compression vs retention window")
    fig.tight_layout()
    fig.savefig(out / "compression_vs_window.png", dpi=120)
    plt.close(fig)

    steps = [f"{WINDOWS[i]}→{WINDOWS[i + 1]}" for i in range(len(WINDOWS) - 1)]
    x = np.arange(len(steps))
    fig, ax = plt.subplots(figsize=(8, 4))
    gains = []
    for pol in POLICIES:
        comp = _comp_by_window(runs, policy=pol, capacity_pct=POLICY_PANEL_CAP, stratum="all")
        gain = np.diff(comp)
        gains.append(gain)
        ax.plot(x, gain, marker=POLICY_MARKER, ls="-", label=pol.upper())
    _zoom(ax, *gains)
    ax.set_xticks(x)
    ax.set_xticklabels(steps)
    ax.set_xlabel("retention window step (blocks)")
    ax.set_ylabel("Δ compression per doubling of N")
    ax.set_title(f"Marginal gain of a larger window (capacity = {POLICY_PANEL_CAP}%)")
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
