//! DoIP entity (server side) for tests and `connect --sim`.
//!
//! TCP: accept (bounded), routing activation, alive check, diagnostic message → ACK → handler →
//! replies scheduled at `arrival + delay`. UDP: vehicle identification responses. Fault
//! injection: diagnostic NACK, missing ACK, socket close, activation response code.
//!
//! The handler interface matches `loom-uds-sim` (`handle(target_la, now_mono, req) -> [(delay,
//! bytes)]`) without depending on it; the `loom-uds` domain wires the two together.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use thiserror::Error;
use tokio::io::AsyncWrite;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use crate::frame::{
    ActivationCode, MAX_DOIP_FRAME, MAX_DOIP_PAYLOAD, MAX_UDS_PDU, Message, ProtocolVersion,
    VehicleAnnouncement, decode_frame, diag_code, header_nack,
};
use crate::io::{MAX_FRAME_WRITE_TIME, ReadError, read_message, write_bounded};

/// Most simultaneous TCP connections; further connections are accepted and closed at once.
pub const MAX_ENTITY_CONNECTIONS: usize = 32;
/// Most target logical addresses behind one entity (config validation).
pub const MAX_ENTITY_TARGETS: usize = 64;
/// Replies waiting for their delay per connection. Full: the reply is dropped and counted in
/// [`EntityStats::replies_dropped`].
pub const MAX_SCHEDULED_REPLIES_PER_CONN: usize = 256;
/// Most queued one-shot faults. Full: [`EntityError::TooManyFaults`].
pub const MAX_QUEUED_FAULTS: usize = 16;
/// Decoded frames between a connection's reader and its loop. Full: the reader waits (TCP
/// back-pressure on the tester), it never drops a frame.
pub const CAP_ENTITY_RX_FRAMES: usize = 16;
/// Longest reply delay honoured; longer delays are clamped.
pub const MAX_REPLY_DELAY: Duration = Duration::from_secs(600);
/// ISO 13400-2 `T_TCP_General_Inactivity` default (5 min). The entity applies it only when set with
/// [`DoipEntity::set_tcp_general_inactivity`] (sim default: off).
pub const DEFAULT_T_TCP_GENERAL_INACTIVITY: Duration = Duration::from_secs(300);

/// Request handler. Same shape as `loom-uds-sim`: given the target LA, the monotonic time since
/// the entity started and the UDS request, return `(delay, response bytes)` pairs (empty = no
/// response).
pub trait EntityHandler: Send + 'static {
    fn handle(&mut self, target_la: u16, now_mono: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)>;
}

impl<F> EntityHandler for F
where
    F: FnMut(u16, Duration, &[u8]) -> Vec<(Duration, Vec<u8>)> + Send + 'static,
{
    fn handle(&mut self, target_la: u16, now_mono: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)> {
        self(target_la, now_mono, req)
    }
}

/// Entity configuration.
#[derive(Debug, Clone)]
pub struct EntityConfig {
    pub tcp_bind: SocketAddr,
    /// UDP discovery socket; `None` disables discovery.
    pub udp_bind: Option<SocketAddr>,
    /// The entity's own logical address (also a valid diagnostic target).
    pub entity_la: u16,
    /// Further target logical addresses reachable through this entity (≤ [`MAX_ENTITY_TARGETS`]).
    pub target_las: Vec<u16>,
    pub vin: [u8; 17],
    pub eid: [u8; 6],
    pub gid: [u8; 6],
    /// Tester source addresses allowed to activate; empty = any.
    pub allowed_testers: Vec<u16>,
    /// ≤ [`MAX_ENTITY_CONNECTIONS`].
    pub max_connections: usize,
    pub version: ProtocolVersion,
}

