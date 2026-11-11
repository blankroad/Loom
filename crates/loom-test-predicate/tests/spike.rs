//! The 10 cases of `spikes/explore/v2-evidence` (CEL and JSONPath gave the same answers),
//! ported to the comparison AST. The spike's per-event `epoch` is the device generation here
//! (a stream-epoch change is a coverage problem, tested in `table.rs`).

mod support;

use loom_proto::event::PublishedRecord;
use loom_test_predicate::Status;
use serde_json::{Value, json};
use support::*;

fn fixture() -> Vec<Value> {
    include_str!("fixtures/spike-events.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).expect("fixture line"))
        .collect()
}

fn to_records(evs: &[Value], lo: u64, hi: u64) -> Vec<PublishedRecord> {
    evs.iter()
        .filter(|e| (lo..=hi).contains(&e["seq"].as_u64().expect("seq")))
        .map(|e| {
            let kind = match e["kind"].as_str().expect("kind") {
                "source.gap" => "source.gap".to_owned(),
                k => format!("{}.{k}", e["domain"].as_str().expect("domain")),
            };
            let domain = if kind == "source.gap" {
                "source"
            } else {
                e["domain"].as_str().expect("domain")
            };
            r(
                e["seq"].as_u64().expect("seq"),
                e["mono_ms"].as_u64().expect("t"),
                domain,
                e["source"].as_str().expect("src"),
                &kind,
                e["payload"].clone(),
            )
            .generation(u32::try_from(e["epoch"].as_u64().expect("epoch")).expect("small"))
            .build()
        })
        .collect()
}

fn exists() -> loom_test_predicate::Citation {
    cite(json!({"filter": {"domain": "uds", "kinds": ["uds.response"]},
                "pred": {"exists": {"where": {"all": [
                    {"field": "/payload/bytes/0", "op": "==", "value": 98},
                    {"field": "/payload/bytes/1", "op": "==", "value": 241}]}}}}))
}

fn hold(ms: u64) -> loom_test_predicate::Citation {
    cite(
        json!({"filter": {"domain": "can", "channel": "can0", "kinds": ["can.signal"]},
                "pred": {"hold": {"sample": {"field": "/payload/signal", "op": "==", "value": "VehicleSpeed"},
                                  "where": {"field": "/payload/value", "op": "between", "value": [10.0, 15.0]},
                                  "min_ms": ms}}}),
    )
}

fn seqs(c: &loom_test_predicate::Check) -> Vec<u64> {
    c.matched.iter().map(|m| m.seq.get()).collect()
}

#[test]
fn spike_exists_cases() {
    let ev = fixture();
    // 1. positive in 10..15 → witness 12
    let c = eval(&exists(), &to_records(&ev, 10, 15), true);
    assert_eq!((c.status, seqs(&c)), (Status::Verified, vec![12]));
    // 2. outside 13..18 → not found
    assert_eq!(
        eval(&exists(), &to_records(&ev, 13, 18), true).status,
        Status::NotFound
    );
    // 3. inclusive boundary 12..12
    let c = eval(&exists(), &to_records(&ev, 12, 12), true);
    assert_eq!((c.status, seqs(&c)), (Status::Verified, vec![12]));
}

#[test]
fn spike_hold_cases() {
    let ev = fixture();
    // 4. held 300 ms in 1..24 → evidence seq 4..13
    let c = eval(&hold(300), &to_records(&ev, 1, 24), true);
    assert_eq!(c.status, Status::Verified);
    assert_eq!(
        (seqs(&c).first().copied(), seqs(&c).last().copied()),
        (Some(4), Some(13))
    );
    // 5. 800 ms too short → fail (mismatch)
    let c = eval(&hold(800), &to_records(&ev, 1, 24), true);
    assert_eq!(
        (c.status, c.cause.as_deref()),
        (Status::Mismatch, Some("hold_too_short"))
    );
    // 6. source.gap in 31..42 → inconclusive
    assert_eq!(
        eval(&hold(300), &to_records(&ev, 31, 42), true).status,
        Status::Inconclusive
    );
    // 7. non-numeric value at seq 43 → error naming the seq
    let c = eval(&hold(200), &to_records(&ev, 43, 50), true);
    assert_eq!(c.status, Status::Error);
    assert!(
        c.cause.as_deref().is_some_and(|w| w.contains("seq=43")),
        "{:?}",
        c.cause
    );
    // 8. an out-of-range sample (seq 25) cannot be bridged
    assert_eq!(
        eval(&hold(800), &to_records(&ev, 1, 30), true).status,
        Status::Mismatch
    );
    // 9. generation change inside the run → inconclusive
    let mut g = ev.clone();
    g[9]["epoch"] = json!(2);
    let c = eval(&hold(300), &to_records(&g, 1, 13), true);
    assert_eq!(
        (c.status, c.cause.as_deref()),
        (Status::Inconclusive, Some("generation_changed"))
    );
    // 10. stale sample (interval 151 ms > 150) → inconclusive
    let mut s = ev.clone();
    s[9]["mono_ms"] = json!(351);
    let c = eval(&hold(300), &to_records(&s, 1, 13), true);
    assert_eq!(
        (c.status, c.cause.as_deref()),
        (Status::Inconclusive, Some("stale_sample"))
    );
}
