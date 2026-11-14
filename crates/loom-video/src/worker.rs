//! Worker piece: one acquisition FFmpeg per capture (own process group), a stdout reader that
//! splits normalized JPEGs into the bounded ring, a read-only Unix-socket media server, the
//! optional recorder FFmpeg fed from the same JPEGs through a bounded queue, snapshots, health
//! and the STOP/close cleanup sequence. Events carry metadata only — never frame bytes, file
//! paths or source query strings.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::num::U64;
use loom_proto::event::{Env, Origin, TsSource, WriteState};
use loom_proto::worker::{Call, Input, WorkerError, Write as WorkerWrite};
use loom_worker_sdk::{BoxFuture, Emitter, EventDraft, JobCmd, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::model::{
    self, ArtifactKind, Frame, JpegSplitter, OpenConfig, Progress, Ring, Source, FIRST_FRAME_MS, STALL_MS,
};
use crate::{KIND_CAPTURE_STATE, KIND_HEALTH, KIND_RECORD_STATE, KIND_SNAPSHOT, WORKER_KIND};

/// stdout read size.
pub const READ_CHUNK: usize = 64 * 1024;
/// stderr kept (bytes) / tail reported.
pub const STDERR_RING: usize = 64 * 1024;
/// stderr tail in events.
pub const STDERR_TAIL: usize = 4096;
/// Longest stderr line kept.
pub const MAX_LINE: usize = 4096;
/// Health period.
pub const HEALTH_PERIOD: Duration = Duration::from_secs(1);
/// Supervisor poll.
pub const POLL: Duration = Duration::from_millis(20);
/// Acquisition: SIGINT → this → SIGKILL.
pub const ACQ_INT_GRACE: Duration = Duration::from_millis(500);
/// Recorder: stdin EOF → this → SIGINT.
pub const REC_EOF_GRACE: Duration = Duration::from_millis(1200);
/// Recorder: SIGINT → this → SIGKILL (total ≤ 2 s with the EOF grace, then reap).
pub const REC_INT_GRACE: Duration = Duration::from_millis(500);
/// Recorder queue (frames).
pub const REC_QUEUE_FRAMES: usize = 64;
/// Recorder queue (bytes).
pub const REC_QUEUE_BYTES: usize = 16 << 20;
/// Concurrent media connections per capture.
pub const MEDIA_MAX_CLIENTS: usize = 8;
/// Media request read timeout.
pub const MEDIA_READ_TIMEOUT: Duration = Duration::from_secs(1);
/// Media response write timeout (slow reader → skipped).
pub const MEDIA_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Bounded helper process run (probe/list devices).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Replay names listed.
pub const MAX_REPLAY_LIST: usize = 100;

fn err(code: ErrorCode, message: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

fn err_d(code: ErrorCode, message: impl Into<String>, detail: Value) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn ready<T: Send + 'static>(v: T) -> BoxFuture<'static, T> {
    Box::pin(async move { v })
}

/// Run `f` on its own thread; the future resolves with its result.
fn offload<T: Send + 'static>(
    name: &str,
    f: impl FnOnce() -> Result<T, WorkerError> + Send + 'static,
) -> BoxFuture<'static, Result<T, WorkerError>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _ = tx.send(f());
    });
    Box::pin(async move {
        spawned.map_err(|e| err(ErrorCode::Internal, format!("thread: {e}")))?;
        rx.await
            .unwrap_or_else(|_| Err(err(ErrorCode::Internal, "worker thread ended without a result")))
    })
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------------------------
// Process groups (same discipline as loom-logic): every child leads its own group; signals go
// to the whole group via kill(1) with a fixed argv — no shell, no unsafe.

