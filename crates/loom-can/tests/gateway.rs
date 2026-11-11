//! Gateway piece (G1) without a daemon: registry, hooks, queries over a fake journal, expect,
//! send, reducer and evidence verifier.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::thread;
use std::time::{Duration, Instant};

use loom_can::gateway::{Can, FIXED_MAX_CHANNELS, FIXED_MAX_IDS, PRED_FRAME_MATCH};
use loom_can::schema::FramePayload;
use loom_can::{
    KIND_CHANNEL_STATE, KIND_FRAME, KIND_TX, RPC_EXPECT, RPC_LAST, RPC_SEND, RPC_STATUS, RPC_TRACE_QUERY,
};
use loom_can_frame::parse_frame;
use loom_domain_api::{Coverage, CoverageNeed, EvalBudget, EventDraft, Predicate, Verification};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, Risk, SeqRange, kinds};
use loom_proto::num::U64;
use loom_proto::rpc::{Role, SubFilter};
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe};
use serde_json::{Value, json};

mod common;
use common::*;

// ---- registration ---------------------------------------------------------------------------------

#[test]
fn registry_probe_passes() {
    let (reg, report) = RegistryProbe::assert_ok(&Can);
    assert_eq!(
        report.commands,
        [
            RPC_SEND,
            RPC_TRACE_QUERY,
            RPC_LAST,
            RPC_EXPECT,
            RPC_STATUS,
            "can.isotp.open",
            "can.isotp.send",
            "can.isotp.cancel",
            "can.isotp.recv",
            "can.isotp.barrier",
            "can.isotp.abort",
            "can.isotp.close",
            "can.isotp.list",
            "can.periodic.start",
            "can.periodic.update",
            "can.periodic.stop",
            "can.periodic.renew",
            "can.periodic.takeover",
            "can.periodic.list",
            "can.db.load",
            "can.db.info",
            "can.sig.get",
            "can.sig.wait_for",
            "can.sig.set",
            "can.db.catalog",
            "can.encode.preview",
            "can.signal.query",
            "can.signal.at"
        ]
    );
    assert_eq!(
        report.write_commands,
        [
            RPC_SEND,
            "can.isotp.send",
            "can.isotp.cancel",
            "can.periodic.start",
            "can.periodic.update",
            "can.periodic.stop",
            "can.periodic.renew",
            "can.periodic.takeover",
            "can.sig.set"
        ]
    );
    assert_eq!(
        report.event_kinds,
        [
            KIND_FRAME,
            KIND_TX,
            KIND_CHANNEL_STATE,
            "can.stats",
            "can.isotp_tx_done",
            "can.isotp_tx_error",
            "can.isotp_rx_start",
            "can.isotp_rx_complete",
            "can.isotp_rx_error",
            "can.isotp_state",
            "can.job_state",
            "can.db_changed"
        ]
    );
    assert_eq!(report.workers, ["can_sim"]);
    assert!(report.warnings.is_empty(), "{report}");

    let frame_kind = &reg.event_kinds()[0];
    let ptrs: Vec<&str> = frame_kind.pointers.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(
        ptrs,
        [
            "/payload/id",
            "/payload/ext",
            "/payload/fd",
            "/payload/dlc",
            "/payload/len",
            "/payload/bytes/data"
        ]
    );
    assert_eq!(reg.event_kinds()[1].pointers[0].path, "/payload/write_state");

    let w = &reg.workers()[0];
    assert_eq!((w.budgets.mem_soft, w.budgets.mem_hard), (192 << 20, 256 << 20));
    assert_eq!(w.reopen_after_crash, loom_domain_api::Reopen::RxOnly);
    assert_eq!(w.platforms.len(), 3);
    assert!(reg.commands().iter().all(|c| c.default_action.is_none()), "F1");
    let verifier = &reg.evidence_verifiers()[0];
    assert_eq!(verifier.predicates(), [PRED_FRAME_MATCH, "can.signal_range"]);
    let help = reg.help_ref().unwrap();
    assert!(help.notes.iter().any(|n| n.contains("Never open the CAN device")));
    assert!(help.examples.iter().any(|e| e.cmd.contains("--expect")));
}

#[test]
fn hooks_classify_and_presend() {
    let mut env = setup();
    let h = HookHarness::new(env.reg.hooks_ref().cloned().unwrap());
    let req = OwnedWriteRequest::new(RPC_SEND, "ch0").args(json!({"channel": "ch0", "frame": "300#01"}));
    let c = h.classify(&req);
    assert_eq!((c.value.risk, c.value.reason.as_str()), (Risk::R1, "sim channel"));
    assert!(c.value.block.is_none());
    assert!(!c.slow, "{:?}", c.elapsed);
    let burst = req
        .clone()
        .args(json!({"channel": "ch0", "frame": "300#01", "count": 11}));
    assert_eq!(h.classify(&burst).value.risk, Risk::R3);
    let mut bench = req.clone();
    bench.env = Env::Bench;
    let mut vehicle = req.clone();
    vehicle.env = Env::Vehicle;
    assert_eq!(h.classify(&bench).value.risk, Risk::R2);
    assert_eq!(
        h.classify(&vehicle).value,
        h.classify(&bench).value,
        "B4: env never changes the class"
    );

    // Unknown channel: the worker decides.
    assert!(h.presend_check(&req, None).value.is_ok());
    env.apply(&channel_state("ch0", "open", 1, 1));
    assert!(h.presend_check(&req, None).value.is_ok());
    env.apply(&channel_state("ch0", "open", 2, 2));
    let stale = h.presend_check(&req, None);
    assert!(stale.value.is_err(), "generation 1 request after reopen to 2");
    assert!(!stale.slow);
    let mut fresh = req.clone();
    fresh.target.generation = Some(2);
    assert!(h.presend_check(&fresh, None).value.is_ok());
    env.apply(&channel_state("ch0", "closed", 2, 3));
    assert!(h.presend_check(&fresh, None).value.is_err());
}

