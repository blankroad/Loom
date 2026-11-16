//! loom-psu end to end: registration, limits, output/current waits, emergency OFF, STOP latch,
//! no-response gaps, evidence holds, CLI.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_cli_kit::{DomainCli, RpcCall};
use loom_domain_api::{
    Coverage, CoverageNeed, EvalBudget, Instances, Predicate, Registry, RoleSet, Verification,
};
use loom_proto::envelope::{Envelope, Meta, OthersSinceLast};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, Risk, SeqRange, WriteState};
use loom_proto::policy::BlockClass;
use loom_proto::rpc::Role;
use loom_psu::cli::PsuCli;
use loom_psu::gateway::Psu;
use loom_psu::gateway::common::{Anchor, Seek, TimeIndex};
use loom_psu::gateway::handlers::out_op;
use loom_psu::gateway::history::{HistStat, Stop, Window, history_page, history_window};
use loom_psu::model::{Limits, config_mismatch, metric};
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
            "psu.disconnect",
            "psu.status",
            "psu.meas",
            "psu.watch",
            "psu.history",
            "psu.wait_for",
            "psu.set",
            "psu.out",
            "psu.off",
            "psu.cycle",
            "psu.sim.inject"
        ]
    );
    assert_eq!(
        report.write_commands,
        ["psu.connect", "psu.set", "psu.out", "psu.off", "psu.cycle"]
    );
    // B5 R5-OFF: the fixed-OFF endpoint is callable by watch viewers; psu.out stays operate-only.
    let roles = |m: &str| {
        reg.commands()
            .iter()
            .find(|c| c.method == m)
            .map(|c| c.roles)
            .expect(m)
    };
    assert_eq!(roles("psu.off"), RoleSet::READ);
    assert_eq!(roles("psu.out"), RoleSet::WRITE);
    assert_eq!(reg.dry_run_preview_roles("psu.off"), Some(RoleSet::READ));
    let off_spec = reg
        .commands()
        .iter()
        .find(|c| c.method == "psu.off")
        .expect("psu.off");
    assert_eq!(
        off_spec.write_class,
        Some(loom_proto::policy::WriteClass::Request)
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
        reg.panel_manifests()[0]["actions"][0]["rpc"],
        "psu.off",
        "B5: watch emergency OFF uses the fixed-OFF endpoint"
    );
    assert_eq!(reg.panel_manifests()[0]["actions"][0]["right"], "watch");
    // UI parity (ui/src/panels/psu/gaugePanel.ts): explicit connect/disconnect, no implicit channel
    let m = &reg.panel_manifests()[0];
    let ids: Vec<&str> = m["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .filter_map(|a| a["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        ["emergencyOff", "output", "set", "cycle", "connect", "disconnect"]
    );
    assert_eq!(m["actions"][3]["rpc"], "psu.cycle", "PSU-04 panel action");
    assert_eq!(m["actions"][3]["right"], "operate");
    assert_eq!(m["actions"][4]["rpc"], "psu.connect");
    assert_eq!(m["actions"][5]["rpc"], "psu.disconnect");
    assert!(
        m["queries"]
            .as_array()
            .expect("queries")
            .iter()
            .any(|q| q == "psu.history"),
        "PSU-03 query declared"
    );
    assert!(m["topics"][0]["filter"].get("defaultChannel").is_none());
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
    // Connect may only tighten the limits; asking for more is refused (B5 F2), not silently capped.
    let svc2 = Services::new();
    let e = call(
        &reg,
        &svc2,
        "psu.connect",
        json!({"ch": "b2", "vmax": 40.0, "imax": 1.0}),
    )
    .expect_err("above profile");
    assert_eq!(e.code, ErrorCode::Usage, "{e:?}");
    let st = call(
        &reg,
        &svc2,
        "psu.connect",
        json!({"ch": "b2", "vmax": 12.0, "imax": 1.0}),
    )
    .expect("tighter");
    assert_eq!(
        (st["vmax"].clone(), st["imax"].clone()),
        (json!(12.0), json!(1.0))
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
fn connected_output_off_event_and_reducer_snapshot() {
    let mut reg = registry();
    let svc = Services::new();
    let connected = call(&reg, &svc, "psu.connect", json!({"ch": "bench"})).expect("connect");
    assert_eq!(connected["connected"], true);
    assert_eq!(connected["out"], false);

    let states: Vec<_> = svc
        .journal
        .all()
        .into_iter()
        .filter(|e| e.kind == "psu.state")
        .collect();
    assert_eq!(states.len(), 1, "connect emits an initial state with output OFF");
    assert_eq!(states[0].channel.as_deref(), Some("bench"));
    assert_eq!(states[0].payload["connected"], true);
    assert_eq!(states[0].payload["out"], false);

    let reducers = feed_reducer(&mut reg, &svc);
    let snap = reducers[0].snapshot(&Default::default());
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0]["state"]["connected"], true);
    assert_eq!(snap[0]["state"]["out"], false);
    let status = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(status["connected"], snap[0]["state"]["connected"]);
    assert_eq!(status["out"], false);

    // Older journal states omit connected: replay must preserve that unknown state.
    let mut legacy = states[0].clone();
    legacy
        .payload
        .as_object_mut()
        .expect("state object")
        .remove("connected");
    let mut legacy_reg = registry();
    let spec = legacy_reg
        .event_kinds()
        .iter()
        .find(|s| s.kind == "psu.state")
        .expect("state spec");
    let schema = serde_json::to_value(&spec.payload_schema).expect("schema");
    assert!(
        !schema["required"]
            .as_array()
            .expect("required")
            .contains(&json!("connected"))
    );
    assert!(
        spec.pointers
            .iter()
            .any(|p| { p.path == "/payload/connected" && p.ty == loom_domain_api::ValueType::Bool })
    );
    let mut legacy_reducers = legacy_reg.take_reducers();
    legacy_reducers[0].apply(&legacy);
    let legacy_snap = legacy_reducers[0].snapshot(&Default::default());
    assert_eq!(legacy_snap[0]["state"], legacy.payload);
    assert!(legacy_snap[0]["state"].get("connected").is_none());
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