fn signal_group(pgid: u32, sig: &str) {
    let _ = Command::new("/bin/kill")
        .args([sig, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn wait_until(child: &mut Child, until: Instant) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

fn kill_reap(child: &mut Child) -> Option<ExitStatus> {
    signal_group(child.id(), "-KILL");
    let _ = child.kill();
    child.wait().ok()
}

fn spawn_group(program: &Path, args: &[String], stdin: Stdio) -> std::io::Result<Child> {
    let mut c = Command::new(program);
    c.args(args)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C");
    {
        use std::os::unix::process::CommandExt as _;
        c.process_group(0);
    }
    c.spawn()
}

fn thread<F: FnOnce() + Send + 'static>(name: &str, f: F) -> Result<JoinHandle<()>, String> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map_err(|e| format!("spawn thread {name}: {e}"))
}

fn exit_json(s: Option<ExitStatus>, how: &str) -> Value {
    json!({"code": s.and_then(|s| s.code()), "success": s.is_some_and(|s| s.success()), "how": how})
}

/// Run a helper invocation to completion within [`PROBE_TIMEOUT`]; returns (stdout, stderr).
fn run_bounded(program: &Path, args: &[String], max: usize) -> Result<(String, String, bool), String> {
    let mut child =
        spawn_group(program, args, Stdio::null()).map_err(|e| format!("spawn ffmpeg: {e}"))?;
    let out = child.stdout.take();
    let errp = child.stderr.take();
    let read_all = move |r: Option<Box<dyn Read + Send>>| -> String {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.by_ref().take(max as u64).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    };
    let o = std::thread::spawn(move || read_all(out.map(|x| Box::new(x) as Box<dyn Read + Send>)));
    let e = std::thread::spawn(move || read_all(errp.map(|x| Box::new(x) as Box<dyn Read + Send>)));
    let st = match wait_until(&mut child, Instant::now() + PROBE_TIMEOUT) {
        Some(s) => {
            signal_group(child.id(), "-KILL");
            Some(s)
        }
        None => {
            kill_reap(&mut child);
            None
        }
    };
    let o = o.join().unwrap_or_default();
    let e = e.join().unwrap_or_default();
    match st {
        Some(s) => Ok((o, e, s.success())),
        None => Err("ffmpeg helper timed out".into()),
    }
}

// ---------------------------------------------------------------------------------------------
// Shared capture state.

#[derive(Debug, Default)]
struct Counters {
    frames: AtomicU64,
    oversize: AtomicU64,
    truncated: AtomicU64,
    junk_bytes: AtomicU64,
    frozen_dropped: AtomicU64,
    emit_errors: AtomicU64,
    media_served: AtomicU64,
    media_skipped: AtomicU64,
    media_clients: AtomicUsize,
    stdout_bytes: AtomicU64,
}

#[derive(Debug, Default)]
struct StderrRing {
    text: Mutex<VecDeque<String>>,
    bytes: AtomicUsize,
    lines: AtomicU64,
}

impl StderrRing {
    fn push(&self, line: String) {
        self.lines.fetch_add(1, Ordering::Relaxed);
        let mut t = lock(&self.text);
        let mut n = self.bytes.load(Ordering::Relaxed) + line.len() + 1;
        t.push_back(line);
        while n > STDERR_RING {
            match t.pop_front() {
                Some(l) => n -= l.len() + 1,
                None => break,
            }
        }
        self.bytes.store(n, Ordering::Relaxed);
    }
    fn tail(&self) -> String {
        let t = lock(&self.text);
        let mut out: Vec<&str> = Vec::new();
        let mut n = 0;
        for l in t.iter().rev() {
            if n + l.len() + 1 > STDERR_TAIL {
                break;
            }
            n += l.len() + 1;
            out.push(l);
        }
        out.reverse();
        out.join("\n")
    }
}

/// Text scrubber: source query string, `$LOOM_HOME` and session paths never leave the worker.
#[derive(Debug, Clone)]
struct Scrub {
    source: Source,
    home: String,
    session: String,
}

impl Scrub {
    fn apply(&self, s: &str) -> String {
        let mut t = model::redact(s, &self.source);
        if !self.session.is_empty() {
            t = t.replace(&self.session, "$SESSION");
        }
        if !self.home.is_empty() {
            t = t.replace(&self.home, "$LOOM_HOME");
        }
        t
    }
}

fn drain_lines(
    r: impl Read + Send + 'static,
    ring: Arc<StderrRing>,
    progress: Option<Arc<Mutex<Progress>>>,
    scrub: Scrub,
) -> Result<JoinHandle<()>, String> {
    thread("video-stderr", move || {
        let mut r = BufReader::new(r);
        let mut line = Vec::new();
        loop {
            line.clear();
            // Bounded line read: stop at newline or MAX_LINE bytes.
            let mut lim = (&mut r).take(MAX_LINE as u64);
            match lim.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let s = String::from_utf8_lossy(&line).trim_end().to_owned();
            if s.is_empty() {
                continue;
            }
            if let Some(p) = &progress
                && lock(p).line(&s)
            {
                continue;
            }
            ring.push(scrub.apply(&s));
        }
    })
}

struct Recorder {
    artifact_id: String,
    final_path: PathBuf,
    part_path: PathBuf,
    tx: Option<SyncSender<Arc<Vec<u8>>>>,
    queued_bytes: Arc<AtomicUsize>,
    gap: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    writer_failed: Arc<AtomicBool>,
    first_frame_id: Option<u64>,
    last_frame_id: u64,
    child: Child,
    writer: Option<JoinHandle<()>>,
    stderr: Arc<StderrRing>,
    stderr_join: Option<JoinHandle<()>>,
    started: Instant,
    declared_fps: u32,
    byte_cap: u64,
    bytes_accepted: u64,
    limit_reason: Option<&'static str>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("artifact_id", &self.artifact_id)
            .finish_non_exhaustive()
    }
}

impl Recorder {
    fn feed(&mut self, frame: &Frame) {
        let len = frame.data.len();
        if self.limit_reason.is_some() {
            return;
        }
        if self.started.elapsed() >= Duration::from_secs(model::MAX_RECORD_SECS) {
            self.limit_reason = Some("max_record_secs");
            return;
        }
        if self.bytes_accepted + len as u64 > self.byte_cap {
            self.limit_reason = Some(if self.byte_cap < model::MAX_RECORD_BYTES {
                "quota_bytes"
            } else {
                "max_record_bytes"
            });
            return;
        }
        if self.queued_bytes.load(Ordering::Acquire) + len > REC_QUEUE_BYTES {
            self.gap.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let Some(tx) = &self.tx else { return };
        self.queued_bytes.fetch_add(len, Ordering::AcqRel);
        match tx.try_send(frame.data.clone()) {
            Ok(()) => {
                self.bytes_accepted += len as u64;
                self.first_frame_id.get_or_insert(frame.id);
                self.last_frame_id = frame.id;
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.queued_bytes.fetch_sub(len, Ordering::AcqRel);
                self.gap.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn live(&self) -> Value {
        json!({"recording_id": self.artifact_id, "artifact_id": self.artifact_id,
               "frames_written": self.written.load(Ordering::Relaxed),
               "gap_frames": self.gap.load(Ordering::Relaxed),
               "declared_fps": self.declared_fps, "bytes_accepted": self.bytes_accepted,
               "byte_cap": self.byte_cap, "max_record_secs": model::MAX_RECORD_SECS,
               "limit_reason": self.limit_reason,
               "first_frame_id": self.first_frame_id, "last_frame_id": self.last_frame_id,
               "elapsed_ms": ms(self.started.elapsed())})
    }

    /// EOF → (until t0+1.2 s) → SIGINT → (until +0.5 s) → SIGKILL → reap. Returns the final
    /// record payload (without state).
    fn finalize(mut self, t0: Instant, reason: &str) -> Value {
        drop(self.tx.take());
        let mut how = "eof";
        let mut st = wait_until(&mut self.child, t0 + REC_EOF_GRACE);
        if st.is_none() {
            how = "interrupted";
            signal_group(self.child.id(), "-INT");
            st = wait_until(&mut self.child, t0 + REC_EOF_GRACE + REC_INT_GRACE);
        }
        if st.is_none() {
            how = "killed";
            st = kill_reap(&mut self.child);
        } else {
            signal_group(self.child.id(), "-KILL");
        }
        let cleanup_ms = ms(t0.elapsed());
        if let Some(j) = self.writer.take() {
            let _ = j.join();
        }
        if let Some(j) = self.stderr_join.take() {
            let _ = j.join();
        }
        let gap = self.gap.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Relaxed);
        let clean = how == "eof" && st.is_some_and(|s| s.success()) && !self.writer_failed.load(Ordering::Relaxed);
        let elapsed = self.started.elapsed().as_secs_f64();
        let (size, sha) = match std::fs::rename(&self.part_path, &self.final_path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = std::fs::set_permissions(&self.final_path, std::fs::Permissions::from_mode(0o600));
                }
                file_hash(&self.final_path).map_or((None, None), |(n, h)| (Some(n), Some(h)))
            }
            Err(_) => (None, None),
        };
        let _ = std::fs::remove_file(model::recording_marker(&self.final_path));
        let ok = size.is_some_and(|n| n > 0) && written > 0;
        let mut partial_reasons = Vec::new();
        if gap > 0 {
            partial_reasons.push("queue_gap");
        }
        if !clean {
            partial_reasons.push("unclean_finalize");
        }
        if let Some(l) = self.limit_reason {
            partial_reasons.push(l);
        }
        json!({
            "state": if ok { "finalized" } else { "failed" },
            "reason": reason, "recording_id": self.artifact_id,
            "artifact_id": if ok { Some(&self.artifact_id) } else { None },
            "frames_written": written, "gap_frames": gap,
            "partial": !partial_reasons.is_empty(), "partial_reasons": partial_reasons,
            "limit_reason": self.limit_reason, "byte_cap": self.byte_cap,
            "max_record_secs": model::MAX_RECORD_SECS, "lossless_claim": false,
            "finalize": how, "exit": exit_json(st, how), "cleanup_ms": cleanup_ms,
            "declared_fps": self.declared_fps,
            "measured_fps": if elapsed > 0.0 { (written as f64 / elapsed * 100.0).round() / 100.0 } else { 0.0 },
            "first_frame_id": self.first_frame_id, "last_frame_id": self.last_frame_id,
            "size": size, "sha256": sha, "container": "matroska", "codec": "mjpeg (copy, not H.264)",
            "stderr_tail": self.stderr.tail(),
            "media_path": if ok { Some(format!("/api/v1/video/artifact/{}", self.artifact_id)) } else { None },
        })
    }
}

fn file_hash(p: &Path) -> Option<(u64, String)> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut n = 0u64;
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => {
                h.update(&buf[..k]);
                n += k as u64;
            }
            Err(_) => return None,
        }
    }
    Some((n, hex::encode(h.finalize())))
}

