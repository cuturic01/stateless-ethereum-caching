use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct ShardEntry {
    pub file: String,
    pub first_block: u64,
    pub last_block: u64,
    pub count: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub network: String,
    pub start_block: u64,
    pub end_block: u64,
    pub shard_size: u32,
    pub schema_version: u32,
    pub key_encoding: String,
    pub shards: Vec<ShardEntry>,
}

impl Manifest {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("manifest.json");
        let bytes = std::fs::read(&path)
            .with_context(|| format!("reading manifest {}", path.display()))?;
        let m: Manifest = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing manifest {}", path.display()))?;
        Ok(m)
    }

    pub fn assert_schema_v1(&self) -> Result<()> {
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            bail!(
                "manifest schema_version {} unsupported (expected {})",
                self.schema_version,
                SUPPORTED_SCHEMA_VERSION
            );
        }
        Ok(())
    }

    pub fn block_span(&self) -> u64 {
        self.end_block - self.start_block + 1
    }
}
