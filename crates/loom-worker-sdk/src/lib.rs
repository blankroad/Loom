//! Worker-side SDK (contract v1 §5, plan §3.3). Stable surface for domain worker crates.
//!
//! * [`Worker`] — the domain's worker logic, driven by the SDK's control thread (P1).
//! * [`Emitter`] — **synchronous, non-blocking** event output for hot-path threads (RX, ISO-TP,
//!   schedulers). It never blocks: when the queue is full it returns [`EmitError::Full`] and the
//!   caller only counts it (can-core W7). The source seq is consumed even on failure, so a loss
//!   is always visible as a hole in `source_seq`.
//! * [`LatchFlag`] (STOP), [`RateGuard`] (token bucket), [`Clock`] (+ [`SystemClock`], [`SimClock`]).
//! * [`codec`] — `loom-worker/1` frames: header + MessagePack map.
//! * [`transport`] — the two local connections (`ctl`, `data`) and async frame I/O.
//! * [`runtime`] — [`run_worker`]: the worker process main loop (IPC thread, dispatcher thread,
//!   credits, health, orphan latch). Uses a tokio `current_thread` runtime per SDK thread; domain
//!   code never needs async beyond the [`Worker`] callbacks.
//! * [`SharedWorker`]·[`run_shared_worker`] — 소유한 context와 static future를 사용하는
//!   동시 콜백 런타임. 입력·lease 종료·latch·tick 슬롯을 일반 요청과 분리한다.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use loom_proto::event::WriteState;
use loom_proto::worker::{
    Call, Input, InputLeaseEnd, JobRenew, JobStart, JobStop, JobUpdate, WorkerError, Write,
};
use serde_json::Value;

pub mod clock;
pub mod codec;
pub mod emit;
pub mod memory;
pub mod runtime;
pub mod transport;

pub use clock::{Clock, SimClock, SystemClock};
pub use emit::{EmitError, EmitSink, Emitter, EventDraft, MemorySink};
pub use runtime::{
    RunHooks, WorkerArgs, decode_write_outcome, encode_write_outcome, run_shared_worker,
    run_shared_worker_with, run_worker, run_worker_with,
};

/// Boxed `Send` future (same shape as `loom_domain_api::BoxFuture`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Job command (job class, A1-②).
#[derive(Debug, Clone, PartialEq)]
pub enum JobCmd {
    /// Start.
    Start(JobStart),
    /// Plan update.
    Update(JobUpdate),
    /// Lease renewal.
    Renew(JobRenew),
    /// Stop.
    Stop(JobStop),
}

/// Result of a write: final state as seen by the worker plus detail.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteOutcome {
    /// `sent`, `confirmed`, `failed` or `outcome_unknown` (never invent success — D4).
    pub state: WriteState,
    /// Detail (`written_bytes`, `sent_escaped` …).
    pub detail: Value,
    /// Error when the state is `failed` / `outcome_unknown`.
    pub error: Option<WorkerError>,
}

/// Context handed to worker callbacks.
#[derive(Clone)]
pub struct WorkerCtx {
    latch: LatchFlag,
    clock: Arc<dyn Clock>,
    emitter_factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync>,
}

impl fmt::Debug for WorkerCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerCtx")
            .field("latch", &self.latch)
            .finish_non_exhaustive()
    }
}

impl WorkerCtx {
    /// Built by the SDK runtime (P1) or a test harness.
    pub fn new(
        latch: LatchFlag,
        clock: Arc<dyn Clock>,
        emitter_factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync>,
    ) -> Self {
        WorkerCtx {
            latch,
            clock,
            emitter_factory,
        }
    }

    /// Emitter for a channel (clone it into hot-path threads).
    pub fn emitter(&self, channel: &str) -> Emitter {
        (self.emitter_factory)(channel)
    }

    /// STOP latch — check before **every** transmitted unit (A1-②).
    pub fn latch(&self) -> &LatchFlag {
        &self.latch
    }

    /// Clock (simulated in tests).
    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
    }
}

/// The domain's worker. Callbacks run on the SDK control thread and must not block: hand work to
/// your own threads and reply when done.
pub trait Worker: Send + 'static {
    /// Worker kind (`uart`, `can` …), equals the `WorkerSpec.kind` registered by the gateway side.
    const KIND: &'static str;

    /// Generic call (open, close, query …).
    fn on_call<'a>(&'a mut self, cx: &'a WorkerCtx, call: Call) -> BoxFuture<'a, Result<Value, WorkerError>>;

    /// A write that passed the gateway pipeline. Emit the write-state rows (priority) before
    /// returning; the SDK sends `Reply{covers}` after them (publish-before-reply, E7).
    fn on_write<'a>(&'a mut self, cx: &'a WorkerCtx, write: Write) -> BoxFuture<'a, WriteOutcome>;

    /// Job start/update/renew/stop.
    fn on_job<'a>(&'a mut self, cx: &'a WorkerCtx, job: JobCmd) -> BoxFuture<'a, Result<Value, WorkerError>>;

    /// Interactive input bytes (lease already checked by the gateway).
    fn on_input<'a>(&'a mut self, cx: &'a WorkerCtx, input: Input) -> BoxFuture<'a, Result<(), WorkerError>>;

    /// Latch changed. The SDK has already set [`LatchFlag`]; override to cancel queued work.
    fn on_latch(&mut self, _on: bool) {}

    /// 입력 lease 종료 통지. 기존 직렬 Worker는 구현하지 않아도 된다.
    fn on_input_lease_end<'a>(&'a mut self, _cx: &'a WorkerCtx, _end: InputLeaseEnd) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
}

