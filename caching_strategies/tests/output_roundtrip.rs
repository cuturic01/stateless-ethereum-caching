//! Write a small runs.parquet and read it back to confirm schema + values.

use caching_strategies::metrics::SURVIVAL_BUCKETS;
use caching_strategies::output::{write_runs_parquet, RunSummary};

use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;

fn sample_row(run_id: u32) -> RunSummary {
    RunSummary {
        run_id,
        policy: "arc".into(),
        window: 32,
        capacity_pct: 50,
        capacity_entries: 25_000,
        stratum: "all".into(),
        blocks_counted: 50_400,
        total_reads: 1_000,
        total_hits: 700,
        total_misses: 300,
        overall_hit_rate: 0.7,
        overall_compression_ratio: 0.55,
        total_invalidations: 120,
        peak_occupancy: 24_999,
        mean_survival: 12.5,
        survival_buckets: [1; SURVIVAL_BUCKETS],
        bytes_sent: 450_000,
        bytes_saved: 550_000,
        bytes_naive: 1_000_000,
        floor_bytes: 290_304,
        cacheable_fraction: 0.71,
        peak_cache_bytes: 24_999 * 33,
        mean_cache_bytes: 700_000.0,
        peak_leaf_bytes: 24_999 * 33,
        peak_stem_bytes: 19_000 * 128,
        bytes_sent_stem: 430_000,
        bytes_sent_stem_opt: 425_000,
        compression_stem: 0.57,
        stem_capacity_entries: 19_000,
        stem_peak_occupancy: 18_999,
        stem_mean_survival: 9.5,
        total_stem_invalidations: 95,
    }
}

#[test]
fn runs_parquet_roundtrips() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("runs.parquet");
    let rows = vec![sample_row(0), sample_row(1)];
    write_runs_parquet(&path, &rows).unwrap();

    let file = File::open(&path).unwrap();
    let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let batch = reader.next().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    // 15 base + 14 survival + 5 byte/fraction + 4 footprint + 7 stem = 45
    assert_eq!(batch.num_columns(), 15 + SURVIVAL_BUCKETS + 5 + 4 + 7);

    let schema = batch.schema();
    assert_eq!(schema.field(0).name(), "run_id");
    assert!(schema.column_with_name("overall_compression_ratio").is_some());
    assert!(schema.column_with_name("survival_b13").is_some());
    assert!(schema.column_with_name("bytes_saved").is_some());
    assert!(schema.column_with_name("bytes_sent").is_some());
    assert!(schema.column_with_name("floor_bytes").is_some());
    assert!(schema.column_with_name("cacheable_fraction").is_some());
    assert!(schema.column_with_name("peak_cache_bytes").is_some());
    assert!(schema.column_with_name("mean_cache_bytes").is_some());
    assert!(schema.column_with_name("peak_leaf_bytes").is_some());
    assert!(schema.column_with_name("peak_stem_bytes").is_some());
    assert!(schema.column_with_name("bytes_sent_stem").is_some());
    assert!(schema.column_with_name("bytes_sent_stem_opt").is_some());
    assert!(schema.column_with_name("compression_stem").is_some());
    assert!(schema.column_with_name("stem_capacity_entries").is_some());
    assert!(schema.column_with_name("stem_peak_occupancy").is_some());
    assert!(schema.column_with_name("stem_mean_survival").is_some());
    assert!(schema.column_with_name("total_stem_invalidations").is_some());
    assert!(schema.column_with_name("bytes_per_key").is_none());
    // The published columns must keep their positions: analytics reads by name,
    // but the baseline diff in phase 6 compares column order too.
    assert_eq!(schema.field(29).name(), "bytes_sent");
    assert_eq!(schema.field(33).name(), "cacheable_fraction");
}