// ---- trace / last -----------------------------------------------------------------------------------

fn fill(env: &Env_) {
    let j = &env.svc.journal;
    for i in 0..300u32 {
        let id = [0x300, 0x301, 0x310][i as usize % 3];
        let origin = if id == 0x300 {
            Origin::Agent
        } else {
            Origin::Device
        };
        j.push(frame(&format!("{id:03X}#{:02X}", i % 256), origin, "ch0"));
    }
    j.push(frame("123#AA", Origin::Device, "ch1"));
    let mut tx = record(
        KIND_TX,
        "ch0",
        Origin::Agent,
        Some("tx"),
        json!({"write_state": "sent", "frame": serde_json::to_value(FramePayload::from_frame(&parse_frame("300#01", false).unwrap(), None)).unwrap(), "count": 1, "sent": 1}),
    );
    tx.request_id = Some("r9".into());
    j.push(tx);
}

#[test]
fn trace_filters_pages_and_reports_activity() {
    let env = setup();
    fill(&env);
    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "ids": ["0x301"], "max": 10}),
        )
        .unwrap();
    assert_eq!(seqs(&d["rows"]), [2, 5, 8, 11, 14, 17, 20, 23, 26, 29]);
    assert_eq!(d["truncated"], json!(true));
    assert_eq!(d["next_cursor"], json!("e1:seq:32"));
    assert_eq!(d["range"], json!({"from": "2", "to": "29"}));
    // Continue from the cursor: no duplicates, no skips.
    let d2 = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "e1:seq:32", "ids": ["0x301"], "max": 2}),
        )
        .unwrap();
    assert_eq!(seqs(&d2["rows"]), [32, 35]);

    // The agent's read is visible to the user (I4) …
    let acts = env.published();
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0].kind, kinds::CORE_ACTIVITY);
    assert_eq!(acts[0].payload["action"], json!("read"));
    assert_eq!(acts[0].payload["client_id"], json!("agent-1#7"));
    assert_eq!(acts[0].payload["range"], json!({"from": "2", "to": "29"}));
    // … a viewer's read is not agent activity.
    env.call(
        RPC_TRACE_QUERY,
        Role::Viewer,
        json!({"channel": "ch0", "since": "seq:1"}),
    )
    .unwrap();
    assert!(env.published().is_empty());

    // Ranges, origins, dir; the send row belongs to tx.
    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "ids": ["0x300-0x30F"], "origins": ["agent"], "dir": "tx", "max": 1000}),
        )
        .unwrap();
    assert_eq!(d["rows"].as_array().unwrap().len(), 101); // 100 frames 0x300 + 1 can.tx row
    assert_eq!(d["truncated"], json!(false));
    assert!(
        d["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["kind"] == json!(KIND_TX))
    );
    let rx = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "dir": "rx", "max": 1000}),
        )
        .unwrap();
    // 200 rx frames do not fit 64 KiB: truncated by size, then the cursor gets the rest.
    let first = rx["rows"].as_array().unwrap().len();
    assert!(first < 200, "{first}");
    assert_eq!(rx["truncated"], json!(true));
    let rest = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": rx["next_cursor"], "dir": "rx", "max": 1000}),
        )
        .unwrap();
    assert_eq!(first + rest["rows"].as_array().unwrap().len(), 200);
    assert_eq!(rest["truncated"], json!(false));
    let (a, b) = (seqs(&rx["rows"]), seqs(&rest["rows"]));
    assert!(a.last().unwrap() < b.first().unwrap(), "no overlap between pages");
    // Default 100 rows.
    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1"}),
        )
        .unwrap();
    assert_eq!(d["rows"].as_array().unwrap().len(), 100);
    assert_eq!(d["truncated"], json!(true));
}

#[test]
fn trace_bounds_and_usage_errors() {
    let env = setup();
    for i in 0..1_200u32 {
        env.svc.journal.push(frame(
            &format!("100##1{}", "AB".repeat(64)),
            Origin::Device,
            "ch0",
        ));
        let _ = i;
    }
    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "max": 1000}),
        )
        .unwrap();
    let n = d["rows"].as_array().unwrap().len();
    assert!(
        n < 1000 && n > 50,
        "{n} rows: 64 KiB cap applies before 1000 rows"
    );
    assert!(serde_json::to_vec(&d["rows"]).unwrap().len() <= 64 * 1024);
    assert_eq!(d["truncated"], json!(true));

    for bad in [
        json!({"max": 0}),
        json!({"max": 1001}),
        json!({"ids": ["0x900-0x100"]}),
        json!({"ids": ["zz"]}),
        json!({"bogus": 1}),
    ] {
        let e = env.call(RPC_TRACE_QUERY, Role::Agent, bad.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::Usage, "{bad}");
    }
    // Gaps make the result incomplete, never silently complete.
    env.svc.journal.gaps.lock().unwrap().push(
        serde_json::from_value(json!({"count": "unknown", "cause": "crash_tail", "detail": {}})).unwrap(),
    );
    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "max": 5}),
        )
        .unwrap();
    assert_eq!(d["complete"], json!(false));
}

