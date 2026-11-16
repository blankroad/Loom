//! Operator upgrade UDS batch 2 — CLI `disconnect` / `forget` / `tp` (Opus-authored specification).
#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_cli_kit::{DomainCli, RpcCall};
use loom_proto::envelope::{Envelope, Meta, OthersSinceLast};
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

fn render(args: &[&str], data: Value) -> String {
    let c = call(args).unwrap();
    let env = Envelope::success("uds", data, Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    UdsCli::render_human(&c, &env, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn b2_c1_to_call() {
    assert_eq!(
        ok(&["disconnect", "-t", "ecu1"]),
        ("uds.disconnect".into(), json!({"target": "ecu1"}))
    );
    assert_eq!(
        ok(&["disconnect", "-t", "ecu1", "--abort"]).1,
        json!({"target": "ecu1", "abort": true})
    );
    assert!(call(&["disconnect"]).is_err(), "target is required");
    let c = call(&["disconnect", "-t", "ecu1"]).unwrap();
    assert!(
        c.wait.is_some_and(|w| w.as_secs() >= 35),
        "CLI waits for the bounded drain: {:?}",
        c.wait
    );
    assert_eq!(
        ok(&["forget", "-t", "ecu1"]),
        ("uds.forget".into(), json!({"target": "ecu1"}))
    );
    assert_eq!(
        ok(&["tp", "on", "--interval-ms", "2000", "-t", "ecu1"]),
        (
            "uds.tester_present".into(),
            json!({"target": "ecu1", "on": true, "interval_ms": 2000})
        )
    );
    assert_eq!(
        ok(&["tp", "on", "--interval-ms", "500", "--no-suppress"]).1,
        json!({"on": true, "interval_ms": 500, "suppress": false})
    );
    assert_eq!(
        ok(&["tp", "off", "-t", "ecu1"]).1,
        json!({"target": "ecu1", "on": false})
    );
    assert!(
        call(&["tp", "on"]).unwrap_err().contains("--interval-ms"),
        "no default interval"
    );
    assert!(
        call(&["tp", "on", "--interval-ms", "100"]).is_err(),
        "below 500 ms"
    );
}

#[test]
fn b2_c1_params_match_registered_schemas() {
    let (reg, _) = loom_testkit::RegistryProbe::assert_ok(&loom_uds::UdsModule);
    for args in [
        &["disconnect", "-t", "ecu1", "--abort"][..],
        &["forget", "-t", "ecu1"][..],
        &["tp", "on", "--interval-ms", "1000", "--no-suppress", "-t", "ecu1"][..],
        &["tp", "off"][..],
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
fn b2_c2_render() {
    let closed = json!({"target": "ecu1", "state": {"link": {"state": "closed", "cause": "disconnected by agent"}},
                        "aborted": {"cancelled_waiting": 2, "in_flight": true}});
    let out = render(&["disconnect", "-t", "ecu1", "--abort"], closed);
    assert_eq!(
        out,
        "ecu1  link closed (disconnected by agent)\n  aborted: 2 waiting, in-flight request aborted (outcome unknown)\n"
    );
    let out = render(
        &["disconnect", "-t", "ecu1"],
        json!({"target": "ecu1", "state": {"link": {"state": "closed"}}, "already_closed": true}),
    );
    assert_eq!(out, "ecu1  link closed (already closed)\n");
    assert_eq!(
        render(
            &["forget", "-t", "ecu1"],
            json!({"target": "ecu1", "forgotten": true, "last_generation": 3})
        ),
        "ecu1  forgotten (last generation 3)\n"
    );
    let on = json!({"target": "ecu1", "state": {"tester_present": {"active": true, "interval_ms": 2000, "ok_count": 0, "suppress": true}}});
    assert_eq!(
        render(&["tp", "on", "--interval-ms", "2000"], on),
        "ecu1  TesterPresent on every 2000 ms (3E 80)\n"
    );
    let off = json!({"target": "ecu1", "state": {"tester_present": null, "tp_stop_cause": "stopped by user"}, "was_active": true});
    assert_eq!(
        render(&["tp", "off"], off),
        "ecu1  TesterPresent off (stopped by user)\n"
    );
    let off2 = json!({"target": "ecu1", "state": {"tester_present": null}, "was_active": false});
    assert_eq!(render(&["tp", "off"], off2), "ecu1  TesterPresent off\n");
}
