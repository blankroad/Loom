//! `UartWorker` — the UART device owner: one worker process per port (UART plan §3.1, §3.13).
//!
//! The gateway sends request writes (`uart.open/close/send/run`) to [`Worker::on_write`]
//! (`op.method` names the verb, `crates/loom-uart/src/commands.rs`), everything else to
//! [`Worker::on_call`], and viewer keystrokes to [`Worker::on_input`].
//!
//! **Concurrency.** `UartWorker` is a cheap handle (`Clone`) over shared state, and every
//! operation also exists as a `&self` method ([`UartWorker::call`], [`UartWorker::write`],
//! [`UartWorker::input`]). A deferred agent send waits for the user to finish a line and an
//! `expect` may wait an hour, while viewer keys (Ctrl-C!) must still reach the device at once
//! (contract 05 §5.1). Production uses [`SharedWorker`] via [`run`]. The serial [`Worker`]
//! adapter remains for existing in-process test harnesses.

mod port;
mod send;
mod util;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use loom_console::arbiter::{ArbiterConfig, InputArbiter, YieldResult};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, TsSource, WriteState};
use loom_proto::worker::{Call, Input, InputLeaseEnd, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, JobCmd, SharedWorker, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};

pub use port::{ARB_TICK, MAX_ADMIT_WAITERS, MAX_EXPECT_WAITERS, MAX_TX_WAITERS, PUMP_BATCH};
pub use send::{DEFER_TIMEOUT_MS, MAX_WAIT_MS, RUN_TIMEOUT_MS, SEND_EXPECT_TIMEOUT_MS};

use port::{Port, PortInfo};
use send::{
    ExpectWait, activity_expect, activity_idle, agent_write, channel_error, expect_finish, parse_patterns,
    submit_user, viewer_input,
};
use util::{arg_str, arg_u64, ctx_clock, draft, env_name, kind, open_error, parse_env, werr};

use crate::bridge::bridge;
use crate::channel::{ChannelCore, ExpectFrom, LinesQuery, Profile, ReadQuery};
use crate::clock::SeqSource;
use crate::reader::Reader;
use crate::transport::{
    self, FlowControl, OpenOpts, Parity, PortConfig, SIM_SCHEME, StopBits, TransportKind,
};
use crate::tx::{TxActor, event_channel};

/// Worker kind registered by the gateway side (`loom-uart`).
pub const WORKER_KIND: &str = "uart";
/// Sim-issued pty paths remembered for `OpenOpts::sim_pty` (C2 bound).
pub const MAX_SIM_PTYS: usize = 16;
/// Channels whose ring/cursors are kept after close for reads and reopen (one port per worker;
/// a second name replaces the first).
pub const MAX_KEPT_CHANNELS: usize = 1;

/// Ring/cursors of a closed channel and its last generation.
type KeptCore = (Arc<Mutex<ChannelCore>>, u32);

#[derive(Debug, Default)]
struct Inner {
    port: Mutex<Option<Arc<Port>>>,
    // Open/close run off the callback executor; serialize teardown and generation transfer.
    lifecycle: Mutex<()>,
    publication: Arc<Mutex<()>>,
    /// Ring/cursors of the last channel, kept across close → reopen (generation + 1).
    kept: Mutex<HashMap<String, KeptCore>>,
    sim_ptys: Mutex<Vec<String>>,
    next_input_seq: AtomicU64,
    next_expect: AtomicU64,
    opens: AtomicU32,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The UART worker (cheap to clone; clones share the port).
#[derive(Debug, Clone, Default)]
pub struct UartWorker {
    inner: Arc<Inner>,
}

impl UartWorker {
    /// New worker with no port open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember a pty slave path issued by Loom's own sim harness: only such paths skip the
    /// baud ioctl (`OpenOpts::sim_pty`, DR-V18). Never call this for real devices.
    pub fn register_sim_pty(&self, path: &str) -> Result<(), WorkerError> {
        let mut v = lock(&self.inner.sim_ptys);
        if v.len() >= MAX_SIM_PTYS {
            return Err(werr(ErrorCode::RateLimit, "too many sim ptys registered"));
        }
        v.push(path.to_owned());
        Ok(())
    }