impl EntityConfig {
    /// Loopback entity on ephemeral TCP and UDP ports.
    pub fn loopback(entity_la: u16, target_las: Vec<u16>) -> Self {
        EntityConfig {
            tcp_bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            udp_bind: Some(SocketAddr::from(([127, 0, 0, 1], 0))),
            entity_la,
            target_las,
            vin: *b"LOOMSIM0000000001",
            eid: [0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E],
            gid: [0; 6],
            allowed_testers: Vec::new(),
            max_connections: 16,
            version: ProtocolVersion::Iso2012,
        }
    }
}

/// One-shot faults applied to the next diagnostic message(s), in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityFault {
    /// Answer the next diagnostic message with diagnostic NACK `code` (handler not called).
    NackNextDiag(u8),
    /// Send no ACK for the next diagnostic message (handler not called).
    DropNextAck,
    /// Close the socket when the next diagnostic message arrives.
    CloseOnNextDiag,
}

/// Entity errors.
#[derive(Debug, Error)]
pub enum EntityError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid entity config: {0}")]
    Config(&'static str),
    #[error("too many queued faults (max {MAX_QUEUED_FAULTS})")]
    TooManyFaults,
}

/// Counters for test assertions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntityStats {
    pub connections_accepted: u64,
    pub connections_refused: u64,
    pub activations: u64,
    pub diag_received: u64,
    pub alive_check_responses: u64,
    pub replies_dropped: u64,
    pub header_nacks: u64,
    /// Connections closed by the `T_TCP_General_Inactivity` timer.
    pub inactivity_closes: u64,
}

#[derive(Debug, Default)]
struct Counters {
    accepted: AtomicU64,
    refused: AtomicU64,
    activations: AtomicU64,
    diag: AtomicU64,
    alive: AtomicU64,
    dropped: AtomicU64,
    nacks: AtomicU64,
    inactivity: AtomicU64,
}

struct EntityShared {
    cfg: EntityConfig,
    start: Instant,
    handler: Mutex<Box<dyn EntityHandler>>,
    faults: Mutex<VecDeque<EntityFault>>,
    activation_override: Mutex<Option<ActivationCode>>,
    registered: Mutex<HashMap<u64, u16>>,
    open: AtomicUsize,
    counters: Counters,
    kick: watch::Sender<u64>,
    alive: watch::Sender<u64>,
    shutdown: watch::Sender<bool>,
    /// `T_TCP_General_Inactivity` (`None` = off).
    general_inactivity: watch::Sender<Option<Duration>>,
}

impl std::fmt::Debug for EntityShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntityShared")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl EntityShared {
    fn serves(&self, la: u16) -> bool {
        la == self.cfg.entity_la || self.cfg.target_las.contains(&la)
    }

    fn announcement(&self) -> VehicleAnnouncement {
        VehicleAnnouncement {
            vin: self.cfg.vin,
            logical_address: self.cfg.entity_la,
            eid: self.cfg.eid,
            gid: self.cfg.gid,
            further_action: 0,
            sync_status: Some(0),
        }
    }

    fn activation_code(
        &self,
        conn_id: u64,
        current_sa: Option<u16>,
        msg_sa: u16,
        ty_supported: bool,
    ) -> ActivationCode {
        if let Some(code) = *lock(&self.activation_override) {
            return code;
        }
        if !ty_supported {
            return ActivationCode::DENIED_UNSUPPORTED_TYPE;
        }
        if !self.cfg.allowed_testers.is_empty() && !self.cfg.allowed_testers.contains(&msg_sa) {
            return ActivationCode::DENIED_UNKNOWN_SA;
        }
        if current_sa.is_some_and(|sa| sa != msg_sa) {
            return ActivationCode::DENIED_SA_MISMATCH_ON_SOCKET;
        }
        let taken = lock(&self.registered)
            .iter()
            .any(|(id, sa)| *id != conn_id && *sa == msg_sa);
        if taken {
            // Simplification: ISO would alive-check the other socket first (SWS_DoIP_00105).
            return ActivationCode::DENIED_SA_ACTIVE_ON_OTHER_SOCKET;
        }
        ActivationCode::SUCCESS
    }
}

