//! Worker piece (K1, `can_sim`): owns one [`SimHub`] (sim buses + fake ECU nodes) and turns
//! everything on the bus into `can.*` events through the worker SDK.
//!
//! Threads (plan §3.3.1): the SDK callbacks (`on_call`, `on_write`, `on_latch`) and the pump share
//! the device state behind one mutex. Every critical section is short and never sleeps. The pump
//! ([`CanSimWorker::pump_once`]) polls the hub, sends due burst frames, drains each channel's
//! observer endpoint into `can.frame` events and says when it wants to run again. In
//! [`PumpMode::Thread`] a dedicated thread (`can-sim-pump`, started by the first `can.open`)
//! calls it; in [`PumpMode::Manual`] the caller does (tests with a `ManualClock`, or a future SDK
//! loop).
//!
//! Event mapping:
//! - device frame → `can.frame` on the data lane (`Emitter::rx`, origin `device`, dir `rx`);
//! - frame put on the bus by Loom (agent/user/script) → `can.frame` on the **priority** lane with
//!   its origin; the channel that sent it adds the request's top-level `request_id` (+
//!   `payload.frame_index` for bursts) — the SDK has no data-lane draft method with
//!   origin/request_id yet;
//! - `can.tx` write-state rows and `can.channel_state` → priority lane;
//! - frames the observer queue could not take (the worker never saw them) → `source.gap`
//!   (cause `sim_observer_overflow`, exact count): loss before Loom received the frame (A13).
//!
//! Timestamps: event `ts` is the SDK clock, back-dated by the frame's age on the sim bus, so frame
//! spacing is the bus spacing even when the pump runs late (`ts.source = synthetic`).
//!
//! Bounds (C2): see [`WorkerLimits`]. Every overflow is refused (channels, bursts) or counted
//! (request tags, observer drops, emit failures); nothing grows without a limit.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use loom_can_frame::{CanFrame, CanId, Direction};
use loom_can_sim::{
    DEFAULT_QUEUE_FRAMES, EndpointId, HubLimits, LocalReceiver, MonotonicClock, NodeConfig, NodeId, NodeInfo,
    NodeState, Sender, SimError, SimHub, SimRecord, parse_nodes,
};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, GapCount, Origin, Ts, TsSource, WriteState};
use loom_proto::num::U64;
use loom_proto::worker::{Call, Input, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, Emitter, EventDraft, JobCmd, LatchFlag, Worker, WorkerCtx, WriteOutcome};
use serde::Serialize;
use serde_json::{Value, json};

#[path = "periodic.rs"]
mod periodic;
use periodic::{Periodic, PeriodicTag};

use crate::isotp::{
    Endpoint, Io, MAX_ENDPOINTS, MAX_ENDPOINTS_PER_CHANNEL, ModelFactory, OpenParams, ReqMeta,
    Role as TpRole, SendOp, Submitted, builtin_model, id_text, parse_pdu,
};
use crate::schema::{FramePayload, IsoTpTag, MAX_SEND_COUNT, SendParams, TxPayload};
use crate::{
    KIND_CHANNEL_STATE, KIND_FRAME, KIND_TX, RPC_ISOTP_ABORT, RPC_ISOTP_BARRIER, RPC_ISOTP_CANCEL,
    RPC_ISOTP_CLOSE, RPC_ISOTP_LIST, RPC_ISOTP_OPEN, RPC_ISOTP_RECV, RPC_ISOTP_SEND, RPC_PERIODIC_LIST,
    RPC_STATUS, WORKER_KIND_SIM,
};

/// Call: open a sim channel. Args `{channel, bus?, nodes?: "default"|"none"|<toml>}`.
pub const CALL_OPEN: &str = "can.open";
/// Call: close a channel. Args `{channel}`.
pub const CALL_CLOSE: &str = "can.close";
/// Call: list sim nodes. Args `{}`.
pub const CALL_NODE_LIST: &str = "can.node.list";
/// Call: start nodes on an open channel's bus. Args `{channel, nodes: "default"|<toml>}`.
pub const CALL_NODE_START: &str = "can.node.start";
/// Call: stop a running node by name. Args `{channel, name}`.
pub const CALL_NODE_STOP: &str = "can.node.stop";

/// Backend name in `can.channel_state` / `can.status`.
pub const BACKEND: &str = "sim";
/// Default fake ECU (BCM) started by `can.open` unless `nodes` says otherwise.
pub const DEFAULT_NODES_TOML: &str = include_str!("sim_default.toml");
/// Most channels one worker keeps open (SimHub default: 16 buses).
pub const MAX_CHANNELS: usize = 16;
/// Longest channel / bus / node name in bytes.
pub const MAX_NAME_BYTES: usize = 64;
/// Longest pump sleep: frames from endpoints the worker does not see (future local sockets) are
/// drained within this time even with no node deadline.
pub const PUMP_MAX_SLEEP: Duration = Duration::from_millis(1);
/// `source.gap` cause for frames the observer queue dropped.
pub const GAP_CAUSE_OBSERVER: &str = "sim_observer_overflow";
/// Model factories that can be registered on top of the built-in ones (`vin_demo`, `echo`).
pub const MAX_MODEL_FACTORIES: usize = 16;
/// Drain → drive rounds per pump (ISO-TP ping-pong at one virtual instant); more → `more_pending`.
const MAX_ISOTP_ROUNDS: usize = 64;

/// Sender endpoints only transmit; their receive queue is never read and keeps one record.
const SENDER_QUEUE: usize = 1;
/// Sender endpoint origins, indexed like [`sender_slot`].
const SENDER_ORIGINS: [loom_can_sim::Origin; 3] = [
    loom_can_sim::Origin::Agent,
    loom_can_sim::Origin::User,
    loom_can_sim::Origin::Script,
];

/// Size limits of one worker (C2). Values below 1 are raised to 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerLimits {
    /// Open channels; more → `can.open` fails with `E_BUSY`.
    pub max_channels: usize,
    /// Bursts (`count > 1`, `gap_ms > 0`) still sending; more → the write fails with `E_BUSY`
    /// before `accepted`.
    pub max_pending_bursts: usize,
    /// `source_seq → request_id` tags waiting for their tx echo, per channel; more → the oldest
    /// tag is evicted (its echo is emitted without `request_id`, counted as `tags.evicted`).
    pub max_tags_per_channel: usize,
    /// Observer endpoint queue (records); full → the bus counts a drop, the worker reports
    /// `source.gap`.
    pub observer_queue: usize,
    /// Records drained per channel per pump; the rest waits for the next pump (no sleep).
    pub max_drain_per_pump: usize,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self {
            max_channels: MAX_CHANNELS,
            max_pending_bursts: MAX_SEND_COUNT as usize,
            max_tags_per_channel: 1_024,
            observer_queue: DEFAULT_QUEUE_FRAMES,
            max_drain_per_pump: 4_096,
        }
    }
}

impl WorkerLimits {
    fn sane(self) -> Self {
        Self {
            max_channels: self.max_channels.max(1),
            max_pending_bursts: self.max_pending_bursts.max(1),
            max_tags_per_channel: self.max_tags_per_channel.max(1),
            observer_queue: self.observer_queue.max(1),
            max_drain_per_pump: self.max_drain_per_pump.max(1),
        }
    }
}

/// Who runs the pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpMode {
    /// A dedicated thread, started by the first `can.open`, stopped on drop.
    Thread,
    /// The caller calls [`CanSimWorker::pump_once`] (tests, or an SDK loop).
    Manual,
}

/// What one [`CanSimWorker::pump_once`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PumpReport {
    /// Hub time at the end of the pump (ns).
    pub now_ns: u64,
    /// Node events handled (replies, periodic frames).
    pub node_events: usize,
    /// Burst frames sent.
    pub burst_frames: usize,
    /// Periodic job frames submitted by this pump.
    pub periodic_frames: usize,
    /// `can.frame` events emitted.
    pub frames: usize,
    /// Work is left for an immediate next pump (drain or poll limit reached).
    pub more_pending: bool,
    /// Hub time of the next node event or burst frame.
    pub next_wake_ns: Option<u64>,
}

/// The `can_sim` worker: sim buses, fake ECUs, `can.*` events.
pub struct CanSimWorker {
    core: Arc<Mutex<Core>>,
    mode: PumpMode,
    pump: Option<PumpThread>,
}

struct PumpThread {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl fmt::Debug for CanSimWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let core = lock(&self.core);
        f.debug_struct("CanSimWorker")
            .field("mode", &self.mode)
            .field("pump_running", &self.pump.is_some())
            .field("channels", &core.channels.keys().collect::<Vec<_>>())
            .field("pending_bursts", &core.bursts.len())
            .finish_non_exhaustive()
    }
}

impl Default for CanSimWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CanSimWorker {
    fn drop(&mut self) {
        if let Some(p) = self.pump.take() {
            p.stop.store(true, Ordering::Release);
            p.handle.thread().unpark();
            let _ = p.handle.join();
        }
    }
}

impl CanSimWorker {
    /// Production worker: real monotonic clock, pump thread, default limits.
    pub fn new() -> Self {
        Self::with_options(MonotonicClock::new(), PumpMode::Thread, WorkerLimits::default())
    }

