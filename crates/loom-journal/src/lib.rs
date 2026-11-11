//! `loom-journal` — Loom's recording format and its writer/reader (contract `06-journal.md`).
//!
//! * [`writer`] — [`JournalWriter`] runs one dedicated OS thread per journal (inside the worker
//!   process for bulk data, inside the gateway for producer `gateway`). Non-blocking
//!   [`JournalHandle::try_append`], bounded queue, written/durable watermarks, rotation,
//!   disk-error retry with exact [`GapReport`]s.
//! * [`segment`] — on-disk layout, crash-tail recovery, sealed-segment verification and
//!   quarantine, sparse index rebuild.
//! * [`reader`] — bounded [`scan`](reader::scan) by source seq or mono time with resume.
//! * [`mapping`] — the gateway's stream mapping log (never dropped; backpressure instead).
//! * [`retain`] — retention GC over sealed segments with pins and a deletion manifest.
//!
//! Layout: `<root>/<producer_id>/<producer_epoch>/<seg_no:06>.lj` (sealed),
//! `….lj.open` (active), `<seg_no:06>.idx` (sparse index). zstd sealing is M2.
//!
//! No async: the journal is plain `std` I/O on its own thread so it never stalls a runtime.

pub mod error;
pub mod format;
pub mod gap;
pub mod index;
pub mod mapping;
pub mod queue;
pub mod reader;
pub mod retain;
pub mod segment;
pub mod writer;

pub use error::JournalError;
pub use format::{Codec, RecType};
pub use gap::GapReport;
pub use index::{IndexEntry, IndexPolicy};
pub use mapping::{MappingEntry, MappingError, MappingLog, lookup_mappings, lookup_seq};
pub use queue::QueueBudget;
pub use reader::{Position, ScanLimits, ScanQuery, ScanRange, ScanResult, ScannedRecord, scan};
pub use retain::{QuotaLevel, RetainPolicy, RetainReport, SegmentId, retain};
pub use segment::{
    CorruptionReport, RecoveryReport, VerifyReport, producer_dir, rebuild_index, recover_producer_dir,
    recover_root, verify_producer_dir, verify_segment,
};
pub use writer::{
    AppendError, CloseReport, FaultHook, FaultPoint, JournalConfig, JournalHandle, JournalStatus,
    JournalWriter,
};
