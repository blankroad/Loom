//! Payload fixtures for the UI (`crates/loom-uds/fixtures/*.json`), generated from the Rust types.
//!
//! Each file is an ordered list of `{kind, request_id, payload}` (what one request row or state
//! looks like on the wire, without the core envelope). The test fails if a file differs from what
//! the types produce now, or if a file does not deserialize back into its payload type.
//! Regenerate with `LOOM_UPDATE_FIXTURES=1 cargo test -p loom-uds --test fixtures`.

use std::path::PathBuf;
use std::time::Duration;

use loom_proto::event::{Origin, PayloadBytes, WriteState};
use loom_proto::num::U64;
use loom_uds::hooks::dry_run;
use loom_uds::payload::{DecodedDid, DryRunResult, DtcItem, RowPayload, RxKind, RxPayload, StatePayload};
use loom_uds::session::{Lease, TargetSession};
use loom_uds_codec::{DidLookup, StandardDids, classify_response, nrc_name};
use loom_uds_sim::{FaultAction, FaultMatch, FaultRule, SimEcu};
use serde::Serialize;
use serde_json::{Value, json};

const T: &str = "ecu1";
const TIMING: (u16, u32) = (50, 5000);
const T0: u64 = 5_000_000_000;

fn ev(kind: &str, rid: Option<&str>, payload: impl Serialize) -> Value {
    json!({"kind": kind, "request_id": rid, "payload": serde_json::to_value(payload).expect("payload serializes")})
}

fn ns(ms_after: f64) -> U64 {
    // whole microseconds keep the JSON exact
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    U64(T0 + (ms_after * 1000.0) as u64 * 1000)
}

fn rx(req: &[u8], resp: &[u8], at_ms: f64, late: bool) -> RxPayload {
    let (kind, nrc) = RxKind::from_codec(classify_response(req, resp));
    RxPayload {
        target: T.into(),
        bytes: PayloadBytes::hex(resp),
        kind,
        nrc,
        nrc_name: nrc.and_then(nrc_name).map(Into::into),
        late,
        t_rx_mono_ns: ns(at_ms),
    }
}

/// requested → accepted → sent rows for `req`.
fn opening(rid: &str, req: &[u8]) -> Vec<Value> {
    [WriteState::Requested, WriteState::Accepted, WriteState::Sent]
        .into_iter()
        .map(|ws| {
            let mut p = RowPayload::request(T, req, false, ws, TIMING);
            if ws == WriteState::Sent {
                p.t_send_mono_ns = Some(ns(0.0));
            }
            ev("uds.tx", Some(rid), p)
        })
        .collect()
}

fn final_row(req: &[u8], ws: WriteState) -> RowPayload {
    let mut p = RowPayload::request(T, req, false, ws, TIMING);
    p.t_send_mono_ns = Some(ns(0.0));
    p
}

fn sim_reply(ecu: &mut SimEcu, req: &[u8]) -> Vec<Vec<u8>> {
    ecu.handle(Duration::ZERO, req)
        .into_vec()
        .into_iter()
        .map(|r| r.bytes)
        .collect()
}

fn decode_vin(resp: &[u8]) -> Vec<DecodedDid> {
    let def = StandardDids.did_def(0xF190).expect("VIN is standard");
    let v = def.decode(&resp[3..]).expect("sim VIN decodes");
    vec![DecodedDid::new(0xF190, Some(def.name), &v)]
}

fn read_ok() -> Value {
    let req = [0x22, 0xF1, 0x90];
    let resp = sim_reply(&mut SimEcu::bench(), &req).remove(0);
    let mut out = opening("r-1", &req);
    out.push(ev("uds.rx", Some("r-1"), rx(&req, &resp, 1.2, false)));
    let mut p = final_row(&req, WriteState::Confirmed);
    p.resp_bytes = Some(PayloadBytes::hex(&resp));
    p.first_rx_ms = Some(1.2);
    p.rtt_ms = Some(1.2);
    p.t_first_rx_mono_ns = Some(ns(1.2));
    p.t_final_rx_mono_ns = Some(ns(1.2));
    p.decoded = Some(decode_vin(&resp));
    out.push(ev("uds.result", Some("r-1"), p));
    Value::Array(out)
}

