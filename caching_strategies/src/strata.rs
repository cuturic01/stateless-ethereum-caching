use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::model::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StratumKind {
    All,
    Defi,
    Transfer,
}

impl StratumKind {
    pub fn name(self) -> &'static str {
        match self {
            StratumKind::All => "all",
            StratumKind::Defi => "defi",
            StratumKind::Transfer => "transfer",
        }
    }

    pub const ALL: [StratumKind; 3] = [StratumKind::All, StratumKind::Defi, StratumKind::Transfer];
}

#[derive(Deserialize)]
struct StrataBlocks {
    defi_heavy: Vec<u64>,
    transfer_only: Vec<u64>,
}

#[derive(Deserialize)]
struct StrataFile {
    defi_contracts: Vec<String>,
    blocks: StrataBlocks,
}

pub struct Strata {
    pub defi_contracts: Vec<Address>,
    defi_heavy: HashSet<u64>,
    transfer_only: HashSet<u64>,
}

impl Strata {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading strata {}", path.display()))?;
        let f: StrataFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing strata {}", path.display()))?;
        let defi_contracts = f
            .defi_contracts
            .iter()
            .map(|s| Address::from_hex(s).map_err(|e| anyhow::anyhow!("bad defi contract {s}: {e}")))
            .collect::<Result<Vec<_>>>()?;
        Ok(Strata {
            defi_contracts,
            defi_heavy: f.blocks.defi_heavy.into_iter().collect(),
            transfer_only: f.blocks.transfer_only.into_iter().collect(),
        })
    }

    pub fn in_stratum(&self, kind: StratumKind, block: u64) -> bool {
        match kind {
            StratumKind::All => true,
            StratumKind::Defi => self.defi_heavy.contains(&block),
            StratumKind::Transfer => self.transfer_only.contains(&block),
        }
    }

    pub fn count(&self, kind: StratumKind, total_blocks: u64) -> u64 {
        match kind {
            StratumKind::All => total_blocks,
            StratumKind::Defi => self.defi_heavy.len() as u64,
            StratumKind::Transfer => self.transfer_only.len() as u64,
        }
    }
}
