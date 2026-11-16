//! operator-upgrade B1 — ETH-01 regression and range-scope contract (FUNCTIONS.md F1.*).
//!
//! Reproduces the ROOT observation: a stopped channel's packet 40 minutes old was reported as
//! `complete:true, matched:0` by a default (`from`-less) query, while an explicit range found it.

mod support;

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::Registry;
use loom_eth::gateway::Eth;
use loom_proto::rpc::SubFilter;
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

use support::*;

const MIN: u64 = 60_000_000_000;

/// Registry whose reducer saw these capture_state records `(seq, ch, state, generation)`.
fn registry_seen(
    states: &[(u64, &str, &str, u32)],
    now: u64,
) -> (Registry, Vec<Box<dyn loom_domain_api::Reducer>>) {
    let mut reg = RegistryProbe::assert_ok(&Eth).0;
    let mut reducers = reg.take_reducers();
    {
        let r = reducers.first_mut().expect("eth reducer");
        for (seq, ch, state, generation) in states {
            let mut ev = record(
                *seq,
                ch,
                "eth.capture_state",
                now - 60 * MIN,
                json!({"ch": ch, "state": state, "backend": "sim", "iface": "sim", "started_mono_ns": "1000"}),
            );
            ev.generation = Some(*generation);
            r.apply(&ev);
        }
    }
    (reg, reducers)
}

/// `qa`: capture started (seq 1, 60 min ago), target UDP packet 40 min ago (seq 1380), noise 1 min ago.
fn qa_journal(now: u64) -> Arc<Services> {
    let s = Arc::new(Services::default());
    let target = udp4(TESTER, ECU, 40000, 65201, b"probe");
    let noise = udp4(ECU, TESTER, 30501, 30501, &[0; 8]);
    let mut start = record(
        1,
        "qa",
        "eth.capture_state",
        now - 60 * MIN,
        json!({"ch": "qa", "state": "starting"}),
    );
    start.generation = Some(1);
    *s.journal.events.lock().expect("lock") = vec![
        start,
        pkt(1380, "qa", now - 40 * MIN, &target, None),
        pkt(2000, "qa", now - MIN, &noise, None),
    ];
    *s.journal.relative_now_wall.lock().expect("lock") = Some(now);
    s
}

fn reasons(v: &Value) -> Vec<&str> {
    v["partial_reasons"]
        .as_array()
        .expect("partial_reasons array")
        .iter()
        .map(|x| x.as_str().expect("reason string"))
        .collect()
}

#[test]
fn eth01_default_query_is_partial_lookback_and_explicit_capture_range_finds_the_packet() {
    let now = wall_now();
    let (reg, _r) = registry_seen(&[(1, "qa", "starting", 1), (2100, "qa", "stopped", 1)], now);
    let svc = qa_journal(now);
    let q = |p: Value| call(&reg, &svc, "eth.query", p, Duration::from_secs(5)).expect("query");

    // Before: `complete:true, matched:0` (false negative). Now: honest partial look-back.
    let r = q(json!({"ch": "qa", "filter": "udp.dstport == 65201"}));
    assert_eq!(r["matched"], 0);
    assert_eq!(
        r["complete"], false,
        "default look-back is never complete history"
    );
    assert_eq!(
        r["scan_complete"], true,
        "the 600 s window itself was fully scanned (coverage only)"
    );
    assert_eq!(reasons(&r), ["default_lookback"]);
    assert_eq!(
        r["scope"],
        json!({"from": "-600s", "to": null, "from_source": "default_lookback", "lookback_secs": 600})
    );

    // The capture start anchor observed by the reducer is published by eth.capture.status.
    let st = call(
        &reg,
        &svc,
        "eth.capture.status",
        json!({}),
        Duration::from_secs(5),
    )
    .expect("status");
    let qa = st["channels"]
        .as_array()
        .expect("channels")
        .iter()
        .find(|c| c["ch"] == "qa")
        .expect("qa listed");
    assert_eq!(qa["start_cursor"], "e1:seq:1");
    assert_eq!(qa["start_seq"], "1");

    // Explicit range from that anchor: the packet is found and the result is complete.
    let r = q(json!({"ch": "qa", "filter": "udp.dstport == 65201", "range": {"from": "e1:seq:1"}}));
    assert_eq!(r["matched"], 1);
    assert_eq!(r["rows"][0]["ref"], "eth:qa@seq:1380");
    assert_eq!(r["complete"], true);
    assert_eq!(r["scan_complete"], true);
    assert_eq!(reasons(&r), Vec::<&str>::new());
    assert_eq!(r["scope"]["from"], "e1:seq:1");
    assert_eq!(r["scope"]["from_source"], "explicit");
    assert_eq!(r["scope"]["lookback_secs"], Value::Null);
    assert_eq!(r["scanned"]["first_seq"], "1380");
    assert_eq!(r["scanned"]["last_seq"], "2000");
    assert_eq!(r["resume_cursor"], Value::Null);
    // Existing fields keep their meaning.
    assert_eq!(r["truncated"], false);
    assert_eq!(r["range"], json!({"from": "1380", "to": "1380"}));
}

