//! Worker against the real `sigrok-cli` (synthetic UART/SPI → libsigrok decoders, libsigrok demo
//! acquisition) and against fake engines (early exit, descendants holding pipes, missing exe).
//! Real-engine tests are skipped (with a message) when sigrok-cli is not installed.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loom_logic::worker::LogicWorker;
use loom_logic::{KIND_ANNOTATIONS, KIND_CAPTURE_STATE, KIND_EDGES};
use loom_proto::error::ErrorCode;
use loom_proto::event::PublishedRecord;
use loom_testkit::{FakeWorker, RegistryProbe};
use serde_json::{Value, json};

fn engine() -> Option<String> {
    let p = loom_logic::model::resolve_engine(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}

macro_rules! need_engine {
    () => {
        match engine() {
            Some(e) => e,
            None => {
                eprintln!("SKIP: sigrok-cli not installed");
                return;
            }
        }
    };
}

struct Run {
    fw: FakeWorker<LogicWorker>,
    events: Vec<PublishedRecord>,
}

impl Run {
    fn new() -> Self {
        Run {
            fw: FakeWorker::new(LogicWorker::new()),
            events: Vec::new(),
        }
    }
    fn start(
        &mut self,
        config: Value,
        engine: Option<&str>,
    ) -> Result<Value, loom_proto::worker::WorkerError> {
        self.fw
            .call("logic.start", json!({"config": config, "engine": engine}))
    }
    fn pull(&mut self) {
        self.events.extend(self.fw.drain_published());
    }
    fn texts(&self) -> Vec<String> {
        annotations(&self.events)
            .iter()
            .map(|a| a["text"].as_str().unwrap_or("").to_owned())
            .collect()
    }
    /// Poll until `pred` holds over collected events (or panic after `limit`).
    fn until(&mut self, limit: Duration, what: &str, pred: impl Fn(&[PublishedRecord]) -> bool) {
        let t0 = Instant::now();
        loop {
            self.pull();
            if pred(&self.events) {
                return;
            }
            assert!(
                t0.elapsed() < limit,
                "timed out waiting for {what}; states: {:?}",
                states(&self.events)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self) -> Value {
        let v = self.fw.call("logic.stop", json!({})).unwrap();
        self.pull();
        v
    }
}

fn annotations(ev: &[PublishedRecord]) -> Vec<Value> {
    ev.iter()
        .filter(|e| e.kind == KIND_ANNOTATIONS)
        .flat_map(|e| e.payload["annotations"].as_array().cloned().unwrap_or_default())
        .collect()
}

fn states(ev: &[PublishedRecord]) -> Vec<String> {
    ev.iter()
        .filter(|e| e.kind == KIND_CAPTURE_STATE)
        .map(|e| e.payload["state"].as_str().unwrap_or("").to_owned())
        .collect()
}

fn final_state(ev: &[PublishedRecord]) -> Value {
    ev.iter()
        .rev()
        .find(|e| e.kind == KIND_CAPTURE_STATE)
        .map(|e| e.payload.clone())
        .unwrap()
}

/// Edges events tile the sample axis with no hole and absolute, increasing edge samples.
fn assert_edges_contiguous(ev: &[PublishedRecord]) -> u64 {
    let mut next = 0u64;
    let mut n = 0;
    for e in ev.iter().filter(|e| e.kind == KIND_EDGES) {
        let p = &e.payload;
        let (a, b) = (
            p["sample_start"].as_u64().unwrap(),
            p["sample_end"].as_u64().unwrap(),
        );
        assert_eq!(a, next, "edges hole at {a}");
        assert!(b > a);
        for x in p["edges"].as_array().unwrap() {
            let s = x[0].as_u64().unwrap();
            assert!((a..b).contains(&s));
        }
        assert!(serde_json::to_string(p).unwrap().len() < loom_logic::model::MAX_PAYLOAD_BYTES);
        next = b;
        n += 1;
    }
    assert!(n > 0, "no edges events");
    next
}

fn hex(s: &str) -> Vec<String> {
    s.bytes().map(|b| format!("{b:02X}")).collect()
}

fn contains_run(hay: &[String], needle: &[String]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn registration() {
    let (reg, report) = RegistryProbe::assert_ok(&loom_logic::gateway::Logic);
    assert_eq!(
        report.commands,
        [
            "logic.status",
            "logic.start",
            "logic.stop",
            "logic.read",
            "logic.wait"
        ]
    );
    assert_eq!(report.write_commands, ["logic.start", "logic.stop"]);
    let stop = reg.commands().iter().find(|c| c.method == "logic.stop").unwrap();
    assert_eq!(stop.roles, loom_domain_api::RoleSet::READ);
}

#[test]
fn synthetic_uart_decodes_before_stop() {
    let e = need_engine!();
    let mut r = Run::new();
    let started = r
        .start(
            json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=115200"],
                   "text": "LOOM {n}\r\n", "frame_period_ms": 50, "duration_ms": 20_000}),
            Some(&e),
        )
        .unwrap();
    assert!(started["capture_id"].as_str().unwrap().starts_with("lc-"));
    let want = hex("LOOM 0001");
    r.until(Duration::from_secs(5), "LOOM 0001 decoded", |ev| {
        let t: Vec<String> = annotations(ev)
            .iter()
            .map(|a| a["text"].as_str().unwrap_or("").to_owned())
            .collect();
        contains_run(&t, &want)
    });
    // Decoded before any stop: the capture is still running.
    assert_eq!(states(&r.events).last().map(String::as_str), Some("running"));
    let fin = r.stop();
    assert_eq!(fin["state"], "stopped", "{fin}");
    assert_eq!(fin["reason"], "user_stop");
    assert_eq!(fin["exit"]["decoder"]["code"], 0);
    let total = assert_edges_contiguous(&r.events);
    assert_eq!(fin["counters"]["samples"].as_u64(), Some(total));
    assert_eq!(
        fin["counters"]["decoder_input_bytes"].as_u64(),
        Some(total),
        "decoder saw every sample"
    );
    // Class filter: only data words (two hex digits), no per-bit annotations.
    let texts = r.texts();
    assert!(
        texts
            .iter()
            .all(|t| t.len() == 2 && u8::from_str_radix(t, 16).is_ok()),
        "{texts:?}"
    );
    let a = &annotations(&r.events)[0];
    assert!(a["class"].is_null());
    assert!(a["decoder"].as_str().unwrap().starts_with("uart-"));
    assert_eq!(
        a["t_start_ns"].as_u64(),
        Some(a["start_sample"].as_u64().unwrap() * 1000)
    );
    assert!(
        r.events
            .iter()
            .filter(|e| e.kind != KIND_CAPTURE_STATE)
            .all(|e| e.quality.contains(&"synthetic".into()))
    );
}

#[test]
fn synthetic_spi_all_modes() {
    let e = need_engine!();
    for (cpol, cpha) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        for order in ["msb-first", "lsb-first"] {
            let mut r = Run::new();
            let spec = format!("spi:clk=D0:mosi=D1:cs=D2:cpol={cpol}:cpha={cpha}:bitorder={order}");
            r.start(
                json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": [spec], "text": "Z<{n}",
                       "frame_period_ms": 20, "duration_ms": 10_000, "clock_hz": 50_000}),
                Some(&e),
            )
            .unwrap();
            let want = hex("Z<0001");
            r.until(
                Duration::from_secs(5),
                &format!("spi mode {cpol}{cpha} {order}"),
                |ev| {
                    let t: Vec<String> = annotations(ev)
                        .iter()
                        .map(|a| a["text"].as_str().unwrap_or("").to_owned())
                        .collect();
                    contains_run(&t, &want)
                },
            );
            let fin = r.stop();
            assert_eq!(fin["state"], "stopped", "mode {cpol}{cpha}: {fin}");
        }
    }
}

