//! Worker piece: owns the external `sigrok-cli` processes of one capture at a time.
//!
//! Threads per capture (none of them block a worker callback):
//! * supervisor — spawns, pumps raw chunks (edge compression + tee to the decoder), enforces
//!   STOP/duration/samples, runs the bounded stop/reap sequence and emits the final state;
//! * acquisition stdout reader (64 KiB chunks), decoder stdin writer (byte-bounded queue),
//!   decoder stdout line reader (≤ 4 KiB lines), annotation batcher, two stderr rings.
//!
//! Stop: newline on the acquisition stdin (clean rc 0, verified) → 2 s → `kill -INT` → 1 s →
//! SIGKILL; then decoder stdin EOF → drain ≤ 2 s → SIGKILL. Every child is waited (reaped). The
//! acquisition stdin pipe stays owned by the worker, so a crashed worker closes it and the child
//! exits on its own.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, TsSource, WriteState};
use loom_proto::worker::{Call, Input, WorkerError, Write as WorkerWrite};
use loom_worker_sdk::{BoxFuture, Emitter, EventDraft, JobCmd, RateGuard, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};

use crate::model::{self, Annotation, EdgeAcc, Source, StartConfig, Synth, mask_channels, parse_annotation};
use crate::{KIND_ANNOTATIONS, KIND_CAPTURE_STATE, KIND_EDGES, WORKER_KIND};

/// Raw read chunk.
pub const READ_CHUNK: usize = 64 * 1024;
/// Decoder stdin queue bound; exceeding it stops the capture (never drops decoder input).
pub const DECODER_QUEUE_BYTES: usize = 4 << 20;
/// Longest decoder stdout line kept.
pub const MAX_LINE: usize = 4096;
/// stderr ring per process.
pub const STDERR_RING: usize = 64 * 1024;
/// stderr tail in state events.
pub const STDERR_TAIL: usize = 4096;
/// Edge event period.
pub const EDGE_FLUSH: Duration = Duration::from_millis(100);
/// Annotation batch period / size.
pub const ANNOT_FLUSH: Duration = Duration::from_millis(50);
/// Annotation rows per batch.
pub const ANNOT_BATCH: usize = 256;
/// Annotation rate (rows/s) and burst; excess is counted as dropped.
pub const ANNOT_RATE: u32 = 2000;
/// Burst.
pub const ANNOT_BURST: u32 = 4000;
/// Grace after the stop newline.
pub const STOP_KEY_GRACE: Duration = Duration::from_secs(2);
/// Grace after SIGINT.
pub const STOP_INT_GRACE: Duration = Duration::from_secs(1);
/// Decoder drain after stdin EOF.
pub const DECODER_DRAIN: Duration = Duration::from_secs(2);
/// Engine probe timeout.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn err(code: ErrorCode, message: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------------------------

/// Stop request + completion waiters of one capture.
#[derive(Debug, Default)]
struct Control {
    stop: Mutex<Option<&'static str>>,
    waiters: Mutex<Vec<tokio::sync::oneshot::Sender<Value>>>,
    finished: AtomicBool,
}

impl Control {
    fn request(&self, reason: &'static str) {
        let mut s = lock(&self.stop);
        if s.is_none() {
            *s = Some(reason);
        }
    }
    fn requested(&self) -> Option<&'static str> {
        *lock(&self.stop)
    }
}

#[derive(Debug)]
struct Running {
    id: String,
    ctl: Arc<Control>,
    join: Option<JoinHandle<()>>,
}

/// The logic worker (one capture at a time).
#[derive(Debug, Default)]
pub struct LogicWorker {
    run: Option<Running>,
    last: Arc<Mutex<Value>>,
    probe: Arc<Mutex<Option<Value>>>,
    next: u64,
}

impl LogicWorker {
    /// New worker.
    pub fn new() -> Self {
        LogicWorker::default()
    }

    fn reap_finished(&mut self) {
        if self
            .run
            .as_ref()
            .is_some_and(|r| r.ctl.finished.load(Ordering::Acquire))
            && let Some(mut r) = self.run.take()
            && let Some(j) = r.join.take()
        {
            let _ = j.join();
        }
    }

