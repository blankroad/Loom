//! CI drift gate: committed `schema/v1` must match `loom-proto`.
//! Fix with `cargo run -p loom-schema -- export` from the workspace root.

use std::path::Path;

#[test]
fn committed_schema_matches_loom_proto() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let drift = loom_schema::check(&loom_schema::schema_dir(&root)).expect("read schema dir");
    assert!(
        drift.is_clean(),
        "schema drift: {drift:?}\nrun: cargo run -p loom-schema -- export"
    );
}

#[test]
fn every_schema_declares_draft_2020_12() {
    for (name, text) in loom_schema::render_all().expect("render") {
        let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(
            v.get("$schema").and_then(|s| s.as_str()),
            Some(loom_proto::schema::DRAFT_2020_12),
            "{name}"
        );
    }
}
