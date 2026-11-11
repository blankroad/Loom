//! `FakeWorker`: drive a domain [`Worker`] in-process (no IPC, no gateway), collect what it emits.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_proto::bytes::Bytes;
use loom_proto::event::{Origin, PublishedRecord, SourceRecord};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Input, Lane, WorkerError, Write};
use loom_worker_sdk::{
    Clock, Emitter, JobCmd, LatchFlag, MemorySink, SimClock, Worker, WorkerCtx, WriteOutcome,
};
use serde_json::Value;

/// Collected sink capacity (tests only).
const CAP: usize = 100_000;

/// In-process worker driver with a simulated clock.
pub struct FakeWorker<W: Worker> {
    worker: W,
    ctx: WorkerCtx,
    clock: SimClock,
    sink: Arc<MemorySink>,
    rt: tokio::runtime::Runtime,
    next_call: u64,
    next_seq: u64,
}

impl<W: Worker> std::fmt::Debug for FakeWorker<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeWorker")
            .field("kind", &W::KIND)
            .finish_non_exhaustive()
    }
}

impl<W: Worker> FakeWorker<W> {
    /// New driver for `worker` (domain = `W::KIND`, producer `<kind>-fake`, session `test`).
    pub fn new(worker: W) -> Self {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(0));
        let clock = SimClock::new(wall);
        let sink = Arc::new(MemorySink::new(CAP));
        let base = Emitter::new(
            "test",
            W::KIND,
            format!("{}-fake", W::KIND),
            U64(1),
            1,
            "default",
            sink.clone(),
            Arc::new(clock.clone()),
        );
        let factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync> =
            Arc::new(move |ch: &str| base.for_channel(ch));
        let ctx = WorkerCtx::new(LatchFlag::new(), Arc::new(clock.clone()), factory);
        // A current-thread runtime is enough: worker callbacks must not block anyway.
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => panic!("FakeWorker runtime: {e}"),
        };
        FakeWorker {
            worker,
            ctx,
            clock,
            sink,
            rt,
            next_call: 1,
            next_seq: 1,
        }
    }

    /// Simulated clock (advance it to move time).
    pub fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// Context handed to callbacks (emitters, latch).
    pub fn ctx(&self) -> &WorkerCtx {
        &self.ctx
    }

    /// The worker itself.
    pub fn worker(&mut self) -> &mut W {
        &mut self.worker
    }

    fn call_id(&mut self) -> U64 {
        self.next_call += 1;
        U64(self.next_call)
    }

    fn deadline(&self) -> U64 {
        U64(self.clock.mono_ns() + 5_000_000_000)
    }

    /// `on_call` as an agent.
    pub fn call(&mut self, method: &str, args: Value) -> Result<Value, WorkerError> {
        let call = Call {
            conn_id: None,
            call_id: self.call_id(),
            method: method.into(),
            args,
            origin: Origin::Agent,
            client_id: Some("test#1".into()),
            actor_id: None,
            request_id: None,
            deadline_mono_ns: self.deadline(),
            approval_ref: None,
        };
        let (w, cx) = (&mut self.worker, &self.ctx);
        self.rt.block_on(w.on_call(cx, call))
    }

    /// `on_write` (request class) with `request_id` and `op`.
    pub fn write(&mut self, request_id: &str, op: Value) -> WriteOutcome {
        let write = Write {
            call_id: self.call_id(),
            request_id: request_id.into(),
            write_class: WriteClass::Request,
            op,
            origin: Origin::Agent,
            client_id: Some("test#1".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: self.deadline(),
        };
        let (w, cx) = (&mut self.worker, &self.ctx);
        self.rt.block_on(w.on_write(cx, write))
    }

    /// `on_job`.
    pub fn job(&mut self, job: JobCmd) -> Result<Value, WorkerError> {
        let (w, cx) = (&mut self.worker, &self.ctx);
        self.rt.block_on(w.on_job(cx, job))
    }

    /// `on_input` with a lease id and bytes.
    pub fn input(&mut self, lease_id: &str, bytes: &[u8], priority: bool) -> Result<(), WorkerError> {
        let input = Input {
            conn_id: None,
            channel: None,
            input_seq: None,
            lease_id: lease_id.into(),
            bytes: Bytes(bytes.to_vec()),
            priority,
            call_id: None,
        };
        let (w, cx) = (&mut self.worker, &self.ctx);
        self.rt.block_on(w.on_input(cx, input))
    }

    /// Set the STOP latch and notify the worker (as the SDK would).
    pub fn latch(&mut self, on: bool) {
        self.ctx.latch().set(on);
        self.worker.on_latch(on);
    }

    /// Drain emitted records with their lane.
    pub fn drain(&mut self) -> Vec<(Lane, SourceRecord)> {
        self.sink.drain()
    }

    /// Drain emitted records as published records (test stream seqs assigned in emit order,
    /// epoch 1) — convenient for `asserts`.
    pub fn drain_published(&mut self) -> Vec<PublishedRecord> {
        let recs = self.sink.drain();
        recs.into_iter()
            .map(|(_, r)| {
                let seq = self.next_seq;
                self.next_seq += 1;
                r.publish(U64(seq), U64(1))
            })
            .collect()
    }
}
