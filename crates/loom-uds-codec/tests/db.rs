//! DB loader limits, validation and identity hash.

use loom_uds_codec::db::{MAX_DB_DIDS, MAX_DB_FILE_BYTES};
use loom_uds_codec::{CodecError, DiagDb, DidLookup, Format, load_db};

const SAMPLE: &str = r#"{
  "loom_diag": 1,
  "ecu": "bench",
  "dids": [
    {"id": 61856, "name": "sim_mode", "length": 1, "codec": {"type": "hex"}, "writable": true},
    {"id": 61453, "name": "VehicleSpeed", "length": 1,
     "codec": {"type": "uint", "unit": "km/h"}},
    {"id": 257, "name": "region", "length": 1,
     "codec": {"type": "enum", "map": [{"value": 2, "name": "EU"}]}}
  ]
}"#;

#[test]
fn loads_and_hashes_deterministically() -> Result<(), CodecError> {
    let a = load_db(SAMPLE.as_bytes(), Format::Json)?;
    let reformatted: String = SAMPLE.split_whitespace().collect::<Vec<_>>().join(" ");
    let b = load_db(reformatted.as_bytes(), Format::Json)?;
    assert_eq!(a.hash, b.hash, "whitespace does not change the identity hash");
    assert_eq!(a.hash_hex().len(), 16);
    assert_eq!(a.did_len(0xF1A0), Some(1));
    assert_eq!(a.did_len(0xF190), Some(17), "falls back to the standard table");
    let changed = SAMPLE.replace("sim_mode", "sim_mode2");
    assert_ne!(load_db(changed.as_bytes(), Format::Json)?.hash, a.hash);
    Ok(())
}

#[test]
fn rejects_bad_files() {
    let err = |s: &str| {
        load_db(s.as_bytes(), Format::Json)
            .map(|_| ())
            .map_err(|e| e.code())
    };
    assert_eq!(err("{"), Err("E_DB"));
    assert_eq!(err(r#"{"loom_diag": 2}"#), Err("E_DB"));
    assert_eq!(
        err(r#"{"loom_diag": 1, "risk": "R0"}"#),
        Err("E_DB"),
        "unknown fields (e.g. risk) rejected"
    );
    assert_eq!(
        err(r#"{"loom_diag":1,"dids":[{"id":1,"name":"a","length":1,"codec":{"type":"hex"},"risk":"R0"}]}"#),
        Err("E_DB"),
        "DID entries cannot carry a risk grade"
    );
    assert_eq!(
        err(
            r#"{"loom_diag":1,"dids":[{"id":1,"name":"a","length":1,"codec":{"type":"hex"}},{"id":1,"name":"b","length":1,"codec":{"type":"hex"}}]}"#
        ),
        Err("E_DB")
    );
    assert_eq!(
        err(r#"{"loom_diag":1,"dids":[{"id":1,"name":"a","length":9,"codec":{"type":"uint"}}]}"#),
        Err("E_DB")
    );
    assert_eq!(
        err(
            r#"{"loom_diag":1,"dids":[{"id":1,"name":"a","length":1,"codec":{"type":"enum","map":[{"value":256,"name":"x"}]}}]}"#
        ),
        Err("E_DB")
    );
    assert!(load_db(&vec![b' '; MAX_DB_FILE_BYTES + 1], Format::Json).is_err());
    assert!(matches!(
        load_db(b"loom_diag: 1", Format::Yaml),
        Err(CodecError::Db(_))
    ));
}

#[test]
fn did_count_limit() {
    let did = r#"{"id":1,"name":"a","length":1,"codec":{"type":"hex"}}"#;
    let many = vec![did; MAX_DB_DIDS + 1].join(",");
    let doc = format!(r#"{{"loom_diag":1,"dids":[{many}]}}"#);
    assert!(load_db(doc.as_bytes(), Format::Json).is_err());
}

#[test]
fn json_schema_exports() -> Result<(), serde_json::Error> {
    let text = serde_json::to_string(&DiagDb::json_schema())?;
    assert!(text.contains("loom_diag"));
    assert!(!text.contains("\"risk\""), "schema has no risk property");
    Ok(())
}
