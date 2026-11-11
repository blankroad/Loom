//! Deterministic cancellation at transport boundaries; no real UART and no sleeps.

use std::collections::BTreeMap;
use std::io;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use loom_uart_worker::clock::{FixedClock, SeqSource};
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::tx::{
    AGENT_QUEUE_BYTES, AGENT_QUEUE_ITEMS, CTRLC_QUEUE_BYTES, CTRLC_QUEUE_ITEMS, CancelResult, ControlOp,
    SubmitError, TX_CANCEL_QUEUE_BYTES, TX_CANCEL_QUEUE_ITEMS, TX_CHUNK_MAX, TxActor, TxDone, TxErrorCode,
    TxEvent, TxHandle, TxLane, TxRequest, TxState, USER_QUEUE_BYTES, USER_QUEUE_ITEMS, event_channel,
};

const WAIT: Duration = Duration::from_secs(2);
// Test-only fixed mailboxes; a single actor cannot have more than one driver call in flight.
const GATE_ITEMS: usize = 4;

#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    Write(Vec<u8>),
    Control,
}

#[derive(Debug)]
enum Release {
    All,
    PartialTimeout(usize),
}

#[derive(Debug)]
struct GatedTransport {
    entered: SyncSender<Attempt>,
    releases: Receiver<Release>,
    cfg: PortConfig,
}

impl Transport for GatedTransport {
    fn read(&mut self, _: &mut [u8], _: Duration) -> io::Result<usize> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
        let _ = self.entered.send(Attempt::Write(buf.to_vec()));
        match self
            .releases
            .recv_timeout(deadline.saturating_duration_since(Instant::now()).min(WAIT))
        {
            Ok(Release::All) => WriteResult {
                written: buf.len(),
                outcome: WriteOutcome::Complete,
            },
            Ok(Release::PartialTimeout(bytes)) => WriteResult {
                written: bytes.min(buf.len()),
                outcome: WriteOutcome::TimedOut,
            },
            Err(_) => WriteResult {
                written: 0,
                outcome: WriteOutcome::TimedOut,
            },
        }
    }

    fn set_dtr(&mut self, _: bool) -> io::Result<()> {
        let _ = self.entered.send(Attempt::Control);
        self.releases
            .recv_timeout(WAIT)
            .map(|_| ())
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "test control gate"))
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
        Err(io::Error::new(io::ErrorKind::Unsupported, "TX-only fake"))
    }
}

struct Harness {
    actor: Option<TxActor>,
    handle: TxHandle,
    events: Receiver<TxEvent>,
    entered: Receiver<Attempt>,
    release: Option<SyncSender<Release>>,
}

impl Harness {
    fn new() -> Self {
        let (entered, attempts) = sync_channel(GATE_ITEMS);
        let (release, releases) = sync_channel(GATE_ITEMS);
        let (events, receiver) = event_channel();
        let actor = TxActor::spawn(
            Box::new(GatedTransport {
                entered,
                releases,
                cfg: PortConfig::default(),
            }),
            Arc::new(SeqSource::default()),
            Arc::new(FixedClock::new(1, 2)),
            events,
        )
        .expect("spawn gated TX actor");
        Self {
            handle: actor.handle(),
            actor: Some(actor),
            events: receiver,
            entered: attempts,
            release: Some(release),
        }
    }

    fn submit(&self, id: u64, lane: TxLane, bytes: &[u8]) {
        self.handle
            .submit(request(id, lane, bytes))
            .expect("test request admitted");
    }

    fn write(&self) -> Vec<u8> {
        match self.entered.recv_timeout(WAIT).expect("driver call must arrive") {
            Attempt::Write(bytes) => bytes,
            Attempt::Control => panic!("expected driver write, got control"),
        }
    }

    fn accept(&self) {
        self.release
            .as_ref()
            .expect("gate is live")
            .send(Release::All)
            .expect("release driver call");
    }

    fn collect(&self, ids: &[u64]) -> Vec<TxEvent> {
        let deadline = Instant::now() + WAIT;
        let mut events = Vec::new();
        let mut pending = ids.to_vec();
        while !pending.is_empty() {
            let event = self
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("all accepted requests must complete");
            if let TxEvent::Done(done) = &event {
                pending.retain(|id| *id != done.request_id);
            }
            events.push(event);
        }
        events
    }

