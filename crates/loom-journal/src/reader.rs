//! Bounded journal scans (contract 06 §6.5).
//!
//! A scan finds its start with the sparse index, then reads frames sequentially across
//! segments (sealed and the active one's written part). It stops at the range end or at a limit:
//! scanned bytes (default 256 MiB), result bytes (default 64 KiB) or the deadline (default 2 s).
//! On a limit it returns `truncated = true` and `resume_from`; the RPC layer answers
//! `E_SCAN_LIMIT` with it. A CRC error inside a **sealed** segment is reported as corruption
//! (the file is quarantined and reading continues with the next segment); in the active
//! segment it just marks the end of the written data.
//!
//! Ordering assumption: `source_seq` strictly increases and `ts.mono_ns` does not decrease
//! within a producer epoch (host monotonic clock assigned at receive). Mono scans stop at the
//! first record past the range end.

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use loom_proto::event::SourceRecord;

use crate::error::JournalError;
use crate::format::RecType;
use crate::index::read_index;
use crate::segment::{
    CorruptionReport, FrameItem, OpenError, SegmentInfo, SegmentIter, Step, index_path, list_segments,
    quarantine, quarantine_offset, segment_file_name,
};

/// Default scanned-bytes limit.
pub const DEFAULT_SCAN_BYTES: u64 = 256 * 1024 * 1024;
/// Default result-bytes limit (RPC).
pub const DEFAULT_RESULT_BYTES: u64 = 64 * 1024;
/// Default time limit.
pub const DEFAULT_SCAN_TIME: Duration = Duration::from_secs(2);

/// Range to scan (both ends inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanRange {
    /// By `source_seq`.
    SourceSeq {
        /// First seq.
        from: u64,
        /// Last seq.
        to: u64,
    },
    /// By `ts.mono_ns`. For opaque codecs, records without a hint are filtered by the last
    /// hinted mono (index granularity).
    Mono {
        /// First mono ns.
        from: u64,
        /// Last mono ns.
        to: u64,
    },
}

/// Scan limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Max frame bytes read.
    pub scan_bytes: u64,
    /// Max payload bytes returned. At least one record is always returned if any matches.
    pub result_bytes: u64,
    /// Stop when this instant passes.
    pub deadline: Option<Instant>,
}

impl ScanLimits {
    /// Contract defaults: 256 MiB scanned, 64 KiB returned, 2 s from now.
    pub fn rpc_default() -> Self {
        ScanLimits {
            scan_bytes: DEFAULT_SCAN_BYTES,
            result_bytes: DEFAULT_RESULT_BYTES,
            deadline: Some(Instant::now() + DEFAULT_SCAN_TIME),
        }
    }

    /// No limits (tests, offline export).
    pub fn unlimited() -> Self {
        ScanLimits {
            scan_bytes: u64::MAX,
            result_bytes: u64::MAX,
            deadline: None,
        }
    }
}

/// Exact place to continue a truncated scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Segment number.
    pub seg_no: u32,
    /// Frame offset in that segment.
    pub offset: u64,
    /// `source_seq` of the event before `offset` (needed to derive seqs in opaque segments).
    pub last_seq: Option<u64>,
}

/// A scan request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanQuery {
    /// Range.
    pub range: ScanRange,
    /// Limits.
    pub limits: ScanLimits,
    /// Continue from a previous `resume_from` (same range).
    pub resume: Option<Position>,
}

/// One event read from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedRecord {
    /// Segment number.
    pub seg_no: u32,
    /// Frame offset.
    pub offset: u64,
    /// `source_seq` (always known for json; derived for opaque codecs).
    pub source_seq: u64,
    /// Exact `mono_ns` when known (json, or hinted opaque records).
    pub mono_ns: Option<u64>,
    /// Payload bytes (SourceRecord JSON, or opaque bytes).
    pub payload: Vec<u8>,
}

impl ScannedRecord {
    /// Decode a `json/1` payload.
    pub fn decode_source_record(&self) -> Result<SourceRecord, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }
}