#[test]
fn last_is_fixed_per_id() {
    let env = setup();
    fill(&env);
    let d = env
        .call(RPC_LAST, Role::Agent, json!({"channel": "ch0"}))
        .unwrap();
    let rows = d["rows"].as_array().unwrap();
    let ids: Vec<u64> = rows.iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(ids, [0x300, 0x301, 0x310]);
    assert_eq!(rows[0]["count"], json!(100));
    assert_eq!(rows[2]["last"]["seq"], json!("300"));
    let d = env
        .call(RPC_LAST, Role::Agent, json!({"channel": "ch0", "id": "0x301"}))
        .unwrap();
    assert_eq!(d["rows"].as_array().unwrap().len(), 1);
    assert_eq!(
        env.call(RPC_LAST, Role::Agent, json!({"window_ms": 60_001}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
}

// ---- expect ------------------------------------------------------------------------------------------

#[test]
fn expect_matches_a_later_device_frame_with_evidence() {
    let env = setup();
    let j = env.svc.journal.clone();
    j.push(frame("301#01", Origin::Device, "ch0")); // before the wait: must not count
    let after = format!("seq:{}", j.next_seq());
    let jj = j.clone();
    let t = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        jj.push(frame("301#01", Origin::Agent, "ch0")); // agent frame: default origin device skips it
        jj.push(frame("301#02", Origin::Device, "ch0")); // data mismatch
        jj.push(frame("301#01", Origin::Device, "ch0")); // the match
    });
    let started = Instant::now();
    let d = env
        .call(
            RPC_EXPECT,
            Role::Agent,
            json!({"channel": "ch0", "id": "0x301", "data": "01", "after": after, "within_ms": 2000}),
        )
        .unwrap();
    t.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "returns as soon as it matches"
    );
    assert_eq!(d["matched"], json!(true));
    assert_eq!(d["frame"]["seq"], json!("4"));
    assert_eq!(d["refs"], json!(["can:ch0@e1:seq:4"]));
    let ev = &d["evidence"][0];
    assert_eq!(ev["kind"], json!("can.frame"));
    assert_eq!(ev["ref"]["seq_range"], json!({"from": "4", "to": "4"}));
    assert_eq!(ev["ref"]["source_refs"][0]["source_seq"], json!("4"));
    assert_eq!(ev["complete"], json!(true));
    // One wait activity with the predicate.
    let acts = env.published();
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0].payload["action"], json!("wait"));
    assert_eq!(acts[0].payload["waiting"]["predicate"], json!(PRED_FRAME_MATCH));
}

#[test]
fn expect_timeout_is_exit_3_with_window_evidence() {
    let env = setup();
    let j = env.svc.journal.clone();
    let jj = j.clone();
    let t = thread::spawn(move || {
        thread::sleep(Duration::from_millis(10));
        jj.push(frame("310#00", Origin::Device, "ch0"));
        jj.push(frame("310#00", Origin::Device, "ch0"));
    });
    let e = env
        .call(
            RPC_EXPECT,
            Role::Agent,
            json!({"channel": "ch0", "id": 0x301, "within_ms": 80}),
        )
        .unwrap_err();
    t.join().unwrap();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(e.code.exit_code(), 3);
    assert_eq!(e.detail["frames_scanned"], json!(2));
    assert_eq!(e.detail["evidence"][0]["kind"], json!("can.wait_timeout"));
    assert_eq!(e.detail["refs"], json!(["can:ch0@e1:seq:1..2"]));

    // Nothing on the bus: no invented window.
    let e = env
        .call(
            RPC_EXPECT,
            Role::Agent,
            json!({"channel": "ch9", "id": 0x301, "within_ms": 20}),
        )
        .unwrap_err();
    assert_eq!(e.detail["evidence"], json!([]));
    assert_eq!(e.detail["frames_scanned"], json!(0));

    for bad in [
        json!({"id": 1, "within_ms": 0}),
        json!({"id": 1, "within_ms": 60_001}),
        json!({"id": "0x1-0x2", "within_ms": 10}),
        json!({"id": 1, "data": "01", "mask": "FFFF", "within_ms": 10}),
    ] {
        assert_eq!(
            env.call(RPC_EXPECT, Role::Agent, bad.clone()).unwrap_err().code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
}

// ---- send --------------------------------------------------------------------------------------------

#[test]
fn send_dry_run_and_usage_never_reach_the_worker() {
    let env = setup();
    let d = env
        .call(
            RPC_SEND,
            Role::Agent,
            json!({"channel": "ch0", "frame": "300#01", "dry_run": true}),
        )
        .unwrap();
    assert_eq!(
        (&d["risk"], &d["reason"], &d["bytes"], &d["policy"]),
        (&json!("R1"), &json!("sim channel"), &json!("01"), &json!("auto"))
    );
    let d = env
        .call(
            RPC_SEND,
            Role::Agent,
            json!({"channel": "ch0", "frame": "300#01", "count": 11, "dry_run": true}),
        )
        .unwrap();
    assert_eq!(d["risk"], json!("R3"));
    for bad in [
        json!({"channel": "ch0", "frame": "300#0"}),
        json!({"channel": "ch0", "frame": "300#01", "count": 0}),
        json!({"channel": "ch0"}),
    ] {
        assert_eq!(
            env.call(RPC_SEND, Role::Agent, bad).unwrap_err().code,
            ErrorCode::Usage
        );
    }
    assert!(env.svc.worker.writes.lock().unwrap().is_empty());
}

#[test]
fn send_with_expect_is_effective_verified() {
    let env = setup();
    let j = env.svc.journal.clone();
    j.push(frame("301#01", Origin::Device, "ch0")); // stale response from before the send
    let jj = j.clone();
    *env.svc.worker.on_write.lock().unwrap() = Some(Box::new(move |rid, op| {
        assert_eq!(op["channel"], json!("ch0"));
        let mut echo = frame("300#01", Origin::Agent, "ch0");
        echo.request_id = Some(rid.into());
        jj.push(echo);
        let jjj = jj.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            jjj.push(frame("301#01", Origin::Device, "ch0"));
        });
    }));
    let d = env
        .call(
            RPC_SEND,
            Role::Agent,
            json!({"frame": "300#01", "expect": {"id": 0x301, "within_ms": 1000}}),
        )
        .unwrap();
    assert_eq!(d["write_state"], json!("effective_verified"));
    assert_eq!(
        d["expect"]["frame"]["seq"],
        json!("3"),
        "the stale seq 1 response is not cited"
    );
    assert_eq!(d["expect"]["refs"], json!(["can:ch0@e1:seq:3"]));
    assert_eq!(d["risk"], json!("R1"));
    let rows: Vec<EventDraft> = env
        .published()
        .into_iter()
        .filter(|p| p.kind == KIND_TX)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].payload["write_state"], json!("effective_verified"));
    assert_eq!(rows[0].request_id.as_deref(), Some("r1"));
    assert_eq!(rows[0].env, Some(Env::Sim));
}