fn pending_then_positive() -> Value {
    let req = [0x22, 0xF1, 0x90];
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::always(
        FaultMatch::sid(0x22),
        FaultAction::PendingThen {
            count: 2,
            every: Duration::from_millis(1000),
        },
    ))
    .expect("one rule");
    let replies = sim_reply(&mut ecu, &req);
    let mut out = opening("r-2", &req);
    for (i, pending) in replies[..2].iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let at = 10.0 + 1000.0 * i as f64;
        out.push(ev("uds.rx", Some("r-2"), rx(&req, pending, at, false)));
        let mut p = final_row(&req, WriteState::Sent);
        p.pending_count = u32::try_from(i + 1).expect("small");
        p.t_first_rx_mono_ns = Some(ns(10.0));
        out.push(ev("uds.pending", Some("r-2"), p));
    }
    let resp = &replies[2];
    out.push(ev("uds.rx", Some("r-2"), rx(&req, resp, 2010.0, false)));
    let mut p = final_row(&req, WriteState::Confirmed);
    p.pending_count = 2;
    p.resp_bytes = Some(PayloadBytes::hex(resp));
    p.first_rx_ms = Some(10.0);
    p.rtt_ms = Some(2010.0);
    p.t_first_rx_mono_ns = Some(ns(10.0));
    p.t_final_rx_mono_ns = Some(ns(2010.0));
    p.decoded = Some(decode_vin(resp));
    out.push(ev("uds.result", Some("r-2"), p));
    Value::Array(out)
}

fn nrc() -> Value {
    let req = [0x22, 0xF1, 0xA1];
    let resp = sim_reply(&mut SimEcu::bench(), &req).remove(0);
    let mut out = opening("r-3", &req);
    out.push(ev("uds.rx", Some("r-3"), rx(&req, &resp, 0.9, false)));
    let mut p = final_row(&req, WriteState::Failed).with_nrc(resp[2]);
    p.resp_bytes = Some(PayloadBytes::hex(&resp));
    p.first_rx_ms = Some(0.9);
    p.rtt_ms = Some(0.9);
    p.t_first_rx_mono_ns = Some(ns(0.9));
    p.t_final_rx_mono_ns = Some(ns(0.9));
    out.push(ev("uds.result", Some("r-3"), p));
    Value::Array(out)
}

fn read_timeout() -> Value {
    let req = [0x22, 0xF1, 0x90];
    let mut out = opening("r-4", &req);
    let mut p = final_row(&req, WriteState::Failed);
    p.timeout = true;
    out.push(ev("uds.result", Some("r-4"), p));
    Value::Array(out)
}

fn write_outcome_unknown() -> Value {
    let req = [0x2E, 0xF1, 0xA0, 0x02];
    let mut out = opening("r-5", &req);
    let mut p = final_row(&req, WriteState::OutcomeUnknown);
    p.timeout = true;
    out.push(ev("uds.result", Some("r-5"), p));
    Value::Array(out)
}

fn late_response() -> Value {
    // r-4 timed out; its response arrives afterwards: recorded on uds.rx only, row not reopened.
    let req = [0x22, 0xF1, 0x90];
    let resp = sim_reply(&mut SimEcu::bench(), &req).remove(0);
    json!([ev("uds.rx", Some("r-4"), rx(&req, &resp, 6200.0, true))])
}

fn sprmib() -> Value {
    let req = [0x3E, 0x80];
    let mut out = opening("r-6", &req);
    let mut p = final_row(&req, WriteState::Sent);
    p.ok_suppressed = true;
    out.push(ev("uds.result", Some("r-6"), p));
    Value::Array(out)
}

fn blocked_2a() -> Value {
    let req = [0x2A, 0x01, 0x01];
    let mut p = RowPayload::request(T, &req, false, WriteState::Failed, TIMING);
    assert!(p.block.is_some(), "0x2A is immutable_deny");
    p.pending_count = 0;
    json!([
        ev(
            "uds.tx",
            Some("r-7"),
            RowPayload::request(T, &req, false, WriteState::Requested, TIMING)
        ),
        ev("uds.result", Some("r-7"), p)
    ])
}

fn dtc_list() -> Value {
    let req = [0x19, 0x02, 0x09];
    let resp = sim_reply(&mut SimEcu::bench(), &req).remove(0);
    let report = loom_uds_codec::response::parse_dtc_by_status_response(&resp).expect("sim DTCs parse");
    let mut out = opening("r-8", &req);
    out.push(ev("uds.rx", Some("r-8"), rx(&req, &resp, 1.0, false)));
    let mut p = final_row(&req, WriteState::Confirmed);
    p.resp_bytes = Some(PayloadBytes::hex(&resp));
    p.first_rx_ms = Some(1.0);
    p.rtt_ms = Some(1.0);
    p.dtcs = Some(
        report
            .dtcs
            .iter()
            .map(|d| DtcItem {
                dtc: format!("{:06X}", d.dtc),
                status: d.status,
            })
            .collect(),
    );
    out.push(ev("uds.result", Some("r-8"), p));
    Value::Array(out)
}

