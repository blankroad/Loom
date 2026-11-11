//! Bounded tokio DoIP client (tester side).
//!
//! One TCP connection per entity + tester SA, shared by many target logical addresses.
//! Tasks: one reader (answers alive checks immediately, routes ACK/NACK and responses by source
//! LA, never blocks on consumers) and one writer (control frames first). Every queue is bounded;
//! see the `CAP_*`/`MAX_*` constants for the overflow policy.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use thiserror::Error;
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};

use crate::frame::{ActivationCode, ActivationType, EncodeError, MAX_UDS_PDU, Message, ProtocolVersion};
use crate::io::{MAX_FRAME_WRITE_TIME, ReadError, read_message, write_bounded};

/// Most target logical addresses one connection serves. Beyond: [`DoipError::TooManyTargets`].
pub const MAX_TARGETS_PER_CONNECTION: usize = 64;
/// Responses buffered per target before the consumer reads them. Full: the response is dropped,
/// counted, and [`LinkEvent::RxOverflow`] is emitted (the reader never waits on a consumer).
pub const CAP_RX_PER_TARGET: usize = 32;
/// PDU bytes queued for one target. Full: drop/count, RxOverflow + sticky degraded health.
pub const MAX_RX_BYTES_PER_TARGET: usize = 32 * 1024;
/// PDU bytes queued across all LAs on one connection. Same overflow policy; never block reader.
pub const MAX_RX_BYTES_PER_CONNECTION: usize = 64 * 1024;
/// Uncertain ACK tombstones survive target drop/reopen. Exhaustion closes the connection;
/// entries are never evicted to permit ambiguous ACK reuse.
pub const MAX_POISONED_LAS: usize = MAX_TARGETS_PER_CONNECTION;
/// Diagnostic/activation frames waiting for the writer. Full: [`DoipError::TxFull`] (new request
/// refused before it is sent).
pub const CAP_TX_FRAMES: usize = 32;
/// Control frames (alive check response, generic NACK) waiting for the writer; served before data.
/// Full: the frame is dropped with a warning. Fatal header NACKs use a separate single slot.
pub const CAP_CONTROL_FRAMES: usize = 8;
/// Link events waiting for the owner. Full: the event is dropped and counted in
/// [`ClientStats::dropped_link_events`]; [`DoipClient::link_state`] always has the current state.
pub const CAP_LINK_EVENTS: usize = 32;

/// Default TCP connect timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Default routing activation response timeout (ISO `A_DoIP_Ctrl`, 2 s).
pub const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(2);
/// Default diagnostic ACK timeout (ISO `A_DoIP_Diagnostic_Message`, 2 s).
pub const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// Client configuration.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub entity: SocketAddr,
    pub tester_la: u16,
    pub activation_type: ActivationType,
    pub oem: Option<[u8; 4]>,
    pub version: ProtocolVersion,
    pub connect_timeout: Duration,
    pub activation_timeout: Duration,
    pub ack_timeout: Duration,
    /// Link generation reported to the runner. The owner increments it on every reconnect.
    pub generation: u64,
}

impl ClientConfig {
    pub fn new(entity: SocketAddr, tester_la: u16) -> Self {
        ClientConfig {
            entity,
            tester_la,
            activation_type: ActivationType::DEFAULT,
            oem: None,
            version: ProtocolVersion::Iso2012,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            activation_timeout: DEFAULT_ACTIVATION_TIMEOUT,
            ack_timeout: DEFAULT_ACK_TIMEOUT,
            generation: 0,
        }
    }
}

/// Current link state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Activating,
    Active,
    Closed,
}

/// Sticky receive health, independent of the bounded link-event queue. Reconnect resets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientHealth {
    Healthy,
    RxOverflowed,
}

/// Link events (bounded channel, see [`CAP_LINK_EVENTS`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    Activated {
        entity_la: u16,
        code: ActivationCode,
    },
    Reactivated {
        entity_la: u16,
        code: ActivationCode,
    },
    ActivationDenied {
        code: ActivationCode,
    },
    /// Entity sent a generic header NACK.
    GenericNack {
        code: u8,
    },
    /// A response for `target_la` was dropped because its queue was full.
    RxOverflow {
        target_la: u16,
        dropped_total: u64,
    },
    /// A diagnostic message from a source LA nobody opened.
    Unrouted {
        source_la: u16,
    },
    /// Protocol violation by the entity; the connection was closed.
    Error {
        reason: String,
    },
    /// Socket closed (peer, I/O error or local close).
    Closed {
        reason: String,
    },
}

