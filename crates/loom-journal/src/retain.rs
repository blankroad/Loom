//! Retention GC (contract 06 §6.6, master D7/E6).
//!
//! * Unit = sealed segment (`.lj` + `.idx` + quarantine marker). Active (`.lj.open`) and pinned
//!   segments are never deleted. The mapping log (`_mapping`) counts toward usage but is never
//!   deleted here (it is the canonical order record).
//! * Pass 1: delete unpinned sealed segments older than `max_age` (file mtime = seal time).
//! * Pass 2: if usage ≥ 90 % of `max_bytes`, delete the oldest unpinned sealed segments until
//!   usage < 90 %.
//! * Result level: < 80 % `Ok`, ≥ 80 % `Warn`, ≥ 90 % `Prune` (could not get below 90 %),
//!   ≥ 100 % `Full` → the caller sets `record.health = failed`.
//! * Every deletion is appended (and fsynced) to `<root>/retention-manifest.jsonl` **before**
//!   the files are removed. Deletions are not `record.gap`s.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use loom_proto::num::U64;
use serde::Serialize;

use crate::error::JournalError;
use crate::index::read_index;
use crate::mapping::MAPPING_PRODUCER;
use crate::segment::{index_path, list_segments, producer_epoch_dirs, quarantine_path};

/// Manifest file name under the session root.
pub const RETENTION_MANIFEST: &str = "retention-manifest.jsonl";
/// Default maximum age (30 days).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Default quota (20 GiB).
pub const DEFAULT_MAX_BYTES: u64 = 20 * 1024 * 1024 * 1024;

/// Retention limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainPolicy {
    /// Delete sealed segments older than this.
    pub max_age: Duration,
    /// Quota.
    pub max_bytes: u64,
}

impl Default for RetainPolicy {
    fn default() -> Self {
        RetainPolicy {
            max_age: DEFAULT_MAX_AGE,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

/// Identity of a segment (for pins).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SegmentId {
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// Segment number.
    pub seg_no: u32,
}

/// Quota stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum QuotaLevel {
    /// < 80 %.
    Ok,
    /// ≥ 80 %: warn.
    Warn,
    /// ≥ 90 % even after pruning (pins/active hold the space).
    Prune,
    /// ≥ 100 %: `record.health = failed`.
    Full,
}

/// Why a segment was deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteReason {
    /// Older than `max_age`.
    MaxAge,
    /// Quota pruning.
    Quota,
}

/// One deleted segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedSegment {
    /// Which segment.
    pub id: SegmentId,
    /// Bytes freed.
    pub bytes: u64,
    /// Why.
    pub reason: DeleteReason,
    /// First `source_seq` (from its index) when known.
    pub first_seq: Option<u64>,
}

/// Result of [`retain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainReport {
    /// Usage before.
    pub bytes_before: u64,
    /// Usage after.
    pub bytes_after: u64,
    /// Quota.
    pub max_bytes: u64,
    /// Stage after GC.
    pub level: QuotaLevel,
    /// Deleted segments, oldest first.
    pub deleted: Vec<DeletedSegment>,
    /// Pinned segments kept although eligible.
    pub pinned_kept: usize,
}

#[derive(Debug)]
struct Candidate {
    id: SegmentId,
    dir: PathBuf,
    files: Vec<PathBuf>,
    bytes: u64,
    mtime: SystemTime,
}

#[derive(Serialize)]
struct ManifestLine<'a> {
    wall_ns: U64,
    producer_id: &'a str,
    producer_epoch: U64,
    seg_no: u32,
    bytes: U64,
    reason: DeleteReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_seq: Option<U64>,
}

