//! Worker + real sigrok-cli + archive (B2b, finding L1-R2 / L1-M) — Opus spec tests.
//! Synthetic source only (no device). Skips with a message when sigrok-cli is not installed.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loom_logic::archive::{ArchiveArgs, ArchiveConfig};
use loom_logic::worker::LogicWorker;
use loom_logic::{KIND_ANNOTATIONS, KIND_CAPTURE_STATE};
use loom_proto::error::ErrorCode;
use loom_proto::event::PublishedRecord;
use loom_testkit::FakeWorker;
use serde_json::{Value, json};

fn engine() -> Option<String> {
    let p = loom_logic::model::resolve_engine(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}

struct Tmp(PathBuf);
impl Tmp {
    fn new(name: &str) -> Tmp {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!("loom-logic-arch-eng-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn archive_args(root: &std::path::Path) -> Value {
    ArchiveArgs::Enabled(ArchiveConfig {
        root: root.to_owned(),
        max_capture_bytes: 256 << 20,
        max_total_bytes: 1 << 30,
        max_age_s: 3600,
        max_captures: 8,
    })
    .to_args()
}

fn states(ev: &[PublishedRecord]) -> Vec<Value> {
    ev.iter()
        .filter(|e| e.kind == KIND_CAPTURE_STATE)
        .map(|e| e.payload.clone())
        .collect()
}

/// Run one capture to its final state; returns (all events, final capture_state payload).
fn run_capture(
    fw: &mut FakeWorker<LogicWorker>,
    start_args: Value,
    limit: Duration,
) -> (Vec<PublishedRecord>, Value) {
    fw.call("logic.start", start_args).unwrap();
    let t0 = Instant::now();
    let mut ev = Vec::new();
    loop {
        ev.extend(fw.drain_published());
        if let Some(last) = states(&ev).last()
            && (last["state"] == "stopped" || last["state"] == "failed")
        {
            return (ev, last.clone());
        }
        assert!(
            t0.elapsed() < limit,
            "capture did not finish: {:?}",
            states(&ev).iter().map(|s| s["state"].clone()).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Page through `logic.archive.read kind=annotations`; returns all rows.
fn all_rows(fw: &mut FakeWorker<LogicWorker>, arch: &Value, id: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut req = json!({"capture_id": id, "kind": "annotations", "limit": 1000});
    loop {
        let r = fw
            .call("logic.archive.read", json!({"archive": arch, "request": req}))
            .unwrap();
        rows.extend(r["rows"].as_array().unwrap().clone());
        match r["next_row"].as_u64() {
            Some(n) => req["after_row"] = json!(n),
            None => {
                assert_eq!(r["complete"], true, "{r}");
                return rows;
            }
        }
    }
}

#[test]
fn archive_absent_is_reported_not_silent() {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let mut fw = FakeWorker::new(LogicWorker::new());
    let cfg = json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=115200"], "duration_ms": 300});
    let (ev, fin) = run_capture(
        &mut fw,
        json!({"config": cfg, "engine": e}),
        Duration::from_secs(10),
    );
    assert_eq!(
        fin["archive"],
        json!({"state": "disabled", "reason": "archive not configured"})
    );
    assert_eq!(states(&ev)[0]["archive"]["state"], "disabled");
    let l = fw.call("logic.archive.list", json!({})).unwrap();
    assert_eq!(
        (l["archives"].clone(), l["disabled"].clone()),
        (json!([]), json!("archive not configured"))
    );
    let r = fw
        .call(
            "logic.archive.read",
            json!({"request": {"capture_id": "lc-1-1", "kind": "edges"}}),
        )
        .unwrap_err();
    assert_eq!(r.code, ErrorCode::Config);
}

#[test]
fn overloaded_live_preview_is_complete_in_the_archive() {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let t = Tmp::new("overload");
    let arch = archive_args(&t.0);
    let mut fw = FakeWorker::new(LogicWorker::new());
    // ROOT check config: 256 B every 25 ms at 115200 ≈ 10,240 rows/s > live 2,000 rows/s
    let cfg = json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=115200"],
                     "text": "A".repeat(256), "frame_period_ms": 25, "duration_ms": 3000});
    let (ev, fin) = run_capture(
        &mut fw,
        json!({"config": cfg, "engine": e, "archive": arch}),
        Duration::from_secs(30),
    );
    assert_eq!(fin["state"], "stopped", "{fin}");
    let id = fin["capture_id"].as_str().unwrap().to_owned();
    let k = &fin["counters"];
    let (published, dropped, samples) = (
        k["annotations"].as_u64().unwrap(),
        k["dropped_annotations"].as_u64().unwrap(),
        k["samples"].as_u64().unwrap(),
    );
    assert!(
        dropped > 0,
        "live preview must have been throttled for this check: {k}"
    );
    let live_rows: u64 = ev
        .iter()
        .filter(|e| e.kind == KIND_ANNOTATIONS)
        .map(|e| e.payload["annotations"].as_array().unwrap().len() as u64)
        .sum();
    assert_eq!(live_rows, published);
    // running state already names the archive
    let running = states(&ev).into_iter().find(|s| s["state"] == "running").unwrap();
    assert_eq!(running["archive"]["state"], "recording");
    assert!(running["archive"]["dir"].as_str().unwrap().ends_with(&id));
    // final: whole decoded history + every sample, complete
    let a = &fin["archive"];
    assert_eq!(a["state"], "complete", "{a}");
    assert_eq!(a["annotation_rows"].as_u64(), Some(published + dropped));
    assert_eq!(a["raw_end_sample"].as_u64(), Some(samples));
    assert_eq!(a["raw_truncated_at_sample"], Value::Null);
    let rows = all_rows(&mut fw, &arch, &id);
    assert_eq!(rows.len() as u64, published + dropped);
    assert!(rows.iter().all(|r| r["text"] == "41" && r["source"] == "archive"));
    assert!(
        rows.windows(2)
            .all(|w| w[0]["start_sample"].as_u64() < w[1]["start_sample"].as_u64())
    );
    // raw: contiguous chunks to the acquired end
    let mut from = 0u64;
    loop {
        let r = fw.call("logic.archive.read", json!({"archive": arch, "request": {"capture_id": id, "kind": "edges", "from_sample": from, "mask": 1}})).unwrap();
        for c in r["chunks"].as_array().unwrap() {
            assert_eq!(c["sample_start"].as_u64(), Some(from));
            from = c["sample_end"].as_u64().unwrap();
        }
        if r["next_from"].is_null() {
            assert_eq!(r["complete"], true);
            break;
        }
    }
    assert_eq!(from, samples);
    // list + export name the same evidence
    let l = fw.call("logic.archive.list", json!({"archive": arch})).unwrap();
    assert_eq!(l["archives"][0]["capture_id"], id.as_str());
    assert_eq!(l["archives"][0]["complete"], true);
    let x = fw
        .call(
            "logic.export",
            json!({"archive": arch, "request": {"capture_id": id, "kind": "annotations", "format": "csv"}}),
        )
        .unwrap();
    assert_eq!(x["rows"].as_u64(), Some(published + dropped));
    assert!(std::path::Path::new(x["path"].as_str().unwrap()).is_file());
}

#[test]
fn archive_rpcs_validate_before_any_file_access() {
    let t = Tmp::new("validate");
    let arch = archive_args(&t.0);
    let mut fw = FakeWorker::new(LogicWorker::new());
    let e = fw
        .call(
            "logic.archive.read",
            json!({"archive": arch, "request": {"capture_id": "../../etc", "kind": "edges"}}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    let e = fw
        .call(
            "logic.archive.read",
            json!({"archive": arch, "request": {"capture_id": "lc-1-1", "kind": "edges"}}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let e = fw
        .call(
            "logic.export",
            json!({"archive": arch, "request": {"capture_id": "lc-1-1", "kind": "edges", "format": "txt"}}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    let mut bad = arch.clone();
    bad["root"] = json!("relative");
    let e = fw
        .call("logic.archive.list", json!({"archive": bad}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    // a start with malformed archive args is refused before anything is spawned
    let e = fw
        .call("logic.start", json!({"config": {"source": "synthetic", "decoders": ["uart:rx=D0"]}, "engine": "/nonexistent", "archive": bad}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(fw.drain_published().is_empty());
}
