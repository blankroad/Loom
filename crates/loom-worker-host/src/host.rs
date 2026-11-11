//! IPC worker host: spawns a worker process, speaks `loom-worker/1` with it and supervises it
//! (contract 05 §5.3–5.5, plan §3.3).
//!
//! [`IpcHost`] offers the same operations with the same semantics as the gateway's
//! `WorkerHost` trait (`loom-gateway/src/worker_host.rs`): `kind`, `call`, `write`, `job`,
//! `input`, `latch`, `state`. The trait itself lives in `loom-gateway`, which depends on this
//! crate, so the gateway adds the one-line delegating `impl WorkerHost for IpcHost`.
//!
//! Event path: every record from the worker goes into the `EmitSink` given at start —
//! `PriorityEvents` as `Lane::Ctl` (in order, before the `Reply` that covers them), `Events` as
//! `Lane::Data`. A `write` therefore resolves only after its write-state rows were pushed into
//! the sink (publish-before-reply); [`IpcHost::write_with_covers`] also returns `Reply.covers`.
//!
//! Supervision: starting → ready (Hello ≤ 5 s) → suspect (no Health 3 s; new writes refused) →
//! stalled (5 s) → killed (10 s or RSS > hard). Any exit → in-flight writes `outcome_unknown`
//! (not yet sent: `failed`), calls `E_WORKER_DOWN`; restart after 250 ms · 2ⁿ ≤ 30 s ± 20 %;
//! 5 exits within 60 s → quarantined (until [`IpcHost::restart`]). The producer epoch increases
//! on every start; `Welcome.config.reopen_after_crash` carries `WorkerSpec.reopen_after_crash`.
//!
//! Queues (plan §9.2): ctl send Q5 = 256 frames / 256 KiB per worker [`E_WORKER_BUSY`, a write
//! that was not queued is `failed`]; latch frames use a separate 16-slot lane that is written
//! first [`E_WORKER_BUSY`]; credit grants 64 [coalesced]. The ctl/data readers have no queue:
//! they push into the sink synchronously, so a slow sink backpressures the worker's socket.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use loom_domain_api::{Reopen, WorkerSpec};
use loom_proto::error::ErrorCode;
use loom_proto::event::{SourceRef, WriteState};
use loom_proto::num::U64;
use loom_proto::worker::JournalHealth;
use loom_proto::worker::{
    Call, Credit, Input, InputLease, InputLeaseEnd, Lane, Latch, Reject, ReplyResult, WORKER_PROTO_MAJOR,
    WORKER_PROTO_MINOR, Welcome, WorkerBudgets, WorkerError, WorkerMsg, Write,
};
use loom_worker_sdk::codec::encode_frame;
use loom_worker_sdk::transport::{FrameReader, FrameWriter, check_lane, msg_name, request_call_id};
use loom_worker_sdk::{BoxFuture, EmitSink, JobCmd, WriteOutcome, decode_write_outcome};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::listen::LaneListeners;

/// Q5: ctl send frames per worker.
pub const CTL_TX_ITEMS: usize = 256;
/// Q5: ctl send bytes per worker.
pub const CTL_TX_BYTES: usize = 256 * 1024;
/// Latch lane capacity.
pub const LATCH_LANE: usize = 16;
/// Credit grant queue (coalesced by the writer).
pub const CREDIT_QUEUE: usize = 64;
/// Read deadline once a frame started.
pub const READ_DEADLINE: Duration = Duration::from_secs(5);

/// Supervisor/host configuration. Build with [`HostConfig::new`] and adjust fields.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Worker spec registered by the domain.
    pub spec: WorkerSpec,
    /// Instance id (also producer id), e.g. `uart-dut`.
    pub instance: String,
    /// Executable (`loomd`, or `loom-eth-worker` for eth).
    pub program: PathBuf,
    /// Arguments before the standard flags (default `["worker", <kind>]`).
    pub args: Vec<String>,
    /// Arguments after the standard flags (domain flags).
    pub extra_args: Vec<String>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Socket directory (Unix).
    pub sock_dir: PathBuf,
    /// Recording session id passed to the worker.
    pub session_id: String,
    /// Stream epoch sent in `Welcome`.
    pub stream_epoch: u64,
    /// Producer epoch of the first start (then +1 per start).
    pub first_producer_epoch: u64,
    /// Domain configuration sent in `Welcome.config` (object; restart info is added).
    pub config: Value,
    /// Hello/Welcome bound.
    pub hello_timeout: Duration,
    /// Health interval asked from the worker (`--health-ms`).
    pub health_interval: Duration,
    /// No Health for this long → suspect.
    pub suspect_after: Duration,
    /// → stalled.
    pub stall_after: Duration,
    /// → killed.
    pub kill_after: Duration,
    /// First restart delay.
    pub backoff_base: Duration,
    /// Largest restart delay.
    pub backoff_max: Duration,
    /// A session that stayed up this long resets the backoff.
    pub backoff_reset: Duration,
    /// Crash window for quarantine.
    pub crash_window: Duration,
    /// Exits within the window that quarantine the worker.
    pub crash_limit: usize,
    /// Orphan time passed to the worker (`--orphan-ms`); `None` = worker default (30 s).
    pub orphan: Option<Duration>,
    /// Journal session root passed as `--journal-dir` (the worker writes its own journal under
    /// `<dir>/<instance>/<producer_epoch>/`). After a worker exit the host recovers that epoch's
    /// open segment (crash tail) and reports the loss in the exit event's `detail.record_gaps`.
    pub journal_dir: Option<PathBuf>,
    /// Latch ack bound.
    pub latch_timeout: Duration,
    /// Initial data credit (`None` = `spec.budgets.credit`).
    pub initial_credit: Option<u64>,
    /// Re-grant credit for every consumed `Events` frame.
    pub regrant_credit: bool,
}