#[test]
fn eth01_row_cap_and_loss_clear_complete_but_not_scan_coverage() {
    let now = wall_now();
    let (reg, _r) = registry_seen(&[], now);
    let s = Arc::new(Services::default());
    let mut ev: Vec<_> = (1..=6)
        .map(|i| pkt(i, "dut", now - 10 * MIN + i, &doip_uds(&[0x22, 0xF1, 0x90]), None))
        .collect();
    ev.insert(
        3,
        record(
            100,
            "dut",
            "source.gap",
            now - 10 * MIN + 3,
            json!({"cause": "kernel_drop", "count": "4", "seq_range": null}),
        ),
    );
    *s.journal.events.lock().expect("lock") = ev;
    let r = call(
        &reg,
        &s,
        "eth.query",
        json!({"ch": "dut", "filter": "uds", "range": {"from": "seq:1"}, "max": 2}),
        Duration::from_secs(5),
    )
    .expect("query");
    assert_eq!(r["matched"], 6);
    assert_eq!(r["truncated"], true);
    assert_eq!(reasons(&r), ["loss", "row_cap"]);
    assert_eq!(
        r["complete"], false,
        "loss / row cap must never read as complete evidence"
    );
    assert_eq!(r["scan_complete"], true, "the explicit range was fully scanned");
    assert_eq!(r["prev_cursor"], "seq:4");
}

#[test]
fn eth01_stats_follow_export_state_their_scope() {
    let now = wall_now();
    let (reg, _r) = registry_seen(&[(1, "qa", "starting", 1)], now);
    let svc = qa_journal(now);
    let c = |m: &str, p: Value| call(&reg, &svc, m, p, Duration::from_secs(5)).expect(m);

    for (m, p) in [
        ("eth.stats", json!({"ch": "qa"})),
        ("eth.follow", json!({"ch": "qa", "proto": "udp", "stream": 0})),
        ("eth.export", json!({"ch": "qa", "format": "pcapng"})),
    ] {
        let r = c(m, p);
        assert_eq!(r["complete"], false, "{m}");
        assert_eq!(r["scan_complete"], true, "{m}");
        assert_eq!(reasons(&r), ["default_lookback"], "{m}");
        assert_eq!(r["scope"]["from_source"], "default_lookback", "{m}");
        if let Some(path) = r["path"].as_str() {
            let _ = std::fs::remove_file(path);
        }
    }

    let r = c(
        "eth.export",
        json!({"ch": "qa", "format": "pcapng", "filter": "udp.dstport == 65201", "range": {"from": "e1:seq:1"}}),
    );
    assert_eq!(r["packets"], 1);
    assert_eq!(r["complete"], true);
    assert_eq!(r["scan_complete"], true);
    assert_eq!(reasons(&r), Vec::<&str>::new());
    assert_eq!(r["filter"], "udp.dstport == 65201");
    assert_eq!(r["scope"]["from"], "e1:seq:1");
    assert_eq!(r["scanned"]["first_seq"], "1380");
    let _ = std::fs::remove_file(r["path"].as_str().expect("path"));

    // An export without a filter states `filter: null`.
    let r = c(
        "eth.export",
        json!({"ch": "qa", "format": "pcapng", "range": {"from": "seq:1", "to": "seq:1380"}}),
    );
    assert_eq!(r["filter"], Value::Null);
    assert_eq!(r["packets"], 1);
    assert_eq!(r["scope"]["to"], "seq:1380");
    let _ = std::fs::remove_file(r["path"].as_str().expect("path"));
}

#[test]
fn eth01_start_anchor_in_status_and_snapshot_is_null_when_not_observed() {
    let now = wall_now();
    // `mid` joined mid-generation (first record `live`); `qa` was seen starting at seq 5.
    let (reg, reducers) = registry_seen(&[(4, "mid", "live", 2), (5, "qa", "starting", 7)], now);
    let svc = Arc::new(Services::default());
    let st = call(
        &reg,
        &svc,
        "eth.capture.status",
        json!({}),
        Duration::from_secs(5),
    )
    .expect("status");
    let by = |ch: &str| -> Value {
        st["channels"]
            .as_array()
            .expect("channels")
            .iter()
            .find(|c| c["ch"] == ch)
            .cloned()
            .expect("listed")
    };
    assert_eq!(by("mid")["start_cursor"], Value::Null);
    assert_eq!(by("mid")["start_seq"], Value::Null);
    assert_eq!(by("qa")["start_cursor"], "e1:seq:5");
    // Existing fields unchanged.
    assert_eq!(by("qa")["generation"], 7);
    assert_eq!(by("qa")["backend"], "sim");

    let snap = reducers[0].snapshot(&SubFilter::default());
    let item = |ch: &str| {
        snap.iter()
            .find(|v| v["ch"] == ch)
            .cloned()
            .expect("snapshot item")
    };
    assert_eq!(item("qa")["start_cursor"], "e1:seq:5");
    assert!(
        item("mid").get("start_cursor").is_none(),
        "unknown anchor is omitted, never invented"
    );
}