#[test]
fn send_with_unanswered_expect_is_effect_unverified() {
    let env = setup();
    let d = env
        .call(
            RPC_SEND,
            Role::Agent,
            json!({"channel": "ch0", "frame": "300#01", "expect": {"id": 0x301, "within_ms": 30}}),
        )
        .unwrap();
    assert_eq!(d["write_state"], json!("effect_unverified"));
    assert_eq!(d["expect"]["matched"], json!(false));
    assert_eq!(env.svc.worker.writes.lock().unwrap().len(), 1);
}

#[test]
fn channel_defaults_and_ambiguity() {
    let mut env = setup();
    env.call(RPC_SEND, Role::Agent, json!({"frame": "300#01"}))
        .unwrap();
    assert_eq!(
        env.svc.worker.writes.lock().unwrap()[0].1["channel"],
        json!("ch0")
    );
    env.apply(&channel_state("chA", "open", 1, 1));
    env.call(RPC_SEND, Role::Agent, json!({"frame": "300#01"}))
        .unwrap();
    assert_eq!(
        env.svc.worker.writes.lock().unwrap()[1].1["channel"],
        json!("chA")
    );
    env.apply(&channel_state("chB", "open", 1, 2));
    let e = env
        .call(RPC_SEND, Role::Agent, json!({"frame": "300#01"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::AmbiguousChannel);
    assert_eq!(
        env.call(RPC_STATUS, Role::Agent, json!({})).unwrap()["channels"][0]["channel"],
        json!("ch0")
    );
}

// ---- reducer -------------------------------------------------------------------------------------------

#[test]
fn fixed_snapshot_is_bounded() {
    let mut env = setup();
    env.apply(&channel_state("ch0", "open", 3, 1));
    let mut seq = 2;
    for id in 0..(FIXED_MAX_IDS as u32 + 1) {
        let mut r = frame(&format!("{id:03X}#01"), Origin::Device, "ch0");
        if id > 0x7FF {
            r = frame(&format!("{id:08X}#01"), Origin::Device, "ch0");
        }
        r.seq = U64(seq);
        seq += 1;
        env.apply(&r);
    }
    let mut again = frame("005#02", Origin::Device, "ch0");
    again.seq = U64(seq);
    env.apply(&again);
    let snap = env.reducers[0].snapshot(&SubFilter::default());
    assert_eq!(snap.len(), 1);
    let s = &snap[0];
    assert_eq!(s["kind"], json!("can.fixed"));
    assert_eq!(s["state"]["generation"], json!(3));
    assert_eq!(s["ids"].as_array().unwrap().len(), FIXED_MAX_IDS);
    assert_eq!(s["ids_replaced"], json!(1));
    let five = s["ids"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == json!(5))
        .unwrap();
    assert_eq!(five["count"], json!(2));
    assert_eq!(five["last"]["payload"]["bytes"]["data"], json!("02"));

    for i in 0..FIXED_MAX_CHANNELS + 1 {
        let mut r = frame("100#01", Origin::Device, &format!("x{i}"));
        r.seq = U64(seq + 1 + i as u64);
        env.apply(&r);
    }
    let all = env.reducers[0].snapshot(&SubFilter::default());
    assert_eq!(all.len(), FIXED_MAX_CHANNELS);
    let only = env.reducers[0].snapshot(&SubFilter {
        channels: vec!["x8".into()],
        ..SubFilter::default()
    });
    assert_eq!(only.len(), 1);
}

// ---- evidence verifier -----------------------------------------------------------------------------------

fn pred(args: Value) -> Predicate {
    Predicate {
        name: PRED_FRAME_MATCH.into(),
        args,
        expect_id: None,
    }
}

fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: U64(1),
            to: U64(3),
        },
        complete,
        causes: if complete {
            vec![]
        } else {
            vec!["record_gap".into()]
        },
    }
}

