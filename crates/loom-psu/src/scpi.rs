//! External supply over SCPI on loopback TCP (`backend: "scpi-tcp"`).
//!
//! Scope (CONTRACT.md): a narrow generic SCPI line profile — `*IDN?`, `VOLT`/`VOLT?`,
//! `CURR`/`CURR?`, `OUTP`/`OUTP?`, `MEAS:VOLT?`, `MEAS:CURR?`, `SYST:ERR?` — to a **loopback**
//! software endpoint. Not VISA, USB-TMC, GPIB or LAN instruments; no vendor compatibility is
//! claimed. Reset, cycle, lists and protection handling are refused by the worker, never
//! simulated.
//!
//! One dedicated thread owns the socket. Callbacks hand it work over a bounded queue and await a
//! thread-signalled reply; STOP (`LatchFlag`) is read by the thread before every agent/script
//! unit and never waits on device IO. Any read timeout, EOF, overlong or unsolicited reply makes
//! the link `lost` (a line protocol cannot be resynchronised safely): no automatic retry and no
//! automatic reconnect. Connect only queries; nothing on connect/STOP/disconnect switches the
//! output (HOLD).

use std::collections::VecDeque;
use std::future::Future;
use std::io::{ErrorKind, Read, Write as _};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, GapCount, Metric, Origin, Ts, TsSource, WriteState};
use loom_proto::num::U64;
use loom_proto::worker::WorkerError;
use loom_worker_sdk::{Emitter, EventDraft, LatchFlag, WriteOutcome};
use serde_json::{Value, json};

use crate::model::{Limits, POLL_HZ, POLL_PERIOD_NS};
use crate::{GAP_NO_RESPONSE, KIND_MEAS, KIND_STATE, KIND_WRITE};

/// Backend name.
pub const BACKEND: &str = "scpi-tcp";
/// TCP connect deadline.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Deadline of one line write and of one reply line.
pub const IO_TIMEOUT: Duration = Duration::from_millis(500);
/// Longest reply line (bytes, without the terminator).
pub const MAX_REPLY: usize = 256;
/// Queued commands per supply (more: `E_WORKER_BUSY`, nothing sent).
pub const QUEUE: usize = 4;
/// Longest a write callback waits for its outcome.
pub const WRITE_BUDGET: Duration = Duration::from_secs(3);
/// Longest a connect callback waits (connect + 4 queries).
pub const CONNECT_BUDGET: Duration = Duration::from_secs(5);
/// Extra wait for the IO thread's verdict after a request deadline.
pub const REPLY_GRACE: Duration = Duration::from_millis(250);
/// A last sample older than this is stale for `psu.meas`.
pub const STALE_MS: u64 = 1000;
/// Gap cause: link closed/reset/garbled by the far end (count unknown).
pub const GAP_LINK_LOST: &str = "scpi_link_lost";
const RATE_WINDOW: usize = 16;

fn err(code: ErrorCode, message: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

/// Parses a loopback `ip:port` (127.0.0.0/8 or ::1, port ≠ 0).
pub fn parse_loopback(addr: &str) -> Result<SocketAddr, String> {
    let a: SocketAddr = addr.parse().map_err(|_| {
        format!("scpi-tcp address {addr:?}: expected <loopback-ip>:<port>, e.g. 127.0.0.1:5025")
    })?;
    let lo = match a.ip() {
        IpAddr::V4(v) => v.is_loopback(),
        IpAddr::V6(v) => v.is_loopback(),
    };
    if !lo {
        return Err(format!(
            "scpi-tcp address {addr:?}: loopback only in this increment (LAN/USB/GPIB/VISA instruments are not supported)"
        ));
    }
    if a.port() == 0 {
        return Err("scpi-tcp address: port 0".into());
    }
    Ok(a)
}

// ---------------------------------------------------------------------------------------------
// Thread-signalled reply (no async runtime dependency).

struct Slot<T> {
    val: Option<T>,
    waker: Option<Waker>,
    expired: bool,
}

/// Sending half of a reply.
pub struct ReplyTx<T>(Arc<Mutex<Slot<T>>>);
/// Receiving half: resolves to `Some(value)` or `None` when the deadline passes first.
pub struct ReplyRx<T> {
    slot: Arc<Mutex<Slot<T>>>,
    until: Instant,
    timer: bool,
}

impl<T> std::fmt::Debug for ReplyTx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplyTx")
    }
}

impl<T> std::fmt::Debug for ReplyRx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplyRx")
            .field("until", &self.until)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A reply pair whose receiver gives up at `until`.
pub fn reply<T: Send + 'static>(until: Instant) -> (ReplyTx<T>, ReplyRx<T>) {
    let slot = Arc::new(Mutex::new(Slot {
        val: None,
        waker: None,
        expired: false,
    }));
    (
        ReplyTx(Arc::clone(&slot)),
        ReplyRx {
            slot,
            until,
            timer: false,
        },
    )
}

