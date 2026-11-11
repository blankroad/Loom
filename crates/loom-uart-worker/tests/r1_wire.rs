//! CORE R1 adapter tests. Uses the real SharedWorker trait and Emitter; no mock SDK API.
//! These are in-process tests, not proof that loomd selects the concurrent runtime.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loom_proto::bytes::Bytes;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, SourceRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Input, InputLeaseEnd, InputLeaseEndCause, Lane, Write};
use loom_uart_worker::bridge::LostRange;
use loom_uart_worker::channel::{ChannelCore, Profile};
use loom_uart_worker::worker::UartWorker;
use loom_worker_sdk::{EmitError, EmitSink, Emitter, LatchFlag, SharedWorker, SystemClock, WorkerCtx};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(2);
// Test capture is bounded too (§3.13); overflow fails the test rather than hiding a record.
const CAPTURE_RECORDS: usize = 2048;

#[derive(Debug, Default)]
struct Capture {
    records: Mutex<Vec<(Lane, SourceRecord)>>,
    full_after_accept: AtomicBool,
}

impl EmitSink for Capture {
    fn try_push(&self, lane: Lane, record: SourceRecord) -> Result<(), EmitError> {
        let is_data = record.kind == "uart.data";
        let mut records = self.records.lock().expect("capture lock");
        assert!(records.len() < CAPTURE_RECORDS, "test capture overflow");
        records.push((lane, record));
        if is_data && self.full_after_accept.swap(false, Ordering::Relaxed) {
            // Mirrors a journal acceptance followed by a full display queue.
            Err(EmitError::Full)
        } else {
            Ok(())
        }
    }
}

fn rig() -> (UartWorker, WorkerCtx, Arc<Capture>) {
    let sink = Arc::new(Capture::default());
    let clock = Arc::new(SystemClock::new());
    let base = Emitter::new(
        "r1",
        "uart",
        "uart.dut",
        U64(7),
        100,
        "dut",
        sink.clone(),
        clock.clone(),
    );
    let cx = WorkerCtx::new(LatchFlag::new(), clock, Arc::new(move |ch| base.for_channel(ch)));
    (UartWorker::new(), cx, sink)
}

fn call(cx: &WorkerCtx, method: &str, args: Value) -> Call {
    Call {
        conn_id: Some("ws-a".into()),
        call_id: U64(1),
        method: method.into(),
        args,
        origin: Origin::User,
        client_id: Some("different-client-id".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + 2_000_000_000),
        approval_ref: None,
    }
}

async fn query(worker: &UartWorker, cx: &WorkerCtx, method: &str, args: Value) -> Value {
    SharedWorker::on_call(worker, cx.clone(), call(cx, method, args))
        .await
        .expect("query")
}

