//! Stream mapping log (contract 06 §6.3): the hub's record of which source range became which
//! stream seq range. It is the only way to restore global order, so it is **never dropped**:
//! when the writer falls behind or the disk fails, [`MappingLog::append_mapping`] blocks for at
//! most [`MAPPING_BLOCK`] and then returns [`MappingError::Backpressure`]; the hub must delay
//! publishing and retry the same entry.
//!
//! Format: same segments (`json/1`, `rec_type = 4`) under
//! `<root>/_mapping/<stream_epoch>/<seg_no:06>.lj`, header producer `_mapping`, producer epoch =
//! stream epoch. Index key = `seq_first`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use loom_proto::num::U64;
use serde::{Deserialize, Serialize};

use crate::error::JournalError;
use crate::format::{Codec, RecType};
use crate::reader::{Decision, Locate, Position, ScanLimits, scan_raw};
use crate::segment::{CorruptionReport, producer_dir};
use crate::writer::{
    AppendError, Entry, JournalConfig, JournalHandle, JournalStatus, JournalWriter, LossPolicy,
};

/// Directory name (and header producer id) of the mapping log.
pub const MAPPING_PRODUCER: &str = "_mapping";
/// Longest time `append_mapping` blocks before returning backpressure.
pub const MAPPING_BLOCK: Duration = Duration::from_millis(50);
/// Mapping queue byte cap (plan Q18).
pub const MAPPING_QUEUE_BYTES: u64 = 16 * 1024 * 1024;

/// One hub batch: stream range ↔ source range of one producer epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingEntry {
    /// Stream epoch.
    pub stream_epoch: U64,
    /// First stream seq (inclusive).
    pub seq_first: U64,
    /// Last stream seq (inclusive).
    pub seq_last: U64,
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: U64,
    /// First source seq (inclusive).
    pub src_first: U64,
    /// Last source seq (inclusive).
    pub src_last: U64,
    /// First mono ns.
    pub mono_first: U64,
    /// Last mono ns.
    pub mono_last: U64,
}

/// Why `append_mapping` did not queue the entry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    /// Still no room after [`MAPPING_BLOCK`] (writer slow or disk failing): delay publishing
    /// and retry the same entry. Nothing was dropped.
    #[error("mapping log backpressure")]
    Backpressure,
    /// Entry rejected (bad ranges, wrong epoch, out of order …).
    #[error("invalid mapping entry: {0}")]
    Invalid(String),
    /// Writer closed.
    #[error("mapping log closed")]
    Closed,
}

impl JournalConfig {
    /// Mapping-log configuration for `stream_epoch` (json, rec_type 4, never drops, 16 MiB).
    pub fn mapping(root: impl Into<PathBuf>, stream_epoch: u64) -> Self {
        let mut c = JournalConfig::new(root, MAPPING_PRODUCER, stream_epoch);
        c.queue_bytes = MAPPING_QUEUE_BYTES;
        c.rec_type = RecType::Mapping;
        c.loss = LossPolicy::Retain;
        c.codec = Codec::Json;
        c
    }
}

/// Mapping log writer (gateway).
#[derive(Debug)]
pub struct MappingLog {
    writer: JournalWriter,
    handle: JournalHandle,
    stream_epoch: u64,
    block: Duration,
}

impl MappingLog {
    /// Start the writer. `cfg` should come from [`JournalConfig::mapping`]; producer id, codec,
    /// record type and loss policy are forced to the mapping values.
    pub fn spawn(mut cfg: JournalConfig) -> Result<Self, JournalError> {
        cfg.producer_id = MAPPING_PRODUCER.to_string();
        cfg.codec = Codec::Json;
        cfg.rec_type = RecType::Mapping;
        cfg.loss = LossPolicy::Retain;
        let stream_epoch = cfg.producer_epoch;
        let writer = JournalWriter::spawn(cfg)?;
        let handle = writer.handle();
        Ok(MappingLog {
            writer,
            handle,
            stream_epoch,
            block: MAPPING_BLOCK,
        })
    }

