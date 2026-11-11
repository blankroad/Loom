//! P1 counterexamples: cancellation, wire timestamps, and byte admission.
use loom_proto::event::WriteState;
use loom_uds_runner::{
    BoxFuture, FakeStep, FakeTransport, Incoming, MonoClock, NoopObserver, Priority, RecvError, RejectReason,
    Runner, RunnerConfig, RunnerHandle, TimeoutPhase, Transport, TransportError, TxnRequest, TxnStatus,
};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::time::Instant;

fn runner(t: impl Transport + 'static, cfg: RunnerConfig) -> Runner {
    Runner::new(
        Box::new(t),
        cfg,
        MonoClock::starting_now(),
        Arc::new(NoopObserver),
    )
}
fn write() -> TxnRequest {
    TxnRequest::write(vec![0x2e, 0xf1, 0xa0, 1], Duration::from_secs(1))
}

#[tokio::test(start_paused = true)]
async fn dropped_execute_preserves_u_and_cannot_confirm_next_write() {
    let mut n = 0;
    let (t, ctl) = FakeTransport::new(move |_| {
        n += 1;
        vec![FakeStep::respond(
            if n == 1 { 60 } else { 300 },
            &[0x6e, 0xf1, 0xa0],
        )]
    });
    let mut r = runner(t, RunnerConfig::default());
    assert!(
        tokio::time::timeout(Duration::from_millis(10), r.execute(write()))
            .await
            .is_err()
    );
    assert!(r.unresolved().overlaps(0x2e), "drop must leave U immediately");
    let next = r.execute(write()).await;
    assert_eq!(
        next.status,
        TxnStatus::Rejected(RejectReason::Quarantine { sid: 0x2e })
    );
    assert_eq!(ctl.sent().len(), 1);
    tokio::time::advance(Duration::from_millis(60)).await;
    let next = r.execute(write()).await;
    assert!(matches!(next.status, TxnStatus::Timeout { .. }));
    assert_eq!(next.write_state, WriteState::OutcomeUnknown);
    assert_eq!(next.late_count, 1);
    assert_eq!(ctl.sent().len(), 2);
}

struct Stamped {
    origin: Instant,
    items: VecDeque<(u64, bool)>,
    delay: u64,
    stall_submit: bool,
}
impl Transport for Stamped {
    fn generation(&self) -> u64 {
        1
    }
    fn submit<'a>(&'a mut self, _: &'a [u8], _: Instant) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move {
            if self.stall_submit {
                std::future::pending::<()>().await;
            }
            Ok(self.origin)
        })
    }
    fn confirm<'a>(&'a mut self, _: Instant) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move { Ok(self.origin) })
    }
    fn recv<'a>(&'a mut self, _: Instant) -> BoxFuture<'a, Result<Incoming, RecvError>> {
        Box::pin(async move {
            tokio::time::advance(Duration::from_millis(self.delay)).await;
            let (ms, start) = self.items.pop_front().expect("scripted response");
            let at = self.origin + Duration::from_millis(ms);
            Ok(if start {
                Incoming::RxStart { at }
            } else {
                Incoming::Pdu {
                    at,
                    data: if ms == 10 {
                        vec![0x7f, 0x2e, 0x78]
                    } else {
                        vec![0x6e, 0xf1, 0xa0]
                    },
                }
            })
        })
    }
    fn try_recv(&mut self) -> Option<Incoming> {
        None
    }
}

#[tokio::test(start_paused = true)]
async fn dropping_during_submit_reserves_u_before_possible_handoff() {
    let t = Stamped {
        origin: Instant::now(),
        items: VecDeque::new(),
        delay: 0,
        stall_submit: true,
    };
    let mut r = runner(t, RunnerConfig::default());
    assert!(
        tokio::time::timeout(Duration::from_millis(1), r.execute(write()))
            .await
            .is_err()
    );
    assert!(r.unresolved().overlaps(0x2e));
}