    /// Threads still running for the open port (0 when closed).
    pub fn live_threads(&self) -> usize {
        lock(&self.inner.port).as_ref().map_or(0, |p| p.live_threads())
    }

    fn port(&self) -> Option<Arc<Port>> {
        lock(&self.inner.port).clone()
    }

    fn port_for(&self, args: &Value) -> Result<Arc<Port>, WorkerError> {
        let port = self
            .port()
            .ok_or_else(|| werr(ErrorCode::NotOpen, "no port is open"))?;
        if let Some(ch) = arg_str(args, "channel")
            && ch != port.info.channel
            && ch != port.info.target
        {
            return Err(werr(
                ErrorCode::NotOpen,
                format!("channel '{ch}' is not open in this worker"),
            ));
        }
        Ok(port)
    }

    /// Kept core for reads after close.
    fn core_for(&self, args: &Value) -> Result<Arc<Mutex<ChannelCore>>, WorkerError> {
        if let Ok(p) = self.port_for(args) {
            return Ok(Arc::clone(&p.core));
        }
        let kept = lock(&self.inner.kept);
        let found = match arg_str(args, "channel") {
            Some(ch) => kept.get(ch),
            None => kept.values().next(),
        };
        found
            .map(|(c, _)| Arc::clone(c))
            .ok_or_else(|| werr(ErrorCode::NotOpen, "channel is not open"))
    }

    // ------------------------------------------------------------------ calls

    /// `on_call` body (`&self`, may run concurrently with writes and input).
    pub async fn call(&self, cx: &WorkerCtx, call: Call) -> Result<Value, WorkerError> {
        let args = &call.args;
        let client = call.client_id.clone().unwrap_or_else(|| "agent".into());
        match call.method.as_str() {
            "uart.ports" => ports(),
            "uart.status" => self.status(args),
            "uart.read" => {
                let q = ReadQuery::from_params(args).map_err(|e| channel_error(&e))?;
                let core = self.core_for(args)?;
                let r = lock(&core).read(&client, &q);
                r.map_err(|e| channel_error(&e))
            }
            "uart.lines" => {
                let q = LinesQuery::from_params(args).map_err(|e| channel_error(&e))?;
                let core = self.core_for(args)?;
                let r = lock(&core).lines(&q);
                r.map_err(|e| channel_error(&e))
            }
            "uart.expect" => self.expect(args, &client, call.request_id.as_deref()).await,
            "uart.input.yield" => self.viewer_yield(args, viewer_conn(&call)?),
            "uart.input.clear" => self.viewer_clear(args, viewer_conn(&call)?),
            "uart.input.takeover" => self.viewer_takeover(args, viewer_conn(&call)?),
            "uart.hold" | "uart.unhold" => self.viewer_hold(args, call.method == "uart.hold"),
            // Request writes normally arrive through `on_write`; accept them here too.
            "uart.open" | "uart.close" | "uart.send" | "uart.run" => Err(werr(
                ErrorCode::Usage,
                format!("{} is a request write (on_write)", call.method),
            )),
            m => {
                let _ = cx;
                Err(werr(
                    ErrorCode::NotImplemented,
                    format!("{m} is not implemented in M1"),
                ))
            }
        }
    }

