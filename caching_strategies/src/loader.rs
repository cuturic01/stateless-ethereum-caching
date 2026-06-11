use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_bytes::ByteBuf;
use sha2::{Digest, Sha256};

use crate::manifest::Manifest;
use crate::model::{Address, BlockRecord, Key, ADDR_LEN, SLOT_LEN};

#[derive(Deserialize)]
struct WireKey(ByteBuf, Option<ByteBuf>);

#[derive(Deserialize)]
struct WireRecord(u64, u64, Vec<WireKey>, Vec<WireKey>);

fn convert_key(w: WireKey) -> Result<Key> {
    let WireKey(addr, slot) = w;
    if addr.len() != ADDR_LEN {
        bail!("address is {} bytes, expected {}", addr.len(), ADDR_LEN);
    }
    let mut a = [0u8; ADDR_LEN];
    a.copy_from_slice(&addr);
    let addr = Address(a);
    match slot {
        None => Ok(Key::account(addr)),
        Some(s) => {
            if s.len() != SLOT_LEN {
                bail!("slot is {} bytes, expected {}", s.len(), SLOT_LEN);
            }
            let mut buf = [0u8; SLOT_LEN];
            buf.copy_from_slice(&s);
            Ok(Key::storage(addr, buf))
        }
    }
}

fn convert_keys(ws: Vec<WireKey>) -> Result<Vec<Key>> {
    ws.into_iter().map(convert_key).collect()
}

fn convert_record(w: WireRecord) -> Result<BlockRecord> {
    let WireRecord(block_number, timestamp, reads, writes) = w;
    Ok(BlockRecord {
        block_number,
        timestamp,
        reads: convert_keys(reads).context("decoding read set")?,
        writes: convert_keys(writes).context("decoding write set")?,
    })
}

pub fn load_shard(path: &Path, expected_sha256: Option<&str>) -> Result<Vec<BlockRecord>> {
    let compressed = std::fs::read(path)
        .with_context(|| format!("reading shard {}", path.display()))?;

    if let Some(expected) = expected_sha256 {
        let mut hasher = Sha256::new();
        hasher.update(&compressed);
        let digest = hex_lower(&hasher.finalize());
        if digest != expected {
            bail!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                path.display(),
                expected,
                digest
            );
        }
    }

    let raw = zstd::stream::decode_all(&compressed[..])
        .with_context(|| format!("zstd-decompressing {}", path.display()))?;
    let wire: Vec<WireRecord> = rmp_serde::from_slice(&raw)
        .with_context(|| format!("msgpack-decoding {}", path.display()))?;

    wire.into_iter().map(convert_record).collect()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub struct ShardStream<'a> {
    manifest: &'a Manifest,
    data_dir: PathBuf,
    verify_sha: bool,
    next_shard: usize,
    buffer: VecDeque<BlockRecord>,
}

impl<'a> ShardStream<'a> {
    pub fn new(manifest: &'a Manifest, data_dir: &Path, verify_sha: bool) -> Self {
        ShardStream {
            manifest,
            data_dir: data_dir.to_path_buf(),
            verify_sha,
            next_shard: 0,
            buffer: VecDeque::new(),
        }
    }

    fn fill(&mut self) -> Result<bool> {
        if self.next_shard >= self.manifest.shards.len() {
            return Ok(false);
        }
        let entry = &self.manifest.shards[self.next_shard];
        self.next_shard += 1;
        let path = self.data_dir.join(&entry.file);
        let sha = if self.verify_sha { Some(entry.sha256.as_str()) } else { None };
        let records = load_shard(&path, sha)?;
        if records.len() as u64 != entry.count {
            bail!(
                "{}: record count {} != manifest count {}",
                entry.file,
                records.len(),
                entry.count
            );
        }
        self.buffer.extend(records);
        Ok(true)
    }
}

impl Iterator for ShardStream<'_> {
    type Item = Result<BlockRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.buffer.is_empty() {
            match self.fill() {
                Ok(true) => continue,
                Ok(false) => return None,
                Err(e) => return Some(Err(e)),
            }
        }
        self.buffer.pop_front().map(Ok)
    }
}
