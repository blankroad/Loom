//! Worker piece: owns one supply (M1: [`SimPsu`]). Calls: `psu.connect`, `psu.status`,
//! `psu.meas`, `psu.poll` (5 Hz tick from the host), `psu.sim.inject`. Writes (`op.action`):
//! `set`, `out`, `cycle`.
//!
//! Polling: one outstanding query per supply. A slow answer (`delay_ms`) stretches the period
//! (queries are skipped, the actual rate is reported), an unanswered one emits
//! `source.gap{cause:"scpi_no_response"}` and no sample (never interpolated).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, GapCount, Metric, Origin, Ts, TsSource, WriteState};
use loom_proto::num::U64;
use loom_proto::worker::{Call, Input, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, Emitter, EventDraft, JobCmd, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};

use crate::model::*;
use crate::scpi::{self, Act, LinkState, ScpiDevice};
use crate::sim::{Load, SimPsu};
use crate::{GAP_NO_RESPONSE, KIND_MEAS, KIND_STATE, KIND_WRITE, WORKER_KIND};

/// Samples emitted by one poll tick when the host fell behind (the rest are skipped, not lost:
/// they were never taken).
pub const MAX_CATCHUP: usize = 50;
/// Sample times kept for the actual-rate estimate.
const RATE_WINDOW: usize = 16;

fn err(code: ErrorCode, message: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

/// Injected faults (`psu.sim.inject`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inject {
    /// Every answer takes this long (ms).
    pub delay_ms: u64,
    /// The next N queries get no answer.
    pub no_response: u32,
}

#[derive(Debug)]
struct PendingCycle {
    on_at_ns: u64,
    request_id: String,
    origin: Origin,
    client_id: Option<String>,
}

#[derive(Debug)]
struct Device {
    ch: String,
    sim: SimPsu,
    next_due_ns: u64,
    inject: Inject,
    last_state: Value,
    recent: VecDeque<u64>,
    samples: u64,
    timeouts: u64,
    cycle: Option<PendingCycle>,
    generation: u32,
}

/// The psu worker: at most one supply, either SimPsu (`dev`) or an external SCPI endpoint (`ext`).
#[derive(Debug, Default)]
pub struct PsuWorker {
    dev: Option<Device>,
    ext: Option<ScpiDevice>,
    generation: u32,
}

/// Bound of joining the external IO thread on disconnect/reconnect.
pub const JOIN_BUDGET: Duration = Duration::from_millis(1500);

/// `Instant` of a host deadline (`deadline_mono_ns` on the worker clock), capped at `cap`.
fn until_of(cx: &WorkerCtx, deadline_mono_ns: u64, cap: Duration) -> Instant {
    let left = Duration::from_nanos(deadline_mono_ns.saturating_sub(cx.clock().mono_ns()));
    Instant::now() + left.min(cap)
}

fn scpi_state_event(cx: &WorkerCtx, d: &ScpiDevice, origin: Origin) {
    let _ = cx
        .emitter(&d.ch)
        .with_generation(Some(d.generation), None)
        .priority(EventDraft {
            kind: KIND_STATE.into(),
            ts: cx.clock().ts(TsSource::HostReadComplete),
            dir: None,
            origin,
            client_id: None,
            request_id: None,
            job_id: None,
            generation: Some(d.generation),
            purpose: None,
            risk: None,
            env: None,
            payload: d.state_payload(),
            metrics: vec![],
            quality: vec![],
        });
}

fn scpi_row(cx: &WorkerCtx, ch: &str, generation: u32, w: &Write, st: WriteState, extra: Value) {
    let mut payload = json!({"ch": ch, "write_state": st, "backend": scpi::BACKEND});
    if let (Some(p), Value::Object(e)) = (payload.as_object_mut(), extra) {
        p.extend(e);
    }
    let _ = cx
        .emitter(ch)
        .with_generation(Some(generation), None)
        .priority(EventDraft {
            kind: KIND_WRITE.into(),
            ts: cx.clock().ts(TsSource::HostReadComplete),
            dir: Some(Dir::Tx),
            origin: w.origin,
            client_id: w.client_id.clone(),
            request_id: Some(w.request_id.clone()),
            job_id: None,
            generation: Some(generation),
            purpose: None,
            risk: None,
            env: None,
            payload,
            metrics: vec![],
            quality: vec![],
        });
}

