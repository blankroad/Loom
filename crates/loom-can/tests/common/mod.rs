//! Shared fakes for the gateway tests: a tiny executor, an in-memory journal, a worker that
//! records writes, and `CtxServices` around them.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_can::gateway::Can;
use loom_can::schema::FramePayload;
use loom_can::{KIND_CHANNEL_STATE, KIND_FRAME, WORKER_KIND_SIM};
use loom_can_frame::parse_frame;
use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft, Handler, JournalQuery,
    JournalReader, Reducer, Registry, ScanResult, WorkerHandle,
};
use loom_proto::cursor::Cursor;
use loom_proto::event::{GapData, Origin, PublishedRecord};
use loom_proto::num::U64;
use loom_proto::rpc::{Rights, Role, SubFilter};
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

// ---- tiny executor (the crate has no runtime dependency) -------------------------------------

struct ThreadWaker(thread::Thread);
impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

// ---- fake journal -----------------------------------------------------------------------------

pub struct Journal {
    pub events: Mutex<Vec<PublishedRecord>>,
    pub start: Instant,
    pub gaps: Mutex<Vec<GapData>>,
}

impl Journal {
    pub fn new() -> Arc<Self> {
        Arc::new(Journal {
            events: Mutex::new(Vec::new()),
            start: Instant::now(),
            gaps: Mutex::new(Vec::new()),
        })
    }
    pub fn now_ns(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap()
    }
    pub fn next_seq(&self) -> u64 {
        self.events.lock().unwrap().len() as u64 + 1
    }
    /// Like [`Journal::push`] but at a chosen monotonic time (ns).
    pub fn push_at(&self, r: PublishedRecord, mono_ns: u64) -> u64 {
        let seq = self.push(r);
        self.events.lock().unwrap()[seq as usize - 1].ts.mono_ns = U64(mono_ns);
        seq
    }

    pub fn push(&self, mut r: PublishedRecord) -> u64 {
        let mut ev = self.events.lock().unwrap();
        let seq = ev.len() as u64 + 1;
        r.seq = U64(seq);
        r.source_seq = U64(seq);
        r.ts.mono_ns = U64(self.now_ns());
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        r.ts.wall_ns = U64(u64::try_from(wall).unwrap());
        ev.push(r);
        seq
    }
}

pub fn passes(f: &SubFilter, r: &PublishedRecord) -> bool {
    (f.domains.is_empty() || f.domains.contains(&r.domain))
        && (f.channels.is_empty() || r.channel.as_ref().is_some_and(|c| f.channels.contains(c)))
        && (f.kinds.is_empty() || f.kinds.contains(&r.kind))
        && (f.origins.is_empty() || f.origins.contains(&r.origin))
}

impl JournalReader for Journal {
    fn scan<'a>(
        &'a self,
        q: &'a JournalQuery,
        _limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            let ev = self.events.lock().unwrap();
            let now = self.now_ns();
            let hw = ev.len() as u64;
            let from_ok = |r: &PublishedRecord| match &q.from {
                Cursor::Seq { seq, .. } => r.seq.get() >= *seq,
                Cursor::Now => r.seq.get() > hw,
                Cursor::Relative { .. } => {
                    let d = u64::try_from(q.from.relative_duration().unwrap().as_nanos()).unwrap();
                    r.ts.mono_ns.get() >= now.saturating_sub(d)
                }
                _ => true, // @last etc.: from the start in this fake
            };
            let until_ok = |r: &PublishedRecord| match &q.until {
                Some(Cursor::Seq { seq, .. }) => r.seq.get() <= *seq,
                _ => true,
            };
            let mut out = Vec::new();
            let mut resume_from = None;
            let mut scanned_bytes = 0u64;
            let mut scanned_through = 0;
            for e in ev.iter().filter(|r| from_ok(r) && until_ok(r)) {
                let matching = passes(&q.filter, e)
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
                gaps: self.gaps.lock().unwrap().clone(),
                resume_from,
                scanned_bytes,
                scanned_through,
            })
        })
    }
}

// ---- fake worker + services ---------------------------------------------------------------------

pub type OnWrite = Box<dyn Fn(&str, &Value) + Send + Sync>;

/// Scripted worker reply: `(method or request id, args/op) → result`.
pub type Reply = Box<dyn Fn(&str, &Value) -> Result<Value, DomainError> + Send + Sync>;

#[derive(Default)]
pub struct Worker {
    pub writes: Mutex<Vec<(String, Value)>>,
    pub calls: Mutex<Vec<(String, Value)>>,
    pub on_write: Mutex<Option<OnWrite>>,
    /// Replaces the default call reply when set.
    pub call_reply: Mutex<Option<Reply>>,
    /// Replaces the default write reply when set.
    pub write_reply: Mutex<Option<Reply>>,
}

