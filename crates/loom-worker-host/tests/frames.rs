//! A fake gateway sends broken frames to a real worker: connection error → latch → orphan exit.
#![allow(clippy::unwrap_used)]
#![cfg(unix)]

mod common;

use std::time::Duration;

use common::{TEST_WORKER, alive, sock_dir, wait_until};
use loom_proto::num::U64;
use loom_proto::worker::{Input, JobRenew, Welcome, WorkerBudgets, WorkerMsg};
use loom_worker_host::LaneListeners;
use loom_worker_sdk::transport::{FrameReader, FrameWriter};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negotiated_minor_controls_explicit_ids_and_preserves_missing_id_ordinals() {
    for minor in [0, 1] {
        let dir = sock_dir();
        let instance = format!("t-minor-{minor}");
        let listeners = LaneListeners::bind(dir.path(), &instance).unwrap();
        let mut child = tokio::process::Command::new(TEST_WORKER)
            .args(["--instance", &instance, "--producer-epoch", "1", "--sock-dir"])
            .arg(dir.path())
            .args(["--orphan-ms", "200", "--health-ms", "100"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let ((cr, cw), (_dr, _dw)) = listeners
            .accept(child.id(), Duration::from_secs(5))
            .await
            .unwrap();
        let mut r = FrameReader::new(cr);
        let mut w = FrameWriter::new(cw);
        assert!(matches!(
            r.read(Some(Duration::from_secs(5))).await.unwrap().unwrap().0,
            WorkerMsg::Hello(_)
        ));
        w.write(&WorkerMsg::Welcome(Welcome {
            accepted_minor: minor,
            stream_epoch: U64(1),
            config: json!({}),
            budgets: WorkerBudgets {
                mem_soft_bytes: U64(0),
                mem_hard_bytes: U64(0),
                credit_bytes: U64(1 << 20),
            },
            latch: false,
        }))
        .await
        .unwrap();
        // priority Input도 일반 요청과 같은 물리 순번을 소비해야 한다.
        let requests = [
            WorkerMsg::JobRenew(JobRenew {
                call_id: Some(9001),
                job_id: "j".into(),
                lease_ms: 100,
            }),
            WorkerMsg::Input(Input {
                conn_id: None,
                channel: None,
                input_seq: None,
                call_id: Some(0),
                lease_id: "l".into(),
                bytes: loom_proto::bytes::Bytes(vec![3]),
                priority: true,
            }),
            WorkerMsg::JobRenew(JobRenew {
                call_id: None,
                job_id: "j".into(),
                lease_ms: 200,
            }),
            WorkerMsg::Input(Input {
                conn_id: None,
                channel: None,
                input_seq: None,
                call_id: None,
                lease_id: "l".into(),
                bytes: loom_proto::bytes::Bytes(vec![3]),
                priority: false,
            }),
        ];
        let expected = if minor == 0 { [1, 2, 3, 4] } else { [9001, 0, 3, 4] };
        for (request, expected_id) in requests.into_iter().zip(expected) {
            w.write(&request).await.unwrap();
            loop {
                let (msg, _) = r.read(Some(Duration::from_secs(5))).await.unwrap().unwrap();
                if let WorkerMsg::Reply(reply) = msg {
                    assert_eq!(reply.call_id.0, expected_id, "minor {minor}");
                    assert!(matches!(reply.result, loom_proto::worker::ReplyResult::Ok(_)));
                    break;
                }
            }
        }
        w.shutdown().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

async fn handshake_then_send(raw: &[u8], instance: &str, close: bool) {
    let dir = sock_dir();
    let latch_file = dir.path().join("latch.log");
    let listeners = LaneListeners::bind(dir.path(), instance).unwrap();
    let mut child = tokio::process::Command::new(TEST_WORKER)
        .args(["--instance", instance, "--producer-epoch", "1", "--sock-dir"])
        .arg(dir.path())
        .args(["--orphan-ms", "200", "--health-ms", "100", "--latch-file"])
        .arg(&latch_file)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let ((cr, cw), (_dr, _dw)) = listeners.accept(Some(pid), Duration::from_secs(5)).await.unwrap();
    let mut r = FrameReader::new(cr);
    let mut w = FrameWriter::new(cw);
    let (hello, _) = r.read(Some(Duration::from_secs(5))).await.unwrap().unwrap();
    assert!(matches!(hello, WorkerMsg::Hello(_)));
    w.write(&WorkerMsg::Welcome(Welcome {
        accepted_minor: 0,
        stream_epoch: U64(1),
        config: json!({}),
        budgets: WorkerBudgets {
            mem_soft_bytes: U64(0),
            mem_hard_bytes: U64(0),
            credit_bytes: U64(1 << 20),
        },
        latch: false,
    }))
    .await
    .unwrap();
    // First health proves the worker is serving.
    loop {
        let (m, _) = r.read(Some(Duration::from_secs(5))).await.unwrap().unwrap();
        if matches!(m, WorkerMsg::Health(_)) {
            break;
        }
    }
    w.write_raw(raw).await.unwrap();
    if close {
        w.shutdown().await.unwrap();
    }
    wait_until(Duration::from_secs(5), || {
        std::fs::read_to_string(&latch_file).is_ok_and(|t| t.contains("on"))
    })
    .await;
    let st = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(st.success(), "orphan exit is clean: {st:?}");
    assert!(!alive(pid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversize_frame_from_gateway_is_a_connection_error() {
    // len = 1 MiB, type Call: rejected from the header alone.
    handshake_then_send(&[0, 0, 0x10, 0, 10, 0, 0, 0], "t-big", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_type_from_gateway_is_a_connection_error() {
    handshake_then_send(&[0, 0, 0, 0, 77, 0, 0, 0], "t-type", false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_frame_from_gateway_is_a_connection_error() {
    // Header promises 100 bytes, 4 arrive, then the gateway closes mid-frame. (A peer that just
    // stops sending hits the 5 s read deadline instead — covered in the SDK transport tests.)
    handshake_then_send(&[100, 0, 0, 0, 10, 0, 0, 0, 1, 2, 3, 4], "t-trunc", true).await;
}