    /// Worker with an injected sim clock (e.g. `ManualClock`), pump mode and limits.
    pub fn with_options(
        clock: impl loom_can_sim::Clock + 'static,
        mode: PumpMode,
        limits: WorkerLimits,
    ) -> Self {
        let core = Core {
            hub: SimHub::new(clock, HubLimits::default()),
            limits: limits.sane(),
            channels: BTreeMap::new(),
            bursts: Vec::new(),
            next_generation: 1,
            latch: None,
            latched: false,
            scratch: Vec::new(),
            emit_failed: 0,
            bursts_rejected: 0,
            bursts_cancelled: 0,
            endpoints: BTreeMap::new(),
            next_endpoint: 1,
            next_epoch: 1,
            models: Vec::new(),
            isotp_denied: 0,
            periodic: Periodic::default(),
        };
        Self {
            core: Arc::new(Mutex::new(core)),
            mode,
            pump: None,
        }
    }

    /// Manual-pump worker with default limits (tests).
    pub fn manual(clock: impl loom_can_sim::Clock + 'static) -> Self {
        Self::with_options(clock, PumpMode::Manual, WorkerLimits::default())
    }

    /// One pump step: poll nodes, send due burst frames, drain observers into events.
    pub fn pump_once(&self) -> PumpReport {
        lock(&self.core).pump()
    }

    /// Synchronous body of `on_call`.
    pub fn call(&mut self, cx: &WorkerCtx, call: &Call) -> Result<Value, WorkerError> {
        let out = match call.method.as_str() {
            CALL_OPEN => {
                self.ensure_pump()?;
                lock(&self.core).open(cx, call)
            }
            CALL_CLOSE => lock(&self.core).close(call),
            RPC_STATUS => lock(&self.core).status(&call.args),
            CALL_NODE_LIST => Ok(json!({"nodes": lock(&self.core).node_list()})),
            CALL_NODE_START => lock(&self.core).node_start(&call.args),
            CALL_NODE_STOP => lock(&self.core).node_stop(&call.args),
            RPC_ISOTP_OPEN => {
                self.ensure_pump()?;
                lock(&self.core).isotp_open(cx, call)
            }
            RPC_ISOTP_CLOSE => lock(&self.core).isotp_close(call),
            RPC_ISOTP_ABORT => lock(&self.core).isotp_abort(call),
            RPC_ISOTP_LIST => lock(&self.core).isotp_list(&call.args),
            RPC_ISOTP_RECV => lock(&self.core).isotp_recv(&call.args),
            RPC_ISOTP_BARRIER => lock(&self.core).isotp_barrier(&call.args),
            RPC_PERIODIC_LIST => lock(&self.core).periodic_list(&call.args),
            m => Err(err(
                ErrorCode::UnknownMethod,
                format!("{m} is not a can_sim call"),
            )),
        };
        self.wake_pump();
        out
    }

    /// Synchronous body of `on_write`: `op` = [`SendParams`] (`can.send`) or
    /// `{"method": "can.isotp.send", endpoint, pdu}`.
    pub fn write(&mut self, cx: &WorkerCtx, w: Write) -> WriteOutcome {
        if let Some(method @ (CALL_OPEN | CALL_CLOSE)) = w.op.get("method").and_then(Value::as_str) {
            if w.origin != Origin::User && (cx.latch().is_on() || lock(&self.core).latched()) {
                return failed(ErrorCode::Stopped, "STOP is latched".into(), 0);
            }
            let call = Call {
                conn_id: None,
                call_id: w.call_id,
                method: method.into(),
                args: w.op.clone(),
                origin: w.origin,
                client_id: w.client_id,
                actor_id: None,
                request_id: Some(w.request_id),
                deadline_mono_ns: w.deadline_mono_ns,
                approval_ref: w.approval_ref,
            };
            return match self.call(cx, &call) {
                Ok(detail) => WriteOutcome {
                    state: WriteState::Sent,
                    detail,
                    error: None,
                },
                Err(error) => WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(error),
                },
            };
        }
        if w.op
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(periodic::is_write)
        {
            if let Err(error) = self.ensure_pump() {
                return WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(error),
                };
            }
            let out = {
                let mut core = lock(&self.core);
                core.latch.get_or_insert_with(|| cx.latch().clone());
                core.periodic_write(&w)
            };
            self.wake_pump();
            return out;
        }
        if matches!(
            w.op.get("method").and_then(Value::as_str),
            Some(RPC_ISOTP_SEND | RPC_ISOTP_CANCEL)
        ) {
            let out = {
                let mut core = lock(&self.core);
                core.latch.get_or_insert_with(|| cx.latch().clone());
                if w.op["method"] == RPC_ISOTP_CANCEL {
                    core.isotp_cancel(&w)
                } else {
                    core.isotp_send(cx, &w)
                }
            };
            self.wake_pump();
            return out;
        }
        let params: SendParams = match serde_json::from_value(w.op.clone()) {
            Ok(p) => p,
            Err(e) => return failed(ErrorCode::Usage, format!("can.send: {e}"), 0),
        };
        let frame = match params.resolve_frame() {
            Ok(f) => f,
            Err(e) => return failed(ErrorCode::Usage, format!("can.send: {e}"), 0),
        };
        if params.dry_run {
            return failed(
                ErrorCode::Usage,
                "can.send dry_run is answered by the gateway; nothing sent".into(),
                0,
            );
        }
        let req = Req {
            request_id: w.request_id,
            client_id: w.client_id,
            origin: w.origin,
        };
        let out = {
            let mut core = lock(&self.core);
            core.latch.get_or_insert_with(|| cx.latch().clone());
            core.send_request(cx, &params, frame, &req)
        };
        self.wake_pump();
        out
    }

    /// STOP latch changed (A3): latch agent/script TX on every open bus and cancel their queued
    /// burst frames; fake ECU nodes and user sends keep running.
    pub fn set_latch(&mut self, on: bool) {
        lock(&self.core).set_latch(on);
    }

    fn ensure_pump(&mut self) -> Result<(), WorkerError> {
        if self.mode != PumpMode::Thread || self.pump.is_some() {
            return Ok(());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (core, flag) = (self.core.clone(), stop.clone());
        let handle = std::thread::Builder::new()
            .name("can-sim-pump".into())
            .spawn(move || pump_loop(&core, &flag))
            .map_err(|e| err(ErrorCode::Internal, format!("can-sim pump thread: {e}")))?;
        self.pump = Some(PumpThread { stop, handle });
        Ok(())
    }

    /// Adds (or replaces) a server model factory usable as `can.isotp.open{role:"server", model}`.
    /// At most [`MAX_MODEL_FACTORIES`] names; built-ins (`vin_demo`, `echo`) cannot be replaced.
    pub fn register_isotp_model(&self, name: &str, factory: ModelFactory) -> Result<(), WorkerError> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES || builtin_model(name).is_some() {
            return Err(err(ErrorCode::Usage, format!("bad model name {name:?}")));
        }
        let mut core = lock(&self.core);
        if let Some(slot) = core.models.iter_mut().find(|(n, _)| n == name) {
            slot.1 = factory;
            return Ok(());
        }
        if core.models.len() >= MAX_MODEL_FACTORIES {
            return Err(err(
                ErrorCode::Busy,
                format!("at most {MAX_MODEL_FACTORIES} registered models"),
            ));
        }
        core.models.push((name.to_owned(), factory));
        Ok(())
    }

    fn wake_pump(&self) {
        if let Some(p) = &self.pump {
            p.handle.thread().unpark();
        }
    }
}

fn pump_loop(core: &Mutex<Core>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        let r = lock(core).pump();
        if r.more_pending {
            std::thread::yield_now();
            continue;
        }
        let wait = r
            .next_wake_ns
            .map_or(PUMP_MAX_SLEEP, |t| {
                Duration::from_nanos(t.saturating_sub(r.now_ns))
            })
            .min(PUMP_MAX_SLEEP);
        if !wait.is_zero() {
            std::thread::park_timeout(wait);
        }
    }
}

impl Worker for CanSimWorker {
    const KIND: &'static str = WORKER_KIND_SIM;

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move { self.call(cx, &call) })
    }

    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, write: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move { self.write(cx, write) })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async {
            Err(err(
                ErrorCode::NotImplemented,
                "periodic controls use can.periodic.* job-class writes; generic JobCmd is unsupported".into(),
            ))
        })
    }

    fn on_input<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _input: Input,
    ) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async {
            Err(err(
                ErrorCode::Unsupported,
                "CAN has no interactive input; use can.send".into(),
            ))
        })
    }

    fn on_latch(&mut self, on: bool) {
        self.set_latch(on);
    }
}

// ---------------------------------------------------------------------------------------------
// Device state

#[derive(Debug, Clone)]
struct Req {
    request_id: String,
    client_id: Option<String>,
    origin: Origin,
}