#[test]
fn verifier_match_mismatch_inconclusive() {
    let (reg, _) = RegistryProbe::assert_ok(&Can);
    let v = reg.evidence_verifiers()[0].clone();
    let mut events = vec![
        frame("300#01", Origin::Agent, "ch0"),
        frame("301#02", Origin::Device, "ch0"),
        frame("301#01", Origin::Device, "ch0"),
    ];
    for (i, e) in events.iter_mut().enumerate() {
        e.seq = U64(i as u64 + 1);
    }
    let p = pred(json!({"id": "0x301", "data": "01"}));
    assert_eq!(v.coverage_need(&p), CoverageNeed::Positive);
    match v.verify(&p, &events, &cov(true), &mut EvalBudget::default()) {
        Verification::Match { matched, .. } => assert_eq!(matched[0].seq, U64(3)),
        other => panic!("{other:?}"),
    }
    // Masked match and origin filter.
    let masked = pred(json!({"id": 0x301, "data": "00", "mask": "FC", "origin": "device"}));
    assert!(matches!(
        v.verify(&masked, &events, &cov(true), &mut EvalBudget::default()),
        Verification::Match { .. }
    ));
    let none = pred(json!({"id": 0x302}));
    assert!(matches!(
        v.verify(&none, &events, &cov(true), &mut EvalBudget::default()),
        Verification::Mismatch { .. }
    ));
    // Absence over an incomplete window is not a mismatch.
    assert!(matches!(
        v.verify(&none, &events, &cov(false), &mut EvalBudget::default()),
        Verification::Inconclusive { .. }
    ));
    // A malformed predicate is an error, not "false".
    assert!(matches!(
        v.verify(
            &pred(json!({"nope": 1})),
            &events,
            &cov(true),
            &mut EvalBudget::default()
        ),
        Verification::Inconclusive { .. }
    ));
    let mut tiny = EvalBudget::new(1, Duration::from_secs(1));
    assert!(matches!(
        v.verify(&none, &events, &cov(true), &mut tiny),
        Verification::Inconclusive { reason } if reason == "eval_budget"
    ));
}

// ---- can.isotp.* -----------------------------------------------------------------------------------

