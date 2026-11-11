//! Gateway ↔ worker boundary (plan §3.3, contract 05).
//!
//! The gateway talks to every device worker through [`WorkerHost`]. Two implementations:
//!
//! * [`InProcessHost`] (here, P1): runs a `loom_worker_sdk::Worker` as a tokio task inside `loomd`.
//!   Used for simulated/pure-Rust domains and tests.
//! * IPC host (`loom-worker-host`, lead's worker): spawns `loomd worker <kind>` and speaks
//!   `loom-worker/1` frames. It must implement the same trait with the same semantics.
//!
//! **Event path**: a host never publishes on its own. Every record a worker emits goes through the
//! [`EmitSink`] handed to the host at construction — in `loomd` that is [`HubSink`], which marks
//! the terminal write row (`payload.final`, contract 02 Q11) and publishes into the hub (control
//! lane = priority, never dropped; data lane = may drop and is counted as `display.gap`).
//!
//! **Semantics every implementation keeps**
//! * `write` resolves only after the worker's write-state rows were pushed into the sink (the
//!   gateway then waits for the hub watermark: publish-before-reply).
//! * `latch` sets the worker's STOP flag and resolves on the worker's ack; the gateway gives up
//!   after 200 ms and reports the worker as `missing`.
//! * Errors are [`WorkerError`]; a dead worker answers `E_WORKER_DOWN`.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use loom_proto::error::ErrorCode;
use loom_proto::event::{SourceRecord, WriteState, payload_write_state};
use loom_proto::num::U64;
use loom_proto::worker::{Call, Input, InputLeaseEnd, Lane, Latch, WorkerError, Write};
use loom_worker_sdk::{
    BoxFuture, Clock, EmitError, EmitSink, Emitter, JobCmd, LatchFlag, Worker, WorkerCtx, WriteOutcome,
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::hub::{Hub, lock};
use crate::journal::Journal;

/// What the gateway can ask of a worker.
pub trait WorkerHost: Send + Sync + fmt::Debug {
    /// Worker kind.
    fn kind(&self) -> &str;
    /// Generic call (open, close, status …).
    fn call(&self, call: Call) -> BoxFuture<'_, Result<Value, WorkerError>>;
    /// A write that passed the pipeline.
    fn write(&self, write: Write) -> BoxFuture<'_, WriteOutcome>;
    /// Job command.
    fn job(&self, job: JobCmd) -> BoxFuture<'_, Result<Value, WorkerError>>;
    /// Interactive input (lease checked by the gateway).
    fn input(&self, input: Input) -> BoxFuture<'_, Result<(), WorkerError>>;
    /// STOP latch on/off; resolves on the worker's ack.
    fn latch(&self, latch: Latch) -> BoxFuture<'_, Result<(), WorkerError>>;
    /// The viewer holding an input lease went away (or released it).
    fn input_lease_end(&self, end: InputLeaseEnd) -> BoxFuture<'_, Result<(), WorkerError>> {
        let _ = end;
        Box::pin(async { Ok(()) })
    }
    /// State for `core.status` (`{state, …}`).
    fn state(&self) -> Value;
    /// The worker writes its own journal (IPC workers). `false` = the gateway journals its events
    /// (in-process workers): see [`HubSink::new`].
    fn journals_itself(&self) -> bool {
        false
    }
    /// Leave quarantine / incompatible and start again (`core.worker.restart`). No-op in-process.
    fn restart(&self) {}
    /// OS pid of the worker process (`None` in-process).
    fn pid(&self) -> Option<u32> {
        None
    }
    /// Graceful stop after the shutdown latch was acked: release the device now (IPC: close the
    /// lanes, wait ≤ 2 s for the process, then kill). No-op in-process.
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

fn worker_down(kind: &str) -> WorkerError {
    WorkerError {
        code: ErrorCode::WorkerDown,
        message: format!("worker {kind} is not running"),
        detail: Value::Null,
    }
}

/// Longest wait for a worker process to exit on graceful shutdown before it is killed.
pub const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Write rows the gateway is waiting on: request id → success terminal state.
#[derive(Debug, Default)]
pub struct PendingWrites {
    map: Mutex<HashMap<String, (WriteState, bool)>>,
}

/// Always-terminal states (contract 02 Q11).
pub fn is_always_terminal(s: WriteState) -> bool {
    matches!(
        s,
        WriteState::Failed
            | WriteState::OutcomeUnknown
            | WriteState::EffectiveVerified
            | WriteState::EffectUnverified
    )
}

impl PendingWrites {
    /// Track `request_id` whose success terminal is `terminal`.
    pub fn begin(&self, request_id: &str, terminal: WriteState) {
        lock(&self.map).insert(request_id.to_owned(), (terminal, false));
    }

    /// Mark `payload.final` on the terminal row of a tracked request (called on the way to the hub).
    pub fn mark(&self, rec: &mut SourceRecord) {
        let Some(rid) = &rec.request_id else { return };
        let Ok(Some(st)) = payload_write_state(&rec.payload) else {
            return;
        };
        let mut m = lock(&self.map);
        if let Some((terminal, seen)) = m.get_mut(rid)
            && !*seen
            && (st == *terminal || is_always_terminal(st))
            && let Some(o) = rec.payload.as_object_mut()
        {
            o.insert("final".into(), Value::Bool(true));
            *seen = true;
        }
    }

    /// Stop tracking; `true` if a final row was already published.
    pub fn end(&self, request_id: &str) -> bool {
        lock(&self.map).remove(request_id).is_some_and(|(_, seen)| seen)
    }
}

/// Worker events → hub.
#[derive(Debug)]
pub struct HubSink {
    hub: Hub,
    pending: Arc<PendingWrites>,
    journal: Option<Arc<Journal>>,
}

impl HubSink {
    /// New sink.
    /// New sink. `journal` = write every record to the recording first (in-process workers, which
    /// have no journal of their own); `None` for IPC workers (they journal in their process).
    pub fn new(hub: Hub, pending: Arc<PendingWrites>, journal: Option<Arc<Journal>>) -> Arc<Self> {
        Arc::new(HubSink {
            hub,
            pending,
            journal,
        })
    }
}

impl EmitSink for HubSink {
    fn try_push(&self, lane: Lane, mut record: SourceRecord) -> Result<(), EmitError> {
        self.pending.mark(&mut record);
        // Original data goes to disk before (and regardless of) the screen path (contract 06 §6.1).
        if let Some(j) = &self.journal {
            j.append(&record);
        }
        match lane {
            Lane::Ctl => {
                // Priority: blocks only in the abnormal case where even the reserve is full.
                drop(self.hub.publish(vec![record]));
                Ok(())
            }
            Lane::Data => self.hub.try_publish(vec![record]).map_err(|r| {
                self.hub.count_drops(&r);
                EmitError::Full
            }),
        }
    }
}

enum Req {
    Call(Call, oneshot::Sender<Result<Value, WorkerError>>),
    Write(Write, oneshot::Sender<WriteOutcome>),
    Job(JobCmd, oneshot::Sender<Result<Value, WorkerError>>),
    Input(Input, oneshot::Sender<Result<(), WorkerError>>),
    LeaseEnd(InputLeaseEnd),
    Latch(bool),
}

/// A worker running inside `loomd` (tokio task).
pub struct InProcessHost {
    kind: String,
    instance: String,
    tx: mpsc::Sender<Req>,
    latch: LatchFlag,
    alive: Arc<AtomicBool>,
    writes: Arc<AtomicU64>,
}

impl fmt::Debug for InProcessHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessHost")
            .field("kind", &self.kind)
            .field("latch", &self.latch)
            .finish_non_exhaustive()
    }
}

