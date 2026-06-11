"""H3 — write-invalidation concentration across contracts.

H3 predicts invalidation concentrates in a few high-churn contracts (DEX pools,
lending protocols). Horizontal bar of the top-20 invalidating contracts for a
canonical config (policy=lru, window=32, capacity=100%, stratum=all), labelled
with their access-frequency rank from dataset_stats.json where known.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from rich.console import Console  # noqa: E402

from analytics_lib.resultio import load_contracts, load_top_contracts, run_id_for  # noqa: E402

console = Console()

TOP_N = 20


def make_plots(results_dir: Path, dataset_dir: Path, out: Path,
               policy="lru", window=32, cap=100, stratum="all") -> None:
    run_id = run_id_for(policy, window, cap, stratum)
    contracts = load_contracts(results_dir, run_id)[:TOP_N]
    if not contracts:
        console.print("[yellow]No invalidation data for the canonical run; skipping plot.")
        return
    ranks = load_top_contracts(dataset_dir)

    labels = []
    for addr, _ in contracts:
        short = addr[:10]  # 0x + first 8 hex chars
        rank = ranks.get(addr.lower())
        labels.append(f"{short} (#{rank})" if rank else short)
    vals = [n for _, n in contracts]

    fig, ax = plt.subplots(figsize=(8, 6))
    ax.barh(range(len(vals)), vals, color="indianred")
    ax.set_yticks(range(len(labels)))
    ax.set_yticklabels(labels, fontsize=7)
    ax.invert_yaxis()
    ax.set_xlabel("write-invalidations")
    ax.set_title(
        f"Top {TOP_N} invalidating contracts "
        f"(policy={policy.upper()}, N={window}, cap={cap}%, {stratum})\n"
        "#rank = access-frequency rank from dataset_stats.json"
    )
    fig.tight_layout()
    fig.savefig(out / "invalidation_by_contract.png", dpi=120)
    plt.close(fig)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Plot invalidation by contract (H3)")
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument(
        "--dataset-dir",
        default=str(Path(__file__).resolve().parent / "out" / "dataset"),
    )
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    make_plots(Path(args.results_dir), Path(args.dataset_dir), out)
    console.print(f"[green]Wrote invalidation_by_contract.png to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
