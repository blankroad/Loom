//! Single-target transaction runner (plan §3.4, U-06).
//!
//! One request at a time: quarantine check → submit (`sent`) → transport confirm → P2 for the
//! first response → `0x78` switches to P2* (bounded count) → final response, all inside the total
//! deadline that started when the request was received. Late responses never complete a later
//! request; they are reported as `late` and may release the unresolved set.

use std::sync::Arc;
use std::time::Duration;

use loom_proto::error::ErrorCode;
use loom_proto::event::WriteState;
use loom_uds_codec::{MAX_PDU_LEN, ResponseKind, SPRMIB, classify_response, service};
use tokio::time::Instant;

use crate::quarantine::{ReleaseCause, Unresolved};
use crate::transport::{Incoming, RecvError, Transport, TransportError};

/// Default P2 (first response) — target profile value, not an ISO constant (plan §3.3).
pub const DEFAULT_P2: Duration = Duration::from_millis(50);
/// Default P2* (after `0x78`).
pub const DEFAULT_P2_STAR: Duration = Duration::from_millis(5000);
/// Default limit of `0x78` responses per transaction.
pub const DEFAULT_MAX_RESPONSE_PENDING: u32 = 50;
/// Hard cap on the `0x78` limit (plan §3.8); larger configured values are clamped.
pub const MAX_RESPONSE_PENDING_HARD: u32 = 1024;
/// Default minimum quiet period after a timeout before a non-overlapping request is sent.
pub const DEFAULT_QUARANTINE_QUIET: Duration = Duration::from_secs(1);
/// Default limit from response start (ISO-TP first frame) to PDU completion.
pub const DEFAULT_COMPLETION_LIMIT: Duration = Duration::from_secs(1);
/// Most probe candidates kept from the configuration; extra candidates are ignored.
pub const MAX_PROBE_CANDIDATES: usize = 8;

/// Runner timing and quarantine configuration.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub p2: Duration,
    pub p2_star: Duration,
    /// `0x78` responses allowed per transaction (clamped to [`MAX_RESPONSE_PENDING_HARD`]).
    pub max_response_pending: u32,
    /// Quiet period is `max(p2_star, quarantine_quiet)` after the newest unresolved request.
    pub quarantine_quiet: Duration,
    pub completion_limit: Duration,
    /// Verified-profile bound after which a late response can no longer arrive. `None` (default):
    /// age never releases U.
    pub late_response_max: Option<Duration>,
    /// Liveness probe requests in preference order (R0, unique SIDs; default `3E 00`, `22 F186`).
    pub probe_candidates: Vec<Vec<u8>>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        RunnerConfig {
            p2: DEFAULT_P2,
            p2_star: DEFAULT_P2_STAR,
            max_response_pending: DEFAULT_MAX_RESPONSE_PENDING,
            quarantine_quiet: DEFAULT_QUARANTINE_QUIET,
            completion_limit: DEFAULT_COMPLETION_LIMIT,
            late_response_max: None,
            probe_candidates: vec![vec![0x3E, 0x00], vec![0x22, 0xF1, 0x86]],
        }
    }
}

/// Maps tokio instants to the gateway's monotonic nanoseconds (`ts.mono_ns`, `core.time`).
#[derive(Debug, Clone, Copy)]
pub struct MonoClock {
    origin: Instant,
    origin_ns: u64,
}

impl MonoClock {
    /// `origin` corresponds to `origin_ns` on the gateway clock.
    pub fn new(origin: Instant, origin_ns: u64) -> Self {
        MonoClock { origin, origin_ns }
    }

    /// Origin = now, 0 ns.
    pub fn starting_now() -> Self {
        MonoClock::new(Instant::now(), 0)
    }

    pub fn ns(&self, t: Instant) -> u64 {
        let d = u64::try_from(t.saturating_duration_since(self.origin).as_nanos()).unwrap_or(u64::MAX);
        self.origin_ns.saturating_add(d)
    }
}

