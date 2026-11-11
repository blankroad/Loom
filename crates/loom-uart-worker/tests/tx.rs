//! TX actor: lane limits, Ctrl-C cancellation at chunk boundaries, user interleaving, deadlines
//! on a blocked transport, per-lane FIFO, control ops (UART plan §3.5, §3.7, §3.13).

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_uart_worker::clock::{FixedClock, SeqSource};
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::tx::{
    AGENT_QUEUE_BYTES, AGENT_QUEUE_ITEMS, CTRLC_QUEUE_BYTES, CTRLC_QUEUE_ITEMS, ControlOp, SubmitError,
    TX_CHUNK_MAX, TxActor, TxDone, TxErrorCode, TxEvent, TxLane, TxOrigin, TxRequest, TxState,
    USER_QUEUE_BYTES, USER_QUEUE_ITEMS, event_channel,
};

const WAIT: Duration = Duration::from_secs(2);

fn no_rx() -> io::Result<Box<dyn RxHandle>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "tx-only fake"))
}

/// Each write announces its bytes on `entered` and completes only when the test releases it.
#[derive(Debug)]
struct Gate {
    entered: Sender<Vec<u8>>,
    release: Receiver<()>,
    cfg: PortConfig,
}

impl Transport for Gate {
    fn read(&mut self, _: &mut [u8], _: Duration) -> io::Result<usize> {
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
        let _ = self.entered.send(buf.to_vec());
        let left = deadline.saturating_duration_since(Instant::now());
        match self.release.recv_timeout(left) {
            Ok(()) => WriteResult {
                written: buf.len(),
                outcome: WriteOutcome::Complete,
            },
            Err(_) => WriteResult {
                written: 0,
                outcome: WriteOutcome::TimedOut,
            },
        }
    }
    fn set_dtr(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_rts(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_break(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn config(&self) -> &PortConfig {
        &self.cfg
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        no_rx()
    }
}

/// Accepts `capacity` bytes in total, then blocks until the deadline (like CTS held low).
#[derive(Debug)]
struct Stall {
    capacity: usize,
    cfg: PortConfig,
}

impl Transport for Stall {
    fn read(&mut self, _: &mut [u8], _: Duration) -> io::Result<usize> {
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
        let n = buf.len().min(self.capacity);
        self.capacity -= n;
        if n == buf.len() {
            return WriteResult {
                written: n,
                outcome: WriteOutcome::Complete,
            };
        }
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        WriteResult {
            written: n,
            outcome: WriteOutcome::TimedOut,
        }
    }
    fn set_dtr(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_rts(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_break(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }
    fn config(&self) -> &PortConfig {
        &self.cfg
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        no_rx()
    }
}

/// Accepts everything; records writes and control calls.
#[derive(Debug, Default)]
struct Record {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    controls: Arc<Mutex<Vec<ControlOp>>>,
    cfg: PortConfig,
}

impl Transport for Record {
    fn read(&mut self, _: &mut [u8], _: Duration) -> io::Result<usize> {
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _: Instant) -> WriteResult {
        self.writes.lock().expect("lock").push(buf.to_vec());
        WriteResult {
            written: buf.len(),
            outcome: WriteOutcome::Complete,
        }
    }
    fn set_dtr(&mut self, on: bool) -> io::Result<()> {
        self.controls.lock().expect("lock").push(ControlOp::Dtr(on));
        Ok(())
    }
    fn set_rts(&mut self, _: bool) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "no RTS here"))
    }
    fn set_break(&mut self, on: bool) -> io::Result<()> {
        self.controls.lock().expect("lock").push(ControlOp::Break(on));
        Ok(())
    }
    fn config(&self) -> &PortConfig {
        &self.cfg
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        no_rx()
    }
}

fn spawn(t: Box<dyn Transport>) -> (TxActor, Receiver<TxEvent>) {
    let (etx, erx) = event_channel();
    let actor = TxActor::spawn(
        t,
        Arc::new(SeqSource::default()),
        Arc::new(FixedClock::new(1, 2)),
        etx,
    )
    .expect("spawn actor");
    (actor, erx)
}

fn gate() -> (Box<dyn Transport>, Receiver<Vec<u8>>, Sender<()>) {
    let (etx, erx) = mpsc::channel();
    let (rtx, rrx) = mpsc::channel();
    (
        Box::new(Gate {
            entered: etx,
            release: rrx,
            cfg: PortConfig::default(),
        }),
        erx,
        rtx,
    )
}

fn req(id: u64, lane: TxLane, bytes: &[u8], within: Duration) -> TxRequest {
    TxRequest {
        request_id: id,
        lane,
        client_id: format!("c{id}"),
        bytes: bytes.to_vec(),
        deadline: Instant::now() + within,
    }
}

/// Collects events until `TxDone` for every id in `ids` arrived.
fn collect_until_done(erx: &Receiver<TxEvent>, ids: &[u64]) -> Vec<TxEvent> {
    let mut out = Vec::new();
    let mut left: Vec<u64> = ids.to_vec();
    while !left.is_empty() {
        let ev = erx.recv_timeout(WAIT).expect("tx event");
        if let TxEvent::Done(d) = &ev {
            left.retain(|&id| id != d.request_id);
        }
        out.push(ev);
    }
    out
}

fn done_of(events: &[TxEvent], id: u64) -> TxDone {
    events
        .iter()
        .find_map(|e| match e {
            TxEvent::Done(d) if d.request_id == id => Some(d.clone()),
            _ => None,
        })
        .expect("done event")
}

#[test]
fn lane_limits_reject_explicitly() {
    let (t, entered, release) = gate();
    let (actor, _erx) = spawn(t);
    let h = actor.handle();
    let long = Duration::from_secs(10);

    // Agent: one request at the byte limit is accepted (and stays charged while in flight).
    h.submit(req(1, TxLane::Agent, &vec![b'a'; AGENT_QUEUE_BYTES], long))
        .expect("agent at limit");
    entered.recv_timeout(WAIT).expect("actor writing");
    assert!(matches!(
        h.submit(req(2, TxLane::Agent, b"x", long)),
        Err(SubmitError::QueueFull {
            lane: TxLane::Agent,
            ..
        })
    ));
    assert!(matches!(
        h.submit(req(3, TxLane::Agent, &vec![b'a'; AGENT_QUEUE_BYTES + 1], long)),
        Err(SubmitError::TooLarge { .. })
    ));
    assert_eq!(h.queued(TxLane::Agent), (AGENT_QUEUE_BYTES, 1));

    // User: byte limit, then item limit.
    h.submit(req(10, TxLane::User, &vec![b'u'; 4000], long))
        .expect("user 4000");
    assert!(matches!(
        h.submit(req(11, TxLane::User, &[b'u'; 200], long)),
        Err(SubmitError::QueueFull {
            lane: TxLane::User,
            ..
        })
    ));
    for i in 0..(USER_QUEUE_ITEMS - 1) {
        h.submit(req(100 + i as u64, TxLane::User, b"k", long))
            .expect("user key");
    }
    assert!(matches!(
        h.submit(req(999, TxLane::User, b"k", long)),
        Err(SubmitError::QueueFull { .. })
    ));
    assert!(matches!(
        h.submit(req(998, TxLane::User, &vec![b'u'; USER_QUEUE_BYTES + 1], long)),
        Err(SubmitError::TooLarge { .. })
    ));

    // Ctrl-C lane: item limit and byte limit.
    for i in 0..CTRLC_QUEUE_ITEMS {
        h.submit(req(2000 + i as u64, TxLane::CtrlC, b"\x03", long))
            .expect("ctrl-c");
    }
    assert!(matches!(
        h.submit(req(3000, TxLane::CtrlC, b"\x03", long)),
        Err(SubmitError::QueueFull {
            lane: TxLane::CtrlC,
            ..
        })
    ));
    assert!(matches!(
        h.submit(req(3001, TxLane::CtrlC, &[3u8; CTRLC_QUEUE_BYTES + 1], long)),
        Err(SubmitError::TooLarge { .. })
    ));
    assert_eq!(
        h.submit(req(3002, TxLane::Agent, b"", long)),
        Err(SubmitError::Empty)
    );

    drop(release); // unblock the gate so shutdown is immediate
    actor.shutdown();
}

#[test]
fn ctrl_c_cancels_agent_rest_at_chunk_boundary() {
    let (t, entered, release) = gate();
    let (actor, erx) = spawn(t);
    let h = actor.handle();
    let total = 4 * TX_CHUNK_MAX;
    h.submit(req(1, TxLane::Agent, &vec![b'a'; total], WAIT))
        .expect("agent");

    for _ in 0..2 {
        assert_eq!(entered.recv_timeout(WAIT).expect("chunk").len(), TX_CHUNK_MAX);
        release.send(()).expect("release");
    }
    // The actor is now inside the 3rd agent chunk; the user presses Ctrl-C.
    assert_eq!(entered.recv_timeout(WAIT).expect("chunk 3").len(), TX_CHUNK_MAX);
    h.submit(req(2, TxLane::CtrlC, b"\x03", WAIT)).expect("ctrl-c");
    release.send(()).expect("release");
    // Next write at the boundary is the Ctrl-C, not the 4th agent chunk.
    assert_eq!(entered.recv_timeout(WAIT).expect("ctrl-c write"), b"\x03");
    release.send(()).expect("release");

    let events = collect_until_done(&erx, &[1, 2]);
    let agent = done_of(&events, 1);
    assert_eq!(agent.state, TxState::OutcomeUnknown);
    assert_eq!(agent.written_bytes, 3 * TX_CHUNK_MAX as u64);
    assert_eq!(agent.remaining_bytes, TX_CHUNK_MAX as u64);
    assert_eq!(agent.error, Some(TxErrorCode::InterruptedByUser));
    assert_eq!(agent.error.map(|e| e.cause()), Some("interrupted_by_user"));
    assert_eq!(agent.origin, TxOrigin::Agent);
    assert!(agent.first_mono_ns.is_some() && agent.last_mono_ns.is_some());
    let cc = done_of(&events, 2);
    assert_eq!((cc.state, cc.written_bytes), (TxState::Sent, 1));
    assert_eq!(cc.origin, TxOrigin::User);

    // Chunk events: 3 agent chunks, then the Ctrl-C; seqs strictly increasing; agent Done
    // is reported before the Ctrl-C chunk.
    let chunks: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            TxEvent::Chunk(c) => Some(c.clone()),
            TxEvent::Done(_) => None,
        })
        .collect();
    assert_eq!(chunks.len(), 4);
    assert!(chunks.windows(2).all(|w| w[0].source_seq < w[1].source_seq));
    assert_eq!(chunks[3].bytes, b"\x03");
    let pos_agent_done = events
        .iter()
        .position(|e| matches!(e, TxEvent::Done(d) if d.request_id == 1));
    let pos_cc_chunk = events
        .iter()
        .position(|e| matches!(e, TxEvent::Chunk(c) if c.request_id == 2));
    assert!(pos_agent_done < pos_cc_chunk);