    fn start(&mut self, cx: &WorkerCtx, call: Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        self.reap_finished();
        let prepared = (|| {
            if cx.latch().is_on() && call.origin != Origin::User {
                return Err(err(ErrorCode::Stopped, "STOP latched: start refused"));
            }
            if let Some(r) = &self.run {
                return Err(err(ErrorCode::Busy, format!("capture {} is running", r.id))
                    .with_detail(json!({"capture_id": r.id})));
            }
            let cfg = StartConfig::from_params(&call.args["config"]).map_err(|e| err(e.code, e.message))?;
            let engine = match call.args.get("engine").and_then(Value::as_str) {
                Some(p) if Path::new(p).is_absolute() => Ok(PathBuf::from(p)),
                Some(p) => Err(format!("engine path {p:?} is not absolute")),
                None => Err(call.args["engine_error"]
                    .as_str()
                    .unwrap_or("sigrok-cli not found")
                    .to_owned()),
            };
            let needs_engine = cfg.source.acquires() || cfg.decoder.is_some();
            if needs_engine && let Err(e) = &engine {
                return Err(err(ErrorCode::NotFound, e.clone()));
            }
            Ok((cfg, engine.ok()))
        })();
        let (cfg, engine) = match prepared {
            Ok(v) => v,
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        self.next += 1;
        let id = format!("lc-{:x}-{}", cx.clock().wall_ns() / 1_000_000, self.next);
        let ctl = Arc::new(Control::default());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let cap = Capture {
            em: cx.emitter(&id),
            cx: cx.clone(),
            id: id.clone(),
            cfg,
            engine,
            requested_by: call.origin,
            counters: Arc::new(Counters::default()),
            last: self.last.clone(),
            ctl: ctl.clone(),
        };
        let join = std::thread::Builder::new()
            .name(format!("logic-{id}"))
            .spawn(move || cap.run(tx));
        match join {
            Ok(j) => {
                self.run = Some(Running {
                    id,
                    ctl,
                    join: Some(j),
                });
                Box::pin(async move {
                    rx.await.unwrap_or_else(|_| {
                        Err(err(ErrorCode::Internal, "capture thread ended before start"))
                    })
                })
            }
            Err(e) => Box::pin(async move { Err(err(ErrorCode::Internal, format!("thread: {e}"))) }),
        }
    }

    fn stop(&mut self, call: &Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let want = call.args.get("capture_id").and_then(Value::as_str);
        let last = lock(&self.last).clone();
        let Some(r) = self
            .run
            .as_ref()
            .filter(|r| !r.ctl.finished.load(Ordering::Acquire))
        else {
            return Box::pin(
                async move { Ok(json!({"stopped": false, "reason": "not_running", "last": last})) },
            );
        };
        if want.is_some_and(|w| w != r.id) {
            let msg = format!(
                "capture {:?} is not running (running: {})",
                want.unwrap_or_default(),
                r.id
            );
            return Box::pin(async move { Err(err(ErrorCode::NotFound, msg)) });
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        lock(&r.ctl.waiters).push(tx);
        r.ctl.request("user_stop");
        // A capture that finished between the check and the push still answers.
        if r.ctl.finished.load(Ordering::Acquire) {
            for w in lock(&r.ctl.waiters).drain(..) {
                let _ = w.send(last.clone());
            }
        }
        Box::pin(async move {
            let mut v = rx
                .await
                .map_err(|_| err(ErrorCode::Internal, "capture ended without a final state"))?;
            v["stopped"] = json!(true);
            Ok(v)
        })
    }

    fn status(&mut self) -> BoxFuture<'static, Result<Value, WorkerError>> {
        self.reap_finished();
        let capture = lock(&self.last).clone();
        let running = self.run.as_ref().map(|r| r.id.clone());
        let cache = self.probe.clone();
        Box::pin(async move {
            let cached = lock(&cache).clone();
            Ok(
                json!({"capture": if capture.is_null() { Value::Null } else { capture },
                      "running": running, "probe": cached}),
            )
        })
    }

    fn probe(&mut self, call: &Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let engine = call.args.get("engine").and_then(Value::as_str).map(PathBuf::from);
        let cache = self.probe.clone();
        if let Some(v) = lock(&cache).clone()
            && v["engine"]["path"].as_str() == engine.as_deref().and_then(Path::to_str)
        {
            return Box::pin(async move { Ok(v) });
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("logic-probe".into())
            .spawn(move || {
                let v = probe_engine(engine.as_deref());
                if v["engine"]["error"].is_null() {
                    *lock(&cache) = Some(v.clone());
                }
                let _ = tx.send(v);
            });
        Box::pin(async move {
            spawned.map_err(|e| err(ErrorCode::Internal, format!("thread: {e}")))?;
            rx.await
                .map_err(|_| err(ErrorCode::Internal, "probe thread ended"))
        })
    }
}

impl Drop for LogicWorker {
    fn drop(&mut self) {
        if let Some(mut r) = self.run.take() {
            r.ctl.request("worker_shutdown");
            if let Some(j) = r.join.take() {
                // Bounded: the supervisor's stop sequence ends with SIGKILL + wait.
                let _ = j.join();
            }
        }
    }
}

trait WithDetail {
    fn with_detail(self, d: Value) -> Self;
}
impl WithDetail for WorkerError {
    fn with_detail(mut self, d: Value) -> Self {
        self.detail = d;
        self
    }
}

impl Worker for LogicWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        match call.method.as_str() {
            "logic.start" => self.start(cx, call),
            "logic.stop" => self.stop(&call),
            "logic.status" => self.status(),
            "logic.probe" => self.probe(&call),
            m => {
                let m = m.to_owned();
                Box::pin(async move { Err(err(ErrorCode::NotImplemented, format!("{m} not implemented"))) })
            }
        }
    }

