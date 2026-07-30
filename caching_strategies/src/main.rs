use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use caching_strategies::loader::ShardStream;
use caching_strategies::manifest::Manifest;
use caching_strategies::sweep::{run_sweep, SweepConfig};

#[derive(Parser)]
#[command(name = "cstrat", about = "Cross-block witness delta compression simulation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    VerifyLoad {
        #[arg(long, default_value = "../data")]
        data: PathBuf,
        #[arg(long, default_value_t = false)]
        verify_sha: bool,
    },
    Run {
        #[arg(long, default_value = "../data")]
        data: PathBuf,
        #[arg(long, default_value = "../analytics/out/dataset/dataset_stats.json")]
        stats: PathBuf,
        #[arg(long, default_value = "../analytics/out/dataset/strata.json")]
        strata: PathBuf,
        #[arg(long, default_value = "../results")]
        out: PathBuf,
        #[arg(long, default_value_t = false)]
        verify_sha: bool,
        #[arg(long)]
        threads: Option<usize>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::VerifyLoad { data, verify_sha } => verify_load(&data, verify_sha),
        Command::Run { data, stats, strata, out, verify_sha, threads } => {
            run_sweep(&SweepConfig {
                data_dir: data,
                stats_path: stats,
                strata_path: strata,
                out_dir: out,
                verify_sha,
                threads,
            })
        }
    }
}

fn verify_load(data: &std::path::Path, verify_sha: bool) -> Result<()> {
    let manifest = Manifest::load(data)?;
    manifest.assert_schema_v1()?;
    println!(
        "manifest: {} blocks {}..{} across {} shards (schema v{})",
        manifest.block_span(),
        manifest.start_block,
        manifest.end_block,
        manifest.shards.len(),
        manifest.schema_version
    );

    let mut count: u64 = 0;
    let mut prev: Option<u64> = None;
    let mut gaps: u64 = 0;
    let mut total_reads: u64 = 0;
    let mut total_writes: u64 = 0;

    for item in ShardStream::new(&manifest, data, verify_sha) {
        let block = item?;
        if let Some(p) = prev {
            if block.block_number != p + 1 {
                gaps += 1;
            }
        }
        prev = Some(block.block_number);
        count += 1;
        total_reads += block.reads.len() as u64;
        total_writes += block.writes.len() as u64;
    }

    println!(
        "streamed {count} blocks, {gaps} gaps, first={:?} last={:?}",
        manifest.start_block, prev
    );
    println!(
        "total reads={total_reads} ({:.1}/block), writes={total_writes} ({:.1}/block)",
        total_reads as f64 / count.max(1) as f64,
        total_writes as f64 / count.max(1) as f64,
    );

    if count != manifest.block_span() {
        bail!("block count {count} != manifest span {}", manifest.block_span());
    }
    if gaps != 0 {
        bail!("{gaps} gaps in block sequence");
    }
    println!("OK");
    Ok(())
}