/// A running DoIP entity. Dropping it stops the listener, UDP task and all connections.
#[derive(Debug)]
pub struct DoipEntity {
    shared: Arc<EntityShared>,
    tcp_addr: SocketAddr,
    udp: Option<Arc<UdpSocket>>,
    accept_task: JoinHandle<()>,
    udp_task: Option<JoinHandle<()>>,
}

impl DoipEntity {
    /// Bind and start serving.
    pub async fn start<H: EntityHandler>(cfg: EntityConfig, handler: H) -> Result<DoipEntity, EntityError> {
        if cfg.target_las.len() > MAX_ENTITY_TARGETS {
            return Err(EntityError::Config("too many target logical addresses"));
        }
        if cfg.max_connections == 0 || cfg.max_connections > MAX_ENTITY_CONNECTIONS {
            return Err(EntityError::Config("max_connections out of range"));
        }
        let listener = TcpListener::bind(cfg.tcp_bind).await?;
        let tcp_addr = listener.local_addr()?;
        let udp = match cfg.udp_bind {
            Some(a) => Some(Arc::new(UdpSocket::bind(a).await?)),
            None => None,
        };
        let shared = Arc::new(EntityShared {
            cfg,
            start: Instant::now(),
            handler: Mutex::new(Box::new(handler)),
            faults: Mutex::new(VecDeque::new()),
            activation_override: Mutex::new(None),
            registered: Mutex::new(HashMap::new()),
            open: AtomicUsize::new(0),
            counters: Counters::default(),
            kick: watch::Sender::new(0),
            alive: watch::Sender::new(0),
            shutdown: watch::Sender::new(false),
            general_inactivity: watch::Sender::new(None),
        });
        let accept_task = tokio::spawn(accept_loop(listener, Arc::clone(&shared)));
        let udp_task = udp
            .as_ref()
            .map(|s| tokio::spawn(udp_loop(Arc::clone(s), Arc::clone(&shared))));
        Ok(DoipEntity {
            shared,
            tcp_addr,
            udp,
            accept_task,
            udp_task,
        })
    }

    pub fn tcp_addr(&self) -> SocketAddr {
        self.tcp_addr
    }

    pub fn udp_addr(&self) -> Option<SocketAddr> {
        self.udp.as_ref().and_then(|s| s.local_addr().ok())
    }

    /// Queue a one-shot fault.
    pub fn inject(&self, fault: EntityFault) -> Result<(), EntityError> {
        let mut q = lock(&self.shared.faults);
        if q.len() >= MAX_QUEUED_FAULTS {
            return Err(EntityError::TooManyFaults);
        }
        q.push_back(fault);
        Ok(())
    }

    /// Force every routing activation response to `code` (`None` = normal rules).
    pub fn set_activation_code(&self, code: Option<ActivationCode>) {
        *lock(&self.shared.activation_override) = code;
    }

    /// Close every open TCP connection now.
    pub fn close_all_connections(&self) {
        self.shared.kick.send_modify(|k| *k = k.wrapping_add(1));
    }

    /// Set the `T_TCP_General_Inactivity` timer (ISO 13400-2, default
    /// [`DEFAULT_T_TCP_GENERAL_INACTIVITY`]) for every connection, open or future; `None` turns it
    /// off (the default). A connection with no DoIP message **received** from the tester for this
    /// long is closed. Any received message resets it — including an alive check response, so a
    /// tester that only answers alive checks stays connected only while the entity sends them.
    pub fn set_tcp_general_inactivity(&self, timeout: Option<Duration>) {
        self.shared.general_inactivity.send_replace(timeout);
    }

    /// Send an alive check request on every open connection.
    pub fn send_alive_check(&self) {
        self.shared.alive.send_modify(|k| *k = k.wrapping_add(1));
    }

    pub fn open_connections(&self) -> usize {
        self.shared.open.load(Ordering::Acquire)
    }