// ---------------------------------------------------------------------------------------------
// Operator upgrade batch 2 (work/reports/operator-upgrade/psu/FUNCTIONS.md §Batch 2). Spec tests
// authored by the lead; implementation must not weaken them.

/// R2: every worker access goes through `worker_for(<resolved ch>)` — never the call's own
/// (possibly absent) channel, which the daemon resolves to "any running instance".
#[test]
fn b2_handlers_route_every_worker_access_by_resolved_channel() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc); // the book knows `bench`
    svc.routed.lock().expect("lock").clear();
    call(&reg, &svc, "psu.status", json!({})).expect("status (only supply)");
    call(&reg, &svc, "psu.meas", json!({"ch": "bench"})).expect("meas");
    call(&reg, &svc, "psu.set", json!({"ch": "bench", "volt": 12.0})).expect("set");
    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "on"})).expect("on");
    call(&reg, &svc, "psu.cycle", json!({"ch": "bench", "off_ms": 500})).expect("cycle");
    call(&reg, &svc, "psu.connect", json!({})).expect("connect (default alias bench, already open)");
    call(&reg, &svc, "psu.disconnect", json!({"ch": "bench"})).expect("disconnect");
    assert_eq!(*svc.routed.lock().expect("lock"), vec!["bench"; 7]);
}

/// R2: the declared emergency OFF sends `psu.out{targets:["*"]}` (EmergencyOffSpec): every known
/// supply, one request id per supply, user origin.
#[test]
fn b2_emergency_targets_switch_every_known_supply_off() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc);
    svc.routed.lock().expect("lock").clear();
    let r = call_as(&reg, &svc, Role::Viewer, "psu.out", json!({"targets": ["*"]})).expect("emergency");
    let res = r["results"].as_array().expect("results");
    assert_eq!(res.len(), 1, "{r}");
    assert_eq!(
        (res[0]["ch"].clone(), res[0]["ok"].clone()),
        (json!("bench"), json!(true)),
        "{r}"
    );
    assert_eq!(*svc.routed.lock().expect("lock"), vec!["bench"]);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], false);
    assert!(
        !rows_of(&svc, "rq-psu.out/bench").is_empty(),
        "per-supply request id"
    );
}

#[test]
fn b2_out_targets_parameter_rules_and_single_channel_shape_unchanged() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc);
    let code = |p: Value| {
        call_as(&reg, &svc, Role::Viewer, "psu.out", p)
            .expect_err("usage")
            .code
    };
    assert_eq!(
        code(json!({"targets": ["*"], "state": "on"})),
        ErrorCode::Usage,
        "targets only switch off"
    );
    assert_eq!(
        code(json!({"targets": ["*"], "ch": "bench"})),
        ErrorCode::Usage,
        "ch or targets, not both"
    );
    assert_eq!(
        code(json!({"targets": "*"})),
        ErrorCode::Usage,
        "targets is a list"
    );
    assert_eq!(code(json!({"targets": [""]})), ErrorCode::Usage);
    assert_eq!(code(json!({"targets": []})), ErrorCode::Usage);
    assert_eq!(code(json!({})), ErrorCode::Usage, "no state, no targets");
    let r = call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.out",
        json!({"targets": ["bench"], "state": "off"}),
    )
    .expect("named");
    assert_eq!(r["results"][0]["ch"], "bench", "{r}");
    let r = call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "on"})).expect("on");
    assert!(
        r.get("results").is_none(),
        "single-channel result shape unchanged: {r}"
    );
    assert_eq!(r["state"], "effective_verified", "{r}");
}

#[test]
fn b2_emergency_with_no_supply_is_an_explicit_empty_result() {
    let reg = registry();
    let svc = Services::new();
    let r = call_as(&reg, &svc, Role::Viewer, "psu.out", json!({"targets": ["*"]})).expect("empty");
    assert_eq!(r, json!({"results": [], "note": "no supply connected"}));
}

/// A partial all-off is an error naming the unconfirmed supply — never an overall success.
#[test]
fn b2_partial_all_off_is_an_error_naming_the_unconfirmed_supply() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc);
    // `ghost` routes to the same (only) test worker, which drives `bench` → refused (foreign ch)
    let e = call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.out",
        json!({"targets": ["bench", "ghost"]}),
    )
    .expect_err("partial");
    assert_eq!(e.code, ErrorCode::Partial, "{e:?}");
    assert!(e.message.contains("ghost"), "{}", e.message);
    let res = e.detail["results"].as_array().expect("detail.results");
    let oks: Vec<(Value, Value)> = res.iter().map(|r| (r["ch"].clone(), r["ok"].clone())).collect();
    assert_eq!(
        oks,
        vec![(json!("bench"), json!(true)), (json!("ghost"), json!(false))]
    );
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], false, "the reachable supply is still switched off");
}

/// R2 defence in depth: a worker process never applies a call/write meant for another alias.
#[test]
fn b2_worker_refuses_a_foreign_channel_and_never_touches_its_supply() {
    let (_reg, svc) = powered();
    let mut d = svc.driver();
    let o = d.write(
        "w-foreign",
        json!({"action": "out", "ch": "other", "state": "off"}),
        Origin::User,
    );
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::NotOpen));
    let o = d.write(
        "w-foreign2",
        json!({"action": "set", "channel": "other", "volt": 1.0}),
        Origin::Agent,
    );
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::NotOpen));
    let e = d
        .call("psu.status", json!({"ch": "other"}))
        .expect_err("foreign status");
    assert_eq!(e.code, ErrorCode::NotOpen);
    let st = d.call("psu.status", json!({"ch": "bench"})).expect("own status");
    assert_eq!(
        (st["out"].clone(), st["set_v"].clone()),
        (json!(true), json!(13.5)),
        "supply untouched"
    );
    assert!(
        d.call("psu.status", json!({})).is_ok(),
        "no ch → this worker's supply"
    );
}

