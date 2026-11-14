//! Event hub (contract 01 §1.4–1.6, plan §3.2·§3.4). One OS thread (outside tokio) owns the
//! sequencer: it assigns stream seqs, keeps a bounded ring, runs reducers, and fans out to
//! connections. Everything else talks to it through one bounded command channel.
//!
//! * **Admission**: the channel holds `DATA_CAP_EVENTS + PRIO_RESERVE` commands. Data publishes are
//!   admitted only while the data counters (16k events / 8 MiB) allow; otherwise they are dropped
//!   per producer and reported as `display.gap{cause:"hub_overload"}`. Priority publishes never
//!   count against the data budget, so the reserve keeps them from failing (01 §1.6).
//! * **Serialize once**: each published record is turned into JSON once (`Arc<str>`) and shared by
//!   the ring, every subscription and snapshot.
//! * **Delivery**: priority kinds (`is_priority_kind`) go once to each viewer connection's control
//!   queue (never dropped — overflow closes the connection); other events go to matching data
//!   subscriptions (bounded bytes, oldest dropped, gap reported to that connection only).
//! * **publish-before-reply**: [`Hub::barrier`] resolves after every earlier command was processed,
//!   i.e. the events are in all viewer queues.

pub mod stream;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_domain_api::Reducer;
use loom_proto::core_events::is_priority_kind;
use loom_proto::envelope::RecordHealth;
use loom_proto::error::ErrorCode;
use loom_proto::event::{
    Env, GapCount, GapData, Origin, ProducerRange, PublishedRecord, SourceRecord, Ts, TsSource, WriteState,
    kinds, payload_write_state,
};
use loom_proto::num::U64;
use loom_proto::rpc::{SubFilter, Subscribe};
use loom_proto::stream::SnapshotItem;
use serde_json::{Value, json};
use tokio::sync::{Notify, oneshot};

/// Data publishes admitted at once (events).
pub const DATA_CAP_EVENTS: usize = 16_384;
/// Data publishes admitted at once (bytes, estimated).
pub const DATA_CAP_BYTES: usize = 8 * 1024 * 1024;
/// Extra channel slots reserved for priority publishes and control commands.
pub const PRIO_RESERVE: usize = 4096;
/// Ring: total conservative retained-heap charge (JSON, parsed records and index ownership).
pub const RING_BYTES: usize = 32 * 1024 * 1024;
/// Ring: conservative retained-heap charge per channel; original records remain in the journal.
pub const RING_CHANNEL_BYTES: usize = 4 * 1024 * 1024;
/// Control queue per viewer connection (events).
pub const PRIO_CAP_EVENTS: usize = 256;
/// Control queue per viewer connection (bytes).
pub const PRIO_CAP_BYTES: usize = 256 * 1024;
/// Data queue per subscription (bytes). Contract: rate × 500 ms clamped to 1–4 MiB; M1 uses the
/// fixed midpoint.
pub const SUB_QUEUE_BYTES: usize = 2 * 1024 * 1024;
/// Data queues of all subscriptions together (bytes).
pub const TOTAL_SUB_BYTES: usize = 32 * 1024 * 1024;
/// Subscriptions per connection.
pub const MAX_SUBS_PER_CONN: usize = 8;
/// Subscriptions in total.
pub const MAX_SUBS_TOTAL: usize = 32;
/// Connections in total.
pub const MAX_CONNS: usize = 64;
/// Default snapshot backlog (events).
pub const DEFAULT_BACKLOG: usize = 1000;
/// Lag after which a subscription drops its backlog and jumps to live.
pub const JUMP_TO_LIVE: Duration = Duration::from_secs(2);

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Hub clock: monotonic since start + wall base.
#[derive(Debug)]
pub struct HubClock {
    start: Instant,
    wall_base_ns: u64,
    /// Clock domain id.
    pub clock_id: String,
}

impl HubClock {
    fn new() -> Self {
        let wall_base_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(0))
            .unwrap_or(0);
        HubClock {
            start: Instant::now(),
            wall_base_ns,
            clock_id: format!("loomd-{}", std::process::id()),
        }
    }
    /// Monotonic ns since hub start.
    pub fn mono_ns(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    /// Wall ns.
    pub fn wall_ns(&self) -> u64 {
        self.wall_base_ns.saturating_add(self.mono_ns())
    }
    /// Timestamp now.
    pub fn ts(&self, source: TsSource) -> Ts {
        Ts {
            mono_ns: U64(self.mono_ns()),
            wall_ns: U64(self.wall_ns()),
            clock_id: self.clock_id.clone(),
            source,
            quality: None,
        }
    }
}

/// A published record with its JSON serialized once.
#[derive(Debug)]
pub struct Published {
    /// The record.
    pub rec: PublishedRecord,
    /// JSON text, shared by ring, subscriptions and snapshots.
    pub json: Arc<str>,
    /// Priority kind (control path).
    pub prio: bool,
    /// When the hub published it.
    pub at: Instant,
    /// Approximate retained-heap charge (record tree, JSON, and eviction-index ownership).
    pub cache_cost: usize,
}

