use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::cache::{MetricSink, WitnessCache};
use crate::config::{DatasetParams, CAPACITY_PCTS, WINDOWS};
use crate::loader::load_shard;
use crate::manifest::Manifest;
use crate::metrics::RunMetrics;
use crate::model::BlockRecord;
use crate::output::{
    write_contracts_parquet, write_runs_parquet, write_series_parquet, RunSummary,
};
use crate::policy::PolicyKind;
use crate::strata::{Strata, StratumKind};

const TOP_CONTRACTS: usize = 50;

#[derive(Debug, Clone)]
pub struct SimSpec {
    pub policy: PolicyKind,
    pub window: u64,
    pub capacity_pct: u32,
    pub capacity_entries: usize,
    pub run_ids: [u32; 3],
}

pub fn build_grid(params: &DatasetParams) -> Result<Vec<SimSpec>> {
    let mut specs = Vec::new();
    let mut run_id = 0u32;
    for policy in PolicyKind::ALL {
        for &window in &WINDOWS {
            for &pct in &CAPACITY_PCTS {
                let capacity_entries = params.capacity_entries(window, pct)?;
                let run_ids = [run_id, run_id + 1, run_id + 2];
                run_id += 3;
                specs.push(SimSpec { policy, window, capacity_pct: pct, capacity_entries, run_ids });
            }
        }
    }
    Ok(specs)
}

struct SimState {
    spec: SimSpec,
    cache: WitnessCache,
    metrics: [RunMetrics; 3],
}

pub struct SweepConfig {
    pub data_dir: PathBuf,
    pub stats_path: PathBuf,
    pub strata_path: PathBuf,
    pub out_dir: PathBuf,
    pub verify_sha: bool,
    pub threads: Option<usize>,
}

pub fn run_sweep(cfg: &SweepConfig) -> Result<()> {
    if let Some(t) = cfg.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .ok();
    }

    let manifest = Manifest::load(&cfg.data_dir)?;
    manifest.assert_schema_v1()?;
    let params = DatasetParams::load(&cfg.stats_path)?;
    let strata = Strata::load(&cfg.strata_path)?;

    let specs = build_grid(&params)?;
    let total_runs = specs.len() * StratumKind::ALL.len();
    println!(
        "sweep: {} cache sims -> {} result rows ({} policies × {} windows × {} capacities × {} strata) over {} blocks on {} threads",
        specs.len(),
        total_runs,
        PolicyKind::ALL.len(),
        WINDOWS.len(),
        CAPACITY_PCTS.len(),
        StratumKind::ALL.len(),
        manifest.block_span(),
        rayon::current_num_threads(),
    );

    let mut states: Vec<SimState> = specs
        .into_iter()
        .map(|spec| {
            let cache = WitnessCache::new(spec.policy.build(spec.capacity_entries), spec.window);
            SimState { spec, cache, metrics: Default::default() }
        })
        .collect();

    let n_shards = manifest.shards.len();
    let start = Instant::now();
    let mut blocks_done: u64 = 0;
    for (si, entry) in manifest.shards.iter().enumerate() {
        let path = cfg.data_dir.join(&entry.file);
        let sha = if cfg.verify_sha { Some(entry.sha256.as_str()) } else { None };
        let shard: Vec<BlockRecord> = load_shard(&path, sha)?;

        states.par_iter_mut().for_each(|st| {
            for block in &shard {
                let now = block.block_number;
                let [m_all, m_defi, m_transfer] = &mut st.metrics;
                let mut sinks = [
                    MetricSink { metrics: m_all, active: true },
                    MetricSink { metrics: m_defi, active: strata.in_stratum(StratumKind::Defi, now) },
                    MetricSink {
                        metrics: m_transfer,
                        active: strata.in_stratum(StratumKind::Transfer, now),
                    },
                ];
                st.cache.process_block(block, &mut sinks);
            }
        });

        blocks_done += shard.len() as u64;
        let done = si + 1;
        if done % 10 == 0 || done == n_shards {
            let elapsed = start.elapsed().as_secs_f64();
            let frac = done as f64 / n_shards as f64;
            let eta = if frac > 0.0 { elapsed / frac - elapsed } else { 0.0 };
            print!(
                "\r  shard {done}/{n_shards} ({blocks_done} blocks)  elapsed {elapsed:.0}s  eta {eta:.0}s   "
            );
            std::io::stdout().flush().ok();
        }
    }
    println!("\nsimulation complete in {:.1}s", start.elapsed().as_secs_f64());

    let series_dir = cfg.out_dir.join("series");
    let contracts_dir = cfg.out_dir.join("contracts");

    let mut summaries: Vec<RunSummary> = states
        .par_iter()
        .map(|st| -> Result<Vec<RunSummary>> {
            let mut rows = Vec::with_capacity(3);
            for (idx, stratum) in StratumKind::ALL.iter().enumerate() {
                let id = st.spec.run_ids[idx];
                let m = &st.metrics[idx];
                write_series_parquet(&series_dir.join(format!("run_{id}.parquet")), &m.series)
                    .with_context(|| format!("writing series for run {id}"))?;
                let top = m.contracts.top_k(TOP_CONTRACTS);
                write_contracts_parquet(&contracts_dir.join(format!("run_{id}.parquet")), &top)
                    .with_context(|| format!("writing contracts for run {id}"))?;
                rows.push(RunSummary::from_metrics(
                    id,
                    st.spec.policy.name(),
                    st.spec.window as u32,
                    st.spec.capacity_pct,
                    st.spec.capacity_entries as u64,
                    stratum.name(),
                    m,
                ));
            }
            Ok(rows)
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    summaries.sort_by_key(|r| r.run_id);
    let runs_path = cfg.out_dir.join("runs.parquet");
    write_runs_parquet(&runs_path, &summaries)?;
    println!("wrote {} run summaries to {}", summaries.len(), runs_path.display());
    Ok(())
}
