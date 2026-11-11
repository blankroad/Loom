//! loom-psu end to end: registration, limits, output/current waits, emergency OFF, STOP latch,
//! no-response gaps, evidence holds, CLI.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_cli_kit::{DomainCli, RpcCall};
use loom_domain_api::{Coverage, CoverageNeed, EvalBudget, Instances, Predicate, Registry, Verification};
use loom_proto::envelope::{Envelope, Meta, OthersSinceLast};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, Risk, SeqRange, WriteState};
use loom_proto::policy::BlockClass;
use loom_proto::rpc::Role;
use loom_psu::cli::PsuCli;
use loom_psu::gateway::Psu;
use loom_psu::gateway::handlers::out_op;
use loom_psu::model::metric;
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe, assert_no_gap_lies, assert_write_rows};
use serde_json::{Value, json};
use support::*;

fn registry() -> Registry {
    RegistryProbe::assert_ok(&Psu::default()).0
}

/// Connected supply `bench`, output on at 13.5 V / 2 A, 2 s of samples.
fn powered() -> (Registry, Arc<Services>) {
    let reg = registry();
    let svc = Services::new();
    call(&reg, &svc, "psu.connect", json!({"ch": "bench"})).expect("connect");
    call(
        &reg,
        &svc,
        "psu.set",
        json!({"ch": "bench", "volt": 13.5, "curr": 2.0}),
    )
    .expect("set");
    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "on"})).expect("on");
    svc.driver().advance_ms(2000);
    (reg, svc)
}

#[test]
fn registration_is_complete() {
    let (reg, report) = RegistryProbe::assert_ok(&Psu::default());
    assert!(report.warnings.is_empty(), "{report}");
    assert_eq!(
        report.commands,
        [
            "psu.connect",
            "psu.status",
            "psu.meas",
            "psu.watch",
            "psu.wait_for",
            "psu.set",
            "psu.out",
            "psu.cycle",
            "psu.sim.inject"
        ]
    );
    assert_eq!(
        report.write_commands,
        ["psu.connect", "psu.set", "psu.out", "psu.cycle"]
    );
    assert_eq!(report.event_kinds, ["psu.state", "psu.meas", "psu.write"]);
    assert_eq!(reg.workers()[0].instances, Instances::PerChannel { max: 4 });
    let eo = &reg.emergency_offs()[0];
    assert_eq!(
        (eo.name.as_str(), eo.method.as_str()),
        ("psu.emergency_off", "psu.out")
    );
    let ss = &reg.safe_states()[0];
    assert_eq!(
        (ss.method.as_str(), ss.args.clone()),
        ("psu.out", json!({"state": "off"}))
    );
    let locks: Vec<&str> = reg.interlocks().map(|(n, _)| n).collect();
    assert_eq!(locks, ["psu.cycling"]);
    assert_eq!(reg.panel_manifests()[0]["id"], "psu.gauge");
    assert_eq!(
        reg.panel_manifests()[0]["actions"][0]["safetyDirection"],
        "reduce"
    );
    assert_eq!(
        reg.evidence_verifiers()[0].predicates(),
        ["psu.current_lt", "psu.current_gt", "psu.voltage_between"]
    );
    assert!(
        reg.help_ref()
            .expect("help")
            .notes
            .iter()
            .any(|n| n.contains("never clamped"))
    );
}

#[test]
fn classify_table_and_limit_block() {
    let reg = registry();
    let h = HookHarness::new(reg.hooks_ref().cloned().expect("hooks"));
    let req = |m: &str, a: Value| OwnedWriteRequest::new(m, "bench").args(a);
    for (r, risk, blocked) in [
        (req("psu.set", json!({"volt": 13.5})), Risk::R2, false),
        (req("psu.set", json!({"volt": 30.0})), Risk::R2, true),
        (req("psu.set", json!({"curr": 6.0})), Risk::R2, true),
        (req("psu.out", json!({"state": "on"})), Risk::R3, false),
        (req("psu.out", json!({"state": "off"})), Risk::R3, false),
        (req("psu.cycle", json!({"off_ms": 2000})), Risk::R3, false),
        (req("psu.connect", json!({})), Risk::R0, false),
    ] {
        let c = h.classify(&r);
        assert_eq!(c.value.risk, risk, "{:?}", r.as_req().args);
        assert_eq!(
            c.value.block == Some(BlockClass::HardDeny),
            blocked,
            "{}",
            c.value.reason
        );
        assert!(c.elapsed < loom_testkit::hooks::FAIL);
    }
}

