//! can_sim worker through `FakeWorker` + `ManualClock` (no daemon, no IPC).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use loom_can::schema::MAX_SEND_COUNT;
use loom_can::worker::{
    CALL_CLOSE, CALL_NODE_LIST, CALL_NODE_START, CALL_NODE_STOP, CALL_OPEN, CanSimWorker, GAP_CAUSE_OBSERVER,
    MAX_CHANNELS, PumpMode, WorkerLimits,
};
use loom_can::{KIND_CHANNEL_STATE, KIND_FRAME, KIND_TX, RPC_STATUS, WORKER_KIND_SIM};
use loom_can_sim::ManualClock;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, PublishedRecord, WriteState, kinds};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Lane, Write};
use loom_testkit::{FakeWorker, assert_no_gap_lies, assert_write_rows};
use loom_worker_sdk::WriteOutcome;
use serde_json::{Value, json};

const MS: u64 = 1_000_000;

struct H {
    fw: FakeWorker<CanSimWorker>,
    clock: ManualClock,
}

fn harness() -> H {
    harness_with(WorkerLimits::default())
}

fn harness_with(limits: WorkerLimits) -> H {
    let clock = ManualClock::new(0);
    let w = CanSimWorker::with_options(clock.clone(), PumpMode::Manual, limits);
    H {
        fw: FakeWorker::new(w),
        clock,
    }
}

impl H {
    /// Moves the sim bus clock and the SDK clock together, then pumps.
    fn step_ms(&mut self, ms: u64) {
        self.clock.advance_ms(ms);
        self.fw.clock().advance_ns(ms * MS);
        self.fw.worker().pump_once();
    }

    fn call(&mut self, method: &str, args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        self.fw.call(method, args)
    }

    fn open(&mut self, ch: &str) -> Value {
        self.call(CALL_OPEN, json!({ "channel": ch })).unwrap()
    }

    fn send(&mut self, origin: Origin, rid: &str, op: Value) -> WriteOutcome {
        let cx = self.fw.ctx().clone();
        let w = Write {
            call_id: U64(1),
            request_id: rid.into(),
            write_class: WriteClass::Request,
            op,
            origin,
            client_id: Some(format!("{origin:?}#1").to_lowercase()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(u64::MAX),
        };
        self.fw.worker().write(&cx, w)
    }

    fn events(&mut self) -> Vec<PublishedRecord> {
        self.fw.drain_published()
    }

    fn status(&mut self) -> Value {
        self.call(RPC_STATUS, json!({})).unwrap()
    }
}

fn frames(evs: &[PublishedRecord]) -> Vec<&PublishedRecord> {
    evs.iter().filter(|e| e.kind == KIND_FRAME).collect()
}

fn id_of(e: &PublishedRecord) -> u64 {
    e.payload["id"].as_u64().unwrap()
}

fn data_of(e: &PublishedRecord) -> &str {
    e.payload["bytes"]["data"].as_str().unwrap()
}

fn tx_rows<'a>(evs: &'a [PublishedRecord], rid: &str) -> Vec<&'a PublishedRecord> {
    evs.iter()
        .filter(|e| e.kind == KIND_TX && e.request_id.as_deref() == Some(rid))
        .collect()
}

