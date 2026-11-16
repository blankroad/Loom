//! B2 (operator-upgrade U3): transmit pacing in the TX actor.
//! Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B2.1 (lead-written tests).
//!
//! Timing assertions use lower bounds with a small scheduler tolerance and generous upper
//! bounds; every wait is bounded (≤ 3 s).

use std::cell::RefCell;
use std::io;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use loom_uart_worker::clock::{FixedClock, SeqSource};
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::tx::{
    CancelResult, ControlOp, PACING_CHAR_MAX, PACING_LINE_MAX, Pacing, TX_CHUNK_MAX, TxActor, TxDone,
    TxErrorCode, TxEvent, TxLane, TxRequest, TxState, event_channel, gap_after, is_line_end, paced_chunk_end,
    pacing_estimate, trailing_gap,
};

const WAIT: Duration = Duration::from_secs(3);
/// Scheduler tolerance subtracted from lower bounds.
const SLACK: Duration = Duration::from_millis(5);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn pacing(char_ms: u64, line_ms: u64) -> Pacing {
    Pacing {
        char_delay: ms(char_ms),
        line_delay: ms(line_ms),
    }
}

// ---------------------------------------------------------------- pure functions

#[test]
fn constants_match_the_u3_ranges() {
    assert_eq!(PACING_CHAR_MAX, ms(1000));
    assert_eq!(PACING_LINE_MAX, ms(10_000));
    assert_eq!(Pacing::default(), Pacing::NONE);
}

#[test]
fn is_none_only_when_both_delays_are_zero() {
    assert!(Pacing::NONE.is_none());
    assert!(!pacing(1, 0).is_none());
    assert!(!pacing(0, 1).is_none());
    assert!(!pacing(5, 7).is_none());
}

#[test]
fn line_ends_are_lf_or_a_lone_cr() {
    let b = b"a\r\nb\rc\n\r";
    let ends: Vec<usize> = (0..b.len()).filter(|&i| is_line_end(b, i)).collect();
    // `\r` before `\n` is not a line end; the lone `\r` (4), `\n` (2, 6) and a final `\r` (7) are.
    assert_eq!(ends, vec![2, 4, 6, 7]);
    assert!(!is_line_end(b, b.len()), "out of range is never a line end");
    assert!(!is_line_end(b"", 0));
    assert!(is_line_end(b"\r", 0), "a CR at the last index counts");
}

#[test]
fn unpaced_chunks_are_unchanged() {
    let b = vec![b'x'; 3000];
    assert_eq!(paced_chunk_end(&b, 0, Pacing::NONE), TX_CHUNK_MAX);
    assert_eq!(paced_chunk_end(&b, TX_CHUNK_MAX, Pacing::NONE), 2 * TX_CHUNK_MAX);
    assert_eq!(paced_chunk_end(&b, 2 * TX_CHUNK_MAX, Pacing::NONE), 3000);
    // Line ends do not cut unpaced chunks.
    assert_eq!(paced_chunk_end(b"ab\ncd\n", 0, Pacing::NONE), 6);
}

#[test]
fn char_pacing_cuts_one_byte_per_chunk() {
    let b = b"ab\r\n";
    for start in 0..b.len() {
        assert_eq!(paced_chunk_end(b, start, pacing(10, 0)), start + 1);
        assert_eq!(paced_chunk_end(b, start, pacing(10, 50)), start + 1);
    }
}

#[test]
fn line_pacing_cuts_after_each_line_end_and_caps_long_lines() {
    let p = pacing(0, 100);
    let b = b"ab\r\ncd\rxy";
    assert_eq!(paced_chunk_end(b, 0, p), 4, "CRLF stays in one chunk");
    assert_eq!(paced_chunk_end(b, 4, p), 7, "lone CR ends a line");
    assert_eq!(paced_chunk_end(b, 7, p), 9, "unterminated tail");
    let long = vec![b'z'; 3000];
    assert_eq!(paced_chunk_end(&long, 0, p), TX_CHUNK_MAX);
    assert_eq!(paced_chunk_end(&long, TX_CHUNK_MAX, p), 2 * TX_CHUNK_MAX);
}

