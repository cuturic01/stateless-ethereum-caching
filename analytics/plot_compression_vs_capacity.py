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


def _comp_by_capacity(runs, policy: str, window: int) -> np.ndarray:
    out = []
    for cap in CAPS:
        mask = select(runs, policy=policy, window=window, capacity_pct=cap, stratum="all")
        out.append(float(runs["overall_compression_ratio"][mask][0]) if mask.any() else np.nan)
    return np.asarray(out)


def make_plots(runs, out: Path) -> None:
    # 2x3 rather than 1x5: at the thesis text width a 20-inch-wide strip scales
    # the tick labels down to roughly 3 pt. Six cells for five windows, so the
    # unused one is turned off.
    ncols = 3
    nrows = -(-len(WINDOWS) // ncols)
    fig, axes = plt.subplots(nrows, ncols, figsize=(4 * ncols, 3.6 * nrows), sharey=True)
    flat = axes.ravel()
    for ax, w in zip(flat, WINDOWS, strict=False):
        for pol in POLICIES:
            ax.plot(CAPS, _comp_by_capacity(runs, pol, w), "o-", label=pol.upper())
        ax.set_xlabel("capacity (% of working set)")
        ax.set_title(f"N = {w}")
        ax.set_xticks(CAPS)
    for ax in flat[len(WINDOWS):]:
        ax.set_visible(False)
    for r in range(nrows):
        flat[r * ncols].set_ylabel("compression (bytes saved / naive)")
    flat[0].legend(fontsize=8)
    fig.suptitle("Compression vs capacity, by retention window")
    fig.tight_layout()
    fig.savefig(out / "compression_vs_capacity.png", dpi=120)
    plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot compression vs capacity (H4)")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    runs = load_runs(Path(args.results_dir))
    make_plots(runs, out)
    console.print(f"[green]Wrote compression_vs_capacity.png to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
