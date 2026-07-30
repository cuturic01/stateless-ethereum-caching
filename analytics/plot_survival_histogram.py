from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from rich.console import Console  # noqa: E402

from analytics_lib.resultio import (  # noqa: E402
    STRATA,
    SURVIVAL_BUCKETS,
    SURVIVAL_LABELS,
    load_runs,
    select,
)

console = Console()


def _buckets(runs, stratum: str, policy: str, window: int, cap: int) -> np.ndarray | None:
    mask = select(runs, stratum=stratum, policy=policy, window=window, capacity_pct=cap)
    if not mask.any():
        return None
    idx = np.flatnonzero(mask)[0]
    return np.asarray([int(runs[f"survival_b{i}"][idx]) for i in range(SURVIVAL_BUCKETS)])


def make_plots(runs, out: Path, policy="lru", window=32, cap=100) -> None:
    fig, axes = plt.subplots(1, len(STRATA), figsize=(5 * len(STRATA), 4.5), sharey=True)
    x = np.arange(SURVIVAL_BUCKETS)
    for ax, stratum in zip(axes, STRATA, strict=True):
        counts = _buckets(runs, stratum, policy, window, cap)
        if counts is None:
            ax.set_visible(False)
            continue
        total = counts.sum()
        frac = counts / total if total else counts
        mean_mask = select(runs, stratum=stratum, policy=policy, window=window, capacity_pct=cap)
        mean = float(runs["mean_survival"][mean_mask][0])
        ax.bar(x, frac, color="steelblue")
        ax.set_xticks(x)
        ax.set_xticklabels(SURVIVAL_LABELS, rotation=45, ha="right", fontsize=7)
        ax.set_xlabel("survival age (blocks)")
        ax.set_title(f"{stratum}  (mean {mean:.1f})")
    axes[0].set_ylabel("fraction of evicted entries")
    fig.suptitle(f"Cache-entry survival (policy={policy.upper()}, N={window}, cap={cap}%)")
    fig.tight_layout()
    fig.savefig(out / "survival_histogram.png", dpi=120)
    plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot survival histogram per stratum (H3)")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    runs = load_runs(Path(args.results_dir))
    make_plots(runs, out)
    console.print(f"[green]Wrote survival_histogram.png to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