#[test]
fn gap_after_adds_line_delay_only_at_line_ends() {
    let b = b"ab\r\ncd";
    let p = pacing(10, 100);
    assert_eq!(gap_after(b, 1, p), ms(10));
    assert_eq!(gap_after(b, 3, p), ms(10), "CR before LF is not a line end");
    assert_eq!(gap_after(b, 4, p), ms(110));
    assert_eq!(gap_after(b, 4, pacing(0, 100)), ms(100));
    assert_eq!(gap_after(b, 2, pacing(0, 100)), Duration::ZERO);
    assert_eq!(gap_after(b, 4, Pacing::NONE), Duration::ZERO);
}

#[test]
fn estimate_excludes_and_trailing_gap_is_the_last_gap() {
    let b = b"ab\r\ncd\r\n";
    // char 10 / line 100: 7 inner gaps of 10 ms, one inner line end (+100); trailing 10 + 100.
    assert_eq!(pacing_estimate(b, pacing(10, 100)), ms(170));
    assert_eq!(trailing_gap(b, pacing(10, 100)), ms(110));
    // line only: one inner line gap; trailing line gap.
    assert_eq!(pacing_estimate(b, pacing(0, 100)), ms(100));
    assert_eq!(trailing_gap(b, pacing(0, 100)), ms(100));
    // single keystroke: no inner gap.
    assert_eq!(pacing_estimate(b"k", pacing(50, 0)), Duration::ZERO);
    assert_eq!(trailing_gap(b"k", pacing(50, 0)), ms(50));
    // unpaced and empty.
    assert_eq!(pacing_estimate(b, Pacing::NONE), Duration::ZERO);
    assert_eq!(trailing_gap(b, Pacing::NONE), Duration::ZERO);
    assert_eq!(pacing_estimate(b"", pacing(10, 10)), Duration::ZERO);
    assert_eq!(trailing_gap(b"", pacing(10, 10)), Duration::ZERO);
}

#[test]
fn estimate_of_a_full_agent_lane_is_bounded_and_exact() {
    let b = vec![b'q'; 64 * 1024];
    let max = Pacing {
        char_delay: PACING_CHAR_MAX,
        line_delay: PACING_LINE_MAX,
    };
    // No line ends: one char gap between each pair of bytes.
    assert_eq!(pacing_estimate(&b, max), PACING_CHAR_MAX * (64 * 1024 - 1));
    assert_eq!(trailing_gap(&b, max), PACING_CHAR_MAX);
}

// ---------------------------------------------------------------- actor rigs

#[derive(Debug)]
struct Silent;
impl RxHandle for Silent {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(ms(20)));
        Ok(0)
    }
}

/// `(entry time, bytes)` of every accepted write.
type Writes = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

/// Accepts every write whole; records `(entry time, bytes)` and control ops.
#[derive(Debug, Default, Clone)]
struct Log {
    writes: Writes,
    controls: Arc<Mutex<Vec<ControlOp>>>,
}

impl Log {
    fn writes(&self) -> Vec<(Instant, Vec<u8>)> {
        self.writes.lock().expect("log").clone()
    }
    fn bytes(&self) -> Vec<u8> {
        self.writes().into_iter().flat_map(|(_, b)| b).collect()
    }
    /// Wait until `n` bytes were written (≤ WAIT).
    fn wait_bytes(&self, n: usize) -> bool {
        let end = Instant::now() + WAIT;
        while Instant::now() < end {
            if self.bytes().len() >= n {
                return true;
            }
            thread::sleep(ms(1));
        }
        false
    }
}

#[derive(Debug)]
struct Rec {
    log: Log,
    cfg: PortConfig,
}

