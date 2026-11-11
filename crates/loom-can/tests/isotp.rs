//! ISO-TP endpoints in the can_sim worker (K10 minimal) through `FakeWorker` + `ManualClock`:
//! client ↔ sim-ECU round trips, frame annotations and event order, FD, reserved tx ids,
//! ownership and limits, abort, N_Cr, STOP.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can::isotp::{MAX_COMPLETED_PDUS, MAX_ENDPOINTS_PER_CHANNEL, VinDemo};
use loom_can::worker::{CALL_CLOSE, CALL_NODE_START, CALL_OPEN, CanSimWorker, PumpMode, WorkerLimits};
use loom_can::{
    KIND_FRAME, KIND_ISOTP_RX_COMPLETE, KIND_ISOTP_RX_ERROR, KIND_ISOTP_RX_START, KIND_ISOTP_STATE,
    KIND_ISOTP_TX_DONE, KIND_ISOTP_TX_ERROR, KIND_TX, RPC_ISOTP_ABORT, RPC_ISOTP_BARRIER, RPC_ISOTP_CANCEL,
    RPC_ISOTP_CLOSE, RPC_ISOTP_LIST, RPC_ISOTP_OPEN, RPC_ISOTP_RECV, RPC_ISOTP_SEND,
};
use loom_can_sim::ManualClock;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{WorkerError, Write};
use loom_testkit::FakeWorker;
use loom_worker_sdk::WriteOutcome;
use serde_json::{Value, json};

const MS: u64 = 1_000_000;

struct H {
    fw: FakeWorker<CanSimWorker>,
    clock: ManualClock,
}

fn harness() -> H {
    harness_with_limits(WorkerLimits::default())
}

fn harness_with_limits(limits: WorkerLimits) -> H {
    let clock = ManualClock::new(0);
    let w = CanSimWorker::with_options(clock.clone(), PumpMode::Manual, limits);
    let mut h = H {
        fw: FakeWorker::new(w),
        clock,
    };
    h.call(CALL_OPEN, json!({"channel": "ch0", "nodes": "none"}))
        .unwrap();
    h.events();
    h
}

impl H {
    fn pump(&mut self) {
        for _ in 0..1_000 {
            if !self.fw.worker().pump_once().more_pending {
                break;
            }
        }
    }

    /// Advances both clocks 1 ms at a time, pumping after each step.
    fn step_ms(&mut self, ms: u64) {
        for _ in 0..ms {
            self.clock.advance_ms(1);
            self.fw.clock().advance_ns(MS);
            self.pump();
        }
    }

    fn call(&mut self, method: &str, args: Value) -> Result<Value, WorkerError> {
        self.fw.call(method, args)
    }

    fn open(&mut self, args: Value) -> Result<String, WorkerError> {
        let mut a = json!({"channel": "ch0"});
        a.as_object_mut()
            .unwrap()
            .extend(args.as_object().unwrap().clone());
        self.call(RPC_ISOTP_OPEN, a)
            .map(|v| v["endpoint"].as_str().unwrap().to_owned())
    }

    fn write(&mut self, origin: Origin, rid: &str, op: Value) -> WriteOutcome {
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

    fn tp_send(&mut self, origin: Origin, rid: &str, ep: &str, pdu: &str) -> WriteOutcome {
        self.write(
            origin,
            rid,
            json!({"method": RPC_ISOTP_SEND, "endpoint": ep, "pdu": pdu}),
        )
    }

    fn raw(&mut self, origin: Origin, rid: &str, frame: &str) -> WriteOutcome {
        self.write(origin, rid, json!({"channel": "ch0", "frame": frame}))
    }

    fn events(&mut self) -> Vec<PublishedRecord> {
        self.fw.drain_published()
    }
}

fn frames(evs: &[PublishedRecord]) -> Vec<&PublishedRecord> {
    evs.iter().filter(|e| e.kind == KIND_FRAME).collect()
}

fn of<'a>(evs: &'a [PublishedRecord], kind: &str, ep: &str) -> Vec<&'a PublishedRecord> {
    evs.iter()
        .filter(|e| e.kind == kind && e.payload["endpoint"] == json!(ep))
        .collect()
}

fn pos(evs: &[PublishedRecord], f: impl Fn(&PublishedRecord) -> bool) -> usize {
    evs.iter().position(f).unwrap()
}

fn id(e: &PublishedRecord) -> u64 {
    e.payload["id"].as_u64().unwrap()
}

