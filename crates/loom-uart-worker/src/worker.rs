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
mod reconnect;
mod send;
mod signal;
mod util;

use std::collections::HashMap;
use std::path::PathBuf;
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
pub use reconnect::{
    PortIdentity, RECONNECT_POLL, RECONNECT_WINDOW_DEFAULT, RECONNECT_WINDOW_MAX, RECONNECT_WINDOW_MIN,
    ReconnectCfg, Reopener, SystemReopener,
};
pub use send::{DEFER_TIMEOUT_MS, MAX_WAIT_MS, RUN_TIMEOUT_MS, SEND_EXPECT_TIMEOUT_MS};
pub use signal::{BREAK_DEFAULT_MS, SIGNAL_CONTROL_TIMEOUT, SIGNAL_MAX_MS};

use port::{LogSlot, Port, PortInfo};
use reconnect::{ReopenStep, identity_json, parse_reconnect, reconnect_json, supervise};
use send::{
    ExpectWait, activity_expect, activity_idle, agent_write, channel_error, expect_finish, parse_patterns,
    submit_user, viewer_input,
};
use signal::{line_json, signal_write};
use util::{
    arg_str, arg_u64, ctx_clock, draft, env_name, kind, log_json, open_error, pacing_fields, pacing_json,
    parse_env, parse_log_cfg, parse_pacing, werr,
};

use crate::bridge::bridge;
use crate::channel::{ChannelCore, ExpectFrom, LinesQuery, Profile, ReadQuery};
use crate::clock::SeqSource;
use crate::logsink::LogCfg;
use crate::reader::Reader;
use crate::transport::{
    self, FlowControl, OpenOpts, Parity, PortConfig, SIM_SCHEME, StopBits, TransportKind,
};
use crate::tx::{Pacing, TxActor, event_channel};

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
    /// B3: reconnect identity/reopen seam (`None` = [`SystemReopener`]).
    reopener: Mutex<Option<Arc<dyn Reopener>>>,
    /// B4: native log sink of this worker's channel (handed to every `Port` it starts).
    log: LogSlot,
    /// B4: test seam for `$LOOM_HOME` (`None` = `loom_worker_sdk::transport::loom_home()`).
    log_home: Mutex<Option<PathBuf>>,
}

/// B3: everything needed to (re)start a port; built by `open_inner` and by `reopen_after_disconnect` from the
/// old port's [`PortInfo`] + live pacing / HOLD. See FUNCTIONS.md §B3.3 `spawn_port`.
#[derive(Debug, Clone)]
struct PortSpec {
    target: String,
    channel: String,
    config: PortConfig,
    profile_name: String,
    profile: Profile,
    env: Env,
    sim_pty: bool,
    pacing: Pacing,
    reconnect: ReconnectCfg,
    identity: Option<PortIdentity>,
    /// carry HOLD over to the new arbiter (reconnect only)
    hold: bool,
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

    /// Test seam (B3): how a disconnected port is re-identified and reopened (default [`SystemReopener`]).
    /// Set before `uart.open`. See FUNCTIONS.md §B3.3.
    pub fn set_reopener(&self, r: Arc<dyn Reopener>) {
        *lock(&self.inner.reopener) = Some(r);
    }

    /// B3: the injected reopener or [`SystemReopener`]. See §B3.3.
    fn reopener(&self) -> Arc<dyn Reopener> {
        lock(&self.inner.reopener)
            .clone()
            .unwrap_or_else(|| Arc::new(SystemReopener))
    }

