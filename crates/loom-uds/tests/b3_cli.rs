//! Operator upgrade UDS batch 3 — CLI `unlock` / `db set|clear` (Opus-authored specification).
//! Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B3-C*). Platform-independent (no process, no shell).
#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use loom_cli_kit::{DomainCli, MAX_WAIT, RpcCall};
use loom_proto::envelope::{Envelope, Meta, OthersSinceLast};
use loom_uds::cli::{MAX_DB_FILE_BYTES, UdsCli, read_db_file};
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

fn render(args: &[&str], data: Value) -> String {
    let c = call(args).unwrap();
    let env = Envelope::success("uds", data, Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    UdsCli::render_human(&c, &env, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("diagdb")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// A scratch file under cargo's per-target temp dir (works the same on every OS).
fn tmp_file(name: &str, bytes: &[u8]) -> String {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&p, bytes).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn b3_c1_unlock_to_call() {
    assert_eq!(
        ok(&["unlock", "--level", "01", "--provider", "bench", "-t", "ecu1"]),
        (
            "uds.security_unlock".into(),
            json!({"target": "ecu1", "level": 1, "provider": "bench"})
        )
    );
    assert_eq!(
        ok(&[
            "unlock",
            "--level",
            "0x41",
            "--provider",
            "oem.v2",
            "--timeout-ms",
            "800"
        ])
        .1,
        json!({"level": 0x41, "provider": "oem.v2", "timeout_ms": 800})
    );
    let c = call(&["unlock", "--level", "01", "--provider", "bench"]).unwrap();
    assert_eq!(
        c.wait,
        Some(std::time::Duration::from_secs(45).min(MAX_WAIT)),
        "seed + provider (≤10 s) + key"
    );
    let e = call(&["unlock", "--level", "02", "--provider", "bench"]).unwrap_err();
    assert!(
        e.starts_with("usage:") && e.contains("odd"),
        "even level refused by the gateway's own encoder: {e}"
    );
    assert!(
        call(&["unlock", "--level", "7F", "--provider", "bench"]).is_err(),
        "above 0x7D"
    );
    assert!(call(&["unlock", "--level", "01"]).is_err(), "provider required");
    assert!(
        call(&["unlock", "--provider", "bench"]).is_err(),
        "level required"
    );
    assert!(call(&["unlock", "--level", "zz", "--provider", "bench"]).is_err());
}

#[test]
fn b3_c3_db_file_read_on_the_cli_side() {
    let (m, p) = ok(&["db", "set", "--file", &fixture("bench.json"), "-t", "ecu1"]);
    assert_eq!(m, "uds.db.set");
    assert_eq!(p["target"], "ecu1");
    assert_eq!(p["db"]["ecu"], "bench");
    assert_eq!(p["db"]["dids"].as_array().map(Vec::len), Some(3));
    assert!(
        call(&["db", "set", "--file", &fixture("bench.json")]).is_err(),
        "target required"
    );
    assert_eq!(
        ok(&["db", "clear", "-t", "ecu1"]),
        ("uds.db.clear".into(), json!({"target": "ecu1"}))
    );
    assert!(call(&["db", "clear"]).is_err(), "target required");

    let missing = fixture("does-not-exist.json");
    assert!(
        read_db_file(&missing)
            .unwrap_err()
            .contains("does-not-exist.json"),
        "names the path"
    );
    let secret = "SECRET-CONTENT-NOT-ECHOED";
    let e = read_db_file(&tmp_file(
        "b3_bad.json",
        format!("{{\"loom_diag\": 1, \"x\": {secret}").as_bytes(),
    ))
    .unwrap_err();
    assert!(!e.contains(secret), "file content never echoed: {e}");
    let big = format!(
        "{{\"loom_diag\": 1, \"ecu\": \"{}\", \"dids\": []}}",
        "x".repeat(MAX_DB_FILE_BYTES)
    );
    let e = read_db_file(&tmp_file("b3_big.json", big.as_bytes())).unwrap_err();
    assert!(
        e.contains(&MAX_DB_FILE_BYTES.to_string()),
        "size refused before parsing: {e}"
    );
    assert_eq!(
        read_db_file(&tmp_file("b3_ok.json", br#"{"loom_diag": 1, "dids": []}"#)),
        Ok(json!({"loom_diag": 1, "dids": []}))
    );
}

#[test]
fn b3_c1_params_match_registered_schemas() {
    let (reg, _) = loom_testkit::RegistryProbe::assert_ok(&loom_uds::UdsModule);
    let f = fixture("bench.json");
    for args in [
        &[
            "unlock",
            "--level",
            "01",
            "--provider",
            "bench",
            "-t",
            "ecu1",
            "--timeout-ms",
            "900",
        ][..],
        &["db", "set", "--file", &f, "-t", "ecu1"][..],
        &["db", "clear", "-t", "ecu1"][..],
    ] {
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

#[test]
fn b3_c2_render() {
    let u = ["unlock", "--level", "01", "--provider", "bench", "-t", "ecu1"];
    let done = json!({
        "target": "ecu1", "level": 1, "unlocked": true, "already_unlocked": false, "seed_request_id": "r7.seed",
        "request_id": "r7",
        "row": {"write_state": "confirmed", "summary": "SecurityAccess sendKey", "risk": "R3", "reason": "SecurityAccess sendKey (no automatic retry)",
                "resp_bytes": {"encoding": "hex", "data": "6702"}},
    });
    assert_eq!(
        render(&u, done),
        "r7  confirmed  SecurityAccess sendKey  R3 · SecurityAccess sendKey (no automatic retry)  → 6702\n\
         ecu1  security level 01 unlocked (seed r7.seed)\n"
    );
    let already = json!({"target": "ecu1", "level": 1, "unlocked": true, "already_unlocked": true, "seed_request_id": "r8.seed"});
    assert_eq!(
        render(&u, already),
        "ecu1  security level 01 already unlocked (zero seed; no key sent)\n"
    );

    let f = fixture("bench.json");
    let set = json!({"target": "ecu1", "db": {"hash": "0123456789abcdef", "ecu": "bench", "dids": 3}, "replaced": false,
                     "warnings": ["DID F18C overrides the ISO table entry ECUSerialNumber"]});
    assert_eq!(
        render(&["db", "set", "--file", &f, "-t", "ecu1"], set),
        "ecu1  DB 0123456789abcdef attached (bench, 3 DIDs)\n  warning: DID F18C overrides the ISO table entry ECUSerialNumber\n"
    );
    let anon = json!({"target": "ecu1", "db": {"hash": "00000000000000aa", "ecu": null, "dids": 0}, "replaced": true, "warnings": []});
    assert_eq!(
        render(&["db", "set", "--file", &f, "-t", "ecu1"], anon),
        "ecu1  DB 00000000000000aa attached (-, 0 DIDs; replaced the previous DB)\n"
    );
    assert_eq!(
        render(
            &["db", "clear", "-t", "ecu1"],
            json!({"target": "ecu1", "cleared": true})
        ),
        "ecu1  DB cleared (ISO DID table only)\n"
    );
    assert_eq!(
        render(
            &["db", "clear", "-t", "ecu1"],
            json!({"target": "ecu1", "cleared": false})
        ),
        "ecu1  no DB attached\n"
    );
}
