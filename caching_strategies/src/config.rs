use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

pub const WINDOWS: [u64; 5] = [8, 16, 32, 64, 128];

pub const CAPACITY_PCTS: [u32; 5] = [10, 25, 50, 75, 100];

#[derive(Deserialize)]
struct WorkingSetStat {
    mean: f64,
}

#[derive(Deserialize)]
struct DatasetStats {
    total_blocks: u64,
    window_working_set: HashMap<String, WorkingSetStat>,
}

pub struct DatasetParams {
    pub total_blocks: u64,
    working_set_mean: HashMap<u64, f64>,
}

impl DatasetParams {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading dataset stats {}", path.display()))?;
        let s: DatasetStats = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing dataset stats {}", path.display()))?;
        let working_set_mean = s
            .window_working_set
            .into_iter()
            .map(|(k, v)| {
                k.parse::<u64>()
                    .map(|n| (n, v.mean))
                    .map_err(|e| anyhow!("bad window key {k}: {e}"))
            })
            .collect::<Result<HashMap<_, _>>>()?;
        Ok(DatasetParams { total_blocks: s.total_blocks, working_set_mean })
    }

    pub fn working_set(&self, window: u64) -> Result<f64> {
        self.working_set_mean
            .get(&window)
            .copied()
            .ok_or_else(|| anyhow!("no working_set mean for window {window} in dataset_stats.json"))
    }

    pub fn capacity_entries(&self, window: u64, pct: u32) -> Result<usize> {
        let ws = self.working_set(window)?;
        let entries = (ws * pct as f64 / 100.0).round() as i64;
        Ok(entries.max(1) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_resolution() {
        let mut working_set_mean = HashMap::new();
        working_set_mean.insert(8u64, 16792.65);
        working_set_mean.insert(128u64, 151238.02);
        let p = DatasetParams { total_blocks: 50400, working_set_mean };
        assert_eq!(p.capacity_entries(128, 100).unwrap(), 151238);
        assert_eq!(p.capacity_entries(8, 10).unwrap(), 1679);
        assert_eq!(p.capacity_entries(8, 100).unwrap(), 16793);
    }
}