/// Echo tag: the bus `source_seq` of a frame this channel sent, and its request.
#[derive(Debug)]
struct Tag {
    seq: u64,
    request_id: Option<String>,
    client_id: Option<String>,
    frame_index: Option<u32>,
    /// ISO-TP annotation (R-TP-07) for frames an endpoint sent.
    isotp: Option<IsoTpTag>,
    periodic: Option<PeriodicTag>,
    override_of: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct ChannelCounters {
    frames: u64,
    emit_failed: u64,
    source_gap_frames: u64,
    source_gap_pending: u64,
    source_gaps_reported: u64,
    tags_evicted: u64,
    tags_stale: u64,
    untagged_echo: u64,
}

struct Channel {
    name: String,
    bus: String,
    generation: u32,
    senders: [EndpointId; 3],
    /// Device-origin sender of ISO-TP server endpoints (sim ECU); attached on first use.
    device_sender: Option<EndpointId>,
    observer: EndpointId,
    rx: LocalReceiver,
    emitter: Emitter,
    nodes: Vec<NodeId>,
    tags: VecDeque<Tag>,
    last_seq: u64,
    /// Host time of the last ingress whose transport processing has returned.
    last_ingest_mono_ns: u64,
    c: ChannelCounters,
}

/// A burst still sending (`count > 1`, `gap_ms > 0`). One entry per request, not per frame.
struct Burst {
    channel: String,
    req: Req,
    frame: CanFrame,
    count: u32,
    next_index: u32,
    gap_ns: u64,
    next_due_ns: u64,
}

struct Core {
    hub: SimHub,
    limits: WorkerLimits,
    channels: BTreeMap<String, Channel>,
    bursts: Vec<Burst>,
    next_generation: u32,
    latch: Option<LatchFlag>,
    latched: bool,
    scratch: Vec<SimRecord>,
    emit_failed: u64,
    bursts_rejected: u64,
    bursts_cancelled: u64,
    /// ISO-TP endpoints by id (`tp1`, …).
    endpoints: BTreeMap<String, Endpoint>,
    next_endpoint: u64,
    next_epoch: u64,
    models: Vec<(String, ModelFactory)>,
    isotp_denied: u64,
    periodic: Periodic,
}

fn lock(m: &Mutex<Core>) -> MutexGuard<'_, Core> {
    // A panic elsewhere must not take the device down; the state stays usable.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// RPC u64 fields accept canonical decimal strings or nonnegative JSON integers.
fn tp_integer(args: &Value, key: &str, default: Option<u64>) -> Result<u64, WorkerError> {
    match args.get(key) {
        None => default.ok_or_else(|| err(ErrorCode::Usage, format!("missing {key}"))),
        Some(Value::Number(v)) => v
            .as_u64()
            .ok_or_else(|| err(ErrorCode::Usage, format!("{key} must be unsigned"))),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse()
                .map_err(|_| err(ErrorCode::Usage, format!("{key} overflows u64")))
        }
        _ => Err(err(ErrorCode::Usage, format!("{key} must be unsigned"))),
    }
}

fn err(code: ErrorCode, message: String) -> WorkerError {
    WorkerError {
        code,
        message,
        detail: Value::Null,
    }
}

fn failed(code: ErrorCode, message: String, sent: u32) -> WriteOutcome {
    WriteOutcome {
        state: WriteState::Failed,
        detail: json!({"sent": sent}),
        error: Some(err(code, message)),
    }
}

fn to_json<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn sender_slot(o: Origin) -> Option<usize> {
    match o {
        Origin::Agent => Some(0),
        Origin::User => Some(1),
        Origin::Script => Some(2),
        Origin::Device => None,
    }
}

fn proto_origin(o: loom_can_sim::Origin) -> Origin {
    match o {
        loom_can_sim::Origin::Agent => Origin::Agent,
        loom_can_sim::Origin::User => Origin::User,
        loom_can_sim::Origin::Device => Origin::Device,
        loom_can_sim::Origin::Script => Origin::Script,
    }
}

fn origin_name(o: loom_can_sim::Origin) -> &'static str {
    match o {
        loom_can_sim::Origin::Agent => "agent",
        loom_can_sim::Origin::User => "user",
        loom_can_sim::Origin::Device => "device",
        loom_can_sim::Origin::Script => "script",
    }
}

fn by_origin(v: [u64; 4]) -> Value {
    // Indexed like `loom_can_sim::Origin::ALL`.
    let mut m = serde_json::Map::new();
    for (o, n) in loom_can_sim::Origin::ALL.iter().zip(v) {
        m.insert(origin_name(*o).into(), json!(n));
    }
    Value::Object(m)
}

fn sim_error_code(e: &SimError) -> ErrorCode {
    match e {
        SimError::Latched { .. } => ErrorCode::Stopped,
        SimError::UnknownEndpoint | SimError::UnknownBus { .. } => ErrorCode::NotOpen,
        SimError::TooManyBuses { .. } | SimError::TooManyEndpoints { .. } | SimError::TooManyNodes { .. } => {
            ErrorCode::Busy
        }
        SimError::UnknownNode => ErrorCode::NotFound,
        SimError::Config(_) => ErrorCode::Config,
    }
}

fn sim_err(e: &SimError) -> WorkerError {
    err(sim_error_code(e), e.to_string())
}

/// SDK-clock timestamp of something that happened on the bus at `at_ns` (hub time).
pub(crate) fn ts_at(em: &Emitter, hub_now: u64, at_ns: u64, source: TsSource) -> Ts {
    let mut ts = em.clock().ts(source);
    let age = hub_now.saturating_sub(at_ns);
    ts.mono_ns = U64(ts.mono_ns.get().saturating_sub(age));
    ts.wall_ns = U64(ts.wall_ns.get().saturating_sub(age));
    ts
}

fn draft(
    kind: &str,
    ts: Ts,
    dir: Option<Dir>,
    origin: Origin,
    generation: Option<u32>,
    payload: Value,
) -> EventDraft {
    EventDraft {
        kind: kind.into(),
        ts,
        dir,
        origin,
        client_id: None,
        request_id: None,
        job_id: None,
        generation,
        purpose: None,
        risk: None,
        env: Some(Env::Sim),
        payload,
        metrics: vec![],
        quality: vec![],
    }
}

/// `can.tx` rows of one request.
struct TxRow<'a> {
    em: &'a Emitter,
    generation: Option<u32>,
    req: &'a Req,
    frame: &'a CanFrame,
    count: u32,
}

impl TxRow<'_> {
    /// Emits one row; `false` if the emit queue refused it (counted by the caller).
    fn emit(&self, state: WriteState, sent: u32, error: Option<ErrorCode>) -> bool {
        let payload = TxPayload {
            write_state: state,
            frame: FramePayload::from_frame(self.frame, None),
            count: self.count,
            sent,
            error: error.map(|c| c.as_str().to_owned()),
        };
        let mut d = draft(
            KIND_TX,
            self.em.clock().ts(TsSource::Driver),
            Some(Dir::Tx),
            self.req.origin,
            self.generation,
            to_json(&payload),
        );
        d.request_id = Some(self.req.request_id.clone());
        d.client_id = self.req.client_id.clone();
        self.em.priority(d).is_ok()
    }
}

fn name_arg<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, WorkerError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= MAX_NAME_BYTES => Ok(Some(s)),
        Some(_) => Err(err(
            ErrorCode::Usage,
            format!("`{key}` must be a string of 1..={MAX_NAME_BYTES} bytes"),
        )),
    }
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, WorkerError> {
    name_arg(args, key)?.ok_or_else(|| err(ErrorCode::Usage, format!("missing `{key}`")))
}

/// Node configs for `nodes` (`None`/`"default"` → BCM, `"none"` → nothing, else TOML), all
/// moved onto `bus`.
fn resolve_nodes(spec: Option<&Value>, bus: &str) -> Result<Vec<NodeConfig>, WorkerError> {
    let text = match spec {
        None | Some(Value::Null) => DEFAULT_NODES_TOML,
        Some(Value::String(s)) if s == "none" => return Ok(Vec::new()),
        Some(Value::String(s)) if s == "default" => DEFAULT_NODES_TOML,
        Some(Value::String(s)) => s.as_str(),
        Some(_) => {
            return Err(err(
                ErrorCode::Usage,
                "`nodes` must be \"default\", \"none\" or node TOML text".into(),
            ));
        }
    };
    let mut nodes = parse_nodes(text).map_err(|e| err(ErrorCode::Config, e.to_string()))?;
    for n in &mut nodes {
        n.channel = bus.to_owned();
    }
    Ok(nodes)
}

fn node_state(s: NodeState) -> &'static str {
    match s {
        NodeState::Running => "running",
        NodeState::Stopped => "stopped",
        NodeState::Crashed => "crashed",
    }
}

/// ISO-TP annotation of a bus frame no endpoint tag covers: a client endpoint receiving it first
/// (so a whole request/response exchange carries the client's endpoint and request), then the server
/// endpoint that sent it, then a server endpoint receiving it.
fn annotation(
    endpoints: &BTreeMap<String, Endpoint>,
    channel: &str,
    rec: &SimRecord,
    device_sender: Option<EndpointId>,
) -> Option<IsoTpTag> {
    let id = rec.frame.frame.id();
    let on = || endpoints.values().filter(move |e| e.channel == channel);
    let from_ecu = matches!(rec.sender, Sender::Endpoint(s) if Some(s) == device_sender);
    on().find(|e| e.role == TpRole::Client && e.rx_id() == id)
        .or_else(|| on().find(|e| e.role == TpRole::Server && from_ecu && e.tx_id() == id))
        .or_else(|| on().find(|e| e.role == TpRole::Server && e.rx_id() == id))
        .map(Endpoint::tag)
}

