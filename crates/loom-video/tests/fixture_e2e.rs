//! Worker + gateway-side media reader against the independent Python MJPEG fixture and the real
//! FFmpeg. Homes are short `/tmp/loom-video-*` dirs (media socket path bound). Skipped with a
//! message when ffmpeg, python3 or Pillow is unavailable. No camera/device is ever touched.
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::PublishedRecord;
use loom_testkit::FakeWorker;
use loom_video::worker::VideoWorker;
use loom_video::{KIND_CAPTURE_STATE, KIND_RECORD_STATE, KIND_SNAPSHOT, model};
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

/// Short private home under /tmp (removed on drop).
struct Home(PathBuf);
impl Home {
    fn new(tag: &str) -> Home {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = PathBuf::from(format!("/tmp/loom-video-{}-{tag}{n}", std::process::id()));
        std::fs::create_dir_all(p.join("records/2026-11-14/s1")).unwrap();
        Home(p)
    }
    fn session(&self) -> PathBuf {
        self.0.join("records/2026-11-14/s1")
    }
    fn storage(&self) -> Value {
        json!({"home": self.0, "session": self.session()})
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
    run_id: String,
    log: PathBuf,
}
impl Fixture {
    fn start(home: &Home, fps: u32) -> Fixture {
        let log = home.0.join("fixture-sent.ndjson");
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(fixture_script())
            .args(["--port", "0", "--fps", &fps.to_string(), "--size", "640x360", "--log"])
            .arg(&log)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        Fixture {
            child,
            port: u16::try_from(v["port"].as_u64().unwrap()).unwrap(),
            run_id: v["run_id"].as_str().unwrap().to_owned(),
            log,
        }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/stream", self.port)
    }
    fn control(&self, path: &str) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.0 200") || out.starts_with("HTTP/1.1 200"), "{out}");
    }
    fn sent(&self) -> usize {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"event\":\"sent\""))
            .count()
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
    events: Vec<PublishedRecord>,
    ffmpeg: String,
    storage: Value,
}
impl Run {
    fn new(ffmpeg: String, home: &Home) -> Run {
        Run {
            fw: FakeWorker::new(VideoWorker::new()),
            events: Vec::new(),
            ffmpeg,
            storage: home.storage(),
        }
    }
    fn call(&mut self, m: &str, mut args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        args["storage"] = self.storage.clone();
        args["ffmpeg"] = json!(self.ffmpeg);
        self.fw.call(m, args)
    }
    fn open(&mut self, config: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        self.call("video.open", json!({"config": config}))
    }
    fn pull(&mut self) {
        self.events.extend(self.fw.drain_published());
    }
    fn states(&self) -> Vec<(String, Option<String>)> {
        self.events
            .iter()
            .filter(|e| e.kind == KIND_CAPTURE_STATE)
            .map(|e| {
                (
                    e.payload["state"].as_str().unwrap_or("").to_owned(),
                    e.payload["reason"].as_str().map(str::to_owned),
                )
            })
            .collect()
    }
    fn status(&mut self) -> Value {
        self.fw.call("video.status", json!({})).unwrap()
    }
    fn until(&mut self, limit: Duration, what: &str, pred: impl Fn(&Value, &[PublishedRecord]) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            self.pull();
            let st = self.status();
            if pred(&st, &self.events) {
                return st;
            }
            assert!(t0.elapsed() < limit, "timed out waiting for {what}; states {:?}; status {st}", self.states());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn last_frame(st: &Value) -> u64 {
    st["health"]["last_frame_id"].as_u64().unwrap_or(0)
}

/// Raw media-socket request (the same protocol loomd's HTTP routes use).
fn socket_frame(home: &Home, capture: &str, id: Option<u64>) -> (Value, Vec<u8>) {
    let p = model::media_socket_path(&home.0, capture).unwrap();
    let mut s = UnixStream::connect(p).unwrap();
    match id {
        Some(n) => writeln!(s, "FRAME {n}").unwrap(),
        None => writeln!(s, "FRAME").unwrap(),
    }
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let head: Value = serde_json::from_str(&line).unwrap();
    let mut body = vec![0u8; usize::try_from(head["len"].as_u64().unwrap()).unwrap()];
    r.read_exact(&mut body).unwrap();
    (head, body)
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

#[test]
fn live_snapshot_record_media_and_stop_cleanup() {
    let ff = need!();
    let home = Home::new("a");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    let open = run
        .open(json!({"source": "http-mjpeg", "url": fx.url(), "width": 640, "height": 360, "fps": 30}))
        .unwrap();
    assert_eq!(open["state"], "live", "{open}");
    assert!(open["capture_epoch"].as_str().is_some_and(|e| e.parse::<u64>().unwrap() > (1 << 53)), "{open}");
    let id = open["capture_id"].as_str().unwrap().to_owned();
    assert!(model::valid_capture_id(&id));
    // No URL query / home path in events.
    let st = run.until(Duration::from_secs(10), "60 frames", |s, _| last_frame(s) >= 60);
    let fps = st["health"]["measured_fps"].as_u64().unwrap();
    assert!((24..=36).contains(&fps), "measured fps {fps} at 30 requested: {st}");

    // Frame identity: worker meta == media socket bytes hash == snapshot file hash.
    let n = last_frame(&st) - 5;
    let meta = run.call("video.frame", json!({"frame_id": n})).unwrap();
    let (head, body) = socket_frame(&home, &id, Some(n));
    assert_eq!(head["frame_id"], n);
    assert_eq!(sha(&body), meta["sha256"].as_str().unwrap());
    assert!(model::looks_like_jpeg(&body));
    assert!(meta["source_pts"].is_null() && meta["timestamp_kind"] == "receive");
    let snap = run.call("video.snapshot", json!({"frame_id": n})).unwrap();
    let (_, path) = model::artifact_path(&home.session(), snap["artifact_id"].as_str().unwrap()).unwrap();
    assert_eq!(sha(&std::fs::read(&path).unwrap()), meta["sha256"].as_str().unwrap());
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    // Gateway-side media reader (HTTP route body) over the same socket.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let r = rt.block_on(loom_video::media::frame(&home.0, &id, Some(format!("id={n}").as_str())));
    assert_eq!(r.status, 200);
    let hdr = |k: &str| r.headers.iter().find(|(h, _)| *h == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(hdr("x-loom-frame-id"), n.to_string());
    assert_eq!(hdr("x-loom-frame-sha256"), meta["sha256"].as_str().unwrap());
    // wide values are decimal strings end to end (meta == header), never JSON numbers
    for k in ["capture_epoch", "recv_utc_ns", "recv_mono_ns"] {
        assert!(meta[k].is_string(), "{k}: {meta}");
    }
    assert_eq!(hdr("x-loom-capture-epoch"), meta["capture_epoch"].as_str().unwrap());
    assert_eq!(hdr("x-loom-recv-utc-ns"), meta["recv_utc_ns"].as_str().unwrap());
    assert_eq!(open["capture_epoch"], meta["capture_epoch"]);
    let loom_video::media::Body::Bytes(b) = &r.body else { panic!("bytes") };
    assert_eq!(sha(b), meta["sha256"].as_str().unwrap());
    assert_eq!(rt.block_on(loom_video::media::frame(&home.0, &id, Some("token=x"))).status, 400);
    assert_eq!(rt.block_on(loom_video::media::frame(&home.0, "../x", None)).status, 400);
    assert_eq!(rt.block_on(loom_video::media::frame(&home.0, &id, Some("id=999999999"))).status, 404);
    let a = rt.block_on(loom_video::media::artifact(&home.session(), snap["artifact_id"].as_str().unwrap()));
    assert_eq!((a.status, a.content_type), (200, "image/jpeg"));
    assert_eq!(rt.block_on(loom_video::media::artifact(&home.session(), "snap-../../x-1")).status, 400);
    let m = rt.block_on(loom_video::media::meta(&home.0, &id));
    assert_eq!(m.status, 200);

    // Recording (MJPEG copy, bounded), then ffprobe-able file.
    let rec = run.call("video.record.start", json!({})).unwrap();
    assert_eq!(rec["state"], "recording");
    let rid = rec["recording_id"].as_str().unwrap().to_owned();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        rt.block_on(loom_video::media::artifact(&home.session(), &rid)).status,
        409,
        "in-progress recording is not served"
    );
    std::thread::sleep(Duration::from_secs(2));
    let fin = run.call("video.record.stop", json!({})).unwrap();
    assert_eq!(fin["state"], "finalized", "{fin}");
    assert!(fin["frames_written"].as_u64().unwrap() >= 60, "{fin}");
    assert!(fin["cleanup_ms"].as_u64().unwrap() <= 2100, "{fin}");
    assert_eq!(fin["lossless_claim"], false);
    let (_, rpath) = model::artifact_path(&home.session(), &rid).unwrap();
    assert_eq!(std::fs::metadata(&rpath).unwrap().len(), fin["size"].as_u64().unwrap());
    let probe = Command::new(Path::new(&run.ffmpeg).with_file_name("ffprobe"))
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries",
               "stream=codec_name,nb_read_frames", "-of", "json"])
        .arg(&rpath)
        .output();
    if let Ok(o) = probe {
        let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
        assert_eq!(v["streams"][0]["codec_name"], "mjpeg", "{v}");
        let frames: u64 = v["streams"][0]["nb_read_frames"].as_str().unwrap_or("0").parse().unwrap();
        assert_eq!(frames, fin["frames_written"].as_u64().unwrap(), "{v} {fin}");
    }

    // STOP: freeze + cleanup bounded; no new frames after; history readable.
    let before = last_frame(&run.status());
    run.fw.latch(true);
    let st = run.until(Duration::from_secs(4), "closed after STOP", |s, _| s["capture"]["state"] == "closed");
    assert_eq!(st["capture"]["reason"], "stop_latch");
    assert!(st["capture"]["cleanup_ms"].as_u64().unwrap() <= 2000, "{st}");
    let frozen_at = st["capture"]["stopped_at_frame_id"].as_u64().unwrap();
    assert!(frozen_at >= before && frozen_at <= before + 3, "{frozen_at} vs {before}");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(last_frame(&run.status()), frozen_at, "no publication after STOP");
    let (h, _) = socket_frame(&home, &id, None);
    assert_eq!(h["frame_id"], frozen_at);
    assert_eq!(h["frozen"], true);
    // Latched: open / snapshot / record refused.
    assert_eq!(run.open(json!({"source": "http-mjpeg", "url": fx.url()})).unwrap_err().code, ErrorCode::Stopped);
    assert_eq!(run.call("video.snapshot", json!({})).unwrap_err().code, ErrorCode::Stopped);
    run.fw.latch(false);
    // Releasing the latch reopens nothing.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run.status()["running"], false);

    run.pull();
    assert!(run.events.iter().any(|e| e.kind == KIND_SNAPSHOT));
    assert!(run.events.iter().any(|e| e.kind == KIND_RECORD_STATE && e.payload["state"] == "finalized"));
    let all = serde_json::to_string(&run.events.iter().map(|e| &e.payload).collect::<Vec<_>>()).unwrap();
    assert!(!all.contains(home.0.to_str().unwrap()), "home path leaked into events");
    assert!(fx.sent() > 0 && !fx.run_id.is_empty());
}

#[test]
fn refused_source_fails_explicitly() {
    let ff = need!();
    let home = Home::new("b");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut run = Run::new(ff, &home);
    let e = run
        .open(json!({"source": "http-mjpeg", "url": format!("http://127.0.0.1:{port}/stream")}))
        .unwrap_err();
    assert!(matches!(e.code, ErrorCode::NotFound | ErrorCode::Disconnected), "{e:?}");
    assert_eq!(e.detail["state"], "failed", "{e:?}");
    assert_eq!(run.status()["running"], false);
}

#[test]
fn stall_disconnect_no_auto_reconnect_and_manual_reopen() {
    let ff = need!();
    let home = Home::new("c");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    run.open(json!({"source": "http-mjpeg", "url": fx.url(), "width": 320, "height": 180})).unwrap();
    run.until(Duration::from_secs(10), "frames", |s, _| last_frame(s) >= 20);
    fx.control("/control/stall?ms=3500");
    run.until(Duration::from_secs(6), "stalled", |s, _| s["capture"]["state"] == "stalled");
    run.until(Duration::from_secs(6), "live again", |s, _| s["capture"]["state"] == "live");
    // corrupt + large parts are bounded: capture keeps going
    fx.control("/control/corrupt?n=5");
    fx.control("/control/large?n=2");
    let n0 = last_frame(&run.status());
    run.until(Duration::from_secs(10), "frames after corrupt/large", |s, _| last_frame(s) >= n0 + 30);
    fx.control("/control/disconnect");
    let st = run.until(Duration::from_secs(10), "terminal after disconnect", |s, _| {
        matches!(s["capture"]["state"].as_str(), Some("closed" | "failed"))
    });
    let reason = st["capture"]["reason"].as_str().unwrap().to_owned();
    assert!(["eof", "disconnect", "stall_timeout"].contains(&reason.as_str()), "{st}");
    let frozen = last_frame(&run.status());
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(last_frame(&run.status()), frozen, "no automatic reconnect");
    // Manual reopen = new capture id / epoch.
    let again = run.open(json!({"source": "http-mjpeg", "url": fx.url(), "width": 320, "height": 180})).unwrap();
    assert_ne!(again["capture_id"], st["capture_id"]);
    let closed = run.call("video.close", json!({})).unwrap();
    assert_eq!((closed["state"].as_str(), closed["closed"].as_bool()), (Some("closed"), Some(true)));
}

#[test]
fn storage_roots_socket_path_and_quota_are_explicit() {
    let ff = need!();
    // No storage roots → refused (never ~/.loom).
    let mut fw = FakeWorker::new(VideoWorker::new());
    let e = fw
        .call("video.open", json!({"config": {"source": "http-mjpeg", "url": "http://127.0.0.1:9/"}, "ffmpeg": ff}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Config);
    // Socket path too long → explicit failure.
    let long = PathBuf::from(format!("/tmp/loom-video-{}-{}", std::process::id(), "x".repeat(80)));
    std::fs::create_dir_all(&long).unwrap();
    let e = fw
        .call(
            "video.open",
            json!({"config": {"source": "http-mjpeg", "url": "http://127.0.0.1:9/"}, "ffmpeg": ff,
                   "storage": {"home": long, "session": long.join("s")}}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Config, "{e:?}");
    let _ = std::fs::remove_dir_all(&long);
    // Quota: a (sparse) full session refuses snapshot/record with E_LIMIT.
    let home = Home::new("d");
    let fx = Fixture::start(&home, 30);
    let mut run = Run::new(ff, &home);
    run.open(json!({"source": "http-mjpeg", "url": fx.url(), "width": 320, "height": 180})).unwrap();
    run.until(Duration::from_secs(10), "frames", |s, _| last_frame(s) >= 5);
    let filler = home.session().join("video/vfill00000000");
    std::fs::create_dir_all(&filler).unwrap();
    std::fs::File::create(filler.join("filler"))
        .unwrap()
        .set_len(model::MAX_SESSION_BYTES)
        .unwrap();
    let e = run.call("video.snapshot", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Limit, "{e:?}");
    assert_eq!(e.detail["reason"], "quota_session_bytes");
    assert_eq!(run.call("video.record.start", json!({})).unwrap_err().code, ErrorCode::Limit);
    run.call("video.close", json!({})).unwrap();
}