/// Read or write (decided by the domain's classifier). Changes only the timeout/link-loss
/// mapping: writes become `outcome_unknown`, reads `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// One request.
#[derive(Debug, Clone)]
pub struct TxnRequest {
    pub pdu: Vec<u8>,
    /// Total deadline, counted from `received_at` (queue wait included).
    pub deadline: Instant,
    pub access: Access,
    /// When the RPC was received.
    pub received_at: Instant,
}

impl TxnRequest {
    pub fn new(pdu: Vec<u8>, access: Access, timeout: Duration) -> Self {
        let now = Instant::now();
        TxnRequest {
            pdu,
            deadline: now + timeout,
            access,
            received_at: now,
        }
    }

    pub fn read(pdu: Vec<u8>, timeout: Duration) -> Self {
        TxnRequest::new(pdu, Access::Read, timeout)
    }

    pub fn write(pdu: Vec<u8>, timeout: Duration) -> Self {
        TxnRequest::new(pdu, Access::Write, timeout)
    }
}

/// Where a timeout happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutPhase {
    /// Not handed to the link before the deadline (may still be sent).
    Handoff,
    /// No transport confirmation (DoIP diagnostic ACK).
    Ack,
    /// No first response within P2.
    P2,
    /// No response within P2* after `0x78`.
    P2Star,
    /// Response started but did not complete within the completion limit.
    Completion,
    /// Total deadline reached first.
    Deadline,
    /// More `0x78` responses than allowed.
    PendingLimit,
}

/// Why a request was not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// Target queue full (`MAX_PENDING_PER_TARGET`).
    QueueFull,
    /// The SID overlaps an unresolved request.
    Quarantine {
        sid: u8,
    },
    /// Unresolved set full.
    QuarantineCapacity,
    /// The quiet period would outlast the deadline.
    QuarantineQuiet,
    /// Deadline passed before the request could be sent.
    Expired,
    InvalidPdu,
    /// Transport refused before hand-off (closed, queue full, busy).
    NotSent(TransportError),
    /// The runner task has stopped.
    RunnerStopped,
}

/// Transaction outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxnStatus {
    Positive,
    Negative {
        nrc: u8,
    },
    /// SPRMIB set and no NRC within P2: not proof of execution.
    OkSuppressed,
    Timeout {
        phase: TimeoutPhase,
    },
    /// Transport refused to forward (DoIP diagnostic NACK).
    TransportNack {
        code: u8,
    },
    LinkLost,
    GenerationChanged,
    Rejected(RejectReason),
}

/// Monotonic timestamps (gateway `mono_ns`) for the UI to compare against `core.time`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TxnTimes {
    pub received_mono_ns: u64,
    pub sent_mono_ns: Option<u64>,
    pub p2_start_mono_ns: Option<u64>,
    pub first_rx_mono_ns: Option<u64>,
    pub final_rx_mono_ns: Option<u64>,
}

/// Transaction result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxnResult {
    pub status: TxnStatus,
    /// Contract 02 `write_state` after this transaction (the domain does read-back verification).
    pub write_state: WriteState,
    /// A device timeout ended the transaction.
    pub timeout: bool,
    pub nrc: Option<u8>,
    /// Handed to the link.
    pub sent: bool,
    /// The ECU may have executed the request (never auto-retry writes when true).
    pub may_have_executed: bool,
    /// Final response bytes (positive or negative).
    pub response: Option<Vec<u8>>,
    pub pending_count: u32,
    /// Late/foreign responses seen while this transaction ran (reported, never used).
    pub late_count: u32,
    pub generation: u64,
    pub times: TxnTimes,
    /// Contract error code (`None` = success: positive or suppressed).
    pub error: Option<ErrorCode>,
}

impl TxnResult {
    /// Result for a request that never reached the runner loop.
    pub fn rejected(req: &TxnRequest, reason: RejectReason, clock: &MonoClock, generation: u64) -> TxnResult {
        let mut t = Txn::new(req, clock);
        t.generation = generation;
        t.finish(TxnStatus::Rejected(reason), clock)
    }
}