    fn on_write<'a>(&'a mut self, _cx: &'a WorkerCtx, _w: WorkerWrite) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async {
            WriteOutcome {
                state: WriteState::Failed,
                detail: Value::Null,
                error: Some(err(ErrorCode::NotImplemented, "logic never writes to a device")),
            }
        })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async { Err(err(ErrorCode::NotImplemented, "logic has no jobs")) })
    }

    fn on_input<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _input: Input,
    ) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async { Err(err(ErrorCode::NotImplemented, "logic takes no input")) })
    }

    fn on_latch(&mut self, on: bool) {
        if on && let Some(r) = &self.run {
            r.ctl.request("stop_latch");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Process groups. Every child is its own group leader (`process_group(0)`), so signals go to the
// whole group: descendants that inherited our pipes die too and reader joins stay bounded.
// No `unsafe` (workspace lint): kill(1) with a fixed argv, never a shell.

fn signal_group(pgid: u32, sig: &str) {
    let _ = Command::new("/bin/kill")
        .args([sig, "--", &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// SIGKILL the group, then reap the leader.
fn kill_group_wait(child: &mut Child) -> Option<ExitStatus> {
    signal_group(child.id(), "-KILL");
    let _ = child.kill();
    child.wait().ok()
}

fn wait_bounded(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if t0.elapsed() < limit => std::thread::sleep(Duration::from_millis(5)),
            _ => return None,
        }
    }
}

/// Reap within `limit` (else group SIGKILL). After the leader exits, any remaining group members
/// (descendants holding our pipes) get SIGKILL so pipe readers reach EOF.
fn reap(child: &mut Child, limit: Duration) -> (Option<ExitStatus>, bool) {
    match wait_bounded(child, limit) {
        Some(s) => {
            signal_group(child.id(), "-KILL");
            (Some(s), false)
        }
        None => (kill_group_wait(child), true),
    }
}

fn spawn_group(engine: &Path, args: &[String], stdin: Stdio) -> std::io::Result<Child> {
    let mut c = Command::new(engine);
    c.args(args)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C");
    #[cfg(unix)]
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

// ---------------------------------------------------------------------------------------------
// Engine probe (`--version`, `-L`): compiled-in lists only, never a device scan.

fn run_bounded(engine: &Path, args: &[&str], max: usize) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let mut child =
        spawn_group(engine, &args, Stdio::null()).map_err(|e| format!("spawn {}: {e}", engine.display()))?;
    drop(child.stderr.take());
    let Some(mut out) = child.stdout.take() else {
        kill_group_wait(&mut child);
        return Err("no stdout".into());
    };
    let (tx, rx) = mpsc::channel();
    let reader = thread("logic-probe-out", move || {
        let mut buf = Vec::new();
        let _ = out.by_ref().take(max as u64).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    if let Err(e) = reader {
        kill_group_wait(&mut child);
        return Err(e);
    }
    let (status, killed) = reap(&mut child, PROBE_TIMEOUT);
    let buf = rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
    match status {
        Some(s) if s.success() && !killed => Ok(String::from_utf8_lossy(&buf).into_owned()),
        _ if killed => Err(format!("{} {args:?} timed out", engine.display())),
        Some(s) => Err(format!("{} {args:?} exited with {s}", engine.display())),
        None => Err(format!("{} {args:?}: wait failed", engine.display())),
    }
}

/// Parse a `-L` section (`Supported protocol decoders:` …) into first-column ids.
pub fn list_section(text: &str, header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("Supported ") {
            inside = line.trim_end().trim_end_matches(':') == header.trim_end_matches(':');
            continue;
        }
        if inside {
            match line.split_whitespace().next() {
                Some(id) if line.starts_with(' ') => out.push(id.to_owned()),
                _ => inside = false,
            }
        }
    }
    out
}

fn probe_engine(engine: Option<&Path>) -> Value {
    let Some(engine) = engine else {
        return json!({"engine": {"path": null, "version": null, "error": "sigrok-cli not found"}});
    };
    let version = run_bounded(engine, &["--version"], 16 * 1024);
    let list = run_bounded(engine, &["-L"], 1 << 20);
    let error = version.as_ref().err().or(list.as_ref().err()).cloned();
    let list = list.unwrap_or_default();
    json!({
        "engine": {"path": engine.to_str(),
                    "version": version.ok().and_then(|v| v.lines().next().map(str::to_owned)),
                    "error": error},
        "compiled": {"drivers": list_section(&list, "Supported hardware drivers"),
                     "decoders": list_section(&list, "Supported protocol decoders")},
    })
}

// ---------------------------------------------------------------------------------------------
// Capture supervisor.

#[derive(Debug, Default)]
struct Counters {
    samples: AtomicU64,
    edges: AtomicU64,
    annotations: AtomicU64,
    dropped_edges: AtomicU64,
    dropped_annotations: AtomicU64,
    truncated_lines: AtomicU64,
    unparsed_lines: AtomicU64,
    emit_errors: AtomicU64,
    decoder_input_bytes: AtomicU64,
    /// Annotations lost before the batcher (queue full); folded into the next batch's `dropped`.
    reader_dropped: AtomicU64,
}

impl Counters {
    fn json(&self) -> Value {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        json!({"samples": g(&self.samples), "edges": g(&self.edges), "annotations": g(&self.annotations),
               "dropped_edges": g(&self.dropped_edges),
               "dropped_annotations": g(&self.dropped_annotations),
               "queue_dropped_annotations": g(&self.reader_dropped),
               "truncated_lines": g(&self.truncated_lines), "unparsed_lines": g(&self.unparsed_lines),
               "emit_errors": g(&self.emit_errors), "decoder_input_bytes": g(&self.decoder_input_bytes)})
    }
}

#[derive(Debug, Default)]
struct Ring(Mutex<VecDeque<u8>>);

impl Ring {
    fn push(&self, b: &[u8]) {
        let mut q = lock(&self.0);
        q.extend(b);
        let over = q.len().saturating_sub(STDERR_RING);
        q.drain(..over);
    }
    fn tail(&self, n: usize) -> String {
        let q = lock(&self.0);
        let skip = q.len().saturating_sub(n);
        String::from_utf8_lossy(&q.iter().skip(skip).copied().collect::<Vec<u8>>()).into_owned()
    }
}

fn drain_stderr(mut r: impl Read + Send + 'static, ring: Arc<Ring>) -> Result<JoinHandle<()>, String> {
    thread("logic-stderr", move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = r.read(&mut buf) {
            if n == 0 {
                break;
            }
            ring.push(&buf[..n]);
        }
    })
}

/// One spawned sigrok-cli and its pipe threads.
struct Proc {
    child: Child,
    stderr: Arc<Ring>,
    threads: Vec<JoinHandle<()>>,
}

impl Proc {
    /// Group-kill, reap and join (setup-failure cleanup).
    fn abort(mut self) {
        kill_group_wait(&mut self.child);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

struct Capture {
    cx: WorkerCtx,
    em: Emitter,
    id: String,
    cfg: StartConfig,
    engine: Option<PathBuf>,
    requested_by: Origin,
    counters: Arc<Counters>,
    last: Arc<Mutex<Value>>,
    ctl: Arc<Control>,
}

fn exit_json(s: Option<ExitStatus>, style: &str) -> Value {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt as _;
        s.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    json!({"code": s.and_then(|s| s.code()), "signal": signal, "stop_style": style})
}

enum Data {
    Chunk(Vec<u8>),
    Idle,
    Ended,
}

/// Decoder tee: byte-bounded queue to the stdin writer thread. The first failure is sticky —
/// nothing is fed afterwards, so the decoder never sees a sample-axis hole.
struct Tee {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Bytes handed to the writer (must all reach the decoder for a clean stop).
    sent: u64,
    queued: Arc<AtomicUsize>,
    broken: Arc<AtomicBool>,
    failed: Option<String>,
}

impl Tee {
    fn feed(&mut self, chunk: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        let Some(tx) = &self.tx else { return };
        if self.broken.load(Ordering::Acquire) {
            self.failed = Some("decoder input closed (write failed)".into());
            return;
        }
        let q = self.queued.fetch_add(chunk.len(), Ordering::AcqRel) + chunk.len();
        if q > DECODER_QUEUE_BYTES {
            self.queued.fetch_sub(chunk.len(), Ordering::AcqRel);
            self.failed = Some("decoder backpressure: input queue exceeded 4 MiB".into());
            return;
        }
        if tx.send(chunk.to_vec()).is_err() {
            self.failed = Some("decoder input closed".into());
        } else {
            self.sent += chunk.len() as u64;
        }
    }
}

impl Capture {
    fn quality(&self) -> Vec<String> {
        if self.cfg.source == Source::Synthetic {
            vec!["synthetic".into()]
        } else {
            Vec::new()
        }
    }

    fn env(&self) -> Option<Env> {
        match self.cfg.source {
            Source::SigrokDevice => Some(Env::Bench),
            _ => Some(Env::Sim),
        }
    }

    fn draft(&self, kind: &str, payload: Value, quality: Vec<String>, dir: Option<Dir>) -> EventDraft {
        EventDraft {
            kind: kind.into(),
            ts: self.cx.clock().ts(TsSource::HostReadComplete),
            dir,
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
            quality,
        }
    }

    fn argv(&self) -> Value {
        let prog = self.engine.as_ref().and_then(|p| p.to_str());
        let with = |a: Option<Vec<String>>| {
            a.map(|a| {
                let mut v = vec![prog.unwrap_or("sigrok-cli").to_owned()];
                v.extend(a);
                v
            })
        };
        json!({"acquisition": with(self.cfg.acquisition_args()), "decoder": with(self.cfg.decoder_args())})
    }

    fn state(
        &self,
        state: &str,
        reason: Option<&str>,
        exit: Value,
        stderr_tail: &str,
        error: Option<&str>,
    ) -> Value {
        let p = json!({
            "capture_id": self.id, "state": state, "source": self.cfg.source.as_str(),
            "driver": self.cfg.driver, "samplerate": self.cfg.samplerate,
            "channels": mask_channels(self.cfg.mask), "mask": self.cfg.mask, "unit_size": 1,
            "decoders": self.cfg.decoder.iter().map(model::Decoder::canonical).collect::<Vec<_>>(),
            "annotation_filter": self.cfg.decoder.as_ref().map(model::Decoder::annotation_filter),
            "limits": {"max_duration_ms": self.cfg.duration_ms, "max_samples": self.cfg.max_samples},
            "config": self.cfg.to_params(), "argv": self.argv(),
            "requested_by": self.requested_by, "reason": reason, "error": error,
            "exit": exit, "stderr_tail": stderr_tail, "counters": self.counters.json(),
            "quality": self.quality(),
        });
        if self
            .em
            .priority(self.draft(KIND_CAPTURE_STATE, p.clone(), self.quality(), None))
            .is_err()
        {
            self.counters.emit_errors.fetch_add(1, Ordering::Relaxed);
        }
        *lock(&self.last) = p.clone();
        p
    }

    fn finish(&self, p: Value) {
        self.ctl.finished.store(true, Ordering::Release);
        for w in lock(&self.ctl.waiters).drain(..) {
            let _ = w.send(p.clone());
        }
    }

    fn flush_edges(&self, acc: &mut EdgeAcc, seq: &mut u64) {
        if let Some((p, n, dropped)) = acc.take(&self.id, *seq, self.cfg.samplerate) {
            *seq += 1;
            self.counters.edges.fetch_add(n, Ordering::Relaxed);
            self.counters.dropped_edges.fetch_add(dropped, Ordering::Relaxed);
            let mut q = self.quality();
            if dropped > 0 {
                q.push("edges_dropped".into());
            }
            if self.em.data(self.draft(KIND_EDGES, p, q, Some(Dir::Rx))).is_err() {
                self.counters.emit_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn setup_decoder(&self, engine: &Path, args: &[String]) -> Result<DecSide, (ErrorCode, String)> {
        let mut child = spawn_group(engine, args, Stdio::piped()).map_err(|e| {
            (
                ErrorCode::NotFound,
                format!("spawn decoder {}: {e}", engine.display()),
            )
        })?;
        let ring = Arc::new(Ring::default());
        let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let mut proc = Proc {
            child,
            stderr: ring.clone(),
            threads: Vec::new(),
        };
        let fail = |proc: Proc, e: String| {
            proc.abort();
            (ErrorCode::Internal, e)
        };
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) else {
            return Err(fail(proc, "decoder pipes unavailable".into()));
        };
        match drain_stderr(stderr, ring) {
            Ok(t) => proc.threads.push(t),
            Err(e) => return Err(fail(proc, e)),
        }
        // Bounded reader → batcher queue; overflow is counted, never buffered without limit.
        let (atx, arx) = mpsc::sync_channel::<Annotation>(ANNOT_QUEUE);
        let counters = self.counters.clone();
        match thread("logic-dec-out", move || read_lines(stdout, &atx, &counters)) {
            Ok(t) => proc.threads.push(t),
            Err(e) => return Err(fail(proc, e)),
        }
        let em = self.em.clone();
        let base = self.draft(KIND_ANNOTATIONS, Value::Null, self.quality(), Some(Dir::Rx));
        let (id, rate, counters, cx) = (
            self.id.clone(),
            self.cfg.samplerate,
            self.counters.clone(),
            self.cx.clone(),
        );
        let batcher = match thread("logic-annot", move || {
            batch_annotations(&arx, &em, &base, &id, rate, &counters, &cx);
        }) {
            Ok(t) => t,
            Err(e) => return Err(fail(proc, e)),
        };
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let queued = Arc::new(AtomicUsize::new(0));
        let broken = Arc::new(AtomicBool::new(false));
        let (q, b, c) = (queued.clone(), broken.clone(), self.counters.clone());
        let writer = thread("logic-dec-in", move || {
            for chunk in rx {
                if stdin.write_all(&chunk).is_err() {
                    b.store(true, Ordering::Release);
                    break;
                }
                q.fetch_sub(chunk.len(), Ordering::AcqRel);
                c.decoder_input_bytes
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            drop(stdin); // EOF → the decoder drains and exits
        });
        let writer = match writer {
            Ok(w) => w,
            Err(e) => {
                let r = fail(proc, e);
                let _ = batcher.join();
                return Err(r);
            }
        };
        Ok(DecSide {
            proc,
            tee: Tee {
                tx: Some(tx),
                sent: 0,
                queued,
                broken,
                failed: None,
            },
            writer,
            batcher,
        })
    }

    fn setup_acquisition(&self, engine: &Path, args: &[String]) -> Result<AcqSide, (ErrorCode, String)> {
        // stdin stays a worker-owned pipe: sigrok-cli --continuous exits at once on a closed stdin
        // and stops cleanly on a newline (Sol probe).
        let mut child = spawn_group(engine, args, Stdio::piped()).map_err(|e| {
            (
                ErrorCode::NotFound,
                format!("spawn acquisition {}: {e}", engine.display()),
            )
        })?;
        let ring = Arc::new(Ring::default());
        let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let mut proc = Proc {
            child,
            stderr: ring.clone(),
            threads: Vec::new(),
        };
        let (Some(stdin), Some(mut stdout), Some(stderr)) = (stdin, stdout, stderr) else {
            proc.abort();
            return Err((ErrorCode::Internal, "acquisition pipes unavailable".into()));
        };
        match drain_stderr(stderr, ring) {
            Ok(t) => proc.threads.push(t),
            Err(e) => {
                proc.abort();
                return Err((ErrorCode::Internal, e));
            }
        }
        // Bounded: 64 × 64 KiB; a slow supervisor back-pressures the pipe instead of buffering.
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let reader = thread("logic-acq-out", move || {
            let mut buf = vec![0u8; READ_CHUNK];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        match reader {
            Ok(t) => proc.threads.push(t),
            Err(e) => {
                proc.abort();
                return Err((ErrorCode::Internal, e));
            }
        }
        Ok(AcqSide {
            proc,
            stdin: Some(stdin),
            rx,
        })
    }

    fn run(self, ready: tokio::sync::oneshot::Sender<Result<Value, WorkerError>>) {
        self.state("starting", None, Value::Null, "", None);
        let setup = (|| {
            let needs = self.cfg.source.acquires() || self.cfg.decoder.is_some();
            let engine = match (&self.engine, needs) {
                (Some(e), _) => e.clone(),
                (None, false) => PathBuf::new(),
                (None, true) => return Err((ErrorCode::NotFound, "sigrok-cli not found".to_owned())),
            };
            // Decoder first, so no raw byte exists before its consumer.
            let dec = match self.cfg.decoder_args() {
                Some(a) => Some(self.setup_decoder(&engine, &a)?),
                None => None,
            };
            let acq = match self.cfg.acquisition_args() {
                Some(a) => match self.setup_acquisition(&engine, &a) {
                    Ok(x) => Some(x),
                    Err(e) => {
                        if let Some(d) = dec {
                            d.abort();
                        }
                        return Err(e);
                    }
                },
                None => None,
            };
            Ok((dec, acq))
        })();
        match setup {
            Ok((dec, acq)) => {
                let _ = ready.send(Ok(json!({
                    "capture_id": self.id, "state": "starting", "source": self.cfg.source.as_str(),
                    "config": self.cfg.to_params(), "argv": self.argv(),
                    "engine": self.engine.as_ref().and_then(|p| p.to_str()),
                })));
                self.main(dec, acq);
            }
            Err((code, msg)) => {
                let p = self.state("failed", Some("error"), Value::Null, "", Some(&msg));
                let _ = ready.send(Err(err(code, msg).with_detail(json!({"capture_id": self.id}))));
                self.finish(p);
            }
        }
    }

    fn main(&self, mut dec: Option<DecSide>, mut acq: Option<AcqSide>) {
        let started = Instant::now();
        let duration = Duration::from_millis(self.cfg.duration_ms);
        let mut acc = EdgeAcc::new(self.cfg.mask);
        let mut edge_seq = 0u64;
        let mut last_flush = Instant::now();
        let mut running = false;
        let mut synth = if self.cfg.source == Source::Synthetic {
            Synth::new(&self.cfg).ok()
        } else {
            None
        };
        let mut produced = 0u64;
        let mut reason: &'static str;
        let mut error: Option<String> = None;
        let mut acq_ended = false;
        let mut sample_bound_hit = false;
        loop {
            if let Some(r) = self.ctl.requested() {
                reason = r;
                break;
            }
            if self.cx.latch().is_on() {
                reason = "stop_latch";
                break;
            }
            if started.elapsed() >= duration {
                reason = "duration";
                break;
            }
            if let Some(d) = dec.as_mut() {
                if let Some(f) = d.tee.failed.clone() {
                    reason = "error";
                    error = Some(f);
                    break;
                }
                if d.tee.broken.load(Ordering::Acquire) || matches!(d.proc.child.try_wait(), Ok(Some(_))) {
                    reason = "error";
                    error = Some("decoder exited during capture".into());
                    break;
                }
            }
            let data = match (&mut synth, &acq) {
                (Some(g), _) => {
                    std::thread::sleep(Duration::from_millis(10));
                    let due = u64::try_from(
                        started.elapsed().as_nanos() * u128::from(self.cfg.samplerate) / 1_000_000_000,
                    )
                    .unwrap_or(u64::MAX);
                    let n = due.saturating_sub(produced).min(self.cfg.samplerate / 10 + 1);
                    if n == 0 {
                        Data::Idle
                    } else {
                        let mut v = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
                        g.fill(&mut v, n);
                        produced += n;
                        Data::Chunk(v)
                    }
                }
                (None, Some(a)) => match a.rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(c) => Data::Chunk(c),
                    Err(RecvTimeoutError::Timeout) => Data::Idle,
                    Err(RecvTimeoutError::Disconnected) => Data::Ended,
                },
                (None, None) => Data::Ended,
            };
            match data {
                Data::Chunk(c) => {
                    sample_bound_hit = self.pump(c, &mut acc, dec.as_mut(), &mut running);
                    if sample_bound_hit {
                        reason = "samples";
                        break;
                    }
                }
                Data::Idle => {}
                Data::Ended => {
                    acq_ended = true;
                    reason = "eof";
                    break;
                }
            }
            if last_flush.elapsed() >= EDGE_FLUSH {
                self.flush_edges(&mut acc, &mut edge_seq);
                last_flush = Instant::now();
            }
        }

        // ---- stop / reap ----
        self.state("stopping", Some(reason), Value::Null, "", error.as_deref());
        let mut acq_exit = Value::Null;
        let mut acq_tail = String::new();
        if let Some(mut a) = acq.take() {
            let mut style = if acq_ended { "exited" } else { "key" };
            if !acq_ended && let Some(s) = a.stdin.as_mut() {
                let _ = s.write_all(b"\n").and_then(|()| s.flush());
            }
            // Drain to EOF; escalate group SIGINT → SIGKILL on deadlines. Drained samples keep the
            // sample axis contiguous; after the sample bound (or a tee failure) they are not used.
            let mut deadline = Instant::now() + STOP_KEY_GRACE;
            let mut escalated = 0;
            loop {
                match a.rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(c) if !sample_bound_hit => {
                        sample_bound_hit = self.pump(c, &mut acc, dec.as_mut(), &mut running);
                    }
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if Instant::now() >= deadline {
                    escalated += 1;
                    if escalated == 1 {
                        style = "sigint";
                        signal_group(a.proc.child.id(), "-INT");
                        deadline = Instant::now() + STOP_INT_GRACE;
                    } else {
                        style = "kill";
                        signal_group(a.proc.child.id(), "-KILL");
                        let _ = a.proc.child.kill();
                        if escalated > 2 {
                            break;
                        }
                        deadline = Instant::now() + STOP_INT_GRACE;
                    }
                }
            }
            drop(a.stdin.take());
            let (status, killed) = reap(&mut a.proc.child, Duration::from_millis(500));
            if killed {
                style = "kill";
            }
            drop(a.rx);
            for t in a.proc.threads.drain(..) {
                let _ = t.join();
            }
            acq_tail = a.proc.stderr.tail(STDERR_TAIL / 2);
            if acq_ended && error.is_none() && !status.is_some_and(|s| s.success()) {
                reason = "error";
                error = Some(format!("acquisition exited: {}", acq_tail.trim()));
            }
            acq_exit = exit_json(status, style);
        }
        self.flush_edges(&mut acc, &mut edge_seq);
        let mut dec_exit = Value::Null;
        let mut dec_tail = String::new();
        if let Some(mut d) = dec.take() {
            if error.is_none()
                && let Some(f) = d.tee.failed.clone()
            {
                reason = "error";
                error = Some(f);
            }
            drop(d.tee.tx.take()); // writer drains its queue, then closes stdin (EOF)
            let (status, killed) = reap(&mut d.proc.child, DECODER_DRAIN);
            let _ = d.writer.join();
            for t in d.proc.threads.drain(..) {
                let _ = t.join();
            }
            let _ = d.batcher.join();
            dec_tail = d.proc.stderr.tail(STDERR_TAIL / 2);
            // The writer may fail after the last feed (decoder closed stdin early, even with exit
            // 0): a clean stop requires every accepted byte to have been written.
            let written = self.counters.decoder_input_bytes.load(Ordering::Relaxed);
            if error.is_none() && (d.tee.broken.load(Ordering::Acquire) || written != d.tee.sent) {
                reason = "error";
                error = Some(format!(
                    "decoder consumed {written} of {} samples (input closed early)",
                    d.tee.sent
                ));
            }
            if error.is_none() && (killed || !status.is_some_and(|s| s.success())) {
                reason = "error";
                error = Some(format!("decoder did not exit cleanly: {}", dec_tail.trim()));
            }
            dec_exit = exit_json(status, if killed { "kill" } else { "eof" });
        }
        let tail = match (acq_tail.is_empty(), dec_tail.is_empty()) {
            (true, true) => String::new(),
            (false, true) => format!("acquisition: {acq_tail}"),
            (true, false) => format!("decoder: {dec_tail}"),
            (false, false) => format!("acquisition: {acq_tail}\ndecoder: {dec_tail}"),
        };
        let exit = json!({"acquisition": acq_exit, "decoder": dec_exit});
        let state = if error.is_some() { "failed" } else { "stopped" };
        let p = self.state(state, Some(reason), exit, &tail, error.as_deref());
        self.finish(p);
    }

    /// Feed one chunk (sample bound applied). Returns true when the bound was reached.
    fn pump(
        &self,
        mut chunk: Vec<u8>,
        acc: &mut EdgeAcc,
        dec: Option<&mut DecSide>,
        running: &mut bool,
    ) -> bool {
        let mut hit = false;
        if let Some(max) = self.cfg.max_samples {
            let have = self.counters.samples.load(Ordering::Relaxed);
            let room = usize::try_from(max.saturating_sub(have)).unwrap_or(usize::MAX);
            if chunk.len() >= room {
                chunk.truncate(room);
                hit = true;
            }
        }
        if chunk.is_empty() {
            return hit;
        }
        if !*running {
            *running = true;
            self.state("running", None, Value::Null, "", None);
        }
        acc.feed(&chunk);
        self.counters
            .samples
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
        if let Some(d) = dec {
            d.tee.feed(&chunk);
        }
        hit
    }
}

struct DecSide {
    proc: Proc,
    tee: Tee,
    writer: JoinHandle<()>,
    batcher: JoinHandle<()>,
}

impl DecSide {
    fn abort(mut self) {
        drop(self.tee.tx.take());
        self.proc.abort();
        let _ = self.writer.join();
        let _ = self.batcher.join();
    }
}

struct AcqSide {
    proc: Proc,
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<Vec<u8>>,
}

/// Reads decoder lines into the bounded queue. Lines dropped on a full queue are attributed to
/// the next delivered annotation (`lost_before`), so readers know exactly where the hole is.
fn read_lines(out: impl Read, tx: &mpsc::SyncSender<Annotation>, counters: &Counters) {
    let mut r = BufReader::new(out);
    let mut buf = Vec::with_capacity(256);
    let mut pending = 0u64;
    loop {
        buf.clear();
        let n = match r.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => {
                if pending > 0 {
                    // Trailing loss: a marker (blocking send; the batcher is draining).
                    let _ = tx.send(Annotation::loss_marker(pending));
                }
                return;
            }
            Ok(n) => n,
        };
        if buf.last() != Some(&b'\n') && n > MAX_LINE {
            counters.truncated_lines.fetch_add(1, Ordering::Relaxed);
            buf.truncate(MAX_LINE);
            let mut skip = Vec::new();
            loop {
                skip.clear();
                match r.by_ref().take(MAX_LINE as u64).read_until(b'\n', &mut skip) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if skip.last() == Some(&b'\n') => break,
                    Ok(_) => {}
                }
            }
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\n', '\r']);
        if line.is_empty() {
            continue;
        }
        let a = parse_annotation(line);
        if a.start.is_none() {
            counters.unparsed_lines.fetch_add(1, Ordering::Relaxed);
        }
        let mut a = a;
        a_lost(&mut a, pending);
        match tx.try_send(a) {
            Ok(()) => pending = 0,
            Err(mpsc::TrySendError::Full(a)) => {
                pending = a.lost_before + 1;
                counters.reader_dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return,
        }
    }
}

fn a_lost(a: &mut Annotation, n: u64) {
    a.lost_before = n;
}

/// Batch payload budget below [`model::MAX_PAYLOAD_BYTES`] (envelope fields + headroom).
const BATCH_BYTES: usize = model::MAX_PAYLOAD_BYTES - 1024;

/// Group annotations into `logic.annotations` events: ≤ 256 rows, ≤ ~47 KiB JSON, ≤ 50 ms.
fn batch_annotations(
    rx: &mpsc::Receiver<Annotation>,
    em: &Emitter,
    base: &EventDraft,
    id: &str,
    samplerate: u64,
    counters: &Counters,
    cx: &WorkerCtx,
) {
    let mut rate = RateGuard::new(ANNOT_RATE, ANNOT_BURST);
    let mut b = Batch::default();
    let mut last = Instant::now();
    let mut gap = 0u64;
    let emit = |b: &mut Batch| {
        let dropped = b.dropped;
        if b.rows.is_empty() && dropped == 0 {
            return;
        }
        counters
            .annotations
            .fetch_add(b.rows.len() as u64, Ordering::Relaxed);
        counters.dropped_annotations.fetch_add(dropped, Ordering::Relaxed);
        let mut d = base.clone();
        d.ts = cx.clock().ts(TsSource::HostReadComplete);
        if dropped > 0 {
            d.quality.push("annotations_dropped".into());
        }
        d.payload = batch_payload(id, samplerate, std::mem::take(&mut b.rows), dropped);
        if em.data(d).is_err() {
            counters.emit_errors.fetch_add(1, Ordering::Relaxed);
        }
        b.dropped = 0;
        b.bytes = 0;
    };
    loop {
        match rx.recv_timeout(ANNOT_FLUSH) {
            Ok(a) if a.is_loss_marker() => {
                b.dropped += a.lost_before;
                gap += a.lost_before;
            }
            Ok(a) => {
                b.dropped += a.lost_before;
                gap += a.lost_before;
                if rate.try_take(1) {
                    let mut row = a.to_row(samplerate);
                    if gap > 0 {
                        row["gap_before"] = json!(gap);
                        gap = 0;
                    }
                    let n = serde_json::to_string(&row).map_or(BATCH_BYTES, |s| s.len() + 1);
                    if b.bytes + n > BATCH_BYTES {
                        emit(&mut b);
                        last = Instant::now();
                    }
                    b.bytes += n;
                    b.rows.push(row);
                } else {
                    b.dropped += 1;
                    gap += 1;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                emit(&mut b);
                return;
            }
        }
        if b.rows.len() >= ANNOT_BATCH || last.elapsed() >= ANNOT_FLUSH {
            emit(&mut b);
            last = Instant::now();
        }
    }
}

#[derive(Debug, Default)]
struct Batch {
    rows: Vec<Value>,
    bytes: usize,
    dropped: u64,
}

/// `logic.annotations` payload.
pub fn batch_payload(id: &str, samplerate: u64, rows: Vec<Value>, dropped: u64) -> Value {
    json!({"capture_id": id, "samplerate": samplerate, "annotations": rows, "dropped": dropped})
}

/// Reader → batcher queue rows.
pub const ANNOT_QUEUE: usize = 1024;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn worst_case_annotation_row_fits_batch_budget() {
        // A maximal line of control bytes escapes to \u00XX (6×) — one row must still fit.
        let text = "\u{1}".repeat(MAX_LINE);
        let a = Annotation {
            decoder: Some("x".repeat(64)),
            start: Some(u64::MAX),
            end: Some(u64::MAX),
            text,
            lost_before: u64::MAX,
        };
        let mut row = a.to_row(1);
        row["gap_before"] = json!(u64::MAX);
        let n = serde_json::to_string(&row).unwrap().len();
        assert!(n < BATCH_BYTES, "{n}");
        let p = batch_payload(&"i".repeat(64), u64::MAX, vec![row], u64::MAX);
        assert!(serde_json::to_string(&p).unwrap().len() < model::MAX_PAYLOAD_BYTES);
    }

    #[test]
    fn list_sections() {
        let t = "Supported hardware drivers:\n  demo   Demo\n  fx2lafw  fx2\n\nSupported protocol decoders:\n  spi  SPI\n  uart UART\n";
        assert_eq!(list_section(t, "Supported hardware drivers"), ["demo", "fx2lafw"]);
        assert_eq!(list_section(t, "Supported protocol decoders"), ["spi", "uart"]);
    }
}
