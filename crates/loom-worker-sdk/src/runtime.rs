//! Worker process runtime: [`run_worker`] (contract 05, plan §3.3).
//!
//! Threads of a worker process:
//! * **IPC thread** (the calling thread, tokio `current_thread`): connects `ctl` and `data`, sends
//!   `Hello`, handles `Welcome`/`Reject`, reads ctl frames and routes them. `Latch` is handled
//!   right there — the [`LatchFlag`] is set and `LatchAck` queued before anything else — and a
//!   priority `Input` goes to a one-slot mailbox ahead of the request queue. It also writes the
//!   ctl lane (priority events first, then replies/acks/health), writes the data lane under byte
//!   credit, and sends `Health` every second.
//! * **Dispatcher thread** (tokio `current_thread`): runs the [`Worker`] callbacks one at a time
//!   and a heartbeat task. A callback that blocks the thread stops the heartbeat, the IPC thread
//!   then stops sending `Health`, and the gateway's watchdog sees the worker as stalled.
//! * Domain threads: talk to the SDK only through the non-blocking [`Emitter`].
//!
//! Queues (all bounded; overflow policy in brackets):
//! * priority events (Q6): 256 KiB / 4096 records [`Emitter::priority` returns `Full`].
//! * screen mirror (Q3): 4 MiB / 8192 records, drained only with data-lane credit (Q4)
//!   [drop the screen copy, count it, report `display.gap{producer_range, cause:
//!   "ipc_backpressure"}` on ctl; `Emitter::rx` still succeeds — the journal is not affected].
//! * requests from the gateway: 256 [reply `E_WORKER_BUSY`]; priority input mailbox: 1 [reply
//!   `E_WORKER_BUSY`]; latch notices to the dispatcher: 16 [drop — the flag is already set].
//! * outgoing control messages: 512 replies + 64 urgent (acks, health) [dispatcher awaits;
//!   urgent drops are only possible when the socket is stuck].
//!
//! Gateway gone (ctl or data EOF / error): latch on at once, keep running (journal continues),
//! after the orphan time (default 30 s) call [`RunHooks::on_orphan`] (journal seal) and exit. After a
//! `Latch{on, reason:"shutdown"}` (graceful gateway stop) the EOF ends the worker without the wait.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use loom_journal::{AppendError, JournalConfig, JournalHandle, JournalWriter};
use loom_proto::envelope::RecordHealth;
use loom_proto::error::ErrorCode;
use loom_proto::event::{GapCount, ProducerRange, SourceRecord, SourceRef, WriteState, kinds};
use loom_proto::num::U64;
use loom_proto::worker::{
    Events, Health, JournalHealth, Lane, LatchAck, MAX_PAYLOAD, PriorityEvents, ProtoVersion, QueueStat,
    Reply, ReplyResult, WORKER_PROTO_MAJOR, WORKER_PROTO_MINOR, WorkerError, WorkerHello, WorkerMsg,
};
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};

use crate::clock::{Clock, SystemClock};
use crate::emit::{EmitError, EmitSink, Emitter};
use crate::transport::{
    FrameReader, FrameWriter, WireError, check_lane, connect_lane, is_request, request_call_id,
};
use crate::{JobCmd, LatchFlag, SharedWorker, Worker, WorkerCtx, WriteOutcome};

mod concurrent;

/// Priority (ctl) queue byte cap (plan Q6).
pub const PRIO_QUEUE_BYTES: usize = 256 * 1024;
/// Priority (ctl) queue record cap (plan Q6).
pub const PRIO_QUEUE_ITEMS: usize = 4096;
/// Screen mirror byte cap (plan Q3).
pub const MIRROR_BYTES: usize = 4 * 1024 * 1024;
/// Screen mirror record cap (plan Q3).
pub const MIRROR_ITEMS: usize = 8192;
/// Worker-side request queue (mirrors the gateway's Q5 count).
pub const REQUEST_QUEUE: usize = 256;
/// Outgoing reply queue.
pub const CTL_OUT_QUEUE: usize = 512;
/// Outgoing urgent queue (latch acks, health).
pub const URGENT_QUEUE: usize = 64;
/// Closed display-gap runs kept before merging.
pub const MAX_GAP_RUNS: usize = 64;
/// Data batches wait at most this long to fill (sent at once after a quiet period).
pub const BATCH_WAIT: Duration = Duration::from_millis(10);
/// An open drop run is reported at least this often.
pub const GAP_REPORT_EVERY: Duration = Duration::from_secs(1);
/// `Latch.reason` of a graceful gateway stop (no orphan wait after the following EOF).
pub const SHUTDOWN_REASON: &str = "shutdown";
/// Default orphan time after the gateway disappears.
pub const DEFAULT_ORPHAN: Duration = Duration::from_secs(30);
/// Default health interval.
pub const DEFAULT_HEALTH: Duration = Duration::from_secs(1);
/// Connect + Hello/Welcome bound.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Once a frame's first byte arrived, the rest must arrive within this.
pub const READ_DEADLINE: Duration = Duration::from_secs(5);
/// Payload budget of one batch frame (room for the envelope keys).
pub const FRAME_BUDGET: usize = MAX_PAYLOAD - 256;

/// Worker journal queue bytes (plan Q2 worker total; one journal per producer).
pub const JOURNAL_QUEUE_BYTES: u64 = 16 * 1024 * 1024;
/// Worker journal queue records (plan Q2 worker total).
pub const JOURNAL_QUEUE_ITEMS: usize = 32 * 1024;
/// Journal health/gap poll interval of the IPC thread.
pub const JOURNAL_POLL: Duration = Duration::from_millis(50);

/// Exit code: configuration / connection failure.
pub const EXIT_SETUP: i32 = 2;
/// Exit code: rejected by the gateway (e.g. protocol major mismatch).
pub const EXIT_REJECTED: i32 = 3;

