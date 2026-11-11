//! R1 추가 필드의 생략 호환과 JSON/MessagePack golden을 고정한다.
#![allow(clippy::unwrap_used)]

use loom_proto::core_events::{ActivityPayload, SafeStateItem, SafeStateKind, SafeStateList};
use loom_proto::error::ErrorCode;
use loom_proto::worker::{Call, Input, InputLease, InputLeaseEnd, InputLeaseEndCause, Lane, WorkerMsg};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn both<T: Serialize + DeserializeOwned>(golden: Value) -> T {
    let value: T = serde_json::from_value(golden.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), golden);
    let bytes = rmp_serde::to_vec_named(&value).unwrap();
    let decoded: T = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), golden);
    value
}

#[test]
fn worker_identity_fields_are_optional_and_roundtrip() {
    let old = json!({"call_id":"1","method":"uart.open","args":{},"origin":"agent","deadline_mono_ns":"0"});
    assert!(both::<Call>(old.clone()).conn_id.is_none());
    let mut new = old;
    new["conn_id"] = json!("c7");
    both::<Call>(new);
    both::<InputLease>(json!({"channel":"dut","lease_id":"l","owner":"user"}));
    both::<InputLease>(json!({"channel":"dut","lease_id":"l","owner":"user","conn_id":"c7"}));
    let input = both::<Input>(
        json!({"lease_id":"l","bytes":"03","priority":true,"conn_id":"c7","channel":"dut","input_seq":"18446744073709551615"}),
    );
    assert_eq!(input.input_seq.map(|n| n.0), Some(u64::MAX));
    for cause in [InputLeaseEndCause::ConnClosed, InputLeaseEndCause::Released] {
        let end = InputLeaseEnd {
            lease_id: "l".into(),
            conn_id: Some("c7".into()),
            cause,
        };
        let msg = WorkerMsg::InputLeaseEnd(end);
        let decoded = both::<WorkerMsg>(serde_json::to_value(&msg).unwrap());
        assert_eq!(decoded.msg_type(), 20);
        assert_eq!(decoded.lane(), Lane::Ctl);
    }
    both::<InputLeaseEnd>(json!({"lease_id":"l","cause":"released"}));
}

#[test]
fn activity_safe_state_and_limit_golden() {
    both::<ActivityPayload>(json!({"client_id":"a","method":"uart.read","summary":"읽는 중"}));
    both::<ActivityPayload>(
        json!({"client_id":"a","method":"uart.read","summary":"읽는 중","action":"waiting","domain":"uart","channel":"dut"}),
    );
    let old: SafeStateItem = serde_json::from_value(
        json!({"id":"psu.recover","domain":"psu","method":"psu.off","description":"복구","blocked":false}),
    )
    .unwrap();
    assert_eq!(old.kind, SafeStateKind::Recover);
    both::<SafeStateList>(
        json!({"items":[{"id":"psu.off","kind":"emergency_off","domain":"psu","method":"psu.off","description":"출력 차단","blocked":false}]}),
    );
    assert_eq!(ErrorCode::Limit.exit_code(), 4);
    assert_eq!(serde_json::to_string(&ErrorCode::Limit).unwrap(), "\"E_LIMIT\"");
}
