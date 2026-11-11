//! D2 gate: runner rules on the scripted fake transport with tokio's paused clock.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use loom_proto::error::ErrorCode;
use loom_proto::event::WriteState;
use loom_uds_codec::{ResponseKind, classify_response};
use loom_uds_runner::quarantine::MAX_UNRESOLVED_PER_TARGET;
use loom_uds_runner::{
    FakeAction, FakeControl, FakeStep, FakeTransport, MAX_PENDING_PER_TARGET, MonoClock, RejectReason,
    ReleaseCause, Runner, RunnerConfig, RunnerEvent, RunnerHandle, RunnerObserver, TimeoutPhase,
    TransportError, TxnRequest, TxnStatus, Unresolved,
};
use tokio::time::Instant;

const READ_VIN: &[u8] = &[0x22, 0xF1, 0x90];
const VIN_OK: &[u8] = &[0x62, 0xF1, 0x90, b'L', b'O', b'O', b'M'];
const TP: &[u8] = &[0x3E, 0x00];
const TP_OK: &[u8] = &[0x7E, 0x00];
const WRITE_DID: &[u8] = &[0x2E, 0xF1, 0x90, 0x01];

/// Records event kinds (test only).
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

fn setup_cfg(
    cfg: RunnerConfig,
    script: impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static,
) -> (Runner, FakeControl, Arc<Rec>) {
    let (t, ctl) = FakeTransport::new(script);
    let rec = Arc::new(Rec::default());
    let runner = Runner::new(
        Box::new(t),
        cfg,
        MonoClock::new(Instant::now(), 1_000_000_000),
        rec.clone(),
    );
    (runner, ctl, rec)
}

fn setup(script: impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static) -> (Runner, FakeControl, Arc<Rec>) {
    setup_cfg(RunnerConfig::default(), script)
}

/// Standard ECU: `22 F190` → VIN after `ms`, `3E 00` → `7E 00`, `11 01` → `51 01`, others silent.
fn ecu(ms: u64) -> impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static {
    move |req: &[u8]| match req {
        [0x22, 0xF1, 0x90] => vec![FakeStep::respond(ms, VIN_OK)],
        [0x3E, 0x00] => vec![FakeStep::respond(5, TP_OK)],
        [0x11, 0x01] => vec![FakeStep::respond(5, &[0x51, 0x01])],
        _ => vec![],
    }
}

fn read(pdu: &[u8], ms: u64) -> TxnRequest {
    TxnRequest::read(pdu.to_vec(), Duration::from_millis(ms))
}

fn write(pdu: &[u8], ms: u64) -> TxnRequest {
    TxnRequest::write(pdu.to_vec(), Duration::from_millis(ms))
}

const MS: u64 = 1_000_000;
/// A response delay longer than any test runs (about 2.8 h of paused time).
const NEVER: u64 = 10_000_000;

#[tokio::test(start_paused = true)]
async fn positive_read_is_confirmed_with_timestamps() {
    let (mut r, ctl, rec) = setup(ecu(10));
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.write_state, WriteState::Confirmed);
    assert_eq!(res.response.as_deref(), Some(VIN_OK));
    assert_eq!(res.error, None);
    let t = res.times;
    assert_eq!(t.received_mono_ns, 1_000_000_000);
    assert_eq!(t.sent_mono_ns, Some(1_000_000_000));
    assert_eq!(t.first_rx_mono_ns, Some(1_000_000_000 + 10 * MS));
    assert_eq!(t.final_rx_mono_ns, t.first_rx_mono_ns);
    assert_eq!(ctl.sent(), vec![READ_VIN.to_vec()]);
    assert_eq!(rec.events(), vec!["sent:22"]);
}

