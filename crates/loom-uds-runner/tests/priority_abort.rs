//! P1: priority queues (abort > user > agent), abort, read retry — fake transport, paused clock.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, WriteState};
use loom_uds_runner::{
    AbortKind, AbortScope, AbortSignal, AbortSummary, FakeControl, FakeStep, FakeTransport,
    MAX_PENDING_ABORT, MAX_PENDING_PER_TARGET, MAX_PENDING_USER, MonoClock, Priority, QueueDepth,
    RejectReason, Runner, RunnerConfig, RunnerEvent, RunnerHandle, RunnerObserver, TimeoutPhase, TxnRequest,
    TxnResult, TxnStatus,
};
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep};

const READ_VIN: &[u8] = &[0x22, 0xF1, 0x90];
const VIN_OK: &[u8] = &[0x62, 0xF1, 0x90, b'L', b'O', b'O', b'M'];
const TP: &[u8] = &[0x3E, 0x00];
const TP_OK: &[u8] = &[0x7E, 0x00];
const ROUTINE: &[u8] = &[0x31, 0x01, 0xFF, 0x00];
const ROUTINE_OK: &[u8] = &[0x71, 0x01, 0xFF, 0x00];
const WRITE_DID: &[u8] = &[0x2E, 0xF1, 0x90, 0x01];
const WRITE_OK: &[u8] = &[0x6E, 0xF1, 0x90];

#[derive(Default)]
struct Rec(Mutex<Vec<String>>);

impl RunnerObserver for Rec {
    fn on_event(&self, e: &RunnerEvent<'_>) {
        let s = match e {
            RunnerEvent::Sent { pdu, .. } => format!("sent:{:02X}", pdu[0]),
            RunnerEvent::Pending { count, .. } => format!("pending:{count}"),
            RunnerEvent::Late { data, released, .. } => format!(
                "late:{:02X}:{released}",
                data.first().copied().unwrap_or_default()
            ),
            RunnerEvent::QuarantineReleased { count, cause } => format!("released:{count}:{cause:?}"),
        };
        self.0.lock().expect("rec lock").push(s);
    }
}

impl Rec {
    fn events(&self) -> Vec<String> {
        self.0.lock().expect("rec lock").clone()
    }
}

/// Routine: `0x78` at 10 ms, final at 1 s. Write DID: positive at 500 ms. VIN: `0x78` at 10 ms,
/// final at 500 ms. Tester present: 5 ms.
fn ecu(req: &[u8]) -> Vec<FakeStep> {
    match req {
        [0x31, ..] => vec![
            FakeStep::respond(10, &[0x7F, 0x31, 0x78]),
            FakeStep::respond(1000, ROUTINE_OK),
        ],
        [0x2E, ..] => vec![FakeStep::respond(500, WRITE_OK)],
        [0x22, 0xF1, 0x90] => vec![
            FakeStep::respond(10, &[0x7F, 0x22, 0x78]),
            FakeStep::respond(500, VIN_OK),
        ],
        [0x3E, 0x00] => vec![FakeStep::respond(5, TP_OK)],
        _ => vec![],
    }
}

fn setup_with(
    cfg: RunnerConfig,
    script: impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static,
) -> (Runner, FakeControl, Arc<Rec>) {
    let (t, ctl) = FakeTransport::new(script);
    let rec = Arc::new(Rec::default());
    let runner = Runner::new(Box::new(t), cfg, MonoClock::starting_now(), rec.clone());
    (runner, ctl, rec)
}

fn setup() -> (Runner, FakeControl, Arc<Rec>) {
    setup_with(RunnerConfig::default(), ecu)
}

fn read(pdu: &[u8]) -> TxnRequest {
    TxnRequest::read(pdu.to_vec(), Duration::from_secs(60))
}

fn write(pdu: &[u8]) -> TxnRequest {
    TxnRequest::write(pdu.to_vec(), Duration::from_secs(60))
}

/// Let the runner task take the first request.
async fn settle() {
    tokio::task::yield_now().await;
    sleep(Duration::from_millis(1)).await;
}

