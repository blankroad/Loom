//! Gateway pieces without a daemon: ASCII wait proof across losses, snapshot and read bounds.
#![allow(clippy::unwrap_used)]

use loom_domain_api::Reducer as _;
use loom_logic::gateway::{LogicReducer, Matcher, ROW_BUDGET, SNAP_EDGE_BYTES, bound, rows};
use loom_logic::model::{EdgeAcc, MAX_DURATION_MS, MAX_EDGES_PER_EVENT, MAX_SAMPLERATE};
use loom_logic::{KIND_ANNOTATIONS, KIND_EDGES};
use loom_proto::event::{Origin, PublishedRecord, SourceRecord, TsSource};
use loom_proto::num::U64;
use loom_proto::rpc::SubFilter;
use loom_worker_sdk::{Clock as _, SimClock};
use serde_json::{Value, json};

fn rec(seq: u64, kind: &str, payload: Value) -> PublishedRecord {
    let src: SourceRecord = serde_json::from_value(json!({
        "schema_version": 1, "session_id": "s", "domain": "logic", "channel": "lc-1", "kind": kind,
        "producer_id": "p", "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": serde_json::to_value(SimClock::new(1).ts(TsSource::HostReadComplete)).unwrap(),
        "origin": serde_json::to_value(Origin::Device).unwrap(), "payload": payload,
    }))
    .unwrap();
    src.publish(U64(seq), U64(1))
}

fn ann(text: &str, gap: Option<u64>) -> Value {
    let mut a = json!({"decoder": "uart-1", "class": null, "start_sample": 1, "end_sample": 2,
                       "t_start_ns": 1, "t_end_ns": 2, "text": text});
    if let Some(g) = gap {
        a["gap_before"] = json!(g);
    }
    a
}

fn batch(seq: u64, anns: Vec<Value>, dropped: u64) -> PublishedRecord {
    rec(
        seq,
        KIND_ANNOTATIONS,
        json!({"capture_id": "lc-1", "samplerate": 1000, "annotations": anns, "dropped": dropped}),
    )
}

fn run(m: &mut Matcher<'_>, evs: &[PublishedRecord]) -> bool {
    rows(evs).iter().any(|r| m.push(r))
}

#[test]
fn ascii_match_needs_lossless_run() {
    let want = b"LOOM".as_slice();
    // Lossless across batches → match.
    let mut m = Matcher::new(None, Some(want), None);
    assert!(run(
        &mut m,
        &[
            batch(1, vec![ann("4C", None)], 0),
            batch(2, vec![ann("4F", None), ann("4F", None), ann("4D", None)], 0)
        ]
    ));
    // A hole attributed inside the run → no false contiguity.
    let mut m = Matcher::new(None, Some(want), None);
    assert!(!run(
        &mut m,
        &[
            batch(1, vec![ann("4C", None)], 0),
            batch(2, vec![ann("4F", Some(1)), ann("4F", None), ann("4D", None)], 1)
        ]
    ));
    // Journal gap between scans → reset.
    let mut m = Matcher::new(None, Some(want), None);
    assert!(!run(&mut m, &[batch(1, vec![ann("4C", None)], 0)]));
    m.reset();
    assert!(!run(
        &mut m,
        &[batch(
            2,
            vec![ann("4F", None), ann("4F", None), ann("4D", None)],
            0
        )]
    ));
    // Text match is exact (case-insensitive), decoder filter by instance prefix.
    let mut m = Matcher::new(Some("4c"), None, Some("uart"));
    assert!(run(&mut m, &[batch(1, vec![ann("4C", None)], 0)]));
    let mut m = Matcher::new(Some("4C"), None, Some("spi"));
    assert!(!run(&mut m, &[batch(1, vec![ann("4C", None)], 0)]));
}

#[test]
fn snapshot_is_byte_bounded() {
    let mut r = LogicReducer::default();
    let big = "\u{1}".repeat(4096);
    for i in 0..300 {
        r.apply(&batch(i, vec![ann(&big, None)], 0));
        let edges: Vec<Value> = (0..2048u64)
            .map(|k| json!([36_000_000_000u64 + k, 255]))
            .collect();
        r.apply(&rec(
            1000 + i,
            KIND_EDGES,
            json!({"capture_id": "lc-1", "sample_start": 0, "sample_end": 1, "initial": 0, "edges": edges}),
        ));
        r.apply(&batch(2000 + i, vec![ann("4C", None)], 0));
    }
    let snap = r.snapshot(&SubFilter::default());
    let n = serde_json::to_string(&snap).unwrap().len();
    assert!(n < 64 * 1024, "snapshot {n} bytes");
    assert!(snap[0]["evicted"]["annotations"].as_u64().unwrap() > 0);
    assert!(!snap[0]["annotations"].as_array().unwrap().is_empty());
}

#[test]
fn read_rows_leave_envelope_headroom() {
    let big = "x".repeat(3000);
    let all: Vec<Value> = (0..200)
        .map(|i| json!({"type": "annotation", "seq": i, "text": big}))
        .collect();
    let (kept, left) = bound(all, 1000);
    let n = serde_json::to_string(&kept).unwrap().len();
    assert!(n <= ROW_BUDGET && left > 0, "{n} {left}");
    let wrapped =
        json!({"rows": kept, "next": "seq:18446744073709551615", "complete": false, "rows_left_out": left});
    assert!(serde_json::to_string(&wrapped).unwrap().len() < 60 * 1024);
}

#[test]
fn producer_edges_cap_fits_snapshot_budget() {
    // Worst producer payload: 11-digit sample indices (10 MS/s × 1 h), all 8 bits toggling.
    let mut e = EdgeAcc::new(0xFF);
    e.sample = MAX_SAMPLERATE * (MAX_DURATION_MS / 1000) - 10_000;
    let data: Vec<u8> = (0..4_000u32)
        .map(|i| if i % 2 == 0 { 0xFF } else { 0x00 })
        .collect();
    e.feed(&data);
    let (p, n, _) = e
        .take("lc-ffffffffffff-18446744073709551615", u64::MAX, MAX_SAMPLERATE)
        .unwrap();
    assert_eq!(n as usize, MAX_EDGES_PER_EVENT);
    let bytes = serde_json::to_string(&p).unwrap().len() + 1;
    assert!(bytes <= SNAP_EDGE_BYTES, "{bytes} > {SNAP_EDGE_BYTES}");
    // …so the reducer keeps it instead of discarding the whole raw chunk.
    let mut r = LogicReducer::default();
    r.apply(&rec(1, KIND_EDGES, p));
    let snap = r.snapshot(&SubFilter::default());
    assert_eq!(snap[0]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(snap[0]["evicted"]["edges"], 0);
}