    /// `on_write` body.
    pub async fn write(&self, cx: &WorkerCtx, w: Write) -> WriteOutcome {
        let method = arg_str(&w.op, "method").unwrap_or_default().to_owned();
        match method.as_str() {
            "uart.open" => outcome(self.open(cx, &w.op)),
            "uart.close" => outcome(self.close(cx, &w.op)),
            "uart.send" | "uart.run" => match self.port_for(&w.op) {
                Ok(port) => agent_write(port, cx, w, method == "uart.run").await,
                Err(e) => WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(e),
                },
            },
            m => WriteOutcome {
                state: WriteState::Failed,
                detail: Value::Null,
                error: Some(werr(
                    ErrorCode::NotImplemented,
                    format!("write '{m}' is not implemented"),
                )),
            },
        }
    }

    /// `on_input` body: viewer keystrokes (write class ③).
    pub async fn input(&self, _cx: &WorkerCtx, input: Input) -> Result<(), WorkerError> {
        let port = self
            .port()
            .ok_or_else(|| werr(ErrorCode::NotOpen, "no port is open"))?;
        if input.channel.as_deref().is_some_and(|ch| ch != port.info.channel) {
            return Err(werr(
                ErrorCode::NotOpen,
                "input channel belongs to another worker",
            ));
        }
        let viewer = input
            .conn_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| werr(ErrorCode::Usage, "viewer input requires conn_id"))?;
        let seq = input
            .input_seq
            .ok_or_else(|| werr(ErrorCode::Usage, "viewer input requires input_seq"))?;
        viewer_input(&port, viewer, seq.0, &input.bytes.0);
        Ok(())
    }

    /// End viewer ownership and discard its held keys; an unfinished device line stays dirty.
    /// An old peer without connection identity cannot safely identify a viewer to release.
    pub fn input_lease_end(&self, end: InputLeaseEnd) {
        let Some(viewer) = end.conn_id.filter(|s| !s.is_empty()) else {
            return;
        };
        let Some(port) = self.port() else { return };
        let (actions, events) = {
            let mut arb = port.arb();
            let actions = arb.viewer_left(&viewer, port.now_ms());
            (actions, arb.take_events())
        };
        port.emit_arbiter(events);
        port.dispatch(actions);
    }

    // ------------------------------------------------------------------ open / close

    fn open(&self, cx: &WorkerCtx, op: &Value) -> Result<Value, WorkerError> {
        self.open_inner(cx, op, None)
    }

    /// Test seam: open with an already-constructed transport (e.g. a deliberately slow one to
    /// test Ctrl-C preemption). `op` is the same as `uart.open`; `target` is only a label.
    /// Not reachable from any RPC — the worker binary only uses [`transport::open`].
    pub fn open_with_transport(
        &self,
        cx: &WorkerCtx,
        op: &Value,
        transport: Box<dyn transport::Transport>,
    ) -> Result<Value, WorkerError> {
        self.open_inner(cx, op, Some(transport))
    }

    fn open_inner(
        &self,
        cx: &WorkerCtx,
        op: &Value,
        injected: Option<Box<dyn transport::Transport>>,
    ) -> Result<Value, WorkerError> {
        let _lifecycle = lock(&self.inner.lifecycle);
        let target = arg_str(op, "target")
            .ok_or_else(|| werr(ErrorCode::Usage, "open needs a target"))?
            .to_owned();
        let config = parse_config(op.get("config"))?;
        config
            .validate()
            .map_err(|e| werr(ErrorCode::Unsupported, format!("unsupported line settings: {e}")))?;
        let profile_name = arg_str(op, "profile").unwrap_or("generic").to_owned();
        let profile = Profile::named(&profile_name)
            .ok_or_else(|| werr(ErrorCode::Usage, format!("unknown profile '{profile_name}'")))?;
        let channel = arg_str(op, "alias")
            .map(str::to_owned)
            .unwrap_or_else(|| default_alias(&target));
        // Simulator provenance controls the default environment. The serial transport detects
        // external macOS PTY descriptors independently; they must not become internal SIMs.
        let sim_issued_pty = lock(&self.inner.sim_ptys).contains(&target) || live_sim_pty(&target);
        let env = match arg_str(op, "env") {
            Some(s) => parse_env(s).ok_or_else(|| werr(ErrorCode::Usage, format!("bad env '{s}'")))?,
            None if target.starts_with(SIM_SCHEME) || sim_issued_pty => Env::Sim,
            None => Env::Bench,
        };

        let mut slot = lock(&self.inner.port);
        if let Some(p) = slot.as_ref() {
            // Idempotent reopen of the same port with the same settings.
            if p.info.target == target && p.info.config == config && p.info.profile == profile_name {
                return Ok(open_result(&p.info));
            }
            return Err(werr(
                ErrorCode::Config,
                format!(
                    "worker already owns '{}' with other settings; close it first",
                    p.info.target
                ),
            ));
        }

        let opts = OpenOpts {
            sim_pty: sim_issued_pty,
            ..OpenOpts::default()
        };
        let transport = match injected {
            Some(t) => t,
            None => transport::open(&target, &config, &opts).map_err(|e| open_error(&e))?,
        };
        let kind_ = transport.kind();
        let rx_handle = transport.rx_handle().map_err(|e| open_error(&e))?;

        // Ring/cursors survive close → reopen of the same channel (generation + 1).
        let (core, generation) = {
            let mut kept = lock(&self.inner.kept);
            match kept.remove(&channel) {
                Some((core, last_gen)) => {
                    let g = last_gen + 1;
                    lock(&core).new_generation(g);
                    (core, g)
                }
                None => {
                    kept.clear();
                    (
                        Arc::new(Mutex::new(ChannelCore::new(&channel, profile.clone(), 1))),
                        1,
                    )
                }
            }
        };
        let emitter = cx.emitter(&channel).with_generation(Some(generation), Some(env));
        lock(&core).set_producer(emitter.producer_id(), emitter.producer_epoch().0);
        let start_offset = lock(&core).rx_end();

        let clock = ctx_clock(cx);
        let seq = Arc::new(SeqSource::default());
        let (btx, brx) = bridge();
        let reader = Reader::spawn(rx_handle, btx, Arc::clone(&seq), Arc::clone(&clock), start_offset)
            .map_err(|e| open_error(&e))?;
        let (etx, erx) = event_channel();
        let actor = TxActor::spawn(transport, seq, clock, etx).map_err(|e| open_error(&e))?;
        let arb = InputArbiter::new(ArbiterConfig {
            has_prompt: profile.prompt.is_some(),
            line_kill_ctrl_u: profile.line_kill_ctrl_u,
        });
        let info = PortInfo {
            channel,
            target,
            generation,
            env,
            profile: profile_name,
            config_json: config_json(&config),
            config,
            clock_id: cx.clock().clock_id().to_owned(),
        };
        let port = Arc::new(Port::new(
            info,
            core,
            arb,
            actor,
            emitter,
            Arc::clone(&self.inner.publication),
        ));
        port.start(reader, brx, erx).map_err(|e| open_error(&e))?;
        self.inner.opens.fetch_add(1, Ordering::Relaxed);

        let ts = port.ts_now(TsSource::HostReadComplete);
        port.emit(draft(
            kind::STATE,
            ts.clone(),
            Origin::Device,
            json!({"state": "open", "path": port.info.target, "transport": transport_name(kind_)}),
        ));
        let mut cfg = port.info.config_json.clone();
        cfg["profile"] = json!(port.info.profile);
        cfg["env"] = json!(env_name(env));
        port.emit(draft(kind::CONFIG, ts, Origin::Device, cfg));
        let result = open_result(&port.info);
        *slot = Some(port);
        Ok(result)
    }

    fn close(&self, _cx: &WorkerCtx, op: &Value) -> Result<Value, WorkerError> {
        let _lifecycle = lock(&self.inner.lifecycle);
        let port = {
            let mut slot = lock(&self.inner.port);
            let Some(p) = slot.as_ref() else {
                return Ok(json!({"closed": false, "reason": "not open"}));
            };
            if let Some(ch) = arg_str(op, "channel")
                && ch != p.info.channel
                && ch != p.info.target
            {
                return Err(werr(ErrorCode::NotOpen, format!("channel '{ch}' is not open")));
            }
            slot.take()
        };
        let Some(port) = port else {
            return Ok(json!({"closed": false}));
        };
        // Unfinished agent writes end `outcome_unknown` (running) or `failed` (queued) with cause
        // `closed`, before the actor is stopped.
        port.cancel_agents("closed", ErrorCode::NotOpen);
        port.shutdown();
        let threads_left = port.live_threads();
        // Every thread closure held an `Arc<Port>`; only ours should remain.
        let refs_left = Arc::strong_count(&port) - 1;
        port.emit(draft(
            kind::STATE,
            port.ts_now(TsSource::HostReadComplete),
            Origin::Device,
            json!({"state": "closed", "path": port.info.target}),
        ));
        let mut kept = lock(&self.inner.kept);
        if kept.len() >= MAX_KEPT_CHANNELS {
            kept.clear();
        }
        kept.insert(
            port.info.channel.clone(),
            (Arc::clone(&port.core), port.info.generation),
        );
        Ok(json!({
            "channel": port.info.channel, "closed": true, "generation": port.info.generation,
            "threads_left": threads_left, "refs_left": refs_left,
        }))
    }

    // ------------------------------------------------------------------ reads & waits

    fn status(&self, args: &Value) -> Result<Value, WorkerError> {
        let core = self.core_for(args)?;
        let mut v = lock(&core).status();
        if let Some(p) = self.port() {
            let a = p.arb();
            let owner = match a.owner() {
                loom_console::arbiter::Owner::None => json!({"owner": "none"}),
                loom_console::arbiter::Owner::User(v) => json!({"owner": "user", "viewer": v}),
                loom_console::arbiter::Owner::Agent { client, request_id } => {
                    json!({"owner": "agent", "client": client, "request_id": request_id})
                }
            };
            let extra = json!({
                "open": true,
                "connected": p.connected.load(Ordering::Acquire),
                "channel": p.info.channel, "path": p.info.target, "generation": p.info.generation,
                "env": env_name(p.info.env), "profile": p.info.profile, "config": p.info.config_json,
                "input": owner, "input_dirty": a.input_dirty(), "hold": a.hold_on(),
                "lock": a.lock_info().map(|(c, r, exp)| json!({"actor": c, "reason": r, "expires_ms": exp})),
                "pending_agent_sends": a.pending_agent_sends(),
                "emit_dropped": {
                    "rx": p.emit_stats.rx_dropped.load(Ordering::Relaxed),
                    "tx": p.emit_stats.tx_dropped.load(Ordering::Relaxed),
                    "ctl": p.emit_stats.ctl_dropped.load(Ordering::Relaxed),
                },
            });
            if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
                m.extend(e);
            }
        } else if let Value::Object(m) = &mut v {
            m.insert("open".into(), json!(false));
        }
        Ok(v)
    }

    async fn expect(
        &self,
        args: &Value,
        client: &str,
        request_id: Option<&str>,
    ) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let patterns = parse_patterns(args.get("patterns"))?;
        if patterns.is_empty() {
            return Err(werr(ErrorCode::Usage, "expect needs at least one pattern"));
        }
        let timeout_ms = arg_u64(args, "timeout_ms")
            .unwrap_or(RUN_TIMEOUT_MS)
            .min(MAX_WAIT_MS);
        // seq:/mark:/time cursors are resolved by the gateway (P1 gateway module).
        let from = ExpectFrom::from_spec(arg_str(args, "from"), client).map_err(|e| channel_error(&e))?;
        let id = self.inner.next_expect.fetch_add(1, Ordering::Relaxed) | (1 << 62);
        let wait = ExpectWait::register(&port, id, &patterns, from)?;
        let label = patterns.first().map(|p| p.pattern.clone()).unwrap_or_default();
        activity_expect(&port, &label, timeout_ms, request_id, client);
        let started = wait.started_rx();
        let end = wait.wait(&port, Duration::from_millis(timeout_ms)).await;
        activity_idle(&port);
        expect_finish(&port, &end, id, started, request_id, None)
    }

    // ------------------------------------------------------------------ viewer arbitration

    fn viewer_yield(&self, args: &Value, viewer: &str) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let now = port.now_ms();
        let (res, acts, evs) = {
            let mut a = port.arb();
            let (res, acts) = a.yield_input(viewer, now);
            (res, acts, a.take_events())
        };
        port.emit_arbiter(evs);
        let out = match &res {
            YieldResult::Released => json!({"result": "released"}),
            YieldResult::ConfirmClear { line_len } => {
                json!({"result": "confirm_clear", "line_len": line_len})
            }
            YieldResult::Cleared { send } => {
                let seq = self.inner.next_input_seq.fetch_add(1, Ordering::Relaxed) + 1;
                submit_user(&port, viewer, seq, send, false, None);
                json!({"result": "cleared"})
            }
            YieldResult::ManualClearNeeded { line_len } => {
                json!({"result": "manual_clear_needed", "line_len": line_len})
            }
        };
        port.dispatch(acts);
        Ok(out)
    }

    fn viewer_clear(&self, args: &Value, viewer: &str) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let now = port.now_ms();
        let (send, acts, evs) = {
            let mut a = port.arb();
            let (send, acts) = a.clear_input(viewer, now);
            (send, acts, a.take_events())
        };
        port.emit_arbiter(evs);
        let sent_kill = !send.is_empty();
        let seq = self.inner.next_input_seq.fetch_add(1, Ordering::Relaxed) + 1;
        submit_user(&port, viewer, seq, &send, false, None);
        port.dispatch(acts);
        Ok(json!({"cleared": true, "sent_line_kill": sent_kill}))
    }

    fn viewer_takeover(&self, args: &Value, viewer: &str) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let now = port.now_ms();
        let (d, evs) = {
            let mut a = port.arb();
            let d = a.takeover(viewer, now);
            (d, a.take_events())
        };
        port.emit_arbiter(evs);
        let seq = self.inner.next_input_seq.fetch_add(1, Ordering::Relaxed) + 1;
        submit_user(
            &port,
            viewer,
            seq,
            &d.send,
            d.interrupt,
            d.preempt_agent.as_deref(),
        );
        Ok(json!({"owner": "user", "viewer": viewer, "sent_held_bytes": d.send.len()}))
    }

    fn viewer_hold(&self, args: &Value, on: bool) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let now = port.now_ms();
        let (acts, evs) = {
            let mut a = port.arb();
            let acts = a.set_hold(on, now);
            (acts, a.take_events())
        };
        port.emit_arbiter(evs);
        port.dispatch(acts);
        let cancelled = if on {
            // HOLD also cuts what is already queued or running (arbiter rejected the waiting ones).
            port.cancel_agents("held_by_user", ErrorCode::HeldByUser)
        } else {
            0
        };
        Ok(json!({"hold": on, "cancelled_agent_requests": cancelled}))
    }
}