impl HostConfig {
    /// Contract defaults (5 s hello, 1 s health, 3/5/10 s watchdog, 250 ms…30 s backoff, 5 in
    /// 60 s quarantine, 200 ms latch ack).
    pub fn new(spec: WorkerSpec, instance: impl Into<String>, program: impl Into<PathBuf>) -> Self {
        let kind = spec.kind.clone();
        HostConfig {
            spec,
            instance: instance.into(),
            program: program.into(),
            args: vec!["worker".into(), kind],
            extra_args: Vec::new(),
            env: Vec::new(),
            sock_dir: loom_worker_sdk::transport::default_sock_dir(),
            session_id: "loomd".into(),
            stream_epoch: 1,
            first_producer_epoch: 1,
            config: json!({}),
            hello_timeout: Duration::from_secs(5),
            health_interval: Duration::from_secs(1),
            suspect_after: Duration::from_secs(3),
            stall_after: Duration::from_secs(5),
            kill_after: Duration::from_secs(10),
            backoff_base: Duration::from_millis(250),
            backoff_max: Duration::from_secs(30),
            backoff_reset: Duration::from_secs(60),
            crash_window: Duration::from_secs(60),
            crash_limit: 5,
            orphan: None,
            journal_dir: None,
            latch_timeout: Duration::from_millis(200),
            initial_credit: None,
            regrant_credit: true,
        }
    }
}

/// Supervisor state (`core.worker_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    /// Spawned, waiting for Hello.
    Starting,
    /// Serving.
    Ready,
    /// Health missing (new writes refused).
    Suspect,
    /// Health missing longer (all new requests refused).
    Stalled,
    /// Killed by the supervisor (watchdog / memory).
    Killed,
    /// Process ended (exit, crash, connection lost).
    Exited,
    /// Waiting for the restart delay.
    Restarting,
    /// Too many crashes; waiting for a manual restart.
    Quarantined,
    /// Rejected (protocol major mismatch / identity mismatch).
    Incompatible,
    /// Host shut down.
    Stopped,
}

impl WorkerState {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkerState::Starting => "starting",
            WorkerState::Ready => "ready",
            WorkerState::Suspect => "suspect",
            WorkerState::Stalled => "stalled",
            WorkerState::Killed => "killed",
            WorkerState::Exited => "exited",
            WorkerState::Restarting => "restarting",
            WorkerState::Quarantined => "quarantined",
            WorkerState::Incompatible => "incompatible",
            WorkerState::Stopped => "stopped",
        }
    }
}

/// One state change, for `core.worker_state`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StateEvent {
    /// Instance.
    pub instance: String,
    /// Kind.
    pub kind: String,
    /// New state (`Ready` with `detail.mem = "pressure"` for soft-memory warnings).
    pub state: WorkerState,
    /// Producer epoch of the current/last start.
    pub producer_epoch: u64,
    /// OS pid when known.
    pub pid: Option<u32>,
    /// Detail (`reason`, `exit`, `in_ms`, `mem` …).
    pub detail: Value,
}

/// State change callback (must not block; the gateway turns it into `core.worker_state`).
pub type StateCallback = Arc<dyn Fn(&StateEvent) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Resolution {
    Reply(ReplyResult, Option<SourceRef>),
    Down { sent: bool },
}

struct PendingEntry {
    tx: oneshot::Sender<Resolution>,
    sent: bool,
}

struct Out {
    frame: Vec<u8>,
    id: Option<u64>,
    charged: bool,
}

struct Conn {
    minor: u16,
    tx: mpsc::Sender<Out>,
    urgent: mpsc::Sender<Out>,
    credit_tx: mpsc::Sender<u64>,
    q_bytes: AtomicUsize,
    next_id: Mutex<u64>,
    pending: Mutex<HashMap<u64, PendingEntry>>,
    latch_waiters: Mutex<HashMap<u64, oneshot::Sender<()>>>,
    last_health: Mutex<Instant>,
    kill: Notify,
    kill_reason: Mutex<Option<String>>,
}

impl Conn {
    fn request_kill(&self, reason: &str) {
        let mut r = lock(&self.kill_reason);
        if r.is_none() {
            *r = Some(reason.to_owned());
        }
        drop(r);
        self.kill.notify_one();
    }
}

#[derive(Debug, Clone)]
struct Status {
    state: WorkerState,
    producer_epoch: u64,
    pid: Option<u32>,
    starts: u64,
    last_exit: Option<Value>,
    last_rss: u64,
    mem_pressure: bool,
    journal: Option<JournalHealth>,
}

struct Inner {
    cfg: HostConfig,
    kind: String,
    sink: Arc<dyn EmitSink>,
    on_state: StateCallback,
    status: Mutex<Status>,
    conn: Mutex<Option<Arc<Conn>>>,
    latch_on: AtomicBool,
    shutdown: AtomicBool,
    shutdown_kill: AtomicBool,
    /// Graceful stop: wait this long (ms) for the worker to exit after closing the lanes, then
    /// kill (0 = do not wait).
    shutdown_grace_ms: AtomicU64,
    shutdown_wake: Notify,
    restart_wake: Notify,
    writes: AtomicU64,
}

impl Inner {
    fn set_state(&self, state: WorkerState, detail: Value) {
        let ev = {
            let mut s = lock(&self.status);
            s.state = state;
            StateEvent {
                instance: self.cfg.instance.clone(),
                kind: self.kind.clone(),
                state,
                producer_epoch: s.producer_epoch,
                pid: s.pid,
                detail,
            }
        };
        info!(instance = %ev.instance, state = ev.state.as_str(), detail = %ev.detail, "worker state");
        (self.on_state)(&ev);
    }

    fn state(&self) -> WorkerState {
        lock(&self.status).state
    }

    fn conn(&self) -> Option<Arc<Conn>> {
        lock(&self.conn).clone()
    }

    fn down(&self) -> WorkerError {
        let st = self.state();
        WorkerError {
            code: ErrorCode::WorkerDown,
            message: format!("worker {} is {}", self.cfg.instance, st.as_str()),
            detail: json!({
                "state": st.as_str(),
                "quarantined": st == WorkerState::Quarantined,
            }),
        }
    }