/// Live events for publish-before-reply (`uds.tx`, `uds.pending`, `uds.late`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerEvent<'a> {
    Sent {
        pdu: &'a [u8],
        mono_ns: u64,
        generation: u64,
    },
    Pending {
        count: u32,
        mono_ns: u64,
    },
    /// A response that does not belong to the request in flight (late, foreign or malformed).
    Late {
        data: &'a [u8],
        mono_ns: u64,
        released: bool,
    },
    QuarantineReleased {
        count: usize,
        cause: ReleaseCause,
    },
}

/// Receives runner events synchronously. Must not block (publish into a bounded queue).
pub trait RunnerObserver: Send + Sync {
    fn on_event(&self, event: &RunnerEvent<'_>);
}

/// Observer that ignores everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopObserver;

impl RunnerObserver for NoopObserver {
    fn on_event(&self, _event: &RunnerEvent<'_>) {}
}

/// Mutable transaction record.
#[derive(Debug)]
struct Txn {
    access: Access,
    received_at: Instant,
    sent_at: Option<Instant>,
    p2_start: Option<Instant>,
    first_rx: Option<Instant>,
    final_rx: Option<Instant>,
    pending: u32,
    late: u32,
    generation: u64,
    response: Option<Vec<u8>>,
}

impl Txn {
    fn new(req: &TxnRequest, _clock: &MonoClock) -> Txn {
        Txn {
            access: req.access,
            received_at: req.received_at,
            sent_at: None,
            p2_start: None,
            first_rx: None,
            final_rx: None,
            pending: 0,
            late: 0,
            generation: 0,
            response: None,
        }
    }

    fn finish(self, status: TxnStatus, clock: &MonoClock) -> TxnResult {
        let sent = self.sent_at.is_some();
        let write = self.access == Access::Write;
        let lost = |code| {
            let ws = if sent && write {
                WriteState::OutcomeUnknown
            } else {
                WriteState::Failed
            };
            (ws, false, sent, Some(code))
        };
        let (write_state, timeout, may_have_executed, error) = match &status {
            TxnStatus::Positive => (WriteState::Confirmed, false, true, None),
            TxnStatus::Negative { .. } => (WriteState::Failed, false, false, Some(ErrorCode::Nrc)),
            TxnStatus::OkSuppressed => (WriteState::Sent, false, true, None),
            TxnStatus::Timeout { phase } => {
                let maybe = sent || *phase == TimeoutPhase::Handoff;
                let ws = if write && maybe {
                    WriteState::OutcomeUnknown
                } else {
                    WriteState::Failed
                };
                (ws, true, maybe, Some(ErrorCode::NoResponse))
            }
            TxnStatus::TransportNack { code } => {
                let e = match code {
                    0x03 | 0x06 | 0x07 => ErrorCode::NotFound,
                    _ => ErrorCode::NoResponse,
                };
                (WriteState::Failed, false, false, Some(e))
            }
            TxnStatus::LinkLost => lost(ErrorCode::Disconnected),
            TxnStatus::GenerationChanged => lost(ErrorCode::ConnectionChanged),
            TxnStatus::Rejected(reason) => {
                let e = match reason {
                    RejectReason::QueueFull | RejectReason::Expired => ErrorCode::Busy,
                    RejectReason::Quarantine { .. }
                    | RejectReason::QuarantineCapacity
                    | RejectReason::QuarantineQuiet => ErrorCode::Quarantine,
                    RejectReason::InvalidPdu => ErrorCode::Usage,
                    RejectReason::NotSent(TransportError::Closed) => ErrorCode::Disconnected,
                    RejectReason::NotSent(TransportError::TxFull | TransportError::Busy) => ErrorCode::Busy,
                    RejectReason::NotSent(_) | RejectReason::RunnerStopped => ErrorCode::NotOpen,
                };
                (WriteState::Failed, false, false, Some(e))
            }
        };
        let nrc = match status {
            TxnStatus::Negative { nrc } => Some(nrc),
            _ => None,
        };
        TxnResult {
            write_state,
            timeout,
            nrc,
            sent,
            may_have_executed,
            response: self.response,
            pending_count: self.pending,
            late_count: self.late,
            generation: self.generation,
            times: TxnTimes {
                received_mono_ns: clock.ns(self.received_at),
                sent_mono_ns: self.sent_at.map(|t| clock.ns(t)),
                p2_start_mono_ns: self.p2_start.map(|t| clock.ns(t)),
                first_rx_mono_ns: self.first_rx.map(|t| clock.ns(t)),
                final_rx_mono_ns: self.final_rx.map(|t| clock.ns(t)),
            },
            error,
            status,
        }
    }
}

/// `0x11` ECUReset positive response confirms a reset.
const ECU_RESET_SID: u8 = 0x11;

/// Runner for one target. Use directly (`execute`) or through [`crate::RunnerHandle`].
pub struct Runner {
    transport: Box<dyn Transport>,
    cfg: RunnerConfig,
    clock: MonoClock,
    observer: Arc<dyn RunnerObserver>,
    unresolved: Unresolved,
    last_generation: u64,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner")
            .field("cfg", &self.cfg)
            .field("unresolved", &self.unresolved)
            .field("last_generation", &self.last_generation)
            .finish_non_exhaustive()
    }
}

impl Runner {
    pub fn new(
        transport: Box<dyn Transport>,
        mut cfg: RunnerConfig,
        clock: MonoClock,
        observer: Arc<dyn RunnerObserver>,
    ) -> Runner {
        cfg.max_response_pending = cfg.max_response_pending.min(MAX_RESPONSE_PENDING_HARD);
        cfg.probe_candidates.truncate(MAX_PROBE_CANDIDATES);
        let last_generation = transport.generation();
        Runner {
            transport,
            cfg,
            clock,
            observer,
            unresolved: Unresolved::default(),
            last_generation,
        }
    }