#[tokio::test(start_paused = true)]
async fn p2_boundary() {
    // Inside P2 (49 ms) → positive; past P2 (51 ms) → timeout, and the answer becomes late.
    let (mut r, _ctl, _rec) = setup(ecu(49));
    assert_eq!(r.execute(read(READ_VIN, 2000)).await.status, TxnStatus::Positive);
    let (mut r, _ctl, _rec) = setup(ecu(51));
    let start = Instant::now();
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert_eq!(start.elapsed(), Duration::from_millis(50));
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    assert_eq!(res.write_state, WriteState::Failed);
    assert!(res.timeout);
    assert_eq!(res.error, Some(ErrorCode::NoResponse));
    assert_eq!(r.unresolved().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn write_timeout_is_outcome_unknown_and_not_resent() {
    let (mut r, ctl, _rec) = setup(ecu(0));
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert!(res.timeout && res.sent && res.may_have_executed);
    assert_eq!(ctl.sent().len(), 1, "no automatic re-send");
}

#[tokio::test(start_paused = true)]
async fn nrc_is_failed_with_nrc() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| vec![FakeStep::respond(5, &[0x7F, 0x2E, 0x31])]);
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(res.status, TxnStatus::Negative { nrc: 0x31 });
    assert_eq!(res.write_state, WriteState::Failed);
    assert_eq!(res.nrc, Some(0x31));
    assert_eq!(res.error, Some(ErrorCode::Nrc));
    assert!(!res.timeout);
}

#[tokio::test(start_paused = true)]
async fn pending_switches_to_p2_star() {
    let (mut r, _ctl, rec) = setup(|_req: &[u8]| {
        vec![
            FakeStep::respond(10, &[0x7F, 0x2E, 0x78]),
            FakeStep::respond(4010, &[0x7F, 0x2E, 0x78]),
            FakeStep::respond(9000, &[0x6E, 0xF1, 0x90]),
        ]
    });
    let res = r.execute(write(WRITE_DID, 30_000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.write_state, WriteState::Confirmed);
    assert_eq!(res.pending_count, 2);
    let t = res.times;
    assert_eq!(t.first_rx_mono_ns.map(|v| v - 1_000_000_000), Some(10 * MS));
    assert_eq!(t.final_rx_mono_ns.map(|v| v - 1_000_000_000), Some(9000 * MS));
    assert_eq!(rec.events(), vec!["sent:2E", "pending:1", "pending:2"]);
}

#[tokio::test(start_paused = true)]
async fn p2_star_timeout_after_pending() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| vec![FakeStep::respond(10, &[0x7F, 0x2E, 0x78])]);
    let start = Instant::now();
    let res = r.execute(write(WRITE_DID, 30_000)).await;
    assert_eq!(start.elapsed(), Duration::from_millis(5010));
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2Star
        }
    );
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert!(r.unresolved().overlaps(0x2E));
}

#[tokio::test(start_paused = true)]
async fn pending_count_is_bounded() {
    let cfg = RunnerConfig {
        max_response_pending: 3,
        ..RunnerConfig::default()
    };
    let (mut r, _ctl, _rec) = setup_cfg(cfg, |_req: &[u8]| {
        (1..=10)
            .map(|i| FakeStep::respond(10 + (i - 1) * 100, &[0x7F, 0x31, 0x78]))
            .collect()
    });
    let res = r.execute(write(&[0x31, 0x01, 0xFF, 0x00], 30_000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::PendingLimit
        }
    );
    assert_eq!(res.pending_count, 4);
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    let hard = RunnerConfig {
        max_response_pending: u32::MAX,
        ..RunnerConfig::default()
    };
    let (r, _c, _e) = setup_cfg(hard, ecu(0));
    assert_eq!(
        r.config().max_response_pending,
        loom_uds_runner::MAX_RESPONSE_PENDING_HARD
    );
}

#[tokio::test(start_paused = true)]
async fn total_deadline_bounds_pending_chain() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| {
        (1..=20)
            .map(|i| FakeStep::respond(10 + (i - 1) * 1000, &[0x7F, 0x31, 0x78]))
            .collect()
    });
    let start = Instant::now();
    let res = r.execute(write(&[0x31, 0x01, 0xFF, 0x00], 2500)).await;
    assert_eq!(start.elapsed(), Duration::from_millis(2500));
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::Deadline
        }
    );
    assert_eq!(res.pending_count, 3);
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
}

