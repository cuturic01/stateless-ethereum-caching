use crate::hash::FastMap;
use crate::model::Address;
use crate::witness::{BlockWitnessBytes, CacheFootprint};

pub const SURVIVAL_BUCKETS: usize = 14;

#[derive(Debug, Clone, Default)]
pub struct SurvivalHistogram {
    pub buckets: [u64; SURVIVAL_BUCKETS],
    pub sum_age: u64,
    pub n: u64,
}

impl SurvivalHistogram {
    fn bucket_of(age: u64) -> usize {
        if age < 8 {
            age as usize
        } else {
            let floor_log2 = 63 - age.leading_zeros() as usize;
            (5 + floor_log2).min(SURVIVAL_BUCKETS - 1)
        }
    }

    pub fn record(&mut self, age: u64) {
        self.buckets[Self::bucket_of(age)] += 1;
        self.sum_age += age;
        self.n += 1;
    }

    pub fn mean(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.sum_age as f64 / self.n as f64
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContractInvalidations {
    pub counts: FastMap<Address, u64>,
}

impl ContractInvalidations {
    pub fn record(&mut self, addr: Address) {
        *self.counts.entry(addr).or_insert(0) += 1;
    }

    pub fn top_k(&self, k: usize) -> Vec<(Address, u64)> {
        let mut v: Vec<_> = self.counts.iter().map(|(a, c)| (*a, *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(k);
        v
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlockOutcome {
    pub block_number: u64,
    pub reads: u32,
    pub hits: u32,
    pub misses: u32,
    pub invalidations: u32,
    pub stem_invalidations: u32,
    pub occupancy: u64,
    pub stem_occupancy: u64,
    pub witness: BlockWitnessBytes,
    pub footprint: CacheFootprint,
}

#[derive(Debug, Clone)]
pub struct BlockSample {
    pub block_number: u64,
    pub reads: u32,
    pub hits: u32,
    pub misses: u32,
    pub invalidations: u32,
    pub occupancy: u64,
    pub witness_naive: u64,
    pub witness_sent: u64,
    pub bytes_saved: u64,
    pub floor: u64,
    pub cache_bytes: u64,
    pub bytes_sent_stem: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RunMetrics {
    pub blocks_counted: u64,
    pub total_reads: u64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub total_invalidations: u64,
    pub peak_occupancy: u64,
    pub survival: SurvivalHistogram,
    pub contracts: ContractInvalidations,
    pub series: Vec<BlockSample>,
    pub total_witness_bytes_naive: u64,
    pub total_witness_bytes_sent: u64,
    pub total_bytes_saved: u64,
    pub total_noncacheable_floor_bytes: u64,
    pub peak_cache_bytes: u64,
    pub peak_leaf_bytes: u64,
    pub peak_stem_bytes: u64,
    pub sum_cache_bytes: u64,
    pub total_stem_invalidations: u64,
    pub peak_stem_occupancy: u64,
    pub stem_survival: SurvivalHistogram,
    pub total_witness_bytes_sent_stem: u64,
    pub total_witness_bytes_sent_stem_opt: u64,
}

impl RunMetrics {
    pub fn record_survival(&mut self, age: u64) {
        self.survival.record(age);
    }

    pub fn record_invalidation(&mut self, addr: Address) {
        self.contracts.record(addr);
        self.total_invalidations += 1;
    }

    pub fn record_stem_survival(&mut self, age: u64) {
        self.stem_survival.record(age);
    }

    pub fn record_block(&mut self, o: BlockOutcome) {
        let BlockOutcome {
            block_number,
            reads,
            hits,
            misses,
            invalidations,
            stem_invalidations,
            occupancy,
            stem_occupancy,
            witness: wb,
            footprint: fp,
        } = o;

        self.blocks_counted += 1;
        self.total_reads += reads as u64;
        self.total_hits += hits as u64;
        self.total_misses += misses as u64;
        self.peak_occupancy = self.peak_occupancy.max(occupancy);
        self.peak_stem_occupancy = self.peak_stem_occupancy.max(stem_occupancy);
        self.total_stem_invalidations += stem_invalidations as u64;
        self.total_witness_bytes_naive += wb.naive;
        self.total_witness_bytes_sent += wb.sent;
        self.total_bytes_saved += wb.saved;
        self.total_noncacheable_floor_bytes += wb.floor;
        self.total_witness_bytes_sent_stem += wb.sent_stem;
        self.total_witness_bytes_sent_stem_opt += wb.sent_stem_opt;

        let cache_bytes = fp.total_bytes();
        self.sum_cache_bytes += cache_bytes;
        if cache_bytes > self.peak_cache_bytes {
            self.peak_cache_bytes = cache_bytes;
            self.peak_leaf_bytes = fp.leaf_bytes();
            self.peak_stem_bytes = fp.stem_bytes();
        }
        self.series.push(BlockSample {
            block_number,
            reads,
            hits,
            misses,
            invalidations,
            occupancy,
            witness_naive: wb.naive,
            witness_sent: wb.sent,
            bytes_saved: wb.saved,
            floor: wb.floor,
            cache_bytes,
            bytes_sent_stem: wb.sent_stem,
        });
    }

    pub fn mean_cache_bytes(&self) -> f64 {
        if self.blocks_counted == 0 {
            0.0
        } else {
            self.sum_cache_bytes as f64 / self.blocks_counted as f64
        }
    }

    pub fn overall_hit_rate(&self) -> f64 {
        if self.total_reads == 0 {
            0.0
        } else {
            self.total_hits as f64 / self.total_reads as f64
        }
    }

    pub fn overall_compression_ratio(&self) -> f64 {
        if self.total_witness_bytes_naive == 0 {
            0.0
        } else {
            self.total_bytes_saved as f64 / self.total_witness_bytes_naive as f64
        }
    }

    pub fn stem_compression_ratio(&self) -> f64 {
        if self.total_witness_bytes_naive == 0 {
            0.0
        } else {
            (self.total_witness_bytes_naive - self.total_witness_bytes_sent_stem) as f64
                / self.total_witness_bytes_naive as f64
        }
    }

    pub fn cacheable_fraction(&self) -> f64 {
        if self.total_witness_bytes_naive == 0 {
            0.0
        } else {
            (self.total_witness_bytes_naive - self.total_noncacheable_floor_bytes) as f64
                / self.total_witness_bytes_naive as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survival_bucketing() {
        assert_eq!(SurvivalHistogram::bucket_of(0), 0);
        assert_eq!(SurvivalHistogram::bucket_of(7), 7);
        assert_eq!(SurvivalHistogram::bucket_of(8), 8);
        assert_eq!(SurvivalHistogram::bucket_of(15), 8);
        assert_eq!(SurvivalHistogram::bucket_of(16), 9);
        assert_eq!(SurvivalHistogram::bucket_of(31), 9);
        assert_eq!(SurvivalHistogram::bucket_of(255), 12);
        assert_eq!(SurvivalHistogram::bucket_of(256), 13);
        assert_eq!(SurvivalHistogram::bucket_of(1_000_000), 13);
    }
}