fn outcome(r: Result<Value, WorkerError>) -> WriteOutcome {
    match r {
        // Port open/close is not a byte transmission; `sent` = the driver accepted the request.
        Ok(detail) => WriteOutcome {
            state: WriteState::Sent,
            detail,
            error: None,
        },
        Err(e) => WriteOutcome {
            state: WriteState::Failed,
            detail: Value::Null,
            error: Some(e),
        },
    }
}

fn open_result(info: &PortInfo) -> Value {
    json!({
        "channel": info.channel, "generation": info.generation, "path": info.target,
        "config": info.config_json, "profile": info.profile, "env": env_name(info.env),
    })
}

/// `sim://bootshell?seed=1` → `bootshell`; `/dev/ttyUSB0` → `ttyUSB0`; `COM3` → `com3`.
fn default_alias(target: &str) -> String {
    let base = target
        .strip_prefix(SIM_SCHEME)
        .map(|s| s.split('?').next().unwrap_or(s))
        .unwrap_or_else(|| target.rsplit(['/', '\\']).next().unwrap_or(target));
    let mut out: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    if out.is_empty() {
        out.push_str("port");
    }
    out
}

fn transport_name(k: TransportKind) -> &'static str {
    match k {
        TransportKind::Serial => "serial",
        TransportKind::Pty => "pty",
        TransportKind::Sim => "sim",
    }
}