/// COMMON PSU edge case: once OFF, STOP latched for an agent cycle means ON is never sent.
#[test]
fn b2_sim_stop_during_off_phase_never_switches_on_for_agent() {
    let (reg, svc) = powered();
    call(&reg, &svc, "psu.cycle", json!({"ch": "bench", "off_ms": 1000})).expect("cycle");
    svc.driver().latch(true);
    svc.driver().advance_ms(1400);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(
        (st["out"].clone(), st["cycling"].clone()),
        (json!(false), json!(false))
    );
    let rows = rows_of(&svc, "rq-psu.cycle");
    assert_write_rows(&rows);
    let last = rows.last().expect("terminal row");
    assert_eq!(last.payload["write_state"], "failed");
    assert_eq!(last.payload["phase"], "on");
    assert_eq!(last.payload["sent"], false);
    assert_eq!(last.payload["error"]["code"], "E_STOPPED");
}

#[test]
fn b2_sim_user_cycle_is_not_latched_and_reports_the_measured_off_time() {
    let (reg, svc) = powered();
    svc.driver().latch(true);
    call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.cycle",
        json!({"ch": "bench", "off_ms": 1000}),
    )
    .expect("cycle");
    svc.driver().advance_ms(1400);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], true);
    let rows = rows_of(&svc, "rq-psu.cycle");
    let last = rows.last().expect("terminal row");
    assert_eq!(last.payload["write_state"], "effective_verified");
    let actual = last.payload["off_ms_actual"].as_u64().expect("off_ms_actual");
    assert!((1000..=1400).contains(&actual), "{actual}");
}

#[test]
fn b2_sim_second_cycle_supersedes_the_first() {
    let (_reg, svc) = powered();
    {
        let mut d = svc.driver();
        assert_eq!(
            d.write(
                "c1",
                json!({"action": "cycle", "ch": "bench", "off_ms": 1000}),
                Origin::Agent
            )
            .state,
            WriteState::Sent
        );
        assert_eq!(
            d.write(
                "c2",
                json!({"action": "cycle", "ch": "bench", "off_ms": 1000}),
                Origin::Agent
            )
            .state,
            WriteState::Sent
        );
        d.advance_ms(1400);
    }
    let c1 = rows_of(&svc, "c1");
    let last = c1.last().expect("c1 terminal");
    assert_eq!(last.payload["write_state"], "effect_unverified");
    assert_eq!(last.payload["superseded_by"], "c2");
    let c2 = rows_of(&svc, "c2");
    assert_eq!(
        c2.last().expect("c2 terminal").payload["write_state"],
        "effective_verified"
    );
}

// ---------------------------------------------------------------------------------------------
// Operator upgrade batch 3 — `psu.history` (FUNCTIONS.md §Batch 3). Spec tests.

/// 60 s of 5 Hz samples on `bench` (2 unanswered polls → 2 source.gap), reducer fed (anchors).
fn hist_setup() -> (Registry, Arc<Services>, Vec<loom_proto::event::PublishedRecord>) {
    let (mut reg, svc) = powered();
    svc.driver()
        .call("psu.sim.inject", json!({"ch": "bench", "no_response": 2}))
        .expect("inject");
    for _ in 0..58 {
        svc.driver().advance_ms(1000);
    }
    let _reducers = feed_reducer(&mut reg, &svc);
    let all = svc.journal.all();
    (reg, svc, all)
}

fn wall_ns(e: &loom_proto::event::PublishedRecord) -> u64 {
    e.ts.wall_ns.get()
}

fn meas_of(all: &[loom_proto::event::PublishedRecord]) -> Vec<&loom_proto::event::PublishedRecord> {
    all.iter().filter(|e| e.kind == "psu.meas").collect()
}

#[test]
fn b3_history_whole_window_matches_the_journal() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    assert!(m.len() > 250, "{}", m.len());
    let from_ms = wall_ns(m[0]) / 1_000_000;
    let to_ms = wall_ns(m[m.len() - 1]) / 1_000_000 + 1;
    let r = call(
        &reg,
        &svc,
        "psu.history",
        json!({"ch": "bench", "from_ms": from_ms, "to_ms": to_ms}),
    )
    .expect("history");
    let inside: Vec<_> = m
        .iter()
        .filter(|e| wall_ns(e) >= from_ms * 1_000_000 && wall_ns(e) < to_ms * 1_000_000)
        .collect();
    assert_eq!(r["samples"], inside.len() as u64, "{r}");
    let vs: Vec<f64> = inside
        .iter()
        .map(|e| metric(&e.metrics, "voltage").expect("v"))
        .collect();
    let min = vs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = vs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert_eq!(r["stats"]["voltage"]["n"], inside.len() as u64);
    assert_eq!(r["stats"]["voltage"]["min"].as_f64(), Some(min));
    assert_eq!(r["stats"]["voltage"]["max"].as_f64(), Some(max));
    assert_eq!(r["stats"]["voltage"]["last"].as_f64(), vs.last().copied());
    let gaps = all
        .iter()
        .filter(|e| {
            e.kind == "source.gap" && wall_ns(e) >= from_ms * 1_000_000 && wall_ns(e) < to_ms * 1_000_000
        })
        .count();
    assert_eq!(gaps, 2, "fixture: two unanswered polls inside the window");
    assert_eq!(r["losses"], 2, "{r}");
    assert_eq!(r["first"]["seq"], inside[0].seq.get());
    assert_eq!(r["last"]["seq"], inside[inside.len() - 1].seq.get());
    assert_eq!(r["range"], json!({"from_ms": from_ms, "to_ms": to_ms}));
    assert!(r["now_ms"].is_u64(), "server time of the query is reported: {r}");
    assert_eq!(r["next_cursor"], Value::Null, "{r}");
    assert_eq!(r["before_index"], false);
    assert_eq!(r["complete"], true, "{r}");
    assert!(r.get("rows").is_none(), "no rows unless asked");
}