fn data(e: &PublishedRecord) -> &str {
    e.payload["bytes"]["data"].as_str().unwrap()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn error_code(e: &WriteOutcome) -> ErrorCode {
    e.error.as_ref().unwrap().code
}

#[test]
fn cancel_checks_identity_and_owner_then_stops_only_target_tx() {
    let mut h = harness();
    let a = h
        .open(json!({"tx_id":0x7e0,"rx_id":0x7e8,"st_min":10,"n_cr_ms":50}))
        .unwrap();
    let b = h
        .open(json!({"tx_id":0x7e8,"rx_id":0x7e0,"st_min":10,"n_cr_ms":50}))
        .unwrap();
    let sent = h.tp_send(Origin::Agent, "target", &a, &"AB".repeat(100));
    assert_eq!(sent.state, WriteState::Accepted);
    let transfer = sent.detail["transfer"].as_str().unwrap().to_owned();
    h.step_ms(25);
    // A simultaneously receives another PDU; cancellation must preserve its RX/FC.
    assert_eq!(
        h.tp_send(Origin::Agent, "reverse", &b, &"CD".repeat(100)).state,
        WriteState::Accepted
    );
    h.events();
    let op = json!({"method":RPC_ISOTP_CANCEL,"endpoint":a,"transfer":transfer});
    let wrong_origin = h.write(Origin::User, "wrong-owner", op.clone());
    assert_eq!(error_code(&wrong_origin), ErrorCode::Locked);
    let wrong_transfer = h.write(
        Origin::Agent,
        "wrong-transfer",
        json!({"method":RPC_ISOTP_CANCEL,"endpoint":a,"transfer":"99.99"}),
    );
    assert_eq!(error_code(&wrong_transfer), ErrorCode::NotFound);
    let cx = h.fw.ctx().clone();
    let wrong_client = h.fw.worker().write(
        &cx,
        Write {
            call_id: U64(9),
            request_id: "other".into(),
            write_class: WriteClass::Request,
            op: op.clone(),
            origin: Origin::Agent,
            client_id: Some("other-agent".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(u64::MAX),
        },
    );
    assert_eq!(error_code(&wrong_client), ErrorCode::Locked);
    let cancelled = h.write(Origin::Agent, "cancel", op.clone());
    assert_eq!(cancelled.detail["state"], "cancelled");
    assert!(cancelled.detail["residual_tx_possible"].as_u64().unwrap() > 0);
    let events = h.events();
    let errors = of(&events, KIND_ISOTP_TX_ERROR, &a);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].payload["code"], "CANCELLED");
    assert_eq!(errors[0].request_id.as_deref(), Some("target"));
    assert_eq!(errors[0].origin, Origin::Agent);
    assert!(of(&events, KIND_ISOTP_RX_ERROR, &a).is_empty());
    assert_eq!(
        error_code(&h.write(Origin::Agent, "again", op)),
        ErrorCode::NotFound
    );
    h.step_ms(200);
    let events = h.events();
    assert_eq!(
        of(&events, KIND_ISOTP_RX_COMPLETE, &a)[0].payload["pdu"]["data"],
        "cd".repeat(100)
    );
    assert_eq!(
        of(&events, KIND_ISOTP_RX_ERROR, &b)[0].payload["code"],
        "N_CR_TIMEOUT"
    );
    assert!(of(&events, KIND_ISOTP_TX_DONE, &a).is_empty());
    // A may still send FC for its surviving receive, but no target data CF follows cancellation.
    assert!(
        frames(&events)
            .iter()
            .filter(|e| id(e) == 0x7e0)
            .all(|e| data(e).starts_with('3'))
    );
    let done = h.tp_send(Origin::Agent, "finished", &a, "3E00");
    assert_eq!(done.state, WriteState::Accepted);
    let out = h.write(
        Origin::Agent,
        "too-late",
        json!({"method":RPC_ISOTP_CANCEL,"endpoint":a,"transfer":done.detail["transfer"]}),
    );
    assert_eq!(error_code(&out), ErrorCode::NotFound);
}