#[test]
fn over_limit_is_refused_not_clamped() {
    let reg = registry();
    let svc = Services::new();
    call(&reg, &svc, "psu.connect", json!({"ch": "bench"})).expect("connect");
    let e = call(&reg, &svc, "psu.set", json!({"ch": "bench", "volt": 30.0})).expect_err("over vmax");
    assert_eq!(e.code, ErrorCode::Denied);
    assert_eq!(e.code.exit_code(), 4);
    assert!(
        svc.bridge.ops.lock().expect("lock").is_empty(),
        "nothing reached the supply"
    );
    // The worker refuses too (defence in depth) and records a failed row.
    let o = svc
        .driver()
        .write("rq-direct", json!({"action": "set", "volt": 16.5}), Origin::Agent);
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(o.error.map(|e| e.code), Some(ErrorCode::Denied));
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["set_v"], 0.0, "not clamped to 16 V");
    // Connect may only tighten the limits.
    let svc2 = Services::new();
    let st = call(
        &reg,
        &svc2,
        "psu.connect",
        json!({"ch": "b2", "vmax": 40.0, "imax": 1.0}),
    )
    .expect("connect");
    assert_eq!(
        (st["vmax"].clone(), st["imax"].clone()),
        (json!(16.0), json!(1.0))
    );
}

#[test]
fn output_on_raises_current_and_waits_resolve() {
    let (reg, svc) = powered();
    // Inrush then ≈ 13.5 V / 40 Ω = 0.34 A, CV.
    let m = call(&reg, &svc, "psu.meas", json!({"ch": "bench"})).expect("meas");
    let i = m["i"].as_f64().expect("current");
    assert!((0.3..0.4).contains(&i), "{m}");
    assert_eq!(m["mode"], "cv");
    let rows = rows_of(&svc, "rq-psu.out");
    assert_write_rows(&rows);
    assert_eq!(
        rows.last().map(|r| r.payload["write_state"].clone()),
        Some(json!("effective_verified"))
    );

    let ok = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_gt": 0.3, "hold_ms": 1000, "timeout_ms": 1000}),
    )
    .expect("current above 0.3 A held 1 s");
    assert!(ok["held_ms"].as_u64().expect("held") >= 1000);
    assert!(
        ok["ref"]
            .as_str()
            .is_some_and(|r| r.starts_with("psu:bench@seq:"))
    );

    let t = Instant::now();
    let e = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_lt": 0.005, "hold_ms": 1000, "timeout_ms": 300}),
    )
    .expect_err("still drawing current");
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(t.elapsed() < Duration::from_secs(2));

    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "off"})).expect("off");
    let since = svc.journal.last_seq();
    svc.driver().advance_ms(2000);
    let ok = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_lt": 0.005, "hold_ms": 1000, "timeout_ms": 1000, "since": format!("seq:{since}")}),
    )
    .expect("sleep current");
    assert_eq!(ok["matched"], true);
}

#[test]
fn user_off_during_wait_ends_it_with_exit_6() {
    let (reg, svc) = powered();
    let svc2 = svc.clone();
    let user = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        svc2.driver().write("ui-1", out_op("bench", "off"), Origin::User)
    });
    let e = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_gt": 1.0, "hold_ms": 0, "timeout_ms": 3000}),
    )
    .expect_err("interrupted");
    assert_eq!(e.code, ErrorCode::InterruptedByUser);
    assert_eq!(e.code.exit_code(), 6);
    assert_eq!(
        user.join().expect("user thread").state,
        WriteState::EffectiveVerified
    );
}

#[test]
fn emergency_off_and_rpc_are_the_same_server_action() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc); // the gateway learns `bench` from psu.state
    let eo = reg.emergency_offs()[0].clone();
    // Emergency-OFF path: the registered method with only {state:"off"} (no channel → every supply).
    call_as(&reg, &svc, Role::Viewer, &eo.method, json!({"state": "off"})).expect("emergency off");
    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "on"})).expect("on again");
    // RPC path, as the panel `!` sends it ({channel, state}).
    call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.out",
        json!({"channel": "bench", "state": "off"}),
    )
    .expect("rpc off");
    let ops = svc.bridge.ops.lock().expect("lock").clone();
    let offs: Vec<&Value> = ops
        .iter()
        .map(|(_, op)| op)
        .filter(|op| op["state"] == "off")
        .collect();
    assert_eq!(offs.len(), 2);
    assert_eq!(offs[0], offs[1]);
    assert_eq!(*offs[0], out_op("bench", "off"));
    // Safe state uses the same method and args.
    assert_eq!(reg.safe_states()[0].method, eo.method);
}

