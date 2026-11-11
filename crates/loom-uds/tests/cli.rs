//! `loom uds …` CLI fragment: argument → RpcCall conversion, refusals, human rendering, and
//! conformance of every produced params object to the registered `uds.*` schemas. No daemon.
#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use loom_cli_kit::{DomainCli, RpcCall};
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast};
use loom_proto::error::ErrorCode;
use loom_uds::cli::UdsCli;
use serde_json::{Value, json};

fn call(args: &[&str]) -> Result<RpcCall, String> {
    let argv: Vec<&str> = std::iter::once("uds").chain(args.iter().copied()).collect();
    let m = UdsCli::command()
        .try_get_matches_from(argv)
        .map_err(|e| format!("clap: {e}"))?;
    UdsCli::to_call(&m).map_err(|e| format!("usage: {e}"))
}

fn ok(args: &[&str]) -> (String, Value) {
    let c = call(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
    (c.method, c.params)
}

#[test]
fn command_tree_is_valid() {
    UdsCli::command().debug_assert();
}

#[test]
fn connect() {
    assert_eq!(
        ok(&["connect", "--sim", "bench", "--name", "ecu1"]),
        ("uds.connect".into(), json!({"name": "ecu1", "sim": "bench"}))
    );
    // Bare `--sim` = profile bench.
    assert_eq!(
        ok(&["connect", "--name", "ecu1", "--sim"]).1,
        json!({"name": "ecu1", "sim": "bench"})
    );
    let c = call(&[
        "connect",
        "--name",
        "ecu2",
        "--host",
        "192.168.0.10",
        "--port",
        "13401",
        "--la",
        "0x1001",
        "--tester-la",
        "0E80",
        "--activation",
        "0xE0",
        "--no-reconnect",
        "--timeout-ms",
        "2000",
    ])
    .unwrap();
    assert_eq!(
        c.params,
        json!({"name": "ecu2", "host": "192.168.0.10", "port": 13401, "la": 0x1001, "tester_la": 0x0E80,
               "activation": 0xE0, "auto_reconnect": false, "timeout_ms": 2000})
    );
    assert_eq!(c.wait, Some(Duration::from_millis(7_000)));
    for bad in [
        &["connect", "--name", "ecu1"][..], // neither sim nor host
        &["connect", "--sim", "bench"][..], // no name
        &["connect", "--name", "e", "--sim", "bench", "--host", "1.2.3.4"][..], // both
        &["connect", "--name", "e", "--sim", "warp"][..], // unknown profile
        &["connect", "--name", "e", "--host", "h", "--la", "12345"][..], // 5 hex digits
        &["connect", "--name", "e", "--host", "h", "--port", "0"][..],
        &["connect", "--name", "e", "--host", "h", "--activation", "100"][..],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn reads_and_queries() {
    assert_eq!(ok(&["status"]), ("uds.status".into(), json!({})));
    assert_eq!(ok(&["status", "-t", "ecu1"]).1, json!({"target": "ecu1"}));
    assert_eq!(
        ok(&["read-did", "F190", "0xF18C", "-t", "ecu1"]),
        (
            "uds.read_did".into(),
            json!({"target": "ecu1", "dids": ["F190", "0xF18C"]})
        )
    );
    assert_eq!(
        ok(&["dtc", "list", "--mask", "09", "--target", "ecu1"]),
        ("uds.dtc.list".into(), json!({"target": "ecu1", "mask": 9}))
    );
    assert_eq!(ok(&["dtc", "list"]).1, json!({}));
    assert_eq!(
        ok(&["trace", "-t", "ecu1", "--tail", "100"]),
        ("uds.trace".into(), json!({"target": "ecu1", "tail": 100}))
    );
    assert_eq!(
        ok(&["abort", "--scope", "agent"]),
        ("uds.abort".into(), json!({"scope": "agent"}))
    );
    let many: Vec<String> = (0..65).map(|i| format!("{:04X}", 0xF100 + i)).collect();
    let mut args = vec!["read-did"];
    args.extend(many.iter().map(String::as_str));
    assert!(call(&args).is_err(), "65 DIDs");
    for bad in [
        &["read-did"][..],
        &["read-did", "F1900"][..],
        &["read-did", "G190"][..],
        &["dtc", "list", "--mask", "100"][..],
        &["dtc"][..],
        &["trace", "--tail", "0"][..],
        &["trace", "--tail", "1001"][..],
        &["abort", "--scope", "everyone"][..],
        &["session", "default", "--timeout-ms", "100"][..], // not in the uds.session schema
        &["dtc", "list", "--timeout-ms", "100"][..],        // not in the uds.dtc.list schema
        &["read-did", "F190", "--timeout-ms", "0"][..],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn writes() {
    let c = call(&[
        "write-did",
        "F1A0",
        "--hex",
        "02",
        "-t",
        "ecu1",
        "--timeout-ms",
        "1000",
    ])
    .unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        (
            "uds.write_did",
            json!({"target": "ecu1", "did": "F1A0", "data_hex": "02"})
                .as_object()
                .map(|o| {
                    let mut o = o.clone();
                    o.insert("timeout_ms".into(), json!(1000));
                    Value::Object(o)
                })
                .unwrap()
        )
    );
    // Write + read-back: the CLI waits for two requests.
    assert_eq!(c.wait, Some(Duration::from_millis(7_000)));
    assert_eq!(
        ok(&["session", "extended"]),
        ("uds.session".into(), json!({"session": "extended"}))
    );
    assert_eq!(ok(&["session", "0x40"]).1, json!({"session": "0x40"}));
    // raw = bytes_hex (never the text grammar); tokens are joined as given.
    assert_eq!(
        ok(&["raw", "22 F1 90", "-t", "ecu1"]),
        (
            "uds.request".into(),
            json!({"target": "ecu1", "bytes_hex": "22 F1 90"})
        )
    );
    assert_eq!(
        ok(&["raw", "22", "F190", "--dry-run"]).1,
        json!({"bytes_hex": "22 F190", "dry_run": true})
    );
    // Unknown services and risky bytes are the gateway's to classify, not the CLI's.
    assert_eq!(ok(&["raw", "11 01"]).1, json!({"bytes_hex": "11 01"}));
    for bad in [
        &["write-did", "F1A0"][..],
        &["write-did", "F1A0", "--hex", "0"][..],
        &["write-did", "F1A0", "--hex", "zz"][..],
        &["session", "warp"][..],
        &["session", "00"][..],
        &["raw", "22 F1 9"][..],
        &["raw", "read F190"][..],
        &["raw", "--dry-run"][..],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
    let big = "00".repeat(4096);
    assert!(call(&["raw", &big]).unwrap_err().starts_with("usage"));
}

/// Every params object the CLI builds names only properties of the registered schema and
/// carries its required ones (the schema is the canonical contract).
#[cfg(feature = "runtime")]
#[test]
fn params_match_registered_schemas() {
    let (reg, _) = loom_testkit::RegistryProbe::assert_ok(&loom_uds::UdsModule);
    let cases: &[&[&str]] = &[
        &[
            "connect",
            "--name",
            "ecu1",
            "--sim",
            "bench",
            "--timeout-ms",
            "100",
        ],
        &[
            "connect",
            "--name",
            "e",
            "--host",
            "h",
            "--port",
            "1",
            "--la",
            "1",
            "--tester-la",
            "2",
            "--activation",
            "1",
            "--no-reconnect",
        ],
        &["status", "-t", "ecu1"],
        &["read-did", "F190", "-t", "ecu1", "--timeout-ms", "100"],
        &[
            "write-did",
            "F1A0",
            "--hex",
            "02",
            "-t",
            "ecu1",
            "--timeout-ms",
            "100",
        ],
        &["session", "default", "-t", "ecu1"],
        &["dtc", "list", "--mask", "FF", "-t", "ecu1"],
        &["raw", "3E 00", "--dry-run", "-t", "ecu1", "--timeout-ms", "100"],
        &["trace", "-t", "ecu1", "--tail", "5"],
        &["abort", "-t", "ecu1", "--scope", "all"],
    ];
    for args in cases {
        let c = call(args).unwrap();
        let spec = reg
            .commands()
            .iter()
            .find(|s| s.method == c.method)
            .unwrap_or_else(|| panic!("{} not registered", c.method));
        let schema = spec.params.as_value();
        let props = schema["properties"].as_object().cloned().unwrap_or_default();
        for k in c.params.as_object().unwrap().keys() {
            assert!(props.contains_key(k), "{}: {k} not in schema", c.method);
        }
        for r in schema["required"].as_array().into_iter().flatten() {
            assert!(
                c.params.get(r.as_str().unwrap()).is_some(),
                "{}: missing required {r}",
                c.method
            );
        }
    }
}

fn render(args: &[&str], env: &Envelope<Value>) -> String {
    let c = call(args).unwrap();
    let mut out = Vec::new();
    UdsCli::render_human(&c, env, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn success(data: Value) -> Envelope<Value> {
    Envelope::success("uds", data, Meta::default(), OthersSinceLast::default())
}

#[test]
fn renders_rows_state_and_dry_run() {
    let row = json!({"request_id": "r1", "row": {
        "target": "ecu1", "write_state": "confirmed", "summary": "ReadDataByIdentifier F190 VIN",
        "risk": "R0", "reason": "read", "resp_bytes": {"encoding": "hex", "data": "62F190"},
        "rtt_ms": 3.25, "decoded": [{"did": "F190", "name": "VIN", "value": "WVW123", "unit": null}]}});
    let s = render(&["read-did", "F190"], &success(row));
    assert!(
        s.contains("r1  confirmed  ReadDataByIdentifier F190 VIN  R0 · read  → 62F190  3.2 ms"),
        "{s}"
    );
    assert!(s.contains("F190 VIN  WVW123"), "{s}");

    let st = json!({"target": "ecu1", "existing": false, "state": {"target": "ecu1", "generation": 1,
        "backend": "sim", "addr": "DoIP 0x1001", "link": {"state": "up"},
        "session": {"id": 1, "name": "default", "confidence": "assumed"},
        "security": {"level": null, "confidence": "assumed"}, "lease": null}});
    let s = render(&["connect", "--name", "ecu1", "--sim"], &success(st));
    assert!(
        s.contains("ecu1  sim DoIP 0x1001  link up  session default (assumed)  security locked  gen 1"),
        "{s}"
    );
    assert!(render(&["status"], &success(json!({"targets": []}))).starts_with("no targets"));

    let dry = json!({"dry_run": true, "target": "ecu1", "bytes": {"encoding": "hex", "data": "1101"},
        "summary": "ECUReset hardReset", "risk": "R3", "reason": "reset", "block": null});
    let s = render(&["raw", "11 01", "--dry-run"], &success(dry));
    assert!(
        s.contains("dry run · ecu1 · 1101 · ECUReset hardReset · R3 · reset · nothing sent"),
        "{s}"
    );
}

#[test]
fn renders_negative_and_unknown_rows_from_error_detail() {
    let body = ErrorBody {
        code: ErrorCode::NoResponse,
        message: "write may or may not have executed".into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: json!({"request_id": "r9", "write_state": "outcome_unknown", "row": {
            "write_state": "outcome_unknown", "summary": "WriteDataByIdentifier F1A0", "risk": "R2",
            "reason": "write", "timeout": true}}),
    };
    let env: Envelope<Value> =
        Envelope::failure("uds.write_did", body, Meta::default(), OthersSinceLast::default());
    let s = render(&["write-did", "F1A0", "--hex", "02"], &env);
    assert!(
        s.contains("r9  outcome_unknown  WriteDataByIdentifier F1A0  R2 · write  TIMEOUT"),
        "{s}"
    );
    assert!(s.contains("do not resend"), "{s}");

    let body = ErrorBody {
        code: ErrorCode::Nrc,
        message: "NRC 0x31 requestOutOfRange".into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: json!({"request_id": "r2", "row": {"write_state": "failed", "summary": "ReadDataByIdentifier 1234",
            "risk": "R0", "reason": "read", "nrc": 0x31, "nrc_name": "requestOutOfRange"}}),
    };
    let env: Envelope<Value> =
        Envelope::failure("uds.read_did", body, Meta::default(), OthersSinceLast::default());
    assert!(render(&["read-did", "1234"], &env).contains("NRC 0x31 requestOutOfRange"));
}
