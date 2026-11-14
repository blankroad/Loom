//! `tx` — the TX actor: one thread that owns the [`Transport`] and serializes every write
//! (UART plan §3.5, §3.7-1, §3.13 TX row).
//!
//! Three bounded lanes: Ctrl-C, user, agent. At **every chunk boundary** (chunks ≤
//! [`TX_CHUNK_MAX`]) the actor picks Ctrl-C > user > agent. A Ctrl-C arriving while an agent
//! request is in progress **cancels the rest of that agent request** (`interrupted_by_user`;
//! bytes already accepted by the driver stay counted in `written_bytes`). Ordinary user bytes do
//! not cancel the agent: they are interleaved at the next chunk boundary and the agent request
//! then continues.
//! Explicit [`TxHandle::cancel`] / [`TxHandle::cancel_lane`] let InputArbiter preempt an agent
//! without pretending ordinary user bytes are Ctrl-C. Cancellation is asynchronous: queued
//! requests leave the runnable queue immediately; active requests stop at a chunk boundary.
//!
//! Every request has an absolute deadline; a blocked transport (CTS low, stalled driver) ends the
//! request with `OutcomeUnknown` (some bytes written) or `Failed{E_TIMEOUT}` (none). No automatic
//! retransmission, and `flush()`/`tcdrain` is never called (§3.5, §7.3): `Sent` means "accepted
//! by the driver", not "on the wire".
//!
//! Results go out on a bounded `sync_channel` as [`TxEvent`]s: one [`TxChunk`] per chunk actually
//! written (P1: `uart.data dir=tx`) and one [`TxDone`] per request. Events are never dropped; a
//! consumer that stops reading back-pressures the actor.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::clock::{Clock, SeqSource};
use crate::transport::{Transport, WriteOutcome};

/// Largest single driver write (§3.5: ≤ 1 KiB chunks).
pub const TX_CHUNK_MAX: usize = 1024;
/// Ctrl-C lane byte limit, queued + in flight (§3.13 TX row: 64 B).
pub const CTRLC_QUEUE_BYTES: usize = 64;
/// Ctrl-C lane item limit (§3.13: 64 items).
pub const CTRLC_QUEUE_ITEMS: usize = 64;
/// User lane byte limit, queued + in flight (§3.13: 4 KiB).
pub const USER_QUEUE_BYTES: usize = 4 * 1024;
/// User lane item limit (§3.13: 64 items).
pub const USER_QUEUE_ITEMS: usize = 64;
/// Agent lane byte limit, queued + in flight (§3.13: 64 KiB).
pub const AGENT_QUEUE_BYTES: usize = 64 * 1024;
/// Agent lane request limit (§3.13: 64 requests).
pub const AGENT_QUEUE_ITEMS: usize = 64;
/// Pending DTR/RTS/break operations; more fail with `WouldBlock` (§3.13 control path).
pub const TX_CONTROL_QUEUE: usize = 8;
/// Cancellation mailbox item cap (§3.13): one slot per accepted lane request at most.
/// Overflow is explicitly refused; cancelled requests retain their original lane permits.
pub const TX_CANCEL_QUEUE_ITEMS: usize = CTRLC_QUEUE_ITEMS + USER_QUEUE_ITEMS + AGENT_QUEUE_ITEMS;
/// Payload bytes retained by the cancellation mailbox (§3.13), shared with lane byte limits.
/// Requests move into the mailbox without cloning their payload or client id.
pub const TX_CANCEL_QUEUE_BYTES: usize = CTRLC_QUEUE_BYTES + USER_QUEUE_BYTES + AGENT_QUEUE_BYTES;
/// Longest supported cancellation cause (§3.13 control path); causes are fixed-size internally.
pub const TX_CANCEL_CAUSE_BYTES: usize = "interrupted_by_user".len();
/// Recommended capacity for the event channel (see [`event_channel`]); a full channel blocks
/// the actor rather than dropping write results (§3.13: write terminals are never dropped).
pub const TX_EVENT_CHANNEL_CAP: usize = 1024;

/// Which queue a request goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxLane {
    /// User Ctrl-C (and other interrupt bytes) — highest priority, cancels in-progress agent.
    CtrlC,
    /// User keystrokes.
    User,
    /// Agent sends.
    Agent,
}