fn ensure_private_dir(d: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(d)?;
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))
}

#[derive(Debug, Default)]
struct Fps {
    times: VecDeque<Instant>,
}

impl Fps {
    fn tick(&mut self, now: Instant) {
        self.times.push_back(now);
        while self.times.len() > 512 {
            self.times.pop_front();
        }
    }
    fn per_sec(&mut self, now: Instant) -> u64 {
        while self
            .times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.times.pop_front();
        }
        self.times.len() as u64
    }
}

struct Shared {
    id: String,
    epoch: u64,
    cfg: OpenConfig,
    home: PathBuf,
    session: PathBuf,
    scrub: Scrub,
    cx: WorkerCtx,
    em: Emitter,
    requested_by: Origin,
    ring: Mutex<Ring>,
    frozen: AtomicBool,
    stop: Mutex<Option<&'static str>>,
    finished: AtomicBool,
    waiters: Mutex<Vec<tokio::sync::oneshot::Sender<Value>>>,
    open_reply: Mutex<Option<tokio::sync::oneshot::Sender<Result<Value, WorkerError>>>>,
    state: Mutex<Value>,
    health: Mutex<Value>,
    counters: Counters,
    fps: Mutex<Fps>,
    last_frame_at: Mutex<Option<Instant>>,
    progress: Arc<Mutex<Progress>>,
    recorder: Mutex<Option<Recorder>>,
    record_last: Mutex<Value>,
    next_artifact: AtomicU64,
    stopped_at_frame: Mutex<Option<u64>>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Shared {
    fn request_stop(&self, reason: &'static str) {
        let mut s = lock(&self.stop);
        if s.is_none() {
            *s = Some(reason);
        }
    }
    fn stop_reason(&self) -> Option<&'static str> {
        *lock(&self.stop)
    }
    fn running(&self) -> bool {
        !self.finished.load(Ordering::Acquire)
    }

    fn env(&self) -> Option<Env> {
        Some(if self.cfg.source.is_device() { Env::Bench } else { Env::Sim })
    }

    fn draft(&self, kind: &str, payload: Value) -> EventDraft {
        EventDraft {
            kind: kind.into(),
            ts: self.cx.clock().ts(TsSource::HostReadComplete),
            dir: None,
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            job_id: None,
            generation: None,
            purpose: None,
            risk: None,
            env: self.env(),
            payload,
            metrics: vec![],
            quality: vec![],
        }
    }