fn state() -> Value {
    let mut s = TargetSession::new(T);
    s.set_endpoint("sim", "DoIP 0x1001");
    s.link_up(Duration::ZERO);
    let a = ev("uds.state", None, s.state_payload());
    s.on_response(
        Duration::from_millis(5),
        &[0x10, 0x03],
        &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4],
    );
    s.set_tester_present(Some(Duration::from_secs(2)));
    s.on_tester_present_ok(Duration::from_millis(2005));
    s.set_lease(Some(Lease {
        owner_origin: Origin::User,
        actor_id: "viewer:alice".into(),
        client_id: "conn-2".into(),
    }));
    json!([a, ev("uds.state", None, s.state_payload())])
}

fn dry_runs() -> Value {
    json!([
        dry_run(T, &json!({"text": "read F190"})),
        dry_run(T, &json!({"text": "write F1A0 02"})),
        dry_run(T, &json!({"bytes_hex": "2A0101"})),
        dry_run(T, &json!({"text": "22 F1 9"})),
    ])
}

fn all() -> Vec<(&'static str, Value)> {
    vec![
        ("read_ok", read_ok()),
        ("pending_then_positive", pending_then_positive()),
        ("nrc", nrc()),
        ("read_timeout", read_timeout()),
        ("write_outcome_unknown", write_outcome_unknown()),
        ("late_response", late_response()),
        ("sprmib", sprmib()),
        ("blocked_2a", blocked_2a()),
        ("dtc_list", dtc_list()),
        ("state", state()),
        ("dry_run", dry_runs()),
    ]
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn roundtrip<T: serde::de::DeserializeOwned + Serialize>(v: &Value, name: &str) {
    let typed: T =
        serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{name}: does not match the type: {e}"));
    let back = serde_json::to_value(typed).expect("serializes");
    assert_eq!(&back, v, "{name}: not lossless through the type");
}

/// Every element of a fixture must deserialize into its payload type.
fn check_types(name: &str, v: &Value) {
    let items = v
        .as_array()
        .unwrap_or_else(|| panic!("{name}: fixture must be an array"));
    for item in items {
        if name == "dry_run" {
            roundtrip::<DryRunResult>(item, name);
            continue;
        }
        let payload = &item["payload"];
        match item["kind"].as_str() {
            Some("uds.tx" | "uds.pending" | "uds.result") => roundtrip::<RowPayload>(payload, name),
            Some("uds.rx") => roundtrip::<RxPayload>(payload, name),
            Some("uds.state") => roundtrip::<StatePayload>(payload, name),
            other => panic!("{name}: unknown kind {other:?}"),
        }
    }
}

#[test]
fn fixtures_match_types() {
    let update = std::env::var_os("LOOM_UPDATE_FIXTURES").is_some();
    for (name, value) in all() {
        check_types(name, &value);
        let path = dir().join(format!("{name}.json"));
        let text = serde_json::to_string_pretty(&value).expect("serializes") + "\n";
        if update {
            std::fs::create_dir_all(dir()).expect("create fixtures dir");
            std::fs::write(&path, &text).expect("write fixture");
            continue;
        }
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run with LOOM_UPDATE_FIXTURES=1)", path.display()));
        let parsed: Value = serde_json::from_str(&on_disk).expect("fixture is JSON");
        check_types(name, &parsed);
        assert_eq!(
            parsed, value,
            "{name}.json differs from the types (regenerate with LOOM_UPDATE_FIXTURES=1)"
        );
    }
}

#[test]
fn row_events_are_self_sufficient_for_the_ui_row_store() {
    // The UI replaces a row's payload with each newer event: the final event must still carry
    // the request fields, and uds.rx must never carry write_state (it would reopen rows).
    for (name, value) in all() {
        for item in value.as_array().into_iter().flatten() {
            let p = &item["payload"];
            match item["kind"].as_str() {
                Some("uds.tx" | "uds.pending" | "uds.result") => {
                    for key in [
                        "target",
                        "write_state",
                        "bytes",
                        "sid",
                        "service",
                        "summary",
                        "risk",
                    ] {
                        assert!(p.get(key).is_some(), "{name}: row payload missing {key}");
                    }
                    assert!(item["request_id"].is_string(), "{name}: row without request_id");
                }
                Some("uds.rx") => assert!(
                    p.get("write_state").is_none(),
                    "{name}: uds.rx must not carry write_state"
                ),
                _ => {}
            }
        }
    }
}