#[test]
fn stop_latch_refuses_agent_but_not_user() {
    let (reg, svc) = powered();
    svc.driver().latch(true);
    let e = call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "off"})).expect_err("latched");
    assert_eq!(e.code, ErrorCode::Stopped);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], true, "STOP keeps the output on");
    // The user's emergency OFF still works under STOP (A3).
    call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.out",
        json!({"ch": "bench", "state": "off"}),
    )
    .expect("user off");
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], false);
}

#[test]
fn interlock_blocks_agent_off_and_warns_user() {
    let reg = RegistryProbe::assert_ok(&Psu::with_interlocks(Arc::new(|n: &str| n == "uds.flashing"))).0;
    let h = HookHarness::new(reg.hooks_ref().cloned().expect("hooks"));
    let off = OwnedWriteRequest::new("psu.out", "bench").args(json!({"state": "off"}));
    assert!(
        h.presend_check(&off, None).value.is_err(),
        "agent off refused while flashing"
    );
    let cyc = OwnedWriteRequest::new("psu.cycle", "bench").args(json!({"off_ms": 1000}));
    assert!(h.presend_check(&cyc, None).value.is_err());
    let user = off.clone().origin(Origin::User);
    assert!(
        h.presend_check(&user, None).value.is_ok(),
        "emergency OFF is never blocked"
    );
    assert!(h.classify(&user).value.reason.contains("WARNING"));
    let on = OwnedWriteRequest::new("psu.out", "bench").args(json!({"state": "on"}));
    assert!(h.presend_check(&on, None).value.is_ok());
}

#[test]
fn no_response_becomes_a_gap_not_a_sample() {
    let reg = registry();
    let svc = Services::new();
    call(&reg, &svc, "psu.connect", json!({"ch": "bench"})).expect("connect");
    call(
        &reg,
        &svc,
        "psu.sim.inject",
        json!({"ch": "bench", "no_response": 2}),
    )
    .expect("inject");
    let before = svc.journal.last_seq();
    let r = svc.driver().advance_ms(1000);
    assert_eq!(
        (r["samples"].clone(), r["timeouts"].clone()),
        (json!(3), json!(2))
    );
    let evs: Vec<_> = svc
        .journal
        .all()
        .into_iter()
        .filter(|e| e.seq.get() > before)
        .collect();
    let gaps: Vec<_> = evs.iter().filter(|e| e.kind == "source.gap").collect();
    assert_eq!(gaps.len(), 2);
    assert_eq!(gaps[0].payload["cause"], "scpi_no_response");
    assert_eq!(gaps[0].channel.as_deref(), Some("bench"));
    assert_no_gap_lies(&svc.journal.all());
    // A direct query also fails honestly.
    call(
        &reg,
        &svc,
        "psu.sim.inject",
        json!({"ch": "bench", "no_response": 1}),
    )
    .expect("inject");
    let e = call(&reg, &svc, "psu.meas", json!({"ch": "bench"})).expect_err("no answer");
    assert_eq!(e.code, ErrorCode::NoResponse);
    // A slow answer stretches the poll period (one outstanding query) and shows its age.
    call(
        &reg,
        &svc,
        "psu.sim.inject",
        json!({"ch": "bench", "clear": true, "delay_ms": 500}),
    )
    .expect("delay");
    let r = svc.driver().advance_ms(2000);
    assert!(r["samples"].as_u64().expect("n") <= 5, "{r}");
    let last = svc
        .journal
        .all()
        .into_iter()
        .rev()
        .find(|e| e.kind == "psu.meas")
        .expect("sample");
    assert_eq!(last.payload["age_ms"], 500);
}

