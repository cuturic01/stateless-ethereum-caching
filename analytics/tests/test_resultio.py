from analytics_lib import resultio


def test_run_id_for_matches_grid():
    # Grid order policy -> window -> capacity, 3 strata per spec (sweep.rs).
    assert resultio.run_id_for("lru", 8, 10, "all") == 0
    assert resultio.run_id_for("lru", 8, 10, "transfer") == 2
    assert resultio.run_id_for("lru", 8, 25, "all") == 3
    assert resultio.run_id_for("lru", 32, 100, "all") == 42
    # Last run: arc / N=128 / cap=100 / transfer.
    assert resultio.run_id_for("arc", 128, 100, "transfer") == 224


def test_load_runs_roundtrip(synthetic_results):
    results_dir, _ = synthetic_results
    runs = resultio.load_runs(results_dir)
    assert len(runs["run_id"]) == 225
    assert set(runs["policy"]) == set(resultio.POLICIES)
    assert all(f"survival_b{i}" in runs for i in range(resultio.SURVIVAL_BUCKETS))


def test_select_mask(synthetic_results):
    results_dir, _ = synthetic_results
    runs = resultio.load_runs(results_dir)
    mask = resultio.select(runs, policy="arc", window=32, capacity_pct=10, stratum="all")
    assert mask.sum() == 1
    # ARC should beat LRU below full capacity.
    arc = runs["overall_compression_ratio"][mask][0]
    lru_mask = resultio.select(runs, policy="lru", window=32, capacity_pct=10, stratum="all")
    assert arc > runs["overall_compression_ratio"][lru_mask][0]


def test_load_series_and_contracts(synthetic_results):
    results_dir, dataset_dir = synthetic_results
    rid = resultio.run_id_for("lru", 32, 100, "all")
    series = resultio.load_series(results_dir, rid)
    assert series["block_number"].tolist() == [1000, 1001, 1002]
    contracts = resultio.load_contracts(results_dir, rid)
    assert contracts[0][1] == 500
    ranks = resultio.load_top_contracts(dataset_dir)
    assert ranks[contracts[0][0].lower()] == 1