impl Channel {
    fn push_tag(&mut self, tag: Tag, max: usize) {
        if self.tags.len() >= max {
            self.tags.pop_front();
            self.c.tags_evicted += 1;
        }
        self.tags.push_back(tag);
    }

    /// Tag of our own frame `seq`. Tags are in bus order, so older ones lost their echo
    /// (observer drop) and are discarded.
    fn take_tag(&mut self, seq: u64) -> Option<Tag> {
        while let Some(front) = self.tags.front() {
            if front.seq < seq {
                self.tags.pop_front();
                self.c.tags_stale += 1;
            } else if front.seq == seq {
                return self.tags.pop_front();
            } else {
                break;
            }
        }
        self.c.untagged_echo += 1;
        None
    }

    /// Emits one bus record as `can.frame` (after any pending `source.gap`); returns the event's
    /// `source_seq` (ISO-TP `first_seq`/`last_seq` refer to it).
    fn emit_record(&mut self, rec: &SimRecord, hub_now: u64, annot: Option<IsoTpTag>) -> Option<u64> {
        let expected = self.last_seq + 1;
        if rec.source_seq > expected {
            let lost = rec.source_seq - expected;
            self.c.source_gap_frames += lost;
            self.c.source_gap_pending += lost;
        }
        self.last_seq = self.last_seq.max(rec.source_seq);
        if self.c.source_gap_pending > 0 {
            let n = GapCount::Exact(U64(self.c.source_gap_pending));
            if self.emitter.source_gap(GAP_CAUSE_OBSERVER, n).is_ok() {
                self.c.source_gap_pending = 0;
                self.c.source_gaps_reported += 1;
            } else {
                // Retried before the next record; the seq hole shows the failed emit.
                self.c.emit_failed += 1;
            }
        }
        let mut payload = FramePayload::from_frame(&rec.frame.frame, rec.frame.device_ts);
        let ts = ts_at(
            &self.emitter,
            hub_now,
            rec.frame.ingest_mono_ns,
            TsSource::Synthetic,
        );
        let res = if rec.origin == loom_can_sim::Origin::Device {
            payload.isotp = annot;
            self.emitter.rx(KIND_FRAME, ts, to_json(&payload))
        } else {
            let own = matches!(rec.sender, Sender::Endpoint(e) if self.senders.contains(&e));
            let annotated = self
                .tags
                .iter()
                .any(|t| t.seq == rec.source_seq && t.periodic.is_some());
            let tag = if own || annotated {
                self.take_tag(rec.source_seq)
            } else {
                None
            };
            let dir = match rec.frame.dir {
                Direction::Rx => Dir::Rx,
                Direction::Tx => Dir::Tx,
            };
            let mut d = draft(
                KIND_FRAME,
                ts,
                Some(dir),
                proto_origin(rec.origin),
                Some(self.generation),
                Value::Null,
            );
            let mut annot = annot;
            let mut periodic_revision = None;
            let mut override_of = None;
            if let Some(t) = tag {
                payload.frame_index = t.frame_index;
                d.request_id = t.request_id;
                d.client_id = t.client_id;
                annot = t.isotp.or(annot);
                if let Some(p) = t.periodic {
                    d.job_id = Some(p.job);
                    d.purpose = Some("periodic".into());
                    periodic_revision = Some(p.revision);
                }
                override_of = t.override_of;
            }
            payload.isotp = annot;
            d.payload = to_json(&payload);
            if let Some(p) = d.payload.as_object_mut() {
                if let Some(revision) = periodic_revision {
                    p.insert("revision".into(), json!(revision));
                }
                if let Some(job) = override_of {
                    p.insert("override_of".into(), json!(job));
                }
            }
            self.emitter.priority(d)
        };
        match res {
            Ok(seq) => {
                self.c.frames += 1;
                Some(seq.get())
            }
            Err(_) => {
                self.c.emit_failed += 1;
                None
            }
        }
    }
}

impl Core {
    fn latched(&self) -> bool {
        self.latched || self.latch.as_ref().is_some_and(LatchFlag::is_on)
    }

    fn pump(&mut self) -> PumpReport {
        let mut r = PumpReport::default();
        let p = self.hub.poll();
        r.node_events = p.events;
        r.more_pending = p.more_pending;
        let mut ended = Vec::new();
        r.burst_frames = self.run_bursts(&mut ended);
        if r.burst_frames > 0 {
            // Zero-delay node reactions to the burst frames.
            let p = self.hub.poll();
            r.node_events += p.events;
            r.more_pending |= p.more_pending;
        }
        let (frames, events, more) = self.settle();
        r.frames = frames;
        r.node_events += events;
        r.more_pending |= more;
        self.end_bursts(ended);
        // ISO-TP gets its FC/CF work first. Periodic dispatch is one bounded batch.
        r.periodic_frames = self.run_periodic();
        if r.periodic_frames > 0 {
            let p = self.hub.poll();
            r.node_events += p.events;
            r.more_pending |= p.more_pending;
            let (frames, events, more) = self.settle();
            r.frames += frames;
            r.node_events += events;
            r.more_pending |= more;
        }
        self.periodic_events();
        r.now_ns = self.hub.now_ns();
        let burst_due = self.bursts.iter().map(|b| b.next_due_ns).min();
        let tp_due = self.endpoints.values().filter_map(Endpoint::next_wake).min();
        r.next_wake_ns = [
            self.hub.next_deadline_ns(),
            burst_due,
            tp_due,
            self.periodic.scheduler.poll_at(),
        ]
        .into_iter()
        .flatten()
        .min();
        r
    }

    /// Drains the buses (feeding ISO-TP endpoints) and drives endpoints until nothing moves at this
    /// instant: an SF request, a zero-delay model reply and its FF/FC/CF exchange can all happen in
    /// one pump. Bounded by [`MAX_ISOTP_ROUNDS`]. Returns (frames emitted, node events, more left).
    fn settle(&mut self) -> (usize, usize, bool) {
        let (mut frames, mut events, mut more) = (0, 0, false);
        for round in 0..MAX_ISOTP_ROUNDS {
            let (f, m) = self.drain_all();
            frames += f;
            more |= m;
            if !self.drive_endpoints() {
                return (frames, events, more);
            }
            // Zero-delay node reactions to ISO-TP frames.
            let p = self.hub.poll();
            events += p.events;
            more |= p.more_pending;
            if round + 1 == MAX_ISOTP_ROUNDS {
                more = true;
            }
        }
        (frames, events, more)
    }

    /// Timers, model replies and due frames of every endpoint; tags their frames for the echo.
    fn drive_endpoints(&mut self) -> bool {
        if self.endpoints.is_empty() {
            return false;
        }
        let now = self.hub.now_ns();
        let mut progress = false;
        let mut out = Vec::new();
        let mut subs: Vec<(String, Submitted)> = Vec::new();
        for ep in self.endpoints.values_mut() {
            let Some(ch) = self.channels.get(&ep.channel) else {
                continue;
            };
            let mut io = Io {
                hub: &mut self.hub,
                senders: ch.senders,
                device_sender: ch.device_sender,
                emitter: &ch.emitter,
                now,
            };
            progress |= ep.drive(&mut io, &mut out);
            subs.extend(out.drain(..).map(|s| (ep.channel.clone(), s)));
        }
        let max = self.limits.max_tags_per_channel;
        for (name, s) in subs {
            if let Some(ch) = self.channels.get_mut(&name) {
                let tag = Tag {
                    seq: s.seq,
                    request_id: s.request_id,
                    client_id: s.client_id,
                    frame_index: None,
                    isotp: Some(s.tag),
                    periodic: None,
                    override_of: None,
                };
                ch.push_tag(tag, max);
            }
        }
        progress
    }

    fn drain_all(&mut self) -> (usize, bool) {
        let now = self.hub.now_ns();
        let max = self.limits.max_drain_per_pump;
        let mut scratch = std::mem::take(&mut self.scratch);
        let (mut frames, mut more) = (0, false);
        let endpoints = &mut self.endpoints;
        for ch in self.channels.values_mut() {
            scratch.clear();
            let n = ch.rx.recv_into(&mut scratch, max);
            let before = ch.c.frames;
            for rec in &scratch {
                if rec.source_seq > ch.last_seq.saturating_add(1) {
                    for ep in endpoints.values_mut().filter(|e| e.channel == ch.name) {
                        ep.note_ingress_loss();
                    }
                }
                let annot = annotation(endpoints, &ch.name, rec, ch.device_sender);
                let event_seq = ch.emit_record(rec, now, annot);
                // ISO-TP ingress after the frame's own event, so `rx_start` follows its FF.
                let f = &rec.frame.frame;
                if let Some(ep) = endpoints
                    .values_mut()
                    .find(|e| e.channel == ch.name && e.rx_id() == f.id())
                {
                    ep.feed(f, event_seq, rec.frame.ingest_mono_ns, &ch.emitter, now);
                }
                ch.last_ingest_mono_ns =
                    ts_at(&ch.emitter, now, rec.frame.ingest_mono_ns, TsSource::Synthetic)
                        .mono_ns
                        .get();
            }
            frames += (ch.c.frames - before) as usize;
            more |= n >= max && !ch.rx.is_empty();
        }
        scratch.clear();
        self.scratch = scratch;
        (frames, more)
    }