impl<T> ReplyTx<T> {
    /// Delivers the value (ignored if the receiver already gave up).
    pub fn send(self, v: T) {
        let w = {
            let mut s = lock(&self.0);
            s.val = Some(v);
            s.waker.take()
        };
        if let Some(w) = w {
            w.wake();
        }
    }
}

impl<T: Send + 'static> Future for ReplyRx<T> {
    type Output = Option<T>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        {
            let mut s = lock(&self.slot);
            if let Some(v) = s.val.take() {
                return Poll::Ready(Some(v));
            }
            if s.expired || Instant::now() >= self.until {
                s.expired = true;
                return Poll::Ready(None);
            }
            s.waker = Some(cx.waker().clone());
        }
        if !self.timer {
            self.timer = true;
            let slot = Arc::clone(&self.slot);
            let until = self.until;
            let _ = std::thread::Builder::new()
                .name("loom-psu-reply-timer".into())
                .spawn(move || {
                    // Wakes early once the value is there (checked every 50 ms).
                    while Instant::now() < until {
                        if lock(&slot).val.is_some() {
                            return;
                        }
                        std::thread::sleep(
                            Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())),
                        );
                    }
                    let w = {
                        let mut s = lock(&slot);
                        s.expired = true;
                        s.waker.take()
                    };
                    if let Some(w) = w {
                        w.wake();
                    }
                });
        }
        Poll::Pending
    }
}

// ---------------------------------------------------------------------------------------------
// Line link.

#[derive(Debug, Clone, PartialEq)]
enum IoFail {
    /// Nothing left the host.
    NotSent(String),
    /// Some bytes (`n`) left the host, not all.
    Partial(usize, String),
    /// The command left the host; the reply did not come in time.
    Timeout,
    /// EOF / reset / overlong / unsolicited bytes / not ASCII.
    Broken(String),
}

impl IoFail {
    fn text(&self) -> String {
        match self {
            IoFail::NotSent(e) => format!("not sent: {e}"),
            IoFail::Partial(n, e) => format!("partially sent ({n} bytes): {e}"),
            IoFail::Timeout => format!(
                "no reply within {} ms (or the request deadline)",
                IO_TIMEOUT.as_millis()
            ),
            IoFail::Broken(e) => e.clone(),
        }
    }
}

struct Link {
    s: TcpStream,
    buf: Vec<u8>,
    /// Request deadline of the running write transaction (each line also has [`IO_TIMEOUT`]).
    cap: Option<Instant>,
}

impl Link {
    fn line_deadline(&self) -> Instant {
        let d = Instant::now() + IO_TIMEOUT;
        self.cap.map_or(d, |c| c.min(d))
    }

    fn send(&mut self, line: &str) -> Result<(), IoFail> {
        if !self.buf.is_empty() {
            return Err(IoFail::Broken(format!(
                "unsolicited {} bytes from the supply",
                self.buf.len()
            )));
        }
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        let mut done = 0;
        let until = self.line_deadline();
        while done < bytes.len() {
            let left = until.saturating_duration_since(Instant::now());
            let fail = |done: usize, e: String| {
                if done == 0 {
                    IoFail::NotSent(e)
                } else {
                    IoFail::Partial(done, e)
                }
            };
            if left.is_zero() {
                return Err(fail(done, "write deadline".into()));
            }
            let _ = self.s.set_write_timeout(Some(left));
            match self.s.write(&bytes[done..]) {
                Ok(0) => return Err(fail(done, "socket closed".into())),
                Ok(n) => done += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(fail(done, e.to_string())),
            }
        }
        Ok(())
    }

    fn read_line(&mut self) -> Result<String, IoFail> {
        let until = self.line_deadline();
        loop {
            if let Some(p) = self.buf.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=p).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.len() > MAX_REPLY {
                    return Err(IoFail::Broken(format!("reply over {MAX_REPLY} bytes")));
                }
                return String::from_utf8(line)
                    .ok()
                    .filter(|s| s.is_ascii())
                    .ok_or_else(|| IoFail::Broken("reply is not ASCII".into()));
            }
            if self.buf.len() > MAX_REPLY {
                return Err(IoFail::Broken(format!("reply over {MAX_REPLY} bytes")));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(IoFail::Timeout);
            }
            let _ = self.s.set_read_timeout(Some(left));
            let mut chunk = [0u8; 512];
            match self.s.read(&mut chunk) {
                Ok(0) => return Err(IoFail::Broken("supply closed the connection".into())),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Err(IoFail::Timeout);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(IoFail::Broken(e.to_string())),
            }
        }
    }

    fn query(&mut self, q: &str) -> Result<String, IoFail> {
        self.send(q)?;
        self.read_line()
    }
}

