# analytics

Python analysis for the stateless-Ethereum witness-cache study. Two kinds of
scripts:

1. **Dataset characterization** (`characterize.py`) — run right after scraping,
   before any simulation, to understand the workload.
2. **Result analysis** (`plot_*.py`, `make_tables.py`) — turn the Rust simulation
   output in `../results/` into the thesis figures and tables.

`analytics_lib/` holds shared IO helpers (`dataio.py` reads the scraped
`../data/` shards; `resultio.py` reads the `../results/` Parquet sweep output).

## Setup

```bash
cd analytics
uv sync
```

## Characterize the dataset

```bash
uv run python characterize.py            # reads ../data, writes out/dataset/
```

Outputs to `out/dataset/`:

| file | purpose |
|---|---|
| `dataset_stats.json` | machine-readable summary; **feeds the Rust capacity sweep** (working-set sizes) |
| `strata.json` | per-block stratum assignment + defi contract set; **Rust simulation input** |
| `summary.md` | human-readable digest |
| `*.png` | reads/writes histogram, working-set growth, working-set vs window, top contracts |

It runs two streaming passes over the shards (the dataset is never fully loaded
into memory), though the global distinct-key set can reach a few GB on the full
~50k-block dataset.

## Analyze the sweep results

Run after the Rust sweep has populated `../results/` (and `characterize.py` has
written `out/dataset/`, used only for labelling contracts):

```bash
uv run python plot_compression_vs_window.py      # H1/H2
uv run python plot_compression_vs_capacity.py    # H4
uv run python plot_policy_comparison.py          # H4
uv run python plot_survival_histogram.py         # H3
uv run python plot_invalidation_by_contract.py   # H3
uv run python make_tables.py                     # all H (LaTeX + Markdown)
```

Outputs to `out/results/`:

| file | hypothesis | purpose |
|---|---|---|
| `compression_vs_window.png`, `marginal_gain_vs_window.png` | H1, H2 | compression rises with window N; marginal gain shows the inflection |
| `compression_vs_capacity.png` | H4 | compression vs capacity, faceted by window |
| `policy_comparison.png` | H4 | ARC vs LFU vs LRU bars + advantage over LRU (N=32) |
| `survival_histogram.png` | H3 | entry-survival age distribution per stratum |
| `invalidation_by_contract.png` | H3 | top invalidating contracts (Zipf-like concentration) |
| `table_*.tex` / `table_*.md` | all | thesis-ready summary tables (booktabs LaTeX + Markdown mirror) |

Compression here is `bytes_saved / bytes_naive` under the structural EIP-4762 Verkle
witness model (`caching_strategies/src/witness.rs`) — decoupled from the hit rate by
the non-cacheable per-block IPA proof floor. Every script accepts `--results-dir`,
`--out` (and `plot_invalidation_by_contract.py`/`make_tables.py` also `--dataset-dir`).

## Tests

```bash
uv run pytest -q
```
