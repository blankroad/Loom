//! Producer reincarnation and finite-capture loss, with real SDK sequence/acceptance semantics.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use loom_can::worker::{CALL_CLOSE, CALL_OPEN, CanSimWorker, PumpMode, WorkerLimits};
use loom_can::{KIND_FRAME, RPC_STATUS};
use loom_can_sim::ManualClock;
use loom_proto::error::ErrorCode;
use loom_proto::policy::WriteClass;
use loom_proto::{
    event::{Origin, SourceRecord, kinds},
    num::U64,
    worker::{Call, Lane, Write},
};
use loom_worker_sdk::{EmitError, EmitSink, Emitter, LatchFlag, SystemClock, WorkerCtx};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct Capture {
    records: Mutex<Vec<SourceRecord>>,
    full_after_gap: bool,
}
impl EmitSink for Capture {
    fn try_push(&self, _: Lane, r: SourceRecord) -> Result<(), EmitError> {
        let gap = r.kind == kinds::SOURCE_GAP;
        let mut records = self.records.lock().unwrap();
        assert!(records.len() < 2048, "bounded capture");
        records.push(r);
        if gap && self.full_after_gap {
            Err(EmitError::Full)
        } else {
            Ok(())
        }
    }
}
struct H {
    worker: CanSimWorker,
    cx: WorkerCtx,
    sink: Arc<Capture>,
}
impl H {
    fn new(epoch: u64, full_after_gap: bool) -> Self {
        let sink = Arc::new(Capture {
            full_after_gap,
            ..Capture::default()
        });
        let clock = Arc::new(SystemClock::new());
        let base = Emitter::new(
            "test",
            "can",
            "can_sim",
            U64(epoch),
            1,
            "ch0",
            sink.clone(),
            clock.clone(),
        );
        let cx = WorkerCtx::new(LatchFlag::new(), clock, Arc::new(move |ch| base.for_channel(ch)));
        let worker = CanSimWorker::with_options(
            ManualClock::new(0),
            PumpMode::Manual,
            WorkerLimits {
                observer_queue: 2,
                ..WorkerLimits::default()
            },
        );
        Self { worker, cx, sink }
    }
    fn call(&mut self, method: &str) -> Result<Value, loom_proto::worker::WorkerError> {
        self.worker.call(
            &self.cx,
            &Call {
                conn_id: None,
                call_id: U64(1),
                method: method.into(),
                args: if method == RPC_STATUS {
                    json!({})
                } else {
                    json!({"channel":"ch0","nodes":"none"})
                },
                origin: Origin::Agent,
                client_id: Some("agent".into()),
                actor_id: None,
                request_id: None,
                deadline_mono_ns: U64(u64::MAX),
                approval_ref: None,
            },
        )
    }
    fn finite(&mut self) {
        let out = self.worker.write(
            &self.cx,
            Write {
                call_id: U64(2),
                request_id: "finite".into(),
                write_class: WriteClass::Request,
                op: json!({"channel":"ch0","frame":"123#01","count":10}),
                origin: Origin::Agent,
                client_id: Some("agent".into()),
                plan_digest: None,
                approval_ref: None,
                deadline_mono_ns: U64(u64::MAX),
            },
        );
        assert!(out.error.is_none(), "{out:?}");
    }
}
#[test]
fn worker_reincarnation_never_reuses_an_old_channel_generation() {
    let mut old = H::new(1, false);
    assert_eq!(old.call(CALL_OPEN).unwrap()["generation"], 1);
    old.call(CALL_CLOSE).unwrap();
    let last = old.call(CALL_OPEN).unwrap()["generation"].as_u64().unwrap();
    assert_eq!(last, 2);
    // A real host increments the producer epoch when the process is recreated.
    let mut fresh = H::new(2, false);
    assert!(fresh.call(CALL_OPEN).unwrap()["generation"].as_u64().unwrap() > last);
}
#[test]
fn generation_exhaustion_is_refused_before_open_instead_of_wrapping() {
    for epoch in [0, 65_537, u64::MAX] {
        let mut h = H::new(epoch, false);
        assert_eq!(h.call(CALL_OPEN).unwrap_err().code, ErrorCode::Busy);
        assert!(
            h.call(RPC_STATUS).unwrap()["channels"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
#[test]
fn finite_capture_reports_tail_loss_without_a_following_marker() {
    let mut h = H::new(1, false);
    h.call(CALL_OPEN).unwrap();
    h.finite();
    let records = h.sink.records.lock().unwrap();
    let frames = records.iter().filter(|r| r.kind == KIND_FRAME).count() as u64;
    let gaps: u64 = records
        .iter()
        .filter(|r| r.kind == kinds::SOURCE_GAP)
        .map(|r| r.payload["count"].as_str().unwrap().parse::<u64>().unwrap())
        .sum();
    assert_eq!(frames, 2);
    assert_eq!(gaps, 8);
    assert_eq!(frames + gaps, 10);
    drop(records);
    h.worker.pump_once();
    assert_eq!(
        h.sink
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.kind == kinds::SOURCE_GAP)
            .count(),
        1
    );
    h.finite();
    let records = h.sink.records.lock().unwrap();
    let frames = records.iter().filter(|r| r.kind == KIND_FRAME).count() as u64;
    let gaps: u64 = records
        .iter()
        .filter(|r| r.kind == kinds::SOURCE_GAP)
        .map(|r| r.payload["count"].as_str().unwrap().parse::<u64>().unwrap())
        .sum();
    assert_eq!(
        frames + gaps,
        20,
        "tail must not be counted again on the next capture"
    );
}
#[test]
fn journal_accepted_full_does_not_duplicate_exact_gap_counts() {
    let mut h = H::new(1, true);
    h.call(CALL_OPEN).unwrap();
    h.finite();
    for _ in 0..3 {
        h.worker.pump_once();
    }
    assert_eq!(
        h.sink
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.kind == kinds::SOURCE_GAP)
            .count(),
        1
    );
    let status = h.call(RPC_STATUS).unwrap();
    assert_eq!(status["channels"][0]["source_gap_frames"], 8);
    assert_eq!(status["channels"][0]["emit_failed"], 1);
}