#[tokio::test(start_paused = true)]
async fn deadline_counts_queue_wait() {
    let (r, ctl, _rec) = setup(|req: &[u8]| match req {
        [0x31, ..] => vec![
            FakeStep::respond(10, &[0x7F, 0x31, 0x78]),
            FakeStep::respond(1000, &[0x71, 0x01, 0xFF, 0x00]),
        ],
        _ => vec![FakeStep::respond(5, TP_OK)],
    });
    let (h, _task) = RunnerHandle::spawn(r);
    let first = h
        .try_submit(write(&[0x31, 0x01, 0xFF, 0x00], 5000))
        .expect("queued");
    let second = h.try_submit(read(TP, 500)).expect("queued");
    let first = first.await.expect("first result");
    let second = second.await.expect("second result");
    assert_eq!(first.status, TxnStatus::Positive);
    assert_eq!(second.status, TxnStatus::Rejected(RejectReason::Expired));
    assert_eq!(second.write_state, WriteState::Failed);
    assert!(!second.sent);
    assert_eq!(ctl.sent().len(), 1, "expired request never sent");
}

#[tokio::test(start_paused = true)]
async fn generation_change_mid_request() {
    let (mut r, _ctl, _rec) = setup(|req: &[u8]| match req {
        [0x2E, ..] => vec![FakeStep::at(20, FakeAction::BumpGeneration)],
        _ => vec![FakeStep::respond(5, TP_OK)],
    });
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(res.status, TxnStatus::GenerationChanged);
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert_eq!(res.error, Some(ErrorCode::ConnectionChanged));
    let res = r.execute(read(TP, 2000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.generation, 1);
}

#[tokio::test(start_paused = true)]
async fn generation_change_read_is_failed() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| vec![FakeStep::at(20, FakeAction::BumpGeneration)]);
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert_eq!(res.status, TxnStatus::GenerationChanged);
    assert_eq!(res.write_state, WriteState::Failed);
}

#[tokio::test(start_paused = true)]
async fn link_loss_mid_write_is_outcome_unknown() {
    let (mut r, ctl, _rec) = setup(|_req: &[u8]| vec![FakeStep::at(20, FakeAction::Close)]);
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(res.status, TxnStatus::LinkLost);
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert_eq!(res.error, Some(ErrorCode::Disconnected));
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Rejected(RejectReason::NotSent(TransportError::Closed))
    );
    assert_eq!(res.write_state, WriteState::Failed);
    assert_eq!(ctl.sent().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn transport_failures() {
    let (mut r, ctl, _rec) = setup(ecu(5));
    ctl.fail_next_submit(TransportError::TxFull);
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Rejected(RejectReason::NotSent(TransportError::TxFull))
    );
    assert_eq!(
        (res.write_state, res.sent, res.error),
        (WriteState::Failed, false, Some(ErrorCode::Busy))
    );

    ctl.fail_next_confirm(TransportError::Nack(0x06));
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(res.status, TxnStatus::TransportNack { code: 0x06 });
    assert_eq!(res.write_state, WriteState::Failed);

    ctl.fail_next_confirm(TransportError::AckTimeout);
    let res = r.execute(write(WRITE_DID, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::Ack
        }
    );
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert!(r.unresolved().overlaps(0x2E));

    let res = r.execute(read(&[], 2000)).await;
    assert_eq!(res.status, TxnStatus::Rejected(RejectReason::InvalidPdu));
}

#[tokio::test(start_paused = true)]
async fn sprmib_without_response_is_ok_suppressed() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| vec![]);
    let start = Instant::now();
    let res = r.execute(write(&[0x3E, 0x80], 2000)).await;
    assert_eq!(
        start.elapsed(),
        Duration::from_millis(50),
        "waits exactly P2 for an NRC"
    );
    assert_eq!(res.status, TxnStatus::OkSuppressed);
    assert_eq!(res.write_state, WriteState::Sent, "suppressed is not confirmed");
    assert_eq!(res.error, None);
    assert!(r.unresolved().is_empty());
}

