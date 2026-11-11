use super::*;
use loom_proto::policy::WriteClass;
use loom_proto::{
    event::{Origin, SourceRecord},
    num::U64,
};
use loom_worker_sdk::{Emitter, LatchFlag, MemorySink, SimClock};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Harness {
    clock: Arc<SimClock>,
    sink: Arc<MemorySink>,
    cx: WorkerCtx,
    latch: LatchFlag,
}
impl Harness {
    fn new(cap: usize) -> Self {
        let clock = Arc::new(SimClock::new(1_700_000_000_000_000_000));
        let sink = Arc::new(MemorySink::new(cap));
        let latch = LatchFlag::new();
        let base = Emitter::new(
            "session",
            "eth",
            "eth-test",
            U64(1),
            1,
            "lan",
            sink.clone(),
            clock.clone(),
        );
        let cx = WorkerCtx::new(
            latch.clone(),
            clock.clone(),
            Arc::new(move |ch| base.for_channel(ch)),
        );
        Self {
            clock,
            sink,
            cx,
            latch,
        }
    }
    fn drain(&self) -> Vec<SourceRecord> {
        self.sink.drain().into_iter().map(|(_, r)| r).collect()
    }
}
#[derive(Debug)]
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn wait<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let start = Instant::now();
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => {
                assert!(start.elapsed() < Duration::from_secs(3), "callback must finish");
                std::thread::park_timeout(Duration::from_millis(5));
            }
        }
    }
}
fn call(method: &str, args: Value) -> Call {
    Call {
        conn_id: None,
        call_id: U64(1),
        method: method.into(),
        args,
        origin: Origin::Agent,
        client_id: Some("cli".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(u64::MAX),
        approval_ref: None,
    }
}
fn op(kind: &str, bytes: &[u8], origin: Origin, id: &str, to: Option<String>) -> Write {
    Write {
        call_id: U64(1),
        request_id: id.into(),
        write_class: WriteClass::Request,
        op: json!({"ch":"lan","kind":kind,"hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>(),"to":to}),
        origin,
        client_id: Some("cli".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(u64::MAX),
    }
}
fn start(worker: &mut EthWorker, h: &Harness, backend: &str) -> Value {
    wait(worker.on_call(
        &h.cx,
        call(
            "eth.capture.start",
            json!({"ch":"lan","iface":backend,"backend":backend}),
        ),
    ))
    .expect("capture start")
}
fn stop(worker: &mut EthWorker, h: &Harness) {
    wait(worker.on_call(&h.cx, call("eth.capture.stop", json!({"ch":"lan"})))).expect("capture stop");
}
fn until(h: &Harness, rows: &mut Vec<SourceRecord>, predicate: impl Fn(&[SourceRecord]) -> bool) {
    let start = Instant::now();
    loop {
        rows.extend(h.drain());
        if predicate(rows) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "event wait timed out: {rows:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn replay(worker: &mut EthWorker, h: &Harness) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../loom-eth-pcapng/tests/fixtures/r1-sol.pcapng");
    wait(worker.on_call(
        &h.cx,
        call(
            "eth.capture.start",
            json!({"ch":"lan","iface":"fixture","backend":"replay","replay":{"path":path,"speed":"max"}}),
        ),
    ))
    .expect("replay");
}
fn sample() -> Vec<Vec<u8>> {
    let mut r = loom_eth_pcapng::PcapNgReader::new(
        &include_bytes!("../../../loom-eth-pcapng/tests/fixtures/r1-sol.pcapng")[..],
    );
    let mut frames = Vec::with_capacity(9);
    while let Some(p) = r.next_packet().expect("fixture") {
        frames.push(p.data);
    }
    frames
}
#[test]
fn replay_nine_packets_continuous_capture_seq_summary_and_bounded_layers() {
    let h = Harness::new(128);
    let mut worker = EthWorker::new();
    replay(&mut worker, &h);
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.payload.get("state") == Some(&json!("replay_done")))
    });
    let packets: Vec<_> = rows.iter().filter(|r| r.kind == KIND_PKT).collect();
    assert_eq!(packets.len(), 9);
    for (i, r) in packets.iter().enumerate() {
        assert_eq!(r.payload["layers"]["_capture"]["source_seq"], (i + 1).to_string());
        assert_eq!(r.payload["ts_src"], "replay");
        assert!(r.payload["proto"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(r.payload["src"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(r.payload["info"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(serde_json::to_vec(&r.payload["layers"]).expect("JSON").len() <= MAX_LAYERS_JSON);
        if i > 0 {
            assert_eq!(r.source_seq.0, packets[i - 1].source_seq.0 + 1);
        }
    }
    assert_eq!(worker.health()["total"], 9);
    assert_eq!(worker.health()["unrecorded"], 0);
    stop(&mut worker, &h);
}
#[test]
fn budget_batch_totals_and_interest_bypass_are_exact() {
    let h = Harness::new(128);
    let mut worker = EthWorker::with_config(WorkerConfig {
        publication_per_sec: 1,
        ..Default::default()
    })
    .expect("config");
    wait(worker.on_call(
        &h.cx,
        call("eth.interest.set", json!({"ch":"lan","filters":["doip"]})),
    ))
    .expect("interest");
    replay(&mut worker, &h);
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.payload.get("state") == Some(&json!("replay_done")))
    });
    let batch = rows.iter().find(|r| r.kind == KIND_BATCH).expect("batch");
    assert_eq!(batch.payload["total"], "9");
    assert_eq!(batch.payload["shown"], "5");
    assert_eq!(
        batch.payload["by_proto"]
            .as_object()
            .expect("protocol map")
            .values()
            .map(|v| v
                .as_str()
                .and_then(|t| t.parse::<u64>().ok())
                .expect("count as U64 string"))
            .sum::<u64>(),
        9
    );
    assert_eq!(rows.iter().filter(|r| r.kind == KIND_PKT).count(), 5);
    assert!(
        rows.iter()
            .any(|r| r.kind == "record.gap" && r.payload["count"] == "4")
    );
    assert_eq!(worker.health()["unrecorded"], 4);
    stop(&mut worker, &h);
}
#[test]
fn simulator_announces_three_times_sd_cadence_and_reset() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    let mut rows = Vec::new();
    let count_ann = |rows: &[SourceRecord]| {
        rows.iter()
            .filter(|r| {
                r.kind == KIND_PKT && r.payload["info"].as_str().is_some_and(|s| s.contains("Vehicle"))
            })
            .count()
    };
    // Decoder wording is not the contract: assert the DoIP payload type instead.
    let announcements = |rows: &[SourceRecord]| {
        rows.iter()
            .filter(|r| r.kind == KIND_PKT && r.payload["layers"]["doip"]["type"] == "0x0004")
            .count()
    };
    until(&h, &mut rows, |r| announcements(r) == 1);
    h.clock.advance_ns(100_000_000);
    until(&h, &mut rows, |r| announcements(r) == 2);
    h.clock.advance_ns(100_000_000);
    until(&h, &mut rows, |r| announcements(r) == 3);
    h.clock.advance_ns(800_000_000);
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_PKT && e.payload["layers"].get("someipsd").is_some())
    });
    assert!(
        rows.iter()
            .filter(|r| r.kind == KIND_PKT)
            .all(|r| r.payload["ts_src"] == "sim")
    );
    let _ = count_ann(&rows);
    wait(worker.on_call(&h.cx, call("eth.sim.reset", json!({"ch":"lan"})))).expect("reset");
    until(&h, &mut rows, |r| announcements(r) == 4);
    h.clock.advance_ns(100_000_000);
    until(&h, &mut rows, |r| announcements(r) == 5);
    h.clock.advance_ns(100_000_000);
    until(&h, &mut rows, |r| announcements(r) == 6);
    stop(&mut worker, &h);
}
#[test]
fn raw_send_echo_attribution_link_write_order_and_routing_response() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    let request = sample()[4].clone();
    let result = wait(worker.on_write(&h.cx, op("raw", &request, Origin::Agent, "tx-1", None)));
    assert_eq!(result.state, WriteState::Sent);
    assert!(result.error.is_none());
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_TX && e.payload["observed_src_seq"].is_string())
    });
    let sent = rows
        .iter()
        .position(|r| {
            r.kind == KIND_TX && r.request_id.as_deref() == Some("tx-1") && r.payload["write_state"] == "sent"
        })
        .expect("sent");
    let captured = rows
        .iter()
        .position(|r| r.kind == KIND_PKT && r.payload["attributed_origin"] == "agent")
        .expect("captured copy");
    assert!(sent < captured);
    let copy = &rows[captured];
    assert_eq!(copy.payload["link"]["domain"], "eth");
    assert_eq!(copy.payload["link"]["request_id"], "tx-1");
    assert_eq!(copy.payload["link"]["confidence"], "exact");
    assert_eq!(
        loom_proto::bytes::decode_base64(copy.payload["bytes"]["data"].as_str().expect("base64"))
            .expect("bytes"),
        request
    );
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_PKT && e.payload["layers"]["doip"]["type"] == "0x0006")
    });
    stop(&mut worker, &h);
}
#[test]
fn stop_latch_blocks_agent_script_but_user_can_send_and_capture_continues() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    h.latch.set(true);
    for origin in [Origin::Agent, Origin::Script] {
        let result = wait(worker.on_write(&h.cx, op("raw", &packet::offer(), origin, "blocked", None)));
        assert_eq!(result.error.expect("STOP error").code, ErrorCode::Stopped);
    }
    let result = wait(worker.on_write(
        &h.cx,
        op("raw", &packet::offer(), Origin::User, "user-send", None),
    ));
    assert_eq!(result.state, WriteState::Sent);
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_PKT && e.payload["attributed_origin"] == "user")
    });
    assert!(
        rows.iter()
            .any(|r| r.kind == KIND_TX && r.payload["write_state"] == "failed")
    );
    stop(&mut worker, &h);
}
#[test]
fn udp_reaches_os_loopback_and_is_not_a_fake_send() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("loopback bind");
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .expect("timeout");
    let to = socket.local_addr().expect("address").to_string();
    let result = wait(worker.on_write(&h.cx, op("udp", b"loom-udp", Origin::Agent, "udp", Some(to))));
    assert_eq!(result.state, WriteState::Sent);
    let mut data = [0; 32];
    let n = socket.recv(&mut data).expect("real UDP payload");
    assert_eq!(&data[..n], b"loom-udp");
    stop(&mut worker, &h);
}
#[test]
fn unknown_stats_gap_and_stop_latency() {
    let h = Harness::new(256);
    let mut worker = EthWorker::with_config(WorkerConfig {
        sim_stats_unavailable: true,
        ..Default::default()
    })
    .expect("config");
    start(&mut worker, &h, "sim");
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == "source.gap" && e.payload["cause"] == "stats_unavailable")
    });
    let gap = rows.iter().find(|e| e.kind == "source.gap").expect("source gap");
    // Contract 02 §2.4: gap domain/channel identify the affected stream.
    assert_eq!(gap.domain, "eth");
    assert_eq!(gap.payload["count"], "unknown");
    let now = Instant::now();
    stop(&mut worker, &h);
    assert!(now.elapsed() < Duration::from_millis(200));
}
#[test]
fn rate_size_filter_and_invalid_tcp_do_not_fake_success() {
    let h = Harness::new(256);
    let mut worker = EthWorker::with_config(WorkerConfig {
        send_per_sec: 1,
        ..Default::default()
    })
    .expect("config");
    start(&mut worker, &h, "sim");
    let bytes = packet::offer();
    assert_eq!(
        wait(worker.on_write(&h.cx, op("raw", &bytes, Origin::Agent, "a", None))).state,
        WriteState::Sent
    );
    assert_eq!(
        wait(worker.on_write(&h.cx, op("raw", &bytes, Origin::Agent, "b", None)))
            .error
            .expect("rate")
            .code,
        ErrorCode::RateLimit
    );
    assert_eq!(
        wait(worker.on_write(
            &h.cx,
            op("tcp", b"x", Origin::Agent, "tcp", Some("invalid".into()))
        ))
        .error
        .expect("invalid TCP destination")
        .code,
        ErrorCode::Usage
    );
    assert!(
        wait(worker.on_write(
            &h.cx,
            op("raw", &vec![0; MAX_SEND_BYTES + 1], Origin::Agent, "large", None)
        ))
        .error
        .is_some()
    );
    assert!(
        wait(worker.on_call(
            &h.cx,
            call("eth.interest.set", json!({"ch":"lan","filters":vec!["ip";17]}))
        ))
        .is_err()
    );
    assert!(
        wait(worker.on_call(
            &h.cx,
            call("eth.interest.set", json!({"ch":"lan","filters":["not-a-field"]}))
        ))
        .is_err()
    );
    stop(&mut worker, &h);
}
#[test]
fn emitter_backpressure_is_counted_and_recovers_with_honest_gaps() {
    let h = Harness::new(2);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    std::thread::sleep(Duration::from_millis(20));
    let _ = h.drain();
    h.clock.advance_ns(100_000_000);
    std::thread::sleep(Duration::from_millis(20));
    let rows = h.drain();
    assert!(worker.health()["emitter_failures"].as_u64().expect("count") > 0);
    assert!(worker.health()["unrecorded"].as_u64().expect("unrecorded") > 0);
    assert!(
        rows.iter()
            .any(|r| r.kind == KIND_BATCH || r.kind == "record.gap" || r.kind == "display.gap")
    );
    stop(&mut worker, &h);
}
#[test]
fn routing_parser_random_100000_inputs_no_panics() {
    let base = sample()[4].clone();
    let mut seed = 0xc0ffeeu64;
    for i in 0..100_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut data = if i % 2 == 0 {
            base.clone()
        } else {
            vec![0; seed as usize % 256]
        };
        for b in &mut data {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            if i % 2 != 0 {
                *b = seed as u8;
            }
        }
        if !data.is_empty() {
            let n = seed as usize % data.len();
            data[n] ^= (seed >> 32) as u8;
        }
        let _ = packet::routing_response(&data);
    }
}