#[test]
fn sigrok_demo_real_acquisition_and_clean_stop() {
    let e = need_engine!();
    let mut r = Run::new();
    r.start(
        json!({"source": "sigrok-demo", "pattern": "graycode", "duration_ms": 20_000}),
        Some(&e),
    )
    .unwrap();
    r.until(Duration::from_secs(5), "3 edges events", |ev| {
        ev.iter().filter(|e| e.kind == KIND_EDGES).count() >= 3
    });
    let t0 = Instant::now();
    let fin = r.stop();
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "stop took {:?}",
        t0.elapsed()
    );
    assert_eq!(fin["state"], "stopped", "{fin}");
    assert_eq!(fin["exit"]["acquisition"]["stop_style"], "key");
    assert_eq!(fin["exit"]["acquisition"]["code"], 0);
    assert_edges_contiguous(&r.events);
    // graycode at 200 kHz toggles every sample: bounded events, loss counted, never silent.
    assert!(fin["counters"]["dropped_edges"].as_u64().unwrap() > 0);
    assert!(
        r.events
            .iter()
            .any(|e| e.kind == KIND_EDGES && e.quality.contains(&"edges_dropped".into()))
    );
}

#[test]
fn sigrok_demo_with_decoder_tee_and_sample_bound() {
    let e = need_engine!();
    let mut r = Run::new();
    r.start(
        json!({"source": "sigrok-demo", "samplerate": 1_000_000, "decoders": ["spi:clk=D0:mosi=D1"],
               "max_samples": 300_000, "duration_ms": 20_000}),
        Some(&e),
    )
    .unwrap();
    r.until(Duration::from_secs(10), "sample bound", |ev| {
        ev.iter()
            .any(|e| e.kind == KIND_CAPTURE_STATE && e.payload["state"] == "stopped")
    });
    let fin = final_state(&r.events);
    assert_eq!(fin["reason"], "samples", "{fin}");
    assert_eq!(fin["counters"]["samples"], 300_000);
    assert_eq!(fin["counters"]["decoder_input_bytes"], 300_000);
    assert_eq!(assert_edges_contiguous(&r.events), 300_000);
    assert!(
        !annotations(&r.events).is_empty(),
        "demo bytes decoded by the real spi decoder"
    );
}