// Payload JSON is also retained as a parsed Value tree. Budget both representations and
// owned metadata; counting only wire bytes lets small structured records retain several
// times the advertised memory. These are approximate logical charges, not allocator RSS.
fn string_charge(s: &String) -> usize {
    if s.capacity() == 0 {
        0
    } else {
        s.capacity().saturating_add(16)
    }
}
fn value_heap(v: &Value, depth: usize) -> usize {
    if depth > 64 {
        return RING_BYTES + 1;
    }
    match v {
        Value::String(s) => string_charge(s),
        Value::Array(a) => a.iter().fold(
            a.capacity()
                .saturating_mul(std::mem::size_of::<Value>())
                .saturating_add(16),
            |sum, v| sum.saturating_add(value_heap(v, depth + 1)),
        ),
        Value::Object(m) => m.iter().fold(0usize, |sum, (key, value)| {
            // Spare BTreeMap node slots + allocator bookkeeping, in addition to each pair.
            sum.saturating_add(std::mem::size_of::<(String, Value)>() + 96)
                .saturating_add(string_charge(key))
                .saturating_add(value_heap(value, depth + 1))
        }),
        _ => 0,
    }
}
fn cache_charge(r: &PublishedRecord, json_bytes: usize) -> usize {
    let mut n = std::mem::size_of::<Published>() + 64 + json_bytes;
    for s in [&r.session_id, &r.domain, &r.kind, &r.producer_id, &r.ts.clock_id] {
        n = n.saturating_add(string_charge(s));
    }
    for s in [
        &r.channel,
        &r.ts.quality,
        &r.client_id,
        &r.actor_id,
        &r.external_actor,
        &r.request_id,
        &r.job_id,
        &r.purpose,
        &r.test_session,
        &r.test_step,
    ]
    .into_iter()
    .flatten()
    {
        n = n.saturating_add(string_charge(s));
    }
    n = n.saturating_add(value_heap(&r.payload, 0));
    n = n.saturating_add(
        r.metrics
            .capacity()
            .saturating_mul(std::mem::size_of::<loom_proto::event::Metric>())
            + 16,
    );
    for metric in &r.metrics {
        n = n.saturating_add(string_charge(&metric.name));
        if let Some(unit) = &metric.unit {
            n = n.saturating_add(string_charge(unit));
        }
    }
    n = n.saturating_add(r.quality.capacity().saturating_mul(std::mem::size_of::<String>()) + 16);
    for s in &r.quality {
        n = n.saturating_add(string_charge(s));
    }
    // The sequence index owns a cloned ring key; VecDeque slots and tree nodes have overhead.
    n = n.saturating_add(std::mem::size_of::<(u64, ChannelKey)>() + 96 + string_charge(&r.domain));
    if let Some(ch) = &r.channel {
        n = n.saturating_add(string_charge(ch));
    }
    n.min(RING_BYTES + 1)
}

/// One data subscription.
#[derive(Debug)]
pub struct Sub {
    /// Subscription id.
    pub id: String,
    /// Filter.
    pub filter: SubFilter,
    /// Batch upper bound.
    pub max_batch_ms: u32,
    /// Queue.
    pub q: VecDeque<Arc<Published>>,
    /// Queued bytes.
    pub bytes: usize,
    /// Events dropped so far.
    pub drops: u64,
    /// Pending gap `(first_seq, last_seq, count, cause)` to report.
    pub pending_gap: Option<(u64, u64, u64, &'static str)>,
    /// Last flush.
    pub last_flush: Instant,
}

/// Queues of one connection.
#[derive(Debug, Default)]
pub struct ConnQ {
    /// Control queue (viewer connections).
    pub prio: VecDeque<Arc<Published>>,
    /// Control queue bytes.
    pub prio_bytes: usize,
    /// Control queue overflowed — the connection must close.
    pub prio_overflow: bool,
    /// Data subscriptions.
    pub subs: Vec<Sub>,
}

/// Shared state of one connection.
#[derive(Debug)]
pub struct ConnShared {
    /// Connection id.
    pub id: u64,
    /// Viewer connections get the automatic control subscription.
    pub viewer: bool,
    /// Queues.
    pub q: Mutex<ConnQ>,
    /// Wakes the connection task.
    pub notify: Notify,
}

impl ConnShared {
    /// New connection state.
    pub fn new(id: u64, viewer: bool) -> Arc<Self> {
        Arc::new(ConnShared {
            id,
            viewer,
            q: Mutex::new(ConnQ::default()),
            notify: Notify::new(),
        })
    }
}

/// How a subscription starts.
#[derive(Debug)]
pub enum SubStart {
    /// Snapshot at high-water `h`.
    Snapshot {
        /// High-water.
        h: u64,
        /// Backlog events (seq ≤ h), oldest first.
        backlog: Vec<Arc<Published>>,
        /// Reducer states.
        items: Vec<SnapshotItem>,
        /// Why a snapshot was sent instead of a resume (`None` = asked for).
        resync: Option<&'static str>,
    },
    /// `after_seq` was in the ring: continue with these.
    Resume {
        /// Events after `after_seq`.
        events: Vec<Arc<Published>>,
    },
}

/// One stream-mapping record (journal truth: stream range ↔ producer range, plan §3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingRecord {
    /// Stream epoch.
    pub stream_epoch: u64,
    /// First stream seq.
    pub seq_first: u64,
    /// Last stream seq.
    pub seq_last: u64,
    /// Producer.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// First source seq.
    pub src_first: u64,
    /// Last source seq.
    pub src_last: u64,
    /// `ts.mono_ns` of the first record.
    pub mono_first: u64,
    /// `ts.mono_ns` of the last record.
    pub mono_last: u64,
}

/// Exit for mapping records. The journal crate implements a durable one; [`MemoryMapping`] keeps
/// the last records in memory.
pub trait MappingSink: Send {
    /// Record one mapping (called on the hub thread; must not block for long).
    fn record(&mut self, m: MappingRecord);
}

/// Bounded in-memory mapping sink (last 4096 records).
#[derive(Debug, Default, Clone)]
pub struct MemoryMapping(pub Arc<Mutex<VecDeque<MappingRecord>>>);

impl MappingSink for MemoryMapping {
    fn record(&mut self, m: MappingRecord) {
        let mut q = lock(&self.0);
        if q.len() == 4096 {
            q.pop_front();
        }
        q.push_back(m);
    }
}

/// Core state folded from events (snapshot `core.snapshot`).
#[derive(Debug, Default, Clone)]
struct CoreState {
    latch: bool,
    env: Option<Env>,
    record_health: RecordHealth,
    workers: BTreeMap<String, Value>,
    writes: BTreeMap<String, Value>,
    /// Open channels `(domain, channel)` → `{domain, channel, state, generation, seq}` (UI Q20).
    channels: BTreeMap<(String, String), Value>,
}

/// `<domain>.state` / `<domain>.*_state` of a device domain, i.e. a channel state event.
fn is_channel_state(r: &PublishedRecord) -> bool {
    let Some((d, ev)) = r.kind.split_once('.') else {
        return false;
    };
    !loom_proto::rpc::is_reserved_domain(d)
        && r.channel.is_some()
        && (ev == "state" || ev.ends_with("_state"))
        // a recording's lifecycle (`video.record_state`: recording/finalized/failed) shares the
        // capture channel but is not its connectivity; it must not replace the channel entry
        && !ev.ends_with("record_state")
}