#[tokio::test(start_paused = true)]
async fn late_stamps_cannot_succeed_in_any_response_phase() {
    for (items, deadline, expected, late_count) in [
        (vec![(80, false)], 1000, TimeoutPhase::P2, 1),
        (vec![(45, false)], 40, TimeoutPhase::Deadline, 1),
        (vec![(10, false), (40, false)], 1000, TimeoutPhase::P2Star, 1),
        (vec![(10, true), (40, false)], 1000, TimeoutPhase::Completion, 1),
        (vec![(80, true)], 1000, TimeoutPhase::P2, 0),
    ] {
        let t = Stamped {
            origin: Instant::now(),
            items: items.into(),
            delay: 100,
            stall_submit: false,
        };
        let mut r = runner(
            t,
            RunnerConfig {
                p2_star: Duration::from_millis(20),
                completion_limit: Duration::from_millis(20),
                ..RunnerConfig::default()
            },
        );
        let mut req = write();
        req.deadline = Instant::now() + Duration::from_millis(deadline);
        let result = r.execute(req).await;
        assert_eq!(result.status, TxnStatus::Timeout { phase: expected });
        assert_eq!(result.write_state, WriteState::OutcomeUnknown);
        assert_eq!(result.late_count, late_count);
        assert_eq!(
            r.unresolved().overlaps(0x2e),
            late_count == 0,
            "late final response is release evidence; late RxStart is not"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn on_time_stamp_survives_delayed_executor() {
    let t = Stamped {
        origin: Instant::now(),
        items: vec![(20, false)].into(),
        delay: 100,
        stall_submit: false,
    };
    let mut r = runner(t, RunnerConfig::default());
    let mut req = write();
    req.deadline = Instant::now() + Duration::from_millis(70);
    assert_eq!(r.execute(req).await.status, TxnStatus::Positive);
}

#[tokio::test(start_paused = true)]
async fn invalid_metadata_and_pdus_rejected_before_admission() {
    let (t, ctl) = FakeTransport::new(|_| vec![]);
    let (h, task) = RunnerHandle::spawn(runner(t, RunnerConfig::default()));
    for req in [
        TxnRequest::read(vec![0x22; 4096], Duration::from_secs(1)),
        write().with_request_id("x".repeat(1024 * 1024)),
        TxnRequest::read(vec![], Duration::from_secs(1)),
    ] {
        assert!(h.try_submit(req).is_err(), "reject before the actor can dequeue");
    }
    assert_eq!(h.queue_depth().agent, 0);
    assert!(ctl.sent().is_empty());
    drop(h);
    task.await.expect("runner exits");
}

#[tokio::test(start_paused = true)]
async fn byte_quota_bounds_lanes_and_is_reusable_after_abort() {
    let (t, _) = FakeTransport::new(|_| vec![]);
    let (h, task) = RunnerHandle::spawn(runner(t, RunnerConfig::default()));
    let big = || TxnRequest::read(vec![0x22; 4095], Duration::from_secs(1));
    let mut admitted = 0;
    while h.try_submit(big()).is_ok() {
        admitted += 1;
    }
    assert!(admitted < 32, "byte quota must bind before count quota");
    assert!(
        h.try_submit(big().with_priority(Priority::User)).is_ok(),
        "reserved user bytes"
    );
    h.abort(loom_uds_runner::AbortScope::All);
    assert!(h.try_submit(big()).is_ok(), "abort returns byte reservations");
    h.abort(loom_uds_runner::AbortScope::All);
    drop(h);
    task.await.expect("runner exits");
}

#[tokio::test(start_paused = true)]
async fn probe_configuration_rejects_oversized_candidates() {
    let (t, _) = FakeTransport::new(|_| vec![]);
    let r = runner(
        t,
        RunnerConfig {
            probe_candidates: vec![vec![0x22; 1_000_000]],
            ..RunnerConfig::default()
        },
    );
    assert!(r.config().probe_candidates.is_empty());
}

#[tokio::test(start_paused = true)]
async fn target_byte_limit_includes_in_flight_and_all_priority_lanes() {
    let (t, _) = FakeTransport::new(|_| vec![]);
    let (h, task) = RunnerHandle::spawn(runner(t, RunnerConfig::default()));
    // Each request retains exactly 4096 payload/metadata bytes.
    let big = |priority| {
        TxnRequest::read(vec![0x22; 4095], Duration::from_secs(1))
            .with_request_id("a")
            .with_priority(priority)
    };
    h.try_submit(big(Priority::Agent)).expect("in flight");
    tokio::task::yield_now().await;
    assert!(h.queue_depth().in_flight);
    for (priority, count) in [(Priority::Agent, 8), (Priority::User, 4), (Priority::Abort, 3)] {
        for _ in 0..count {
            h.try_submit(big(priority)).expect("fits target quota");
        }
    }
    assert_eq!(
        h.try_submit(big(Priority::Abort))
            .expect_err("target bytes full")
            .status,
        TxnStatus::Rejected(RejectReason::QueueFull)
    );
    h.abort(loom_uds_runner::AbortScope::All);
    drop(h);
    task.await.expect("runner exits");
}

#[tokio::test(start_paused = true)]
async fn direct_and_async_handle_paths_validate_request_ids() {
    let (t, ctl) = FakeTransport::new(|_| vec![]);
    let mut r = runner(t, RunnerConfig::default());
    let bad = || write().with_request_id("é".repeat(65)); // 130 UTF-8 bytes.
    assert_eq!(
        r.execute(bad()).await.status,
        TxnStatus::Rejected(RejectReason::InvalidRequestId)
    );
    let (h, task) = RunnerHandle::spawn(r);
    assert_eq!(
        h.request(bad()).await.status,
        TxnStatus::Rejected(RejectReason::InvalidRequestId)
    );
    assert_eq!(h.queue_depth().agent, 0);
    assert!(ctl.sent().is_empty());
    drop(h);
    task.await.expect("runner exits");
}

#[tokio::test(start_paused = true)]
async fn dropped_handle_receiver_does_not_drop_transaction() {
    let (t, ctl) = FakeTransport::new(|_| vec![FakeStep::respond(20, &[0x6e, 0xf1, 0xa0])]);
    let (h, task) = RunnerHandle::spawn(runner(t, RunnerConfig::default()));
    drop(h.try_submit(write()).expect("admitted"));
    drop(h);
    let r = task
        .await
        .expect("actor completes orphaned request before exiting");
    assert_eq!(ctl.sent().len(), 1);
    assert!(r.unresolved().is_empty());
}