    fn busy(&self, what: &str) -> WorkerError {
        WorkerError {
            code: ErrorCode::WorkerBusy,
            message: format!("worker {} ctl queue full ({what})", self.cfg.instance),
            detail: Value::Null,
        }
    }

    /// Queue a request frame (Q5). Returns the reply receiver.
    fn send_request(
        &self,
        mut msg: WorkerMsg,
        allow: &[WorkerState],
    ) -> Result<oneshot::Receiver<Resolution>, WorkerError> {
        if !allow.contains(&self.state()) {
            return Err(self.down());
        }
        let conn = self.conn().ok_or_else(|| self.down())?;
        let mut next = lock(&conn.next_id);
        let permit = conn.tx.try_reserve().map_err(|_| self.busy("frames"))?;
        let ordinal = next
            .checked_add(1)
            .ok_or_else(|| self.busy("request ordinal exhausted"))?;
        match &mut msg {
            WorkerMsg::Call(c) => c.call_id = U64(ordinal),
            WorkerMsg::Write(w) => w.call_id = U64(ordinal),
            WorkerMsg::JobStart(j) if conn.minor == 0 => j.call_id = None,
            WorkerMsg::JobUpdate(j) if conn.minor == 0 => j.call_id = None,
            WorkerMsg::JobRenew(j) if conn.minor == 0 => j.call_id = None,
            WorkerMsg::JobStop(j) if conn.minor == 0 => j.call_id = None,
            WorkerMsg::Input(i) if conn.minor == 0 => i.call_id = None,
            _ => {}
        }
        if conn.minor < 2 {
            match &mut msg {
                WorkerMsg::Call(c) => c.conn_id = None,
                WorkerMsg::Input(i) => {
                    i.conn_id = None;
                    i.channel = None;
                    i.input_seq = None;
                }
                WorkerMsg::InputLease(lease) => lease.conn_id = None,
                _ => {}
            }
        }
        let id = request_call_id(&msg).unwrap_or(ordinal);
        if lock(&conn.pending).contains_key(&id) {
            return Err(self.busy("duplicate in-flight call_id"));
        }
        let frame = encode_frame(&msg).map_err(|e| WorkerError {
            code: ErrorCode::Usage,
            message: format!("request not encodable: {e}"),
            detail: Value::Null,
        })?;
        if conn.q_bytes.load(Ordering::Acquire) + frame.len() > CTL_TX_BYTES {
            return Err(self.busy("bytes"));
        }
        conn.q_bytes.fetch_add(frame.len(), Ordering::AcqRel);
        let (tx, rx) = oneshot::channel();
        lock(&conn.pending).insert(id, PendingEntry { tx, sent: false });
        *next = ordinal;
        permit.send(Out {
            frame,
            id: Some(id),
            charged: true,
        });
        Ok(rx)
    }
}

/// A worker process behind `loom-worker/1`.
pub struct IpcHost {
    inner: Arc<Inner>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for IpcHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpcHost")
            .field("kind", &self.inner.kind)
            .field("instance", &self.inner.cfg.instance)
            .field("state", &self.inner.state())
            .finish_non_exhaustive()
    }
}

impl Drop for IpcHost {
    fn drop(&mut self) {
        // Like a gateway death: connections close, the worker latches and exits after its
        // orphan time. Use `shutdown(true)` to kill instead.
        self.inner.shutdown.store(true, Ordering::Release);
        self.inner.shutdown_wake.notify_waiters();
        self.inner.shutdown_wake.notify_one();
    }
}

const READY_ONLY: &[WorkerState] = &[WorkerState::Ready];
const READY_OR_SUSPECT: &[WorkerState] = &[WorkerState::Ready, WorkerState::Suspect];

impl IpcHost {
    /// Start supervising (spawns the first worker process). Needs a tokio runtime.
    pub fn start(cfg: HostConfig, sink: Arc<dyn EmitSink>, on_state: StateCallback) -> Arc<IpcHost> {
        let inner = Arc::new(Inner {
            kind: cfg.spec.kind.clone(),
            status: Mutex::new(Status {
                state: WorkerState::Starting,
                producer_epoch: cfg.first_producer_epoch,
                pid: None,
                starts: 0,
                last_exit: None,
                last_rss: 0,
                mem_pressure: false,
                journal: None,
            }),
            cfg,
            sink,
            on_state,
            conn: Mutex::new(None),
            latch_on: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            shutdown_kill: AtomicBool::new(false),
            shutdown_grace_ms: AtomicU64::new(0),
            shutdown_wake: Notify::new(),
            restart_wake: Notify::new(),
            writes: AtomicU64::new(0),
        });
        let task = tokio::spawn(supervise(inner.clone()));
        Arc::new(IpcHost {
            inner,
            task: Mutex::new(Some(task)),
        })
    }

    /// Worker kind.
    pub fn kind(&self) -> &str {
        &self.inner.kind
    }

    /// Instance id.
    pub fn instance(&self) -> &str {
        &self.inner.cfg.instance
    }

    /// Current supervisor state.
    pub fn worker_state(&self) -> WorkerState {
        self.inner.state()
    }