#[test]
fn isotp_handlers_check_and_forward() {
    let env = setup();
    let ptrs = |kind: &str| -> Vec<String> {
        env.reg
            .event_kinds()
            .iter()
            .find(|k| k.kind == kind)
            .unwrap()
            .pointers
            .iter()
            .map(|p| p.path.clone())
            .collect()
    };
    assert_eq!(
        ptrs("can.isotp_tx_done"),
        ["/payload/endpoint", "/payload/tx_state"]
    );
    assert_eq!(ptrs("can.isotp_rx_complete")[1], "/payload/pdu/data");

    // open: the channel is filled in; bad parameters never reach the worker.
    env.call(
        "can.isotp.open",
        Role::Agent,
        json!({"tx_id": 0x7E0, "rx_id": 0x7E8}),
    )
    .unwrap();
    let calls = env.svc.worker.calls.lock().unwrap().clone();
    assert_eq!(calls[0].0, "can.isotp.open");
    assert_eq!(calls[0].1["channel"], json!("ch0"));
    for bad in [
        json!({"tx_id": 1, "rx_id": 1}),
        json!({"tx_id": 1, "rx_id": 2, "st_min": 200}),
        json!({"tx_id": 1}),
    ] {
        assert_eq!(
            env.call("can.isotp.open", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    assert_eq!(env.svc.worker.calls.lock().unwrap().len(), 1);

    // send: a worker write with the method in the op and normalized hex.
    let d = env
        .call(
            "can.isotp.send",
            Role::Agent,
            json!({"endpoint": "tp1", "pdu": "22 F1 90"}),
        )
        .unwrap();
    assert_eq!(d["request_id"], json!("r1"));
    let writes = env.svc.worker.writes.lock().unwrap().clone();
    assert_eq!(
        writes[0].1,
        json!({"method": "can.isotp.send", "endpoint": "tp1", "pdu": "22f190"})
    );
    for bad in [json!({"endpoint": "tp1", "pdu": "2"}), json!({"endpoint": "tp1"})] {
        assert_eq!(
            env.call("can.isotp.send", Role::Agent, bad).unwrap_err().code,
            ErrorCode::Usage
        );
    }
    for m in ["can.isotp.abort", "can.isotp.close", "can.isotp.list"] {
        env.call(m, Role::Agent, json!({"endpoint": "tp1"})).unwrap();
    }
    let methods: Vec<String> = env
        .svc
        .worker
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.0.clone())
        .collect();
    assert_eq!(
        methods[1..],
        ["can.isotp.abort", "can.isotp.close", "can.isotp.list"]
    );

    // classify: one PDU on the sim channel is R1.
    let h = HookHarness::new(env.reg.hooks_ref().cloned().unwrap());
    let req =
        OwnedWriteRequest::new("can.isotp.send", "ch0").args(json!({"endpoint": "tp1", "pdu": "22f190"}));
    assert_eq!(h.classify(&req).value.risk, Risk::R1);
}

// ---- can.periodic.* -------------------------------------------------------------------------------

#[test]
fn periodic_start_raw_checks_and_forwards() {
    let env = setup();
    let d = env
        .call(
            "can.periodic.start",
            Role::Agent,
            json!({"frame": "300#0102", "period_ms": 100, "offset_ms": 5, "count": 50, "lease_ms": 2000,
                   "owner": "me"}),
        )
        .unwrap();
    let (rid, op) = env.svc.worker.writes.lock().unwrap()[0].clone();
    assert_eq!(rid, "r1");
    assert_eq!(op["method"], json!("can.periodic.start"));
    assert_eq!(op["channel"], json!("ch0"));
    assert_eq!(op["frame"]["id"], json!(0x300));
    assert_eq!(op["frame"]["bytes"]["data"], json!("0102"));
    assert_eq!(
        (
            op["period_ms"].clone(),
            op["offset_ms"].clone(),
            op["count"].clone(),
            op["lease_ms"].clone()
        ),
        (json!(100), json!(5), json!(50), json!(2000))
    );
    assert_eq!(op["owner"], json!("me"));
    assert!(
        op.get("max_duration_ms").is_none(),
        "worker default (1 h) applies"
    );
    assert_eq!(
        (d["risk"].clone(), d["frame"].clone()),
        (json!("R1"), json!("300#0102"))
    );
    // A `can.frame` payload object is accepted as the frame too.
    env.call(
        "can.periodic.start",
        Role::Agent,
        json!({"frame": op["frame"], "period_ms": 10}),
    )
    .unwrap();

    for bad in [
        json!({"period_ms": 10}),
        json!({"frame": "300#01", "msg": "M", "period_ms": 10}),
        json!({"frame": "300#01"}),
        json!({"frame": "300#01", "period_ms": 0}),
        json!({"frame": "300#01", "period_ms": 60_001}),
        json!({"frame": "300#01", "period_ms": 10, "count": 0}),
        json!({"frame": "300#01", "period_ms": 10, "lease_ms": 0}),
        json!({"frame": "300#01", "period_ms": 10, "max_duration_ms": 86_400_001_u64}),
        json!({"frame": "300#01", "period_ms": 10, "signals": {"A": 1}}),
        json!({"frame": "300#R", "period_ms": 10}),
        json!({"frame": "300#0", "period_ms": 10}),
        json!({"frame": "300#01", "period_ms": 10, "bogus": 1}),
    ] {
        assert_eq!(
            env.call("can.periodic.start", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    assert_eq!(
        env.svc.worker.writes.lock().unwrap().len(),
        2,
        "bad starts never reach the worker"
    );
}

#[test]
fn periodic_job_writes_and_list() {
    let env = setup();
    let ok = [
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 2, "patch": {"data": "0102"}}),
        ),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 3, "patch": {"mask": "ff00", "bytes": "0900"}}),
        ),
        ("can.periodic.stop", json!({"job": "j1"})),
        ("can.periodic.stop", json!({"all_mine": true})),
        ("can.periodic.renew", json!({"job": "j1"})),
        ("can.periodic.takeover", json!({"job": "j1"})),
    ];
    for (m, p) in &ok {
        let d = env.call(m, Role::Agent, p.clone()).unwrap();
        assert_eq!(d["request_id"], json!("r1"));
    }
    let writes = env.svc.worker.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), ok.len());
    for ((_, op), (m, p)) in writes.iter().zip(&ok) {
        let mut want = p.clone();
        want["method"] = json!(m);
        assert_eq!(*op, want);
    }
    for (m, bad) in [
        (
            "can.periodic.update",
            json!({"job": "j1", "patch": {"data": "01"}}),
        ),
        ("can.periodic.update", json!({"job": "j1", "expected_rev": 1})),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 1, "patch": {"data": "0"}}),
        ),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 1, "patch": {"mask": "ff"}}),
        ),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 1, "patch": {"mask": "ff", "bytes": "0102"}}),
        ),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 1, "patch": {"data": "01", "mask": "ff", "bytes": "01"}}),
        ),
        ("can.periodic.stop", json!({})),
        ("can.periodic.stop", json!({"job": "j1", "all_mine": true})),
        ("can.periodic.renew", json!({})),
        ("can.periodic.takeover", json!({"job": "j1", "x": 1})),
    ] {
        assert_eq!(
            env.call(m, Role::Agent, bad.clone()).unwrap_err().code,
            ErrorCode::Usage,
            "{m} {bad}"
        );
    }
    assert_eq!(env.svc.worker.writes.lock().unwrap().len(), ok.len());

    env.call("can.periodic.list", Role::Viewer, json!({"channel": "ch0"}))
        .unwrap();
    let calls = env.svc.worker.calls.lock().unwrap().clone();
    assert_eq!(
        calls.last().unwrap(),
        &("can.periodic.list".to_owned(), json!({"channel": "ch0"}))
    );

    let job_state = env
        .reg
        .event_kinds()
        .iter()
        .find(|k| k.kind == "can.job_state")
        .unwrap();
    let ptrs: Vec<&str> = job_state.pointers.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(ptrs, ["/payload/job", "/payload/state"]);
}

