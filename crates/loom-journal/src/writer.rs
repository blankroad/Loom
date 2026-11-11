//! Journal writer: one dedicated OS thread per journal (contract 06 §6.4, plan §6.2 / Q2).
//!
//! * Input: a bounded queue (default 8 MiB / 32k items per channel, optional shared
//!   [`QueueBudget`] for the worker total). [`JournalHandle::try_append`] never blocks; when the
//!   queue is full it returns [`AppendError::Full`], the `source_seq` is consumed, the loss is
//!   recorded as a `journal_queue_full` [`GapReport`] and health is `failed` (plan Q2).
//! * OS write: buffered entries are written when 64 KiB is buffered or the oldest buffered entry
//!   is 10 ms old. `sync_data` (macOS: `F_FULLFSYNC`) runs at most every 100 ms while there is
//!   unsynced data, and at seal. Watermarks `written_through` / `durable_through` (source seq).
//! * Rotation at 64 MiB or 60 s. Seal = fsync → rename `.lj.open` → `.lj` → index fsync → dir
//!   fsync. (zstd compression is M2.)
//! * Disk error: health `failed` with the error text, buffered and newly arriving records are
//!   dropped and counted (the queue keeps draining, so memory stays bounded), a new segment is
//!   tried every 5 s, and on success a `disk_error`/`disk_full` [`GapReport`] with the exact
//!   lost `source_seq` range is published and also written into the journal as a marker frame.
//!   A failed `sync_data` gives a `sync_failed` gap with count `unknown` for the records written
//!   but not yet durable.
//! * Mapping mode ([`crate::mapping`]): same thread, but nothing is dropped — on disk error the
//!   writer stops draining, the queue fills and `append_mapping` returns backpressure.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use loom_proto::envelope::RecordHealth;
use loom_proto::event::{GapCount, SourceRecord};
use loom_proto::num::U64;
use loom_proto::worker::{JournalHealth, QueueStat};
use tracing::{info, warn};

use crate::error::JournalError;
use crate::format::{
    Codec, HINT_FRAME_LEN, IndexHint, MAX_FRAME_PAYLOAD, MAX_HEADER_TEXT, RecType, SegmentHeader,
    encode_frame, frame_len,
};
use crate::gap::{GapMarker, GapReport, causes};
use crate::index::{IndexEntry, IndexPolicy, IndexState, create_index_file};
use crate::queue::{BoundedQueue, PushError, QueueBudget};
use crate::segment::{
    index_path, list_segments, producer_dir, rebuild_index, recover_producer_dir, segment_file_name,
    sync_dir, validate_component, wall_ns_now,
};

/// Per-channel queue byte cap (plan Q2).
pub const DEFAULT_QUEUE_BYTES: u64 = 8 * 1024 * 1024;
/// Per-channel queue item cap (plan Q2).
pub const DEFAULT_QUEUE_ITEMS: usize = 32 * 1024;
/// 채널(class)별 상한. worker 전체 상한과 별도로 집행한다(Q2).
pub const DEFAULT_CLASS_QUEUE_BYTES: u64 = 8 * 1024 * 1024;
/// 계정 이름과 pending gap 메타데이터도 유한하게 제한한다.
pub const MAX_CLASS_NAME_BYTES: usize = 256;
/// OS write when the oldest buffered record is this old.
pub const DEFAULT_WRITE_INTERVAL: Duration = Duration::from_millis(10);
/// OS write when this many payload bytes are buffered.
pub const DEFAULT_WRITE_BYTES: usize = 64 * 1024;
/// Group `sync_data` interval.
pub const DEFAULT_SYNC_INTERVAL: Duration = Duration::from_millis(100);
/// Rotate at this segment size.
pub const DEFAULT_ROTATE_BYTES: u64 = 64 * 1024 * 1024;
/// Rotate at this segment age.
pub const DEFAULT_ROTATE_AGE: Duration = Duration::from_secs(60);
/// Retry interval after a disk error.
pub const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// Accounting overhead per queued entry (on top of the payload length).
pub const ENTRY_OVERHEAD_BYTES: u64 = 48;
/// Cap of undelivered [`GapReport`]s; when full, new gaps are merged into the last one
/// (cause becomes `multiple` if causes differ). Nothing is silently forgotten.
pub const MAX_PENDING_GAPS: usize = 256;
/// Cap of concurrent [`JournalWriter::flush`] callers; more get [`JournalError::Timeout`].
pub const MAX_FLUSH_WAITERS: usize = 16;
const POP_BATCH: usize = 1024;
const MAX_IDLE_WAIT: Duration = Duration::from_millis(100);

/// Where a fault hook is consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// Before creating a segment file.
    Open,
    /// Before every `write` (header and data).
    Write,
    /// Before every `sync_data`.
    Sync,
}

/// Test hook: return `Err` to inject an I/O error at a [`FaultPoint`]. The hook may also block
/// (e.g. to stall the writer and fill the queue).
pub type FaultHook = Arc<dyn Fn(FaultPoint) -> io::Result<()> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LossPolicy {
    /// Data journal: on disk error drop and count (queue keeps draining).
    Drop,
    /// Mapping log: never drop; stop draining so producers see backpressure.
    Retain,
}