    // Nothing else is written.
    assert!(entered.recv_timeout(Duration::from_millis(50)).is_err());
    drop(release);
    actor.shutdown();
}

#[test]
fn user_keys_interleave_without_cancelling_agent() {
    let (t, entered, release) = gate();
    let (actor, erx) = spawn(t);
    let h = actor.handle();
    h.submit(req(1, TxLane::Agent, &vec![b'a'; 3 * TX_CHUNK_MAX], WAIT))
        .expect("agent");
    entered.recv_timeout(WAIT).expect("chunk 1");
    release.send(()).expect("release");
    entered.recv_timeout(WAIT).expect("chunk 2");
    h.submit(req(2, TxLane::User, b"x", WAIT)).expect("user key");
    release.send(()).expect("release");
    assert_eq!(entered.recv_timeout(WAIT).expect("user write"), b"x");
    release.send(()).expect("release");
    assert_eq!(
        entered.recv_timeout(WAIT).expect("agent chunk 3").len(),
        TX_CHUNK_MAX
    );
    release.send(()).expect("release");

    let events = collect_until_done(&erx, &[1, 2]);
    let agent = done_of(&events, 1);
    assert_eq!(agent.state, TxState::Sent);
    assert_eq!(agent.written_bytes, 3 * TX_CHUNK_MAX as u64);
    assert_eq!(agent.error, None);
    assert_eq!(done_of(&events, 2).state, TxState::Sent);
    drop(release);
    actor.shutdown();
}