#[test]
fn cycle_turns_the_output_back_on_and_sets_the_interlock() {
    let (mut reg, svc) = powered();
    call(&reg, &svc, "psu.cycle", json!({"ch": "bench", "off_ms": 1000})).expect("cycle");
    let reducers = feed_reducer(&mut reg, &svc);
    let cycling = reg
        .interlocks()
        .find(|(n, _)| *n == "psu.cycling")
        .map(|(_, f)| f.clone())
        .expect("interlock");
    assert!(cycling(), "psu.cycling active during the off time");
    drop(reducers);
    svc.driver().advance_ms(1400);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(
        (st["out"].clone(), st["cycling"].clone()),
        (json!(true), json!(false))
    );
    let rows = rows_of(&svc, "rq-psu.cycle");
    assert_write_rows(&rows);
    assert_eq!(
        rows.last().map(|r| r.payload["write_state"].clone()),
        Some(json!("effective_verified"))
    );
}

fn pred(name: &str, args: Value) -> Predicate {
    Predicate {
        name: name.into(),
        args,
        expect_id: None,
    }
}

fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: 1.into(),
            to: 10_000.into(),
        },
        complete,
        causes: vec![],
    }
}

#[test]
fn evidence_hold() {
    let (reg, svc) = powered();
    let v = &reg.evidence_verifiers()[0];
    let on_from = svc
        .journal
        .all()
        .iter()
        .find(|e| e.kind == "psu.meas" && metric(&e.metrics, "voltage").is_some_and(|x| x > 13.0))
        .map(|e| e.seq.get())
        .expect("on");
    let evs: Vec<_> = svc
        .journal
        .all()
        .into_iter()
        .filter(|e| e.seq.get() >= on_from)
        .collect();
    let run = |p: &Predicate, e: &[loom_proto::event::PublishedRecord], c: bool| {
        v.verify(p, e, &cov(c), &mut EvalBudget::default())
    };

    let m = run(
        &pred("psu.current_gt", json!({"value": 0.3, "hold_ms": 1000})),
        &evs,
        true,
    );
    assert!(
        matches!(&m, Verification::Match { matched, .. } if matched.len() >= 6),
        "{m:?}"
    );
    let m = run(
        &pred("psu.current_lt", json!({"value": 0.005, "hold_ms": 1000})),
        &evs,
        true,
    );
    assert!(matches!(m, Verification::Mismatch { .. }), "{m:?}");
    let m = run(
        &pred("psu.voltage_between", json!({"lo": 13.4, "hi": 13.6})),
        &evs,
        true,
    );
    assert!(matches!(m, Verification::Match { .. }), "{m:?}");
    let m = run(
        &pred("psu.voltage_between", json!({"lo": 13.6, "hi": 14.0})),
        &evs,
        true,
    );
    assert!(matches!(m, Verification::Mismatch { .. }), "{m:?}");

    // Losses: a hold broken by gaps is Inconclusive, never a Mismatch.
    call(
        &reg,
        &svc,
        "psu.sim.inject",
        json!({"ch": "bench", "no_response": 1000}),
    )
    .expect("inject");
    svc.driver().advance_ms(600);
    call(
        &reg,
        &svc,
        "psu.sim.inject",
        json!({"ch": "bench", "clear": true}),
    )
    .expect("clear");
    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "off"})).expect("off");
    let mut lossy: Vec<_> = svc
        .journal
        .all()
        .into_iter()
        .filter(|e| e.seq.get() >= on_from)
        .collect();
    lossy.retain(|e| e.kind != "psu.meas" || metric(&e.metrics, "current").is_some_and(|i| i > 0.1));
    let m = run(
        &pred("psu.current_lt", json!({"value": 0.005, "hold_ms": 1000})),
        &lossy,
        true,
    );
    assert!(matches!(m, Verification::Inconclusive { .. }), "{m:?}");
    let m = run(
        &pred("psu.current_lt", json!({"value": 0.005, "hold_ms": 1000})),
        &evs,
        false,
    );
    assert!(
        matches!(m, Verification::Inconclusive { .. }),
        "incomplete coverage: {m:?}"
    );

    assert_eq!(
        v.coverage_need(&pred("psu.voltage_between", json!({}))),
        CoverageNeed::Complete
    );
    assert_eq!(
        v.coverage_need(&pred("psu.current_lt", json!({}))),
        CoverageNeed::Measure
    );
}