    pub fn config(&self) -> &RunnerConfig {
        &self.cfg
    }

    pub fn clock(&self) -> &MonoClock {
        &self.clock
    }

    pub fn generation(&self) -> u64 {
        self.transport.generation()
    }

    /// The unresolved set U.
    pub fn unresolved(&self) -> &Unresolved {
        &self.unresolved
    }

    /// Update P2/P2* (e.g. from a `0x50` response).
    pub fn set_timing(&mut self, p2: Duration, p2_star: Duration) {
        self.cfg.p2 = p2;
        self.cfg.p2_star = p2_star;
    }

    /// Replace the transport after a reconnect. A different generation releases U on the next
    /// request.
    pub fn replace_transport(&mut self, transport: Box<dyn Transport>) {
        self.transport = transport;
    }

    /// External evidence that the ECU reset (e.g. confirmed by another path): release U.
    pub fn note_ecu_reset(&mut self) {
        self.release_all(ReleaseCause::EcuReset);
    }

    fn release_all(&mut self, cause: ReleaseCause) {
        let count = self.unresolved.clear();
        if count > 0 {
            self.observer
                .on_event(&RunnerEvent::QuarantineReleased { count, cause });
        }
    }

    fn sync_generation(&mut self) {
        let g = self.transport.generation();
        if g != self.last_generation {
            self.last_generation = g;
            self.release_all(ReleaseCause::GenerationChanged);
        }
    }

    fn expire_unresolved(&mut self) {
        if let Some(max) = self.cfg.late_response_max {
            let count = self.unresolved.expire(Instant::now(), max);
            if count > 0 {
                self.observer.on_event(&RunnerEvent::QuarantineReleased {
                    count,
                    cause: ReleaseCause::LateWindowElapsed,
                });
            }
        }
    }

    fn on_late(&mut self, data: &[u8], at: Instant, txn: &mut Txn) {
        let released = self.unresolved.release_by_response(data).is_some();
        txn.late = txn.late.saturating_add(1);
        self.observer.on_event(&RunnerEvent::Late {
            data,
            mono_ns: self.clock.ns(at),
            released,
        });
        if released {
            self.observer.on_event(&RunnerEvent::QuarantineReleased {
                count: 1,
                cause: ReleaseCause::LateFinalResponse,
            });
        }
    }