    /// OS pid of the current process.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.inner.status).pid
    }

    /// Producer epoch of the current start.
    pub fn producer_epoch(&self) -> u64 {
        lock(&self.inner.status).producer_epoch
    }

    /// Generic call. Refused unless ready or suspect.
    pub fn call(&self, call: Call) -> BoxFuture<'_, Result<Value, WorkerError>> {
        Box::pin(async move {
            let rx = self.inner.send_request(WorkerMsg::Call(call), READY_OR_SUSPECT)?;
            match rx.await {
                Ok(Resolution::Reply(ReplyResult::Ok(v), _)) => Ok(v),
                Ok(Resolution::Reply(ReplyResult::Err(e), _)) => Err(e),
                Ok(Resolution::Down { .. }) | Err(_) => Err(self.inner.down()),
            }
        })
    }

    /// Write; resolves after the worker's write-state rows were pushed into the sink.
    pub fn write(&self, write: Write) -> BoxFuture<'_, WriteOutcome> {
        Box::pin(async move { self.write_with_covers(write).await.0 })
    }

    /// Write plus `Reply.covers` (the gateway waits for the hub watermark ≥ covers).
    /// Only accepted while `ready`. Not sent → `failed`; sent but the worker died →
    /// `outcome_unknown` (never retried).
    pub fn write_with_covers(&self, write: Write) -> BoxFuture<'_, (WriteOutcome, Option<SourceRef>)> {
        Box::pin(async move {
            self.inner.writes.fetch_add(1, Ordering::Relaxed);
            let rx = match self.inner.send_request(WorkerMsg::Write(write), READY_ONLY) {
                Ok(rx) => rx,
                Err(e) => {
                    return (
                        WriteOutcome {
                            state: WriteState::Failed,
                            detail: json!({"sent": false}),
                            error: Some(e),
                        },
                        None,
                    );
                }
            };
            match rx.await {
                Ok(Resolution::Reply(r, covers)) => (decode_write_outcome(r), covers),
                Ok(Resolution::Down { sent: true }) => (
                    WriteOutcome {
                        state: WriteState::OutcomeUnknown,
                        detail: json!({"sent": true, "worker": "exited"}),
                        error: Some(self.inner.down()),
                    },
                    None,
                ),
                Ok(Resolution::Down { sent: false }) | Err(_) => (
                    WriteOutcome {
                        state: WriteState::Failed,
                        detail: json!({"sent": false}),
                        error: Some(self.inner.down()),
                    },
                    None,
                ),
            }
        })
    }

    /// Job command.
    pub fn job(&self, job: JobCmd) -> BoxFuture<'_, Result<Value, WorkerError>> {
        Box::pin(async move {
            let msg = match job {
                JobCmd::Start(j) => WorkerMsg::JobStart(j),
                JobCmd::Update(j) => WorkerMsg::JobUpdate(j),
                JobCmd::Renew(j) => WorkerMsg::JobRenew(j),
                JobCmd::Stop(j) => WorkerMsg::JobStop(j),
            };
            let allow: &[WorkerState] = if matches!(msg, WorkerMsg::JobStop(_)) {
                READY_OR_SUSPECT
            } else {
                READY_ONLY
            };
            let rx = self.inner.send_request(msg, allow)?;
            match rx.await {
                Ok(Resolution::Reply(ReplyResult::Ok(v), _)) => Ok(v),
                Ok(Resolution::Reply(ReplyResult::Err(e), _)) => Err(e),
                Ok(Resolution::Down { .. }) | Err(_) => Err(self.inner.down()),
            }
        })
    }

    /// Interactive input (priority input skips the worker's request queue).
    pub fn input(&self, input: Input) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            let rx = self
                .inner
                .send_request(WorkerMsg::Input(input), READY_OR_SUSPECT)?;
            match rx.await {
                Ok(Resolution::Reply(ReplyResult::Ok(_), _)) => Ok(()),
                Ok(Resolution::Reply(ReplyResult::Err(e), _)) => Err(e),
                Ok(Resolution::Down { .. }) | Err(_) => Err(self.inner.down()),
            }
        })
    }

    /// 입력 lease를 worker에 전달하고 기존 소켓 순번 Reply를 기다린다.
    pub fn input_lease(&self, lease: InputLease) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            let rx = self
                .inner
                .send_request(WorkerMsg::InputLease(lease), READY_OR_SUSPECT)?;
            match rx.await {
                Ok(Resolution::Reply(ReplyResult::Ok(_), _)) => Ok(()),
                Ok(Resolution::Reply(ReplyResult::Err(e), _)) => Err(e),
                Ok(Resolution::Down { .. }) | Err(_) => Err(self.inner.down()),
            }
        })
    }

    /// v1.2 lease 종료 통지. Reply·요청 순번 없이 유한 ctl 큐에 넣는다.
    pub fn input_lease_end(&self, end: InputLeaseEnd) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            let conn = self.inner.conn().ok_or_else(|| self.inner.down())?;
            if conn.minor < 2 {
                return Err(WorkerError {
                    code: ErrorCode::NotImplemented,
                    message: "InputLeaseEnd에는 worker minor 2가 필요하다".into(),
                    detail: Value::Null,
                });
            }
            let _order = lock(&conn.next_id);
            let permit = conn.tx.try_reserve().map_err(|_| self.inner.busy("frames"))?;
            let frame = encode_frame(&WorkerMsg::InputLeaseEnd(end)).map_err(|e| WorkerError {
                code: ErrorCode::Usage,
                message: e.to_string(),
                detail: Value::Null,
            })?;
            if conn.q_bytes.load(Ordering::Acquire) + frame.len() > CTL_TX_BYTES {
                return Err(self.inner.busy("bytes"));
            }
            conn.q_bytes.fetch_add(frame.len(), Ordering::AcqRel);
            permit.send(Out {
                frame,
                id: None,
                charged: true,
            });
            Ok(())
        })
    }

    /// STOP latch: remembered for restarts (`Welcome.latch`), sent on the latch lane ahead of
    /// queued requests, resolved on `LatchAck` or `E_TIMEOUT{missing:true}` after the bound.
    pub fn latch(&self, latch: Latch) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            self.inner.latch_on.store(latch.on, Ordering::Release);
            let conn = self.inner.conn().ok_or_else(|| self.inner.down())?;
            let seq = latch.latch_seq.0;
            let frame = encode_frame(&WorkerMsg::Latch(latch)).map_err(|e| WorkerError {
                code: ErrorCode::Usage,
                message: e.to_string(),
                detail: Value::Null,
            })?;
            let (tx, rx) = oneshot::channel();
            lock(&conn.latch_waiters).insert(seq, tx);
            if conn
                .urgent
                .try_send(Out {
                    frame,
                    id: None,
                    charged: false,
                })
                .is_err()
            {
                lock(&conn.latch_waiters).remove(&seq);
                return Err(self.inner.busy("latch lane"));
            }
            match tokio::time::timeout(self.inner.cfg.latch_timeout, rx).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(self.inner.down()),
                Err(_) => {
                    lock(&conn.latch_waiters).remove(&seq);
                    Err(WorkerError {
                        code: ErrorCode::Timeout,
                        message: format!("worker {} did not ack the latch", self.inner.cfg.instance),
                        detail: json!({"missing": true, "instance": self.inner.cfg.instance}),
                    })
                }
            }
        })
    }

    /// State for `core.status`.
    pub fn state(&self) -> Value {
        let s = lock(&self.inner.status).clone();
        json!({
            "kind": self.inner.kind,
            "instance": self.inner.cfg.instance,
            "host": "ipc",
            "state": s.state.as_str(),
            "producer_epoch": s.producer_epoch.to_string(),
            "pid": s.pid,
            "starts": s.starts,
            "last_exit": s.last_exit,
            "rss_bytes": s.last_rss.to_string(),
            "mem_pressure": s.mem_pressure,
            "journal": s.journal.map(|j| json!({
                "health": j.health,
                "written_through": j.written_through,
                "durable_through": j.durable_through,
            })),
            "latch": self.inner.latch_on.load(Ordering::Acquire),
            "writes": self.inner.writes.load(Ordering::Relaxed),
        })
    }

    /// Grant extra data-lane credit (flow control / tests).
    pub fn grant_credit(&self, bytes: u64) {
        if let Some(c) = self.inner.conn() {
            let _ = c.credit_tx.try_send(bytes);
        }
    }

    /// Leave quarantine / incompatible and start again (viewer `core.worker.restart`).
    pub fn restart(&self) {
        self.inner.restart_wake.notify_one();
    }

    /// Graceful stop (contract 05 §5.5): send `Latch{on, reason:"shutdown"}` first (the caller,
    /// via [`IpcHost::latch`]); this then closes both lanes so the worker seals its journal and
    /// exits at once, waits up to `grace` for the process to end and kills it otherwise.
    pub async fn shutdown_graceful(&self, grace: Duration) {
        let ms = u64::try_from(grace.as_millis()).unwrap_or(u64::MAX).max(1);
        self.inner.shutdown_grace_ms.store(ms, Ordering::Release);
        self.shutdown(false).await;
    }

    /// Stop supervising. `kill = true` kills the worker; `false` leaves it to its orphan exit
    /// (or, after [`IpcHost::shutdown_graceful`], waits for its prompt exit).
    pub async fn shutdown(&self, kill: bool) {
        self.inner.shutdown_kill.store(kill, Ordering::Release);
        self.inner.shutdown.store(true, Ordering::Release);
        self.inner.shutdown_wake.notify_waiters();
        self.inner.shutdown_wake.notify_one();
        let task = lock(&self.task).take();
        if let Some(t) = task {
            let _ = t.await;
        }
    }
}