/// Client errors.
#[derive(Debug, Error)]
pub enum DoipError {
    #[error("connect to {0} timed out")]
    ConnectTimeout(SocketAddr),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("routing activation denied: 0x{:02X} ({})", .0.0, .0.name())]
    ActivationDenied(ActivationCode),
    #[error("routing activation response timed out")]
    ActivationTimeout,
    #[error("routing activation already in progress")]
    ActivationBusy,
    #[error("connection closed")]
    Closed,
    #[error("transmit queue full")]
    TxFull,
    #[error("frame not handed to the socket before the deadline")]
    TxTimeout,
    #[error("target 0x{0:04X} already open on this connection")]
    TargetInUse(u16),
    #[error("too many targets on one connection (max {MAX_TARGETS_PER_CONNECTION})")]
    TooManyTargets,
    #[error("previous diagnostic message to 0x{0:04X} still awaits its ACK")]
    AckPending(u16),
    #[error("invalid UDS PDU length {0} (1..={MAX_UDS_PDU})")]
    InvalidPdu(usize),
    #[error("diagnostic NACK 0x{0:02X}")]
    Nack(u8),
    #[error("diagnostic ACK timed out")]
    AckTimeout,
    #[error(transparent)]
    Encode(#[from] EncodeError),
}

/// One diagnostic response for a target, stamped when the reader decoded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub data: Vec<u8>,
    pub at: Instant,
}

/// Counters for health display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientStats {
    pub dropped_link_events: u64,
    pub unrouted: u64,
    pub alive_checks_answered: u64,
    pub rx_buffered_bytes: usize,
    pub rx_dropped_total: u64,
}

#[derive(Debug)]
enum AckOutcome {
    Ack(Instant),
    Nack(u8, Instant),
}

#[derive(Debug)]
struct TxItem {
    frame: Vec<u8>,
    deadline: Instant,
    control: Arc<TxControl>,
    handed: Option<oneshot::Sender<Result<Instant, DoipError>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TxPhase {
    Queued,
    Writing,
    Handed,
    Cancelled,
}

#[derive(Debug)]
struct TxControl {
    phase: watch::Sender<TxPhase>,
    reply_received: AtomicBool,
}

impl TxControl {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            phase: watch::Sender::new(TxPhase::Queued),
            reply_received: AtomicBool::new(false),
        })
    }

    fn begin(&self) -> bool {
        self.phase.send_if_modified(|phase| {
            if *phase != TxPhase::Queued {
                return false;
            }
            *phase = TxPhase::Writing;
            true
        })
    }

    fn handed(&self) -> bool {
        self.phase.send_if_modified(|phase| {
            if *phase != TxPhase::Writing {
                return false;
            }
            *phase = TxPhase::Handed;
            true
        })
    }

    fn cancel(&self) -> TxPhase {
        self.phase.send_replace(TxPhase::Cancelled)
    }

    async fn cancelled(&self) {
        let mut rx = self.phase.subscribe();
        let _ = rx.wait_for(|phase| *phase == TxPhase::Cancelled).await;
    }
}

#[derive(Debug)]
struct PendingAck {
    id: u64,
    control: Arc<TxControl>,
    reply: oneshot::Sender<AckOutcome>,
}

#[derive(Debug)]
struct PendingActivation {
    id: u64,
    deadline: Instant,
    control: Arc<TxControl>,
    reply: oneshot::Sender<(u16, ActivationCode)>,
}