/// Timestamp of an event that happened at `t_ns` on the worker's monotonic clock.
fn ts_at(cx: &WorkerCtx, t_ns: u64) -> Ts {
    let c = cx.clock();
    let back = c.mono_ns().saturating_sub(t_ns);
    Ts {
        mono_ns: U64(t_ns),
        wall_ns: U64(c.wall_ns().saturating_sub(back)),
        clock_id: c.clock_id().to_owned(),
        source: TsSource::Synthetic,
        quality: None,
    }
}

struct Cause<'a> {
    origin: Origin,
    request_id: Option<&'a str>,
    client_id: Option<&'a str>,
}

const DEVICE: Cause<'static> = Cause {
    origin: Origin::Device,
    request_id: None,
    client_id: None,
};

impl Device {
    fn emitter(&self, cx: &WorkerCtx) -> Emitter {
        cx.emitter(&self.ch).with_generation(Some(self.generation), None)
    }

    fn state_payload(&self) -> Value {
        let mut s = self.sim.state_json();
        if let Some(o) = s.as_object_mut() {
            o.insert("ch".into(), json!(self.ch));
            // A Device represents a connected supply, independently of its output state.
            o.insert("connected".into(), json!(true));
            o.insert("cycling".into(), json!(self.cycle.is_some()));
        }
        s
    }

    /// Emits `psu.state` when it differs from the last one.
    fn state_if_changed(&mut self, cx: &WorkerCtx, t_ns: u64, cause: &Cause<'_>) {
        let s = self.state_payload();
        if s == self.last_state {
            return;
        }
        self.last_state = s.clone();
        let _ = self.emitter(cx).priority(EventDraft {
            kind: KIND_STATE.into(),
            ts: ts_at(cx, t_ns),
            dir: None,
            origin: cause.origin,
            client_id: cause.client_id.map(str::to_owned),
            // request_id marks write rows only (E8); the state change keeps the origin.
            request_id: None,
            job_id: None,
            generation: Some(self.generation),
            purpose: None,
            risk: None,
            env: None,
            payload: s,
            metrics: vec![],
            quality: vec!["simulated".into()],
        });
    }

    /// One poll query at `t_ns`.
    fn query(&mut self, cx: &WorkerCtx, t_ns: u64) -> Result<(f64, f64), WorkerError> {
        if self.inject.no_response > 0 {
            self.inject.no_response -= 1;
            self.timeouts += 1;
            let _ = self
                .emitter(cx)
                .source_gap(GAP_NO_RESPONSE, GapCount::Exact(U64(1)));
            return Err(err(
                ErrorCode::NoResponse,
                "supply did not answer the measurement query",
            ));
        }
        let m = self.sim.measure(t_ns);
        self.samples += 1;
        if self.recent.len() == RATE_WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(t_ns);
        let metric = |name: &str, value: f64, unit: &str| Metric {
            name: name.into(),
            value,
            unit: Some(unit.into()),
        };
        let _ = self.emitter(cx).priority(EventDraft {
            kind: KIND_MEAS.into(),
            ts: ts_at(cx, t_ns),
            dir: Some(Dir::Rx),
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            job_id: None,
            generation: Some(self.generation),
            purpose: Some("poll".into()),
            risk: None,
            env: None,
            payload: json!({"ch": self.ch, "age_ms": self.inject.delay_ms}),
            metrics: vec![
                metric("voltage", m.v, "V"),
                metric("current", m.i, "A"),
                metric("power", m.v * m.i, "W"),
            ],
            quality: vec!["simulated".into()],
        });
        self.state_if_changed(cx, t_ns, &DEVICE);
        Ok((m.v, m.i))
    }

    fn finish_cycle(&mut self, cx: &WorkerCtx, at_ns: u64) {
        let Some(c) = self.cycle.take() else { return };
        self.sim.output(true, c.on_at_ns);
        let st = if self.sim.out {
            WriteState::EffectiveVerified
        } else {
            WriteState::EffectUnverified
        };
        let cause = Cause {
            origin: c.origin,
            request_id: Some(&c.request_id),
            client_id: c.client_id.as_deref(),
        };
        write_row(
            self,
            cx,
            &cause,
            at_ns,
            st,
            json!({"action": "cycle", "phase": "on", "readback": {"out": self.sim.out, "prot": self.sim.prot.as_str()}}),
        );
        self.state_if_changed(cx, at_ns, &cause);
    }

