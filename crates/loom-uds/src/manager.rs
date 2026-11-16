//! Connection manager (plan U-07/U-08): targets, runners, live events, lease, abort, reconnect.
//!
//! One [`Target`] per connected ECU. Its runner is a `loom-uds-runner` [`RunnerHandle`]; a
//! per-target gate keeps one of *our* requests inside the runner at a time so every runner event
//! maps to exactly one `request_id` (see `HOOK(runner-priority)` — replaced by the runner's own
//! priority queue + request tags when they land). `connect --sim` starts a [`DoipEntity`] on
//! 127.0.0.1 with the virtual ECU behind it and connects with the real DoIP client, so simulated
//! traffic takes exactly the hardware path.
//!
//! Link lifecycle is transport-neutral: a target owns an `Arc<dyn Connector>` and every link
//! (initial connect and each reconnect, generation + 1) comes from [`Connector::connect`]. `uds.connect`
//! builds a [`DoipConnector`]; [`UdsManager::connect_link`] attaches any other connector (future
//! CAN-TP / vendor Ethernet diagnostics) through the same request, lease, lane and runner logic.
//!
//! Events are published through the caller's [`CallCtx`] **before** the handler returns
//! (publish-before-reply, E7): `uds.tx` requested/accepted (only for rows the gateway did not
//! open itself) and sent, `uds.rx` per received PDU (0x78
//! and late responses included), `uds.pending`, `uds.result`, and `uds.state` when the
//! [`TargetSession`] changes. Background events (link loss, reconnect) use the context of the
//! call that connected the target (`publisher`).

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use loom_domain_api::{CallCtx, DomainError, EventDraft};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, PayloadBytes, Risk, WriteState};
use loom_proto::num::U64;
use loom_uds_codec::response::parse_dtc_by_status_response;
use loom_uds_codec::{DidLookup, NEGATIVE_RESPONSE_SID, StandardDids, classify_response, nrc_name};
use loom_uds_doip::{ActivationType, ClientConfig, DoipEntity, EntityConfig};
use loom_uds_runner::{
    AbortKind, AbortScope, Access, Connector, DoipConnector, Link, LinkEvents, LinkSignal, MonoClock,
    Priority, RejectReason, Runner, RunnerConfig, RunnerEvent, RunnerHandle, RunnerObserver, TxnRequest,
    TxnResult, TxnStatus,
};
use loom_uds_sim::{SimEcu, SimNetwork};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::diagdb::{AttachedDb, db_warnings, decode_dids_with, parse_inline_db};
use crate::events;
use crate::keyprovider::{provider_from_config, run_provider};
use crate::lanes::{GateError, Lane, PriorityGate};
use crate::payload::{DtcItem, PURPOSE_TESTER_PRESENT, RowPayload, RxKind, RxPayload};
use crate::risk::classify_pdu;
use crate::session::{Lease, TargetSession};
use crate::tester_present::{
    TP_BUSY_RECHECK, TP_EXCHANGE_TIMEOUT, TP_MAX_CONSECUTIVE_FAILURES, TpCancel, TpConfig, TpRequest, TpTick,
    tp_decide, tp_exchange_outcome,
};
use crate::{KIND_PENDING, KIND_RESULT, KIND_TX};