/// 공유 상태를 가진 동시 실행 worker. 콜백은 소유한 context와 Send + 'static future를 쓴다.
/// 동기 부분이나 future의 poll에서 블로킹하지 말고 장비 블로킹 I/O는 별도 스레드로 보낸다.
/// 일반 콜백 56개, 입력 6개, lease 종료 1개, latch 1개의 예약 슬롯(총 64개)을 사용한다.
/// tick을 설정하면 일반 슬롯 하나를 tick 전용으로 남겨 일반 요청이 55개로 제한된다.
/// 일반·입력 상한 초과는 E_WORKER_BUSY, tick 중복은 생략, 실행 중 latch 변경은 최신 상태로 합친다.
/// lease 종료 대기는 최대 16개다. 통지나 응답 큐 포화는 손실을 숨기지 않고 worker를 종료한다.
pub trait SharedWorker: Send + Sync + 'static {
    /// gateway에 등록한 worker 종류.
    const KIND: &'static str;
    /// 일반 요청.
    fn on_call(&self, cx: WorkerCtx, call: Call) -> BoxFuture<'static, Result<Value, WorkerError>>;
    /// 쓰기. 필요한 우선 이벤트를 발행한 뒤 결과를 반환한다.
    fn on_write(&self, cx: WorkerCtx, write: Write) -> BoxFuture<'static, WriteOutcome>;
    /// 잡 요청.
    fn on_job(&self, cx: WorkerCtx, job: JobCmd) -> BoxFuture<'static, Result<Value, WorkerError>>;
    /// 대화형 입력. 일반 요청과 다른 예약 슬롯을 사용한다.
    fn on_input(&self, cx: WorkerCtx, input: Input) -> BoxFuture<'static, Result<(), WorkerError>>;
    /// STOP 변경. atomic latch는 이 콜백보다 먼저 반영된다.
    fn on_latch(&self, _on: bool) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
    /// Welcome.config.tick_ms 주기의 콜백. 이전 tick이 실행 중이면 새 tick을 생략한다.
    fn on_tick(&self, _cx: WorkerCtx) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
    /// 입력 lease 종료 통지.
    fn on_input_lease_end(&self, _cx: WorkerCtx, _end: InputLeaseEnd) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
}

/// STOP latch flag shared between the SDK IPC thread and domain threads.
#[derive(Debug, Clone, Default)]
pub struct LatchFlag(Arc<AtomicBool>);

impl LatchFlag {
    /// New flag (off).
    pub fn new() -> Self {
        LatchFlag::default()
    }
    /// True while STOP is latched: do not transmit agent/script units.
    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    /// Set by the SDK when a `Latch` message arrives.
    pub fn set(&self, on: bool) {
        self.0.store(on, Ordering::Release);
    }
}

/// Token bucket for per-unit rate enforcement in workers (A1-②). Not thread-safe by design:
/// keep one per sending thread.
#[derive(Debug, Clone)]
pub struct RateGuard {
    rate_per_sec: f64,
    burst: f64,
    tokens: f64,
    last: Instant,
}

impl RateGuard {
    /// `rate_per_sec` tokens per second, at most `burst` stored. Starts full.
    pub fn new(rate_per_sec: u32, burst: u32) -> Self {
        Self::new_at(rate_per_sec, burst, Instant::now())
    }

    /// Same as [`RateGuard::new`] with an explicit start time (tests, simulation).
    pub fn new_at(rate_per_sec: u32, burst: u32, now: Instant) -> Self {
        let burst = f64::from(burst.max(1));
        RateGuard {
            rate_per_sec: f64::from(rate_per_sec),
            burst,
            tokens: burst,
            last: now,
        }
    }

    /// Take `n` tokens now if available.
    pub fn try_take(&mut self, n: u32) -> bool {
        self.try_take_at(Instant::now(), n)
    }

    /// Take `n` tokens at time `now` if available (time never goes backwards here).
    pub fn try_take_at(&mut self, now: Instant, n: u32) -> bool {
        if let Some(dt) = now.checked_duration_since(self.last) {
            self.tokens = (self.tokens + dt.as_secs_f64() * self.rate_per_sec).min(self.burst);
            self.last = now;
        }
        let n = f64::from(n);
        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }
}