#[test]
fn recv_ring_is_ordered_replayable_bounded_and_reports_loss() {
    let mut h = harness();
    let ep = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    let other = h.open(json!({"tx_id":0x700,"rx_id":0x708})).unwrap();
    for i in 1..=40 {
        h.raw(Origin::User, "incoming", &format!("7E8#01{i:02X}"));
        h.events();
    }
    let args = json!({"endpoint":ep,"timeout_ms":60000});
    let first = h.call(RPC_ISOTP_RECV, args.clone()).unwrap();
    assert_eq!(first["pdus"].as_array().unwrap().len(), 16);
    assert_eq!(first["pdus"][0]["pdu_seq"], "9");
    assert_eq!(first["pdus"][0]["pdu"]["data"], "09");
    assert_eq!(first["next_pdu_seq"], "24");
    assert_eq!(first["latest_pdu_seq"], "40");
    assert_eq!(first["dropped"], "8");
    assert_eq!(first["cursor_gap"], true);
    assert_eq!(h.call(RPC_ISOTP_RECV, args).unwrap(), first);
    let second = h
        .call(
            RPC_ISOTP_RECV,
            json!({"endpoint":ep,"after_pdu_seq":"24","max_pdus":16}),
        )
        .unwrap();
    assert_eq!(second["pdus"][0]["pdu_seq"], "25");
    assert_eq!(second["next_pdu_seq"], "40");
    assert_eq!(second["cursor_gap"], false);
    let empty = h
        .call(RPC_ISOTP_RECV, json!({"endpoint":ep,"after_pdu_seq":40}))
        .unwrap();
    assert!(empty["pdus"].as_array().unwrap().is_empty());
    assert_eq!(empty["next_pdu_seq"], "40");
    let empty = h.call(RPC_ISOTP_RECV, json!({"endpoint":other})).unwrap();
    assert_eq!(empty["next_pdu_seq"], "0");
    let list = h.call(RPC_ISOTP_LIST, json!({})).unwrap();
    assert!(
        list["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["endpoint"] == ep && e["completed_pdus"] == MAX_COMPLETED_PDUS)
    );
    for extra in [
        json!({"max_pdus":0}),
        json!({"max_pdus":17}),
        json!({"timeout_ms":60001}),
        json!({"after_pdu_seq":"18446744073709551616"}),
        json!({"after_pdu_seq":41}),
        json!({"after_pdu_seq":-1}),
    ] {
        let mut args = json!({"endpoint":ep});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert_eq!(h.call(RPC_ISOTP_RECV, args).unwrap_err().code, ErrorCode::Usage);
    }
}

#[test]
fn recv_large_pdus_page_within_worker_envelope_without_skipping() {
    let mut h = harness();
    let a = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    let b = h.open(json!({"tx_id":0x7e8,"rx_id":0x7e0})).unwrap();
    for _ in 0..8 {
        assert_eq!(
            h.tp_send(Origin::Agent, "large", &b, &"AB".repeat(4095)).state,
            WriteState::Accepted
        );
        h.pump();
        h.events();
    }
    let first = h
        .call(RPC_ISOTP_RECV, json!({"endpoint":a,"max_pdus":16}))
        .unwrap();
    let returned = first["pdus"].as_array().unwrap().len();
    assert!((1..8).contains(&returned));
    assert!(serde_json::to_vec(&first).unwrap().len() < 64 * 1024);
    assert_eq!(first["next_pdu_seq"], returned.to_string());
    let second = h
        .call(
            RPC_ISOTP_RECV,
            json!({"endpoint":a,"after_pdu_seq":first["next_pdu_seq"]}),
        )
        .unwrap();
    assert_eq!(returned + second["pdus"].as_array().unwrap().len(), 8);
    assert_eq!(second["next_pdu_seq"], "8");
}

fn queued_node(h: &mut H) {
    let nodes = r#"[[node]]
name = "barrier-input"
channel = "ch0"
[[node.periodic]]
id = 0x111
period_ms = 1000.0
offset_ms = 1.0
data = "00"
[[node.periodic]]
id = 0x7E8
period_ms = 1000.0
offset_ms = 1.0
data = "10 14 62 F1 90 00 00 00"
"#;
    h.call(CALL_NODE_START, json!({"channel":"ch0","nodes":nodes}))
        .unwrap();
    h.clock.advance_ms(2);
    h.fw.clock().advance_ns(2 * MS);
}

#[test]
fn barrier_processes_queued_ff_before_proving_deadline_and_checks_epoch() {
    let mut h = harness_with_limits(WorkerLimits {
        max_drain_per_pump: 1,
        ..WorkerLimits::default()
    });
    let ep = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    let epoch = h.call(RPC_ISOTP_LIST, json!({})).unwrap()["endpoints"][0]["epoch"].clone();
    h.events();
    queued_node(&mut h);
    let args = json!({"endpoint":ep,"epoch":epoch,"deadline_mono_ns":(2*MS).to_string(),"timeout_ms":60000});
    let first = h.call(RPC_ISOTP_BARRIER, args.clone()).unwrap();
    assert_eq!(first["complete"], false);
    assert_eq!(first["covered_ingest_seq"], "1");
    assert!(of(&h.events(), KIND_ISOTP_RX_START, &ep).is_empty());
    let second = h.call(RPC_ISOTP_BARRIER, args.clone()).unwrap();
    assert_eq!(second["complete"], true);
    assert_eq!(second["covered_ingest_seq"], "2");
    assert_eq!(second["covered_mono_ns"], (2 * MS).to_string());
    let events = h.events();
    let start = of(&events, KIND_ISOTP_RX_START, &ep)[0];
    assert_eq!(start.payload["ts_first_frame"], MS.to_string());
    assert_eq!(second["transport_event_seq"], start.source_seq.get().to_string());
    assert!(
        of(&events, KIND_ISOTP_RX_COMPLETE, &ep).is_empty(),
        "barrier proves processing, not PDU completion"
    );
    let mut future = args.clone();
    future["deadline_mono_ns"] = json!((3 * MS).to_string());
    assert_eq!(h.call(RPC_ISOTP_BARRIER, future).unwrap()["complete"], false);
    let mut stale = args.clone();
    stale["epoch"] = json!(999);
    let error = h.call(RPC_ISOTP_BARRIER, stale).unwrap_err();
    assert_eq!(error.code, ErrorCode::CursorGap);
    assert_eq!(error.detail["code"], "E_EPOCH_CHANGED");
    h.call(RPC_ISOTP_CLOSE, json!({"endpoint":ep})).unwrap();
    let reopened = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    let mut old = args;
    old["endpoint"] = json!(reopened);
    assert_eq!(
        h.call(RPC_ISOTP_BARRIER, old).unwrap_err().code,
        ErrorCode::CursorGap
    );
}

#[test]
fn barrier_never_proves_missing_ingress_after_queue_overflow() {
    let mut h = harness_with_limits(WorkerLimits {
        observer_queue: 1,
        ..WorkerLimits::default()
    });
    let ep = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    let epoch = h.call(RPC_ISOTP_LIST, json!({})).unwrap()["endpoints"][0]["epoch"].clone();
    queued_node(&mut h);
    let args = json!({"endpoint":ep,"epoch":epoch,"deadline_mono_ns":(2*MS).to_string()});
    for _ in 0..2 {
        let result = h.call(RPC_ISOTP_BARRIER, args.clone()).unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["covered_mono_ns"], "0");
    }
}