#[test]
fn truncated_own_frame_is_attributed_conservatively() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    wait(worker.on_call(
        &h.cx,
        call(
            "eth.capture.start",
            json!({"ch":"lan","iface":"sim","backend":"sim","snaplen":50}),
        ),
    ))
    .expect("sim");
    assert_eq!(
        wait(worker.on_write(&h.cx, op("raw", &packet::offer(), Origin::Agent, "short", None))).state,
        WriteState::Sent
    );
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_PKT && e.payload["attributed_origin"] == "agent")
    });
    let row = rows
        .iter()
        .find(|e| e.kind == KIND_PKT && e.payload["attributed_origin"] == "agent")
        .expect("own copy");
    assert_eq!(row.payload["link"]["confidence"], "probable");
    stop(&mut worker, &h);
}
#[test]
fn native_udp_match_does_not_claim_exact_wire_identity() {
    let mut table = RecentTable::default();
    let w = op(
        "udp",
        b"owned",
        Origin::Agent,
        "udp-native",
        Some("127.0.0.1:13400".into()),
    );
    let p: SendParams = serde_json::from_value(w.op.clone()).expect("params");
    let src = "127.0.0.1:50000".parse().expect("source");
    let dst = "127.0.0.1:13400".parse().expect("dest");
    table.insert_udp(
        b"owned",
        src,
        dst,
        TxMeta::new(&w, &p, 5, b"owned", Env::Bench),
        0,
    );
    let mut frame = packet::udp(src, dst, b"owned").expect("frame");
    frame[0] = 0x44;
    assert!(table.find(&frame, frame.len() as u32, 0).is_none());
    let d = loom_eth_decode::decode(
        1,
        &frame,
        &Default::default(),
        &mut loom_eth_decode::StreamTable::new(),
    );
    let (r, probable) = table.find_udp(&d, 0).expect("tuple attribution");
    assert!(probable);
    assert_eq!(r.meta.request_id, "udp-native");
    assert!(table.find_udp(&d, CAP_TX_TTL_NS + 1).is_none());
}
#[test]
fn capture_queue_loss_and_interface_probe_are_honest() {
    let h = Harness::new(256);
    let mut worker = EthWorker::with_config(WorkerConfig {
        capture_queue: QueueConfig {
            max_frames: 1,
            max_bytes: 4096,
        },
        ..Default::default()
    })
    .expect("config");
    // The live inventory probe opens devices; its pure mapping is covered in capture tests.
    #[cfg(not(feature = "live"))]
    assert_eq!(
        wait(worker.on_call(&h.cx, call("eth.ifaces", json!({})))).expect("virtual ifaces")["ifaces"]
            .as_array()
            .expect("list")
            .len(),
        2
    );
    replay(&mut worker, &h);
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter().any(|e| e.payload["state"] == "replay_done")
    });
    let packets = rows.iter().filter(|r| r.kind == KIND_PKT).count() as u64;
    let dropped = rows
        .iter()
        .filter(|r| r.kind == "record.gap" && r.payload["cause"] == "capture_queue_overflow")
        .map(|r| {
            r.payload["count"]
                .as_str()
                .expect("gap count")
                .parse::<u64>()
                .expect("count")
        })
        .sum::<u64>();
    assert_eq!(packets + dropped, 9);
    assert_eq!(worker.health()["total"], 9);
    stop(&mut worker, &h);
}
#[test]
fn abandoned_control_future_releases_threads_and_capture() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    let wake = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&wake);
    {
        let future = worker.on_call(
            &h.cx,
            call(
                "eth.capture.start",
                json!({"ch":"lan","iface":"sim","backend":"sim"}),
            ),
        );
        let mut future = std::pin::pin!(future);
        let _ = future.as_mut().poll(&mut context);
    }
    let deadline = Instant::now();
    while worker.shared.control_tasks.load(Ordering::Acquire) > 0 {
        assert!(deadline.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(2));
    }
    start(&mut worker, &h, "sim");
    stop(&mut worker, &h);
}