/// A pty slave this process's own sim harness is running ([`crate::sim::is_live_sim_pty`]).
#[cfg(unix)]
fn live_sim_pty(path: &str) -> bool {
    crate::sim::is_live_sim_pty(path)
}

#[cfg(not(unix))]
fn live_sim_pty(_path: &str) -> bool {
    false
}

/// `config` object → `PortConfig` (CLI names: `baud`, `data_bits`, `parity`, `stop_bits`, `flow`).
fn parse_config(v: Option<&Value>) -> Result<PortConfig, WorkerError> {
    let mut c = PortConfig::default();
    let Some(v) = v else {
        return Ok(c);
    };
    let bad = |k: &str, x: &Value| werr(ErrorCode::Usage, format!("bad config {k}: {x}"));
    if let Some(x) = v.get("baud") {
        c.baud = x
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| bad("baud", x))?;
    }
    if let Some(x) = v.get("data_bits") {
        c.data_bits = x
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| bad("data_bits", x))?;
    }
    if let Some(x) = v.get("parity") {
        c.parity = match x.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("none" | "n") => Parity::None,
            Some("odd" | "o") => Parity::Odd,
            Some("even" | "e") => Parity::Even,
            Some("mark" | "m") => Parity::Mark,
            Some("space" | "s") => Parity::Space,
            _ => return Err(bad("parity", x)),
        };
    }
    if let Some(x) = v.get("stop_bits") {
        let s = match x {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        c.stop = match s.as_str() {
            "1" => StopBits::One,
            "1.5" => StopBits::OnePointFive,
            "2" => StopBits::Two,
            _ => return Err(bad("stop_bits", x)),
        };
    }
    if let Some(x) = v.get("flow") {
        c.flow = match x.as_str() {
            Some("none") => FlowControl::None,
            Some("software" | "xonxoff") => FlowControl::Software,
            Some("hardware" | "rtscts") => FlowControl::Hardware,
            _ => return Err(bad("flow", x)),
        };
    }
    Ok(c)
}