#[test]
fn missing_engine_is_an_error_not_a_fallback() {
    let mut r = Run::new();
    let e = r.start(json!({"source": "sigrok-demo"}), None).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let e = r
        .start(
            json!({"source": "synthetic", "decoders": ["uart:rx=D0"]}),
            Some("/nonexistent/sigrok-cli"),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    r.pull();
    assert_eq!(final_state(&r.events)["state"], "failed");
}

fn fake_engine(name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("loom-logic-fake-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("sigrok-cli");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn acquisition_failure_is_reported_with_stderr() {
    let p = fake_engine("exit3", "echo boom-from-engine >&2; exit 3");
    let mut r = Run::new();
    r.start(json!({"source": "sigrok-demo"}), p.to_str()).unwrap();
    r.until(Duration::from_secs(5), "failed state", |ev| {
        ev.iter()
            .any(|e| e.kind == KIND_CAPTURE_STATE && e.payload["state"] == "failed")
    });
    let fin = final_state(&r.events);
    assert_eq!(fin["reason"], "error");
    assert!(
        fin["stderr_tail"].as_str().unwrap().contains("boom-from-engine"),
        "{fin}"
    );
    assert_eq!(fin["exit"]["acquisition"]["code"], 3);
}

#[test]
fn descendants_holding_pipes_are_group_killed() {
    // The fake decoder ignores stdin EOF and leaves a background child holding stdout/stderr.
    let marker = format!("{}.{}", 299, std::process::id() % 1000);
    let p = fake_engine("hang", &format!("sleep {marker} &\nexec sleep {marker}"));
    let mut r = Run::new();
    r.start(
        json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "duration_ms": 10_000}),
        p.to_str(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let t0 = Instant::now();
    let fin = r.stop();
    assert!(
        t0.elapsed() < Duration::from_secs(6),
        "stop took {:?}",
        t0.elapsed()
    );
    assert_eq!(fin["state"], "failed", "decoder had to be killed: {fin}");
    assert_eq!(fin["exit"]["decoder"]["stop_style"], "kill");
    std::thread::sleep(Duration::from_millis(200));
    let left = std::process::Command::new("pgrep")
        .args(["-f", &format!("sleep {marker}")])
        .output()
        .unwrap();
    assert!(
        left.stdout.is_empty(),
        "descendants left: {}",
        String::from_utf8_lossy(&left.stdout)
    );
}

#[test]
fn busy_latch_and_drop_cleanup() {
    let e = need_engine!();
    let mut r = Run::new();
    let cfg = json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "duration_ms": 20_000});
    r.start(cfg.clone(), Some(&e)).unwrap();
    assert_eq!(r.start(cfg.clone(), Some(&e)).unwrap_err().code, ErrorCode::Busy);
    r.fw.latch(true);
    r.until(Duration::from_secs(6), "stop_latch", |ev| {
        ev.iter().any(|e| {
            e.kind == KIND_CAPTURE_STATE
                && e.payload["reason"] == "stop_latch"
                && e.payload["state"] == "stopped"
        })
    });
    // Agent start refused while latched.
    assert_eq!(
        r.start(cfg.clone(), Some(&e)).unwrap_err().code,
        ErrorCode::Stopped
    );
    r.fw.latch(false);
    r.start(cfg, Some(&e)).unwrap();
    let t0 = Instant::now();
    drop(r); // worker drop stops and reaps the running capture
    assert!(t0.elapsed() < Duration::from_secs(6));
}

#[test]
fn decoder_closing_input_early_is_a_failure() {
    // Consumes 1000 bytes, then exits 0 — fewer samples decoded than captured.
    let p = fake_engine("early", "head -c 1000 >/dev/null; exit 0");
    let mut r = Run::new();
    r.start(
        json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "duration_ms": 10_000}),
        p.to_str(),
    )
    .unwrap();
    r.until(Duration::from_secs(6), "failed", |ev| {
        ev.iter()
            .any(|e| e.kind == KIND_CAPTURE_STATE && e.payload["state"] == "failed")
    });
    let fin = final_state(&r.events);
    assert_eq!(fin["reason"], "error", "{fin}");
    assert!(
        fin["counters"]["decoder_input_bytes"].as_u64().unwrap()
            < fin["counters"]["samples"].as_u64().unwrap()
    );
}
