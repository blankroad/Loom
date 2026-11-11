//! Segment files: paths, listing, frame iteration, crash recovery, verification/quarantine and
//! index rebuild (contract 06 §6.2–6.3).
//!
//! Layout under a session root:
//! ```text
//! <root>/<producer_id>/<producer_epoch>/<seg_no:06>.lj.open        active (being written)
//! <root>/<producer_id>/<producer_epoch>/<seg_no:06>.lj             sealed (fsync → rename)
//! <root>/<producer_id>/<producer_epoch>/<seg_no:06>.idx            sparse index (derived)
//! <root>/<producer_id>/<producer_epoch>/<seg_no:06>.lj.quarantine  corruption marker (JSON)
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use loom_proto::event::GapCount;
use serde_json::json;

use crate::error::JournalError;
use crate::format::{
    Codec, FRAME_HEADER_LEN, HeaderError, IndexHint, RecType, SegmentHeader, json_event_key,
    json_mapping_key, read_frame,
};
use crate::gap::{GapReport, causes};
use crate::index::{IndexEntry, IndexPolicy, IndexState, read_index, write_index_atomic};

/// Directory of one producer epoch.
pub fn producer_dir(root: &Path, producer_id: &str, producer_epoch: u64) -> PathBuf {
    root.join(producer_id).join(producer_epoch.to_string())
}

/// File name of a segment.
pub fn segment_file_name(seg_no: u32, sealed: bool) -> String {
    if sealed {
        format!("{seg_no:06}.lj")
    } else {
        format!("{seg_no:06}.lj.open")
    }
}

/// Path of a segment's sparse index.
pub fn index_path(dir: &Path, seg_no: u32) -> PathBuf {
    dir.join(format!("{seg_no:06}.idx"))
}

/// Path of a segment's quarantine marker.
pub fn quarantine_path(dir: &Path, seg_no: u32) -> PathBuf {
    dir.join(format!("{seg_no:06}.lj.quarantine"))
}

/// Validate a producer id used as a path component.
pub(crate) fn validate_component(s: &str) -> Result<(), JournalError> {
    let ok = !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
    if ok {
        Ok(())
    } else {
        Err(JournalError::Config(format!(
            "producer_id {s:?} is not a safe path component"
        )))
    }
}

fn parse_digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `(seg_no, sealed)` of a segment file name.
pub fn parse_segment_name(name: &str) -> Option<(u32, bool)> {
    if let Some(stem) = name.strip_suffix(".lj.open") {
        parse_digits(stem).map(|n| (n, false))
    } else if let Some(stem) = name.strip_suffix(".lj") {
        parse_digits(stem).map(|n| (n, true))
    } else {
        None
    }
}

/// One segment file found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentInfo {
    /// Segment number.
    pub seg_no: u32,
    /// Current path.
    pub path: PathBuf,
    /// Sealed (`.lj`) vs active (`.lj.open`).
    pub sealed: bool,
    /// A quarantine marker exists.
    pub quarantined: bool,
    /// File length when listed.
    pub len: u64,
}

/// List the segments of a producer-epoch directory, sorted by `seg_no`. Missing dir → empty.
/// If a rename races the listing and both names show up, the sealed one wins.
pub fn list_segments(dir: &Path) -> Result<Vec<SegmentInfo>, JournalError> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(JournalError::io(dir)(e)),
    };
    let mut out: Vec<SegmentInfo> = Vec::new();
    for ent in rd {
        let ent = ent.map_err(JournalError::io(dir))?;
        let name = ent.file_name();
        let Some((seg_no, sealed)) = name.to_str().and_then(parse_segment_name) else {
            continue;
        };
        let len = ent.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(SegmentInfo {
            seg_no,
            path: ent.path(),
            sealed,
            quarantined: quarantine_path(dir, seg_no).exists(),
            len,
        });
    }
    out.sort_by(|a, b| a.seg_no.cmp(&b.seg_no).then(b.sealed.cmp(&a.sealed)));
    out.dedup_by(|later, first| later.seg_no == first.seg_no);
    Ok(out)
}

/// Segment number from a segment path.
pub(crate) fn seg_no_of(path: &Path) -> Option<u32> {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(parse_segment_name)
        .map(|(n, _)| n)
}