#[test]
fn barrier_publication_failure_cannot_turn_into_a_negative_response_proof() {
    use loom_proto::worker::Call;
    use loom_worker_sdk::{Emitter, LatchFlag, MemorySink, SimClock, WorkerCtx};
    use std::sync::Arc;
    let sink = Arc::new(MemorySink::new(1));
    let base = Emitter::new(
        "test",
        "can",
        "can-proof",
        U64(1),
        1,
        "ch0",
        sink.clone(),
        Arc::new(SimClock::new(0)),
    );
    let factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync> = Arc::new(move |ch| base.for_channel(ch));
    let cx = WorkerCtx::new(LatchFlag::new(), Arc::new(SimClock::new(0)), factory);
    let mut worker = CanSimWorker::manual(ManualClock::new(0));
    let mut call = Call {
        conn_id: None,
        call_id: U64(1),
        method: CALL_OPEN.into(),
        args: json!({"channel":"ch0","nodes":"none"}),
        origin: Origin::Agent,
        client_id: Some("a".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(u64::MAX),
        approval_ref: None,
    };
    worker.call(&cx, &call).unwrap();
    sink.drain();
    call.method = RPC_ISOTP_OPEN.into();
    call.args = json!({"channel":"ch0","tx_id":0x7e0,"rx_id":0x7e8});
    let opened = worker.call(&cx, &call).unwrap(); // Fill the only slot with isotp_state.
    worker.write(
        &cx,
        Write {
            call_id: U64(2),
            request_id: "sf".into(),
            write_class: WriteClass::Request,
            op: json!({"channel":"ch0","frame":"7E8#013E"}),
            origin: Origin::User,
            client_id: Some("u".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(u64::MAX),
        },
    );
    sink.drain(); // Clearing the sink cannot retroactively publish the lost rx_start.
    call.method = RPC_ISOTP_BARRIER.into();
    call.args = json!({"endpoint":opened["endpoint"],"epoch":opened["epoch"],"deadline_mono_ns":"0"});
    let barrier = worker.call(&cx, &call).unwrap();
    assert_eq!(barrier["complete"], false);
    assert_eq!(barrier["covered_mono_ns"], "0");
    call.method = RPC_ISOTP_RECV.into();
    call.args = json!({"endpoint":opened["endpoint"]});
    let fallback = worker.call(&cx, &call).unwrap();
    assert_eq!(fallback["pdus"][0]["pdu"]["data"], "3e");
    assert_eq!(fallback["next_pdu_seq"], "1");
    call.method = RPC_ISOTP_LIST.into();
    call.args = json!({});
    let list = worker.call(&cx, &call).unwrap();
    assert!(
        list["endpoints"][0]["counters"]["events_failed"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn contract_call_validation_is_bounded_and_cancel_is_write_only() {
    let mut h = harness();
    let ep = h.open(json!({"tx_id":0x7e0,"rx_id":0x7e8})).unwrap();
    assert_eq!(
        h.call(RPC_ISOTP_CANCEL, json!({"endpoint":ep,"transfer":"1.1"}))
            .unwrap_err()
            .code,
        ErrorCode::UnknownMethod
    );
    for bad in [
        json!({"timeout_ms":60001}),
        json!({"deadline_mono_ns":-1}),
        json!({"epoch":"not-an-epoch"}),
        json!({"deadline_mono_ns":"18446744073709551616"}),
    ] {
        let mut args = json!({"endpoint":ep,"epoch":1,"deadline_mono_ns":"0"});
        args.as_object_mut()
            .unwrap()
            .extend(bad.as_object().unwrap().clone());
        assert_eq!(
            h.call(RPC_ISOTP_BARRIER, args).unwrap_err().code,
            ErrorCode::Usage
        );
    }
}

// ---- round trips ----------------------------------------------------------------------------------

#[test]
fn client_server_vin_round_trip() {
    let mut h = harness();
    let client = h
        .open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8, "st_min": 5, "owner": "uds:test"}))
        .unwrap();
    let server = h
        .open(json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server", "model": "vin_demo"}))
        .unwrap();
    let opened = h.events();
    assert_eq!(
        of(&opened, KIND_ISOTP_STATE, &client)[0].payload["state"],
        json!("open")
    );
    assert_eq!(of(&opened, KIND_ISOTP_STATE, &server).len(), 1);

    let out = h.tp_send(Origin::Agent, "u1", &client, "22F190");
    assert_eq!(out.state, WriteState::Accepted, "{:?}", out.error);
    assert_eq!(out.detail["accepted"], json!(true));
    assert_eq!(out.detail["endpoint"], json!(client));
    assert!(out.detail["transfer"].is_string());
    h.step_ms(40);
    let evs = h.events();

    // Every ISO-TP frame is an ordinary `can.frame` (no hidden traffic), annotated.
    let fr = frames(&evs);
    assert_eq!(
        fr.iter().map(|e| id(e)).collect::<Vec<_>>(),
        [0x7E0, 0x7E8, 0x7E0, 0x7E8, 0x7E8],
        "SF, FF, FC, CF, CF"
    );
    assert_eq!(data(fr[0]), "0322f190cccccccc", "SF padded with CC");
    assert!(data(fr[1]).starts_with("101462f190"), "FF of 20 bytes");
    assert_eq!(data(fr[2]), "300805cccccccccc", "FC CTS BS 8 STmin 5");
    assert!(data(fr[3]).starts_with("21") && data(fr[4]).starts_with("22"));
    for f in &fr {
        assert_eq!(f.payload["isotp"]["endpoint"], json!(client), "{:?}", f.payload);
        assert_eq!(f.payload["isotp"]["request_id"], json!("u1"));
    }
    assert_eq!((fr[0].origin, fr[2].origin), (Origin::Agent, Origin::Agent));
    assert_eq!((fr[1].origin, fr[3].origin), (Origin::Device, Origin::Device));
    assert_eq!(fr[0].request_id.as_deref(), Some("u1"));
    assert_eq!(
        fr[2].request_id.as_deref(),
        Some("u1"),
        "FC carries the request too"
    );

    // Reply after the model delay; CF spacing ≥ the STmin the client asked for.
    let gap = |a: &PublishedRecord, b: &PublishedRecord| b.ts.mono_ns.get() - a.ts.mono_ns.get();
    assert!(gap(fr[0], fr[1]) >= VinDemo::DELAY.as_nanos() as u64);
    assert!(gap(fr[3], fr[4]) >= 5 * MS, "STmin 5 ms: {}", gap(fr[3], fr[4]));

    // Order: FF frame → rx_start → FC frame (rx_start is published before its FC).
    let ff = pos(&evs, |e| e.kind == KIND_FRAME && id(e) == 0x7E8);
    let start = pos(&evs, |e| {
        e.kind == KIND_ISOTP_RX_START && e.payload["endpoint"] == json!(client)
    });
    let fc = pos(&evs, |e| e.kind == KIND_FRAME && data(e).starts_with("30"));
    assert!(ff < start && start < fc, "ff {ff} start {start} fc {fc}");
    assert_eq!(evs[start].payload["expected_length"], json!(20));
    assert_eq!(evs[start].payload["first_seq"], json!(fr[1].source_seq.get()));

    let done = of(&evs, KIND_ISOTP_TX_DONE, &client);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].payload["tx_state"], json!("driver_accepted"));
    assert_eq!(done[0].request_id.as_deref(), Some("u1"));
    assert_eq!(done[0].payload["frames"], json!(1));

    let rx = of(&evs, KIND_ISOTP_RX_COMPLETE, &client);
    assert_eq!(rx.len(), 1);
    let mut vin = vec![0x62, 0xF1, 0x90];
    vin.extend_from_slice(VinDemo::VIN);
    assert_eq!(rx[0].payload["pdu"]["data"], json!(hex(&vin)));
    assert_eq!(rx[0].payload["len"], json!(20));
    assert_eq!(rx[0].payload["frames"], json!(3));
    assert_eq!(rx[0].payload["first_seq"], json!(fr[1].source_seq.get()));
    assert_eq!(rx[0].payload["last_seq"], json!(fr[4].source_seq.get()));
    assert_eq!(rx[0].request_id.as_deref(), Some("u1"));
    assert_eq!(rx[0].origin, Origin::Device);

    // The server saw the SF request: a synthetic rx_start (single frame) then rx_complete.
    let s_start = of(&evs, KIND_ISOTP_RX_START, &server);
    assert_eq!(s_start[0].payload["single_frame"], json!(true));
    assert_eq!(
        of(&evs, KIND_ISOTP_RX_COMPLETE, &server)[0].payload["pdu"]["data"],
        json!("22f190")
    );
    // Unknown service → negative response.
    h.tp_send(Origin::Agent, "u2", &client, "1001");
    h.step_ms(10);
    let evs = h.events();
    assert_eq!(
        of(&evs, KIND_ISOTP_RX_COMPLETE, &client)[0].payload["pdu"]["data"],
        json!("7f1011")
    );

    let list = h.call(RPC_ISOTP_LIST, json!({})).unwrap();
    let c = &list["endpoints"][0];
    assert_eq!(
        (c["endpoint"].clone(), c["owner"].clone()),
        (json!(client), json!("uds:test"))
    );
    assert_eq!(c["counters"]["tx_pdu"], json!(2));
    assert_eq!(c["counters"]["rx_pdu"], json!(2));
    assert_eq!(list["endpoints"][1]["counters"]["replies"], json!(2));
}

#[test]
fn fd_29bit_round_trip_above_64_bytes() {
    let mut h = harness();
    let client = h
        .open(json!({"tx_id": 0x18DA10F1_u32, "rx_id": 0x18DAF110_u32, "fd": true}))
        .unwrap();
    h.open(
        json!({"tx_id": 0x18DAF110_u32, "rx_id": 0x18DA10F1_u32, "fd": true, "role": "server",
                  "model": "echo"}),
    )
    .unwrap();
    let pdu: Vec<u8> = (0..100u8).collect();
    let out = h.tp_send(Origin::User, "fd1", &client, &hex(&pdu));
    assert_eq!(out.state, WriteState::Accepted, "{:?}", out.error);
    h.step_ms(10);
    let evs = h.events();
    let rx = of(&evs, KIND_ISOTP_RX_COMPLETE, &client);
    assert_eq!(rx.len(), 1);
    assert_eq!(rx[0].payload["pdu"]["data"], json!(hex(&pdu)));
    let fr = frames(&evs);
    assert!(
        fr.iter()
            .all(|f| f.payload["fd"] == json!(true) && f.payload["ext"] == json!(true))
    );
    assert!(
        fr.iter().any(|f| f.payload["len"] == json!(64)),
        "64-byte FD frames"
    );
    assert_eq!(fr[0].origin, Origin::User, "user request → user frames");
    assert_eq!(of(&evs, KIND_ISOTP_TX_DONE, &client).len(), 1);
}

// ---- reserved ids, ownership, limits ---------------------------------------------------------------

#[test]
fn reserved_tx_id_refuses_raw_sends_until_close() {
    let mut h = harness();
    let tp = h
        .open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8, "owner": "uds:s1"}))
        .unwrap();
    for origin in [Origin::Agent, Origin::User] {
        let out = h.raw(origin, "r", "7E0#0210010000000000");
        assert_eq!(out.state, WriteState::Failed);
        let e = out.error.unwrap();
        assert_eq!((e.code, e.code.exit_code()), (ErrorCode::Denied, 4));
        assert_eq!(e.detail["reason"], json!("tx_reserved"));
        assert_eq!(e.detail["endpoint"], json!(tp));
        assert_eq!(e.detail["owner"], json!("uds:s1"));
    }
    let evs = h.events();
    assert!(
        evs.iter().all(|e| e.kind != KIND_FRAME),
        "nothing reached the bus"
    );
    let rows: Vec<_> = evs.iter().filter(|e| e.kind == KIND_TX).collect();
    assert!(rows.iter().all(|r| r.payload["error"] == json!("E_DENIED")));
    // The endpoint's rx id is not reserved.
    assert_eq!(h.raw(Origin::Agent, "r2", "7E8#01").state, WriteState::Sent);

    // A burst already running stops when an endpoint takes its id.
    let out = h.write(
        Origin::Agent,
        "b1",
        json!({"channel": "ch0", "frame": "7E1#01", "count": 3, "gap_ms": 10}),
    );
    assert_eq!(out.state, WriteState::Accepted);
    h.open(json!({"tx_id": 0x7E1, "rx_id": 0x7E9})).unwrap();
    h.step_ms(25);
    let evs = h.events();
    let last = evs
        .iter()
        .rfind(|e| e.kind == KIND_TX && e.request_id.as_deref() == Some("b1"))
        .unwrap();
    assert_eq!(last.payload["write_state"], json!("failed"));
    assert_eq!(last.payload["error"], json!("E_DENIED"));

    h.call(RPC_ISOTP_CLOSE, json!({"endpoint": tp})).unwrap();
    let evs = h.events();
    assert_eq!(
        of(&evs, KIND_ISOTP_STATE, &tp)[0].payload["state"],
        json!("closed")
    );
    assert_eq!(h.raw(Origin::Agent, "r3", "7E0#01").state, WriteState::Sent);
    assert_eq!(
        h.call(RPC_ISOTP_CLOSE, json!({"endpoint": tp})).unwrap_err().code,
        ErrorCode::NotOpen
    );
}