    fn drain_late(&mut self, txn: &mut Txn) {
        while let Some(item) = self.transport.try_recv() {
            if let Incoming::Pdu { data, at } = item {
                self.on_late(&data, at, txn);
            }
        }
    }

    /// Wait out the quiet period, treating everything received as late. `Err` ends the request.
    /// Ends early once every unresolved request has been released.
    async fn wait_quiet(&mut self, until: Instant, txn: &mut Txn) -> Result<(), TxnStatus> {
        loop {
            match self.transport.recv(until).await {
                Ok(Incoming::Pdu { data, at }) => {
                    self.on_late(&data, at, txn);
                    if self.unresolved.is_empty() {
                        return Ok(());
                    }
                }
                Ok(Incoming::RxStart { .. }) => {}
                Err(RecvError::Timeout) => return Ok(()),
                Err(RecvError::Closed) => {
                    return Err(TxnStatus::Rejected(RejectReason::NotSent(TransportError::Closed)));
                }
                Err(RecvError::GenerationChanged) => {
                    self.sync_generation();
                    return Ok(());
                }
            }
        }
    }

    fn timed_out(&mut self, pdu: &[u8], phase: TimeoutPhase, txn: Txn) -> TxnResult {
        if !self.unresolved.insert(pdu, Instant::now()) {
            tracing::warn!("unresolved set full; timed-out request not recorded");
        }
        txn.finish(TxnStatus::Timeout { phase }, &self.clock)
    }