impl Transport for Rec {
    fn read(&mut self, _: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(ms(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _: Instant) -> WriteResult {
        self.log
            .writes
            .lock()
            .expect("log")
            .push((Instant::now(), buf.to_vec()));
        WriteResult {
            written: buf.len(),
            outcome: WriteOutcome::Complete,
        }
    }
    fn set_dtr(&mut self, on: bool) -> io::Result<()> {
        self.log.controls.lock().expect("log").push(ControlOp::Dtr(on));
        Ok(())
    }
    fn set_rts(&mut self, on: bool) -> io::Result<()> {
        self.log.controls.lock().expect("log").push(ControlOp::Rts(on));
        Ok(())
    }
    fn set_break(&mut self, on: bool) -> io::Result<()> {
        self.log.controls.lock().expect("log").push(ControlOp::Break(on));
        Ok(())
    }
    fn config(&self) -> &PortConfig {
        &self.cfg
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        Ok(Box::new(Silent))
    }
}

/// Each write announces itself and blocks until released (or its deadline).
#[derive(Debug)]
struct Gate {
    log: Log,
    entered: Sender<()>,
    release: Receiver<()>,
    cfg: PortConfig,
}

impl Transport for Gate {
    fn read(&mut self, _: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(ms(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
        let _ = self.entered.send(());
        let left = deadline.saturating_duration_since(Instant::now());
        match self.release.recv_timeout(left) {
            Ok(()) => {
                self.log
                    .writes
                    .lock()
                    .expect("log")
                    .push((Instant::now(), buf.to_vec()));
                WriteResult {
                    written: buf.len(),
                    outcome: WriteOutcome::Complete,
                }
            }
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
        Ok(Box::new(Silent))
    }
}

struct Rig {
    actor: Option<TxActor>,
    events: Receiver<TxEvent>,
    /// Done events received while waiting for another id.
    early: RefCell<Vec<TxDone>>,
    log: Log,
}

fn spawn(t: Box<dyn Transport>, log: Log) -> Rig {
    let (etx, events) = event_channel();
    let actor = TxActor::spawn(
        t,
        Arc::new(SeqSource::default()),
        Arc::new(FixedClock::new(1, 1)),
        etx,
    )
    .expect("tx actor");
    Rig {
        actor: Some(actor),
        events,
        early: RefCell::new(Vec::new()),
        log,
    }
}

fn rec_rig() -> Rig {
    let log = Log::default();
    spawn(
        Box::new(Rec {
            log: log.clone(),
            cfg: PortConfig::default(),
        }),
        log,
    )
}

fn req(id: u64, lane: TxLane, bytes: &[u8], p: Pacing, within: Duration) -> TxRequest {
    TxRequest {
        request_id: id,
        lane,
        client_id: format!("c{id}"),
        bytes: bytes.to_vec(),
        secret: false,
        deadline: Instant::now() + within,
        pacing: p,
    }
}

impl Rig {
    fn submit(&self, r: TxRequest) {
        self.actor
            .as_ref()
            .expect("actor")
            .handle()
            .submit(r)
            .expect("submit");
    }
    fn handle(&self) -> loom_uart_worker::tx::TxHandle {
        self.actor.as_ref().expect("actor").handle()
    }
    /// The Done for `id` (chunks skipped, other Dones kept for later calls), ≤ WAIT.
    fn done(&self, id: u64) -> TxDone {
        let pos = self.early.borrow().iter().position(|d| d.request_id == id);
        if let Some(i) = pos {
            return self.early.borrow_mut().remove(i);
        }
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left).expect("tx event") {
                TxEvent::Done(d) if d.request_id == id => return d,
                TxEvent::Done(d) => self.early.borrow_mut().push(d),
                TxEvent::Chunk(_) => {}
            }
        }
    }
}

fn spacing(w: &[(Instant, Vec<u8>)]) -> Vec<Duration> {
    w.windows(2).map(|p| p[1].0.duration_since(p[0].0)).collect()
}

// ---------------------------------------------------------------- schedule

#[test]
fn a_line_paced_three_line_send_does_not_burst() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::Agent, b"l1\nl2\nl3\n", pacing(0, 80), WAIT));
    let d = rig.done(1);
    assert_eq!(d.state, TxState::Sent);
    assert_eq!(d.written_bytes, 9);
    let w = rig.log.writes();
    let chunks: Vec<&[u8]> = w.iter().map(|(_, b)| b.as_slice()).collect();
    assert_eq!(chunks, vec![&b"l1\n"[..], b"l2\n", b"l3\n"], "one line per write");
    for gap in spacing(&w) {
        assert!(gap + SLACK >= ms(80), "lines {gap:?} apart, want ≥ 80 ms");
    }
}