impl TxLane {
    const ALL: [TxLane; 3] = [TxLane::CtrlC, TxLane::User, TxLane::Agent];

    fn idx(self) -> usize {
        match self {
            TxLane::CtrlC => 0,
            TxLane::User => 1,
            TxLane::Agent => 2,
        }
    }

    /// `(bytes, items)` limits of the lane.
    pub fn limits(self) -> (usize, usize) {
        match self {
            TxLane::CtrlC => (CTRLC_QUEUE_BYTES, CTRLC_QUEUE_ITEMS),
            TxLane::User => (USER_QUEUE_BYTES, USER_QUEUE_ITEMS),
            TxLane::Agent => (AGENT_QUEUE_BYTES, AGENT_QUEUE_ITEMS),
        }
    }

    /// Origin implied by the lane.
    pub fn origin(self) -> TxOrigin {
        match self {
            TxLane::CtrlC | TxLane::User => TxOrigin::User,
            TxLane::Agent => TxOrigin::Agent,
        }
    }
}

/// Who caused a write (decided by connection credentials upstream, §3.7-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxOrigin {
    /// The human at the UI.
    User,
    /// An agent.
    Agent,
}

/// One write request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRequest {
    /// Caller-assigned id, echoed in events.
    pub request_id: u64,
    /// Queue.
    pub lane: TxLane,
    /// Requesting client.
    pub client_id: String,
    /// Raw bytes to send.
    pub bytes: Vec<u8>,
    /// Secret input (`--secret-env`): the bytes go to the device, but every shared record of them
    /// (event, ring, journal) carries only the length and a mask.
    pub secret: bool,
    /// Absolute deadline for the whole request.
    pub deadline: Instant,
}

/// Terminal state of a request (common write states D4·A14; UART success terminal is `Sent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxState {
    /// Every byte was accepted by the driver.
    Sent,
    /// Nothing was written.
    Failed,
    /// Some bytes were written, or cancellation claimed an active write; effect is unknown.
    OutcomeUnknown,
}

/// Why a request did not reach `Sent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxErrorCode {
    /// The deadline passed (blocked transport or late dequeue).
    Timeout,
    /// A user Ctrl-C cancelled the rest of an agent request.
    InterruptedByUser,
    /// The transport reported an I/O error.
    Io(io::ErrorKind),
    /// The actor shut down first.
    Shutdown,
}

impl TxErrorCode {
    /// Error code string (`E_TIMEOUT`, ...).
    pub fn code(&self) -> &'static str {
        match self {
            TxErrorCode::Timeout => "E_TIMEOUT",
            TxErrorCode::InterruptedByUser => "E_INTERRUPTED",
            TxErrorCode::Io(_) => "E_IO",
            TxErrorCode::Shutdown => "E_SHUTDOWN",
        }
    }

    /// Cause string (`interrupted_by_user`, ...).
    pub fn cause(&self) -> &'static str {
        match self {
            TxErrorCode::Timeout => "deadline",
            TxErrorCode::InterruptedByUser => "interrupted_by_user",
            TxErrorCode::Io(_) => "io_error",
            TxErrorCode::Shutdown => "shutdown",
        }
    }
}

/// Bytes actually handed to the driver in one write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxChunk {
    /// Worker-wide sequence number (shared with RX chunks).
    pub source_seq: u64,
    /// Lane.
    pub lane: TxLane,
    /// Origin.
    pub origin: TxOrigin,
    /// Requesting client.
    pub client_id: String,
    /// Request id.
    pub request_id: u64,
    /// Host monotonic time after the write returned.
    pub mono_ns: u64,
    /// The bytes written (real bytes; the consumer must mask them when `secret`).
    pub bytes: Vec<u8>,
    /// Copied from [`TxRequest::secret`].
    pub secret: bool,
}

