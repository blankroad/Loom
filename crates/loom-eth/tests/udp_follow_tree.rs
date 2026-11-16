//! operator-upgrade B3a — ETH-03 UDP follow / `udp.stream` filter on worker-shaped records, and
//! ETH-02 daemon tree `filter` expressions (FUNCTIONS.md F3.4–F3.7).

mod support;

use std::time::Duration;

use loom_eth::gateway::Eth;
use loom_eth::gateway::handlers::{filter_check, filter_expr};
use loom_eth_decode::FieldValue;
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

use support::*;

/// A record as the B3a worker publishes it: `stream` + `stream_proto` (None = legacy record).
fn rec(seq: u64, frame: &[u8], stream: Option<(u64, &str)>) -> loom_proto::event::PublishedRecord {
    let mut r = pkt(seq, "qa", wall_now() - 60_000_000_000 + seq, frame, None);
    if let Some((n, p)) = stream {
        r.payload["stream"] = json!(n);
        r.payload["stream_proto"] = json!(p);
    }
    r
}

fn journal() -> std::sync::Arc<Services> {
    let s = std::sync::Arc::new(Services::default());
    *s.journal.events.lock().expect("lock") = vec![
        rec(1, &udp4(TESTER, ECU, 40000, 65201, b"ping"), Some((0, "udp"))),
        rec(2, &udp4(ECU, TESTER, 65201, 40000, b"pong"), Some((0, "udp"))),
        rec(3, &udp4(TESTER, ECU, 40001, 65201, b"other"), Some((1, "udp"))),
        // Synthetic: index 0 of the TCP space on a UDP frame — must never join UDP stream 0.
        rec(
            4,
            &udp4(TESTER, ECU, 40000, 65201, b"tcp-space"),
            Some((0, "tcp")),
        ),
        // Legacy record (pre-B3a): no stream → no conversation identity at all.
        rec(5, &udp4(TESTER, ECU, 40000, 65201, b"legacy"), None),
    ];
    s
}