fn parse_f(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_uppercase().as_str() {
        "1" | "ON" => Some(true),
        "0" | "OFF" => Some(false),
        _ => None,
    }
}

/// `<code>,"<msg>"` → (code, msg).
fn parse_err(s: &str) -> Option<(i64, String)> {
    let (c, m) = s.split_once(',')?;
    Some((c.trim().parse().ok()?, m.trim().trim_matches('"').to_owned()))
}

/// Half a unit of the reply's last printed digit (read-back resolution).
fn resolution(reply: &str) -> f64 {
    let t = reply.trim();
    let mant = t.split(['e', 'E']).next().unwrap_or(t);
    let d = mant.split_once('.').map_or(0, |(_, f)| f.len()) as i32;
    0.5 * 10f64.powi(-d)
}

// ---------------------------------------------------------------------------------------------
// Shared state.

/// Link state (`psu.state.link`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Connected and answering.
    Up,
    /// Broken by the far end or a timeout; manual `psu.connect` opens a new generation.
    Lost,
    /// Closed by `psu.disconnect`.
    Closed,
}

impl LinkState {
    /// Name.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkState::Up => "up",
            LinkState::Lost => "lost",
            LinkState::Closed => "closed",
        }
    }
}

/// Last received sample.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Volts.
    pub v: f64,
    /// Amperes.
    pub i: f64,
    /// Output read in the same poll.
    pub out: bool,
    /// Host mono ns of the last reply line.
    pub mono_ns: u64,
    /// Host wall ns of the last reply line.
    pub wall_ns: u64,
    /// Poll counter.
    pub poll_n: u64,
    /// First query sent → last reply received.
    pub age_ms: u64,
}

#[derive(Debug)]
struct Shared {
    link: LinkState,
    lost_why: Option<String>,
    idn: String,
    set_v: Option<f64>,
    set_i: Option<f64>,
    out: Option<bool>,
    last: Option<Sample>,
    samples: u64,
    timeouts: u64,
    polls: u64,
    recent: VecDeque<u64>,
    last_state: Value,
    queued: usize,
}

/// One connected external supply (worker side handle).
#[derive(Debug)]
pub struct ScpiDevice {
    /// Channel.
    pub ch: String,
    /// Endpoint.
    pub address: SocketAddr,
    /// Generation of this connection.
    pub generation: u32,
    limits: Limits,
    tx: SyncSender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    closing: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    sock: TcpStream,
}

/// What a write asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    /// `VOLT`/`CURR`.
    Set(Option<f64>, Option<f64>),
    /// `OUTP ON|OFF`.
    Out(bool),
}

#[derive(Debug, Clone)]
struct Cause {
    origin: Origin,
    request_id: Option<String>,
    client_id: Option<String>,
}

enum Cmd {
    Write(Act, Cause, Instant, ReplyTx<WriteOutcome>),
}

impl std::fmt::Debug for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cmd")
    }
}

struct Io {
    ch: String,
    address: SocketAddr,
    generation: u32,
    limits: Limits,
    link: Link,
    em: Emitter,
    latch: LatchFlag,
    shared: Arc<Mutex<Shared>>,
    closing: Arc<AtomicBool>,
    rx: Receiver<Cmd>,
}

fn ts_now(em: &Emitter) -> Ts {
    em.clock().ts(TsSource::HostReadComplete)
}

