//! Software trigger (operator-upgrade logic B5, finding L7) — Opus spec tests.
//! Pure model checks + real sigrok-cli on synthetic bytes (skipped when absent). No device.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use loom_logic::archive::{ArchiveArgs, ArchiveConfig};
use loom_logic::model::{PreRing, StartConfig, Trigger, TriggerDetector, TriggerEdge, parse_annotation};
use loom_logic::worker::LogicWorker;
use loom_logic::{KIND_ANNOTATIONS, KIND_CAPTURE_STATE, KIND_EDGES};
use loom_proto::error::ErrorCode;
use loom_proto::event::PublishedRecord;
use loom_testkit::FakeWorker;
use serde_json::{Value, json};

fn trig(v: Value) -> Result<Trigger, ErrorCode> {
    Trigger::parse(&v, 10_000).map_err(|e| e.code)
}

#[test]
fn trigger_parameter_is_strict_and_round_trips() {
    let t = trig(json!({"channel": "D3", "edge": "rising", "pre_ms": 10, "post_ms": 500})).unwrap();
    assert_eq!(
        t,
        Trigger {
            bit: 3,
            edge: TriggerEdge::Rising,
            pre_ms: 10,
            post_ms: 500
        }
    );
    assert_eq!(
        t.to_json(),
        json!({"channel": "D3", "edge": "rising", "pre_ms": 10, "post_ms": 500})
    );
    assert_eq!(trig(t.to_json()).unwrap(), t);
    assert_eq!(
        trig(json!({"channel": "D0", "edge": "either", "post_ms": 1}))
            .unwrap()
            .pre_ms,
        0
    );
    assert_eq!(
        [TriggerEdge::Rising, TriggerEdge::Falling, TriggerEdge::Either].map(TriggerEdge::as_str),
        ["rising", "falling", "either"]
    );
    for bad in [
        json!({"channel": "D8", "edge": "rising", "post_ms": 5}),
        json!({"channel": "D1", "edge": "high", "post_ms": 5}),
        json!({"channel": "D1", "edge": "rising", "pre_ms": 1001, "post_ms": 5}),
        json!({"channel": "D1", "edge": "rising", "post_ms": 0}),
        json!({"channel": "D1", "edge": "rising", "post_ms": 10_001}),
        json!({"channel": "D1", "edge": "rising"}),
        json!({"channel": "D1", "edge": "rising", "post_ms": 5, "level": 1}),
        json!("D1:rising"),
    ] {
        assert_eq!(trig(bad.clone()).unwrap_err(), ErrorCode::Usage, "{bad}");
    }
    assert_eq!(
        (t.pre_samples(1_000_000), t.post_samples(1_000_000)),
        (10_000, 500_000)
    );
    let tiny = Trigger {
        bit: 0,
        edge: TriggerEdge::Falling,
        pre_ms: 0,
        post_ms: 1,
    };
    assert_eq!((tiny.pre_samples(1_000), tiny.post_samples(1_000)), (0, 1));
    // start config: any source, normalized, duration bound
    let p = json!({"source": "sigrok-demo", "duration_ms": 2000, "trigger": {"channel": "D2", "edge": "falling", "post_ms": 1500}});
    let c = StartConfig::from_params(&p).unwrap();
    assert_eq!(c.trigger.unwrap().bit, 2);
    assert_eq!(
        c.to_params()["trigger"],
        json!({"channel": "D2", "edge": "falling", "pre_ms": 0, "post_ms": 1500})
    );
    assert_eq!(StartConfig::from_params(&c.to_params()).unwrap(), c);
    let mut over = p.clone();
    over["trigger"]["post_ms"] = json!(2001);
    assert!(StartConfig::from_params(&over).is_err());
    assert!(
        StartConfig::from_params(&json!({"source": "sigrok-demo"}))
            .unwrap()
            .to_params()
            .get("trigger")
            .is_none()
    );
}