    pub fn stats(&self) -> EntityStats {
        let c = &self.shared.counters;
        EntityStats {
            connections_accepted: c.accepted.load(Ordering::Relaxed),
            connections_refused: c.refused.load(Ordering::Relaxed),
            activations: c.activations.load(Ordering::Relaxed),
            diag_received: c.diag.load(Ordering::Relaxed),
            alive_check_responses: c.alive.load(Ordering::Relaxed),
            replies_dropped: c.dropped.load(Ordering::Relaxed),
            header_nacks: c.nacks.load(Ordering::Relaxed),
            inactivity_closes: c.inactivity.load(Ordering::Relaxed),
        }
    }

    pub fn announcement(&self) -> VehicleAnnouncement {
        self.shared.announcement()
    }

    /// Send one vehicle announcement datagram to `dest` (needs the UDP socket).
    pub async fn send_announcement(&self, dest: SocketAddr) -> Result<(), EntityError> {
        let Some(sock) = &self.udp else {
            return Err(EntityError::Config("UDP disabled"));
        };
        let frame = Message::VehicleAnnouncement(self.shared.announcement())
            .encode(self.shared.cfg.version)
            .map_err(|_| EntityError::Config("announcement not encodable"))?;
        sock.send_to(&frame, dest).await?;
        Ok(())
    }
}

impl Drop for DoipEntity {
    fn drop(&mut self) {
        self.shared.shutdown.send_replace(true);
        self.accept_task.abort();
        if let Some(t) = &self.udp_task {
            t.abort();
        }
    }
}

async fn accept_loop(listener: TcpListener, sh: Arc<EntityShared>) {
    let mut shutdown = sh.shutdown.subscribe();
    let mut next_id: u64 = 0;
    loop {
        let accepted = tokio::select! {
            _ = shutdown.changed() => break,
            r = listener.accept() => r,
        };
        let stream = match accepted {
            Ok((s, _)) => s,
            Err(e) => {
                tracing::debug!("entity accept: {e}");
                continue;
            }
        };
        if sh.open.load(Ordering::Acquire) >= sh.cfg.max_connections {
            sh.counters.refused.fetch_add(1, Ordering::Relaxed);
            drop(stream);
            continue;
        }
        sh.open.fetch_add(1, Ordering::AcqRel);
        sh.counters.accepted.fetch_add(1, Ordering::Relaxed);
        next_id = next_id.wrapping_add(1);
        tokio::spawn(connection(stream, next_id, Arc::clone(&sh)));
    }
}

/// All entity writes, including ACK/alive and scheduled replies, observe shutdown/kick while
/// waiting for the socket. A failed/partial write terminates the owning connection loop.
struct EntityWriter<W = OwnedWriteHalf> {
    socket: W,
    shutdown: watch::Receiver<bool>,
    kick: watch::Receiver<u64>,
}

impl<W: AsyncWrite + Unpin> EntityWriter<W> {
    async fn write_frame(&mut self, frame: &[u8]) -> bool {
        let Self {
            socket,
            shutdown,
            kick,
        } = self;
        let closed = async {
            if *shutdown.borrow() || kick.has_changed().unwrap_or(true) {
                return;
            }
            tokio::select! {
                _ = shutdown.wait_for(|s| *s) => {},
                _ = kick.changed() => {},
            }
        };
        write_bounded(socket, frame, Instant::now() + MAX_FRAME_WRITE_TIME, closed)
            .await
            .is_ok()
    }
}

async fn write_msg<W: AsyncWrite + Unpin>(
    wr: &mut EntityWriter<W>,
    msg: &Message,
    version: ProtocolVersion,
) -> bool {
    match msg.encode(version) {
        Ok(frame) => wr.write_frame(&frame).await,
        Err(e) => {
            tracing::warn!("entity frame not encodable: {e}");
            true
        }
    }
}

type Schedule = BinaryHeap<Reverse<(Instant, u64, Vec<u8>)>>;

enum Step {
    Continue,
    Close,
}

struct Conn {
    id: u64,
    sa: Option<u16>,
    sched: Schedule,
    seq: u64,
}