async fn done(rx: oneshot::Receiver<TxnResult>) -> TxnResult {
    rx.await.expect("result")
}

fn assert_aborted_unsent(r: &TxnResult, kind: AbortKind) {
    assert_eq!(r.status, TxnStatus::Aborted { kind });
    assert_eq!(r.write_state, WriteState::Failed);
    assert!(!r.sent && !r.may_have_executed && !r.timeout);
    assert_eq!(r.error, Some(kind.error_code()));
}

#[tokio::test(start_paused = true)]
async fn user_request_runs_before_waiting_agents() {
    let (r, ctl, _rec) = setup();
    let (h, _task) = RunnerHandle::spawn(r);
    let first = h.try_submit(write(ROUTINE)).expect("first");
    settle().await;
    let agents: Vec<_> = (0..3).map(|_| h.try_submit(read(TP)).expect("agent")).collect();
    let user = h
        .try_submit(
            read(READ_VIN)
                .with_priority(Priority::User)
                .with_origin(Origin::User),
        )
        .expect("user");
    assert_eq!(
        h.queue_depth(),
        QueueDepth {
            abort: 0,
            user: 1,
            agent: 3,
            in_flight: true
        }
    );
    // The routine in flight is not pre-empted.
    assert_eq!(done(first).await.status, TxnStatus::Positive);
    let user = done(user).await;
    assert_eq!(user.status, TxnStatus::Positive);
    for a in agents {
        let a = done(a).await;
        assert_eq!(a.status, TxnStatus::Positive);
        assert!(a.times.sent_mono_ns >= user.times.final_rx_mono_ns);
    }
    assert_eq!(
        ctl.sent(),
        vec![
            ROUTINE.to_vec(),
            READ_VIN.to_vec(),
            TP.to_vec(),
            TP.to_vec(),
            TP.to_vec()
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn abort_all_empties_the_queue_but_keeps_abort_priority() {
    let (r, ctl, rec) = setup();
    let (h, _task) = RunnerHandle::spawn(r);
    let first = h.try_submit(write(ROUTINE)).expect("first");
    settle().await;
    let a1 = h.try_submit(read(TP)).expect("a1");
    let a2 = h.try_submit(write(WRITE_DID)).expect("a2");
    let u1 = h
        .try_submit(read(READ_VIN).with_priority(Priority::User))
        .expect("u1");
    let recover = h
        .try_submit(write(TP).with_priority(Priority::Abort).with_origin(Origin::User))
        .expect("recover");
    sleep(Duration::from_millis(50)).await;

    let wall = std::time::Instant::now();
    let summary = h.abort(AbortScope::All);
    assert!(wall.elapsed() < Duration::from_millis(10), "{:?}", wall.elapsed());
    assert_eq!(
        summary,
        AbortSummary {
            cancelled_waiting: 3,
            in_flight: true
        }
    );
    let t0 = Instant::now();
    for rx in [a1, a2, u1] {
        assert_aborted_unsent(&done(rx).await, AbortKind::User);
    }
    // In flight write: wait stopped at once, outcome unknown.
    let first = done(first).await;
    assert_eq!(t0.elapsed(), Duration::ZERO, "in-flight result without waiting");
    assert_eq!(
        first.status,
        TxnStatus::Aborted {
            kind: AbortKind::User
        }
    );
    assert_eq!(first.write_state, WriteState::OutcomeUnknown);
    assert!(first.sent && first.may_have_executed);
    assert_eq!(first.pending_count, 1);
    assert_eq!(first.error, Some(ErrorCode::InterruptedByUser));
    // The abort-priority request survives, waits the quiet period and sees the late final
    // response of the aborted routine (released) before it is sent.
    let recover = done(recover).await;
    assert_eq!(recover.status, TxnStatus::Positive);
    assert_eq!(recover.late_count, 1);
    assert!(rec.events().contains(&"late:71:true".to_string()));
    assert_eq!(ctl.sent(), vec![ROUTINE.to_vec(), TP.to_vec()]);
}

#[tokio::test(start_paused = true)]
async fn aborted_write_in_flight_is_outcome_unknown_and_its_answer_quarantined() {
    let (mut r, ctl, rec) = setup();
    let sig = AbortSignal::new();
    let s2 = sig.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(30)).await;
        s2.abort(AbortKind::User);
    });
    let start = Instant::now();
    // Inside P2 (50 ms): the abort, not a timeout, ends it.
    let res = r.execute_abortable(write(WRITE_DID), &sig).await;
    assert_eq!(start.elapsed(), Duration::from_millis(30));
    assert_eq!(
        res.status,
        TxnStatus::Aborted {
            kind: AbortKind::User
        }
    );
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert!(res.sent && res.may_have_executed && !res.timeout);
    assert_eq!(res.error, Some(ErrorCode::InterruptedByUser));
    assert!(r.unresolved().overlaps(0x2E));

    // Same SID while the answer is outstanding: refused, never sent.
    let res = r.execute(write(&[0x2E, 0xF1, 0x91, 0x02])).await;
    assert_eq!(
        res.status,
        TxnStatus::Rejected(RejectReason::Quarantine { sid: 0x2E })
    );
    assert_eq!(res.error, Some(ErrorCode::Quarantine));
    assert_eq!(ctl.sent().len(), 1);

    // The ECU answers at 500 ms: late, releases U, never a result.
    sleep(Duration::from_millis(600)).await;
    let res = r.execute(read(TP)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.late_count, 1);
    assert!(r.unresolved().is_empty());
    assert!(rec.events().contains(&"late:6E:true".to_string()));
}