#[test]
fn b3_history_pages_by_rows_without_loss_or_duplicate() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let from_ms = wall_ns(m[0]) / 1_000_000;
    let to_ms = wall_ns(m[m.len() - 1]) / 1_000_000 + 1;
    let mut cursor: Option<String> = None;
    let mut seqs: Vec<u64> = Vec::new();
    let mut n = 0u64;
    let mut pages = 0;
    loop {
        let mut p = json!({"ch": "bench", "from_ms": from_ms, "to_ms": to_ms, "rows": 7});
        if let Some(c) = &cursor {
            p["cursor"] = json!(c);
        }
        let r = call(&reg, &svc, "psu.history", p).expect("page");
        pages += 1;
        assert!(pages < 200, "paging must terminate");
        let rows = r["rows"].as_array().expect("rows");
        assert!(rows.len() <= 7);
        assert_eq!(
            r["stats"]["voltage"]["n"].as_u64().unwrap_or(0),
            rows.len() as u64,
            "page stats cover exactly its rows"
        );
        n += r["samples"].as_u64().expect("samples");
        seqs.extend(rows.iter().map(|x| x["seq"].as_u64().expect("seq")));
        assert!(
            rows.iter()
                .all(|x| x["wall_ms"].is_u64() && x["v"].is_f64() && x["i"].is_f64() && x["p"].is_f64()),
            "{r}"
        );
        match r["next_cursor"].as_str() {
            Some(c) => {
                assert_eq!(rows.len(), 7, "only a full page continues");
                cursor = Some(c.to_owned());
            }
            None => break,
        }
    }
    let want: Vec<u64> = m.iter().map(|e| e.seq.get()).collect();
    assert_eq!(seqs, want, "every sample exactly once, in order");
    assert_eq!(n, want.len() as u64);
}

#[test]
fn b3_history_start_older_than_the_index_is_reported_not_complete() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let from_ms = wall_ns(m[0]) / 1_000_000 - 3_600_000;
    let to_ms = wall_ns(m[m.len() - 1]) / 1_000_000 + 1;
    let r = call(
        &reg,
        &svc,
        "psu.history",
        json!({"ch": "bench", "from_ms": from_ms, "to_ms": to_ms}),
    )
    .expect("history");
    assert_eq!(r["before_index"], true, "{r}");
    assert_eq!(r["complete"], false, "{r}");
    assert_eq!(r["samples"], m.len() as u64, "{r}");
    assert!(r["index"]["oldest_ms"].as_u64().is_some(), "{r}");
}

#[test]
fn b3_history_is_a_registered_read_with_schema() {
    let reg = registry();
    let c = reg
        .commands()
        .iter()
        .find(|c| c.method == "psu.history")
        .expect("psu.history");
    assert!(c.write_class.is_none(), "read only");
    let usage = |p: Value| {
        let svc = Services::new();
        call(&reg, &svc, "psu.history", p).expect_err("usage").code
    };
    assert_eq!(
        usage(json!({"ch": "bench"})),
        ErrorCode::Usage,
        "needs from_ms or last_ms"
    );
    assert_eq!(
        usage(json!({"ch": "bench", "last_ms": 1000, "rows": 5001})),
        ErrorCode::Usage
    );
    assert_eq!(
        usage(json!({"ch": "bench", "last_ms": 1000, "cursor": "mark:x"})),
        ErrorCode::Usage,
        "cursor must be a seq cursor"
    );
}

#[test]
fn b3_history_window_rules() {
    let now_ms: u64 = 1_800_000_000_000;
    let now = now_ms * 1_000_000;
    let w = |p: Value| history_window(&p, now);
    assert_eq!(
        w(json!({"last_ms": 3_600_000})).expect("last"),
        Window {
            from_wall_ns: now - 3_600_000_000_000,
            to_wall_ns: now
        }
    );
    assert_eq!(
        w(json!({"from_ms": now_ms - 10_000, "to_ms": now_ms - 5_000})).expect("from/to"),
        Window {
            from_wall_ns: (now_ms - 10_000) * 1_000_000,
            to_wall_ns: (now_ms - 5_000) * 1_000_000
        }
    );
    assert_eq!(
        w(json!({"from_ms": now_ms - 10_000}))
            .expect("to = now")
            .to_wall_ns,
        now
    );
    // an explicit end is kept even when it is in the future (the page stops at the newest journaled sample)
    assert_eq!(
        w(json!({"from_ms": now_ms - 10_000, "to_ms": now_ms + 99_000}))
            .expect("future end kept")
            .to_wall_ns,
        (now_ms + 99_000) * 1_000_000
    );
    for bad in [
        json!({}),
        json!({"from_ms": now_ms - 10, "last_ms": 5}),
        json!({"last_ms": 0}),
        json!({"last_ms": 86_400_001}),
        json!({"from_ms": now_ms - 10, "to_ms": now_ms - 10}),
        json!({"from_ms": now_ms - 86_400_001}),
        json!({"from_ms": now_ms + 1}),
        json!({"last_ms": "1h"}),
    ] {
        assert_eq!(w(bad.clone()).expect_err("usage").code, ErrorCode::Usage, "{bad}");
    }
}

#[test]
fn b3_hist_stat_merge_equals_a_single_pass() {
    let xs = [3.0, -1.0, 7.5, 2.0, 2.0];
    let mut one = HistStat::default();
    for x in xs {
        one.add(x);
    }
    let (mut a, mut b) = (HistStat::default(), HistStat::default());
    for x in &xs[..2] {
        a.add(*x);
    }
    for x in &xs[2..] {
        b.add(*x);
    }
    a.merge(&b);
    assert_eq!(a, one);
    let mut c = one;
    c.merge(&HistStat::default());
    assert_eq!(c, one, "merging an empty later page keeps last");
    assert_eq!(HistStat::default().json(), Value::Null);
    assert_eq!(
        one.json(),
        json!({"n": 5, "min": -1.0, "max": 7.5, "sum": 13.5, "mean": 2.7, "last": 2.0})
    );
}