async fn connection(stream: TcpStream, id: u64, sh: Arc<EntityShared>) {
    if let Err(e) = stream.set_nodelay(true) {
        tracing::debug!("entity nodelay: {e}");
    }
    let (mut rd, wr) = stream.into_split();
    let mut wr = EntityWriter {
        socket: wr,
        shutdown: sh.shutdown.subscribe(),
        kick: sh.kick.subscribe(),
    };
    let (ftx, mut frx) = mpsc::channel(CAP_ENTITY_RX_FRAMES);
    let reader = tokio::spawn(async move {
        loop {
            let r = read_message(&mut rd).await;
            let stop = matches!(r, Err(ReadError::Fatal(_) | ReadError::Closed(_)));
            if ftx.send(r).await.is_err() || stop {
                break;
            }
        }
    });
    let mut kick = sh.kick.subscribe();
    let mut alive = sh.alive.subscribe();
    let mut shutdown = sh.shutdown.subscribe();
    let mut inactivity = sh.general_inactivity.subscribe();
    let mut last_rx = Instant::now();
    kick.borrow_and_update();
    alive.borrow_and_update();
    let version = sh.cfg.version;
    let mut conn = Conn {
        id,
        sa: None,
        sched: BinaryHeap::new(),
        seq: 0,
    };
    loop {
        let next_due = conn.sched.peek().map(|Reverse((due, _, _))| *due);
        let far = Instant::now() + MAX_REPLY_DELAY;
        let idle_limit = *inactivity.borrow_and_update();
        let idle_due = idle_limit.map(|t| last_rx + t);
        let step = tokio::select! {
            biased;
            _ = shutdown.changed() => Step::Close,
            _ = kick.changed() => Step::Close,
            r = alive.changed() => {
                if r.is_err() || !write_msg(&mut wr, &Message::AliveCheckRequest, version).await {
                    Step::Close
                } else {
                    Step::Continue
                }
            }
            _ = inactivity.changed() => Step::Continue,
            r = frx.recv() => match r {
                Some(frame) => {
                    last_rx = Instant::now();
                    on_frame(frame, &mut conn, &mut wr, &sh).await
                }
                None => Step::Close,
            },
            _ = sleep_until(next_due.unwrap_or(far)), if next_due.is_some() => {
                flush_due(&mut conn, &mut wr).await
            }
            _ = sleep_until(idle_due.unwrap_or(far)), if idle_due.is_some() => {
                sh.counters.inactivity.fetch_add(1, Ordering::Relaxed);
                tracing::debug!("entity: T_TCP_General_Inactivity elapsed, closing connection {id}");
                Step::Close
            }
        };
        if matches!(step, Step::Close) {
            break;
        }
    }
    reader.abort();
    lock(&sh.registered).remove(&id);
    sh.open.fetch_sub(1, Ordering::AcqRel);
    // Drop the socket on partial write, timeout or shutdown; no graceful I/O wait here.
}

async fn flush_due(conn: &mut Conn, wr: &mut EntityWriter) -> Step {
    let now = Instant::now();
    while conn.sched.peek().is_some_and(|Reverse((due, _, _))| *due <= now) {
        if let Some(Reverse((_, _, frame))) = conn.sched.pop()
            && !wr.write_frame(&frame).await
        {
            return Step::Close;
        }
    }
    Step::Continue
}

