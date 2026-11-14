use super::{packet, shared::*, *};
use loom_eth_capture::{
    CaptureSource, DropStats, Interface, RawFrame, StopSignal, TimestampResolution, TimestampSource,
};
use loom_proto::event::{Origin, WriteState};
use loom_worker_sdk::{Emitter, WorkerCtx, WriteOutcome};
use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError},
    },
    time::Duration,
};

/// Bounded simulator response queue; overflow is a capture/source error, never silent.
pub const MAX_SIM_PENDING: usize = 64;
pub const MAX_SIM_PENDING_BYTES: usize = 64 * 1024;
pub const CAP_SOURCE_POLL: Duration = Duration::from_millis(2);
pub(crate) enum Operation {
    Raw,
    Udp { to: SocketAddr, from_port: u16 },
    Tcp { to: SocketAddr, keep_open: bool },
}
#[derive(Debug)]
pub(crate) struct Slot {
    budget: Arc<AtomicUsize>,
    charge: usize,
}
impl Slot {
    pub fn reserve(budget: Arc<AtomicUsize>, charge: usize) -> Result<Self, WorkerError> {
        budget
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(charge).filter(|v| *v <= MAX_TX_QUEUE_BYTES)
            })
            .map_err(|_| error(ErrorCode::WorkerBusy, "transmit byte queue full"))?;
        Ok(Self { budget, charge })
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.budget.fetch_sub(self.charge, Ordering::AcqRel);
    }
}
pub(crate) struct Ticket {
    pub bytes: Vec<u8>,
    pub op: Operation,
    pub meta: TxMeta,
    pub cx: WorkerCtx,
    pub emitter: Emitter,
    pub shared: Arc<Shared>,
    pub complete: Option<Complete<WriteOutcome>>,
    pub _slot: Slot,
    pub deadline_ns: u64,
}
impl Ticket {
    pub(super) fn finish(&mut self, result: Result<(), WorkerError>, publication_ok: bool) {
        let outcome = match result {
            Ok(()) => WriteOutcome {
                state: WriteState::Sent,
                detail: serde_json::to_value(&self.meta.row).unwrap_or(Value::Null),
                error: if publication_ok {
                    None
                } else {
                    Some(error(
                        ErrorCode::Partial,
                        "driver accepted; eth.tx could not be published",
                    ))
                },
            },
            Err(e) => {
                self.meta
                    .emit(&self.emitter, &self.shared.metrics, WriteState::Failed, None);
                WriteOutcome {
                    state: WriteState::Failed,
                    detail: Value::Null,
                    error: Some(e),
                }
            }
        };
        if let Some(c) = self.complete.take() {
            c.finish(outcome);
        }
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        if self.complete.is_some() {
            self.finish(
                Err(error(ErrorCode::WorkerDown, "capture stopped before queued send")),
                false,
            );
        }
    }
}
pub(crate) enum Command {
    Write(Box<Ticket>),
    Reset(Reset),
}
pub(crate) struct Reset {
    pub complete: Option<Complete<Result<Value, WorkerError>>>,
}
impl Drop for Reset {
    fn drop(&mut self) {
        if let Some(c) = self.complete.take() {
            c.finish(Err(error(ErrorCode::NotOpen, "sim stopped before reset")));
        }
    }
}
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Write(_) => "Write",
            Self::Reset(_) => "Reset",
        })
    }
}
#[derive(Debug)]
struct SimGenerator {
    remaining: u8,
    next_announcement: u64,
    next_sd: u64,
    seed: u64,
    period: u64,
    jitter: u64,
}
impl SimGenerator {
    fn new(config: &WorkerConfig, now: u64) -> Self {
        Self {
            remaining: 3,
            next_announcement: now,
            next_sd: now.saturating_add(config.sim_sd_period_ms * 1_000_000),
            seed: 0x51_4d_4c_4f,
            period: config.sim_sd_period_ms * 1_000_000,
            jitter: config.sim_jitter_ms * 1_000_000,
        }
    }
    fn next(&mut self, now: u64) -> Option<Vec<u8>> {
        if self.remaining > 0 && now >= self.next_announcement {
            self.remaining -= 1;
            self.next_announcement = self.next_announcement.saturating_add(100_000_000);
            return Some(packet::announcement());
        }
        if now >= self.next_sd {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 7;
            self.seed ^= self.seed << 17;
            let span = self.jitter.saturating_mul(2).saturating_add(1);
            let offset = self.seed % span;
            let delay = self
                .period
                .saturating_sub(self.jitter)
                .saturating_add(offset)
                .max(1_000_000);
            self.next_sd = now.saturating_add(delay);
            return Some(packet::offer());
        }
        None
    }
}
#[derive(Debug)]
pub(crate) struct Source {
    inner: Option<Box<dyn CaptureSource>>,
    interface: Interface,
    commands: Receiver<Command>,
    pending: VecDeque<Vec<u8>>,
    pending_bytes: usize,
    generator: Option<SimGenerator>,
    cx: WorkerCtx,
    shared: Arc<Shared>,
    stop: StopSignal,
    backend: String,
    injected: u64,
    config: WorkerConfig,
    native_tcp: Option<super::tcp::NativeTcp>,
    virtual_tcp: super::tcp::VirtualTcp,
}
impl Source {
    pub fn open(
        params: &crate::model::CaptureStartParams,
        commands: Receiver<Command>,
        cx: WorkerCtx,
        shared: Arc<Shared>,
        config: WorkerConfig,
    ) -> Result<Self, WorkerError> {
        let inner: Option<Box<dyn CaptureSource>> = match params.backend.as_str() {
            "sim" => None,
            "replay" => {
                let p = params
                    .replay
                    .as_ref()
                    .ok_or_else(|| error(ErrorCode::Usage, "replay options required"))?;
                let metadata =
                    std::fs::metadata(&p.path).map_err(|e| error(ErrorCode::NotFound, e.to_string()))?;
                if !metadata.is_file() {
                    return Err(error(ErrorCode::Usage, "replay requires a regular capture file"));
                }
                let speed = if p.speed.as_str() == Some("max") {
                    loom_eth_capture::ReplaySpeed::Max
                } else {
                    let n = p
                        .speed
                        .as_f64()
                        .filter(|n| n.is_finite() && *n > 0.0)
                        .ok_or_else(|| {
                            error(ErrorCode::Usage, "replay speed must be max or positive factor")
                        })?;
                    loom_eth_capture::ReplaySpeed::Realtime { factor: 1.0 / n }
                };
                Some(Box::new(
                    loom_eth_capture::ReplaySource::open(
                        &p.path,
                        loom_eth_capture::ReplayOptions {
                            speed,
                            repeat: p.repeat,
                        },
                    )
                    .map_err(capture_error)?,
                ))
            }
            "stream" => {
                let p = params
                    .stream
                    .as_ref()
                    .ok_or_else(|| error(ErrorCode::Usage, "stream options required"))?;
                let options = stream_options(p)?;
                Some(Box::new(
                    loom_eth_capture::StreamSource::connect(options).map_err(|e| match e {
                        loom_eth_capture::CaptureError::Io(_)
                        | loom_eth_capture::CaptureError::Backend(_) => {
                            error(ErrorCode::NotOpen, e.to_string())
                        }
                        other => capture_error(other),
                    })?,
                ))
            }
            "live" => {
                #[cfg(feature = "live")]
                {
                    let mut c = loom_eth_capture::LiveConfig::new(&params.iface);
                    c.snaplen = params.snaplen;
                    c.promisc = params.promisc;
                    c.buffer_size = (params.buffer_mib * 1024 * 1024) as i32;
                    let mut live = loom_eth_capture::LiveSource::open(c).map_err(capture_error)?;
                    if let Some(expr) = &params.bpf {
                        live.set_bpf(expr).map_err(capture_error)?;
                    }
                    Some(Box::new(live))
                }
                #[cfg(not(feature = "live"))]
                {
                    return Err(error(ErrorCode::NotImplemented, "live feature disabled"));
                }
            }
            _ => {
                return Err(error(
                    ErrorCode::Usage,
                    "backend must be live, replay, sim or stream",
                ));
            }
        };
        let interface = inner.as_ref().map(|s| s.interface()).unwrap_or(Interface {
            linktype: 1,
            snaplen: params.snaplen,
            name: params.iface.clone(),
            tsresol: TimestampResolution::NANO,
        });
        let generator = (params.backend == "sim").then(|| SimGenerator::new(&config, cx.clock().mono_ns()));
        Ok(Self {
            inner,
            interface,
            commands,
            pending: VecDeque::with_capacity(MAX_SIM_PENDING),
            pending_bytes: 0,
            generator,
            cx,
            shared,
            stop: StopSignal::default(),
            backend: params.backend.clone(),
            injected: 0,
            config,
            native_tcp: None,
            virtual_tcp: Default::default(),
        })
    }
    fn frame(&mut self, mut data: Vec<u8>) -> RawFrame {
        let origlen = data.len() as u32;
        if data.len() > self.interface.snaplen as usize {
            data.truncate(self.interface.snaplen as usize);
        }
        self.injected = self.injected.saturating_add(1);
        RawFrame {
            ts_ns: self.cx.clock().wall_ns().into(),
            caplen: data.len() as u32,
            origlen,
            data,
            interface_id: 0,
            section_index: u64::MAX,
            interface: self.interface.clone(),
            comments: Vec::new(),
            meta: None,
        }
    }
    fn virtual_frame(&mut self, frame: super::tcp::VirtualFrame) -> loom_eth_capture::Result<()> {
        if let Some(meta) = frame.meta {
            lock(&self.shared.recent).insert(&frame.bytes, meta, self.cx.clock().mono_ns(), true);
        }
        self.enqueue(frame.bytes)
    }
    fn enqueue(&mut self, data: Vec<u8>) -> loom_eth_capture::Result<()> {
        if self.pending.len() >= MAX_SIM_PENDING
            || data.len() > MAX_SIM_PENDING_BYTES.saturating_sub(self.pending_bytes)
        {
            return Err(loom_eth_capture::CaptureError::Limit("sim responses"));
        }
        self.pending_bytes += data.len();
        self.pending.push_back(data);
        Ok(())
    }
    fn execute(&mut self, mut t: Box<Ticket>) -> loom_eth_capture::Result<Option<RawFrame>> {
        if t.cx.latch().is_on() && t.meta.origin != Origin::User {
            t.finish(
                Err(error(ErrorCode::Stopped, "STOP latched before driver send")),
                false,
            );
            return Ok(None);
        }
        if t.deadline_ns > 0 && t.cx.clock().mono_ns() >= t.deadline_ns {
            t.finish(
                Err(error(ErrorCode::Timeout, "send deadline expired before driver")),
                false,
            );
            return Ok(None);
        }
        if let Operation::Tcp { to, keep_open } = t.op {
            if self.backend == "live" {
                if self.native_tcp.is_none() {
                    match super::tcp::NativeTcp::new(self.stop.clone(), self.cx.clone()) {
                        Ok(s) => self.native_tcp = Some(s),
                        Err(e) => {
                            t.finish(Err(e), false);
                            return Ok(None);
                        }
                    }
                }
                if let Some(service) = &self.native_tcp {
                    service.submit(t);
                }
            } else {
                match self.virtual_tcp.send(
                    to,
                    &t.bytes,
                    keep_open,
                    t.meta.clone(),
                    self.cx.clock().mono_ns(),
                ) {
                    Ok(frames) => {
                        t.meta.row.write_state = "sent".into();
                        let ok = t.meta.emit(&t.emitter, &t.shared.metrics, WriteState::Sent, None);
                        t.finish(Ok(()), ok);
                        for f in frames {
                            self.virtual_frame(f)?;
                        }
                    }
                    Err(e) => t.finish(Err(e), false),
                }
            }
            return Ok(None);
        }
        let mut udp_tuple = None;
        let result: Result<Option<Vec<u8>>, WorkerError> = (|| match t.op {
            Operation::Tcp { .. } => Err(error(ErrorCode::Internal, "TCP dispatched separately")),
            Operation::Raw => {
                if self.backend == "live" {
                    self.inner
                        .as_mut()
                        .ok_or_else(|| error(ErrorCode::NotOpen, "live source absent"))?
                        .send(&t.bytes)
                        .map_err(capture_error)?;
                    Ok(None)
                } else {
                    Ok(Some(t.bytes.clone()))
                }
            }
            Operation::Udp { to, from_port } => {
                let bind = SocketAddr::new(
                    if to.is_ipv4() {
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                    } else {
                        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
                    },
                    from_port,
                );
                let socket =
                    UdpSocket::bind(bind).map_err(|e| error(ErrorCode::Disconnected, e.to_string()))?;
                socket
                    .set_nonblocking(true)
                    .map_err(|e| error(ErrorCode::Disconnected, e.to_string()))?;
                socket
                    .connect(to)
                    .map_err(|e| error(ErrorCode::Disconnected, e.to_string()))?;
                if t.cx.latch().is_on() && t.meta.origin != Origin::User {
                    return Err(error(ErrorCode::Stopped, "STOP latched before UDP send"));
                }
                let n = socket
                    .send(&t.bytes)
                    .map_err(|e| error(ErrorCode::Disconnected, e.to_string()))?;
                if n != t.bytes.len() {
                    return Err(error(ErrorCode::Disconnected, "partial UDP send"));
                }
                let local = socket
                    .local_addr()
                    .map_err(|e| error(ErrorCode::Disconnected, e.to_string()))?;
                udp_tuple = Some((local, to));
                if self.backend == "live" {
                    Ok(None)
                } else {
                    Ok(packet::udp(local, to, &t.bytes))
                }
            }
        })();
        let echo = match result {
            Ok(echo) => echo,
            Err(e) => {
                t.finish(Err(e), false);
                return Ok(None);
            }
        };
        {
            let mut table = lock(&self.shared.recent);
            if let Some(bytes) = &echo {
                table.insert(bytes, t.meta.clone(), self.cx.clock().mono_ns(), true);
            } else if let Some((src, dst)) = udp_tuple {
                table.insert_udp(&t.bytes, src, dst, t.meta.clone(), self.cx.clock().mono_ns());
            } else {
                table.insert(&t.bytes, t.meta.clone(), self.cx.clock().mono_ns(), false);
            }
        }
        // Publish sent before any captured copy is exposed; updates cannot regress observed_src_seq.
        t.meta.row.write_state = "sent".into();
        let published = t
            .meta
            .emit(&t.emitter, &self.shared.metrics, WriteState::Sent, None);
        t.finish(Ok(()), published);
        if let Some(echo) = echo {
            if self.backend == "sim"
                && let Some(response) = packet::routing_response(&echo)
            {
                self.enqueue(response)?;
            }
            let virtual_udp = matches!(&t.op, Operation::Udp { .. });
            let mut frame = self.frame(echo);
            if virtual_udp {
                frame.interface.linktype = 1;
            }
            return Ok(Some(frame));
        }
        Ok(None)
    }
}
impl CaptureSource for Source {
    fn interface(&self) -> Interface {
        self.interface.clone()
    }
    fn timestamp_source(&self) -> TimestampSource {
        match self.backend.as_str() {
            "sim" => TimestampSource::Sim,
            "replay" => TimestampSource::Replay,
            "stream" => TimestampSource::External,
            _ => TimestampSource::PcapHost,
        }
    }
    fn set_stop_signal(&mut self, s: StopSignal) {
        if let Some(inner) = &mut self.inner {
            inner.set_stop_signal(s.clone());
        }
        self.stop = s;
    }
    fn stats(&mut self) -> DropStats {
        let stats = if let Some(inner) = &mut self.inner {
            let mut stats = inner.stats();
            stats.recv = stats.recv.saturating_add(self.injected);
            stats
        } else if self.config.sim_stats_unavailable {
            DropStats::UNAVAILABLE
        } else {
            DropStats {
                recv: self.injected,
                ..DropStats::ZERO
            }
        };
        *lock(&self.shared.drop_stats) = stats;
        stats
    }
    fn is_exhausted(&self) -> bool {
        self.pending.is_empty() && self.inner.as_ref().is_some_and(|s| s.is_exhausted())
    }
    fn producer_seq_missing(&self) -> Option<u64> {
        self.inner.as_ref().and_then(|s| s.producer_seq_missing())
    }
    fn next(&mut self, timeout: Duration) -> loom_eth_capture::Result<Option<RawFrame>> {
        if self.stop.is_stopped() {
            return Ok(None);
        }
        for f in self
            .virtual_tcp
            .expire(self.cx.clock().mono_ns())
            .map_err(|e| loom_eth_capture::CaptureError::Backend(e.message))?
        {
            self.virtual_frame(f)?;
        }
        if let Some(data) = self.pending.pop_front() {
            self.pending_bytes -= data.len();
            let mut frame = self.frame(data);
            frame.interface.linktype = 1;
            return Ok(Some(frame));
        }
        if let Some(g) = &mut self.generator
            && let Some(data) = g.next(self.cx.clock().mono_ns())
        {
            return Ok(Some(self.frame(data)));
        }
        match self.commands.try_recv() {
            Ok(Command::Write(t)) => {
                if let Some(f) = self.execute(t)? {
                    return Ok(Some(f));
                }
            }
            Ok(Command::Reset(mut reset)) => {
                if self.generator.is_some() {
                    self.generator = Some(SimGenerator::new(&self.config, self.cx.clock().mono_ns()));
                    if let Some(c) = reset.complete.take() {
                        c.finish(Ok(json!({"announcements":3})));
                    }
                } else if let Some(c) = reset.complete.take() {
                    c.finish(Err(error(ErrorCode::NotImplemented, "reset only supports sim")));
                }
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => (),
        }
        if let Some(inner) = &mut self.inner {
            inner.next(timeout.min(CAP_SOURCE_POLL))
        } else {
            std::thread::sleep(timeout.min(CAP_SOURCE_POLL));
            Ok(None)
        }
    }
}
pub(crate) fn channel() -> (SyncSender<Command>, Receiver<Command>) {
    std::sync::mpsc::sync_channel(MAX_TX_COMMANDS)
}

/// Validates `stream` params into capture options (loopback-only, bounded timeouts).
pub(crate) fn stream_options(
    p: &crate::model::StreamParams,
) -> Result<loom_eth_capture::StreamOptions, WorkerError> {
    let addr: SocketAddr = p.connect.parse().map_err(|_| {
        error(
            ErrorCode::Usage,
            "--connect must be IP:PORT (loopback, no hostnames)",
        )
    })?;
    let mut o = loom_eth_capture::StreamOptions::new(addr);
    o.start_timeout = Duration::from_millis(u64::from(p.connect_timeout_ms));
    o.idle_timeout = (p.idle_timeout_ms > 0).then(|| Duration::from_millis(u64::from(p.idle_timeout_ms)));
    o.validate().map_err(|e| error(ErrorCode::Usage, e.to_string()))?;
    Ok(o)
}