struct SessionEnd {
    ready_for: Option<Duration>,
    incompatible: bool,
    shutdown: bool,
}

fn backoff(cfg: &HostConfig, attempt: u32) -> Duration {
    let base = cfg.backoff_base.as_secs_f64() * 2f64.powi(i32::try_from(attempt.min(20)).unwrap_or(20));
    let capped = base.min(cfg.backoff_max.as_secs_f64());
    let jitter = 0.8 + 0.4 * rand::random::<f64>();
    Duration::from_secs_f64((capped * jitter).max(0.0))
}

async fn wait_restart_or_shutdown(inner: &Inner) -> bool {
    if inner.shutdown.load(Ordering::Acquire) {
        return true;
    }
    tokio::select! {
        () = inner.restart_wake.notified() => false,
        () = inner.shutdown_wake.notified() => true,
    }
}

async fn supervise(inner: Arc<Inner>) {
    let mut epoch = inner.cfg.first_producer_epoch;
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    let mut attempt = 0u32;
    loop {
        if inner.shutdown.load(Ordering::Acquire) {
            break;
        }
        let end = run_session(&inner, epoch).await;
        epoch += 1;
        if end.shutdown || inner.shutdown.load(Ordering::Acquire) {
            break;
        }
        if end.incompatible {
            inner.set_state(WorkerState::Incompatible, json!({}));
            if wait_restart_or_shutdown(&inner).await {
                break;
            }
            continue;
        }
        let now = Instant::now();
        if end.ready_for.is_some_and(|d| d >= inner.cfg.backoff_reset) {
            attempt = 0;
        }
        crashes.push_back(now);
        while crashes
            .front()
            .is_some_and(|t| now.duration_since(*t) > inner.cfg.crash_window)
        {
            crashes.pop_front();
        }
        if crashes.len() >= inner.cfg.crash_limit {
            inner.set_state(
                WorkerState::Quarantined,
                json!({"crashes": crashes.len(), "window_ms": inner.cfg.crash_window.as_millis() as u64}),
            );
            if wait_restart_or_shutdown(&inner).await {
                break;
            }
            crashes.clear();
            attempt = 0;
            continue;
        }
        let delay = backoff(&inner.cfg, attempt);
        attempt = attempt.saturating_add(1);
        inner.set_state(
            WorkerState::Restarting,
            json!({"in_ms": delay.as_millis() as u64, "attempt": attempt}),
        );
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = inner.shutdown_wake.notified() => break,
        }
    }
    inner.set_state(WorkerState::Stopped, json!({}));
}

fn exit_detail(st: &std::process::ExitStatus) -> Value {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        json!({"code": st.code(), "signal": st.signal()})
    }
    #[cfg(not(unix))]
    {
        json!({"code": st.code()})
    }
}