/// Writer configuration. Build with [`JournalConfig::new`] and adjust public fields.
#[derive(Clone)]
pub struct JournalConfig {
    /// Session root (`…/records/<date>/<daemon_session>`).
    pub root: PathBuf,
    /// Producer id (path component).
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// Monotonic clock domain id written into segment headers.
    pub clock_id: String,
    /// Record codec.
    pub codec: Codec,
    /// Queue byte cap.
    pub queue_bytes: u64,
    /// Queue item cap.
    pub queue_items: usize,
    /// 채널별 큐 바이트 상한. 채널 없는 제어 레코드는 전체 상한만 적용한다.
    pub class_queue_bytes: u64,
    /// Optional budget shared with other writers of the same process.
    pub budget: Option<Arc<QueueBudget>>,
    /// See [`DEFAULT_WRITE_INTERVAL`].
    pub write_interval: Duration,
    /// See [`DEFAULT_WRITE_BYTES`].
    pub write_bytes: usize,
    /// See [`DEFAULT_SYNC_INTERVAL`].
    pub sync_interval: Duration,
    /// See [`DEFAULT_ROTATE_BYTES`].
    pub rotate_bytes: u64,
    /// See [`DEFAULT_ROTATE_AGE`].
    pub rotate_age: Duration,
    /// Sparse index policy.
    pub index: IndexPolicy,
    /// See [`DEFAULT_RETRY_INTERVAL`].
    pub retry_interval: Duration,
    /// Fault injection (tests).
    pub fault: Option<FaultHook>,
    pub(crate) rec_type: RecType,
    pub(crate) loss: LossPolicy,
}

impl std::fmt::Debug for JournalConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalConfig")
            .field("root", &self.root)
            .field("producer_id", &self.producer_id)
            .field("producer_epoch", &self.producer_epoch)
            .field("clock_id", &self.clock_id)
            .field("codec", &self.codec)
            .field("queue_bytes", &self.queue_bytes)
            .field("queue_items", &self.queue_items)
            .field("class_queue_bytes", &self.class_queue_bytes)
            .field("budget", &self.budget)
            .field("write_interval", &self.write_interval)
            .field("write_bytes", &self.write_bytes)
            .field("sync_interval", &self.sync_interval)
            .field("rotate_bytes", &self.rotate_bytes)
            .field("rotate_age", &self.rotate_age)
            .field("index", &self.index)
            .field("retry_interval", &self.retry_interval)
            .field("fault", &self.fault.is_some())
            .finish()
    }
}

impl JournalConfig {
    /// Defaults from the contract: json codec, 8 MiB/32k queue, 10 ms/64 KiB write, 100 ms
    /// sync, 64 MiB/60 s rotation, 4096/1 s index, 5 s retry.
    pub fn new(root: impl Into<PathBuf>, producer_id: impl Into<String>, producer_epoch: u64) -> Self {
        JournalConfig {
            root: root.into(),
            producer_id: producer_id.into(),
            producer_epoch,
            clock_id: "unknown".into(),
            codec: Codec::Json,
            queue_bytes: DEFAULT_QUEUE_BYTES,
            queue_items: DEFAULT_QUEUE_ITEMS,
            class_queue_bytes: DEFAULT_CLASS_QUEUE_BYTES,
            budget: None,
            write_interval: DEFAULT_WRITE_INTERVAL,
            write_bytes: DEFAULT_WRITE_BYTES,
            sync_interval: DEFAULT_SYNC_INTERVAL,
            rotate_bytes: DEFAULT_ROTATE_BYTES,
            rotate_age: DEFAULT_ROTATE_AGE,
            index: IndexPolicy::default(),
            retry_interval: DEFAULT_RETRY_INTERVAL,
            fault: None,
            rec_type: RecType::Event,
            loss: LossPolicy::Drop,
        }
    }

    /// Directory this writer writes to.
    pub fn dir(&self) -> PathBuf {
        producer_dir(&self.root, &self.producer_id, self.producer_epoch)
    }

    fn validate(&self) -> Result<(), JournalError> {
        validate_component(&self.producer_id)?;
        if self.clock_id.len() > MAX_HEADER_TEXT {
            return Err(JournalError::Config("clock_id too long".into()));
        }
        if self.queue_items == 0
            || self.queue_bytes == 0
            || self.class_queue_bytes == 0
            || self.write_bytes == 0
        {
            return Err(JournalError::Config("queue/write caps must be > 0".into()));
        }
        if self.rotate_bytes < 4096 || self.index.every_records == 0 {
            return Err(JournalError::Config(
                "rotate_bytes >= 4096 and index.every_records > 0".into(),
            ));
        }
        Ok(())
    }
}

/// Why `try_append` refused a record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppendError {
    /// Queue full: the record is dropped, `source_seq` is consumed, health is `failed` and a
    /// `journal_queue_full` gap is pending. The caller emits `record.gap` (plan Q2).
    #[error("journal queue full (source_seq {source_seq})")]
    Full {
        /// The dropped record's seq.
        source_seq: u64,
    },
    /// `source_seq` must strictly increase per writer.
    #[error("source_seq {source_seq} not after {last}")]
    OutOfOrder {
        /// Rejected seq.
        source_seq: u64,
        /// Last accepted (or dropped) seq.
        last: u64,
    },
    /// Record's producer id/epoch differ from the writer's.
    #[error("record producer does not match writer")]
    WrongProducer,
    /// `try_append(SourceRecord)` on an opaque-codec or mapping writer.
    #[error("codec or record type mismatch")]
    CodecMismatch,
    /// Payload above [`MAX_FRAME_PAYLOAD`].
    #[error("payload too large ({0} bytes)")]
    TooLarge(usize),
    /// JSON encoding failed.
    #[error("encode: {0}")]
    Encode(String),
    /// Writer closed.
    #[error("journal closed")]
    Closed,
}

/// Snapshot of a writer's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalStatus {
    /// `failed` while a disk error lasts or a loss is not yet taken with
    /// [`JournalHandle::take_gaps`]; `degraded` when the queue is ≥ 50 % full.
    pub health: RecordHealth,
    /// Highest `source_seq` handed to the OS.
    pub written_through: Option<u64>,
    /// Highest `source_seq` synced to disk.
    pub durable_through: Option<u64>,
    /// Queue depth (items).
    pub queue_depth: usize,
    /// Queue bytes.
    pub queue_bytes: u64,
    /// Queue high-water depth.
    pub queue_max_depth: usize,
    /// Records refused with `Full` so far.
    pub drops: u64,
    /// Last disk error text while failed.
    pub last_error: Option<String>,
    /// Gaps waiting for `take_gaps`.
    pub pending_gaps: usize,
}

