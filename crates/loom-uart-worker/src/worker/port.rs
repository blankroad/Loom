//! One open port: device threads, shared state and the event pumps (UART plan §3.1, §3.5).
//!
//! Threads per open port (all joined by [`Port::shutdown`]):
//! * reader (P0 [`crate::reader`]) → bridge,
//! * pump: bridge → [`ChannelCore::on_rx`] → `uart.data` rx; expect hits; arbiter hints and
//!   the arbiter tick every [`ARB_TICK`],
//! * TX actor (P0 [`crate::tx`]),
//! * TX event pump: chunks → `uart.data` tx; `TxDone` → waiting writes, `agent_done`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine as _;
use loom_console::arbiter::{InputArbiter, TickAction};
use loom_console::matcher::MatchHit;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, TsSource, kinds};
use loom_worker_sdk::Emitter;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use super::util::{arbiter_draft, draft, kind, ts_at, tx_data_draft};
use crate::bridge::{BridgeItem, BridgeRx, PopError, RxChunk};
use crate::channel::ChannelCore;
use crate::reader::Reader;
use crate::transport::PortConfig;
use crate::tx::{TxActor, TxDone, TxEvent, TxHandle, TxOrigin};

/// Arbiter tick period (task p1-opus-worker: 50 ms) and the pump's bridge wait.
pub const ARB_TICK: Duration = Duration::from_millis(50);
/// Bridge items taken per pump wakeup (one lock per batch, §3.13).
pub const PUMP_BATCH: usize = 64;
/// Concurrent expect waiters per channel (= matcher `max_waiters`, §3.13 expect row).
/// Extra waiters are refused before registration (`E_RATE_LIMIT`).
pub const MAX_EXPECT_WAITERS: usize = 8;
/// Deferred agent sends waiting for admission (= arbiter `AGENT_PENDING_MAX`, §3.13 TX row).
pub const MAX_ADMIT_WAITERS: usize = loom_console::arbiter::AGENT_PENDING_MAX;
/// TX requests tracked until `TxDone` (3 lanes × 64 items, §3.13 TX row).
pub const MAX_TX_WAITERS: usize = 3 * 64;
/// RX coalescing window before `on_rx` (lead: ~10 ms; plan §2.0② worker batch ≤ 10 ms).
pub const RX_COALESCE: Duration = Duration::from_millis(10);
/// Largest coalesced RX chunk (= matcher window 64 KiB, §3.13 expect row). Bounded buffer.
pub const RX_COALESCE_MAX_BYTES: usize = 64 * 1024;

/// Bookkeeping for one TX request until its `TxDone`.
#[derive(Debug)]
pub(crate) struct TxWait {
    /// Who wants the result (agent writes); `None` for viewer input.
    pub(crate) done: Option<oneshot::Sender<TxDone>>,
    /// Agent request id (string) whose lease ends with this request.
    pub(crate) agent_request: Option<String>,
    /// Origin stamped on `uart.data` tx rows.
    pub(crate) origin: Origin,
    /// The arbiter counted this as an interrupt (call `interrupt_written`).
    pub(crate) interrupt: bool,
}

/// Waiting async callers, all bounded.
#[derive(Debug, Default)]
pub(crate) struct Waiters {
    pub(crate) expect: HashMap<u64, oneshot::Sender<MatchHit>>,
    pub(crate) admit: HashMap<String, oneshot::Sender<TickAction>>,
    pub(crate) tx: HashMap<u64, TxWait>,
}

/// Static facts of the open session.
#[derive(Debug, Clone)]
pub(crate) struct PortInfo {
    pub(crate) channel: String,
    pub(crate) target: String,
    pub(crate) generation: u32,
    pub(crate) env: Env,
    pub(crate) profile: String,
    pub(crate) config: PortConfig,
    pub(crate) config_json: Value,
    pub(crate) clock_id: String,
}