#[test]
fn own_transmissions_bypass_publication_budget_and_sim_time_can_jump() {
    let h = Harness::new(256);
    let mut worker = EthWorker::with_config(WorkerConfig {
        publication_per_sec: 1,
        ..Default::default()
    })
    .expect("config");
    start(&mut worker, &h, "sim");
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| r.iter().any(|e| e.kind == KIND_PKT));
    assert_eq!(
        wait(worker.on_write(
            &h.cx,
            op("raw", &packet::offer(), Origin::Agent, "own-priority", None)
        ))
        .state,
        WriteState::Sent
    );
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.kind == KIND_PKT && e.payload["attributed_origin"] == "agent")
    });
    stop(&mut worker, &h);
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    h.clock.advance_ns(5_000_000_000);
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter()
            .filter(|e| e.kind == KIND_PKT && e.payload["layers"]["doip"]["type"] == "0x0004")
            .count()
            == 3
    });
    stop(&mut worker, &h);
}

#[test]
fn rejected_size_has_requested_failed_rows_and_restart_keeps_capture_seq() {
    let h = Harness::new(256);
    let mut worker = EthWorker::new();
    start(&mut worker, &h, "sim");
    let result = wait(worker.on_write(
        &h.cx,
        op(
            "raw",
            &vec![0; MAX_SEND_BYTES + 1],
            Origin::Agent,
            "size-denied",
            None,
        ),
    ));
    assert_eq!(result.state, WriteState::Failed);
    let mut rows = h.drain();
    let states: Vec<_> = rows
        .iter()
        .filter(|r| r.request_id.as_deref() == Some("size-denied"))
        .map(|r| r.payload["write_state"].as_str().expect("state"))
        .collect();
    assert_eq!(states, ["requested", "failed"]);
    until(&h, &mut rows, |r| r.iter().any(|e| e.kind == KIND_PKT));
    let first =
        rows.iter().find(|e| e.kind == KIND_PKT).expect("first").payload["layers"]["_capture"]["source_seq"]
            .as_str()
            .expect("seq")
            .parse::<u64>()
            .expect("seq");
    stop(&mut worker, &h);
    h.drain();
    start(&mut worker, &h, "sim");
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| r.iter().any(|e| e.kind == KIND_PKT));
    let next =
        rows.iter().find(|e| e.kind == KIND_PKT).expect("next").payload["layers"]["_capture"]["source_seq"]
            .as_str()
            .expect("seq")
            .parse::<u64>()
            .expect("seq");
    assert!(next > first);
    assert!(
        rows.iter()
            .any(|e| e.kind == "source.gap" && e.payload["cause"] == "capture_restart")
    );
    stop(&mut worker, &h);
}