/// Command line of a worker process: `… --instance <id> --producer-epoch <n> [--sock-dir <dir>]
/// [--session <id>] [--orphan-ms <ms>] [--health-ms <ms>] [--journal-dir <dir>]`. Other tokens
/// (`worker <kind>`, domain flags) are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerArgs {
    /// Instance (`uart-dut`); also the producer id.
    pub instance: String,
    /// Producer epoch (the gateway increments it on every start).
    pub producer_epoch: u64,
    /// Socket directory (Unix).
    pub sock_dir: PathBuf,
    /// Recording session id stamped on events.
    pub session_id: String,
    /// Orphan time.
    pub orphan: Duration,
    /// Health interval.
    pub health_interval: Duration,
    /// Journal session root (`--journal-dir`); empty = take `Welcome.config.journal_dir`, and
    /// without either the journal is off (`Health.journal.health = disabled`).
    pub journal_dir: String,
    /// Journal retry interval after a disk error (`--journal-retry-ms`, default 5 s).
    pub journal_retry: Option<Duration>,
}

impl WorkerArgs {
    /// Parse from tokens.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut instance = None;
        let mut epoch = None;
        let mut sock_dir = None;
        let mut session = None;
        let mut orphan = DEFAULT_ORPHAN;
        let mut health = DEFAULT_HEALTH;
        let mut journal_dir = String::new();
        let mut journal_retry = None;
        let mut it = args.into_iter();
        let ms = |v: Option<String>, f: &str| -> Result<Duration, String> {
            v.and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_millis)
                .ok_or_else(|| format!("{f} needs milliseconds"))
        };
        while let Some(a) = it.next() {
            match a.as_str() {
                "--instance" => instance = it.next(),
                "--producer-epoch" => {
                    epoch = Some(
                        it.next()
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or("--producer-epoch needs a number")?,
                    )
                }
                "--sock-dir" => sock_dir = it.next().map(PathBuf::from),
                "--session" => session = it.next(),
                "--orphan-ms" => orphan = ms(it.next(), "--orphan-ms")?,
                "--health-ms" => health = ms(it.next(), "--health-ms")?,
                "--journal-dir" => journal_dir = it.next().unwrap_or_default(),
                "--journal-retry-ms" => journal_retry = Some(ms(it.next(), "--journal-retry-ms")?),
                _ => {}
            }
        }
        let instance = instance.ok_or("--instance is required")?;
        crate::transport::validate_instance(&instance)?;
        Ok(WorkerArgs {
            instance,
            producer_epoch: epoch.ok_or("--producer-epoch is required")?,
            sock_dir: sock_dir.unwrap_or_else(crate::transport::default_sock_dir),
            session_id: session.unwrap_or_else(|| "loomd".into()),
            orphan,
            health_interval: health.max(Duration::from_millis(10)),
            journal_dir,
            journal_retry,
        })
    }

    /// Parse `std::env::args()`.
    pub fn from_env() -> Result<Self, String> {
        Self::parse(std::env::args().skip(1))
    }
}

/// Record hook (journal append).
pub type RecordHook = Arc<dyn Fn(&SourceRecord) + Send + Sync>;
/// RSS source.
pub type RssHook = Arc<dyn Fn() -> Option<u64> + Send + Sync>;
/// Journal state source.
pub type JournalHook = Arc<dyn Fn() -> JournalHealth + Send + Sync>;

/// Integration points the lead wires (journal) and tests use.
#[derive(Default)]
pub struct RunHooks {
    /// Every record the worker emits, before lane routing (journal `try_append`). Must not block.
    pub on_record: Option<RecordHook>,
    /// Called once when the orphan time ends, before exit (journal seal).
    pub on_orphan: Option<Box<dyn FnOnce() + Send>>,
    /// RSS source; default: Linux `/proc/self/status`, otherwise unknown (0).
    pub rss: Option<RssHook>,
    /// Journal state for `Health`; default: `disabled` (no journal wired).
    pub journal: Option<JournalHook>,
}

impl std::fmt::Debug for RunHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunHooks")
            .field("on_record", &self.on_record.is_some())
            .field("on_orphan", &self.on_orphan.is_some())
            .field("rss", &self.rss.is_some())
            .field("journal", &self.journal.is_some())
            .finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone)]
struct DropRun {
    channel: Option<String>,
    from: u64,
    to: u64,
    count: u64,
    bytes: u64,
    since: Instant,
}

#[derive(Debug, Default)]
struct Prio {
    q: VecDeque<(SourceRecord, usize)>,
    bytes: usize,
    pushed: u64,
    last: Option<SourceRef>,
    drops: u64,
    max_depth: usize,
}

#[derive(Debug, Default)]
struct Mirror {
    q: VecDeque<(SourceRecord, usize)>,
    bytes: usize,
    open: Option<DropRun>,
    closed: VecDeque<DropRun>,
    drops: u64,
    max_depth: usize,
}

impl Mirror {
    fn close_run(&mut self) {
        let Some(run) = self.open.take() else { return };
        if self.closed.len() >= MAX_GAP_RUNS
            && let Some(last) = self.closed.back_mut()
        {
            // Bounded: widen the last run (count stays exact).
            last.from = last.from.min(run.from);
            last.to = last.to.max(run.to);
            last.count += run.count;
            last.bytes += run.bytes;
        } else {
            self.closed.push_back(run);
        }
    }

    fn drop_record(&mut self, rec: &SourceRecord, size: usize) {
        self.drops += 1;
        let seq = rec.source_seq.0;
        if let Some(run) = self.open.as_mut()
            && run.channel == rec.channel
        {
            run.to = run.to.max(seq);
            run.from = run.from.min(seq);
            run.count += 1;
            run.bytes += size as u64;
            return;
        }
        self.close_run();
        self.open = Some(DropRun {
            channel: rec.channel.clone(),
            from: seq,
            to: seq,
            count: 1,
            bytes: size as u64,
            since: Instant::now(),
        });
    }
}

struct Shared {
    latch: LatchFlag,
    prio: Mutex<Prio>,
    mirror: Mutex<Mirror>,
    ctl_wake: Notify,
    data_wake: Notify,
    credit: AtomicI64,
    credit_wake: Notify,
    beat_ms: AtomicU64,
    start: Instant,
    on_record: Option<RecordHook>,
    /// Worker-side journal (contract 06 §6.1): every record, independent of credit/gateway.
    journal: Option<JournalHandle>,
}