impl WorkerHandle for Worker {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let reply = self.call_reply.lock().unwrap().as_ref().map(|f| f(method, &args));
            self.calls.lock().unwrap().push((method.into(), args));
            reply.unwrap_or_else(|| Ok(json!({"channels": [{"channel": "ch0", "state": "open"}]})))
        })
    }
    fn write<'a>(
        &'a self,
        rid: &'a str,
        op: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            if let Some(f) = self.on_write.lock().unwrap().as_ref() {
                f(rid, &op);
            }
            let reply = self.write_reply.lock().unwrap().as_ref().map(|f| f(rid, &op));
            self.writes.lock().unwrap().push((rid.into(), op));
            reply.unwrap_or_else(|| Ok(json!({"state": "sent", "source_seq": "41"})))
        })
    }
}

pub struct NoApprovals;
impl ApprovalHandle for NoApprovals {
    fn invalidate(&self, _: &str, _: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

pub struct Services {
    pub journal: Arc<Journal>,
    pub worker: Worker,
    pub published: Mutex<Vec<EventDraft>>,
}

impl CtxServices for Services {
    fn worker(&self, kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        assert_eq!(kind, WORKER_KIND_SIM);
        Ok(&self.worker)
    }
    fn publish(&self, draft: EventDraft) -> Result<(), DomainError> {
        self.published.lock().unwrap().push(draft);
        Ok(())
    }
    fn journal(&self) -> &dyn JournalReader {
        self.journal.as_ref()
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &NoApprovals
    }
}

pub struct Env_ {
    pub reg: Registry,
    pub svc: Arc<Services>,
    pub reducers: Vec<Box<dyn Reducer>>,
}

pub fn setup() -> Env_ {
    let (mut reg, _) = RegistryProbe::assert_ok(&Can);
    let reducers = reg.take_reducers();
    let svc = Arc::new(Services {
        journal: Journal::new(),
        worker: Worker::default(),
        published: Mutex::new(Vec::new()),
    });
    Env_ { reg, svc, reducers }
}

impl Env_ {
    pub fn handler(&self, method: &str) -> Arc<dyn Handler> {
        self.reg
            .commands()
            .iter()
            .find(|c| c.method == method)
            .unwrap()
            .handler
            .clone()
    }
    pub fn ctx(&self, role: Role) -> CallCtx {
        let rights = (role == Role::Viewer).then_some(Rights::Operate);
        CallCtx::new(
            role,
            rights,
            "agent-1#7",
            Some("r1".into()),
            Instant::now() + Duration::from_secs(20),
            self.svc.clone(),
        )
    }
    pub fn call(&self, method: &str, role: Role, params: Value) -> Result<Value, DomainError> {
        let h = self.handler(method);
        let mut ctx = self.ctx(role);
        block_on(h.call(&mut ctx, params))
    }
    pub fn published(&self) -> Vec<EventDraft> {
        std::mem::take(&mut *self.svc.published.lock().unwrap())
    }
    pub fn apply(&mut self, r: &PublishedRecord) {
        for red in &mut self.reducers {
            if red.kinds().contains(&r.kind.as_str()) {
                red.apply(r);
            }
        }
    }
}

// ---- records ------------------------------------------------------------------------------------

pub fn record(
    kind: &str,
    channel: &str,
    origin: Origin,
    dir: Option<&str>,
    payload: Value,
) -> PublishedRecord {
    serde_json::from_value(json!({
        "schema_version": 1, "session_id": "s1", "domain": "can", "channel": channel, "kind": kind,
        "seq": "0", "epoch": "1", "producer_id": "can_sim-1", "producer_epoch": "1", "source_seq": "0",
        "ts": {"mono_ns": "0", "wall_ns": "0", "clock_id": "c", "source": "synthetic"},
        "dir": dir, "origin": origin, "payload": payload,
    }))
    .unwrap()
}

pub fn frame(text: &str, origin: Origin, channel: &str) -> PublishedRecord {
    let f = parse_frame(text, false).unwrap();
    let dir = if origin == Origin::Device { "rx" } else { "tx" };
    record(
        KIND_FRAME,
        channel,
        origin,
        Some(dir),
        serde_json::to_value(FramePayload::from_frame(&f, None)).unwrap(),
    )
}

pub fn channel_state(ch: &str, state: &str, generation: u32, seq: u64) -> PublishedRecord {
    let mut r = record(
        KIND_CHANNEL_STATE,
        ch,
        Origin::Device,
        None,
        json!({"state": state, "backend": "sim", "channel": ch, "generation": generation}),
    );
    r.generation = Some(generation);
    r.seq = U64(seq);
    r
}

pub fn seqs(rows: &Value) -> Vec<u64> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_str().unwrap().parse().unwrap())
        .collect()
}