#[test]
fn detector_fires_on_the_requested_edge_across_chunks() {
    let mk = |edge| {
        TriggerDetector::new(&Trigger {
            bit: 3,
            edge,
            pre_ms: 0,
            post_ms: 1,
        })
    };
    let mut d = mk(TriggerEdge::Rising);
    assert_eq!(d.feed(&[0x08, 0x08]), None, "the first sample never fires");
    let mut d2 = mk(TriggerEdge::Rising);
    assert_eq!(d2.feed(&[0x00, 0x00]), None);
    assert_eq!(
        d2.feed(&[0x08, 0x00, 0x08]),
        Some(0),
        "edge against the previous chunk's last sample"
    );
    assert_eq!(
        d2.feed(&[0x08]),
        None,
        "remembers the last sample of the previous call"
    );
    assert_eq!(d.feed(&[0x00, 0x01, 0x08]), Some(2));
    let mut f = mk(TriggerEdge::Falling);
    assert_eq!(f.feed(&[0x08, 0x0F, 0x07, 0x08]), Some(2));
    let mut e = mk(TriggerEdge::Either);
    assert_eq!(
        e.feed(&[0x00, 0x01, 0xF7, 0x08]),
        Some(3),
        "other bits never fire"
    );
}

#[test]
fn pre_ring_keeps_the_last_window_with_absolute_numbering() {
    let mut r = PreRing::new(5);
    r.push(&[1, 2, 3]);
    r.push(&[4, 5, 6, 7]);
    assert_eq!(r.take(), (2, vec![3, 4, 5, 6, 7]));
    assert_eq!(r.take(), (7, vec![]));
    let mut z = PreRing::new(0);
    z.push(&[1, 2]);
    assert_eq!(z.take(), (2, vec![]));
}

#[test]
fn decoder_samples_shift_to_the_absolute_axis() {
    let a = parse_annotation("10-90 uart-1: 41").shifted(24_020);
    assert_eq!((a.start, a.end), (Some(24_030), Some(24_110)));
    let raw = parse_annotation("garbage").shifted(5);
    assert_eq!((raw.start, raw.text.as_str()), (None, "garbage"));
}

// ---------------------------------------------------------------------------------------------