/// Bounded wait of `uds.disconnect` for the request in flight to end after `abort` (runner
/// deadline ≤ [`MAX_REQUEST_TIMEOUT`] plus margin). Registered RPC max_wait is larger.
pub const DISCONNECT_DRAIN_TIMEOUT: Duration = Duration::from_secs(35);

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

    /// B2-M1 (Sonnet body): drop the snapshot of a forgotten target (U6).
    fn remove(&self, target: &str) {
        lock(&self.targets).remove(target);
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

/// Runner, its event queue and the current link's resources — only touched while holding the gate.
#[derive(Debug)]
struct Gate {
    handle: Option<RunnerHandle>,
    task: Option<JoinHandle<Runner>>,
    events: mpsc::Receiver<OwnedEvent>,
    /// [`Link::resources`] of the link the runner's transport belongs to (DoIP: the client).
    link: Option<Box<dyn Any + Send>>,
}

/// Simulated side kept alive while the target exists (`connect --sim`, DoIP loopback only).
#[derive(Debug)]
struct SimSide {
    entity: DoipEntity,
    network: Arc<Mutex<SimNetwork>>,
    ecu_la: u16,
}

/// One connected ECU.
#[derive(Debug)]
pub struct Target {
    name: String,
    env: Env,
    auto_reconnect: bool,
    connector: Arc<dyn Connector>,
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
    /// The manager's STOP latch (same mutex as `UdsManager::stopped`), for the TesterPresent job.
    stopped: Arc<Mutex<bool>>,
    /// U1: the running TesterPresent job, at most one.
    tp: Mutex<Option<TpJob>>,
    /// U1: a TesterPresent exchange holds the gate right now (not "real" traffic for disconnect).
    tp_in_flight: AtomicBool,
    /// U6: disconnect/replace started: no reconnect, no new requests.
    closing: AtomicBool,
    /// Link watcher task (aborted on disconnect/replace).
    watch: Mutex<Option<JoinHandle<()>>>,
    /// U5: diagnostic DB attached with `uds.db.set` (decode only). A replaced target starts without one.
    db: Mutex<Option<Arc<AttachedDb>>>,
}

/// U1 job bookkeeping (the task owns its own `CallCtx` clone for publishing rows).
#[derive(Debug)]
pub struct TpJob {
    pub cfg: TpConfig,
    pub owner: Lease,
    /// Link generation it was started on; a different generation means the job must end.
    pub generation: u32,
    pub cancel: Arc<TpCancel>,
    pub task: JoinHandle<()>,
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

/// `lookup`: the DB snapshot taken when the row was created (B3-M2), else `&StandardDids`.
fn finish_row(pdu: &[u8], res: &TxnResult, row: &mut RowPayload, lookup: &dyn DidLookup) {
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
                Some(0x22) => row.decoded = decode_dids_with(lookup, pdu, resp),
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
        let mut p = lock(&self.session).state_payload();
        p.db = self.db_snapshot().map(|d| d.info.clone());
        p
    }

    /// Read a DID straight from the simulated ECU (tests, sim only).
    pub fn sim_did(&self, did: u16) -> Option<Vec<u8>> {
        let sim = self.sim.as_ref()?;
        lock(&sim.network)
            .get(sim.ecu_la)
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
        net.get_mut(sim.ecu_la).map(f)
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

    /// B3-M1 (Sonnet body): the attached DB, one snapshot (lookup + hash) per request.
    pub(crate) fn db_snapshot(&self) -> Option<Arc<AttachedDb>> {
        lock(&self.db).clone()
    }

    /// B2-M2 (Sonnet body): stop the TesterPresent job (sync, never awaits, no device I/O).
    /// Returns `true` if a job was running.
    pub(crate) fn stop_tp(&self, cause: &str) -> bool {
        let Some(job) = lock(&self.tp).take() else {
            return false;
        };
        job.cancel.cancel();
        drop(job); // detached: the task ends by itself after at most one in-flight exchange
        let changed = lock(&self.session).tp_stop(cause);
        if changed {
            self.publish_bg(self.state_draft());
        }
        true
    }

    /// B2-M2b (Sonnet body): like [`Target::stop_tp`] but only if the running job is the one
    /// owning `cancel` (a job ending itself must never stop its replacement).
    fn stop_tp_job(&self, cancel: &Arc<TpCancel>, cause: &str) -> bool {
        let job = {
            let mut slot = lock(&self.tp);
            if slot.as_ref().is_some_and(|j| Arc::ptr_eq(&j.cancel, cancel)) {
                slot.take()
            } else {
                None
            }
        };
        let Some(job) = job else {
            return false;
        };
        job.cancel.cancel();
        drop(job);
        let changed = lock(&self.session).tp_stop(cause);
        if changed {
            self.publish_bg(self.state_draft());
        }
        true
    }

    /// B2-M3 (Sonnet body): after any lease change, stop a job whose owner no longer matches.
    fn tp_on_lease_change(&self) {
        let Some(owner) = lock(&self.tp).as_ref().map(|j| j.owner.client_id.clone()) else {
            return;
        };
        let holder = lock(&self.session).lease().map(|l| l.client_id.clone());
        if let Some(holder) = holder
            && holder != owner
        {
            self.stop_tp(&format!("lease taken by {holder}"));
        }
    }

    /// B2-M4 (Sonnet body): disconnect/replace has started or finished for this target.
    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// B2-M4 (Sonnet body): may `uds.connect` replace / `uds.forget` remove this target?
    pub fn is_replaceable(&self) -> bool {
        if self.is_closing() {
            return true;
        }
        let link = lock(&self.session).link();
        link == crate::payload::LinkState::Closed
            || (link == crate::payload::LinkState::Down
                && lock(&self.watch).as_ref().is_none_or(JoinHandle::is_finished))
    }

    /// B2-M5 (Sonnet body): stop TP and watcher, wait for the runner to finish, close the link.
    /// Idempotent. Does not change the published link state (callers do).
    async fn retire(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.stop_tp("target closed");
        let watcher = lock(&self.watch).take();
        if let Some(w) = watcher {
            w.abort();
        }
        let _permit = self.lanes.acquire(Lane::Control, Origin::Script, "retire").await;
        let mut gate = self.gate.lock().await;
        drop(lock(&self.ctl_handle).take());
        drop(gate.handle.take());
        if let Some(task) = gate.task.take() {
            let _ = tokio::time::timeout(DISCONNECT_DRAIN_TIMEOUT, task).await;
        }
        drop(gate.link.take()); // closes the DoIP TCP connection
    }

    /// B2-M6 (Sonnet body): one TesterPresent exchange through the gate holder's runner.
    /// Caller holds a gate permit. Publishes nothing except late responses; fills `row`.
    async fn tp_exchange(&self, ctx: &CallCtx, txn: TxnRequest, row: &mut RowPayload) -> TxnResult {
        let clock = MonoClock::new(self.clock.at, self.clock.base_ns);
        // Keep-alive publishes nothing except late responses: no tx / pending / rx rows.
        let apply = |row: &mut RowPayload, ev: OwnedEvent, stale: bool| match ev {
            OwnedEvent::Sent { mono_ns } if !stale => {
                row.write_state = WriteState::Sent;
                row.t_send_mono_ns = Some(U64(mono_ns));
            }
            OwnedEvent::Pending { count, .. } if !stale => row.pending_count = count,
            OwnedEvent::Late { data, mono_ns } => self.publish_late(ctx, &data, mono_ns),
            OwnedEvent::Released => {
                lock(&self.timed_out).clear();
                self.presend.update(&self.name, |p| p.quarantined.clear());
            }
            OwnedEvent::Sent { .. } | OwnedEvent::Pending { .. } => {}
        };
        let mut gate = self.gate.lock().await;
        while let Ok(ev) = gate.events.try_recv() {
            apply(row, ev, true);
        }
        let Some(handle) = gate.handle.clone() else {
            return TxnResult::rejected(&txn, RejectReason::RunnerStopped, &clock, 0);
        };
        let probe = txn.clone();
        // STOP check and runner submission under one lock (see `UdsManager::stopped`).
        let submitted = {
            let stopped = lock(&self.stopped);
            (!*stopped).then(|| handle.try_submit(txn))
        };
        let Some(submitted) = submitted else {
            return TxnResult::aborted(&probe, AbortKind::Stop, &clock, 0);
        };
        let res = match submitted {
            Err(rejected) => *rejected,
            Ok(rx) => {
                tokio::pin!(rx);
                loop {
                    tokio::select! {
                        biased;
                        Some(ev) = gate.events.recv() => apply(row, ev, false),
                        r = &mut rx => break r.unwrap_or_else(|_| {
                            TxnResult::rejected(&probe, RejectReason::RunnerStopped, &clock, 0)
                        }),
                    }
                }
            }
        };
        drop(handle);
        while let Ok(ev) = gate.events.try_recv() {
            apply(row, ev, false);
        }
        res
    }

    fn on_link_down(&self, cause: &str) {
        lock(&self.session).link_down(cause);
        self.sync_presend();
        self.publish_bg(self.state_draft());
    }

    /// Reconnect with generation + 1 through the target's connector and swap the runner's
    /// transport. Nothing that was in flight is resent. Returns the new link's signal source, or
    /// `None` after [`MAX_RECONNECT_ATTEMPTS`].
    async fn reconnect(&self) -> Option<Box<dyn LinkEvents>> {
        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            tokio::time::sleep(RECONNECT_INTERVAL).await;
            let generation = lock(&self.session).generation().saturating_add(1);
            let link = match open_link(self.connector.as_ref(), u64::from(generation)).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::info!(target = %self.name, attempt, "uds reconnect failed: {e}");
                    continue;
                }
            };
            let Link {
                transport,
                events,
                resources,
            } = link;
            {
                let _permit = self
                    .lanes
                    .acquire(Lane::Control, Origin::Script, "reconnect")
                    .await;
                let mut gate = self.gate.lock().await;
                self.restart_runner(&mut gate, |r| r.replace_transport(transport))
                    .await;
                // The old link's resources go only after its transport left the runner.
                gate.link = Some(resources);
            }
            let now = self.now();
            lock(&self.session).link_up(now);
            lock(&self.timed_out).clear();
            self.presend.update(&self.name, |p| p.quarantined.clear());
            self.sync_presend();
            self.publish_bg(self.state_draft());
            return Some(events);
        }
        None
    }
}

