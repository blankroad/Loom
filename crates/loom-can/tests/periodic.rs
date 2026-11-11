//! Worker-side periodic contract, with no gateway/CLI/real clock.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can::schema::FramePayload;
use loom_can::worker::{CALL_CLOSE, CALL_OPEN, CanSimWorker};
use loom_can::*;
use loom_can_frame::parse_frame;
use loom_can_sim::ManualClock;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{WorkerError, Write};
use loom_testkit::FakeWorker;
use loom_worker_sdk::WriteOutcome;
use serde_json::{Value, json};

struct H {
    fw: FakeWorker<CanSimWorker>,
    clock: ManualClock,
}
impl H {
    fn new() -> Self {
        let clock = ManualClock::new(0);
        let mut fw = FakeWorker::new(CanSimWorker::manual(clock.clone()));
        fw.call(CALL_OPEN, json!({"channel":"ch0","nodes":"none"}))
            .unwrap();
        fw.drain();
        Self { fw, clock }
    }
    fn write(&mut self, origin: Origin, client: &str, op: Value) -> WriteOutcome {
        let cx = self.fw.ctx().clone();
        self.fw.worker().write(
            &cx,
            Write {
                call_id: U64(1),
                request_id: "periodic-test".into(),
                write_class: WriteClass::Job,
                op,
                origin,
                client_id: Some(client.into()),
                plan_digest: None,
                approval_ref: None,
                deadline_mono_ns: U64(u64::MAX),
            },
        )
    }
    fn start_as(&mut self, origin: Origin, client: &str, frame: &str, extra: Value) -> WriteOutcome {
        let f = parse_frame(frame, false).unwrap();
        let mut op = json!({"method":RPC_PERIODIC_START,"channel":"ch0",
            "frame":FramePayload::from_frame(&f,None),"period_ms":10});
        op.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.write(origin, client, op)
    }
    fn start(&mut self, frame: &str, extra: Value) -> String {
        let out = self.start_as(Origin::Agent, "agent#1", frame, extra);
        assert_eq!(out.state, WriteState::Accepted, "{out:?}");
        out.detail["job"].as_str().unwrap().into()
    }
    fn step(&mut self, ms: u64) {
        for _ in 0..ms {
            self.clock.advance_ms(1);
            self.fw.clock().advance_ns(1_000_000);
            self.fw.worker().pump_once();
        }
    }
    fn events(&mut self) -> Vec<PublishedRecord> {
        self.fw.drain_published()
    }
    fn list(&mut self) -> Value {
        self.fw.call(RPC_PERIODIC_LIST, json!({})).unwrap()
    }
    fn op(&mut self, method: &str, job: &str, extra: Value) -> WriteOutcome {
        let mut op = json!({"method":method,"job":job});
        op.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.write(Origin::Agent, "agent#1", op)
    }
    fn raw(&mut self, origin: Origin, count: u32) -> WriteOutcome {
        self.write(
            origin,
            "raw#1",
            json!({"channel":"ch0","frame":"123#FF","count":count}),
        )
    }
}
fn frames(events: &[PublishedRecord]) -> Vec<&PublishedRecord> {
    events.iter().filter(|e| e.kind == KIND_FRAME).collect()
}
fn fail(out: WriteOutcome, code: ErrorCode) -> WorkerError {
    assert_eq!(out.state, WriteState::Failed);
    let error = out.error.unwrap();
    assert_eq!(error.code, code);
    error
}

#[test]
fn ten_ms_one_second_100_frames_origin_tags_and_statistics() {
    let mut h = H::new();
    let job = h.start("123#0102", json!({}));
    let started = h.events();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].kind, KIND_JOB_STATE);
    assert_eq!(started[0].payload["state"], "started");
    assert_eq!(started[0].payload["owner"], "agent#1");
    h.step(1000);
    let events = h.events();
    let fs = frames(&events);
    assert_eq!(fs.len(), 100);
    for (i, f) in fs.iter().enumerate() {
        assert_eq!(f.origin, Origin::Agent);
        assert_eq!(f.job_id.as_deref(), Some(job.as_str()));
        assert_eq!(f.purpose.as_deref(), Some("periodic"));
        assert_eq!(f.payload["bytes"]["data"], "0102");
        assert_eq!(f.ts.mono_ns.get(), (i as u64 + 1) * 10_000_000);
    }
    let list = h.list();
    assert_eq!(list["jobs"][0]["sent"], 100);
    assert_eq!(list["jobs"][0]["missed"], 0);
    assert_eq!(list["jobs"][0]["lateness_p99_us"], 0);
    let status = h.fw.call(RPC_STATUS, json!({})).unwrap();
    assert_eq!(status["periodic_jobs"], 1);
    assert_eq!(status["channels"][0]["periodic_jobs"], 1);
}