impl CoreState {
    fn apply(&mut self, r: &PublishedRecord) {
        if r.env.is_some() {
            self.env = r.env;
        }
        match r.kind.as_str() {
            kinds::CORE_LATCH => self.latch = r.payload["on"].as_bool().unwrap_or(self.latch),
            kinds::RECORD_HEALTH => {
                if let Ok(h) = serde_json::from_value(r.payload["health"].clone()) {
                    self.record_health = h;
                }
            }
            kinds::CORE_WORKER_STATE => {
                if let Some(i) = r.payload["instance"].as_str() {
                    self.workers.insert(i.to_owned(), r.payload.clone());
                }
            }
            _ => {}
        }
        if is_channel_state(r)
            && let Some(ch) = &r.channel
        {
            let key = (r.domain.clone(), ch.clone());
            let state = r
                .payload
                .get("state")
                .cloned()
                .unwrap_or_else(|| r.payload.clone());
            if matches!(state.as_str(), Some("closed" | "removed")) {
                self.channels.remove(&key);
            } else if self.channels.len() < 1024 || self.channels.contains_key(&key) {
                self.channels.insert(
                    key,
                    json!({"domain": r.domain, "channel": ch, "state": state, "generation": r.generation, "seq": r.seq, "kind": r.kind}),
                );
            }
        }
        if let (Some(rid), Ok(Some(st))) = (&r.request_id, payload_write_state(&r.payload)) {
            let done = r.payload["final"].as_bool() == Some(true)
                || matches!(st, WriteState::Failed | WriteState::OutcomeUnknown);
            if done {
                self.writes.remove(rid);
            } else if self.writes.len() < 1024 || self.writes.contains_key(rid) {
                self.writes.insert(
                    rid.clone(),
                    json!({"kind": r.kind, "write_state": st, "seq": r.seq, "origin": r.origin}),
                );
            }
        }
    }

    fn channel_list(&self) -> Vec<Value> {
        self.channels.values().cloned().collect()
    }

    fn channels_item(&self) -> SnapshotItem {
        SnapshotItem {
            domain: "core".into(),
            channel: None,
            kind: "core.channels".into(),
            state: json!({"channels": self.channel_list()}),
        }
    }

    fn item(&self) -> SnapshotItem {
        SnapshotItem {
            domain: "core".into(),
            channel: None,
            kind: "core.snapshot".into(),
            state: json!({
                "latch": self.latch, "env": self.env, "record_health": self.record_health,
                "workers": self.workers, "writes_in_flight": self.writes,
            }),
        }
    }
}

/// Hub status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubStatus {
    /// High-water stream seq.
    pub hw: u64,
    /// Latch per core reducer.
    pub latch: bool,
    /// Recording health per core reducer.
    pub record_health: RecordHealth,
    /// Connections.
    pub conns: usize,
    /// Subscriptions.
    pub subs: usize,
    /// Ring events.
    pub ring_events: usize,
    /// Entries in the global eviction index (one per retained record).
    pub ring_order_entries: usize,
    /// Serialized JSON bytes retained by the ring (bounded by RING_BYTES).
    pub ring_json_bytes: usize,
    /// Retained-heap charge, bounded independently from the serialized wire size.
    pub ring_cost_bytes: usize,
    /// Open channels (`core.channels` reducer).
    pub channels: Vec<Value>,
}

enum Cmd {
    Publish {
        recs: Vec<SourceRecord>,
        data_bytes: usize,
        data_events: usize,
        only: Option<u64>,
        ack: Option<oneshot::Sender<Vec<Arc<Published>>>>,
    },
    Connect {
        conn: Arc<ConnShared>,
        ack: oneshot::Sender<Result<(), ErrorCode>>,
    },
    Disconnect {
        id: u64,
    },
    Subscribe {
        conn: Arc<ConnShared>,
        sub_id: String,
        req: Box<Subscribe>,
        ack: oneshot::Sender<Result<SubStart, ErrorCode>>,
    },
    Unsubscribe {
        conn: Arc<ConnShared>,
        sub_id: String,
    },
    Barrier {
        ack: oneshot::Sender<()>,
    },
    Status {
        ack: oneshot::Sender<HubStatus>,
    },
    Stall {
        dur: Duration,
    },
    Query {
        filter: SubFilter,
        request_id: Option<String>,
        max: usize,
        ack: oneshot::Sender<Vec<Arc<Published>>>,
    },
    Range {
        q: Box<RangeQuery>,
        ack: oneshot::Sender<RangeResult>,
    },
}

/// Ring range query (`core.events.query`).
#[derive(Debug, Clone, Default)]
pub struct RangeQuery {
    /// Filter.
    pub filter: SubFilter,
    /// Only seq > this.
    pub after: Option<u64>,
    /// Only seq <= this.
    pub until: Option<u64>,
    /// Only events published at or after this instant.
    pub since_at: Option<Instant>,
    /// Only events published at or before this instant.
    pub until_at: Option<Instant>,
    /// Most events returned.
    pub max: usize,
    /// Take the newest `max` (else the oldest `max` after the start).
    pub newest: bool,
}

/// Result of a [`RangeQuery`].
#[derive(Debug, Default)]
pub struct RangeResult {
    /// Events, oldest first.
    pub events: Vec<Arc<Published>>,
    /// More matching events exist beyond `max`.
    pub more: bool,
    /// High-water when the query ran.
    pub hw: u64,
    /// Oldest seq still in the ring (any channel).
    pub oldest: Option<u64>,
    /// Highest stream seq evicted from any ring the filter can select (domain/channel). A read
    /// starting at or below it is missing events in the ring and must use the journal.
    pub evicted_through: Option<u64>,
    /// Publish instant of that evicted event (time-cursor reads compare against it).
    pub evicted_at: Option<Instant>,
}

/// Called on the hub thread for every published (non-private) record, in seq order. Must be quick.
pub type Observer = Box<dyn FnMut(&PublishedRecord) + Send>;

/// Hub construction.
pub struct HubBuilder {
    epoch: u64,
    reducers: Vec<(String, Box<dyn Reducer>)>,
    mapping: Box<dyn MappingSink>,
    observers: Vec<Observer>,
    pre: Vec<PrePublish>,
}