#[test]
fn ownership_validation_and_limits() {
    let mut h = harness();
    let tp1 = h
        .open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8, "owner": "uds:a"}))
        .unwrap();
    for (tx, rx) in [(0x7E0, 0x7E9), (0x7E1, 0x7E8)] {
        let e = h.open(json!({"tx_id": tx, "rx_id": rx})).unwrap_err();
        assert_eq!((e.code, e.code.exit_code()), (ErrorCode::Locked, 6));
        assert_eq!(e.detail["endpoint"], json!(tp1));
        assert_eq!(e.detail["owner"], json!("uds:a"));
    }
    // The mirrored server is a different address pair.
    h.open(json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server"}))
        .unwrap();

    let usage = [
        json!({"tx_id": 0x100, "rx_id": 0x100}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "role": "server", "model": "nope"}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "model": "vin_demo"}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "st_min": 0x80}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "tx_dl": 64}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "n_cr_ms": 70_000}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "padding": 300}),
        json!({"tx_id": 0x20000000_u32, "rx_id": 0x101}),
        json!({"tx_id": 0x100, "rx_id": 0x101, "bogus": 1}),
    ];
    for a in usage {
        assert_eq!(h.open(a.clone()).unwrap_err().code, ErrorCode::Usage, "{a}");
    }
    let e = h
        .call(RPC_ISOTP_OPEN, json!({"channel": "ch9", "tx_id": 1, "rx_id": 2}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotOpen);
    // `padding: null` = no padding.
    let np = h
        .open(json!({"tx_id": 0x600, "rx_id": 0x601, "padding": null}))
        .unwrap();
    let list = h.call(RPC_ISOTP_LIST, json!({"channel": "ch0"})).unwrap();
    let info = list["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["endpoint"] == json!(np))
        .unwrap()
        .clone();
    assert_eq!(info["padding"], Value::Null);

    let open_now = list["endpoints"].as_array().unwrap().len();
    for i in open_now..MAX_ENDPOINTS_PER_CHANNEL {
        h.open(json!({"tx_id": 0x100 + i, "rx_id": 0x300 + i})).unwrap();
    }
    let e = h.open(json!({"tx_id": 0x500, "rx_id": 0x501})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    let list = h.call(RPC_ISOTP_LIST, json!({})).unwrap();
    assert_eq!(
        list["endpoints"].as_array().unwrap().len(),
        MAX_ENDPOINTS_PER_CHANNEL
    );

    // Closing the channel closes its endpoints.
    h.events();
    h.call(CALL_CLOSE, json!({"channel": "ch0"})).unwrap();
    let evs = h.events();
    let closed = evs
        .iter()
        .filter(|e| e.kind == KIND_ISOTP_STATE && e.payload["state"] == json!("closed"))
        .count();
    assert_eq!(closed, MAX_ENDPOINTS_PER_CHANNEL);
    assert_eq!(h.call(RPC_ISOTP_LIST, json!({})).unwrap()["endpoints"], json!([]));
}

#[test]
fn send_errors() {
    let mut h = harness();
    let tp = h.open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8})).unwrap();
    let srv = h
        .open(json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server", "st_min": 50}))
        .unwrap();
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "e1", "tp99", "01")),
        ErrorCode::NotOpen
    );
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "e2", &tp, "0")),
        ErrorCode::Usage
    );
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "e3", &tp, "zz")),
        ErrorCode::Usage
    );
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "e4", &tp, &"00".repeat(4096))),
        ErrorCode::Usage
    );
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "e5", &srv, "01")),
        ErrorCode::Usage
    );
    // One transfer at a time: the server asks for STmin 50 ms, so this PDU is still sending.
    assert_eq!(
        h.tp_send(Origin::Agent, "s1", &tp, &"11".repeat(100)).state,
        WriteState::Accepted
    );
    let busy = h.tp_send(Origin::Agent, "s2", &tp, "3E00");
    assert_eq!(error_code(&busy), ErrorCode::Busy);
}