#[test]
fn reducer_snapshot_and_watch_summary() {
    let (mut reg, svc) = powered();
    let reducers = feed_reducer(&mut reg, &svc);
    let mut reducers = reducers;
    for e in svc.journal.all().into_iter().filter(|e| e.kind == "psu.meas") {
        reducers[0].apply(&e);
    }
    let snap = reducers[0].snapshot(&Default::default());
    assert_eq!(snap[0]["state"]["out"], true);
    assert!(snap[0]["meas_60s"]["samples"].as_u64().expect("n") >= 10);
    let w = call(
        &reg,
        &svc,
        "psu.watch",
        json!({"ch": "bench", "for_ms": 0, "since": "seq:1"}),
    )
    .expect("watch");
    assert!(w["samples"].as_u64().expect("n") >= 10);
    assert!(w["current"]["max"].as_f64().expect("max") > 0.3);
}

// --- CLI --------------------------------------------------------------------------------------

fn to_call(args: &[&str]) -> Result<RpcCall, String> {
    let m = PsuCli::command()
        .try_get_matches_from(std::iter::once("psu").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    PsuCli::to_call(&m).map_err(|e| e.message)
}

#[test]
fn cli_maps_and_renders() {
    let c = to_call(&[
        "wait-for",
        "bench",
        "--curr-lt",
        "0.005",
        "--hold",
        "5s",
        "--timeout",
        "60s",
    ])
    .expect("wait");
    assert_eq!(c.method, "psu.wait_for");
    assert_eq!(c.params["curr_lt"], 0.005);
    assert_eq!(
        (c.params["hold_ms"].clone(), c.params["timeout_ms"].clone()),
        (json!(5000), json!(60_000))
    );
    let c = to_call(&["wait-for", "bench", "--volt-between", "13,14", "--timeout", "5s"]).expect("between");
    assert_eq!(c.params["volt_between"], json!([13.0, 14.0]));
    assert_eq!(
        to_call(&["set", "bench", "--volt", "13.5"]).expect("set").params["volt"],
        13.5
    );
    assert_eq!(
        to_call(&["out", "bench", "on", "--dry-run"]).expect("out").params["dry_run"],
        true
    );
    assert_eq!(
        to_call(&["cycle", "bench", "--off", "2s"]).expect("cycle").params["off_ms"],
        2000
    );
    assert_eq!(
        to_call(&["connect", "bench", "--visa", "TCPIP::1.2.3.4::5025::SOCKET"])
            .expect("connect")
            .params["backend"],
        "visa"
    );
    for bad in [
        &["wait-for", "bench", "--timeout", "5s"][..],
        &[
            "wait-for",
            "bench",
            "--curr-lt",
            "1",
            "--curr-gt",
            "0",
            "--timeout",
            "5s",
        ],
        &["wait-for", "bench", "--curr-lt", "1", "--timeout", "2h"],
        &["set", "bench"],
        &["cycle", "bench", "--off", "2m"],
        &["watch", "bench", "--for", "20m"],
    ] {
        assert!(to_call(bad).is_err(), "{bad:?}");
    }

    let call = to_call(&["watch", "bench", "--for", "5s"]).expect("watch");
    let data = json!({"ch": "bench", "voltage": {"min": 13.49, "max": 13.51, "mean": 13.5},
                      "current": {"min": 0.312, "max": 0.388, "mean": 0.351}, "samples": 24, "expected": 25,
                      "timeouts": 1, "rate_hz": 4.9, "user_actions": [], "ref": "psu:bench@seq:10..40", "next_cursor": "seq:41"});
    let env = Envelope::success("psu.watch", data, Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    PsuCli::render_human(&call, &env, &mut out).expect("render");
    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        concat!(
            "bench  voltage min 13.490  max 13.510  mean 13.500 V\n",
            "current  min 0.312  max 0.388  mean 0.351 A   samples 24/25 (1 timeout)  rate 4.9 Hz\n",
            "evidence psu:bench@seq:10..40 · next: --since seq:41\n",
        )
    );
}

#[test]
fn connect_to_a_real_supply_is_not_implemented() {
    let reg = registry();
    let svc = Services::new();
    let e = call(
        &reg,
        &svc,
        "psu.connect",
        json!({"ch": "bench", "backend": "visa"}),
    )
    .expect_err("M1 = sim");
    assert_eq!(e.code, ErrorCode::NotImplemented);
}
