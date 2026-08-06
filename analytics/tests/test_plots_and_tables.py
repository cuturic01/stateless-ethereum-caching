import make_tables
import plot_compression_vs_capacity
import plot_compression_vs_window
import plot_invalidation_by_contract
import plot_policy_comparison
import plot_survival_histogram


def _run(mod, results_dir, out, dataset_dir=None):
    argv = ["--results-dir", str(results_dir), "--out", str(out)]
    if dataset_dir is not None:
        argv += ["--dataset-dir", str(dataset_dir)]
    assert mod.main(argv) == 0


def test_plots_generate_pngs(synthetic_results, tmp_path):
    results_dir, dataset_dir = synthetic_results
    out = tmp_path / "results_out"

    _run(plot_compression_vs_window, results_dir, out)
    _run(plot_compression_vs_capacity, results_dir, out)
    _run(plot_policy_comparison, results_dir, out)
    _run(plot_survival_histogram, results_dir, out)
    _run(plot_invalidation_by_contract, results_dir, out, dataset_dir)

    for png in (
        "compression_vs_window.png", "marginal_gain_vs_window.png",
        "compression_vs_capacity.png", "policy_comparison.png",
        "survival_histogram.png", "invalidation_by_contract.png",
    ):
        assert (out / png).exists(), png
        assert (out / png).stat().st_size > 0


def test_tables_generate_tex_and_md(synthetic_results, tmp_path):
    results_dir, dataset_dir = synthetic_results
    out = tmp_path / "results_out"
    assert make_tables.main(
        ["--results-dir", str(results_dir), "--dataset-dir", str(dataset_dir), "--out", str(out)]
    ) == 0

    names = [
        "compression_vs_window", "policy_comparison", "survival_summary",
        "top_invalidators", "compression_by_stratum", "cache_footprint",
        "stem_summary",
    ]
    for name in names:
        md = out / f"table_{name}.md"
        tex = out / f"table_{name}.tex"
        assert md.exists() and tex.exists(), name
        assert "|" in md.read_text()
        tex_text = tex.read_text()
        assert r"\begin{tabular}" in tex_text and r"\bottomrule" in tex_text

    # The freq rank from dataset_stats.json should be attached to a known address.
    top_inv = (out / "table_top_invalidators.md").read_text()
    assert "0xdac17f958d2ee523a2206206994597c13d831ec7" in top_inv
    assert "USDT" in top_inv, "known contracts should be named alongside the address"

    # Footprint is reported in MB, not raw bytes: a thesis table of 9-digit
    # byte counts is unreadable.
    footprint = (out / "table_cache_footprint.md").read_text()
    assert "peak (MB)" in footprint
