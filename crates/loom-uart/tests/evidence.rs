//! `uart.text_contains` evidence verifier on `uart.data` records shaped like the worker's
//! (`bytes{encoding:"base64"}`, `rx_offset` decimal string, origin device for RX).

use loom_domain_api::{Coverage, CoverageNeed, EvalBudget, Predicate, Registry, Verification};
use loom_proto::event::{PayloadBytes, PublishedRecord, SeqRange};
use loom_proto::num::U64;
use loom_testkit::RegistryProbe;
use loom_uart::Uart;
use loom_uart::evidence::PRED_TEXT_CONTAINS;
use serde_json::{Value, json};

fn registry() -> Registry {
    RegistryProbe::assert_ok(&Uart::new()).0
}

#[derive(Clone, Copy)]
struct Src {
    ch: &'static str,
    generation: u32,
    producer_epoch: u64,
}

const DUT: Src = Src {
    ch: "dut",
    generation: 1,
    producer_epoch: 1,
};

fn base(seq: u64, kind: &str, s: Src) -> Value {
    json!({
        "schema_version": 1, "session_id": "s", "domain": "uart", "channel": s.ch, "kind": kind,
        "seq": seq.to_string(), "epoch": "2", "producer_id": format!("uart.{}", s.ch),
        "producer_epoch": s.producer_epoch.to_string(), "source_seq": seq.to_string(),
        "ts": {"mono_ns": (seq * 1000).to_string(), "wall_ns": "0", "clock_id": "c", "source": "driver"},
        "origin": "device", "generation": s.generation, "payload": {},
    })
}

fn rec(v: Value) -> PublishedRecord {
    serde_json::from_value(v).expect("record")
}

fn rx(seq: u64, off: u64, text: &[u8], s: Src) -> PublishedRecord {
    let mut v = base(seq, "uart.data", s);
    v["dir"] = json!("rx");
    v["payload"] =
        json!({"bytes": PayloadBytes::base64(text), "len": text.len(), "rx_offset": off.to_string()});
    rec(v)
}

fn tx(seq: u64, text: &[u8]) -> PublishedRecord {
    let mut v = base(seq, "uart.data", DUT);
    v["dir"] = json!("tx");
    v["origin"] = json!("agent");
    v["payload"] = json!({"bytes": PayloadBytes::base64(text), "len": text.len()});
    rec(v)
}

fn gap(seq: u64) -> PublishedRecord {
    let mut v = base(seq, "source.gap", DUT);
    v["domain"] = json!("source");
    v.as_object_mut().expect("obj").remove("channel");
    rec(v)
}

/// TC-1 S2 as the TEST wire passes it.
fn tc1() -> Predicate {
    pred(json!({"pattern": "login:", "ch": "dut",
                "filter": {"domain": "uart", "kinds": ["uart.data"], "channel": "dut", "origins": ["device"]}}))
}

fn pred(args: Value) -> Predicate {
    Predicate {
        name: PRED_TEXT_CONTAINS.into(),
        args,
        expect_id: Some("S2#0".into()),
    }
}

fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: U64(0),
            to: U64(0),
        },
        complete,
        causes: if complete {
            vec![]
        } else {
            vec!["gap_in_window".into()]
        },
    }
}

fn check(reg: &Registry, p: &Predicate, w: &[PublishedRecord], complete: bool) -> Verification {
    let v = reg
        .evidence_verifiers()
        .iter()
        .find(|v| v.predicates().contains(&PRED_TEXT_CONTAINS))
        .expect("uart verifier registered")
        .clone();
    v.verify(p, w, &cov(complete), &mut EvalBudget::default())
}

fn seqs(v: &Verification) -> Vec<u64> {
    match v {
        Verification::Match { matched, .. } => matched.iter().map(|m| m.seq.get()).collect(),
        other => panic!("expected Match, got {other:?}"),
    }
}

fn reason(v: &Verification) -> String {
    match v {
        Verification::Inconclusive { reason } | Verification::Mismatch { reason, .. } => reason.clone(),
        Verification::Match { .. } => String::new(),
    }
}