#[tokio::test(start_paused = true)]
async fn aborted_read_in_flight_is_failed_and_stop_kind_maps_to_e_stopped() {
    let (mut r, _ctl, _rec) = setup();
    let sig = AbortSignal::new();
    let s2 = sig.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(100)).await;
        s2.abort(AbortKind::Stop);
        s2.abort(AbortKind::User); // first kind wins
    });
    let res = r.execute_abortable(read(READ_VIN), &sig).await;
    assert_eq!(
        res.status,
        TxnStatus::Aborted {
            kind: AbortKind::Stop
        }
    );
    assert_eq!(res.write_state, WriteState::Failed);
    assert_eq!(res.error, Some(ErrorCode::Stopped));
    assert!(res.sent);
    assert_eq!(res.pending_count, 1);
    assert!(
        r.unresolved().overlaps(0x22),
        "answer still outstanding → quarantined"
    );
}

#[tokio::test(start_paused = true)]
async fn abort_before_send_sends_nothing() {
    // Already fired.
    let (mut r, ctl, _rec) = setup();
    let sig = AbortSignal::new();
    sig.abort(AbortKind::User);
    let res = r.execute_abortable(write(WRITE_DID), &sig).await;
    assert_aborted_unsent(&res, AbortKind::User);
    assert!(ctl.sent().is_empty());
    assert!(r.unresolved().is_empty());

    // Fired during the quarantine quiet period.
    let (mut r, ctl, _rec) = setup_with(RunnerConfig::default(), |req: &[u8]| match req {
        [0x3E, 0x00] => vec![FakeStep::respond(5, TP_OK)],
        _ => vec![],
    });
    let res = r
        .execute(TxnRequest::read(READ_VIN.to_vec(), Duration::from_secs(2)))
        .await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    let sig = AbortSignal::new();
    let s2 = sig.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;
        s2.abort(AbortKind::Stop);
    });
    let res = r.execute_abortable(read(TP), &sig).await;
    assert_aborted_unsent(&res, AbortKind::Stop);
    assert_eq!(ctl.sent(), vec![READ_VIN.to_vec()]);
}

