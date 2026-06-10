# analytics

Python analysis for the stateless-Ethereum witness-cache study. Two kinds of
scripts:

1. **Dataset characterization** (`characterize.py`) — run right after scraping,
   before any simulation, to understand the workload.
2. **Result analysis** (`plot_*.py`, planned) — turn the Rust simulation output
   in `../results/` into the thesis figures.

`analytics_lib/` holds shared IO helpers (`dataio.py` reads the scraped
`../data/` shards; `resultio.py` will read `../results/`).

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

## Tests

```bash
uv run pytest -q
```