fn config_json(c: &PortConfig) -> Value {
    json!({
        "baud": c.baud,
        "data_bits": c.data_bits,
        "parity": match c.parity {
            Parity::None => "none", Parity::Odd => "odd", Parity::Even => "even",
            Parity::Mark => "mark", Parity::Space => "space",
        },
        "stop_bits": match c.stop { StopBits::One => "1", StopBits::OnePointFive => "1.5", StopBits::Two => "2" },
        "flow": match c.flow {
            FlowControl::None => "none", FlowControl::Software => "software", FlowControl::Hardware => "hardware",
        },
    })
}

fn ports() -> Result<Value, WorkerError> {
    let list =
        transport::list_ports().map_err(|e| werr(ErrorCode::NotFound, format!("port list failed: {e}")))?;
    let ports: Vec<Value> = list
        .ports
        .iter()
        .map(|p| {
            json!({
                "path": p.path, "vid": p.vid, "pid": p.pid, "serial": p.serial,
                "manufacturer": p.manufacturer, "product": p.product,
                "interface": p.interface, "location": p.location,
            })
        })
        .collect();
    Ok(json!({"ports": ports, "truncated": list.truncated}))
}

impl Worker for UartWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move { self.call(cx, call).await })
    }

    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, write: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move { self.write(cx, write).await })
    }

    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move { Err(werr(ErrorCode::NotImplemented, "UART has no jobs in M1–M3")) })
    }

    fn on_input<'a>(&'a mut self, cx: &'a WorkerCtx, input: Input) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async move { self.input(cx, input).await })
    }

    fn on_latch(&mut self, on: bool) {
        // STOP: queued agent units fail `E_STOPPED`, the running one stops at the next chunk
        // boundary (`outcome_unknown` + exact `written_bytes`). New sends check `cx.latch()`.
        // User input is never latched (A3, contract 04 §4.4).
        if on && let Some(p) = self.port() {
            p.cancel_agents("stopped", ErrorCode::Stopped);
        }
    }

    fn on_input_lease_end<'a>(&'a mut self, _cx: &'a WorkerCtx, end: InputLeaseEnd) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.input_lease_end(end) })
    }
}