#[tokio::test(start_paused = true)]
async fn per_priority_caps_overflow_is_busy() {
    let (r, ctl, _rec) = setup();
    let (h, _task) = RunnerHandle::spawn(r);
    let _first = h.try_submit(write(ROUTINE)).expect("first");
    settle().await;
    let mut held = Vec::new();
    for _ in 0..MAX_PENDING_USER {
        held.push(
            h.try_submit(read(TP).with_priority(Priority::User))
                .expect("user fits"),
        );
    }
    let busy = h
        .try_submit(read(TP).with_priority(Priority::User))
        .expect_err("user lane full");
    assert_eq!(busy.status, TxnStatus::Rejected(RejectReason::QueueFull));
    assert_eq!(busy.error, Some(ErrorCode::Busy));
    assert!(!busy.sent);
    for _ in 0..MAX_PENDING_ABORT {
        held.push(
            h.try_submit(read(TP).with_priority(Priority::Abort))
                .expect("abort fits"),
        );
    }
    let busy = h
        .try_submit(read(TP).with_priority(Priority::Abort))
        .expect_err("abort lane full");
    assert_eq!(busy.error, Some(ErrorCode::Busy));
    // The agent lane is independent: user/abort traffic never eats agent capacity and vice versa.
    for _ in 0..MAX_PENDING_PER_TARGET {
        held.push(h.try_submit(read(TP)).expect("agent fits"));
    }
    let busy = h.try_submit(read(TP)).expect_err("agent lane full");
    assert_eq!(busy.error, Some(ErrorCode::Busy));
    assert_eq!(
        h.queue_depth(),
        QueueDepth {
            abort: MAX_PENDING_ABORT,
            user: MAX_PENDING_USER,
            agent: MAX_PENDING_PER_TARGET,
            in_flight: true
        }
    );
    // Measured: abort with every lane full returns well inside 10 ms.
    let wall = std::time::Instant::now();
    let s = h.abort(AbortScope::All);
    let took = wall.elapsed();
    eprintln!("abort latency (56 waiting + in flight): {took:?}");
    assert!(took < Duration::from_millis(10), "{took:?}");
    assert_eq!(s.cancelled_waiting, MAX_PENDING_USER + MAX_PENDING_PER_TARGET);
    assert!(s.in_flight);
    let n = held.len();
    let mut aborted = 0;
    for rx in held {
        if matches!(done(rx).await.status, TxnStatus::Aborted { .. }) {
            aborted += 1;
        }
    }
    assert_eq!(aborted, n - MAX_PENDING_ABORT, "abort-priority requests survive");
    assert_eq!(ctl.sent().len(), 1 + MAX_PENDING_ABORT);
}

#[tokio::test(start_paused = true)]
async fn abort_by_origin_and_by_request_id() {
    let (r, ctl, _rec) = setup();
    let (h, _task) = RunnerHandle::spawn(r);
    let first = h.try_submit(write(ROUTINE).with_request_id("a0")).expect("a0");
    settle().await;
    let a1 = h.try_submit(read(TP).with_request_id("a1")).expect("a1");
    let s1 = h
        .try_submit(read(TP).with_origin(Origin::Script).with_request_id("s1"))
        .expect("s1");
    let u1 = h
        .try_submit(
            read(READ_VIN)
                .with_priority(Priority::User)
                .with_origin(Origin::User)
                .with_request_id("u1"),
        )
        .expect("u1");
    let x1 = h
        .try_submit(
            read(TP)
                .with_priority(Priority::Abort)
                .with_origin(Origin::User)
                .with_request_id("x1"),
        )
        .expect("x1");

    let s = h.abort_with(AbortScope::Origin(Origin::Script), AbortKind::Stop);
    assert_eq!(
        s,
        AbortSummary {
            cancelled_waiting: 1,
            in_flight: false
        }
    );
    assert_aborted_unsent(&done(s1).await, AbortKind::Stop);
    // Origin scope spares abort-priority; the request-id scope reaches it.
    let s = h.abort(AbortScope::Origin(Origin::User));
    assert_eq!(s.cancelled_waiting, 1);
    assert_aborted_unsent(&done(u1).await, AbortKind::User);
    let s = h.abort(AbortScope::Request("x1".into()));
    assert_eq!(s.cancelled_waiting, 1);
    assert_aborted_unsent(&done(x1).await, AbortKind::User);
    assert_eq!(
        h.abort(AbortScope::Request("nope".into())),
        AbortSummary::default()
    );
    // Request id of the one in flight.
    let s = h.abort(AbortScope::Request("a0".into()));
    assert_eq!(
        s,
        AbortSummary {
            cancelled_waiting: 0,
            in_flight: true
        }
    );
    let first = done(first).await;
    assert_eq!(first.write_state, WriteState::OutcomeUnknown);
    // a1 (agent) still runs; it does not overlap the aborted routine's SID.
    assert_eq!(done(a1).await.status, TxnStatus::Positive);
    assert_eq!(ctl.sent(), vec![ROUTINE.to_vec(), TP.to_vec()]);
}

