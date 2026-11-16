//! Measured load acceptance (finding L1-M). `#[ignore]`: run explicitly, record the printed numbers.
//!   CARGO_TARGET_DIR=… cargo test -p loom-logic --release --test load -- --ignored --nocapture
//! Synthetic + real sigrok-cli on stdin only (no device). Asserts honesty, prints throughput.
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use loom_logic::KIND_CAPTURE_STATE;
use loom_logic::archive::{ArchiveArgs, ArchiveConfig};
use loom_logic::worker::LogicWorker;
use loom_testkit::FakeWorker;
use serde_json::{Value, json};

fn engine() -> Option<String> {
    let p = loom_logic::model::resolve_engine(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}

fn measure(name: &str, cfg: Value) {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("loom-logic-load-{name}-{}-{n}", std::process::id()));
    let arch = ArchiveArgs::Enabled(ArchiveConfig {
        root: root.clone(),
        max_capture_bytes: 1 << 30,
        max_total_bytes: 4 << 30,
        max_age_s: 3600,
        max_captures: 4,
    })
    .to_args();
    let mut fw = FakeWorker::new(LogicWorker::new());
    let t0 = Instant::now();
    fw.call(
        "logic.start",
        json!({"config": cfg, "engine": e, "archive": arch}),
    )
    .unwrap();
    let fin = loop {
        let ev = fw.drain_published();
        if let Some(s) = ev
            .iter()
            .rev()
            .find(|e| e.kind == KIND_CAPTURE_STATE)
            .map(|e| e.payload.clone())
            && (s["state"] == "stopped" || s["state"] == "failed")
        {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(50));
    };
    let wall = t0.elapsed().as_secs_f64();
    let k = &fin["counters"];
    let a = &fin["archive"];
    println!(
        "LOAD {name}: state={} reason={} error={} wall={wall:.2}s samples={} published={} dropped={} edges={} dropped_edges={} decoder_input_bytes={} | archive state={} rows={} raw_end={} raw_bytes={} trunc_raw={} trunc_ann={}",
        fin["state"],
        fin["reason"],
        fin["error"],
        k["samples"],
        k["annotations"],
        k["dropped_annotations"],
        k["edges"],
        k["dropped_edges"],
        k["decoder_input_bytes"],
        a["state"],
        a["annotation_rows"],
        a["raw_end_sample"],
        a["raw_bytes"],
        a["raw_truncated_at_sample"],
        a["annotation_truncated_at_row"]
    );
    // honesty, whatever the throughput: a complete archive holds every acquired sample and every decoded row
    if a["state"] == "complete" {
        assert_eq!(a["raw_end_sample"], k["samples"]);
        assert_eq!(
            a["annotation_rows"].as_u64().unwrap(),
            k["annotations"].as_u64().unwrap() + k["dropped_annotations"].as_u64().unwrap()
        );
    } else {
        assert!(a["state"] == "partial" || a["state"] == "failed", "{a}");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "measured load acceptance; run explicitly"]
fn uart_115200_near_continuous_10s() {
    measure(
        "uart115k",
        json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=115200"],
                               "text": "A".repeat(256), "frame_period_ms": 23, "duration_ms": 10_000}),
    );
}

#[test]
#[ignore = "measured load acceptance; run explicitly"]
fn spi_1mhz_at_10msps_10s() {
    measure(
        "spi1m",
        json!({"source": "synthetic", "samplerate": 10_000_000, "clock_hz": 1_000_000,
                            "decoders": ["spi:clk=D0:mosi=D1:cs=D2"], "text": "A".repeat(256), "frame_period_ms": 3, "duration_ms": 10_000}),
    );
}
