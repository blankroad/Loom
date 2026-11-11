//! Sparse index sidecar `<seg_no:06>.idx` (M1 stand-in for SQLite `index.db`, contract 06 §6.3).
//!
//! ```text
//! "LOOMIX1\0" | entry*   entry = u64 key | u64 mono_ns | u64 file_offset   (24 B, LE)
//! ```
//! `key` is `source_seq` (event segments) or `seq_first` (mapping log). `file_offset` points at
//! the event frame (`json/1`) or at the [`crate::format::IndexHint`] frame in front of it
//! (opaque codecs). The file is derived data: a trailing partial entry is ignored and the whole
//! file can be rebuilt from its segment ([`crate::segment::rebuild_index`]).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

/// Index file magic.
pub const INDEX_MAGIC: [u8; 8] = *b"LOOMIX1\0";
/// Bytes per index entry.
pub const INDEX_ENTRY_LEN: usize = 24;

/// One sparse index entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexEntry {
    /// `source_seq` (or `seq_first` for mapping segments).
    pub key: u64,
    /// `mono_ns` of that record.
    pub mono_ns: u64,
    /// Byte offset of the frame to start reading at.
    pub offset: u64,
}

impl IndexEntry {
    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.key.to_le_bytes());
        out.extend_from_slice(&self.mono_ns.to_le_bytes());
        out.extend_from_slice(&self.offset.to_le_bytes());
    }

    fn decode(b: &[u8]) -> Self {
        let word = |i: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&b[i..i + 8]);
            u64::from_le_bytes(w)
        };
        IndexEntry {
            key: word(0),
            mono_ns: word(8),
            offset: word(16),
        }
    }
}

/// When to add an index point (contract: every 4096 records or 1 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexPolicy {
    /// Records between points.
    pub every_records: u64,
    /// `mono_ns` distance between points.
    pub every_mono_ns: u64,
}

impl Default for IndexPolicy {
    fn default() -> Self {
        IndexPolicy {
            every_records: 4096,
            every_mono_ns: 1_000_000_000,
        }
    }
}

/// Deterministic point selection shared by the writer and `rebuild_index` (per segment).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct IndexState {
    since: u64,
    last_mono: Option<u64>,
}

impl IndexState {
    /// Observe the next keyed record; true if it is an index point.
    pub(crate) fn observe(&mut self, policy: &IndexPolicy, mono_ns: u64) -> bool {
        let hit = match self.last_mono {
            None => true,
            Some(m) => {
                self.since >= policy.every_records || mono_ns.saturating_sub(m) >= policy.every_mono_ns
            }
        };
        if hit {
            self.last_mono = Some(mono_ns);
            self.since = 1;
        } else {
            self.since += 1;
        }
        hit
    }
}

/// Create (truncate) an index file and write its magic.
pub(crate) fn create_index_file(path: &Path) -> io::Result<File> {
    let mut f = File::create(path)?;
    f.write_all(&INDEX_MAGIC)?;
    Ok(f)
}

/// Read an index file. Missing file → `Ok(empty)`. Bad magic or non-increasing offsets →
/// `Err(InvalidData)` (callers fall back to a sequential read).
pub fn read_index(path: &Path) -> io::Result<Vec<IndexEntry>> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    if buf.len() < INDEX_MAGIC.len() || buf[..INDEX_MAGIC.len()] != INDEX_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad index magic"));
    }
    let body = &buf[INDEX_MAGIC.len()..];
    let mut out = Vec::with_capacity(body.len() / INDEX_ENTRY_LEN);
    for chunk in body.chunks_exact(INDEX_ENTRY_LEN) {
        let e = IndexEntry::decode(chunk);
        if out
            .last()
            .is_some_and(|p: &IndexEntry| p.offset >= e.offset || p.key > e.key)
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "index not increasing"));
        }
        out.push(e);
    }
    Ok(out)
}

/// Write an index file atomically (temp file + fsync + rename).
pub(crate) fn write_index_atomic(path: &Path, entries: &[IndexEntry]) -> io::Result<()> {
    let tmp = path.with_extension("idx.tmp");
    let mut buf = Vec::with_capacity(INDEX_MAGIC.len() + entries.len() * INDEX_ENTRY_LEN);
    buf.extend_from_slice(&INDEX_MAGIC);
    for e in entries {
        e.encode_into(&mut buf);
    }
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&buf)?;
        f.sync_data()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_points() {
        let p = IndexPolicy {
            every_records: 3,
            every_mono_ns: 100,
        };
        let mut s = IndexState::default();
        let hits: Vec<bool> = [0u64, 1, 2, 3, 4, 200, 201]
            .iter()
            .map(|m| s.observe(&p, *m))
            .collect();
        assert_eq!(hits, vec![true, false, false, true, false, true, false]);
    }
}
