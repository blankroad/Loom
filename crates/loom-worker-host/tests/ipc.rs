//! IPC host ↔ real worker process (`loom-test-worker`).
#![allow(clippy::unwrap_used)]
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use loom_proto::bytes::Bytes;
use loom_proto::error::ErrorCode;
use loom_proto::event::{GapCount, GapData, WriteState};
use loom_proto::num::U64;
use loom_proto::worker::{Input, JobRenew, JobStart, JobStop, JobUpdate, Lane, Latch};
use loom_worker_host::WorkerState;
use loom_worker_sdk::JobCmd;
use serde_json::json;

const T: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_end_notifications_do_not_block_serial_worker_latch_reader() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-end-latch", &[]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    let host = s.host.clone();
    let slow = tokio::spawn(async move { host.call(call("sleep", json!({"ms":400}))).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    for i in 0..3 {
        s.host
            .input_lease_end(loom_proto::worker::InputLeaseEnd {
                lease_id: format!("l{i}"),
                conn_id: Some("c1".into()),
                cause: loom_proto::worker::InputLeaseEndCause::Released,
            })
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    let begin = Instant::now();
    s.host
        .latch(Latch {
            on: true,
            scope: "all".into(),
            reason: None,
            latch_seq: U64(1),
        })
        .await
        .unwrap();
    assert!(
        begin.elapsed() < Duration::from_millis(100),
        "통지가 직렬 callback 뒤에서 ctl reader를 막으면 안 된다"
    );
    assert!(!slow.is_finished());
    slow.await.unwrap().unwrap();
    assert_eq!(s.host.call(call("echo", json!(true))).await.unwrap(), json!(true));
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_job_input_ids_mix_with_legacy_ordinals() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-ids", &[]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    let jobs = [
        (
            JobCmd::Start(JobStart {
                call_id: Some(u64::MAX),
                job_id: "j".into(),
                plan: json!({}),
                lease_ms: 1000,
                approval_ref: None,
            }),
            json!({"started":"j"}),
        ),
        (
            JobCmd::Update(JobUpdate {
                call_id: Some(0),
                job_id: "j".into(),
                plan: json!({"v":2}),
            }),
            json!({"updated":"j"}),
        ),
        (
            JobCmd::Renew(JobRenew {
                call_id: Some(9001),
                job_id: "j".into(),
                lease_ms: 2000,
            }),
            json!({"renewed":"j", "lease_ms":2000}),
        ),
        (
            JobCmd::Stop(JobStop {
                call_id: Some(9002),
                job_id: "j".into(),
                reason: "user".into(),
            }),
            json!({"stopped":"j"}),
        ),
    ];
    for (job, expected) in jobs {
        assert_eq!(s.host.job(job).await.unwrap(), expected);
        // 명시 ID가 소켓 요청 순번을 바꾸거나 응답을 다른 요청에 연결하면 실패한다.
        assert_eq!(
            s.host.call(call("echo", json!("legacy"))).await.unwrap(),
            json!("legacy")
        );
        for priority in [false, true] {
            for call_id in [Some(7000), None] {
                s.host
                    .input(Input {
                        conn_id: None,
                        channel: None,
                        input_seq: None,
                        call_id,
                        lease_id: "l".into(),
                        bytes: Bytes(vec![3]),
                        priority,
                    })
                    .await
                    .unwrap();
            }
        }
    }
    assert_eq!(s.sink.kinds(Lane::Ctl, "test.input").len(), 16);
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn roundtrip_call_write_covers_job_input() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-rt", &[]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    assert_eq!(s.host.producer_epoch(), 1);

    let v = s.host.call(call("echo", json!({"x": [1, 2, 3]}))).await.unwrap();
    assert_eq!(v, json!({"x": [1, 2, 3]}));
    let e = s.host.call(call("nope", json!({}))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownMethod);

    // Write: priority rows reach the sink before the write resolves, covers = last row.
    let (out, covers) = s.host.write_with_covers(write("req-1", data("hello"))).await;
    assert_eq!(out.state, WriteState::Confirmed, "{out:?}");
    assert_eq!(out.detail, json!({"echo": "hello"}));
    let rows = s.sink.kinds(Lane::Ctl, "test.tx");
    assert_eq!(rows.len(), 2, "rows pushed before the reply resolved");
    assert_eq!(rows[0].payload["write_state"], "sent");
    assert_eq!(rows[1].payload["write_state"], "confirmed");
    assert_eq!(rows[1].request_id.as_deref(), Some("req-1"));
    assert_eq!(covers, Some(rows[1].source_ref()));
    // The echo comes back as device RX on the data lane.
    wait_until(T, || !s.sink.kinds(Lane::Data, "test.rx").is_empty()).await;
    let rx = &s.sink.kinds(Lane::Data, "test.rx")[0];
    assert_eq!(rx.payload["data"], "hello");
    assert!(rx.source_seq > rows[1].source_seq);

    // A call that emits nothing has no covers; jobs and input get replies too.
    let j = s
        .host
        .job(JobCmd::Start(JobStart {
            call_id: None,
            job_id: "j1".into(),
            plan: json!({}),
            lease_ms: 1000,
            approval_ref: None,
        }))
        .await
        .unwrap();
    assert_eq!(j, json!({"started": "j1"}));
    let j = s
        .host
        .job(JobCmd::Stop(JobStop {
            call_id: None,
            job_id: "j1".into(),
            reason: "user".into(),
        }))
        .await
        .unwrap();
    assert_eq!(j, json!({"stopped": "j1"}));
    for priority in [false, true] {
        s.host
            .input(Input {
                conn_id: None,
                channel: None,
                input_seq: None,
                call_id: None,
                lease_id: "l1".into(),
                bytes: Bytes(vec![3]),
                priority,
            })
            .await
            .unwrap();
    }
    assert_eq!(s.sink.kinds(Lane::Ctl, "test.input").len(), 2);

    // Many concurrent calls: ids stay matched.
    let futs: Vec<_> = (0..50).map(|i| s.host.call(call("echo", json!(i)))).collect();
    for (i, f) in futs.into_iter().enumerate() {
        assert_eq!(f.await.unwrap(), json!(i));
    }
    let st = s.host.state();
    assert_eq!(st["state"], "ready");
    assert_eq!(st["host"], "ipc");
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latch_ack_is_fast_and_blocks_writes() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-latch", &[]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    let mut lat = Vec::new();
    for i in 0..200u64 {
        let t = Instant::now();
        s.host
            .latch(Latch {
                on: i % 2 == 0,
                scope: "all".into(),
                reason: None,
                latch_seq: U64(i + 1),
            })
            .await
            .unwrap();
        lat.push(t.elapsed());
    }
    lat.sort();
    let p50 = lat[lat.len() / 2];
    let p99 = lat[lat.len() * 99 / 100];
    println!(
        "latch ack latency: p50 {p50:?} p99 {p99:?} max {:?}",
        lat[lat.len() - 1]
    );
    assert!(p50 <= Duration::from_millis(10), "p50 {p50:?}");

    // Latch on while a slow call runs: the ack does not wait behind it.
    let slow = s.host.call(call("sleep", json!({"ms": 300})));
    let t = Instant::now();
    s.host
        .latch(Latch {
            on: true,
            scope: "all".into(),
            reason: Some("test".into()),
            latch_seq: U64(1000),
        })
        .await
        .unwrap();
    assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
    slow.await.unwrap();
    assert_eq!(
        s.host.call(call("latched", json!({}))).await.unwrap(),
        json!(true)
    );
    let out = s.host.write(write("req-l", data("x"))).await;
    assert_eq!(out.state, WriteState::Failed);
    assert_eq!(out.error.unwrap().code, ErrorCode::Stopped);
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_credit_keeps_device_rx_and_reports_exact_display_gap() {
    let dir = sock_dir();
    let mut cfg = config(dir.path(), "t-credit", &[]);
    cfg.initial_credit = Some(0);
    cfg.regrant_credit = false;
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;

    let n = 10_000u64;
    let t = Instant::now();
    let r = s.host.call(call("rx", json!({"n": n}))).await.unwrap();
    println!("fake producer: {n} rx in {:?} with zero credit", t.elapsed());
    assert_eq!(r["ok"], json!(n), "device reads never blocked/failed: {r}");
    let (first, last) = (r["first"].as_u64().unwrap(), r["last"].as_u64().unwrap());

    wait_until(T, || !s.sink.kinds(Lane::Ctl, "display.gap").is_empty()).await;
    assert!(
        s.sink.kinds(Lane::Data, "test.rx").is_empty(),
        "no credit, no data"
    );
    let gaps = s.sink.kinds(Lane::Ctl, "display.gap");
    let mut dropped = 0u64;
    for g in &gaps {
        let d: GapData = serde_json::from_value(g.payload.clone()).unwrap();
        assert_eq!(d.cause, "ipc_backpressure");
        assert_eq!(g.channel.as_deref(), Some("ch0"));
        let pr = d.producer_range.unwrap();
        assert_eq!(pr.producer_id, "t-credit");
        assert!(pr.from.0 >= first && pr.to.0 <= last);
        let GapCount::Exact(c) = d.count else {
            panic!("count unknown")
        };
        dropped += c.0;
    }
    // Screen mirror holds 8192 records; the rest is the reported loss.
    assert_eq!(dropped, n - 8192);

    // Credit arrives: the mirrored records are delivered.
    s.host.grant_credit(64 * 1024 * 1024);
    wait_until(T, || s.sink.kinds(Lane::Data, "test.rx").len() == 8192).await;
    let got = s.sink.kinds(Lane::Data, "test.rx");
    assert_eq!(got[0].source_seq.0, first);
    assert_eq!(got.len() as u64 + dropped, n);
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_worker_restarts_inflight_unknown_others_unaffected() {
    let dir = sock_dir();
    let a = start(config(dir.path(), "t-a", &[]));
    let b = start(config(dir.path(), "t-b", &[]));
    wait_state(&a.host, WorkerState::Ready, T).await;
    wait_state(&b.host, WorkerState::Ready, T).await;
    let pid = a.host.pid().unwrap();

    let inflight = a
        .host
        .write(write("req-k", json!({"data": "x", "delay_ms": 3000})));
    let killer = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        kill9(pid);
    };
    let (out, ()) = tokio::join!(inflight, killer);
    assert_eq!(out.state, WriteState::OutcomeUnknown, "{out:?}");
    assert_eq!(out.error.unwrap().code, ErrorCode::WorkerDown);

    wait_until(T, || {
        a.host.worker_state() == WorkerState::Ready && a.host.pid() != Some(pid)
    })
    .await;
    assert_eq!(a.host.producer_epoch(), 2, "new producer epoch after restart");
    assert!(a.states.seen(WorkerState::Exited) >= 1);
    assert!(a.states.seen(WorkerState::Restarting) >= 1);
    assert_eq!(a.host.call(call("echo", json!(1))).await.unwrap(), json!(1));
    // The other worker never noticed.
    assert_eq!(b.states.seen(WorkerState::Exited), 0);
    assert_eq!(b.host.call(call("echo", json!(2))).await.unwrap(), json!(2));
    a.host.shutdown(true).await;
    b.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_loop_quarantines() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-loop", &["--fault", "abort"]));
    wait_state(&s.host, WorkerState::Quarantined, Duration::from_secs(8)).await;
    assert!(
        s.states.seen(WorkerState::Ready) >= 1,
        "each run reached ready before crashing"
    );
    let e = s.host.call(call("echo", json!(1))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::WorkerDown);
    assert_eq!(e.detail["quarantined"], true);
    let restarts_before = s.states.seen(WorkerState::Starting);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        s.states.seen(WorkerState::Starting),
        restarts_before,
        "no restart while quarantined"
    );
    s.host.shutdown(true).await;

    // A panic counts the same (SDK aborts on panic).
    let dir = sock_dir();
    let p = start(config(dir.path(), "t-panic", &["--fault", "panic"]));
    wait_until(T, || p.states.seen(WorkerState::Exited) >= 1).await;
    p.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hang_goes_suspect_stalled_killed_then_restarts() {
    let dir = sock_dir();
    let s = start(config(dir.path(), "t-hang", &[]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();
    let t = Instant::now();
    let hung = s.host.call(call("hang", json!({})));
    let watch = async {
        wait_state(&s.host, WorkerState::Suspect, T).await;
        // Suspect: new writes refused without being sent.
        let out = s.host.write(write("req-h", data("x"))).await;
        assert_eq!(out.state, WriteState::Failed);
        assert_eq!(out.detail["sent"], false);
    };
    let (r, ()) = tokio::join!(hung, watch);
    assert_eq!(r.unwrap_err().code, ErrorCode::WorkerDown);
    println!("hang detected and killed in {:?}", t.elapsed());
    let ev = s.states.events();
    let order: Vec<WorkerState> = ev.iter().map(|e| e.state).collect();
    let pos = |st| order.iter().position(|s| *s == st).unwrap();
    assert!(pos(WorkerState::Suspect) < pos(WorkerState::Stalled));
    assert!(pos(WorkerState::Stalled) < pos(WorkerState::Killed));
    let killed = ev.iter().find(|e| e.state == WorkerState::Killed).unwrap();
    assert_eq!(killed.detail["reason"], "no_health");
    wait_until(T, || {
        s.host.worker_state() == WorkerState::Ready && s.host.pid() != Some(pid)
    })
    .await;
    assert!(!alive(pid));
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memory_hard_limit_kills() {
    let dir = sock_dir();
    let mut cfg = config(dir.path(), "t-mem", &["--fake-rss", "300000000"]);
    cfg.spec.budgets.mem_soft = 100_000_000;
    cfg.spec.budgets.mem_hard = 200_000_000;
    let s = start(cfg);
    wait_until(T, || {
        s.states
            .events()
            .iter()
            .any(|e| e.state == WorkerState::Killed && e.detail["reason"] == "mem_hard")
    })
    .await;
    s.host.shutdown(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_gone_worker_latches_then_orphan_exits() {
    let dir = sock_dir();
    let latch_file = dir.path().join("latch.log");
    let lf = latch_file.to_str().unwrap().to_string();
    let s = start(config(dir.path(), "t-orph", &["--latch-file", &lf]));
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();
    let t = Instant::now();
    s.host.shutdown(false).await;
    drop(s);
    wait_until(T, || {
        std::fs::read_to_string(&latch_file).is_ok_and(|t| t.contains("on"))
    })
    .await;
    let latched = t.elapsed();
    wait_until(T, || !alive(pid)).await;
    println!(
        "orphan: latched after {latched:?}, exited after {:?} (orphan 300 ms)",
        t.elapsed()
    );
    assert!(
        t.elapsed() >= Duration::from_millis(250),
        "waited the orphan time"
    );
}

/// Graceful stop: after `Latch{reason:"shutdown"}` the closed lanes end the worker at once — no
/// orphan wait (orphan set to 10 s here) — and the host does not have to kill it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_skips_orphan_wait() {
    let dir = sock_dir();
    let mut cfg = config(dir.path(), "t-grace", &[]);
    cfg.orphan = Some(Duration::from_secs(10));
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();
    let l = Latch {
        on: true,
        scope: "all".into(),
        reason: Some("shutdown".into()),
        latch_seq: U64(1),
    };
    s.host.latch(l).await.unwrap();
    let t = Instant::now();
    s.host.shutdown_graceful(Duration::from_secs(2)).await;
    let took = t.elapsed();
    assert!(!alive(pid), "exited");
    println!("graceful: worker gone after {took:?}");
    assert!(
        took < Duration::from_secs(1),
        "no orphan wait, no kill timeout: {took:?}"
    );
}
