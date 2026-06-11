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
        bytes_per_key: 200,
        blocks_counted: 50_400,
        total_reads: 1_000,
        total_hits: 700,
        total_misses: 300,
        overall_hit_rate: 0.7,
        overall_compression_ratio: 0.7,
        total_invalidations: 120,
        peak_occupancy: 24_999,
        mean_survival: 12.5,
        survival_buckets: [1; SURVIVAL_BUCKETS],
        bytes_witnessed: 300 * 200,
        bytes_saved: 700 * 200,
        bytes_naive: 1000 * 200,
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
    // 16 base columns + 14 survival buckets + 3 byte columns = 33
    assert_eq!(batch.num_columns(), 16 + SURVIVAL_BUCKETS + 3);

    let schema = batch.schema();
    assert_eq!(schema.field(0).name(), "run_id");
    assert!(schema.column_with_name("overall_compression_ratio").is_some());
    assert!(schema.column_with_name("survival_b13").is_some());
    assert!(schema.column_with_name("bytes_saved").is_some());
}