async fn on_frame(
    frame: Result<Message, ReadError>,
    conn: &mut Conn,
    wr: &mut EntityWriter,
    sh: &EntityShared,
) -> Step {
    let version = sh.cfg.version;
    let msg = match frame {
        Ok(m) => m,
        Err(ReadError::Discarded(e)) => {
            sh.counters.nacks.fetch_add(1, Ordering::Relaxed);
            return if write_msg(wr, &Message::GenericNack { code: e.nack_code() }, version).await {
                Step::Continue
            } else {
                Step::Close
            };
        }
        Err(ReadError::Fatal(e)) => {
            sh.counters.nacks.fetch_add(1, Ordering::Relaxed);
            write_msg(wr, &Message::GenericNack { code: e.nack_code() }, version).await;
            return Step::Close;
        }
        Err(ReadError::Closed(_)) => return Step::Close,
    };
    match msg {
        Message::RoutingActivationRequest {
            source,
            activation_type,
            ..
        } => {
            let code = sh.activation_code(conn.id, conn.sa, source, activation_type.is_supported());
            if code.is_success() {
                conn.sa = Some(source);
                lock(&sh.registered).insert(conn.id, source);
                sh.counters.activations.fetch_add(1, Ordering::Relaxed);
            }
            let resp = Message::RoutingActivationResponse {
                tester: source,
                entity: sh.cfg.entity_la,
                code,
                oem: None,
            };
            let ok = write_msg(wr, &resp, version).await;
            if !ok || !(code.is_success() || code == ActivationCode::CONFIRMATION_REQUIRED) {
                return Step::Close;
            }
            Step::Continue
        }
        Message::AliveCheckResponse { .. } => {
            sh.counters.alive.fetch_add(1, Ordering::Relaxed);
            Step::Continue
        }
        Message::DiagnosticMessage { source, target, data } => {
            on_diag(source, target, &data, conn, wr, sh).await
        }
        Message::EntityStatusRequest => {
            let resp = Message::EntityStatusResponse {
                node_type: 0,
                max_sockets: u8::try_from(sh.cfg.max_connections).unwrap_or(u8::MAX),
                open_sockets: u8::try_from(sh.open.load(Ordering::Acquire)).unwrap_or(u8::MAX),
                max_data_size: u32::try_from(MAX_DOIP_PAYLOAD).ok(),
            };
            if write_msg(wr, &resp, version).await {
                Step::Continue
            } else {
                Step::Close
            }
        }
        Message::PowerModeRequest => {
            if write_msg(wr, &Message::PowerModeResponse { mode: 0x01 }, version).await {
                Step::Continue
            } else {
                Step::Close
            }
        }
        other => {
            tracing::debug!("entity ignores payload type 0x{:04X}", other.payload_type());
            Step::Continue
        }
    }
}