/// One frame read from a segment, with the event identity derived from it.
#[derive(Debug)]
pub(crate) struct FrameItem {
    /// Offset of this frame.
    pub offset: u64,
    /// Offset to restart at to re-read this record with full state (its hint frame, if any).
    pub resume_offset: u64,
    /// Derived `source_seq` of the event before this one (for resuming opaque segments).
    pub seq_before: Option<u64>,
    /// Frame length incl. header.
    pub len: u64,
    pub rec_type: RecType,
    pub payload: Vec<u8>,
    /// `source_seq` / `seq_first` (events, mapping) or hinted seq (hint frames).
    pub key: Option<u64>,
    /// Exact `mono_ns` when known.
    pub mono: Option<u64>,
    /// Last known `mono_ns` (lower bound for opaque events without a hint).
    pub approx_mono: Option<u64>,
    /// Hint frames: the writer put an index point here.
    pub index_point: bool,
}

#[derive(Debug)]
pub(crate) enum Step {
    Item(FrameItem),
    End,
    Bad { offset: u64, reason: String },
}

/// Sequential frame reader for one segment.
#[derive(Debug)]
pub(crate) struct SegmentIter {
    reader: BufReader<File>,
    pub header: SegmentHeader,
    pub header_len: u64,
    pub offset: u64,
    last_seq: Option<u64>,
    hint: Option<(IndexHint, u64, Option<u64>)>,
    approx_mono: Option<u64>,
    buf: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum OpenError {
    Io(io::Error),
    Header(HeaderError),
}

impl SegmentIter {
    pub(crate) fn open(path: &Path) -> Result<Self, OpenError> {
        let f = File::open(path).map_err(OpenError::Io)?;
        let mut reader = BufReader::with_capacity(64 * 1024, f);
        let (header, header_len) = SegmentHeader::read_from(&mut reader).map_err(OpenError::Header)?;
        Ok(SegmentIter {
            reader,
            header,
            header_len,
            offset: header_len,
            last_seq: None,
            hint: None,
            approx_mono: None,
            buf: Vec::new(),
        })
    }

    /// Position at `offset` (≥ header end) with the derived seq state `last_seq`.
    pub(crate) fn seek_to(&mut self, offset: u64, last_seq: Option<u64>) -> io::Result<()> {
        let offset = offset.max(self.header_len);
        self.reader.seek(SeekFrom::Start(offset))?;
        self.offset = offset;
        self.last_seq = last_seq;
        self.hint = None;
        Ok(())
    }

    /// Where to restart so the next record is read with full state.
    pub(crate) fn resume_point(&self) -> (u64, Option<u64>) {
        match &self.hint {
            Some((_, off, before)) => (*off, *before),
            None => (self.offset, self.last_seq),
        }
    }

    pub(crate) fn last_seq(&self) -> Option<u64> {
        self.last_seq
    }

    pub(crate) fn next_step(&mut self) -> Step {
        let off = self.offset;
        let rec_type = match read_frame(&mut self.reader, &mut self.buf) {
            Ok(None) => return Step::End,
            Ok(Some(t)) => t,
            Err(e) => {
                return Step::Bad {
                    offset: off,
                    reason: e.to_string(),
                };
            }
        };
        let payload = std::mem::take(&mut self.buf);
        let len = FRAME_HEADER_LEN + payload.len() as u64;
        let mut item = FrameItem {
            offset: off,
            resume_offset: off,
            seq_before: self.last_seq,
            len,
            rec_type,
            payload,
            key: None,
            mono: None,
            approx_mono: self.approx_mono,
            index_point: false,
        };
        match rec_type {
            RecType::IndexHint => {
                let Some(h) = IndexHint::decode(&item.payload) else {
                    return Step::Bad {
                        offset: off,
                        reason: "malformed index hint".into(),
                    };
                };
                self.hint = Some((h, off, self.last_seq));
                self.approx_mono = Some(h.mono_ns);
                item.key = Some(h.source_seq);
                item.mono = Some(h.mono_ns);
                item.approx_mono = Some(h.mono_ns);
                item.index_point = h.index_point;
            }
            RecType::Event if self.header.codec.is_opaque() => match self.hint.take() {
                Some((h, hoff, before)) => {
                    item.key = Some(h.source_seq);
                    item.mono = Some(h.mono_ns);
                    item.resume_offset = hoff;
                    item.seq_before = before;
                }
                None => item.key = self.last_seq.and_then(|s| s.checked_add(1)),
            },
            RecType::Event | RecType::Mapping => {
                let k = if rec_type == RecType::Event {
                    json_event_key(&item.payload)
                } else {
                    json_mapping_key(&item.payload)
                };
                if let Some((key, mono)) = k {
                    item.key = Some(key);
                    item.mono = Some(mono);
                }
            }
            RecType::Marker => {}
        }
        if rec_type.is_keyed() {
            if item.key.is_some() {
                self.last_seq = item.key;
            }
            if let Some(m) = item.mono {
                self.approx_mono = Some(m);
            }
            item.approx_mono = self.approx_mono;
        }
        self.offset += len;
        Step::Item(item)
    }
}

/// Result of recovering one active segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Path after recovery (sealed name), or the removed path if the file held no header.
    pub path: PathBuf,
    /// Segment number.
    pub seg_no: u32,
    /// Valid keyed records kept.
    pub records: u64,
    /// Bytes cut from the tail.
    pub truncated_bytes: u64,
    /// Last `source_seq` kept.
    pub last_seq: Option<u64>,
    /// `crash_tail` gap when bytes were cut.
    pub gap: Option<GapReport>,
}