fn engine() -> Option<String> {
    let p = loom_logic::model::resolve_engine(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}

fn run(fw: &mut FakeWorker<LogicWorker>, args: Value) -> (Vec<PublishedRecord>, Value) {
    fw.call("logic.start", args).unwrap();
    let t0 = Instant::now();
    let mut ev = Vec::new();
    loop {
        ev.extend(fw.drain_published());
        if let Some(s) = ev
            .iter()
            .rev()
            .find(|x| x.kind == KIND_CAPTURE_STATE)
            .map(|x| x.payload.clone())
            && (s["state"] == "stopped" || s["state"] == "failed")
        {
            return (ev, s);
        }
        assert!(t0.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Tmp(std::path::PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(name: &str) -> (Tmp, Value) {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!("loom-logic-trig-{name}-{}-{n}", std::process::id()));
    let a = ArchiveArgs::Enabled(ArchiveConfig {
        root: p.clone(),
        max_capture_bytes: 64 << 20,
        max_total_bytes: 1 << 30,
        max_age_s: 3600,
        max_captures: 8,
    })
    .to_args();
    (Tmp(p), a)
}

#[test]
fn fired_trigger_publishes_from_the_pre_window_on_the_absolute_axis() {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let (_t, arch) = tmp("fired");
    let mut fw = FakeWorker::new(LogicWorker::new());
    // 1 MS/s, 100 kBd, 50 ms frames: RX bytes from 20 (+50 000·f), TX bytes from 25 020 (+50 000·f).
    // Trigger on the first TX start bit (D1 falling at 25 020), keep 1 ms before it, 120 ms after it.
    let cfg = json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:tx=D1:baudrate=100000"],
                     "text": "LOOM", "frame_period_ms": 50, "duration_ms": 5000,
                     "trigger": {"channel": "D1", "edge": "falling", "pre_ms": 1, "post_ms": 120}});
    let (ev, fin) = run(&mut fw, json!({"config": cfg, "engine": e, "archive": arch}));
    assert_eq!(
        (fin["state"].clone(), fin["reason"].clone()),
        (json!("stopped"), json!("trigger_post")),
        "{fin}"
    );
    assert_eq!(
        fin["trigger"],
        json!({"state": "fired", "sample": 25_020, "channel": "D1", "edge": "falling", "pre_ms": 1, "post_ms": 120})
    );
    let states: Vec<&Value> = ev
        .iter()
        .filter(|x| x.kind == KIND_CAPTURE_STATE)
        .map(|x| &x.payload)
        .collect();
    assert!(
        states
            .iter()
            .any(|s| s["state"] == "running" && s["trigger"]["state"] == "armed")
    );
    let edges: Vec<&Value> = ev
        .iter()
        .filter(|x| x.kind == KIND_EDGES)
        .map(|x| &x.payload)
        .collect();
    assert_eq!(
        edges.first().unwrap()["sample_start"],
        24_020,
        "publishing starts at the pre-window start"
    );
    assert_eq!(
        edges.last().unwrap()["sample_end"],
        145_020,
        "and ends exactly post samples after the trigger"
    );
    let rows: Vec<Value> = ev
        .iter()
        .filter(|x| x.kind == KIND_ANNOTATIONS)
        .flat_map(|x| x.payload["annotations"].as_array().cloned().unwrap_or_default())
        .filter(|r| r["role"] == "data")
        .collect();
    let aligned = |dec: &str, first: u64| {
        let r: Vec<u64> = rows
            .iter()
            .filter(|r| r["decoder"] == dec)
            .map(|r| r["start_sample"].as_u64().unwrap())
            .collect();
        assert!(r.len() >= 8, "{dec}: {r:?}");
        assert!(
            r.iter()
                .all(|s| *s >= 24_020 && ((s - first) % 50_000).is_multiple_of(100)),
            "{dec} rows on the absolute axis: {r:?}"
        );
    };
    aligned("uart-1", 30); // data starts one bit after each RX start bit
    aligned("uart-2", 25_030);
    // archive: same window, complete
    let a = &fin["archive"];
    assert_eq!(
        (a["state"].clone(), a["raw_end_sample"].clone()),
        (json!("complete"), json!(145_020))
    );
    let id = fin["capture_id"].as_str().unwrap();
    let r = fw
        .call(
            "logic.archive.read",
            json!({"archive": arch, "request": {"capture_id": id, "kind": "edges", "limit": 16}}),
        )
        .unwrap();
    assert_eq!(r["chunks"][0]["sample_start"], 24_020);
}

#[test]
fn trigger_that_never_fires_publishes_nothing_and_says_so() {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let (_t, arch) = tmp("notfired");
    let mut fw = FakeWorker::new(LogicWorker::new());
    let cfg = json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=100000"],
                     "duration_ms": 400, "trigger": {"channel": "D5", "edge": "rising", "post_ms": 100}});
    let (ev, fin) = run(&mut fw, json!({"config": cfg, "engine": e, "archive": arch}));
    assert_eq!(
        (fin["state"].clone(), fin["reason"].clone()),
        (json!("stopped"), json!("duration")),
        "{fin}"
    );
    assert_eq!(fin["trigger"]["state"], "not_fired");
    assert_eq!(fin["trigger"]["sample"], Value::Null);
    assert!(
        fin["counters"]["samples"].as_u64().unwrap() > 300_000,
        "acquisition ran while armed"
    );
    assert!(
        ev.iter()
            .all(|x| x.kind != KIND_EDGES && x.kind != KIND_ANNOTATIONS),
        "nothing published before a trigger"
    );
    assert_eq!(
        fin["archive"]["state"], "complete",
        "an empty archive range is still exact"
    );
    assert_eq!(fin["archive"]["raw_end_sample"], fin["counters"]["samples"]);
}
