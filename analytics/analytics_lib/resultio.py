"""Read the Rust simulation outputs in ``results/`` for Stage-4 analysis.

Mirrors ``dataio.py``'s role for the scraped dataset: a single IO/decoding layer
that every plot and table script imports. Returns numpy arrays / plain dicts
(no pandas), matching the package's minimal-dependency style.

Schema (written by ``caching_strategies/src/output.rs``):
- ``runs.parquet``        one row per sweep run (225 rows).
- ``series/run_<id>.parquet``    per-block time series for that run.
- ``contracts/run_<id>.parquet`` top-50 invalidating contracts for that run.
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq

# Sweep grid (must match caching_strategies/src/config.rs + policy/strata).
POLICIES = ["lru", "lfu", "arc"]
WINDOWS = [8, 16, 32, 64, 128]
CAPS = [10, 25, 50, 75, 100]
STRATA = ["all", "defi", "transfer"]

SURVIVAL_BUCKETS = 14
# Human labels for survival_b0..b13 (see caching_strategies/src/metrics.rs:14-21):
# b0..b7 are exact ages 0..7; b8..b13 are log2-scaled ranges.
SURVIVAL_LABELS = [
    "0", "1", "2", "3", "4", "5", "6", "7",
    "8–15", "16–31", "32–63", "64–127", "128–255", "256+",
]


def load_runs(results_dir: Path) -> dict[str, np.ndarray]:
    """Load ``runs.parquet`` as a dict of column-name -> numpy array."""
    path = Path(results_dir) / "runs.parquet"
    if not path.exists():
        raise FileNotFoundError(
            f"No runs.parquet at {path}. Run the Rust sweep first "
            "(see caching_strategies/README.md)."
        )
    table = pq.read_table(path).to_pydict()
    return {k: np.asarray(v) for k, v in table.items()}


def run_id_for(policy: str, window: int, capacity_pct: int, stratum: str) -> int:
    """Map a (policy, window, capacity, stratum) config to its run_id.

    Mirrors ``build_grid`` in caching_strategies/src/sweep.rs: the grid iterates
    policy -> window -> capacity, emitting 3 consecutive ids (one per stratum).
    """
    pi = POLICIES.index(policy)
    wi = WINDOWS.index(window)
    ci = CAPS.index(capacity_pct)
    si = STRATA.index(stratum)
    return ((pi * len(WINDOWS) + wi) * len(CAPS) + ci) * len(STRATA) + si


def load_series(results_dir: Path, run_id: int) -> dict[str, np.ndarray]:
    """Load the per-block time series for one run."""
    path = Path(results_dir) / "series" / f"run_{run_id}.parquet"
    table = pq.read_table(path).to_pydict()
    return {k: np.asarray(v) for k, v in table.items()}


def load_contracts(results_dir: Path, run_id: int) -> list[tuple[str, int]]:
    """Load the top invalidating contracts for one run, sorted as written."""
    path = Path(results_dir) / "contracts" / f"run_{run_id}.parquet"
    table = pq.read_table(path).to_pydict()
    return list(zip(table["address"], (int(n) for n in table["invalidations"]), strict=True))


def load_top_contracts(dataset_dir: Path) -> dict[str, int]:
    """Map lowercased ``0x`` address -> 1-based access-frequency rank.

    Reads ``dataset_stats.json`` produced by characterize.py. Used to attach a
    readable rank label to the addresses in the invalidation plots/tables. Keys
    keep the ``0x`` prefix so they match the addresses in ``contracts/*.parquet``.
    """
    stats_path = Path(dataset_dir) / "dataset_stats.json"
    if not stats_path.exists():
        return {}
    stats = json.loads(stats_path.read_text())
    out: dict[str, int] = {}
    for rank, c in enumerate(stats.get("top_contracts", []), start=1):
        out[c["address"].lower()] = rank
    return out


def select(runs: dict[str, np.ndarray], **filters) -> np.ndarray:
    """Return a boolean row mask over ``runs`` matching every filter.

    Example: ``select(runs, policy="lru", capacity_pct=100, stratum="all")``.
    """
    mask = np.ones(len(next(iter(runs.values()))), dtype=bool)
    for col, val in filters.items():
        mask &= runs[col] == val
    return mask
