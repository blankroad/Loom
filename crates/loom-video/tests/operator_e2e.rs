//! Operator upgrade B2 end-to-end (FUNCTIONS.md §Batch 2), Opus-authored: real FFmpeg + the independent
//! Python MJPEG fixture, private short `/tmp/loom-video-op-*` homes, no device, no daemon. Skipped with a
//! message when ffmpeg, python3+Pillow or the fixture script is unavailable. Sonnet must not edit this file.
//! V-03: a pinned frame survives ring eviction, close and STOP, and stays snapshot-able as exact bytes;
//! V-05: health carries `last_recv_utc_ns`, and terminal health is `stale:true`.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_testkit::FakeWorker;
use loom_video::model;
use loom_video::worker::VideoWorker;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

fn ffmpeg() -> Option<String> {
    let p = model::resolve_ffmpeg(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}
fn fixture_script() -> PathBuf {
    if let Ok(p) = std::env::var("LOOM_VIDEO_FIXTURE") {
        return PathBuf::from(p);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/reports/video-capture/fixture/mjpeg_server.py")
}
fn python_ok() -> bool {
    Command::new("python3")
        .args(["-I", "-c", "import PIL"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
        && fixture_script().is_file()
}
macro_rules! need {
    () => {
        match (ffmpeg(), python_ok()) {
            (Some(f), true) => f,
            _ => {
                eprintln!("SKIP: ffmpeg, python3+Pillow or the fixture script is unavailable");
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
        let p = PathBuf::from(format!("/tmp/loom-video-op-{}-{tag}{n}", std::process::id()));
        std::fs::create_dir_all(p.join("records/2026-11-15/s1")).unwrap();
        Home(p)
    }
    fn session(&self) -> PathBuf {
        self.0.join("records/2026-11-15/s1")
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    child: Child,
    port: u16,
}
impl Fixture {
    fn start(home: &Home, fps: u32) -> Fixture {
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(fixture_script())
            .args([
                "--port",
                "0",
                "--fps",
                &fps.to_string(),
                "--size",
                "640x360",
                "--log",
            ])
            .arg(home.0.join("fixture-sent.ndjson"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        Fixture {
            child,
            port: u16::try_from(v["port"].as_u64().unwrap()).unwrap(),
        }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/stream", self.port)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Run {
    fw: FakeWorker<VideoWorker>,
    ffmpeg: String,
    storage: Value,
}
impl Run {
    fn new(ffmpeg: String, home: &Home) -> Run {
        Run {
            fw: FakeWorker::new(VideoWorker::new()),
            ffmpeg,
            storage: json!({"home": home.0, "session": home.session()}),
        }
    }
    fn call(&mut self, m: &str, mut args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        args["storage"] = self.storage.clone();
        args["ffmpeg"] = json!(self.ffmpeg);
        self.fw.call(m, args)
    }
    fn open(&mut self, url: &str) -> Value {
        let v = self
            .call("video.open", json!({"config": {"source": "http-mjpeg", "url": url, "width": 640, "height": 360, "fps": 30}}))
            .unwrap();
        assert_eq!(v["state"], "live", "{v}");
        v
    }
    fn status(&mut self) -> Value {
        self.fw.call("video.status", json!({})).unwrap()
    }
    fn until(&mut self, limit: Duration, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            let _ = self.fw.drain_published();
            let st = self.status();
            if pred(&st) {
                return st;
            }
            assert!(t0.elapsed() < limit, "timed out waiting for {what}; status {st}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn last(st: &Value) -> u64 {
    st["health"]["last_frame_id"].as_u64().unwrap_or(0)
}
fn first_in_ring(st: &Value) -> u64 {
    st["health"]["ring"]["first_frame_id"].as_u64().unwrap_or(0)
}
fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}
fn socket_frame(home: &Home, capture: &str, id: u64) -> (Value, Vec<u8>) {
    let mut s = UnixStream::connect(model::media_socket_path(&home.0, capture).unwrap()).unwrap();
    writeln!(s, "FRAME {id}").unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let head: Value = serde_json::from_str(&line).unwrap();
    let mut body = vec![0u8; usize::try_from(head["len"].as_u64().unwrap()).unwrap()];
    r.read_exact(&mut body).unwrap();
    (head, body)
}

/// V-03 regression: freeze → inspect for longer than the ring holds → snapshot still saves the exact frame.
#[test]
fn pinned_frame_survives_eviction_and_snapshots_exact_bytes() {
    let ff = need!();
    let home = Home::new("pin");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    let cap = run.open(&fx.url())["capture_id"].as_str().unwrap().to_owned();
    let st = run.until(Duration::from_secs(10), "10 frames", |s| last(s) >= 10);
    let n = last(&st);
    let p = run
        .call("video.pin", json!({"frame_id": n, "ttl_ms": 60_000}))
        .unwrap();
    assert_eq!(
        (
            p["frame_id"].as_u64(),
            p["owner"].as_str(),
            p["retained_by_before"].as_str()
        ),
        (Some(n), Some("user"), Some("ring")),
        "{p}"
    );
    assert_eq!(p["ttl_ms"], 60_000);
    assert!(
        p["expires_in_ms"].as_u64().unwrap() <= 60_000 && p["expires_in_ms"].as_u64().unwrap() > 55_000,
        "{p}"
    );
    assert_eq!(p["pins"]["user_count"], 1);
    let pinned_sha = p["sha256"].as_str().unwrap().to_owned();
    // an unpinned neighbour for contrast
    let other = n - 1;
    let st = run.until(Duration::from_secs(15), "ring evicts the pinned frame", |s| {
        first_in_ring(s) > n
    });
    assert_eq!(st["pins"]["count"], 1, "status carries the pin store: {st}");
    let m = run.call("video.frame", json!({"frame_id": n})).unwrap();
    assert_eq!(
        (m["retained_by"].as_str(), m["sha256"].as_str()),
        (Some("pin"), Some(pinned_sha.as_str())),
        "{m}"
    );
    assert_eq!(m["pin"]["owners"][0]["owner"], "user");
    let e = run.call("video.frame", json!({"frame_id": other})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert_eq!(e.detail["expired"], true, "{e:?}");
    let (head, body) = socket_frame(&home, &cap, n);
    assert_eq!(
        (head["ok"].as_bool(), sha(&body)),
        (Some(true), pinned_sha.clone())
    );
    let snap = run.call("video.snapshot", json!({"frame_id": n})).unwrap();
    assert_eq!(
        (snap["sha256"].as_str(), snap["retained_by"].as_str()),
        (Some(pinned_sha.as_str()), Some("pin"))
    );
    let (_, path) = model::artifact_path(&home.session(), snap["artifact_id"].as_str().unwrap()).unwrap();
    assert_eq!(sha(&std::fs::read(path).unwrap()), pinned_sha);
    // release → gone, idempotent
    let u = run.call("video.unpin", json!({"frame_id": n})).unwrap();
    assert_eq!(
        (u["released"].as_u64(), u["pins"]["count"].as_u64()),
        (Some(1), Some(0)),
        "{u}"
    );
    assert_eq!(
        run.call("video.unpin", json!({"frame_id": n})).unwrap()["released"],
        0
    );
    let e = run.call("video.frame", json!({"frame_id": n})).unwrap_err();
    assert_eq!(
        (e.code, e.detail["expired"].as_bool()),
        (ErrorCode::NotFound, Some(true))
    );
    let (head, _) = socket_frame(&home, &cap, n);
    assert_eq!(head["error"], "expired");
    run.call("video.close", json!({})).unwrap();
}

#[test]
fn ttl_limit_and_errors_are_explicit() {
    let ff = need!();
    let home = Home::new("ttl");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    run.open(&fx.url());
    let st = run.until(Duration::from_secs(10), "12 frames", |s| last(s) >= 12);
    let n = last(&st);
    // short TTL: gone shortly after eviction even though it was pinned
    run.call("video.pin", json!({"frame_id": n, "ttl_ms": 300}))
        .unwrap();
    run.until(Duration::from_secs(15), "eviction", |s| first_in_ring(s) > n);
    // FakeWorker runs on a SimClock (mono stays put unless advanced); pins share the frames' clock (W1).
    run.fw.clock().advance_ns(350_000_000);
    let e = run.call("video.frame", json!({"frame_id": n})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "TTL expired pin must not be served");
    assert!(run.status()["pins"]["expired_total"].as_u64().unwrap() >= 1);
    // user limit (latest frames are always in the ring)
    let base = last(&run.status());
    for k in 0..model::MAX_USER_PINS as u64 {
        run.call("video.pin", json!({"frame_id": base - k})).unwrap();
    }
    let e = run
        .call(
            "video.pin",
            json!({"frame_id": base - model::MAX_USER_PINS as u64}),
        )
        .unwrap_err();
    assert_eq!(
        (e.code, e.detail["reason"].as_str()),
        (ErrorCode::Limit, Some("pin_user_limit")),
        "{e:?}"
    );
    assert_eq!(e.detail["pins"]["user_count"], model::MAX_USER_PINS);
    // default target = latest; default TTL
    let all = run.call("video.unpin", json!({})).unwrap();
    assert_eq!(all["released"], model::MAX_USER_PINS);
    let d = run.call("video.pin", json!({})).unwrap();
    assert_eq!(d["ttl_ms"], model::PIN_TTL_DEFAULT_MS);
    // an id that never arrived / bad ttl
    let e = run
        .call("video.pin", json!({"frame_id": 999_999_999}))
        .unwrap_err();
    assert_eq!(
        (e.code, e.detail["expired"].as_bool()),
        (ErrorCode::NotFound, Some(false))
    );
    assert_eq!(
        run.call("video.pin", json!({"ttl_ms": 0})).unwrap_err().code,
        ErrorCode::Usage
    );
    assert_eq!(
        run.call("video.pin", json!({"ttl_ms": model::PIN_TTL_MAX_MS + 1}))
            .unwrap_err()
            .code,
        ErrorCode::Usage
    );
    run.call("video.close", json!({})).unwrap();
}

/// V-05 engine half + pin release rules across STOP, close and the next open.
#[test]
fn health_truth_and_pin_lifetime_across_stop_close_reopen() {
    let ff = need!();
    let home = Home::new("life");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    let cap = run.open(&fx.url())["capture_id"].as_str().unwrap().to_owned();
    let st = run.until(Duration::from_secs(10), "10 frames", |s| last(s) >= 10);
    let utc = st["health"]["last_recv_utc_ns"]
        .as_str()
        .unwrap_or_else(|| panic!("decimal string: {st}"));
    assert!(utc.parse::<u64>().unwrap() > 1_700_000_000_000_000_000, "{utc}");
    assert_eq!(st["health"]["stale"], false);
    assert_eq!(st["health"]["pins"]["count"], 0);
    let n = last(&st);
    run.call("video.pin", json!({"frame_id": n})).unwrap();
    // STOP: capture freezes and closes; pinning is still allowed (memory, safe direction), snapshot is not
    run.fw.latch(true);
    let st = run.until(Duration::from_secs(4), "closed after STOP", |s| {
        s["capture"]["state"] == "closed"
    });
    assert_eq!(
        st["health"]["stale"], true,
        "terminal health is never fresh: {}",
        st["health"]
    );
    assert_eq!(st["health"]["state"], "closed");
    assert!(st["health"]["last_recv_utc_ns"].is_string());
    let stop = st["capture"]["stopped_at_frame_id"].as_u64().unwrap();
    let p = run.call("video.pin", json!({"frame_id": stop})).unwrap();
    assert_eq!(p["capture_id"], cap.as_str());
    assert_eq!(
        run.call("video.snapshot", json!({"frame_id": n}))
            .unwrap_err()
            .code,
        ErrorCode::Stopped
    );
    run.fw.latch(false);
    // after close the pinned frames stay readable until TTL / next open
    assert_eq!(
        run.call("video.frame", json!({"frame_id": n})).unwrap()["frame_id"],
        n
    );
    assert_eq!(
        run.status()["pins"]["user_count"],
        if stop == n { 1 } else { 2 },
        "re-pinning a held frame is a refresh"
    );
    let again = run.open(&fx.url());
    assert_ne!(again["capture_id"], cap.as_str());
    assert_eq!(
        run.status()["pins"]["count"],
        0,
        "next open releases the previous capture's pins"
    );
    assert_eq!(
        run.call("video.frame", json!({"capture_id": cap, "frame_id": n}))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    run.call("video.close", json!({})).unwrap();
}