#[test]
fn b3_time_index_spacing_cap_and_clock_step() {
    let s = 1_000_000_000u64;
    let base = 1_000_000 * s;
    let a = |t: u64, epoch: u64, seq: u64| Anchor {
        wall_ns: base + t,
        epoch,
        seq,
    };
    let ix = TimeIndex::default();
    assert_eq!(ix.seek("x", base), Seek::Empty);
    ix.note("x", a(0, 1, 1));
    ix.note("x", a(5 * s, 1, 2)); // < 10 s after the last anchor → not kept
    ix.note("x", a(10 * s, 1, 3));
    ix.note("x", a(12 * s, 2, 4)); // new epoch → kept
    assert_eq!(ix.seek("x", base + 11 * s), Seek::At(a(10 * s, 1, 3)));
    assert_eq!(ix.seek("x", base + 12 * s), Seek::At(a(12 * s, 2, 4)));
    assert_eq!(
        ix.seek("x", base + 4 * s),
        Seek::At(a(0, 1, 1)),
        "the skipped one never becomes an anchor"
    );
    assert_eq!(ix.seek("x", base - 1), Seek::BeforeIndex(a(0, 1, 1)));
    assert_eq!(ix.coverage("x"), (Some(a(0, 1, 1)), false));
    assert_eq!(ix.seek("other", base), Seek::Empty, "per supply");
    // cap: oldest dropped, flagged
    let cap = TimeIndex::default();
    for k in 0..8_650u64 {
        cap.note("x", a(k * 10 * s, 1, k + 1));
    }
    assert_eq!(cap.coverage("x"), (Some(a(10 * 10 * s, 1, 11)), true));
    // wall clock stepped back: older anchors are no longer ordered → reset to the new one, flagged
    ix.note("x", a(3 * s, 2, 9));
    assert_eq!(ix.coverage("x"), (Some(a(3 * s, 2, 9)), true));
    assert_eq!(ix.seek("x", base + 20 * s), Seek::At(a(3 * s, 2, 9)));
}

#[test]
fn b3_history_page_fold_stops_at_row_cap_and_window_end() {
    let (_reg, _svc, all) = hist_setup();
    let m = meas_of(&all);
    let w = Window {
        from_wall_ns: wall_ns(m[0]),
        to_wall_ns: wall_ns(m[m.len() - 1]) + 1,
    };
    let f = history_page(&all, w, 3);
    assert_eq!(f.rows.len(), 3);
    assert_eq!(f.samples, 3, "stats cover exactly the consumed samples");
    assert_eq!(f.v.n, 3);
    assert_eq!(
        f.stop,
        Stop::RowCap {
            epoch: 1,
            seq: m[3].seq.get()
        }
    );
    assert_eq!(f.first, Some((m[0].seq.get(), wall_ns(m[0]))));
    assert_eq!(f.last, Some((m[2].seq.get(), wall_ns(m[2]))));
    // window end inside the events: everything before it, then ReachedEnd
    let w2 = Window {
        from_wall_ns: wall_ns(m[0]),
        to_wall_ns: wall_ns(m[4]),
    };
    let f2 = history_page(&all, w2, 0);
    assert_eq!((f2.samples, f2.stop), (4, Stop::ReachedEnd));
    assert!(f2.rows.is_empty());
    let p0 = metric(&m[0].metrics, "power").expect("p");
    assert!(f2.p.min <= p0 && p0 <= f2.p.max);
    // samples before the window start are skipped, not counted
    let w3 = Window {
        from_wall_ns: wall_ns(m[2]),
        to_wall_ns: u64::MAX,
    };
    let f3 = history_page(&all, w3, 0);
    assert_eq!(f3.first, Some((m[2].seq.get(), wall_ns(m[2]))));
    assert_eq!((f3.samples, f3.stop), (m.len() as u64 - 2, Stop::Exhausted));
}

#[test]
fn b3_cli_history_maps_to_one_page() {
    let c = to_call(&["history", "bench", "--last", "8h"]).expect("last");
    assert_eq!(c.method, "psu.history");
    assert_eq!(c.params, json!({"ch": "bench", "last_ms": 28_800_000}));
    let c = to_call(&[
        "history",
        "bench",
        "--from-ms",
        "1",
        "--to-ms",
        "2",
        "--rows",
        "10",
        "--cursor",
        "e1:seq:5",
    ])
    .expect("from");
    assert_eq!(
        c.params,
        json!({"ch": "bench", "from_ms": 1, "to_ms": 2, "rows": 10, "cursor": "e1:seq:5"})
    );
    assert!(to_call(&["history", "bench"]).is_err(), "a window is required");
    assert!(to_call(&["history", "bench", "--last", "25h"]).is_err(), "≤ 24 h");
}

// --- operator upgrade batch 5: FIRST3 runtime corrections (FUNCTIONS.md §Batch 5) ----------------

fn offs(svc: &Services) -> Vec<Value> {
    svc.bridge
        .ops
        .lock()
        .expect("lock")
        .iter()
        .map(|(_, op)| op.clone())
        .filter(|op| op["state"] == "off")
        .collect()
}