impl JournalStatus {
    /// Contract form for worker `Health.journal`.
    pub fn to_proto(&self) -> JournalHealth {
        JournalHealth {
            written_through: self.written_through.map(U64),
            durable_through: self.durable_through.map(U64),
            health: self.health,
        }
    }

    /// Queue statistics for worker `Health.queues`.
    pub fn queue_stat(&self) -> QueueStat {
        QueueStat {
            depth: u32::try_from(self.queue_depth).unwrap_or(u32::MAX),
            bytes: U64(self.queue_bytes),
            max_depth: u32::try_from(self.queue_max_depth).unwrap_or(u32::MAX),
            drops: U64(self.drops),
        }
    }
}

/// Final report of [`JournalWriter::close`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseReport {
    /// Status after the last flush/seal.
    pub status: JournalStatus,
    /// All gaps not yet taken.
    pub gaps: Vec<GapReport>,
}

pub(crate) struct Entry {
    pub rec_type: RecType,
    pub seq: u64,
    pub mono: u64,
    pub payload: Vec<u8>,
}

impl Entry {
    fn cost(&self) -> u64 {
        self.payload.len() as u64 + ENTRY_OVERHEAD_BYTES
    }
}

#[derive(Debug, Clone)]
struct LossAcc {
    from: u64,
    to: u64,
    count: u64,
    bytes: u64,
}

impl LossAcc {
    fn new(seq: u64, bytes: u64) -> Self {
        LossAcc {
            from: seq,
            to: seq,
            count: 1,
            bytes,
        }
    }

    fn add(&mut self, seq: u64, bytes: u64) {
        self.from = self.from.min(seq);
        self.to = self.to.max(seq);
        self.count += 1;
        self.bytes += bytes;
    }

    fn report(&self, shared: &Shared, cause: &str) -> GapReport {
        GapReport {
            producer_id: shared.producer_id.clone(),
            producer_epoch: shared.producer_epoch,
            from: self.from,
            to: Some(self.to),
            count: GapCount::Exact(U64(self.count)),
            cause: cause.to_string(),
            bytes: Some(self.bytes),
            class: None,
        }
    }
}

fn add_loss(acc: &mut Option<LossAcc>, e: &Entry) {
    if !e.rec_type.is_keyed() {
        return;
    }
    match acc {
        Some(a) => a.add(e.seq, e.payload.len() as u64),
        None => *acc = Some(LossAcc::new(e.seq, e.payload.len() as u64)),
    }
}

#[derive(Default)]
struct GapState {
    done: Vec<GapReport>,
    overflow: BTreeMap<Option<String>, LossAcc>,
}

impl GapState {
    fn push(&mut self, g: GapReport) {
        if self.done.len() >= MAX_PENDING_GAPS
            && let Some(last) = self.done.last_mut()
        {
            last.absorb(&g);
        } else {
            self.done.push(g);
        }
    }
}

type Waiter = mpsc::SyncSender<Result<(), String>>;

pub(crate) struct Shared {
    producer_id: String,
    producer_epoch: u64,
    codec: Codec,
    rec_type: RecType,
    queue: BoundedQueue<Entry>,
    order: Mutex<Option<u64>>,
    written: AtomicU64,
    durable: AtomicU64,
    disk_failed: AtomicBool,
    last_error: Mutex<Option<String>>,
    gaps: Mutex<GapState>,
    drops: AtomicU64,
    waiters: Mutex<Vec<Waiter>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Watermarks are stored as `seq + 1` (0 = none).
fn wm_set(a: &AtomicU64, seq: u64) {
    a.fetch_max(seq.saturating_add(1), Ordering::AcqRel);
}

fn wm_get(a: &AtomicU64) -> Option<u64> {
    a.load(Ordering::Acquire).checked_sub(1)
}

impl Shared {
    fn push_gap(&self, g: GapReport) {
        lock(&self.gaps).push(g);
    }

    fn set_error(&self, msg: Option<String>) {
        *lock(&self.last_error) = msg;
    }
}

/// Cloneable append/status side of a journal writer (any thread).
#[derive(Clone)]
pub struct JournalHandle {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for JournalHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalHandle")
            .field("producer_id", &self.shared.producer_id)
            .field("producer_epoch", &self.shared.producer_epoch)
            .finish()
    }
}

impl JournalHandle {
    /// Queue a `SourceRecord` (json codec). Never blocks. The JSON is encoded on the calling
    /// thread so the queue's byte accounting is exact.
    pub fn try_append(&self, rec: &SourceRecord) -> Result<(), AppendError> {
        self.append_record(rec, rec.channel.as_deref())
    }

    /// 기존 producer seq를 유지하면서 지정 채널 계정의 상한을 추가로 집행한다.
    pub fn try_append_class(&self, class: &str, rec: &SourceRecord) -> Result<(), AppendError> {
        self.append_record(rec, Some(class))
    }

    fn append_record(&self, rec: &SourceRecord, class: Option<&str>) -> Result<(), AppendError> {
        if class.is_some_and(|c| c.len() > MAX_CLASS_NAME_BYTES) {
            return Err(AppendError::Encode(format!(
                "journal class exceeds {MAX_CLASS_NAME_BYTES} bytes"
            )));
        }
        if self.shared.codec != Codec::Json || self.shared.rec_type != RecType::Event {
            return Err(AppendError::CodecMismatch);
        }
        if rec.producer_id != self.shared.producer_id || rec.producer_epoch.0 != self.shared.producer_epoch {
            return Err(AppendError::WrongProducer);
        }
        let payload = serde_json::to_vec(rec).map_err(|e| AppendError::Encode(e.to_string()))?;
        self.push(
            Entry {
                rec_type: RecType::Event,
                seq: rec.source_seq.0,
                mono: rec.ts.mono_ns.0,
                payload,
            },
            class,
        )
    }

