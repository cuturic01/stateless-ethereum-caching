use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{ArrayRef, Float64Array, StringArray, UInt32Array, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::metrics::{BlockSample, RunMetrics, SURVIVAL_BUCKETS};
use crate::model::Address;

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run_id: u32,
    pub policy: String,
    pub window: u32,
    pub capacity_pct: u32,
    pub capacity_entries: u64,
    pub stratum: String,
    pub bytes_per_key: u32,
    pub blocks_counted: u64,
    pub total_reads: u64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub overall_hit_rate: f64,
    pub overall_compression_ratio: f64,
    pub total_invalidations: u64,
    pub peak_occupancy: u64,
    pub mean_survival: f64,
    pub survival_buckets: [u64; SURVIVAL_BUCKETS],
    pub bytes_witnessed: u64,
    pub bytes_saved: u64,
    pub bytes_naive: u64,
}

impl RunSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn from_metrics(
        run_id: u32,
        policy: &str,
        window: u32,
        capacity_pct: u32,
        capacity_entries: u64,
        stratum: &str,
        bytes_per_key: u32,
        m: &RunMetrics,
    ) -> Self {
        let bpk = bytes_per_key as u64;
        RunSummary {
            run_id,
            policy: policy.to_string(),
            window,
            capacity_pct,
            capacity_entries,
            stratum: stratum.to_string(),
            bytes_per_key,
            blocks_counted: m.blocks_counted,
            total_reads: m.total_reads,
            total_hits: m.total_hits,
            total_misses: m.total_misses,
            overall_hit_rate: m.overall_hit_rate(),
            overall_compression_ratio: m.overall_hit_rate(),
            total_invalidations: m.total_invalidations,
            peak_occupancy: m.peak_occupancy,
            mean_survival: m.survival.mean(),
            survival_buckets: m.survival.buckets,
            bytes_witnessed: m.total_misses * bpk,
            bytes_saved: m.total_hits * bpk,
            bytes_naive: m.total_reads * bpk,
        }
    }
}

fn props() -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build()
}

fn write_batch(path: &Path, batch: &RecordBatch) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props()))?;
    writer.write(batch).with_context(|| format!("writing {}", path.display()))?;
    writer.close()?;
    Ok(())
}

pub fn write_runs_parquet(path: &Path, rows: &[RunSummary]) -> Result<()> {
    let mut fields = vec![
        Field::new("run_id", DataType::UInt32, false),
        Field::new("policy", DataType::Utf8, false),
        Field::new("window", DataType::UInt32, false),
        Field::new("capacity_pct", DataType::UInt32, false),
        Field::new("capacity_entries", DataType::UInt64, false),
        Field::new("stratum", DataType::Utf8, false),
        Field::new("bytes_per_key", DataType::UInt32, false),
        Field::new("blocks_counted", DataType::UInt64, false),
        Field::new("total_reads", DataType::UInt64, false),
        Field::new("total_hits", DataType::UInt64, false),
        Field::new("total_misses", DataType::UInt64, false),
        Field::new("overall_hit_rate", DataType::Float64, false),
        Field::new("overall_compression_ratio", DataType::Float64, false),
        Field::new("total_invalidations", DataType::UInt64, false),
        Field::new("peak_occupancy", DataType::UInt64, false),
        Field::new("mean_survival", DataType::Float64, false),
    ];
    for i in 0..SURVIVAL_BUCKETS {
        fields.push(Field::new(format!("survival_b{i}"), DataType::UInt64, false));
    }
    fields.push(Field::new("bytes_witnessed", DataType::UInt64, false));
    fields.push(Field::new("bytes_saved", DataType::UInt64, false));
    fields.push(Field::new("bytes_naive", DataType::UInt64, false));
    let schema = Arc::new(Schema::new(fields));

    let mut cols: Vec<ArrayRef> = vec![
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|r| r.run_id))),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|r| r.policy.clone()))),
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|r| r.window))),
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|r| r.capacity_pct))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.capacity_entries))),
        Arc::new(StringArray::from_iter_values(rows.iter().map(|r| r.stratum.clone()))),
        Arc::new(UInt32Array::from_iter_values(rows.iter().map(|r| r.bytes_per_key))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.blocks_counted))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.total_reads))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.total_hits))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.total_misses))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|r| r.overall_hit_rate))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|r| r.overall_compression_ratio))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.total_invalidations))),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.peak_occupancy))),
        Arc::new(Float64Array::from_iter_values(rows.iter().map(|r| r.mean_survival))),
    ];
    for i in 0..SURVIVAL_BUCKETS {
        cols.push(Arc::new(UInt64Array::from_iter_values(
            rows.iter().map(|r| r.survival_buckets[i]),
        )));
    }
    cols.push(Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.bytes_witnessed))));
    cols.push(Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.bytes_saved))));
    cols.push(Arc::new(UInt64Array::from_iter_values(rows.iter().map(|r| r.bytes_naive))));

    let batch = RecordBatch::try_new(schema, cols)?;
    write_batch(path, &batch)
}

pub fn write_series_parquet(path: &Path, samples: &[BlockSample], bytes_per_key: u32) -> Result<()> {
    let bpk = bytes_per_key as u64;
    let schema = Arc::new(Schema::new(vec![
        Field::new("block_number", DataType::UInt64, false),
        Field::new("reads", DataType::UInt32, false),
        Field::new("hits", DataType::UInt32, false),
        Field::new("misses", DataType::UInt32, false),
        Field::new("block_hit_rate", DataType::Float64, false),
        Field::new("bytes_witnessed", DataType::UInt64, false),
        Field::new("bytes_saved", DataType::UInt64, false),
        Field::new("invalidations", DataType::UInt32, false),
        Field::new("occupancy", DataType::UInt64, false),
    ]));
    let cols: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from_iter_values(samples.iter().map(|s| s.block_number))),
        Arc::new(UInt32Array::from_iter_values(samples.iter().map(|s| s.reads))),
        Arc::new(UInt32Array::from_iter_values(samples.iter().map(|s| s.hits))),
        Arc::new(UInt32Array::from_iter_values(samples.iter().map(|s| s.misses))),
        Arc::new(Float64Array::from_iter_values(samples.iter().map(|s| {
            if s.reads == 0 { 0.0 } else { s.hits as f64 / s.reads as f64 }
        }))),
        Arc::new(UInt64Array::from_iter_values(samples.iter().map(|s| s.misses as u64 * bpk))),
        Arc::new(UInt64Array::from_iter_values(samples.iter().map(|s| s.hits as u64 * bpk))),
        Arc::new(UInt32Array::from_iter_values(samples.iter().map(|s| s.invalidations))),
        Arc::new(UInt64Array::from_iter_values(samples.iter().map(|s| s.occupancy))),
    ];
    let batch = RecordBatch::try_new(schema, cols)?;
    write_batch(path, &batch)
}

pub fn write_contracts_parquet(path: &Path, top: &[(Address, u64)]) -> Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("address", DataType::Utf8, false),
        Field::new("invalidations", DataType::UInt64, false),
    ]));
    let cols: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(top.iter().map(|(a, _)| a.to_hex()))),
        Arc::new(UInt64Array::from_iter_values(top.iter().map(|(_, c)| *c))),
    ];
    let batch = RecordBatch::try_new(schema, cols)?;
    write_batch(path, &batch)
}