#[test]
fn patch_cas_and_desired_bytes_are_visible_before_next_tick() {
    let mut h = H::new();
    let job = h.start("123#1122", json!({}));
    h.events();
    let out = h.op(
        RPC_PERIODIC_UPDATE,
        &job,
        json!({"expected_rev":0,"patch":{"mask":"0f","bytes":"ab"}}),
    );
    assert_eq!(out.detail["revision"], 1);
    let current = h.list();
    assert_eq!(current["jobs"][0]["revision"], 1);
    assert_eq!(current["jobs"][0]["applied_revision"], 0);
    assert!(current["jobs"][0].get("effective_revision").is_none());
    assert_eq!(current["jobs"][0]["data"], "1b22");
    assert_eq!(current["jobs"][0]["effective_frame"]["bytes"]["data"], "1122");
    let conflict = fail(
        h.op(
            RPC_PERIODIC_UPDATE,
            &job,
            json!({"expected_rev":0,"patch":{"data":"FFFF"}}),
        ),
        ErrorCode::Locked,
    );
    assert_eq!(conflict.detail["current_rev"], 1);
    assert_eq!(conflict.detail["code"], "E_CONFLICT");
    h.op(
        RPC_PERIODIC_UPDATE,
        &job,
        json!({"expected_rev":1,"patch":{"mask":"00f0","bytes":"00c0"}}),
    );
    let pending = h.list();
    assert_eq!(pending["jobs"][0]["data"], "1bc2");
    assert_eq!(pending["jobs"][0]["revision"], 2);
    assert_eq!(pending["jobs"][0]["applied_revision"], 0);
    assert_eq!(pending["jobs"][0]["effective_frame"]["bytes"]["data"], "1122");
    assert!(frames(&h.events()).is_empty());
    h.step(10);
    let events = h.events();
    let fs = frames(&events);
    assert_eq!(fs.len(), 1);
    assert_eq!(fs[0].payload["bytes"]["data"], "1bc2");
    assert_eq!(fs[0].payload["revision"], 2);
    let applied = h.list();
    assert_eq!(applied["jobs"][0]["data"], "1bc2");
    assert_eq!(applied["jobs"][0]["applied_revision"], 2);
}

#[test]
fn count_completion_is_published_after_last_echo() {
    let mut h = H::new();
    let job = h.start("123#01", json!({"count":3}));
    h.events();
    h.step(100);
    let events = h.events();
    assert_eq!(frames(&events).len(), 3);
    let stopped = events
        .iter()
        .position(|e| e.kind == KIND_JOB_STATE && e.payload["reason"] == "completed")
        .unwrap();
    assert!(stopped > events.iter().rposition(|e| e.kind == KIND_FRAME).unwrap());
    assert_eq!(events[stopped].payload["job"], job);
    assert_eq!(events[stopped].payload["sent"], 3);
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
}

#[test]
fn lease_renew_maximum_and_offset() {
    let mut h = H::new();
    let job = h.start(
        "123#01",
        json!({"lease_ms":30,"max_duration_ms":100,"offset_ms":2}),
    );
    h.events();
    h.step(20);
    let e = h.events();
    assert_eq!(frames(&e).len(), 1);
    assert_eq!(frames(&e)[0].ts.mono_ns.get(), 12_000_000);
    let renewed = h.op(RPC_PERIODIC_RENEW, &job, json!({}));
    assert_eq!(renewed.state, WriteState::Accepted);
    assert_eq!(renewed.detail["lease_left_ms"], 30);
    h.step(30);
    let e = h.events();
    assert!(e.iter().any(|e| e.payload["state"] == "renewed"));
    assert!(e.iter().any(|e| e.payload["reason"] == "lease_expired"));
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
    let job = h.start("123#02", json!({"lease_ms":30,"max_duration_ms":40}));
    h.step(20);
    fail(h.op(RPC_PERIODIC_RENEW, &job, json!({})), ErrorCode::Lease);
}