    /// Catch up polling to `now_ns`. Returns (samples, timeouts) emitted.
    fn poll(&mut self, cx: &WorkerCtx, now_ns: u64) -> (u64, u64) {
        let (s0, t0) = (self.samples, self.timeouts);
        let mut n = 0;
        while self.next_due_ns <= now_ns && n < MAX_CATCHUP {
            let t = self.next_due_ns;
            if self.cycle.as_ref().is_some_and(|c| c.on_at_ns <= t) {
                self.finish_cycle(cx, t);
            }
            let _ = self.query(cx, t);
            // One outstanding query: a slow answer delays the next one.
            self.next_due_ns = t + POLL_PERIOD_NS.max(self.inject.delay_ms.saturating_mul(1_000_000));
            n += 1;
        }
        if self.next_due_ns <= now_ns {
            self.next_due_ns = now_ns + POLL_PERIOD_NS;
        }
        if self.cycle.as_ref().is_some_and(|c| c.on_at_ns <= now_ns) {
            self.finish_cycle(cx, now_ns);
        }
        (self.samples - s0, self.timeouts - t0)
    }

    fn rate_hz(&self) -> Option<f64> {
        let (a, b) = (self.recent.front()?, self.recent.back()?);
        (b > a).then(|| (self.recent.len() - 1) as f64 / ((b - a) as f64 / 1e9))
    }

    fn status(&self) -> Value {
        let mut s = self.state_payload();
        if let Some(o) = s.as_object_mut() {
            o.insert("idn".into(), json!(self.sim.idn()));
            o.insert("poll_hz_req".into(), json!(POLL_HZ));
            o.insert("poll_hz_actual".into(), json!(self.rate_hz()));
            o.insert("samples".into(), json!(self.samples));
            o.insert("timeouts".into(), json!(self.timeouts));
            o.insert(
                "inject".into(),
                json!({"delay_ms": self.inject.delay_ms, "no_response": self.inject.no_response}),
            );
            o.insert("generation".into(), json!(self.generation));
        }
        s
    }
}

fn write_row(d: &Device, cx: &WorkerCtx, cause: &Cause<'_>, t_ns: u64, st: WriteState, extra: Value) {
    let mut payload = json!({"ch": d.ch, "write_state": st});
    if let (Some(p), Value::Object(e)) = (payload.as_object_mut(), extra) {
        p.extend(e);
    }
    let _ = d.emitter(cx).priority(EventDraft {
        kind: KIND_WRITE.into(),
        ts: ts_at(cx, t_ns),
        dir: Some(Dir::Tx),
        origin: cause.origin,
        client_id: cause.client_id.map(str::to_owned),
        request_id: cause.request_id.map(str::to_owned),
        job_id: None,
        generation: Some(d.generation),
        purpose: None,
        risk: None,
        env: None,
        payload,
        metrics: vec![],
        quality: vec!["simulated".into()],
    });
}

fn outcome(state: WriteState, detail: Value, error: Option<WorkerError>) -> WriteOutcome {
    WriteOutcome { state, detail, error }
}

impl PsuWorker {
    fn dev(&mut self) -> Result<&mut Device, WorkerError> {
        self.dev.as_mut().ok_or_else(|| {
            err(
                ErrorCode::NotOpen,
                "no supply connected (loom psu connect <name>)",
            )
        })
    }

    fn connect(&mut self, cx: &WorkerCtx, args: &Value) -> Result<Value, WorkerError> {
        let ch = param_ch(args).unwrap_or("bench").to_owned();
        if let Some(e) = &self.ext {
            return Err(err(
                ErrorCode::PortBusy,
                format!(
                    "this worker drives external supply {} (disconnect it first)",
                    e.ch
                ),
            ));
        }
        if let Some(d) = &self.dev {
            if d.ch == ch {
                return Ok(d.status());
            }
            return Err(err(
                ErrorCode::PortBusy,
                format!("this worker already drives {}", d.ch),
            ));
        }
        let limits = Limits::from_value(args);
        let mut sim = SimPsu::new(limits, args.get("seed").and_then(Value::as_u64).unwrap_or(0x5EED));
        if let Some(l) = args.get("load") {
            sim.load = load_from(l, sim.load);
        }
        self.generation += 1;
        let now = cx.clock().mono_ns();
        let mut d = Device {
            ch,
            sim,
            next_due_ns: now + POLL_PERIOD_NS,
            inject: Inject::default(),
            last_state: Value::Null,
            recent: VecDeque::with_capacity(RATE_WINDOW),
            samples: 0,
            timeouts: 0,
            cycle: None,
            generation: self.generation,
        };
        d.state_if_changed(cx, now, &DEVICE);
        let st = d.status();
        self.dev = Some(d);
        Ok(st)
    }

