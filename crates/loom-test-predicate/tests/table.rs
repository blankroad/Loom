//! Table cases: AST parse/type errors, evaluation, common predicates with coverage/role rules,
//! budget, digests, rendering, verdict judgment and session aggregation.

mod support;

use std::time::Duration;

use loom_domain_api::{CoverageNeed, EvalBudget, EvidenceVerifier, Predicate, Verification};
use loom_proto::error::ErrorCode;
use loom_proto::event::WriteState;
use loom_test_predicate::verdict::*;
use loom_test_predicate::{Citation, CommonVerifier, Expr, Status};
use serde_json::{Value, json};
use support::*;

// --- parse errors (each: path + expected + actual) ---------------------------------------------

fn parse_err(v: Value) -> String {
    match Expr::parse(&v, "steps[2].expect[0].pred.where") {
        Ok(_) => panic!("{v} should not parse"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn parse_errors_carry_the_path() {
    let deep = (0..40).fold(
        json!({"field": "/payload/value", "op": "==", "value": 1}),
        |acc, _| json!({"not": acc}),
    );
    let wide = json!({"all": vec![json!({"field": "/payload/value", "op": "==", "value": 1}); 600]});
    let cases: Vec<(Value, &str)> = vec![
        (json!({"field": "/payload/value", "op": "~=", "value": 1}), ".op"),
        (json!({"op": "==", "value": 1}), "missing field"),
        (
            json!({"field": "payload/value", "op": "==", "value": 1}),
            "bad pointer",
        ),
        (json!({"field": "/payload/value", "op": "=="}), "missing value"),
        (
            json!({"field": "/payload/value", "op": "==", "value": 1, "extra": 2}),
            ".extra",
        ),
        (
            json!({"all": [{"field": "/a", "op": "==", "value": 1}, {"field": "/b", "op": "<", "value": "x"}]}),
            "where.all[1].value",
        ),
        (
            json!({"field": "/payload/value", "op": "in", "value": vec![1; 300]}),
            "list too long",
        ),
        (
            json!({"field": "/payload/text", "op": "regex", "value": "a".repeat(2000)}),
            "regex too long",
        ),
        (
            json!({"field": "/payload/text", "op": "regex", "value": "(unclosed"}),
            "valid Rust regex",
        ),
        (
            json!({"field": "/payload/value", "op": "between", "value": [1]}),
            "two bounds",
        ),
        (
            json!({"field": "/payload/raw", "op": "bytes_eq", "value": "0g"}),
            "bad hex",
        ),
        (json!({"all": [], "any": []}), "extra keys"),
        (json!([1, 2]), "not an object"),
        (deep, "too deep"),
        (wide, "too large"),
    ];
    for (v, want) in cases {
        let e = parse_err(v.clone());
        assert!(e.contains(want), "{v} → {e}");
        assert!(e.starts_with("steps[2].expect[0].pred.where"), "{e}");
    }
}

#[test]
fn citation_parse_errors() {
    let bad = |v: Value, want: &str| {
        let e = Citation::parse(&v, "steps[1].expect[0]")
            .map(|_| ())
            .expect_err("bad citation")
            .to_string();
        assert!(e.contains(want), "{v} → {e}");
    };
    bad(json!({"pred": {"exists": {}}}), "missing filter");
    bad(
        json!({"filter": {"domain": "uart", "colour": 1}, "pred": {"exists": {}}}),
        "filter.colour",
    );
    bad(json!({"filter": {}, "pred": {"maybe": {}}}), "unknown predicate");
    bad(
        json!({"filter": {}, "pred": {"count": {}}}),
        "count needs a bound",
    );
    bad(
        json!({"filter": {}, "pred": {"count": {"min": 3, "max": 1}}}),
        "bad count range",
    );
    bad(
        json!({"filter": {}, "pred": {"within_ms": {"max": "5 s"}}}),
        "missing anchor",
    );
    bad(
        json!({"filter": {}, "pred": {"within_ms": {"anchor": {"mono_ns": "1"}, "max": "5 parsecs"}}}),
        "pred.within_ms.max",
    );
    bad(
        json!({"filter": {"origins": ["robot"]}, "pred": {"exists": {}}}),
        "unknown origin",
    );
    bad(
        json!({"filter": {}, "pred": {"exists": {}, "absent": {}}}),
        "exactly one predicate",
    );
}

// --- type check ------------------------------------------------------------------------------------

fn check_err(where_: Value) -> String {
    let v = json!({"filter": {"domain": "x"}, "pred": {"exists": {"where": where_}}});
    let mut c = Citation::parse(&v, "p").expect("parse");
    c.check(&specs(), "p")
        .map(|_| ())
        .expect_err("type error")
        .to_string()
}

#[test]
fn type_check_rejects_unknown_and_mismatched_fields() {
    assert!(check_err(json!({"field": "/payload/nope", "op": "==", "value": 1})).contains("unknown field"));
    assert!(
        check_err(json!({"field": "/payload/value", "op": "==", "value": "on"})).contains("does not fit")
    );
    assert!(check_err(json!({"field": "/payload/state", "op": "==", "value": "maybe"})).contains("on|off"));
    assert!(check_err(json!({"field": "/payload/value", "op": "regex", "value": "x"})).contains("regex on"));
    assert!(check_err(json!({"field": "/payload/text", "op": ">", "value": 3})).contains("numeric field"));
    assert!(
        check_err(json!({"field": "/payload/value", "op": "<", "value": "5 s"})).contains("plain number")
    );
    assert!(
        check_err(json!({"field": "/payload/age_ms", "op": "<", "value": "5 V"})).contains("not comparable")
    );
    assert!(check_err(json!({"field": "/payload/ok", "op": "==", "value": 1})).contains("does not fit"));
    // Fine: builtin fields, quantities in the field's dimension, enum members.
    let ok = |w: Value| {
        let mut c =
            Citation::parse(&json!({"filter": {}, "pred": {"exists": {"where": w}}}), "p").expect("parse");
        c.check(&specs(), "p").expect("type ok");
    };
    ok(json!({"field": "/origin", "op": "==", "value": "device"}));
    ok(json!({"field": "/payload/age_ms", "op": "<=", "value": "5 s"}));
    ok(json!({"field": "/payload/state", "op": "in", "value": ["on", "off"]}));
    ok(json!({"field": "/ts/mono_ns", "op": ">", "value": "1 s"}));
}

// --- evaluation -----------------------------------------------------------------------------------

fn ev(where_: Value, payload: Value) -> Result<bool, String> {
    let mut e = Expr::parse(&where_, "w").expect("parse");
    e.check(&specs(), "w").expect("check");
    let rec = serde_json::to_value(r(1, 0, "x", "c", "x.e", payload).build()).expect("json");
    e.eval(&rec).map_err(|e| e.to_string())
}

#[test]
fn evaluation_semantics() {
    let t = |w: Value, p: Value| ev(w, p).expect("no error");
    assert!(t(
        json!({"field": "/payload/value", "op": "==", "value": 2}),
        json!({"value": 2.0})
    ));
    assert!(t(
        json!({"field": "/payload/age_ms", "op": "<=", "value": "5 s"}),
        json!({"age_ms": 5000})
    ));
    assert!(!t(
        json!({"field": "/payload/age_ms", "op": "<", "value": "5 s"}),
        json!({"age_ms": 5000})
    ));
    assert!(t(
        json!({"field": "/payload/volt", "op": "between", "value": ["13 V", "14000 mV"]}),
        json!({"volt": 14.0})
    ));
    assert!(t(
        json!({"field": "/payload/big", "op": ">", "value": "18446744073709551614"}),
        json!({"big": "18446744073709551615"})
    ));
    assert!(t(
        json!({"field": "/payload/state", "op": "in", "value": ["on"]}),
        json!({"state": "on"})
    ));
    assert!(t(
        json!({"field": "/payload/text", "op": "contains", "value": "login:"}),
        json!({"text": "bench login: "})
    ));
    assert!(t(
        json!({"field": "/payload/bytes", "op": "contains", "value": 241}),
        json!({"bytes": [98, 241]})
    ));
    assert!(t(
        json!({"field": "/payload/text", "op": "regex", "value": "^U-Boot \\d+"}),
        json!({"text": "U-Boot 2024"})
    ));
    assert!(t(
        json!({"field": "/payload/sid", "op": "bytes_eq", "value": "62 f1 90"}),
        json!({"sid": "62F190"})
    ));
    assert!(t(
        json!({"field": "/payload/bytes", "op": "bytes_eq", "value": "62f1"}),
        json!({"bytes": [98, 241]})
    ));
    assert!(t(
        json!({"field": "/payload/raw", "op": "bytes_eq", "value": "414243"}),
        json!({"raw": {"encoding": "base64", "data": "QUJD"}})
    ));
    assert!(t(
        json!({"not": {"field": "/payload/value", "op": ">", "value": 5}}),
        json!({"value": 1})
    ));
    assert!(t(
        json!({"any": [{"field": "/payload/value", "op": "<", "value": 0}, {"field": "/payload/value", "op": ">", "value": 0}]}),
        json!({"value": 1})
    ));
    assert!(t(json!({"all": []}), json!({})));
    // Missing / wrong type are errors, never false — also under `!=` and `not`.
    for (w, p) in [
        (
            json!({"field": "/payload/value", "op": "==", "value": 1}),
            json!({}),
        ),
        (
            json!({"field": "/payload/value", "op": "!=", "value": 1}),
            json!({}),
        ),
        (
            json!({"not": {"field": "/payload/value", "op": "==", "value": 1}}),
            json!({}),
        ),
        (
            json!({"field": "/payload/value", "op": "<", "value": 1}),
            json!({"value": "bad"}),
        ),
        (
            json!({"field": "/payload/text", "op": "regex", "value": "x"}),
            json!({"text": 5}),
        ),
    ] {
        assert!(ev(w.clone(), p.clone()).is_err(), "{w} on {p}");
    }
}

// --- common predicates ------------------------------------------------------------------------------

fn login() -> Value {
    json!({"field": "/payload/text", "op": "contains", "value": "login:"})
}

fn uart(seq: u64, t: u64, text: &str) -> R {
    r(seq, t, "uart", "dut", "uart.line", json!({"text": text}))
}

#[test]
fn exists_and_roles() {
    let c =
        cite(json!({"filter": {"domain": "uart", "channel": "dut"}, "pred": {"exists": {"where": login()}}}));
    assert_eq!(c.pred.need(), CoverageNeed::Positive);
    let ok = eval(
        &c,
        &recs(vec![uart(1, 0, "boot"), uart(2, 10, "bench login:")]),
        true,
    );
    assert!(ok.status == Status::Verified && ok.device_evidence);
    // Positive match still verifies with incomplete coverage.
    assert_eq!(
        eval(&c, &recs(vec![uart(2, 10, "login:")]), false).status,
        Status::Verified
    );
    // No match + incomplete coverage → inconclusive, + complete → not found.
    assert_eq!(
        eval(&c, &recs(vec![uart(1, 0, "boot")]), false).status,
        Status::Inconclusive
    );
    assert_eq!(
        eval(&c, &recs(vec![uart(1, 0, "boot")]), true).status,
        Status::NotFound
    );
    // Only our own tx matches → agent_tx_only (R-8).
    let own = eval(&c, &recs(vec![uart(3, 0, "login:").origin("agent")]), true);
    assert_eq!(
        (own.status, own.cause.as_deref()),
        (Status::Mismatch, Some("agent_tx_only"))
    );
    // eth: Loom's own frame seen on the wire (origin device, attributed_origin agent).
    let e = cite(json!({"filter": {"domain": "eth"}, "pred": {"exists": {}}}));
    let wire = r(
        4,
        0,
        "eth",
        "dut",
        "eth.pkt",
        json!({"attributed_origin": "agent"}),
    )
    .build();
    assert_eq!(eval(&e, &[wire], true).cause.as_deref(), Some("agent_tx_only"));
    // Explicit origins allow agent evidence (stimulus checks).
    let a = cite(
        json!({"filter": {"domain": "uart", "origins": ["agent"]}, "pred": {"exists": {"where": login()}}}),
    );
    assert_eq!(
        eval(&a, &recs(vec![uart(3, 0, "login:").origin("agent")]), true).status,
        Status::Verified
    );
}

#[test]
fn request_pairing() {
    let c = cite(json!({"filter": {"domain": "uds", "request_id": "q-7"},
                        "pred": {"exists": {"where": {"field": "/payload/sid", "op": "==", "value": "62"}}}}));
    let resp = |seq, id: &str| r(seq, 0, "uds", "ecu", "uds.response", json!({"sid": "62"})).req(id);
    assert_eq!(
        eval(&c, &recs(vec![resp(1, "q-7")]), true).status,
        Status::Verified
    );
    let other = eval(&c, &recs(vec![resp(1, "q-8")]), true);
    assert_eq!(
        (other.status, other.cause.as_deref()),
        (Status::Mismatch, Some("request_id_mismatch"))
    );
}

#[test]
fn absent_count_and_coverage() {
    let ab = cite(
        json!({"filter": {"domain": "uart", "channel": "dut"}, "pred": {"absent": {"where": {"field": "/payload/text", "op": "contains", "value": "panic"}}}}),
    );
    assert_eq!(ab.pred.need(), CoverageNeed::Complete);
    let quiet = recs(vec![uart(1, 0, "ok"), uart(2, 5, "ok")]);
    let v = eval(&ab, &quiet, true);
    assert_eq!((v.status, v.matched_total), (Status::Verified, 0));
    assert_eq!(
        eval(&ab, &recs(vec![uart(1, 0, "Kernel panic")]), true).status,
        Status::Mismatch
    );
    assert_eq!(
        eval(&ab, &quiet, false).status,
        Status::Inconclusive,
        "incomplete coverage"
    );
    let gap = recs(vec![
        uart(1, 0, "ok"),
        r(
            2,
            5,
            "source",
            "dut",
            "source.gap",
            json!({"cause": "overflow", "count": "unknown"}),
        ),
    ]);
    assert!(
        eval(&ab, &gap, true)
            .cause
            .as_deref()
            .is_some_and(|c| c.contains("gap_in_window"))
    );
    let other_gap = recs(vec![
        uart(1, 0, "ok"),
        r(
            2,
            5,
            "source",
            "can0",
            "source.gap",
            json!({"cause": "x", "count": "unknown"}),
        ),
    ]);
    assert_eq!(
        eval(&ab, &other_gap, true).status,
        Status::Verified,
        "a gap on another channel does not matter"
    );
    let epoch = recs(vec![uart(1, 0, "ok"), uart(2, 5, "ok").epoch(2)]);
    assert!(
        eval(&ab, &epoch, true)
            .cause
            .as_deref()
            .is_some_and(|c| c.contains("epoch_changed"))
    );

    let exact = cite(
        json!({"filter": {"domain": "uart"}, "pred": {"count": {"where": login(), "min": 2, "max": 2}}}),
    );
    let two = recs(vec![uart(1, 0, "login:"), uart(2, 5, "login:")]);
    assert_eq!(eval(&exact, &two, true).status, Status::Verified);
    assert_eq!(
        eval(&exact, &recs(vec![uart(1, 0, "login:")]), true)
            .cause
            .as_deref(),
        Some("count_out_of_range")
    );
    let at_least =
        cite(json!({"filter": {"domain": "uart"}, "pred": {"count": {"where": login(), "min": 2}}}));
    assert_eq!(at_least.pred.need(), CoverageNeed::Positive);
    assert_eq!(eval(&at_least, &two, false).status, Status::Verified);
    assert_eq!(
        eval(&at_least, &recs(vec![uart(1, 0, "login:")]), false).status,
        Status::Inconclusive
    );
}

#[test]
fn user_intervention_makes_it_inconclusive() {
    let c =
        cite(json!({"filter": {"domain": "uart", "channel": "dut"}, "pred": {"exists": {"where": login()}}}));
    let user_write = r(2, 5, "uart", "dut", "uart.write", json!({"write_state": "sent"})).origin("user");
    let v = eval(&c, &recs(vec![uart(1, 0, "login:"), user_write.clone()]), true);
    assert_eq!(
        (v.status, v.cause.as_deref()),
        (Status::Inconclusive, Some("user_intervention"))
    );
    // Another channel: no effect.
    let elsewhere = r(2, 5, "uart", "aux", "uart.write", json!({"write_state": "sent"})).origin("user");
    assert_eq!(
        eval(&c, &recs(vec![uart(1, 0, "login:"), elsewhere]), true).status,
        Status::Verified
    );
}

#[test]
fn order_within_and_measure() {
    let o = cite(json!({"filter": {"domain": "uart"}, "pred": {"order": {
        "a": {"field": "/payload/text", "op": "contains", "value": "U-Boot"}, "b": login()}}}));
    assert_eq!(
        eval(&o, &recs(vec![uart(1, 0, "U-Boot"), uart(2, 9, "login:")]), true).status,
        Status::Verified
    );
    assert_eq!(
        eval(&o, &recs(vec![uart(1, 0, "login:"), uart(2, 9, "U-Boot")]), true)
            .cause
            .as_deref(),
        Some("b_not_after_a")
    );

    let within = |max: &str, tol: u64| {
        cite(json!({"filter": {"domain": "uart"}, "pred": {"within_ms": {
            "anchor": {"step": "S2", "where": {"field": "/kind", "op": "==", "value": "psu.write"}},
            "max": max, "tolerance_ms": tol, "where": login()}}}))
    };
    let stim = r(
        1,
        1000,
        "psu",
        "bench",
        "psu.write",
        json!({"write_state": "sent"}),
    )
    .origin("agent");
    let boot = |t| recs(vec![stim.clone(), uart(2, t, "login:")]);
    assert_eq!(
        eval(&within("5 s", 0), &boot(5_000), true).status,
        Status::Verified
    );
    assert_eq!(
        eval(&within("5 s", 0), &boot(6_001), true).cause.as_deref(),
        Some("too_late")
    );
    assert_eq!(
        eval(&within("5 s", 50), &boot(6_030), true).cause.as_deref(),
        Some("clock_uncertainty")
    );
    assert_eq!(
        eval(&within("5 s", 0), &recs(vec![uart(2, 3, "login:")]), true)
            .cause
            .as_deref(),
        Some("anchor_not_found")
    );
    let mono = cite(
        json!({"filter": {"domain": "uart"}, "pred": {"within_ms": {"anchor": {"mono_ns": "1000000000"}, "max": 500, "where": login()}}}),
    );
    assert_eq!(
        eval(&mono, &recs(vec![uart(2, 1_400, "login:")]), true).status,
        Status::Verified
    );

    let measure = |min: u64, max: u64, tol: u64| {
        cite(json!({"filter": {"domain": "uart"}, "pred": {"measure": {
            "from": {"field": "/kind", "op": "==", "value": "psu.write"}, "to": login(),
            "min_ms": min, "max_ms": max, "tolerance_ms": tol}}}))
    };
    let m = eval(&measure(1000, 5000, 0), &boot(4_200), true);
    assert_eq!(m.status, Status::Verified);
    assert_eq!(m.observed["ms"], 3200.0);
    assert_eq!(
        eval(&measure(1000, 3000, 0), &boot(4_200), true).cause.as_deref(),
        Some("out_of_bounds")
    );
    assert_eq!(
        eval(&measure(1000, 3190, 20), &boot(4_200), true)
            .cause
            .as_deref(),
        Some("clock_uncertainty")
    );
    assert_eq!(
        eval(&measure(1000, 3210, 20), &boot(4_200), true)
            .cause
            .as_deref(),
        Some("clock_uncertainty")
    );
    assert_eq!(
        eval(&measure(1000, 5000, 0), &recs(vec![uart(2, 3, "login:")]), true)
            .cause
            .as_deref(),
        Some("from_not_found")
    );
    assert_eq!(
        eval(&measure(1000, 5000, 0), &recs(vec![stim.clone()]), true)
            .cause
            .as_deref(),
        Some("to_not_found")
    );
    assert_eq!(
        eval(&measure(1000, 5000, 0), &boot(4_200), false).status,
        Status::Inconclusive,
        "Measure needs coverage"
    );
}

#[test]
fn hold_rules_beyond_the_spike() {
    let h = |gap: u64| {
        cite(json!({"filter": {"domain": "can"}, "pred": {"hold": {
        "where": {"field": "/payload/value", "op": ">", "value": 1}, "min_ms": 300, "max_gap_ms": gap}}}))
    };
    let s = |seq, t| r(seq, t, "can", "can0", "can.signal", json!({"value": 2}));
    let every_200 = recs(vec![s(1, 0), s(2, 200), s(3, 400)]);
    assert_eq!(
        eval(&h(150), &every_200, true).cause.as_deref(),
        Some("stale_sample")
    );
    assert_eq!(
        eval(&h(250), &every_200, true).status,
        Status::Verified,
        "custom max_gap_ms"
    );
    let two_sources = recs(vec![s(1, 0), s(2, 100).producer("w2"), s(3, 200), s(4, 300)]);
    assert_eq!(
        eval(&h(150), &two_sources, true).cause.as_deref(),
        Some("source_changed")
    );
}

#[test]
fn budget_digest_and_verifier_adapter() {
    let c = cite(json!({"filter": {"domain": "uart"}, "pred": {"exists": {"where": login()}}}));
    let many: Vec<_> = (1..=2000).map(|i| uart(i, i, "x").build()).collect();
    let mut tiny = EvalBudget::new(100, Duration::from_secs(1));
    let b = c.evaluate(&many, &cov(true), &mut tiny);
    assert_eq!(
        (b.status, b.cause.as_deref()),
        (Status::Inconclusive, Some("eval_budget"))
    );
    // Digest: same matches → same digest; different witness set → different.
    let a1 = eval(&c, &recs(vec![uart(1, 0, "login:")]), true);
    let a2 = eval(&c, &recs(vec![uart(1, 0, "login:"), uart(9, 50, "noise")]), true);
    let a3 = eval(&c, &recs(vec![uart(2, 0, "login:")]), true);
    assert_eq!(a1.digest, a2.digest);
    assert_ne!(a1.digest, a3.digest);
    assert_eq!(a1.digest.len(), 64);
    // CORE adapter.
    let v = CommonVerifier { specs: specs() };
    let p = Predicate {
        name: "test.absent".into(),
        args: json!({"filter": {"domain": "uart"}, "where": login()}),
        expect_id: None,
    };
    assert_eq!(v.coverage_need(&p), CoverageNeed::Complete);
    let out = v.verify(
        &p,
        &recs(vec![uart(1, 0, "boot")]),
        &cov(true),
        &mut EvalBudget::default(),
    );
    assert!(matches!(out, Verification::Match { .. }), "{out:?}");
    let bad = Predicate {
        name: "test.absent".into(),
        args: json!({"filter": {}, "where": {"field": "/payload/nope", "op": "==", "value": 1}}),
        expect_id: None,
    };
    let out = v.verify(&bad, &[], &cov(true), &mut EvalBudget::default());
    assert!(
        matches!(&out, Verification::Inconclusive { reason } if reason.contains("unknown field")),
        "{out:?}"
    );
}

#[test]
fn korean_rendering() {
    let mut e = Expr::parse(
        &json!({"field": "/payload/signals/VehState.PowerMode", "op": "==", "value": 2}),
        "w",
    )
    .expect("parse");
    e.check(&specs(), "w").expect("check");
    assert_eq!(
        loom_test_predicate::render::expr(&e),
        "신호 VehState.PowerMode가 2"
    );
    let w = cite(
        json!({"filter": {"domain": "uart", "channel": "dut"}, "pred": {"within_ms": {
        "anchor": {"step": "S2", "mono_ns": "0"}, "max": "5 s", "where": login()}}}),
    );
    assert_eq!(
        w.render(),
        "S2 매치 후 5 s 안에 uart dut에서 text에 \"login:\" 포함"
    );
    let a = cite(
        json!({"filter": {"domain": "uart"}, "pred": {"absent": {"where": {"field": "/payload/value", "op": ">=", "value": 3}}}}),
    );
    assert_eq!(a.render(), "uart에서 value가 3 이상 이벤트가 창 전체에 없음");
}

// --- verdicts --------------------------------------------------------------------------------------

fn sc(status: Status, device: bool, total: u64) -> StepCheck {
    let c = cite(json!({"filter": {}, "pred": {"exists": {}}}));
    let mut check = eval(&c, &[], true);
    check.status = status;
    check.device_evidence = device;
    check.matched_total = total;
    StepCheck {
        check,
        plan_match: true,
        in_window: true,
    }
}

#[test]
fn step_judgment_rules() {
    let pass = |checks: &[StepCheck], h| judge_step(&Proposed::Pass, StepKind::Assert, checks, &[], h);
    assert_eq!(
        pass(&[sc(Status::Verified, true, 1)], RecordHealth::Ok),
        Ok(StepVerdict::Pass)
    );
    assert_eq!(
        pass(&[sc(Status::Verified, true, 1)], RecordHealth::Degraded),
        Ok(StepVerdict::Pass)
    );
    assert_eq!(pass(&[], RecordHealth::Ok), Err(Reject::EvidenceMissing));
    assert_eq!(
        pass(
            &[sc(Status::Verified, true, 1), sc(Status::Mismatch, true, 1)],
            RecordHealth::Ok
        ),
        Err(Reject::EvidenceMismatch)
    );
    assert_eq!(
        pass(&[sc(Status::NotFound, false, 0)], RecordHealth::Ok),
        Err(Reject::EvidenceMismatch)
    );
    assert_eq!(
        pass(&[sc(Status::Inconclusive, false, 0)], RecordHealth::Ok),
        Err(Reject::EvidenceMissing)
    );
    assert_eq!(
        pass(&[sc(Status::Error, false, 0)], RecordHealth::Ok),
        Err(Reject::EvidenceMissing)
    );
    assert_eq!(
        pass(&[sc(Status::Verified, true, 1)], RecordHealth::Failed),
        Err(Reject::RecordingFailed)
    );
    assert_eq!(
        pass(&[sc(Status::Verified, false, 1)], RecordHealth::Ok),
        Err(Reject::EvidenceMissing),
        "agent tx only"
    );
    assert_eq!(
        pass(&[sc(Status::Verified, false, 0)], RecordHealth::Ok),
        Ok(StepVerdict::Pass),
        "verified absence"
    );
    let mut off = sc(Status::Verified, true, 1);
    off.plan_match = false;
    assert_eq!(pass(&[off], RecordHealth::Ok), Err(Reject::PlanMismatch));
    let mut late = sc(Status::Verified, true, 1);
    late.in_window = false;
    assert_eq!(pass(&[late], RecordHealth::Ok), Err(Reject::OutOfWindow));
    assert_eq!(Reject::EvidenceMissing.code(), ErrorCode::EvidenceMissing);
    assert_eq!(Reject::OutOfWindow.code().exit_code(), 4);

    let fail =
        |checks: &[StepCheck]| judge_step(&Proposed::Fail, StepKind::Assert, checks, &[], RecordHealth::Ok);
    assert_eq!(
        fail(&[sc(Status::Mismatch, true, 1)]),
        Ok(StepVerdict::Fail { unsupported: false })
    );
    assert_eq!(
        fail(&[sc(Status::Verified, true, 1)]),
        Ok(StepVerdict::Fail { unsupported: true })
    );
    assert_eq!(fail(&[]), Ok(StepVerdict::Fail { unsupported: true }));

    let stim = |w: &[WriteState], p: Proposed| judge_step(&p, StepKind::Stimulus, &[], w, RecordHealth::Ok);
    assert_eq!(
        stim(&[WriteState::Confirmed], Proposed::Done),
        Ok(StepVerdict::Done)
    );
    assert_eq!(
        stim(&[WriteState::EffectiveVerified], Proposed::Pass),
        Ok(StepVerdict::Done),
        "stimulus never passes"
    );
    assert_eq!(
        stim(&[WriteState::Sent], Proposed::Done),
        Err(Reject::EvidenceMissing)
    );
    assert_eq!(
        judge_step(&Proposed::Pass, StepKind::Manual, &[], &[], RecordHealth::Ok),
        Err(Reject::EvidenceMissing)
    );
    assert_eq!(
        judge_step(
            &Proposed::Skipped { when_unmet: true },
            StepKind::Assert,
            &[],
            &[],
            RecordHealth::Ok
        ),
        Ok(StepVerdict::Skipped { by_user: false })
    );
    assert_eq!(
        judge_step(
            &Proposed::Skipped { when_unmet: false },
            StepKind::Assert,
            &[],
            &[],
            RecordHealth::Ok
        ),
        Err(Reject::PlanMismatch)
    );
    assert_eq!(
        judge_step(&Proposed::Done, StepKind::Assert, &[], &[], RecordHealth::Ok),
        Err(Reject::EvidenceMissing)
    );
}

fn st(id: &str, required: bool, v: Option<StepVerdict>) -> StepState {
    StepState {
        id: id.into(),
        required,
        verdict: v.map(Final::Auto),
        recovery: vec![],
    }
}

#[test]
fn session_aggregation_and_exit_codes() {
    let agg = |s: &[StepState]| aggregate(s, false).expect("aggregate");
    let r = agg(&[
        st("S1", true, Some(StepVerdict::Done)),
        st("S2", true, Some(StepVerdict::Pass)),
    ]);
    assert_eq!((r.outcome, r.exit_code), (Outcome::Passed, 0));
    let r = agg(&[
        st("S1", true, Some(StepVerdict::Pass)),
        st("S2", true, Some(StepVerdict::Fail { unsupported: false })),
    ]);
    assert_eq!(
        (r.outcome, r.exit_code, r.decisive.clone()),
        (Outcome::Failed, 11, vec!["S2".to_owned()])
    );
    let r = agg(&[st("S1", true, Some(StepVerdict::Inconclusive("gap".into())))]);
    assert_eq!((r.outcome, r.exit_code), (Outcome::Inconclusive, 8));
    let r = agg(&[st("S1", true, Some(StepVerdict::Pass)), st("S2", true, None)]);
    assert_eq!((r.outcome, r.exit_code), (Outcome::Incomplete, 8));
    let r = agg(&[st("S1", true, Some(StepVerdict::Skipped { by_user: true }))]);
    assert_eq!(
        (r.outcome, r.exit_code),
        (Outcome::Inconclusive, 8),
        "user skip of a required step"
    );
    let r = agg(&[
        st("S1", true, Some(StepVerdict::Skipped { by_user: false })),
        st("S2", false, None),
    ]);
    assert_eq!(r.outcome, Outcome::Passed, "plan branch skip + optional step");
    let r = agg(&[
        st("S1", true, Some(StepVerdict::Blocked("no device".into()))),
        st("S2", true, Some(StepVerdict::Inconclusive("x".into()))),
    ]);
    assert_eq!(
        (r.outcome, r.exit_code, r.decisive.clone()),
        (Outcome::Inconclusive, 8, vec!["S1".to_owned()])
    );
    // on_fail goto: replaced only when every recovery step passed; the failure stays reported.
    let mut failed = st("S3", true, Some(StepVerdict::Fail { unsupported: false }));
    failed.recovery = vec!["R1".into()];
    let r = agg(&[failed.clone(), st("R1", true, Some(StepVerdict::Pass))]);
    assert_eq!(
        (r.outcome, r.recovered_failures.clone()),
        (Outcome::Passed, vec!["S3".to_owned()])
    );
    let r = agg(&[
        failed,
        st("R1", true, Some(StepVerdict::Inconclusive("x".into()))),
    ]);
    assert_eq!(r.outcome, Outcome::Failed);
    // User verdicts are their own type and count.
    let user = StepState {
        id: "M1".into(),
        required: true,
        verdict: Some(Final::User(UserVerdict {
            pass: true,
            reason: "LED on".into(),
            overrides: None,
        })),
        recovery: vec![],
    };
    assert_eq!(agg(&[user]).outcome, Outcome::Passed);
    assert_eq!(
        aggregate(&[st("S1", true, Some(StepVerdict::Pass))], true)
            .expect("agg")
            .exit_code,
        6,
        "user STOP"
    );
    assert!(aggregate(&vec![st("S", true, None); 501], false).is_err());
}