impl Shared {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// `EmitSink` of a worker process: priority → ctl queue, data → screen mirror.
struct IpcSink {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for IpcSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcSink").finish_non_exhaustive()
    }
}

fn encoded_len(rec: &SourceRecord) -> usize {
    rmp_serde::to_vec_named(rec).map_or(usize::MAX, |v| v.len())
}

impl EmitSink for IpcSink {
    fn try_push(&self, lane: Lane, record: SourceRecord) -> Result<(), EmitError> {
        let s = &self.shared;
        if let Some(h) = &s.on_record {
            h(&record);
        }
        if let Some(j) = &s.journal {
            // Never blocks. `Full` / disk errors are recorded by the journal itself (gap + health
            // failed) and reported on ctl by the journal monitor; the screen copy still goes on.
            if let Err(e) = j.try_append(&record)
                && !matches!(e, AppendError::Full { .. } | AppendError::Closed)
            {
                eprintln!("loom-worker-sdk: journal append: {e}");
            }
        }
        let size = encoded_len(&record);
        match lane {
            Lane::Ctl => {
                let mut p = lock(&s.prio);
                if size > FRAME_BUDGET || p.q.len() >= PRIO_QUEUE_ITEMS || p.bytes + size > PRIO_QUEUE_BYTES {
                    p.drops += 1;
                    return Err(EmitError::Full);
                }
                p.last = Some(record.source_ref());
                p.pushed += 1;
                p.bytes += size;
                p.q.push_back((record, size));
                p.max_depth = p.max_depth.max(p.q.len());
                drop(p);
                s.ctl_wake.notify_one();
                Ok(())
            }
            Lane::Data => {
                let mut m = lock(&s.mirror);
                if size > FRAME_BUDGET || m.q.len() >= MIRROR_ITEMS || m.bytes + size > MIRROR_BYTES {
                    m.drop_record(&record, size);
                    // Screen copy dropped; the device read is not slowed down (journal has it).
                    return Ok(());
                }
                m.close_run();
                m.bytes += size;
                m.q.push_back((record, size));
                m.max_depth = m.max_depth.max(m.q.len());
                drop(m);
                s.data_wake.notify_one();
                Ok(())
            }
        }
    }
}

/// Reply value of a write: `{write_state, detail, error?}` (see [`decode_write_outcome`]).
pub fn encode_write_outcome(o: &WriteOutcome) -> Value {
    json!({
        "write_state": o.state,
        "detail": o.detail,
        "error": o.error,
    })
}

/// Inverse of [`encode_write_outcome`]. An `Err` reply means the write did not reach the domain
/// (`failed`); an undecodable reply is `outcome_unknown` (never invent success).
pub fn decode_write_outcome(r: ReplyResult) -> WriteOutcome {
    match r {
        ReplyResult::Ok(v) => {
            let state = v
                .get("write_state")
                .cloned()
                .and_then(|s| serde_json::from_value::<WriteState>(s).ok());
            let error = v
                .get("error")
                .cloned()
                .and_then(|e| serde_json::from_value::<Option<WorkerError>>(e).ok())
                .flatten();
            match state {
                Some(state) => WriteOutcome {
                    state,
                    detail: v.get("detail").cloned().unwrap_or(Value::Null),
                    error,
                },
                None => WriteOutcome {
                    state: WriteState::OutcomeUnknown,
                    detail: v,
                    error: Some(WorkerError {
                        code: ErrorCode::Internal,
                        message: "write reply without write_state".into(),
                        detail: Value::Null,
                    }),
                },
            }
        }
        ReplyResult::Err(e) => WriteOutcome {
            state: WriteState::Failed,
            detail: Value::Null,
            error: Some(e),
        },
    }
}

enum ReqKind {
    Call(loom_proto::worker::Call),
    Write(loom_proto::worker::Write),
    Job(JobCmd),
    Input(loom_proto::worker::Input),
    Lease,
    LeaseEnd(loom_proto::worker::InputLeaseEnd),
}

struct Req {
    id: u64,
    kind: ReqKind,
}

fn busy(code: ErrorCode, msg: &str) -> ReplyResult {
    ReplyResult::Err(WorkerError {
        code,
        message: msg.into(),
        detail: Value::Null,
    })
}

/// Run the worker process: parse `std::env::args()`, connect, serve, exit. Never returns.
pub fn run_worker<W: Worker>(worker: W) -> ! {
    match WorkerArgs::from_env() {
        Ok(args) => run_worker_with(worker, args, RunHooks::default()),
        Err(e) => {
            eprintln!("loom-worker-sdk: {e}");
            std::process::exit(EXIT_SETUP)
        }
    }
}

/// Run with explicit arguments and hooks. Never returns.
pub fn run_worker_with<W: Worker>(worker: W, args: WorkerArgs, hooks: RunHooks) -> ! {
    run_driver(Serial(worker), args, hooks)
}

/// 공유 worker의 동시 콜백 런타임을 실행한다. 프로세스 main에서 호출한다.
pub fn run_shared_worker<W: SharedWorker>(worker: Arc<W>) -> ! {
    match WorkerArgs::from_env() {
        Ok(args) => run_shared_worker_with(worker, args, RunHooks::default()),
        Err(e) => {
            eprintln!("loom-worker-sdk: {e}");
            std::process::exit(EXIT_SETUP)
        }
    }
}

/// 공유 worker를 명시한 실행 인자·훅으로 실행한다.
pub fn run_shared_worker_with<W: SharedWorker>(worker: Arc<W>, args: WorkerArgs, hooks: RunHooks) -> ! {
    run_driver(Concurrent(worker), args, hooks)
}

