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

pub const LEAF_ENTRY_BYTES: u64 = SUFFIX_BYTES + VALUE_BYTES; // 33
pub const STEM_ENTRY_BYTES: u64 = COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES; // 128

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
                let suffix = HEADER_STORAGE_OFFSET as u8 + slot[SLOT_LEN - 1];
                (StemId::header(key.addr), suffix)
            } else {
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
    /// Residency as of the start of the block, before its writes are applied.
    /// A write consumes the copy the client already held, so it must not be the
    /// thing that makes its own leaf look cold.
    resident: bool,
    written: bool,
}

#[derive(Default)]
pub struct BlockWitnessAccum {
    leaves: FastMap<(StemId, u8), LeafState>,
    stems: FastMap<StemId, bool>,
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

    pub fn record_stem(&mut self, id: StemId, resident: bool) {
        self.stems.insert(id, resident);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheFootprint {
    pub leaf_entries: u64,
    pub stem_entries: u64,
}

impl CacheFootprint {
    pub fn leaf_bytes(&self) -> u64 {
        self.leaf_entries * LEAF_ENTRY_BYTES
    }
    pub fn stem_bytes(&self) -> u64 {
        self.stem_entries * STEM_ENTRY_BYTES
    }
    pub fn total_bytes(&self) -> u64 {
        self.leaf_bytes() + self.stem_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockWitnessBytes {
    pub naive: u64,
    pub sent: u64,
    pub saved: u64,
    pub floor: u64,
    pub sent_stem: u64,
    pub sent_stem_opt: u64,
}

#[derive(Default, Clone, Copy)]
struct StemAcc {
    dirty: bool,
    resident: bool,
    written: bool,
    leaf_naive: u64,
    leaf_sent: u64,
}

pub fn seal(accum: &BlockWitnessAccum) -> BlockWitnessBytes {
    let mut stems: FastMap<StemId, StemAcc> = FastMap::default();

    for ((stem, _suffix), st) in &accum.leaves {
        let leaf_naive = SUFFIX_BYTES + VALUE_BYTES + if st.written { VALUE_BYTES } else { 0 };
        // A write carries suffix + currentValue + newValue. Only currentValue is
        // reconstructible from a resident entry, so a written leaf still costs
        // its suffix and its new value however warm the cache is.
        let leaf_sent = match (st.resident, st.written) {
            (true, false) => 0,
            (true, true) => LEAF_ENTRY_BYTES,
            (false, _) => leaf_naive,
        };

        let e = stems.entry(*stem).or_default();
        e.leaf_naive += leaf_naive;
        e.leaf_sent += leaf_sent;
        e.dirty |= !st.resident || st.written;
        e.resident |= st.resident && !st.written;
        e.written |= st.written;
    }

    let num_stems = stems.len() as u64;
    let mut naive =
        IPA_FLOOR_BYTES + commitments_by_path_bytes(num_stems) + num_stems * STEM_SCAFFOLD_BYTES;
    let mut sent = IPA_FLOOR_BYTES;
    let mut sent_stem = IPA_FLOOR_BYTES;
    let mut sent_stem_opt = IPA_FLOOR_BYTES;

    for (stem, e) in &stems {
        naive += e.leaf_naive;

        sent += e.leaf_sent;
        sent_stem += e.leaf_sent;
        sent_stem_opt += e.leaf_sent;

        if !(e.resident && !e.dirty) {
            sent += STEM_ENTRY_BYTES;
        }
       let stem_entry_resident = accum.stems.get(stem).copied().unwrap_or(false);
        if !(stem_entry_resident && !e.written) {
            sent_stem += STEM_ENTRY_BYTES;
        }
        if !(e.resident && !e.written) {
            sent_stem_opt += STEM_ENTRY_BYTES;
        }
    }

    debug_assert!(sent_stem_opt <= sent, "the relaxed rule cannot send more than `sent`");

    BlockWitnessBytes {
        naive,
        sent,
        saved: naive - sent,
        floor: IPA_FLOOR_BYTES,
        sent_stem,
        sent_stem_opt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Address, ADDR_LEN};

    const A: Address = Address([7u8; ADDR_LEN]);
    const B: Address = Address([9u8; ADDR_LEN]);

    fn slot_key(addr: Address, n: u64) -> Key {
        let mut s = [0u8; SLOT_LEN];
        s[SLOT_LEN - 8..].copy_from_slice(&n.to_be_bytes());
        Key::storage(addr, s)
    }

    fn accum(leaves: &[(Key, bool, bool)]) -> BlockWitnessAccum {
        let mut a = BlockWitnessAccum::default();
        for (k, resident, written) in leaves {
            a.record_leaf(k, *resident, *written);
        }
        a
    }


    #[test]
    fn account_header_is_suffix_zero() {
        let (stem, suffix) = key_to_leaf(&Key::account(A));
        assert_eq!(suffix, 0);
        assert!(!stem.storage);
        assert_eq!(stem.chunk, [0u8; 31]);
    }

    #[test]
    fn low_slots_share_the_header_stem() {
        let header = key_to_leaf(&Key::account(A)).0;
        // EIP-6800 HEADER_STORAGE_OFFSET: slots 0..63 live at suffixes 64..127
        // of the account's own stem rather than in a storage stem of their own.
        let (s0, suf0) = key_to_leaf(&slot_key(A, 0));
        let (s63, suf63) = key_to_leaf(&slot_key(A, 63));
        assert_eq!(s0, header);
        assert_eq!(s63, header);
        assert_eq!(suf0, 64);
        assert_eq!(suf63, 127);
    }

    #[test]
    fn slot_64_is_the_first_main_storage_leaf() {
        // The boundary: 63 is still header, 64 is the first main-storage slot.
        let (stem, suffix) = key_to_leaf(&slot_key(A, 64));
        assert!(stem.storage);
        assert_eq!(stem.chunk, [0u8; 31], "slot 64 sits in storage chunk 0");
        assert_eq!(suffix, 64);
        assert_ne!(stem, key_to_leaf(&slot_key(A, 63)).0);
    }

    #[test]
    fn slot_256_opens_the_next_chunk() {
        let (stem, suffix) = key_to_leaf(&slot_key(A, 256));
        assert!(stem.storage);
        assert_eq!(suffix, 0);
        let mut want = [0u8; 31];
        want[30] = 1;
        assert_eq!(stem.chunk, want);
    }

    #[test]
    fn stems_group_by_256_slot_blocks() {
        let same_a = key_to_leaf(&slot_key(A, 300)).0;
        let same_b = key_to_leaf(&slot_key(A, 400)).0; // both in chunk 1
        let other = key_to_leaf(&slot_key(A, 556)).0; // chunk 2, i.e. 300 + 256
        assert_eq!(same_a, same_b);
        assert_ne!(same_a, other);
    }

    #[test]
    fn stems_are_per_address() {
        assert_ne!(key_to_leaf(&slot_key(A, 300)).0, key_to_leaf(&slot_key(B, 300)).0);
        assert_ne!(key_to_leaf(&Key::account(A)).0, key_to_leaf(&Key::account(B)).0);
    }

    #[test]
    fn header_path_needs_the_whole_slot_below_64() {
        let mut s = [0u8; SLOT_LEN];
        s[SLOT_LEN - 2] = 1; // slot = 256
        assert!(!slot_below_header_offset(&s));
        s[SLOT_LEN - 1] = 5; // slot = 261, low byte 5 < 64
        assert!(!slot_below_header_offset(&s));

        let mut low = [0u8; SLOT_LEN];
        low[SLOT_LEN - 1] = 63;
        assert!(slot_below_header_offset(&low));
        low[SLOT_LEN - 1] = 64;
        assert!(!slot_below_header_offset(&low), "64 is the exclusive bound");
    }

    #[test]
    fn empty_block_is_just_the_proof_floor() {
        let wb = seal(&accum(&[]));
        assert_eq!(wb.naive, IPA_FLOOR_BYTES);
        assert_eq!(wb.sent, IPA_FLOOR_BYTES);
        assert_eq!(wb.floor, IPA_FLOOR_BYTES);
        assert_eq!(wb.saved, 0);
    }

    #[test]
    fn single_cold_leaf_saves_nothing() {
        let wb = seal(&accum(&[(Key::account(A), false, false)]));
        assert_eq!(wb.naive, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33);
        assert_eq!(wb.sent, wb.naive);
        assert_eq!(wb.saved, 0);
    }

    #[test]
    fn resident_unwritten_leaf_elides_its_stem() {
        let wb = seal(&accum(&[(Key::account(A), true, false)]));
        assert_eq!(wb.sent, IPA_FLOOR_BYTES, "leaf and its stem both elided");
        assert_eq!(wb.saved, wb.naive - IPA_FLOOR_BYTES);
    }

    #[test]
    fn stem_is_charged_once_for_all_its_leaves() {
        let wb = seal(&accum(&[
            (slot_key(A, 300), true, false),
            (slot_key(A, 400), false, false),
        ]));
        assert_eq!(wb.sent, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33);
        assert_eq!(wb.naive, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33 + 33);
    }

    #[test]
    fn account_header_and_low_slot_share_one_stem_charge() {
        let wb = seal(&accum(&[(Key::account(A), false, false), (slot_key(A, 0), false, false)]));
        assert_eq!(wb.naive, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33 + 33);
    }

    #[test]
    fn cold_written_leaf_costs_65_and_dirties_its_stem() {
        let wb = seal(&accum(&[(Key::account(A), false, true)]));
        assert_eq!(wb.naive, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 65);
        assert_eq!(wb.sent, wb.naive);
        assert_eq!(wb.saved, 0);
    }

    #[test]
    fn resident_written_leaf_pays_for_the_new_value_only() {
        let wb = seal(&accum(&[(Key::account(A), true, true)]));
        assert_eq!(wb.naive, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 65);
        assert_eq!(wb.sent, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33);
        assert_eq!(wb.saved, VALUE_BYTES);
    }

    #[test]
    fn a_write_dirties_the_stem_for_its_resident_siblings() {
        let wb = seal(&accum(&[
            (slot_key(A, 300), true, false),
            (slot_key(A, 400), true, true),
        ]));
        assert_eq!(wb.sent, IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 33);
    }

    #[test]
    fn read_and_write_of_one_key_merges_to_written() {
        let k = Key::account(A);
        let read_then_write = seal(&accum(&[(k, true, false), (k, false, true)]));
        let write_then_read = seal(&accum(&[(k, false, true), (k, true, false)]));
        assert_eq!(read_then_write, write_then_read);
        assert_eq!(
            read_then_write.sent,
            IPA_FLOOR_BYTES + COMMITMENT_BYTES + STEM_SCAFFOLD_BYTES + 65
        );
    }

    #[test]
    fn naive_ignores_residency() {
        let keys = [Key::account(A), slot_key(A, 0), slot_key(A, 300), slot_key(B, 900)];
        let cold: Vec<_> = keys.iter().map(|k| (*k, false, false)).collect();
        let warm: Vec<_> = keys.iter().map(|k| (*k, true, false)).collect();
        let cold_wb = seal(&accum(&cold));
        let warm_wb = seal(&accum(&warm));
        assert_eq!(cold_wb.naive, warm_wb.naive);
        assert_ne!(cold_wb.sent, warm_wb.sent, "residency must still move `sent`");
    }
}