    /// `psu.connect{backend:"scpi-tcp"}`: queries only; a lost/closed link reconnects as a new
    /// generation after the old IO thread is joined (bounded).
    async fn connect_scpi(&mut self, cx: &WorkerCtx, call: &Call) -> Result<Value, WorkerError> {
        let args = &call.args;
        let ch = param_ch(args).unwrap_or("bench").to_owned();
        if self.dev.is_some() {
            return Err(err(
                ErrorCode::PortBusy,
                "this worker drives the simulated supply",
            ));
        }
        let addr_s = args
            .get("address")
            .and_then(Value::as_str)
            .ok_or_else(|| err(ErrorCode::Usage, "scpi-tcp needs address"))?;
        let addr = scpi::parse_loopback(addr_s).map_err(|m| err(ErrorCode::Usage, m))?;
        if let Some(k) = ["seed", "load"]
            .into_iter()
            .find(|k| args.get(*k).is_some_and(|v| !v.is_null()))
        {
            return Err(err(ErrorCode::Usage, format!("{k} is SIM-only")));
        }
        let now = cx.clock().mono_ns();
        if let Some(e) = &self.ext {
            if e.ch != ch {
                return Err(err(
                    ErrorCode::PortBusy,
                    format!("this worker already drives {}", e.ch),
                ));
            }
            if e.link() == LinkState::Up {
                if e.address == addr {
                    return Ok(e.status(now));
                }
                return Err(err(
                    ErrorCode::PortBusy,
                    format!("{ch} is connected to {} (disconnect first)", e.address),
                ));
            }
            // Manual reconnect after lost/closed: the old IO thread must have ended.
            let joined = e
                .close_and_join(Instant::now() + JOIN_BUDGET)
                .await
                .unwrap_or(false);
            if !joined {
                return Err(WorkerError {
                    code: ErrorCode::Busy,
                    message: "previous IO thread still running; not reconnecting".into(),
                    detail: json!({"io_thread_finished": false, "join_budget_ms": JOIN_BUDGET.as_millis() as u64}),
                });
            }
            self.ext = None;
        }
        let generation = self.generation + 1;
        let until = until_of(cx, call.deadline_mono_ns.get(), scpi::CONNECT_BUDGET);
        let rx = scpi::connect(
            ch.clone(),
            addr,
            generation,
            Limits::from_value(args),
            cx.emitter(&ch),
            cx.latch().clone(),
            until,
        );
        match rx.await {
            Some(Ok(d)) => {
                self.generation = generation;
                let st = d.status(cx.clock().mono_ns());
                self.ext = Some(d);
                Ok(st)
            }
            Some(Err(e)) => Err(e),
            None => Err(err(
                ErrorCode::Timeout,
                format!(
                    "scpi-tcp {addr}: handshake (dial ≤ {} ms + 4 queries ≤ {} ms each) not done before the deadline",
                    scpi::CONNECT_TIMEOUT.as_millis(),
                    scpi::IO_TIMEOUT.as_millis()
                ),
            )),
        }
    }