#[test]
fn registered_positive_predicate() {
    let reg = registry();
    let v = &reg.evidence_verifiers()[0];
    assert_eq!(v.predicates(), [PRED_TEXT_CONTAINS]);
    assert_eq!(v.coverage_need(&tc1()), CoverageNeed::Positive);
}

#[test]
fn match_joins_contiguous_rx_chunks() {
    let reg = registry();
    let w = [
        rx(10, 100, b"boot ok\r\nlog", DUT),
        tx(11, b"login: (typed by agent)"),
        rx(12, 112, b"in: ", DUT),
    ];
    let r = check(&reg, &tc1(), &w, true);
    assert_eq!(seqs(&r), [10, 12]);
    let Verification::Match { observed, .. } = &r else {
        unreachable!()
    };
    assert_eq!(observed["rx_offset_start"], "109");
    assert_eq!(observed["rx_offset_end"], "115");
    assert!(
        !observed.to_string().contains("login") && !observed.to_string().contains("boot"),
        "observed carries no RX text: {observed}"
    );
    // A positive match stands over an incomplete window.
    assert_eq!(seqs(&check(&reg, &tc1(), &w, false)), [10, 12]);
    // Single-key form with `literal`.
    let nested =
        pred(json!({"uart.text_contains": {"literal": "login:"}, "filter": {"domain": "uart", "kinds": []}}));
    assert_eq!(seqs(&check(&reg, &nested, &w, true)), [10, 12]);
}

#[test]
fn no_join_across_loss_or_identity_change() {
    let reg = registry();
    // rx_offset jump: bytes missing between the chunks.
    let w = [rx(1, 0, b"log", DUT), rx(2, 10, b"in:", DUT)];
    let r = check(&reg, &tc1(), &w, true);
    assert!(reason(&r).contains("rx_offset jump"), "{r:?}");
    // Gap record between.
    let w = [rx(1, 0, b"log", DUT), gap(2), rx(3, 3, b"in:", DUT)];
    let r = check(&reg, &tc1(), &w, true);
    assert!(reason(&r).contains("source.gap"), "{r:?}");
    // Reconnect (new generation) and worker restart (new producer epoch): no join, no loss.
    for s in [
        Src { generation: 2, ..DUT },
        Src {
            producer_epoch: 2,
            ..DUT
        },
    ] {
        let w = [rx(1, 0, b"log", DUT), rx(2, 3, b"in:", s)];
        assert!(matches!(
            check(&reg, &tc1(), &w, true),
            Verification::Mismatch { .. }
        ));
    }
    // Port state change in between.
    let w = [
        rx(1, 0, b"log", DUT),
        rec(base(2, "uart.state", DUT)),
        rx(3, 3, b"in:", DUT),
    ];
    assert!(matches!(
        check(&reg, &tc1(), &w, true),
        Verification::Mismatch { .. }
    ));
}

#[test]
fn only_device_rx_of_the_channel_counts() {
    let reg = registry();
    let other = Src { ch: "aux", ..DUT };
    let mut agent = rx(3, 0, b"login:", DUT);
    agent.payload["attributed_origin"] = json!("agent");
    let w = [tx(1, b"login:"), rx(2, 0, b"login:", other), agent];
    let r = check(&reg, &tc1(), &w, true);
    let Verification::Mismatch { observed, .. } = &r else {
        panic!("{r:?}")
    };
    assert_eq!(
        (
            observed["tx_skipped"].as_u64(),
            observed["origin_skipped"].as_u64()
        ),
        (Some(1), Some(1))
    );
    // An empty window never passes.
    assert!(matches!(
        check(&reg, &tc1(), &[], true),
        Verification::Mismatch { .. }
    ));
}

#[test]
fn a_miss_is_mismatch_only_when_provably_complete() {
    let reg = registry();
    let w = [rx(1, 0, b"no prompt here", DUT)];
    assert!(matches!(
        check(&reg, &tc1(), &w, true),
        Verification::Mismatch { .. }
    ));
    assert!(matches!(
        check(&reg, &tc1(), &w, false),
        Verification::Inconclusive { .. }
    ));
    // Escape sequences inside the literal: raw search cannot decide.
    let w = [rx(1, 0, b"log\x1b[0min:", DUT)];
    let r = check(&reg, &tc1(), &w, true);
    assert!(reason(&r).contains("escape"), "{r:?}");
    // Bytes not inline (raw_ref) are a loss.
    let mut v = rx(1, 0, b"", DUT);
    v.payload["bytes"] =
        json!({"encoding": "raw_ref", "ref": {"id": "x", "sha256": "00", "length": "6", "format": "raw"}});
    let r = check(&reg, &tc1(), &[v], true);
    assert!(reason(&r).contains("unreadable"), "{r:?}");
}