/// One connector attempt, holding the connector to its generation contract: a link reporting
/// any other generation is dropped (closed) and the attempt fails.
async fn open_link(connector: &dyn Connector, generation: u64) -> Result<Link, String> {
    let link = connector.connect(generation).await.map_err(|e| e.to_string())?;
    let got = link.transport.generation();
    if got != generation {
        drop(link);
        return Err(format!(
            "connector {} returned link generation {got}, expected {generation}",
            connector.endpoint()
        ));
    }
    Ok(link)
}

/// `uds.state` link cause when a link's signal source ended without a `Down`.
pub const LINK_EVENTS_ENDED: &str = "link signal source ended";

async fn watch_link(target: Arc<Target>, mut events: Box<dyn LinkEvents>) {
    loop {
        let reason = match events.next().await {
            Some(LinkSignal::Down { reason }) => reason,
            Some(LinkSignal::RxOverflow { dropped_total }) => {
                tracing::warn!(target = %target.name, dropped_total, "uds rx queue overflow");
                continue;
            }
            // Nobody can report the loss of this link any more: never stay online on it.
            None => LINK_EVENTS_ENDED.to_owned(),
        };
        target.on_link_down(&reason);
        target.stop_tp("link down");
        if target.is_closing() || !target.auto_reconnect {
            return;
        }
        match target.reconnect().await {
            Some(next) => {
                if target.is_closing() {
                    return;
                }
                events = next;
            }
            None => return,
        }
    }
}