#[tokio::test(start_paused = true)]
async fn sprmib_nrc_and_pending_still_reported() {
    let (mut r, _ctl, _rec) = setup(|req: &[u8]| match req {
        [0x10, 0x83] => vec![FakeStep::respond(5, &[0x7F, 0x10, 0x22])],
        [0x10, 0x82] => vec![
            FakeStep::respond(5, &[0x7F, 0x10, 0x78]),
            FakeStep::respond(1000, &[0x50, 0x02, 0x00, 0x32, 0x01, 0xF4]),
        ],
        _ => vec![],
    });
    let res = r.execute(write(&[0x10, 0x83], 2000)).await;
    assert_eq!(res.status, TxnStatus::Negative { nrc: 0x22 });
    // After 0x78 the final response is required, even with SPRMIB.
    let res = r.execute(write(&[0x10, 0x82], 5000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.pending_count, 1);
}

#[tokio::test(start_paused = true)]
async fn echo_mismatch_is_never_the_answer() {
    let (mut r, _ctl, rec) = setup(|_req: &[u8]| {
        vec![
            FakeStep::respond(10, &[0x62, 0xF1, 0x91, 0x00]),
            FakeStep::respond(20, &[0x7F, 0x2E, 0x31]),
        ]
    });
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    assert_eq!(res.late_count, 2);
    assert_eq!(rec.events(), vec!["sent:22", "late:62:false", "late:7F:false"]);
}

#[tokio::test(start_paused = true)]
async fn completion_limit_after_rx_start() {
    let (mut r, _ctl, _rec) = setup(|_req: &[u8]| vec![FakeStep::at(10, FakeAction::RxStart)]);
    let start = Instant::now();
    let res = r.execute(read(READ_VIN, 5000)).await;
    assert_eq!(start.elapsed(), Duration::from_millis(1010));
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::Completion
        }
    );
    assert_eq!(
        res.times.first_rx_mono_ns.map(|v| v - 1_000_000_000),
        Some(10 * MS)
    );
}

// ---- quarantine (plan §3.4 D19) ----