    fn shutdown(&mut self) {
        self.release.take(); // Also unblocks a failed assertion's fake write without sleeping.
        if let Some(actor) = self.actor.take() {
            actor.shutdown();
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn request(id: u64, lane: TxLane, bytes: &[u8]) -> TxRequest {
    TxRequest {
        request_id: id,
        lane,
        client_id: format!("client-{id}"),
        bytes: bytes.to_vec(),
        deadline: Instant::now() + WAIT,
    }
}

fn completions(events: &[TxEvent], ids: &[u64]) -> BTreeMap<u64, TxDone> {
    let mut done = BTreeMap::new();
    for event in events {
        if let TxEvent::Done(result) = event {
            assert!(
                done.insert(result.request_id, result.clone()).is_none(),
                "duplicate terminal for {}",
                result.request_id
            );
        }
    }
    assert_eq!(done.len(), ids.len(), "one Done per accepted request");
    for id in ids {
        let result = done.get(id).expect("completion must exist");
        let written: usize = events
            .iter()
            .filter_map(|event| match event {
                TxEvent::Chunk(chunk) if chunk.request_id == *id => {
                    assert!(chunk.bytes.len() <= TX_CHUNK_MAX);
                    Some(chunk.bytes.len())
                }
                _ => None,
            })
            .sum();
        assert_eq!(
            result.written_bytes, written as u64,
            "count only driver-accepted bytes"
        );
    }
    done
}

#[test]
fn queued_cancel_removes_request_and_preserves_other_agent_fifo() {
    for cause in ["interrupted_by_user", "stopped", "held_by_user", "closed"] {
        let mut h = Harness::new();
        h.submit(1, TxLane::User, b"u");
        assert_eq!(h.write(), b"u");
        h.submit(2, TxLane::Agent, b"a");
        h.submit(3, TxLane::Agent, b"never written");
        h.submit(4, TxLane::Agent, b"c");
        assert_eq!(h.handle.cancel(3, cause), CancelResult::Queued);
        assert_eq!(h.handle.cancel(3, "closed"), CancelResult::AlreadyRequested);
        h.accept();
        assert_eq!(h.write(), b"a");
        h.accept();
        assert_eq!(h.write(), b"c");
        h.accept();
        let mut events = h.collect(&[1, 2, 3, 4]);
        h.shutdown();
        events.extend(h.events.try_iter());
        let done = completions(&events, &[1, 2, 3, 4]);
        assert_eq!(done[&3].state, TxState::Failed);
        assert_eq!((done[&3].written_bytes, done[&3].remaining_bytes), (0, 13));
        assert_eq!(done[&3].cause, Some(cause));
        assert_eq!(done[&3].first_mono_ns, None);
        assert_eq!(h.handle.queued(TxLane::Agent), (0, 0));
        assert_eq!(h.handle.cancel(3, cause), CancelResult::NotFound);
    }
}

#[test]
fn active_cancel_counts_partial_driver_progress_exactly() {
    let mut h = Harness::new();
    let total = TX_CHUNK_MAX * 3;
    h.submit(1, TxLane::Agent, &vec![b'a'; total]);
    assert_eq!(h.write().len(), TX_CHUNK_MAX);
    h.accept();
    assert_eq!(h.write().len(), TX_CHUNK_MAX);
    assert_eq!(
        h.handle.cancel(1, "interrupted_by_user"),
        CancelResult::InProgress
    );
    h.release
        .as_ref()
        .expect("gate")
        .send(Release::PartialTimeout(7))
        .expect("accept seven bytes");
    let mut events = h.collect(&[1]);
    h.shutdown();
    events.extend(h.events.try_iter());
    let done = completions(&events, &[1]);
    assert_eq!(done[&1].state, TxState::OutcomeUnknown);
    assert_eq!(done[&1].written_bytes, (TX_CHUNK_MAX + 7) as u64);
    assert_eq!(done[&1].remaining_bytes, (total - TX_CHUNK_MAX - 7) as u64);
    assert_eq!(done[&1].error, Some(TxErrorCode::InterruptedByUser));
    assert_eq!(done[&1].cause, Some("interrupted_by_user"));
    assert!(h.entered.try_recv().is_err(), "no third chunk after cancellation");
}

#[test]
fn active_cancel_is_conservative_with_zero_or_all_bytes_accepted() {
    for accepted in [0, 1] {
        let mut h = Harness::new();
        h.submit(1, TxLane::Agent, b"x");
        assert_eq!(h.write(), b"x");
        assert_eq!(h.handle.cancel(1, "held_by_user"), CancelResult::InProgress);
        if accepted == 0 {
            h.release
                .as_ref()
                .expect("gate")
                .send(Release::PartialTimeout(0))
                .expect("zero bytes");
        } else {
            h.accept();
        }
        let mut events = h.collect(&[1]);
        h.shutdown();
        events.extend(h.events.try_iter());
        let done = completions(&events, &[1]);
        assert_eq!(done[&1].state, TxState::OutcomeUnknown);
        assert_eq!(done[&1].written_bytes, accepted);
        assert_eq!(done[&1].remaining_bytes, 1 - accepted);
        assert_eq!(done[&1].cause, Some("held_by_user"));
    }
}

#[test]
fn unknown_completed_and_invalid_cancellations_create_no_terminal_events() {
    let mut h = Harness::new();
    assert_eq!(h.handle.cancel(999, "closed"), CancelResult::NotFound);
    assert_eq!(h.handle.cancel(999, "STOPPED"), CancelResult::InvalidCause);
    assert_eq!(h.handle.cancel_lane(TxLane::Agent, "invalid"), 0);
    h.submit(1, TxLane::Agent, b"x");
    assert_eq!(h.write(), b"x");
    assert_eq!(
        h.handle.cancel(1, "x".repeat(1024).as_str()),
        CancelResult::InvalidCause
    );
    h.accept();
    let mut events = h.collect(&[1]);
    assert_eq!(h.handle.cancel(1, "stopped"), CancelResult::NotFound);
    h.shutdown();
    events.extend(h.events.try_iter());
    let done = completions(&events, &[1]);
    assert_eq!(done[&1].state, TxState::Sent);
    assert_eq!(done[&1].cause, None);
}

#[test]
fn cancel_agent_lane_preserves_user_and_ctrl_c_lanes() {
    let mut h = Harness::new();
    h.submit(1, TxLane::Agent, &vec![b'a'; 3 * TX_CHUNK_MAX]);
    assert_eq!(h.write().len(), TX_CHUNK_MAX);
    h.submit(2, TxLane::Agent, b"not sent");
    h.submit(3, TxLane::User, b"key");
    h.submit(4, TxLane::CtrlC, b"\x03");
    assert_eq!(h.handle.cancel_lane(TxLane::Agent, "stopped"), 2);
    assert_eq!(h.handle.cancel_lane(TxLane::Agent, "closed"), 0);
    h.accept();
    assert_eq!(h.write(), b"\x03");
    h.accept();
    assert_eq!(h.write(), b"key");
    h.accept();
    let mut events = h.collect(&[1, 2, 3, 4]);
    h.shutdown();
    events.extend(h.events.try_iter());
    let done = completions(&events, &[1, 2, 3, 4]);
    assert_eq!(done[&1].state, TxState::OutcomeUnknown);
    assert_eq!(done[&1].written_bytes, TX_CHUNK_MAX as u64);
    assert_eq!(done[&1].cause, Some("stopped"));
    assert_eq!(done[&2].state, TxState::Failed);
    assert_eq!(done[&2].cause, Some("stopped"));
    assert_eq!(done[&3].state, TxState::Sent);
    assert_eq!(done[&4].state, TxState::Sent);
    assert_eq!(h.handle.queued(TxLane::Agent), (0, 0));
}

#[test]
fn cancelled_agent_completes_while_user_lane_remains_active() {
    let mut h = Harness::new();
    h.submit(1, TxLane::Agent, &vec![b'a'; 3 * TX_CHUNK_MAX]);
    h.write();
    h.submit(2, TxLane::User, &vec![b'u'; 3 * TX_CHUNK_MAX]);
    assert_eq!(h.handle.cancel(1, "held_by_user"), CancelResult::InProgress);
    h.accept();
    assert_eq!(h.write(), vec![b'u'; TX_CHUNK_MAX]);
    let mut events: Vec<_> = h.events.try_iter().collect();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, TxEvent::Done(done) if done.request_id == 1)),
        "agent cancellation cannot wait until all user chunks drain"
    );
    h.accept();
    for _ in 0..2 {
        assert_eq!(h.write(), vec![b'u'; TX_CHUNK_MAX]);
        h.accept();
    }
    events.extend(h.collect(&[2]));
    h.shutdown();
    events.extend(h.events.try_iter());
    let done = completions(&events, &[1, 2]);
    assert_eq!(done[&1].written_bytes, TX_CHUNK_MAX as u64);
    assert_eq!(done[&1].cause, Some("held_by_user"));
    assert_eq!(done[&2].state, TxState::Sent);
}