struct Dispatch {
    cx: WorkerCtx,
    shared: Arc<Shared>,
    req_rx: mpsc::Receiver<Req>,
    pin_rx: mpsc::Receiver<Req>,
    latch_rx: mpsc::Receiver<bool>,
    out_tx: mpsc::Sender<WorkerMsg>,
    beat_every: Duration,
    tick: Option<Duration>,
}
trait Driver: Send + 'static {
    const KIND: &'static str;
    const DOMAIN: &'static str;
    fn dispatch(self, args: Dispatch);
}
struct Serial<W>(W);
impl<W: Worker> Driver for Serial<W> {
    const KIND: &'static str = W::KIND;
    const DOMAIN: &'static str = W::DOMAIN;
    fn dispatch(self, d: Dispatch) {
        dispatcher(
            self.0,
            d.cx,
            d.shared,
            d.req_rx,
            d.pin_rx,
            d.latch_rx,
            d.out_tx,
            d.beat_every,
        );
    }
}
struct Concurrent<W>(Arc<W>);
impl<W: SharedWorker> Driver for Concurrent<W> {
    const KIND: &'static str = W::KIND;
    const DOMAIN: &'static str = W::DOMAIN;
    fn dispatch(self, d: Dispatch) {
        concurrent::dispatcher(self.0, d);
    }
}

fn run_driver<D: Driver>(worker: D, args: WorkerArgs, hooks: RunHooks) -> ! {
    // Same crash semantics as the release profile (`panic = "abort"`): a panic anywhere ends the
    // process, the gateway sees the exit and restarts the worker.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        std::process::abort();
    }));
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("loom-worker-sdk: runtime: {e}");
            std::process::exit(EXIT_SETUP)
        }
    };
    let code = rt.block_on(ipc_main(worker, args, hooks));
    std::process::exit(code)
}

fn rss_default() -> Option<u64> {
    crate::memory::process_rss_bytes()
}

fn threads_default() -> u32 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("Threads:"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
            })
            .unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