/// Completion of one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxDone {
    /// Request id.
    pub request_id: u64,
    /// Lane.
    pub lane: TxLane,
    /// Origin.
    pub origin: TxOrigin,
    /// Requesting client.
    pub client_id: String,
    /// Terminal state.
    pub state: TxState,
    /// Bytes accepted by the driver.
    pub written_bytes: u64,
    /// Bytes not written.
    pub remaining_bytes: u64,
    /// Mono time of the first written chunk.
    pub first_mono_ns: Option<u64>,
    /// Mono time of the last written chunk.
    pub last_mono_ns: Option<u64>,
    /// Set unless `state == Sent`.
    pub error: Option<TxErrorCode>,
    /// Exact terminal cause, including `stopped`, `held_by_user`, and `closed` cancellation.
    /// Existing `error` remains compatible: non-user cancellation uses `Shutdown` there.
    /// New integrations should use this field to distinguish those cancellation reasons.
    pub cause: Option<&'static str>,
}

/// Result of a nonblocking cancellation request, not a replacement for the eventual TxDone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CancelResult {
    /// Removed from the runnable queue; a Failed/zero-byte completion will follow.
    Queued,
    /// Claimed an active request; it stops at a chunk boundary with OutcomeUnknown.
    InProgress,
    /// This request already has a cancellation claim; the original cause wins.
    AlreadyRequested,
    /// Unknown, terminal-committed, or shutting down; no completion is manufactured.
    NotFound,
    /// The bounded cancellation mailbox cannot reserve space; nothing changed.
    QueueFull,
    /// Cause is not interrupted_by_user, stopped, held_by_user, or closed; nothing changed.
    InvalidCause,
}

#[derive(Debug, Clone, Copy)]
enum CancelCause {
    InterruptedByUser,
    Stopped,
    HeldByUser,
    Closed,
}

impl CancelCause {
    fn parse(cause: &str) -> Option<Self> {
        if cause.len() > TX_CANCEL_CAUSE_BYTES {
            return None;
        }
        match cause {
            "interrupted_by_user" => Some(Self::InterruptedByUser),
            "stopped" => Some(Self::Stopped),
            "held_by_user" => Some(Self::HeldByUser),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::InterruptedByUser => "interrupted_by_user",
            Self::Stopped => "stopped",
            Self::HeldByUser => "held_by_user",
            Self::Closed => "closed",
        }
    }

    fn legacy_error(self) -> TxErrorCode {
        match self {
            Self::InterruptedByUser => TxErrorCode::InterruptedByUser,
            Self::Stopped | Self::HeldByUser | Self::Closed => TxErrorCode::Shutdown,
        }
    }
}

/// Actor output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxEvent {
    /// A chunk was written.
    Chunk(TxChunk),
    /// A request finished.
    Done(TxDone),
}

/// Line-control operation executed on the actor thread at a chunk boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlOp {
    /// Set DTR.
    Dtr(bool),
    /// Set RTS.
    Rts(bool),
    /// Assert/clear break.
    Break(bool),
}

/// Why a request was not queued (nothing is silently dropped).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    /// The lane is at its byte or item limit.
    #[error("{lane:?} queue full ({queued_bytes} B / {queued_items} items queued)")]
    QueueFull {
        /// Lane.
        lane: TxLane,
        /// Bytes queued + in flight.
        queued_bytes: usize,
        /// Items queued + in flight.
        queued_items: usize,
    },
    /// The request alone exceeds the lane's byte limit.
    #[error("{lane:?} request of {len} B exceeds lane limit {cap} B")]
    TooLarge {
        /// Lane.
        lane: TxLane,
        /// Request length.
        len: usize,
        /// Lane byte limit.
        cap: usize,
    },
    /// Empty request.
    #[error("empty request")]
    Empty,
    /// The actor is shutting down.
    #[error("tx actor stopped")]
    Stopped,
}

/// Creates the bounded event channel for [`TxActor::spawn`].
pub fn event_channel() -> (SyncSender<TxEvent>, Receiver<TxEvent>) {
    sync_channel(TX_EVENT_CHANNEL_CAP)
}

type ControlReq = (ControlOp, SyncSender<io::Result<()>>);

#[derive(Debug, Default)]
struct LaneQ {
    q: VecDeque<TxRequest>,
    /// Bytes queued + in flight.
    bytes: usize,
    /// Items queued + in flight.
    items: usize,
}

#[derive(Debug)]
struct ActiveRequest {
    request_id: u64,
    cancel: Option<CancelCause>,
}

#[derive(Debug)]
struct CancelledRequest {
    req: TxRequest,
    cause: CancelCause,
}