impl Io {
    fn sh(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    fn state_payload(&self, s: &Shared) -> Value {
        let mode = match s.out {
            Some(false) => "off",
            _ => "unknown",
        };
        json!({
            "ch": self.ch, "backend": BACKEND, "address": self.address.to_string(), "idn": s.idn,
            "set_v": s.set_v, "set_i": s.set_i, "out": s.out.unwrap_or(false), "out_known": s.out.is_some(),
            "prot": "unknown", "mode": mode, "vmax": self.limits.vmax, "imax": self.limits.imax,
            "connected": s.link == LinkState::Up, "link": s.link.as_str(), "lost_why": s.lost_why,
            "cycling": false, "generation": self.generation,
        })
    }

    fn state_if_changed(&self, cause: &Cause) {
        let p = {
            let mut s = self.sh();
            let p = self.state_payload(&s);
            if p == s.last_state {
                return;
            }
            s.last_state = p.clone();
            p
        };
        let _ = self.em.priority(EventDraft {
            kind: KIND_STATE.into(),
            ts: ts_now(&self.em),
            dir: None,
            origin: cause.origin,
            client_id: cause.client_id.clone(),
            request_id: None,
            job_id: None,
            generation: Some(self.generation),
            purpose: None,
            risk: None,
            env: None,
            payload: p,
            metrics: vec![],
            quality: vec![],
        });
    }

    fn row(&self, cause: &Cause, st: WriteState, extra: Value) {
        let mut payload = json!({"ch": self.ch, "write_state": st, "backend": BACKEND});
        if let (Some(p), Value::Object(e)) = (payload.as_object_mut(), extra) {
            p.extend(e);
        }
        let _ = self.em.priority(EventDraft {
            kind: KIND_WRITE.into(),
            ts: ts_now(&self.em),
            dir: Some(Dir::Tx),
            origin: cause.origin,
            client_id: cause.client_id.clone(),
            request_id: cause.request_id.clone(),
            job_id: None,
            generation: Some(self.generation),
            purpose: None,
            risk: None,
            env: None,
            payload,
            metrics: vec![],
            quality: vec![],
        });
    }

    /// Marks the link lost (once), emits the gap and the state change.
    fn lose(&self, cause_gap: &str, count: GapCount, why: String) {
        {
            let mut s = self.sh();
            if s.link != LinkState::Up {
                return;
            }
            s.link = LinkState::Lost;
            s.lost_why = Some(why.clone());
        }
        let _ = self.link.s.shutdown(Shutdown::Both);
        let _ = self.em.source_gap_with(
            cause_gap,
            count,
            None,
            json!({"backend": BACKEND, "why": why, "auto_reconnect": false}),
        );
        self.state_if_changed(&Cause {
            origin: Origin::Device,
            request_id: None,
            client_id: None,
        });
    }

    fn fail_io(&self, e: &IoFail) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        match e {
            IoFail::Timeout => {
                self.sh().timeouts += 1;
                self.lose(GAP_NO_RESPONSE, GapCount::Exact(U64(1)), e.text());
            }
            _ => self.lose(GAP_LINK_LOST, GapCount::Unknown, e.text()),
        }
    }

    fn poll(&mut self) {
        let t0 = Instant::now();
        let n = {
            let mut s = self.sh();
            s.polls += 1;
            s.polls
        };
        let r = (|| {
            let v = self.link.query("MEAS:VOLT?")?;
            let i = self.link.query("MEAS:CURR?")?;
            let o = self.link.query("OUTP?")?;
            Ok::<_, IoFail>((v, i, o))
        })();
        let (v, i, o) = match r {
            Ok(x) => x,
            Err(e) => return self.fail_io(&e),
        };
        let ts = ts_now(&self.em);
        let (Some(v), Some(i), Some(out)) = (parse_f(&v), parse_f(&i), parse_bool(&o)) else {
            return self.lose(
                GAP_LINK_LOST,
                GapCount::Unknown,
                format!("unparsable poll reply {v:?} {i:?} {o:?}"),
            );
        };
        let age_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
        {
            let mut s = self.sh();
            s.samples += 1;
            s.out = Some(out);
            if s.recent.len() == RATE_WINDOW {
                s.recent.pop_front();
            }
            s.recent.push_back(ts.mono_ns.get());
            s.last = Some(Sample {
                v,
                i,
                out,
                mono_ns: ts.mono_ns.get(),
                wall_ns: ts.wall_ns.get(),
                poll_n: n,
                age_ms,
            });
        }
        let metric = |name: &str, value: f64, unit: &str| Metric {
            name: name.into(),
            value,
            unit: Some(unit.into()),
        };
        let _ = self.em.priority(EventDraft {
            kind: KIND_MEAS.into(),
            ts,
            dir: Some(Dir::Rx),
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            job_id: None,
            generation: Some(self.generation),
            purpose: Some("poll".into()),
            risk: None,
            env: None,
            payload: json!({"ch": self.ch, "age_ms": age_ms, "backend": BACKEND, "poll_n": U64(n), "out": out}),
            metrics: vec![
                metric("voltage", v, "V"),
                metric("current", i, "A"),
                metric("power", v * i, "W"),
            ],
            quality: vec![],
        });
        // A front-panel output change shows up here as a device-origin state change.
        self.state_if_changed(&Cause {
            origin: Origin::Device,
            request_id: None,
            client_id: None,
        });
    }

