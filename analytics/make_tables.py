"""Generate the Stage-4 summary tables (LaTeX + Markdown) from results/.

Emits, into out/results/, both a ``table_<name>.tex`` (booktabs) and a
``table_<name>.md`` for each:
  T1 compression_vs_window  (H2) — compression + marginal Δ, per (window, policy).
  T2 policy_comparison      (H4) — per-capacity ARC/LFU/LRU + ARC advantage (N=32).
  T3 survival_summary       (H3) — mean survival + churn / long-lived fractions per stratum.
  T4 top_invalidators       (H3) — top-15 invalidating contracts, with freq rank.
  T5 compression_by_stratum (H1) — compression per stratum at a canonical config.
"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
from rich.console import Console

from analytics_lib.resultio import (
    CAPS,
    POLICIES,
    STRATA,
    SURVIVAL_BUCKETS,
    WINDOWS,
    load_contracts,
    load_runs,
    load_top_contracts,
    run_id_for,
    select,
)

console = Console()

CANON_POLICY, CANON_WINDOW, CANON_CAP = "lru", 32, 100


def _esc_tex(s: str) -> str:
    return (
        s.replace("\\", r"\textbackslash{}")
        .replace("_", r"\_")
        .replace("%", r"\%")
        .replace("&", r"\&")
    )


def _md_table(headers: list[str], rows: list[list[str]]) -> str:
    out = ["| " + " | ".join(headers) + " |",
           "|" + "|".join(["---"] * len(headers)) + "|"]
    for r in rows:
        out.append("| " + " | ".join(r) + " |")
    return "\n".join(out) + "\n"


def _latex_table(headers: list[str], rows: list[list[str]], caption: str, label: str) -> str:
    cols = "l" + "r" * (len(headers) - 1)
    lines = [
        r"\begin{table}[ht]", r"\centering",
        rf"\caption{{{_esc_tex(caption)}}}", rf"\label{{tab:{label}}}",
        rf"\begin{{tabular}}{{{cols}}}", r"\toprule",
        " & ".join(_esc_tex(h) for h in headers) + r" \\", r"\midrule",
    ]
    for r in rows:
        lines.append(" & ".join(_esc_tex(c) for c in r) + r" \\")
    lines += [r"\bottomrule", r"\end{tabular}", r"\end{table}", ""]
    return "\n".join(lines)


def _write(out: Path, name: str, headers, rows, caption, label) -> None:
    (out / f"table_{name}.md").write_text(f"### {caption}\n\n" + _md_table(headers, rows))
    (out / f"table_{name}.tex").write_text(_latex_table(headers, rows, caption, label))


def _comp(runs, **f) -> float:
    m = select(runs, **f)
    return float(runs["overall_compression_ratio"][m][0]) if m.any() else float("nan")


def t1_compression_vs_window(runs, out: Path) -> None:
    headers = ["policy", "window N", "compression", "Δ vs prev N"]
    rows = []
    for pol in POLICIES:
        prev = None
        for w in WINDOWS:
            c = _comp(runs, policy=pol, window=w, capacity_pct=100, stratum="all")
            d = "—" if prev is None else f"{c - prev:+.3f}"
            rows.append([pol.upper(), str(w), f"{c:.3f}", d])
            prev = c
    _write(out, "compression_vs_window", headers, rows,
           "Compression vs retention window (capacity 100%, stratum all)",
           "compression_vs_window")


def t2_policy_comparison(runs, out: Path, window=CANON_WINDOW) -> None:
    headers = ["capacity", "LRU", "LFU", "ARC", "ARC−LRU", "LFU−LRU"]
    rows = []
    for cap in CAPS:
        lru = _comp(runs, policy="lru", window=window, capacity_pct=cap, stratum="all")
        lfu = _comp(runs, policy="lfu", window=window, capacity_pct=cap, stratum="all")
        arc = _comp(runs, policy="arc", window=window, capacity_pct=cap, stratum="all")
        rows.append([f"{cap}%", f"{lru:.3f}", f"{lfu:.3f}", f"{arc:.3f}",
                     f"{arc - lru:+.3f}", f"{lfu - lru:+.3f}"])
    _write(out, "policy_comparison", headers, rows,
           f"Policy comparison at N={window}, stratum all (compression = hit rate)",
           "policy_comparison")


def t3_survival_summary(
    runs, out: Path, policy=CANON_POLICY, window=CANON_WINDOW, cap=CANON_CAP
) -> None:
    headers = ["stratum", "mean survival", "churn (age 0–1)", "long-lived (age ≥ 8)"]
    rows = []
    for stratum in STRATA:
        m = select(runs, stratum=stratum, policy=policy, window=window, capacity_pct=cap)
        if not m.any():
            continue
        idx = np.flatnonzero(m)[0]
        buckets = np.asarray([int(runs[f"survival_b{i}"][idx]) for i in range(SURVIVAL_BUCKETS)])
        total = buckets.sum() or 1
        churn = buckets[0:2].sum() / total
        longlived = buckets[8:].sum() / total
        mean = float(runs["mean_survival"][idx])
        rows.append([stratum, f"{mean:.1f}", f"{churn:.1%}".rstrip(), f"{longlived:.1%}"])
    _write(out, "survival_summary", headers, rows,
           f"Survival summary (policy={policy.upper()}, N={window}, cap={cap}%)",
           "survival_summary")


def t4_top_invalidators(results_dir: Path, dataset_dir: Path, out: Path,
                        policy=CANON_POLICY, window=CANON_WINDOW, cap=CANON_CAP) -> None:
    run_id = run_id_for(policy, window, cap, "all")
    contracts = load_contracts(results_dir, run_id)
    if not contracts:
        return
    ranks = load_top_contracts(dataset_dir)
    total = sum(n for _, n in contracts) or 1
    headers = ["#", "address", "freq rank", "invalidations", "share of top-50"]
    rows = []
    for i, (addr, n) in enumerate(contracts[:15], start=1):
        r = ranks.get(addr.lower())
        rows.append([str(i), addr, str(r) if r else "—", f"{n:,}", f"{n / total:.1%}"])
    _write(out, "top_invalidators", headers, rows,
           f"Top invalidating contracts (policy={policy.upper()}, N={window}, cap={cap}%, all)",
           "top_invalidators")


def t5_compression_by_stratum(
    runs, out: Path, policy=CANON_POLICY, window=CANON_WINDOW, cap=CANON_CAP
) -> None:
    headers = ["stratum", "compression", "mean survival", "invalidations"]
    rows = []
    for stratum in STRATA:
        m = select(runs, stratum=stratum, policy=policy, window=window, capacity_pct=cap)
        if not m.any():
            continue
        idx = np.flatnonzero(m)[0]
        rows.append([
            stratum,
            f"{float(runs['overall_compression_ratio'][idx]):.3f}",
            f"{float(runs['mean_survival'][idx]):.1f}",
            f"{int(runs['total_invalidations'][idx]):,}",
        ])
    _write(out, "compression_by_stratum", headers, rows,
           f"Compression by stratum (policy={policy.upper()}, N={window}, cap={cap}%)",
           "compression_by_stratum")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate Stage-4 summary tables (LaTeX + Markdown)"
    )
    repo_root = Path(__file__).resolve().parents[1]
    parser.add_argument("--results-dir", default=str(repo_root / "results"))
    parser.add_argument(
        "--dataset-dir", default=str(Path(__file__).resolve().parent / "out" / "dataset")
    )
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "out" / "results"))
    args = parser.parse_args(argv)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    results_dir = Path(args.results_dir)
    runs = load_runs(results_dir)

    t1_compression_vs_window(runs, out)
    t2_policy_comparison(runs, out)
    t3_survival_summary(runs, out)
    t4_top_invalidators(results_dir, Path(args.dataset_dir), out)
    t5_compression_by_stratum(runs, out)

    console.print(f"[green]Wrote table_*.tex and table_*.md (5 tables) to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
