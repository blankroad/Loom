//! Test fakes: an in-memory journal, a recording worker, frame builders and a tiny executor.
//! (`loom-testkit` has no CtxServices/JournalReader fakes yet.)
#![allow(dead_code)]

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft, JournalQuery, JournalReader,
    Registry, ScanResult, WorkerHandle,
};
use loom_proto::cursor::Cursor;
use loom_proto::event::{PayloadBytes, PublishedRecord};
use loom_proto::rpc::Role;
use serde_json::{Value, json};

// --- executor ---------------------------------------------------------------------------------

struct ThreadWaker(std::thread::Thread);
impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs a future to completion on this thread (parks until woken).
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

// --- journal ----------------------------------------------------------------------------------

/// Events in seq order. Supports `Seq` cursors; every other cursor means "from the start".
#[derive(Default)]
pub struct FakeJournal {
    pub events: Mutex<Vec<PublishedRecord>>,
    pub scans: Mutex<u32>,
}

fn seq_of(c: &Cursor) -> Option<u64> {
    match c {
        Cursor::Seq { seq, .. } => Some(*seq),
        _ => None,
    }
}

impl JournalReader for FakeJournal {
    fn scan<'a>(
        &'a self,
        q: &'a JournalQuery,
        _byte_limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            *self.scans.lock().expect("lock") += 1;
            let from = seq_of(&q.from).unwrap_or(0);
            let until = q.until.as_ref().and_then(seq_of).unwrap_or(u64::MAX);
            let all = self.events.lock().expect("lock").clone();
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
                if scanned_bytes.saturating_add(size) > _byte_limit
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
                gaps: Vec::new(),
                resume_from,
                scanned_bytes,
                scanned_through,
            })
        })
    }
}

// --- worker / services ------------------------------------------------------------------------

/// Records calls; answers `{"ok": true, "method": …}`.
#[derive(Default)]
pub struct FakeWorker {
    pub calls: Mutex<Vec<(String, Value)>>,
}

impl WorkerHandle for FakeWorker {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            self.calls.lock().expect("lock").push((method.to_owned(), args));
            Ok(json!({"ok": true, "method": method}))
        })
    }
    fn write<'a>(
        &'a self,
        rid: &'a str,
        op: Value,
        _d: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("lock")
                .push((format!("write:{rid}"), op));
            Ok(json!({"state": "sent"}))
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

#[derive(Default)]
pub struct Services {
    pub journal: FakeJournal,
    pub worker: FakeWorker,
    pub approvals: NoApprovals,
    pub published: Mutex<Vec<EventDraft>>,
}

impl CtxServices for Services {
    fn worker(&self, _kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Ok(&self.worker)
    }
    fn publish(&self, d: EventDraft) -> Result<(), DomainError> {
        self.published.lock().expect("lock").push(d);
        Ok(())
    }
    fn journal(&self) -> &dyn JournalReader {
        &self.journal
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &self.approvals
    }
}

/// Calls a registered command with an agent context.
pub fn call(
    reg: &Registry,
    svc: &Arc<Services>,
    method: &str,
    params: Value,
    budget: Duration,
) -> Result<Value, DomainError> {
    let spec = reg
        .commands()
        .iter()
        .find(|c| c.method == method)
        .expect("command registered");
    let services: Arc<dyn CtxServices> = svc.clone();
    let mut ctx = CallCtx::new(
        Role::Agent,
        None,
        "test",
        Some("rq-1".into()),
        Instant::now() + budget,
        services,
    );
    block_on(spec.handler.call(&mut ctx, params))
}

// --- records ----------------------------------------------------------------------------------

pub fn wall_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .expect("clock after 1970")
}

/// A published record of `kind` on channel `ch`.
pub fn record(seq: u64, ch: &str, kind: &str, wall_ns: u64, payload: Value) -> PublishedRecord {
    let v = json!({
        "schema_version": 1, "session_id": "s1", "domain": "eth",
        "channel": ch, "kind": kind, "seq": seq.to_string(), "epoch": "1",
        "producer_id": "eth-w1", "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": {"mono_ns": wall_ns.to_string(), "wall_ns": wall_ns.to_string(), "clock_id": "c1", "source": "hardware"},
        "origin": "device", "payload": payload,
    });
    serde_json::from_value(v).expect("valid record")
}

/// An `eth.pkt` record for `frame` (decoded once to fill the summary columns like the worker).
pub fn pkt(seq: u64, ch: &str, wall_ns: u64, frame: &[u8], own: Option<&str>) -> PublishedRecord {
    let mut t = loom_eth_decode::StreamTable::new();
    let d = loom_eth_decode::decode(1, frame, &Default::default(), &mut t);
    let bytes = serde_json::to_value(PayloadBytes::base64(frame)).expect("bytes json");
    record(
        seq,
        ch,
        "eth.pkt",
        wall_ns,
        json!({"ch": ch, "linktype": 1, "len": frame.len(), "caplen": frame.len(), "dir": "rx",
               "attributed_origin": own, "ts_src": "sim", "proto": d.summary.proto, "src": d.summary.src,
               "dst": d.summary.dst, "info": d.summary.info, "stream": null, "layers": null, "link": null,
               "bytes": bytes, "bytes_truncated": false}),
    )
}

// --- frames -----------------------------------------------------------------------------------

pub const TESTER: [u8; 4] = [192, 168, 0, 2];
pub const ECU: [u8; 4] = [192, 168, 0, 10];

/// Ethernet + IPv4 + UDP (checksums zero).
pub fn udp4(src: [u8; 4], dst: [u8; 4], sp: u16, dp: u16, payload: &[u8]) -> Vec<u8> {
    let total = (20 + 8 + payload.len()) as u16;
    let mut f = vec![0x02, 0, 0, 0, 0, 0x0A, 0x02, 0, 0, 0, 0, 0x02, 0x08, 0x00];
    f.extend([
        0x45,
        0,
        (total >> 8) as u8,
        total as u8,
        0,
        1,
        0x40,
        0,
        64,
        17,
        0,
        0,
    ]);
    f.extend(src);
    f.extend(dst);
    f.extend(sp.to_be_bytes());
    f.extend(dp.to_be_bytes());
    f.extend(((8 + payload.len()) as u16).to_be_bytes());
    f.extend([0, 0]);
    f.extend(payload);
    f
}

/// DoIP message.
pub fn doip(ty: u16, body: &[u8]) -> Vec<u8> {
    let mut p = vec![2, 0xFD];
    p.extend(ty.to_be_bytes());
    p.extend((body.len() as u32).to_be_bytes());
    p.extend(body);
    p
}

/// DoIP diagnostic message (0x8001) carrying `uds` from the tester to the ECU, over UDP 13400.
pub fn doip_uds(uds: &[u8]) -> Vec<u8> {
    let mut body = vec![0x0E, 0x00, 0x10, 0x01];
    body.extend(uds);
    udp4(TESTER, ECU, 50000, 13400, &doip(0x8001, &body))
}

/// Hex of a frame (for `eth.send` args).
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
