//! Offline loader round-trip: build a tiny shard the same way the Python scraper
//! does (msgpack positional records → zstd), write a manifest, then stream it back
//! through `ShardStream` and assert the records survive byte-for-byte.

use std::path::Path;

use caching_strategies::loader::{load_shard, ShardStream};
use caching_strategies::manifest::Manifest;
use caching_strategies::model::{Address, Key, ADDR_LEN, SLOT_LEN};

use serde::Serialize;
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};

/// Mirror of the wire types, on the serialize side.
#[derive(Serialize)]
struct WireKey(ByteBuf, Option<ByteBuf>);
#[derive(Serialize)]
struct WireRecord(u64, u64, Vec<WireKey>, Vec<WireKey>);

fn addr(n: u8) -> Address {
    Address([n; ADDR_LEN])
}

fn wkey(a: u8, slot: Option<u8>) -> WireKey {
    WireKey(
        ByteBuf::from(vec![a; ADDR_LEN]),
        slot.map(|s| ByteBuf::from(vec![s; SLOT_LEN])),
    )
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Build a 3-block shard, return (compressed_bytes, sha256_hex).
fn build_shard() -> (Vec<u8>, String) {
    let records = vec![
        // block 100: read account header + slot; write the slot
        WireRecord(
            100,
            1_700_000_000,
            vec![wkey(1, None), wkey(1, Some(0))],
            vec![wkey(1, Some(0))],
        ),
        // block 101: two storage reads on different contracts, no writes
        WireRecord(101, 1_700_000_012, vec![wkey(2, Some(7)), wkey(3, Some(9))], vec![]),
        // block 102: empty read set, one account write
        WireRecord(102, 1_700_000_024, vec![], vec![wkey(2, None)]),
    ];
    let packed = rmp_serde::to_vec(&records).unwrap();
    let compressed = zstd::stream::encode_all(&packed[..], 10).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&compressed);
    (compressed, hex_lower(&hasher.finalize()))
}

fn write_dataset(dir: &Path, compressed: &[u8], sha: &str) {
    std::fs::create_dir_all(dir.join("shards")).unwrap();
    std::fs::write(dir.join("shards/blocks_00000.msgpack.zst"), compressed).unwrap();
    let manifest = format!(
        r#"{{
  "network": "eth-mainnet",
  "start_block": 100,
  "end_block": 102,
  "shard_size": 500,
  "created_at": "2026-06-10T00:00:00+00:00",
  "schema_version": 1,
  "key_encoding": "raw-bytes:addr20+slot32-or-nil",
  "shards": [
    {{ "file": "shards/blocks_00000.msgpack.zst", "first_block": 100, "last_block": 102, "count": 3, "sha256": "{sha}" }}
  ]
}}"#
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
}

#[test]
fn roundtrip_streams_records_with_sha_verification() {
    let tmp = tempfile::tempdir().unwrap();
    let (compressed, sha) = build_shard();
    write_dataset(tmp.path(), &compressed, &sha);

    let manifest = Manifest::load(tmp.path()).unwrap();
    manifest.assert_schema_v1().unwrap();
    assert_eq!(manifest.block_span(), 3);

    let blocks: Vec<_> = ShardStream::new(&manifest, tmp.path(), true)
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(blocks.len(), 3);
    // ascending
    assert_eq!(blocks[0].block_number, 100);
    assert_eq!(blocks[2].block_number, 102);

    // block 100: nil slot → account header, real slot → storage
    assert_eq!(blocks[0].timestamp, 1_700_000_000);
    assert!(blocks[0].reads.contains(&Key::account(addr(1))));
    assert!(blocks[0].reads.contains(&Key::storage(addr(1), [0; SLOT_LEN])));
    assert_eq!(blocks[0].writes, vec![Key::storage(addr(1), [0; SLOT_LEN])]);

    // block 101: two distinct-contract storage reads, no writes
    assert_eq!(blocks[1].reads.len(), 2);
    assert!(blocks[1].writes.is_empty());

    // block 102: empty reads, one account write
    assert!(blocks[2].reads.is_empty());
    assert_eq!(blocks[2].writes, vec![Key::account(addr(2))]);
}

#[test]
fn corrupted_shard_fails_sha() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut compressed, sha) = build_shard();
    // flip a byte after writing the manifest with the *original* sha
    write_dataset(tmp.path(), &compressed, &sha);
    let last = compressed.len() - 1;
    compressed[last] ^= 0xff;
    std::fs::write(tmp.path().join("shards/blocks_00000.msgpack.zst"), &compressed).unwrap();

    let manifest = Manifest::load(tmp.path()).unwrap();
    let err = ShardStream::new(&manifest, tmp.path(), true)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_err();
    assert!(err.to_string().contains("SHA-256 mismatch"), "got: {err}");
}

#[test]
fn wrong_address_length_is_rejected() {
    // A record whose address is 19 bytes must fail validation.
    #[derive(Serialize)]
    struct BadKey(ByteBuf, Option<ByteBuf>);
    let records = vec![(50u64, 0u64, vec![BadKey(ByteBuf::from(vec![1u8; 19]), None)], Vec::<BadKey>::new())];
    let packed = rmp_serde::to_vec(&records).unwrap();
    let compressed = zstd::stream::encode_all(&packed[..], 10).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.msgpack.zst");
    std::fs::write(&path, &compressed).unwrap();

    let err = load_shard(&path, None).unwrap_err();
    assert!(err.to_string().contains("decoding") || err.to_string().contains("bytes"), "got: {err}");
}