fn file_len(p: &Path) -> u64 {
    fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn permille(used: u64, max: u64) -> u128 {
    if max == 0 {
        return u128::MAX;
    }
    u128::from(used) * 1000 / u128::from(max)
}

fn level_of(used: u64, max: u64) -> QuotaLevel {
    match permille(used, max) {
        p if p >= 1000 => QuotaLevel::Full,
        p if p >= 900 => QuotaLevel::Prune,
        p if p >= 800 => QuotaLevel::Warn,
        _ => QuotaLevel::Ok,
    }
}

/// Run retention now.
pub fn retain(
    root: &Path,
    policy: &RetainPolicy,
    pins: &HashSet<SegmentId>,
) -> Result<RetainReport, JournalError> {
    retain_at(root, policy, pins, SystemTime::now())
}

/// Run retention as of `now` (testable clock).
pub fn retain_at(
    root: &Path,
    policy: &RetainPolicy,
    pins: &HashSet<SegmentId>,
    now: SystemTime,
) -> Result<RetainReport, JournalError> {
    let mut total = 0u64;
    let mut cands: Vec<Candidate> = Vec::new();
    let mut pinned_kept = 0usize;
    for pd in producer_epoch_dirs(root)? {
        for s in list_segments(&pd.path)? {
            let idx = index_path(&pd.path, s.seg_no);
            let q = quarantine_path(&pd.path, s.seg_no);
            let files: Vec<PathBuf> = [s.path.clone(), idx, q]
                .into_iter()
                .filter(|p| p.exists())
                .collect();
            let bytes: u64 = files.iter().map(|p| file_len(p)).sum();
            total += bytes;
            if !s.sealed || pd.producer_id == MAPPING_PRODUCER {
                continue;
            }
            let id = SegmentId {
                producer_id: pd.producer_id.clone(),
                producer_epoch: pd.producer_epoch,
                seg_no: s.seg_no,
            };
            if pins.contains(&id) {
                pinned_kept += 1;
                continue;
            }
            let mtime = fs::metadata(&s.path)
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            cands.push(Candidate {
                id,
                dir: pd.path.clone(),
                files,
                bytes,
                mtime,
            });
        }
    }
    cands.sort_by(|a, b| a.mtime.cmp(&b.mtime).then(a.id.cmp(&b.id)));
    let bytes_before = total;
    let mut deleted = Vec::new();
    let manifest = root.join(RETENTION_MANIFEST);
    let mut remaining = Vec::new();
    for c in cands {
        let age = now.duration_since(c.mtime).unwrap_or(Duration::ZERO);
        if age > policy.max_age {
            total -= c.bytes;
            deleted.push(delete(&manifest, c, DeleteReason::MaxAge)?);
        } else {
            remaining.push(c);
        }
    }
    for c in remaining {
        if permille(total, policy.max_bytes) < 900 {
            break;
        }
        total -= c.bytes;
        deleted.push(delete(&manifest, c, DeleteReason::Quota)?);
    }
    Ok(RetainReport {
        bytes_before,
        bytes_after: total,
        max_bytes: policy.max_bytes,
        level: level_of(total, policy.max_bytes),
        deleted,
        pinned_kept,
    })
}

fn delete(manifest: &Path, c: Candidate, reason: DeleteReason) -> Result<DeletedSegment, JournalError> {
    let first_seq = read_index(&index_path(&c.dir, c.id.seg_no))
        .ok()
        .and_then(|i| i.first().map(|e| e.key));
    let line = ManifestLine {
        wall_ns: U64(crate::segment::wall_ns_now()),
        producer_id: &c.id.producer_id,
        producer_epoch: U64(c.id.producer_epoch),
        seg_no: c.id.seg_no,
        bytes: U64(c.bytes),
        reason,
        first_seq: first_seq.map(U64),
    };
    let mut text = serde_json::to_string(&line).map_err(|e| JournalError::Config(e.to_string()))?;
    text.push('\n');
    {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(manifest)
            .map_err(JournalError::io(manifest))?;
        f.write_all(text.as_bytes()).map_err(JournalError::io(manifest))?;
        f.sync_data().map_err(JournalError::io(manifest))?;
    }
    for p in &c.files {
        match fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(JournalError::io(p)(e)),
        }
    }
    Ok(DeletedSegment {
        id: c.id,
        bytes: c.bytes,
        reason,
        first_seq,
    })
}