    /// Run one request to completion.
    pub async fn execute(&mut self, req: TxnRequest) -> TxnResult {
        let mut txn = Txn::new(&req, &self.clock);
        let pdu = req.pdu.as_slice();
        let Some(&sid) = pdu.first() else {
            return txn.finish(TxnStatus::Rejected(RejectReason::InvalidPdu), &self.clock);
        };
        if pdu.len() > MAX_PDU_LEN {
            return txn.finish(TxnStatus::Rejected(RejectReason::InvalidPdu), &self.clock);
        }
        self.sync_generation();
        self.expire_unresolved();
        self.drain_late(&mut txn);
        txn.generation = self.transport.generation();
        if Instant::now() >= req.deadline {
            return txn.finish(TxnStatus::Rejected(RejectReason::Expired), &self.clock);
        }

        // Quarantine (D19): overlapping SID → refuse; otherwise wait out the quiet period.
        if self.unresolved.overlaps(sid) {
            return txn.finish(TxnStatus::Rejected(RejectReason::Quarantine { sid }), &self.clock);
        }
        if self.unresolved.is_full() {
            return txn.finish(TxnStatus::Rejected(RejectReason::QuarantineCapacity), &self.clock);
        }
        if let Some(newest) = self.unresolved.newest() {
            let quiet_until = newest + self.cfg.p2_star.max(self.cfg.quarantine_quiet);
            if quiet_until > req.deadline {
                return txn.finish(TxnStatus::Rejected(RejectReason::QuarantineQuiet), &self.clock);
            }
            if let Err(status) = self.wait_quiet(quiet_until, &mut txn).await {
                return txn.finish(status, &self.clock);
            }
            txn.generation = self.transport.generation();
        }

        // Send.
        let generation = self.transport.generation();
        let sent_at = match self.transport.submit(pdu, req.deadline).await {
            Ok(at) => at,
            Err(TransportError::TxTimeout) => return self.timed_out(pdu, TimeoutPhase::Handoff, txn),
            Err(TransportError::GenerationChanged) => {
                return txn.finish(TxnStatus::GenerationChanged, &self.clock);
            }
            Err(e) => return txn.finish(TxnStatus::Rejected(RejectReason::NotSent(e)), &self.clock),
        };
        txn.sent_at = Some(sent_at);
        self.observer.on_event(&RunnerEvent::Sent {
            pdu,
            mono_ns: self.clock.ns(sent_at),
            generation,
        });

        let p2_start = match self.transport.confirm(req.deadline).await {
            Ok(at) => at,
            Err(TransportError::Nack(code)) => {
                return txn.finish(TxnStatus::TransportNack { code }, &self.clock);
            }
            Err(TransportError::AckTimeout | TransportError::TxTimeout) => {
                return self.timed_out(pdu, TimeoutPhase::Ack, txn);
            }
            Err(TransportError::GenerationChanged) => {
                return txn.finish(TxnStatus::GenerationChanged, &self.clock);
            }
            Err(e) => {
                tracing::debug!("confirm failed: {e}");
                return txn.finish(TxnStatus::LinkLost, &self.clock);
            }
        };
        txn.p2_start = Some(p2_start);

        // Responses.
        let suppress =
            service(sid).is_some_and(|s| s.has_subfunction) && pdu.get(1).is_some_and(|b| b & SPRMIB != 0);
        let mut phase = TimeoutPhase::P2;
        let mut timer = p2_start + self.cfg.p2;
        loop {
            let until = timer.min(req.deadline);
            match self.transport.recv(until).await {
                Ok(Incoming::RxStart { at }) => {
                    txn.first_rx.get_or_insert(at);
                    if phase != TimeoutPhase::Completion {
                        phase = TimeoutPhase::Completion;
                        timer = at + self.cfg.completion_limit;
                    }
                }
                Ok(Incoming::Pdu { data, at }) => {
                    if self.transport.generation() != generation {
                        return txn.finish(TxnStatus::GenerationChanged, &self.clock);
                    }
                    match classify_response(pdu, &data) {
                        ResponseKind::Positive => {
                            txn.first_rx.get_or_insert(at);
                            txn.final_rx = Some(at);
                            txn.response = Some(data);
                            if sid == ECU_RESET_SID {
                                self.release_all(ReleaseCause::EcuReset);
                            }
                            return txn.finish(TxnStatus::Positive, &self.clock);
                        }
                        ResponseKind::Negative { nrc } => {
                            txn.first_rx.get_or_insert(at);
                            txn.final_rx = Some(at);
                            txn.response = Some(data);
                            return txn.finish(TxnStatus::Negative { nrc }, &self.clock);
                        }
                        ResponseKind::Pending => {
                            txn.first_rx.get_or_insert(at);
                            txn.pending = txn.pending.saturating_add(1);
                            self.observer.on_event(&RunnerEvent::Pending {
                                count: txn.pending,
                                mono_ns: self.clock.ns(at),
                            });
                            if txn.pending > self.cfg.max_response_pending {
                                return self.timed_out(pdu, TimeoutPhase::PendingLimit, txn);
                            }
                            phase = TimeoutPhase::P2Star;
                            timer = at + self.cfg.p2_star;
                        }
                        ResponseKind::Foreign { .. } | ResponseKind::Malformed { .. } => {
                            self.on_late(&data, at, &mut txn);
                        }
                    }
                }
                Err(RecvError::Timeout) => {
                    let deadline_first = req.deadline < timer;
                    if suppress && !deadline_first && phase == TimeoutPhase::P2 && txn.pending == 0 {
                        return txn.finish(TxnStatus::OkSuppressed, &self.clock);
                    }
                    let hit = if deadline_first {
                        TimeoutPhase::Deadline
                    } else {
                        phase
                    };
                    return self.timed_out(pdu, hit, txn);
                }
                Err(RecvError::Closed) => return txn.finish(TxnStatus::LinkLost, &self.clock),
                Err(RecvError::GenerationChanged) => {
                    return txn.finish(TxnStatus::GenerationChanged, &self.clock);
                }
            }
        }
    }

    /// Liveness probe: the first configured candidate whose SID does not overlap U (a timed-out
    /// probe joins U, so it is not reused). Success proves the ECU is alive — it does **not**
    /// release U. `None` when every candidate overlaps U.
    pub async fn probe(&mut self, timeout: Duration) -> Option<TxnResult> {
        self.sync_generation();
        let candidate = self
            .cfg
            .probe_candidates
            .iter()
            .find(|c| c.first().is_some_and(|s| !self.unresolved.overlaps(*s)))?
            .clone();
        Some(self.execute(TxnRequest::read(candidate, timeout)).await)
    }
}
