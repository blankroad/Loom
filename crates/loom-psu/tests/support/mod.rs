//! Test harness: the real `PsuWorker` driven with a simulated clock (agent *or* user origin —
//! `loom-testkit::FakeWorker` only writes as agent), its events published into an in-memory
//! journal, and a `CtxServices` whose worker handle forwards to it. So gateway RPCs, the worker
//! and the journal-reading handlers run together, as in `loomd`.
#![allow(dead_code)]

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft as GwDraft, JournalQuery,
    JournalReader, Registry, ScanResult, WorkerHandle,
};
use loom_proto::cursor::Cursor;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::rpc::Role;
use loom_proto::worker::{Call, Write};
use loom_psu::worker::PsuWorker;
use loom_worker_sdk::{Clock, Emitter, LatchFlag, MemorySink, SimClock, Worker, WorkerCtx, WriteOutcome};
use serde_json::{Value, json};

// --- executors --------------------------------------------------------------------------------

struct ThreadWaker(std::thread::Thread);
impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs a future to completion on this thread.
pub fn block_on<F: Future>(f: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

/// Worker callbacks never await anything real: one poll finishes them.
fn ready<F: Future>(f: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(f).as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("worker callback blocked"),
    }
}

// --- journal ----------------------------------------------------------------------------------

/// Events in seq order; `Seq` cursors honoured, every other cursor = from the start.
#[derive(Default)]
pub struct FakeJournal {
    pub events: Mutex<Vec<PublishedRecord>>,
}

impl FakeJournal {
    pub fn all(&self) -> Vec<PublishedRecord> {
        self.events.lock().expect("lock").clone()
    }
    pub fn last_seq(&self) -> u64 {
        self.events
            .lock()
            .expect("lock")
            .last()
            .map_or(0, |e| e.seq.get())
    }
}

impl JournalReader for FakeJournal {
    fn scan<'a>(
        &'a self,
        q: &'a JournalQuery,
        _limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            let from = match &q.from {
                Cursor::Seq { seq, .. } => *seq,
                _ => 0,
            };
            let until = match &q.until {
                Some(Cursor::Seq { seq, .. }) => *seq,
                _ => u64::MAX,
            };
            let all = self.events.lock().expect("lock");
            let mut out = Vec::new();
            let mut resume_from = None;
            let mut scanned_bytes = 0u64;
            let mut scanned_through = 0;
            for e in all.iter().filter(|e| e.seq.get() >= from && e.seq.get() <= until) {
                let matching = (q.filter.kinds.is_empty() || q.filter.kinds.contains(&e.kind))
                    && (q.filter.domains.is_empty() || q.filter.domains.contains(&e.domain))
                    && (q.filter.channels.is_empty()
                        || e.channel.as_ref().is_some_and(|c| q.filter.channels.contains(c)))
                    && (q.filter.origins.is_empty() || q.filter.origins.contains(&e.origin))
                    && q.request_id
                        .as_ref()
                        .is_none_or(|id| e.request_id.as_ref() == Some(id));
                let size = serde_json::to_vec(e).expect("record JSON").len() as u64;
                // The pending row stays unread when either budget is exhausted.
                if scanned_bytes.saturating_add(size) > _limit
                    || (matching && out.len() >= q.max_events as usize)
                {
                    resume_from = Some(Cursor::Seq {
                        epoch: Some(1),
                        seq: e.seq.get(),
                    });
                    break;
                }
                scanned_bytes += size;
                scanned_through = e.seq.get();
                if matching {
                    out.push(e.clone());
                }
            }
            Ok(ScanResult {
                events: out,
                complete: resume_from.is_none(),
                gaps: vec![],
                resume_from,
                scanned_bytes,
                scanned_through,
            })
        })
    }
}

// --- worker driver ----------------------------------------------------------------------------

pub struct Driver {
    pub worker: PsuWorker,
    pub ctx: WorkerCtx,
    pub clock: SimClock,
    sink: Arc<MemorySink>,
    pub journal: Arc<FakeJournal>,
    next_call: u64,
}

impl Driver {
    pub fn new(journal: Arc<FakeJournal>) -> Driver {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .expect("clock");
        let clock = SimClock::new(wall);
        let sink = Arc::new(MemorySink::new(100_000));
        let base = Emitter::new(
            "test",
            "psu",
            "psu-test",
            U64(1),
            1,
            "bench",
            sink.clone(),
            Arc::new(clock.clone()),
        );
        let factory: Arc<dyn Fn(&str) -> Emitter + Send + Sync> =
            Arc::new(move |ch: &str| base.for_channel(ch));
        let ctx = WorkerCtx::new(LatchFlag::new(), Arc::new(clock.clone()), factory);
        Driver {
            worker: PsuWorker::default(),
            ctx,
            clock,
            sink,
            journal,
            next_call: 0,
        }
    }

    /// Publishes what the worker emitted (stream seq in emit order, epoch 1).
    pub fn flush(&mut self) {
        let mut j = self.journal.events.lock().expect("lock");
        for (_, rec) in self.sink.drain() {
            let seq = j.last().map_or(1, |e| e.seq.get() + 1);
            let mut v = serde_json::to_value(&rec).expect("record json");
            v["seq"] = json!(seq.to_string());
            v["epoch"] = json!("1");
            j.push(serde_json::from_value(v).expect("published record"));
        }
    }

