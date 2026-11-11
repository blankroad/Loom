//! Connection manager (plan U-07/U-08): targets, runners, live events, lease, abort, reconnect.
//!
//! One [`Target`] per connected ECU. Its runner is a `loom-uds-runner` [`RunnerHandle`]; a
//! per-target gate keeps one of *our* requests inside the runner at a time so every runner event
//! maps to exactly one `request_id` (see `HOOK(runner-priority)` — replaced by the runner's own
//! priority queue + request tags when they land). `connect --sim` starts a [`DoipEntity`] on
//! 127.0.0.1 with the virtual ECU behind it and connects with the real DoIP client, so simulated
//! traffic takes exactly the hardware path.
//!
//! Events are published through the caller's [`CallCtx`] **before** the handler returns
//! (publish-before-reply, E7): `uds.tx` requested/accepted (only for rows the gateway did not
//! open itself) and sent, `uds.rx` per received PDU (0x78
//! and late responses included), `uds.pending`, `uds.result`, and `uds.state` when the
//! [`TargetSession`] changes. Background events (link loss, reconnect) use the context of the
//! call that connected the target (`publisher`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use loom_domain_api::{CallCtx, DomainError, EventDraft};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, PayloadBytes, Risk, WriteState};
use loom_proto::num::U64;
use loom_uds_codec::response::{parse_dtc_by_status_response, parse_read_did_response};
use loom_uds_codec::{DidLookup, NEGATIVE_RESPONSE_SID, StandardDids, classify_response, nrc_name};
use loom_uds_doip::{ActivationType, ClientConfig, DoipClient, DoipEntity, EntityConfig, LinkEvent};
use loom_uds_runner::{
    AbortKind, AbortScope, Access, DoipTransport, MonoClock, Priority, RejectReason, Runner, RunnerConfig,
    RunnerEvent, RunnerHandle, RunnerObserver, TxnRequest, TxnResult, TxnStatus,
};
use loom_uds_sim::{SimEcu, SimNetwork};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::events;
use crate::lanes::{GateError, Lane, PriorityGate};
use crate::payload::{DecodedDid, DtcItem, RowPayload, RxKind, RxPayload};
use crate::risk::classify_pdu;
use crate::session::{Lease, TargetSession};
use crate::{KIND_PENDING, KIND_RESULT, KIND_TX};

/// Most connected targets. A new `connect` beyond this is refused (`E_BUSY`).
pub const MAX_TARGETS: usize = 16;
/// Runner events buffered per target between the runner task and the publishing handler.
/// Full: the event is dropped and counted (`dropped_events` in results); rows still finish
/// because the final `uds.result` comes from the transaction result, not from this queue.
pub const CAP_RUNNER_EVENTS: usize = 256;
/// Timed-out requests remembered per target to attribute late responses. Oldest is forgotten.
pub const MAX_TIMED_OUT_TRACKED: usize = 16;
/// Write log for `others_since_last` (all targets). Oldest entries are dropped.
pub const MAX_WRITE_LOG: usize = 1024;
/// Clients with a remembered `others_since_last` cursor. Oldest-inserted is evicted.
pub const MAX_CLIENT_CURSORS: usize = 256;
/// Reconnect attempts after a link loss, [`RECONNECT_INTERVAL`] apart.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 20;
pub const RECONNECT_INTERVAL: Duration = Duration::from_millis(250);
/// Request timeouts (total deadline incl. queue wait), clamped to [`MAX_REQUEST_TIMEOUT`].
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(2);
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
pub const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// `connect --sim` addressing.
pub const SIM_ENTITY_LA: u16 = 0x1000;
pub const SIM_ECU_LA: u16 = 0x1001;
pub const DEFAULT_TESTER_LA: u16 = 0x0E80;

/// Hub time (`ts.mono_ns` of gateway events), anchored once from the call that connected the
/// target ([`CallCtx::now_mono_ns`]) and advanced with the same monotonic source the runner uses,
/// so row timestamps and event timestamps share one time base.
#[derive(Debug, Clone, Copy)]
pub struct HubClock {
    at: Instant,
    base_ns: u64,
}

impl HubClock {
    /// Anchor at the hub time `ctx` reports now.
    pub fn from_ctx(ctx: &CallCtx) -> Self {
        HubClock {
            at: Instant::now(),
            base_ns: ctx.now_mono_ns(),
        }
    }

