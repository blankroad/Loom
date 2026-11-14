//! Ethernet worker: one channel, one capture owner, bounded decode/publication and send paths.
mod packet;
mod pipeline;
mod shared;
mod source;
mod tcp;
#[cfg(test)]
mod tests;

use crate::model::*;
use loom_eth_capture::{BoundedFrameQueue, CaptureConfig, CaptureLoop, DropStats, QueueConfig, StopHandle};
use loom_eth_filter::Filter;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, WriteState};
use loom_proto::worker::{Call, Input, WorkerError, Write};
use loom_worker_sdk::{BoxFuture, JobCmd, RateGuard, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};
use shared::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Hard publication ceiling; reject configurations above this (interest bypass is explicit).
pub const MAX_PUBLICATION_PPS: u32 = 20_000;
/// RateGuard default transmit rate; reject higher worker-local rates until policy wiring exists.
pub const MAX_SEND_PPS: u32 = 1000;
/// Simulator cadence range; reject out-of-range periods/jitter, never busy-spin.
pub const MAX_SIM_PERIOD_MS: u64 = 60_000;
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub publication_per_sec: u32,
    pub live_environment: Env,
    pub send_per_sec: u32,
    pub capture_queue: QueueConfig,
    pub sim_sd_period_ms: u64,
    pub sim_jitter_ms: u64,
    /// Diagnostic simulator option: exercise honest unknown native drop counters.
    pub sim_stats_unavailable: bool,
}
impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            publication_per_sec: 5000,
            live_environment: Env::Bench,
            send_per_sec: 1000,
            capture_queue: QueueConfig::default(),
            sim_sd_period_ms: 1000,
            sim_jitter_ms: 0,
            sim_stats_unavailable: false,
        }
    }
}
impl WorkerConfig {
    fn validate(&self) -> Result<(), WorkerError> {
        if !(1..=MAX_PUBLICATION_PPS).contains(&self.publication_per_sec)
            || !(1..=MAX_SEND_PPS).contains(&self.send_per_sec)
            || !(10..=MAX_SIM_PERIOD_MS).contains(&self.sim_sd_period_ms)
            || self.sim_jitter_ms >= self.sim_sd_period_ms
        {
            return Err(error(
                ErrorCode::Config,
                "publication/send rate or sim period/jitter outside bounds",
            ));
        }
        if self.capture_queue.max_frames == 0
            || self.capture_queue.max_frames > loom_eth_capture::MAX_QUEUE_FRAMES
            || self.capture_queue.max_bytes > loom_eth_capture::MAX_QUEUE_BYTES
        {
            return Err(error(ErrorCode::Config, "capture queue limits"));
        }
        Ok(())
    }
}
#[derive(Debug)]
pub(crate) struct Shared {
    state: Mutex<Option<CaptureStatePayload>>,
    metrics: Metrics,
    interests: Mutex<Vec<Filter>>,
    recent: Mutex<RecentTable>,
    drop_stats: Mutex<DropStats>,
    next_capture_seq: AtomicU64,
    control_tasks: Arc<AtomicUsize>,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            state: Mutex::new(None),
            metrics: Metrics::default(),
            interests: Mutex::new(Vec::new()),
            recent: Mutex::new(RecentTable::default()),
            drop_stats: Mutex::new(DropStats::ZERO),
            next_capture_seq: AtomicU64::new(1),
            control_tasks: Arc::new(AtomicUsize::new(0)),
        }
    }
}
#[derive(Debug)]
struct Running {
    commands: SyncSender<source::Command>,
    budget: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    capture_stop: StopHandle,
    queue: BoundedFrameQueue,
    thread: Option<JoinHandle<Result<(), WorkerError>>>,
    emitter: loom_worker_sdk::Emitter,
}
impl Running {
    fn open(
        params: CaptureStartParams,
        cx: WorkerCtx,
        shared: Arc<Shared>,
        config: WorkerConfig,
        generation: u32,
    ) -> Result<Self, WorkerError> {
        let ch = params.ch.as_deref().unwrap_or(&params.iface).to_owned();
        let env = if params.backend == "live" {
            config.live_environment
        } else {
            Env::Sim
        };
        let em = cx.emitter(&ch).with_generation(Some(generation), Some(env));
        let (tx, rx) = source::channel();
        let mut source = source::Source::open(&params, rx, cx.clone(), shared.clone(), config.clone())?;
        use loom_eth_capture::CaptureSource;
        let iface = source.interface();
        let stats = source.stats();
        *lock(&shared.drop_stats) = stats;
        let capture = CaptureLoop::start(
            source,
            CaptureConfig {
                queue: config.capture_queue,
                first_seq: shared.next_capture_seq.load(Ordering::Acquire),
            },
        )
        .map_err(capture_error)?;
        let queue = capture.queue();
        let capture_stop = capture.stop_handle();
        if let Some(s) = lock(&shared.state).as_mut() {
            s.state = CaptureStateKind::Live;
            s.linktype = iface.linktype;
            s.snaplen = iface.snaplen;
            s.ts_precision = if iface.tsresol == loom_eth_capture::TimestampResolution::NANO {
                "nano"
            } else {
                "micro"
            }
            .into();
            s.drop_stats = match stats.availability {
                loom_eth_capture::StatsAvailability::Available => "available",
                loom_eth_capture::StatsAvailability::Partial => "partial",
                loom_eth_capture::StatsAvailability::Unavailable => "unavailable",
            }
            .into();
            state_event(&em, s, &shared.metrics);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_queue = queue.clone();
        let thread_em = em.clone();
        let failed_shared = shared.clone();
        let failed_em = em.clone();
        let thread = std::thread::Builder::new()
            .name("loom-eth-decode".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    pipeline::run(capture, thread_queue, thread_em, shared, thread_stop, config, ch)
                }))
                .unwrap_or_else(|_| Err(error(ErrorCode::Internal, "eth decode thread panicked")));
                if let Err(e) = &result
                    && let Some(s) = lock(&failed_shared.state).as_mut()
                {
                    s.state = CaptureStateKind::Error;
                    s.error = Some(e.message.clone());
                    state_event(&failed_em, s, &failed_shared.metrics);
                }
                result
            })
            .map_err(|e| error(ErrorCode::Internal, e.to_string()))?;
        Ok(Self {
            commands: tx,
            budget: Arc::new(AtomicUsize::new(0)),
            stop,
            capture_stop,
            queue,
            thread: Some(thread),
            emitter: em,
        })
    }
    fn shutdown(mut self) -> Result<(), WorkerError> {
        // A run that already ended has published its terminal state (`error` included);
        // joining it is cleanup, not a new failure of the next start/stop.
        let already_ended = self.thread.as_ref().is_some_and(JoinHandle::is_finished);
        self.stop.store(true, Ordering::Release);
        self.capture_stop.stop();
        if let Some(t) = self.thread.take() {
            let result = t
                .join()
                .map_err(|_| error(ErrorCode::Internal, "decode thread join failed"))?;
            if !already_ended {
                result?;
            }
        }
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.capture_stop.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
/// One capture-channel worker. Callbacks share the same packet producer used by the UI.
#[derive(Debug)]
pub struct EthWorker {
    config: WorkerConfig,
    shared: Arc<Shared>,
    running: Option<Running>,
    channel: Option<String>,
    generation: u32,
    send_rate: RateGuard,
    rate_base: Instant,
    rate_clock: Option<u64>,
}
impl Default for EthWorker {
    fn default() -> Self {
        Self::new()
    }
}
impl EthWorker {
    pub fn new() -> Self {
        Self::with_config(WorkerConfig::default()).expect("valid built-in worker configuration")
    }
    pub fn with_config(config: WorkerConfig) -> Result<Self, WorkerError> {
        config.validate()?;
        let rate_base = Instant::now();
        let send_rate = RateGuard::new_at(config.send_per_sec, config.send_per_sec, rate_base);
        Ok(Self {
            config,
            shared: Arc::new(Shared::default()),
            running: None,
            channel: None,
            generation: 0,
            send_rate,
            rate_base,
            rate_clock: None,
        })
    }
    fn check_channel(&self, ch: &str) -> Result<(), WorkerError> {
        if ch.is_empty() || ch.len() > MAX_ID_BYTES || self.channel.as_ref().is_some_and(|c| c != ch) {
            Err(error(ErrorCode::Usage, "invalid or mismatched worker channel"))
        } else {
            Ok(())
        }
    }
    /// CORE Health adapter input. RSS is deliberately absent rather than invented.
    pub fn health(&self) -> Value {
        let total = Metrics::get(&self.shared.metrics.total)
            .saturating_add(Metrics::get(&self.shared.metrics.capture_dropped));
        let shown = Metrics::get(&self.shared.metrics.shown);
        json!({"queue_frames":self.running.as_ref().map_or(0,|r|r.queue.len()),"queue_bytes":self.running.as_ref().map_or(0,|r|r.queue.byte_usage()),"tx_queue_bytes":self.running.as_ref().map_or(0,|r|r.budget.load(Ordering::Acquire)),"decode_lag_ms":Metrics::get(&self.shared.metrics.decode_lag_ms),"shown":shown,"total":total,"publication_permille":if total==0{1000}else{shown.saturating_mul(1000)/total},"emitter_failures":Metrics::get(&self.shared.metrics.emit_failed),"display_lost":Metrics::get(&self.shared.metrics.display_lost),"unrecorded":Metrics::get(&self.shared.metrics.unrecorded),"decode_resets":Metrics::get(&self.shared.metrics.decode_resets),"timestamp_fallbacks":Metrics::get(&self.shared.metrics.timestamp_fallbacks),"stream_readers_outstanding":loom_eth_capture::outstanding_stream_readers(),"journal_available":false})
    }
    fn status(&self) -> Value {
        let mut v = lock(&self.shared.state)
            .as_ref()
            .and_then(|s| serde_json::to_value(s).ok())
            .unwrap_or(json!({"ch":self.channel,"state":"stopped"}));
        if let Some(o) = v.as_object_mut() {
            o.insert("health".into(), self.health());
            o.insert("journal_available".into(), Value::Bool(false));
        }
        v
    }
    fn invalid_write_row(&self, cx: &WorkerCtx, w: &Write) {
        if w.request_id.len() > MAX_ID_BYTES
            || w.client_id.as_ref().is_some_and(|s| s.len() > MAX_ID_BYTES)
            || w.op.get("dry_run").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let ch =
            w.op.get("ch")
                .and_then(Value::as_str)
                .filter(|ch| self.check_channel(ch).is_ok())
                .or(self.channel.as_deref())
                .unwrap_or("eth");
        let kind = bounded_text(w.op.get("kind").and_then(Value::as_str).unwrap_or("invalid"), 16);
        let encoded = w.op.get("hex").and_then(Value::as_str).unwrap_or("");
        let preview = encoded
            .get(..encoded.len().min(TX_PREVIEW_BYTES * 2))
            .and_then(|s| loom_proto::bytes::decode_hex(s).ok())
            .unwrap_or_default();
        let p = SendParams {
            ch: ch.into(),
            kind,
            hex: String::new(),
            to: None,
            from_port: None,
            dry_run: false,
            keep_open: w.op.get("keep_open").and_then(Value::as_bool).unwrap_or(false),
        };
        let env = if lock(&self.shared.state)
            .as_ref()
            .is_some_and(|s| s.backend == "live")
        {
            self.config.live_environment
        } else {
            Env::Sim
        };
        let meta = TxMeta::new(w, &p, (encoded.len() / 2).min(u32::MAX as usize), &preview, env);
        let em = self
            .running
            .as_ref()
            .map(|r| r.emitter.clone())
            .unwrap_or_else(|| cx.emitter(ch));
        meta.emit(&em, &self.shared.metrics, WriteState::Requested, None);
        meta.emit(&em, &self.shared.metrics, WriteState::Failed, None);
    }
    fn validate_start(&self, p: &CaptureStartParams) -> Result<(), WorkerError> {
        let ch = p.ch.as_deref().unwrap_or(&p.iface);
        self.check_channel(ch)?;
        if p.iface.is_empty()
            || p.iface.len() > MAX_ID_BYTES
            || p.snaplen == 0
            || p.snaplen as usize > loom_eth_capture::MAX_FRAME_BYTES
            || !(1..=64).contains(&p.buffer_mib)
        {
            return Err(error(
                ErrorCode::Usage,
                "capture interface, snaplen or buffer outside bounds",
            ));
        }
        if p.backend != "live" && p.bpf.is_some() {
            return Err(error(
                ErrorCode::NotImplemented,
                "BPF is supported only for live capture; use a display filter (-Y) for sim/replay",
            ));
        }
        if let Some(r) = &p.replay
            && r.path.len() > 4096
        {
            return Err(error(ErrorCode::Usage, "replay path exceeds bound"));
        }
        if (p.backend == "stream") != p.stream.is_some() {
            return Err(error(
                ErrorCode::Usage,
                "backend stream needs stream options (and only stream takes them)",
            ));
        }
        if p.backend == "stream" && p.replay.is_some() {
            return Err(error(ErrorCode::Usage, "stream does not take replay options"));
        }
        if let Some(s) = &p.stream {
            source::stream_options(s)?;
        }
        Ok(())
    }
}
impl Worker for EthWorker {
    const KIND: &'static str = crate::WORKER_KIND;
    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async move {
            request_bound(&call.args)?;
            match call.method.as_str() {
                "eth.ifaces" => {
                    #[cfg(feature = "live")]
                    {
                        let list = background(self.shared.control_tasks.clone(), || {
                            loom_eth_capture::list_interfaces().map_err(capture_error)
                        })
                        .await?;
                        Ok(
                            json!({"ifaces":list.into_iter().map(|d|json!({"name":d.name,"desc":d.desc,"ips":d.ips,"up":d.up,"loopback":d.loopback,"can_capture":d.can_capture,"reason":d.reason,"permission_denied":d.permission_denied})).collect::<Vec<_>>()}),
                        )
                    }
                    #[cfg(not(feature = "live"))]
                    {
                        Ok(
                            json!({"ifaces":[{"name":"sim","desc":"permission-free synthetic tap","ips":[],"up":true,"loopback":true,"can_capture":true},{"name":"replay","desc":"pcapng/pcap replay","ips":[],"up":true,"loopback":true,"can_capture":true},{"name":"stream","desc":"external pcapng producer over loopback TCP (not a NIC capture)","ips":[],"up":true,"loopback":true,"can_capture":true}]}),
                        )
                    }
                }
                "eth.capture.start" => {
                    let p: CaptureStartParams = serde_json::from_value(call.args)
                        .map_err(|e| error(ErrorCode::Usage, e.to_string()))?;
                    self.validate_start(&p)?;
                    if self.shared.control_tasks.load(Ordering::Acquire) > 0 {
                        return Err(error(
                            ErrorCode::WorkerBusy,
                            "capture/control operation still pending",
                        ));
                    }
                    if self
                        .running
                        .as_ref()
                        .is_some_and(|r| r.thread.as_ref().is_some_and(|t| !t.is_finished()))
                    {
                        return Err(error(ErrorCode::Locked, "capture already running"));
                    }
                    if let Some(old) = self.running.take() {
                        background(self.shared.control_tasks.clone(), move || old.shutdown()).await?;
                    }
                    self.generation = self
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| error(ErrorCode::Config, "capture generation exhausted"))?;
                    let ch = p.ch.as_deref().unwrap_or(&p.iface).to_owned();
                    self.channel = Some(ch.clone());
                    self.shared.metrics.reset();
                    let s = CaptureStatePayload {
                        ch: ch.clone(),
                        state: CaptureStateKind::Starting,
                        backend: p.backend.clone(),
                        iface: p.iface.clone(),
                        bpf: p.bpf.clone(),
                        snaplen: p.snaplen,
                        promisc: p.promisc,
                        buffer_bytes_requested: loom_proto::num::U64(u64::from(p.buffer_mib) * 1024 * 1024),
                        buffer_bytes_actual: None,
                        ts_precision: "nano".into(),
                        drop_stats: "unavailable".into(),
                        linktype: 1,
                        started_mono_ns: loom_proto::num::U64(cx.clock().mono_ns()),
                        error: None,
                        reason: None,
                    };
                    let em = cx.emitter(&ch).with_generation(
                        Some(self.generation),
                        Some(if p.backend == "live" {
                            self.config.live_environment
                        } else {
                            Env::Sim
                        }),
                    );
                    if self.generation > 1 {
                        gap(
                            &em,
                            "source.gap",
                            "capture_restart",
                            None,
                            None,
                            None,
                            &self.shared.metrics,
                        );
                    }
                    state_event(&em, &s, &self.shared.metrics);
                    *lock(&self.shared.state) = Some(s);
                    *lock(&self.shared.recent) = RecentTable::default();
                    let shared = self.shared.clone();
                    let config = self.config.clone();
                    let context = cx.clone();
                    let generation = self.generation;
                    match background(self.shared.control_tasks.clone(), move || {
                        Running::open(p, context, shared, config, generation)
                    })
                    .await
                    {
                        Ok(r) => {
                            self.running = Some(r);
                            Ok(self.status())
                        }
                        Err(e) => {
                            if let Some(s) = lock(&self.shared.state).as_mut() {
                                s.state = if e.code == ErrorCode::Permission {
                                    CaptureStateKind::PermissionDenied
                                } else {
                                    CaptureStateKind::Error
                                };
                                s.error = Some(e.message.clone());
                                state_event(&em, s, &self.shared.metrics);
                            }
                            Err(e)
                        }
                    }
                }
                "eth.capture.status" => {
                    if let Some(ch) = call.args.get("ch").and_then(Value::as_str) {
                        self.check_channel(ch)?;
                    }
                    Ok(self.status())
                }
                "eth.capture.stop" => {
                    let ch = call
                        .args
                        .get("ch")
                        .and_then(Value::as_str)
                        .ok_or_else(|| error(ErrorCode::Usage, "ch required"))?;
                    self.check_channel(ch)?;
                    if let Some(r) = self.running.take() {
                        background(self.shared.control_tasks.clone(), move || r.shutdown()).await?;
                    }
                    Ok(self.status())
                }
                "eth.interest.set" => {
                    let ch = call
                        .args
                        .get("ch")
                        .and_then(Value::as_str)
                        .ok_or_else(|| error(ErrorCode::Usage, "ch required"))?;
                    self.check_channel(ch)?;
                    let filters = call
                        .args
                        .get("filters")
                        .and_then(Value::as_array)
                        .ok_or_else(|| error(ErrorCode::Usage, "filters array required"))?;
                    if filters.len() > MAX_INTEREST_FILTERS {
                        return Err(error(ErrorCode::Usage, "at most 16 interest filters"));
                    }
                    let mut texts = Vec::with_capacity(filters.len());
                    let mut bytes = 0usize;
                    for f in filters {
                        let f = f
                            .as_str()
                            .ok_or_else(|| error(ErrorCode::Usage, "filter must be a string"))?;
                        bytes = bytes.saturating_add(f.len());
                        if bytes > MAX_INTEREST_TEXT_BYTES {
                            return Err(error(ErrorCode::Usage, "interest text exceeds 8 KiB"));
                        }
                        texts.push(f.to_owned());
                    }
                    let compiled = background(self.shared.control_tasks.clone(), move || {
                        texts
                            .iter()
                            .map(|s| Filter::compile(s).map_err(|e| error(ErrorCode::Usage, e.to_string())))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .await?;
                    let count = compiled.len();
                    *lock(&self.shared.interests) = compiled;
                    Ok(json!({"filters":count}))
                }
                "eth.sim.reset" => {
                    let ch = call
                        .args
                        .get("ch")
                        .and_then(Value::as_str)
                        .ok_or_else(|| error(ErrorCode::Usage, "ch required"))?;
                    self.check_channel(ch)?;
                    if !lock(&self.shared.state)
                        .as_ref()
                        .is_some_and(|s| s.backend == "sim" && s.state == CaptureStateKind::Live)
                    {
                        return Err(error(ErrorCode::NotOpen, "active sim required"));
                    }
                    let r = self
                        .running
                        .as_ref()
                        .ok_or_else(|| error(ErrorCode::NotOpen, "capture not open"))?;
                    let (wait, complete) = reply();
                    r.commands
                        .try_send(source::Command::Reset(source::Reset {
                            complete: Some(complete),
                        }))
                        .map_err(|_| error(ErrorCode::WorkerBusy, "sim control queue full"))?;
                    wait.await
                }
                _ => Err(error(
                    ErrorCode::NotImplemented,
                    "eth worker method not implemented",
                )),
            }
        })
    }
    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, w: Write) -> BoxFuture<'a, WriteOutcome> {
        Box::pin(async move {
            let parsed = (|| {
                if w.request_id.len() > MAX_ID_BYTES
                    || w.client_id.as_ref().is_some_and(|s| s.len() > MAX_ID_BYTES)
                {
                    return Err(error(ErrorCode::Usage, "write identifiers exceed bounds"));
                }
                request_bound(&w.op)?;
                if w.op
                    .get("hex")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.len() > 2 * MAX_SEND_BYTES)
                {
                    return Err(error(ErrorCode::Usage, "send exceeds 9 KiB"));
                }
                let p: SendParams = serde_json::from_value(w.op.clone())
                    .map_err(|e| error(ErrorCode::Usage, e.to_string()))?;
                self.check_channel(&p.ch)?;
                if p.to.as_ref().is_some_and(|s| s.len() > MAX_ID_BYTES) {
                    return Err(error(ErrorCode::Usage, "destination exceeds bounds"));
                }
                let bytes = loom_proto::bytes::decode_hex(&p.hex)
                    .map_err(|e| error(ErrorCode::Usage, e.to_string()))?;
                Ok((p, bytes))
            })();
            let (p, bytes) = match parsed {
                Ok(v) => v,
                Err(e) => {
                    self.invalid_write_row(cx, &w);
                    return WriteOutcome {
                        state: WriteState::Failed,
                        detail: Value::Null,
                        error: Some(e),
                    };
                }
            };
            if p.dry_run {
                let d = loom_eth_decode::decode(
                    1,
                    &bytes,
                    &Default::default(),
                    &mut loom_eth_decode::StreamTable::new(),
                );
                return WriteOutcome {
                    state: WriteState::Requested,
                    detail: json!({"proto":d.summary.proto,"info":d.summary.info,"len":bytes.len(),"risk":"R4","risk_note":"conservative preview; gateway/shared UDS classification required"}),
                    error: None,
                };
            }
            let env = if lock(&self.shared.state)
                .as_ref()
                .is_some_and(|s| s.backend == "live")
            {
                self.config.live_environment
            } else {
                Env::Sim
            };
            let meta = TxMeta::new(&w, &p, bytes.len(), &bytes, env);
            let em = self
                .running
                .as_ref()
                .map(|r| r.emitter.clone())
                .unwrap_or_else(|| cx.emitter(&p.ch));
            let fail = |e: WorkerError| {
                meta.emit(&em, &self.shared.metrics, WriteState::Failed, None);
                WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(e),
                }
            };
            if !meta.emit(&em, &self.shared.metrics, WriteState::Requested, None) {
                return fail(error(
                    ErrorCode::WorkerBusy,
                    "requested row could not be published; nothing sent",
                ));
            }
            if w.origin == Origin::Device {
                return fail(error(ErrorCode::Denied, "device cannot originate a send request"));
            }
            if cx.latch().is_on() && w.origin != Origin::User {
                return fail(error(ErrorCode::Stopped, "STOP latched"));
            }
            let Some(r) = self.running.as_ref() else {
                return fail(error(ErrorCode::NotOpen, "capture not started"));
            };
            if !lock(&self.shared.state)
                .as_ref()
                .is_some_and(|s| s.state == CaptureStateKind::Live)
            {
                return fail(error(ErrorCode::NotOpen, "capture no longer active"));
            }
            let op = match p.kind.as_str() {
                "raw" => source::Operation::Raw,
                "udp" => {
                    let to = match p.to.as_ref().and_then(|s| s.parse().ok()) {
                        Some(to) => to,
                        None => return fail(error(ErrorCode::Usage, "numeric IP:PORT destination required")),
                    };
                    source::Operation::Udp {
                        to,
                        from_port: p.from_port.unwrap_or(0),
                    }
                }
                "tcp" => {
                    let to = match p.to.as_ref().and_then(|s| s.parse().ok()) {
                        Some(to) => to,
                        None => return fail(error(ErrorCode::Usage, "numeric IP:PORT destination required")),
                    };
                    if p.from_port.is_some() {
                        return fail(error(ErrorCode::Usage, "from_port is UDP-only"));
                    }
                    let keep_open = match w.op.get("keep_open") {
                        None => false,
                        Some(Value::Bool(b)) => *b,
                        _ => return fail(error(ErrorCode::Usage, "keep_open must be a boolean")),
                    };
                    source::Operation::Tcp { to, keep_open }
                }
                _ => return fail(error(ErrorCode::Usage, "send kind must be raw, udp or tcp")),
            };
            let base = *self.rate_clock.get_or_insert(cx.clock().mono_ns());
            let at = self
                .rate_base
                .checked_add(Duration::from_nanos(cx.clock().mono_ns().saturating_sub(base)))
                .unwrap_or(self.rate_base);
            if !self.send_rate.try_take_at(at, 1) {
                return fail(error(ErrorCode::RateLimit, "transmit rate limit"));
            }
            let charge =
                bytes.len() + std::mem::size_of::<source::Ticket>() + 4 * MAX_ID_BYTES + 2 * TX_PREVIEW_BYTES;
            let slot = match source::Slot::reserve(r.budget.clone(), charge) {
                Ok(v) => v,
                Err(e) => return fail(e),
            };
            if !meta.emit(&em, &self.shared.metrics, WriteState::Accepted, None) {
                return fail(error(
                    ErrorCode::WorkerBusy,
                    "accepted row could not be published; nothing sent",
                ));
            }
            let (wait, complete) = reply();
            let t = source::Ticket {
                bytes,
                op,
                meta: meta.clone(),
                cx: cx.clone(),
                emitter: em.clone(),
                shared: self.shared.clone(),
                complete: Some(complete),
                _slot: slot,
                deadline_ns: w.deadline_mono_ns.0,
            };
            if let Err(e) = r.commands.try_send(source::Command::Write(Box::new(t))) {
                let (cmd, code) = match e {
                    std::sync::mpsc::TrySendError::Full(cmd) => (cmd, ErrorCode::WorkerBusy),
                    std::sync::mpsc::TrySendError::Disconnected(cmd) => (cmd, ErrorCode::NotOpen),
                };
                if let source::Command::Write(mut ticket) = cmd {
                    ticket.finish(Err(error(code, "capture transmit queue full/closed")), false);
                }
            }
            wait.await
        })
    }
    fn on_job<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _job: JobCmd,
    ) -> BoxFuture<'a, Result<Value, WorkerError>> {
        Box::pin(async {
            Err(error(
                ErrorCode::NotImplemented,
                "eth worker jobs require CORE job runtime",
            ))
        })
    }
    fn on_input<'a>(
        &'a mut self,
        _cx: &'a WorkerCtx,
        _input: Input,
    ) -> BoxFuture<'a, Result<(), WorkerError>> {
        Box::pin(async {
            Err(error(
                ErrorCode::NotImplemented,
                "eth uses eth.send rather than interactive input",
            ))
        })
    }
}