impl InProcessHost {
    /// Start `worker` for `domain`. Its events go to `sink`; `clock` stamps them.
    pub fn spawn<W: Worker>(
        worker: W,
        domain: &str,
        instance: &str,
        sink: Arc<dyn EmitSink>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        let kind = W::KIND.to_owned();
        let latch = LatchFlag::new();
        let base = Emitter::new(
            "loomd",
            domain,
            format!("worker.{instance}"),
            U64(1),
            1,
            "",
            sink,
            clock.clone(),
        );
        let cx = WorkerCtx::new(
            latch.clone(),
            clock,
            Arc::new(move |ch: &str| base.for_channel(ch)),
        );
        let (tx, mut rx) = mpsc::channel::<Req>(256);
        let alive = Arc::new(AtomicBool::new(true));
        let writes = Arc::new(AtomicU64::new(0));
        let (a2, w2) = (alive.clone(), writes.clone());
        tokio::spawn(async move {
            let mut w = worker;
            while let Some(r) = rx.recv().await {
                match r {
                    Req::Call(c, ack) => drop(ack.send(w.on_call(&cx, c).await)),
                    Req::Write(x, ack) => {
                        w2.fetch_add(1, Ordering::Relaxed);
                        drop(ack.send(w.on_write(&cx, x).await));
                    }
                    Req::Job(j, ack) => drop(ack.send(w.on_job(&cx, j).await)),
                    Req::Input(i, ack) => drop(ack.send(w.on_input(&cx, i).await)),
                    Req::LeaseEnd(end) => w.on_input_lease_end(&cx, end).await,
                    Req::Latch(on) => w.on_latch(on),
                }
            }
            a2.store(false, Ordering::Release);
        });
        Arc::new(InProcessHost {
            kind,
            instance: instance.to_owned(),
            tx,
            latch,
            alive,
            writes,
        })
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Req) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(make(tx)).await.ok()?;
        rx.await.ok()
    }
}