    /// Queue already-encoded record bytes (any codec; the crate does not parse them). For the
    /// json codec the bytes must be the `SourceRecord` JSON whose `source_seq`/`ts.mono_ns`
    /// equal the arguments (index rebuild reads them back from the JSON).
    pub fn try_append_raw(&self, source_seq: u64, mono_ns: u64, payload: Vec<u8>) -> Result<(), AppendError> {
        self.append_raw(source_seq, mono_ns, payload, None)
    }

    /// opaque codec도 호출자가 채널 계정을 명시할 수 있다.
    pub fn try_append_raw_class(
        &self,
        class: &str,
        source_seq: u64,
        mono_ns: u64,
        payload: Vec<u8>,
    ) -> Result<(), AppendError> {
        if class.len() > MAX_CLASS_NAME_BYTES {
            return Err(AppendError::Encode(format!(
                "journal class exceeds {MAX_CLASS_NAME_BYTES} bytes"
            )));
        }
        self.append_raw(source_seq, mono_ns, payload, Some(class))
    }

    fn append_raw(
        &self,
        source_seq: u64,
        mono_ns: u64,
        payload: Vec<u8>,
        class: Option<&str>,
    ) -> Result<(), AppendError> {
        if self.shared.rec_type != RecType::Event {
            return Err(AppendError::CodecMismatch);
        }
        self.push(
            Entry {
                rec_type: RecType::Event,
                seq: source_seq,
                mono: mono_ns,
                payload,
            },
            class,
        )
    }

    fn push(&self, e: Entry, class: Option<&str>) -> Result<(), AppendError> {
        if e.payload.len() > MAX_FRAME_PAYLOAD {
            return Err(AppendError::TooLarge(e.payload.len()));
        }
        let mut order = lock(&self.shared.order);
        if let Some(last) = *order
            && e.seq <= last
        {
            return Err(AppendError::OutOfOrder {
                source_seq: e.seq,
                last,
            });
        }
        let seq = e.seq;
        // 계정 이름의 복제/인덱스 메타데이터도 보수적으로 큐 비용에 포함한다.
        let cost = e.cost() + class.map_or(0, |c| 2 * c.len() as u64);
        let len = e.payload.len() as u64;
        match self.shared.queue.try_push_class(e, cost, class) {
            Ok(()) => {
                *order = Some(seq);
                let closed = std::mem::take(&mut lock(&self.shared.gaps).overflow);
                for (class, acc) in closed {
                    let mut g = acc.report(&self.shared, causes::QUEUE_FULL);
                    g.class = class;
                    if let Ok(marker) = serde_json::to_vec(&GapMarker {
                        marker: "gap",
                        gap: &g,
                    }) {
                        let m = Entry {
                            rec_type: RecType::Marker,
                            seq: 0,
                            mono: 0,
                            payload: marker,
                        };
                        let c = m.cost();
                        // Best effort: if the marker does not fit, the gap is still reported.
                        let _ = self.shared.queue.try_push(m, c);
                    }
                    self.shared.push_gap(g);
                }
                Ok(())
            }
            Err(PushError::Full(_)) => {
                *order = Some(seq);
                self.shared.drops.fetch_add(1, Ordering::Relaxed);
                let mut g = lock(&self.shared.gaps);
                let key = class.map(str::to_owned);
                if !g.overflow.contains_key(&key) && g.overflow.len() >= MAX_PENDING_GAPS {
                    // pending 상태도 유한하다. 채널을 거짓으로 합치지 않고 완료 목록으로 이동.
                    let closed = std::mem::take(&mut g.overflow);
                    for (class, acc) in closed {
                        let mut report = acc.report(&self.shared, causes::QUEUE_FULL);
                        report.class = class;
                        g.push(report);
                    }
                }
                g.overflow
                    .entry(key)
                    .and_modify(|a| a.add(seq, len))
                    .or_insert_with(|| LossAcc::new(seq, len));
                Err(AppendError::Full { source_seq: seq })
            }
            Err(PushError::Closed(_)) => Err(AppendError::Closed),
        }
    }

    /// Blocking push for the mapping log: waits up to `timeout`, never drops. `Ok(false)` =
    /// still full after the wait (caller slows down and retries).
    pub(crate) fn push_wait(&self, e: Entry, timeout: Duration) -> Result<bool, AppendError> {
        if e.payload.len() > MAX_FRAME_PAYLOAD {
            return Err(AppendError::TooLarge(e.payload.len()));
        }
        let mut order = lock(&self.shared.order);
        if let Some(last) = *order
            && e.seq <= last
        {
            return Err(AppendError::OutOfOrder {
                source_seq: e.seq,
                last,
            });
        }
        let seq = e.seq;
        let cost = e.cost();
        match self.shared.queue.push_timeout(e, cost, timeout) {
            Ok(()) => {
                *order = Some(seq);
                Ok(true)
            }
            Err(PushError::Full(_)) => Ok(false),
            Err(PushError::Closed(_)) => Err(AppendError::Closed),
        }
    }

    /// Current status.
    pub fn status(&self) -> JournalStatus {
        let s = &self.shared;
        let q = s.queue.stats();
        let (loss, pending_gaps) = {
            let g = lock(&s.gaps);
            (!g.done.is_empty() || !g.overflow.is_empty(), g.done.len())
        };
        let failed = s.disk_failed.load(Ordering::Acquire) || loss;
        let health = if failed {
            RecordHealth::Failed
        } else if q.depth * 2 >= q.max_items || q.bytes * 2 >= q.max_bytes {
            RecordHealth::Degraded
        } else {
            RecordHealth::Ok
        };
        JournalStatus {
            health,
            written_through: wm_get(&s.written),
            durable_through: wm_get(&s.durable),
            queue_depth: q.depth,
            queue_bytes: q.bytes,
            queue_max_depth: q.max_depth,
            drops: s.drops.load(Ordering::Relaxed),
            last_error: lock(&s.last_error).clone(),
            pending_gaps,
        }
    }

    /// Shorthand for `status().health`.
    pub fn health(&self) -> RecordHealth {
        self.status().health
    }