/// Called on the hub thread for every non-private record **before** it gets its stream seq (may
/// restamp source identity and journal it). Must be quick and non-blocking.
pub type PrePublish = Box<dyn FnMut(&mut SourceRecord) + Send>;

impl std::fmt::Debug for HubBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubBuilder")
            .field("epoch", &self.epoch)
            .field("reducers", &self.reducers.len())
            .finish_non_exhaustive()
    }
}

impl HubBuilder {
    /// New builder with stream epoch.
    pub fn new(epoch: u64) -> Self {
        HubBuilder {
            epoch,
            reducers: Vec::new(),
            mapping: Box::new(MemoryMapping::default()),
            observers: Vec::new(),
            pre: Vec::new(),
        }
    }
    /// Add a pre-publish hook (journal of gateway producers).
    pub fn pre_publish(mut self, h: PrePublish) -> Self {
        self.pre.push(h);
        self
    }
    /// Add an observer (others_since_last, activity …).
    pub fn observer(mut self, o: Observer) -> Self {
        self.observers.push(o);
        self
    }
    /// Add a domain reducer (snapshot provider).
    pub fn reducer(mut self, domain: &str, r: Box<dyn Reducer>) -> Self {
        self.reducers.push((domain.to_owned(), r));
        self
    }
    /// Use a mapping sink (journal).
    pub fn mapping(mut self, m: Box<dyn MappingSink>) -> Self {
        self.mapping = m;
        self
    }
    /// Start the sequencer thread.
    pub fn start(self) -> std::io::Result<Hub> {
        let (tx, rx) = sync_channel(DATA_CAP_EVENTS + PRIO_RESERVE);
        let clock = Arc::new(HubClock::new());
        let shared = Arc::new(Counters::default());
        let seq = Sequencer {
            rx,
            epoch: self.epoch,
            clock: clock.clone(),
            counters: shared.clone(),
            next_seq: 1,
            rings: HashMap::new(),
            evicted: Evictions::default(),
            ring_order: BTreeMap::new(),
            ring_bytes: 0,
            ring_json_bytes: 0,
            conns: Vec::new(),
            reducers: self.reducers,
            core: CoreState::default(),
            mapping: self.mapping,
            observers: self.observers,
            pre: self.pre,
            subs_total: 0,
        };
        std::thread::Builder::new()
            .name("loom-hub".into())
            .spawn(move || seq.run())?;
        Ok(Hub {
            tx,
            epoch: self.epoch,
            clock,
            counters: shared,
        })
    }
}

/// Dropped data publishes per `(producer, epoch)`: `(first src seq, last src seq, count)`.
type DropMap = BTreeMap<(String, u64), (u64, u64, u64)>;

#[derive(Debug, Default)]
struct Counters {
    data_events: AtomicUsize,
    data_bytes: AtomicUsize,
    sub_bytes: AtomicUsize,
    dropped: Mutex<DropMap>,
}

/// Handle to the hub (cheap to clone).
#[derive(Clone, Debug)]
pub struct Hub {
    tx: SyncSender<Cmd>,
    /// Stream epoch.
    pub epoch: u64,
    /// Clock.
    pub clock: Arc<HubClock>,
    counters: Arc<Counters>,
}

fn est_bytes(r: &SourceRecord) -> usize {
    256 + r.payload.to_string().len()
}

/// The hub thread is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("event hub stopped")]
pub struct HubGone;

impl Hub {
    /// Publish records to every matching connection. Data records may be dropped under overload
    /// (reported as `display.gap`); the returned receiver yields the published form.
    pub fn publish(&self, recs: Vec<SourceRecord>) -> oneshot::Receiver<Vec<Arc<Published>>> {
        self.publish_inner(recs, None)
    }

    /// Publish to one connection only (per-subscriber `display.gap`).
    pub fn publish_to(&self, recs: Vec<SourceRecord>, conn: u64) -> oneshot::Receiver<Vec<Arc<Published>>> {
        self.publish_inner(recs, Some(conn))
    }

    fn publish_inner(
        &self,
        recs: Vec<SourceRecord>,
        only: Option<u64>,
    ) -> oneshot::Receiver<Vec<Arc<Published>>> {
        let (ack, rx) = oneshot::channel();
        if let Err(recs) = self.send_publish(recs, only, Some(ack), true) {
            self.count_drops(&recs);
        }
        rx
    }

    /// Non-blocking publish for worker emitters (`Err` = full, caller counts the loss).
    pub fn try_publish(&self, recs: Vec<SourceRecord>) -> Result<(), Vec<SourceRecord>> {
        self.send_publish(recs, None, None, false)
    }

    fn send_publish(
        &self,
        recs: Vec<SourceRecord>,
        only: Option<u64>,
        ack: Option<oneshot::Sender<Vec<Arc<Published>>>>,
        may_block_prio: bool,
    ) -> Result<(), Vec<SourceRecord>> {
        let prio = recs.iter().any(|r| is_priority_kind(&r.kind, &r.payload));
        let (mut events, mut bytes) = (0, 0);
        if !prio {
            events = recs.len();
            bytes = recs.iter().map(est_bytes).sum();
            let e = self.counters.data_events.fetch_add(events, Ordering::AcqRel) + events;
            let b = self.counters.data_bytes.fetch_add(bytes, Ordering::AcqRel) + bytes;
            if e > DATA_CAP_EVENTS || b > DATA_CAP_BYTES {
                self.counters.data_events.fetch_sub(events, Ordering::AcqRel);
                self.counters.data_bytes.fetch_sub(bytes, Ordering::AcqRel);
                return Err(recs);
            }
        }
        let cmd = Cmd::Publish {
            recs,
            data_bytes: bytes,
            data_events: events,
            only,
            ack,
        };
        let res = if prio && may_block_prio {
            self.tx.send(cmd).map_err(|e| e.0)
        } else {
            self.tx.try_send(cmd).map_err(|e| match e {
                TrySendError::Full(c) | TrySendError::Disconnected(c) => c,
            })
        };
        res.map_err(|cmd| {
            self.counters.data_events.fetch_sub(events, Ordering::AcqRel);
            self.counters.data_bytes.fetch_sub(bytes, Ordering::AcqRel);
            match cmd {
                Cmd::Publish { recs, .. } => recs,
                _ => Vec::new(),
            }
        })
    }