    fn write(&mut self, act: Act, cause: &Cause, until: Instant) -> WriteOutcome {
        let action = match act {
            Act::Set(..) => "set",
            Act::Out(_) => "out",
        };
        let failed = |io: &Io, code: ErrorCode, msg: String, extra: Value| {
            let mut p = json!({"action": action, "error": {"code": code, "message": msg}});
            if let (Some(o), Value::Object(e)) = (p.as_object_mut(), extra) {
                o.extend(e);
            }
            io.row(cause, WriteState::Failed, p);
            WriteOutcome {
                state: WriteState::Failed,
                detail: Value::Null,
                error: Some(err(code, msg)),
            }
        };
        // STOP latches agent/script units; the user (incl. emergency OFF) is never latched (A3).
        if self.latch.is_on() && cause.origin != Origin::User {
            return failed(
                self,
                ErrorCode::Stopped,
                "STOP latched".into(),
                json!({"sent": false}),
            );
        }
        if self.sh().link != LinkState::Up {
            return failed(
                self,
                ErrorCode::Disconnected,
                "link not up; reconnect with psu connect".into(),
                json!({"sent": false}),
            );
        }
        let cmds: Vec<(String, &str, Value)> = match act {
            Act::Set(v, i) => {
                if let Some(why) = self.limits.violation(v, i) {
                    return failed(self, ErrorCode::Denied, why, json!({"sent": false}));
                }
                let mut c = Vec::new();
                if let Some(v) = v {
                    c.push((format!("VOLT {v}"), "VOLT?", json!(v)));
                }
                if let Some(i) = i {
                    c.push((format!("CURR {i}"), "CURR?", json!(i)));
                }
                c
            }
            Act::Out(on) => vec![(
                format!("OUTP {}", if on { "ON" } else { "OFF" }),
                "OUTP?",
                json!(on),
            )],
        };
        let sent_payload = match act {
            Act::Set(v, i) => json!({"action": "set", "volt": v, "curr": i}),
            Act::Out(on) => json!({"action": "out", "state": if on { "on" } else { "off" }}),
        };
        let mut readback = serde_json::Map::new();
        let mut lines = Vec::new();
        let mut effective = true;
        for (k, (cmd, q, want)) in cmds.iter().enumerate() {
            // The caller's request deadline bounds the whole transaction, unit by unit.
            if Instant::now() >= until {
                let msg = if k == 0 {
                    "request deadline passed before the command was sent"
                } else {
                    "request deadline passed mid-transaction; remaining units not sent"
                };
                return failed(
                    self,
                    ErrorCode::Timeout,
                    msg.into(),
                    json!({"sent": k > 0, "applied": lines}),
                );
            }
            // Re-check STOP between units of a multi-command write (agent/script only).
            if k > 0 && self.latch.is_on() && cause.origin != Origin::User {
                self.row(
                    cause,
                    WriteState::Failed,
                    json!({"action": action, "error": {"code": ErrorCode::Stopped, "message": "STOP latched mid-write"},
                           "applied": lines}),
                );
                return WriteOutcome {
                    state: WriteState::Failed,
                    detail: json!({"applied": lines}),
                    error: Some(err(ErrorCode::Stopped, "STOP latched mid-write")),
                };
            }
            if let Err(e) = self.link.send(cmd) {
                let first_not_sent = k == 0 && matches!(e, IoFail::NotSent(_));
                self.fail_io(&e);
                if first_not_sent {
                    return failed(self, ErrorCode::Disconnected, e.text(), json!({"sent": false}));
                }
                return self.unknown(cause, action, &e, &lines);
            }
            if k == 0 {
                self.row(cause, WriteState::Sent, sent_payload.clone());
            }
            lines.push(cmd.clone());
            let e = match self.link.query("SYST:ERR?") {
                Ok(r) => r,
                Err(e) => {
                    self.fail_io(&e);
                    return self.unknown(cause, action, &e, &lines);
                }
            };
            match parse_err(&e) {
                Some((0, _)) => {}
                Some((code, msg)) => {
                    self.state_after_readback();
                    return failed(
                        self,
                        ErrorCode::Denied,
                        format!("supply refused {cmd:?}: {code},{msg}"),
                        json!({"device_error": {"code": code, "message": msg}, "applied": &lines[..lines.len() - 1]}),
                    );
                }
                None => {
                    let f = IoFail::Broken(format!("unparsable SYST:ERR? reply {e:?}"));
                    self.fail_io(&f);
                    return self.unknown(cause, action, &f, &lines);
                }
            }
            let r = match self.link.query(q) {
                Ok(r) => r,
                Err(e) => {
                    self.fail_io(&e);
                    return self.unknown(cause, action, &e, &lines);
                }
            };
            let ok = match *q {
                "OUTP?" => {
                    let b = parse_bool(&r);
                    if let Some(b) = b {
                        self.sh().out = Some(b);
                    }
                    readback.insert("out".into(), json!(b));
                    b == want.as_bool()
                }
                _ => {
                    let x = parse_f(&r);
                    let key = if *q == "VOLT?" { "set_v" } else { "set_i" };
                    {
                        let mut s = self.sh();
                        if *q == "VOLT?" {
                            s.set_v = x;
                        } else {
                            s.set_i = x;
                        }
                    }
                    readback.insert(key.into(), json!(x));
                    let w = want.as_f64().unwrap_or(f64::NAN);
                    x.is_some_and(|x| (x - w).abs() <= resolution(&r) + 1e-9 * w.abs().max(1.0))
                }
            };
            effective &= ok;
        }
        let st = if effective {
            WriteState::EffectiveVerified
        } else {
            WriteState::EffectUnverified
        };
        let rb = Value::Object(readback);
        self.row(
            cause,
            st,
            json!({"action": action, "readback": rb, "lines": lines}),
        );
        self.state_if_changed(cause);
        WriteOutcome {
            state: st,
            detail: json!({"readback": rb}),
            error: None,
        }
    }