#[test]
fn blocked_transport_partial_is_outcome_unknown() {
    let (actor, erx) = spawn(Box::new(Stall {
        capacity: 1500,
        cfg: PortConfig::default(),
    }));
    let h = actor.handle();
    let started = Instant::now();
    h.submit(req(1, TxLane::Agent, &[b'z'; 3000], Duration::from_millis(150)))
        .expect("agent");
    let events = collect_until_done(&erx, &[1]);
    let elapsed = started.elapsed();
    let d = done_of(&events, 1);
    assert_eq!(d.state, TxState::OutcomeUnknown);
    assert_eq!((d.written_bytes, d.remaining_bytes), (1500, 1500));
    assert_eq!(d.error, Some(TxErrorCode::Timeout));
    assert!(elapsed >= Duration::from_millis(140), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(1), "deadline overrun {elapsed:?}");
    actor.shutdown();
}

#[test]
fn blocked_transport_nothing_written_is_failed_timeout() {
    let (actor, erx) = spawn(Box::new(Stall {
        capacity: 0,
        cfg: PortConfig::default(),
    }));
    let h = actor.handle();
    h.submit(req(1, TxLane::Agent, b"hello", Duration::from_millis(50)))
        .expect("agent");
    // Already-expired request: failed without any write attempt.
    h.submit(req(2, TxLane::Agent, b"late", Duration::ZERO))
        .expect("agent");
    let events = collect_until_done(&erx, &[1, 2]);
    for id in [1, 2] {
        let d = done_of(&events, id);
        assert_eq!(d.state, TxState::Failed);
        assert_eq!(d.written_bytes, 0);
        assert_eq!(d.error.map(|e| e.code()), Some("E_TIMEOUT"));
    }
    assert!(!events.iter().any(|e| matches!(e, TxEvent::Chunk(_))));
    assert_eq!(actor.handle().queued(TxLane::Agent), (0, 0));
    actor.shutdown();
}