/// R5-OFF: `psu.off{ch}` is the gauge's emergency OFF — exactly one named supply, the same op as `psu.out off`.
#[test]
fn b5_off_is_fixed_off_and_alias_scoped() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc);
    svc.routed.lock().expect("lock").clear();
    let r = call_as(&reg, &svc, Role::Viewer, "psu.off", json!({"channel": "bench"})).expect("viewer off");
    assert_eq!(r["state"], "effective_verified", "{r}");
    assert!(r.get("results").is_none(), "single-supply result shape: {r}");
    assert_eq!(
        *svc.routed.lock().expect("lock"),
        vec!["bench"],
        "only the named alias is routed"
    );
    assert_eq!(offs(&svc), vec![out_op("bench", "off")]);
    assert!(
        !rows_of(&svc, "rq-psu.off").is_empty(),
        "write rows under the request id"
    );
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], false);
    // dry run: preview only, nothing sent
    let n = svc.bridge.ops.lock().expect("lock").len();
    let p = call_as(
        &reg,
        &svc,
        Role::Viewer,
        "psu.off",
        json!({"ch": "bench", "dry_run": true}),
    )
    .expect("preview");
    assert_eq!(p["dry_run"], true, "{p}");
    assert_eq!(
        svc.bridge.ops.lock().expect("lock").len(),
        n,
        "dry run sends nothing"
    );
}

#[test]
fn b5_off_refuses_any_on_or_extra_param() {
    let (mut reg, svc) = powered();
    let _reducers = feed_reducer(&mut reg, &svc);
    for p in [
        json!({"ch": "bench", "state": "on"}),
        json!({"ch": "bench", "state": "off"}),
        json!({"targets": ["*"]}),
        json!({"ch": "bench", "volt": 1.0}),
        json!({}),
        json!({"ch": ""}),
        json!("bench"),
    ] {
        let e = call_as(&reg, &svc, Role::Viewer, "psu.off", p.clone()).expect_err("usage");
        assert_eq!(e.code, ErrorCode::Usage, "{p}: {e:?}");
    }
    assert!(offs(&svc).is_empty(), "nothing sent");
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], true, "the output was never touched");
}

#[test]
fn b5_off_under_stop_user_passes_agent_refused() {
    let (reg, svc) = powered();
    svc.driver().latch(true);
    let e = call(&reg, &svc, "psu.off", json!({"ch": "bench"})).expect_err("agent under STOP");
    assert_eq!(e.code, ErrorCode::Stopped);
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], true, "STOP keeps the output on");
    call_as(&reg, &svc, Role::Viewer, "psu.off", json!({"ch": "bench"})).expect("user OFF under STOP");
    let st = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status");
    assert_eq!(st["out"], false);
}

#[test]
fn b5_off_hooks_interlock_and_risk() {
    let reg = RegistryProbe::assert_ok(&Psu::with_interlocks(Arc::new(|n: &str| n == "uds.flashing"))).0;
    let h = HookHarness::new(reg.hooks_ref().cloned().expect("hooks"));
    let off = OwnedWriteRequest::new("psu.off", "bench").args(json!({"ch": "bench"}));
    let c = h.classify(&off);
    assert_eq!(c.value.risk, Risk::R3);
    assert!(c.value.block.is_none());
    assert!(
        h.presend_check(&off, None).value.is_err(),
        "agent psu.off refused while flashing"
    );
    let user = off.clone().origin(Origin::User);
    assert!(
        h.presend_check(&user, None).value.is_ok(),
        "the user's emergency OFF is never blocked"
    );
    assert!(
        h.classify(&user).value.reason.contains("WARNING"),
        "{}",
        h.classify(&user).value.reason
    );
    let plain = HookHarness::new(registry().hooks_ref().cloned().expect("hooks"));
    assert!(
        plain.classify(&off).value.reason.contains("output OFF"),
        "{}",
        plain.classify(&off).value.reason
    );
}

/// F1: a connect to an already-connected alias never reports success with a config it did not apply.
#[test]
fn b5_f1_reconnect_same_alias() {
    let reg = registry();
    let svc = Services::new();
    call(
        &reg,
        &svc,
        "psu.connect",
        json!({"ch": "bench", "vmax": 12.0, "imax": 2.0}),
    )
    .expect("connect");
    let g = call(&reg, &svc, "psu.status", json!({"ch": "bench"})).expect("status")["generation"].clone();
    for p in [
        json!({"ch": "bench", "vmax": 10.0}),
        json!({"ch": "bench", "vmax": 12.0, "imax": 1.0}),
        json!({"ch": "bench", "seed": 7}),
    ] {
        let e = call(&reg, &svc, "psu.connect", p.clone()).expect_err("changed config");
        assert_eq!(e.code, ErrorCode::PortBusy, "{p}: {e:?}");
        assert!(e.message.contains("disconnect first"), "{}", e.message);
    }
    let e = call(&reg, &svc, "psu.connect", json!({"ch": "bench", "vmax": 10.0})).expect_err("changed");
    assert!(
        e.message.contains("vmax 10 differs from the connected 12 V"),
        "{}",
        e.message
    );
    for p in [
        json!({"ch": "bench", "vmax": 12.0, "imax": 2.0}),
        json!({"ch": "bench"}),
        json!({"ch": "bench", "imax": 2.0}),
    ] {
        let st = call(&reg, &svc, "psu.connect", p.clone()).expect("same or omitted");
        assert_eq!(st["already_connected"], true, "{p}: {st}");
        assert_eq!(
            (st["vmax"].clone(), st["imax"].clone()),
            (json!(12.0), json!(2.0)),
            "{st}"
        );
        assert_eq!(st["generation"], g, "no new connection");
    }
    // a fresh connect does not carry the flag
    let svc2 = Services::new();
    let st = call(&reg, &svc2, "psu.connect", json!({"ch": "bench"})).expect("fresh");
    assert!(st.get("already_connected").is_none(), "{st}");
}