    /// Account dropped (not admitted) records per producer; the hub reports them as a gap.
    pub fn count_drops(&self, recs: &[SourceRecord]) {
        let mut d = lock(&self.counters.dropped);
        for r in recs {
            let e = d
                .entry((r.producer_id.clone(), r.producer_epoch.get()))
                .or_insert((r.source_seq.get(), r.source_seq.get(), 0));
            e.0 = e.0.min(r.source_seq.get());
            e.1 = e.1.max(r.source_seq.get());
            e.2 += 1;
        }
    }

    /// Register a connection.
    pub async fn connect(&self, conn: Arc<ConnShared>) -> Result<(), ErrorCode> {
        let (ack, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Connect { conn, ack })
            .map_err(|_| ErrorCode::Daemon)?;
        rx.await.map_err(|_| ErrorCode::Daemon)?
    }

    /// Unregister a connection.
    pub fn disconnect(&self, id: u64) {
        let _ = self.tx.send(Cmd::Disconnect { id });
    }

    /// Atomically register a subscription and capture its start (snapshot or resume).
    pub async fn subscribe(
        &self,
        conn: Arc<ConnShared>,
        sub_id: &str,
        req: Subscribe,
    ) -> Result<SubStart, ErrorCode> {
        let (ack, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Subscribe {
                conn,
                sub_id: sub_id.to_owned(),
                req: Box::new(req),
                ack,
            })
            .map_err(|_| ErrorCode::Daemon)?;
        rx.await.map_err(|_| ErrorCode::Daemon)?
    }

    /// Remove a subscription.
    pub fn unsubscribe(&self, conn: Arc<ConnShared>, sub_id: &str) {
        let _ = self.tx.send(Cmd::Unsubscribe {
            conn,
            sub_id: sub_id.to_owned(),
        });
    }

    /// Resolves once every command sent before it has been processed (publish-before-reply).
    /// `false` if the deadline passed (hub stalled) — the caller answers `E_PARTIAL`.
    pub async fn barrier(&self, deadline: Duration) -> bool {
        let (ack, rx) = oneshot::channel();
        if self.tx.send(Cmd::Barrier { ack }).is_err() {
            return false;
        }
        matches!(tokio::time::timeout(deadline, rx).await, Ok(Ok(())))
    }

    /// Hub status.
    pub async fn status(&self) -> Result<HubStatus, HubGone> {
        let (ack, rx) = oneshot::channel();
        self.tx.send(Cmd::Status { ack }).map_err(|_| HubGone)?;
        rx.await.map_err(|_| HubGone)
    }

    /// Latest `max` ring events matching `filter` (and `request_id`), oldest first. Bounded by the
    /// ring; durable history comes from the journal.
    pub async fn query(
        &self,
        filter: SubFilter,
        request_id: Option<String>,
        max: usize,
    ) -> Result<Vec<Arc<Published>>, HubGone> {
        let (ack, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Query {
                filter,
                request_id,
                max,
                ack,
            })
            .map_err(|_| HubGone)?;
        rx.await.map_err(|_| HubGone)
    }

    /// Range query over the ring (cursor-based reads).
    pub async fn range(&self, q: RangeQuery) -> Result<RangeResult, HubGone> {
        let (ack, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Range { q: Box::new(q), ack })
            .map_err(|_| HubGone)?;
        rx.await.map_err(|_| HubGone)
    }

    /// Test hook: block the sequencer thread for `dur` (simulates a stalled hub).
    #[doc(hidden)]
    pub fn stall_for_test(&self, dur: Duration) {
        let _ = self.tx.send(Cmd::Stall { dur });
    }

    /// Release accounted subscription bytes (called by connection tasks when they send).
    pub(crate) fn release_sub_bytes(&self, n: usize) {
        self.counters.sub_bytes.fetch_sub(
            n.min(self.counters.sub_bytes.load(Ordering::Acquire)),
            Ordering::AcqRel,
        );
    }
}

type ChannelKey = (String, Option<String>);

struct Sequencer {
    rx: Receiver<Cmd>,
    epoch: u64,
    clock: Arc<HubClock>,
    counters: Arc<Counters>,
    next_seq: u64,
    rings: HashMap<ChannelKey, (VecDeque<Arc<Published>>, usize)>,
    /// Per ring: newest evicted `(stream seq, publish instant)` (bounded by the ring keys).
    evicted: Evictions,
    ring_order: BTreeMap<u64, ChannelKey>,
    ring_bytes: usize,
    ring_json_bytes: usize,
    conns: Vec<Arc<ConnShared>>,
    reducers: Vec<(String, Box<dyn Reducer>)>,
    core: CoreState,
    mapping: Box<dyn MappingSink>,
    observers: Vec<Observer>,
    pre: Vec<PrePublish>,
    subs_total: usize,
}

/// Churn may leave empty ring keys behind. Bound the new coverage index independently;
/// overflow retains a global watermark, conservatively marking any older read incomplete.
const MAX_EVICTION_KEYS: usize = 4096;
#[derive(Default)]
struct Evictions {
    by_ring: HashMap<ChannelKey, (u64, Instant)>,
    overflow: Option<(u64, Instant)>,
}
impl Evictions {
    fn note(&mut self, key: ChannelKey, value: (u64, Instant)) {
        if self.by_ring.contains_key(&key) || self.by_ring.len() < MAX_EVICTION_KEYS {
            self.by_ring.insert(key, value);
        } else if self.overflow.is_none_or(|v| value.0 > v.0) {
            self.overflow = Some(value);
        }
    }
    fn selected(&self, filter: &SubFilter) -> Option<(u64, Instant)> {
        self.by_ring
            .iter()
            .filter(|(key, _)| ring_selectable(filter, key))
            .map(|(_, value)| *value)
            .chain(self.overflow)
            .max_by_key(|v| v.0)
    }
}