// ---- abort, timeouts --------------------------------------------------------------------------------

#[test]
fn abort_reports_residual_and_peer_times_out() {
    let mut h = harness();
    let client = h.open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8})).unwrap();
    let server = h
        .open(json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server", "st_min": 10, "n_cr_ms": 50}))
        .unwrap();
    h.tp_send(Origin::Agent, "big", &client, &"AB".repeat(100));
    h.step_ms(25);
    let sent_before = frames(&h.events()).iter().filter(|f| id(f) == 0x7E0).count();
    assert!(
        (2..14).contains(&sent_before),
        "mid transfer: {sent_before} frames"
    );

    let r = h.call(RPC_ISOTP_ABORT, json!({"endpoint": client})).unwrap();
    assert_eq!(
        (r["aborted_tx"].clone(), r["aborted_rx"].clone()),
        (json!(true), json!(false))
    );
    assert!(r["residual_tx_possible"].is_u64());
    let evs = h.events();
    let err = of(&evs, KIND_ISOTP_TX_ERROR, &client);
    assert_eq!(err[0].payload["code"], json!("ABORTED"));
    assert_eq!(err[0].request_id.as_deref(), Some("big"));

    // No more CFs; the server's receive times out (N_Cr 50 ms).
    h.step_ms(60);
    let evs = h.events();
    assert!(frames(&evs).iter().all(|f| id(f) != 0x7E0));
    assert_eq!(
        of(&evs, KIND_ISOTP_RX_ERROR, &server)[0].payload["code"],
        json!("N_CR_TIMEOUT")
    );
    // The endpoint is usable again.
    assert_eq!(
        h.tp_send(Origin::Agent, "next", &client, "3E00").state,
        WriteState::Accepted
    );
    h.step_ms(1);
    let evs = h.events();
    assert_eq!(
        of(&evs, KIND_ISOTP_RX_COMPLETE, &server)[0].payload["pdu"]["data"],
        json!("3e00")
    );
}