#[tokio::test(start_paused = true)]
async fn late_response_is_marked_late_and_releases() {
    // 22 F190 answers after 300 ms (P2 = 50 ms): timeout, then the answer arrives late.
    let (mut r, _ctl, rec) = setup(ecu(300));
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    // Non-overlapping request: waits the quiet period max(P2*, 1 s) = 5 s, sees the late answer.
    let start = Instant::now();
    let res = r.execute(read(TP, 10_000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.response.as_deref(), Some(TP_OK));
    assert_eq!(res.late_count, 1);
    assert_eq!(
        start.elapsed(),
        Duration::from_millis(255),
        "U emptied: quiet wait ends early"
    );
    assert!(r.unresolved().is_empty());
    let ev = rec.events();
    assert!(ev.contains(&"late:62:true".to_string()), "{ev:?}");
    assert!(ev.contains(&"released:1:LateFinalResponse".to_string()), "{ev:?}");
}

#[tokio::test(start_paused = true)]
async fn counterexample_same_sid_other_did_is_quarantined() {
    // Why echo checks are not enough: a late `7F 22 31` meant for F190 would classify as the
    // NRC of a new F191 request.
    assert_eq!(
        classify_response(&[0x22, 0xF1, 0x91], &[0x7F, 0x22, 0x31]),
        ResponseKind::Negative { nrc: 0x31 }
    );
    let (mut r, ctl, _rec) = setup(ecu(NEVER));
    let res = r.execute(read(READ_VIN, 2000)).await;
    assert!(res.timeout);
    let res = r.execute(read(&[0x22, 0xF1, 0x91], 60_000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Rejected(RejectReason::Quarantine { sid: 0x22 })
    );
    assert_eq!(res.error, Some(ErrorCode::Quarantine));
    assert_eq!(res.write_state, WriteState::Failed);
    assert!(!res.sent);
    assert_eq!(ctl.sent().len(), 1, "quarantined request never reaches the link");
}

#[tokio::test(start_paused = true)]
async fn late_pending_does_not_release_final_does() {
    let (mut r, ctl, _rec) = setup(ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    ctl.inject(
        Duration::from_millis(10),
        FakeAction::Respond(vec![0x7F, 0x22, 0x78]),
    );
    let res = r.execute(read(TP, 10_000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert!(r.unresolved().overlaps(0x22), "0x78 is not a final response");
    ctl.inject(Duration::ZERO, FakeAction::Respond(vec![0x7F, 0x22, 0x31]));
    let res = r.execute(read(TP, 10_000)).await;
    assert_eq!(res.late_count, 1);
    assert!(r.unresolved().is_empty(), "late final NRC releases");
    assert!(
        r.execute(read(&[0x22, 0xF1, 0x91], 2000)).await.sent,
        "released: same SID allowed again"
    );
}

#[tokio::test(start_paused = true)]
async fn probe_success_does_not_release() {
    let (mut r, _ctl, _rec) = setup(ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    let probe = r.probe(Duration::from_secs(10)).await.expect("a candidate");
    assert_eq!(probe.status, TxnStatus::Positive, "ECU alive");
    assert!(
        r.unresolved().overlaps(0x22),
        "probe success is not release evidence"
    );
    let res = r.execute(read(&[0x22, 0xF1, 0x91], 10_000)).await;
    assert_eq!(
        res.status,
        TxnStatus::Rejected(RejectReason::Quarantine { sid: 0x22 })
    );
}

#[tokio::test(start_paused = true)]
async fn timed_out_probe_is_not_reused() {
    let (mut r, ctl, _rec) = setup(|_req: &[u8]| vec![]);
    let p1 = r.probe(Duration::from_secs(10)).await.expect("first");
    assert!(p1.timeout);
    let p2 = r.probe(Duration::from_secs(10)).await.expect("second");
    assert!(p2.timeout);
    assert_eq!(ctl.sent(), vec![vec![0x3E, 0x00], vec![0x22, 0xF1, 0x86]]);
    assert!(
        r.probe(Duration::from_secs(10)).await.is_none(),
        "every candidate overlaps U"
    );
}

#[tokio::test(start_paused = true)]
async fn reconnect_releases_quarantine() {
    let (mut r, ctl, rec) = setup(ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    ctl.bump_generation();
    let res = r.execute(read(&[0x22, 0xF1, 0x91], 2000)).await;
    assert!(res.sent, "new generation: U released, request sent");
    assert!(rec.events().contains(&"released:1:GenerationChanged".to_string()));
}

#[tokio::test(start_paused = true)]
async fn confirmed_ecu_reset_releases_quarantine() {
    let (mut r, _ctl, rec) = setup(ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    let res = r.execute(write(&[0x11, 0x01], 10_000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert!(r.unresolved().is_empty());
    assert!(rec.events().contains(&"released:1:EcuReset".to_string()));
}

#[tokio::test(start_paused = true)]
async fn verified_late_window_releases_quarantine() {
    let cfg = RunnerConfig {
        late_response_max: Some(Duration::from_secs(3)),
        ..RunnerConfig::default()
    };
    let (mut r, _ctl, _rec) = setup_cfg(cfg, ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let res = r.execute(read(&[0x22, 0xF1, 0x91], 2000)).await;
    assert!(res.sent);
}

#[tokio::test(start_paused = true)]
async fn quiet_period_longer_than_deadline_is_refused() {
    let (mut r, ctl, _rec) = setup(ecu(NEVER));
    r.execute(read(READ_VIN, 2000)).await;
    let res = r.execute(read(TP, 1000)).await;
    assert_eq!(res.status, TxnStatus::Rejected(RejectReason::QuarantineQuiet));
    assert_eq!(res.error, Some(ErrorCode::Quarantine));
    assert_eq!(ctl.sent().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn late_answer_during_next_request_is_not_used() {
    // F190's answer lands at 5.5 s, while the next (non-overlapping) request is in flight.
    let (mut r, _ctl, _rec) = setup(|req: &[u8]| match req {
        [0x22, 0xF1, 0x90] => vec![FakeStep::respond(5500, VIN_OK)],
        [0x31, ..] => vec![
            FakeStep::respond(5, &[0x7F, 0x31, 0x78]),
            FakeStep::respond(600, &[0x71, 0x01, 0xFF, 0x00]),
        ],
        _ => vec![],
    });
    r.execute(read(READ_VIN, 2000)).await;
    let res = r.execute(write(&[0x31, 0x01, 0xFF, 0x00], 20_000)).await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.response.as_deref(), Some(&[0x71, 0x01, 0xFF, 0x00][..]));
    assert_eq!(res.late_count, 1);
    assert!(r.unresolved().is_empty());
}

#[test]
fn unresolved_capacity_is_bounded_and_never_evicts() {
    let mut u = Unresolved::default();
    let now = Instant::now();
    for i in 0..MAX_UNRESOLVED_PER_TARGET {
        assert!(u.insert(&[0x22, 0xF1, i.to_le_bytes()[0]], now));
    }
    assert!(u.is_full());
    assert!(!u.insert(&[0x3E, 0x00], now), "full: refuse, do not evict");
    assert!(u.overlaps(0x22) && !u.overlaps(0x3E));
    assert_eq!(u.release_by_response(&[0x7F, 0x22, 0x78]), None);
    assert_eq!(u.release_by_response(&[0x7F, 0x22, 0x31]), Some(0x22));
    assert_eq!(u.len(), MAX_UNRESOLVED_PER_TARGET - 1);
    assert_eq!(ReleaseCause::LateFinalResponse, ReleaseCause::LateFinalResponse);
}

// ---- queue ----

#[tokio::test(start_paused = true)]
async fn queue_full_is_busy() {
    let (r, _ctl, _rec) = setup(|_req: &[u8]| {
        vec![
            FakeStep::respond(10, &[0x7F, 0x31, 0x78]),
            FakeStep::respond(1000, &[0x71, 0x01, 0xFF, 0x00]),
        ]
    });
    let (h, _task) = RunnerHandle::spawn(r);
    let first = h
        .try_submit(write(&[0x31, 0x01, 0xFF, 0x00], 60_000))
        .expect("first");
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(1)).await;
    let mut queued = Vec::new();
    for _ in 0..MAX_PENDING_PER_TARGET {
        queued.push(
            h.try_submit(read(&[0x31, 0x01, 0xFF, 0x00], 60_000))
                .expect("fits"),
        );
    }
    let busy = h.try_submit(read(TP, 60_000)).expect_err("queue full");
    assert_eq!(busy.status, TxnStatus::Rejected(RejectReason::QueueFull));
    assert_eq!(busy.error, Some(ErrorCode::Busy));
    assert_eq!(first.await.expect("first done").status, TxnStatus::Positive);
    // Serial: each result's send time is after the previous final response.
    let mut last_final = 0;
    for rx in queued {
        let res = rx.await.expect("done");
        assert_eq!(res.status, TxnStatus::Positive);
        let sent = res.times.sent_mono_ns.expect("sent");
        assert!(sent >= last_final);
        last_final = res.times.final_rx_mono_ns.expect("final");
    }
}

// ---- random responses ----

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[tokio::test(start_paused = true)]
async fn random_responses_never_confuse_the_runner() {
    let rng = Arc::new(Mutex::new(Rng(0xD2)));
    let script_rng = rng.clone();
    let (mut r, ctl, _rec) = setup(move |req: &[u8]| {
        let mut g = script_rng.lock().expect("rng");
        let n = (g.next() % 3) as usize;
        (0..n)
            .map(|_| {
                let len = (g.next() % 6) as usize;
                let mut b: Vec<u8> = (0..len).map(|_| g.next().to_le_bytes()[0]).collect();
                if g.next().is_multiple_of(2) && !b.is_empty() {
                    b[0] = req[0].wrapping_add(0x40); // plausible positive SID
                }
                FakeStep::respond(g.next() % 80, &b)
            })
            .collect()
    });
    let reqs: [&[u8]; 4] = [READ_VIN, TP, &[0x10, 0x03], &[0x22, 0xF1, 0x86]];
    for i in 0..2000usize {
        if i % 50 == 0 {
            ctl.bump_generation(); // keep U from blocking everything
        }
        let req = reqs[i % reqs.len()];
        let res = r.execute(read(req, 200)).await;
        match &res.status {
            TxnStatus::Positive => {
                let resp = res.response.as_deref().expect("response");
                assert_eq!(classify_response(req, resp), ResponseKind::Positive);
                assert_eq!(res.write_state, WriteState::Confirmed);
            }
            TxnStatus::Negative { nrc } => assert_ne!(*nrc, 0x78),
            _ => assert_ne!(res.write_state, WriteState::Confirmed),
        }
    }
    let _ = rng;
}