#[test]
fn char_pacing_writes_one_byte_per_gap() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::User, b"abcd", pacing(25, 0), WAIT));
    assert_eq!(rig.done(1).state, TxState::Sent);
    let w = rig.log.writes();
    assert_eq!(w.len(), 4);
    assert!(w.iter().all(|(_, b)| b.len() == 1));
    for gap in spacing(&w) {
        assert!(gap + SLACK >= ms(25), "bytes {gap:?} apart, want ≥ 25 ms");
    }
}

#[test]
fn unpaced_requests_keep_1_kib_chunks_and_never_wait() {
    let rig = rec_rig();
    let t0 = Instant::now();
    rig.submit(req(1, TxLane::Agent, &vec![b'u'; 3000], Pacing::NONE, WAIT));
    let d = rig.done(1);
    assert_eq!(d.state, TxState::Sent);
    let sizes: Vec<usize> = rig.log.writes().iter().map(|(_, b)| b.len()).collect();
    assert_eq!(sizes, vec![TX_CHUNK_MAX, TX_CHUNK_MAX, 3000 - 2 * TX_CHUNK_MAX]);
    assert!(t0.elapsed() < ms(500), "unpaced path must not wait");
}

#[test]
fn the_trailing_gap_spaces_the_next_request_on_the_same_lane_but_not_its_done() {
    let rig = rec_rig();
    let t0 = Instant::now();
    rig.submit(req(1, TxLane::User, b"a", pacing(150, 0), WAIT));
    let d1 = rig.done(1);
    assert!(t0.elapsed() < ms(120), "Done is not delayed by the trailing gap");
    assert_eq!(d1.state, TxState::Sent);
    rig.submit(req(2, TxLane::User, b"b", pacing(150, 0), WAIT));
    assert_eq!(rig.done(2).state, TxState::Sent);
    let w = rig.log.writes();
    assert_eq!(w.len(), 2);
    assert!(
        spacing(&w)[0] + SLACK >= ms(150),
        "second keystroke waited the gap"
    );
}

#[test]
fn a_paced_user_request_blocks_the_agent_lane_during_its_gaps() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::User, b"abc", pacing(60, 0), WAIT));
    assert!(rig.log.wait_bytes(1));
    rig.submit(req(2, TxLane::Agent, b"Z", Pacing::NONE, WAIT));
    assert_eq!(rig.done(1).state, TxState::Sent);
    assert_eq!(rig.done(2).state, TxState::Sent);
    assert_eq!(
        rig.log.bytes(),
        b"abcZ",
        "agent byte never lands inside the user's paced line"
    );
}

#[test]
fn ctrl_c_lane_is_never_paced() {
    let rig = rec_rig();
    let t0 = Instant::now();
    rig.submit(req(1, TxLane::CtrlC, &[3, 3, 3], pacing(500, 500), WAIT));
    let d = rig.done(1);
    assert_eq!(d.state, TxState::Sent);
    assert!(t0.elapsed() < ms(300), "Ctrl-C waited for pacing");
    assert_eq!(rig.log.writes().len(), 1, "one unpaced write");
}

// ---------------------------------------------------------------- cancellation in a gap

