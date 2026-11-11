//! Bounded tokio DoIP client (tester side).
//!
//! One TCP connection per entity + tester SA, shared by many target logical addresses.
//! Tasks: one reader (answers alive checks immediately, routes ACK/NACK and responses by source
//! LA, never blocks on consumers) and one writer (control frames first). Every queue is bounded;
//! see the `CAP_*`/`MAX_*` constants for the overflow policy.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};

use crate::frame::{ActivationCode, ActivationType, EncodeError, MAX_UDS_PDU, Message, ProtocolVersion};
use crate::io::{ReadError, read_message};

/// Most target logical addresses one connection serves. Beyond: [`DoipError::TooManyTargets`].
pub const MAX_TARGETS_PER_CONNECTION: usize = 64;
/// Responses buffered per target before the consumer reads them. Full: the response is dropped,
/// counted, and [`LinkEvent::RxOverflow`] is emitted (the reader never waits on a consumer).
pub const CAP_RX_PER_TARGET: usize = 32;
/// Diagnostic/activation frames waiting for the writer. Full: [`DoipError::TxFull`] (new request
/// refused before it is sent).
pub const CAP_TX_FRAMES: usize = 32;
/// Control frames (alive check response, generic NACK) waiting for the writer; served before data.
/// Full: the frame is dropped with a warning (only possible if the socket is stuck).
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
}

#[derive(Debug)]
enum AckOutcome {
    Ack(Instant),
    Nack(u8),
}

#[derive(Debug)]
struct TxItem {
    frame: Vec<u8>,
    handed: Option<oneshot::Sender<Instant>>,
}

#[derive(Debug)]
struct TargetSlot {
    rx: mpsc::Sender<Incoming>,
    ack: Option<oneshot::Sender<AckOutcome>>,
    dropped: u64,
}

#[derive(Debug, Default)]
struct State {
    targets: HashMap<u16, TargetSlot>,
    activation: Option<oneshot::Sender<(u16, ActivationCode)>>,
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
    closed: AtomicBool,
    dropped_link_events: AtomicU64,
    unrouted: AtomicU64,
    alive_answered: AtomicU64,
}

impl Shared {
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

