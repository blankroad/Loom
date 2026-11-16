//! UI ⇄ backend decoder-spec contract (operator-upgrade logic batch 1, finding L2).
//! The UI must only emit specs the backend accepts, with the meaning the backend gives them.
//! Fixture shared with ui/src/panels/logic/logicBatch1.test.ts.
#![allow(clippy::unwrap_used)]

use loom_logic::model::{Decoder, StartConfig};
use loom_proto::error::ErrorCode;
use serde_json::{Value, json};

fn cases() -> Vec<Value> {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ui-decoder-specs.json"
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["cases"].as_array().unwrap().clone()
}

#[test]
fn every_ui_spec_parses_to_the_documented_meaning() {
    let all = cases();
    assert!(all.len() >= 8);
    for c in all {
        let name = c["name"].as_str().unwrap();
        let spec = c["spec"].as_str().unwrap();
        let d = Decoder::parse(spec).unwrap_or_else(|e| panic!("{name}: {spec} refused: {e}"));
        assert_eq!(d.canonical(), c["canonical"].as_str().unwrap(), "{name}");
        // a real device capture accepts it unchanged
        let dev = json!({"source": "sigrok-device", "driver": "fx2lafw", "decoders": [spec]});
        assert!(
            StartConfig::from_params(&dev).is_ok(),
            "{name}: device start refused"
        );
    }
}

#[test]
fn synthetic_limits_match_ui_validation() {
    for c in cases() {
        let name = c["name"].as_str().unwrap();
        let p = json!({"source": "synthetic", "samplerate": 2_000_000, "decoders": [c["spec"]]});
        let r = StartConfig::from_params(&p);
        if c["synthetic_ok"].as_bool().unwrap() {
            assert!(r.is_ok(), "{name}: {:?}", r.err());
        } else {
            assert_eq!(r.unwrap_err().code, ErrorCode::NotImplemented, "{name}");
        }
    }
}