async fn write(worker: &UartWorker, cx: &WorkerCtx, op: Value) -> loom_worker_sdk::WriteOutcome {
    SharedWorker::on_write(
        worker,
        cx.clone(),
        Write {
            call_id: U64(2),
            request_id: "request-r1".into(),
            write_class: WriteClass::Request,
            op,
            origin: Origin::Agent,
            client_id: Some("agent-r1".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(cx.clock().mono_ns() + 2_000_000_000),
        },
    )
    .await
}

async fn open(worker: &UartWorker, cx: &WorkerCtx) {
    let result = write(
        worker,
        cx,
        json!({"method":"uart.open", "target":"sim://binary?split=0", "alias":"dut"}),
    )
    .await;
    assert_eq!(result.state, WriteState::Sent, "{:?}", result.error);
}

async fn close(worker: &UartWorker, cx: &WorkerCtx) {
    let result = write(worker, cx, json!({"method":"uart.close", "channel":"dut"})).await;
    assert_eq!(result.state, WriteState::Sent, "{:?}", result.error);
    assert_eq!(result.detail["threads_left"], 0);
}

fn input(seq: u64, bytes: &[u8]) -> Input {
    Input {
        call_id: None,
        conn_id: Some("ws-a".into()),
        channel: Some("dut".into()),
        input_seq: Some(U64(seq)),
        lease_id: "opaque-lease-not-a-viewer".into(),
        bytes: Bytes(bytes.to_vec()),
        priority: bytes.contains(&3),
    }
}

#[tokio::test]
async fn input_uses_connection_and_client_sequence_and_lease_end_keeps_dirty_line() {
    let (worker, cx, sink) = rig();
    open(&worker, &cx).await;
    let seq = 9_007_199_254_740_993;
    SharedWorker::on_input(&worker, cx.clone(), input(seq, b"half-line"))
        .await
        .expect("input");
    let status = query(&worker, &cx, "uart.status", json!({"channel":"dut"})).await;
    assert_eq!(status["input"]["viewer"], "ws-a");
    assert_eq!(status["input_dirty"], true);
    assert!(sink.records.lock().expect("capture").iter().any(|(_, r)| {
        r.kind == "uart.input_ack" && r.payload["input_seq"].as_str() == Some(seq.to_string().as_str())
    }));

    let mut wrong = input(2, b"x");
    wrong.channel = Some("other".into());
    assert_eq!(
        SharedWorker::on_input(&worker, cx.clone(), wrong)
            .await
            .expect_err("wrong worker")
            .code,
        ErrorCode::NotOpen
    );
    let mut missing = input(3, b"x");
    missing.input_seq = None;
    assert_eq!(
        SharedWorker::on_input(&worker, cx.clone(), missing)
            .await
            .expect_err("missing sequence")
            .code,
        ErrorCode::Usage
    );
    let mut missing = input(4, b"x");
    missing.conn_id = None;
    assert_eq!(
        SharedWorker::on_input(&worker, cx.clone(), missing)
            .await
            .expect_err("missing connection")
            .code,
        ErrorCode::Usage
    );

    SharedWorker::on_input_lease_end(
        &worker,
        cx.clone(),
        InputLeaseEnd {
            lease_id: "opaque-lease-not-a-viewer".into(),
            conn_id: Some("ws-a".into()),
            cause: InputLeaseEndCause::ConnClosed,
        },
    )
    .await;
    let status = query(&worker, &cx, "uart.status", json!({"channel":"dut"})).await;
    assert_eq!(status["input"]["owner"], "none");
    assert_eq!(status["input_dirty"], true);
    // Control calls use conn_id too, although client_id differs.
    let cleared = query(&worker, &cx, "uart.input.clear", json!({"channel":"dut"})).await;
    assert_eq!(cleared["cleared"], true);
    close(&worker, &cx).await;
}

#[tokio::test]
async fn shared_input_and_latch_run_while_expect_is_pending() {
    let (worker, cx, _) = rig();
    open(&worker, &cx).await;
    let pending = tokio::spawn(SharedWorker::on_call(
        &worker,
        cx.clone(),
        call(
            &cx,
            "uart.expect",
            json!({
                "channel":"dut", "patterns":[{"kind":"literal", "pattern":"never-r1-match"}], "timeout_ms":1500,
            }),
        ),
    ));
    tokio::time::timeout(WAIT, async {
        loop {
            let status = query(&worker, &cx, "uart.status", json!({"channel":"dut"})).await;
            if status["expect"]["pending"] == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expect registered");
    tokio::time::timeout(
        Duration::from_millis(100),
        SharedWorker::on_input(&worker, cx.clone(), input(71, &[3])),
    )
    .await
    .expect("input bypasses expect")
    .expect("Ctrl-C accepted");
    cx.latch().set(true);
    tokio::time::timeout(Duration::from_millis(100), SharedWorker::on_latch(&worker, true))
        .await
        .expect("latch bypasses expect");
    assert!(!pending.is_finished(), "expect must still be pending");
    close(&worker, &cx).await;
    assert_eq!(
        pending
            .await
            .expect("expect task")
            .expect_err("close cancels expect")
            .code,
        ErrorCode::NotOpen
    );
}

#[tokio::test]
async fn ring_sequences_equal_real_data_records_even_after_accepted_full_and_reopen() {
    let (worker, cx, sink) = rig();
    sink.full_after_accept.store(true, Ordering::Relaxed);
    open(&worker, &cx).await;
    tokio::time::timeout(WAIT, async {
        loop {
            let status = query(&worker, &cx, "uart.status", json!({"channel":"dut"})).await;
            if status["emit_dropped"]["rx"] == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("accepted RX reported Full");
    let sent = write(
        &worker,
        &cx,
        json!({"method":"uart.send", "channel":"dut", "data":hex::encode("r1\n"), "eol":"none"}),
    )
    .await;
    assert_eq!(sent.state, WriteState::Sent, "{:?}", sent.error);
    // Flush the RX coalescing window deterministically by waiting for the echoed bytes.
    query(
        &worker,
        &cx,
        "uart.expect",
        json!({"channel":"dut", "patterns":[{"kind":"literal", "pattern":"r1\n"}], "timeout_ms":1000}),
    )
    .await;
    close(&worker, &cx).await;
    open(&worker, &cx).await;
    close(&worker, &cx).await;
    let lines = query(
        &worker,
        &cx,
        "uart.lines",
        json!({"channel":"dut", "from":"off:0", "limit":2000}),
    )
    .await;
    let records = sink.records.lock().expect("capture");
    assert!(
        records.windows(2).all(|w| w[0].1.source_seq < w[1].1.source_seq),
        "strict producer order, no retry"
    );
    let data: Vec<_> = records.iter().filter(|(_, r)| r.kind == "uart.data").collect();
    assert!(data.iter().any(|(_, r)| r.dir == Some(Dir::Rx)));
    let tx = data
        .iter()
        .find(|(_, r)| r.dir == Some(Dir::Tx))
        .expect("TX event");
    assert_eq!(tx.0, Lane::Data);
    assert_eq!(tx.1.origin, Origin::Agent);
    assert_eq!(tx.1.client_id.as_deref(), Some("agent-r1"));
    assert_eq!(tx.1.request_id.as_deref(), Some("request-r1"));
    assert_eq!(tx.1.env, Some(Env::Sim));
    let rows = lines["rows"].as_array().expect("line rows");
    assert!(!rows.is_empty());
    for row in rows {
        assert!(
            data.iter()
                .any(|(_, r)| row["source_seq"].as_str() == Some(r.source_seq.0.to_string().as_str())),
            "ring seq must name real event: {row}"
        );
    }
}

#[test]
fn bridge_loss_before_reservation_has_no_invented_producer_range() {
    let mut core = ChannelCore::new("dut", Profile::default(), 1);
    core.set_producer("uart.dut", 7);
    let payload = core.on_lost_unsequenced(&LostRange {
        rx_from: 0,
        rx_to: 20,
        bytes: 20,
        seq_from: 42,
        seq_to: 45,
        chunks: 4,
        cause: "reader_bridge_overflow",
    });
    assert!(payload["producer_range"].is_null());
    assert_eq!(payload["bytes"], "20");
    assert_eq!(payload["detail"]["reader_chunk_from"], "42");
    assert_eq!(core.status()["lost"]["chunks"], "4");
}