#[derive(Debug, Default)]
struct Threads {
    reader: Option<Reader>,
    actor: Option<TxActor>,
    pump: Option<JoinHandle<()>>,
    tx_pump: Option<JoinHandle<()>>,
}

/// Emission failures (queue full / closed); never block the hot path (SDK W7).
#[derive(Debug, Default)]
pub(crate) struct EmitStats {
    pub(crate) rx_dropped: AtomicU64,
    pub(crate) tx_dropped: AtomicU64,
    pub(crate) ctl_dropped: AtomicU64,
}

/// An open port.
#[derive(Debug)]
pub(crate) struct Port {
    pub(crate) info: PortInfo,
    pub(crate) core: Arc<Mutex<ChannelCore>>,
    pub(crate) arb: Mutex<InputArbiter>,
    pub(crate) waiters: Mutex<Waiters>,
    pub(crate) tx: TxHandle,
    emitter: Emitter,
    // One in-flight reservation, no queue (§3.13). Covers ring stamping and all UART output;
    // no thread may publish a higher producer seq between reserve_seq and data_with_seq.
    publication: Arc<Mutex<()>>,
    pub(crate) emit_stats: EmitStats,
    pub(crate) connected: AtomicBool,
    next_tx_id: AtomicU64,
    t0: Instant,
    stop: AtomicBool,
    threads: Mutex<Threads>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Port {
    pub(crate) fn new(
        info: PortInfo,
        core: Arc<Mutex<ChannelCore>>,
        arb: InputArbiter,
        actor: TxActor,
        emitter: Emitter,
        publication: Arc<Mutex<()>>,
    ) -> Self {
        let tx = actor.handle();
        Self {
            info,
            core,
            arb: Mutex::new(arb),
            waiters: Mutex::new(Waiters::default()),
            tx,
            emitter,
            publication,
            emit_stats: EmitStats::default(),
            connected: AtomicBool::new(true),
            next_tx_id: AtomicU64::new(1),
            t0: Instant::now(),
            stop: AtomicBool::new(false),
            threads: Mutex::new(Threads {
                actor: Some(actor),
                ..Threads::default()
            }),
        }
    }

    /// Start the pumps (after the `Arc` exists).
    pub(crate) fn start(
        self: &Arc<Self>,
        reader: Reader,
        bridge: BridgeRx,
        tx_events: Receiver<TxEvent>,
    ) -> std::io::Result<()> {
        let p = Arc::clone(self);
        let pump = std::thread::Builder::new()
            .name("loom-uart-pump".into())
            .spawn(move || p.pump_loop(&bridge))?;
        let p = Arc::clone(self);
        let tx_pump = std::thread::Builder::new()
            .name("loom-uart-txev".into())
            .spawn(move || p.tx_loop(&tx_events))?;
        let mut t = lock(&self.threads);
        t.reader = Some(reader);
        t.pump = Some(pump);
        t.tx_pump = Some(tx_pump);
        Ok(())
    }

    /// Arbiter time base (ms since open).
    pub(crate) fn now_ms(&self) -> u64 {
        u64::try_from(self.t0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) fn core(&self) -> MutexGuard<'_, ChannelCore> {
        lock(&self.core)
    }

    pub(crate) fn arb(&self) -> MutexGuard<'_, InputArbiter> {
        lock(&self.arb)
    }