    /// `psu.disconnect{ch}`: nothing is sent; the output is held.
    async fn disconnect(&mut self, cx: &WorkerCtx, call: &Call) -> Result<Value, WorkerError> {
        let ch = param_ch(&call.args).unwrap_or("bench").to_owned();
        if self.ext.as_ref().is_some_and(|e| e.ch == ch) {
            let Some(d) = self.ext.take() else {
                return Err(err(ErrorCode::NotOpen, "no supply connected"));
            };
            let joined = d
                .close_and_join(Instant::now() + JOIN_BUDGET)
                .await
                .unwrap_or(false);
            scpi_state_event(cx, &d, call.origin);
            let generation = d.generation;
            if !joined {
                // Keep the handle: status shows it, reconnect re-checks the join.
                self.ext = Some(d);
                return Err(WorkerError {
                    code: ErrorCode::Busy,
                    message: format!("IO thread did not finish within {} ms", JOIN_BUDGET.as_millis()),
                    detail: json!({"io_thread_finished": false}),
                });
            }
            return Ok(
                json!({"ch": ch, "closed": true, "backend": scpi::BACKEND, "generation": generation,
                             "io_thread_finished": true, "output": "held (nothing sent)"}),
            );
        }
        if self.dev.as_ref().is_some_and(|d| d.ch == ch) {
            if let Some(mut d) = self.dev.take() {
                let mut st = d.state_payload();
                st["connected"] = json!(false);
                d.last_state = Value::Null;
                let _ = d.emitter(cx).priority(EventDraft {
                    kind: KIND_STATE.into(),
                    ts: ts_at(cx, cx.clock().mono_ns()),
                    dir: None,
                    origin: call.origin,
                    client_id: None,
                    request_id: None,
                    job_id: None,
                    generation: Some(d.generation),
                    purpose: None,
                    risk: None,
                    env: None,
                    payload: st,
                    metrics: vec![],
                    quality: vec!["simulated".into()],
                });
            }
            return Ok(
                json!({"ch": ch, "closed": true, "backend": "sim", "output": "held (nothing switched)"}),
            );
        }
        Err(err(ErrorCode::NotOpen, format!("{ch} is not connected")))
    }

    async fn write_scpi(&mut self, cx: &WorkerCtx, w: Write) -> WriteOutcome {
        let Some(d) = self.ext.as_ref() else {
            return outcome(
                WriteState::Failed,
                Value::Null,
                Some(err(ErrorCode::NotOpen, "no supply connected")),
            );
        };
        let action =
            w.op.get("action")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
        let refuse = |code: ErrorCode, msg: String| {
            scpi_row(
                cx,
                &d.ch,
                d.generation,
                &w,
                WriteState::Failed,
                json!({"action": action, "sent": false, "error": {"code": code, "message": msg}}),
            );
            outcome(WriteState::Failed, Value::Null, Some(err(code, msg)))
        };
        let act = match action.as_str() {
            "set" => {
                let (v, i) = (param_volt(&w.op), param_curr(&w.op));
                if v.is_none() && i.is_none() {
                    return refuse(ErrorCode::Usage, "set needs volt and/or curr".into());
                }
                Act::Set(v, i)
            }
            "out" => match w.op.get("state").and_then(Value::as_str) {
                Some("on") => Act::Out(true),
                Some("off") => Act::Out(false),
                _ => return refuse(ErrorCode::Usage, "out needs state on|off".into()),
            },
            "cycle" => {
                return refuse(
                    ErrorCode::NotImplemented,
                    "cycle is not implemented for scpi-tcp (refused, not simulated)".into(),
                );
            }
            other => return refuse(ErrorCode::Usage, format!("unknown action {other:?}")),
        };
        let until = until_of(cx, w.deadline_mono_ns.get(), scpi::WRITE_BUDGET);
        if until <= Instant::now() {
            return refuse(
                ErrorCode::Timeout,
                "request deadline already passed; nothing sent".into(),
            );
        }
        let rx = match d.write(act, w.origin, w.request_id.clone(), w.client_id.clone(), until) {
            Ok(rx) => rx,
            Err(e) => return refuse(e.code, e.message),
        };
        // The IO thread enforces `until` per unit; the grace only lets its answer arrive.
        match rx.await {
            Some(o) => o,
            None => outcome(
                WriteState::OutcomeUnknown,
                json!({"retry": false}),
                Some(err(
                    ErrorCode::Timeout,
                    "no outcome before the request deadline; the psu.write rows carry the final state",
                )),
            ),
        }
    }