/// B2-M7 (Sonnet body): the TesterPresent job task (U1). `ctx` = the `uds.tester_present` call
/// that started it (rows are published with its origin). Ends when `cancel` fires, the target
/// generation differs from `generation`, or after [`TP_MAX_CONSECUTIVE_FAILURES`].
async fn tp_loop(
    target: Arc<Target>,
    ctx: CallCtx,
    cfg: TpConfig,
    owner: Lease,
    generation: u32,
    cancel: Arc<TpCancel>,
) {
    use crate::payload::LinkState;
    let mut n: u64 = 0;
    let mut fails: u32 = 0;
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let (gen_now, link, owner_client, last) = {
            let s = lock(&target.session);
            (
                s.generation(),
                s.link(),
                s.tp_owner().map(|l| l.client_id.clone()),
                s.last_activity(),
            )
        };
        if gen_now != generation {
            target.stop_tp_job(&cancel, "generation changed");
            return;
        }
        if link != LinkState::Up {
            target.stop_tp_job(&cancel, "link down");
            return;
        }
        if owner_client.as_deref() != Some(owner.client_id.as_str()) {
            target.stop_tp_job(&cancel, "owner changed");
            return;
        }
        let wait = match tp_decide(target.now(), last, cfg.interval, target.lanes.is_busy()) {
            TpTick::Wait(d) => Some(d),
            TpTick::Busy => Some(TP_BUSY_RECHECK),
            TpTick::Send => None,
        };
        if let Some(d) = wait {
            tokio::select! {
                () = tokio::time::sleep(d) => {}
                () = cancel.cancelled() => return,
            }
            continue;
        }
        let Some(permit) = target.lanes.try_acquire(Lane::Agent, owner.owner_origin, "tp") else {
            tokio::select! {
                () = tokio::time::sleep(TP_BUSY_RECHECK) => {}
                () = cancel.cancelled() => return,
            }
            continue;
        };
        if cancel.is_cancelled() {
            return;
        }
        n += 1;
        let rid = format!("tp-{}-{}-{}", target.name, generation, n);
        target.tp_in_flight.store(true, Ordering::SeqCst);
        let pdu = cfg.pdu();
        let (timing, stamp) = {
            let s = lock(&target.session);
            (s.timing(), s.route_stamp())
        };
        let mut row =
            RowPayload::request(&target.name, &pdu, false, WriteState::Requested, timing).with_route(&stamp);
        row.purpose = Some(PURPOSE_TESTER_PRESENT.into());
        let txn = TxnRequest::new(pdu.clone(), Access::Read, cfg.interval.min(TP_EXCHANGE_TIMEOUT))
            .with_priority(Priority::Agent)
            .with_origin(owner.owner_origin)
            .with_request_id(rid.clone());
        let res = target.tp_exchange(&ctx, txn, &mut row).await;
        target.tp_in_flight.store(false, Ordering::SeqCst);
        drop(permit);

        finish_row(&pdu, &res, &mut row, &StandardDids);
        if res.timeout
            || (res.sent && matches!(res.status, TxnStatus::LinkLost | TxnStatus::GenerationChanged))
        {
            {
                let mut q = lock(&target.timed_out);
                if q.len() >= MAX_TIMED_OUT_TRACKED {
                    q.pop_front();
                }
                q.push_back((0x3E, rid.clone()));
            }
            target.presend.update(&target.name, |p| {
                p.quarantined.insert(0x3E);
            });
        }
        target.publish(
            &ctx,
            events::row(KIND_RESULT, &rid, stamp.generation, target.env, &row),
        );

        match tp_exchange_outcome(&res) {
            Ok(()) => {
                fails = 0;
                lock(&target.session).on_tester_present_ok(target.now());
            }
            Err(e) => {
                lock(&target.session).on_sent(target.now());
                if matches!(
                    res.status,
                    TxnStatus::Aborted {
                        kind: AbortKind::Stop
                    }
                ) {
                    target.stop_tp_job(&cancel, "STOP");
                    return;
                }
                fails += 1;
                if fails >= TP_MAX_CONSECUTIVE_FAILURES {
                    target.stop_tp_job(&cancel, &format!("{fails} consecutive failures: {e}"));
                    return;
                }
            }
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

/// [`UdsManager::connect_link`] parameters: a target over any [`Connector`].
#[derive(Debug, Clone)]
pub struct LinkSpec {
    pub name: String,
    /// `uds.state` backend label (`doip`, `sim`, …).
    pub backend: String,
    pub env: Env,
    pub auto_reconnect: bool,
    pub connector: Arc<dyn Connector>,
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
    /// or refused here — never sent after STOP. Shared with every [`Target`] (TesterPresent job).
    stopped: Arc<Mutex<bool>>,
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
        let existing = lock(&self.targets).get(&spec.name).cloned();
        let mut replaced: Option<u32> = None;
        if let Some(t) = existing {
            if !t.is_replaceable() {
                let reconnecting = lock(&t.session).link() == crate::payload::LinkState::Down;
                return Ok(
                    json!({"target": t.name, "state": t.state(), "existing": true, "reconnecting": reconnecting}),
                );
            }
            let prev = t.generation();
            t.retire().await;
            {
                let mut m = lock(&self.targets);
                if m.get(&spec.name).is_some_and(|x| Arc::ptr_eq(x, &t)) {
                    m.remove(&spec.name);
                }
            }
            replaced = Some(prev);
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
            let sim = SimSide {
                entity,
                network,
                ecu_la: spec.la,
            };
            (addr, Some(sim), "sim", Env::Sim)
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
        let link = LinkSpec {
            name: spec.name,
            backend: backend.into(),
            env,
            auto_reconnect: spec.auto_reconnect,
            connector: Arc::new(DoipConnector::new(cfg, spec.la)),
        };
        let mut data = self.attach(ctx, link, sim, replaced.unwrap_or(0)).await?;
        data["entity"] = json!(addr.to_string());
        if let Some(prev) = replaced {
            data["replaced"] = json!(true);
            data["previous_generation"] = json!(prev);
        }
        Ok(data)
    }

    /// Connect (or return the existing connection of) a target over any [`Connector`]. Same
    /// request/lease/lane/runner/reconnect behaviour as `uds.connect`; only the link differs.
    pub async fn connect_link(&self, ctx: &CallCtx, spec: LinkSpec) -> Result<Value, DomainError> {
        let existing = lock(&self.targets).get(&spec.name).cloned();
        let mut replaced: Option<u32> = None;
        if let Some(t) = existing {
            if !t.is_replaceable() {
                let reconnecting = lock(&t.session).link() == crate::payload::LinkState::Down;
                return Ok(
                    json!({"target": t.name, "state": t.state(), "existing": true, "reconnecting": reconnecting}),
                );
            }
            let prev = t.generation();
            t.retire().await;
            {
                let mut m = lock(&self.targets);
                if m.get(&spec.name).is_some_and(|x| Arc::ptr_eq(x, &t)) {
                    m.remove(&spec.name);
                }
            }
            replaced = Some(prev);
        }
        if lock(&self.targets).len() >= MAX_TARGETS {
            return Err(DomainError::new(
                ErrorCode::Busy,
                format!("at most {MAX_TARGETS} UDS targets"),
            ));
        }
        let mut data = self.attach(ctx, spec, None, replaced.unwrap_or(0)).await?;
        if let Some(prev) = replaced {
            data["replaced"] = json!(true);
            data["previous_generation"] = json!(prev);
        }
        Ok(data)
    }

    /// Open the first link (generation `generation_base + 1`), start the runner, register and
    /// watch the target. `generation_base` = last generation of a replaced target (U6/U8), else 0.
    async fn attach(
        &self,
        ctx: &CallCtx,
        spec: LinkSpec,
        sim: Option<SimSide>,
        generation_base: u32,
    ) -> Result<Value, DomainError> {
        let link = open_link(spec.connector.as_ref(), u64::from(generation_base) + 1)
            .await
            .map_err(|e| DomainError::new(ErrorCode::NotOpen, e))?;
        let Link {
            transport,
            events: link_events,
            resources,
        } = link;

        let (tx, events) = mpsc::channel(CAP_RUNNER_EVENTS);
        let observer = Arc::new(QueueObserver {
            tx,
            dropped: AtomicU64::new(0),
        });
        let mut session = TargetSession::new(spec.name.clone()).with_generation_base(generation_base);
        session.set_endpoint(spec.backend, spec.connector.endpoint());
        let clock = HubClock::from_ctx(ctx);
        session.link_up(Duration::from_nanos(clock.mono_ns()));
        let (p2, p2s) = session.timing();
        let runner = Runner::new(
            transport,
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
            env: spec.env,
            auto_reconnect: spec.auto_reconnect,
            connector: spec.connector,
            session: Mutex::new(session),
            gate: tokio::sync::Mutex::new(Gate {
                handle: Some(handle),
                task: Some(task),
                events,
                link: Some(resources),
            }),
            lanes: PriorityGate::default(),
            ctl_handle: Mutex::new(Some(ctl_handle)),
            observer,
            publisher: Mutex::new(Some(ctx.clone())),
            timed_out: Mutex::new(VecDeque::new()),
            sim,
            clock,
            presend: self.presend.clone(),
            stopped: self.stopped.clone(),
            tp: Mutex::new(None),
            tp_in_flight: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            watch: Mutex::new(None),
            db: Mutex::new(None),
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
        *lock(&target.watch) = Some(tokio::spawn(watch_link(target.clone(), link_events)));
        target.publish(ctx, target.state_draft());
        Ok(json!({"target": target.name, "state": target.state(), "existing": false}))
    }

    /// Run one request through lease → gate → runner, publishing every stage.
    pub async fn request(&self, ctx: &CallCtx, spec: ReqSpec) -> Result<Outcome, DomainError> {
        let target = self.target(&spec.target)?;
        if target.is_closing() {
            return Err(DomainError::new(
                ErrorCode::NotOpen,
                format!(
                    "UDS target {:?} is closed (uds.connect opens it again)",
                    spec.target
                ),
            ));
        }
        let rid = spec
            .request_id
            .clone()
            .or_else(|| ctx.request_id().map(str::to_owned))
            .unwrap_or_else(|| self.next_request_id());
        let origin = ctx.origin();
        let class = classify_pdu(&spec.pdu, false);
        let (timing, stamp) = {
            let s = lock(&target.session);
            (s.timing(), s.route_stamp())
        };
        let db = target.db_snapshot();
        // The gateway write path already published `requested` and `accepted` for the call's own
        // request id (contract 02 §2.3); repeating them would step the row back. Rows of
        // sub-requests (`<rid>.verify`) are ours from the start.
        let core_row = spec.request_id.is_none() && ctx.request_id().is_some();
        let mut row = RowPayload::request(&target.name, &spec.pdu, false, WriteState::Requested, timing)
            .with_route(&stamp);
        row.db_hash = db.as_ref().map(|d| d.info.hash.clone());
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
                target.tp_on_lease_change();
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
                target.tp_on_lease_change();
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

        finish_row(
            &spec.pdu,
            &res,
            &mut row,
            db.as_ref().map_or(&StandardDids as &dyn DidLookup, |d| &d.db),
        );
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
        target.tp_on_lease_change();
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

    /// B2-M9 (Sonnet body): `uds.disconnect` (U6). See FUNCTIONS.md for the exact sequence.
    pub async fn disconnect(&self, ctx: &CallCtx, name: &str, abort: bool) -> Result<Value, DomainError> {
        let origin_str = |o: Origin| {
            serde_json::to_value(o)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        let t = self.target(name)?;
        if lock(&t.session)
            .lease()
            .is_some_and(|l| l.owner_origin == Origin::User)
            && ctx.origin() != Origin::User
        {
            return Err(DomainError::new(
                ErrorCode::HeldByUser,
                "target is held by the user",
            ));
        }
        if t.is_closing() || lock(&t.session).link() == crate::payload::LinkState::Closed {
            return Ok(json!({"target": t.name, "state": t.state(), "already_closed": true}));
        }
        let (wa, wu, wx, wc) = t.lanes.depth();
        let waiting = wa + wu + wx + wc;
        let busy = waiting > 0 || (t.lanes.is_busy() && !t.tp_in_flight.load(Ordering::SeqCst));
        if busy && !abort {
            return Err(DomainError::new(
                ErrorCode::Busy,
                "target has requests in progress; pass abort:true to abort them and close",
            )
            .with_detail(json!({"target": t.name, "waiting": waiting, "in_flight": t.lanes.is_busy()})));
        }
        let aborted = if abort {
            let cw = t.lanes.cancel(AbortKind::User, |_, _| true, false);
            let s = t.ctl().map(|h| h.abort(AbortScope::All));
            json!({
                "cancelled_waiting": cw + s.as_ref().map_or(0, |s| s.cancelled_waiting),
                "in_flight": s.as_ref().is_some_and(|s| s.in_flight),
            })
        } else {
            json!({"cancelled_waiting": 0, "in_flight": false})
        };
        t.retire().await;
        lock(&t.session).link_closed(format!("disconnected by {}", origin_str(ctx.origin())));
        t.sync_presend();
        t.publish(ctx, t.state_draft());
        Ok(json!({
            "target": t.name,
            "state": t.state(),
            "aborted": aborted,
            "others_since_last": self.others_since_last(ctx.client_id()),
        }))
    }

    /// B2-M10 (Sonnet body): `uds.forget` (U6).
    pub async fn forget(&self, ctx: &CallCtx, name: &str) -> Result<Value, DomainError> {
        let t = self.target(name)?;
        if lock(&t.session)
            .lease()
            .is_some_and(|l| l.owner_origin == Origin::User)
            && ctx.origin() != Origin::User
        {
            return Err(DomainError::new(
                ErrorCode::HeldByUser,
                "target is held by the user",
            ));
        }
        if !t.is_replaceable() {
            return Err(DomainError::new(
                ErrorCode::Busy,
                format!("target {name} is not closed (uds.disconnect first)"),
            ));
        }
        t.retire().await;
        {
            let mut m = lock(&self.targets);
            if m.get(name).is_some_and(|x| Arc::ptr_eq(x, &t)) {
                m.remove(name);
            }
        }
        self.presend.remove(name);
        let g = t.generation();
        lock(&t.session).mark_forgotten();
        t.publish(ctx, t.state_draft());
        Ok(json!({"target": name, "forgotten": true, "last_generation": g}))
    }

    /// B2-M11 (Sonnet body): `uds.tester_present` (U1).
    pub fn tester_present(
        &self,
        ctx: &CallCtx,
        name: Option<&str>,
        req: TpRequest,
    ) -> Result<Value, DomainError> {
        let origin_str = |o: Origin| {
            serde_json::to_value(o)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        let t = self.resolve(name)?;
        let req_cfg = match req {
            TpRequest::Off => {
                let was = t.stop_tp(&format!("stopped by {}", origin_str(ctx.origin())));
                return Ok(json!({"target": t.name, "state": t.state(), "was_active": was}));
            }
            TpRequest::On(cfg) => cfg,
        };
        if self.stopped() {
            return Err(DomainError::new(
                ErrorCode::Stopped,
                "STOP is latched: TesterPresent not started",
            ));
        }
        if t.is_closing() || lock(&t.session).link() != crate::payload::LinkState::Up {
            return Err(DomainError::new(
                ErrorCode::NotOpen,
                format!("target {} link is not up", t.name),
            ));
        }
        if lock(&t.session)
            .lease()
            .is_some_and(|l| l.owner_origin == Origin::User)
            && ctx.origin() != Origin::User
        {
            return Err(DomainError::new(
                ErrorCode::HeldByUser,
                "target is held by the user",
            ));
        }
        let owner = Lease {
            owner_origin: ctx.origin(),
            actor_id: ctx.client_id().to_owned(),
            client_id: ctx.client_id().to_owned(),
        };
        t.stop_tp("replaced by a new TesterPresent job");
        let generation = t.generation();
        let cancel = Arc::new(TpCancel::default());
        lock(&t.session).tp_start(req_cfg.interval, req_cfg.suppress, owner.clone());
        {
            // the slot is filled before the task can stop itself (stop_tp_job locks it too)
            let mut slot = lock(&t.tp);
            let task = tokio::spawn(tp_loop(
                t.clone(),
                ctx.clone(),
                req_cfg,
                owner.clone(),
                generation,
                cancel.clone(),
            ));
            *slot = Some(TpJob {
                cfg: req_cfg,
                owner,
                generation,
                cancel,
                task,
            });
        }
        t.publish(ctx, t.state_draft());
        Ok(json!({"target": t.name, "state": t.state()}))
    }

    /// B3-M5 (Sonnet body): `uds.security_unlock` (U2). Provider config is resolved before any byte;
    /// seed `27 level` as sub-request `<rid>.seed` (R1); the approved R3 sendKey `27 level+1 key` is the
    /// call's own request. No retry of anything. See FUNCTIONS.md B3-M5 for the exact sequence.
    pub async fn security_unlock(
        &self,
        ctx: &CallCtx,
        name: Option<&str>,
        level: u8,
        provider: &str,
        timeout: Option<Duration>,
    ) -> Result<Value, DomainError> {
        let t = self.resolve(name)?;
        let cfg = provider_from_config(&ctx.config(), provider, level)
            .map_err(|e| DomainError::new(e.code(), e.message()))?;
        let rid = ctx
            .request_id()
            .map(str::to_owned)
            .unwrap_or_else(|| self.next_request_id());
        let seed_rid = format!("{rid}.seed");
        let unexpected = |seed_rid: &str| {
            DomainError::new(ErrorCode::Internal, "unexpected seed response")
                .with_detail(json!({"seed_request_id": seed_rid}))
        };
        let out = self
            .request(
                ctx,
                ReqSpec {
                    target: t.name().into(),
                    pdu: vec![0x27, level],
                    timeout,
                    request_id: Some(seed_rid.clone()),
                    recovery: false,
                },
            )
            .await?;
        if !out
            .result
            .as_ref()
            .is_some_and(|r| matches!(r.status, TxnStatus::Positive))
        {
            return match out.into_result() {
                Err(e) => Err(e),
                Ok(_) => Err(unexpected(&seed_rid)),
            };
        }
        let resp = out
            .result
            .as_ref()
            .and_then(|r| r.response.clone())
            .unwrap_or_default();
        let seed = match resp.as_slice() {
            [0x67, l, seed @ ..] if *l == level && !seed.is_empty() => seed.to_vec(),
            _ => return Err(unexpected(&seed_rid)),
        };
        if seed.iter().all(|b| *b == 0) {
            return Ok(json!({
                "target": t.name(),
                "level": level,
                "unlocked": true,
                "already_unlocked": true,
                "seed_request_id": seed_rid,
                "others_since_last": self.others_since_last(ctx.client_id()),
            }));
        }
        if stoppable(ctx.origin()) && self.stopped() {
            return Err(DomainError::new(
                ErrorCode::Stopped,
                "STOP is latched: key provider not started, no key sent",
            )
            .with_detail(json!({"seed_request_id": seed_rid})));
        }
        let key = run_provider(&cfg, level, &seed).await.map_err(|e| {
            DomainError::new(e.code(), e.message()).with_detail(json!({"seed_request_id": seed_rid}))
        })?;
        let mut pdu = vec![0x27, level + 1];
        pdu.extend_from_slice(&key);
        let out = self
            .request(
                ctx,
                ReqSpec {
                    target: t.name().into(),
                    pdu,
                    timeout,
                    request_id: if ctx.request_id().is_some() {
                        None
                    } else {
                        Some(rid)
                    },
                    recovery: false,
                },
            )
            .await?;
        let unlocked = out
            .result
            .as_ref()
            .is_some_and(|r| matches!(r.status, TxnStatus::Positive));
        let mut d = out.into_result()?;
        d["target"] = json!(t.name());
        d["level"] = json!(level);
        d["unlocked"] = json!(unlocked);
        d["already_unlocked"] = json!(false);
        d["seed_request_id"] = json!(seed_rid);
        Ok(d)
    }

    /// B3-M6 (Sonnet body): `uds.db.set` (U5): validate and attach an inline DB to an open target.
    pub fn db_set(&self, ctx: &CallCtx, name: &str, db: &Value) -> Result<Value, DomainError> {
        let t = self.target(name)?;
        if lock(&t.session)
            .lease()
            .is_some_and(|l| l.owner_origin == Origin::User)
            && ctx.origin() != Origin::User
        {
            return Err(DomainError::new(
                ErrorCode::HeldByUser,
                "target is held by the user",
            ));
        }
        if t.is_closing() || lock(&t.session).link() == crate::payload::LinkState::Closed {
            return Err(DomainError::new(
                ErrorCode::NotOpen,
                format!("target {name} is closed (uds.connect opens it again)"),
            ));
        }
        let loaded = parse_inline_db(db).map_err(|m| DomainError::new(ErrorCode::Usage, m))?;
        let warnings = db_warnings(&loaded);
        let att = Arc::new(AttachedDb::new(loaded));
        let info = att.info.clone();
        let replaced = lock(&t.db).replace(att).is_some();
        t.publish(ctx, t.state_draft());
        Ok(
            json!({"target": name, "db": info, "warnings": warnings, "replaced": replaced, "state": t.state()}),
        )
    }

    /// B3-M7 (Sonnet body): `uds.db.clear` (U5).
    pub fn db_clear(&self, ctx: &CallCtx, name: &str) -> Result<Value, DomainError> {
        let t = self.target(name)?;
        if lock(&t.session)
            .lease()
            .is_some_and(|l| l.owner_origin == Origin::User)
            && ctx.origin() != Origin::User
        {
            return Err(DomainError::new(
                ErrorCode::HeldByUser,
                "target is held by the user",
            ));
        }
        let cleared = lock(&t.db).take().is_some();
        if cleared {
            t.publish(ctx, t.state_draft());
        }
        Ok(json!({"target": name, "cleared": cleared, "state": t.state()}))
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
            t.stop_tp("STOP");
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