#[test]
fn dropped_cf_gives_client_n_cr_timeout() {
    let mut h = harness();
    let client = h
        .open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8, "n_cr_ms": 50}))
        .unwrap();
    let server = h
        .open(
            json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server", "model": "vin_demo",
                     "faults": {"drop_cf": 2}}),
        )
        .unwrap();
    h.tp_send(Origin::Agent, "v", &client, "22F190");
    h.step_ms(100);
    let evs = h.events();
    assert_eq!(of(&evs, KIND_ISOTP_RX_START, &client).len(), 1, "FF arrived");
    assert!(of(&evs, KIND_ISOTP_RX_COMPLETE, &client).is_empty());
    let err = of(&evs, KIND_ISOTP_RX_ERROR, &client);
    assert_eq!(err[0].payload["code"], json!("N_CR_TIMEOUT"));
    assert_eq!(err[0].request_id.as_deref(), Some("v"));
    // The sim ECU believes it sent everything (the fault drops the CF on the link).
    assert_eq!(of(&evs, KIND_ISOTP_TX_DONE, &server).len(), 1);
    let cfs = frames(&evs)
        .iter()
        .filter(|f| id(f) == 0x7E8 && data(f).starts_with('2'))
        .count();
    assert_eq!(cfs, 1, "CF 2 never reached the bus");
}