    fn inject(&mut self, args: &Value) -> Result<Value, WorkerError> {
        let d = self.dev()?;
        if args.get("clear").and_then(Value::as_bool).unwrap_or(false) {
            d.inject = Inject::default();
        }
        if let Some(ms) = args.get("delay_ms").and_then(Value::as_u64) {
            if ms > MAX_INJECT_DELAY_MS {
                return Err(err(ErrorCode::Usage, format!("delay_ms ≤ {MAX_INJECT_DELAY_MS}")));
            }
            d.inject.delay_ms = ms;
        }
        if let Some(n) = args.get("no_response").and_then(Value::as_u64) {
            d.inject.no_response = u32::try_from(n).unwrap_or(u32::MAX).min(MAX_INJECT_NO_RESPONSE);
        }
        if let Some(l) = args.get("load") {
            d.sim.load = load_from(l, d.sim.load);
        }
        if let Some(o) = args.get("ocp").and_then(Value::as_bool) {
            d.sim.ocp = o;
        }
        Ok(
            json!({"inject": {"delay_ms": d.inject.delay_ms, "no_response": d.inject.no_response},
                  "load": {"r_ohm": d.sim.load.r_ohm, "inrush_a": d.sim.load.inrush_a}, "ocp": d.sim.ocp}),
        )
    }
}

fn load_from(v: &Value, base: Load) -> Load {
    let f = |k: &str, def: f64| {
        v.get(k)
            .and_then(Value::as_f64)
            .filter(|x| x.is_finite() && *x >= 0.0)
            .unwrap_or(def)
    };
    Load {
        r_ohm: f("r_ohm", base.r_ohm).max(1e-3),
        inrush_a: f("inrush_a", base.inrush_a),
        inrush_tau_ms: f("inrush_tau_ms", base.inrush_tau_ms),
        noise_a: f("noise_a", base.noise_a),
    }
}

