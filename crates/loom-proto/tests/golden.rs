//! Golden fixtures: deserialize → serialize must reproduce the file (key order ignored), and the
//! records must satisfy the contract rules (kind names, payload.write_state, gap payloads).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use loom_proto::core_events::{LatchPayload, is_priority_kind};
use loom_proto::envelope::Envelope;
use loom_proto::event::{
    GapCount, GapData, PublishedRecord, WriteState, payload_bytes, payload_write_state, validate_event_kind,
};
use loom_proto::rpc::{Hello, HelloResult};
use loom_proto::stream::{Heartbeat, SnapshotPart};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures() -> Vec<(String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out = Vec::new();
    for e in fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("json") {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
            out.push((name, v));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(out.len() >= 15, "fixtures missing: {}", out.len());
    out
}

fn roundtrip<T: DeserializeOwned + Serialize>(name: &str, v: &Value) -> T {
    let t: T = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let back = serde_json::to_value(&t).unwrap();
    assert_eq!(&back, v, "{name}: re-serialization differs");
    t
}

fn records(name: &str, v: &Value) -> Vec<PublishedRecord> {
    match v {
        Value::Array(items) => items
            .iter()
            .map(|i| roundtrip::<PublishedRecord>(name, i))
            .collect(),
        _ => vec![roundtrip::<PublishedRecord>(name, v)],
    }
}

#[test]
fn all_fixtures_roundtrip() {
    let mut n = 0;
    for (name, v) in fixtures() {
        if name.starts_with("event_") {
            for r in records(&name, &v) {
                validate_event_kind(&r.kind).unwrap();
                // Source/published conversion is lossless apart from seq/epoch.
                let (seq, epoch) = (r.seq, r.epoch);
                assert_eq!(r.clone().into_source().publish(seq, epoch), r);
            }
        } else if name.starts_with("envelope_") {
            let env: Envelope<Value> = roundtrip(&name, &v);
            assert_eq!(env.ok, env.error.is_none(), "{name}");
        } else if name == "stream_snapshot_part" {
            roundtrip::<SnapshotPart>(&name, &v);
        } else if name == "stream_heartbeat" {
            roundtrip::<Heartbeat>(&name, &v);
        } else if name == "hello" {
            roundtrip::<Hello>(&name, &v);
        } else if name == "hello_result" {
            roundtrip::<HelloResult>(&name, &v);
        } else {
            panic!("unclassified fixture {name}");
        }
        n += 1;
    }
    assert_eq!(n, 15);
}

fn load(name: &str) -> Value {
    fixtures()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .unwrap()
}

#[test]
fn write_rows_share_request_id_and_progress() {
    let rows = records("uart", &load("event_uart_write_rows"));
    let states: Vec<_> = rows.iter().map(|r| r.write_state().unwrap().unwrap()).collect();
    assert_eq!(
        states,
        [WriteState::Requested, WriteState::Accepted, WriteState::Sent]
    );
    assert!(rows.iter().all(|r| r.request_id.as_deref() == Some("q-01JC7")));
    // payload.bytes decodes (hex) to the sent text.
    assert_eq!(payload_bytes(&rows[2].payload).unwrap().unwrap(), b"reboot\n");

    let uds = records("uds", &load("event_uds_rows"));
    let s: Vec<_> = uds
        .iter()
        .map(|r| payload_write_state(&r.payload).unwrap().unwrap())
        .collect();
    assert_eq!(
        s,
        [
            WriteState::Sent,
            WriteState::Confirmed,
            WriteState::OutcomeUnknown
        ]
    );
}

#[test]
fn uart_rx_bytes_base64() {
    let r = records("uart_rx", &load("event_uart_rx")).remove(0);
    assert_eq!(payload_bytes(&r.payload).unwrap().unwrap(), b"root@dut:~# ");
    assert_eq!(r.write_state().unwrap(), None);
}

#[test]
fn gap_payloads_parse() {
    for (name, cause, count) in [
        ("event_display_gap", "slow_client", GapCount::Exact(601.into())),
        ("event_record_gap", "crash_tail", GapCount::Unknown),
        ("event_source_gap", "usb_reenumeration", GapCount::Unknown),
    ] {
        let r = records(name, &load(name)).remove(0);
        let g: GapData = serde_json::from_value(r.payload.clone()).unwrap();
        assert_eq!(g.cause, cause);
        assert_eq!(g.count, count);
        assert_eq!(serde_json::to_value(&g).unwrap(), r.payload, "{name}");
    }
}

#[test]
fn latch_payload_and_priority() {
    let r = records("latch", &load("event_core_latch")).remove(0);
    let p: LatchPayload = serde_json::from_value(r.payload.clone()).unwrap();
    assert!(p.on && p.persisted);
    assert_eq!(p.uncancellable[0].count, 2.into());
    assert_eq!(serde_json::to_value(&p).unwrap(), r.payload);
    assert!(is_priority_kind(&r.kind, &r.payload));

    let snap: SnapshotPart = serde_json::from_value(load("stream_snapshot_part")).unwrap();
    assert_eq!(
        (snap.n, snap.total, snap.items.len(), snap.events.len()),
        (1, 2, 2, 1)
    );

    // Priority classification (contract 01 §1.5).
    let rows = records("uart", &load("event_uart_write_rows"));
    assert!(
        rows.iter().all(|r| is_priority_kind(&r.kind, &r.payload)),
        "every write-row step"
    );
    let rx = records("rx", &load("event_uart_rx")).remove(0);
    assert!(
        !is_priority_kind(&rx.kind, &rx.payload),
        "bulk data is not priority"
    );
    let can = records("can", &load("event_can_frame")).remove(0);
    assert!(!is_priority_kind(&can.kind, &can.payload));
    for name in ["event_display_gap", "event_record_gap", "event_source_gap"] {
        let g = records(name, &load(name)).remove(0);
        assert!(is_priority_kind(&g.kind, &g.payload), "{name}");
    }
    let empty = serde_json::json!({});
    for k in [
        "uart.state",
        "can.channel_state",
        "uds.timeout",
        "uds.alert",
        "test.verdict",
        "run.step",
        "core.agent_cursor",
    ] {
        assert!(is_priority_kind(k, &empty), "{k}");
    }
    for k in ["uart.data", "can.frame", "dlt.log", "core.heartbeat"] {
        assert!(!is_priority_kind(k, &empty), "{k}");
    }
    // A malformed write_state still goes on the priority path (never silently dropped).
    assert!(is_priority_kind(
        "uart.write",
        &serde_json::json!({"write_state": "teleported"})
    ));
}
