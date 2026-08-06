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

# Names for the hot-set contracts the thesis refers to by name (report.md:109,
# outline.md:199). Only these are named; anything else is shown as a bare
# address. Keys are lowercase and 0x-prefixed, matching contracts/*.parquet.
KNOWN_CONTRACTS = {
    "0xdac17f958d2ee523a2206206994597c13d831ec7": "USDT",
    "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48": "USDC",
    "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2": "WETH",
    "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599": "WBTC",
    "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2": "Aave v3",
    "0x000000000004444c5dc75cb358380d2e3de08a90": "Uniswap v4",
}


def short_addr(addr: str) -> str:
    """Elide an address to ``0xxxxxxxxx…yyyyyy``, keeping head *and* tail.

    Head-only truncation is not injective over this dataset: five of the top-30
    contracts begin with eight zero nibbles and would all render as
    ``0x00000000``. Keeping the last six nibbles separates them.
    """
    a = addr.lower()
    return f"{a[:10]}…{a[-6:]}" if len(a) > 18 else a


def contract_label(addr: str, rank: int | None = None) -> str:
    """Axis label for a contract: name if known, elided address, optional rank."""
    parts = [KNOWN_CONTRACTS.get(addr.lower(), ""), short_addr(addr)]
    if rank:
        parts.append(f"(#{rank})")
    return " ".join(p for p in parts if p)


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