#[derive(Debug)]
struct RxReservation {
    total: Arc<Mutex<usize>>,
    target: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for RxReservation {
    fn drop(&mut self) {
        let mut total = self.total.lock().unwrap_or_else(|p| p.into_inner());
        *total -= self.bytes;
        self.target.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct QueuedIncoming {
    incoming: Incoming,
    _reservation: RxReservation,
}

#[derive(Debug)]
struct TargetSlot {
    rx: mpsc::Sender<QueuedIncoming>,
    rx_bytes: Arc<AtomicUsize>,
    ack: Option<PendingAck>,
    dropped: u64,
}

#[derive(Debug, Default)]
struct State {
    targets: HashMap<u16, TargetSlot>,
    activation: Option<PendingActivation>,
    /// At most MAX_POISONED_LAS; exhaustion closes instead of evicting.
    poisoned: HashSet<u16>,
    next_attempt_id: u64,
    activated: bool,
    entity_la: Option<u16>,
}

#[derive(Debug)]
struct Shared {
    tester_la: u16,
    version: ProtocolVersion,
    activation_type: ActivationType,
    oem: Option<[u8; 4]>,
    activation_timeout: Duration,
    ack_timeout: Duration,
    generation: u64,
    state: Mutex<State>,
    data_tx: mpsc::Sender<TxItem>,
    ctrl_tx: mpsc::Sender<Vec<u8>>,
    link_tx: mpsc::Sender<LinkEvent>,
    link_state: watch::Sender<LinkState>,
    health: watch::Sender<ClientHealth>,
    rx_bytes: Arc<Mutex<usize>>,
    rx_dropped: AtomicU64,
    /// One fatal header NACK (9 bytes), drained on idle writer close; never appended after a
    /// partial failed write. Replaces a slot, never accumulates a control backlog.
    fatal_nack: Mutex<Option<Vec<u8>>>,
    closed: AtomicBool,
    dropped_link_events: AtomicU64,
    unrouted: AtomicU64,
    alive_answered: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
enum WaitKind {
    Activation,
    Ack(u16),
}

/// Owned across every await, then transferred into AckWait. The ID prevents old cleanup from
/// removing a new attempt. RA has no wire request ID: cancellation after start closes its socket.
#[derive(Debug)]
struct RequestGuard {
    shared: Arc<Shared>,
    control: Arc<TxControl>,
    id: u64,
    kind: WaitKind,
    armed: bool,
    force_poison: bool,
}

impl RequestGuard {
    fn finish(&mut self) {
        self.armed = false;
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let phase = self.control.cancel();
        let started = matches!(phase, TxPhase::Writing | TxPhase::Handed);
        let mut close = phase == TxPhase::Writing;
        {
            let mut state = self.shared.lock();
            match self.kind {
                WaitKind::Activation => {
                    if state.activation.as_ref().is_some_and(|a| a.id == self.id) {
                        state.activation = None;
                    }
                    close |= started;
                }
                WaitKind::Ack(la) => {
                    if let Some(slot) = state.targets.get_mut(&la)
                        && slot.ack.as_ref().is_some_and(|a| a.id == self.id)
                    {
                        slot.ack = None;
                    }
                    if self.force_poison || (started && !self.control.reply_received.load(Ordering::Acquire))
                    {
                        close |= !poison(&mut state, la);
                    }
                }
            }
        }
        if close {
            self.shared.close(LinkEvent::Closed {
                reason: "cancelled/uncertain in-progress frame or routing activation".into(),
            });
        }
    }
}

fn poison(state: &mut State, la: u16) -> bool {
    if state.poisoned.contains(&la) {
        return true;
    }
    if state.poisoned.len() >= MAX_POISONED_LAS {
        return false;
    }
    state.poisoned.insert(la);
    true
}

fn next_id(state: &mut State) -> Result<u64, DoipError> {
    state.next_attempt_id = state.next_attempt_id.checked_add(1).ok_or(DoipError::Closed)?;
    Ok(state.next_attempt_id)
}

impl Shared {
    fn new(
        cfg: &ClientConfig,
        data_tx: mpsc::Sender<TxItem>,
        ctrl_tx: mpsc::Sender<Vec<u8>>,
        link_tx: mpsc::Sender<LinkEvent>,
        state_tx: watch::Sender<LinkState>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tester_la: cfg.tester_la,
            version: cfg.version,
            activation_type: cfg.activation_type,
            oem: cfg.oem,
            activation_timeout: cfg.activation_timeout,
            ack_timeout: cfg.ack_timeout,
            generation: cfg.generation,
            state: Mutex::new(State::default()),
            data_tx,
            ctrl_tx,
            link_tx,
            link_state: state_tx,
            health: watch::Sender::new(ClientHealth::Healthy),
            rx_bytes: Arc::new(Mutex::new(0)),
            rx_dropped: AtomicU64::new(0),
            fatal_nack: Mutex::new(None),
            closed: AtomicBool::new(false),
            dropped_link_events: AtomicU64::new(0),
            unrouted: AtomicU64::new(0),
            alive_answered: AtomicU64::new(0),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, ev: LinkEvent) {
        if self.link_tx.try_send(ev).is_err() {
            self.dropped_link_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn send_ctrl(&self, msg: &Message) {
        match msg.encode(self.version) {
            Ok(frame) => {
                if self.ctrl_tx.try_send(frame).is_err() {
                    tracing::warn!("doip control queue full; frame dropped");
                }
            }
            Err(e) => tracing::warn!("doip control frame not encodable: {e}"),
        }
    }

    /// Close once: state → Closed, waiters and target queues released, event emitted.
    fn close(&self, ev: LinkEvent) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        {
            let mut st = self.lock();
            st.targets.clear();
            st.activation = None;
            st.activated = false;
        }
        self.link_state.send_replace(LinkState::Closed);
        self.emit(ev);
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    async fn closed(&self) {
        let mut state = self.link_state.subscribe();
        let _ = state.wait_for(|s| *s == LinkState::Closed).await;
    }

    fn reserve_rx(&self, slot: &TargetSlot, bytes: usize) -> Option<RxReservation> {
        let mut total = self.rx_bytes.lock().unwrap_or_else(|p| p.into_inner());
        let target_bytes = slot.rx_bytes.load(Ordering::Relaxed);
        if bytes > MAX_RX_BYTES_PER_TARGET.saturating_sub(target_bytes)
            || bytes > MAX_RX_BYTES_PER_CONNECTION.saturating_sub(*total)
        {
            return None;
        }
        *total += bytes;
        slot.rx_bytes.fetch_add(bytes, Ordering::Relaxed);
        Some(RxReservation {
            total: Arc::clone(&self.rx_bytes),
            target: Arc::clone(&slot.rx_bytes),
            bytes,
        })
    }

    fn dispatch(&self, msg: Message) {
        let now = Instant::now();
        match msg {
            Message::AliveCheckRequest => {
                self.send_ctrl(&Message::AliveCheckResponse {
                    source: self.tester_la,
                });
                self.alive_answered.fetch_add(1, Ordering::Relaxed);
            }
            Message::RoutingActivationResponse {
                tester, entity, code, ..
            } => {
                if tester != self.tester_la {
                    self.close(LinkEvent::Error {
                        reason: "routing activation tester address mismatch".into(),
                    });
                    return;
                }
                let waiter = self.lock().activation.take();
                match waiter {
                    Some(w) => {
                        if now > w.deadline {
                            self.close(LinkEvent::Error {
                                reason: "routing activation response after deadline".into(),
                            });
                            return;
                        }
                        if w.control.phase.borrow().eq(&TxPhase::Cancelled) {
                            self.close(LinkEvent::Error {
                                reason: "late cancelled routing activation".into(),
                            });
                            return;
                        }
                        if w.reply.send((entity, code)).is_err() {
                            tracing::debug!("activation waiter gone");
                        }
                    }
                    None => tracing::debug!("unsolicited routing activation response 0x{:02X}", code.0),
                }
            }
            Message::DiagnosticAck { source, target, .. }
            | Message::DiagnosticNack { source, target, .. }
                if target != self.tester_la =>
            {
                tracing::debug!("ACK/NACK from 0x{source:04X} for another tester 0x{target:04X}");
            }
            Message::DiagnosticAck { source, .. } => self.resolve_ack(source, AckOutcome::Ack(now)),
            Message::DiagnosticNack { source, code, .. } => {
                self.resolve_ack(source, AckOutcome::Nack(code, now))
            }
            Message::DiagnosticMessage { source, target, data } => {
                if target != self.tester_la {
                    self.unrouted.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                let mut overflow = None;
                let routed = {
                    let mut st = self.lock();
                    match st.targets.get_mut(&source) {
                        Some(slot) => {
                            let reservation = self.reserve_rx(slot, data.len());
                            let delivered = match reservation {
                                Some(reservation) => slot
                                    .rx
                                    .try_send(QueuedIncoming {
                                        incoming: Incoming { data, at: now },
                                        _reservation: reservation,
                                    })
                                    .is_ok(),
                                None => false,
                            };
                            if !delivered {
                                slot.dropped += 1;
                                overflow = Some(slot.dropped);
                            }
                            true
                        }
                        None => false,
                    }
                };
                if let Some(dropped_total) = overflow {
                    self.rx_dropped.fetch_add(1, Ordering::Relaxed);
                    self.health.send_replace(ClientHealth::RxOverflowed);
                    self.emit(LinkEvent::RxOverflow {
                        target_la: source,
                        dropped_total,
                    });
                }
                if !routed {
                    self.unrouted.fetch_add(1, Ordering::Relaxed);
                    self.emit(LinkEvent::Unrouted { source_la: source });
                }
            }
            Message::GenericNack { code } => self.emit(LinkEvent::GenericNack { code }),
            other => tracing::debug!("doip client ignores payload type 0x{:04X}", other.payload_type()),
        }
    }

    fn resolve_ack(&self, source: u16, outcome: AckOutcome) {
        let waiter = {
            let mut state = self.lock();
            if state.poisoned.contains(&source) {
                return;
            }
            state.targets.get_mut(&source).and_then(|s| s.ack.take())
        };
        match waiter {
            Some(w) => {
                w.control.reply_received.store(true, Ordering::Release);
                if w.reply.send(outcome).is_err() {
                    tracing::debug!("ACK waiter for 0x{source:04X} gone");
                }
            }
            None => tracing::debug!("unexpected diagnostic ACK/NACK from 0x{source:04X}"),
        }
    }

    async fn activate(self: &Arc<Self>) -> Result<ActivationCode, DoipError> {
        if self.is_closed() {
            return Err(DoipError::Closed);
        }
        let (tx, rx) = oneshot::channel();
        let now = Instant::now();
        let deadline = now
            .checked_add(self.activation_timeout)
            .unwrap_or(now + MAX_FRAME_WRITE_TIME);
        let control = TxControl::new();
        let id = {
            let mut st = self.lock();
            if self.is_closed() {
                return Err(DoipError::Closed);
            }
            if st.activation.is_some() {
                return Err(DoipError::ActivationBusy);
            }
            let id = next_id(&mut st)?;
            st.activation = Some(PendingActivation {
                id,
                deadline,
                control: Arc::clone(&control),
                reply: tx,
            });
            id
        };
        let mut guard = RequestGuard {
            shared: Arc::clone(self),
            control: Arc::clone(&control),
            id,
            kind: WaitKind::Activation,
            armed: true,
            force_poison: false,
        };
        let frame = Message::RoutingActivationRequest {
            source: self.tester_la,
            activation_type: self.activation_type,
            oem: self.oem,
        }
        .encode(self.version)?;
        if self
            .data_tx
            .try_send(TxItem {
                frame,
                deadline,
                control,
                handed: None,
            })
            .is_err()
        {
            return Err(DoipError::TxFull);
        }
        let (entity_la, code) = match timeout_at(deadline, rx).await {
            Err(_) => {
                return Err(DoipError::ActivationTimeout);
            }
            Ok(Err(_)) => return Err(DoipError::Closed),
            Ok(Ok(v)) => v,
        };
        guard.finish();
        if !code.is_success() {
            self.emit(LinkEvent::ActivationDenied { code });
            return Err(DoipError::ActivationDenied(code));
        }
        let was_active = {
            let mut st = self.lock();
            if self.is_closed() {
                return Err(DoipError::Closed);
            }
            st.entity_la = Some(entity_la);
            // Serialize publication with close(): never overwrite Closed after its socket
            // was already discarded. close() takes this same lock before publishing Closed.
            self.link_state.send_replace(LinkState::Active);
            std::mem::replace(&mut st.activated, true)
        };
        self.emit(if was_active {
            LinkEvent::Reactivated { entity_la, code }
        } else {
            LinkEvent::Activated { entity_la, code }
        });
        Ok(code)
    }
}

async fn reader_task(mut rd: OwnedReadHalf, shared: Arc<Shared>) {
    loop {
        let message = tokio::select! {
            biased;
            _ = shared.closed() => break,
            message = read_message(&mut rd) => message,
        };
        match message {
            Ok(msg) => shared.dispatch(msg),
            Err(ReadError::Discarded(e)) => shared.send_ctrl(&Message::GenericNack { code: e.nack_code() }),
            Err(ReadError::Fatal(e)) => {
                if let Ok(frame) = (Message::GenericNack { code: e.nack_code() }).encode(shared.version) {
                    *shared.fatal_nack.lock().unwrap_or_else(|p| p.into_inner()) = Some(frame);
                }
                shared.close(LinkEvent::Error {
                    reason: e.to_string(),
                });
                break;
            }
            Err(ReadError::Closed(e)) => {
                shared.close(LinkEvent::Closed {
                    reason: e.to_string(),
                });
                break;
            }
        }
    }
}

async fn writer_task<W: tokio::io::AsyncWrite + Unpin>(
    mut wr: W,
    mut ctrl: mpsc::Receiver<Vec<u8>>,
    mut data: mpsc::Receiver<TxItem>,
    mut state: watch::Receiver<LinkState>,
    shared: Arc<Shared>,
) {
    loop {
        let (frame, handed, control, deadline) = tokio::select! {
            biased;
            _ = async { let _ = state.wait_for(|s| *s == LinkState::Closed).await; } => {
                let frame = shared.fatal_nack.lock().unwrap_or_else(|p| p.into_inner()).take();
                if let Some(frame) = frame {
                    let _ = write_bounded(&mut wr, &frame, Instant::now() + MAX_FRAME_WRITE_TIME,
                        std::future::pending()).await;
                }
                break;
            },
            Some(f) = ctrl.recv() => (f, None, None, Instant::now() + MAX_FRAME_WRITE_TIME),
            Some(item) = data.recv() => (item.frame, item.handed, Some(item.control), item.deadline),
            else => break,
        };
        if Instant::now() >= deadline {
            if let Some(h) = handed {
                let _ = h.send(Err(DoipError::TxTimeout));
            }
            continue;
        }
        if let Some(control) = &control
            && !control.begin()
        {
            continue;
        }
        let cancel_or_close = async {
            match &control {
                Some(control) => tokio::select! {
                    _ = shared.closed() => {},
                    _ = control.cancelled() => {},
                },
                None => shared.closed().await,
            }
        };
        let until = deadline.min(Instant::now() + MAX_FRAME_WRITE_TIME);
        if let Err(e) = write_bounded(&mut wr, &frame, until, cancel_or_close).await {
            if let Some(h) = handed {
                let _ = h.send(Err(DoipError::TxTimeout));
            }
            shared.close(LinkEvent::Closed {
                reason: format!("write: {e}"),
            });
            break;
        }
        if let Some(control) = &control
            && !control.handed()
        {
            shared.close(LinkEvent::Closed {
                reason: "write cancelled at handoff".into(),
            });
            break;
        }
        if let Some(h) = handed
            && h.send(Ok(Instant::now())).is_err()
        {
            tracing::debug!("submit caller gone before handoff");
        }
    }
    // Drop both socket halves on close. Never wait unboundedly on a graceful shutdown.
}

/// A connected, routing-activated DoIP connection.
#[derive(Debug)]
pub struct DoipClient {
    shared: Arc<Shared>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

impl DoipClient {
    /// Connect (bounded by `connect_timeout`), then routing-activate (bounded by
    /// `activation_timeout`). Returns the client and its link event receiver.
    pub async fn connect(cfg: ClientConfig) -> Result<(DoipClient, mpsc::Receiver<LinkEvent>), DoipError> {
        let stream = timeout(cfg.connect_timeout, TcpStream::connect(cfg.entity))
            .await
            .map_err(|_| DoipError::ConnectTimeout(cfg.entity))??;
        stream.set_nodelay(true)?;
        let (rd, wr) = stream.into_split();
        let (data_tx, data_rx) = mpsc::channel(CAP_TX_FRAMES);
        let (ctrl_tx, ctrl_rx) = mpsc::channel(CAP_CONTROL_FRAMES);
        let (link_tx, link_rx) = mpsc::channel(CAP_LINK_EVENTS);
        let (state_tx, state_rx) = watch::channel(LinkState::Activating);
        let shared = Shared::new(&cfg, data_tx, ctrl_tx, link_tx, state_tx);
        let reader = tokio::spawn(reader_task(rd, Arc::clone(&shared)));
        let writer = tokio::spawn(writer_task(wr, ctrl_rx, data_rx, state_rx, Arc::clone(&shared)));
        let client = DoipClient {
            shared,
            reader,
            writer,
        };
        client.shared.activate().await?;
        Ok((client, link_rx))
    }

    /// Send routing activation again on this socket (emits [`LinkEvent::Reactivated`]).
    pub async fn reactivate(&self) -> Result<ActivationCode, DoipError> {
        self.shared.activate().await
    }

    /// Register a target logical address on this connection.
    pub fn open_target(&self, target_la: u16) -> Result<DoipTarget, DoipError> {
        if self.shared.is_closed() {
            return Err(DoipError::Closed);
        }
        let (tx, rx) = mpsc::channel(CAP_RX_PER_TARGET);
        let rx_bytes = Arc::new(AtomicUsize::new(0));
        let mut st = self.shared.lock();
        if st.poisoned.contains(&target_la) {
            return Err(DoipError::AckPending(target_la));
        }
        if self.shared.is_closed() {
            return Err(DoipError::Closed);
        }
        if st.targets.contains_key(&target_la) {
            return Err(DoipError::TargetInUse(target_la));
        }
        if st.targets.len() >= MAX_TARGETS_PER_CONNECTION {
            return Err(DoipError::TooManyTargets);
        }
        st.targets.insert(
            target_la,
            TargetSlot {
                rx: tx,
                rx_bytes: Arc::clone(&rx_bytes),
                ack: None,
                dropped: 0,
            },
        );
        Ok(DoipTarget {
            la: target_la,
            shared: Arc::clone(&self.shared),
            rx,
            rx_bytes,
        })
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation
    }

    pub fn entity_la(&self) -> Option<u16> {
        self.shared.lock().entity_la
    }

    /// Current link state (always up to date, independent of the event channel).
    pub fn link_state(&self) -> watch::Receiver<LinkState> {
        self.shared.link_state.subscribe()
    }

    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }

    pub fn stats(&self) -> ClientStats {
        ClientStats {
            dropped_link_events: self.shared.dropped_link_events.load(Ordering::Relaxed),
            unrouted: self.shared.unrouted.load(Ordering::Relaxed),
            alive_checks_answered: self.shared.alive_answered.load(Ordering::Relaxed),
            rx_buffered_bytes: *self.shared.rx_bytes.lock().unwrap_or_else(|p| p.into_inner()),
            rx_dropped_total: self.shared.rx_dropped.load(Ordering::Relaxed),
        }
    }

    /// Sticky health survives link-event overflow; reconnect starts a healthy connection.
    pub fn health(&self) -> watch::Receiver<ClientHealth> {
        self.shared.health.subscribe()
    }

    /// Close the connection. Pending requests see [`DoipError::Closed`].
    pub fn close(&self) {
        self.shared.close(LinkEvent::Closed {
            reason: "closed by owner".into(),
        });
    }
}

impl Drop for DoipClient {
    fn drop(&mut self) {
        self.close();
        self.reader.abort();
        self.writer.abort();
    }
}

/// Pending diagnostic ACK for one submitted message.
#[derive(Debug)]
pub struct AckWait {
    handed_at: Instant,
    rx: Option<oneshot::Receiver<AckOutcome>>,
    guard: RequestGuard,
}

impl AckWait {
    /// When the writer handed the frame to the socket (write_all returned).
    pub fn handed_at(&self) -> Instant {
        self.handed_at
    }

    /// Wait for the ACK until `min(until, handed_at + ack_timeout)`. Returns the ACK time.
    /// Timeout/drop with an uncertain ACK poisons this LA until a new connection is activated.
    pub async fn wait(mut self, until: Instant) -> Result<Instant, DoipError> {
        let ack_deadline = self
            .handed_at
            .checked_add(self.guard.shared.ack_timeout)
            .unwrap_or(until);
        let limit = until.min(ack_deadline);
        let Some(rx) = self.rx.take() else {
            return Err(DoipError::Closed);
        };
        match timeout_at(limit, rx).await {
            Err(_) => {
                self.guard.force_poison = true;
                Err(DoipError::AckTimeout)
            }
            Ok(Err(_)) => Err(DoipError::Closed),
            Ok(Ok(AckOutcome::Ack(at))) if at <= limit => {
                self.guard.finish();
                Ok(at)
            }
            Ok(Ok(AckOutcome::Ack(_))) => {
                self.guard.force_poison = true;
                Err(DoipError::AckTimeout)
            }
            Ok(Ok(AckOutcome::Nack(code, at))) if at <= limit => {
                self.guard.finish();
                Err(DoipError::Nack(code))
            }
            Ok(Ok(AckOutcome::Nack(_, _))) => {
                self.guard.force_poison = true;
                Err(DoipError::AckTimeout)
            }
        }
    }
}

/// Response receive error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RecvError {
    #[error("no response before the deadline")]
    Timeout,
    #[error("connection closed")]
    Closed,
}

/// One target logical address on a shared connection. Dropping it unregisters the address.
#[derive(Debug)]
pub struct DoipTarget {
    la: u16,
    shared: Arc<Shared>,
    rx: mpsc::Receiver<QueuedIncoming>,
    rx_bytes: Arc<AtomicUsize>,
}

impl DoipTarget {
    /// PDU bytes currently queued, released on receive/drop even after link close.
    pub fn buffered_bytes(&self) -> usize {
        self.rx_bytes.load(Ordering::Relaxed)
    }
    pub fn target_la(&self) -> u16 {
        self.la
    }

    pub fn tester_la(&self) -> u16 {
        self.shared.tester_la
    }

    pub fn generation(&self) -> u64 {
        self.shared.generation
    }

    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }

    /// Queue a diagnostic message and wait until the writer has handed it to the socket (bounded by
    /// `until`). The ACK is awaited separately with [`AckWait::wait`].
    /// Drop cancels queued data; cancelling a partial write discards the socket. Once handed,
    /// an uncertain ACK poisons the LA across target drop/reopen until a fresh connection.
    pub async fn submit(&self, pdu: &[u8], until: Instant) -> Result<AckWait, DoipError> {
        if pdu.is_empty() || pdu.len() > MAX_UDS_PDU {
            return Err(DoipError::InvalidPdu(pdu.len()));
        }
        if self.shared.is_closed() {
            return Err(DoipError::Closed);
        }
        if Instant::now() >= until {
            return Err(DoipError::TxTimeout);
        }
        let frame = Message::DiagnosticMessage {
            source: self.shared.tester_la,
            target: self.la,
            data: pdu.to_vec(),
        }
        .encode(self.shared.version)?;
        let (ack_tx, ack_rx) = oneshot::channel();
        let control = TxControl::new();
        let id = {
            let mut st = self.shared.lock();
            if st.poisoned.contains(&self.la) {
                return Err(DoipError::AckPending(self.la));
            }
            let id = next_id(&mut st)?;
            let Some(slot) = st.targets.get_mut(&self.la) else {
                return Err(DoipError::Closed);
            };
            if slot.ack.is_some() {
                return Err(DoipError::AckPending(self.la));
            }
            slot.ack = Some(PendingAck {
                id,
                control: Arc::clone(&control),
                reply: ack_tx,
            });
            id
        };
        let guard = RequestGuard {
            shared: Arc::clone(&self.shared),
            control: Arc::clone(&control),
            id,
            kind: WaitKind::Ack(self.la),
            armed: true,
            force_poison: false,
        };
        let (h_tx, h_rx) = oneshot::channel();
        if let Err(e) = self.shared.data_tx.try_send(TxItem {
            frame,
            deadline: until,
            control,
            handed: Some(h_tx),
        }) {
            return Err(match e {
                mpsc::error::TrySendError::Full(_) => DoipError::TxFull,
                mpsc::error::TrySendError::Closed(_) => DoipError::Closed,
            });
        }
        let handed_at = match timeout_at(until, h_rx).await {
            Err(_) => return Err(DoipError::TxTimeout),
            Ok(Err(_)) => return Err(DoipError::Closed),
            Ok(Ok(Ok(at))) if at <= until => at,
            Ok(Ok(Ok(_))) => return Err(DoipError::TxTimeout),
            Ok(Ok(Err(error))) => return Err(error),
        };
        Ok(AckWait {
            handed_at,
            rx: Some(ack_rx),
            guard,
        })
    }

    /// Submit and wait for the ACK. Returns `(handed_at, ack_at)`.
    pub async fn send(&self, pdu: &[u8], until: Instant) -> Result<(Instant, Instant), DoipError> {
        let wait = self.submit(pdu, until).await?;
        let handed = wait.handed_at();
        let acked = wait.wait(until).await?;
        Ok((handed, acked))
    }

    /// Next response, or `None` once the connection closed and the queue is drained.
    pub async fn recv(&mut self) -> Option<Incoming> {
        self.rx.recv().await.map(|queued| queued.incoming)
    }

    /// Next response before `until`.
    pub async fn recv_until(&mut self, until: Instant) -> Result<Incoming, RecvError> {
        match timeout_at(until, self.rx.recv()).await {
            Err(_) => Err(RecvError::Timeout),
            Ok(None) => Err(RecvError::Closed),
            Ok(Some(x)) => Ok(x.incoming),
        }
    }

    /// A queued response, if any.
    pub fn try_recv(&mut self) -> Option<Incoming> {
        self.rx.try_recv().ok().map(|queued| queued.incoming)
    }
}

impl Drop for DoipTarget {
    fn drop(&mut self) {
        let close = {
            let mut state = self.shared.lock();
            let slot = state.targets.remove(&self.la);
            if let Some(ack) = slot.and_then(|slot| slot.ack) {
                let phase = ack.control.cancel();
                if matches!(phase, TxPhase::Writing | TxPhase::Handed)
                    && !ack.control.reply_received.load(Ordering::Acquire)
                {
                    phase == TxPhase::Writing || !poison(&mut state, self.la)
                } else {
                    false
                }
            } else {
                false
            }
        };
        if close {
            self.shared.close(LinkEvent::Closed {
                reason: "target dropped during ambiguous transmit".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    use tokio::io::{AsyncReadExt, DuplexStream};

    fn blocked_writer_client() -> (DoipClient, DuplexStream) {
        let cfg = ClientConfig::new(SocketAddr::from(([127, 0, 0, 1], 0)), 0x0e80);
        let (data_tx, data_rx) = mpsc::channel(CAP_TX_FRAMES);
        let (ctrl_tx, ctrl_rx) = mpsc::channel(CAP_CONTROL_FRAMES);
        let (link_tx, _link_rx) = mpsc::channel(CAP_LINK_EVENTS);
        let (state_tx, state_rx) = watch::channel(LinkState::Active);
        let shared = Shared::new(&cfg, data_tx, ctrl_tx, link_tx, state_tx);
        shared.lock().activated = true;
        let (socket, peer) = tokio::io::duplex(1);
        let writer = tokio::spawn(writer_task(
            socket,
            ctrl_rx,
            data_rx,
            state_rx,
            Arc::clone(&shared),
        ));
        // A bounded placeholder reader: this unit fixture drives ACKs/state explicitly.
        let reader = tokio::spawn(tokio::time::sleep(MAX_FRAME_WRITE_TIME * 4));
        (
            DoipClient {
                shared,
                reader,
                writer,
            },
            peer,
        )
    }

    async fn assert_prefix_and_eof(peer: &mut DuplexStream) {
        let mut bytes = [0; 16];
        assert_eq!(peer.read(&mut bytes).await.expect("prefix"), 1);
        assert_eq!(peer.read(&mut bytes).await.expect("discarded partial stream"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_partial_client_write_closes_connection_and_never_appends_next_frame() {
        let (mut client, mut peer) = blocked_writer_client();
        let target = client.open_target(0x1001).expect("target");
        let mut submit =
            Box::pin(target.submit(&[0x22, 0xf1, 0x90], Instant::now() + MAX_FRAME_WRITE_TIME * 2));
        assert!(poll_fn(|cx| Poll::Ready(submit.as_mut().poll(cx).is_pending())).await);
        tokio::task::yield_now().await;
        drop(submit);
        assert!(client.is_closed());
        timeout(Duration::from_millis(10), &mut client.writer)
            .await
            .expect("writer observes cancel")
            .expect("writer");
        assert!(matches!(
            target
                .submit(&[0x3e, 0], Instant::now() + MAX_FRAME_WRITE_TIME)
                .await,
            Err(DoipError::Closed)
        ));
        assert_prefix_and_eof(&mut peer).await;
    }

    #[tokio::test(start_paused = true)]
    async fn client_close_interrupts_in_progress_write() {
        let (mut client, mut peer) = blocked_writer_client();
        let target = client.open_target(0x1001).expect("target");
        let mut submit =
            Box::pin(target.submit(&[0x22, 0xf1, 0x90], Instant::now() + MAX_FRAME_WRITE_TIME * 2));
        assert!(poll_fn(|cx| Poll::Ready(submit.as_mut().poll(cx).is_pending())).await);
        tokio::task::yield_now().await;
        client.close();
        timeout(Duration::from_millis(10), &mut client.writer)
            .await
            .expect("writer observes close")
            .expect("writer");
        assert!(submit.await.is_err());
        assert_prefix_and_eof(&mut peer).await;
    }

    #[tokio::test(start_paused = true)]
    async fn client_stalled_write_has_independent_socket_timeout() {
        let (client, mut peer) = blocked_writer_client();
        let target = client.open_target(0x1001).expect("target");
        let start = Instant::now();
        let result = target
            .submit(&[0x22, 0xf1, 0x90], start + MAX_FRAME_WRITE_TIME * 2)
            .await;
        assert!(matches!(result, Err(DoipError::TxTimeout)));
        assert_eq!(start.elapsed(), MAX_FRAME_WRITE_TIME);
        assert!(client.is_closed());
        assert_prefix_and_eof(&mut peer).await;
    }
}
