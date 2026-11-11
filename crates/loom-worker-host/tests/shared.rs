//! 새 공유 런타임·minor 2 필드·tick·종료 통지의 실제 프로세스 왕복.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use loom_proto::bytes::Bytes;
use loom_proto::event::Origin;
use loom_proto::num::U64;
use loom_proto::worker::{Input, InputLease, InputLeaseEnd, InputLeaseEndCause, Lane, Latch};
use loom_worker_host::WorkerState;
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_runtime_preserves_wire_fields_and_services_control_while_call_waits() {
    let dir = sock_dir();
    let mut cfg = config(dir.path(), "t-shared", &[]);
    cfg.program = env!("CARGO_BIN_EXE_loom-test-shared-worker").into();
    cfg.config = json!({"tick_ms":10});
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, Duration::from_secs(5)).await;
    let host = s.host.clone();
    let long = tokio::spawn(async move { host.call(call("sleep", json!({}))).await });
    wait_until(Duration::from_secs(1), || {
        !s.sink.kinds(Lane::Ctl, "test.shared_started").is_empty()
    })
    .await;
    s.host
        .input_lease(InputLease {
            lease_id: "l".into(),
            conn_id: Some("c7".into()),
            channel: "dut".into(),
            owner: Origin::User,
            client_id: Some("viewer".into()),
        })
        .await
        .unwrap();
    let input = Input {
        call_id: Some(9001),
        conn_id: Some("c7".into()),
        channel: Some("dut".into()),
        input_seq: Some(U64(33)),
        lease_id: "l".into(),
        bytes: Bytes(vec![3]),
        priority: true,
    };
    let begin = Instant::now();
    s.host.input(input).await.unwrap();
    assert!(
        begin.elapsed() < Duration::from_millis(100),
        "실제 IPC 왕복도 긴 호출을 기다리지 않는다"
    );
    s.host
        .latch(Latch {
            on: true,
            scope: "all".into(),
            reason: None,
            latch_seq: U64(10),
        })
        .await
        .unwrap();
    s.host
        .input_lease_end(InputLeaseEnd {
            lease_id: "l".into(),
            conn_id: Some("c7".into()),
            cause: InputLeaseEndCause::ConnClosed,
        })
        .await
        .unwrap();
    wait_until(Duration::from_secs(1), || {
        !s.sink.kinds(Lane::Ctl, "test.shared_end").is_empty()
    })
    .await;
    assert!(!long.is_finished());
    let row = &s.sink.kinds(Lane::Ctl, "test.shared_input")[0];
    assert_eq!(row.payload["conn_id"], "c7");
    assert_eq!(row.payload["channel"], "dut");
    assert_eq!(row.payload["input_seq"], "33");
    assert_eq!(
        s.sink.kinds(Lane::Ctl, "test.shared_end")[0].payload["cause"],
        "conn_closed"
    );
    assert_eq!(long.await.unwrap().unwrap(), json!({"slept":5}));
    let mut query = call("stats", json!({}));
    query.conn_id = Some("c8".into());
    let stats = s.host.call(query).await.unwrap();
    assert_eq!(stats["conn_id"], "c8");
    assert_eq!(stats["latched"], true);
    assert!(stats["ticks"].as_u64().unwrap() >= 3);
    // 종료 통지가 요청 순번을 소비하면 이 생략 ID Input의 Reply가 다른 id로 와서 멈춘다.
    s.host
        .input(Input {
            call_id: None,
            conn_id: None,
            channel: None,
            input_seq: None,
            lease_id: "next".into(),
            bytes: Bytes(vec![1]),
            priority: false,
        })
        .await
        .unwrap();
    s.host.shutdown(true).await;
}
