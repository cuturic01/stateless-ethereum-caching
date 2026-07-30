use crate::hash::FastMap;
use crate::model::{Address, Key, SLOT_LEN};

pub const COMMITMENT_BYTES: u64 = 32;
pub const STEM_BYTES: u64 = 31;
pub const SUFFIX_BYTES: u64 = 1;
pub const VALUE_BYTES: u64 = 32;
pub const DEPTH_EXT_BYTES: u64 = 1;
pub const EXT_COMMITMENTS_BYTES: u64 = 2 * COMMITMENT_BYTES; // 64

pub const IPA_LR_ROUNDS: u64 = 8;
pub const IPA_PROOF_BYTES: u64 = (2 * IPA_LR_ROUNDS + 1) * COMMITMENT_BYTES; // 544
pub const MULTIPROOF_D_BYTES: u64 = COMMITMENT_BYTES; // 32
pub const IPA_FLOOR_BYTES: u64 = MULTIPROOF_D_BYTES + IPA_PROOF_BYTES; // 576

pub const STEM_SCAFFOLD_BYTES: u64 = STEM_BYTES + DEPTH_EXT_BYTES + EXT_COMMITMENTS_BYTES; // 96

pub const HEADER_STORAGE_OFFSET: u64 = 64;
pub const VERKLE_NODE_WIDTH: u64 = 256;

pub const HEADER_LEAVES_NOTE: &str =
    "header counted as 1 leaf at suffix 0 (EIP-6800 spreads it over suffixes 0..=4)";

#[inline]
pub fn commitments_by_path_bytes(distinct_stems: u64) -> u64 {
    COMMITMENT_BYTES * distinct_stems
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StemId {
    pub addr: Address,
    pub storage: bool,
    pub chunk: [u8; 31],
}

impl StemId {
    fn header(addr: Address) -> Self {
        StemId { addr, storage: false, chunk: [0u8; 31] }
    }
    fn storage(addr: Address, chunk: [u8; 31]) -> Self {
        StemId { addr, storage: true, chunk }
    }
}

pub fn key_to_leaf(key: &Key) -> (StemId, u8) {
    match key.slot {
        None => (StemId::header(key.addr), 0),
        Some(slot) => {
            if slot_below_header_offset(&slot) {
                // s < 64: shares the header stem, suffix 64 + s.
                let suffix = HEADER_STORAGE_OFFSET as u8 + slot[SLOT_LEN - 1];
                (StemId::header(key.addr), suffix)
            } else {
                // s >= 64: main storage. chunk = s / 256 = the high 31 bytes;
                // suffix = s % 256 = the low byte.
                let mut chunk = [0u8; 31];
                chunk.copy_from_slice(&slot[..SLOT_LEN - 1]);
                let suffix = slot[SLOT_LEN - 1];
                (StemId::storage(key.addr, chunk), suffix)
            }
        }
    }
}

#[inline]
fn slot_below_header_offset(slot: &[u8; SLOT_LEN]) -> bool {
    slot[..SLOT_LEN - 1].iter().all(|&b| b == 0)
        && (slot[SLOT_LEN - 1] as u64) < HEADER_STORAGE_OFFSET
}

#[derive(Clone, Copy)]
struct LeafState {
    resident: bool,
    written: bool,
}

#[derive(Default)]
pub struct BlockWitnessAccum {
    leaves: FastMap<(StemId, u8), LeafState>,
}

impl BlockWitnessAccum {
    pub fn record_leaf(&mut self, key: &Key, resident: bool, written: bool) {
        let id = key_to_leaf(key);
        self.leaves
            .entry(id)
            .and_modify(|s| {
                s.resident &= resident;
                s.written |= written;
            })
            .or_insert(LeafState { resident, written });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockWitnessBytes {
    pub naive: u64,
    pub sent: u64,
    pub saved: u64,
    pub floor: u64,
}

#[derive(Default, Clone, Copy)]
struct StemAcc {
    dirty: bool,
    resident: bool,
    leaf_naive: u64,
    leaf_sent: u64,
}

pub fn seal(accum: &BlockWitnessAccum) -> BlockWitnessBytes {
    let mut stems: FastMap<StemId, StemAcc> = FastMap::default();

    for ((stem, _suffix), st) in &accum.leaves {
        let leaf_naive = SUFFIX_BYTES + VALUE_BYTES + if st.written { VALUE_BYTES } else { 0 };
        let leaf_sent = if st.resident && !st.written { 0 } else { leaf_naive };

        let e = stems.entry(*stem).or_default();
        e.leaf_naive += leaf_naive;
        e.leaf_sent += leaf_sent;
        e.dirty |= !st.resident || st.written;
        e.resident |= st.resident && !st.written;
    }

    let num_stems = stems.len() as u64;
    let mut naive =
        IPA_FLOOR_BYTES + commitments_by_path_bytes(num_stems) + num_stems * STEM_SCAFFOLD_BYTES;
    let mut sent = IPA_FLOOR_BYTES;

    for e in stems.values() {
        naive += e.leaf_naive;
        let stem_cacheable = e.resident && !e.dirty;
        if !stem_cacheable {
            sent += COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES;
        }
        sent += e.leaf_sent;
    }

    BlockWitnessBytes { naive, sent, saved: naive - sent, floor: IPA_FLOOR_BYTES }
}