#[derive(Debug, Default)]
struct State {
    lanes: [LaneQ; 3],
    controls: VecDeque<ControlReq>,
    // Three fixed active cancellation slots, including while Transport::write is blocked.
    active: [Option<ActiveRequest>; 3],
    // §3.13: bounded by TX_CANCEL_QUEUE_ITEMS/BYTES and the original lane permits.
    cancelled: VecDeque<CancelledRequest>,
    cancelled_bytes: usize,
    stopping: bool,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Cloneable submission handle.
#[derive(Debug, Clone)]
pub struct TxHandle {
    shared: Arc<Shared>,
}

impl TxHandle {
    /// Queues a request or rejects it explicitly.
    pub fn submit(&self, req: TxRequest) -> Result<(), SubmitError> {
        if req.bytes.is_empty() {
            return Err(SubmitError::Empty);
        }
        let lane = req.lane;
        let (cap_bytes, cap_items) = lane.limits();
        if req.bytes.len() > cap_bytes {
            return Err(SubmitError::TooLarge {
                lane,
                len: req.bytes.len(),
                cap: cap_bytes,
            });
        }
        let mut st = self.shared.lock();
        if st.stopping {
            return Err(SubmitError::Stopped);
        }
        let l = &mut st.lanes[lane.idx()];
        if l.bytes + req.bytes.len() > cap_bytes || l.items >= cap_items {
            return Err(SubmitError::QueueFull {
                lane,
                queued_bytes: l.bytes,
                queued_items: l.items,
            });
        }
        l.bytes += req.bytes.len();
        l.items += 1;
        l.q.push_back(req);
        drop(st);
        self.shared.cv.notify_one();
        Ok(())
    }

    /// Cancel a live request without waiting for the transport or event consumer.
    ///
    /// Accepted requests must have unique live ids. A queued request is removed immediately
    /// and completes asynchronously with Failed/zero bytes. An active request stops at the
    /// next chunk boundary (at most one already reserved write of TX_CHUNK_MAX bytes) and
    /// completes with OutcomeUnknown and exact driver-accepted bytes, possibly zero or all.
    /// Completion and cancellation claims are serialized; each accepted request emits one Done.
    /// Fixed supported causes are `interrupted_by_user`, `stopped`, `held_by_user`, `closed`.
    /// Existing transport deadlines and event-channel backpressure still apply.
    pub fn cancel(&self, request_id: u64, cause: &str) -> CancelResult {
        let Some(cause) = CancelCause::parse(cause) else {
            return CancelResult::InvalidCause;
        };
        let mut st = self.shared.lock();
        if st.stopping {
            return CancelResult::NotFound;
        }
        for active in st.active.iter_mut().flatten() {
            if active.request_id == request_id {
                if active.cancel.is_some() {
                    return CancelResult::AlreadyRequested;
                }
                active.cancel = Some(cause);
                drop(st);
                self.shared.cv.notify_one();
                return CancelResult::InProgress;
            }
        }
        if st
            .cancelled
            .iter()
            .any(|cancelled| cancelled.req.request_id == request_id)
        {
            return CancelResult::AlreadyRequested;
        }
        for lane in TxLane::ALL {
            let index = lane.idx();
            if let Some(position) = st.lanes[index]
                .q
                .iter()
                .position(|req| req.request_id == request_id)
            {
                let bytes = st.lanes[index].q[position].bytes.len();
                if st.cancelled.len() >= TX_CANCEL_QUEUE_ITEMS
                    || st.cancelled_bytes + bytes > TX_CANCEL_QUEUE_BYTES
                {
                    return CancelResult::QueueFull;
                }
                if let Some(req) = st.lanes[index].q.remove(position) {
                    st.cancelled_bytes += bytes;
                    st.cancelled.push_back(CancelledRequest { req, cause });
                    drop(st);
                    self.shared.cv.notify_one();
                    return CancelResult::Queued;
                }
            }
        }
        CancelResult::NotFound
    }

