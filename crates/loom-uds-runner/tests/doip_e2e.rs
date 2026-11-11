//! End-to-end: runner → DoipTransport → DoIP client → loopback TCP → DoipEntity → handler.
//! Real time (loopback), generous P2 so the tests do not depend on machine load.

use std::sync::Arc;
use std::time::Duration;

use loom_proto::event::WriteState;
use loom_uds_doip::frame::diag_code;
use loom_uds_doip::{ClientConfig, DoipClient, DoipEntity, EntityConfig, EntityFault};
use loom_uds_runner::{
    DoipTransport, MonoClock, NoopObserver, Runner, RunnerConfig, RunnerHandle, TimeoutPhase, TxnRequest,
    TxnStatus,
};

const TESTER: u16 = 0x0E00;
const ECU: u16 = 0x0E80;

fn handler(_target: u16, _now: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)> {
    match req {
        [0x22, 0xF1, 0x90] => vec![(Duration::ZERO, b"\x62\xF1\x90LOOMSIM0000000001".to_vec())],
        [0x22, 0xF1, 0x91] => vec![(Duration::from_millis(300), vec![0x62, 0xF1, 0x91, 0x01])],
        [0x2E, 0xF1, 0x90, ..] => vec![
            (Duration::ZERO, vec![0x7F, 0x2E, 0x78]),
            (Duration::from_millis(100), vec![0x6E, 0xF1, 0x90]),
        ],
        [0x3E, 0x00] => vec![(Duration::ZERO, vec![0x7E, 0x00])],
        _ => vec![],
    }
}

fn cfg() -> RunnerConfig {
    RunnerConfig {
        p2: Duration::from_millis(200),
        p2_star: Duration::from_millis(500),
        quarantine_quiet: Duration::from_millis(500),
        ..RunnerConfig::default()
    }
}

async fn setup(ack_timeout: Duration) -> (DoipEntity, DoipClient, Runner) {
    let entity = DoipEntity::start(EntityConfig::loopback(0x1000, vec![ECU]), handler)
        .await
        .expect("entity");
    let mut cc = ClientConfig::new(entity.tcp_addr(), TESTER);
    cc.ack_timeout = ack_timeout;
    let (client, _events) = DoipClient::connect(cc).await.expect("connect");
    let target = client.open_target(ECU).expect("open");
    let runner = Runner::new(
        Box::new(DoipTransport::new(target)),
        cfg(),
        MonoClock::starting_now(),
        Arc::new(NoopObserver),
    );
    (entity, client, runner)
}

#[tokio::test]
async fn read_write_and_suppressed_over_doip() {
    let (_entity, _client, runner) = setup(Duration::from_secs(1)).await;
    let (h, _task) = RunnerHandle::spawn(runner);

    let res = h
        .request(TxnRequest::read(vec![0x22, 0xF1, 0x90], Duration::from_secs(2)))
        .await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.write_state, WriteState::Confirmed);
    assert_eq!(
        res.response.as_deref().map(|r| &r[3..]),
        Some(&b"LOOMSIM0000000001"[..])
    );
    let t = res.times;
    let (sent, p2, fin) = (
        t.sent_mono_ns.expect("sent"),
        t.p2_start_mono_ns.expect("ack"),
        t.final_rx_mono_ns.expect("final"),
    );
    assert!(t.received_mono_ns <= sent && sent <= p2 && p2 <= fin, "{t:?}");

    let res = h
        .request(TxnRequest::write(
            vec![0x2E, 0xF1, 0x90, 0x01],
            Duration::from_secs(2),
        ))
        .await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.pending_count, 1);
    assert_eq!(res.write_state, WriteState::Confirmed);

    let res = h
        .request(TxnRequest::write(vec![0x3E, 0x80], Duration::from_secs(2)))
        .await;
    assert_eq!(res.status, TxnStatus::OkSuppressed);
    assert_eq!(res.write_state, WriteState::Sent);
}

#[tokio::test]
async fn nack_ack_loss_and_link_loss_over_doip() {
    let (entity, _client, mut runner) = setup(Duration::from_millis(150)).await;

    entity
        .inject(EntityFault::NackNextDiag(diag_code::TARGET_UNREACHABLE))
        .expect("inject");
    let res = runner
        .execute(TxnRequest::write(
            vec![0x2E, 0xF1, 0x90, 0x01],
            Duration::from_secs(2),
        ))
        .await;
    assert_eq!(res.status, TxnStatus::TransportNack { code: 0x06 });
    assert_eq!(res.write_state, WriteState::Failed);

    entity.inject(EntityFault::DropNextAck).expect("inject");
    let res = runner
        .execute(TxnRequest::write(
            vec![0x2E, 0xF1, 0x90, 0x01],
            Duration::from_secs(2),
        ))
        .await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::Ack
        }
    );
    assert_eq!(res.write_state, WriteState::OutcomeUnknown);
    assert!(runner.unresolved().overlaps(0x2E));

    entity.inject(EntityFault::CloseOnNextDiag).expect("inject");
    let res = runner
        .execute(TxnRequest::read(vec![0x22, 0xF1, 0x90], Duration::from_secs(3)))
        .await;
    assert_eq!(res.status, TxnStatus::LinkLost);
    assert_eq!(res.write_state, WriteState::Failed, "read + link loss = failed");
}

#[tokio::test]
async fn late_response_over_doip_is_quarantined_then_released() {
    let (_entity, _client, mut runner) = setup(Duration::from_secs(1)).await;
    // Answer at 300 ms > P2 200 ms: timeout, entry in U.
    let res = runner
        .execute(TxnRequest::read(vec![0x22, 0xF1, 0x91], Duration::from_secs(2)))
        .await;
    assert_eq!(
        res.status,
        TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        }
    );
    // Same SID is refused while unresolved.
    let res = runner
        .execute(TxnRequest::read(vec![0x22, 0xF1, 0x90], Duration::from_secs(2)))
        .await;
    assert!(matches!(res.status, TxnStatus::Rejected(_)), "{:?}", res.status);
    // A non-overlapping request waits the quiet period, sees the late answer (released), proceeds.
    let res = runner
        .execute(TxnRequest::read(vec![0x3E, 0x00], Duration::from_secs(2)))
        .await;
    assert_eq!(res.status, TxnStatus::Positive);
    assert_eq!(res.late_count, 1);
    assert!(runner.unresolved().is_empty());
    let res = runner
        .execute(TxnRequest::read(vec![0x22, 0xF1, 0x90], Duration::from_secs(2)))
        .await;
    assert_eq!(res.status, TxnStatus::Positive);
}