    fn dispatch(&self, msg: Message) {
        let now = Instant::now();
        match msg {
            Message::AliveCheckRequest => {
                self.send_ctrl(&Message::AliveCheckResponse {
                    source: self.tester_la,
                });
                self.alive_answered.fetch_add(1, Ordering::Relaxed);
            }
            Message::RoutingActivationResponse { entity, code, .. } => {
                let waiter = self.lock().activation.take();
                match waiter {
                    Some(w) => {
                        if w.send((entity, code)).is_err() {
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
            Message::DiagnosticNack { source, code, .. } => self.resolve_ack(source, AckOutcome::Nack(code)),
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
                            if let Err(mpsc::error::TrySendError::Full(_)) =
                                slot.rx.try_send(Incoming { data, at: now })
                            {
                                slot.dropped += 1;
                                overflow = Some(slot.dropped);
                            }
                            true
                        }
                        None => false,
                    }
                };
                if let Some(dropped_total) = overflow {
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
        let waiter = self.lock().targets.get_mut(&source).and_then(|s| s.ack.take());
        match waiter {
            Some(w) => {
                if w.send(outcome).is_err() {
                    tracing::debug!("ACK waiter for 0x{source:04X} gone");
                }
            }
            None => tracing::debug!("unexpected diagnostic ACK/NACK from 0x{source:04X}"),
        }
    }

    async fn activate(&self) -> Result<ActivationCode, DoipError> {
        if self.is_closed() {
            return Err(DoipError::Closed);
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut st = self.lock();
            if st.activation.is_some() {
                return Err(DoipError::ActivationBusy);
            }
            st.activation = Some(tx);
        }
        let frame = Message::RoutingActivationRequest {
            source: self.tester_la,
            activation_type: self.activation_type,
            oem: self.oem,
        }
        .encode(self.version)?;
        if self.data_tx.try_send(TxItem { frame, handed: None }).is_err() {
            self.lock().activation = None;
            return Err(DoipError::TxFull);
        }
        let (entity_la, code) = match timeout(self.activation_timeout, rx).await {
            Err(_) => {
                self.lock().activation = None;
                return Err(DoipError::ActivationTimeout);
            }
            Ok(Err(_)) => return Err(DoipError::Closed),
            Ok(Ok(v)) => v,
        };
        if !code.is_success() {
            self.emit(LinkEvent::ActivationDenied { code });
            return Err(DoipError::ActivationDenied(code));
        }
        let was_active = {
            let mut st = self.lock();
            st.entity_la = Some(entity_la);
            std::mem::replace(&mut st.activated, true)
        };
        self.link_state.send_replace(LinkState::Active);
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
        match read_message(&mut rd).await {
            Ok(msg) => shared.dispatch(msg),
            Err(ReadError::Discarded(e)) => shared.send_ctrl(&Message::GenericNack { code: e.nack_code() }),
            Err(ReadError::Fatal(e)) => {
                shared.send_ctrl(&Message::GenericNack { code: e.nack_code() });
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

async fn writer_task(
    mut wr: OwnedWriteHalf,
    mut ctrl: mpsc::Receiver<Vec<u8>>,
    mut data: mpsc::Receiver<TxItem>,
    mut state: watch::Receiver<LinkState>,
    shared: Arc<Shared>,
) {
    loop {
        let (frame, handed) = tokio::select! {
            biased;
            Some(f) = ctrl.recv() => (f, None),
            _ = state.wait_for(|s| *s == LinkState::Closed) => break,
            Some(item) = data.recv() => (item.frame, item.handed),
            else => break,
        };
        if let Err(e) = wr.write_all(&frame).await {
            shared.close(LinkEvent::Closed {
                reason: format!("write: {e}"),
            });
            break;
        }
        if let Some(h) = handed
            && h.send(Instant::now()).is_err()
        {
            tracing::debug!("submit caller gone before handoff");
        }
    }
    if let Err(e) = wr.shutdown().await {
        tracing::debug!("doip shutdown: {e}");
    }
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
        let shared = Arc::new(Shared {
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
            closed: AtomicBool::new(false),
            dropped_link_events: AtomicU64::new(0),
            unrouted: AtomicU64::new(0),
            alive_answered: AtomicU64::new(0),
        });
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
        let mut st = self.shared.lock();
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
                ack: None,
                dropped: 0,
            },
        );
        Ok(DoipTarget {
            la: target_la,
            shared: Arc::clone(&self.shared),
            rx,
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
        }
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
    la: u16,
    handed_at: Instant,
    rx: oneshot::Receiver<AckOutcome>,
    shared: Arc<Shared>,
}

impl AckWait {
    /// When the writer handed the frame to the socket (write_all returned).
    pub fn handed_at(&self) -> Instant {
        self.handed_at
    }

    /// Wait for the ACK until `min(until, handed_at + ack_timeout)`. Returns the ACK time.
    pub async fn wait(self, until: Instant) -> Result<Instant, DoipError> {
        let limit = until.min(self.handed_at + self.shared.ack_timeout);
        match timeout_at(limit, self.rx).await {
            Err(_) => {
                if let Some(slot) = self.shared.lock().targets.get_mut(&self.la) {
                    slot.ack = None;
                }
                Err(DoipError::AckTimeout)
            }
            Ok(Err(_)) => Err(DoipError::Closed),
            Ok(Ok(AckOutcome::Ack(at))) => Ok(at),
            Ok(Ok(AckOutcome::Nack(code))) => Err(DoipError::Nack(code)),
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
    rx: mpsc::Receiver<Incoming>,
}

impl DoipTarget {
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
    pub async fn submit(&self, pdu: &[u8], until: Instant) -> Result<AckWait, DoipError> {
        if pdu.is_empty() || pdu.len() > MAX_UDS_PDU {
            return Err(DoipError::InvalidPdu(pdu.len()));
        }
        if self.shared.is_closed() {
            return Err(DoipError::Closed);
        }
        let frame = Message::DiagnosticMessage {
            source: self.shared.tester_la,
            target: self.la,
            data: pdu.to_vec(),
        }
        .encode(self.shared.version)?;
        let (ack_tx, ack_rx) = oneshot::channel();
        {
            let mut st = self.shared.lock();
            let Some(slot) = st.targets.get_mut(&self.la) else {
                return Err(DoipError::Closed);
            };
            if slot.ack.as_ref().is_some_and(|s| !s.is_closed()) {
                return Err(DoipError::AckPending(self.la));
            }
            slot.ack = Some(ack_tx);
        }
        let (h_tx, h_rx) = oneshot::channel();
        if let Err(e) = self.shared.data_tx.try_send(TxItem {
            frame,
            handed: Some(h_tx),
        }) {
            if let Some(slot) = self.shared.lock().targets.get_mut(&self.la) {
                slot.ack = None;
            }
            return Err(match e {
                mpsc::error::TrySendError::Full(_) => DoipError::TxFull,
                mpsc::error::TrySendError::Closed(_) => DoipError::Closed,
            });
        }
        let handed_at = match timeout_at(until, h_rx).await {
            Err(_) => return Err(DoipError::TxTimeout),
            Ok(Err(_)) => return Err(DoipError::Closed),
            Ok(Ok(at)) => at,
        };
        Ok(AckWait {
            la: self.la,
            handed_at,
            rx: ack_rx,
            shared: Arc::clone(&self.shared),
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
        self.rx.recv().await
    }

    /// Next response before `until`.
    pub async fn recv_until(&mut self, until: Instant) -> Result<Incoming, RecvError> {
        match timeout_at(until, self.rx.recv()).await {
            Err(_) => Err(RecvError::Timeout),
            Ok(None) => Err(RecvError::Closed),
            Ok(Some(x)) => Ok(x),
        }
    }

    /// A queued response, if any.
    pub fn try_recv(&mut self) -> Option<Incoming> {
        self.rx.try_recv().ok()
    }
}

impl Drop for DoipTarget {
    fn drop(&mut self) {
        self.shared.lock().targets.remove(&self.la);
    }
}