    /// Atomically cancel every currently queued/active request in one lane.
    ///
    /// Returns the number newly claimed (not previously cancelled). `Agent` leaves both user
    /// lanes intact. This is a snapshot, not a persistent latch: the caller must close agent
    /// admission before using it for STOP/HOLD/close. Unsupported causes, shutdown, or inability
    /// to reserve the whole bounded mailbox return zero without changing any request.
    pub fn cancel_lane(&self, lane: TxLane, cause: &str) -> usize {
        let Some(cause) = CancelCause::parse(cause) else {
            return 0;
        };
        let mut st = self.shared.lock();
        if st.stopping {
            return 0;
        }
        let index = lane.idx();
        let queued_items = st.lanes[index].q.len();
        let queued_bytes: usize = st.lanes[index].q.iter().map(|req| req.bytes.len()).sum();
        if st.cancelled.len() + queued_items > TX_CANCEL_QUEUE_ITEMS
            || st.cancelled_bytes + queued_bytes > TX_CANCEL_QUEUE_BYTES
        {
            return 0;
        }
        let mut count = queued_items;
        while let Some(req) = st.lanes[index].q.pop_front() {
            st.cancelled_bytes += req.bytes.len();
            st.cancelled.push_back(CancelledRequest { req, cause });
        }
        if let Some(active) = &mut st.active[index]
            && active.cancel.is_none()
        {
            active.cancel = Some(cause);
            count += 1;
        }
        drop(st);
        self.shared.cv.notify_one();
        count
    }

    /// Runs a line-control operation on the actor thread at the next chunk boundary and waits
    /// up to `timeout` for its result.
    pub fn control(&self, op: ControlOp, timeout: Duration) -> io::Result<()> {
        let (tx, rx) = sync_channel(1);
        {
            let mut st = self.shared.lock();
            if st.stopping {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "tx actor stopped"));
            }
            if st.controls.len() >= TX_CONTROL_QUEUE {
                return Err(io::Error::new(io::ErrorKind::WouldBlock, "tx control queue full"));
            }
            st.controls.push_back((op, tx));
        }
        self.shared.cv.notify_one();
        rx.recv_timeout(timeout)
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "tx control did not complete in time"))?
    }

    /// `(bytes, items)` charged to `lane`, including cancelled requests awaiting completion.
    pub fn queued(&self, lane: TxLane) -> (usize, usize) {
        let st = self.shared.lock();
        let l = &st.lanes[lane.idx()];
        (l.bytes, l.items)
    }
}

/// The running actor. Dropping it shuts the thread down (pending requests end `E_SHUTDOWN`).
pub struct TxActor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Box<dyn Transport>>>,
}

impl fmt::Debug for TxActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TxActor")
            .field("running", &self.thread.is_some())
            .finish()
    }
}