fn states(rows: &[&PublishedRecord]) -> Vec<(String, u64)> {
    rows.iter()
        .map(|r| {
            (
                r.payload["write_state"].as_str().unwrap().to_owned(),
                r.payload["sent"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn check_rows(evs: &[PublishedRecord], rid: &str) {
    let rows: Vec<PublishedRecord> = tx_rows(evs, rid).into_iter().cloned().collect();
    assert_write_rows(&rows);
}

fn op(frame: &str) -> Value {
    json!({"channel": "ch0", "frame": frame})
}

#[test]
fn kind_and_default_nodes() {
    assert_eq!(<CanSimWorker as loom_worker_sdk::Worker>::KIND, WORKER_KIND_SIM);
    let nodes = loom_can_sim::parse_nodes(loom_can::worker::DEFAULT_NODES_TOML).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "BCM");
}

#[test]
fn open_emits_channel_state() {
    let mut h = harness();
    let r = h.open("ch0");
    assert_eq!(r["generation"], json!(1));
    assert_eq!(r["bus"], json!("ch0"));
    assert_eq!(r["nodes"], json!(["BCM"]));
    let evs = h.fw.drain();
    assert_eq!(evs.len(), 1);
    let (lane, rec) = &evs[0];
    assert_eq!(*lane, Lane::Ctl);
    assert_eq!(rec.kind, KIND_CHANNEL_STATE);
    assert_eq!(rec.channel.as_deref(), Some("ch0"));
    assert_eq!(rec.generation, Some(1));
    assert_eq!(rec.env, Some(Env::Sim));
    assert_eq!(rec.payload["state"], json!("open"));
    assert_eq!(rec.payload["backend"], json!("sim"));
    assert_eq!(rec.payload["channel"], json!("ch0"));
    assert_eq!(rec.payload["generation"], json!(1));
    // Same channel twice → busy.
    let e = h.call(CALL_OPEN, json!({"channel": "ch0"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    let e = h.call(CALL_OPEN, json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
}

/// Plan §2.0: agent 300#01 → tx echo r1 → 301#01 after 5 ms; 0x310 follows; user 300#00.
#[test]
fn walking_skeleton_loop() {
    let mut h = harness();
    h.open("ch0");
    h.events();

    let out = h.send(Origin::Agent, "r1", op("300#01"));
    assert_eq!(out.state, WriteState::Sent, "{out:?}");
    assert_eq!(out.detail["frames"], json!(1));
    assert_eq!(out.detail["sent"], json!(1));
    assert!(out.detail["source_seq"].as_u64().is_some());
    let lanes: Vec<Lane> = h.fw.drain().iter().map(|(l, _)| *l).collect();
    assert_eq!(lanes, [Lane::Ctl, Lane::Ctl, Lane::Ctl]);

    // Same again, now as published records (lanes checked above).
    let out = h.send(Origin::Agent, "r1b", op("300#01"));
    assert_eq!(out.state, WriteState::Sent);
    let evs = h.events();
    let kinds: Vec<&str> = evs.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, [KIND_TX, KIND_TX, KIND_FRAME]);
    assert_eq!(
        states(&tx_rows(&evs, "r1b")),
        [("accepted".into(), 0), ("sent".into(), 1)]
    );
    check_rows(&evs, "r1b");
    let row = &evs[1];
    assert_eq!(row.origin, Origin::Agent);
    assert_eq!(row.dir, Some(Dir::Tx));
    assert_eq!(row.payload["count"], json!(1));
    assert_eq!(row.payload["frame"]["id"], json!(0x300));
    let echo = &evs[2];
    assert_eq!(echo.request_id.as_deref(), Some("r1b"));
    assert_eq!(echo.origin, Origin::Agent);
    assert_eq!(echo.dir, Some(Dir::Tx));
    assert_eq!(echo.client_id.as_deref(), Some("agent#1"));
    assert_eq!(echo.generation, Some(1));
    assert_eq!(echo.env, Some(Env::Sim));
    assert_eq!(
        echo.payload,
        json!({"id": 0x300, "ext": false, "fd": false, "brs": false, "esi": false, "rtr": false,
               "dlc": 1, "len": 1, "bytes": {"encoding": "hex", "data": "01"}})
    );
    let echo_ts = echo.ts.mono_ns.get();

    // 5 ms later: both replies (one per request), device origin, data lane, no request_id.
    h.step_ms(4);
    assert!(frames(&h.events()).is_empty(), "reply not before 5 ms");
    h.step_ms(1);
    let raw = h.fw.drain();
    assert_eq!(raw.len(), 2);
    assert!(raw.iter().all(|(l, _)| *l == Lane::Data));
    let evs: Vec<PublishedRecord> = raw.into_iter().map(|(_, r)| r.publish(U64(1), U64(1))).collect();
    for e in &evs {
        assert_eq!(e.kind, KIND_FRAME);
        assert_eq!((id_of(e), data_of(e)), (0x301, "01"));
        assert_eq!(e.origin, Origin::Device);
        assert_eq!(e.dir, Some(Dir::Rx));
        assert!(e.request_id.is_none());
        assert_eq!(e.ts.mono_ns.get() - echo_ts, 5 * MS);
    }

    // 0x310 at +50 ms, byte0 follows the last 0x300 byte0.
    h.step_ms(45);
    let evs = h.events();
    let f = frames(&evs);
    assert_eq!(f.len(), 1);
    assert_eq!((id_of(f[0]), data_of(f[0])), (0x310, "0100000000000000"));
    assert_eq!(f[0].ts.mono_ns.get(), 50 * MS);

    // User intervention on the same bus.
    let out = h.send(Origin::User, "r2", op("300#00"));
    assert_eq!(out.state, WriteState::Sent);
    h.step_ms(5);
    let evs = h.events();
    check_rows(&evs, "r2");
    let f = frames(&evs);
    assert_eq!(f.len(), 2);
    assert_eq!(
        (f[0].origin, id_of(f[0]), data_of(f[0])),
        (Origin::User, 0x300, "00")
    );
    assert_eq!(f[0].request_id.as_deref(), Some("r2"));
    assert_eq!(f[0].client_id.as_deref(), Some("user#1"));
    assert_eq!(
        (f[1].origin, id_of(f[1]), data_of(f[1])),
        (Origin::Device, 0x301, "00")
    );
    assert_eq!(f[1].ts.mono_ns.get() - f[0].ts.mono_ns.get(), 5 * MS);
    assert_no_gap_lies(&evs);
}

#[test]
fn structured_fd_frame_payload() {
    let mut h = harness();
    h.call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    h.events();
    let out = h.send(
        Origin::Agent,
        "fd1",
        json!({"channel": "ch0", "id": 0x123, "fd": true, "data": "00 01 02 03 04 05 06 07 08 09 0a 0b"}),
    );
    assert_eq!(out.state, WriteState::Sent);
    let evs = h.events();
    let f = frames(&evs);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].payload["dlc"], json!(9));
    assert_eq!(f[0].payload["len"], json!(12));
    assert_eq!(f[0].payload["fd"], json!(true));
    assert_eq!(f[0].payload["brs"], json!(true));
    assert_eq!(data_of(f[0]), "000102030405060708090a0b");
}

#[test]
fn burst_with_gap_is_one_request() {
    let mut h = harness();
    h.call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    h.events();
    let out = h.send(
        Origin::Agent,
        "b1",
        json!({"channel": "ch0", "frame": "300#AA", "count": 3, "gap_ms": 10}),
    );
    // Still sending: the reply says `accepted`; the pump emits `sent` after the last frame.
    assert_eq!(out.state, WriteState::Accepted);
    assert_eq!(out.detail["sent"], json!(1));
    assert_eq!(out.detail["pending"], json!(2));
    let mut all = h.events();
    assert_eq!(h.status()["pending_bursts"], json!(1));
    for _ in 0..4 {
        h.step_ms(5);
        all.extend(h.events());
    }
    let f = frames(&all);
    assert_eq!(f.len(), 3);
    for (i, e) in f.iter().enumerate() {
        assert_eq!(e.request_id.as_deref(), Some("b1"));
        assert_eq!(e.payload["frame_index"], json!(i));
        assert_eq!(e.ts.mono_ns.get(), i as u64 * 10 * MS);
    }
    assert_eq!(
        states(&tx_rows(&all, "b1")),
        [("accepted".into(), 0), ("sent".into(), 3)]
    );
    // The completion row comes after the last echo.
    let last_row = all.iter().rposition(|e| e.kind == KIND_TX).unwrap();
    let last_frame = all.iter().rposition(|e| e.kind == KIND_FRAME).unwrap();
    assert!(last_row > last_frame);
    check_rows(&all, "b1");
    assert_eq!(h.status()["pending_bursts"], json!(0));
}

#[test]
fn burst_without_gap_is_sent_at_once() {
    let mut h = harness();
    h.open("ch0");
    h.events();
    let out = h.send(
        Origin::Script,
        "b0",
        json!({"channel": "ch0", "frame": "300#01", "count": 3}),
    );
    assert_eq!(out.state, WriteState::Sent);
    assert_eq!(out.detail["pending"], json!(0));
    let evs = h.events();
    let f = frames(&evs);
    let idx: Vec<&Value> = f.iter().map(|e| &e.payload["frame_index"]).collect();
    assert_eq!(idx, [&json!(0), &json!(1), &json!(2)]);
    assert!(f.iter().all(|e| e.origin == Origin::Script));
    assert_eq!(
        states(&tx_rows(&evs, "b0")),
        [("accepted".into(), 0), ("sent".into(), 3)]
    );
}

#[test]
fn stop_latch_refuses_agent_and_script_only() {
    let mut h = harness();
    h.open("ch0");
    h.events();
    // An agent burst in progress is cancelled by STOP; a user burst continues.
    h.send(
        Origin::Agent,
        "ab",
        json!({"channel": "ch0", "frame": "300#05", "count": 5, "gap_ms": 10}),
    );
    h.send(
        Origin::User,
        "ub",
        json!({"channel": "ch0", "frame": "300#06", "count": 3, "gap_ms": 10}),
    );
    h.events();
    h.fw.latch(true);
    let evs = h.events();
    assert_eq!(
        states(&tx_rows(&evs, "ab")),
        [("failed".into(), 1)],
        "agent burst cancelled with what was sent"
    );
    assert_eq!(tx_rows(&evs, "ab")[0].payload["error"], json!("E_STOPPED"));

    let out = h.send(Origin::Agent, "r1", op("300#01"));
    assert_eq!(out.state, WriteState::Failed);
    assert_eq!(out.error.unwrap().code, ErrorCode::Stopped);
    let out = h.send(Origin::Script, "s1", op("300#01"));
    assert_eq!(out.state, WriteState::Failed);
    let evs = h.events();
    assert!(frames(&evs).is_empty(), "nothing on the bus while latched");
    assert_eq!(states(&tx_rows(&evs, "r1")), [("failed".into(), 0)]);
    assert_eq!(states(&tx_rows(&evs, "s1")), [("failed".into(), 0)]);

    let st = h.status();
    assert_eq!(st["latch"], json!(true));
    assert_eq!(st["channels"][0]["latched"], json!(["agent", "script"]));

    let out = h.send(Origin::User, "u1", op("300#02"));
    assert_eq!(out.state, WriteState::Sent, "user intervention passes STOP (A3)");
    let mut all = h.events();
    for _ in 0..15 {
        h.step_ms(10);
        all.extend(h.events());
    }
    let f = frames(&all);
    assert!(
        f.iter().all(|e| e.origin != Origin::Agent),
        "no agent frame after STOP"
    );
    let user_burst: Vec<_> = f
        .iter()
        .filter(|e| e.request_id.as_deref() == Some("ub"))
        .collect();
    assert_eq!(user_burst.len(), 2, "user burst frames 1 and 2 still sent");
    assert!(
        f.iter().any(|e| id_of(e) == 0x301 && data_of(e) == "02"),
        "fake ECU still answers"
    );
    assert!(
        f.iter().filter(|e| id_of(e) == 0x310).count() >= 1,
        "periodic 0x310 continues"
    );

    h.fw.latch(false);
    let out = h.send(Origin::Agent, "r2", op("300#03"));
    assert_eq!(out.state, WriteState::Sent, "released → agent passes");
    assert_eq!(h.status()["channels"][0]["latched"], json!([]));
}

#[test]
fn usage_errors_send_nothing() {
    let mut h = harness();
    h.open("ch0");
    h.events();
    for (rid, bad) in [
        ("u1", json!({"channel": "ch0", "id": 0x300, "data": "0"})),
        ("u2", json!({"channel": "ch0", "frame": "300#01", "count": 0})),
        (
            "u3",
            json!({"channel": "ch0", "frame": "300#01", "count": MAX_SEND_COUNT + 1}),
        ),
        ("u4", json!({"channel": "ch0", "frame": "nonsense"})),
        ("u5", json!("300#01")),
        (
            "u6",
            json!({"channel": "ch0", "frame": "300#01", "dry_run": true}),
        ),
    ] {
        let out = h.send(Origin::Agent, rid, bad.clone());
        assert_eq!(out.state, WriteState::Failed, "{bad}");
        assert_eq!(out.error.unwrap().code, ErrorCode::Usage, "{bad}");
    }
    assert!(h.events().is_empty());
    assert_eq!(h.status()["channels"][0]["frames_total"], json!(0));

    let out = h.send(Origin::Agent, "n1", json!({"channel": "nope", "frame": "300#01"}));
    assert_eq!(out.error.unwrap().code, ErrorCode::NotOpen);
    let evs = h.events();
    assert!(frames(&evs).is_empty());
    assert_eq!(states(&tx_rows(&evs, "n1")), [("failed".into(), 0)]);

    let out = h.send(Origin::Device, "d1", op("300#01"));
    assert_eq!(out.error.unwrap().code, ErrorCode::Usage);
}

#[test]
fn burst_queue_is_bounded() {
    assert_eq!(
        WorkerLimits::default().max_pending_bursts,
        MAX_SEND_COUNT as usize
    );
    let mut h = harness_with(WorkerLimits {
        max_pending_bursts: 2,
        ..WorkerLimits::default()
    });
    h.call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    let burst = json!({"channel": "ch0", "frame": "300#01", "count": 2, "gap_ms": 1000});
    assert_eq!(
        h.send(Origin::Agent, "q1", burst.clone()).state,
        WriteState::Accepted
    );
    assert_eq!(
        h.send(Origin::Agent, "q2", burst.clone()).state,
        WriteState::Accepted
    );
    h.events();
    let out = h.send(Origin::Agent, "q3", burst.clone());
    assert_eq!(out.error.unwrap().code, ErrorCode::Busy);
    let evs = h.events();
    assert!(frames(&evs).is_empty(), "refused before accepted, nothing sent");
    assert_eq!(states(&tx_rows(&evs, "q3")), [("failed".into(), 0)]);
    // A single frame does not need the queue.
    assert_eq!(h.send(Origin::Agent, "q4", op("300#01")).state, WriteState::Sent);
    let st = h.status();
    assert_eq!(st["bursts_rejected"], json!(1));
    assert_eq!(st["pending_bursts"], json!(2));
    // Room again once a burst finishes.
    h.step_ms(1000);
    assert_eq!(h.send(Origin::Agent, "q5", burst).state, WriteState::Accepted);
}

#[test]
fn request_tags_are_bounded() {
    let mut h = harness_with(WorkerLimits {
        max_tags_per_channel: 4,
        ..WorkerLimits::default()
    });
    h.call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    h.events();
    let out = h.send(
        Origin::Agent,
        "t1",
        json!({"channel": "ch0", "frame": "300#01", "count": 10}),
    );
    assert_eq!(out.state, WriteState::Sent);
    let evs = h.events();
    let f = frames(&evs);
    assert_eq!(f.len(), 10, "every echo is still emitted");
    let tagged: Vec<&Value> = f
        .iter()
        .filter(|e| e.request_id.is_some())
        .map(|e| &e.payload["frame_index"])
        .collect();
    assert_eq!(tagged, [&json!(6), &json!(7), &json!(8), &json!(9)]);
    let tags = &h.status()["channels"][0]["tags"];
    assert_eq!(tags["evicted"], json!(6));
    assert_eq!(tags["untagged_echo"], json!(6));
    assert_eq!(tags["live"], json!(0));
}

#[test]
fn observer_overflow_is_a_source_gap() {
    let mut h = harness_with(WorkerLimits {
        observer_queue: 2,
        ..WorkerLimits::default()
    });
    h.open("ch0");
    h.events();
    // 10 frames at once into a 2-record observer queue: 8 echoes lost.
    h.send(
        Origin::Agent,
        "g1",
        json!({"channel": "ch0", "frame": "300#01", "count": 10}),
    );
    let mut evs = h.events();
    assert_eq!(frames(&evs).len(), 2);
    // The tail is reported immediately, without waiting for a BCM reply.
    assert!(evs.iter().any(|e| e.kind == kinds::SOURCE_GAP));
    // Replies may overflow too; their tail must also be reported, exactly once.
    h.step_ms(5);
    evs.extend(h.events());
    let gap = evs
        .iter()
        .find(|e| e.kind == kinds::SOURCE_GAP)
        .expect("source.gap");
    assert_eq!(gap.payload["count"], json!("8"));
    assert_eq!(gap.payload["cause"], json!(GAP_CAUSE_OBSERVER));
    assert_eq!(gap.channel.as_deref(), Some("ch0"));
    let gi = evs.iter().position(|e| e.kind == kinds::SOURCE_GAP).unwrap();
    assert_eq!(evs[gi + 1].kind, KIND_FRAME);
    assert_no_gap_lies(&evs);
    let ch = &h.status()["channels"][0];
    assert!(ch["observer"]["drops"].as_u64().unwrap() >= 8);
    assert_eq!(ch["source_gap_frames"], ch["observer"]["drops"]);
    let gaps: u64 = evs
        .iter()
        .filter(|e| e.kind == kinds::SOURCE_GAP)
        .map(|e| e.payload["count"].as_str().unwrap().parse::<u64>().unwrap())
        .sum();
    assert_eq!(gaps, ch["observer"]["drops"].as_u64().unwrap());
}

#[test]
fn channels_are_bounded() {
    assert_eq!(WorkerLimits::default().max_channels, MAX_CHANNELS);
    let mut h = harness_with(WorkerLimits {
        max_channels: 2,
        ..WorkerLimits::default()
    });
    h.open("ch0");
    h.open("ch1");
    let e = h.call(CALL_OPEN, json!({"channel": "ch2"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    h.call(CALL_CLOSE, json!({"channel": "ch1"})).unwrap();
    h.open("ch2");
}

#[test]
fn status_and_close() {
    let mut h = harness();
    h.open("ch0");
    h.send(Origin::Agent, "r1", op("300#01"));
    h.step_ms(5);
    let st = h.status();
    assert_eq!(st["latch"], json!(false));
    let ch = &st["channels"][0];
    assert_eq!(ch["channel"], json!("ch0"));
    assert_eq!(ch["state"], json!("open"));
    assert_eq!(ch["generation"], json!(1));
    assert_eq!(ch["frames_by_origin"]["agent"], json!(1));
    assert_eq!(ch["frames_by_origin"]["device"], json!(1));
    assert_eq!(ch["observer"]["drops"], json!(0));
    assert_eq!(ch["frames_emitted"], json!(2));
    assert_eq!(st["nodes"][0]["name"], json!("BCM"));
    assert_eq!(st["nodes"][0]["state"], json!("running"));
    assert_eq!(st["nodes"][0]["channels"], json!(["ch0"]));
    let e = h.call(RPC_STATUS, json!({"channel": "zz"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotOpen);

    // A pending burst is ended by close.
    h.send(
        Origin::Agent,
        "b1",
        json!({"channel": "ch0", "frame": "300#01", "count": 3, "gap_ms": 50}),
    );
    h.events();
    let r = h.call(CALL_CLOSE, json!({"channel": "ch0"})).unwrap();
    assert_eq!(r["state"], json!("closed"));
    let evs = h.events();
    assert_eq!(states(&tx_rows(&evs, "b1")), [("failed".into(), 1)]);
    assert_eq!(tx_rows(&evs, "b1")[0].payload["error"], json!("E_NOT_OPEN"));
    let cs = evs.iter().find(|e| e.kind == KIND_CHANNEL_STATE).unwrap();
    assert_eq!(cs.payload["state"], json!("closed"));
    assert_eq!(cs.generation, Some(1));

    let st = h.status();
    assert_eq!(st["channels"], json!([]));
    assert_eq!(st["nodes"], json!([]), "nodes started by the channel are removed");
    assert_eq!(
        h.send(Origin::Agent, "r2", op("300#01")).error.unwrap().code,
        ErrorCode::NotOpen
    );
    h.step_ms(200);
    assert!(frames(&h.events()).is_empty(), "nothing emitted after close");
    let e = h.call(CALL_CLOSE, json!({"channel": "ch0"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotOpen);

    // Reopen → next generation, same bus, a fresh BCM.
    assert_eq!(h.open("ch0")["generation"], json!(2));
    h.events();
    h.send(Origin::Agent, "r3", op("300#07"));
    h.step_ms(5);
    let evs = h.events();
    let f = frames(&evs);
    assert_eq!(f.len(), 2);
    assert!(f.iter().all(|e| e.generation == Some(2)));
}

#[test]
fn node_calls() {
    let mut h = harness();
    let r = h
        .call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    assert_eq!(r["nodes"], json!([]));
    h.send(Origin::Agent, "r1", op("300#01"));
    h.step_ms(200);
    assert_eq!(frames(&h.events()).len(), 1, "no ECU → only the echo");
    assert_eq!(h.call(CALL_NODE_LIST, json!({})).unwrap()["nodes"], json!([]));

    let r = h
        .call(CALL_NODE_START, json!({"channel": "ch0", "nodes": "default"}))
        .unwrap();
    assert_eq!(r["started"], json!(["BCM"]));
    let r = h
        .call(CALL_NODE_START, json!({"channel": "ch0", "nodes": "default"}))
        .unwrap();
    assert_eq!(r["started"], json!([]));
    assert_eq!(r["already_running"], json!(["BCM"]));

    let custom = r#"
        [[node]]
        name = "GW"
        channel = "ignored"
        [[node.on]]
        id = 0x400
        reply_id = 0x401
        reply = "AA"
        delay_ms = 1.0
    "#;
    let r = h
        .call(CALL_NODE_START, json!({"channel": "ch0", "nodes": custom}))
        .unwrap();
    assert_eq!(r["started"], json!(["GW"]));
    h.events();
    h.send(Origin::Agent, "r2", op("400#01"));
    h.step_ms(1);
    let evs = h.events();
    let f = frames(&evs);
    assert_eq!((id_of(f[1]), data_of(f[1])), (0x401, "aa"));

    let r = h
        .call(CALL_NODE_STOP, json!({"channel": "ch0", "name": "BCM"}))
        .unwrap();
    assert_eq!(r["state"], json!("stopped"));
    h.step_ms(500);
    assert!(
        frames(&h.events()).iter().all(|e| id_of(e) != 0x310),
        "stopped node is silent"
    );
    let list = h.call(CALL_NODE_LIST, json!({})).unwrap();
    let bcm = list["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == json!("BCM"))
        .unwrap();
    assert_eq!(bcm["state"], json!("stopped"));
    let e = h
        .call(CALL_NODE_STOP, json!({"channel": "ch0", "name": "BCM"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);

    let e = h
        .call(
            CALL_NODE_START,
            json!({"channel": "ch0", "nodes": "[[node]]\nname = 1"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Config);
    let e = h
        .call(CALL_NODE_START, json!({"channel": "zz", "nodes": "default"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotOpen);
    let e = h.call("can.nope", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownMethod);
}

/// Two channels on one sim bus: the sender's channel tags its echo; the other channel sees the
/// same frame with the same origin but no request_id. The BCM is not started twice.
#[test]
fn two_channels_share_a_bus() {
    let mut h = harness();
    h.call(CALL_OPEN, json!({"channel": "ch0", "bus": "sim1"}))
        .unwrap();
    let r = h
        .call(CALL_OPEN, json!({"channel": "ch1", "bus": "sim1"}))
        .unwrap();
    assert_eq!(r["nodes"], json!([]));
    assert_eq!(r["nodes_already_running"], json!(["BCM"]));
    h.events();
    h.send(Origin::Agent, "r1", op("300#01"));
    h.step_ms(5);
    let evs = h.events();
    let on = |ch: &str| -> Vec<&PublishedRecord> {
        evs.iter()
            .filter(|e| e.kind == KIND_FRAME && e.channel.as_deref() == Some(ch))
            .collect()
    };
    let (a, b) = (on("ch0"), on("ch1"));
    assert_eq!(a.len(), 2, "echo + one reply");
    assert_eq!(b.len(), 2);
    assert_eq!(a[0].request_id.as_deref(), Some("r1"));
    assert_eq!((b[0].origin, b[0].request_id.as_deref()), (Origin::Agent, None));
    assert_eq!(id_of(a[1]), 0x301);
    assert_eq!(id_of(b[1]), 0x301);
}

#[test]
fn jobs_and_input_are_refused() {
    let mut h = harness();
    let e = h.fw.input("lease-1", b"x", false).expect_err("CAN has no input");
    assert_eq!(e.code, ErrorCode::Unsupported);
}

/// Thread mode with the real clock: the pump thread delivers the BCM reply on its own.
#[test]
fn pump_thread_delivers_replies() {
    let mut fw = FakeWorker::new(CanSimWorker::new());
    fw.call(CALL_OPEN, json!({"channel": "ch0"})).unwrap();
    let start = Instant::now();
    let mut seen = false;
    let mut n = 0;
    while !seen && start.elapsed() < Duration::from_secs(5) {
        // Re-send now and then: the test sink drops a push if the test holds its lock.
        if n % 20 == 0 {
            let out = fw.write(&format!("t{n}"), json!({"channel": "ch0", "frame": "300#01"}));
            assert_eq!(out.state, WriteState::Sent);
        }
        n += 1;
        std::thread::sleep(Duration::from_millis(2));
        seen = fw
            .drain()
            .iter()
            .any(|(_, r)| r.kind == KIND_FRAME && r.payload["id"] == json!(0x301));
    }
    assert!(seen, "pump thread delivered 0x301");
    drop(fw); // joins the pump thread
}