#[test]
fn ctrl_c_write_is_not_delayed_by_a_batch_of_cancellation_completions() {
    let mut h = Harness::new();
    h.submit(1, TxLane::User, b"barrier");
    assert_eq!(h.write(), b"barrier");
    for id in 10..10 + AGENT_QUEUE_ITEMS as u64 {
        h.submit(id, TxLane::Agent, b"x");
    }
    assert_eq!(h.handle.cancel_lane(TxLane::Agent, "stopped"), AGENT_QUEUE_ITEMS);
    h.submit(2, TxLane::CtrlC, b"\x03");
    h.accept();
    assert_eq!(h.write(), b"\x03", "Ctrl-C wins over cancelled terminals");
    let mut events: Vec<_> = h.events.try_iter().collect();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TxEvent::Done(done) if done.lane == TxLane::Agent))
    );
    h.accept();
    let ids: Vec<_> = std::iter::once(2)
        .chain(10..10 + AGENT_QUEUE_ITEMS as u64)
        .collect();
    events.extend(h.collect(&ids));
    h.shutdown();
    events.extend(h.events.try_iter());
    let all_ids: Vec<_> = std::iter::once(1).chain(ids).collect();
    completions(&events, &all_ids);
}

#[test]
fn repeated_cancel_and_ctrl_c_races_emit_one_done_per_request() {
    for _ in 0..32 {
        let mut h = Harness::new();
        h.submit(1, TxLane::Agent, &vec![b'a'; 2 * TX_CHUNK_MAX]);
        h.write();
        let barrier = Arc::new(Barrier::new(3));
        let threads: Vec<_> = ["stopped", "held_by_user"]
            .into_iter()
            .map(|cause| {
                let handle = h.handle.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    (cause, handle.cancel(1, cause))
                })
            })
            .collect();
        h.submit(2, TxLane::CtrlC, b"\x03");
        barrier.wait();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().expect("cancel thread"))
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|(_, result)| *result == CancelResult::InProgress)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|(_, result)| *result == CancelResult::AlreadyRequested)
                .count(),
            1
        );
        let winner = results
            .iter()
            .find(|(_, result)| *result == CancelResult::InProgress)
            .expect("one winning claim")
            .0;
        h.accept();
        assert_eq!(h.write(), b"\x03");
        h.accept();
        let mut events = h.collect(&[1, 2]);
        h.shutdown();
        events.extend(h.events.try_iter());
        let done = completions(&events, &[1, 2]);
        assert_eq!(done[&1].state, TxState::OutcomeUnknown);
        assert_eq!(done[&1].written_bytes, TX_CHUNK_MAX as u64);
        assert_eq!(done[&1].cause, Some(winner));
        assert_eq!(done[&2].state, TxState::Sent);
    }
}