    /// Sends burst frames that are due; moves finished or refused bursts to `ended`.
    fn run_bursts(&mut self, ended: &mut Vec<(Burst, WriteState, Option<ErrorCode>)>) -> usize {
        let now = self.hub.now_ns();
        let latched = self.latched();
        let max_tags = self.limits.max_tags_per_channel;
        let mut sent_total = 0;
        let mut i = 0;
        while i < self.bursts.len() {
            let b = &mut self.bursts[i];
            if b.next_due_ns > now {
                i += 1;
                continue;
            }
            let Some(ch) = self.channels.get_mut(&b.channel) else {
                // Channel closed without cancelling (should not happen): drop it, counted.
                self.bursts.remove(i);
                self.bursts_cancelled += 1;
                continue;
            };
            let slot = sender_slot(b.req.origin).unwrap_or(0);
            let mut end: Option<(WriteState, Option<ErrorCode>)> = None;
            let reserved = self
                .endpoints
                .values()
                .any(|e| e.channel == b.channel && e.tx_id() == b.frame.id());
            if reserved {
                // An ISO-TP endpoint opened on this id during the burst (contract §12).
                end = Some((WriteState::Failed, Some(ErrorCode::Denied)));
            }
            while end.is_none() && b.next_index < b.count && b.next_due_ns <= now {
                // A1-②: the latch is checked before every unit.
                if latched && b.req.origin != Origin::User {
                    end = Some((WriteState::Failed, Some(ErrorCode::Stopped)));
                    break;
                }
                match self.hub.send(ch.senders[slot], b.frame) {
                    Ok(seq) => {
                        let tag = Tag {
                            seq,
                            request_id: Some(b.req.request_id.clone()),
                            client_id: b.req.client_id.clone(),
                            frame_index: Some(b.next_index),
                            isotp: None,
                            periodic: None,
                            override_of: None,
                        };
                        ch.push_tag(tag, max_tags);
                        b.next_index += 1;
                        b.next_due_ns += b.gap_ns;
                        sent_total += 1;
                    }
                    Err(e) => {
                        end = Some((WriteState::Failed, Some(sim_error_code(&e))));
                        break;
                    }
                }
            }
            if end.is_none() && b.next_index >= b.count {
                end = Some((WriteState::Sent, None));
            }
            if let Some((state, error)) = end {
                if state == WriteState::Failed {
                    self.bursts_cancelled += 1;
                }
                ended.push((self.bursts.remove(i), state, error));
            } else {
                i += 1;
            }
        }
        sent_total
    }

    /// Final `can.tx` row of bursts that ended in [`Core::run_bursts`] (after their echoes).
    fn end_bursts(&mut self, ended: Vec<(Burst, WriteState, Option<ErrorCode>)>) {
        for (b, state, error) in ended {
            let Some(ch) = self.channels.get(&b.channel) else {
                continue;
            };
            let row = TxRow {
                em: &ch.emitter,
                generation: Some(ch.generation),
                req: &b.req,
                frame: &b.frame,
                count: b.count,
            };
            if !row.emit(state, b.next_index, error) {
                self.emit_failed += 1;
            }
        }
    }

    /// Ends queued bursts for which `cancel` is true with a `failed` row.
    fn cancel_bursts(&mut self, code: ErrorCode, cancel: impl Fn(&Burst) -> bool) {
        let mut i = 0;
        while i < self.bursts.len() {
            if !cancel(&self.bursts[i]) {
                i += 1;
                continue;
            }
            let b = self.bursts.remove(i);
            if let Some(ch) = self.channels.get(&b.channel) {
                let row = TxRow {
                    em: &ch.emitter,
                    generation: Some(ch.generation),
                    req: &b.req,
                    frame: &b.frame,
                    count: b.count,
                };
                if !row.emit(WriteState::Failed, b.next_index, Some(code)) {
                    self.emit_failed += 1;
                }
            }
            self.bursts_cancelled += 1;
        }
    }

    fn set_latch(&mut self, on: bool) {
        // Final device gate first; periodic includes User-origin jobs too.
        self.latched = on;
        let buses: Vec<String> = self.channels.values().map(|c| c.bus.clone()).collect();
        for bus in &buses {
            // Buses of open channels always exist; an error here would only mean "no bus".
            let _ = if on {
                self.hub
                    .latch(bus, &[loom_can_sim::Origin::Agent, loom_can_sim::Origin::Script])
            } else {
                self.hub.release(bus)
            };
        }
        if on {
            self.cancel_bursts(ErrorCode::Stopped, |b| b.req.origin != Origin::User);
        }
        self.periodic_latch(on);
        // ISO-TP: abort agent/script transfers; user transfers and sim ECU endpoints keep running.
        let now = self.hub.now_ns();
        for ep in self.endpoints.values_mut() {
            if let Some(ch) = self.channels.get(&ep.channel) {
                if on {
                    ep.stop(&ch.emitter, now);
                } else {
                    ep.resume(&ch.emitter);
                }
            }
        }
    }

    fn send_request(&mut self, cx: &WorkerCtx, p: &SendParams, frame: CanFrame, req: &Req) -> WriteOutcome {
        if let Some(owner) = self.reserved(&p.channel, frame.id()) {
            // Reserved by an ISO-TP endpoint: refused for every origin, no takeover (plan D13).
            self.isotp_denied += 1;
            if let Some(ch) = self.channels.get(&p.channel) {
                let row = TxRow {
                    em: &ch.emitter,
                    generation: Some(ch.generation),
                    req,
                    frame: &frame,
                    count: p.count,
                };
                if !row.emit(WriteState::Failed, 0, Some(ErrorCode::Denied)) {
                    self.emit_failed += 1;
                }
            }
            return WriteOutcome {
                state: WriteState::Failed,
                detail: json!({"sent": 0}),
                error: Some(WorkerError {
                    code: ErrorCode::Denied,
                    message: format!(
                        "id {} on {} is reserved by ISO-TP endpoint {}",
                        id_text(frame.id()),
                        p.channel,
                        owner["endpoint"].as_str().unwrap_or("?")
                    ),
                    detail: owner,
                }),
            };
        }
        let override_of = self.periodic.owner_for(&p.channel, frame.id());
        if let Some(owner) = &override_of
            && (req.origin != Origin::User || p.count != 1)
        {
            return WriteOutcome {
                state: WriteState::Failed,
                detail: json!({"sent":0}),
                error: Some(WorkerError {
                    code: ErrorCode::Locked,
                    message: "periodic producer owns this id; only a user one-shot may override".into(),
                    detail: owner.clone(),
                }),
            };
        }
        let latched = self.latched() || cx.latch().is_on();
        let max_tags = self.limits.max_tags_per_channel;
        let Some(ch) = self.channels.get_mut(&p.channel) else {
            let em = cx.emitter(&p.channel).with_generation(None, Some(Env::Sim));
            let row = TxRow {
                em: &em,
                generation: None,
                req,
                frame: &frame,
                count: p.count,
            };
            if !row.emit(WriteState::Failed, 0, Some(ErrorCode::NotOpen)) {
                self.emit_failed += 1;
            }
            return failed(
                ErrorCode::NotOpen,
                format!("channel {} is not open", p.channel),
                0,
            );
        };
        let em = ch.emitter.clone();
        let row = TxRow {
            em: &em,
            generation: Some(ch.generation),
            req,
            frame: &frame,
            count: p.count,
        };
        let Some(slot) = sender_slot(req.origin) else {
            if !row.emit(WriteState::Failed, 0, Some(ErrorCode::Usage)) {
                self.emit_failed += 1;
            }
            return failed(ErrorCode::Usage, "origin device cannot send".into(), 0);
        };
        if latched && req.origin != Origin::User {
            if !row.emit(WriteState::Failed, 0, Some(ErrorCode::Stopped)) {
                self.emit_failed += 1;
            }
            return failed(
                ErrorCode::Stopped,
                "STOP latched: agent/script frames are refused".into(),
                0,
            );
        }
        let gap_ns = u64::from(p.gap_ms) * 1_000_000;
        let deferred = p.count > 1 && gap_ns > 0;
        if deferred && self.bursts.len() >= self.limits.max_pending_bursts {
            self.bursts_rejected += 1;
            if !row.emit(WriteState::Failed, 0, Some(ErrorCode::Busy)) {
                self.emit_failed += 1;
            }
            return failed(
                ErrorCode::Busy,
                format!(
                    "TX burst queue full ({} bursts still sending)",
                    self.limits.max_pending_bursts
                ),
                0,
            );
        }
        if !row.emit(WriteState::Accepted, 0, None) {
            self.emit_failed += 1;
        }
        let multi = p.count > 1;
        let now_count = if deferred { 1 } else { p.count };
        let sender = ch.senders[slot];
        let (mut first_seq, mut sent) = (None, 0u32);
        let mut error = None;
        for i in 0..now_count {
            if i > 0 && req.origin != Origin::User && cx.latch().is_on() {
                error = Some((ErrorCode::Stopped, "STOP latched during the burst".to_owned()));
                break;
            }
            match self.hub.send(sender, frame) {
                Ok(seq) => {
                    first_seq.get_or_insert(seq);
                    let tag = Tag {
                        seq,
                        request_id: Some(req.request_id.clone()),
                        client_id: req.client_id.clone(),
                        frame_index: multi.then_some(i),
                        isotp: None,
                        periodic: None,
                        override_of: override_of
                            .as_ref()
                            .and_then(|v| v["job"].as_str())
                            .map(str::to_owned),
                    };
                    ch.push_tag(tag, max_tags);
                    sent += 1;
                }
                Err(e) => {
                    error = Some((sim_error_code(&e), e.to_string()));
                    break;
                }
            }
        }
        if let Some((code, message)) = error {
            if !row.emit(WriteState::Failed, sent, Some(code)) {
                self.emit_failed += 1;
            }
            self.drain_all();
            return failed(code, message, sent);
        }
        if deferred {
            self.bursts.push(Burst {
                channel: p.channel.clone(),
                req: req.clone(),
                frame,
                count: p.count,
                next_index: 1,
                gap_ns,
                next_due_ns: self.hub.now_ns().saturating_add(gap_ns),
            });
        }
        // A burst still sending reports `accepted`; its pump emits the final `sent` row (A14). The reply
        // never claims more than the rows published before it (publish-before-reply, E7).
        let state = if deferred {
            WriteState::Accepted
        } else {
            WriteState::Sent
        };
        if !deferred && !row.emit(WriteState::Sent, sent, None) {
            self.emit_failed += 1;
        }
        // Tx echo (and zero-delay replies) before the reply.
        self.hub.poll();
        self.drain_all();
        WriteOutcome {
            state,
            detail: json!({
                "source_seq": first_seq,
                "frames": p.count,
                "sent": sent,
                "pending": p.count - sent,
                "override_of": override_of.as_ref().and_then(|v|v["job"].as_str()),
            }),
            error: None,
        }
    }