    /// Hub monotonic nanoseconds now.
    pub fn mono_ns(&self) -> u64 {
        let d = u64::try_from(self.at.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.base_ns.saturating_add(d)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding one of these short locks leaves plain data behind; keep going.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Owned copy of a [`RunnerEvent`] (the observer runs inside the runner task).
#[derive(Debug, Clone, PartialEq, Eq)]
enum OwnedEvent {
    Sent { mono_ns: u64 },
    Pending { count: u32, mono_ns: u64 },
    Late { data: Vec<u8>, mono_ns: u64 },
    Released,
}

/// `RunnerObserver` → bounded queue (never blocks the runner).
#[derive(Debug)]
struct QueueObserver {
    tx: mpsc::Sender<OwnedEvent>,
    dropped: AtomicU64,
}

impl RunnerObserver for QueueObserver {
    fn on_event(&self, event: &RunnerEvent<'_>) {
        let ev = match event {
            RunnerEvent::Sent { mono_ns, .. } => OwnedEvent::Sent { mono_ns: *mono_ns },
            RunnerEvent::Pending { count, mono_ns } => OwnedEvent::Pending {
                count: *count,
                mono_ns: *mono_ns,
            },
            RunnerEvent::Late { data, mono_ns, .. } => OwnedEvent::Late {
                data: data.to_vec(),
                mono_ns: *mono_ns,
            },
            RunnerEvent::QuarantineReleased { .. } => OwnedEvent::Released,
        };
        if self.tx.try_send(ev).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// What `presend_check` needs, readable with one short lock and no I/O.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresendSnap {
    pub generation: u32,
    /// The user holds the lease (agent/script R1+ writes are refused).
    pub user_lease: bool,
    /// SIDs of requests that timed out and may still answer late (runner U, approximated).
    pub quarantined: BTreeSet<u8>,
}

/// Shared with the write hooks.
#[derive(Debug, Default)]
pub struct PresendState {
    targets: Mutex<BTreeMap<String, PresendSnap>>,
}

impl PresendState {
    pub fn get(&self, target: &str) -> Option<PresendSnap> {
        lock(&self.targets).get(target).cloned()
    }

    fn update(&self, target: &str, f: impl FnOnce(&mut PresendSnap)) {
        let mut m = lock(&self.targets);
        if let Some(s) = m.get_mut(target) {
            f(s);
        } else if m.len() < MAX_TARGETS {
            let mut s = PresendSnap::default();
            f(&mut s);
            m.insert(target.to_owned(), s);
        }
    }
}

#[derive(Debug, Default)]
struct WriteLog {
    next: u64,
    entries: VecDeque<(u64, Origin, String)>,
    cursors: BTreeMap<String, u64>,
}

impl WriteLog {
    /// Writes by other clients since this client's last call; advances the cursor.
    fn others_since_last(&mut self, client: &str) -> Value {
        let from = self.cursors.get(client).copied().unwrap_or(0);
        let others: Vec<&(u64, Origin, String)> = self
            .entries
            .iter()
            .filter(|(n, _, c)| *n > from && c != client)
            .collect();
        let origins: BTreeSet<String> = others
            .iter()
            .map(|(_, o, _)| {
                serde_json::to_value(o)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            })
            .collect();
        let count = others.len();
        if !self.cursors.contains_key(client) && self.cursors.len() >= MAX_CLIENT_CURSORS {
            self.cursors.pop_first();
        }
        self.cursors.insert(client.to_owned(), self.next);
        json!({"count": count, "origins": origins, "from_cursor": format!("w:{from}")})
    }

    fn record(&mut self, origin: Origin, client: &str) {
        self.next += 1;
        if self.entries.len() >= MAX_WRITE_LOG {
            self.entries.pop_front();
        }
        self.entries.push_back((self.next, origin, client.to_owned()));
    }
}

/// Runner, its event queue and the DoIP client — only touched while holding the gate.
#[derive(Debug)]
struct Gate {
    handle: Option<RunnerHandle>,
    task: Option<JoinHandle<Runner>>,
    events: mpsc::Receiver<OwnedEvent>,
    client: Option<DoipClient>,
}

/// Simulated side kept alive while the target exists.
#[derive(Debug)]
struct SimSide {
    entity: DoipEntity,
    network: Arc<Mutex<SimNetwork>>,
}

/// One connected ECU.
#[derive(Debug)]
pub struct Target {
    name: String,
    env: Env,
    la: u16,
    auto_reconnect: bool,
    client_cfg: Mutex<ClientConfig>,
    session: Mutex<TargetSession>,
    gate: tokio::sync::Mutex<Gate>,
    lanes: PriorityGate,
    /// Runner handle clone for abort (refreshed on every restart).
    ctl_handle: Mutex<Option<RunnerHandle>>,
    observer: Arc<QueueObserver>,
    publisher: Mutex<Option<CallCtx>>,
    timed_out: Mutex<VecDeque<(u8, String)>>,
    sim: Option<SimSide>,
    clock: HubClock,
    presend: Arc<PresendState>,
}

/// Parameters of one request handed to [`UdsManager::request`].
#[derive(Debug, Clone)]
pub struct ReqSpec {
    pub target: String,
    pub pdu: Vec<u8>,
    pub timeout: Option<Duration>,
    /// Defaults to the call's request id.
    pub request_id: Option<String>,
    /// Safe-state recovery from a viewer (runs in the Abort lane, not cancelled by `!`).
    pub recovery: bool,
}

/// Result of one request: the final row (as published) and the runner result.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub request_id: String,
    pub row: RowPayload,
    pub result: Option<TxnResult>,
    pub others_since_last: Value,
    pub dropped_events: u64,
}

impl Outcome {
    /// `Ok(data)` for positive / suppressed results, `Err` with the row as detail otherwise.
    pub fn into_result(self) -> Result<Value, DomainError> {
        let error = self.result.as_ref().and_then(|r| r.error);
        let code = match (&self.row.block, error) {
            (Some(_), _) => Some(ErrorCode::Unsupported),
            (None, e) => e,
        };
        let data = json!({
            "request_id": self.request_id,
            "row": self.row,
            "others_since_last": self.others_since_last,
            "dropped_events": self.dropped_events,
        });
        match code {
            None => Ok(data),
            Some(code) => {
                // Top-level `write_state` is what the gateway records for a failed call (audit
                // `write.result`, fallback row): an aborted write after hand-off stays
                // `outcome_unknown` there too, never `failed`.
                let mut data = data;
                data["write_state"] = json!(self.row.write_state);
                let msg = match (self.row.nrc, &self.row.nrc_name) {
                    (Some(n), Some(name)) => format!("NRC 0x{n:02X} {name}"),
                    (Some(n), None) => format!("NRC 0x{n:02X}"),
                    _ => self.row.reason.clone(),
                };
                Err(DomainError::new(code, msg).with_detail(data))
            }
        }
    }
}

fn ms_between(a: Option<u64>, b: Option<u64>) -> Option<f64> {
    let (a, b) = (a?, b?);
    #[allow(clippy::cast_precision_loss)]
    let ms = b.saturating_sub(a) as f64 / 1e6;
    Some((ms * 1000.0).round() / 1000.0)
}

/// SID a response belongs to (`7F sid nrc` or positive `sid + 0x40`).
fn response_sid(data: &[u8]) -> Option<u8> {
    match data {
        [NEGATIVE_RESPONSE_SID, sid, ..] => Some(*sid),
        [s, ..] if *s >= 0x40 => Some(s - 0x40),
        _ => None,
    }
}

fn decode_dids(pdu: &[u8], resp: &[u8]) -> Option<Vec<DecodedDid>> {
    let recs = parse_read_did_response(pdu, resp, &StandardDids).ok()?;
    Some(
        recs.iter()
            .map(|r| {
                match StandardDids
                    .did_def(r.did)
                    .and_then(|d| d.decode(&r.data).ok().map(|v| (d.name, v)))
                {
                    Some((name, v)) => DecodedDid::new(r.did, Some(name), &v),
                    None => DecodedDid {
                        did: format!("{:04X}", r.did),
                        name: None,
                        value: json!(hex::encode_upper(&r.data)),
                        unit: None,
                    },
                }
            })
            .collect(),
    )
}

impl Target {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.clock.mono_ns())
    }

    /// Stop the runner task, change the runner, start it again (U and timing are preserved).
    /// Caller holds the priority gate and the data lock, so no request is inside the runner.
    async fn restart_runner(&self, gate: &mut Gate, f: impl FnOnce(&mut Runner)) {
        drop(lock(&self.ctl_handle).take());
        drop(gate.handle.take());
        let Some(task) = gate.task.take() else { return };
        match task.await {
            Ok(mut runner) => {
                f(&mut runner);
                let (h, t) = RunnerHandle::spawn(runner);
                *lock(&self.ctl_handle) = Some(h.clone());
                gate.handle = Some(h);
                gate.task = Some(t);
            }
            Err(e) => tracing::error!(target = %self.name, "uds runner task failed: {e}"),
        }
    }

    fn ctl(&self) -> Option<RunnerHandle> {
        lock(&self.ctl_handle).clone()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn env(&self) -> Env {
        self.env
    }

    pub fn generation(&self) -> u32 {
        lock(&self.session).generation()
    }

    /// `uds.state` payload now.
    pub fn state(&self) -> crate::payload::StatePayload {
        lock(&self.session).state_payload()
    }

    /// Read a DID straight from the simulated ECU (tests, sim only).
    pub fn sim_did(&self, did: u16) -> Option<Vec<u8>> {
        let sim = self.sim.as_ref()?;
        lock(&sim.network)
            .get(self.la)
            .and_then(|e| e.did(did).map(<[u8]>::to_vec))
    }

    /// Drop every connection of the simulated DoIP entity (link-loss tests, sim only).
    pub fn sim_close_connections(&self) -> bool {
        self.sim
            .as_ref()
            .map(|s| s.entity.close_all_connections())
            .is_some()
    }

    /// Mutate the simulated ECU (fault injection in tests, sim only).
    pub fn with_sim_ecu<R>(&self, f: impl FnOnce(&mut SimEcu) -> R) -> Option<R> {
        let sim = self.sim.as_ref()?;
        let mut net = lock(&sim.network);
        net.get_mut(self.la).map(f)
    }

    fn publish(&self, ctx: &CallCtx, d: EventDraft) {
        if let Err(e) = ctx.publish(d) {
            tracing::warn!(target = %self.name, "uds publish failed: {e}");
        }
    }

    fn publish_bg(&self, d: EventDraft) {
        let ctx = lock(&self.publisher).clone();
        match ctx {
            Some(ctx) => self.publish(&ctx, d),
            None => tracing::warn!(target = %self.name, "uds background event without publisher"),
        }
    }

    fn state_draft(&self) -> EventDraft {
        events::state(self.env, &self.state())
    }

    fn sync_presend(&self) {
        let (generation, user_lease) = {
            let s = lock(&self.session);
            (
                s.generation(),
                s.lease().is_some_and(|l| l.owner_origin == Origin::User),
            )
        };
        self.presend.update(&self.name, |p| {
            p.generation = generation;
            p.user_lease = user_lease;
        });
    }

    fn row_draft(&self, kind: &str, rid: &str, row: &RowPayload) -> EventDraft {
        events::row(kind, rid, self.generation(), self.env, row)
    }

    fn rx_payload(&self, req: &[u8], resp: &[u8], mono_ns: u64, late: bool) -> RxPayload {
        let (kind, nrc) = if late {
            let (k, n) = RxKind::from_codec(classify_response(req, resp));
            (
                if matches!(k, RxKind::Positive | RxKind::Negative | RxKind::Pending) {
                    k
                } else {
                    RxKind::Foreign
                },
                n,
            )
        } else {
            RxKind::from_codec(classify_response(req, resp))
        };
        RxPayload {
            target: self.name.clone(),
            bytes: PayloadBytes::hex(resp),
            kind,
            nrc,
            nrc_name: nrc.and_then(nrc_name).map(Into::into),
            late,
            t_rx_mono_ns: U64(mono_ns),
        }
    }

    /// Late response: attribute it to the most recent timed-out request with the same SID.
    fn publish_late(&self, ctx: &CallCtx, data: &[u8], mono_ns: u64) {
        let owner = response_sid(data).and_then(|sid| {
            lock(&self.timed_out)
                .iter()
                .rev()
                .find(|(s, _)| *s == sid)
                .map(|(_, rid)| rid.clone())
        });
        let mut p = self.rx_payload(&[], data, mono_ns, true);
        let (kind, nrc) = match data {
            [NEGATIVE_RESPONSE_SID, _, n] => (
                if *n == 0x78 {
                    RxKind::Pending
                } else {
                    RxKind::Negative
                },
                Some(*n),
            ),
            _ => (RxKind::Positive, None),
        };
        p.kind = kind;
        p.nrc = nrc;
        p.nrc_name = nrc.and_then(nrc_name).map(Into::into);
        self.publish(ctx, events::rx(owner.as_deref(), self.generation(), self.env, &p));
    }

    fn on_link_down(&self, cause: &str) {
        lock(&self.session).link_down(cause);
        self.sync_presend();
        self.publish_bg(self.state_draft());
    }

    /// Reconnect with generation + 1 and swap the runner's transport. Returns the new link
    /// event receiver, or `None` after [`MAX_RECONNECT_ATTEMPTS`].
    async fn reconnect(&self) -> Option<mpsc::Receiver<LinkEvent>> {
        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            tokio::time::sleep(RECONNECT_INTERVAL).await;
            let generation = lock(&self.session).generation().saturating_add(1);
            let cfg = {
                let mut c = lock(&self.client_cfg);
                c.generation = u64::from(generation);
                c.clone()
            };
            let (client, link_rx) = match DoipClient::connect(cfg).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::info!(target = %self.name, attempt, "uds reconnect failed: {e}");
                    continue;
                }
            };
            let doip_target = match client.open_target(self.la) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(target = %self.name, "uds reconnect open_target: {e}");
                    continue;
                }
            };
            {
                let _permit = self
                    .lanes
                    .acquire(Lane::Control, Origin::Script, "reconnect")
                    .await;
                let mut gate = self.gate.lock().await;
                self.restart_runner(&mut gate, |r| {
                    r.replace_transport(Box::new(DoipTransport::new(doip_target)))
                })
                .await;
                gate.client = Some(client);
            }
            let now = self.now();
            lock(&self.session).link_up(now);
            lock(&self.timed_out).clear();
            self.presend.update(&self.name, |p| p.quarantined.clear());
            self.sync_presend();
            self.publish_bg(self.state_draft());
            return Some(link_rx);
        }
        None
    }
}