#[test]
fn periodic_classify_by_method() {
    let env = setup();
    let h = HookHarness::new(env.reg.hooks_ref().cloned().unwrap());
    let req = |m: &str, args: Value| OwnedWriteRequest::new(m, "ch0").args(args);
    // A long periodic job is not a "burst".
    let start = req(
        "can.periodic.start",
        json!({"frame": "300#01", "period_ms": 10, "count": 500}),
    );
    assert_eq!(h.classify(&start).value.risk, Risk::R1);
    assert_eq!(
        h.classify(&req("can.periodic.stop", json!({"job": "j1"})))
            .value
            .risk,
        Risk::R0
    );
    let mut bench = start.clone();
    bench.env = Env::Bench;
    let c = h.classify(&bench).value;
    assert_eq!((c.risk, c.reason.as_str()), (Risk::R3, "periodic"));
    let mut vehicle = req("can.sig.set", json!({"signal": "A.B", "value": 1}));
    vehicle.env = Env::Vehicle;
    assert_eq!(h.classify(&vehicle).value.risk, Risk::R3);
    // can.send keeps its own rules.
    let burst = req(
        "can.send",
        json!({"channel": "ch0", "frame": "300#01", "count": 11}),
    );
    assert_eq!(h.classify(&burst).value.risk, Risk::R3);
}

#[test]
fn ui_queries_are_read_only() {
    let (reg, _) = RegistryProbe::assert_ok(&Can);
    for m in [
        "can.db.catalog",
        "can.encode.preview",
        "can.signal.query",
        "can.signal.at",
    ] {
        let c = reg.commands().iter().find(|c| c.method == m).unwrap();
        assert!(c.write_class.is_none(), "{m} is R0");
    }
}

// ---- can.isotp.cancel / recv / barrier --------------------------------------------------------

/// Worker answering `method` calls with `replies[i]` (the last one repeats).
fn scripted_calls(
    env: &Env_,
    replies: Vec<Result<Value, ErrorCode>>,
) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = std::sync::Arc::new(AtomicUsize::new(0));
    let count = n.clone();
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(move |_, _| {
        let i = count.fetch_add(1, Ordering::SeqCst).min(replies.len() - 1);
        replies[i]
            .clone()
            .map_err(|c| loom_domain_api::DomainError::new(c, "worker says no"))
    }));
    n
}