    // ---- calls ------------------------------------------------------------------------------

    fn open(&mut self, cx: &WorkerCtx, call: &Call) -> Result<Value, WorkerError> {
        let args = &call.args;
        let channel = required(args, "channel")?.to_owned();
        let bus = name_arg(args, "bus")?.unwrap_or(&channel).to_owned();
        if self.channels.contains_key(&channel) {
            return Err(err(ErrorCode::Busy, format!("channel {channel} is already open")));
        }
        if self.channels.len() >= self.limits.max_channels {
            return Err(err(
                ErrorCode::Busy,
                format!("at most {} channels per can_sim worker", self.limits.max_channels),
            ));
        }
        let configs = resolve_nodes(args.get("nodes"), &bus)?;
        self.latch.get_or_insert_with(|| cx.latch().clone());

        let mut attached: Vec<EndpointId> = Vec::new();
        let mut started: Vec<NodeId> = Vec::new();
        let res = self.attach_and_start(&channel, &bus, &configs, &mut attached, &mut started);
        let (observer, rx, skipped) = match res {
            Ok(v) => v,
            Err(e) => {
                for id in started {
                    let _ = self.hub.remove_node(id);
                }
                for id in attached {
                    let _ = self.hub.detach(id);
                }
                return Err(sim_err(&e));
            }
        };
        let senders = [attached[1], attached[2], attached[3]];
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let latched = self.latched() || cx.latch().is_on();
        let _ = if latched {
            self.hub
                .latch(&bus, &[loom_can_sim::Origin::Agent, loom_can_sim::Origin::Script])
        } else {
            self.hub.release(&bus)
        };
        let last_seq = self.hub.bus_stats(&bus).map_or(0, |s| s.last_seq);
        let emitter = cx
            .emitter(&channel)
            .with_generation(Some(generation), Some(Env::Sim));
        let started_names: Vec<String> = self
            .hub
            .nodes()
            .into_iter()
            .filter(|n| started.contains(&n.id))
            .map(|n| n.name)
            .collect();
        let mut d = draft(
            KIND_CHANNEL_STATE,
            emitter.clock().ts(TsSource::Driver),
            None,
            call.origin,
            Some(generation),
            json!({"state": "open", "backend": BACKEND, "channel": channel, "bus": bus,
                   "generation": generation}),
        );
        d.client_id = call.client_id.clone();
        d.request_id = call.request_id.clone();
        if emitter.priority(d).is_err() {
            self.emit_failed += 1;
        }
        self.channels.insert(
            channel.clone(),
            Channel {
                name: channel.clone(),
                bus: bus.clone(),
                generation,
                senders,
                device_sender: None,
                observer,
                rx,
                emitter,
                nodes: started,
                tags: VecDeque::new(),
                last_seq,
                last_ingest_mono_ns: 0,
                c: ChannelCounters::default(),
            },
        );
        Ok(json!({
            "channel": channel, "bus": bus, "backend": BACKEND, "generation": generation,
            "latched": latched, "nodes": started_names, "nodes_already_running": skipped,
        }))
    }

    /// Attaches observer + 3 senders (pushed to `attached` in that order) and starts nodes.
    fn attach_and_start(
        &mut self,
        channel: &str,
        bus: &str,
        configs: &[NodeConfig],
        attached: &mut Vec<EndpointId>,
        started: &mut Vec<NodeId>,
    ) -> Result<(EndpointId, LocalReceiver, Vec<String>), SimError> {
        let (observer, rx) = self.hub.attach_local_with_capacity(
            bus,
            &format!("{channel}/observer"),
            loom_can_sim::Origin::Device,
            self.limits.observer_queue,
        )?;
        attached.push(observer);
        for o in SENDER_ORIGINS {
            let name = format!("{channel}/{}", origin_name(o));
            let (id, _unread) = self.hub.attach_local_with_capacity(bus, &name, o, SENDER_QUEUE)?;
            attached.push(id);
        }
        let skipped = self.attach_nodes(bus, configs, started)?;
        Ok((observer, rx, skipped))
    }

    fn close(&mut self, call: &Call) -> Result<Value, WorkerError> {
        let channel = required(&call.args, "channel")?.to_owned();
        if !self.channels.contains_key(&channel) {
            return Err(err(ErrorCode::NotOpen, format!("channel {channel} is not open")));
        }
        self.stop_periodic_channel(&channel);
        self.cancel_bursts(ErrorCode::NotOpen, |b| b.channel == channel);
        // Frames already on the bus still reach the stream.
        self.drain_all();
        let ids: Vec<String> = self
            .endpoints
            .values()
            .filter(|e| e.channel == channel)
            .map(|e| e.id.clone())
            .collect();
        for id in ids {
            self.close_endpoint(&id, "channel closed");
        }
        let Some(ch) = self.channels.remove(&channel) else {
            return Err(err(ErrorCode::NotOpen, format!("channel {channel} is not open")));
        };
        for id in ch
            .senders
            .iter()
            .chain([&ch.observer])
            .chain(ch.device_sender.as_ref())
        {
            let _ = self.hub.detach(*id);
        }
        for id in &ch.nodes {
            // Unknown = already replaced by a later start; nothing to remove.
            let _ = self.hub.remove_node(*id);
        }
        let mut d = draft(
            KIND_CHANNEL_STATE,
            ch.emitter.clock().ts(TsSource::Driver),
            None,
            call.origin,
            Some(ch.generation),
            json!({"state": "closed", "backend": BACKEND, "channel": ch.name, "bus": ch.bus,
                   "generation": ch.generation}),
        );
        d.client_id = call.client_id.clone();
        d.request_id = call.request_id.clone();
        if ch.emitter.priority(d).is_err() {
            self.emit_failed += 1;
        }
        Ok(json!({"channel": ch.name, "state": "closed", "generation": ch.generation}))
    }

    fn node_json(&self, n: &NodeInfo) -> Value {
        let channels: Vec<&str> = self
            .channels
            .values()
            .filter(|c| c.bus == n.channel)
            .map(|c| c.name.as_str())
            .collect();
        json!({
            "name": n.name, "bus": n.channel, "channels": channels, "state": node_state(n.state),
            "tx_frames": n.tx_frames, "fault_dropped": n.fault_dropped,
            "deadline_overflow": n.deadline_overflow, "reply_errors": n.reply_errors,
            "pending": n.pending,
        })
    }

    fn node_list(&self) -> Vec<Value> {
        self.hub.nodes().iter().map(|n| self.node_json(n)).collect()
    }

    fn open_channel(&self, args: &Value) -> Result<&Channel, WorkerError> {
        let name = required(args, "channel")?;
        self.channels
            .get(name)
            .ok_or_else(|| err(ErrorCode::NotOpen, format!("channel {name} is not open")))
    }

    fn node_start(&mut self, args: &Value) -> Result<Value, WorkerError> {
        let ch = self.open_channel(args)?;
        let (name, bus) = (ch.name.clone(), ch.bus.clone());
        if args.get("nodes").is_none_or(Value::is_null) {
            return Err(err(ErrorCode::Usage, "missing `nodes`".into()));
        }
        let configs = resolve_nodes(args.get("nodes"), &bus)?;
        let mut started = Vec::new();
        let res = self.attach_nodes(&bus, &configs, &mut started);
        match res {
            Ok(skipped) => {
                let names: Vec<String> = self
                    .hub
                    .nodes()
                    .into_iter()
                    .filter(|n| started.contains(&n.id))
                    .map(|n| n.name)
                    .collect();
                if let Some(ch) = self.channels.get_mut(&name) {
                    ch.nodes.extend(started);
                }
                Ok(json!({"channel": name, "bus": bus, "started": names, "already_running": skipped}))
            }
            Err(e) => {
                for id in started {
                    let _ = self.hub.remove_node(id);
                }
                Err(sim_err(&e))
            }
        }
    }