#[test]
fn cancellation_racing_the_final_write_has_one_linearized_outcome() {
    for _ in 0..64 {
        let mut h = Harness::new();
        h.submit(1, TxLane::Agent, b"x");
        h.write();
        let barrier = Arc::new(Barrier::new(2));
        let handle = h.handle.clone();
        let ready = Arc::clone(&barrier);
        let cancel = std::thread::spawn(move || {
            ready.wait();
            handle.cancel(1, "closed")
        });
        barrier.wait();
        h.accept();
        let result = cancel.join().expect("cancel thread");
        let mut events = h.collect(&[1]);
        h.shutdown();
        events.extend(h.events.try_iter());
        let done = completions(&events, &[1]);
        match result {
            CancelResult::InProgress => {
                assert_eq!(done[&1].state, TxState::OutcomeUnknown);
                assert_eq!(done[&1].cause, Some("closed"));
            }
            CancelResult::NotFound => {
                assert_eq!(done[&1].state, TxState::Sent);
                assert_eq!(done[&1].cause, None);
            }
            other => panic!("unexpected race result: {other:?}"),
        }
        assert_eq!(done[&1].written_bytes, 1);
    }
}

#[test]
fn ctrl_c_boundary_racing_an_explicit_cancel_has_one_terminal() {
    for _ in 0..64 {
        let mut h = Harness::new();
        h.submit(1, TxLane::Agent, &vec![b'a'; 2 * TX_CHUNK_MAX]);
        h.write();
        let barrier = Arc::new(Barrier::new(3));
        let handle = h.handle.clone();
        let ready = Arc::clone(&barrier);
        let cancel = std::thread::spawn(move || {
            ready.wait();
            handle.cancel(1, "stopped")
        });
        let handle = h.handle.clone();
        let ready = Arc::clone(&barrier);
        let release = h.release.as_ref().expect("gate").clone();
        let interrupt = std::thread::spawn(move || {
            ready.wait();
            handle
                .submit(request(2, TxLane::CtrlC, b"\x03"))
                .expect("Ctrl-C admission");
            release.send(Release::All).expect("release agent chunk");
        });
        barrier.wait();
        interrupt.join().expect("interrupt thread");
        let result = cancel.join().expect("cancel thread");
        assert_eq!(h.write(), b"\x03");
        h.accept();
        let mut events = h.collect(&[1, 2]);
        h.shutdown();
        events.extend(h.events.try_iter());
        let done = completions(&events, &[1, 2]);
        assert_eq!(done[&1].state, TxState::OutcomeUnknown);
        assert_eq!(done[&1].written_bytes, TX_CHUNK_MAX as u64);
        match result {
            CancelResult::InProgress => assert_eq!(done[&1].cause, Some("stopped")),
            CancelResult::NotFound => assert_eq!(done[&1].cause, Some("interrupted_by_user")),
            other => panic!("unexpected cancellation race: {other:?}"),
        }
        assert_eq!(done[&2].state, TxState::Sent);
    }
}