async fn ipc_main<D: Driver>(worker: D, args: WorkerArgs, hooks: RunHooks) -> i32 {
    let RunHooks {
        on_record,
        on_orphan,
        rss,
        journal,
    } = hooks;
    let connect = async {
        let ctl = connect_lane(&args.sock_dir, &args.instance, Lane::Ctl, HELLO_TIMEOUT).await?;
        let data = connect_lane(&args.sock_dir, &args.instance, Lane::Data, HELLO_TIMEOUT).await?;
        Ok::<_, std::io::Error>((ctl, data))
    };
    let ((ctl_r, ctl_w), (data_r, data_w)) = match connect.await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("loom-worker-sdk: connect {}: {e}", args.sock_dir.display());
            return EXIT_SETUP;
        }
    };
    let mut ctl_r = FrameReader::new(ctl_r);
    let mut ctl_w = FrameWriter::new(ctl_w);
    let hello = WorkerMsg::Hello(WorkerHello {
        proto: ProtoVersion {
            major: WORKER_PROTO_MAJOR,
            minor: WORKER_PROTO_MINOR,
        },
        kind: D::KIND.to_owned(),
        instance: args.instance.clone(),
        producer_id: args.instance.clone(),
        producer_epoch: U64(args.producer_epoch),
        pid: std::process::id(),
        build: env!("CARGO_PKG_VERSION").to_owned(),
        schema_hash: String::new(),
        codecs: vec!["msgpack-map".into()],
        domains: vec![D::DOMAIN.to_owned()],
        capabilities: Vec::new(),
        journal_dir: args.journal_dir.clone(),
        resume_nonce: None,
    });
    if let Err(e) = ctl_w.write(&hello).await {
        eprintln!("loom-worker-sdk: hello: {e}");
        return EXIT_SETUP;
    }
    let welcome = match tokio::time::timeout(HELLO_TIMEOUT, ctl_r.read(Some(READ_DEADLINE))).await {
        Ok(Ok(Some((WorkerMsg::Welcome(w), _)))) => w,
        Ok(Ok(Some((WorkerMsg::Reject(r), _)))) => {
            eprintln!("loom-worker-sdk: rejected: {}", r.reason);
            return EXIT_REJECTED;
        }
        other => {
            eprintln!("loom-worker-sdk: no welcome: {other:?}");
            return EXIT_SETUP;
        }
    };

    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let journal_root = if args.journal_dir.is_empty() {
        welcome
            .config
            .get("journal_dir")
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        Some(args.journal_dir.clone())
    };
    let mut journal_error: Option<String> = None;
    let mut journal_writer = journal_root.and_then(|root| {
        let mut jc = JournalConfig::new(root, args.instance.clone(), args.producer_epoch);
        jc.clock_id = clock.clock_id().to_owned();
        // One journal per producer (seq order); the worker total of plan Q2.
        jc.queue_bytes = JOURNAL_QUEUE_BYTES;
        jc.queue_items = JOURNAL_QUEUE_ITEMS;
        if let Some(r) = args.journal_retry {
            jc.retry_interval = r;
        }
        match JournalWriter::spawn(jc) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("loom-worker-sdk: journal: {e}");
                journal_error = Some(e.to_string());
                None
            }
        }
    });
    let journal_handle = journal_writer.as_ref().map(JournalWriter::handle);

    let shared = Arc::new(Shared {
        latch: LatchFlag::new(),
        prio: Mutex::new(Prio::default()),
        mirror: Mutex::new(Mirror::default()),
        ctl_wake: Notify::new(),
        data_wake: Notify::new(),
        credit: AtomicI64::new(i64::try_from(welcome.budgets.credit_bytes.0).unwrap_or(i64::MAX)),
        credit_wake: Notify::new(),
        beat_ms: AtomicU64::new(0),
        start: Instant::now(),
        on_record,
        journal: journal_handle.clone(),
    });
    shared.latch.set(welcome.latch);
    let sink: Arc<dyn EmitSink> = Arc::new(IpcSink {
        shared: shared.clone(),
    });
    let base = Emitter::new(
        args.session_id.clone(),
        D::DOMAIN,
        args.instance.clone(),
        U64(args.producer_epoch),
        1,
        "",
        sink,
        clock.clone(),
    );
    let gap_emitter = base.clone();
    let sys_emitter = base.clone();
    let factory_base = base.clone();
    let cx = WorkerCtx::new(
        shared.latch.clone(),
        clock.clone(),
        Arc::new(move |ch: &str| factory_base.for_channel(ch)),
    );

    let (req_tx, req_rx) = mpsc::channel::<Req>(REQUEST_QUEUE);
    let (pin_tx, pin_rx) = mpsc::channel::<Req>(1);
    let (latch_tx, latch_rx) = mpsc::channel::<bool>(16);
    let (out_tx, out_rx) = mpsc::channel::<WorkerMsg>(CTL_OUT_QUEUE);
    let (urgent_tx, urgent_rx) = mpsc::channel::<WorkerMsg>(URGENT_QUEUE);
    let gone = Arc::new(Notify::new());

    // Dispatcher thread.
    {
        let shared = shared.clone();
        let out_tx = out_tx.clone();
        let beat_every = (args.health_interval / 4).max(Duration::from_millis(5));
        let spawned = std::thread::Builder::new()
            .name("loom-worker-dispatch".into())
            .spawn(move || {
                worker.dispatch(Dispatch {
                    cx,
                    shared,
                    req_rx,
                    pin_rx,
                    latch_rx,
                    out_tx,
                    beat_every,
                    tick: welcome
                        .config
                        .get("tick_ms")
                        .and_then(Value::as_u64)
                        .filter(|ms| *ms > 0)
                        .map(Duration::from_millis)
                        .filter(|period| Instant::now().checked_add(*period).is_some()),
                })
            });
        if let Err(e) = spawned {
            eprintln!("loom-worker-sdk: dispatcher thread: {e}");
            return EXIT_SETUP;
        }
    }

    // ctl writer.
    {
        let shared = shared.clone();
        let gone = gone.clone();
        tokio::spawn(async move {
            if let Err(e) = ctl_writer(ctl_w, shared, out_rx, urgent_rx).await {
                eprintln!("loom-worker-sdk: ctl write: {e}");
            }
            gone.notify_one();
        });
    }
    // data writer.
    {
        let shared = shared.clone();
        let gone = gone.clone();
        tokio::spawn(async move {
            if let Err(e) = data_writer(FrameWriter::new(data_w), shared, gap_emitter).await {
                eprintln!("loom-worker-sdk: data write: {e}");
            }
            gone.notify_one();
        });
    }
    // data reader (credits).
    {
        let shared = shared.clone();
        let gone = gone.clone();
        tokio::spawn(async move {
            let mut r = FrameReader::new(data_r);
            loop {
                match r.read(Some(READ_DEADLINE)).await {
                    Ok(Some((WorkerMsg::Credit(c), _))) => {
                        let add = i64::try_from(c.bytes.0).unwrap_or(i64::MAX);
                        let _ = shared
                            .credit
                            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                                Some(v.saturating_add(add))
                            });
                        shared.credit_wake.notify_one();
                    }
                    Ok(Some((m, _))) => {
                        eprintln!(
                            "loom-worker-sdk: unexpected {} on data lane",
                            crate::transport::msg_name(&m)
                        );
                        break;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("loom-worker-sdk: data read: {e}");
                        break;
                    }
                }
            }
            gone.notify_one();
        });
    }
    let jsrc = JournalSource {
        hook: journal,
        handle: journal_handle.clone(),
        error: journal_error.clone(),
    };
    // Journal monitor: health changes → `record.health` + an immediate Health; finished losses →
    // `record.gap` (ctl). The gateway turns `failed` into the agent-write block (A12).
    {
        let shared = shared.clone();
        let urgent_tx = urgent_tx.clone();
        let clock = clock.clone();
        let rss = rss.clone();
        let jsrc = jsrc.clone();
        tokio::spawn(async move {
            journal_monitor(shared, sys_emitter, jsrc, clock, rss, urgent_tx).await;
        });
    }
    // Health.
    {
        let shared = shared.clone();
        let urgent_tx = urgent_tx.clone();
        let interval = args.health_interval;
        let clock = clock.clone();
        let jsrc = jsrc.clone();
        tokio::spawn(async move {
            let stale_after = u64::try_from((interval * 2).as_millis()).unwrap_or(u64::MAX) + 500;
            loop {
                let beat = shared.beat_ms.load(Ordering::Acquire);
                // Dispatcher blocked (callback stuck): stay silent so the gateway sees a stall.
                if shared.now_ms().saturating_sub(beat) <= stale_after {
                    let h = health(&shared, &*clock, rss.as_deref(), &jsrc);
                    let _ = urgent_tx.try_send(WorkerMsg::Health(h));
                }
                tokio::time::sleep(interval).await;
            }
        });
    }

    // A `Latch{on, reason:"shutdown"}` announces a graceful gateway stop: the EOF that follows
    // ends the worker at once (no orphan wait). Any other EOF is a gateway crash.
    let shutdown_announced = std::sync::atomic::AtomicBool::new(false);
    // ctl reader on this task.
    let reader = async {
        let mut n: u64 = 0;
        loop {
            let msg = match ctl_r.read(Some(READ_DEADLINE)).await {
                Ok(Some((m, _))) => m,
                Ok(None) => return,
                Err(e) => {
                    eprintln!("loom-worker-sdk: ctl read: {e}");
                    return;
                }
            };
            if check_lane(&msg, Lane::Ctl).is_err() {
                eprintln!(
                    "loom-worker-sdk: {} on ctl lane",
                    crate::transport::msg_name(&msg)
                );
                return;
            }
            if is_request(&msg) {
                n += 1;
            }
            let id = if welcome.accepted_minor >= 1 {
                request_call_id(&msg).unwrap_or(n)
            } else {
                n
            };
            let kind = match msg {
                WorkerMsg::Latch(l) => {
                    if l.on && l.reason.as_deref() == Some(SHUTDOWN_REASON) {
                        shutdown_announced.store(true, Ordering::Release);
                    }
                    shared.latch.set(l.on);
                    let _ = urgent_tx.try_send(WorkerMsg::LatchAck(LatchAck {
                        latch_seq: l.latch_seq,
                    }));
                    let _ = latch_tx.try_send(l.on);
                    continue;
                }
                WorkerMsg::Input(i) if i.priority => {
                    if let Err(mpsc::error::TrySendError::Full(r) | mpsc::error::TrySendError::Closed(r)) =
                        pin_tx.try_send(Req {
                            id,
                            kind: ReqKind::Input(i),
                        })
                    {
                        let _ = out_tx.try_send(reply(
                            r.id,
                            busy(ErrorCode::WorkerBusy, "priority input busy"),
                            None,
                        ));
                    }
                    continue;
                }
                WorkerMsg::Call(mut c) => {
                    c.call_id = U64(id);
                    ReqKind::Call(c)
                }
                WorkerMsg::Write(mut w) => {
                    w.call_id = U64(id);
                    ReqKind::Write(w)
                }
                WorkerMsg::JobStart(j) => ReqKind::Job(JobCmd::Start(j)),
                WorkerMsg::JobUpdate(j) => ReqKind::Job(JobCmd::Update(j)),
                WorkerMsg::JobRenew(j) => ReqKind::Job(JobCmd::Renew(j)),
                WorkerMsg::JobStop(j) => ReqKind::Job(JobCmd::Stop(j)),
                WorkerMsg::Input(i) => ReqKind::Input(i),
                WorkerMsg::InputLease(_) => ReqKind::Lease,
                WorkerMsg::InputLeaseEnd(end) => {
                    // ctl reader는 직렬 Worker의 긴 콜백 뒤에서도 STOP을 읽을 수 있어야 한다.
                    // 응답 없는 종료 통지는 일반 유한 큐에 non-blocking으로 넣는다.
                    if req_tx
                        .try_send(Req {
                            id: 0,
                            kind: ReqKind::LeaseEnd(end),
                        })
                        .is_err()
                    {
                        eprintln!("loom-worker-sdk: lease 종료 통지를 수락할 수 없어 worker 종료");
                        std::process::abort();
                    }
                    continue;
                }
                other => {
                    eprintln!(
                        "loom-worker-sdk: unexpected {} from gateway",
                        crate::transport::msg_name(&other)
                    );
                    return;
                }
            };
            if let Err(mpsc::error::TrySendError::Full(r) | mpsc::error::TrySendError::Closed(r)) =
                req_tx.try_send(Req { id, kind })
            {
                let _ = out_tx.try_send(reply(
                    r.id,
                    busy(ErrorCode::WorkerBusy, "worker request queue full"),
                    None,
                ));
            }
        }
    };
    tokio::select! {
        () = reader => {},
        () = gone.notified() => {},
    }

    // Gateway gone: latch at once. Graceful stop (shutdown latch seen) → seal and exit now;
    // otherwise (crash) the journal keeps running and we exit after the orphan time.
    shared.latch.set(true);
    let _ = latch_tx.try_send(true);
    if shutdown_announced.load(Ordering::Acquire) {
        eprintln!("loom-worker-sdk: gateway shutting down; sealing journal and exiting");
    } else {
        eprintln!(
            "loom-worker-sdk: gateway connection lost; latched, orphan exit in {:?}",
            args.orphan
        );
        tokio::time::sleep(args.orphan).await;
    }
    // Seal the journal (drain, fsync, rename) before exit.
    if let Some(w) = journal_writer.take() {
        let rep = w.close();
        if !rep.gaps.is_empty() {
            eprintln!(
                "loom-worker-sdk: journal closed with {} unreported gap(s)",
                rep.gaps.len()
            );
        }
    }
    if let Some(f) = on_orphan {
        f();
    }
    0
}