    fn priority(&self, kind: &str, payload: Value) {
        if self.em.priority(self.draft(kind, payload)).is_err() {
            self.counters.emit_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn media_paths(&self) -> Value {
        json!({"frame": format!("/api/v1/video/{}/frame", self.id),
               "meta": format!("/api/v1/video/{}/meta", self.id)})
    }

    fn state(&self, state: &str, reason: Option<&str>, extra: Value) -> Value {
        let mut p = json!({
            "capture_id": self.id, "capture_epoch": U64(self.epoch), "state": state, "reason": reason,
            "config": self.cfg.public(), "requested_by": self.requested_by,
            "media": self.media_paths(), "stopped_at_frame_id": *lock(&self.stopped_at_frame),
            "frames_total": self.counters.frames.load(Ordering::Relaxed),
        });
        if let (Some(o), Value::Object(e)) = (p.as_object_mut(), extra) {
            o.extend(e);
        }
        self.priority(KIND_CAPTURE_STATE, p.clone());
        *lock(&self.state) = p.clone();
        p
    }

    fn health(&self, state: &str) -> Value {
        let now = Instant::now();
        let fps = lock(&self.fps).per_sec(now);
        let (latest, ring) = {
            let r = lock(&self.ring);
            (r.latest().map(|f| (f.id, f.recv_mono_ns)), r.range())
        };
        let age_ms = lock(&self.last_frame_at).map(|t| ms(now.duration_since(t)));
        let prog = lock(&self.progress).clone();
        let record = lock(&self.recorder).as_ref().map(Recorder::live);
        let c = &self.counters;
        let p = json!({
            "capture_id": self.id, "capture_epoch": U64(self.epoch), "state": state,
            "frames_total": c.frames.load(Ordering::Relaxed),
            "measured_fps": fps, "requested_fps": self.cfg.fps,
            "last_frame_id": latest.map(|l| l.0), "last_recv_mono_ns": latest.map(|l| U64(l.1)),
            "age_ms": age_ms, "stale": age_ms.is_none_or(|a| a > STALL_MS),
            "oversize": c.oversize.load(Ordering::Relaxed),
            "truncated": c.truncated.load(Ordering::Relaxed),
            "junk_bytes": c.junk_bytes.load(Ordering::Relaxed),
            "frozen_dropped": c.frozen_dropped.load(Ordering::Relaxed),
            "ffmpeg": {"frame": prog.frame, "drop_frames": prog.drop_frames, "dup_frames": prog.dup_frames, "speed": prog.speed},
            "media": {"clients": c.media_clients.load(Ordering::Relaxed),
                      "served": c.media_served.load(Ordering::Relaxed),
                      "skipped": c.media_skipped.load(Ordering::Relaxed)},
            "ring": ring, "record": record,
            "emit_errors": c.emit_errors.load(Ordering::Relaxed),
        });
        *lock(&self.health) = p.clone();
        p
    }

    fn emit_health(&self, state: &str) {
        let p = self.health(state);
        if self.em.data(self.draft(KIND_HEALTH, p)).is_err() {
            self.counters.emit_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Accept one parsed frame (reader thread). Frozen captures publish nothing.
    fn accept(&self, data: Vec<u8>) {
        if self.frozen.load(Ordering::Acquire) || self.cx.latch().is_on() || self.stop_reason().is_some() {
            self.frozen.store(true, Ordering::Release);
            self.counters.frozen_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let id = self.counters.frames.fetch_add(1, Ordering::AcqRel) + 1;
        let clock = self.cx.clock();
        let f = Frame::new(id, self.epoch, clock.mono_ns(), clock.wall_ns(), data);
        let now = Instant::now();
        if let Some(r) = lock(&self.recorder).as_mut() {
            r.feed(&f);
        }
        lock(&self.ring).push(f);
        lock(&self.fps).tick(now);
        *lock(&self.last_frame_at) = Some(now);
    }

    fn freeze(&self) {
        self.frozen.store(true, Ordering::Release);
        let last = lock(&self.ring).latest().map(|f| f.id);
        let mut s = lock(&self.stopped_at_frame);
        if s.is_none() {
            *s = last;
        }
    }

    fn finish(&self, p: Value) {
        self.finished.store(true, Ordering::Release);
        for w in lock(&self.waiters).drain(..) {
            let _ = w.send(p.clone());
        }
    }

    fn reply_open(&self, r: Result<Value, WorkerError>) {
        if let Some(tx) = lock(&self.open_reply).take() {
            let _ = tx.send(r);
        }
    }

    fn frame_meta(&self, frame_id: Option<u64>) -> Result<Value, WorkerError> {
        let r = lock(&self.ring);
        let f = match frame_id {
            Some(id) => r.get(id).ok_or_else(|| {
                let first = r.range()["first_frame_id"].as_u64();
                let expired = first.is_some_and(|f| id < f);
                err_d(
                    ErrorCode::NotFound,
                    if expired { "frame expired from the ring" } else { "frame not received" },
                    json!({"frame_id": id, "expired": expired, "ring": r.range()}),
                )
            })?,
            None => r
                .latest()
                .ok_or_else(|| err_d(ErrorCode::NotFound, "no frame yet", json!({"no_frame": true})))?,
        };
        let mut m = f.meta(&self.id);
        m["ring"] = r.range();
        m["frozen"] = json!(self.frozen.load(Ordering::Acquire));
        m["media_path"] = json!(format!("/api/v1/video/{}/frame?id={}", self.id, f.id));
        Ok(m)
    }

    fn snapshot(&self, frame_id: Option<u64>) -> Result<Value, WorkerError> {
        let f = {
            let r = lock(&self.ring);
            match frame_id {
                Some(id) => r.get(id).cloned().ok_or_else(|| {
                    err_d(ErrorCode::NotFound, "frame not in the ring", json!({"frame_id": id, "ring": r.range()}))
                })?,
                None => r
                    .latest()
                    .cloned()
                    .ok_or_else(|| err(ErrorCode::NotFound, "no frame yet"))?,
            }
        };
        let usage = model::scan_usage(&self.home, &self.session);
        if let Some(reason) = usage.refuse(f.data.len() as u64) {
            return Err(err_d(
                ErrorCode::Limit,
                format!("video storage quota: {reason} (no automatic deletion)"),
                json!({"reason": reason, "quota": usage.json()}),
            ));
        }
        let n = self.next_artifact.fetch_add(1, Ordering::AcqRel) + 1;
        let aid = model::artifact_id(ArtifactKind::Snapshot, &self.id, n);
        let dir = model::capture_dir(&self.session, &self.id);
        ensure_private_dir(&dir).map_err(|e| err(ErrorCode::Internal, format!("artifact dir: {e}")))?;
        let path = dir.join(format!("{aid}.jpg"));
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| err(ErrorCode::Internal, format!("snapshot write: {e}")))?;
            file.write_all(&f.data)
                .and_then(|()| file.sync_all())
                .map_err(|e| err(ErrorCode::Internal, format!("snapshot write: {e}")))?;
        }
        let mut p = f.meta(&self.id);
        p["artifact_id"] = json!(aid);
        p["kind"] = json!("snapshot");
        p["media_path"] = json!(format!("/api/v1/video/artifact/{aid}"));
        p["frozen"] = json!(self.frozen.load(Ordering::Acquire));
        self.priority(KIND_SNAPSHOT, p.clone());
        Ok(p)
    }

    fn record_start(self: &Arc<Self>, ffmpeg: &Path) -> Result<Value, WorkerError> {
        if !self.running() || self.frozen.load(Ordering::Acquire) {
            return Err(err(ErrorCode::NotOpen, "capture is not live"));
        }
        if let Some(r) = lock(&self.recorder).as_ref() {
            return Err(err_d(ErrorCode::Busy, "already recording", json!({"recording_id": r.artifact_id})));
        }
        let usage = model::scan_usage(&self.home, &self.session);
        let byte_cap = usage.remaining().min(model::MAX_RECORD_BYTES);
        let refuse = usage.refuse(model::MIN_RECORD_BUDGET).or((byte_cap < model::MIN_RECORD_BUDGET).then_some("quota_bytes"));
        if let Some(reason) = refuse {
            return Err(err_d(
                ErrorCode::Limit,
                format!("video storage quota: {reason}; recording refused (no automatic deletion)"),
                json!({"reason": reason, "quota": usage.json()}),
            ));
        }
        let n = self.next_artifact.fetch_add(1, Ordering::AcqRel) + 1;
        let aid = model::artifact_id(ArtifactKind::Recording, &self.id, n);
        let dir = model::capture_dir(&self.session, &self.id);
        ensure_private_dir(&dir).map_err(|e| err(ErrorCode::Internal, format!("artifact dir: {e}")))?;
        let final_path = dir.join(format!("{aid}.mkv"));
        let part_path = model::partial_path(&final_path);
        let marker = model::recording_marker(&final_path);
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&marker)
                .map_err(|e| err(ErrorCode::Internal, format!("recording marker: {e}")))?;
        }
        let args = model::recorder_args(self.cfg.fps, &part_path);
        let mut child = match spawn_group(ffmpeg, &args, Stdio::piped()) {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&marker);
                return Err(err(ErrorCode::Internal, format!("spawn recorder: {e}")));
            }
        };
        drop(child.stdout.take());
        let stdin: Option<ChildStdin> = child.stdin.take();
        let stderr = Arc::new(StderrRing::default());
        let stderr_join = child
            .stderr
            .take()
            .and_then(|e| drain_lines(e, stderr.clone(), None, self.scrub.clone()).ok());
        let (tx, rx) = mpsc::sync_channel::<Arc<Vec<u8>>>(REC_QUEUE_FRAMES);
        let queued = Arc::new(AtomicUsize::new(0));
        let written = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let (q2, w2, f2) = (queued.clone(), written.clone(), failed.clone());
        let writer = thread("video-rec-writer", move || {
            let Some(mut stdin) = stdin else {
                f2.store(true, Ordering::Relaxed);
                return;
            };
            for data in rx {
                let len = data.len();
                let ok = !f2.load(Ordering::Relaxed) && stdin.write_all(&data).is_ok();
                q2.fetch_sub(len, Ordering::AcqRel);
                if ok {
                    w2.fetch_add(1, Ordering::Relaxed);
                } else {
                    f2.store(true, Ordering::Relaxed);
                }
            }
            drop(stdin);
        });
        let writer = match writer {
            Ok(j) => j,
            Err(e) => {
                kill_reap(&mut child);
                let _ = std::fs::remove_file(&part_path);
                let _ = std::fs::remove_file(&marker);
                return Err(err(ErrorCode::Internal, e));
            }
        };
        let rec = Recorder {
            artifact_id: aid.clone(),
            final_path,
            part_path,
            tx: Some(tx),
            queued_bytes: queued,
            gap: Arc::new(AtomicU64::new(0)),
            written,
            writer_failed: failed,
            first_frame_id: None,
            last_frame_id: 0,
            child,
            writer: Some(writer),
            stderr,
            stderr_join,
            started: Instant::now(),
            declared_fps: self.cfg.fps,
            byte_cap,
            bytes_accepted: 0,
            limit_reason: None,
        };
        let mut p = rec.live();
        p["capture_id"] = json!(self.id);
        p["state"] = json!("recording");
        p["container"] = json!("matroska");
        p["codec"] = json!("mjpeg (copy, not H.264)");
        p["lossless_claim"] = json!(false);
        p["quota"] = usage.json();
        *lock(&self.recorder) = Some(rec);
        self.priority(KIND_RECORD_STATE, p.clone());
        *lock(&self.record_last) = p.clone();
        Ok(p)
    }

    /// Stop the recorder (if any) with deadlines measured from `t0`.
    fn record_stop(&self, t0: Instant, reason: &str) -> Option<Value> {
        let rec = lock(&self.recorder).take()?;
        let mut fin = json!({"capture_id": self.id, "state": "finalizing", "recording_id": rec.artifact_id, "reason": reason});
        self.priority(KIND_RECORD_STATE, fin.clone());
        let mut p = rec.finalize(t0, reason);
        p["capture_id"] = json!(self.id);
        self.priority(KIND_RECORD_STATE, p.clone());
        *lock(&self.record_last) = p.clone();
        fin = p;
        Some(fin)
    }
}