fn spawn_child(cfg: &HostConfig, epoch: u64) -> std::io::Result<Child> {
    let mut cmd = Command::new(&cfg.program);
    cmd.args(&cfg.args)
        .arg("--instance")
        .arg(&cfg.instance)
        .arg("--producer-epoch")
        .arg(epoch.to_string())
        .arg("--sock-dir")
        .arg(&cfg.sock_dir)
        .arg("--session")
        .arg(&cfg.session_id)
        .arg("--health-ms")
        .arg(cfg.health_interval.as_millis().to_string());
    if let Some(j) = &cfg.journal_dir {
        cmd.arg("--journal-dir").arg(j);
    }
    if let Some(o) = cfg.orphan {
        cmd.arg("--orphan-ms").arg(o.as_millis().to_string());
    }
    cmd.args(&cfg.extra_args)
        .envs(cfg.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        // Orphan mode needs the worker to outlive us (seal before exit).
        .kill_on_drop(false);
    cmd.spawn()
}

async fn kill_child(child: &mut Child) -> Option<std::process::ExitStatus> {
    let _ = child.start_kill();
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .ok()?
        .ok()
}

fn welcome_config(cfg: &HostConfig, starts: u64) -> Value {
    let mut c = match &cfg.config {
        Value::Object(m) => m.clone(),
        Value::Null => serde_json::Map::new(),
        other => {
            let mut m = serde_json::Map::new();
            m.insert("domain".into(), other.clone());
            m
        }
    };
    c.insert(
        "reopen_after_crash".into(),
        json!(match cfg.spec.reopen_after_crash {
            Reopen::None => "none",
            Reopen::RxOnly => "rx_only",
        }),
    );
    c.insert("restart".into(), json!(starts > 1));
    Value::Object(c)
}

#[allow(clippy::too_many_lines)]
async fn run_session(inner: &Arc<Inner>, epoch: u64) -> SessionEnd {
    let cfg = &inner.cfg;
    let fail = |ready_for| SessionEnd {
        ready_for,
        incompatible: false,
        shutdown: false,
    };
    {
        let mut s = lock(&inner.status);
        s.producer_epoch = epoch;
        s.pid = None;
        s.starts += 1;
        s.mem_pressure = false;
    }
    inner.set_state(WorkerState::Starting, json!({}));
    let listeners = match LaneListeners::bind(&cfg.sock_dir, &cfg.instance) {
        Ok(l) => l,
        Err(e) => {
            inner.set_state(
                WorkerState::Exited,
                json!({"reason": "bind", "error": e.to_string()}),
            );
            return fail(None);
        }
    };
    let mut child = match spawn_child(cfg, epoch) {
        Ok(c) => c,
        Err(e) => {
            inner.set_state(
                WorkerState::Exited,
                json!({"reason": "spawn", "error": e.to_string()}),
            );
            return fail(None);
        }
    };
    let pid = child.id();
    lock(&inner.status).pid = pid;

    // Connect + Hello, racing the child's early exit and shutdown.
    let hello = async {
        let (ctl, data) = listeners.accept(pid, cfg.hello_timeout).await?;
        let mut ctl_r = FrameReader::new(ctl.0);
        let msg = tokio::time::timeout(cfg.hello_timeout, ctl_r.read(Some(READ_DEADLINE)))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "no hello"))?
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok::<_, std::io::Error>((ctl_r, ctl.1, data, msg))
    };
    let started = tokio::select! {
        r = hello => Some(r),
        st = child.wait() => {
            let d = st.as_ref().map_or(Value::Null, exit_detail);
            inner.set_state(WorkerState::Exited, json!({"reason": "exited_before_hello", "exit": d}));
            lock(&inner.status).last_exit = Some(d);
            return fail(None);
        }
        () = inner.shutdown_wake.notified() => None,
    };
    let Some(started) = started else {
        if inner.shutdown_kill.load(Ordering::Acquire) {
            kill_child(&mut child).await;
        }
        return SessionEnd {
            ready_for: None,
            incompatible: false,
            shutdown: true,
        };
    };
    let (mut ctl_r, ctl_w, (data_r, data_w), msg) = match started {
        Ok((r, w, d, Some((m, _)))) => (r, w, d, m),
        Ok((_, _, _, None)) | Err(_) => {
            let reason = match &started {
                Err(e) => e.to_string(),
                _ => "closed before hello".into(),
            };
            let st = kill_child(&mut child).await;
            inner.set_state(
                WorkerState::Killed,
                json!({"reason": "hello", "error": reason, "exit": st.as_ref().map(exit_detail)}),
            );
            return fail(None);
        }
    };
    let mut ctl_w = FrameWriter::new(ctl_w);
    let hello = match msg {
        WorkerMsg::Hello(h) => h,
        other => {
            warn!(msg = msg_name(&other), "worker sent something other than hello");
            let st = kill_child(&mut child).await;
            inner.set_state(
                WorkerState::Killed,
                json!({"reason": "no_hello", "exit": st.as_ref().map(exit_detail)}),
            );
            return fail(None);
        }
    };
    let mismatch = if hello.proto.major != WORKER_PROTO_MAJOR {
        Some(format!(
            "protocol major {} != {WORKER_PROTO_MAJOR}",
            hello.proto.major
        ))
    } else if hello.kind != inner.kind || hello.instance != cfg.instance || hello.producer_epoch.0 != epoch {
        Some(format!(
            "identity mismatch: kind {} instance {} epoch {}",
            hello.kind, hello.instance, hello.producer_epoch
        ))
    } else {
        None
    };
    if let Some(reason) = mismatch {
        let _ = ctl_w
            .write(&WorkerMsg::Reject(Reject {
                reason: reason.clone(),
            }))
            .await;
        let st = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        if st.is_err() {
            kill_child(&mut child).await;
        }
        warn!(instance = %cfg.instance, %reason, "worker rejected");
        return SessionEnd {
            ready_for: None,
            incompatible: true,
            shutdown: false,
        };
    }
    let budgets = cfg.spec.budgets;
    let credit = cfg.initial_credit.unwrap_or(budgets.credit);
    let starts = lock(&inner.status).starts;
    // 양쪽 minor의 교집합. v1.0 상대에게는 선택적 call_id를 보내지 않는다.
    let accepted_minor = hello.proto.minor.min(WORKER_PROTO_MINOR);
    let welcome = WorkerMsg::Welcome(Welcome {
        accepted_minor,
        stream_epoch: U64(cfg.stream_epoch),
        config: welcome_config(cfg, starts),
        budgets: WorkerBudgets {
            mem_soft_bytes: U64(budgets.mem_soft),
            mem_hard_bytes: U64(budgets.mem_hard),
            credit_bytes: U64(credit),
        },
        latch: inner.latch_on.load(Ordering::Acquire),
    });
    if let Err(e) = ctl_w.write(&welcome).await {
        let st = kill_child(&mut child).await;
        inner.set_state(
            WorkerState::Killed,
            json!({"reason": "welcome", "error": e.to_string(), "exit": st.as_ref().map(exit_detail)}),
        );
        return fail(None);
    }

    let (tx, rx) = mpsc::channel::<Out>(CTL_TX_ITEMS);
    let (urgent, urgent_rx) = mpsc::channel::<Out>(LATCH_LANE);
    let (credit_tx, credit_rx) = mpsc::channel::<u64>(CREDIT_QUEUE);
    let conn = Arc::new(Conn {
        minor: accepted_minor,
        tx,
        urgent,
        credit_tx,
        q_bytes: AtomicUsize::new(0),
        next_id: Mutex::new(0),
        pending: Mutex::new(HashMap::new()),
        latch_waiters: Mutex::new(HashMap::new()),
        last_health: Mutex::new(Instant::now()),
        kill: Notify::new(),
        kill_reason: Mutex::new(None),
    });
    *lock(&inner.conn) = Some(conn.clone());
    let ready_at = Instant::now();
    inner.set_state(WorkerState::Ready, json!({"pid": pid, "producer_epoch": epoch}));

    let done = Arc::new(Notify::new());
    let mut tasks: Vec<JoinHandle<()>> = Vec::new();
    // ctl writer: latch lane first.
    {
        let conn = conn.clone();
        let done = done.clone();
        tasks.push(tokio::spawn(async move {
            let mut rx = rx;
            let mut urgent_rx = urgent_rx;
            loop {
                let out = tokio::select! {
                    biased;
                    Some(o) = urgent_rx.recv() => o,
                    Some(o) = rx.recv() => o,
                    else => break,
                };
                if let Some(id) = out.id
                    && let Some(p) = lock(&conn.pending).get_mut(&id)
                {
                    p.sent = true;
                }
                let len = out.frame.len();
                let r = ctl_w.write_raw(&out.frame).await;
                if out.charged {
                    conn.q_bytes.fetch_sub(len, Ordering::AcqRel);
                }
                if let Err(e) = r {
                    warn!(error = %e, "worker ctl write failed");
                    break;
                }
            }
            done.notify_one();
        }));
    }
    // ctl reader.
    {
        let conn = conn.clone();
        let inner = inner.clone();
        let done = done.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let msg = match ctl_r.read(Some(READ_DEADLINE)).await {
                    Ok(Some((m, _))) => m,
                    Ok(None) => break,
                    Err(e) => {
                        warn!(error = %e, instance = %inner.cfg.instance, "worker ctl read error");
                        break;
                    }
                };
                if check_lane(&msg, Lane::Ctl).is_err() {
                    warn!(msg = msg_name(&msg), "worker message on wrong lane");
                    break;
                }
                if !handle_ctl(&inner, &conn, msg) {
                    break;
                }
            }
            done.notify_one();
        }));
    }
    // data reader (+ credit re-grant).
    {
        let inner = inner.clone();
        let done = done.clone();
        let regrant = conn.credit_tx.clone();
        tasks.push(tokio::spawn(async move {
            let mut r = FrameReader::new(data_r);
            loop {
                match r.read(Some(READ_DEADLINE)).await {
                    Ok(Some((WorkerMsg::Events(ev), n))) => {
                        for rec in ev.records {
                            // Data lane may drop (the sink counts it as display.gap).
                            let _ = inner.sink.try_push(Lane::Data, rec);
                        }
                        if inner.cfg.regrant_credit {
                            let _ = regrant.try_send(n as u64);
                        }
                    }
                    Ok(Some((m, _))) => {
                        warn!(msg = msg_name(&m), "unexpected message on data lane");
                        break;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!(error = %e, "worker data read error");
                        break;
                    }
                }
            }
            done.notify_one();
        }));
    }
    // credit writer (coalesces grants).
    {
        let done = done.clone();
        tasks.push(tokio::spawn(async move {
            let mut w = FrameWriter::new(data_w);
            let mut rx = credit_rx;
            while let Some(mut bytes) = rx.recv().await {
                while let Ok(more) = rx.try_recv() {
                    bytes = bytes.saturating_add(more);
                }
                if w.write(&WorkerMsg::Credit(Credit { bytes: U64(bytes) }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            done.notify_one();
        }));
    }
    // watchdog.
    {
        let conn = conn.clone();
        let inner = inner.clone();
        tasks.push(tokio::spawn(async move {
            let tick =
                (inner.cfg.suspect_after / 6).clamp(Duration::from_millis(10), Duration::from_millis(250));
            loop {
                tokio::time::sleep(tick).await;
                let age = lock(&conn.last_health).elapsed();
                let st = inner.state();
                if age >= inner.cfg.kill_after {
                    conn.request_kill("no_health");
                    break;
                } else if age >= inner.cfg.stall_after {
                    if st != WorkerState::Stalled {
                        inner.set_state(
                            WorkerState::Stalled,
                            json!({"no_health_ms": age.as_millis() as u64}),
                        );
                    }
                } else if age >= inner.cfg.suspect_after && st == WorkerState::Ready {
                    inner.set_state(
                        WorkerState::Suspect,
                        json!({"no_health_ms": age.as_millis() as u64}),
                    );
                }
            }
        }));
    }

    // Wait for the end of this session.
    enum Why {
        Exit(Option<std::process::ExitStatus>),
        Conn,
        Kill,
        Shutdown,
    }
    let why = tokio::select! {
        st = child.wait() => Why::Exit(st.ok()),
        () = done.notified() => Why::Conn,
        () = conn.kill.notified() => Why::Kill,
        () = inner.shutdown_wake.notified() => Why::Shutdown,
    };
    *lock(&inner.conn) = None;
    for t in &tasks {
        t.abort();
    }
    let ready_for = Some(ready_at.elapsed());
    let shutdown = matches!(why, Why::Shutdown) || inner.shutdown.load(Ordering::Acquire);
    let (state, detail, status) = match why {
        Why::Exit(st) => (WorkerState::Exited, json!({"reason": "exit"}), st),
        Why::Conn => {
            // Connection lost: the process is dead or broken — make sure it is gone.
            let st = match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
                Ok(Ok(st)) => Some(st),
                _ => kill_child(&mut child).await,
            };
            (WorkerState::Exited, json!({"reason": "connection_lost"}), st)
        }
        Why::Kill => {
            let reason = lock(&conn.kill_reason).clone().unwrap_or_default();
            let st = kill_child(&mut child).await;
            (WorkerState::Killed, json!({"reason": reason}), st)
        }
        Why::Shutdown => {
            let grace = inner.shutdown_grace_ms.load(Ordering::Acquire);
            let st = if inner.shutdown_kill.load(Ordering::Acquire) {
                kill_child(&mut child).await
            } else if grace > 0 {
                // Lanes are closing (tasks aborted, connection dropped): the worker saw the
                // shutdown latch, so it exits on EOF. Bounded wait, then kill.
                match tokio::time::timeout(Duration::from_millis(grace), child.wait()).await {
                    Ok(Ok(st)) => Some(st),
                    _ => kill_child(&mut child).await,
                }
            } else {
                None
            };
            (WorkerState::Stopped, json!({"reason": "host_shutdown"}), st)
        }
    };
    // Resolve everything in flight: sent → outcome_unknown, queued → failed.
    let pending: Vec<PendingEntry> = lock(&conn.pending).drain().map(|(_, p)| p).collect();
    let in_flight = pending.len();
    for p in pending {
        let sent = p.sent;
        let _ = p.tx.send(Resolution::Down { sent });
    }
    lock(&conn.latch_waiters).clear();
    let exit = status.as_ref().map_or(Value::Null, exit_detail);
    lock(&inner.status).last_exit = Some(exit.clone());
    if !shutdown {
        let mut d = detail;
        // The worker is gone: seal its journal epoch (cut a torn tail) before reporting.
        let record_gaps = match &cfg.journal_dir {
            Some(root) => recover_journal(root, &cfg.instance, epoch).await,
            None => Value::Null,
        };
        if let Some(o) = d.as_object_mut() {
            o.insert("exit".into(), exit);
            o.insert("in_flight".into(), json!(in_flight));
            if !record_gaps.is_null() {
                o.insert("journal_recovery".into(), record_gaps);
            }
        }
        inner.set_state(state, d);
    }
    SessionEnd {
        ready_for,
        incompatible: false,
        shutdown,
    }
}

/// Recover `<root>/<instance>/<epoch>` after the worker died: `{segments, record_gaps:[GapData]}`.
async fn recover_journal(root: &std::path::Path, instance: &str, epoch: u64) -> Value {
    let dir = loom_journal::producer_dir(root, instance, epoch);
    let res = tokio::task::spawn_blocking(move || {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        loom_journal::recover_producer_dir(&dir, &loom_journal::IndexPolicy::default())
    })
    .await;
    match res {
        Ok(Ok(reports)) => json!({
            "segments": reports.len(),
            "record_gaps": reports
                .iter()
                .filter_map(|r| r.gap.as_ref().map(loom_journal::GapReport::to_gap_data))
                .collect::<Vec<_>>(),
        }),
        Ok(Err(e)) => json!({"error": e.to_string()}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// Handle one W→G ctl message. `false` = protocol error (close the session).
fn handle_ctl(inner: &Inner, conn: &Conn, msg: WorkerMsg) -> bool {
    match msg {
        WorkerMsg::Reply(r) => {
            if let Some(p) = lock(&conn.pending).remove(&r.call_id.0) {
                let _ = p.tx.send(Resolution::Reply(r.result, r.covers));
            }
        }
        WorkerMsg::PriorityEvents(p) => {
            for rec in p.records {
                // Control lane: never dropped by the gateway sink.
                let _ = inner.sink.try_push(Lane::Ctl, rec);
            }
        }
        WorkerMsg::Health(h) => {
            *lock(&conn.last_health) = Instant::now();
            let rss = h.rss_bytes.0;
            let b = inner.cfg.spec.budgets;
            let (state, pressure_now) = {
                let mut s = lock(&inner.status);
                s.last_rss = rss;
                s.journal = Some(h.journal);
                let was = s.mem_pressure;
                s.mem_pressure = b.mem_soft > 0 && rss > b.mem_soft;
                (s.state, s.mem_pressure && !was)
            };
            if b.mem_hard > 0 && rss > b.mem_hard {
                conn.request_kill("mem_hard");
                return true;
            }
            if matches!(state, WorkerState::Suspect | WorkerState::Stalled) {
                inner.set_state(WorkerState::Ready, json!({"recovered": true}));
            } else if pressure_now {
                inner.set_state(
                    WorkerState::Ready,
                    json!({"mem": "pressure", "rss_bytes": rss.to_string()}),
                );
            }
        }
        WorkerMsg::LatchAck(a) => {
            if let Some(tx) = lock(&conn.latch_waiters).remove(&a.latch_seq.0) {
                let _ = tx.send(());
            }
        }
        WorkerMsg::Segment(_) => {
            // Journal wiring (sealed segment catalog) is the lead's task.
        }
        other => {
            warn!(msg = msg_name(&other), "unexpected message from worker");
            return false;
        }
    }
    true
}