async fn watch_link(target: Arc<Target>, mut rx: mpsc::Receiver<LinkEvent>) {
    loop {
        let Some(ev) = rx.recv().await else { return };
        match ev {
            LinkEvent::Closed { reason } | LinkEvent::Error { reason } => {
                target.on_link_down(&reason);
                if !target.auto_reconnect {
                    return;
                }
                match target.reconnect().await {
                    Some(new_rx) => rx = new_rx,
                    None => return,
                }
            }
            LinkEvent::RxOverflow { dropped_total, .. } => {
                tracing::warn!(target = %target.name, dropped_total, "uds rx queue overflow");
            }
            _ => {}
        }
    }
}

/// `uds.connect` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSpec {
    pub name: String,
    pub sim: bool,
    pub host: Option<String>,
    pub port: u16,
    pub la: u16,
    pub tester_la: u16,
    pub activation: u8,
    pub auto_reconnect: bool,
    pub timeout: Duration,
}

/// The UDS domain's runtime state: one per registration (see [`crate::UdsModule`]).
#[derive(Debug, Default)]
pub struct UdsManager {
    targets: Mutex<BTreeMap<String, Arc<Target>>>,
    presend: Arc<PresendState>,
    writes: Mutex<WriteLog>,
    request_seq: AtomicU64,
    /// CORE STOP latch as last reported through `WriteHooks::on_latch`. Agent/script requests
    /// check it under this lock right before entering the runner, so a request that passed the
    /// gateway's latch check just before STOP is either cancelled by [`UdsManager::stop_latch`]
    /// or refused here — never sent after STOP.
    stopped: Mutex<bool>,
}