// ---------------------------------------------------------------------------------------------
// Media socket server (read-only: FRAME [id] | META).

struct MediaServer {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for MediaServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaServer").finish_non_exhaustive()
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn remove_stale_socket(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::FileTypeExt as _;
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_socket() => std::fs::remove_file(p),
        Ok(_) => Err(std::io::Error::other("media socket path exists and is not a socket")),
        Err(_) => Ok(()),
    }
}

fn start_media(path: PathBuf, shared: Arc<Shared>) -> Result<MediaServer, String> {
    if let Some(d) = path.parent() {
        ensure_private_dir(d).map_err(|e| format!("media dir: {e}"))?;
    }
    remove_stale_socket(&path).map_err(|e| format!("media socket: {e}"))?;
    let l = UnixListener::bind(&path).map_err(|e| format!("media socket bind: {e}"))?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    l.set_nonblocking(true).map_err(|e| format!("media socket: {e}"))?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let join = thread("video-media", move || {
        while !stop2.load(Ordering::Acquire) {
            match l.accept() {
                Ok((s, _)) => {
                    let c = &shared.counters;
                    if c.media_clients.load(Ordering::Acquire) >= MEDIA_MAX_CLIENTS {
                        c.media_skipped.fetch_add(1, Ordering::Relaxed);
                        let _ = s.set_nonblocking(false);
                        let _ = s.set_write_timeout(Some(Duration::from_millis(100)));
                        let mut s = s;
                        let _ = s.write_all(b"{\"ok\":false,\"error\":\"busy\",\"len\":0}\n");
                        continue;
                    }
                    c.media_clients.fetch_add(1, Ordering::AcqRel);
                    let sh = shared.clone();
                    let spawned = thread("video-media-conn", move || {
                        serve_conn(s, &sh);
                        sh.counters.media_clients.fetch_sub(1, Ordering::AcqRel);
                    });
                    if spawned.is_err() {
                        c.media_clients.fetch_sub(1, Ordering::AcqRel);
                        c.media_skipped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })?;
    Ok(MediaServer {
        path,
        stop,
        join: Some(join),
    })
}

fn serve_conn(mut s: UnixStream, sh: &Shared) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(MEDIA_READ_TIMEOUT));
    let _ = s.set_write_timeout(Some(MEDIA_WRITE_TIMEOUT));
    let mut req = Vec::new();
    let mut byte = [0u8; 1];
    while req.len() < 64 {
        match s.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) => req.push(byte[0]),
            _ => return,
        }
    }
    let req = String::from_utf8_lossy(&req).into_owned();
    let mut parts = req.split_ascii_whitespace();
    let (head, body): (Value, Option<Arc<Vec<u8>>>) = match (parts.next(), parts.next(), parts.next()) {
        (Some("META"), None, None) => {
            let latest = lock(&sh.ring).latest().map(|f| f.meta(&sh.id));
            let health = if sh.running() { sh.health(lock(&sh.state)["state"].as_str().unwrap_or("unknown")) } else { lock(&sh.health).clone() };
            (
                json!({"ok": true, "capture_id": sh.id, "capture_epoch": U64(sh.epoch),
                       "state": lock(&sh.state)["state"], "frozen": sh.frozen.load(Ordering::Acquire),
                       "stopped_at_frame_id": *lock(&sh.stopped_at_frame),
                       "latest": latest, "health": health, "record": *lock(&sh.record_last),
                       "config": sh.cfg.public(), "len": 0}),
                None,
            )
        }
        (Some("FRAME"), id, None) => {
            let id = match id.map(str::parse::<u64>) {
                None => None,
                Some(Ok(n)) => Some(n),
                Some(Err(_)) => {
                    let _ = s.write_all(b"{\"ok\":false,\"error\":\"bad_request\",\"len\":0}\n");
                    return;
                }
            };
            let r = lock(&sh.ring);
            let f = match id {
                Some(n) => r.get(n),
                None => r.latest(),
            };
            match f {
                Some(f) => {
                    let mut m = f.meta(&sh.id);
                    m["ok"] = json!(true);
                    m["len"] = json!(f.data.len());
                    m["frozen"] = json!(sh.frozen.load(Ordering::Acquire));
                    (m, Some(f.data.clone()))
                }
                None => {
                    let first = r.range()["first_frame_id"].as_u64();
                    let e = match (id, first) {
                        (None, _) | (_, None) => "no_frame",
                        (Some(n), Some(f)) if n < f => "expired",
                        _ => "not_received",
                    };
                    (json!({"ok": false, "error": e, "len": 0}), None)
                }
            }
        }
        _ => (json!({"ok": false, "error": "bad_request", "len": 0}), None),
    };
    let mut line = head.to_string();
    line.push('\n');
    let ok = s.write_all(line.as_bytes()).is_ok() && body.as_ref().is_none_or(|b| s.write_all(b).is_ok());
    if ok {
        sh.counters.media_served.fetch_add(1, Ordering::Relaxed);
    } else {
        sh.counters.media_skipped.fetch_add(1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------------------
// Supervisor.

enum End {
    Stop(&'static str),
    Exit(Option<ExitStatus>),
    OpenTimeout,
}

fn supervise(sh: Arc<Shared>, ffmpeg: PathBuf, replay_dir: PathBuf) {
    let args = sh.cfg.acquisition_args(&replay_dir);
    let mut child = match spawn_group(&ffmpeg, &args, Stdio::null()) {
        Ok(c) => c,
        Err(e) => {
            sh.freeze();
            let p = sh.state("failed", Some("spawn_error"), json!({"error": format!("spawn ffmpeg: {e}")}));
            sh.reply_open(Err(err_d(ErrorCode::Internal, "spawn ffmpeg failed", p.clone())));
            sh.finish(p);
            return;
        }
    };
    let t_open = Instant::now();
    sh.state("opening", None, json!({}));
    let stderr = Arc::new(StderrRing::default());
    let mut joins = Vec::new();
    if let Some(e) = child.stderr.take()
        && let Ok(j) = drain_lines(e, stderr.clone(), Some(sh.progress.clone()), sh.scrub.clone())
    {
        joins.push(j);
    }
    if let Some(mut out) = child.stdout.take() {
        let sh2 = sh.clone();
        if let Ok(j) = thread("video-reader", move || {
            let mut sp = JpegSplitter::default();
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match out.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        sh2.counters.stdout_bytes.fetch_add(n as u64, Ordering::Relaxed);
                        let r = sp.push(&buf[..n]);
                        let c = &sh2.counters;
                        c.oversize.fetch_add(r.oversize, Ordering::Relaxed);
                        c.truncated.fetch_add(r.truncated, Ordering::Relaxed);
                        c.junk_bytes.fetch_add(r.junk_bytes, Ordering::Relaxed);
                        for f in r.frames {
                            sh2.accept(f);
                        }
                    }
                }
            }
        }) {
            joins.push(j);
        }
    }

    let mut live = false;
    let mut stalled = false;
    let mut next_health = Instant::now() + HEALTH_PERIOD;
    let end = loop {
        if sh.cx.latch().is_on() {
            sh.request_stop("stop_latch");
        }
        if let Some(r) = sh.stop_reason() {
            sh.freeze();
            break End::Stop(r);
        }
        if let Ok(Some(s)) = child.try_wait() {
            break End::Exit(Some(s));
        }
        let frames = sh.counters.frames.load(Ordering::Acquire);
        let now = Instant::now();
        if !live {
            if frames > 0 {
                live = true;
                let first = sh.frame_meta(None).ok();
                let p = sh.state("live", None, json!({"first_frame": first, "open_ms": ms(t_open.elapsed())}));
                sh.reply_open(Ok(p));
            } else if now.duration_since(t_open) > Duration::from_millis(FIRST_FRAME_MS) {
                sh.freeze();
                break End::OpenTimeout;
            }
        } else {
            let age = lock(&sh.last_frame_at).map(|t| now.duration_since(t));
            let is_stale = age.is_some_and(|a| a > Duration::from_millis(STALL_MS));
            if is_stale && !stalled {
                stalled = true;
                sh.state("stalled", Some("no_frames"), json!({"age_ms": age.map(ms)}));
            } else if !is_stale && stalled {
                stalled = false;
                sh.state("live", Some("frames_resumed"), json!({}));
            }
        }
        let hit = lock(&sh.recorder).as_ref().and_then(|r| r.limit_reason);
        if let Some(reason) = hit {
            // Finalize off the supervisor thread (bounded ≤ 2 s); a second check finds nothing.
            let sh3 = sh.clone();
            let _ = thread("video-rec-limit", move || {
                let _ = sh3.record_stop(Instant::now(), reason);
            });
        }
        if now >= next_health {
            next_health = now + HEALTH_PERIOD;
            let st = if !live { "opening" } else if stalled { "stalled" } else { "live" };
            sh.emit_health(st);
        }
        std::thread::sleep(POLL);
    };

    // Cleanup: freeze → (recorder + acquisition deadlines from t0) → reap → final events.
    let t0 = Instant::now();
    sh.freeze();
    let stopping = matches!(end, End::Stop(_) | End::OpenTimeout);
    if stopping {
        sh.state("stopping", match &end { End::Stop(r) => Some(*r), _ => Some("open_timeout") }, json!({}));
    }
    let (exit, how) = match end {
        End::Exit(s) => (s, "exited"),
        _ => {
            signal_group(child.id(), "-INT");
            match wait_until(&mut child, t0 + ACQ_INT_GRACE) {
                Some(s) => {
                    signal_group(child.id(), "-KILL");
                    (Some(s), "interrupted")
                }
                None => (kill_reap(&mut child), "killed"),
            }
        }
    };
    if how == "exited" {
        // Leader is gone; descendants holding our pipes must not outlive it.
        signal_group(child.id(), "-KILL");
    }
    let rec_reason = match &end {
        End::Stop(r) => *r,
        End::OpenTimeout => "open_timeout",
        End::Exit(_) => "source_ended",
    };
    let rec = sh.record_stop(t0, rec_reason);
    for j in joins {
        let _ = j.join();
    }
    let cleanup_ms = ms(t0.elapsed());
    let tail = stderr.tail();
    let frames = sh.counters.frames.load(Ordering::Acquire);
    let (state, reason) = match &end {
        End::Stop(r) => ("closed", *r),
        End::OpenTimeout => ("failed", "open_timeout"),
        End::Exit(s) => model::classify_exit(s.is_some_and(|s| s.success()), frames, &tail),
    };
    let final_health = sh.health(state);
    if sh.em.data(sh.draft(KIND_HEALTH, final_health)).is_err() {
        sh.counters.emit_errors.fetch_add(1, Ordering::Relaxed);
    }
    let p = sh.state(
        state,
        Some(reason),
        json!({"exit": exit_json(exit, how), "cleanup_ms": cleanup_ms, "stderr_tail": tail,
               "record": rec, "error": if state == "failed" { Some(reason) } else { None }}),
    );
    if !live {
        sh.reply_open(Err(err_d(
            if reason == "connection_refused" || reason == "source_not_found" { ErrorCode::NotFound } else { ErrorCode::Disconnected },
            format!("video source did not deliver a frame ({reason})"),
            p.clone(),
        )));
    }
    sh.finish(p);
}

// ---------------------------------------------------------------------------------------------
// Worker.

struct Running {
    sh: Arc<Shared>,
    join: Option<JoinHandle<()>>,
    // Held only to keep the read-only socket alive; history stays readable until the next open.
    _media: Option<MediaServer>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running").field("id", &self.sh.id).finish_non_exhaustive()
    }
}

