//! B4 spec (Opus): `can.export` (CAN-4). FUNCTIONS.md §B4-EXPORT.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use loom_can::export::{ExportFormat, output_path, safe_label};
use loom_can::schema::FramePayload;
use loom_can::{KIND_TX, RPC_EXPORT};
use loom_can_frame::parse_frame;
use loom_proto::error::ErrorCode;
use loom_proto::event::{GapCount, GapData, Origin, SeqRange, kinds};
use loom_proto::num::U64;
use loom_proto::rpc::Role;
use serde_json::{Value, json};

mod common;
use common::*;

/// Removes the files an export created, also when an assertion fails.
struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}
fn cleanup(d: &Value) -> Cleanup {
    let mut v = Vec::new();
    for k in ["path", "sidecar"] {
        if let Some(s) = d[k].as_str() {
            v.push(PathBuf::from(s));
        }
    }
    Cleanup(v)
}
fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("loom-can-export-test-{}-{name}", std::process::id()))
}
fn sidecar_kinds(path: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}
fn no_partials(path: &str) {
    for s in [".partial", ".loom.jsonl.partial"] {
        assert!(
            !Path::new(&format!("{path}{s}")).exists(),
            "{path}{s} left behind"
        );
    }
}

/// ch0: 0x300 rx, 0x301 rx, 0x302 tx(agent echo); ch1 frame; a can.tx request row on ch0.
fn fill(env: &Env_) {
    let j = &env.svc.journal;
    j.push(frame("300#01", Origin::Device, "ch0"));
    j.push(frame("301#0203", Origin::Device, "ch0"));
    j.push(frame("302#04", Origin::Agent, "ch0"));
    j.push(frame("123#AA", Origin::Device, "ch1"));
    let mut tx = record(
        KIND_TX,
        "ch0",
        Origin::Agent,
        Some("tx"),
        json!({"write_state": "sent", "frame": serde_json::to_value(FramePayload::from_frame(&parse_frame("300#01", false).unwrap(), None)).unwrap(), "count": 1, "sent": 1}),
    );
    tx.request_id = Some("r9".into());
    j.push(tx);
}

#[test]
fn registered_as_read_with_schema() {
    let env = setup();
    let c = env
        .reg
        .commands()
        .iter()
        .find(|c| c.method == RPC_EXPORT)
        .expect("can.export registered");
    assert!(c.write_class.is_none(), "export is a read");
    let s = serde_json::to_value(&c.params).unwrap();
    assert_eq!(s["additionalProperties"], json!(false));
    assert_eq!(s["required"], json!(["format"]));
    assert_eq!(s["properties"]["format"]["enum"], json!(["blf", "asc"]));
}

#[test]
fn asc_default_path_is_a_new_daemon_temp_file() {
    let env = setup();
    fill(&env);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "asc"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    let path = d["path"].as_str().unwrap();
    let p = Path::new(path);
    assert_eq!(p.parent().unwrap(), std::env::temp_dir().as_path());
    let name = p.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("loom-can-ch0-r1") && name.ends_with(".asc"),
        "{name}"
    );
    assert_eq!(d["sidecar"], json!(format!("{path}.loom.jsonl")));
    assert_eq!(d["location"], json!("daemon"));
    assert_eq!(d["format"], json!("asc"));
    assert_eq!(d["channel"], json!("ch0"));
    assert_eq!(
        d["frames"],
        json!(3),
        "can.frame rows only: not ch1, not the can.tx request row"
    );
    assert_eq!(d["gaps"], json!(0));
    assert_eq!(d["truncated"], json!(false));
    assert_eq!(d["truncated_reason"], Value::Null);
    assert_eq!(d["next_cursor"], Value::Null);
    assert_eq!(d["complete"], json!(true));
    assert_eq!(d["range"], json!({"from": "1", "to": "3"}));
    assert_eq!(d["clock_id"], json!("c"));
    assert_eq!(d["wall_anchor_known"], json!(true));
    let text = std::fs::read_to_string(path).unwrap().to_lowercase();
    assert!(text.contains("301"), "ASC holds the frames");
    let k = sidecar_kinds(d["sidecar"].as_str().unwrap());
    assert_eq!(k.first().map(String::as_str), Some("header"));
    assert_eq!(k.iter().filter(|s| *s == "frame").count(), 3);
    assert_eq!(k.last().map(String::as_str), Some("complete"));
    no_partials(path);

    // A second export never reuses the first file.
    let d2 = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "asc"}),
        )
        .unwrap();
    let _c2 = cleanup(&d2);
    assert_ne!(d2["path"], d["path"]);
}

#[test]
fn filters_match_trace_query() {
    let env = setup();
    fill(&env);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Viewer,
            json!({"channel": "ch0", "since": "seq:1", "ids": ["0x301-0x302"], "format": "blf"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    assert_eq!(d["frames"], json!(2));
    assert_eq!(d["range"], json!({"from": "2", "to": "3"}));
    let d = env
        .call(
            RPC_EXPORT,
            Role::Viewer,
            json!({"channel": "ch0", "since": "seq:1", "dir": "tx", "format": "blf"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    assert_eq!(d["frames"], json!(1));
    let d = env
        .call(
            RPC_EXPORT,
            Role::Viewer,
            json!({"channel": "ch0", "since": "seq:1", "origins": ["device"], "format": "blf"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    assert_eq!(d["frames"], json!(2));
    // a viewer's export is not agent activity
    assert!(env.published().is_empty());
}

#[test]
fn explicit_path_must_be_absolute_and_new() {
    let env = setup();
    fill(&env);
    let out = scratch("explicit.blf");
    let _c = Cleanup(vec![
        out.clone(),
        PathBuf::from(format!("{}.loom.jsonl", out.display())),
    ]);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "blf", "path": out.to_str().unwrap()}),
        )
        .unwrap();
    assert_eq!(d["path"], json!(out.to_str().unwrap()));
    assert!(std::fs::metadata(&out).unwrap().len() > 0);
    no_partials(out.to_str().unwrap());

    // existing target: refused, untouched
    let before = std::fs::read(&out).unwrap();
    let e = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "blf", "path": out.to_str().unwrap()}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("exists"), "{}", e.message);
    assert_eq!(std::fs::read(&out).unwrap(), before);

    // relative path: refused before any file is made
    let e = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "format": "asc", "path": "out.asc"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("absolute"), "{}", e.message);
    assert!(!Path::new("out.asc").exists());

    // bad format / unknown param
    assert_eq!(
        env.call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "format": "csv"})
        )
        .unwrap_err()
        .code,
        ErrorCode::Usage
    );
    assert_eq!(
        env.call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "format": "asc", "x": 1})
        )
        .unwrap_err()
        .code,
        ErrorCode::Usage
    );
}