    /// Best-effort refresh of set values after a device error (no-op if it fails: link lost).
    fn state_after_readback(&mut self) {
        for q in ["VOLT?", "CURR?"] {
            match self.link.query(q) {
                Ok(r) => {
                    let mut s = self.sh();
                    if q == "VOLT?" {
                        s.set_v = parse_f(&r);
                    } else {
                        s.set_i = parse_f(&r);
                    }
                }
                Err(e) => return self.fail_io(&e),
            }
        }
    }

    fn unknown(&self, cause: &Cause, action: &str, e: &IoFail, lines: &[String]) -> WriteOutcome {
        self.row(
            cause,
            WriteState::OutcomeUnknown,
            json!({"action": action, "error": e.text(), "applied_or_partial": lines, "retry": false}),
        );
        WriteOutcome {
            state: WriteState::OutcomeUnknown,
            detail: json!({"lines": lines, "retry": false}),
            error: Some(err(
                ErrorCode::NoResponse,
                format!(
                    "{}; link lost, not retried — check the supply, then reconnect",
                    e.text()
                ),
            )),
        }
    }

    fn run(mut self) {
        let period = Duration::from_nanos(POLL_PERIOD_NS);
        let mut next = Instant::now() + period;
        loop {
            if self.closing.load(Ordering::Acquire) {
                break;
            }
            let up = self.sh().link == LinkState::Up;
            let wait = if up {
                next.saturating_duration_since(Instant::now())
            } else {
                Duration::from_millis(200)
            };
            match self.rx.recv_timeout(wait) {
                Ok(Cmd::Write(act, cause, until, tx)) => {
                    {
                        let mut s = self.sh();
                        s.queued = s.queued.saturating_sub(1);
                    }
                    self.link.cap = Some(until);
                    let o = self.write(act, &cause, until);
                    self.link.cap = None;
                    tx.send(o);
                }
                Err(RecvTimeoutError::Timeout) if up => {
                    self.poll();
                    next += period;
                    let now = Instant::now();
                    if next <= now {
                        // Behind (slow answers): one outstanding query, the actual rate drops.
                        next = now + period;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // Commands still queued get a truthful answer; nothing is sent.
        while let Ok(Cmd::Write(_, cause, _, tx)) = self.rx.try_recv() {
            let o = WriteOutcome {
                state: WriteState::Failed,
                detail: json!({"sent": false}),
                error: Some(err(
                    ErrorCode::Disconnected,
                    "supply disconnected before the write",
                )),
            };
            self.row(
                &cause,
                WriteState::Failed,
                json!({"error": "disconnected before send", "sent": false}),
            );
            tx.send(o);
        }
        let _ = self.link.s.shutdown(Shutdown::Both);
    }
}

/// Result of a connect attempt.
pub type ConnectResult = Result<ScpiDevice, WorkerError>;

/// Connects (queries only: `*IDN?`, `VOLT?`, `CURR?`, `OUTP?`) on a fresh dedicated thread that
/// then owns the socket. Resolves with the device or the error.
pub fn connect(
    ch: String,
    address: SocketAddr,
    generation: u32,
    limits: Limits,
    em: Emitter,
    latch: LatchFlag,
    until: Instant,
) -> ReplyRx<ConnectResult> {
    let (tx, rx) = reply::<ConnectResult>(until);
    let name = format!("loom-psu-scpi-{ch}");
    let spawn = std::thread::Builder::new().name(name).spawn(move || {
        let s = match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(s) => s,
            Err(e) => {
                return tx.send(Err(err(
                    ErrorCode::Disconnected,
                    format!("scpi-tcp connect {address}: {e}"),
                )));
            }
        };
        let _ = s.set_nodelay(true);
        let (sock, sock2) = match (s.try_clone(), s.try_clone()) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return tx.send(Err(err(ErrorCode::Internal, "socket clone failed"))),
        };
        let mut link = Link {
            s,
            buf: Vec::new(),
            cap: None,
        };
        let q = |link: &mut Link, q: &str| {
            link.query(q).map_err(|e| {
                let _ = link.s.shutdown(Shutdown::Both);
                let code = if e == IoFail::Timeout {
                    ErrorCode::NoResponse
                } else {
                    ErrorCode::Disconnected
                };
                err(code, format!("scpi-tcp {address} {q}: {}", e.text()))
            })
        };
        let idn = match q(&mut link, "*IDN?") {
            Ok(x) => x,
            Err(e) => return tx.send(Err(e)),
        };
        let mut rb = Vec::new();
        for c in ["VOLT?", "CURR?", "OUTP?"] {
            match q(&mut link, c) {
                Ok(x) => rb.push(x),
                Err(e) => return tx.send(Err(e)),
            }
        }
        let shared = Arc::new(Mutex::new(Shared {
            link: LinkState::Up,
            lost_why: None,
            idn,
            set_v: parse_f(&rb[0]),
            set_i: parse_f(&rb[1]),
            out: parse_bool(&rb[2]),
            last: None,
            samples: 0,
            timeouts: 0,
            polls: 0,
            recent: VecDeque::with_capacity(RATE_WINDOW),
            last_state: Value::Null,
            queued: 0,
        }));
        let closing = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let (ctx, crx) = sync_channel::<Cmd>(QUEUE);
        let io = Io {
            ch: ch.clone(),
            address,
            generation,
            limits,
            link,
            em: em.with_generation(Some(generation), None),
            latch,
            shared: Arc::clone(&shared),
            closing: Arc::clone(&closing),
            rx: crx,
        };
        io.state_if_changed(&Cause {
            origin: Origin::Device,
            request_id: None,
            client_id: None,
        });
        let dev = ScpiDevice {
            ch,
            address,
            generation,
            limits,
            tx: ctx,
            shared,
            closing,
            finished: Arc::clone(&finished),
            sock: sock2,
        };
        drop(sock);
        tx.send(Ok(dev));
        io.run();
        finished.store(true, Ordering::Release);
    });
    if let Err(e) = spawn {
        // The receiver resolves at its deadline; report the spawn error immediately instead.
        let (tx2, rx2) = reply::<ConnectResult>(until);
        tx2.send(Err(err(ErrorCode::Internal, format!("IO thread: {e}"))));
        return rx2;
    }
    rx
}

impl ScpiDevice {
    fn sh(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }

    /// Link state.
    pub fn link(&self) -> LinkState {
        self.sh().link
    }

    /// Queues a write; the outcome arrives when the IO thread finished it (rows emitted first).
    pub fn write(
        &self,
        act: Act,
        origin: Origin,
        request_id: String,
        client_id: Option<String>,
        until: Instant,
    ) -> Result<ReplyRx<WriteOutcome>, WorkerError> {
        // The IO thread enforces `until` itself; the grace lets its verdict arrive.
        let (tx, rx) = reply(until + REPLY_GRACE);
        let cause = Cause {
            origin,
            request_id: Some(request_id),
            client_id,
        };
        match self.tx.try_send(Cmd::Write(act, cause, until, tx)) {
            Ok(()) => {
                self.sh().queued += 1;
                Ok(rx)
            }
            Err(TrySendError::Full(_)) => Err(err(
                ErrorCode::WorkerBusy,
                format!("{QUEUE} commands already queued for this supply; nothing sent"),
            )),
            Err(TrySendError::Disconnected(_)) => {
                Err(err(ErrorCode::Disconnected, "IO thread ended; reconnect"))
            }
        }
    }

    /// Closes the link without touching the output (HOLD). Returns at once; the thread ends
    /// within one IO deadline.
    pub fn close(&self) {
        self.closing.store(true, Ordering::Release);
        {
            let mut s = self.sh();
            if s.link == LinkState::Up {
                s.link = LinkState::Closed;
            }
        }
        let _ = self.sock.shutdown(Shutdown::Both);
    }

    /// IO thread has returned (socket shut down, queue answered).
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Closes (HOLD) and resolves `true` once the IO thread returned, `false` if it is still
    /// running at `until` (reported, never treated as a clean close).
    pub fn close_and_join(&self, until: Instant) -> ReplyRx<bool> {
        self.close();
        let (tx, rx) = reply::<bool>(until);
        let fin = Arc::clone(&self.finished);
        let spawned = std::thread::Builder::new()
            .name("loom-psu-scpi-join".into())
            .spawn(move || {
                while Instant::now() < until {
                    if fin.load(Ordering::Acquire) {
                        return tx.send(true);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                tx.send(fin.load(Ordering::Acquire));
            });
        if spawned.is_err() {
            let (tx2, rx2) = reply::<bool>(until);
            tx2.send(self.finished());
            return rx2;
        }
        rx
    }

    /// `psu.state`-shaped payload for the worker to emit on close.
    pub fn state_payload(&self) -> Value {
        let s = self.sh();
        let mode = match s.out {
            Some(false) => "off",
            _ => "unknown",
        };
        json!({
            "ch": self.ch, "backend": BACKEND, "address": self.address.to_string(), "idn": s.idn,
            "set_v": s.set_v, "set_i": s.set_i, "out": s.out.unwrap_or(false), "out_known": s.out.is_some(),
            "prot": "unknown", "mode": mode, "vmax": self.limits.vmax, "imax": self.limits.imax,
            "connected": s.link == LinkState::Up, "link": s.link.as_str(), "lost_why": s.lost_why,
            "cycling": false, "generation": self.generation,
        })
    }

    /// Non-blocking snapshot (`psu.status`).
    pub fn status(&self, now_mono_ns: u64) -> Value {
        let mut st = self.state_payload();
        let s = self.sh();
        let rate = match (s.recent.front(), s.recent.back()) {
            (Some(a), Some(b)) if b > a => Some((s.recent.len() - 1) as f64 / ((b - a) as f64 / 1e9)),
            _ => None,
        };
        if let Some(o) = st.as_object_mut() {
            o.insert("poll_hz_req".into(), json!(POLL_HZ));
            o.insert("poll_hz_actual".into(), json!(rate));
            o.insert("samples".into(), json!(U64(s.samples)));
            o.insert("timeouts".into(), json!(U64(s.timeouts)));
            o.insert("polls".into(), json!(U64(s.polls)));
            o.insert("queued".into(), json!(s.queued));
            o.insert(
                "last".into(),
                s.last.map_or(Value::Null, |l| {
                    json!({"v": l.v, "i": l.i, "out": l.out, "poll_n": U64(l.poll_n), "host_wall_ns": U64(l.wall_ns),
                           "host_mono_ns": U64(l.mono_ns), "age_ms": l.age_ms,
                           "since_ms": now_mono_ns.saturating_sub(l.mono_ns) / 1_000_000})
                }),
            );
            o.insert(
                "limits".into(),
                json!({"io_timeout_ms": IO_TIMEOUT.as_millis() as u64, "connect_timeout_ms": CONNECT_TIMEOUT.as_millis() as u64,
                       "max_reply": MAX_REPLY, "queue": QUEUE, "auto_reconnect": false, "auto_retry": false}),
            );
        }
        st
    }

    /// Last received sample if the link is up and it is fresh (`psu.meas`): no IO, no fabrication.
    pub fn meas(&self, now_mono_ns: u64) -> Result<Value, WorkerError> {
        let s = self.sh();
        if s.link != LinkState::Up {
            return Err(err(
                ErrorCode::Disconnected,
                format!("link {}; reconnect with psu connect", s.link.as_str()),
            ));
        }
        let Some(l) = s.last else {
            return Err(err(ErrorCode::NoResponse, "no sample received yet"));
        };
        let since = now_mono_ns.saturating_sub(l.mono_ns) / 1_000_000;
        if since > STALE_MS {
            return Err(err(
                ErrorCode::NoResponse,
                format!("last sample is {since} ms old (> {STALE_MS} ms)"),
            ));
        }
        let mode = if l.out { "unknown" } else { "off" };
        Ok(
            json!({"ch": self.ch, "v": l.v, "i": l.i, "p": l.v * l.i, "mode": mode, "age_ms": l.age_ms,
                  "since_ms": since, "out": l.out, "poll_n": U64(l.poll_n), "host_wall_ns": U64(l.wall_ns),
                  "backend": BACKEND, "generation": self.generation}),
        )
    }

    /// Counters (`psu.poll` — the IO thread polls by itself).
    pub fn counters(&self) -> Value {
        let s = self.sh();
        json!({"samples": U64(s.samples), "timeouts": U64(s.timeouts), "backend": BACKEND, "link": s.link.as_str()})
    }
}

impl Drop for ScpiDevice {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_only() {
        assert!(parse_loopback("127.0.0.1:5025").is_ok());
        assert!(parse_loopback("127.1.2.3:5025").is_ok());
        assert!(parse_loopback("[::1]:5025").is_ok());
        for bad in [
            "10.0.0.5:5025",
            "192.168.1.2:5025",
            "localhost:5025",
            "127.0.0.1:0",
            "TCPIP::1.2.3.4::5025::SOCKET",
        ] {
            assert!(parse_loopback(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn reply_parsing() {
        assert_eq!(parse_err("0,\"No error\""), Some((0, "No error".into())));
        assert_eq!(
            parse_err("-222,\"Data out of range\""),
            Some((-222, "Data out of range".into()))
        );
        assert_eq!(parse_bool(" ON"), Some(true));
        assert_eq!(parse_bool("2"), None);
        assert!((resolution("5.0000") - 0.00005).abs() < 1e-12);
        assert!((resolution("5") - 0.5).abs() < 1e-12);
    }
}
