//! operator-upgrade B3b (ETH-06) — `decode_as` request parameter on eth.query / show / stats / export
//! (FUNCTIONS.md F5.7–F5.12). Rules only change this request's interpretation; records are untouched.

mod support;

use std::sync::Arc;
use std::time::Duration;

use loom_eth::gateway::Eth;
use loom_proto::error::ErrorCode;
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

use support::*;

const RULE: &str = r#"[{"transport": "udp", "port": 64681, "proto": "doip"}]"#;

fn rules() -> Value {
    serde_json::from_str(RULE).expect("rule json")
}

/// seq 1: DoIP ReadDataByIdentifier on the fixture port 64681 (UDP); seq 2: plain UDP noise.
fn journal() -> Arc<Services> {
    let s = Arc::new(Services::default());
    let mut body = vec![0x0E, 0x00, 0x10, 0x01];
    body.extend([0x22, 0xF1, 0x90]);
    *s.journal.events.lock().expect("lock") = vec![
        pkt(
            1,
            "qa",
            wall_now() - 1_000,
            &udp4(TESTER, ECU, 50000, 64681, &doip(0x8001, &body)),
            None,
        ),
        pkt(
            2,
            "qa",
            wall_now() - 900,
            &udp4(ECU, TESTER, 30501, 30501, &[0; 8]),
            None,
        ),
    ];
    s
}

fn call_ok(m: &str, p: Value) -> Value {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    call(&reg, &journal(), m, p, Duration::from_secs(5)).unwrap_or_else(|e| panic!("{m}: {e:?}"))
}

#[test]
fn eth06_query_filter_and_rows_follow_the_rules_and_echo_them() {
    let base = json!({"ch": "qa", "filter": "doip", "range": {"from": "seq:1"}});
    let plain = call_ok("eth.query", base.clone());
    assert_eq!(plain["matched"], 0, "no rule → port 64681 is plain UDP");
    assert!(plain.get("decode_as").is_none(), "no echo without rules");
    let mut p = base.clone();
    p["decode_as"] = rules();
    let r = call_ok("eth.query", p);
    assert_eq!(r["matched"], 1);
    assert_eq!(r["decode_as"], rules());
    let row = &r["rows"][0];
    assert_eq!(row["ref"], "eth:qa@seq:1");
    assert_eq!(
        row["proto"], "DoIP/UDS",
        "summary columns re-decoded for a rule-matched packet"
    );
    assert_eq!(row["decode_as_applied"], true);
    // unfiltered: the noise row keeps the worker's summary and says no rule applied
    let r = call_ok(
        "eth.query",
        json!({"ch": "qa", "range": {"from": "seq:1"}, "decode_as": rules()}),
    );
    let rows = r["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["decode_as_applied"], false);
    assert_eq!(rows[1]["proto"], "UDP");
    // without rules rows never carry the flag
    let r = call_ok("eth.query", json!({"ch": "qa", "range": {"from": "seq:1"}}));
    assert!(r["rows"][0].get("decode_as_applied").is_none());
    assert_eq!(r["rows"][0]["proto"], "UDP");
}

#[test]
fn eth06_show_stats_export_use_the_rules() {
    let s = call_ok("eth.show", json!({"ref": "eth:qa@seq:1", "decode_as": rules()}));
    assert_eq!(s["decode_as"], rules());
    assert_eq!(s["summary"]["proto"], "DoIP/UDS");
    assert!(
        s["tree"]
            .as_array()
            .expect("tree")
            .iter()
            .any(|n| n["name"] == "doip")
    );
    assert!(
        s["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|w| w["code"] == "decode_as")
    );
    let plain = call_ok("eth.show", json!({"ref": "eth:qa@seq:1"}));
    assert!(plain.get("decode_as").is_none());
    assert!(
        !plain["tree"]
            .as_array()
            .expect("tree")
            .iter()
            .any(|n| n["name"] == "doip")
    );

    let st = call_ok(
        "eth.stats",
        json!({"ch": "qa", "by": "proto", "range": {"from": "seq:1"}, "decode_as": rules()}),
    );
    assert_eq!(st["decode_as"], rules());
    let keys: Vec<&str> = st["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|r| r["key"].as_str())
        .collect();
    assert!(keys.contains(&"DoIP/UDS"), "{keys:?}");
    assert!(keys.contains(&"UDP"), "{keys:?}");

    let ex = call_ok(
        "eth.export",
        json!({"ch": "qa", "format": "pcapng", "filter": "doip", "range": {"from": "seq:1"}, "decode_as": rules()}),
    );
    assert_eq!(ex["packets"], 1);
    assert_eq!(ex["decode_as"], rules());
    let _ = std::fs::remove_file(ex["path"].as_str().expect("path"));
}

#[test]
fn eth06_bad_rules_are_usage_errors_with_the_index() {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    let svc = journal();
    let err = |da: Value| {
        call(
            &reg,
            &svc,
            "eth.query",
            json!({"ch": "qa", "decode_as": da}),
            Duration::from_secs(5),
        )
        .expect_err("usage")
    };
    let e = err(json!({"transport": "udp"}));
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("decode_as must be an array"), "{}", e.message);
    for (bad, idx, msg) in [
        (
            json!([{"transport": "sctp", "port": 1, "proto": "doip"}]),
            0,
            "transport",
        ),
        (
            json!([{"transport": "udp", "port": 1, "proto": "doip"}, {"transport": "udp", "port": 0, "proto": "doip"}]),
            1,
            "port",
        ),
        (
            json!([{"transport": "udp", "port": 70000, "proto": "doip"}]),
            0,
            "port",
        ),
        (
            json!([{"transport": "udp", "port": "1", "proto": "doip"}]),
            0,
            "port",
        ),
        (
            json!([{"transport": "udp", "port": 1, "proto": "uds"}]),
            0,
            "proto",
        ),
        (json!(["udp:1=doip"]), 0, "object"),
    ] {
        let e = err(bad.clone());
        assert_eq!(e.code, ErrorCode::Usage, "{bad}");
        assert!(
            e.message.starts_with(&format!("decode_as[{idx}]")),
            "{bad}: {}",
            e.message
        );
        assert!(e.message.contains(msg), "{bad}: {}", e.message);
        assert_eq!(e.detail["index"], json!(idx), "{bad}");
    }
    let nine: Vec<Value> = (1..=9)
        .map(|p| json!({"transport": "udp", "port": 40000 + p, "proto": "someip"}))
        .collect();
    let e = err(Value::Array(nine));
    assert_eq!(e.message, "decode_as allows at most 8 rules");
    let e = err(
        json!([{"transport": "udp", "port": 5, "proto": "doip"}, {"transport": "udp", "port": 5, "proto": "someip"}]),
    );
    assert_eq!(e.message, "decode_as maps udp port 5 to two protocols");
    // null / absent are fine
    assert!(
        call(
            &reg,
            &svc,
            "eth.query",
            json!({"ch": "qa", "decode_as": null}),
            Duration::from_secs(5)
        )
        .is_ok()
    );
}