    pub(crate) fn waiters(&self) -> MutexGuard<'_, Waiters> {
        lock(&self.waiters)
    }

    pub(crate) fn next_tx_id(&self) -> u64 {
        self.next_tx_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Timestamp "now" in the SDK clock domain.
    pub(crate) fn ts_now(&self, source: TsSource) -> loom_proto::event::Ts {
        let c = self.emitter.clock();
        ts_at(c.mono_ns(), c.wall_ns(), &self.info.clock_id, source)
    }

    /// Emit a priority draft (counts failures, never blocks).
    pub(crate) fn emit(&self, mut d: loom_worker_sdk::EventDraft) {
        if d.generation.is_none() {
            d.generation = Some(self.info.generation);
        }
        if d.env.is_none() {
            d.env = Some(self.info.env);
        }
        let _publication = lock(&self.publication);
        if self.emitter.priority(d).is_err() {
            self.emit_stats.ctl_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Emit pending arbiter events (call after every arbiter call; arbiter lock not held).
    pub(crate) fn emit_arbiter(&self, events: Vec<loom_console::arbiter::ArbiterEvent>) {
        for ev in &events {
            self.emit(arbiter_draft(
                ev,
                self.ts_now(TsSource::HostReadComplete),
                self.info.generation,
            ));
        }
    }

    /// Hand admission results to the waiting writes. An `AgentReady` nobody waits for any more
    /// (caller gone) releases the lease at once so the channel does not stay agent-owned.
    pub(crate) fn dispatch(&self, mut actions: Vec<TickAction>) {
        while !actions.is_empty() {
            let mut orphans = Vec::new();
            {
                let mut w = self.waiters();
                for a in actions.drain(..) {
                    let rid = match &a {
                        TickAction::AgentReady { request_id, .. }
                        | TickAction::AgentRejected { request_id, .. } => request_id.clone(),
                    };
                    let ready = matches!(a, TickAction::AgentReady { .. });
                    let delivered = match w.admit.remove(&rid) {
                        Some(tx) => tx.send(a).is_ok(),
                        None => false,
                    };
                    if ready && !delivered {
                        orphans.push(rid);
                    }
                }
            }
            for rid in orphans {
                let now = self.now_ms();
                let (more, evs) = {
                    let mut a = self.arb();
                    let more = a.agent_done(&rid, now);
                    (more, a.take_events())
                };
                self.emit_arbiter(evs);
                actions.extend(more);
            }
        }
    }

    /// Periodic arbiter step.
    pub(crate) fn tick(&self) {
        let now = self.now_ms();
        let (acts, evs) = {
            let mut a = self.arb();
            let acts = a.tick(now);
            (acts, a.take_events())
        };
        self.emit_arbiter(evs);
        self.dispatch(acts);
    }

    /// Pump: RX is coalesced for up to [`RX_COALESCE`] (contiguous chunks only, ≤
    /// [`RX_COALESCE_MAX_BYTES`]) before `on_rx`, so regex waiters re-scan their window once
    /// per window instead of once per small chunk. Loss / disconnect items flush first and keep
    /// stream order.
    fn pump_loop(&self, bridge: &BridgeRx) {
        let mut batch = Vec::with_capacity(PUMP_BATCH);
        let mut last_tick = Instant::now();
        let mut pending: Option<RxChunk> = None;
        let mut window_start: Option<Instant> = None;
        while !self.stop.load(Ordering::Acquire) {
            let wait = match window_start {
                Some(t) => RX_COALESCE
                    .saturating_sub(t.elapsed())
                    .max(Duration::from_millis(1)),
                None => ARB_TICK,
            };
            batch.clear();
            match bridge.pop_batch(&mut batch, PUMP_BATCH, wait) {
                Ok(_) => {
                    for item in batch.drain(..) {
                        match item {
                            BridgeItem::Data(c) => {
                                let joins = pending.as_ref().is_some_and(|p| {
                                    p.rx_offset + p.bytes.len() as u64 == c.rx_offset
                                        && p.bytes.len() + c.bytes.len() <= RX_COALESCE_MAX_BYTES
                                });
                                if joins {
                                    if let Some(p) = pending.as_mut() {
                                        p.bytes.extend_from_slice(&c.bytes);
                                    }
                                } else {
                                    if let Some(p) = pending.take() {
                                        self.ingest(BridgeItem::Data(p));
                                    }
                                    window_start = Some(Instant::now());
                                    pending = Some(c);
                                }
                            }
                            other => {
                                if let Some(p) = pending.take() {
                                    self.ingest(BridgeItem::Data(p));
                                }
                                window_start = None;
                                self.ingest(other);
                            }
                        }
                    }
                }
                Err(PopError::Timeout) => {}
                Err(PopError::Closed) => std::thread::sleep(if pending.is_some() {
                    Duration::ZERO
                } else {
                    ARB_TICK
                }),
            }
            if window_start.is_some_and(|t| t.elapsed() >= RX_COALESCE) {
                if let Some(p) = pending.take() {
                    self.ingest(BridgeItem::Data(p));
                }
                window_start = None;
            }
            if last_tick.elapsed() >= ARB_TICK {
                last_tick = Instant::now();
                self.tick();
            }
        }
        if let Some(p) = pending.take() {
            self.ingest(BridgeItem::Data(p));
        }
    }

    fn ingest(&self, item: BridgeItem) {
        match item {
            BridgeItem::Data(mut c) => {
                // Reader sequence numbers describe transport hand-off only. Reserve the real
                // producer seq after coalescing so the ring and emitted record agree exactly.
                let ing = {
                    let _publication = lock(&self.publication);
                    let Ok(seq) = self.emitter.reserve_seq() else {
                        self.emit_stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                        return;
                    };
                    c.source_seq = seq.0;
                    let mut ing = self.core().on_rx(&c);
                    let ts = ts_at(
                        c.mono_ns,
                        c.wall_ns,
                        &self.info.clock_id,
                        TsSource::HostReadComplete,
                    );
                    let payload = std::mem::take(&mut ing.data_payload);
                    if self.emitter.rx_with_seq(seq, kind::DATA, ts, payload).is_err() {
                        // A Full result may follow journal acceptance. Never retry this seq.
                        self.emit_stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    ing
                };
                if !ing.hits.is_empty() {
                    let mut w = self.waiters();
                    for hit in ing.hits {
                        if let Some(tx) = w.expect.remove(&hit.id) {
                            let _ = tx.send(hit);
                        }
                    }
                }
                if let Some(alt) = ing.alt_screen {
                    self.arb().set_alt_screen(alt);
                }
                if ing.prompt_seen {
                    let now = self.now_ms();
                    let (acts, evs) = {
                        let mut a = self.arb();
                        let acts = a.prompt_seen(now);
                        (acts, a.take_events())
                    };
                    self.emit_arbiter(evs);
                    self.dispatch(acts);
                }
            }
            BridgeItem::Lost(l) => {
                // Reader bridge overflow: both the record and the screen lost it (DR-V14).
                // These bytes were dropped before any Emitter reservation. Their reader
                // chunk ids must never masquerade as this producer's event sequence range.
                let payload = self.core().on_lost_unsequenced(&l);
                for k in [kinds::RECORD_GAP, kinds::DISPLAY_GAP] {
                    self.emit(draft(
                        k,
                        self.ts_now(TsSource::HostReadComplete),
                        Origin::Device,
                        payload.clone(),
                    ));
                }
            }
            BridgeItem::Disconnected(kind_) => {
                self.connected.store(false, Ordering::Release);
                self.emit(draft(
                    kind::STATE,
                    self.ts_now(TsSource::HostReadComplete),
                    Origin::Device,
                    json!({"state": "disconnected", "path": self.info.target, "cause": format!("{kind_:?}")}),
                ));
            }
        }
    }

    fn tx_loop(&self, events: &Receiver<TxEvent>) {
        // Ends when the TX actor thread exits and drops its sender.
        while let Ok(ev) = events.recv() {
            match ev {
                TxEvent::Chunk(mut c) => {
                    let (rid, origin) = {
                        let w = self.waiters();
                        match w.tx.get(&c.request_id) {
                            Some(t) => (t.agent_request.clone(), t.origin),
                            None => (
                                None,
                                match c.origin {
                                    TxOrigin::User => Origin::User,
                                    TxOrigin::Agent => Origin::Agent,
                                },
                            ),
                        }
                    };
                    let wall = self.emitter.clock().wall_ns();
                    let ts = ts_at(c.mono_ns, wall, &self.info.clock_id, TsSource::Driver);
                    let _publication = lock(&self.publication);
                    let Ok(seq) = self.emitter.reserve_seq() else {
                        self.emit_stats.tx_dropped.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    c.source_seq = seq.0;
                    let payload = self.core().on_tx(&c);
                    let data = tx_data_draft(payload, ts, origin, &c.client_id, rid, self.info.generation);
                    if self.emitter.data_with_seq(seq, data).is_err() {
                        self.emit_stats.tx_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
                TxEvent::Done(d) => {
                    let wait = self.waiters().tx.remove(&d.request_id);
                    let Some(wait) = wait else { continue };
                    if wait.interrupt {
                        self.arb().interrupt_written(1);
                    }
                    if let Some(rid) = &wait.agent_request {
                        let now = self.now_ms();
                        let (acts, evs) = {
                            let mut a = self.arb();
                            let acts = a.agent_done(rid, now);
                            (acts, a.take_events())
                        };
                        self.emit_arbiter(evs);
                        self.dispatch(acts);
                    }
                    if let Some(tx) = wait.done {
                        let _ = tx.send(d);
                    }
                }
            }
        }
    }

    /// Stop every thread (each ends within ~ one [`ARB_TICK`] / reader wait) and fail waiters.
    /// Unfinished TX requests end `E_SHUTDOWN` → their writes report `outcome_unknown`/`failed`.
    pub(crate) fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        let (reader, actor, pump, tx_pump) = {
            let mut t = lock(&self.threads);
            (t.reader.take(), t.actor.take(), t.pump.take(), t.tx_pump.take())
        };
        if let Some(r) = reader {
            let _ = r.stop();
        }
        if let Some(a) = actor {
            let _ = a.shutdown();
        }
        if let Some(h) = tx_pump {
            let _ = h.join();
        }
        if let Some(h) = pump {
            let _ = h.join();
        }
        // Deferred sends and expects can no longer complete.
        let (admit, expect) = {
            let mut w = self.waiters();
            (std::mem::take(&mut w.admit), std::mem::take(&mut w.expect))
        };
        for (rid, tx) in admit {
            let _ = tx.send(TickAction::AgentRejected {
                request_id: rid,
                code: ErrorCode::NotOpen,
                detail: json!({"reason": "closed"}),
            });
        }
        drop(expect); // receivers see a closed channel → E_NOT_OPEN
    }

    /// Cancel every queued or running agent TX request (`stopped` / `held_by_user` / `closed`).
    /// Queued ones end `failed` with 0 bytes, the running one `outcome_unknown` at the next 1 KiB
    /// chunk boundary with the exact `written_bytes` (168ac1c). User and Ctrl-C lanes are
    /// untouched (contract 04 §4.4). Also fails sends still waiting for admission.
    pub(crate) fn cancel_agents(&self, cause: &str, code: ErrorCode) -> usize {
        let n = self.tx.cancel_lane(crate::tx::TxLane::Agent, cause);
        let admit = std::mem::take(&mut self.waiters().admit);
        for (rid, tx) in admit {
            let _ = tx.send(TickAction::AgentRejected {
                request_id: rid,
                code,
                detail: json!({ "cause": cause }),
            });
        }
        n
    }

    /// Threads still alive (tests: no leak after close).
    pub(crate) fn live_threads(&self) -> usize {
        let t = lock(&self.threads);
        usize::from(t.reader.as_ref().is_some_and(|r| !r.is_finished()))
            + usize::from(t.actor.is_some())
            + usize::from(t.pump.as_ref().is_some_and(|h| !h.is_finished()))
            + usize::from(t.tx_pump.as_ref().is_some_and(|h| !h.is_finished()))
    }
}

/// `uart.data`-style bytes object.
pub(crate) fn bytes_obj(b: &[u8]) -> Value {
    json!({"encoding": "base64", "data": base64::engine::general_purpose::STANDARD.encode(b)})
}