#[test]
fn stop_latches_all_origins_one_shot_user_survives_and_no_auto_resume() {
    let mut h = H::new();
    for (origin, client, frame) in [
        (Origin::Agent, "a", "120#01"),
        (Origin::User, "u", "121#02"),
        (Origin::Script, "s", "122#03"),
    ] {
        assert_eq!(
            h.start_as(origin, client, frame, json!({})).state,
            WriteState::Accepted
        );
    }
    h.step(10);
    h.events();
    h.fw.latch(true);
    let stops = h.events();
    assert_eq!(
        stops.iter().filter(|e| e.payload["reason"] == "latched").count(),
        3
    );
    h.step(100);
    assert!(frames(&h.events()).is_empty());
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
    for origin in [Origin::Agent, Origin::User, Origin::Script] {
        fail(h.start_as(origin, "x", "123#01", json!({})), ErrorCode::Stopped);
    }
    assert_eq!(h.raw(Origin::User, 1).state, WriteState::Sent);
    assert_eq!(frames(&h.events()).len(), 1);
    h.fw.latch(false);
    h.step(100);
    assert!(frames(&h.events()).is_empty());
}

#[test]
fn atomic_sdk_latch_is_checked_even_before_on_latch_notification() {
    let mut h = H::new();
    h.start_as(Origin::User, "u", "123#01", json!({}));
    h.events();
    h.fw.ctx().latch().set(true);
    h.step(10);
    let events = h.events();
    assert!(frames(&events).is_empty());
    assert!(events.iter().any(|e| e.payload["reason"] == "latched"));
}

#[test]
fn raw_arbitration_user_override_tag_and_producer_unchanged() {
    let mut h = H::new();
    let job = h.start("123#01", json!({}));
    h.events();
    let e = fail(h.raw(Origin::Agent, 1), ErrorCode::Locked);
    assert_eq!(e.detail["job"], job);
    fail(h.raw(Origin::Script, 1), ErrorCode::Locked);
    fail(h.raw(Origin::User, 2), ErrorCode::Locked);
    let out = h.raw(Origin::User, 1);
    assert_eq!(out.state, WriteState::Sent);
    assert_eq!(out.detail["override_of"], job);
    let events = h.events();
    let fs = frames(&events);
    assert_eq!(fs.len(), 1);
    assert_eq!(fs[0].origin, Origin::User);
    assert_eq!(fs[0].payload["override_of"], job);
    assert!(fs[0].job_id.is_none());
    h.step(10);
    let events = h.events();
    assert_eq!(frames(&events)[0].payload["bytes"]["data"], "01");
    assert_eq!(frames(&events)[0].job_id.as_deref(), Some(job.as_str()));
}

#[test]
fn duplicate_takeover_and_client_ownership() {
    let mut h = H::new();
    let job = h.start("123#01", json!({"count":3}));
    h.events();
    fail(
        h.start_as(Origin::User, "u", "123#02", json!({})),
        ErrorCode::Locked,
    );
    fail(
        h.write(
            Origin::Agent,
            "other",
            json!({"method":RPC_PERIODIC_UPDATE,"job":job,"expected_rev":0,"patch":{"data":"02"}}),
        ),
        ErrorCode::Locked,
    );
    h.op(
        RPC_PERIODIC_UPDATE,
        &job,
        json!({"expected_rev":0,"patch":{"data":"FF"}}),
    );
    let out = h.write(
        Origin::User,
        "human#2",
        json!({"method":RPC_PERIODIC_TAKEOVER,"job":job}),
    );
    assert_eq!(out.state, WriteState::Accepted);
    let new = out.detail["job"].as_str().unwrap().to_owned();
    assert_ne!(new, job);
    let list = h.list();
    assert_eq!(list["jobs"][0]["owner"], "human#2");
    assert_eq!(list["jobs"][0]["origin"], "user");
    assert_eq!(list["jobs"][0]["data"], "01");
    assert_eq!(list["jobs"][0]["revision"], 0);
    h.events();
    h.step(10);
    let events = h.events();
    assert_eq!(frames(&events)[0].origin, Origin::User);
    assert_eq!(frames(&events)[0].job_id.as_deref(), Some(new.as_str()));
    fail(h.op(RPC_PERIODIC_STOP, &job, json!({})), ErrorCode::NotFound);
}