/// A CRC/format error inside a sealed segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptionReport {
    /// Segment path.
    pub path: PathBuf,
    /// Segment number.
    pub seg_no: u32,
    /// Offset of the first bad frame (reads stop here).
    pub offset: u64,
    /// Reason.
    pub reason: String,
    /// File length.
    pub file_len: u64,
    /// Last good `source_seq` before the damage.
    pub last_seq: Option<u64>,
    /// Producer id (empty if the header itself is damaged).
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
}

impl CorruptionReport {
    /// `record.gap{cause:"corruption"}` for this damage (end and count unknown).
    pub fn gap(&self) -> GapReport {
        GapReport {
            producer_id: self.producer_id.clone(),
            producer_epoch: self.producer_epoch,
            from: self.last_seq.map_or(0, |s| s.saturating_add(1)),
            to: None,
            count: GapCount::Unknown,
            cause: causes::CORRUPTION.to_string(),
            bytes: Some(self.file_len.saturating_sub(self.offset)),
            class: None,
        }
    }
}

/// Result of verifying one segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// Segment path.
    pub path: PathBuf,
    /// Segment number.
    pub seg_no: u32,
    /// Valid keyed records.
    pub records: u64,
    /// End of the last valid frame.
    pub valid_len: u64,
    /// File length.
    pub file_len: u64,
    /// Last valid `source_seq`.
    pub last_seq: Option<u64>,
    /// Damage found (only meaningful for sealed segments).
    pub corruption: Option<CorruptionReport>,
}

pub(crate) fn wall_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Read a whole segment and report its valid prefix. Never modifies the file.
pub fn verify_segment(path: &Path) -> Result<VerifyReport, JournalError> {
    let seg_no = seg_no_of(path).unwrap_or(0);
    let file_len = fs::metadata(path).map_err(JournalError::io(path))?.len();
    let mut it = match SegmentIter::open(path) {
        Ok(it) => it,
        Err(OpenError::Io(e)) => return Err(JournalError::io(path)(e)),
        Err(OpenError::Header(h)) => {
            return Ok(VerifyReport {
                path: path.to_path_buf(),
                seg_no,
                records: 0,
                valid_len: 0,
                file_len,
                last_seq: None,
                corruption: Some(CorruptionReport {
                    path: path.to_path_buf(),
                    seg_no,
                    offset: 0,
                    reason: h.to_string(),
                    file_len,
                    last_seq: None,
                    producer_id: String::new(),
                    producer_epoch: 0,
                }),
            });
        }
    };
    let mut records = 0u64;
    let corruption = loop {
        match it.next_step() {
            Step::Item(item) => {
                if item.rec_type.is_keyed() {
                    records += 1;
                }
            }
            Step::End => break None,
            Step::Bad { offset, reason } => {
                break Some(CorruptionReport {
                    path: path.to_path_buf(),
                    seg_no,
                    offset,
                    reason,
                    file_len,
                    last_seq: it.last_seq(),
                    producer_id: it.header.producer_id.clone(),
                    producer_epoch: it.header.producer_epoch,
                });
            }
        }
    };
    Ok(VerifyReport {
        path: path.to_path_buf(),
        seg_no,
        records,
        valid_len: it.offset,
        file_len,
        last_seq: it.last_seq(),
        corruption,
    })
}

/// Write the quarantine marker for a damaged sealed segment (no-op if it exists). The segment
/// itself is left untouched; readers stop at `offset`.
pub fn quarantine(c: &CorruptionReport) -> Result<(), JournalError> {
    let dir = c.path.parent().unwrap_or(Path::new("."));
    let qp = quarantine_path(dir, c.seg_no);
    if qp.exists() {
        return Ok(());
    }
    let body = json!({
        "offset": c.offset.to_string(),
        "reason": c.reason,
        "file_len": c.file_len.to_string(),
        "detected_wall_ns": wall_ns_now().to_string(),
    });
    fs::write(&qp, body.to_string()).map_err(JournalError::io(&qp))
}