impl TxActor {
    /// Spawns the actor thread, which takes ownership of `transport`.
    pub fn spawn(
        transport: Box<dyn Transport>,
        seq: Arc<SeqSource>,
        clock: Arc<dyn Clock>,
        events: SyncSender<TxEvent>,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("loom-uart-tx".into())
            .spawn(move || {
                let mut w = Worker {
                    shared: s,
                    transport,
                    seq,
                    clock,
                    events,
                    active: [None, None, None],
                };
                w.run();
                w.transport
            })?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// A submission handle.
    pub fn handle(&self) -> TxHandle {
        TxHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Stops the actor; every unfinished request gets a `TxDone` with `E_SHUTDOWN`. Returns the
    /// transport.
    pub fn shutdown(mut self) -> Option<Box<dyn Transport>> {
        self.stop_inner()
    }

    fn stop_inner(&mut self) -> Option<Box<dyn Transport>> {
        self.shared.lock().stopping = true;
        self.shared.cv.notify_all();
        self.thread.take().and_then(|t| t.join().ok())
    }
}

impl Drop for TxActor {
    fn drop(&mut self) {
        let _ = self.stop_inner();
    }
}

#[derive(Debug)]
struct InFlight {
    req: TxRequest,
    written: usize,
    first: Option<u64>,
    last: Option<u64>,
}

struct Worker {
    shared: Arc<Shared>,
    transport: Box<dyn Transport>,
    seq: Arc<SeqSource>,
    clock: Arc<dyn Clock>,
    events: SyncSender<TxEvent>,
    active: [Option<InFlight>; 3],
}

impl Worker {
    fn run(&mut self) {
        loop {
            let mut controls = Vec::new();
            let mut pick = None;
            let mut cancel_agent = false;
            {
                let mut st = self.shared.lock();
                while !st.stopping
                    && st.controls.is_empty()
                    && st.cancelled.is_empty()
                    && self.active.iter().all(Option::is_none)
                    && st.lanes.iter().all(|l| l.q.is_empty())
                {
                    st = self.shared.cv.wait(st).unwrap_or_else(|p| p.into_inner());
                }
                if st.stopping {
                    drop(st);
                    self.drain_on_shutdown();
                    return;
                }
                for lane in TxLane::ALL {
                    let i = lane.idx();
                    if self.active[i].is_none()
                        && let Some(req) = st.lanes[i].q.pop_front()
                    {
                        st.active[i] = Some(ActiveRequest {
                            request_id: req.request_id,
                            cancel: None,
                        });
                        self.active[i] = Some(InFlight {
                            req,
                            written: 0,
                            first: None,
                            last: None,
                        });
                        // Only the highest non-empty lane is activated per boundary.
                    }
                    if self.active[i].is_some() {
                        pick = Some(lane);
                        break;
                    }
                }
                if pick == Some(TxLane::CtrlC) && self.active[TxLane::Agent.idx()].is_some() {
                    cancel_agent = true;
                }
                // Ctrl-C never waits behind cancellation notifications or line controls.
                if pick != Some(TxLane::CtrlC) {
                    controls.extend(st.controls.drain(..));
                }
            }
            for (op, reply) in controls {
                let r = match op {
                    ControlOp::Dtr(on) => self.transport.set_dtr(on),
                    ControlOp::Rts(on) => self.transport.set_rts(on),
                    ControlOp::Break(on) => self.transport.set_break(on),
                };
                let _ = reply.try_send(r);
            }
            if cancel_agent && let Some(f) = self.active[TxLane::Agent.idx()].take() {
                self.finish(f, Some(TxErrorCode::InterruptedByUser));
            }
            if pick != Some(TxLane::CtrlC) {
                // A cancelled, paused agent must complete even while user input stays ready.
                self.finish_cancelled_active();
            }
            if let Some(lane) = pick {
                self.step(lane);
            }
            self.finish_one_cancelled();
        }
    }

    /// Writes one chunk of the lane's in-flight request.
    fn step(&mut self, lane: TxLane) {
        let i = lane.idx();
        let Some(mut f) = self.active[i].take() else {
            return;
        };
        let cancelled = self.shared.lock().active[i]
            .as_ref()
            .is_some_and(|active| active.cancel.is_some());
        if cancelled {
            self.finish(f, None);
            return;
        }
        let len = f.req.bytes.len();
        if Instant::now() >= f.req.deadline {
            self.finish(f, Some(TxErrorCode::Timeout));
            return;
        }
        let end = (f.written + TX_CHUNK_MAX).min(len);
        let r = self.transport.write(&f.req.bytes[f.written..end], f.req.deadline);
        if r.written > 0 {
            let mono = self.clock.mono_ns();
            f.first.get_or_insert(mono);
            f.last = Some(mono);
            let chunk = TxChunk {
                source_seq: self.seq.next(),
                lane,
                origin: lane.origin(),
                client_id: f.req.client_id.clone(),
                request_id: f.req.request_id,
                mono_ns: mono,
                bytes: f.req.bytes[f.written..f.written + r.written].to_vec(),
                secret: f.req.secret,
            };
            f.written += r.written;
            let _ = self.events.send(TxEvent::Chunk(chunk));
        }
        match r.outcome {
            WriteOutcome::Complete if f.written >= len => self.finish(f, None),
            WriteOutcome::Complete => self.active[i] = Some(f),
            WriteOutcome::TimedOut => self.finish(f, Some(TxErrorCode::Timeout)),
            WriteOutcome::Failed(kind) => self.finish(f, Some(TxErrorCode::Io(kind))),
        }
    }

    fn finish(&mut self, f: InFlight, error: Option<TxErrorCode>) {
        self.finish_inner(f, error, None, true);
    }

    fn finish_inner(
        &mut self,
        mut f: InFlight,
        error: Option<TxErrorCode>,
        queued_cancel: Option<CancelCause>,
        was_active: bool,
    ) {
        let len = f.req.bytes.len();
        // Release payload storage before returning its permits or blocking on the Done sink.
        // Cancellation metadata never creates a second retained copy of the request bytes.
        drop(std::mem::take(&mut f.req.bytes));
        let cancel = {
            let mut st = self.shared.lock();
            let index = f.req.lane.idx();
            // Linearization point: after this lock is released the id is terminal-committed,
            // so cancel cannot create a second Done, even if sending this Done backpressures.
            let active_cancel = if was_active {
                st.active[index].take().and_then(|active| active.cancel)
            } else {
                None
            };
            let l = &mut st.lanes[f.req.lane.idx()];
            l.bytes -= len;
            l.items -= 1;
            active_cancel.or(queued_cancel)
        };
        let error = cancel.map(CancelCause::legacy_error).or(error);
        let state = match (cancel.is_some() && was_active, error, f.written) {
            (true, _, _) => TxState::OutcomeUnknown,
            (false, None, _) => TxState::Sent,
            (false, Some(_), 0) => TxState::Failed,
            (false, Some(_), _) => TxState::OutcomeUnknown,
        };
        let done = TxDone {
            request_id: f.req.request_id,
            lane: f.req.lane,
            origin: f.req.lane.origin(),
            client_id: f.req.client_id,
            state,
            written_bytes: f.written as u64,
            remaining_bytes: (len - f.written) as u64,
            first_mono_ns: f.first,
            last_mono_ns: f.last,
            error,
            cause: cancel
                .map(CancelCause::as_str)
                .or_else(|| error.map(|error| error.cause())),
        };
        let _ = self.events.send(TxEvent::Done(done));
    }

    fn finish_one_cancelled(&mut self) {
        let cancelled = {
            let mut st = self.shared.lock();
            let ctrl_c = TxLane::CtrlC.idx();
            if st.active[ctrl_c].is_some() || !st.lanes[ctrl_c].q.is_empty() {
                return;
            }
            let cancelled = st.cancelled.pop_front();
            if let Some(cancelled) = &cancelled {
                st.cancelled_bytes -= cancelled.req.bytes.len();
            }
            cancelled
        };
        if let Some(cancelled) = cancelled {
            self.finish_inner(
                InFlight {
                    req: cancelled.req,
                    written: 0,
                    first: None,
                    last: None,
                },
                None,
                Some(cancelled.cause),
                false,
            );
        }
    }

    fn finish_cancelled_active(&mut self) {
        let cancelled: [bool; 3] = {
            let st = self.shared.lock();
            std::array::from_fn(|index| {
                st.active[index]
                    .as_ref()
                    .is_some_and(|active| active.cancel.is_some())
            })
        };
        for (index, cancelled) in cancelled.into_iter().enumerate() {
            if cancelled && let Some(f) = self.active[index].take() {
                self.finish(f, None);
            }
        }
    }

    fn drain_on_shutdown(&mut self) {
        for i in 0..self.active.len() {
            if let Some(f) = self.active[i].take() {
                self.finish(f, Some(TxErrorCode::Shutdown));
            }
        }
        let (queued, controls, cancelled) = {
            let mut st = self.shared.lock();
            let q: Vec<TxRequest> = st.lanes.iter_mut().flat_map(|l| l.q.drain(..)).collect();
            let c: Vec<ControlReq> = st.controls.drain(..).collect();
            let cancelled: Vec<CancelledRequest> = st.cancelled.drain(..).collect();
            st.cancelled_bytes = 0;
            (q, c, cancelled)
        };
        for req in queued {
            self.finish_inner(
                InFlight {
                    req,
                    written: 0,
                    first: None,
                    last: None,
                },
                Some(TxErrorCode::Shutdown),
                None,
                false,
            );
        }
        for cancelled in cancelled {
            self.finish_inner(
                InFlight {
                    req: cancelled.req,
                    written: 0,
                    first: None,
                    last: None,
                },
                None,
                Some(cancelled.cause),
                false,
            );
        }
        for (_, reply) in controls {
            let _ = reply.try_send(Err(io::Error::new(io::ErrorKind::BrokenPipe, "tx actor stopped")));
        }
    }
}