fn viewer_conn(call: &Call) -> Result<&str, WorkerError> {
    call.conn_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| werr(ErrorCode::Usage, "viewer arbitration requires conn_id"))
}

/// Run the UART device owner with the SDK's reserved input/latch callback slots.
/// CORE's `loomd worker uart` dispatch must call this entry point instead of `run_worker`.
pub fn run() -> ! {
    loom_worker_sdk::run_shared_worker(Arc::new(UartWorker::new()))
}

impl SharedWorker for UartWorker {
    const KIND: &'static str = WORKER_KIND;

    fn on_call(&self, cx: WorkerCtx, call: Call) -> BoxFuture<'static, Result<Value, WorkerError>> {
        let worker = self.clone();
        Box::pin(async move {
            if call.method == "uart.ports" {
                tokio::task::spawn_blocking(ports)
                    .await
                    .map_err(|e| werr(ErrorCode::Internal, format!("port enumeration failed: {e}")))?
            } else {
                worker.call(&cx, call).await
            }
        })
    }

    fn on_write(&self, cx: WorkerCtx, write: Write) -> BoxFuture<'static, WriteOutcome> {
        let worker = self.clone();
        Box::pin(async move {
            // Device open and thread joins must not block the concurrent callback executor.
            if matches!(arg_str(&write.op, "method"), Some("uart.open" | "uart.close")) {
                match tokio::task::spawn_blocking(move || {
                    outcome(match arg_str(&write.op, "method") {
                        Some("uart.open") => worker.open(&cx, &write.op),
                        _ => worker.close(&cx, &write.op),
                    })
                })
                .await
                {
                    Ok(result) => result,
                    Err(e) => outcome(Err(werr(
                        ErrorCode::Internal,
                        format!("port operation failed: {e}"),
                    ))),
                }
            } else {
                worker.write(&cx, write).await
            }
        })
    }

    fn on_job(&self, _cx: WorkerCtx, _job: JobCmd) -> BoxFuture<'static, Result<Value, WorkerError>> {
        Box::pin(async { Err(werr(ErrorCode::NotImplemented, "UART has no jobs in M1–M3")) })
    }

    fn on_input(&self, cx: WorkerCtx, input: Input) -> BoxFuture<'static, Result<(), WorkerError>> {
        let worker = self.clone();
        Box::pin(async move { worker.input(&cx, input).await })
    }

    fn on_latch(&self, on: bool) -> BoxFuture<'static, ()> {
        let mut worker = self.clone();
        Box::pin(async move { Worker::on_latch(&mut worker, on) })
    }

    fn on_input_lease_end(&self, _cx: WorkerCtx, end: InputLeaseEnd) -> BoxFuture<'static, ()> {
        let worker = self.clone();
        Box::pin(async move { worker.input_lease_end(end) })
    }
}