fn refs(v: &Value) -> Vec<String> {
    v["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["ref"].as_str().expect("ref").to_owned())
        .collect()
}

#[test]
fn eth03_udp_follow_joins_both_directions_of_one_conversation_only() {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    let svc = journal();
    let r = call(
        &reg,
        &svc,
        "eth.follow",
        json!({"ch": "qa", "proto": "udp", "stream": 0, "range": {"from": "seq:1"}}),
        Duration::from_secs(5),
    )
    .expect("follow");
    let segs: Vec<(String, String)> = r["segments"]
        .as_array()
        .expect("segments")
        .iter()
        .map(|s| {
            (
                s["dir"].as_str().unwrap_or("").into(),
                s["data"].as_str().unwrap_or("").into(),
            )
        })
        .collect();
    assert_eq!(segs, [("c2s".into(), hex(b"ping")), ("s2c".into(), hex(b"pong"))]);
    assert_eq!(r["bytes"], 8);
    assert_eq!(r["complete"], true);
    // tcp follow of stream 0 must not pick up the UDP records.
    let t = call(
        &reg,
        &svc,
        "eth.follow",
        json!({"ch": "qa", "proto": "tcp", "stream": 0, "range": {"from": "seq:1"}}),
        Duration::from_secs(5),
    )
    .expect("tcp follow");
    assert_eq!(t["segments"], json!([]));
}

#[test]
fn eth03_udp_stream_filter_uses_the_worker_number_and_rows_carry_stream_proto() {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    let svc = journal();
    let q = |f: &str| {
        call(
            &reg,
            &svc,
            "eth.query",
            json!({"ch": "qa", "filter": f, "range": {"from": "seq:1"}, "max": 100}),
            Duration::from_secs(5),
        )
        .expect("query")
    };
    assert_eq!(refs(&q("udp.stream == 0")), ["eth:qa@seq:1", "eth:qa@seq:2"]);
    assert_eq!(refs(&q("udp.stream == 1")), ["eth:qa@seq:3"]);
    // Legacy and tcp-space records never expose a request-local udp.stream.
    assert_eq!(
        refs(&q("udp.stream")),
        ["eth:qa@seq:1", "eth:qa@seq:2", "eth:qa@seq:3"]
    );
    let all = q("udp");
    let protos: Vec<Value> = all["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["stream_proto"].clone())
        .collect();
    assert_eq!(
        protos,
        [
            json!("udp"),
            json!("udp"),
            json!("udp"),
            json!("tcp"),
            Value::Null
        ]
    );
}

/// Every node of a tree, depth first.
fn nodes(v: &Value, out: &mut Vec<Value>) {
    for n in v.as_array().into_iter().flatten() {
        out.push(n.clone());
        nodes(&n["children"], out);
    }
}

#[test]
fn eth02_show_tree_nodes_carry_compile_checked_filters() {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    let svc = journal();
    let r = call(
        &reg,
        &svc,
        "eth.show",
        json!({"ref": "eth:qa@seq:1"}),
        Duration::from_secs(5),
    )
    .expect("show");
    let mut all = Vec::new();
    nodes(&r["tree"], &mut all);
    let by = |name: &str| {
        all.iter()
            .find(|n| n["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("node {name}"))
    };
    assert_eq!(by("udp")["filter"], "udp");
    assert_eq!(by("udp.dstport")["filter"], "udp.dstport == 65201");
    assert_eq!(by("udp.checksum")["filter"], "udp.checksum == 0x0");
    assert_eq!(by("ip.src")["filter"], "ip.src == 192.168.0.2");
    assert_eq!(by("eth.src")["filter"], "eth.src == 02:00:00:00:00:02");
    // The worker's conversation number, not a request-local one.
    assert_eq!(by("udp.stream")["filter"], "udp.stream == 0");
    assert_eq!(by("udp.stream")["label"], "Stream index: 0");
    for n in &all {
        match n["filter"].as_str() {
            Some(f) => assert_eq!(filter_check(f)["ok"], true, "{f} must compile"),
            None => assert!(n["filter"].is_null(), "filter is a string or null: {n}"),
        }
    }
    // DoIP: hex field, protocol layer.
    let s2 = std::sync::Arc::new(Services::default());
    *s2.journal.events.lock().expect("lock") =
        vec![pkt(7, "qa", wall_now(), &doip_uds(&[0x22, 0xF1, 0x90]), None)];
    let r = call(
        &reg,
        &s2,
        "eth.show",
        json!({"ref": "eth:qa@seq:7"}),
        Duration::from_secs(5),
    )
    .expect("show doip");
    let mut all = Vec::new();
    nodes(&r["tree"], &mut all);
    let by = |name: &str| {
        all.iter()
            .find(|n| n["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("node {name}"))
    };
    assert_eq!(by("doip")["filter"], "doip");
    assert_eq!(
        by("doip.target_address")["filter"],
        "doip.target_address == 0x1001"
    );
}

#[test]
fn eth02_filter_expr_literals_escapes_caps_and_refusals() {
    let ip = |s: &str| FieldValue::Ipv4(s.parse().expect("ip"));
    assert_eq!(
        filter_expr("udp.dstport", Some(&FieldValue::Uint(65201))).as_deref(),
        Some("udp.dstport == 65201")
    );
    assert_eq!(
        filter_expr("udp.checksum", Some(&FieldValue::Uint(0xBEEF))).as_deref(),
        Some("udp.checksum == 0xbeef")
    );
    assert_eq!(
        filter_expr("tcp.flags.syn", Some(&FieldValue::Bool(true))).as_deref(),
        Some("tcp.flags.syn == 1")
    );
    assert_eq!(
        filter_expr("tcp.flags.syn", Some(&FieldValue::Bool(false))).as_deref(),
        Some("tcp.flags.syn == 0")
    );
    assert_eq!(
        filter_expr("ip.dst", Some(&ip("192.168.0.10"))).as_deref(),
        Some("ip.dst == 192.168.0.10")
    );
    assert_eq!(
        filter_expr("eth.src", Some(&FieldValue::Mac([2, 0, 0, 0, 0, 0x1a]))).as_deref(),
        Some("eth.src == 02:00:00:00:00:1a")
    );
    assert_eq!(
        filter_expr("doip.eid", Some(&FieldValue::Bytes(vec![0xAA, 0x0B, 0]))).as_deref(),
        Some("doip.eid == aa:0b:00")
    );
    assert_eq!(
        filter_expr("doip.eid", Some(&FieldValue::Bytes(vec![0xAA]))).as_deref(),
        Some("doip.eid == aa")
    );
    assert_eq!(filter_expr("doip.eid", Some(&FieldValue::Bytes(vec![]))), None);
    assert_eq!(
        filter_expr("doip.eid", Some(&FieldValue::Bytes(vec![1; 65]))),
        None,
        "over 64 bytes"
    );
    assert!(filter_expr("doip.eid", Some(&FieldValue::Bytes(vec![1; 64]))).is_some());
    // Strings are quoted and escaped so the filter matches the exact value.
    let s = filter_expr("doip.vin", Some(&FieldValue::Str("A\"B\\C\nD\tE\u{1}".into()))).expect("vin");
    assert_eq!(s, r#"doip.vin == "A\"B\\C\nD\tE\x01""#);
    assert_eq!(filter_check(&s)["ok"], true);
    assert_eq!(
        filter_expr("doip.vin", Some(&FieldValue::Str("x".repeat(129)))),
        None,
        "over 128 chars"
    );
    assert!(filter_expr("doip.vin", Some(&FieldValue::Str("x".repeat(128)))).is_some());
    // Layers: protocol name only when it is a registered protocol.
    assert_eq!(filter_expr("udp", None).as_deref(), Some("udp"));
    assert_eq!(
        filter_expr("udp.dstport", None),
        None,
        "a field without value is not a filter"
    );
    // Unknown names and type mismatches never produce a filter.
    assert_eq!(filter_expr("no.such.field", Some(&FieldValue::Uint(1))), None);
    assert_eq!(
        filter_expr("udp.dstport", Some(&FieldValue::Str("x".into()))),
        None
    );
    assert_eq!(
        filter_expr("udp.dstport", Some(&FieldValue::Uint(70000))),
        None,
        "out of u16 range"
    );
}