/// Where `Health.journal` comes from.
#[derive(Clone)]
struct JournalSource {
    hook: Option<JournalHook>,
    handle: Option<JournalHandle>,
    error: Option<String>,
}

impl JournalSource {
    fn health(&self) -> JournalHealth {
        if let Some(f) = &self.hook {
            return f();
        }
        match (&self.handle, &self.error) {
            (Some(h), _) => h.status().to_proto(),
            // Configured but could not start: originals are not recorded.
            (None, Some(_)) => JournalHealth {
                written_through: None,
                durable_through: None,
                health: RecordHealth::Failed,
            },
            (None, None) => JournalHealth {
                written_through: None,
                durable_through: None,
                health: RecordHealth::Disabled,
            },
        }
    }
}

fn health_name(h: RecordHealth) -> &'static str {
    match h {
        RecordHealth::Ok => "ok",
        RecordHealth::Degraded => "degraded",
        RecordHealth::Failed => "failed",
        RecordHealth::Disabled => "disabled",
    }
}

async fn journal_monitor(
    shared: Arc<Shared>,
    sys: Emitter,
    src: JournalSource,
    clock: Arc<dyn Clock>,
    rss: Option<RssHook>,
    urgent_tx: mpsc::Sender<WorkerMsg>,
) {
    let mut last = if src.handle.is_none() && src.error.is_none() {
        RecordHealth::Disabled
    } else {
        RecordHealth::Ok
    };
    // Gaps that could not be emitted yet (priority queue full) — bounded, merged when full.
    let mut pending: VecDeque<loom_journal::GapReport> = VecDeque::new();
    loop {
        let jh = src.health();
        let last_error = src
            .handle
            .as_ref()
            .and_then(|h| h.status().last_error)
            .or_else(|| src.error.clone());
        if jh.health != last {
            let payload = json!({
                "health": health_name(jh.health),
                "previous": health_name(last),
                "written_through": jh.written_through,
                "durable_through": jh.durable_through,
                "error": last_error,
            });
            if sys.system(kinds::RECORD_HEALTH, payload).is_ok() {
                last = jh.health;
                let h = health(&shared, &*clock, rss.as_deref(), &src);
                let _ = urgent_tx.try_send(WorkerMsg::Health(h));
            }
        }
        if let Some(h) = &src.handle {
            for g in h.take_gaps() {
                if pending.len() >= MAX_GAP_RUNS
                    && let Some(back) = pending.back_mut()
                {
                    back.to = match (back.to, g.to) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    };
                    back.count = GapCount::Unknown;
                } else {
                    pending.push_back(g);
                }
            }
        }
        while let Some(g) = pending.front() {
            let payload = serde_json::to_value(g.to_gap_data()).unwrap_or(Value::Null);
            let emitter = g.class.as_ref().map(|class| sys.for_channel(class.clone()));
            if emitter
                .as_ref()
                .unwrap_or(&sys)
                .system(kinds::RECORD_GAP, payload)
                .is_err()
            {
                break;
            }
            pending.pop_front();
        }
        tokio::time::sleep(JOURNAL_POLL).await;
    }
}