/// Scan result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanResult {
    /// Matching events in order.
    pub records: Vec<ScannedRecord>,
    /// A limit stopped the scan before the range end.
    pub truncated: bool,
    /// Where to continue when truncated.
    pub resume_from: Option<Position>,
    /// Damaged sealed segments met on the way (quarantined).
    pub corruptions: Vec<CorruptionReport>,
    /// Frame bytes read.
    pub scanned_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locate {
    Key(u64),
    Mono(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Take,
    Skip,
    Stop,
}

#[derive(Debug, Default)]
pub(crate) struct RawScan {
    pub items: Vec<(u32, FrameItem)>,
    pub truncated: bool,
    pub resume_from: Option<Position>,
    pub corruptions: Vec<CorruptionReport>,
    pub scanned_bytes: u64,
}

/// First index entry of a segment, or its first keyed record when the index is missing.
fn segment_first(dir: &Path, s: &SegmentInfo) -> Option<(u64, u64)> {
    if let Ok(idx) = read_index(&index_path(dir, s.seg_no))
        && let Some(e) = idx.first()
    {
        return Some((e.key, e.mono_ns));
    }
    let mut it = open_iter(dir, s).ok()?;
    loop {
        match it.next_step() {
            Step::Item(i) if i.rec_type.is_keyed() => return i.key.zip(i.mono.or(i.approx_mono)),
            Step::Item(_) => {}
            _ => return None,
        }
    }
}

fn open_iter(dir: &Path, s: &SegmentInfo) -> Result<SegmentIter, OpenError> {
    match SegmentIter::open(&s.path) {
        Err(OpenError::Io(e)) if e.kind() == io::ErrorKind::NotFound && !s.sealed => {
            // Sealed (renamed) between listing and open.
            SegmentIter::open(&dir.join(segment_file_name(s.seg_no, true)))
        }
        r => r,
    }
}

fn before(locate: Locate, key: u64, mono: u64) -> bool {
    match locate {
        Locate::Key(k) => key <= k,
        // Strict: equal monos may sit just before an index point.
        Locate::Mono(m) => mono < m,
    }
}

/// Start position: (index into `segs`, offset, last_seq).
fn locate_start(dir: &Path, segs: &[SegmentInfo], locate: Locate) -> (usize, u64, Option<u64>) {
    let mut chosen = 0;
    for (i, s) in segs.iter().enumerate() {
        match segment_first(dir, s) {
            Some((k, m)) if before(locate, k, m) => chosen = i,
            Some(_) => break,
            None => {}
        }
    }
    let Some(s) = segs.get(chosen) else {
        return (0, 0, None);
    };
    let offset = read_index(&index_path(dir, s.seg_no))
        .ok()
        .and_then(|idx| {
            idx.iter()
                .rev()
                .find(|e| before(locate, e.key, e.mono_ns))
                .map(|e| e.offset)
        })
        .unwrap_or(0);
    (chosen, offset, None)
}

pub(crate) fn scan_raw(
    dir: &Path,
    locate: Locate,
    resume: Option<&Position>,
    limits: &ScanLimits,
    mut decide: impl FnMut(&FrameItem) -> Decision,
) -> Result<RawScan, JournalError> {
    let segs = list_segments(dir)?;
    let mut out = RawScan::default();
    let (start, offset, last_seq) = match resume {
        Some(p) => match segs.iter().position(|s| s.seg_no >= p.seg_no) {
            Some(i) if segs[i].seg_no == p.seg_no => (i, p.offset, p.last_seq),
            Some(i) => (i, 0, None),
            None => return Ok(out),
        },
        None => locate_start(dir, &segs, locate),
    };
    let mut result_bytes = 0u64;
    'segs: for (i, s) in segs.iter().enumerate().skip(start) {
        let mut it = match open_iter(dir, s) {
            Ok(it) => it,
            Err(OpenError::Io(e)) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(OpenError::Io(e)) => return Err(JournalError::io(&s.path)(e)),
            Err(OpenError::Header(h)) => {
                if s.sealed {
                    let c = CorruptionReport {
                        path: s.path.clone(),
                        seg_no: s.seg_no,
                        offset: 0,
                        reason: h.to_string(),
                        file_len: s.len,
                        last_seq: None,
                        producer_id: String::new(),
                        producer_epoch: 0,
                    };
                    let _ = quarantine(&c);
                    out.corruptions.push(c);
                }
                continue;
            }
        };
        if i == start && (offset > 0 || last_seq.is_some()) {
            it.seek_to(offset, last_seq).map_err(JournalError::io(&s.path))?;
        }
        // A quarantined segment is read only up to its recorded damage offset, even if the
        // index would jump past it (contract 06 §6.2).
        let q_limit = if s.quarantined {
            Some(quarantine_offset(dir, s.seg_no).unwrap_or(0))
        } else {
            None
        };
        loop {
            if let Some(q) = q_limit
                && it.offset >= q
            {
                out.corruptions.push(CorruptionReport {
                    path: s.path.clone(),
                    seg_no: s.seg_no,
                    offset: q,
                    reason: "quarantined".into(),
                    file_len: s.len,
                    last_seq: it.last_seq(),
                    producer_id: it.header.producer_id.clone(),
                    producer_epoch: it.header.producer_epoch,
                });
                break;
            }
            if out.scanned_bytes >= limits.scan_bytes || limits.deadline.is_some_and(|d| Instant::now() >= d)
            {
                let (offset, last_seq) = it.resume_point();
                out.truncated = true;
                out.resume_from = Some(Position {
                    seg_no: s.seg_no,
                    offset,
                    last_seq,
                });
                break 'segs;
            }
            match it.next_step() {
                Step::Item(item) => {
                    out.scanned_bytes += item.len;
                    match decide(&item) {
                        Decision::Skip => {}
                        Decision::Stop => break 'segs,
                        Decision::Take => {
                            let n = item.payload.len() as u64;
                            if !out.items.is_empty() && result_bytes.saturating_add(n) > limits.result_bytes {
                                out.truncated = true;
                                out.resume_from = Some(Position {
                                    seg_no: s.seg_no,
                                    offset: item.resume_offset,
                                    last_seq: item.seq_before,
                                });
                                break 'segs;
                            }
                            result_bytes += n;
                            out.items.push((s.seg_no, item));
                        }
                    }
                }
                Step::End => break,
                Step::Bad { offset, reason } => {
                    if s.sealed {
                        let c = CorruptionReport {
                            path: s.path.clone(),
                            seg_no: s.seg_no,
                            offset,
                            reason,
                            file_len: s.len,
                            last_seq: it.last_seq(),
                            producer_id: it.header.producer_id.clone(),
                            producer_epoch: it.header.producer_epoch,
                        };
                        let _ = quarantine(&c);
                        out.corruptions.push(c);
                    }
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Scan one producer-epoch directory (see module docs).
pub fn scan(dir: &Path, q: &ScanQuery) -> Result<ScanResult, JournalError> {
    let locate = match q.range {
        ScanRange::SourceSeq { from, .. } => Locate::Key(from),
        ScanRange::Mono { from, .. } => Locate::Mono(from),
    };
    let range = q.range;
    let raw = scan_raw(dir, locate, q.resume.as_ref(), &q.limits, |item| {
        if item.rec_type != RecType::Event {
            return Decision::Skip;
        }
        let Some(key) = item.key else {
            return Decision::Skip;
        };
        match range {
            ScanRange::SourceSeq { from, to } => {
                if key > to {
                    Decision::Stop
                } else if key >= from {
                    Decision::Take
                } else {
                    Decision::Skip
                }
            }
            ScanRange::Mono { from, to } => match item.mono.or(item.approx_mono) {
                Some(m) if m > to => Decision::Stop,
                Some(m) if m >= from => Decision::Take,
                _ => Decision::Skip,
            },
        }
    })?;
    Ok(ScanResult {
        records: raw
            .items
            .into_iter()
            .filter_map(|(seg_no, i)| {
                i.key.map(|k| ScannedRecord {
                    seg_no,
                    offset: i.offset,
                    source_seq: k,
                    mono_ns: i.mono,
                    payload: i.payload,
                })
            })
            .collect(),
        truncated: raw.truncated,
        resume_from: raw.resume_from,
        corruptions: raw.corruptions,
        scanned_bytes: raw.scanned_bytes,
    })
}