async fn on_diag(
    source: u16,
    target: u16,
    data: &[u8],
    conn: &mut Conn,
    wr: &mut EntityWriter,
    sh: &EntityShared,
) -> Step {
    let version = sh.cfg.version;
    sh.counters.diag.fetch_add(1, Ordering::Relaxed);
    let nack = |code| Message::DiagnosticNack {
        source: target,
        target: source,
        code,
        previous: Vec::new(),
    };
    if conn.sa != Some(source) {
        // ISO 13400-2: invalid source address → NACK 0x02 and close the socket.
        write_msg(wr, &nack(diag_code::INVALID_SOURCE_ADDRESS), version).await;
        return Step::Close;
    }
    let fault = lock(&sh.faults).pop_front();
    match fault {
        Some(EntityFault::CloseOnNextDiag) => return Step::Close,
        Some(EntityFault::DropNextAck) => return Step::Continue,
        Some(EntityFault::NackNextDiag(code)) => {
            return if write_msg(wr, &nack(code), version).await {
                Step::Continue
            } else {
                Step::Close
            };
        }
        None => {}
    }
    if !sh.serves(target) {
        return if write_msg(wr, &nack(diag_code::UNKNOWN_TARGET_ADDRESS), version).await {
            Step::Continue
        } else {
            Step::Close
        };
    }
    let ack = Message::DiagnosticAck {
        source: target,
        target: source,
        code: diag_code::ACK,
        previous: Vec::new(),
    };
    if !write_msg(wr, &ack, version).await {
        return Step::Close;
    }
    let arrived = Instant::now();
    let replies = {
        let mut h = lock(&sh.handler);
        h.handle(target, arrived.duration_since(sh.start), data)
    };
    let mut immediate = false;
    for (delay, bytes) in replies {
        if bytes.is_empty() || bytes.len() > MAX_UDS_PDU {
            sh.counters.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        if conn.sched.len() >= MAX_SCHEDULED_REPLIES_PER_CONN {
            sh.counters.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let msg = Message::DiagnosticMessage {
            source: target,
            target: source,
            data: bytes,
        };
        let Ok(frame) = msg.encode(version) else {
            sh.counters.dropped.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        immediate |= delay.is_zero();
        conn.seq = conn.seq.wrapping_add(1);
        conn.sched
            .push(Reverse((arrived + delay.min(MAX_REPLY_DELAY), conn.seq, frame)));
    }
    if immediate {
        return flush_due(conn, wr).await;
    }
    Step::Continue
}

async fn udp_loop(sock: Arc<UdpSocket>, sh: Arc<EntityShared>) {
    let mut shutdown = sh.shutdown.subscribe();
    let mut buf = vec![0u8; MAX_DOIP_FRAME];
    let version = sh.cfg.version;
    loop {
        let r = tokio::select! {
            _ = shutdown.changed() => break,
            r = sock.recv_from(&mut buf) => r,
        };
        let Ok((n, from)) = r else {
            continue;
        };
        let reply = match decode_frame(buf.get(..n).unwrap_or_default()) {
            Ok(Some((_, msg, _))) => match msg {
                Message::VehicleIdRequest => Some(Message::VehicleAnnouncement(sh.announcement())),
                Message::VehicleIdRequestEid { eid } if eid == sh.cfg.eid => {
                    Some(Message::VehicleAnnouncement(sh.announcement()))
                }
                Message::VehicleIdRequestVin { vin } if vin == sh.cfg.vin => {
                    Some(Message::VehicleAnnouncement(sh.announcement()))
                }
                Message::PowerModeRequest => Some(Message::PowerModeResponse { mode: 0x01 }),
                _ => None,
            },
            Ok(None) => Some(Message::GenericNack {
                code: header_nack::INVALID_PAYLOAD_LENGTH,
            }),
            Err(e) => Some(Message::GenericNack { code: e.nack_code() }),
        };
        if let Some(m) = reply
            && let Ok(frame) = m.encode(version)
            && let Err(e) = sock.send_to(&frame, from).await
        {
            tracing::debug!("entity udp send: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test(start_paused = true)]
    async fn entity_stalled_reply_is_bounded_and_partial_stream_is_discarded() {
        let (socket, mut peer) = tokio::io::duplex(1);
        let (_stop, shutdown) = watch::channel(false);
        let (_kick, kick) = watch::channel(0);
        let task = tokio::spawn(async move {
            let mut writer = EntityWriter {
                socket,
                shutdown,
                kick,
            };
            write_msg(&mut writer, &Message::AliveCheckRequest, ProtocolVersion::Iso2012).await
        });
        let ok = tokio::time::timeout(MAX_FRAME_WRITE_TIME + Duration::from_millis(10), task)
            .await
            .expect("entity write bound")
            .expect("task");
        assert!(!ok, "a partial frame cannot be reported complete");
        let mut bytes = [0; 16];
        assert_eq!(peer.read(&mut bytes).await.expect("prefix"), 1);
        assert_eq!(peer.read(&mut bytes).await.expect("EOF"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn entity_close_interrupts_stalled_write() {
        let (socket, mut peer) = tokio::io::duplex(1);
        let (_stop, shutdown) = watch::channel(false);
        let (close, kick) = watch::channel(0);
        let task = tokio::spawn(async move {
            let mut writer = EntityWriter {
                socket,
                shutdown,
                kick,
            };
            writer.write_frame(&[1, 2, 3]).await
        });
        tokio::task::yield_now().await;
        close.send_replace(1);
        assert!(
            !tokio::time::timeout(Duration::from_millis(10), task)
                .await
                .expect("kick promptly")
                .expect("task")
        );
        let mut bytes = [0; 8];
        assert_eq!(peer.read(&mut bytes).await.expect("prefix"), 1);
        assert_eq!(peer.read(&mut bytes).await.expect("EOF"), 0);
    }
}
