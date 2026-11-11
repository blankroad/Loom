//! v1.0 map과 v1.1 선택 call_id의 하위 호환을 검사한다.
#![allow(clippy::unwrap_used)]
use loom_proto::worker::{Input, JobRenew, JobStart, JobStop, JobUpdate, WORKER_PROTO_MINOR};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;

fn check<T: Serialize + DeserializeOwned>(old: serde_json::Value) {
    let value: T = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), old);
    let bytes = rmp_serde::to_vec_named(&value).unwrap();
    let decoded: T = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), old);
    for id in [0, 9123, u64::MAX] {
        let mut new = old.clone();
        new["call_id"] = json!(id);
        let value: T = serde_json::from_value(new.clone()).unwrap();
        assert_eq!(serde_json::to_value(&value).unwrap(), new);
        let bytes = rmp_serde::to_vec_named(&value).unwrap();
        let decoded: T = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), new);
    }
}

#[test]
fn optional_call_ids_keep_old_maps_and_roundtrip_full_u64() {
    assert_eq!(WORKER_PROTO_MINOR, 2);
    check::<JobStart>(json!({"job_id":"j", "plan":{}, "lease_ms":100}));
    check::<JobUpdate>(json!({"job_id":"j", "plan":{}}));
    check::<JobRenew>(json!({"job_id":"j", "lease_ms":100}));
    check::<JobStop>(json!({"job_id":"j", "reason":"user"}));
    check::<Input>(json!({"lease_id":"l", "bytes":"03", "priority":true}));
}