#[test]
fn same_lane_requests_keep_fifo_order() {
    let rec = Record::default();
    let writes = Arc::clone(&rec.writes);
    let (actor, erx) = spawn(Box::new(rec));
    let h = actor.handle();
    let mut ids = Vec::new();
    for i in 0..10u64 {
        let lane = if i % 3 == 0 { TxLane::User } else { TxLane::Agent };
        h.submit(req(i, lane, format!("<{i}>").as_bytes(), WAIT))
            .expect("submit");
        ids.push(i);
    }
    let events = collect_until_done(&erx, &ids);
    let log: Vec<String> = writes
        .lock()
        .expect("lock")
        .iter()
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .collect();
    let order = |pred: fn(u64) -> bool| -> Vec<u64> {
        log.iter()
            .filter_map(|s| s.trim_matches(['<', '>']).parse::<u64>().ok())
            .filter(|&i| pred(i))
            .collect()
    };
    assert_eq!(order(|i| i % 3 == 0), vec![0, 3, 6, 9]);
    assert_eq!(order(|i| i % 3 != 0), vec![1, 2, 4, 5, 7, 8]);
    assert!(events.iter().all(|e| match e {
        TxEvent::Done(d) => d.state == TxState::Sent,
        TxEvent::Chunk(_) => true,
    }));
    actor.shutdown();
}

#[test]
fn control_ops_run_on_actor_and_report_errors() {
    let rec = Record::default();
    let controls = Arc::clone(&rec.controls);
    let (actor, _erx) = spawn(Box::new(rec));
    let h = actor.handle();
    h.control(ControlOp::Dtr(false), WAIT).expect("dtr");
    h.control(ControlOp::Break(true), WAIT).expect("break");
    let e = h
        .control(ControlOp::Rts(true), WAIT)
        .expect_err("rts error is returned");
    assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    assert_eq!(
        *controls.lock().expect("lock"),
        vec![ControlOp::Dtr(false), ControlOp::Break(true)]
    );
    actor.shutdown();
}

#[test]
fn shutdown_reports_every_pending_request() {
    let (t, entered, release) = gate();
    let (actor, erx) = spawn(t);
    let h = actor.handle();
    h.submit(req(1, TxLane::Agent, &[b'a'; 10], WAIT)).expect("1");
    entered.recv_timeout(WAIT).expect("in flight");
    h.submit(req(2, TxLane::Agent, &[b'b'; 10], WAIT)).expect("2");
    h.submit(req(3, TxLane::User, b"c", WAIT)).expect("3");
    // Shut down while request 1 is blocked; release it afterwards from another thread.
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let _ = release.send(());
        // Let any further writes (user key) through as well.
        for _ in 0..4 {
            let _ = release.send(());
        }
    });
    actor.shutdown();
    releaser.join().expect("releaser");
    let events: Vec<TxEvent> = erx.try_iter().collect();
    let mut done: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            TxEvent::Done(d) => Some(d.request_id),
            TxEvent::Chunk(_) => None,
        })
        .collect();
    done.sort_unstable();
    assert_eq!(done, vec![1, 2, 3], "every accepted request gets a TxDone");
    assert_eq!(
        h.submit(req(4, TxLane::User, b"d", WAIT)),
        Err(SubmitError::Stopped)
    );
}

#[test]
fn agent_request_count_limit() {
    let (t, entered, release) = gate();
    let (actor, _erx) = spawn(t);
    let h = actor.handle();
    let long = Duration::from_secs(10);
    for i in 0..AGENT_QUEUE_ITEMS as u64 {
        h.submit(req(i, TxLane::Agent, b"a", long))
            .expect("agent request within limit");
    }
    entered.recv_timeout(WAIT).expect("actor writing");
    assert!(matches!(
        h.submit(req(1000, TxLane::Agent, b"a", long)),
        Err(SubmitError::QueueFull {
            lane: TxLane::Agent,
            queued_items: AGENT_QUEUE_ITEMS,
            ..
        })
    ));
    drop(release);
    actor.shutdown();
}