#[test]
fn b5_f2_limit_above_profile_is_usage() {
    for (p, msg) in [
        (
            json!({"vmax": 20.0}),
            "vmax 20 V is above the profile limit 16 V; only loom.toml raises it",
        ),
        (
            json!({"imax": 6}),
            "imax 6 A is above the profile limit 5 A; only loom.toml raises it",
        ),
        (json!({"vmax": 0.0}), "vmax must be a positive number"),
        (json!({"imax": -1.0}), "imax must be a positive number"),
        (json!({"vmax": "12"}), "vmax must be a positive number"),
    ] {
        assert_eq!(Limits::requested(&p), Err(msg.to_owned()), "{p}");
    }
    assert_eq!(Limits::requested(&json!({})), Ok(Limits::default()));
    assert_eq!(
        Limits::requested(&json!({"vmax": null, "imax": 2.5})),
        Ok(Limits {
            vmax: 16.0,
            imax: 2.5
        })
    );
    assert_eq!(
        Limits::requested(&json!({"vmax": 16.0, "imax": 5.0})),
        Ok(Limits::default()),
        "the default itself is allowed"
    );
    let have = Limits {
        vmax: 12.0,
        imax: 2.0,
    };
    assert_eq!(config_mismatch(&have, &json!({}), &["seed", "load"]), None);
    assert_eq!(
        config_mismatch(&have, &json!({"vmax": 12.0, "seed": null}), &["seed", "load"]),
        None
    );
    assert_eq!(
        config_mismatch(&have, &json!({"imax": 1.5}), &[]).as_deref(),
        Some("imax 1.5 differs from the connected 2 A")
    );
    assert_eq!(
        config_mismatch(&have, &json!({"load": {"ohm": 5}}), &["seed", "load"]).as_deref(),
        Some("load applies only to a new connection")
    );
    assert_eq!(
        config_mismatch(&have, &json!({"seed": 1}), &[]),
        None,
        "keys outside new_only are not compared"
    );
    // gateway path: refused before any device exists
    let reg = registry();
    let svc = Services::new();
    let e = call(&reg, &svc, "psu.connect", json!({"ch": "bench", "vmax": 20.0})).expect_err("above");
    assert_eq!((e.code, e.code.exit_code()), (ErrorCode::Usage, 2));
    assert_eq!(
        call(&reg, &svc, "psu.status", json!({"ch": "bench"}))
            .expect_err("none")
            .code,
        ErrorCode::NotOpen
    );
}

/// F3: the default look-back can match samples from before a write; `since: "now"` cannot.
#[test]
fn b5_f3_wait_since_now_ignores_samples_before_the_write() {
    let (reg, svc) = powered(); // ON, current ≈ 0.365 A for 2 s
    call(&reg, &svc, "psu.out", json!({"ch": "bench", "state": "off"})).expect("off");
    let w = |extra: Value| {
        let mut p = json!({"ch": "bench", "curr_gt": 0.3, "hold_ms": 400, "timeout_ms": 300});
        p.as_object_mut()
            .expect("obj")
            .extend(extra.as_object().expect("obj").clone());
        call(&reg, &svc, "psu.wait_for", p)
    };
    let stale = w(json!({})).expect("default look-back sees the pre-OFF samples");
    assert_eq!(stale["matched"], true, "{stale}");
    let e = w(json!({"since": "now"})).expect_err("output is OFF after the write");
    assert_eq!(e.code, ErrorCode::Timeout, "{e:?}");
}

/// F5 (5b): the real daemon reports a reached scan budget as `E_SCAN_LIMIT{resume_from, partial}`.
#[test]
fn b5_f5_history_pages_through_journal_scan_limit() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let from_ms = wall_ns(m[0]) / 1_000_000;
    let to_ms = wall_ns(m[m.len() - 1]) / 1_000_000 + 1;
    let q = json!({"ch": "bench", "from_ms": from_ms, "to_ms": to_ms});
    let whole = call(&reg, &svc, "psu.history", q.clone()).expect("unlimited");
    svc.journal.scan_limit_error(16 * 1024);
    let mut p = q.clone();
    let (mut samples, mut n, mut pages) = (0u64, 0u64, 0);
    let last = loop {
        let r = call(&reg, &svc, "psu.history", p.clone()).expect("page, not an error");
        pages += 1;
        if !r["next_cursor"].is_null() {
            assert_eq!(
                r["complete"], false,
                "an interrupted page never claims complete: {r}"
            );
        }
        assert!(
            r["scanned_bytes"].as_u64().unwrap_or(0) > 0,
            "byte budget accounted: {r}"
        );
        samples += r["samples"].as_u64().expect("samples");
        n += r["stats"]["voltage"]["n"].as_u64().unwrap_or(0);
        match r["next_cursor"].as_str() {
            Some(c) => p["cursor"] = json!(c),
            None => break r,
        }
        assert!(pages < 200, "pages terminate");
    };
    assert!(pages > 1, "the budget was hit: {pages} page(s)");
    assert_eq!(samples, whole["samples"].as_u64().expect("samples"));
    assert_eq!(n, whole["stats"]["voltage"]["n"].as_u64().expect("n"));
    assert_eq!(last["complete"], true, "{last}");
}

#[test]
fn b5_f5_wait_for_over_a_scan_limited_journal() {
    let (reg, svc) = powered();
    svc.journal.scan_limit_error(4 * 1024);
    let r = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_gt": 0.3, "hold_ms": 400, "timeout_ms": 500, "since": "seq:1"}),
    )
    .expect("matched, not an E_SCAN_LIMIT error");
    assert_eq!(r["matched"], true, "{r}");
}