#[test]
fn cancellation_mailbox_keeps_lane_permits_and_coalesces_duplicate_claims() {
    let mut h = Harness::new();
    let handle = h.handle.clone();
    let control = std::thread::spawn(move || handle.control(ControlOp::Dtr(false), WAIT));
    assert_eq!(
        h.entered.recv_timeout(WAIT).expect("control barrier"),
        Attempt::Control
    );
    let mut ids = Vec::new();
    for (lane, base, count, bytes) in [
        (
            TxLane::Agent,
            1000,
            AGENT_QUEUE_ITEMS,
            AGENT_QUEUE_BYTES / AGENT_QUEUE_ITEMS,
        ),
        (
            TxLane::User,
            2000,
            USER_QUEUE_ITEMS,
            USER_QUEUE_BYTES / USER_QUEUE_ITEMS,
        ),
        (
            TxLane::CtrlC,
            3000,
            CTRLC_QUEUE_ITEMS,
            CTRLC_QUEUE_BYTES / CTRLC_QUEUE_ITEMS,
        ),
    ] {
        for id in base..base + count as u64 {
            h.submit(id, lane, &vec![b'x'; bytes]);
            ids.push(id);
        }
        assert_eq!(h.handle.cancel_lane(lane, "closed"), count);
        assert_eq!(h.handle.queued(lane), lane.limits());
    }
    assert_eq!(ids.len(), TX_CANCEL_QUEUE_ITEMS);
    assert_eq!(
        TX_CANCEL_QUEUE_BYTES,
        AGENT_QUEUE_BYTES + USER_QUEUE_BYTES + CTRLC_QUEUE_BYTES
    );
    for _ in 0..100 {
        assert_eq!(h.handle.cancel(1000, "stopped"), CancelResult::AlreadyRequested);
    }
    assert!(matches!(
        h.handle.submit(request(9999, TxLane::Agent, b"x")),
        Err(SubmitError::QueueFull { .. })
    ));
    h.accept();
    control.join().expect("control thread").expect("control result");
    let mut events = h.collect(&ids);
    h.shutdown();
    events.extend(h.events.try_iter());
    let done = completions(&events, &ids);
    assert!(!events.iter().any(|event| matches!(event, TxEvent::Chunk(_))));
    assert!(
        done.values()
            .all(|done| done.state == TxState::Failed && done.cause == Some("closed"))
    );
    for lane in [TxLane::Agent, TxLane::User, TxLane::CtrlC] {
        assert_eq!(h.handle.queued(lane), (0, 0));
    }
}

#[test]
fn shutdown_preserves_already_accepted_cancellation_causes() {
    let mut h = Harness::new();
    h.submit(1, TxLane::Agent, b"active");
    h.write();
    h.submit(2, TxLane::Agent, b"queued");
    assert_eq!(h.handle.cancel(1, "closed"), CancelResult::InProgress);
    assert_eq!(h.handle.cancel(2, "held_by_user"), CancelResult::Queued);
    h.shutdown();
    let events: Vec<_> = h.events.try_iter().collect();
    let done = completions(&events, &[1, 2]);
    assert_eq!(done[&1].cause, Some("closed"));
    assert_eq!(done[&2].cause, Some("held_by_user"));
    assert_eq!(h.handle.cancel(1, "stopped"), CancelResult::NotFound);
    assert_eq!(h.handle.cancel_lane(TxLane::Agent, "stopped"), 0);
}
