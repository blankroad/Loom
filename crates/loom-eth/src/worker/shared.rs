use super::*;
use loom_proto::event::{Dir, Env, Origin, TsSource, WriteState};
use loom_proto::worker::{WorkerError, Write};
use loom_worker_sdk::{Emitter, EventDraft};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

/// UTF-8 bytes per identifier; reject longer control inputs before cloning.
pub const MAX_ID_BYTES: usize = 256;
/// Maximum serialized control request; reject before deserializing/cloning its fields.
pub const MAX_REQUEST_JSON: usize = 32 * 1024;
/// Outstanding transmit commands; reject newest with E_WORKER_BUSY.
pub const MAX_TX_COMMANDS: usize = 64;
/// Each command is ≤9 KiB + bounded identifiers/preview; total queue charge ≤1 MiB.
pub const MAX_TX_QUEUE_BYTES: usize = 1024 * 1024;
/// Frame bytes per write; reject before hex decoding (jumbo ceiling).
pub const MAX_SEND_BYTES: usize = 9 * 1024;
/// Recent transmit entries; evict oldest on overflow, expire after two seconds.
pub const MAX_RECENT_TX: usize = 256;
/// Includes Ethernet + IPv6/TCP headers for the largest owned socket payload; reject larger copies.
pub const MAX_RECENT_FRAME_BYTES: usize = MAX_SEND_BYTES + 128;
/// Recent frame copies are bounded independently of the entry count.
pub const MAX_RECENT_TX_BYTES: usize = MAX_RECENT_TX * MAX_RECENT_FRAME_BYTES;
pub const CAP_TX_TTL_NS: u64 = 2_000_000_000;
/// Compiled interest filters; reject replacement atomically if over count/text limits.
pub const MAX_INTEREST_FILTERS: usize = 16;
pub const MAX_INTEREST_TEXT_BYTES: usize = 8192;

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
pub(crate) fn error(code: ErrorCode, msg: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: msg.into(),
        detail: Value::Null,
    }
}
pub(crate) fn capture_error(e: loom_eth_capture::CaptureError) -> WorkerError {
    use loom_eth_capture::CaptureError as E;
    let code = match &e {
        E::Permission(_) => ErrorCode::Permission,
        E::Invalid(_) | E::Format(_) | E::Bpf { .. } => ErrorCode::Usage,
        E::Limit(_) => ErrorCode::WorkerBusy,
        E::Unsupported(_) => ErrorCode::NotImplemented,
        _ => ErrorCode::Disconnected,
    };
    error(code, e.to_string())
}
pub(crate) fn bounded_text(s: &str, max: usize) -> String {
    let mut n = s.len().min(max);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_owned()
}
#[derive(Debug)]
pub(crate) struct LimitedJson {
    pub bytes: Vec<u8>,
    limit: usize,
}
impl LimitedJson {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit),
            limit,
        }
    }
}
impl std::io::Write for LimitedJson {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("JSON byte ceiling"));
        }
        self.bytes.extend(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn request_bound(v: &Value) -> Result<(), WorkerError> {
    serde_json::to_writer(LimitedJson::new(MAX_REQUEST_JSON), v)
        .map_err(|_| error(ErrorCode::Usage, "request exceeds 32 KiB"))
}
#[derive(Debug, Default)]
pub(crate) struct Metrics {
    pub bytes: AtomicU64,
    pub capture_dropped: AtomicU64,
    pub display_lost: AtomicU64,
    pub total: AtomicU64,
    pub shown: AtomicU64,
    pub emit_failed: AtomicU64,
    pub unrecorded: AtomicU64,
    pub decode_lag_ms: AtomicU64,
    pub decode_resets: AtomicU64,
    pub timestamp_fallbacks: AtomicU64,
}
impl Metrics {
    pub fn reset(&self) {
        for a in [
            &self.bytes,
            &self.capture_dropped,
            &self.display_lost,
            &self.total,
            &self.shown,
            &self.emit_failed,
            &self.unrecorded,
            &self.decode_lag_ms,
            &self.decode_resets,
            &self.timestamp_fallbacks,
        ] {
            a.store(0, Ordering::Relaxed);
        }
    }
    pub fn add(a: &AtomicU64, n: u64) {
        let _ = a.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
            Some(old.saturating_add(n))
        });
    }
    pub fn get(a: &AtomicU64) -> u64 {
        a.load(Ordering::Relaxed)
    }
}
#[derive(Debug, Clone)]
pub(crate) struct TxMeta {
    pub request_id: String,
    pub client_id: Option<String>,
    pub origin: Origin,
    pub env: Env,
    pub row: crate::model::TxPayload,
}
impl TxMeta {
    pub fn new(w: &Write, p: &crate::model::SendParams, len: usize, bytes: &[u8], env: Env) -> Self {
        let preview = bytes
            .iter()
            .take(crate::model::TX_PREVIEW_BYTES)
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Self {
            request_id: w.request_id.clone(),
            client_id: w.client_id.clone(),
            origin: w.origin,
            env,
            row: crate::model::TxPayload {
                ch: p.ch.clone(),
                kind: p.kind.clone(),
                to: p.to.clone(),
                len: len as u32,
                hex_preview: preview,
                write_state: "requested".into(),
                observed_src_seq: None,
            },
        }
    }
    pub fn emit(&self, em: &Emitter, metrics: &Metrics, state: WriteState, observed: Option<u64>) -> bool {
        let mut row = self.row.clone();
        row.write_state = match state {
            WriteState::Requested => "requested",
            WriteState::Accepted => "accepted",
            WriteState::Sent => "sent",
            WriteState::Confirmed => "confirmed",
            WriteState::Failed => "failed",
            _ => "outcome_unknown",
        }
        .into();
        row.observed_src_seq = observed.map(loom_proto::num::U64);
        let payload = serde_json::to_value(row).unwrap_or(Value::Null);
        let draft = EventDraft {
            kind: crate::model::KIND_TX.into(),
            ts: em.clock().ts(TsSource::Driver),
            dir: Some(Dir::Tx),
            origin: self.origin,
            client_id: self.client_id.clone(),
            request_id: Some(self.request_id.clone()),
            job_id: None,
            generation: None,
            purpose: Some("agent_send".into()),
            risk: None,
            env: Some(self.env),
            payload,
            metrics: vec![],
            quality: vec![],
        };
        if em.priority(draft).is_err() {
            Metrics::add(&metrics.emit_failed, 1);
            Metrics::add(&metrics.display_lost, 1);
            false
        } else {
            true
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct TxObservation {
    pub sent: bool,
    pub seq: Option<u64>,
}
#[derive(Debug, Clone)]
pub(crate) struct Recent {
    pub hash: u64,
    pub bytes: Vec<u8>,
    pub meta: TxMeta,
    pub sent_ns: u64,
    pub virtual_copy: bool,
    pub udp: Option<(std::net::SocketAddr, std::net::SocketAddr)>,
    pub tcp: Option<(std::net::SocketAddr, std::net::SocketAddr)>,
    pub connecting: Option<std::net::SocketAddr>,
    pub observation: Option<Arc<Mutex<TxObservation>>>,
}
#[derive(Debug, Default)]
pub(crate) struct RecentTable {
    entries: VecDeque<Recent>,
    bytes: usize,
}
impl RecentTable {
    fn hash(bytes: &[u8]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    }
    fn expire(&mut self, now: u64) {
        while self
            .entries
            .front()
            .is_some_and(|r| now.saturating_sub(r.sent_ns) > CAP_TX_TTL_NS)
        {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.bytes.len();
            }
        }
    }
    pub fn insert(&mut self, bytes: &[u8], meta: TxMeta, now: u64, virtual_copy: bool) {
        self.expire(now);
        if bytes.len() > MAX_RECENT_FRAME_BYTES {
            return;
        }
        while self.entries.len() >= MAX_RECENT_TX || self.bytes + bytes.len() > MAX_RECENT_TX_BYTES {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.bytes.len();
            } else {
                break;
            }
        }
        self.bytes += bytes.len();
        self.entries.push_back(Recent {
            hash: Self::hash(bytes),
            bytes: bytes.to_vec(),
            meta,
            sent_ns: now,
            virtual_copy,
            udp: None,
            tcp: None,
            connecting: None,
            observation: None,
        });
    }
    pub fn insert_udp(
        &mut self,
        bytes: &[u8],
        src: std::net::SocketAddr,
        dst: std::net::SocketAddr,
        meta: TxMeta,
        now: u64,
    ) {
        self.insert(bytes, meta, now, false);
        if let Some(r) = self.entries.back_mut() {
            r.udp = Some((src, dst));
        }
    }
    pub fn tcp_attempt(
        &mut self,
        dst: std::net::SocketAddr,
        meta: TxMeta,
        now: u64,
    ) -> Arc<Mutex<TxObservation>> {
        self.insert(&[], meta, now, false);
        let observation = Arc::new(Mutex::new(TxObservation::default()));
        if let Some(r) = self.entries.back_mut() {
            r.connecting = Some(dst);
            r.observation = Some(observation.clone());
        }
        observation
    }
    pub fn tcp_bound(
        &mut self,
        observation: &Arc<Mutex<TxObservation>>,
        src: std::net::SocketAddr,
        dst: std::net::SocketAddr,
    ) {
        if let Some(r) = self.entries.iter_mut().find(|r| {
            r.observation
                .as_ref()
                .is_some_and(|o| Arc::ptr_eq(o, observation))
        }) {
            r.tcp = Some((src, dst));
        }
    }
    pub fn has_socket(&self) -> bool {
        self.entries
            .iter()
            .any(|r| r.udp.is_some() || r.observation.is_some())
    }
    pub fn find_tcp(&mut self, d: &loom_eth_decode::Decoded, now: u64) -> Option<(Recent, bool)> {
        self.expire(now);
        let (src, dst) = socket_endpoints(d, "tcp")?;
        let syn = d
            .field_values("tcp.flags.syn")
            .any(|v| v == loom_eth_decode::FieldValue::Bool(true));
        let ack = d
            .field_values("tcp.flags.ack")
            .any(|v| v == loom_eth_decode::FieldValue::Bool(true));
        self.entries
            .iter()
            .rev()
            .find(|r| r.tcp == Some((src, dst)) || (syn && !ack && r.connecting == Some(dst)))
            .cloned()
            .map(|r| (r, true))
    }
    pub fn find_udp(&mut self, d: &loom_eth_decode::Decoded, now: u64) -> Option<(Recent, bool)> {
        use loom_eth_decode::FieldValue;
        let addr = |v| match v {
            FieldValue::Ipv4(a) => Some(std::net::IpAddr::V4(a)),
            FieldValue::Ipv6(a) => Some(std::net::IpAddr::V6(a)),
            _ => None,
        };
        let src = d
            .field_values("ip.src")
            .next()
            .or_else(|| d.field_values("ipv6.src").next())
            .and_then(addr)?;
        let dst = d
            .field_values("ip.dst")
            .next()
            .or_else(|| d.field_values("ipv6.dst").next())
            .and_then(addr)?;
        let number = |name: &str| {
            d.field_values(name).find_map(|v| {
                if let FieldValue::Uint(n) = v {
                    Some(n)
                } else {
                    None
                }
            })
        };
        let sp = number("udp.srcport")?;
        let dp = number("udp.dstport")?;
        let declared = number("udp.length")?.checked_sub(8)?;
        let layer = d.layers.iter().find(|l| l.name == "udp")?;
        let payload = d.layer_bytes(layer).get(8..)?;
        self.expire(now);
        let recent = self.entries.iter().rev().find(|r| {
            r.udp.is_some_and(|(s, t)| {
                s.ip() == src && t.ip() == dst && u64::from(s.port()) == sp && u64::from(t.port()) == dp
            }) && r.bytes.len() as u64 == declared
                && r.bytes.starts_with(payload)
        })?;
        Some((recent.clone(), true))
    }
    pub fn find(&mut self, bytes: &[u8], origlen: u32, now: u64) -> Option<(Recent, bool)> {
        self.expire(now);
        let hash = Self::hash(bytes);
        let mut hits = self.entries.iter().rev().filter(|r| {
            r.udp.is_none()
                && r.observation.is_none()
                && r.bytes.len() == origlen as usize
                && if r.bytes.len() == bytes.len() {
                    r.hash == hash && r.bytes == bytes
                } else {
                    !bytes.is_empty() && r.bytes.starts_with(bytes)
                }
        });
        let result = hits.next()?.clone();
        let ambiguous =
            bytes.len() != origlen as usize || hits.any(|r| r.meta.request_id != result.meta.request_id);
        Some((result, ambiguous))
    }
}
#[derive(Debug)]
struct Cell<T> {
    value: Option<T>,
    waker: Option<Waker>,
    cancelled: bool,
}
#[derive(Debug)]
pub(crate) struct Reply<T>(Arc<Mutex<Cell<T>>>);
#[derive(Debug)]
pub(crate) struct Complete<T>(Arc<Mutex<Cell<T>>>);
pub(crate) fn reply<T>() -> (Reply<T>, Complete<T>) {
    let s = Arc::new(Mutex::new(Cell {
        value: None,
        waker: None,
        cancelled: false,
    }));
    (Reply(s.clone()), Complete(s))
}
impl<T> Complete<T> {
    fn store(self, value: T) -> Option<Waker> {
        let mut s = lock(&self.0);
        if s.cancelled {
            drop(s);
            drop(value);
            None
        } else {
            s.value = Some(value);
            s.waker.take()
        }
    }
    pub fn finish(self, value: T) {
        if let Some(w) = self.store(value) {
            w.wake();
        }
    }
}
impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        let value = {
            let mut s = lock(&self.0);
            s.cancelled = true;
            s.waker = None;
            s.value.take()
        };
        drop(value);
    }
}
/// One background control operation per worker; refuse additional work, never grow a pool.
pub const MAX_CONTROL_TASKS: usize = 1;
struct ControlPermit(Arc<AtomicUsize>);
impl Drop for ControlPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl<T> Future for Reply<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut s = lock(&self.0);
        if let Some(v) = s.value.take() {
            Poll::Ready(v)
        } else {
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
pub(crate) fn background<T: Send + 'static>(
    budget: Arc<AtomicUsize>,
    job: impl FnOnce() -> Result<T, WorkerError> + Send + 'static,
) -> Reply<Result<T, WorkerError>> {
    let (r, c) = reply();
    if budget
        .compare_exchange(0, MAX_CONTROL_TASKS, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        c.finish(Err(error(
            ErrorCode::WorkerBusy,
            "control operation already pending",
        )));
        return r;
    }
    let permit = ControlPermit(budget);
    let cell = c.0.clone();
    let spawned = std::thread::Builder::new()
        .name("loom-eth-control".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err(error(ErrorCode::Internal, "eth control task panicked")));
            let wake = c.store(result);
            drop(permit);
            if let Some(w) = wake {
                w.wake();
            }
        });
    if let Err(e) = spawned {
        Complete(cell).finish(Err(error(ErrorCode::Internal, e.to_string())));
    }
    r
}
pub(crate) fn state_event(em: &Emitter, state: &crate::model::CaptureStatePayload, metrics: &Metrics) {
    let payload = serde_json::to_value(state).unwrap_or(Value::Null);
    priority(em, crate::model::KIND_CAPTURE_STATE, payload, metrics);
}
pub(crate) fn priority(em: &Emitter, kind: &str, payload: Value, metrics: &Metrics) -> bool {
    let d = EventDraft {
        kind: kind.into(),
        ts: em.clock().ts(TsSource::HostReadComplete),
        dir: None,
        origin: Origin::Device,
        client_id: None,
        request_id: None,
        job_id: None,
        generation: None,
        purpose: None,
        risk: None,
        env: None,
        payload,
        metrics: vec![],
        quality: vec![],
    };
    if em.priority(d).is_err() {
        Metrics::add(&metrics.emit_failed, 1);
        if !kind.ends_with(".gap") {
            Metrics::add(&metrics.display_lost, 1);
        }
        false
    } else {
        true
    }
}
pub(crate) fn gap(
    em: &Emitter,
    kind: &str,
    cause: &str,
    count: Option<u64>,
    first: Option<u64>,
    last: Option<u64>,
    metrics: &Metrics,
) -> bool {
    if kind == "source.gap" {
        let count = count
            .map(|n| loom_proto::event::GapCount::Exact(loom_proto::num::U64(n)))
            .unwrap_or(loom_proto::event::GapCount::Unknown);
        if em.source_gap(cause, count).is_err() {
            Metrics::add(&metrics.emit_failed, 1);
            false
        } else {
            true
        }
    } else {
        priority(
            em,
            kind,
            json!({"cause":cause,"count":count.map(|n|n.to_string()).unwrap_or("unknown".into()),"detail":{"capture_first_src_seq":first.map(|n|n.to_string()),"capture_last_src_seq":last.map(|n|n.to_string()),"coordinate":"capture.source_seq","journal_available":false}}),
            metrics,
        )
    }
}

fn socket_endpoints(
    d: &loom_eth_decode::Decoded,
    proto: &str,
) -> Option<(std::net::SocketAddr, std::net::SocketAddr)> {
    use loom_eth_decode::FieldValue;
    let ip = |v| match v {
        FieldValue::Ipv4(a) => Some(std::net::IpAddr::V4(a)),
        FieldValue::Ipv6(a) => Some(std::net::IpAddr::V6(a)),
        _ => None,
    };
    let src = d
        .field_values("ip.src")
        .next()
        .or_else(|| d.field_values("ipv6.src").next())
        .and_then(ip)?;
    let dst = d
        .field_values("ip.dst")
        .next()
        .or_else(|| d.field_values("ipv6.dst").next())
        .and_then(ip)?;
    let port = |name: &str| {
        d.field_values(name).find_map(|v| {
            if let FieldValue::Uint(n) = v {
                u16::try_from(n).ok()
            } else {
                None
            }
        })
    };
    Some((
        std::net::SocketAddr::new(src, port(&format!("{proto}.srcport"))?),
        std::net::SocketAddr::new(dst, port(&format!("{proto}.dstport"))?),
    ))
}