/// Can `f` select any record of ring `(domain, channel)`? (kinds/origins are not ring keys.)
fn ring_selectable(f: &SubFilter, (domain, channel): &ChannelKey) -> bool {
    (f.domains.is_empty() || f.domains.contains(domain))
        && (f.channels.is_empty() || channel.as_ref().is_some_and(|c| f.channels.contains(c)))
}

/// Does `f` select `r`? (`ext` is a domain push-down filter; the hub ignores it in M1.)
pub fn matches(f: &SubFilter, r: &PublishedRecord) -> bool {
    (f.domains.is_empty() || f.domains.contains(&r.domain))
        && (f.channels.is_empty() || r.channel.as_ref().is_some_and(|c| f.channels.contains(c)))
        && (f.kinds.is_empty() || f.kinds.contains(&r.kind))
        && (f.origins.is_empty() || f.origins.contains(&r.origin))
}

impl Sequencer {
    fn run(mut self) {
        while let Ok(cmd) = self.rx.recv() {
            self.handle(cmd);
            self.report_drops();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Publish {
                recs,
                data_bytes,
                data_events,
                only,
                ack,
            } => {
                self.counters.data_events.fetch_sub(data_events, Ordering::AcqRel);
                self.counters.data_bytes.fetch_sub(data_bytes, Ordering::AcqRel);
                let out = self.publish(recs, only);
                if let Some(a) = ack {
                    let _ = a.send(out);
                }
            }
            Cmd::Connect { conn, ack } => {
                let r = if self.conns.len() >= MAX_CONNS {
                    Err(ErrorCode::Busy)
                } else {
                    self.conns.push(conn);
                    Ok(())
                };
                let _ = ack.send(r);
            }
            Cmd::Disconnect { id } => {
                if let Some(i) = self.conns.iter().position(|c| c.id == id) {
                    let c = self.conns.remove(i);
                    let q = lock(&c.q);
                    self.subs_total -= q.subs.len();
                    let bytes: usize = q.subs.iter().map(|s| s.bytes).sum();
                    self.counters.sub_bytes.fetch_sub(
                        bytes.min(self.counters.sub_bytes.load(Ordering::Acquire)),
                        Ordering::AcqRel,
                    );
                }
            }
            Cmd::Subscribe {
                conn,
                sub_id,
                req,
                ack,
            } => {
                let _ = ack.send(self.subscribe(&conn, sub_id, &req));
            }
            Cmd::Unsubscribe { conn, sub_id } => {
                let mut q = lock(&conn.q);
                let before = q.subs.len();
                q.subs.retain(|s| s.id != sub_id);
                self.subs_total -= before - q.subs.len();
            }
            Cmd::Barrier { ack } => {
                let _ = ack.send(());
            }
            Cmd::Status { ack } => {
                let _ = ack.send(HubStatus {
                    hw: self.next_seq - 1,
                    latch: self.core.latch,
                    record_health: self.core.record_health,
                    conns: self.conns.len(),
                    subs: self.subs_total,
                    ring_events: self.rings.values().map(|(q, _)| q.len()).sum(),
                    ring_order_entries: self.ring_order.len(),
                    ring_json_bytes: self.ring_json_bytes,
                    ring_cost_bytes: self.ring_bytes,
                    channels: self.core.channel_list(),
                });
            }
            Cmd::Stall { dur } => std::thread::sleep(dur),
            Cmd::Query {
                filter,
                request_id,
                max,
                ack,
            } => {
                let mut all: Vec<Arc<Published>> = Vec::new();
                for (q, _) in self.rings.values() {
                    all.extend(
                        q.iter()
                            .filter(|p| matches(&filter, &p.rec))
                            .filter(|p| request_id.is_none() || p.rec.request_id == request_id)
                            .cloned(),
                    );
                }
                all.sort_by_key(|p| p.rec.seq.get());
                let skip = all.len().saturating_sub(max);
                let _ = ack.send(all.split_off(skip));
            }
            Cmd::Range { q, ack } => {
                let mut all: Vec<Arc<Published>> = Vec::new();
                for (rq, _) in self.rings.values() {
                    all.extend(
                        rq.iter()
                            .filter(|p| q.after.is_none_or(|a| p.rec.seq.get() > a))
                            .filter(|p| q.until.is_none_or(|u| p.rec.seq.get() <= u))
                            .filter(|p| q.since_at.is_none_or(|t| p.at >= t))
                            .filter(|p| q.until_at.is_none_or(|t| p.at <= t))
                            .filter(|p| matches(&q.filter, &p.rec))
                            .cloned(),
                    );
                }
                all.sort_by_key(|p| p.rec.seq.get());
                let more = all.len() > q.max;
                let events = if q.newest {
                    let skip = all.len().saturating_sub(q.max);
                    all.split_off(skip)
                } else {
                    all.truncate(q.max);
                    all
                };
                let oldest = self
                    .rings
                    .values()
                    .filter_map(|(r, _)| r.front())
                    .map(|p| p.rec.seq.get())
                    .min();
                let evicted = self.evicted.selected(&q.filter);
                let _ = ack.send(RangeResult {
                    events,
                    more,
                    hw: self.next_seq - 1,
                    oldest,
                    evicted_through: evicted.map(|e| e.0),
                    evicted_at: evicted.map(|e| e.1),
                });
            }
        }
    }

    fn report_drops(&mut self) {
        let dropped = std::mem::take(&mut *lock(&self.counters.dropped));
        if dropped.is_empty() {
            return;
        }
        let gaps = dropped
            .into_iter()
            .map(|((producer_id, epoch), (from, to, n))| {
                let g = GapData {
                    seq_range: None,
                    producer_range: Some(ProducerRange {
                        producer_id: producer_id.clone(),
                        producer_epoch: U64(epoch),
                        from: U64(from),
                        to: U64(to),
                    }),
                    count: GapCount::Exact(U64(n)),
                    cause: "hub_overload".into(),
                    bytes: None,
                    detail: Value::Null,
                };
                core_source(
                    &self.clock,
                    "display",
                    kinds::DISPLAY_GAP,
                    serde_json::to_value(g).unwrap_or(Value::Null),
                )
            })
            .collect();
        self.publish(gaps, None);
    }

