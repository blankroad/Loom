//! JSON ↔ MessagePack (rmp-serde `to_vec_named`) round trips: one serde type, two wire forms.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_proto::bytes::{Base64Bytes, Bytes};
use loom_proto::envelope::RecordHealth;
use loom_proto::error::ErrorCode;
use loom_proto::event::{
    Dir, Env, GapCount, GapData, Metric, Origin, PublishedRecord, Risk, SourceRecord, SourceRef, Ts, TsSource,
};
use loom_proto::num::U64;
use loom_proto::worker::{
    Events, Health, Input, JournalHealth, ProtoVersion, Reply, ReplyResult, WorkerError, WorkerHello,
    WorkerMsg,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn mp<T: Serialize>(t: &T) -> Vec<u8> {
    rmp_serde::to_vec_named(t).unwrap()
}

fn both<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(t: &T) {
    let j = serde_json::to_string(t).unwrap();
    assert_eq!(&serde_json::from_str::<T>(&j).unwrap(), t, "json");
    let m = mp(t);
    assert_eq!(&rmp_serde::from_slice::<T>(&m).unwrap(), t, "msgpack");
}

fn sample(max: bool) -> SourceRecord {
    let big = if max { U64::MAX } else { U64(7) };
    SourceRecord {
        schema_version: 1,
        session_id: "s-ünïcødé-✓".into(),
        domain: "can".into(),
        channel: Some("ch0".into()),
        kind: "can.frame".into(),
        producer_id: "can-sim".into(),
        producer_epoch: big,
        source_seq: big,
        ts: Ts {
            mono_ns: big,
            wall_ns: U64(1_794_146_701_234_567_000),
            clock_id: "host-boot".into(),
            source: TsSource::Driver,
            quality: None,
        },
        dir: Some(Dir::Rx),
        origin: Origin::Device,
        client_id: None,
        actor_id: None,
        external_actor: Some("canoe".into()),
        original_origin: None,
        request_id: None,
        job_id: None,
        generation: Some(u32::MAX),
        purpose: None,
        risk: Some(Risk::R0),
        test_session: None,
        test_step: None,
        env: Some(Env::Vehicle),
        payload: json!({"id": 291, "bytes": {"encoding": "hex", "data": "00ff"}, "text": "한글"}),
        metrics: vec![Metric {
            name: "v".into(),
            value: 1.5,
            unit: None,
        }],
        quality: vec![],
    }
}

#[test]
fn records_roundtrip_both_forms() {
    for max in [false, true] {
        let s = sample(max);
        both(&s);
        let p = s.clone().publish(U64::MAX, U64(3));
        both(&p);
    }
    // Empty payload object and no optionals.
    let mut s = sample(false);
    s.payload = json!({});
    s.metrics.clear();
    s.channel = None;
    s.dir = None;
    both(&s);
    // payload is required on the wire: an empty one is written as `{}` (contract 02 §2.2),
    // and a record without `payload` still reads as `{}`.
    let j = serde_json::to_value(&s).unwrap();
    assert_eq!(j["payload"], json!({}));
    let mut missing = j.clone();
    missing.as_object_mut().unwrap().remove("payload");
    assert_eq!(serde_json::from_value::<SourceRecord>(missing).unwrap(), s);
}

#[test]
fn u64_is_string_in_json_and_integer_in_msgpack() {
    let p = sample(true).publish(U64::MAX, U64(3));
    let j = serde_json::to_value(&p).unwrap();
    assert_eq!(j["seq"], json!("18446744073709551615"));
    assert_eq!(j["source_seq"], json!("18446744073709551615"));
    // Decode the MessagePack bytes generically: seq must be a number there.
    let v: Value = rmp_serde::from_slice(&mp(&p)).unwrap();
    assert_eq!(v["seq"].as_u64(), Some(u64::MAX));
    assert_eq!(v["epoch"].as_u64(), Some(3));
    assert!(v["ts"]["mono_ns"].is_u64());
}

#[test]
fn unknown_fields_are_ignored() {
    let p = sample(false).publish(U64(1), U64(1));
    let mut j = serde_json::to_value(&p).unwrap();
    j["future_field"] = json!({"x": 1});
    assert_eq!(serde_json::from_value::<PublishedRecord>(j).unwrap(), p);

    let mut v: Value = rmp_serde::from_slice(&mp(&p)).unwrap();
    v["future_field"] = json!([1, 2, 3]);
    let back: PublishedRecord = rmp_serde::from_slice(&mp(&v)).unwrap();
    assert_eq!(back, p);
}

#[test]
fn bytes_forms() {
    for b in [vec![], vec![0u8, 1, 0xab, 0xff], vec![0x5a; 65536]] {
        both(&Bytes(b.clone()));
        both(&Base64Bytes(b.clone()));
    }
    assert_eq!(
        serde_json::to_string(&Bytes(vec![0xAB, 0x01])).unwrap(),
        "\"ab01\""
    );
    assert_eq!(
        serde_json::from_str::<Bytes>("\"AB01\"").unwrap(),
        Bytes(vec![0xab, 0x01])
    );
    assert_eq!(
        serde_json::to_string(&Base64Bytes(b"hi".to_vec())).unwrap(),
        "\"aGk=\""
    );
    // msgpack uses bin, not a string.
    let m = mp(&Bytes(vec![1, 2, 3]));
    assert_eq!(m[0], 0xc4, "bin8 marker");
}

#[test]
fn gap_count_forms() {
    for g in [
        GapCount::Exact(U64(0)),
        GapCount::Exact(U64::MAX),
        GapCount::Unknown,
    ] {
        let d = GapData {
            seq_range: None,
            producer_range: None,
            count: g,
            cause: "x".into(),
            bytes: None,
            detail: Value::Null,
        };
        both(&d);
    }
    assert_eq!(serde_json::to_value(GapCount::Unknown).unwrap(), json!("unknown"));
    assert_eq!(serde_json::to_value(GapCount::Exact(U64(5))).unwrap(), json!("5"));
}

#[test]
fn worker_messages_roundtrip() {
    let msgs = vec![
        WorkerMsg::Hello(WorkerHello {
            proto: ProtoVersion { major: 1, minor: 0 },
            kind: "uart".into(),
            instance: "uart-dut".into(),
            producer_id: "uart-dut".into(),
            producer_epoch: U64(2),
            pid: 4121,
            build: "e43dc3f".into(),
            schema_hash: "sha256:0".into(),
            codecs: vec!["msgpack-map".into()],
            domains: vec!["uart".into()],
            capabilities: vec![],
            journal_dir: "/tmp/j".into(),
            resume_nonce: None,
        }),
        WorkerMsg::Input(Input {
            conn_id: None,
            channel: None,
            input_seq: None,
            call_id: None,
            lease_id: "l1".into(),
            bytes: Bytes(vec![3]),
            priority: true,
        }),
        WorkerMsg::Reply(Reply {
            call_id: U64(9),
            result: ReplyResult::Err(WorkerError {
                code: ErrorCode::WorkerBusy,
                message: "busy".into(),
                detail: json!({"write_state": "failed"}),
            }),
            covers: Some(SourceRef {
                producer_id: "uart-dut".into(),
                producer_epoch: U64(2),
                source_seq: U64::MAX,
            }),
        }),
        WorkerMsg::Reply(Reply {
            call_id: U64(10),
            result: ReplyResult::Ok(json!({"ok": 1})),
            covers: None,
        }),
        WorkerMsg::Health(Health {
            mono_ns: U64(1),
            rss_bytes: U64(21 << 20),
            commit_bytes: None,
            queues: Default::default(),
            journal: JournalHealth {
                written_through: Some(U64(5)),
                durable_through: None,
                health: RecordHealth::Degraded,
            },
            threads: 6,
        }),
        WorkerMsg::Events(Events {
            first_source_seq: U64(7),
            count: 2,
            records: vec![sample(false), sample(true)],
        }),
    ];
    for m in &msgs {
        both(m);
    }
    let j = serde_json::to_value(&msgs[1]).unwrap();
    assert_eq!(
        j,
        json!({"input": {"lease_id": "l1", "bytes": "03", "priority": true}})
    );
}

#[test]
fn stream_types_both_forms() {
    use loom_proto::core_events::{
        ActivityPayload, AgentCursorPayload, ApprovalRespond, Decision, SafeStateOutcome, SafeStateRunItem,
        SafeStateRunResult,
    };
    use loom_proto::envelope::RecordHealth;
    use loom_proto::stream::{
        CloseReason, Closed, Heartbeat, SnapshotEnd, SubStats, TimeResult, ViewerPresence,
    };
    both(&SnapshotEnd {
        sub: "s1".into(),
        stream_seq: U64::MAX,
    });
    both(&Heartbeat {
        stream_seq_hw: U64(9),
        daemon_mono_ns: U64(1),
        daemon_wall_ns: U64(2),
        record_health: RecordHealth::Degraded,
        latch: false,
        sub_stats: vec![SubStats {
            sub: "control".into(),
            depth: 1,
            drops: U64(0),
            last_flush_mono_ns: U64(1),
        }],
    });
    both(&Closed {
        reason: CloseReason::SlowConsumer,
        detail: None,
        retry_after_ms: Some(500),
    });
    assert_eq!(
        serde_json::to_value(CloseReason::SlowConsumer).unwrap(),
        json!("slow_consumer")
    );
    both(&TimeResult {
        recv_mono_ns: U64(1),
        send_mono_ns: U64(2),
        wall_ns: U64(3),
        clock_id: "c".into(),
    });
    both(&ViewerPresence {
        channel: None,
        visible: true,
        rendered_through_seq: Some(U64(5)),
        oldest_pending_age_ms: None,
    });
    both(&AgentCursorPayload {
        client_id: "a#1".into(),
        from_seq: U64(1),
        to_seq: U64(9),
        others_count: U64(2),
    });
    both(&ActivityPayload {
        action: None,
        domain: None,
        channel: None,
        client_id: "a#1".into(),
        method: "uart.read".into(),
        summary: "read 100".into(),
        range: None,
        waiting: Some(json!({"regex": "login:"})),
    });
    both(&ApprovalRespond {
        approval_id: "ap-1".into(),
        decision: Decision::Approve,
        scope: None,
        presence: None,
    });
    both(&SafeStateRunResult {
        items: vec![SafeStateRunItem {
            id: "psu.off".into(),
            outcome: SafeStateOutcome::Skipped,
            finally_blocked: true,
            reason: Some("uds.flashing".into()),
            request_id: None,
        }],
    });
}