    fn attach_nodes(
        &mut self,
        bus: &str,
        configs: &[NodeConfig],
        started: &mut Vec<NodeId>,
    ) -> Result<Vec<String>, SimError> {
        let mut skipped = Vec::new();
        for cfg in configs {
            let running = self
                .hub
                .nodes()
                .iter()
                .any(|n| n.name == cfg.name && n.channel == bus && n.state == NodeState::Running);
            if running {
                skipped.push(cfg.name.clone());
                continue;
            }
            started.push(self.hub.start_node(cfg.clone())?);
        }
        Ok(skipped)
    }

    fn node_stop(&mut self, args: &Value) -> Result<Value, WorkerError> {
        let bus = self.open_channel(args)?.bus.clone();
        let name = required(args, "name")?;
        let node = self
            .hub
            .nodes()
            .into_iter()
            .find(|n| n.name == name && n.channel == bus && n.state == NodeState::Running)
            .ok_or_else(|| {
                err(
                    ErrorCode::NotFound,
                    format!("no running node {name} on bus {bus}"),
                )
            })?;
        self.hub.stop_node(node.id).map_err(|e| sim_err(&e))?;
        Ok(json!({"name": name, "bus": bus, "state": "stopped"}))
    }

    fn status(&self, args: &Value) -> Result<Value, WorkerError> {
        let only = name_arg(args, "channel")?;
        if let Some(c) = only
            && !self.channels.contains_key(c)
        {
            return Err(err(ErrorCode::NotOpen, format!("channel {c} is not open")));
        }
        let channels: Vec<Value> = self
            .channels
            .values()
            .filter(|c| only.is_none_or(|o| o == c.name))
            .map(|c| self.channel_status(c))
            .collect();
        Ok(json!({
            "backend": BACKEND,
            "latch": self.latched(),
            "channels": channels,
            "nodes": self.node_list(),
            "pending_bursts": self.bursts.len(),
            "max_pending_bursts": self.limits.max_pending_bursts,
            "bursts_rejected": self.bursts_rejected,
            "bursts_cancelled": self.bursts_cancelled,
            "emit_failed": self.emit_failed,
            "periodic_jobs": self.periodic.scheduler.len(),
            "periodic": {"jobs":self.periodic.scheduler.len(),"max_jobs":loom_can_periodic::MAX_JOBS,
                "pending_patches":self.periodic.scheduler.pending_patches(),"failures":self.periodic.failures,"inhibited":self.periodic.inhibited()},
            "isotp": {
                "endpoints": self.endpoints.len(),
                "max_endpoints": MAX_ENDPOINTS,
                "max_per_channel": MAX_ENDPOINTS_PER_CHANNEL,
                "raw_tx_denied": self.isotp_denied,
            },
        }))
    }

    fn channel_status(&self, c: &Channel) -> Value {
        let stats = self.hub.bus_stats(&c.bus);
        let obs = stats
            .as_ref()
            .and_then(|s| s.endpoints.iter().find(|e| e.id == c.observer));
        let latched: Vec<&str> = stats
            .as_ref()
            .map(|s| s.latched.iter().map(|o| origin_name(*o)).collect())
            .unwrap_or_default();
        json!({
            "channel": c.name, "bus": c.bus, "backend": BACKEND, "state": "open",
            "generation": c.generation,
            "latched": latched,
            "frames_total": stats.as_ref().map_or(0, |s| s.frames_total),
            "frames_by_origin": by_origin(stats.as_ref().map_or([0; 4], |s| s.frames_by_origin)),
            "latch_rejected": by_origin(stats.as_ref().map_or([0; 4], |s| s.latch_rejected)),
            "observer": {
                "delivered": obs.map_or(0, |e| e.delivered),
                "drops": obs.map_or(0, |e| e.drops.total),
                "gaps": obs.map_or(0, |e| e.drops.gaps.len() as u64 + e.drops.gaps_evicted),
                "queued": c.rx.len(),
                "capacity": c.rx.capacity(),
            },
            "frames_emitted": c.c.frames,
            "emit_failed": c.c.emit_failed,
            "source_gap_frames": c.c.source_gap_frames,
            "source_gaps_reported": c.c.source_gaps_reported,
            "tags": {
                "live": c.tags.len(), "max": self.limits.max_tags_per_channel,
                "evicted": c.c.tags_evicted, "stale": c.c.tags_stale,
                "untagged_echo": c.c.untagged_echo,
            },
            "pending_bursts": self.bursts.iter().filter(|b| b.channel == c.name).count(),
            "nodes": c.nodes.len(),
            "isotp_endpoints": self.endpoints.values().filter(|e| e.channel == c.name).count(),
            "periodic_jobs": self.periodic.count_channel(&c.name),
        })
    }

    // ---- ISO-TP ------------------------------------------------------------------------------

    /// Endpoint that reserves `id` for sending on `channel` (detail for `E_DENIED`).
    fn reserved(&self, channel: &str, id: CanId) -> Option<Value> {
        self.endpoints
            .values()
            .find(|e| e.channel == channel && e.tx_id() == id)
            .map(|e| {
                json!({
                    "reason": "tx_reserved",
                    "endpoint": e.id,
                    "owner": e.owner,
                    "opened_by": e.opener,
                    "client_id": e.client_id,
                    "id": id_text(id),
                })
            })
    }

    fn model(&self, name: &str) -> Option<Box<dyn crate::isotp::IsoTpServerModel>> {
        match self.models.iter().find(|(n, _)| n == name) {
            Some((_, f)) => Some(f()),
            None => builtin_model(name),
        }
    }

    fn isotp_open(&mut self, cx: &WorkerCtx, call: &Call) -> Result<Value, WorkerError> {
        let p: OpenParams = serde_json::from_value(call.args.clone())
            .map_err(|e| err(ErrorCode::Usage, format!("can.isotp.open: {e}")))?;
        let cfg = p.config().map_err(|(c, m)| err(c, m))?;
        if let Some(owner) = self.periodic.owner_for(&p.channel, cfg.tx_id) {
            return Err(WorkerError {
                code: ErrorCode::Locked,
                message: "periodic producer owns ISO-TP tx_id".into(),
                detail: owner,
            });
        }
        let Some(ch) = self.channels.get(&p.channel) else {
            return Err(err(
                ErrorCode::NotOpen,
                format!("channel {} is not open", p.channel),
            ));
        };
        let (generation, bus) = (ch.generation, ch.bus.clone());
        if let Some(other) = self
            .endpoints
            .values()
            .find(|e| e.channel == p.channel && (e.tx_id() == cfg.tx_id || e.rx_id() == cfg.rx_id))
        {
            return Err(WorkerError {
                code: ErrorCode::Locked,
                message: format!(
                    "{} on {} already uses tx {} / rx {}",
                    other.id,
                    p.channel,
                    id_text(other.tx_id()),
                    id_text(other.rx_id())
                ),
                detail: json!({
                    "endpoint": other.id, "owner": other.owner, "opened_by": other.opener,
                    "client_id": other.client_id, "tx_id": id_text(other.tx_id()),
                    "rx_id": id_text(other.rx_id()),
                }),
            });
        }
        let on_channel = self.endpoints.values().filter(|e| e.channel == p.channel).count();
        if on_channel >= MAX_ENDPOINTS_PER_CHANNEL || self.endpoints.len() >= MAX_ENDPOINTS {
            return Err(err(
                ErrorCode::Busy,
                format!(
                    "at most {MAX_ENDPOINTS_PER_CHANNEL} ISO-TP endpoints per channel and {MAX_ENDPOINTS} in total"
                ),
            ));
        }
        let model = match p.model.as_deref() {
            Some(name) => Some(
                self.model(name)
                    .ok_or_else(|| err(ErrorCode::Usage, format!("unknown ISO-TP server model {name:?}")))?,
            ),
            None => None,
        };
        if p.role == TpRole::Server
            && let Some(ch) = self.channels.get_mut(&p.channel)
            && ch.device_sender.is_none()
        {
            let name = format!("{}/isotp-ecu", p.channel);
            let (id, _unread) = self
                .hub
                .attach_local_with_capacity(&bus, &name, loom_can_sim::Origin::Device, SENDER_QUEUE)
                .map_err(|e| sim_err(&e))?;
            ch.device_sender = Some(id);
        }
        self.latch.get_or_insert_with(|| cx.latch().clone());
        let id = format!("tp{}", self.next_endpoint);
        let epoch = self.next_epoch;
        let mut ep = Endpoint::new(
            id.clone(),
            p.channel.clone(),
            generation,
            epoch,
            &p,
            cfg,
            call.origin,
            call.client_id.clone(),
            model,
        )
        .map_err(|(c, m)| err(c, m))?;
        self.next_endpoint += 1;
        self.next_epoch += 1;
        if let Some(ch) = self.channels.get(&p.channel) {
            ep.emit_state(&ch.emitter, "open", None);
        }
        let info = ep.info();
        self.endpoints.insert(id, ep);
        Ok(info)
    }