#[test]
fn cancel_during_a_gap_ends_at_once_and_writes_nothing_later() {
    let rig = rec_rig();
    rig.submit(req(7, TxLane::Agent, b"abcdef", pacing(300, 0), WAIT));
    assert!(rig.log.wait_bytes(1));
    let t0 = Instant::now();
    assert_eq!(rig.handle().cancel(7, "stopped"), CancelResult::InProgress);
    let d = rig.done(7);
    assert!(
        t0.elapsed() < ms(150),
        "cancel must wake the gap wait, took {:?}",
        t0.elapsed()
    );
    assert_eq!(d.state, TxState::OutcomeUnknown);
    assert_eq!(d.cause, Some("stopped"));
    assert_eq!(d.written_bytes, 1);
    thread::sleep(ms(500));
    assert_eq!(rig.log.bytes(), b"a", "no delayed write after the cancel");
}

#[test]
fn cancel_lane_hold_during_a_gap_writes_nothing_later() {
    let rig = rec_rig();
    // Line-only pacing without a line end: one chunk, no gap.
    rig.submit(req(8, TxLane::Agent, b"abcdef", pacing(0, 300), WAIT));
    assert_eq!(rig.done(8).state, TxState::Sent);
    assert_eq!(rig.log.writes().len(), 1);
    rig.submit(req(9, TxLane::Agent, b"x\ny\nz\n", pacing(0, 300), WAIT));
    assert!(rig.log.wait_bytes(6 + 2));
    assert_eq!(rig.handle().cancel_lane(TxLane::Agent, "held_by_user"), 1);
    let d = rig.done(9);
    assert_eq!(d.state, TxState::OutcomeUnknown);
    assert_eq!(d.cause, Some("held_by_user"));
    assert_eq!(d.written_bytes, 2);
    thread::sleep(ms(700));
    assert_eq!(rig.log.bytes(), b"abcdefx\n");
}

#[test]
fn shutdown_during_a_gap_is_prompt_and_final() {
    let mut rig = rec_rig();
    rig.submit(req(1, TxLane::Agent, b"abcdef", pacing(400, 0), WAIT));
    assert!(rig.log.wait_bytes(1));
    let t0 = Instant::now();
    let actor = rig.actor.take().expect("actor");
    let transport = actor.shutdown();
    assert!(transport.is_some());
    assert!(
        t0.elapsed() < ms(200),
        "shutdown waited for the gap: {:?}",
        t0.elapsed()
    );
    let d = rig.done(1);
    assert_eq!(d.state, TxState::OutcomeUnknown);
    assert_eq!(d.error, Some(TxErrorCode::Shutdown));
    assert_eq!(rig.log.bytes(), b"a");
}

#[test]
fn a_deadline_inside_a_gap_ends_the_request_at_the_deadline() {
    let rig = rec_rig();
    let t0 = Instant::now();
    rig.submit(req(1, TxLane::Agent, b"abc", pacing(400, 0), ms(120)));
    let d = rig.done(1);
    let took = t0.elapsed();
    assert!(took < ms(300), "ended {took:?} after submit, deadline was 120 ms");
    assert_eq!(d.state, TxState::OutcomeUnknown);
    assert_eq!(d.error, Some(TxErrorCode::Timeout));
    assert_eq!(d.written_bytes, 1);
    thread::sleep(ms(500));
    assert_eq!(rig.log.bytes(), b"a");
}

// ---------------------------------------------------------------- Ctrl-C