// ---- STOP ---------------------------------------------------------------------------------------------

#[test]
fn stop_aborts_agent_transfers_only() {
    let mut h = harness();
    let a = h.open(json!({"tx_id": 0x7E0, "rx_id": 0x7E8})).unwrap();
    h.open(json!({"tx_id": 0x7E8, "rx_id": 0x7E0, "role": "server", "model": "echo", "st_min": 20}))
        .unwrap();
    let u = h.open(json!({"tx_id": 0x7E2, "rx_id": 0x7EA})).unwrap();
    let su = h
        .open(json!({"tx_id": 0x7EA, "rx_id": 0x7E2, "role": "server", "model": "echo", "st_min": 20}))
        .unwrap();
    h.tp_send(Origin::Agent, "a1", &a, &"AA".repeat(100));
    h.tp_send(Origin::User, "u1", &u, &"BB".repeat(100));
    h.step_ms(30);
    h.events();

    h.fw.latch(true);
    let evs = h.events();
    let err = of(&evs, KIND_ISOTP_TX_ERROR, &a);
    assert_eq!(err[0].payload["code"], json!("ABORTED"));
    assert_eq!(err[0].request_id.as_deref(), Some("a1"));
    assert_eq!(
        of(&evs, KIND_ISOTP_STATE, &a)[0].payload["state"],
        json!("stopped")
    );
    assert!(
        of(&evs, KIND_ISOTP_TX_ERROR, &u).is_empty(),
        "user transfer keeps going"
    );

    // New agent/script sends are refused on any endpoint; user ones are not.
    assert_eq!(
        error_code(&h.tp_send(Origin::Agent, "a2", &a, "3E00")),
        ErrorCode::Stopped
    );
    assert_eq!(
        error_code(&h.tp_send(Origin::Script, "s2", &a, "3E00")),
        ErrorCode::Stopped
    );

    // The user transfer finishes and the sim ECU (device) still answers under STOP.
    h.step_ms(600);
    let evs = h.events();
    assert_eq!(
        of(&evs, KIND_ISOTP_TX_DONE, &u)[0].request_id.as_deref(),
        Some("u1")
    );
    let back = of(&evs, KIND_ISOTP_RX_COMPLETE, &u);
    assert_eq!(back[0].payload["len"], json!(100));
    assert!(of(&evs, KIND_ISOTP_TX_DONE, &su).len() == 1);
    assert!(
        frames(&evs).iter().all(|f| f.origin != Origin::Agent),
        "no agent frames under STOP"
    );
    assert_eq!(
        h.tp_send(Origin::User, "u2", &u, "3E00").state,
        WriteState::Accepted
    );

    h.fw.latch(false);
    let evs = h.events();
    assert_eq!(of(&evs, KIND_ISOTP_STATE, &a)[0].payload["state"], json!("open"));
    h.step_ms(1);
    assert_eq!(
        h.tp_send(Origin::Agent, "a3", &a, "3E00").state,
        WriteState::Accepted
    );
}