    fn ring_push(&mut self, p: &Arc<Published>) {
        let key = (p.rec.domain.clone(), p.rec.channel.clone());
        let n = p.cache_cost;
        // An individually uncacheable record must not flush unrelated retained history.
        // The original is still journaled/delivered; its absence from the ring is explicit.
        if n > RING_CHANNEL_BYTES {
            self.evicted.note(key, (p.rec.seq.get(), p.at));
            return;
        }
        let entry = self
            .rings
            .entry(key.clone())
            .or_insert_with(|| (VecDeque::new(), 0));
        entry.0.push_back(p.clone());
        entry.1 += n;
        self.ring_order.insert(p.rec.seq.get(), key.clone());
        self.ring_bytes += n;
        self.ring_json_bytes += p.json.len();
        let mut evicted = None;
        while entry.1 > RING_CHANNEL_BYTES {
            if let Some(old) = entry.0.pop_front() {
                entry.1 -= old.cache_cost;
                self.ring_bytes -= old.cache_cost;
                self.ring_json_bytes -= old.json.len();
                self.ring_order.remove(&old.rec.seq.get());
                evicted = Some((old.rec.seq.get(), old.at));
            } else {
                break;
            }
        }
        if let Some(e) = evicted {
            self.evicted.note(key.clone(), e);
        }
        // Global cap: evict the oldest event across channels.
        while self.ring_bytes > RING_BYTES {
            let Some((seq, k)) = self.ring_order.pop_first() else {
                break;
            };
            if let Some((q, b)) = self.rings.get_mut(&k)
                && let Some(old) = q.pop_front()
            {
                debug_assert_eq!(seq, old.rec.seq.get());
                *b -= old.cache_cost;
                self.ring_bytes -= old.cache_cost;
                self.ring_json_bytes -= old.json.len();
                self.evicted.note(k, (old.rec.seq.get(), old.at));
            }
        }
    }

    fn publish(&mut self, recs: Vec<SourceRecord>, only: Option<u64>) -> Vec<Arc<Published>> {
        let mut out = Vec::with_capacity(recs.len());
        let mut map: Vec<MappingRecord> = Vec::new();
        for mut s in recs {
            if only.is_none() {
                for h in &mut self.pre {
                    h(&mut s);
                }
            }
            let rec = s.publish(U64(self.next_seq), U64(self.epoch));
            self.next_seq += 1;
            let json: Arc<str> = serde_json::to_string(&rec).unwrap_or_default().into();
            let prio = is_priority_kind(&rec.kind, &rec.payload);
            let cache_cost = cache_charge(&rec, json.len());
            let p = Arc::new(Published {
                rec,
                json,
                prio,
                at: Instant::now(),
                cache_cost,
            });
            if only.is_none() {
                self.ring_push(&p);
                self.core.apply(&p.rec);
                for o in &mut self.observers {
                    o(&p.rec);
                }
                for (d, r) in &mut self.reducers {
                    if *d == p.rec.domain && r.kinds().contains(&p.rec.kind.as_str()) {
                        r.apply(&p.rec);
                    }
                }
                match map.last_mut() {
                    Some(m)
                        if m.producer_id == p.rec.producer_id
                            && m.producer_epoch == p.rec.producer_epoch.get()
                            && m.seq_last + 1 == p.rec.seq.get()
                            && m.src_last + 1 == p.rec.source_seq.get() =>
                    {
                        // one entry = contiguous stream seqs ↔ contiguous source seqs (1:1)
                        m.seq_last = p.rec.seq.get();
                        m.src_last = p.rec.source_seq.get();
                        m.mono_last = p.rec.ts.mono_ns.get();
                    }
                    _ => map.push(MappingRecord {
                        stream_epoch: self.epoch,
                        seq_first: p.rec.seq.get(),
                        seq_last: p.rec.seq.get(),
                        producer_id: p.rec.producer_id.clone(),
                        producer_epoch: p.rec.producer_epoch.get(),
                        src_first: p.rec.source_seq.get(),
                        src_last: p.rec.source_seq.get(),
                        mono_first: p.rec.ts.mono_ns.get(),
                        mono_last: p.rec.ts.mono_ns.get(),
                    }),
                }
            }
            self.deliver(&p, only);
            out.push(p);
        }
        for m in map {
            self.mapping.record(m);
        }
        for c in &self.conns {
            c.notify.notify_one();
        }
        out
    }

    fn deliver(&self, p: &Arc<Published>, only: Option<u64>) {
        for c in self.conns.iter().filter(|c| only.is_none_or(|id| id == c.id)) {
            let mut q = lock(&c.q);
            if p.prio && c.viewer {
                if q.prio.len() >= PRIO_CAP_EVENTS || q.prio_bytes + p.json.len() > PRIO_CAP_BYTES {
                    q.prio_overflow = true;
                } else {
                    q.prio_bytes += p.json.len();
                    q.prio.push_back(p.clone());
                }
                continue;
            }
            for sub in q.subs.iter_mut().filter(|s| matches(&s.filter, &p.rec)) {
                let n = p.json.len();
                let total = self.counters.sub_bytes.load(Ordering::Acquire);
                while !sub.q.is_empty() && (sub.bytes + n > SUB_QUEUE_BYTES || total + n > TOTAL_SUB_BYTES) {
                    let Some(old) = sub.q.pop_front() else { break };
                    sub.bytes -= old.json.len();
                    self.counters
                        .sub_bytes
                        .fetch_sub(old.json.len(), Ordering::AcqRel);
                    sub.drops += 1;
                    let s = old.rec.seq.get();
                    sub.pending_gap = Some(match sub.pending_gap {
                        Some((a, _, k, c)) => (a, s, k + 1, c),
                        None => (s, s, 1, "slow_client"),
                    });
                    if self.counters.sub_bytes.load(Ordering::Acquire) + n <= TOTAL_SUB_BYTES
                        && sub.bytes + n <= SUB_QUEUE_BYTES
                    {
                        break;
                    }
                }
                sub.bytes += n;
                self.counters.sub_bytes.fetch_add(n, Ordering::AcqRel);
                sub.q.push_back(p.clone());
            }
        }
    }