#[test]
fn bpf_virtual_rejection_and_usage_mapping() {
    let h = Harness::new(256);
    let mut w = EthWorker::new();
    for backend in ["sim", "replay"] {
        let e = wait(w.on_call(
            &h.cx,
            call(
                "eth.capture.start",
                json!({"ch":"lan","iface":backend,"backend":backend,"bpf":"udp"}),
            ),
        ))
        .expect_err("BPF cannot be silently ignored");
        assert_eq!(e.code, ErrorCode::NotImplemented);
        assert!(e.message.contains("-Y"));
    }
    let e = capture_error(loom_eth_capture::CaptureError::Bpf {
        msg: "invalid expression".into(),
    });
    assert_eq!(e.code, ErrorCode::Usage);
    assert_eq!(e.code.exit_code(), 2);
    start(&mut w, &h, "sim");
    stop(&mut w, &h);
}
fn tcp_write(
    worker: &mut EthWorker,
    h: &Harness,
    to: &str,
    data: &[u8],
    id: &str,
    keep: bool,
) -> WriteOutcome {
    let mut request = op("tcp", data, Origin::Agent, id, Some(to.into()));
    request.op["keep_open"] = json!(keep);
    wait(worker.on_write(&h.cx, request))
}
fn carries(r: &SourceRecord, data: &[u8]) -> bool {
    r.kind == KIND_PKT
        && r.payload
            .pointer("/bytes/data")
            .and_then(Value::as_str)
            .and_then(|s| loom_proto::bytes::decode_base64(s).ok())
            .is_some_and(|b| b.ends_with(data))
}
#[test]
fn tcp_sim_owned_stream_reuse_cap_idle_and_ipv6() {
    let h = Harness::new(1024);
    let mut w = EthWorker::new();
    start(&mut w, &h, "sim");
    // No server exists at this peer: virtual TCP must succeed without touching OS sockets.
    assert_eq!(
        tcp_write(&mut w, &h, "127.0.0.1:1", b"first-owned", "t1", true).state,
        WriteState::Sent
    );
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| r.iter().any(|e| carries(e, b"first-owned")));
    let first = rows.iter().find(|e| carries(e, b"first-owned")).expect("payload");
    let stream = first.payload["stream"].clone();
    assert!(stream.is_number());
    assert_eq!(first.payload["link"]["request_id"], "t1");
    assert_eq!(first.payload["attributed_origin"], "agent");
    assert_eq!(first.payload["ts_src"], "sim");
    assert_eq!(
        tcp_write(&mut w, &h, "127.0.0.1:1", b"second-owned", "t2", true).state,
        WriteState::Sent
    );
    until(&h, &mut rows, |r| r.iter().any(|e| carries(e, b"second-owned")));
    assert_eq!(
        rows.iter()
            .find(|e| carries(e, b"second-owned"))
            .expect("second")
            .payload["stream"],
        stream
    );
    for port in 2..=4 {
        assert_eq!(
            tcp_write(&mut w, &h, &format!("127.0.0.1:{port}"), b"keep", "keep", true).state,
            WriteState::Sent
        );
    }
    assert_eq!(
        tcp_write(&mut w, &h, "127.0.0.1:5", b"cap", "cap", true)
            .error
            .expect("cap")
            .code,
        ErrorCode::WorkerBusy
    );
    h.clock.advance_ns(tcp::CAP_TCP_IDLE_NS + 1);
    until(&h, &mut rows, |r| {
        r.iter()
            .any(|e| e.payload.pointer("/layers/tcp/flags.fin") == Some(&Value::Bool(true)))
    });
    assert_eq!(
        tcp_write(&mut w, &h, "[::1]:13400", b"ipv6-owned", "v6", false).state,
        WriteState::Sent
    );
    until(&h, &mut rows, |r| r.iter().any(|e| carries(e, b"ipv6-owned")));
    let v6 = rows.iter().find(|e| carries(e, b"ipv6-owned")).expect("IPv6");
    assert!(v6.payload["layers"]["ipv6"].is_object());
    assert_eq!(v6.payload["link"]["request_id"], "v6");
    let now = Instant::now();
    stop(&mut w, &h);
    assert!(now.elapsed() < Duration::from_millis(200));
}
#[derive(Debug)]
struct CaptureFixture(std::path::PathBuf);
impl CaptureFixture {
    fn new(packets: &[loom_eth_pcapng::Packet]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "loom-p1b-{}-{}.pcapng",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::File::create(&path).expect("fixture file");
        let mut w = loom_eth_pcapng::PcapNgWriter::new(file).expect("SHB");
        w.write_interface(&loom_eth_capture::Interface {
            linktype: 1,
            snaplen: 65535,
            name: "fixture".into(),
            tsresol: loom_eth_capture::TimestampResolution::NANO,
        })
        .expect("IDB");
        for p in packets {
            w.write_packet(p).expect("EPB");
        }
        drop(w);
        Self(path)
    }
}
impl Drop for CaptureFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[test]
fn replay_restores_all_x10_origins_dirs_links_and_full_user_comments() {
    use loom_eth_pcapng::{Direction, LoomMeta, Origin as POrigin, Packet};
    let data = sample()[0].clone();
    let comments = vec![
        "한글 사용자 주석".into(),
        "loom1 seq=broken".into(),
        "x".repeat(4096),
        "y".repeat(4096),
    ];
    let mut packets = Vec::new();
    for org in [POrigin::Agent, POrigin::User, POrigin::Script] {
        for dir in [Direction::Rx, Direction::Tx] {
            let n = packets.len();
            packets.push(Packet {
                ts_ns: n as u128 * 1_000_000,
                caplen: data.len() as u32,
                origlen: data.len() as u32,
                data: data.clone(),
                comments: comments.clone(),
                meta: Some(LoomMeta {
                    seq: 900 + n as u64,
                    dir,
                    req: Some(format!("old-{n}")),
                    org,
                }),
                ..Default::default()
            });
        }
    }
    let f = CaptureFixture::new(&packets);
    let h = Harness::new(256);
    let mut w = EthWorker::new();
    wait(w.on_call(
        &h.cx,
        call(
            "eth.capture.start",
            json!({"ch":"lan","iface":"replay","backend":"replay","replay":{"path":f.0,"speed":"max"}}),
        ),
    ))
    .expect("replay");
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| {
        r.iter().any(|e| e.payload["state"] == "replay_done")
    });
    let got = rows.iter().filter(|r| r.kind == KIND_PKT).collect::<Vec<_>>();
    assert_eq!(got.len(), packets.len());
    for (n, (row, p)) in got.iter().zip(&packets).enumerate() {
        let m = p.meta.as_ref().expect("meta");
        let org = match m.org {
            POrigin::Agent => "agent",
            POrigin::User => "user",
            POrigin::Script => "script",
        };
        assert_eq!(row.payload["attributed_origin"], org);
        assert_eq!(
            row.payload["dir"],
            if m.dir == Direction::Tx { "tx" } else { "rx" }
        );
        assert_eq!(row.payload["link"]["request_id"], format!("old-{n}"));
        assert_eq!(row.payload["ts_src"], "replay");
        assert_eq!(row.payload["bytes"]["comments"], json!(comments));
        assert_eq!(row.payload["bytes"]["loom_comment"], m.to_comment().expect("X10"));
        assert_eq!(
            row.payload["layers"]["_capture"]["replay_src_seq"],
            m.seq.to_string()
        );
        assert_ne!(row.payload["layers"]["_capture"]["source_seq"], m.seq.to_string());
        assert!(serde_json::to_vec(&row.payload["layers"]).expect("layers").len() <= MAX_LAYERS_JSON);
    }
    assert!(!rows.iter().any(|r| r.kind == KIND_TX));
    stop(&mut w, &h);
}
#[test]
fn tcp_replay_uses_simtap_inside_same_capture_stream() {
    let data = sample()[0].clone();
    let f = CaptureFixture::new(&[
        loom_eth_pcapng::Packet {
            data: data.clone(),
            caplen: data.len() as u32,
            origlen: data.len() as u32,
            ..Default::default()
        },
        loom_eth_pcapng::Packet {
            data: data.clone(),
            caplen: data.len() as u32,
            origlen: data.len() as u32,
            ts_ns: 60_000_000_000,
            ..Default::default()
        },
    ]);
    let h = Harness::new(256);
    let mut w = EthWorker::new();
    wait(w.on_call(
        &h.cx,
        call(
            "eth.capture.start",
            json!({"ch":"lan","iface":"replay","backend":"replay","replay":{"path":f.0,"speed":1.0}}),
        ),
    ))
    .expect("replay");
    assert_eq!(
        tcp_write(&mut w, &h, "127.0.0.1:1", b"replay-tcp", "replay-own", false).state,
        WriteState::Sent
    );
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| r.iter().any(|e| carries(e, b"replay-tcp")));
    let own = rows.iter().find(|e| carries(e, b"replay-tcp")).expect("copy");
    assert_eq!(own.payload["ts_src"], "sim");
    assert_eq!(own.payload["attributed_origin"], "agent");
    assert_eq!(own.payload["link"]["request_id"], "replay-own");
    stop(&mut w, &h);
}
fn native_submit(
    service: &tcp::NativeTcp,
    h: &Harness,
    shared: &Arc<Shared>,
    to: std::net::SocketAddr,
    data: &[u8],
    keep: bool,
) -> WriteOutcome {
    let request = op("tcp", data, Origin::Agent, "native-owned", Some(to.to_string()));
    let p: SendParams = serde_json::from_value(request.op.clone()).expect("params");
    let meta = TxMeta::new(&request, &p, data.len(), data, Env::Bench);
    let (reply, complete) = reply();
    service.submit(Box::new(source::Ticket {
        bytes: data.to_vec(),
        op: source::Operation::Tcp { to, keep_open: keep },
        meta,
        cx: h.cx.clone(),
        emitter: h.cx.emitter("lan"),
        shared: shared.clone(),
        complete: Some(complete),
        _slot: source::Slot::reserve(Arc::new(AtomicUsize::new(0)), data.len() + 2048).expect("slot"),
        deadline_ns: u64::MAX,
    }));
    wait(reply)
}
#[test]
fn native_tcp_loopback_reuses_closes_expires_and_reports_connection_failure() {
    use std::{io::Read, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let to = listener.local_addr().expect("address");
    let h = Harness::new(512);
    let shared = Arc::new(Shared::default());
    let signal = loom_eth_capture::StopSignal::default();
    let service = tcp::NativeTcp::new(signal.clone(), h.cx.clone()).expect("service");
    assert_eq!(
        native_submit(&service, &h, &shared, to, b"first", true).state,
        WriteState::Sent
    );
    let (mut peer, _) = listener.accept().expect("owned connection");
    peer.set_read_timeout(Some(Duration::from_secs(1)))
        .expect("timeout");
    let mut first = [0; 5];
    peer.read_exact(&mut first).expect("first bytes");
    assert_eq!(&first, b"first");
    assert_eq!(
        native_submit(&service, &h, &shared, to, b"second", true).state,
        WriteState::Sent
    );
    let mut second = [0; 6];
    peer.read_exact(&mut second).expect("second bytes");
    assert_eq!(&second, b"second");
    listener.set_nonblocking(true).expect("nonblocking");
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    listener.set_nonblocking(false).expect("blocking");
    h.clock.advance_ns(tcp::CAP_TCP_IDLE_NS + 1);
    assert_eq!(peer.read(&mut [0; 1]).expect("idle EOF"), 0);
    assert_eq!(
        native_submit(&service, &h, &shared, to, b"new", false).state,
        WriteState::Sent
    );
    let (mut peer, _) = listener.accept().expect("new owned connection");
    peer.set_read_timeout(Some(Duration::from_secs(1)))
        .expect("timeout");
    let mut bytes = Vec::new();
    peer.read_to_end(&mut bytes).expect("closed");
    assert_eq!(bytes, b"new");
    drop(listener);
    let failed = native_submit(&service, &h, &shared, to, b"fail", false);
    assert_eq!(failed.state, WriteState::Failed);
    assert_eq!(failed.error.expect("connection failure").code, ErrorCode::NotOpen);
    let now = Instant::now();
    signal.stop();
    drop(service);
    assert!(now.elapsed() < Duration::from_millis(200));
    assert!(
        h.drain()
            .iter()
            .any(|r| r.kind == KIND_TX && r.payload["write_state"] == "failed")
    );
}
#[test]
fn native_tcp_pool_cap_stop_and_no_retry() {
    use std::net::TcpListener;
    let h = Harness::new(512);
    let shared = Arc::new(Shared::default());
    let service =
        tcp::NativeTcp::new(loom_eth_capture::StopSignal::default(), h.cx.clone()).expect("service");
    let listeners = (0..5)
        .map(|_| TcpListener::bind("127.0.0.1:0").expect("listener"))
        .collect::<Vec<_>>();
    let mut peers = Vec::new();
    for listener in listeners.iter().take(4) {
        let to = listener.local_addr().expect("to");
        assert_eq!(
            native_submit(&service, &h, &shared, to, b"keep", true).state,
            WriteState::Sent
        );
        peers.push(listener.accept().expect("peer").0);
    }
    let fifth = listeners[4].local_addr().expect("to");
    assert_eq!(
        native_submit(&service, &h, &shared, fifth, b"cap", true)
            .error
            .expect("cap")
            .code,
        ErrorCode::WorkerBusy
    );
    h.latch.set(true);
    assert_eq!(
        native_submit(&service, &h, &shared, fifth, b"blocked", false)
            .error
            .expect("STOP")
            .code,
        ErrorCode::Stopped
    );
    drop(service);
    drop(peers);
}

#[test]
fn tcp_sim_jumbo_ipv6_payload_keeps_attribution() {
    let h = Harness::new(256);
    let mut w = EthWorker::new();
    start(&mut w, &h, "sim");
    let bytes = vec![0xab; MAX_SEND_BYTES];
    assert_eq!(
        tcp_write(&mut w, &h, "[::1]:13400", &bytes, "jumbo6", false).state,
        WriteState::Sent
    );
    let mut rows = Vec::new();
    until(&h, &mut rows, |r| r.iter().any(|e| carries(e, &bytes)));
    let packet = rows
        .iter()
        .find(|e| carries(e, &bytes))
        .expect("jumbo IPv6 payload");
    assert_eq!(packet.payload["attributed_origin"], "agent");
    assert_eq!(packet.payload["link"]["request_id"], "jumbo6");
    assert_eq!(packet.payload["link"]["confidence"], "exact");
    stop(&mut w, &h);
}
#[test]
fn native_tcp_pending_syn_owned_tuple_and_expiry_are_probable() {
    use loom_eth_capture::{CaptureSource, Direction, SimTapSource, SocketConnection, TcpFlags};
    let request = op(
        "tcp",
        b"bytes",
        Origin::Agent,
        "tuple",
        Some("127.0.0.1:13400".into()),
    );
    let p: SendParams = serde_json::from_value(request.op.clone()).expect("params");
    let meta = TxMeta::new(&request, &p, 5, b"bytes", Env::Bench);
    let local = "127.0.0.1:50000".parse().expect("local");
    let remote = "127.0.0.1:13400".parse().expect("remote");
    let conn = SocketConnection {
        client: local,
        server: remote,
        client_mac: [2, 0, 0, 0, 0, 1],
        server_mac: [2, 0, 0, 0, 0, 2],
    };
    let mut tap = SimTapSource::new();
    tap.handle()
        .push_tcp_socket(
            conn,
            Direction::Tx,
            &[],
            TcpFlags {
                syn: true,
                fin: false,
            },
        )
        .expect("SYN");
    tap.handle()
        .push_tcp_socket(
            conn,
            Direction::Rx,
            &[],
            TcpFlags {
                syn: true,
                fin: false,
            },
        )
        .expect("SYN ACK");
    tap.handle()
        .push_tcp_socket(conn, Direction::Tx, b"bytes", TcpFlags::default())
        .expect("payload");
    let mut decode = || {
        let f = tap.next(Duration::ZERO).expect("tap").expect("frame");
        loom_eth_decode::decode(
            1,
            &f.data,
            &Default::default(),
            &mut loom_eth_decode::StreamTable::new(),
        )
    };
    let mut recent = RecentTable::default();
    let observation = recent.tcp_attempt(remote, meta, 0);
    let syn = decode();
    assert!(
        recent
            .find_tcp(&syn, 0)
            .expect("pending SYN conservative ownership")
            .1
    );
    let incoming = decode();
    assert!(recent.find_tcp(&incoming, 0).is_none());
    let payload = decode();
    assert!(recent.find_tcp(&payload, 0).is_none());
    recent.tcp_bound(&observation, local, remote);
    let (owned, probable) = recent.find_tcp(&payload, 0).expect("owned tuple");
    assert!(probable);
    assert!(!lock(owned.observation.as_ref().expect("state")).sent);
    assert!(recent.find_tcp(&payload, CAP_TX_TTL_NS + 1).is_none());
}