#[tokio::test(start_paused = true)]
async fn dropping_every_handle_finishes_the_queue_then_stops() {
    let (r, _ctl, _rec) = setup();
    let (h, task) = RunnerHandle::spawn(r);
    let h2 = h.clone();
    let rx = h.try_submit(read(TP)).expect("queued");
    drop(h);
    // Queue is finished before the task stops.
    assert_eq!(done(rx).await.status, TxnStatus::Positive);
    drop(h2);
    let runner = task.await.expect("task");
    assert!(runner.unresolved().is_empty());
}

// ---- read retry (plan §3.4: reads once, writes never) ----

fn busy_then_ok() -> (
    impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static,
    Arc<AtomicU32>,
) {
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let script = move |req: &[u8]| {
        let n = c.fetch_add(1, Ordering::SeqCst);
        let sid = req.first().copied().unwrap_or_default();
        if n == 0 {
            vec![FakeStep::respond(5, &[0x7F, sid, 0x21])]
        } else if sid == 0x22 {
            vec![FakeStep::respond(5, VIN_OK)]
        } else {
            vec![FakeStep::respond(5, WRITE_OK)]
        }
    };
    (script, calls)
}

#[tokio::test(start_paused = true)]
async fn read_is_retried_once_after_busy_repeat_request_when_enabled() {
    let (script, _calls) = busy_then_ok();
    let (mut r, ctl, rec) = setup_with(RunnerConfig::default().with_read_retry(true), script);
    let res = r.execute(read(READ_VIN)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.response.as_deref(), Some(VIN_OK));
    assert_eq!(ctl.sent(), vec![READ_VIN.to_vec(), READ_VIN.to_vec()]);
    assert_eq!(rec.events(), vec!["sent:22", "sent:22"]);

    // Only once: a second busy answer is the result.
    let (mut r, ctl, _rec) = setup_with(RunnerConfig::default().with_read_retry(true), |req: &[u8]| {
        vec![FakeStep::respond(5, &[0x7F, req[0], 0x21])]
    });
    let res = r.execute(read(READ_VIN)).await;
    assert_eq!(res.status, TxnStatus::Negative { nrc: 0x21 });
    assert_eq!(ctl.sent().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn writes_and_default_config_are_never_retried() {
    let (script, _calls) = busy_then_ok();
    let (mut r, ctl, _rec) = setup_with(RunnerConfig::default().with_read_retry(true), script);
    let res = r.execute(write(WRITE_DID)).await;
    assert_eq!(res.status, TxnStatus::Negative { nrc: 0x21 });
    assert_eq!(ctl.sent().len(), 1, "writes are never re-sent");

    let (script, _calls) = busy_then_ok();
    let (mut r, ctl, _rec) = setup_with(RunnerConfig::default(), script);
    let res = r.execute(read(READ_VIN)).await;
    assert_eq!(res.status, TxnStatus::Negative { nrc: 0x21 });
    assert_eq!(ctl.sent().len(), 1, "retry is off by default");

    // A timed-out read is not retried even when enabled (it joined U).
    let (mut r, ctl, _rec) = setup_with(
        RunnerConfig::default().with_read_retry(true),
        |_req: &[u8]| vec![],
    );
    let res = r.execute(read(READ_VIN)).await;
    assert!(res.timeout);
    assert_eq!(ctl.sent().len(), 1);
}