/// Damage offset recorded in a segment's quarantine marker (`None` if there is no readable
/// marker; readers treat an unreadable existing marker as offset 0).
pub fn quarantine_offset(dir: &Path, seg_no: u32) -> Option<u64> {
    let text = fs::read_to_string(quarantine_path(dir, seg_no)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("offset")?.as_str()?.parse().ok()
}

/// Verify every sealed segment of a producer-epoch dir; quarantine damaged ones.
pub fn verify_producer_dir(dir: &Path) -> Result<Vec<VerifyReport>, JournalError> {
    let mut out = Vec::new();
    for s in list_segments(dir)?.into_iter().filter(|s| s.sealed) {
        let r = verify_segment(&s.path)?;
        if let Some(c) = &r.corruption {
            quarantine(c)?;
        }
        out.push(r);
    }
    Ok(out)
}

/// Rebuild `<seg>.idx` from the segment's frames (same points the writer chose).
pub fn rebuild_index(segment: &Path, policy: &IndexPolicy) -> Result<Vec<IndexEntry>, JournalError> {
    let seg_no = seg_no_of(segment)
        .ok_or_else(|| JournalError::Config(format!("not a segment path: {}", segment.display())))?;
    let entries = compute_index(segment, policy)?;
    let dir = segment.parent().unwrap_or(Path::new("."));
    let ip = index_path(dir, seg_no);
    write_index_atomic(&ip, &entries).map_err(JournalError::io(&ip))?;
    Ok(entries)
}

/// Compute the index of a segment without writing it.
pub fn compute_index(segment: &Path, policy: &IndexPolicy) -> Result<Vec<IndexEntry>, JournalError> {
    let mut it = match SegmentIter::open(segment) {
        Ok(it) => it,
        Err(OpenError::Io(e)) => return Err(JournalError::io(segment)(e)),
        Err(OpenError::Header(h)) => {
            return Err(JournalError::BadHeader {
                path: segment.to_path_buf(),
                reason: h.to_string(),
            });
        }
    };
    let opaque = it.header.codec.is_opaque();
    let mut state = IndexState::default();
    let mut out = Vec::new();
    while let Step::Item(item) = it.next_step() {
        if opaque {
            if item.rec_type == RecType::IndexHint
                && item.index_point
                && let (Some(key), Some(mono)) = (item.key, item.mono)
            {
                out.push(IndexEntry {
                    key,
                    mono_ns: mono,
                    offset: item.offset,
                });
            }
        } else if item.rec_type.is_keyed()
            && let (Some(key), Some(mono)) = (item.key, item.mono)
            && state.observe(policy, mono)
        {
            out.push(IndexEntry {
                key,
                mono_ns: mono,
                offset: item.offset,
            });
        }
    }
    Ok(out)
}

pub(crate) fn sync_dir(dir: &Path) {
    // Directory fsync makes the rename durable on Unix; not supported on Windows (ignored).
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Recover one active (`.lj.open`) segment: cut the tail after the last valid frame, fsync,
/// rename to sealed and rebuild its index. A file without a complete header is removed.
pub fn recover_segment(path: &Path, policy: &IndexPolicy) -> Result<RecoveryReport, JournalError> {
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let seg_no = seg_no_of(path)
        .ok_or_else(|| JournalError::Config(format!("not a segment path: {}", path.display())))?;
    let file_len = fs::metadata(path).map_err(JournalError::io(path))?.len();
    let mut it = match SegmentIter::open(path) {
        Ok(it) => it,
        Err(OpenError::Io(e)) => return Err(JournalError::io(path)(e)),
        Err(OpenError::Header(_)) => {
            fs::remove_file(path).map_err(JournalError::io(path))?;
            let _ = fs::remove_file(index_path(&dir, seg_no));
            return Ok(RecoveryReport {
                path: path.to_path_buf(),
                seg_no,
                records: 0,
                truncated_bytes: file_len,
                last_seq: None,
                gap: None,
            });
        }
    };
    let mut records = 0u64;
    while let Step::Item(item) = it.next_step() {
        if item.rec_type.is_keyed() {
            records += 1;
        }
    }
    let valid_len = it.offset;
    let last_seq = it.last_seq();
    let producer_id = it.header.producer_id.clone();
    let producer_epoch = it.header.producer_epoch;
    drop(it);
    let truncated = file_len.saturating_sub(valid_len);
    {
        let f = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(JournalError::io(path))?;
        if truncated > 0 {
            f.set_len(valid_len).map_err(JournalError::io(path))?;
        }
        f.sync_data().map_err(JournalError::io(path))?;
    }
    let sealed = dir.join(segment_file_name(seg_no, true));
    fs::rename(path, &sealed).map_err(JournalError::io(path))?;
    sync_dir(&dir);
    rebuild_index(&sealed, policy)?;
    let gap = (truncated > 0).then(|| GapReport {
        producer_id,
        producer_epoch,
        from: last_seq.map_or(0, |s| s.saturating_add(1)),
        to: None,
        count: GapCount::Unknown,
        cause: causes::CRASH_TAIL.to_string(),
        bytes: Some(truncated),
        class: None,
    });
    Ok(RecoveryReport {
        path: sealed,
        seg_no,
        records,
        truncated_bytes: truncated,
        last_seq,
        gap,
    })
}

/// Recover every active segment of a producer-epoch dir (see [`recover_segment`]). For a
/// segment with no valid record, `gap.from` continues after the previous segment's last seq.
pub fn recover_producer_dir(dir: &Path, policy: &IndexPolicy) -> Result<Vec<RecoveryReport>, JournalError> {
    let segs = list_segments(dir)?;
    let mut out = Vec::new();
    let mut prev_last: Option<u64> = None;
    for s in &segs {
        if s.sealed {
            continue;
        }
        let mut r = recover_segment(&s.path, policy)?;
        if r.last_seq.is_none() {
            if prev_last.is_none() {
                prev_last = segs
                    .iter()
                    .rfind(|p| p.sealed && p.seg_no < s.seg_no)
                    .and_then(|p| segment_last_seq(&p.path).ok().flatten());
            }
            if let (Some(g), Some(p)) = (r.gap.as_mut(), prev_last) {
                g.from = p.saturating_add(1);
            }
        } else {
            prev_last = r.last_seq;
        }
        out.push(r);
    }
    Ok(out)
}

/// Recover all producer-epoch dirs under a session root (`<root>/<producer>/<epoch>`).
pub fn recover_root(root: &Path, policy: &IndexPolicy) -> Result<Vec<RecoveryReport>, JournalError> {
    let mut out = Vec::new();
    for dir in producer_epoch_dirs(root)? {
        out.extend(recover_producer_dir(&dir.path, policy)?);
    }
    Ok(out)
}

/// A `<root>/<producer>/<epoch>` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerEpochDir {
    /// Producer id (directory name).
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// Path.
    pub path: PathBuf,
}

/// List all producer-epoch directories under a session root.
pub fn producer_epoch_dirs(root: &Path) -> Result<Vec<ProducerEpochDir>, JournalError> {
    let mut out = Vec::new();
    let rd = match fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(JournalError::io(root)(e)),
    };
    for p in rd {
        let p = p.map_err(JournalError::io(root))?;
        if !p.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(pid) = p.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let pdir = p.path();
        for e in fs::read_dir(&pdir).map_err(JournalError::io(&pdir))? {
            let e = e.map_err(JournalError::io(&pdir))?;
            let Some(epoch) = e.file_name().to_str().and_then(|n| n.parse::<u64>().ok()) else {
                continue;
            };
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                out.push(ProducerEpochDir {
                    producer_id: pid.clone(),
                    producer_epoch: epoch,
                    path: e.path(),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.producer_id, a.producer_epoch).cmp(&(&b.producer_id, b.producer_epoch)));
    Ok(out)
}

/// Last keyed seq of a segment (reads from the last index point).
pub fn segment_last_seq(path: &Path) -> Result<Option<u64>, JournalError> {
    let mut it = match SegmentIter::open(path) {
        Ok(it) => it,
        Err(OpenError::Io(e)) => return Err(JournalError::io(path)(e)),
        Err(OpenError::Header(_)) => return Ok(None),
    };
    if let (Some(seg_no), Some(dir)) = (seg_no_of(path), path.parent())
        && let Ok(idx) = read_index(&index_path(dir, seg_no))
        && let Some(last) = idx.last()
    {
        it.seek_to(last.offset, None).map_err(JournalError::io(path))?;
    }
    while let Step::Item(_) = it.next_step() {}
    Ok(it.last_seq())
}

/// Codec of a segment file.
pub fn segment_codec(path: &Path) -> Result<Codec, JournalError> {
    match SegmentIter::open(path) {
        Ok(it) => Ok(it.header.codec),
        Err(OpenError::Io(e)) => Err(JournalError::io(path)(e)),
        Err(OpenError::Header(h)) => Err(JournalError::BadHeader {
            path: path.to_path_buf(),
            reason: h.to_string(),
        }),
    }
}