    fn collect_backlog(&self, filter: &SubFilter, max: usize) -> Vec<Arc<Published>> {
        let mut all: Vec<Arc<Published>> = Vec::new();
        for (q, _) in self.rings.values() {
            all.extend(q.iter().filter(|p| matches(filter, &p.rec)).cloned());
        }
        all.sort_by_key(|p| p.rec.seq.get());
        let skip = all.len().saturating_sub(max);
        all.split_off(skip)
    }

    fn subscribe(
        &mut self,
        conn: &Arc<ConnShared>,
        sub_id: String,
        req: &Subscribe,
    ) -> Result<SubStart, ErrorCode> {
        let mut q = lock(&conn.q);
        if q.subs.len() >= MAX_SUBS_PER_CONN || self.subs_total >= MAX_SUBS_TOTAL {
            return Err(ErrorCode::Busy);
        }
        let filter = req.filter.clone();
        let oldest = self
            .rings
            .values()
            .filter_map(|(q, _)| q.front())
            .map(|p| p.rec.seq.get())
            .min();
        let mut resync = None;
        let resume = match (req.after_seq, req.stream_epoch) {
            (Some(_), Some(e)) if e.get() != self.epoch => {
                resync = Some("epoch_changed");
                None
            }
            (Some(a), _) if !req.snapshot => {
                let in_ring = oldest.is_none_or(|o| a.get() + 1 >= o);
                if in_ring {
                    let mut ev: Vec<Arc<Published>> = Vec::new();
                    for (rq, _) in self.rings.values() {
                        ev.extend(
                            rq.iter()
                                .filter(|p| p.rec.seq.get() > a.get() && matches(&filter, &p.rec))
                                .cloned(),
                        );
                    }
                    ev.sort_by_key(|p| p.rec.seq.get());
                    Some(ev)
                } else {
                    resync = Some("resume_out_of_ring");
                    None
                }
            }
            // `snapshot:false` without `after_seq`: live only, no snapshot frames (UI C-9).
            (None, _) if !req.snapshot => Some(Vec::new()),
            _ => None,
        };
        let start = match resume {
            Some(events) => SubStart::Resume { events },
            None => {
                let max = req.backlog.map_or(DEFAULT_BACKLOG, |b| b.max_events as usize);
                let mut items = vec![self.core.item(), self.core.channels_item()];
                for (d, r) in &self.reducers {
                    for state in r.snapshot(&filter) {
                        items.push(SnapshotItem {
                            domain: d.clone(),
                            channel: state["channel"].as_str().map(str::to_owned),
                            kind: format!("{d}.snapshot"),
                            state,
                        });
                    }
                }
                SubStart::Snapshot {
                    h: self.next_seq - 1,
                    backlog: self.collect_backlog(&filter, max),
                    items,
                    resync,
                }
            }
        };
        q.subs.push(Sub {
            id: sub_id,
            filter,
            max_batch_ms: req.max_batch_ms.clamp(1, 1000),
            q: VecDeque::new(),
            bytes: 0,
            drops: 0,
            pending_gap: None,
            last_flush: Instant::now() - Duration::from_secs(1),
        });
        self.subs_total += 1;
        Ok(start)
    }
}

/// A gateway-produced source record (`producer_id = "gateway"`).
pub fn core_source(clock: &HubClock, domain: &str, kind: &str, payload: Value) -> SourceRecord {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    SourceRecord {
        schema_version: loom_proto::SCHEMA_VERSION,
        session_id: "loomd".into(),
        domain: domain.into(),
        channel: None,
        kind: kind.into(),
        producer_id: "gateway".into(),
        producer_epoch: U64(1),
        source_seq: U64(NEXT.fetch_add(1, Ordering::Relaxed) as u64),
        ts: clock.ts(TsSource::Gateway),
        dir: None,
        origin: Origin::Device,
        client_id: None,
        actor_id: None,
        external_actor: None,
        original_origin: None,
        request_id: None,
        job_id: None,
        generation: None,
        purpose: None,
        risk: None,
        test_session: None,
        test_step: None,
        env: None,
        payload,
        metrics: Vec::new(),
        quality: Vec::new(),
    }
}

#[cfg(test)]
mod eviction_coverage_tests {
    use super::*;
    #[test]
    fn coverage_index_is_bounded_and_overflow_is_conservative() {
        let mut e = Evictions::default();
        let now = Instant::now();
        for i in 0..=MAX_EVICTION_KEYS {
            e.note(("can".into(), Some(format!("channel{i}"))), (i as u64 + 1, now));
        }
        assert_eq!(e.by_ring.len(), MAX_EVICTION_KEYS);
        let unrelated = SubFilter {
            domains: vec!["uart".into()],
            ..Default::default()
        };
        assert_eq!(
            e.selected(&unrelated).map(|v| v.0),
            Some(MAX_EVICTION_KEYS as u64 + 1)
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod channel_state_tests {
    use super::*;
    use loom_proto::event::SourceRecord;
    use loom_proto::num::U64;

    fn rec(seq: u64, kind: &str, channel: &str, payload: Value) -> PublishedRecord {
        let src: SourceRecord = serde_json::from_value(json!({
            "schema_version": 1, "session_id": "s", "domain": kind.split('.').next().unwrap(),
            "channel": channel, "kind": kind, "producer_id": "p", "producer_epoch": "1",
            "source_seq": seq.to_string(),
            "ts": {"mono_ns": "1", "wall_ns": "1", "clock_id": "t", "source": "host_read_complete"},
            "origin": "device", "payload": payload,
        }))
        .unwrap();
        src.publish(U64(seq), U64(1))
    }

    #[test]
    fn recording_lifecycle_never_replaces_capture_connectivity() {
        let mut c = CoreState::default();
        c.apply(&rec(1, "video.capture_state", "v1", json!({"capture_id": "v1", "state": "live"})));
        c.apply(&rec(2, "video.record_state", "v1", json!({"capture_id": "v1", "state": "failed"})));
        let list = c.channel_list();
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0]["state"], "live");
        assert_eq!(list[0]["kind"], "video.capture_state");
        c.apply(&rec(3, "video.capture_state", "v1", json!({"capture_id": "v1", "state": "closed"})));
        c.apply(&rec(4, "video.record_state", "v1", json!({"capture_id": "v1", "state": "finalized"})));
        assert!(c.channel_list().is_empty(), "a closed capture is not resurrected by a late record row");
    }
}