/// Origins STOP cancels (contract A3): agents and scripts. User traffic is never touched.
fn stoppable(origin: Origin) -> bool {
    matches!(origin, Origin::Agent | Origin::Script)
}

impl UdsManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn presend(&self) -> Arc<PresendState> {
        self.presend.clone()
    }

    pub fn target(&self, name: &str) -> Result<Arc<Target>, DomainError> {
        lock(&self.targets).get(name).cloned().ok_or_else(|| {
            DomainError::new(
                ErrorCode::NotOpen,
                format!("UDS target {name:?} is not connected"),
            )
        })
    }

    /// The single target when `name` is omitted (plan §4.1: ambiguous → error).
    pub fn resolve(&self, name: Option<&str>) -> Result<Arc<Target>, DomainError> {
        if let Some(n) = name {
            return self.target(n);
        }
        let m = lock(&self.targets);
        match m.len() {
            1 => Ok(m
                .values()
                .next()
                .cloned()
                .ok_or_else(|| DomainError::new(ErrorCode::Internal, "empty"))?),
            0 => Err(DomainError::new(
                ErrorCode::NotOpen,
                "no UDS target connected (loom uds connect)",
            )),
            _ => Err(DomainError::new(
                ErrorCode::AmbiguousChannel,
                "several UDS targets: pass -t <target>",
            )),
        }
    }

    pub fn targets(&self) -> Vec<Arc<Target>> {
        lock(&self.targets).values().cloned().collect()
    }

    /// `others_since_last` for the calling client (advances its cursor).
    pub fn others_since_last(&self, client: &str) -> Value {
        lock(&self.writes).others_since_last(client)
    }

    fn next_request_id(&self) -> String {
        format!("uds-{}", self.request_seq.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// Connect (or return the existing connection of) a target.
    pub async fn connect(&self, ctx: &CallCtx, spec: ConnectSpec) -> Result<Value, DomainError> {
        if let Some(t) = lock(&self.targets).get(&spec.name).cloned() {
            return Ok(json!({"target": t.name, "state": t.state(), "existing": true}));
        }
        if lock(&self.targets).len() >= MAX_TARGETS {
            return Err(DomainError::new(
                ErrorCode::Busy,
                format!("at most {MAX_TARGETS} UDS targets"),
            ));
        }
        let (addr, sim, backend, env) = if spec.sim {
            let mut net = SimNetwork::new();
            net.insert(spec.la, SimEcu::bench())
                .map_err(|e| DomainError::new(ErrorCode::Internal, e.to_string()))?;
            let network = Arc::new(Mutex::new(net));
            let net2 = network.clone();
            let entity = DoipEntity::start(
                EntityConfig::loopback(SIM_ENTITY_LA, vec![spec.la]),
                move |la, now, req: &[u8]| {
                    lock(&net2)
                        .handle_physical(la, now, req)
                        .map(|r| r.into_vec().into_iter().map(|x| (x.delay, x.bytes)).collect())
                        .unwrap_or_default()
                },
            )
            .await
            .map_err(|e| DomainError::new(ErrorCode::Internal, format!("sim DoIP entity: {e}")))?;
            let addr = entity.tcp_addr();
            (addr, Some(SimSide { entity, network }), "sim", Env::Sim)
        } else {
            let host = spec
                .host
                .clone()
                .ok_or_else(|| DomainError::new(ErrorCode::Usage, "connect needs host or sim"))?;
            let addr: SocketAddr = format!("{host}:{}", spec.port).parse().map_err(|e| {
                DomainError::new(ErrorCode::Usage, format!("bad address {host}:{}: {e}", spec.port))
            })?;
            (addr, None, "doip", Env::Bench)
        };

        let mut cfg = ClientConfig::new(addr, spec.tester_la);
        cfg.activation_type = ActivationType(spec.activation);
        cfg.connect_timeout = spec.timeout;
        cfg.generation = 1;
        let (client, link_rx) = DoipClient::connect(cfg.clone())
            .await
            .map_err(|e| DomainError::new(ErrorCode::NotOpen, format!("DoIP connect {addr}: {e}")))?;
        let doip_target = client.open_target(spec.la).map_err(|e| {
            DomainError::new(ErrorCode::NotOpen, format!("DoIP target 0x{:04X}: {e}", spec.la))
        })?;

        let (tx, events) = mpsc::channel(CAP_RUNNER_EVENTS);
        let observer = Arc::new(QueueObserver {
            tx,
            dropped: AtomicU64::new(0),
        });
        let mut session = TargetSession::new(spec.name.clone());
        session.set_endpoint(backend, format!("DoIP 0x{:04X}", spec.la));
        let clock = HubClock::from_ctx(ctx);
        session.link_up(Duration::from_nanos(clock.mono_ns()));
        let (p2, p2s) = session.timing();
        let runner = Runner::new(
            Box::new(DoipTransport::new(doip_target)),
            RunnerConfig {
                p2: Duration::from_millis(u64::from(p2)),
                p2_star: Duration::from_millis(u64::from(p2s)),
                ..RunnerConfig::default()
            },
            MonoClock::new(clock.at, clock.base_ns),
            observer.clone(),
        );
        let (handle, task) = RunnerHandle::spawn(runner);
        let ctl_handle = handle.clone();
        let target = Arc::new(Target {
            name: spec.name.clone(),
            env,
            la: spec.la,
            auto_reconnect: spec.auto_reconnect,
            client_cfg: Mutex::new(cfg),
            session: Mutex::new(session),
            gate: tokio::sync::Mutex::new(Gate {
                handle: Some(handle),
                task: Some(task),
                events,
                client: Some(client),
            }),
            lanes: PriorityGate::default(),
            ctl_handle: Mutex::new(Some(ctl_handle)),
            observer,
            publisher: Mutex::new(Some(ctx.clone())),
            timed_out: Mutex::new(VecDeque::new()),
            sim,
            clock,
            presend: self.presend.clone(),
        });
        {
            let mut m = lock(&self.targets);
            if m.len() >= MAX_TARGETS {
                return Err(DomainError::new(
                    ErrorCode::Busy,
                    format!("at most {MAX_TARGETS} UDS targets"),
                ));
            }
            m.insert(spec.name.clone(), target.clone());
        }
        target.sync_presend();
        tokio::spawn(watch_link(target.clone(), link_rx));
        target.publish(ctx, target.state_draft());
        Ok(
            json!({"target": target.name, "state": target.state(), "existing": false, "entity": addr.to_string()}),
        )
    }

    /// Run one request through lease → gate → runner, publishing every stage.
    pub async fn request(&self, ctx: &CallCtx, spec: ReqSpec) -> Result<Outcome, DomainError> {
        let target = self.target(&spec.target)?;
        let rid = spec
            .request_id
            .clone()
            .or_else(|| ctx.request_id().map(str::to_owned))
            .unwrap_or_else(|| self.next_request_id());
        let origin = ctx.origin();
        let class = classify_pdu(&spec.pdu, false);
        let timing = lock(&target.session).timing();
        // The gateway write path already published `requested` and `accepted` for the call's own
        // request id (contract 02 §2.3); repeating them would step the row back. Rows of
        // sub-requests (`<rid>.verify`) are ours from the start.
        let core_row = spec.request_id.is_none() && ctx.request_id().is_some();
        let mut row = RowPayload::request(&target.name, &spec.pdu, false, WriteState::Requested, timing);
        if !core_row {
            target.publish(ctx, target.row_draft(KIND_TX, &rid, &row));
        }

        let others = self.others_since_last(ctx.client_id());
        let finish_refused = |row: &mut RowPayload, reason: String| {
            row.write_state = WriteState::Failed;
            row.reason = reason;
            target.publish(ctx, target.row_draft(KIND_RESULT, &rid, row));
        };

        if row.block.is_some() {
            finish_refused(&mut row, class.reason.clone());
            return Ok(Outcome {
                request_id: rid,
                row,
                result: None,
                others_since_last: others,
                dropped_events: 0,
            });
        }

        if stoppable(origin) && self.stopped() {
            return Err(self.refuse_stopped(&mut row, &rid, others, finish_refused));
        }

        // Lease (plan U-08 minimal): a user write takes the target; agent/script writes wait for it.
        if class.risk >= Risk::R1 {
            let mut s = lock(&target.session);
            let held_by_user = s.lease().is_some_and(|l| l.owner_origin == Origin::User);
            if origin == Origin::User {
                let changed = s
                    .lease()
                    .is_none_or(|l| l.owner_origin != Origin::User || l.client_id != ctx.client_id());
                s.set_lease(Some(Lease {
                    owner_origin: Origin::User,
                    actor_id: ctx.client_id().to_owned(),
                    client_id: ctx.client_id().to_owned(),
                }));
                drop(s);
                if changed {
                    target.sync_presend();
                    target.publish(ctx, target.state_draft());
                }
            } else if held_by_user {
                drop(s);
                finish_refused(&mut row, "target held by the user (lease)".into());
                let data = json!({"request_id": rid, "row": row, "others_since_last": others});
                return Err(
                    DomainError::new(ErrorCode::HeldByUser, "target is held by the user").with_detail(data),
                );
            } else if s.lease().is_none() {
                s.set_lease(Some(Lease {
                    owner_origin: origin,
                    actor_id: ctx.client_id().to_owned(),
                    client_id: ctx.client_id().to_owned(),
                }));
                drop(s);
                target.publish(ctx, target.state_draft());
            }
            lock(&self.writes).record(origin, ctx.client_id());
        }

        row.write_state = WriteState::Accepted;
        if !core_row {
            target.publish(ctx, target.row_draft(KIND_TX, &rid, &row));
        }

        // Priority gate (lanes.rs): one of our requests in the runner at a time so runner events map
        // to `rid` (RunnerEvent has no request tag yet); order Abort > User > Agent like the runner.
        let lane = match (origin, spec.recovery) {
            (Origin::User, true) => Lane::Abort,
            (Origin::User, false) => Lane::User,
            _ => Lane::Agent,
        };
        let priority = match lane {
            Lane::Abort | Lane::Control => Priority::Abort,
            Lane::User => Priority::User,
            Lane::Agent => Priority::Agent,
        };
        let _permit = match target.lanes.acquire(lane, origin, &rid).await {
            Ok(p) => p,
            Err(e) => {
                let (code, reason) = match e {
                    GateError::Busy => (ErrorCode::Busy, "target queue full (not sent)".to_string()),
                    GateError::Aborted(kind) => {
                        (kind.error_code(), format!("aborted before sending ({kind:?})"))
                    }
                };
                finish_refused(&mut row, reason.clone());
                let data = json!({"request_id": rid, "row": row, "others_since_last": others});
                return Err(DomainError::new(code, reason).with_detail(data));
            }
        };
        let mut gate = target.gate.lock().await;
        while let Ok(ev) = gate.events.try_recv() {
            self.on_event(ctx, &target, &mut row, &rid, ev, true);
        }
        let Some(handle) = gate.handle.clone() else {
            drop(gate);
            finish_refused(&mut row, "runner not available".into());
            return Err(DomainError::new(ErrorCode::NotOpen, "UDS runner not available"));
        };
        let access = if class.risk == Risk::R0 {
            Access::Read
        } else {
            Access::Write
        };
        let default_timeout = if access == Access::Read {
            DEFAULT_READ_TIMEOUT
        } else {
            DEFAULT_WRITE_TIMEOUT
        };
        let timeout = spec.timeout.unwrap_or(default_timeout).min(MAX_REQUEST_TIMEOUT);
        let txn = TxnRequest::new(spec.pdu.clone(), access, timeout)
            .with_priority(priority)
            .with_origin(origin)
            .with_request_id(rid.clone());
        let probe = txn.clone();
        // STOP check and runner submission under one lock: see `UdsManager::stopped`.
        let submitted = {
            let stopped = lock(&self.stopped);
            (!(*stopped && stoppable(origin))).then(|| handle.try_submit(txn))
        };
        let Some(submitted) = submitted else {
            drop(handle);
            drop(gate);
            return Err(self.refuse_stopped(&mut row, &rid, others, finish_refused));
        };
        let res = match submitted {
            Err(rejected) => *rejected,
            Ok(rx) => {
                tokio::pin!(rx);
                loop {
                    tokio::select! {
                        biased;
                        Some(ev) = gate.events.recv() => self.on_event(ctx, &target, &mut row, &rid, ev, false),
                        r = &mut rx => break r.unwrap_or_else(|_| {
                            let clock = MonoClock::new(target.clock.at, target.clock.base_ns);
                            TxnResult::rejected(&probe, RejectReason::RunnerStopped, &clock, 0)
                        }),
                    }
                }
            }
        };
        drop(handle);
        while let Ok(ev) = gate.events.try_recv() {
            self.on_event(ctx, &target, &mut row, &rid, ev, false);
        }

        self.finish_row(&target, &spec.pdu, &res, &mut row);
        if let Some(resp) = &res.response {
            let mono = res
                .times
                .final_rx_mono_ns
                .unwrap_or_else(|| target.clock.mono_ns());
            let p = target.rx_payload(&spec.pdu, resp, mono, false);
            target.publish(ctx, events::rx(Some(&rid), target.generation(), target.env, &p));
        }

        // Session belief, runner timing, quarantine bookkeeping.
        let now = target.now();
        let (changed, timing_after) = {
            let mut s = lock(&target.session);
            let changed = match (&res.status, &res.response) {
                (TxnStatus::Positive | TxnStatus::Negative { .. }, Some(resp)) => {
                    s.on_response(now, &spec.pdu, resp)
                }
                (TxnStatus::OkSuppressed, _) => {
                    let before = s.state_payload();
                    s.on_suppressed_ok(now, &spec.pdu);
                    before != s.state_payload()
                }
                _ => {
                    s.on_sent(now);
                    false
                }
            };
            (changed, s.timing())
        };
        if timing_after != timing {
            let (p2, p2s) = timing_after;
            target
                .restart_runner(&mut gate, |r| {
                    r.set_timing(
                        Duration::from_millis(u64::from(p2)),
                        Duration::from_millis(u64::from(p2s)),
                    )
                })
                .await;
        }
        drop(gate);
        let sid = spec.pdu.first().copied().unwrap_or(0);
        if res.timeout
            || (res.sent && matches!(res.status, TxnStatus::LinkLost | TxnStatus::GenerationChanged))
        {
            {
                let mut q = lock(&target.timed_out);
                if q.len() >= MAX_TIMED_OUT_TRACKED {
                    q.pop_front();
                }
                q.push_back((sid, rid.clone()));
            }
            target.presend.update(&target.name, |p| {
                p.quarantined.insert(sid);
            });
        }
        if changed {
            target.sync_presend();
            target.publish(ctx, target.state_draft());
        }
        target.publish(ctx, target.row_draft(KIND_RESULT, &rid, &row));
        let dropped_events = target.observer.dropped.load(Ordering::Relaxed);
        Ok(Outcome {
            request_id: rid,
            row,
            result: Some(res),
            others_since_last: others,
            dropped_events,
        })
    }

    /// Apply one runner event to the row being executed and publish it.
    fn on_event(
        &self,
        ctx: &CallCtx,
        target: &Target,
        row: &mut RowPayload,
        rid: &str,
        ev: OwnedEvent,
        stale: bool,
    ) {
        match ev {
            OwnedEvent::Sent { mono_ns } if !stale => {
                row.write_state = WriteState::Sent;
                row.t_send_mono_ns = Some(U64(mono_ns));
                target.publish(ctx, target.row_draft(KIND_TX, rid, row));
            }
            OwnedEvent::Pending { count, mono_ns } if !stale => {
                let req = row.bytes.decode().unwrap_or_default();
                let sid = req.first().copied().unwrap_or(0);
                let p = target.rx_payload(&req, &[NEGATIVE_RESPONSE_SID, sid, 0x78], mono_ns, false);
                target.publish(ctx, events::rx(Some(rid), target.generation(), target.env, &p));
                row.pending_count = count;
                if row.t_first_rx_mono_ns.is_none() {
                    row.t_first_rx_mono_ns = Some(U64(mono_ns));
                }
                target.publish(ctx, target.row_draft(KIND_PENDING, rid, row));
            }
            OwnedEvent::Late { data, mono_ns } => target.publish_late(ctx, &data, mono_ns),
            OwnedEvent::Released => {
                lock(&target.timed_out).clear();
                target.presend.update(&target.name, |p| p.quarantined.clear());
            }
            OwnedEvent::Sent { .. } | OwnedEvent::Pending { .. } => {}
        }
    }

    fn finish_row(&self, target: &Target, pdu: &[u8], res: &TxnResult, row: &mut RowPayload) {
        row.write_state = res.write_state;
        row.pending_count = res.pending_count;
        row.timeout = res.timeout;
        row.ok_suppressed = matches!(res.status, TxnStatus::OkSuppressed);
        if let Some(n) = res.nrc {
            *row = row.clone().with_nrc(n);
        }
        let t = res.times;
        row.t_send_mono_ns = t.sent_mono_ns.map(U64).or(row.t_send_mono_ns);
        row.t_first_rx_mono_ns = t.first_rx_mono_ns.map(U64).or(row.t_first_rx_mono_ns);
        row.t_final_rx_mono_ns = t.final_rx_mono_ns.map(U64);
        row.first_rx_ms = ms_between(t.sent_mono_ns, t.first_rx_mono_ns);
        row.rtt_ms = ms_between(t.sent_mono_ns, t.final_rx_mono_ns);
        if let Some(resp) = &res.response {
            row.resp_bytes = Some(PayloadBytes::hex(resp));
            if matches!(res.status, TxnStatus::Positive) {
                match pdu.first() {
                    Some(0x22) => row.decoded = decode_dids(pdu, resp),
                    Some(0x19) if pdu.get(1).map(|b| b & 0x7F) == Some(0x02) => {
                        row.dtcs = parse_dtc_by_status_response(resp).ok().map(|r| {
                            r.dtcs
                                .iter()
                                .map(|d| DtcItem {
                                    dtc: format!("{:06X}", d.dtc),
                                    status: d.status,
                                })
                                .collect()
                        });
                    }
                    _ => {}
                }
            }
        }
        if let TxnStatus::Rejected(reason) = &res.status {
            row.reason = match reason {
                RejectReason::Quarantine { sid } => {
                    format!("quarantine: SID 0x{sid:02X} may still answer late")
                }
                other => format!("not sent: {other:?}"),
            };
        }
        let _ = target;
    }

    /// Publish an updated final row (e.g. read-back verification of a write).
    pub fn republish(&self, ctx: &CallCtx, target: &Target, rid: &str, row: &RowPayload) {
        target.publish(ctx, target.row_draft(KIND_RESULT, rid, row));
    }

    /// User takes the target (viewer `T`).
    pub fn lease_takeover(&self, ctx: &CallCtx, name: Option<&str>) -> Result<Value, DomainError> {
        let target = self.resolve(name)?;
        lock(&target.session).set_lease(Some(Lease {
            owner_origin: ctx.origin(),
            actor_id: ctx.client_id().to_owned(),
            client_id: ctx.client_id().to_owned(),
        }));
        target.sync_presend();
        target.publish(ctx, target.state_draft());
        Ok(json!({"target": target.name, "state": target.state()}))
    }

    /// User returns the target (viewer `R`): agent writes are allowed again.
    pub fn lease_release(&self, ctx: &CallCtx, name: Option<&str>) -> Result<Value, DomainError> {
        let target = self.resolve(name)?;
        lock(&target.session).set_lease(None);
        target.sync_presend();
        target.publish(ctx, target.state_draft());
        Ok(json!({"target": target.name, "state": target.state()}))
    }

    /// Abort (`!`, `uds.abort`). A viewer aborts everything (`scope: "all"`, default) or only agent
    /// requests (`scope: "agent"`); an agent or script may only abort its own origin. Waiting
    /// requests end `failed` unsent; the one in flight stops waiting (read `failed`, write
    /// `outcome_unknown` + quarantine). Safe-state recovery (Abort lane) is never cancelled here.
    pub fn abort(
        &self,
        ctx: &CallCtx,
        name: Option<&str>,
        scope: Option<&str>,
    ) -> Result<Value, DomainError> {
        let target = self.resolve(name)?;
        let origin = ctx.origin();
        let runner_scope = match (origin, scope) {
            (Origin::User, None | Some("all")) => AbortScope::All,
            (Origin::User, Some("agent")) => AbortScope::Origin(Origin::Agent),
            (Origin::User, Some(other)) => {
                return Err(DomainError::new(
                    ErrorCode::Usage,
                    format!("abort scope {other:?}: all | agent"),
                ));
            }
            (o, _) => AbortScope::Origin(o),
        };
        let only = match &runner_scope {
            AbortScope::Origin(o) => Some(*o),
            _ => None,
        };
        let gate_waiting = target
            .lanes
            .cancel(AbortKind::User, |o, _| only.is_none_or(|s| s == o), false);
        let summary = target.ctl().map(|h| h.abort(runner_scope.clone()));
        Ok(json!({
            "target": target.name,
            "scope": format!("{runner_scope:?}"),
            "cancelled_waiting": gate_waiting + summary.as_ref().map_or(0, |s| s.cancelled_waiting),
            "in_flight_aborted": summary.as_ref().is_some_and(|s| s.in_flight),
        }))
    }

    /// CORE STOP latch is on (as last reported through `WriteHooks::on_latch`).
    pub fn stopped(&self) -> bool {
        *lock(&self.stopped)
    }

    /// Refuse an agent/script request because STOP is latched: row `failed`, not sent.
    fn refuse_stopped(
        &self,
        row: &mut RowPayload,
        rid: &str,
        others: Value,
        finish_refused: impl FnOnce(&mut RowPayload, String),
    ) -> DomainError {
        let reason = "STOP is latched (not sent)".to_string();
        finish_refused(row, reason.clone());
        let data = json!({"request_id": rid, "row": row, "others_since_last": others, "write_state": row.write_state});
        DomainError::new(ErrorCode::Stopped, reason).with_detail(data)
    }

    /// STOP latch on (core A3, `WriteHooks::on_latch(true)`): stop agent and script traffic on
    /// every target with `E_STOPPED`. Waiting requests end `failed` unsent; the one in flight
    /// stops waiting (read `failed`, write after hand-off `outcome_unknown` + quarantine). Later
    /// agent/script requests are refused before the runner until [`UdsManager::release_latch`].
    /// User requests and safe-state recovery are untouched. Synchronous and bounded: short
    /// locks only, no await, no device I/O (at most [`MAX_TARGETS`] targets).
    pub fn stop_latch(&self) -> Value {
        *lock(&self.stopped) = true;
        let mut out = Vec::new();
        for t in self.targets() {
            let waiting = t.lanes.cancel(AbortKind::Stop, |o, _| stoppable(o), false);
            let mut runner_waiting = 0;
            let mut in_flight = false;
            if let Some(h) = t.ctl() {
                for o in [Origin::Agent, Origin::Script] {
                    let s = h.abort_with(AbortScope::Origin(o), AbortKind::Stop);
                    runner_waiting += s.cancelled_waiting;
                    in_flight |= s.in_flight;
                }
            }
            out.push(json!({"target": t.name, "cancelled_waiting": waiting + runner_waiting, "in_flight_aborted": in_flight}));
        }
        json!({"targets": out})
    }

    /// STOP released (`WriteHooks::on_latch(false)`): agent/script requests may enter the runner
    /// again. Nothing cancelled is resumed or resent — those calls already returned `E_STOPPED`.
    pub fn release_latch(&self) {
        *lock(&self.stopped) = false;
    }
}