    /// Queue one entry; blocks ≤ 50 ms when full, then [`MappingError::Backpressure`].
    /// `seq_first` must be greater than the previous entry's.
    pub fn append_mapping(&self, m: &MappingEntry) -> Result<(), MappingError> {
        if m.stream_epoch.0 != self.stream_epoch {
            return Err(MappingError::Invalid(format!(
                "stream_epoch {} != {}",
                m.stream_epoch, self.stream_epoch
            )));
        }
        if m.seq_first > m.seq_last || m.src_first > m.src_last || m.mono_first > m.mono_last {
            return Err(MappingError::Invalid("first > last".into()));
        }
        let payload = serde_json::to_vec(m).map_err(|e| MappingError::Invalid(e.to_string()))?;
        let e = Entry {
            rec_type: RecType::Mapping,
            seq: m.seq_first.0,
            mono: m.mono_first.0,
            payload,
        };
        match self.handle.push_wait(e, self.block) {
            Ok(true) => Ok(()),
            Ok(false) => Err(MappingError::Backpressure),
            Err(AppendError::Closed) => Err(MappingError::Closed),
            Err(e) => Err(MappingError::Invalid(e.to_string())),
        }
    }

    /// Status; `durable_through` is the highest durable `seq_first` (cursors above it are
    /// `uncertain`).
    pub fn status(&self) -> JournalStatus {
        self.handle.status()
    }

    /// Write and sync everything queued.
    pub fn flush(&self, timeout: Duration) -> Result<JournalStatus, JournalError> {
        self.writer.flush(timeout)
    }

    /// Directory of this mapping log.
    pub fn dir(&self) -> &Path {
        self.writer.dir()
    }

    /// Close (drain, seal, join).
    pub fn close(self) -> crate::writer::CloseReport {
        self.writer.close()
    }
}

/// Mapping lookup result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MappingLookup {
    /// Entries overlapping the requested stream range, in order.
    pub entries: Vec<MappingEntry>,
    /// A limit stopped the lookup.
    pub truncated: bool,
    /// Where to continue.
    pub resume_from: Option<Position>,
    /// Damaged sealed mapping segments met.
    pub corruptions: Vec<CorruptionReport>,
}

/// Entries whose stream range overlaps `[seq_from, seq_to]` (stream seq → source ref ranges).
pub fn lookup_mappings(
    root: &Path,
    stream_epoch: u64,
    seq_from: u64,
    seq_to: u64,
    limits: &ScanLimits,
    resume: Option<&Position>,
) -> Result<MappingLookup, JournalError> {
    let dir = producer_dir(root, MAPPING_PRODUCER, stream_epoch);
    let raw = scan_raw(&dir, Locate::Key(seq_from), resume, limits, |item| {
        if item.rec_type != RecType::Mapping {
            return Decision::Skip;
        }
        match item.key {
            Some(first) if first > seq_to => Decision::Stop,
            Some(_) => Decision::Take,
            None => Decision::Skip,
        }
    })?;
    let mut entries: Vec<MappingEntry> = Vec::new();
    for (_, item) in raw.items {
        let Ok(e) = serde_json::from_slice::<MappingEntry>(&item.payload) else {
            continue;
        };
        if e.seq_last.0 < seq_from || entries.last().is_some_and(|l| l.seq_first == e.seq_first) {
            continue;
        }
        entries.push(e);
    }
    Ok(MappingLookup {
        entries,
        truncated: raw.truncated,
        resume_from: raw.resume_from,
        corruptions: raw.corruptions,
    })
}

/// The entry containing stream `seq`, if any.
pub fn lookup_seq(root: &Path, stream_epoch: u64, seq: u64) -> Result<Option<MappingEntry>, JournalError> {
    let r = lookup_mappings(root, stream_epoch, seq, seq, &ScanLimits::rpc_default(), None)?;
    Ok(r.entries
        .into_iter()
        .find(|e| e.seq_first.0 <= seq && seq <= e.seq_last.0))
}
