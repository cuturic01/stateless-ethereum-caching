//! The Rust stem derivation against the fixture the Python side also consumes.
//! See analytics/tests/test_stem_derivation.py and extention-plan.md §7.

use std::path::PathBuf;

use caching_strategies::model::{Address, Key, SLOT_LEN};
use caching_strategies::witness::{key_to_leaf, HEADER_STORAGE_OFFSET};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    header_storage_offset: u64,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    addr: String,
    slot: Option<String>,
    storage: bool,
    chunk: String,
    suffix: u8,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("caching_strategies has a parent")
        .join("fixtures/stem_derivation.json")
}

fn unhex_slot(s: &str) -> [u8; SLOT_LEN] {
    let h = s.strip_prefix("0x").unwrap_or(s);
    assert_eq!(h.len(), SLOT_LEN * 2, "slot hex must be 32 bytes: {s}");
    let mut out = [0u8; SLOT_LEN];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).expect("valid hex");
    }
    out
}

#[test]
fn stem_derivation_matches_shared_fixture() {
    let raw = std::fs::read(fixture_path()).expect("fixtures/stem_derivation.json is readable");
    let fx: Fixture = serde_json::from_slice(&raw).expect("fixture parses");
    assert_eq!(fx.header_storage_offset, HEADER_STORAGE_OFFSET);
    assert!(!fx.cases.is_empty());

    for c in &fx.cases {
        let addr = Address::from_hex(&c.addr).expect("valid address");
        let key = match &c.slot {
            None => Key::account(addr),
            Some(s) => Key::storage(addr, unhex_slot(s)),
        };
        let (stem, suffix) = key_to_leaf(&key);

        assert_eq!(stem.addr, addr, "{}", c.name);
        assert_eq!(stem.storage, c.storage, "{}: storage flag", c.name);
        assert_eq!(hex31(&stem.chunk), c.chunk, "{}: chunk", c.name);
        assert_eq!(suffix, c.suffix, "{}: suffix", c.name);
    }
}

fn hex31(b: &[u8; 31]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