    /// 지정 채널이 현재 입력 큐에 예약한 바이트(레코드·계정 메타데이터 포함).
    pub fn class_queue_bytes(&self, class: &str) -> u64 {
        self.shared.queue.class_used(class)
    }

    /// Highest seq handed to the OS.
    pub fn written_through(&self) -> Option<u64> {
        wm_get(&self.shared.written)
    }

    /// Highest seq synced to disk.
    pub fn durable_through(&self) -> Option<u64> {
        wm_get(&self.shared.durable)
    }

    /// Take finished gaps (crash tail at spawn, queue overflow after it ended, disk error after
    /// recovery). An overflow still in progress stays pending until the next accepted record.
    pub fn take_gaps(&self) -> Vec<GapReport> {
        std::mem::take(&mut lock(&self.shared.gaps).done)
    }
}

/// Owner of the writer thread. Dropping it closes the journal (drains, seals, joins).
pub struct JournalWriter {
    handle: JournalHandle,
    join: Option<JoinHandle<()>>,
    dir: PathBuf,
    recovered: Vec<crate::segment::RecoveryReport>,
}

impl std::fmt::Debug for JournalWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalWriter")
            .field("dir", &self.dir)
            .field("recovered", &self.recovered.len())
            .finish()
    }
}

impl JournalWriter {
    /// Recover leftover `.lj.open` segments of this producer epoch (crash tail), then start the
    /// writer thread. Disk errors at this point do not fail the spawn: the writer starts and
    /// reports `failed` on its first write (live data keeps flowing).
    pub fn spawn(cfg: JournalConfig) -> Result<Self, JournalError> {
        cfg.validate()?;
        let dir = cfg.dir();
        let shared = Arc::new(Shared {
            producer_id: cfg.producer_id.clone(),
            producer_epoch: cfg.producer_epoch,
            codec: cfg.codec,
            rec_type: cfg.rec_type,
            queue: BoundedQueue::new(cfg.queue_items, cfg.queue_bytes, cfg.budget.clone())
                .with_class_limit(cfg.class_queue_bytes),
            order: Mutex::new(None),
            written: AtomicU64::new(0),
            durable: AtomicU64::new(0),
            disk_failed: AtomicBool::new(false),
            last_error: Mutex::new(None),
            gaps: Mutex::new(GapState::default()),
            drops: AtomicU64::new(0),
            waiters: Mutex::new(Vec::new()),
        });
        let mut recovered = Vec::new();
        if dir.is_dir() {
            match recover_producer_dir(&dir, &cfg.index) {
                Ok(r) => recovered = r,
                Err(e) => {
                    warn!(error = %e, dir = %dir.display(), "journal recovery failed");
                    shared.set_error(Some(format!("recovery: {e}")));
                }
            }
        }
        for g in recovered.iter().filter_map(|r| r.gap.clone()) {
            shared.push_gap(g);
        }
        // Keep the seq order across a same-epoch restart.
        if let Some(last) = recovered.iter().filter_map(|r| r.last_seq).max() {
            *lock(&shared.order) = Some(last);
        }
        let next_seg_no = list_segments(&dir)
            .ok()
            .and_then(|s| s.last().map(|l| l.seg_no.saturating_add(1)))
            .unwrap_or(1);
        let worker = Worker {
            shared: shared.clone(),
            dir: dir.clone(),
            seg: None,
            next_seg_no,
            pending: VecDeque::new(),
            pending_since: None,
            pending_bytes: 0,
            last_sync: Instant::now(),
            failure: None,
            scratch: Vec::with_capacity(cfg.write_bytes * 2),
            idx_scratch: Vec::new(),
            unknown_reported: None,
            cfg,
        };
        let name = format!("loom-journal-{}", worker.shared.producer_id);
        let join = std::thread::Builder::new()
            .name(name)
            .spawn(move || worker.run())
            .map_err(JournalError::io(&dir))?;
        Ok(JournalWriter {
            handle: JournalHandle { shared },
            join: Some(join),
            dir,
            recovered,
        })
    }

    /// Append/status handle (cloneable).
    pub fn handle(&self) -> JournalHandle {
        self.handle.clone()
    }

    /// Producer-epoch directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Segments recovered at spawn.
    pub fn recovered(&self) -> &[crate::segment::RecoveryReport] {
        &self.recovered
    }

    /// Write everything queued so far and `sync_data` it; returns the status afterwards.
    /// Errors: `Failed` while the disk is failing, `Timeout` after `timeout`.
    pub fn flush(&self, timeout: Duration) -> Result<JournalStatus, JournalError> {
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut w = lock(&self.handle.shared.waiters);
            if w.len() >= MAX_FLUSH_WAITERS {
                return Err(JournalError::Timeout("too many concurrent flush calls"));
            }
            w.push(tx);
        }
        self.handle.shared.queue.wake();
        match rx.recv_timeout(timeout) {
            Ok(Ok(())) => Ok(self.handle.status()),
            Ok(Err(e)) => Err(JournalError::Failed(e)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(JournalError::Timeout("flush")),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(JournalError::Closed),
        }
    }

    /// Stop accepting, drain the queue, seal the active segment and join the thread.
    pub fn close(mut self) -> CloseReport {
        self.shutdown();
        CloseReport {
            status: self.handle.status(),
            gaps: self.handle.take_gaps(),
        }
    }

    fn shutdown(&mut self) {
        self.handle.shared.queue.close();
        if let Some(j) = self.join.take()
            && j.join().is_err()
        {
            self.handle
                .shared
                .set_error(Some("journal writer thread panicked".into()));
            self.handle.shared.disk_failed.store(true, Ordering::Release);
        }
    }
}

impl Drop for JournalWriter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Active {
    seg_no: u32,
    path: PathBuf,
    file: File,
    idx: Option<File>,
    len: u64,
    opened: Instant,
    frames: u64,
    last_seq: Option<u64>,
    index: IndexState,
    unsynced: bool,
}