/// The video worker (one capture at a time; the last one stays readable until the next open).
#[derive(Debug, Default)]
pub struct VideoWorker {
    cur: Option<Running>,
    next: u64,
    probe: Arc<Mutex<Option<Value>>>,
}

fn storage(call: &Call) -> Result<(PathBuf, PathBuf), WorkerError> {
    let s = &call.args["storage"];
    match (s["home"].as_str(), s["session"].as_str()) {
        (Some(h), Some(se)) if Path::new(h).is_absolute() && Path::new(se).is_absolute() => {
            Ok((PathBuf::from(h), PathBuf::from(se)))
        }
        _ => Err(err(
            ErrorCode::Config,
            "daemon storage roots unavailable (no fallback to ~/.loom)",
        )),
    }
}

fn ffmpeg_path(call: &Call) -> Result<PathBuf, WorkerError> {
    match call.args["ffmpeg"].as_str() {
        Some(p) if Path::new(p).is_absolute() => Ok(PathBuf::from(p)),
        Some(p) => Err(err(ErrorCode::Config, format!("ffmpeg path {p:?} is not absolute"))),
        None => Err(err(
            ErrorCode::NotFound,
            call.args["ffmpeg_error"].as_str().unwrap_or("ffmpeg not found").to_owned(),
        )),
    }
}