fn reply(id: u64, result: ReplyResult, covers: Option<SourceRef>) -> WorkerMsg {
    WorkerMsg::Reply(Reply {
        call_id: U64(id),
        result,
        covers,
    })
}

fn health(
    s: &Shared,
    clock: &dyn Clock,
    rss: Option<&(dyn Fn() -> Option<u64> + Send + Sync)>,
    journal: &JournalSource,
) -> Health {
    let mut queues = std::collections::BTreeMap::new();
    {
        let p = lock(&s.prio);
        queues.insert(
            "ctl_tx".to_owned(),
            QueueStat {
                depth: u32::try_from(p.q.len()).unwrap_or(u32::MAX),
                bytes: U64(p.bytes as u64),
                max_depth: u32::try_from(p.max_depth).unwrap_or(u32::MAX),
                drops: U64(p.drops),
            },
        );
    }
    {
        let m = lock(&s.mirror);
        queues.insert(
            "mirror".to_owned(),
            QueueStat {
                depth: u32::try_from(m.q.len()).unwrap_or(u32::MAX),
                bytes: U64(m.bytes as u64),
                max_depth: u32::try_from(m.max_depth).unwrap_or(u32::MAX),
                drops: U64(m.drops),
            },
        );
    }
    let rss_bytes = match rss {
        Some(f) => f(),
        None => rss_default(),
    }
    .unwrap_or(0);
    if let Some(h) = &journal.handle {
        queues.insert("journal".to_owned(), h.status().queue_stat());
    }
    let journal = journal.health();
    Health {
        mono_ns: U64(clock.mono_ns()),
        rss_bytes: U64(rss_bytes),
        commit_bytes: None,
        queues,
        journal,
        threads: threads_default(),
    }
}

async fn flush_prio(w: &mut FrameWriter, s: &Shared) -> Result<(), WireError> {
    loop {
        let records = {
            let mut p = lock(&s.prio);
            let mut total = 0usize;
            let mut out = Vec::new();
            while let Some((_, size)) = p.q.front() {
                if !out.is_empty() && total + size > FRAME_BUDGET {
                    break;
                }
                total += *size;
                if let Some((rec, sz)) = p.q.pop_front() {
                    p.bytes -= sz;
                    out.push(rec);
                }
            }
            out
        };
        if records.is_empty() {
            return Ok(());
        }
        w.write(&WorkerMsg::PriorityEvents(PriorityEvents { records }))
            .await?;
    }
}