#[test]
fn ctrl_c_cancels_paced_user_requests_and_goes_out_at_once() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::User, b"pasted", pacing(200, 0), WAIT));
    rig.submit(req(2, TxLane::User, b"more", pacing(200, 0), WAIT));
    assert!(rig.log.wait_bytes(1));
    let t0 = Instant::now();
    rig.submit(req(3, TxLane::CtrlC, &[3], Pacing::NONE, WAIT));
    let c = rig.done(3);
    assert_eq!(c.state, TxState::Sent);
    assert!(t0.elapsed() < ms(150), "Ctrl-C waited behind a paced gap");
    let d1 = rig.done(1);
    assert_eq!(d1.state, TxState::OutcomeUnknown);
    assert_eq!(d1.error, Some(TxErrorCode::InterruptedByUser));
    assert_eq!(d1.cause, Some("interrupted_by_user"));
    assert_eq!(d1.written_bytes, 1);
    let d2 = rig.done(2);
    assert_eq!(d2.state, TxState::Failed);
    assert_eq!(d2.cause, Some("interrupted_by_user"));
    assert_eq!(d2.written_bytes, 0);
    thread::sleep(ms(500));
    assert_eq!(
        rig.log.bytes(),
        vec![b'p', 3],
        "nothing of the paste after Ctrl-C"
    );
    // The user's next key is not held back by the interrupted request's gap.
    let t1 = Instant::now();
    rig.submit(req(4, TxLane::User, b"k", pacing(200, 0), WAIT));
    assert_eq!(rig.done(4).state, TxState::Sent);
    assert!(t1.elapsed() < ms(150), "user lane gap must be cleared by Ctrl-C");
}

#[test]
fn ctrl_c_still_leaves_unpaced_user_requests_alone() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::User, b"plain", Pacing::NONE, WAIT));
    rig.submit(req(2, TxLane::CtrlC, &[3], Pacing::NONE, WAIT));
    assert_eq!(rig.done(1).state, TxState::Sent);
    assert_eq!(rig.done(2).state, TxState::Sent);
    let mut b = rig.log.bytes();
    b.sort_unstable();
    let mut want = b"plain\x03".to_vec();
    want.sort_unstable();
    assert_eq!(b, want);
}

// ---------------------------------------------------------------- controls & backlog

#[test]
fn line_controls_run_during_a_gap() {
    let rig = rec_rig();
    rig.submit(req(1, TxLane::Agent, b"ab", pacing(400, 0), WAIT));
    assert!(rig.log.wait_bytes(1));
    let t0 = Instant::now();
    rig.handle()
        .control(ControlOp::Dtr(true), ms(200))
        .expect("control ran inside the gap");
    assert!(t0.elapsed() < ms(200));
    assert_eq!(*rig.log.controls.lock().expect("log"), vec![ControlOp::Dtr(true)]);
    assert_eq!(rig.done(1).state, TxState::Sent);
}

#[test]
fn paced_backlog_charges_estimate_plus_trailing_gap_and_the_lane_gap() {
    let log = Log::default();
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    let rig = spawn(
        Box::new(Gate {
            log: log.clone(),
            entered: entered_tx,
            release: release_rx,
            cfg: PortConfig::default(),
        }),
        log,
    );
    let h = rig.handle();
    assert_eq!(h.paced_backlog(TxLane::User), Duration::ZERO);
    // Unpaced blocker holds the actor inside a write.
    rig.submit(req(1, TxLane::User, b"x", Pacing::NONE, WAIT));
    entered.recv_timeout(WAIT).expect("blocker entered");
    let p = pacing(0, 300);
    rig.submit(req(2, TxLane::User, b"a\nb\n", p, WAIT));
    let want = pacing_estimate(b"a\nb\n", p) + trailing_gap(b"a\nb\n", p);
    assert_eq!(want, ms(600));
    assert_eq!(
        h.paced_backlog(TxLane::User),
        want,
        "queued paced request is charged"
    );
    assert_eq!(h.paced_backlog(TxLane::Agent), Duration::ZERO, "per lane");
    release.send(()).expect("release blocker");
    assert_eq!(rig.done(1).state, TxState::Sent);
    for _ in 0..2 {
        entered.recv_timeout(WAIT).expect("paced chunk entered");
        release.send(()).expect("release chunk");
    }
    assert_eq!(rig.done(2).state, TxState::Sent);
    // Charge released at finish; only the lane's trailing gap (≤ 300 ms) remains.
    let left = h.paced_backlog(TxLane::User);
    assert!(
        left <= ms(300) && left > Duration::ZERO,
        "backlog after finish {left:?}"
    );
    thread::sleep(ms(350));
    assert_eq!(h.paced_backlog(TxLane::User), Duration::ZERO);
}