struct Failure {
    cause: &'static str,
    retry_at: Instant,
    loss: Option<LossAcc>,
}

struct Worker {
    cfg: JournalConfig,
    shared: Arc<Shared>,
    dir: PathBuf,
    seg: Option<Active>,
    next_seg_no: u32,
    pending: VecDeque<Entry>,
    pending_since: Option<Instant>,
    pending_bytes: usize,
    last_sync: Instant,
    failure: Option<Failure>,
    scratch: Vec<u8>,
    idx_scratch: Vec<u8>,
    /// Highest seq already covered by a `sync_failed` gap.
    unknown_reported: Option<u64>,
}

fn fault(hook: &Option<FaultHook>, p: FaultPoint) -> io::Result<()> {
    match hook {
        Some(h) => h(p),
        None => Ok(()),
    }
}

fn cause_of(e: &io::Error) -> &'static str {
    if e.kind() == io::ErrorKind::StorageFull {
        causes::DISK_FULL
    } else {
        causes::DISK_ERROR
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_entry(
    e: &Entry,
    opaque: bool,
    policy: &IndexPolicy,
    last_seq: &mut Option<u64>,
    index: &mut IndexState,
    base: u64,
    out: &mut Vec<u8>,
    idx_out: &mut Vec<u8>,
) {
    if !e.rec_type.is_keyed() {
        encode_frame(e.rec_type, &e.payload, out);
        return;
    }
    let point = index.observe(policy, e.mono);
    let at = base + out.len() as u64;
    if opaque {
        let discontinuous = last_seq.is_none_or(|l| l.checked_add(1) != Some(e.seq));
        if point || discontinuous {
            let hint = IndexHint {
                source_seq: e.seq,
                mono_ns: e.mono,
                index_point: point,
            };
            encode_frame(RecType::IndexHint, &hint.encode(), out);
        }
    }
    if point {
        IndexEntry {
            key: e.seq,
            mono_ns: e.mono,
            offset: at,
        }
        .encode_into(idx_out);
    }
    encode_frame(e.rec_type, &e.payload, out);
    *last_seq = Some(e.seq);
}

impl Worker {
    fn blocked(&self) -> bool {
        self.failure.is_some() && self.cfg.loss == LossPolicy::Retain
    }

    fn run(mut self) {
        let mut batch = Vec::with_capacity(POP_BATCH);
        loop {
            let now = Instant::now();
            let wait = self.next_deadline(now).saturating_duration_since(now);
            let closed = if self.blocked() {
                self.shared.queue.park(wait)
            } else {
                self.shared.queue.pop_batch(wait, POP_BATCH, &mut batch)
            };
            for e in batch.drain(..) {
                self.accept(e);
            }
            self.serve_waiters();
            self.tick(Instant::now());
            if closed {
                break;
            }
        }
        self.shutdown();
    }

    fn next_deadline(&self, now: Instant) -> Instant {
        let mut d = now + MAX_IDLE_WAIT;
        if let Some(f) = &self.failure {
            d = d.min(f.retry_at);
        }
        if let Some(t) = self.pending_since {
            d = d.min(t + self.cfg.write_interval);
        }
        if let Some(s) = &self.seg {
            if s.unsynced {
                d = d.min(self.last_sync + self.cfg.sync_interval);
            }
            if s.frames > 0 {
                d = d.min(s.opened + self.cfg.rotate_age);
            }
        }
        d
    }

    fn accept(&mut self, e: Entry) {
        if self.cfg.loss == LossPolicy::Drop
            && let Some(f) = self.failure.as_mut()
        {
            add_loss(&mut f.loss, &e);
            return;
        }
        self.pending_bytes += e.payload.len();
        self.pending_since.get_or_insert_with(Instant::now);
        self.pending.push_back(e);
        if self.pending_bytes >= self.cfg.write_bytes {
            self.flush_os();
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.failure.as_ref().is_some_and(|f| now >= f.retry_at) {
            self.retry(now);
        }
        if self.failure.is_some() {
            return;
        }
        if self
            .pending_since
            .is_some_and(|t| now.duration_since(t) >= self.cfg.write_interval)
        {
            self.flush_os();
        }
        if self.seg.as_ref().is_some_and(|s| s.unsynced)
            && now.duration_since(self.last_sync) >= self.cfg.sync_interval
        {
            self.sync();
        }
        if self
            .seg
            .as_ref()
            .is_some_and(|s| s.frames > 0 && now.duration_since(s.opened) >= self.cfg.rotate_age)
        {
            self.flush_os();
            if self.failure.is_none() {
                self.seal_active();
            }
        }
    }

    fn serve_waiters(&mut self) {
        let waiters = std::mem::take(&mut *lock(&self.shared.waiters));
        if waiters.is_empty() {
            return;
        }
        // Take everything that was queued when the flush was requested (bounded by that depth,
        // so a fast producer cannot keep the flush running forever).
        if !self.blocked() {
            let mut left = self.shared.queue.stats().depth;
            let mut batch = Vec::new();
            while left > 0 {
                self.shared
                    .queue
                    .pop_batch(Duration::ZERO, left.min(POP_BATCH), &mut batch);
                if batch.is_empty() {
                    break;
                }
                left = left.saturating_sub(batch.len());
                for e in batch.drain(..) {
                    self.accept(e);
                }
            }
        }
        self.flush_os();
        if self.failure.is_none() && self.seg.as_ref().is_some_and(|s| s.unsynced) {
            self.sync();
        }
        let res = match &self.failure {
            None => Ok(()),
            Some(f) => Err(lock(&self.shared.last_error)
                .clone()
                .unwrap_or_else(|| f.cause.to_string())),
        };
        for w in waiters {
            let _ = w.try_send(res.clone());
        }
    }

    fn ensure_segment(&mut self) -> io::Result<()> {
        if self.seg.is_some() {
            return Ok(());
        }
        fault(&self.cfg.fault, FaultPoint::Open)?;
        fs::create_dir_all(&self.dir)?;
        let mut tries = 0;
        let (seg_no, path, mut file) = loop {
            let seg_no = self.next_seg_no;
            self.next_seg_no = self.next_seg_no.saturating_add(1);
            let path = self.dir.join(segment_file_name(seg_no, false));
            if self.dir.join(segment_file_name(seg_no, true)).exists() {
                continue;
            }
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(f) => break (seg_no, path, f),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && tries < 1000 => tries += 1,
                Err(e) => return Err(e),
            }
        };
        let header = SegmentHeader {
            codec: self.cfg.codec,
            producer_id: self.cfg.producer_id.clone(),
            producer_epoch: self.cfg.producer_epoch,
            created_wall_ns: wall_ns_now(),
            clock_id: self.cfg.clock_id.clone(),
        }
        .encode()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        if let Err(e) = fault(&self.cfg.fault, FaultPoint::Write).and_then(|()| file.write_all(&header)) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(e);
        }
        let idx = match create_index_file(&index_path(&self.dir, seg_no)) {
            Ok(f) => Some(f),
            Err(e) => {
                warn!(error = %e, "journal index file not created; will rebuild at seal");
                None
            }
        };
        self.seg = Some(Active {
            seg_no,
            path,
            file,
            idx,
            len: header.len() as u64,
            opened: Instant::now(),
            frames: 0,
            last_seq: None,
            index: IndexState::default(),
            unsynced: true,
        });
        Ok(())
    }

    fn flush_os(&mut self) {
        let opaque = self.cfg.codec.is_opaque();
        while !self.pending.is_empty() {
            if self.failure.is_some() {
                return;
            }
            if let Err(e) = self.ensure_segment() {
                let msg = format!("open segment in {}: {e}", self.dir.display());
                self.on_error(cause_of(&e), msg);
                return;
            }
            let Some(seg) = self.seg.as_mut() else { return };
            self.scratch.clear();
            self.idx_scratch.clear();
            let base = seg.len;
            let mut last_seq = seg.last_seq;
            let mut index = seg.index;
            let mut n = 0usize;
            let mut rotate = false;
            let mut last_key = None;
            let mut done_bytes = 0usize;
            for e in &self.pending {
                let need = frame_len(e.payload.len()) + if opaque { HINT_FRAME_LEN } else { 0 };
                if base + self.scratch.len() as u64 + need > self.cfg.rotate_bytes
                    && seg.frames + n as u64 > 0
                {
                    rotate = true;
                    break;
                }
                encode_entry(
                    e,
                    opaque,
                    &self.cfg.index,
                    &mut last_seq,
                    &mut index,
                    base,
                    &mut self.scratch,
                    &mut self.idx_scratch,
                );
                if e.rec_type.is_keyed() {
                    last_key = Some(e.seq);
                }
                done_bytes += e.payload.len();
                n += 1;
            }
            let res =
                fault(&self.cfg.fault, FaultPoint::Write).and_then(|()| seg.file.write_all(&self.scratch));
            if let Err(e) = res {
                // Cut any partial frame; if that fails too, crash recovery trims it later.
                let _ = seg.file.set_len(base);
                let msg = format!("write {}: {e}", seg.path.display());
                // Flag the failure before the (possibly slow) fsync of the abandoned segment.
                self.on_error(cause_of(&e), msg);
                self.abandon_segment();
                return;
            }
            seg.len += self.scratch.len() as u64;
            seg.frames += n as u64;
            seg.last_seq = last_seq;
            seg.index = index;
            seg.unsynced = true;
            if !self.idx_scratch.is_empty()
                && let Some(f) = seg.idx.as_mut()
                && let Err(e) = f.write_all(&self.idx_scratch)
            {
                warn!(error = %e, "journal index write failed; will rebuild at seal");
                seg.idx = None;
            }
            if let Some(k) = last_key {
                wm_set(&self.shared.written, k);
            }
            self.pending.drain(..n);
            self.pending_bytes -= done_bytes;
            if rotate {
                self.seal_active();
            }
        }
        self.pending_since = None;
    }

    /// Record a disk failure and enter the failed state.
    fn on_error(&mut self, cause: &'static str, msg: String) {
        warn!(producer = %self.cfg.producer_id, error = %msg, "journal write failed");
        self.shared.disk_failed.store(true, Ordering::Release);
        self.shared.set_error(Some(msg));
        let mut loss = None;
        if self.cfg.loss == LossPolicy::Drop {
            for e in self.pending.drain(..) {
                add_loss(&mut loss, &e);
            }
            self.pending_bytes = 0;
            self.pending_since = None;
        }
        let retry_at = Instant::now() + self.cfg.retry_interval;
        match self.failure.as_mut() {
            Some(f) => {
                if let Some(l) = loss {
                    match f.loss.as_mut() {
                        Some(a) => {
                            a.from = a.from.min(l.from);
                            a.to = a.to.max(l.to);
                            a.count += l.count;
                            a.bytes += l.bytes;
                        }
                        None => f.loss = Some(l),
                    }
                }
                f.retry_at = retry_at;
            }
            None => {
                self.failure = Some(Failure {
                    cause,
                    retry_at,
                    loss,
                })
            }
        }
    }

    /// Gap for records written to the OS but not known durable (after a failed sync).
    fn sync_failed_gap(&mut self) {
        let w = wm_get(&self.shared.written);
        // Start after what is durable and after what an earlier sync failure already reported;
        // `durable_through` itself is never advanced over data that is not known durable.
        let d = match (wm_get(&self.shared.durable), self.unknown_reported) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        if let Some(w) = w
            && d.is_none_or(|d| d < w)
        {
            self.shared.push_gap(GapReport {
                producer_id: self.cfg.producer_id.clone(),
                producer_epoch: self.cfg.producer_epoch,
                from: d.map_or(0, |d| d + 1),
                to: Some(w),
                count: GapCount::Unknown,
                cause: causes::SYNC_FAILED.to_string(),
                bytes: None,
                class: None,
            });
            self.unknown_reported = Some(w);
        }
    }

    fn mark_durable(&self) {
        if let Some(w) = wm_get(&self.shared.written) {
            wm_set(&self.shared.durable, w);
        }
    }

    fn sync(&mut self) {
        let Some(seg) = self.seg.as_mut() else { return };
        let res = fault(&self.cfg.fault, FaultPoint::Sync).and_then(|()| seg.file.sync_data());
        self.last_sync = Instant::now();
        match res {
            Ok(()) => {
                seg.unsynced = false;
                self.mark_durable();
            }
            Err(e) => {
                let msg = format!("sync {}: {e}", seg.path.display());
                // After a failed fsync the page cache state is unknown: stop using this file
                // (it stays `.lj.open`; the next recovery seals its valid prefix). A file with
                // no records is removed so retry cycles do not leave empty segments behind.
                if let Some(seg) = self.seg.take()
                    && seg.frames == 0
                {
                    drop(seg.file);
                    let _ = fs::remove_file(&seg.path);
                    let _ = fs::remove_file(index_path(&self.dir, seg.seg_no));
                }
                self.sync_failed_gap();
                self.on_error(causes::SYNC_FAILED, msg);
            }
        }
    }

    /// Close the active segment after a write error: keep its valid prefix if it can be synced.
    fn abandon_segment(&mut self) {
        let Some(seg) = self.seg.take() else { return };
        if seg.frames == 0 {
            drop(seg.file);
            let _ = fs::remove_file(&seg.path);
            let _ = fs::remove_file(index_path(&self.dir, seg.seg_no));
            return;
        }
        match fault(&self.cfg.fault, FaultPoint::Sync).and_then(|()| seg.file.sync_data()) {
            Ok(()) => {
                self.mark_durable();
                let sealed = self.dir.join(segment_file_name(seg.seg_no, true));
                if fs::rename(&seg.path, &sealed).is_ok() {
                    let _ = rebuild_index(&sealed, &self.cfg.index);
                    sync_dir(&self.dir);
                }
            }
            Err(_) => self.sync_failed_gap(),
        }
    }

    fn seal_active(&mut self) {
        let Some(seg) = self.seg.take() else { return };
        let res = fault(&self.cfg.fault, FaultPoint::Sync).and_then(|()| seg.file.sync_data());
        self.last_sync = Instant::now();
        if let Err(e) = res {
            let msg = format!("seal sync {}: {e}", seg.path.display());
            self.sync_failed_gap();
            self.on_error(causes::SYNC_FAILED, msg);
            return;
        }
        self.mark_durable();
        let sealed = self.dir.join(segment_file_name(seg.seg_no, true));
        if let Err(e) = fs::rename(&seg.path, &sealed) {
            // Data is durable; the file stays `.lj.open` and is sealed by the next recovery.
            let msg = format!("seal rename {}: {e}", seg.path.display());
            self.on_error(cause_of(&e), msg);
            return;
        }
        match seg.idx {
            Some(f) => {
                let _ = f.sync_data();
            }
            None => {
                if let Err(e) = rebuild_index(&sealed, &self.cfg.index) {
                    warn!(error = %e, "journal index rebuild at seal failed");
                }
            }
        }
        sync_dir(&self.dir);
    }

    fn retry(&mut self, now: Instant) {
        let Some(f) = self.failure.take() else { return };
        match self.ensure_segment() {
            Ok(()) => {
                self.shared.disk_failed.store(false, Ordering::Release);
                self.shared.set_error(None);
                if let Some(loss) = f.loss {
                    let g = loss.report(&self.shared, f.cause);
                    if let Ok(payload) = serde_json::to_vec(&GapMarker {
                        marker: "gap",
                        gap: &g,
                    }) {
                        self.pending_bytes += payload.len();
                        self.pending.push_front(Entry {
                            rec_type: RecType::Marker,
                            seq: 0,
                            mono: 0,
                            payload,
                        });
                        self.pending_since.get_or_insert(now);
                    }
                    self.shared.push_gap(g);
                }
                info!(producer = %self.cfg.producer_id, "journal recovered from disk error");
                if !self.pending.is_empty() {
                    self.flush_os();
                }
            }
            Err(e) => {
                self.shared
                    .set_error(Some(format!("retry open in {}: {e}", self.dir.display())));
                self.failure = Some(Failure {
                    retry_at: now + self.cfg.retry_interval,
                    ..f
                });
            }
        }
    }

    fn shutdown(&mut self) {
        // Drain what is left (the queue is closed, so this terminates).
        let mut batch = Vec::new();
        loop {
            batch.clear();
            self.shared.queue.pop_batch(Duration::ZERO, POP_BATCH, &mut batch);
            if batch.is_empty() {
                break;
            }
            for e in batch.drain(..) {
                if self.blocked() {
                    self.pending_bytes += e.payload.len();
                    self.pending.push_back(e);
                } else {
                    self.accept(e);
                }
            }
        }
        if self.failure.is_some() {
            self.retry(Instant::now());
        }
        self.flush_os();
        if self.failure.is_none() {
            self.seal_active();
        }
        if let Some(f) = self.failure.take() {
            let mut loss = f.loss;
            for e in self.pending.drain(..) {
                add_loss(&mut loss, &e);
            }
            if let Some(l) = loss {
                let mut g = l.report(&self.shared, f.cause);
                if self.cfg.loss == LossPolicy::Retain {
                    g.cause = causes::CLOSED_FAILED.to_string();
                }
                self.shared.push_gap(g);
            }
        }
        let overflow = std::mem::take(&mut lock(&self.shared.gaps).overflow);
        for (class, acc) in overflow {
            let mut g = acc.report(&self.shared, causes::QUEUE_FULL);
            g.class = class;
            self.shared.push_gap(g);
        }
        let waiters = std::mem::take(&mut *lock(&self.shared.waiters));
        for w in waiters {
            let _ = w.try_send(Err("journal closed".into()));
        }
    }
}
