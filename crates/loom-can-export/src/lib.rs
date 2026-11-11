//! Streaming export of canonical CAN records. Standard files are derivatives;
//! the JSONL sidecar retains provenance and exact clock facts.
//!
//! A complete primary file is published only after finish, sidecar completion
//! and fsync. See README for bounds, timestamp anchors and partial-file recovery.
#![forbid(unsafe_code)]

mod files;
mod stream;

pub use files::{ExportFailure, ExportPaths, export_file, export_frames};
use loom_can_frame::{CanFrame, DeviceTimestamp, Direction};
use serde::Serialize;
use std::{fmt, io};
pub use stream::{Exporter, bounded_blf_reader};

/// Maximum accepted BLF header when independently checking an export.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Maximum accepted compressed or decompressed BLF container.
pub const MAX_CONTAINER_BYTES: usize = 8 * 1024 * 1024;
/// Maximum accepted BLF object.
pub const MAX_OBJECT_BYTES: usize = 1024 * 1024;
/// Upper bound reserved for the writer's growable object staging buffer.
pub const WRITER_BUFFER_BYTES: usize = 256 * 1024;
/// Half-limit flushing accounts for Vec growth in the upstream streaming writer.
pub const CONTAINER_FLUSH_BYTES: usize = WRITER_BUFFER_BYTES / 2;
/// Bounded one-row provenance serialization buffer.
pub const MAX_SIDECAR_LINE: usize = 4096;
/// Bounded one-line ASC serialization buffer (including CAN FD64 bytes).
pub const MAX_ASC_LINE: usize = 512;
/// Request/producer labels are bounded before serialization.
pub const MAX_LABEL_BYTES: usize = 128;

/// Output format. ASC uses integer nanosecond text, not a whole-file string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Blf,
    Asc,
}
/// Server-established origin, preserved in the sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Agent,
    User,
    Device,
    Script,
}
/// Quality of the supplied host-ingest timestamp; device clocks are never promoted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeQuality {
    #[default]
    Exact,
    Estimated,
    Unknown,
}
/// Original identity in the canonical domain journal, not a regenerated stream seq.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub producer_id: String,
    pub producer_epoch: u64,
    pub source_seq: u64,
}
/// Nonstandard metadata; bounded and emitted even when values are unknown.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub request_id: Option<String>,
    pub source: Option<SourceRef>,
    pub time_quality: TimeQuality,
}
impl Metadata {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        for value in [
            self.request_id.as_deref(),
            self.source.as_ref().map(|s| s.producer_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if value.is_empty() || value.len() > MAX_LABEL_BYTES {
                return Err(Error::Invalid("metadata label length"));
            }
        }
        Ok(())
    }
}
/// A frame with host time and optional independent wall/device clock facts.
/// channel_index is zero-based; wire channel numbers are one-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFrame {
    pub ts_mono_ns: u64,
    pub wall_ns: Option<u64>,
    pub channel_index: u16,
    pub dir: Direction,
    pub frame: CanFrame,
    pub origin: Origin,
    pub device_ts: Option<DeviceTimestamp>,
    pub metadata: Metadata,
}
impl ExportFrame {
    /// Frame with no wall/device anchor or provenance label and exact caller-supplied ingest time.
    pub fn new(ts_mono_ns: u64, channel_index: u16, dir: Direction, frame: CanFrame, origin: Origin) -> Self {
        Self {
            ts_mono_ns,
            wall_ns: None,
            channel_index,
            dir,
            frame,
            origin,
            device_ts: None,
            metadata: Metadata::default(),
        }
    }
}
/// A generic CAN error observation. Direction/description stay exact in the sidecar;
/// basic BLF/ASC ErrorFrame objects cannot express all driver-specific error facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorFrame {
    pub ts_mono_ns: u64,
    pub wall_ns: Option<u64>,
    pub channel_index: u16,
    pub dir: Direction,
    pub origin: Origin,
    pub description: String,
    pub metadata: Metadata,
}
/// A loss marker. count=None means unknown, never zero; scope distinguishes loss layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapScope {
    Source,
    Record,
    Display,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub ts_mono_ns: u64,
    pub scope: GapScope,
    pub count: Option<u64>,
    pub reason: String,
    pub metadata: Metadata,
}
/// Streaming input; a gap does not manufacture a fake CAN frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportRecord {
    Frame(ExportFrame),
    Error(ErrorFrame),
    Gap(Gap),
}
impl From<ExportFrame> for ExportRecord {
    fn from(frame: ExportFrame) -> Self {
        Self::Frame(frame)
    }
}
impl ExportRecord {
    pub(crate) fn time(&self) -> u64 {
        match self {
            Self::Frame(f) => f.ts_mono_ns,
            Self::Error(e) => e.ts_mono_ns,
            Self::Gap(g) => g.ts_mono_ns,
        }
    }
    pub(crate) fn metadata(&self) -> &Metadata {
        match self {
            Self::Frame(f) => &f.metadata,
            Self::Error(e) => &e.metadata,
            Self::Gap(g) => &g.metadata,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.metadata().validate()?;
        match self {
            Self::Frame(f) if f.channel_index == u16::MAX => {
                Err(Error::Invalid("channel_index cannot map to u16 wire channel"))
            }
            Self::Error(e) if e.channel_index == u16::MAX || e.description.len() > 256 => {
                Err(Error::Invalid("error channel/description"))
            }
            Self::Gap(g) if g.reason.len() > 256 || g.count == Some(0) => {
                Err(Error::Invalid("gap reason/count"))
            }
            _ => Ok(()),
        }
    }
}
/// Fixed anchors; callers can obtain them from the sealed journal before export.
/// Unknown wall time writes an explicit epoch placeholder, labelled unknown in sidecar.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub anchor_mono_ns: u64,
    pub anchor_wall_ns: Option<u64>,
    pub compressed: bool,
}
/// Bounded summary; per-record provenance is on disk rather than collected here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub records: u64,
    pub frames: u64,
    pub errors: u64,
    pub gaps: u64,
    pub objects: u64,
    pub last_mono_ns: Option<u64>,
    pub first_source: Option<SourceRef>,
    pub last_source: Option<SourceRef>,
    pub max_sidecar_line: usize,
    pub max_asc_line: usize,
}
/// Errors are explicit; a filesystem wrapper also returns the retained partial paths.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Blf(autors_blf::Error),
    Json(serde_json::Error),
    Invalid(&'static str),
    OutOfOrder,
    TimestampOverflow,
    CountOverflow,
    RowLimit,
    AlreadyExists,
    Input(&'static str),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO: {e}"),
            Self::Blf(e) => write!(f, "BLF: {e}"),
            Self::Json(e) => write!(f, "JSON: {e}"),
            other => write!(f, "{other:?}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<autors_blf::Error> for Error {
    fn from(e: autors_blf::Error) -> Self {
        Self::Blf(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