async fn ctl_writer(
    mut w: FrameWriter,
    s: Arc<Shared>,
    mut out_rx: mpsc::Receiver<WorkerMsg>,
    mut urgent_rx: mpsc::Receiver<WorkerMsg>,
) -> Result<(), WireError> {
    loop {
        let msg = tokio::select! {
            biased;
            m = urgent_rx.recv() => m,
            m = out_rx.recv() => m,
            () = s.ctl_wake.notified() => None,
        };
        // Priority events pushed before this message go first (publish-before-reply).
        flush_prio(&mut w, &s).await?;
        if let Some(m) = msg {
            match w.write(&m).await {
                Ok(_) => {}
                Err(WireError::Codec(e)) => {
                    // Oversize reply: answer with an error instead (the connection stays fine).
                    if let WorkerMsg::Reply(r) = &m {
                        let err = reply(
                            r.call_id.0,
                            busy(ErrorCode::Internal, &format!("reply not sendable: {e}")),
                            r.covers.clone(),
                        );
                        w.write(&err).await?;
                    } else {
                        eprintln!("loom-worker-sdk: dropped unsendable ctl message: {e}");
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

fn take_gap_runs(s: &Shared, force_open: bool) -> Vec<DropRun> {
    let mut m = lock(&s.mirror);
    if m.open
        .as_ref()
        .is_some_and(|r| force_open || r.since.elapsed() >= GAP_REPORT_EVERY)
    {
        m.close_run();
    }
    m.closed.drain(..).collect()
}

fn report_gaps(s: &Shared, gap: &Emitter, runs: Vec<DropRun>) {
    let mut left = runs.into_iter();
    for run in left.by_ref() {
        let em = gap.for_channel(run.channel.clone().unwrap_or_default());
        let range = ProducerRange {
            producer_id: gap.producer_id().to_owned(),
            producer_epoch: gap.producer_epoch(),
            from: U64(run.from),
            to: U64(run.to),
        };
        if em
            .display_gap(
                range,
                GapCount::Exact(U64(run.count)),
                "ipc_backpressure",
                Some(run.bytes),
            )
            .is_err()
        {
            // Priority queue full: keep the run for the next round.
            let mut m = lock(&s.mirror);
            m.closed.push_front(run);
            break;
        }
    }
    let rest: Vec<DropRun> = left.collect();
    if !rest.is_empty() {
        let mut m = lock(&s.mirror);
        for r in rest.into_iter().rev() {
            m.closed.push_front(r);
        }
    }
}

async fn data_writer(mut w: FrameWriter, s: Arc<Shared>, gap: Emitter) -> Result<(), WireError> {
    let mut last_send: Option<Instant> = None;
    loop {
        let runs = take_gap_runs(&s, false);
        if !runs.is_empty() {
            report_gaps(&s, &gap, runs);
        }
        let (len, bytes, first) = {
            let m = lock(&s.mirror);
            (m.q.len(), m.bytes, m.q.front().map(|(_, sz)| *sz))
        };
        let Some(first) = first else {
            tokio::select! {
                () = s.data_wake.notified() => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            continue;
        };
        let _ = len;
        let credit = s.credit.load(Ordering::Acquire);
        if credit < i64::try_from(first).unwrap_or(i64::MAX) {
            tokio::select! {
                () = s.credit_wake.notified() => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            continue;
        }
        // Adaptive batching: right away after a quiet period, else gather for ≤ 10 ms.
        if let Some(t) = last_send {
            let since = t.elapsed();
            if since < BATCH_WAIT && bytes < FRAME_BUDGET {
                tokio::time::sleep(BATCH_WAIT - since).await;
            }
        }
        let budget = usize::try_from(s.credit.load(Ordering::Acquire).max(0))
            .unwrap_or(usize::MAX)
            .min(FRAME_BUDGET);
        let records = {
            let mut m = lock(&s.mirror);
            let mut total = 0usize;
            let mut out = Vec::new();
            while let Some((_, size)) = m.q.front() {
                if total + size > budget {
                    break;
                }
                total += *size;
                if let Some((rec, sz)) = m.q.pop_front() {
                    m.bytes -= sz;
                    out.push(rec);
                }
            }
            out
        };
        if records.is_empty() {
            continue;
        }
        let first_source_seq = records[0].source_seq;
        let count = u32::try_from(records.len()).unwrap_or(u32::MAX);
        let n = w
            .write(&WorkerMsg::Events(Events {
                first_source_seq,
                count,
                records,
            }))
            .await?;
        s.credit
            .fetch_sub(i64::try_from(n).unwrap_or(i64::MAX), Ordering::AcqRel);
        last_send = Some(Instant::now());
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatcher<W: Worker>(
    mut worker: W,
    cx: WorkerCtx,
    shared: Arc<Shared>,
    mut req_rx: mpsc::Receiver<Req>,
    mut pin_rx: mpsc::Receiver<Req>,
    mut latch_rx: mpsc::Receiver<bool>,
    out_tx: mpsc::Sender<WorkerMsg>,
    beat_every: Duration,
) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("loom-worker-sdk: dispatcher runtime: {e}");
            std::process::abort();
        }
    };
    rt.block_on(async move {
        {
            let shared = shared.clone();
            tokio::spawn(async move {
                loop {
                    shared.beat_ms.store(shared.now_ms(), Ordering::Release);
                    tokio::time::sleep(beat_every).await;
                }
            });
        }
        loop {
            let req = tokio::select! {
                biased;
                Some(on) = latch_rx.recv() => {
                    worker.on_latch(on);
                    continue;
                }
                Some(r) = pin_rx.recv() => r,
                r = req_rx.recv() => match r {
                    Some(r) => r,
                    None => break,
                },
            };
            let before = lock(&shared.prio).pushed;
            let result = match req.kind {
                ReqKind::Call(c) => match worker.on_call(&cx, c).await {
                    Ok(v) => ReplyResult::Ok(v),
                    Err(e) => ReplyResult::Err(e),
                },
                ReqKind::Write(x) => ReplyResult::Ok(encode_write_outcome(&worker.on_write(&cx, x).await)),
                ReqKind::Job(j) => match worker.on_job(&cx, j).await {
                    Ok(v) => ReplyResult::Ok(v),
                    Err(e) => ReplyResult::Err(e),
                },
                ReqKind::Input(i) => match worker.on_input(&cx, i).await {
                    Ok(()) => ReplyResult::Ok(Value::Null),
                    Err(e) => ReplyResult::Err(e),
                },
                ReqKind::Lease => ReplyResult::Ok(Value::Null),
                ReqKind::LeaseEnd(end) => {
                    worker.on_input_lease_end(&cx, end).await;
                    continue;
                }
            };
            let covers = {
                let p = lock(&shared.prio);
                if p.pushed != before { p.last.clone() } else { None }
            };
            if out_tx.send(reply(req.id, result, covers)).await.is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn args_parse() {
        let a = WorkerArgs::parse(
            [
                "worker",
                "uart",
                "--instance",
                "uart-dut",
                "--producer-epoch",
                "7",
                "--sock-dir",
                "/tmp/x",
                "--orphan-ms",
                "250",
                "--fault",
                "panic",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(a.instance, "uart-dut");
        assert_eq!(a.producer_epoch, 7);
        assert_eq!(a.orphan, Duration::from_millis(250));
        assert!(WorkerArgs::parse(["--instance".to_string(), "../x".to_string()]).is_err());
    }

    #[test]
    fn write_outcome_roundtrip() {
        let o = WriteOutcome {
            state: WriteState::Confirmed,
            detail: json!({"written_bytes": 3}),
            error: None,
        };
        assert_eq!(decode_write_outcome(ReplyResult::Ok(encode_write_outcome(&o))), o);
        let bad = decode_write_outcome(ReplyResult::Ok(json!({"x": 1})));
        assert_eq!(bad.state, WriteState::OutcomeUnknown);
    }

    #[test]
    fn drop_runs_are_exact_and_bounded() {
        let mut m = Mirror::default();
        let rec = |seq: u64, ch: &str| {
            let mut r: SourceRecord = serde_json::from_value(json!({
                "schema_version": 1, "session_id": "s", "domain": "uart", "channel": ch, "kind": "uart.rx",
                "producer_id": "p", "producer_epoch": "1", "source_seq": seq.to_string(),
                "ts": {"mono_ns": "1", "wall_ns": "1", "clock_id": "c", "source": "host_read_complete"},
                "origin": "device", "payload": {}
            }))
            .unwrap();
            r.channel = Some(ch.into());
            r
        };
        for s in 10..20 {
            m.drop_record(&rec(s, "a"), 5);
        }
        m.drop_record(&rec(25, "b"), 5);
        m.close_run();
        let runs: Vec<_> = m.closed.iter().map(|r| (r.from, r.to, r.count)).collect();
        assert_eq!(runs, vec![(10, 19, 10), (25, 25, 1)]);
        for s in 0..200u64 {
            m.drop_record(&rec(1000 + s * 2, if s % 2 == 0 { "a" } else { "b" }), 1);
        }
        m.close_run();
        assert!(m.closed.len() <= MAX_GAP_RUNS);
        assert_eq!(m.closed.iter().map(|r| r.count).sum::<u64>(), 211);
    }
}