fn frame_arg(call: &Call) -> Result<Option<u64>, WorkerError> {
    match call.args.get("frame_id") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .filter(|n| *n > 0)
            .map(Some)
            .ok_or_else(|| err(ErrorCode::Usage, "frame_id: expected a positive integer")),
    }
}

impl VideoWorker {
    /// New worker.
    pub fn new() -> Self {
        VideoWorker::default()
    }

    fn target(&self, call: &Call) -> Result<Arc<Shared>, WorkerError> {
        let want = call.args.get("capture_id").and_then(Value::as_str);
        let Some(r) = &self.cur else {
            return Err(err(ErrorCode::NotOpen, "no capture"));
        };
        if want.is_some_and(|w| w != r.sh.id) {
            return Err(err_d(
                ErrorCode::NotFound,
                format!("capture {:?} is not the current capture", want.unwrap_or_default()),
                json!({"current": r.sh.id}),
            ));
        }
        Ok(r.sh.clone())
    }

    fn reap_finished(&mut self) {
        if let Some(r) = self.cur.as_mut()
            && r.sh.finished.load(Ordering::Acquire)
            && let Some(j) = r.join.take()
        {
            let _ = j.join();
        }
    }

    fn open(&mut self, cx: &WorkerCtx, call: Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        self.reap_finished();
        let prepared = (|| {
            if cx.latch().is_on() {
                return Err(err(ErrorCode::Stopped, "STOP latched: open refused"));
            }
            if let Some(r) = &self.cur
                && r.sh.running()
            {
                return Err(err_d(
                    ErrorCode::Busy,
                    format!("capture {} is open (one capture at a time)", r.sh.id),
                    json!({"capture_id": r.sh.id}),
                ));
            }
            let cfg = OpenConfig::from_params(&call.args["config"]).map_err(|e| err(e.code, e.message))?;
            let (home, session) = storage(&call)?;
            let ffmpeg = ffmpeg_path(&call)?;
            let replay = model::replay_dir(&home);
            if let Source::Replay { name } = &cfg.source {
                let p = replay.join(name);
                match std::fs::symlink_metadata(&p) {
                    Ok(m) if m.is_file() => {}
                    _ => {
                        return Err(err(
                            ErrorCode::NotFound,
                            format!("replay file {name:?} not found in $LOOM_HOME/video/replay"),
                        ));
                    }
                }
            }
            Ok((cfg, home, session, ffmpeg, replay))
        })();
        let (cfg, home, session, ffmpeg, replay) = match prepared {
            Ok(v) => v,
            Err(e) => return ready(Err(e)),
        };
        self.next += 1;
        let clock = cx.clock();
        let id = model::new_capture_id(&format!(
            "{}-{}-{}-{}",
            clock.wall_ns(),
            clock.mono_ns(),
            std::process::id(),
            self.next
        ));
        let sock = match model::media_socket_path(&home, &id) {
            Ok(p) => p,
            Err(e) => return ready(Err(err(e.code, e.message))),
        };
        // Previous capture's history (ring + socket) ends here.
        self.cur = None;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let scrub = Scrub {
            source: cfg.source.clone(),
            home: home.display().to_string(),
            session: session.display().to_string(),
        };
        let sh = Arc::new(Shared {
            id: id.clone(),
            epoch: clock.wall_ns().wrapping_add(self.next),
            cfg,
            home: home.clone(),
            session,
            scrub,
            em: cx.emitter(&id),
            cx: cx.clone(),
            requested_by: call.origin,
            ring: Mutex::new(Ring::default()),
            frozen: AtomicBool::new(false),
            stop: Mutex::new(None),
            finished: AtomicBool::new(false),
            waiters: Mutex::new(Vec::new()),
            open_reply: Mutex::new(Some(tx)),
            state: Mutex::new(Value::Null),
            health: Mutex::new(Value::Null),
            counters: Counters::default(),
            fps: Mutex::new(Fps::default()),
            last_frame_at: Mutex::new(None),
            progress: Arc::new(Mutex::new(Progress::default())),
            recorder: Mutex::new(None),
            record_last: Mutex::new(Value::Null),
            next_artifact: AtomicU64::new(0),
            stopped_at_frame: Mutex::new(None),
        });
        let media = match start_media(sock, sh.clone()) {
            Ok(m) => m,
            Err(e) => return ready(Err(err(ErrorCode::Internal, e))),
        };
        let sh2 = sh.clone();
        let join = std::thread::Builder::new()
            .name(format!("video-{id}"))
            .spawn(move || supervise(sh2, ffmpeg, replay));
        match join {
            Ok(j) => {
                self.cur = Some(Running {
                    sh,
                    join: Some(j),
                    _media: Some(media),
                });
                Box::pin(async move {
                    rx.await
                        .unwrap_or_else(|_| Err(err(ErrorCode::Internal, "capture ended before open completed")))
                })
            }
            Err(e) => ready(Err(err(ErrorCode::Internal, format!("thread: {e}")))),
        }
    }