impl WorkerHost for InProcessHost {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Result<Value, WorkerError>> {
        Box::pin(async move {
            self.ask(|t| Req::Call(call, t))
                .await
                .unwrap_or_else(|| Err(worker_down(&self.kind)))
        })
    }

    fn write(&self, write: Write) -> BoxFuture<'_, WriteOutcome> {
        Box::pin(async move {
            self.ask(|t| Req::Write(write, t))
                .await
                .unwrap_or_else(|| WriteOutcome {
                    state: WriteState::OutcomeUnknown,
                    detail: Value::Null,
                    error: Some(worker_down(&self.kind)),
                })
        })
    }

    fn job(&self, job: JobCmd) -> BoxFuture<'_, Result<Value, WorkerError>> {
        Box::pin(async move {
            self.ask(|t| Req::Job(job, t))
                .await
                .unwrap_or_else(|| Err(worker_down(&self.kind)))
        })
    }

    fn input(&self, input: Input) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            self.ask(|t| Req::Input(input, t))
                .await
                .unwrap_or_else(|| Err(worker_down(&self.kind)))
        })
    }

    fn input_lease_end(&self, end: InputLeaseEnd) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            self.tx
                .send(Req::LeaseEnd(end))
                .await
                .map_err(|_| worker_down(&self.kind))
        })
    }

    fn latch(&self, latch: Latch) -> BoxFuture<'_, Result<(), WorkerError>> {
        Box::pin(async move {
            // The flag is shared memory: set it first so a write in progress sees it at its next
            // unit, then tell the worker (cancel queued work) without waiting behind that write.
            self.latch.set(latch.on);
            if !self.alive.load(Ordering::Acquire) {
                return Err(worker_down(&self.kind));
            }
            match self.tx.try_send(Req::Latch(latch.on)) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => Ok(()),
                Err(mpsc::error::TrySendError::Closed(_)) => Err(worker_down(&self.kind)),
            }
        })
    }

    fn state(&self) -> Value {
        json!({
            "kind": self.kind,
            "instance": self.instance,
            "host": "in_process",
            "state": if self.alive.load(Ordering::Acquire) { "running" } else { "down" },
            "latch": self.latch.is_on(),
            "writes": self.writes.load(Ordering::Relaxed),
        })
    }
}

/// IPC worker (`loomd worker <kind>` or a domain executable) — same semantics as
/// [`InProcessHost`], plus supervision (restart, quarantine) in `loom-worker-host`. The worker
/// writes its own journal, so the gateway does not record its events again.
impl WorkerHost for loom_worker_host::IpcHost {
    fn kind(&self) -> &str {
        loom_worker_host::IpcHost::kind(self)
    }
    fn call(&self, c: Call) -> BoxFuture<'_, Result<Value, WorkerError>> {
        loom_worker_host::IpcHost::call(self, c)
    }
    fn write(&self, w: Write) -> BoxFuture<'_, WriteOutcome> {
        loom_worker_host::IpcHost::write(self, w)
    }
    fn job(&self, j: JobCmd) -> BoxFuture<'_, Result<Value, WorkerError>> {
        loom_worker_host::IpcHost::job(self, j)
    }
    fn input(&self, i: Input) -> BoxFuture<'_, Result<(), WorkerError>> {
        loom_worker_host::IpcHost::input(self, i)
    }
    fn latch(&self, l: Latch) -> BoxFuture<'_, Result<(), WorkerError>> {
        loom_worker_host::IpcHost::latch(self, l)
    }
    fn input_lease_end(&self, end: InputLeaseEnd) -> BoxFuture<'_, Result<(), WorkerError>> {
        loom_worker_host::IpcHost::input_lease_end(self, end)
    }
    fn state(&self) -> Value {
        loom_worker_host::IpcHost::state(self)
    }
    fn journals_itself(&self) -> bool {
        true
    }
    fn restart(&self) {
        loom_worker_host::IpcHost::restart(self);
    }
    fn pid(&self) -> Option<u32> {
        loom_worker_host::IpcHost::pid(self)
    }
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(loom_worker_host::IpcHost::shutdown_graceful(self, SHUTDOWN_GRACE))
    }
}