#[test]
fn isotp_reservation_and_periodic_conflict_in_both_directions() {
    let mut h = H::new();
    h.fw.call(
        RPC_ISOTP_OPEN,
        json!({"channel":"ch0","tx_id":0x123,"rx_id":0x456}),
    )
    .unwrap();
    let err = fail(
        h.start_as(Origin::User, "u", "123#01", json!({})),
        ErrorCode::Denied,
    );
    assert_eq!(err.detail["reason"], "tx_reserved");
    fail(h.raw(Origin::User, 1), ErrorCode::Denied);
    h.start("124#01", json!({}));
    let err =
        h.fw.call(
            RPC_ISOTP_OPEN,
            json!({"channel":"ch0","tx_id":0x124,"rx_id":0x457}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Locked);
    assert!(err.detail["job"].is_string());
}

#[test]
fn close_stops_jobs_and_reopen_does_not_restore_them() {
    let mut h = H::new();
    let job = h.start("123#01", json!({}));
    h.events();
    h.fw.call(CALL_CLOSE, json!({"channel":"ch0"})).unwrap();
    let events = h.events();
    assert!(
        events
            .iter()
            .any(|e| e.payload["job"] == job && e.payload["reason"] == "channel_closed")
    );
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
    h.fw.call(CALL_OPEN, json!({"channel":"ch0","nodes":"none"}))
        .unwrap();
    h.events();
    h.step(100);
    assert!(frames(&h.events()).is_empty());
}

#[test]
fn all_mine_does_not_stop_another_client_or_origin() {
    let mut h = H::new();
    h.start("120#01", json!({}));
    h.start("121#01", json!({}));
    h.start_as(Origin::User, "agent#1", "122#01", json!({}));
    h.start_as(Origin::Agent, "other", "123#01", json!({}));
    let out = h.write(
        Origin::Agent,
        "agent#1",
        json!({"method":RPC_PERIODIC_STOP,"all_mine":true}),
    );
    assert_eq!(out.detail["stopped"].as_array().unwrap().len(), 2);
    assert_eq!(h.list()["jobs"].as_array().unwrap().len(), 2);
}

#[test]
fn shared_budget_rejects_129th_across_channels_and_metadata_reclaims() {
    let mut h = H::new();
    h.fw.call(CALL_OPEN, json!({"channel":"ch1","nodes":"none"}))
        .unwrap();
    for i in 0..128 {
        h.start(
            &format!("{:03X}#01", 0x100 + i),
            json!({"channel":if i%2==0{"ch0"}else{"ch1"}}),
        );
    }
    fail(
        h.start_as(Origin::User, "u", "555#01", json!({})),
        ErrorCode::WorkerBusy,
    );
    let out = h.write(
        Origin::Agent,
        "agent#1",
        json!({"method":RPC_PERIODIC_STOP,"all_mine":true}),
    );
    assert_eq!(out.detail["stopped"].as_array().unwrap().len(), 128);
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
    assert_eq!(
        h.start_as(Origin::User, "u", "555#01", json!({})).state,
        WriteState::Accepted
    );
}

#[test]
fn skipped_ticks_list_stats_and_fd_update() {
    let mut h = H::new();
    let job = h.start("123##30102030405060708090A0B0C", json!({}));
    h.events();
    h.clock.advance_ms(45);
    h.fw.clock().advance_ns(45_000_000);
    h.fw.worker().pump_once();
    let list = h.list();
    assert_eq!(list["jobs"][0]["sent"], 1);
    assert_eq!(list["jobs"][0]["missed"], 3);
    assert_eq!(list["jobs"][0]["fd"], true);
    h.op(
        RPC_PERIODIC_UPDATE,
        &job,
        json!({"expected_rev":0,"patch":{"data":"FFFFFFFFFFFFFFFFFFFFFFFF"}}),
    );
    h.events();
    h.step(5);
    let events = h.events();
    let f = frames(&events)[0];
    assert_eq!(f.payload["bytes"]["data"], "ffffffffffffffffffffffff");
    assert_eq!(f.payload["brs"], true);
    assert_eq!(f.payload["esi"], true);
}

#[test]
fn validation_and_write_only_boundary() {
    let mut h = H::new();
    for bad in [
        json!({"period_ms":0}),
        json!({"period_ms":60001}),
        json!({"count":0}),
        json!({"lease_ms":0}),
        json!({"owner":"x".repeat(129)}),
    ] {
        fail(h.start_as(Origin::Agent, "a", "123#01", bad), ErrorCode::Usage);
    }
    fail(
        h.start_as(Origin::Device, "d", "123#01", json!({})),
        ErrorCode::Denied,
    );
    let e =
        h.fw.call(
            RPC_PERIODIC_START,
            json!({"channel":"ch0","frame":"123#01","period_ms":10}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownMethod);
    fail(
        h.start_as(Origin::Agent, "a", "123#01", json!({"channel":"closed"})),
        ErrorCode::NotOpen,
    );
    fail(
        h.start_as(Origin::Agent, "a", "123#R", json!({})),
        ErrorCode::Usage,
    );
    assert!(h.list()["jobs"].as_array().unwrap().is_empty());
}

#[test]
fn shared_bus_observer_keeps_periodic_job_annotation() {
    let mut h = H::new();
    h.fw.call(CALL_OPEN, json!({"channel":"ch1","bus":"ch0","nodes":"none"}))
        .unwrap();
    let job = h.start("123#01", json!({"count":1}));
    h.events();
    h.step(10);
    let events = h.events();
    let fs = frames(&events);
    assert_eq!(fs.len(), 2);
    for frame in fs {
        assert_eq!(frame.job_id.as_deref(), Some(job.as_str()));
        assert_eq!(frame.purpose.as_deref(), Some("periodic"));
    }
}

#[test]
fn lifecycle_publication_failure_refuses_acceptance_and_inhibits_periodic_tx() {
    use loom_proto::worker::Call;
    use loom_worker_sdk::{Emitter, LatchFlag, MemorySink, SimClock, WorkerCtx};
    use std::sync::Arc;
    let sink = Arc::new(MemorySink::new(1));
    let clock = ManualClock::new(0);
    let sdk = SimClock::new(0);
    let base = Emitter::new(
        "test",
        "can",
        "can-test",
        U64(1),
        1,
        "ch0",
        sink.clone(),
        Arc::new(sdk),
    );
    let factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync> = Arc::new(move |ch| base.for_channel(ch));
    let cx = WorkerCtx::new(LatchFlag::new(), Arc::new(SimClock::new(0)), factory);
    let mut worker = CanSimWorker::manual(clock.clone());
    let call = Call {
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
    worker.call(&cx, &call).unwrap(); // Occupy the only sink slot with channel_state.
    let write = Write {
        call_id: U64(2),
        request_id: "r".into(),
        write_class: WriteClass::Job,
        op: json!({"method":RPC_PERIODIC_START,"channel":"ch0","frame":"123#01","period_ms":10}),
        origin: Origin::Agent,
        client_id: Some("a".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(u64::MAX),
    };
    fail(worker.write(&cx, write), ErrorCode::RecordingFailed);
    clock.advance_ms(100);
    assert_eq!(worker.pump_once().periodic_frames, 0);
    let mut status = call;
    status.method = RPC_STATUS.into();
    status.args = json!({});
    let info = worker.call(&cx, &status).unwrap();
    assert_eq!(info["periodic_jobs"], 0);
    assert_eq!(info["periodic"]["inhibited"], true);
    assert!(info["periodic"]["failures"].as_u64().unwrap() > 0);
}
