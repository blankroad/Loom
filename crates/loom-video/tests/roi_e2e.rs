//! Operator upgrade B5b end-to-end (FUNCTIONS.md §Batch 5b), Opus-authored. Real FFmpeg replays a clip it
//! generates itself (lavfi: 2 s black → 4 s white, 160×120@10) inside a private `/tmp/loom-video-roi-*` home;
//! FakeWorker (SimClock: monotonic time stays 0, so the specs use dwell/cooldown 0 and evidence completes
//! by frame count). No device, no daemon, no python. Skipped with a message when ffmpeg is unavailable.
//! V-01/V-02: server-side ROI judgment from the same frames, published once as events (UI = agent);
//! V-04: before/after evidence bundle with byte-exact JPEGs; V-07: config survives reopen, baselines do not.
//! Sonnet must not edit this file.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;
use loom_domain_api::Reducer as _;
use loom_proto::error::ErrorCode;
use loom_proto::event::PublishedRecord;
use loom_proto::rpc::SubFilter;
use loom_testkit::FakeWorker;
use loom_video::gateway::VideoReducer;
use loom_video::inspect::EVIDENCE_SCHEMA;
use loom_video::model;
use loom_video::worker::VideoWorker;
use loom_video::{
    KIND_CAPTURE_STATE, KIND_EVIDENCE, KIND_ROI_CONFIG, KIND_ROI_METRIC, KIND_ROI_STATE, ROI_IDLE_CHANNEL,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const CLIP: &str = "roi-clip.mkv";

fn ffmpeg() -> Option<String> {
    let p = model::resolve_ffmpeg(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}
macro_rules! need {
    () => {
        match ffmpeg() {
            Some(f) => f,
            None => {
                eprintln!("SKIP: ffmpeg is unavailable");
                return;
            }
        }
    };
}

struct Home(PathBuf);
impl Home {
    fn new(tag: &str) -> Home {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = PathBuf::from(format!("/tmp/loom-video-roi-{}-{tag}{n}", std::process::id()));
        std::fs::create_dir_all(p.join("records/2026-11-16/s1")).unwrap();
        Home(p)
    }
    fn session(&self) -> PathBuf {
        self.0.join("records/2026-11-16/s1")
    }
    /// 2 s black then 4 s white, MJPEG in Matroska, 160×120 @ 10 fps.
    fn clip(&self, ff: &str) {
        let dir = model::replay_dir(&self.0);
        std::fs::create_dir_all(&dir).unwrap();
        let ok = Command::new(ff)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "lavfi", "-i", "color=c=black:s=160x120:r=10:d=2"])
            .args(["-f", "lavfi", "-i", "color=c=white:s=160x120:r=10:d=4"])
            .args([
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1[v]",
                "-map",
                "[v]",
                "-c:v",
                "mjpeg",
                "-q:v",
                "3",
            ])
            .arg(dir.join(CLIP))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(ok.success(), "lavfi clip generation failed");
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    fw: FakeWorker<VideoWorker>,
    ffmpeg: String,
    storage: Value,
    seen: Vec<PublishedRecord>,
}
impl Run {
    fn new(ffmpeg: String, home: &Home) -> Run {
        Run {
            fw: FakeWorker::new(VideoWorker::new()),
            ffmpeg,
            storage: json!({"home": home.0, "session": home.session()}),
            seen: vec![],
        }
    }
    fn call(&mut self, m: &str, mut args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        args["storage"] = self.storage.clone();
        args["ffmpeg"] = json!(self.ffmpeg);
        let r = self.fw.call(m, args);
        self.drain();
        r
    }
    fn drain(&mut self) {
        let mut d = self.fw.drain_published();
        self.seen.append(&mut d);
    }
    fn open_replay(&mut self) -> String {
        let v = self
            .call(
                "video.open",
                json!({"config": {"source": "replay", "name": CLIP, "width": 160, "height": 120, "fps": 10}}),
            )
            .unwrap();
        assert_eq!(v["state"], "live", "{v}");
        v["capture_id"].as_str().unwrap().to_owned()
    }
    fn status(&mut self) -> Value {
        let s = self.fw.call("video.status", json!({})).unwrap();
        self.drain();
        s
    }
    fn until(&mut self, limit: Duration, what: &str, pred: impl Fn(&Run, &Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            let st = self.status();
            if pred(self, &st) {
                return st;
            }
            assert!(t0.elapsed() < limit, "timed out waiting for {what}; status {st}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn events(&self, kind: &str) -> Vec<&PublishedRecord> {
        self.seen.iter().filter(|r| r.kind == kind).collect()
    }
    fn roi_states(&self, cap: &str, roi: &str) -> Vec<Value> {
        self.events(KIND_ROI_STATE)
            .into_iter()
            .filter(|r| r.channel.as_deref() == Some(cap) && r.payload["roi"] == roi)
            .map(|r| r.payload.clone())
            .collect()
    }
}

fn last(st: &Value) -> u64 {
    st["health"]["last_frame_id"].as_u64().unwrap_or(0)
}

fn scene() -> Value {
    json!({"name": "scene", "x": 0.25, "y": 0.25, "w": 0.5, "h": 0.5, "metric": "mean_abs_diff", "enter": 30, "exit": 10,
           "dwell_ms": 0, "cooldown_ms": 0, "evidence_pre": 2, "evidence_post": 2})
}

/// Config edits are worker-scoped and need no capture; refusals are explicit; idle edits are published.
#[test]
fn roi_config_without_capture() {
    let home = Home::new("cfg");
    let mut run = Run::new("/usr/bin/false".into(), &home);
    let st = run.status();
    assert_eq!(
        st["roi"],
        json!({"config": {"config_rev": 0, "rois": []}, "inspect": null}),
        "{st}"
    );
    let p = run.call("video.roi.put", json!({"roi": scene()})).unwrap();
    assert_eq!(
        (
            p["config_rev"].as_u64(),
            p["roi"].as_str(),
            p["rois"].as_array().map(Vec::len)
        ),
        (Some(1), Some("scene"), Some(1))
    );
    let ev = run.events(KIND_ROI_CONFIG);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].channel.as_deref(), Some(ROI_IDLE_CHANNEL));
    assert_eq!(ev[0].payload["capture_id"], Value::Null);
    assert_eq!(ev[0].payload["config_rev"], 1);
    assert_eq!(
        ev[0].payload["change"],
        json!({"op": "put", "roi": "scene", "expect_rev": null})
    );
    let e = run
        .call("video.roi.put", json!({"roi": scene(), "expect_rev": 0}))
        .unwrap_err();
    assert_eq!(
        (
            e.code,
            e.detail["reason"].as_str(),
            e.detail["config_rev"].as_u64()
        ),
        (ErrorCode::Busy, Some("config_rev_mismatch"), Some(1))
    );
    let e = run
        .call("video.roi.put", json!({"roi": {"name": "x"}}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("roi.x"), "{}", e.message);
    assert_eq!(
        run.call("video.roi.put", json!({"roi": "scene"}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
    assert_eq!(
        run.call("video.roi.put", json!({"roi": scene(), "expect_rev": "1"}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
    // list (recipe) → replace re-applies it; remove absent is not an edit
    let list = run.call("video.roi.list", json!({})).unwrap();
    assert_eq!(
        (
            list["config_rev"].as_u64(),
            list["capture_id"].clone(),
            list["inspect"].clone()
        ),
        (Some(1), Value::Null, Value::Null)
    );
    assert_eq!(list["limits"]["max_rois"], 8);
    assert_eq!(list["limits"]["analysis_hz"], 10);
    assert!(list["note"].as_str().unwrap().contains("no scene understanding"));
    let r = run
        .call(
            "video.roi.replace",
            json!({"rois": list["rois"], "expect_rev": 1}),
        )
        .unwrap();
    assert_eq!(
        (r["config_rev"].as_u64(), r["rois"].clone()),
        (Some(2), list["rois"].clone())
    );
    assert_eq!(
        run.call("video.roi.replace", json!({"rois": [scene(), scene()]}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
    let n = run.events(KIND_ROI_CONFIG).len();
    let rm = run.call("video.roi.remove", json!({"roi": "nope"})).unwrap();
    assert_eq!(
        (rm["removed"].as_bool(), rm["config_rev"].as_u64()),
        (Some(false), Some(2))
    );
    assert_eq!(run.events(KIND_ROI_CONFIG).len(), n, "no edit, no event");
    let rm = run
        .call("video.roi.remove", json!({"roi": "scene", "expect_rev": 2}))
        .unwrap();
    assert_eq!(
        (
            rm["removed"].as_bool(),
            rm["config_rev"].as_u64(),
            rm["rois"].clone()
        ),
        (Some(true), Some(3), json!([]))
    );
    assert_eq!(
        run.events(KIND_ROI_CONFIG).last().unwrap().payload["change"]["op"],
        "remove"
    );
    assert_eq!(
        run.call("video.roi.remove", json!({})).unwrap_err().code,
        ErrorCode::Usage
    );
    // baselines need a capture
    assert_eq!(
        run.call("video.roi.baseline", json!({})).unwrap_err().code,
        ErrorCode::NotOpen
    );
}

/// The whole V-01/V-02/V-04 path on real frames.
#[test]
fn roi_judgment_evidence_reducer_and_reopen() {
    let ff = need!();
    let home = Home::new("e2e");
    home.clip(&ff);
    let mut run = Run::new(ff, &home);
    run.call("video.roi.put", json!({"roi": scene()})).unwrap();
    let cap = run.open_replay();
    // the config is announced on the new capture's channel
    let open_cfg = run
        .events(KIND_ROI_CONFIG)
        .into_iter()
        .find(|r| r.channel.as_deref() == Some(cap.as_str()))
        .map(|r| r.payload.clone());
    let open_cfg = open_cfg.unwrap_or_else(|| panic!("roi_config on {cap}"));
    assert_eq!(
        (
            open_cfg["change"]["op"].as_str(),
            open_cfg["config_rev"].as_u64(),
            open_cfg["capture_id"].as_str()
        ),
        (Some("open"), Some(1), Some(cap.as_str()))
    );

    // baseline = one exact black frame
    run.until(Duration::from_secs(10), "3 frames", |_, s| last(s) >= 3);
    let b = run.call("video.roi.baseline", json!({})).unwrap();
    let f0 = b["frame_id"].as_u64().unwrap();
    assert!(
        f0 <= 15,
        "baseline taken while the clip is still black (frame {f0})"
    );
    assert_eq!(b["capture_id"], cap.as_str());
    assert_eq!(b["retained_by"], "ring");
    assert_eq!(
        (
            b["baselines"][0]["roi"].as_str(),
            b["baselines"][0]["baseline"]["frame_id"].as_u64()
        ),
        (Some("scene"), Some(f0))
    );
    let meta = run.call("video.frame", json!({"frame_id": f0})).unwrap();
    assert_eq!(
        b["sha256"], meta["sha256"],
        "baseline identity = that exact frame"
    );
    let bev = run
        .events(KIND_ROI_CONFIG)
        .into_iter()
        .rev()
        .find(|r| r.payload["change"]["op"] == "baseline")
        .map(|r| r.payload.clone())
        .unwrap();
    assert_eq!(
        (bev["change"]["frame_id"].as_u64(), bev["capture_id"].as_str()),
        (Some(f0), Some(cap.as_str()))
    );
    assert_eq!(
        run.call("video.roi.baseline", json!({"roi": "zz"}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );

    // judgment: normal on black, changed on white — computed once by the worker, published as events
    let cap2 = cap.clone();
    run.until(Duration::from_secs(15), "roi changed", move |r, _| {
        r.roi_states(&cap2, "scene")
            .iter()
            .any(|p| p["state"] == "changed")
    });
    let states = run.roi_states(&cap, "scene");
    let names: Vec<&str> = states.iter().map(|p| p["state"].as_str().unwrap()).collect();
    let first_changed = names.iter().position(|s| *s == "changed").unwrap();
    assert_eq!(states[0]["state"], "unknown", "{names:?}");
    assert!(
        names[..first_changed].contains(&"normal"),
        "normal before changed: {names:?}"
    );
    let seqs: Vec<u64> = states
        .iter()
        .map(|p| p["transition_seq"].as_u64().unwrap())
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "every transition published once, in order: {seqs:?}"
    );
    let ch = &states[first_changed];
    assert_eq!(ch["capture_id"], cap.as_str());
    assert!(ch["capture_epoch"].is_string());
    assert!(ch["metrics"]["mean_abs_diff"].as_f64().unwrap() >= 90.0, "{ch}");
    assert_eq!(ch["baseline"]["frame_id"].as_u64(), Some(f0));
    let trig = ch["frame_id"].as_u64().unwrap();
    assert!(trig > f0);
    assert_eq!(
        (ch["evidence"].as_bool(), ch["config_rev"].as_u64()),
        (Some(true), Some(1))
    );
    assert!(ch["note"].as_str().unwrap().contains("no scene understanding"));

    // evidence: collecting → written; byte-exact JPEGs inside one JSON manifest
    let cap3 = cap.clone();
    run.until(Duration::from_secs(15), "evidence written", move |r, _| {
        r.events(KIND_EVIDENCE)
            .iter()
            .any(|e| e.channel.as_deref() == Some(cap3.as_str()) && e.payload["state"] == "written")
    });
    let ev: Vec<Value> = run
        .events(KIND_EVIDENCE)
        .iter()
        .map(|e| e.payload.clone())
        .collect();
    assert_eq!(ev[0]["state"], "collecting", "{ev:?}");
    assert_eq!(
        (ev[0]["evidence_id"].as_u64(), ev[0]["trigger_frame_id"].as_u64()),
        (Some(1), Some(trig))
    );
    let w = ev.iter().find(|e| e["state"] == "written").unwrap();
    let aid = w["artifact_id"].as_str().unwrap();
    assert_eq!(aid, format!("evid-{cap}-1"));
    assert_eq!(w["media_path"], format!("/api/v1/video/artifact/{aid}"));
    assert_eq!(w["partial"], json!([]), "{w}");
    let roles: Vec<(String, u64)> = w["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["role"].as_str().unwrap().to_owned(),
                f["frame_id"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        roles,
        vec![
            ("pre".into(), trig - 2),
            ("pre".into(), trig - 1),
            ("trigger".into(), trig),
            ("post".into(), trig + 1),
            ("post".into(), trig + 2)
        ]
    );
    let (_, path) = model::artifact_path(&home.session(), aid).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(w["size"].as_u64(), Some(bytes.len() as u64));
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let m: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        (
            m["schema"].as_str(),
            m["artifact_id"].as_str(),
            m["capture_id"].as_str()
        ),
        (Some(EVIDENCE_SCHEMA), Some(aid), Some(cap.as_str()))
    );
    assert_eq!(
        (
            m["roi"].as_str(),
            m["transition_seq"].clone(),
            m["config_rev"].as_u64()
        ),
        (Some("scene"), ch["transition_seq"].clone(), Some(1))
    );
    assert_eq!(m["spec"]["name"], "scene");
    assert_eq!(m["spec"]["enter"], 30.0);
    assert_eq!(m["baseline"]["frame_id"].as_u64(), Some(f0));
    assert_eq!(m["transition"]["to"], "changed");
    assert_eq!(m["source"]["source"], "replay");
    assert!(m["note"].as_str().unwrap().contains("no scene understanding"));
    let frames = m["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 5);
    for f in frames {
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(f["jpeg_b64"].as_str().unwrap())
            .unwrap();
        assert_eq!(jpeg.len() as u64, f["size"].as_u64().unwrap());
        assert_eq!(
            hex::encode(Sha256::digest(&jpeg)),
            f["sha256"].as_str().unwrap(),
            "byte-exact frame {}",
            f["frame_id"]
        );
        assert!(f["recv_utc_ns"].is_string() && f["recv_mono_ns"].is_string());
    }
    let st = run.status();
    assert_eq!(
        st["pins"]["count"], 0,
        "evidence pins released after writing: {}",
        st["pins"]
    );

    // end of clip → closed → ROI UNKNOWN(not_live); metrics were published at ≤ 10 Hz inspection
    let st = run.until(Duration::from_secs(20), "closed", |_, s| {
        s["capture"]["state"] == "closed"
    });
    let cap4 = cap.clone();
    run.until(Duration::from_secs(5), "final not_live", move |r, _| {
        r.roi_states(&cap4, "scene")
            .last()
            .is_some_and(|p| p["reason"] == "not_live")
    });
    let insp = &st["roi"]["inspect"];
    assert_eq!(st["roi"]["config"]["config_rev"], 1);
    assert!(insp["analysis"]["analysed"].as_u64().unwrap() >= 5, "{insp}");
    assert!(!insp["rois"][0]["history"].as_array().unwrap().is_empty());
    let met: Vec<&PublishedRecord> = run
        .events(KIND_ROI_METRIC)
        .into_iter()
        .filter(|r| r.channel.as_deref() == Some(cap.as_str()))
        .collect();
    assert!(!met.is_empty(), "1 s roi_metric published");
    assert_eq!(
        (
            met[0].payload["inspection_hz_max"].as_u64(),
            met[0].payload["rois"][0]["roi"].as_str()
        ),
        (Some(10), Some("scene"))
    );
    let list = run.call("video.roi.list", json!({})).unwrap();
    assert_eq!(
        (
            list["capture_id"].as_str(),
            list["inspect"]["rois"][0]["baseline"]["frame_id"].as_u64()
        ),
        (Some(cap.as_str()), Some(f0))
    );

    // late joiners: the reducer rebuilds the same view from the journal records
    let mut red = VideoReducer::default();
    for r in &run.seen {
        if red.kinds().contains(&r.kind.as_str()) {
            red.apply(r);
        }
    }
    let snap = red.snapshot(&SubFilter::default()).remove(0);
    assert_eq!(snap["roi_config"]["config_rev"], 1, "{snap}");
    let rs = snap["roi_states"].as_array().unwrap();
    let mine = rs
        .iter()
        .find(|p| p["capture_id"] == cap.as_str() && p["roi"] == "scene")
        .unwrap();
    assert_eq!(
        (mine["state"].as_str(), mine["reason"].as_str()),
        (Some("unknown"), Some("not_live"))
    );
    assert!(
        snap["roi_metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["capture_id"] == cap.as_str())
    );
    let evs = snap["evidence"].as_array().unwrap();
    assert!(
        evs.iter()
            .any(|e| e["capture_id"] == cap.as_str() && e["evidence_id"] == 1 && e["state"] == "written"),
        "latest per bundle: {evs:?}"
    );
    assert_eq!(
        evs.iter()
            .filter(|e| e["capture_id"] == cap.as_str() && e["evidence_id"] == 1)
            .count(),
        1
    );
    assert!(run.events(KIND_CAPTURE_STATE).len() >= 3);

    // reopen: the config survives (worker-scoped), the baseline does not (capture-scoped)
    let capb = run.open_replay();
    assert_ne!(capb, cap);
    let capb2 = capb.clone();
    run.until(
        Duration::from_secs(10),
        "no_baseline on the new capture",
        move |r, _| {
            r.roi_states(&capb2, "scene")
                .iter()
                .any(|p| p["reason"] == "no_baseline")
        },
    );
    let list = run.call("video.roi.list", json!({})).unwrap();
    assert_eq!(
        (list["config_rev"].as_u64(), list["capture_id"].as_str()),
        (Some(1), Some(capb.as_str()))
    );
    assert_eq!(list["inspect"]["rois"][0]["baseline"], Value::Null);
    run.call("video.close", json!({})).unwrap();
}

/// STOP latch: judgments go UNKNOWN(not_live) at once; config edits and baselines stay memory-only and allowed.
#[test]
fn stop_latch_makes_judgments_unknown() {
    let ff = need!();
    let home = Home::new("stop");
    home.clip(&ff);
    let mut run = Run::new(ff, &home);
    run.call("video.roi.put", json!({"roi": scene()})).unwrap();
    let cap = run.open_replay();
    run.until(Duration::from_secs(10), "3 frames", |_, s| last(s) >= 3);
    run.call("video.roi.baseline", json!({})).unwrap();
    let c1 = cap.clone();
    run.until(Duration::from_secs(10), "normal", move |r, _| {
        r.roi_states(&c1, "scene")
            .last()
            .is_some_and(|p| p["state"] == "normal")
    });
    run.fw.latch(true);
    let c2 = cap.clone();
    run.until(
        Duration::from_secs(5),
        "unknown(not_live) after STOP",
        move |r, _| {
            r.roi_states(&c2, "scene")
                .last()
                .is_some_and(|p| p["state"] == "unknown" && p["reason"] == "not_live")
        },
    );
    assert!(
        !run.roi_states(&cap, "scene")
            .iter()
            .any(|p| p["state"] == "changed"),
        "no judgment after STOP; nothing inferred"
    );
    let p = run
        .call(
            "video.roi.put",
            json!({"roi": {"name": "corner", "x": 0, "y": 0, "w": 0.25, "h": 0.25, "enter": 20}}),
        )
        .unwrap();
    assert_eq!(
        p["config_rev"], 2,
        "memory-only config edit allowed while latched"
    );
    run.fw.latch(false);
}