    fn endpoint_arg(&self, args: &Value) -> Result<String, WorkerError> {
        let id = required(args, "endpoint")?;
        if self.endpoints.contains_key(id) {
            Ok(id.to_owned())
        } else {
            Err(err(
                ErrorCode::NotOpen,
                format!("ISO-TP endpoint {id} is not open"),
            ))
        }
    }

    /// Removes an endpoint: aborts work in progress, publishes `closed`.
    fn close_endpoint(&mut self, id: &str, reason: &str) -> Value {
        let Some(mut ep) = self.endpoints.remove(id) else {
            return Value::Null;
        };
        let now = self.hub.now_ns();
        let Some(ch) = self.channels.get(&ep.channel) else {
            return Value::Null;
        };
        let abort = ep.abort(&ch.emitter, now, reason);
        ep.emit_state(&ch.emitter, "closed", Some(reason));
        json!({"endpoint": ep.id, "epoch": ep.epoch, "state": "closed", "abort": abort})
    }

    fn isotp_close(&mut self, call: &Call) -> Result<Value, WorkerError> {
        let id = self.endpoint_arg(&call.args)?;
        Ok(self.close_endpoint(&id, "closed"))
    }

    fn isotp_abort(&mut self, call: &Call) -> Result<Value, WorkerError> {
        let id = self.endpoint_arg(&call.args)?;
        let now = self.hub.now_ns();
        let (Some(ep), channels) = (self.endpoints.get_mut(&id), &self.channels) else {
            return Err(err(
                ErrorCode::NotOpen,
                format!("ISO-TP endpoint {id} is not open"),
            ));
        };
        let ch = channels
            .get(&ep.channel)
            .ok_or_else(|| err(ErrorCode::NotOpen, format!("channel {} is not open", ep.channel)))?;
        Ok(ep.abort(&ch.emitter, now, "abort"))
    }

    fn isotp_list(&self, args: &Value) -> Result<Value, WorkerError> {
        let only = name_arg(args, "channel")?;
        let eps: Vec<Value> = self
            .endpoints
            .values()
            .filter(|e| only.is_none_or(|c| c == e.channel))
            .map(Endpoint::info)
            .collect();
        Ok(json!({
            "endpoints": eps,
            "max_per_channel": MAX_ENDPOINTS_PER_CHANNEL,
            "max_endpoints": MAX_ENDPOINTS,
        }))
    }

    fn isotp_recv(&mut self, args: &Value) -> Result<Value, WorkerError> {
        let id = self.endpoint_arg(args)?;
        let after = tp_integer(args, "after_pdu_seq", Some(0))?;
        let max = tp_integer(args, "max_pdus", Some(crate::isotp::MAX_RECV_PDUS as u64))?;
        let timeout = tp_integer(args, "timeout_ms", Some(0))?;
        if max == 0 || max > crate::isotp::MAX_RECV_PDUS as u64 || timeout > 60_000 {
            return Err(err(
                ErrorCode::Usage,
                "max_pdus must be1..16; timeout_ms must be0..60000".into(),
            ));
        }
        // One bounded drain; gateway owns waiting/polling, not this worker call.
        self.drain_all();
        self.endpoints
            .get(&id)
            .ok_or_else(|| err(ErrorCode::NotOpen, "endpoint closed".into()))?
            .recv(after, max as usize)
            .map_err(|(c, m)| err(c, m))
    }

    fn isotp_barrier(&mut self, args: &Value) -> Result<Value, WorkerError> {
        let id = self.endpoint_arg(args)?;
        let epoch = tp_integer(args, "epoch", None)?;
        let deadline = tp_integer(args, "deadline_mono_ns", None)?;
        let timeout = tp_integer(args, "timeout_ms", Some(0))?;
        if timeout > 60_000 {
            return Err(err(ErrorCode::Usage, "timeout_ms exceeds60000".into()));
        }
        let ep = &self.endpoints[&id];
        if ep.epoch != epoch {
            return Err(WorkerError {
                code: ErrorCode::CursorGap,
                message: "ISO-TP endpoint epoch changed".into(),
                detail: json!({"code":"E_EPOCH_CHANGED","expected_epoch":epoch.to_string(),"current_epoch":ep.epoch.to_string()}),
            });
        }
        // Synchronously feed transports and publish rx_start after polling due sim ingress.
        // Bounded; no transport output submission and no waiting for future time.
        let polled = self.hub.poll();
        self.drain_all();
        let ep = self
            .endpoints
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::NotOpen, "endpoint closed".into()))?;
        let ch = self
            .channels
            .get(&ep.channel)
            .ok_or_else(|| err(ErrorCode::NotOpen, "channel closed".into()))?;
        let head = self.hub.bus_stats(&ch.bus).map_or(ch.last_seq, |s| s.last_seq);
        let drained = ch.rx.is_empty() && ch.last_seq >= head;
        if ch.rx.is_empty() && ch.last_seq < head {
            ep.note_ingress_loss();
        }
        let (event_seq, proof_ok) = ep.barrier_status();
        let now = ch.emitter.clock().mono_ns();
        let covered = if !proof_ok {
            0
        } else if drained && !polled.more_pending {
            now
        } else {
            ch.last_ingest_mono_ns
        };
        Ok(
            json!({"endpoint":id,"epoch":epoch,"covered_ingest_seq":ch.last_seq.to_string(),
            "transport_event_seq":event_seq.to_string(),"covered_mono_ns":covered.to_string(),
            "complete":proof_ok && drained && !polled.more_pending && now >= deadline}),
        )
    }

    fn isotp_cancel(&mut self, w: &Write) -> WriteOutcome {
        let result = (|| {
            let id = self.endpoint_arg(&w.op)?;
            let transfer = required(&w.op, "transfer")?;
            if transfer.len() > 64 {
                return Err(err(ErrorCode::Usage, "transfer id too long".into()));
            }
            let ep = self
                .endpoints
                .get_mut(&id)
                .ok_or_else(|| err(ErrorCode::NotOpen, "endpoint closed".into()))?;
            let ch = self
                .channels
                .get(&ep.channel)
                .ok_or_else(|| err(ErrorCode::NotOpen, "channel closed".into()))?;
            ep.cancel(
                transfer,
                &ReqMeta {
                    origin: w.origin,
                    client_id: w.client_id.clone(),
                    request_id: Some(w.request_id.clone()),
                },
                &ch.emitter,
                self.hub.now_ns(),
            )
            .map_err(|(c, m)| err(c, m))
        })();
        match result {
            Ok(detail) => WriteOutcome {
                state: WriteState::Accepted,
                detail,
                error: None,
            },
            Err(error) => WriteOutcome {
                state: WriteState::Failed,
                detail: json!({"accepted":false}),
                error: Some(error),
            },
        }
    }

    /// `can.isotp.send` (write): queue one PDU → `accepted` (the UDS request row owns `write_state`).
    fn isotp_send(&mut self, cx: &WorkerCtx, w: &Write) -> WriteOutcome {
        let refuse = |code: ErrorCode, message: String| WriteOutcome {
            state: WriteState::Failed,
            detail: json!({"accepted": false}),
            error: Some(err(code, message)),
        };
        let op: SendOp = match serde_json::from_value(w.op.clone()) {
            Ok(o) => o,
            Err(e) => return refuse(ErrorCode::Usage, format!("can.isotp.send: {e}")),
        };
        if w.request_id.len() > 128 || w.client_id.as_ref().is_some_and(|s| s.len() > 128) {
            return refuse(ErrorCode::Usage, "request/client id exceeds128 bytes".into());
        }
        let pdu = match parse_pdu(&op.pdu) {
            Ok(p) => p,
            Err(e) => return refuse(ErrorCode::Usage, format!("can.isotp.send: {e}")),
        };
        let latched = self.latched() || cx.latch().is_on();
        let now = self.hub.now_ns();
        let Some(ep) = self.endpoints.get_mut(&op.endpoint) else {
            return refuse(
                ErrorCode::NotOpen,
                format!("ISO-TP endpoint {} is not open", op.endpoint),
            );
        };
        if ep.role == TpRole::Server {
            return refuse(
                ErrorCode::Usage,
                format!("{} is a server endpoint; its model answers", ep.id),
            );
        }
        if latched && w.origin != Origin::User {
            return refuse(
                ErrorCode::Stopped,
                "STOP latched: agent/script ISO-TP sends are refused".into(),
            );
        }
        let Some(ch) = self.channels.get(&ep.channel) else {
            return refuse(ErrorCode::NotOpen, format!("channel {} is not open", ep.channel));
        };
        let req = ReqMeta {
            request_id: Some(w.request_id.clone()),
            client_id: w.client_id.clone(),
            origin: w.origin,
        };
        let (id, epoch) = (ep.id.clone(), ep.epoch);
        let transfer = match ep.send(&pdu, req, &ch.emitter, now) {
            Ok(t) => t,
            Err((code, message)) => return refuse(code, message),
        };
        // First frame(s) and zero-delay reactions before the reply.
        self.settle();
        WriteOutcome {
            state: WriteState::Accepted,
            detail: json!({
                "accepted": true,
                "endpoint": id,
                "epoch": epoch,
                "transfer": format!("{}.{}", transfer.generation, transfer.sequence),
                "len": pdu.len(),
            }),
            error: None,
        }
    }
}