#[test]
fn budget_and_bad_args() {
    let reg = registry();
    let w: Vec<PublishedRecord> = (0..600).map(|i| rx(i, i, b"x", DUT)).collect();
    let v = reg.evidence_verifiers()[0].clone();
    let r = v.verify(
        &tc1(),
        &w,
        &cov(true),
        &mut EvalBudget::new(300, std::time::Duration::from_secs(5)),
    );
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
    for bad in [
        json!({"filter": {}}),
        json!({"pattern": ""}),
        json!({"pattern": "a", "literal": "b"}),
        json!({"regex": "log.n"}),
        json!({"pattern": "a", "x": 1}),
        json!({"pattern": "a", "filter": {"domain": "uart", "kinds": ["uart.write"]}}),
        json!({"pattern": "a", "filter": {"domain": "can"}}),
    ] {
        let r = check(&reg, &pred(bad.clone()), &w, true);
        assert!(reason(&r).starts_with("error:"), "{bad} → {r:?}");
    }
}

/// 4 KiB needle `a…ab` over ~3.8 MB of `a`: the old window search was O(N·M) (~15 G byte
/// compares) with no time check inside the 64 chunks; KMP is linear.
fn long_needle_window(chunks: u64, tail_b: bool) -> (Predicate, Vec<PublishedRecord>) {
    const CHUNK: usize = 60_000;
    let mut needle = "a".repeat(loom_uart::evidence::MAX_PATTERN_BYTES - 1);
    needle.push('b');
    let p = pred(
        json!({"pattern": needle, "filter": {"domain": "uart", "kinds": ["uart.data"], "channel": "dut"}}),
    );
    let body = vec![b'a'; CHUNK];
    let mut w: Vec<PublishedRecord> = (0..chunks)
        .map(|k| rx(k + 1, k * CHUNK as u64, &body, DUT))
        .collect();
    if tail_b {
        w.push(rx(chunks + 1, chunks * CHUNK as u64, b"b", DUT));
    }
    (p, w)
}

#[test]
fn long_repeated_needle_is_linear() {
    let reg = registry();
    let v = reg.evidence_verifiers()[0].clone();
    let roomy = || EvalBudget::new(EvalBudget::DEFAULT_EVENTS, std::time::Duration::from_secs(30));
    let (p, w) = long_needle_window(64, false);
    let t = std::time::Instant::now();
    let r = v.verify(&p, &w, &cov(true), &mut roomy());
    assert!(matches!(r, Verification::Mismatch { .. }), "{r:?}");
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
    // The match closing in the next chunk joins both and reports exact offsets.
    let (p, w) = long_needle_window(64, true);
    let r = v.verify(&p, &w, &cov(true), &mut roomy());
    assert_eq!(seqs(&r), [64, 65]);
    let Verification::Match { observed, .. } = &r else {
        unreachable!()
    };
    let end = 64 * 60_000 + 1;
    assert_eq!(observed["rx_offset_end"], end.to_string());
    assert_eq!(observed["rx_offset_start"], (end - 4096).to_string());
}

#[test]
fn time_budget_is_checked_inside_long_chunks() {
    let reg = registry();
    let v = reg.evidence_verifiers()[0].clone();
    let (p, w) = long_needle_window(64, false);
    // Already expired: the first check fails.
    let mut zero = EvalBudget::new(EvalBudget::DEFAULT_EVENTS, std::time::Duration::ZERO);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let r = v.verify(&p, &w, &cov(true), &mut zero);
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
    // 1 ms over 65 events (< one 256-event charge): only the in-chunk checks can stop it.
    let mut short = EvalBudget::new(EvalBudget::DEFAULT_EVENTS, std::time::Duration::from_millis(1));
    let r = v.verify(&p, &w, &cov(true), &mut short);
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
}