impl Worker for PsuWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move {
            let now = cx.clock().mono_ns();
            if call.method == "psu.connect"
                && call.args.get("backend").and_then(Value::as_str) == Some(scpi::BACKEND)
            {
                return self.connect_scpi(cx, &call).await;
            }
            if call.method == "psu.disconnect" {
                return self.disconnect(cx, &call).await;
            }
            if let Some(e) = &self.ext {
                return match call.method.as_str() {
                    "psu.status" => Ok(e.status(now)),
                    "psu.poll" => Ok(e.counters()),
                    "psu.meas" => e.meas(now),
                    "psu.connect" => Err(err(
                        ErrorCode::PortBusy,
                        format!(
                            "this worker drives external supply {} (disconnect it first)",
                            e.ch
                        ),
                    )),
                    "psu.sim.inject" => Err(err(
                        ErrorCode::NotImplemented,
                        "fault injection is SIM-only; an external supply is never simulated",
                    )),
                    m => Err(err(ErrorCode::NotImplemented, format!("{m} not implemented"))),
                };
            }
            match call.method.as_str() {
                "psu.connect" => self.connect(cx, &call.args),
                "psu.status" => {
                    let d = self.dev()?;
                    d.poll(cx, now);
                    Ok(d.status())
                }
                "psu.poll" => {
                    let d = self.dev()?;
                    let (samples, timeouts) = d.poll(cx, now);
                    Ok(json!({"samples": samples, "timeouts": timeouts}))
                }
                "psu.meas" => {
                    let d = self.dev()?;
                    d.poll(cx, now);
                    let (v, i) = d.query(cx, now)?;
                    Ok(
                        json!({"ch": d.ch, "v": v, "i": i, "p": v * i, "mode": d.sim.mode().as_str(),
                              "age_ms": d.inject.delay_ms, "out": d.sim.out}),
                    )
                }
                "psu.sim.inject" => self.inject(&call.args),
                m => Err(err(ErrorCode::NotImplemented, format!("{m} not implemented"))),
            }
        })
    }

    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, w: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move {
            if self.ext.is_some() {
                return self.write_scpi(cx, w).await;
            }
            let now = cx.clock().mono_ns();
            let Some(d) = self.dev.as_mut() else {
                return outcome(
                    WriteState::Failed,
                    Value::Null,
                    Some(err(ErrorCode::NotOpen, "no supply connected")),
                );
            };
            d.poll(cx, now);
            let action =
                w.op.get("action")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
            let cause = Cause {
                origin: w.origin,
                request_id: Some(&w.request_id),
                client_id: w.client_id.as_deref(),
            };
            let fail = |d: &Device, code: ErrorCode, msg: String| {
                write_row(
                    d,
                    cx,
                    &cause,
                    now,
                    WriteState::Failed,
                    json!({"action": action, "error": {"code": code, "message": msg}}),
                );
                outcome(WriteState::Failed, Value::Null, Some(err(code, msg)))
            };
            // STOP latches agent/script writes; the user (incl. emergency OFF) is never latched (A3).
            if cx.latch().is_on() && w.origin != Origin::User {
                return fail(d, ErrorCode::Stopped, "STOP latched".into());
            }
            let unanswered = d.inject.no_response > 0;
            let (sent, readback, effective) = match action.as_str() {
                "set" => {
                    let (v, i) = (param_volt(&w.op), param_curr(&w.op));
                    if v.is_none() && i.is_none() {
                        return fail(d, ErrorCode::Usage, "set needs volt and/or curr".into());
                    }
                    if let Err(why) = d.sim.set(v, i) {
                        return fail(d, ErrorCode::Denied, why);
                    }
                    let ok = v.is_none_or(|v| d.sim.set_v == v) && i.is_none_or(|i| d.sim.set_i == i);
                    (
                        json!({"action": "set", "volt": v, "curr": i}),
                        json!({"set_v": d.sim.set_v, "set_i": d.sim.set_i}),
                        ok,
                    )
                }
                "out" => {
                    let on = match w.op.get("state").and_then(Value::as_str) {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return fail(d, ErrorCode::Usage, "out needs state on|off".into()),
                    };
                    if let Some(c) = d.cycle.take() {
                        let cc = Cause {
                            origin: c.origin,
                            request_id: Some(&c.request_id),
                            client_id: c.client_id.as_deref(),
                        };
                        write_row(
                            d,
                            cx,
                            &cc,
                            now,
                            WriteState::EffectUnverified,
                            json!({"action": "cycle", "superseded_by": w.request_id}),
                        );
                    }
                    d.sim.output(on, now);
                    let ok = d.sim.out == on;
                    (
                        json!({"action": "out", "state": if on { "on" } else { "off" }}),
                        json!({"out": d.sim.out, "prot": d.sim.prot.as_str()}),
                        ok,
                    )
                }
                "cycle" => {
                    let off_ms = w.op.get("off_ms").and_then(Value::as_u64).unwrap_or(1000);
                    if off_ms == 0 || off_ms > MAX_CYCLE_OFF_MS {
                        return fail(d, ErrorCode::Usage, format!("off_ms 1..={MAX_CYCLE_OFF_MS}"));
                    }
                    d.sim.output(false, now);
                    d.cycle = Some(PendingCycle {
                        on_at_ns: now + off_ms * 1_000_000,
                        request_id: w.request_id.clone(),
                        origin: w.origin,
                        client_id: w.client_id.clone(),
                    });
                    write_row(
                        d,
                        cx,
                        &cause,
                        now,
                        WriteState::Sent,
                        json!({"action": "cycle", "off_ms": off_ms, "phase": "off"}),
                    );
                    d.state_if_changed(cx, now, &cause);
                    // Terminal row comes when the output is back on (next poll after off_ms).
                    return outcome(
                        WriteState::Sent,
                        json!({"off_ms": off_ms, "on_at_mono_ns": U64(now + off_ms * 1_000_000)}),
                        None,
                    );
                }
                other => return fail(d, ErrorCode::Usage, format!("unknown action {other:?}")),
            };
            write_row(d, cx, &cause, now, WriteState::Sent, sent);
            d.state_if_changed(cx, now, &cause);
            if unanswered {
                // The supply took the command but did not answer the read-back.
                d.inject.no_response -= 1;
                d.timeouts += 1;
                let _ = d.emitter(cx).source_gap(GAP_NO_RESPONSE, GapCount::Exact(U64(1)));
                write_row(
                    d,
                    cx,
                    &cause,
                    now,
                    WriteState::OutcomeUnknown,
                    json!({"action": action, "error": "read-back unanswered"}),
                );
                return outcome(
                    WriteState::OutcomeUnknown,
                    Value::Null,
                    Some(err(ErrorCode::NoResponse, "read-back unanswered")),
                );
            }
            let st = if effective {
                WriteState::EffectiveVerified
            } else {
                WriteState::EffectUnverified
            };
            write_row(
                d,
                cx,
                &cause,
                now,
                st,
                json!({"action": action, "readback": readback}),
            );
            outcome(st, json!({"readback": readback}), None)
        })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async {
            Err(err(
                ErrorCode::NotImplemented,
                "psu has no jobs (watch polling is a read)",
            ))
        })
    }

    fn on_input<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _input: Input,
    ) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async { Err(err(ErrorCode::NotImplemented, "psu has no interactive input")) })
    }
}
