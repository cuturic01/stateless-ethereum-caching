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

fn parse_by_window(raw: HashMap<String, WorkingSetStat>) -> Result<HashMap<u64, f64>> {
    raw.into_iter()
        .map(|(k, v)| {
            k.parse::<u64>()
                .map(|n| (n, v.mean))
                .map_err(|e| anyhow!("bad window key {k}: {e}"))
        })
        .collect()
}

#[derive(Deserialize)]
struct DatasetStats {
    total_blocks: u64,
    window_working_set: HashMap<String, WorkingSetStat>,
    #[serde(default)]
    window_working_set_stems: Option<HashMap<String, WorkingSetStat>>,
}

pub struct DatasetParams {
    pub total_blocks: u64,
    working_set_mean: HashMap<u64, f64>,
    stem_working_set_mean: Option<HashMap<u64, f64>>,
}

impl DatasetParams {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading dataset stats {}", path.display()))?;
        let s: DatasetStats = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing dataset stats {}", path.display()))?;
        let working_set_mean = parse_by_window(s.window_working_set)?;
        let stem_working_set_mean = s.window_working_set_stems.map(parse_by_window).transpose()?;
        Ok(DatasetParams {
            total_blocks: s.total_blocks,
            working_set_mean,
            stem_working_set_mean,
        })
    }

    pub fn has_stem_working_set(&self) -> bool {
        self.stem_working_set_mean.is_some()
    }

    pub fn stem_working_set(&self, window: u64) -> Result<f64> {
        let m = self.stem_working_set_mean.as_ref().ok_or_else(|| {
            anyhow!(
                "dataset_stats.json has no window_working_set_stems; \
                 re-run analytics/characterize.py to add it"
            )
        })?;
        m.get(&window).copied().ok_or_else(|| {
            anyhow!("no stem working_set mean for window {window} in dataset_stats.json")
        })
    }

   pub fn stem_capacity_entries(&self, window: u64, pct: u32) -> Result<usize> {
        let ws = self.stem_working_set(window)?;
        let entries = (ws * pct as f64 / 100.0).round() as i64;
        Ok(entries.max(1) as usize)
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
        let p = DatasetParams {
            total_blocks: 50400,
            working_set_mean,
            stem_working_set_mean: None,
        };
        assert_eq!(p.capacity_entries(128, 100).unwrap(), 151238);
        assert_eq!(p.capacity_entries(8, 10).unwrap(), 1679);
        assert_eq!(p.capacity_entries(8, 100).unwrap(), 16793);
    }

    #[test]
    fn stem_capacity_uses_the_stem_working_set() {
        let mut working_set_mean = HashMap::new();
        working_set_mean.insert(32u64, 50757.0);
        let mut stem = HashMap::new();
        stem.insert(32u64, 38452.4);
        let p = DatasetParams {
            total_blocks: 50400,
            working_set_mean,
            stem_working_set_mean: Some(stem),
        };
        assert!(p.has_stem_working_set());
        assert_eq!(p.stem_capacity_entries(32, 100).unwrap(), 38452);
        assert_eq!(p.stem_capacity_entries(32, 10).unwrap(), 3845);
        assert_eq!(p.capacity_entries(32, 100).unwrap(), 50757);
    }

    #[test]
    fn missing_stem_working_set_is_a_clear_error() {
        let mut working_set_mean = HashMap::new();
        working_set_mean.insert(32u64, 50757.0);
        let p = DatasetParams {
            total_blocks: 50400,
            working_set_mean,
            stem_working_set_mean: None,
        };
        assert!(!p.has_stem_working_set());
        let err = p.stem_capacity_entries(32, 100).unwrap_err().to_string();
        assert!(err.contains("characterize.py"), "unhelpful error: {err}");
    }
}