/// F3 (real daemon: `wait-for --since now` returned E_TIMEOUT with 0 samples, RUNTIME.md): `Cursor::Now`
/// re-resolved to the newest seq on every poll, so a wait never saw a sample. A scan that found no
/// event must still return where to continue (`scanned_through + 1`), pinning `now` once.
#[test]
fn b5_f3_scan_from_now_returns_a_resume_seq() {
    let (_reg, svc) = powered();
    let services: Arc<dyn loom_domain_api::CtxServices> = svc.clone();
    let ctx = loom_domain_api::CallCtx::new(
        Role::Agent,
        None,
        "test",
        None,
        Instant::now() + Duration::from_secs(10),
        services,
    );
    let last = svc.journal.last_seq();
    let (ev, next, _) = block_on(loom_psu::gateway::common::scan(
        &ctx,
        "bench",
        loom_proto::cursor::Cursor::Now,
        1 << 20,
    ))
    .expect("scan");
    assert!(ev.is_empty());
    assert_eq!(
        next,
        Some(last + 1),
        "resume right after the high-water mark seen at arm time"
    );
    svc.driver().advance_ms(1000);
    let (ev, _, _) = block_on(loom_psu::gateway::common::scan(
        &ctx,
        "bench",
        loom_proto::cursor::Cursor::Seq {
            epoch: None,
            seq: last + 1,
        },
        1 << 20,
    ))
    .expect("scan");
    assert!(
        ev.iter().any(|e| e.kind == "psu.meas"),
        "samples after the arm point are seen"
    );
}

/// F5 (5b): a scan-limit error without a usable `resume_from` is returned unchanged.
#[test]
fn b5_f5_malformed_scan_limit_propagates_unchanged() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let q = json!({"ch": "bench", "from_ms": wall_ns(m[0]) / 1_000_000, "to_ms": wall_ns(m[m.len() - 1]) / 1_000_000 + 1});
    svc.journal.scan_limit_error(16 * 1024);
    *svc.journal.limit_err_shape.lock().expect("lock") = "malformed";
    let e = call(&reg, &svc, "psu.history", q).expect_err("propagated");
    assert_eq!(e.code, ErrorCode::ScanLimit);
    assert_eq!(e.message, "scan budget reached — continue from resume_from");
    assert!(
        e.detail.get("resume_from").is_none() && e.detail["partial"].is_array(),
        "detail unchanged: {:?}",
        e.detail
    );
    let e = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_gt": 0.3, "hold_ms": 400, "timeout_ms": 500, "since": "seq:1"}),
    )
    .expect_err("propagated");
    assert_eq!(e.code, ErrorCode::ScanLimit);
}

/// F5 (5b): resume_from == from with no partial is a budget hit without progress: an error, never a loop.
#[test]
fn b5_f5_non_advancing_scan_limit_is_an_error_not_a_loop() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let q = json!({"ch": "bench", "from_ms": wall_ns(m[0]) / 1_000_000, "to_ms": wall_ns(m[m.len() - 1]) / 1_000_000 + 1});
    svc.journal.scan_limit_error(16 * 1024);
    *svc.journal.limit_err_shape.lock().expect("lock") = "stuck";
    let t = Instant::now();
    let e = call(&reg, &svc, "psu.history", q).expect_err("no progress");
    assert_eq!(e.code, ErrorCode::ScanLimit);
    let e = call(
        &reg,
        &svc,
        "psu.wait_for",
        json!({"ch": "bench", "curr_gt": 0.3, "hold_ms": 400, "timeout_ms": 5000, "since": "seq:1"}),
    )
    .expect_err("no progress");
    assert_eq!(e.code, ErrorCode::ScanLimit);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "returned promptly, not after the wait timeout: {:?}",
        t.elapsed()
    );
}

// --- operator upgrade batch 6: history provenance (FUNCTIONS.md §Batch 6) -----------------------

/// Every raw row names its own canonical journal record: `e<epoch>:seq:<seq>`, generation, clock id.
#[test]
fn b6_history_rows_carry_the_records_own_provenance() {
    let (reg, svc, all) = hist_setup();
    let m = meas_of(&all);
    let from_ms = wall_ns(m[0]) / 1_000_000;
    let to_ms = wall_ns(m[m.len() - 1]) / 1_000_000 + 1;
    let r = call(
        &reg,
        &svc,
        "psu.history",
        json!({"ch": "bench", "from_ms": from_ms, "to_ms": to_ms, "rows": 50}),
    )
    .expect("history");
    let rows = r["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 50, "{r}");
    for row in rows {
        let seq = row["seq"].as_u64().expect("seq");
        let rec = m
            .iter()
            .find(|e| e.seq.get() == seq)
            .expect("journal record of the row");
        assert_eq!(
            row["ref"],
            json!(format!("e{}:seq:{}", rec.epoch.get(), seq)),
            "{row}"
        );
        assert_eq!(row["generation"], json!(rec.generation), "{row}");
        assert_eq!(row["clock_id"], json!(rec.ts.clock_id), "{row}");
        assert!(
            row.get("v").is_some() && row.get("wall_ms").is_some(),
            "existing fields kept: {row}"
        );
    }
}

/// Seq restarts per epoch: two records with the same seq in different epochs must not share a ref.
#[test]
fn b6_same_seq_in_two_epochs_has_distinct_refs() {
    let (_reg, _svc, all) = hist_setup();
    let base = meas_of(&all)[0].clone();
    let mut old = base.clone();
    old.epoch = loom_proto::num::U64(1794774986);
    old.ts.clock_id = "proc-old".into();
    old.generation = Some(4);
    let mut new = base.clone();
    new.epoch = loom_proto::num::U64(1794778200);
    new.ts.clock_id = "proc-new".into();
    new.generation = None;
    new.ts.wall_ns = loom_proto::num::U64(base.ts.wall_ns.get() + 1_000_000);
    let w = Window {
        from_wall_ns: base.ts.wall_ns.get(),
        to_wall_ns: base.ts.wall_ns.get() + 10_000_000,
    };
    let f = history_page(&[old, new], w, 10);
    let seq = base.seq.get();
    assert_eq!(
        f.rows
            .iter()
            .map(|r| (r["ref"].clone(), r["generation"].clone(), r["clock_id"].clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                json!(format!("e1794774986:seq:{seq}")),
                json!(4),
                json!("proc-old")
            ),
            (
                json!(format!("e1794778200:seq:{seq}")),
                Value::Null,
                json!("proc-new")
            ),
        ]
    );
}