#[test]
fn isotp_cancel_is_an_r0_worker_write() {
    let env = setup();
    let d = env
        .call(
            "can.isotp.cancel",
            Role::Agent,
            json!({"endpoint": "tp1", "transfer": "0.3"}),
        )
        .unwrap();
    assert_eq!(d["request_id"], json!("r1"));
    let (rid, op) = env.svc.worker.writes.lock().unwrap()[0].clone();
    assert_eq!(rid, "r1");
    assert_eq!(
        op,
        json!({"method": "can.isotp.cancel", "endpoint": "tp1", "transfer": "0.3"})
    );
    assert!(
        env.svc.worker.calls.lock().unwrap().is_empty(),
        "a write, never a call"
    );
    for bad in [
        json!({"endpoint": "tp1"}),
        json!({"transfer": "0.3"}),
        json!({"endpoint": "tp1", "transfer": "x".repeat(65)}),
        json!({"endpoint": "tp1", "transfer": "0.3", "force": true}),
    ] {
        assert_eq!(
            env.call("can.isotp.cancel", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    // Stop control: R0 on every env (never treated as a new send).
    let h = HookHarness::new(env.reg.hooks_ref().cloned().unwrap());
    let mut req =
        OwnedWriteRequest::new("can.isotp.cancel", "ch0").args(json!({"endpoint": "tp1", "transfer": "0.3"}));
    assert_eq!(h.classify(&req).value.risk, Risk::R0);
    req.env = Env::Vehicle;
    assert_eq!(h.classify(&req).value.risk, Risk::R0);
    assert!(h.presend_check(&req, None).value.is_ok());
    let cmd = env
        .reg
        .commands()
        .iter()
        .find(|c| c.method == "can.isotp.cancel")
        .unwrap();
    assert_eq!(cmd.write_class, Some(loom_proto::policy::WriteClass::Request));
}

#[test]
fn isotp_recv_polls_until_a_pdu_or_timeout() {
    let env = setup();
    let empty = json!({"endpoint": "tp1", "epoch": 1, "pdus": [], "next_pdu_seq": "4", "latest_pdu_seq": "4",
                       "oldest_pdu_seq": "1", "dropped": "0", "cursor_gap": false});
    let mut one = empty.clone();
    one["pdus"] = json!([{"pdu_seq": "5", "pdu": {"encoding": "hex", "data": "62f190"}, "len": 3}]);
    one["next_pdu_seq"] = json!("5");
    let calls = scripted_calls(&env, vec![Ok(empty.clone()), Ok(empty.clone()), Ok(one)]);
    let args = json!({"endpoint": "tp1", "after_pdu_seq": "4", "timeout_ms": 2000, "max_pdus": 4});
    let started = Instant::now();
    let d = env.call("can.isotp.recv", Role::Agent, args.clone()).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "returns at the first PDU"
    );
    assert_eq!(d["pdus"][0]["pdu_seq"], json!("5"));
    assert_eq!(
        (d["polls"].clone(), d["timed_out"].clone()),
        (json!(3), json!(false))
    );
    let seen = env.svc.worker.calls.lock().unwrap().clone();
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter().all(|(m, a)| m == "can.isotp.recv" && *a == args),
        "same cursor every time"
    );
    drop(calls);

    // timeout_ms 0 (default): exactly one look.
    let env = setup();
    scripted_calls(&env, vec![Ok(empty.clone())]);
    let d = env
        .call("can.isotp.recv", Role::Agent, json!({"endpoint": "tp1"}))
        .unwrap();
    assert_eq!(
        (d["polls"].clone(), d["timed_out"].clone()),
        (json!(1), json!(true))
    );

    // Nothing arrives: the last snapshot after the timeout.
    let env = setup();
    scripted_calls(&env, vec![Ok(empty.clone())]);
    let d = env
        .call(
            "can.isotp.recv",
            Role::Agent,
            json!({"endpoint": "tp1", "timeout_ms": 40}),
        )
        .unwrap();
    assert_eq!(
        (d["pdus"].clone(), d["timed_out"].clone()),
        (json!([]), json!(true))
    );
    assert!(d["polls"].as_u64().unwrap() >= 2);
    assert!(d["waited_ms"].as_u64().unwrap() >= 40);

    // Worker errors end the wait at once.
    let env = setup();
    scripted_calls(&env, vec![Err(ErrorCode::NotOpen)]);
    let e = env
        .call(
            "can.isotp.recv",
            Role::Agent,
            json!({"endpoint": "tp9", "timeout_ms": 5000}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotOpen);
    assert_eq!(env.svc.worker.calls.lock().unwrap().len(), 1);

    for bad in [
        json!({}),
        json!({"endpoint": "tp1", "timeout_ms": 60_001}),
        json!({"endpoint": "tp1", "max_pdus": 0}),
        json!({"endpoint": "tp1", "max_pdus": 17}),
        json!({"endpoint": "tp1", "after_pdu_seq": "-1"}),
        json!({"endpoint": "tp1", "after_pdu_seq": "1".repeat(21)}),
        json!({"endpoint": "tp1", "after_pdu_seq": "99999999999999999999"}),
        json!({"endpoint": "tp1", "after_pdu_seq": -1}),
        json!({"endpoint": "tp1", "epoch": 1}),
    ] {
        assert_eq!(
            env.call("can.isotp.recv", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
}

#[test]
fn isotp_barrier_polls_until_complete_or_timeout() {
    let env = setup();
    let snap = |complete: bool| {
        json!({"endpoint": "tp1", "epoch": "3", "covered_ingest_seq": "41", "transport_event_seq": "17",
               "covered_mono_ns": "0", "complete": complete})
    };
    scripted_calls(&env, vec![Ok(snap(false)), Ok(snap(false)), Ok(snap(true))]);
    let args = json!({"endpoint": "tp1", "epoch": "3", "deadline_mono_ns": "123456789", "timeout_ms": 2000});
    let d = env.call("can.isotp.barrier", Role::Agent, args.clone()).unwrap();
    assert_eq!(
        (d["complete"].clone(), d["polls"].clone()),
        (json!(true), json!(3))
    );
    assert!(
        env.svc
            .worker
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(m, a)| m == "can.isotp.barrier" && *a == args)
    );

    // Never complete: the last `complete:false` comes back unchanged (UDS: "uncertain").
    let env = setup();
    scripted_calls(&env, vec![Ok(snap(false))]);
    let d = env
        .call(
            "can.isotp.barrier",
            Role::Agent,
            json!({"endpoint": "tp1", "epoch": 3, "deadline_mono_ns": 5, "timeout_ms": 30}),
        )
        .unwrap();
    assert_eq!(
        (d["complete"].clone(), d["timed_out"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(d["covered_ingest_seq"], json!("41"));

    // Epoch changed: the worker's error as is.
    let env = setup();
    scripted_calls(&env, vec![Err(ErrorCode::CursorGap)]);
    let e = env
        .call(
            "can.isotp.barrier",
            Role::Agent,
            json!({"endpoint": "tp1", "epoch": 2, "deadline_mono_ns": 5}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::CursorGap);

    for bad in [
        json!({"endpoint": "tp1", "deadline_mono_ns": 5}),
        json!({"endpoint": "tp1", "epoch": 1}),
        json!({"endpoint": "tp1", "epoch": "x", "deadline_mono_ns": 5}),
        json!({"endpoint": "tp1", "epoch": 1, "deadline_mono_ns": 5, "timeout_ms": 60_001}),
        json!({"endpoint": "tp1", "epoch": 1, "deadline_mono_ns": 5, "after_pdu_seq": 1}),
    ] {
        assert_eq!(
            env.call("can.isotp.barrier", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    let reads: Vec<_> = env
        .reg
        .commands()
        .iter()
        .filter(|c| c.method == "can.isotp.recv" || c.method == "can.isotp.barrier")
        .map(|c| (c.write_class, c.bounds.max_wait >= Duration::from_secs(60)))
        .collect();
    assert_eq!(
        reads,
        [(None, true), (None, true)],
        "R0 reads; wait covers 60 s + IPC"
    );
}

#[test]
fn isotp_event_pointers_for_uds() {
    let env = setup();
    let kind = |k: &str| {
        env.reg
            .event_kinds()
            .iter()
            .find(|e| e.kind == k)
            .unwrap()
            .clone()
    };
    let rx: Vec<String> = kind("can.isotp_rx_complete")
        .pointers
        .iter()
        .map(|p| p.path.clone())
        .collect();
    assert!(rx.contains(&"/payload/pdu_seq".to_owned()) && rx.contains(&"/payload/ts_last_frame".to_owned()));
    let code = kind("can.isotp_tx_error")
        .pointers
        .into_iter()
        .find(|p| p.path == "/payload/code")
        .unwrap();
    match code.ty {
        loom_domain_api::ValueType::Enum(v) => {
            assert!(v.contains(&"CANCELLED".to_owned()) && v.contains(&"N_BS_TIMEOUT".to_owned()))
        }
        other => panic!("{other:?}"),
    }
}