    fn close(&mut self, call: &Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let sh = match self.target(call) {
            Ok(s) => s,
            Err(e) => return ready(Err(e)),
        };
        if !sh.running() {
            let last = lock(&sh.state).clone();
            return ready(Ok(json!({"closed": false, "reason": "not_running", "last": last})));
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        lock(&sh.waiters).push(tx);
        sh.request_stop("close");
        if !sh.running() {
            let last = lock(&sh.state).clone();
            for w in lock(&sh.waiters).drain(..) {
                let _ = w.send(last.clone());
            }
        }
        Box::pin(async move {
            let mut v = rx
                .await
                .map_err(|_| err(ErrorCode::Internal, "capture ended without a final state"))?;
            v["closed"] = json!(true);
            Ok(v)
        })
    }

    fn status(&mut self) -> Value {
        self.reap_finished();
        match &self.cur {
            None => json!({"capture": null, "running": false}),
            Some(r) => {
                let sh = &r.sh;
                let health = if sh.running() {
                    sh.health(lock(&sh.state)["state"].as_str().unwrap_or("unknown"))
                } else {
                    lock(&sh.health).clone()
                };
                json!({"capture": *lock(&sh.state), "health": health, "record": *lock(&sh.record_last),
                       "recording": lock(&sh.recorder).is_some(),
                       "running": sh.running(), "capture_id": sh.id, "frozen": sh.frozen.load(Ordering::Acquire)})
            }
        }
    }

    fn sources(&self, call: &Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let local = call.args["local"].as_bool().unwrap_or(false);
        let home = storage(call).ok().map(|(h, _)| h);
        let ffmpeg = ffmpeg_path(call).ok();
        offload("video-sources", move || {
            let mut replay = Vec::new();
            if let Some(h) = &home
                && let Ok(rd) = std::fs::read_dir(model::replay_dir(h))
            {
                for e in rd.flatten().take(MAX_REPLAY_LIST * 4) {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if model::check_replay_name(&name).is_ok() && e.file_type().is_ok_and(|t| t.is_file()) {
                        replay.push(name);
                    }
                    if replay.len() >= MAX_REPLAY_LIST {
                        break;
                    }
                }
                replay.sort();
            }
            let mut v = json!({"kinds": model::SOURCE_KINDS, "replay": replay,
                               "local": {"enumerated": false,
                                         "note": "local AVFoundation devices are listed only with local=true (may prompt for camera permission)"}});
            if local {
                let Some(ff) = ffmpeg else {
                    return Err(err(ErrorCode::NotFound, "ffmpeg not found"));
                };
                let (_, stderr, _) = run_bounded(&ff, &model::list_devices_args(), 256 * 1024)
                    .map_err(|e| err(ErrorCode::Timeout, e))?;
                v["local"] = json!({"enumerated": true, "avfoundation_video": model::parse_avf_devices(&stderr),
                                    "warning": "enumeration may trigger macOS camera permission (TCC); selection must use the exact name",
                                    "device_verified": false});
            }
            Ok(v)
        })
    }

    fn probe(&self, call: &Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let ff = call.args["ffmpeg"].as_str().map(PathBuf::from);
        let ff_err = call.args["ffmpeg_error"].as_str().map(str::to_owned);
        let cache = self.probe.clone();
        if let Some(v) = lock(&cache).clone()
            && v["path"].as_str() == ff.as_deref().and_then(Path::to_str)
        {
            return ready(Ok(v));
        }
        offload("video-probe", move || {
            let Some(ff) = ff else {
                return Ok(json!({"path": null, "version": null, "error": ff_err}));
            };
            let v = match run_bounded(&ff, &["-hide_banner".into(), "-version".into()], 64 * 1024) {
                Ok((out, _, true)) => {
                    json!({"path": ff, "version": out.lines().next().unwrap_or("").trim(), "error": null})
                }
                Ok((_, e, false)) => json!({"path": ff, "version": null, "error": e.lines().last().unwrap_or("ffmpeg -version failed")}),
                Err(e) => json!({"path": ff, "version": null, "error": e}),
            };
            if v["error"].is_null() {
                *lock(&cache) = Some(v.clone());
            }
            Ok(v)
        })
    }
}

impl Drop for VideoWorker {
    fn drop(&mut self) {
        if let Some(mut r) = self.cur.take() {
            r.sh.request_stop("worker_shutdown");
            if let Some(j) = r.join.take() {
                // Bounded: the supervisor's cleanup ends with SIGKILL + wait.
                let _ = j.join();
            }
        }
    }
}

impl Worker for VideoWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        match call.method.as_str() {
            "video.open" => self.open(cx, call),
            "video.close" => self.close(&call),
            "video.status" => ready(Ok(self.status())),
            "video.sources" => self.sources(&call),
            "video.probe" => self.probe(&call),
            "video.quota" => match storage(&call) {
                Ok((home, session)) => offload("video-quota", move || {
                    Ok(model::scan_usage(&home, &session).json())
                }),
                Err(e) => ready(Err(e)),
            },
            "video.frame" => {
                let r = self.target(&call).and_then(|sh| frame_arg(&call).and_then(|id| sh.frame_meta(id)));
                ready(r)
            }
            "video.snapshot" => {
                if cx.latch().is_on() {
                    return ready(Err(err(ErrorCode::Stopped, "STOP latched: snapshot refused")));
                }
                let sh = match self.target(&call) {
                    Ok(s) => s,
                    Err(e) => return ready(Err(e)),
                };
                let id = match frame_arg(&call) {
                    Ok(i) => i,
                    Err(e) => return ready(Err(e)),
                };
                offload("video-snapshot", move || sh.snapshot(id))
            }
            "video.record.start" => {
                if cx.latch().is_on() {
                    return ready(Err(err(ErrorCode::Stopped, "STOP latched: recording refused")));
                }
                let sh = match self.target(&call) {
                    Ok(s) => s,
                    Err(e) => return ready(Err(e)),
                };
                let ff = match ffmpeg_path(&call) {
                    Ok(f) => f,
                    Err(e) => return ready(Err(e)),
                };
                offload("video-rec-start", move || sh.record_start(&ff))
            }
            "video.record.stop" => {
                let sh = match self.target(&call) {
                    Ok(s) => s,
                    Err(e) => return ready(Err(e)),
                };
                offload("video-rec-stop", move || {
                    Ok(sh
                        .record_stop(Instant::now(), "record_stop")
                        .unwrap_or_else(|| json!({"stopped": false, "reason": "not_recording", "last": *lock(&sh.record_last)})))
                })
            }
            m => {
                let m = m.to_owned();
                ready(Err(err(ErrorCode::NotImplemented, format!("{m} not implemented"))))
            }
        }
    }

    fn on_write<'a>(&'a mut self, _cx: &'a WorkerCtx, _w: WorkerWrite) -> BoxFuture<'a, WriteOutcome> {
        ready(WriteOutcome {
            state: WriteState::Failed,
            detail: Value::Null,
            error: Some(err(ErrorCode::NotImplemented, "video never writes to a device")),
        })
    }

    fn on_job<'a>(&'a mut self, _cx: &'a WorkerCtx, _job: JobCmd) -> BoxFuture<'a, Result<Value, WorkerError>> {
        ready(Err(err(ErrorCode::NotImplemented, "video has no jobs")))
    }

    fn on_input<'a>(&'a mut self, _cx: &'a WorkerCtx, _input: Input) -> BoxFuture<'a, Result<(), WorkerError>> {
        ready(Err(err(ErrorCode::NotImplemented, "video takes no input")))
    }

    fn on_latch(&mut self, on: bool) {
        // The supervisor and reader also poll the atomic latch; this only makes the freeze
        // immediate when the callback arrives first.
        if on && let Some(r) = &self.cur {
            r.sh.freeze();
            r.sh.request_stop("stop_latch");
        }
    }
}