    pub fn call(&mut self, method: &str, args: Value) -> Result<Value, loom_proto::worker::WorkerError> {
        self.next_call += 1;
        let call = Call {
            conn_id: None,
            call_id: U64(self.next_call),
            method: method.into(),
            args,
            origin: Origin::Agent,
            client_id: Some("test#1".into()),
            actor_id: None,
            request_id: None,
            deadline_mono_ns: U64(self.clock.mono_ns() + 5_000_000_000),
            approval_ref: None,
        };
        let r = ready(self.worker.on_call(&self.ctx, call));
        self.flush();
        r
    }

    pub fn write(&mut self, rid: &str, op: Value, origin: Origin) -> WriteOutcome {
        self.next_call += 1;
        let w = Write {
            call_id: U64(self.next_call),
            request_id: rid.into(),
            write_class: WriteClass::Request,
            op,
            origin,
            client_id: Some(
                if origin == Origin::User {
                    "web:tab-1"
                } else {
                    "test#1"
                }
                .into(),
            ),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(self.clock.mono_ns() + 5_000_000_000),
        };
        let r = ready(self.worker.on_write(&self.ctx, w));
        self.flush();
        r
    }

    /// Moves simulated time and runs the host's 5 Hz poll tick.
    pub fn advance_ms(&mut self, ms: u64) -> Value {
        self.clock.advance_ns(ms * 1_000_000);
        self.call("psu.poll", json!({})).expect("poll")
    }

    pub fn latch(&mut self, on: bool) {
        self.ctx.latch().set(on);
        self.worker.on_latch(on);
    }
}

// --- services ---------------------------------------------------------------------------------

/// Worker handle forwarding to the driver; records every write op.
pub struct Bridge {
    pub driver: Mutex<Driver>,
    pub origin: Mutex<Origin>,
    pub ops: Mutex<Vec<(String, Value)>>,
}

impl WorkerHandle for Bridge {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            self.driver
                .lock()
                .expect("lock")
                .call(method, args)
                .map_err(|e| DomainError::new(e.code, e.message))
        })
    }
    fn write<'a>(
        &'a self,
        rid: &'a str,
        op: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            self.ops.lock().expect("lock").push((rid.to_owned(), op.clone()));
            let origin = *self.origin.lock().expect("lock");
            let o = self.driver.lock().expect("lock").write(rid, op, origin);
            match o.error {
                Some(e) => Err(DomainError::new(e.code, e.message).with_detail(json!({"state": o.state}))),
                None => Ok(
                    json!({"state": o.state, "readback": o.detail.get("readback"), "off_ms": o.detail.get("off_ms")}),
                ),
            }
        })
    }
}

#[derive(Default)]
pub struct NoApprovals;
impl ApprovalHandle for NoApprovals {
    fn invalidate(&self, _id: &str, _r: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

pub struct Services {
    pub journal: Arc<FakeJournal>,
    pub bridge: Bridge,
    approvals: NoApprovals,
}

impl Services {
    pub fn new() -> Arc<Services> {
        let journal = Arc::new(FakeJournal::default());
        Arc::new(Services {
            journal: journal.clone(),
            bridge: Bridge {
                driver: Mutex::new(Driver::new(journal)),
                origin: Mutex::new(Origin::Agent),
                ops: Mutex::new(vec![]),
            },
            approvals: NoApprovals,
        })
    }

    pub fn driver(&self) -> std::sync::MutexGuard<'_, Driver> {
        self.bridge.driver.lock().expect("lock")
    }
}

impl CtxServices for Services {
    fn worker(&self, _kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Ok(&self.bridge)
    }
    fn publish(&self, _d: GwDraft) -> Result<(), DomainError> {
        Ok(())
    }
    fn journal(&self) -> &dyn JournalReader {
        self.journal.as_ref()
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &self.approvals
    }
}

/// Calls a registered command as `role` (Agent → origin agent, Viewer → origin user).
pub fn call_as(
    reg: &Registry,
    svc: &Arc<Services>,
    role: Role,
    method: &str,
    params: Value,
) -> Result<Value, DomainError> {
    let spec = reg
        .commands()
        .iter()
        .find(|c| c.method == method)
        .unwrap_or_else(|| panic!("{method} registered"));
    *svc.bridge.origin.lock().expect("lock") = if role == Role::Viewer {
        Origin::User
    } else {
        Origin::Agent
    };
    let services: Arc<dyn CtxServices> = svc.clone();
    let mut ctx = CallCtx::new(
        role,
        None,
        "test",
        Some(format!("rq-{method}")),
        Instant::now() + Duration::from_secs(10),
        services,
    );
    block_on(spec.handler.call(&mut ctx, params))
}

pub fn call(reg: &Registry, svc: &Arc<Services>, method: &str, params: Value) -> Result<Value, DomainError> {
    call_as(reg, svc, Role::Agent, method, params)
}

/// Feeds every `psu.state` record of the journal to the registry's reducer (as the hub would).
pub fn feed_reducer(reg: &mut Registry, svc: &Services) -> Vec<Box<dyn loom_domain_api::Reducer>> {
    let mut rs = reg.take_reducers();
    for e in svc.journal.all() {
        for r in rs.iter_mut() {
            if r.kinds().contains(&e.kind.as_str()) {
                r.apply(&e);
            }
        }
    }
    rs
}

/// Write rows of one request id.
pub fn rows_of(svc: &Services, rid: &str) -> Vec<PublishedRecord> {
    svc.journal
        .all()
        .into_iter()
        .filter(|e| e.request_id.as_deref() == Some(rid))
        .collect()
}

pub fn state_of(o: &WriteOutcome) -> WriteState {
    o.state
}