#[test]
fn journal_gaps_become_gap_records_once_and_mark_incomplete() {
    let env = setup();
    let j = &env.svc.journal;
    // > MAX_ROWS rows → several scans; the fake journal repeats its gap list on every scan
    for i in 0..1_500u32 {
        j.push(frame(
            &format!("{:03X}#{:02X}", 0x100 + (i % 16), i % 256),
            Origin::Device,
            "ch0",
        ));
    }
    j.gaps.lock().unwrap().push(GapData {
        seq_range: Some(SeqRange {
            from: U64(40),
            to: U64(44),
        }),
        producer_range: None,
        count: GapCount::Exact(U64(5)),
        cause: "disk_full".into(),
        bytes: None,
        detail: Value::Null,
    });
    j.gaps.lock().unwrap().push(GapData {
        seq_range: None,
        producer_range: None,
        count: GapCount::Unknown,
        cause: "driver_overrun".into(),
        bytes: None,
        detail: Value::Null,
    });
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "asc"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    assert_eq!(d["frames"], json!(1_500));
    assert_eq!(d["gaps"], json!(2), "distinct journal gaps, not one per scan");
    assert_eq!(d["gaps_dropped"], json!(0));
    assert_eq!(d["complete"], json!(false));
    assert_eq!(d["truncated"], json!(false));
    let rows: Vec<Value> = std::fs::read_to_string(d["sidecar"].as_str().unwrap())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|v: &Value| v["kind"] == "gap")
        .collect();
    assert_eq!(rows.len(), 2);
}

#[test]
fn clock_change_ends_the_file_with_a_cursor() {
    let env = setup();
    let j = &env.svc.journal;
    j.push(frame("300#01", Origin::Device, "ch0"));
    j.push(frame("301#02", Origin::Device, "ch0"));
    let mut r = frame("302#03", Origin::Device, "ch0");
    r.ts.clock_id = "d".into();
    j.push(r);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "asc"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    assert_eq!(d["frames"], json!(2));
    assert_eq!(d["truncated"], json!(true));
    assert_eq!(d["truncated_reason"], json!("clock_change"));
    assert_eq!(d["next_cursor"], json!("e1:seq:3"));
    assert_eq!(d["complete"], json!(false));
    assert_eq!(d["clock_id"], json!("c"));
}

#[test]
fn empty_selection_writes_no_file() {
    let env = setup();
    fill(&env);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "ids": ["0x7FF"], "format": "blf"}),
        )
        .unwrap();
    assert_eq!(d["frames"], json!(0));
    assert_eq!(d["path"], Value::Null);
    assert_eq!(d["sidecar"], Value::Null);
    assert_eq!(d["range"], Value::Null);
    assert_eq!(d["complete"], json!(true));
}

#[test]
fn agent_export_is_visible_activity() {
    let env = setup();
    fill(&env);
    let d = env
        .call(
            RPC_EXPORT,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "format": "asc"}),
        )
        .unwrap();
    let _c = cleanup(&d);
    let acts = env.published();
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0].kind, kinds::CORE_ACTIVITY);
    assert_eq!(acts[0].payload["action"], json!("read"));
    assert_eq!(acts[0].payload["range"], json!({"from": "1", "to": "3"}));
}

#[test]
fn labels_and_paths() {
    assert_eq!(safe_label("ch0"), "ch0");
    assert_eq!(safe_label("a/b c"), "a_b_c");
    assert_eq!(safe_label("../x"), "_x");
    assert_eq!(safe_label("..."), "x");
    assert_eq!(safe_label(""), "x");
    assert_eq!(safe_label(&"y".repeat(100)).len(), 64);

    let dir = scratch("dir");
    std::fs::create_dir_all(&dir).unwrap();
    let p = output_path(&dir, None, "c/h", "r 1", ExportFormat::Blf).unwrap();
    assert_eq!(p, dir.join("loom-can-c_h-r_1.blf"));
    std::fs::write(&p, b"x").unwrap();
    let p2 = output_path(&dir, None, "c/h", "r 1", ExportFormat::Blf).unwrap();
    assert_eq!(p2, dir.join("loom-can-c_h-r_1-1.blf"));
    // a leftover partial also blocks the name
    std::fs::write(dir.join("loom-can-c_h-r_1-1.blf.partial"), b"x").unwrap();
    assert_eq!(
        output_path(&dir, None, "c/h", "r 1", ExportFormat::Blf).unwrap(),
        dir.join("loom-can-c_h-r_1-2.blf")
    );
    let e = output_path(&dir, Some("rel.asc"), "c", "t", ExportFormat::Asc).unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    let e = output_path(&dir, Some(p.to_str().unwrap()), "c", "t", ExportFormat::Blf).unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    let _ = std::fs::remove_dir_all(&dir);
}