    /// B3: start a port from `spec` on `transport` (kept core / generation, emitter, reader, TX actor, arbiter,
    /// `Port::new` + `start`, reconnect supervisor when enabled, `uart.state open` (+ `reopen` note) and
    /// `uart.config`). Caller holds the lifecycle lock and an empty port slot. See FUNCTIONS.md §B3.3.
    fn spawn_port(
        &self,
        cx: &WorkerCtx,
        spec: &PortSpec,
        transport: Box<dyn transport::Transport>,
        reopen: Option<Value>,
    ) -> Result<Arc<Port>, WorkerError> {
        let kind_ = transport.kind();
        let rx_handle = transport.rx_handle().map_err(|e| open_error(&e))?;

        // Ring/cursors survive close → reopen of the same channel (generation + 1).
        let (core, generation) = {
            let mut kept = lock(&self.inner.kept);
            match kept.remove(&spec.channel) {
                Some((core, last_gen)) => {
                    let g = last_gen + 1;
                    lock(&core).new_generation(g);
                    (core, g)
                }
                None => {
                    kept.clear();
                    (
                        Arc::new(Mutex::new(ChannelCore::new(
                            &spec.channel,
                            spec.profile.clone(),
                            1,
                        ))),
                        1,
                    )
                }
            }
        };
        let emitter = cx
            .emitter(&spec.channel)
            .with_generation(Some(generation), Some(spec.env));
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
            has_prompt: spec.profile.prompt.is_some(),
            line_kill_ctrl_u: spec.profile.line_kill_ctrl_u,
        });
        let info = PortInfo {
            channel: spec.channel.clone(),
            target: spec.target.clone(),
            generation,
            env: spec.env,
            profile: spec.profile_name.clone(),
            config_json: config_json(&spec.config),
            config: spec.config,
            clock_id: cx.clock().clock_id().to_owned(),
            sim_pty: spec.sim_pty,
            reconnect: spec.reconnect,
            identity: spec.identity.clone(),
        };
        let port = Arc::new(Port::new(
            info,
            core,
            arb,
            actor,
            emitter,
            Arc::clone(&self.inner.publication),
            spec.pacing,
            Arc::clone(&self.inner.log),
        ));
        port.start(reader, brx, erx).map_err(|e| open_error(&e))?;
        self.inner.opens.fetch_add(1, Ordering::Relaxed);

        if spec.hold {
            let now = port.now_ms();
            let (acts, evs) = {
                let mut a = port.arb();
                let acts = a.set_hold(true, now);
                (acts, a.take_events())
            };
            port.emit_arbiter(evs);
            port.dispatch(acts);
        }
        if spec.reconnect.enabled {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            port.set_disconnect_notify(tx);
            let (w, cx2, rc) = (self.clone(), cx.clone(), spec.reconnect);
            std::thread::Builder::new()
                .name("loom-uart-reconnect".into())
                .spawn(move || supervise(w, cx2, rx, generation, rc))
                .map_err(|e| open_error(&e))?;
        }

        let ts = port.ts_now(TsSource::HostReadComplete);
        let mut opened =
            json!({"state": "open", "path": port.info.target, "transport": transport_name(kind_)});
        if let Some(r) = reopen {
            opened["reopen"] = r;
        }
        port.emit(draft(kind::STATE, ts.clone(), Origin::Device, opened));
        let mut cfg = port.info.config_json.clone();
        cfg["profile"] = json!(port.info.profile);
        cfg["env"] = json!(env_name(spec.env));
        cfg["pacing"] = pacing_json(port.pacing());
        port.emit(draft(kind::CONFIG, ts, Origin::Device, cfg));
        Ok(port)
    }

    /// B3: one reconnect attempt for the disconnected port of generation `expect_gen`. See FUNCTIONS.md §B3.3.
    fn reopen_after_disconnect(
        &self,
        cx: &WorkerCtx,
        expect_gen: u32,
        attempt: u32,
        after_ms: u64,
    ) -> ReopenStep {
        let _lifecycle = lock(&self.inner.lifecycle);
        let mut slot = lock(&self.inner.port);
        let Some(old) = slot.as_ref().cloned() else {
            return ReopenStep::Gone;
        };
        if old.info.generation != expect_gen || old.connected.load(Ordering::Acquire) {
            return ReopenStep::Gone;
        }
        let info = &old.info;
        let reopener = self.reopener();
        let found = reopener.identity(&info.target);
        if let Some(exp) = &info.identity {
            match found {
                None => return ReopenStep::NotYet("device absent".into()),
                Some(f) if !exp.matches(&f) => {
                    return ReopenStep::IdentityMismatch {
                        expected: exp.clone(),
                        found: f,
                    };
                }
                Some(_) => {}
            }
        }
        let Some(profile) = Profile::named(&info.profile) else {
            return ReopenStep::Gone;
        };
        let opts = OpenOpts {
            sim_pty: info.sim_pty,
            ..OpenOpts::default()
        };
        let transport = match reopener.open(&info.target, &info.config, &opts) {
            Ok(t) => t,
            Err(e) => return ReopenStep::NotYet(e.to_string()),
        };
        // Take the old port out without a `closed` state event: this is a reconnect, not a close.
        let Some(old) = slot.take() else {
            return ReopenStep::Gone;
        };
        old.cancel_agents("closed", ErrorCode::NotOpen);
        let hold = old.arb().hold_on();
        let pacing = old.pacing();
        old.shutdown();
        {
            let mut kept = lock(&self.inner.kept);
            if kept.len() >= MAX_KEPT_CHANNELS {
                kept.clear();
            }
            kept.insert(
                old.info.channel.clone(),
                (Arc::clone(&old.core), old.info.generation),
            );
        }
        let spec = PortSpec {
            target: old.info.target.clone(),
            channel: old.info.channel.clone(),
            config: old.info.config,
            profile_name: old.info.profile.clone(),
            profile,
            env: old.info.env,
            sim_pty: old.info.sim_pty,
            pacing,
            reconnect: old.info.reconnect,
            identity: old.info.identity.clone(),
            hold,
        };
        let reopen = json!({
            "attempt": attempt,
            "after_ms": after_ms,
            "identity": if old.info.identity.is_some() { "usb" } else { "path_only" },
            "previous_generation": expect_gen,
        });
        match self.spawn_port(cx, &spec, transport, Some(reopen)) {
            Ok(port) => {
                let generation = port.info.generation;
                *slot = Some(port);
                ReopenStep::Reopened(generation)
            }
            Err(_) => ReopenStep::Gone,
        }
    }

    /// Test seam (B4): directory used instead of `$LOOM_HOME` for native logs. See FUNCTIONS.md §B4.2.
    pub fn set_log_home(&self, home: PathBuf) {
        *lock(&self.inner.log_home) = Some(home);
    }

    /// B4 `uart.log.start{channel, format?, rotate_mib?, quota_mib?}` (also used by `uart.open{log}`). See §B4.2.
    fn log_start(&self, args: &Value, cfg: LogCfg) -> Result<Value, WorkerError> {
        use crate::logsink::{LogHeader, LogSink, LogStartError, log_dir};
        let port = self.port_for(args)?;
        {
            let slot = lock(&port.log);
            if let Some((sink, cur)) = slot.as_ref() {
                let st = sink.stats();
                if st.active {
                    if *cur == cfg {
                        return Ok(
                            json!({"channel": port.info.channel, "log": log_json(&st, Some(cur)), "already": true}),
                        );
                    }
                    let file = st
                        .file
                        .as_deref()
                        .map(|f| f.display().to_string())
                        .unwrap_or_default();
                    return Err(werr(
                        ErrorCode::Config,
                        format!("already logging ({file}); stop it first"),
                    ));
                }
            }
        }
        let home = lock(&self.inner.log_home)
            .clone()
            .unwrap_or_else(loom_worker_sdk::transport::loom_home);
        let (producer_id, producer_epoch) = port.producer();
        let header = LogHeader {
            channel: port.info.channel.clone(),
            target: port.info.target.clone(),
            producer_id,
            producer_epoch,
            generation: port.info.generation,
            clock_id: port.info.clock_id.clone(),
        };
        let wall_now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        // The sink starts outside the slot lock: the RX/TX pumps lock the slot and must not wait for disk IO.
        let sink = match LogSink::start(&log_dir(&home, &port.info.channel), header, cfg, wall_now) {
            Ok(s) => s,
            Err(
                e @ LogStartError::Quota {
                    dir_bytes,
                    quota_bytes,
                },
            ) => {
                return Err(util::werr_detail(
                    ErrorCode::Limit,
                    e.to_string(),
                    json!({"dir_bytes": dir_bytes, "quota_bytes": quota_bytes}),
                ));
            }
            Err(e) => return Err(werr(ErrorCode::Internal, e.to_string())),
        };
        let stats = {
            let mut slot = lock(&port.log);
            if slot.as_ref().is_some_and(|(s, _)| s.stats().active) {
                drop(slot);
                let _ = sink.stop("closed");
                return Err(werr(ErrorCode::Config, "already logging; stop it first"));
            }
            let stats = sink.stats();
            *slot = Some((sink, cfg));
            stats
        };
        let mut payload = log_json(&stats, Some(&cfg));
        payload["state"] = json!("started");
        payload["provenance"] = json!("native-worker-log");
        port.emit(draft(
            kind::LOG,
            port.ts_now(TsSource::HostReadComplete),
            Origin::Device,
            payload,
        ));
        Ok(json!({"channel": port.info.channel, "log": log_json(&stats, Some(&cfg))}))
    }

    /// B4 `uart.log.stop{channel}` and the close path (`reason` `user` | `closed`). See §B4.2.
    fn log_stop(&self, args: &Value, reason: &str) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let taken = {
            let mut slot = lock(&port.log);
            if slot.as_ref().is_some_and(|(s, _)| s.stats().active) {
                slot.take()
            } else {
                None
            }
        };
        let Some((sink, cfg)) = taken else {
            let slot = lock(&port.log);
            let log = match slot.as_ref() {
                Some((s, c)) => log_json(&s.stats(), Some(c)),
                None => log_json(&crate::logsink::LogStats::default(), None),
            };
            return Ok(json!({"channel": port.info.channel, "stopped": false, "log": log}));
        };
        let fin = sink.stop(reason);
        let mut payload = log_json(&fin, Some(&cfg));
        payload["state"] = json!("stopped");
        payload["reason"] = json!(reason);
        port.emit(draft(
            kind::LOG,
            port.ts_now(TsSource::HostReadComplete),
            Origin::Device,
            payload,
        ));
        Ok(json!({"channel": port.info.channel, "stopped": true, "log": log_json(&fin, Some(&cfg))}))
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
            "uart.pacing" => self.set_pacing(args),
            "uart.log.start" => self.log_start(args, parse_log_cfg(args, true)?),
            "uart.log.stop" => self.log_stop(args, "user"),
            // Request writes normally arrive through `on_write`; accept them here too.
            "uart.open" | "uart.close" | "uart.send" | "uart.run" | "uart.signal" => Err(werr(
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
            "uart.signal" => match self.port_for(&w.op) {
                Ok(port) => signal_write(port, cx, w).await,
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
        let pacing = parse_pacing(op.get("pacing"))?;
        let reconnect = parse_reconnect(op.get("reconnect"))?;
        let log = op.get("log").map(|v| parse_log_cfg(v, false)).transpose()?;
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
                if !pacing.is_none_or(|x| x == p.pacing()) {
                    return Err(werr(
                        ErrorCode::Config,
                        format!(
                            "worker already owns '{}' with other pacing; use uart.pacing or close it first",
                            p.info.target
                        ),
                    ));
                }
                if !reconnect.is_none_or(|x| x == p.info.reconnect) {
                    return Err(werr(
                        ErrorCode::Config,
                        format!(
                            "worker already owns '{}' with other reconnect setting; close it first",
                            p.info.target
                        ),
                    ));
                }
                return Ok(open_result(&p.info, p.pacing()));
            }
            return Err(werr(
                ErrorCode::Config,
                format!(
                    "worker already owns '{}' with other settings; close it first",
                    p.info.target
                ),
            ));
        }

        let reconnect = reconnect.unwrap_or(ReconnectCfg::OFF);
        // No port enumeration on ordinary opens: identity is only recorded when reconnect is on.
        let identity = if reconnect.enabled {
            self.reopener().identity(&target).filter(PortIdentity::is_known)
        } else {
            None
        };
        let opts = OpenOpts {
            sim_pty: sim_issued_pty,
            ..OpenOpts::default()
        };
        let transport = match injected {
            Some(t) => t,
            None => transport::open(&target, &config, &opts).map_err(|e| open_error(&e))?,
        };
        let spec = PortSpec {
            target,
            channel,
            config,
            profile_name,
            profile,
            env,
            sim_pty: sim_issued_pty,
            pacing: pacing.unwrap_or(Pacing::NONE),
            reconnect,
            identity,
            hold: false,
        };
        let port = self.spawn_port(cx, &spec, transport, None)?;
        let mut result = open_result(&port.info, port.pacing());
        *slot = Some(port);
        // The sink is started through `log_start`, which looks the port up in the slot: release it first.
        drop(slot);
        if let Some(cfg) = log
            && let Err(e) = self.log_start(op, cfg)
        {
            result["log_error"] = json!({"code": e.code.as_str(), "message": e.message});
        }
        Ok(result)
    }

    fn close(&self, _cx: &WorkerCtx, op: &Value) -> Result<Value, WorkerError> {
        let _lifecycle = lock(&self.inner.lifecycle);
        // Stop an active native log first (reason `closed`), while the port can still emit its event.
        let _ = self.log_stop(op, "closed");
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

    /// B2 `uart.pacing{channel, char_delay_ms?, line_delay_ms?}`: change the port pacing.
    /// See FUNCTIONS.md §B2.4 `set_pacing`.
    fn set_pacing(&self, args: &Value) -> Result<Value, WorkerError> {
        let port = self.port_for(args)?;
        let new = pacing_fields(args, port.pacing())?;
        let old = port.set_pacing(new);
        if old != new {
            let mut cfg = port.info.config_json.clone();
            cfg["profile"] = json!(port.info.profile);
            cfg["env"] = json!(env_name(port.info.env));
            cfg["pacing"] = pacing_json(new);
            port.emit(draft(
                kind::CONFIG,
                port.ts_now(TsSource::HostReadComplete),
                Origin::Device,
                cfg,
            ));
        }
        Ok(json!({
            "channel": port.info.channel,
            "generation": port.info.generation,
            "pacing": pacing_json(new),
            "changed": old != new,
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
            let log = match lock(&p.log).as_ref() {
                Some((s, c)) => log_json(&s.stats(), Some(c)),
                None => log_json(&crate::logsink::LogStats::default(), None),
            };
            let extra = json!({
                "open": true,
                "log": log,
                "connected": p.connected.load(Ordering::Acquire),
                "channel": p.info.channel, "path": p.info.target, "generation": p.info.generation,
                "env": env_name(p.info.env), "profile": p.info.profile, "config": p.info.config_json,
                "pacing": pacing_json(p.pacing()),
                "lines": line_json(*lock(&p.lines)),
                "reconnect": reconnect_json(p.info.reconnect, *lock(&p.reconnect)),
                "identity": p.info.identity.as_ref().map(identity_json),
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

/// Open result; B2 adds `pacing` (FUNCTIONS.md §B2.4).
fn open_result(info: &PortInfo, pacing: Pacing) -> Value {
    json!({
        "channel": info.channel, "generation": info.generation, "path": info.target,
        "config": info.config_json, "profile": info.profile, "env": env_name(info.env),
        "pacing": pacing_json(pacing),
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
