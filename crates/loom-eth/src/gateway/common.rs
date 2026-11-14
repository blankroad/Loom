//! Shared gateway helpers: limits, REF parsing, record → row, record decoding, bounded journal
//! scanning, a runtime-agnostic delay, and the channel book shared by hooks and the reducer.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use loom_domain_api::{CallCtx, DomainError, JournalQuery};
use loom_eth_decode::{Decoded, Dir as DDir, FieldValue, FrameMeta, Origin as DOrigin, StreamTable, decode};
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{GapCount, GapData, PublishedRecord, payload_bytes};

/// Read a 64-bit counter from a payload field. Contract v1 §6.1 sends u64 as a canonical decimal
/// string in JSON; plain numbers are accepted too. Missing or malformed → `None` (callers must not
/// treat that as zero when it decides evidence completeness).
pub(crate) fn json_u64(v: Option<&serde_json::Value>) -> Option<u64> {
    match v? {
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Number(n) => n.as_u64(),
        _ => None,
    }
}
use loom_proto::rpc::SubFilter;
use serde_json::{Value, json};

use crate::DOMAIN;
use crate::model::{KIND_BATCH, KIND_PKT};

/// Bytes one journal scan call may read. Larger ranges continue from `resume_from`.
pub const SCAN_BYTES_PER_CALL: u64 = 8 * 1024 * 1024;
/// Total bytes one query/stats/follow/export/show may scan. Over: partial result, `truncated`.
pub const SCAN_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
/// Events requested per scan call.
pub const SCAN_EVENTS_PER_CALL: u32 = 10_000;
/// Packets decoded per request (filters, follow, stats). Over: partial result.
pub const MAX_DECODE_PER_REQUEST: usize = 200_000;
/// Default look-back when a query has no `from` (journal has no reverse/tail scan yet — CORE ask).
pub const DEFAULT_LOOKBACK_SECS: u64 = 600;
/// Gap entries reported per result. Over: the rest are counted in `gaps_omitted`.
pub const MAX_GAPS: usize = 256;
/// Hard row ceiling of the CORE contract (D5); `model::QUERY_MAX_ROWS` (2000) is clamped to this.
pub const CONTRACT_MAX_ROWS: u32 = 1000;
/// Channels remembered in the channel book. Over: the oldest entry is dropped.
pub const MAX_BOOK_CHANNELS: usize = 16;

/// Event kinds a packet scan reads (packets, coverage, losses).
pub const SCAN_KINDS: &[&str] = &[KIND_PKT, KIND_BATCH, "source.gap", "record.gap"];

/// `eth:<ch>@[e<E>/]seq:<a>[..<b>]` → (channel, epoch, from, to).
pub fn parse_ref(s: &str) -> Result<(String, Option<u64>, u64, u64), DomainError> {
    let bad = || {
        DomainError::new(ErrorCode::Usage, format!("bad REF {s:?}"))
            .with_detail(json!({"expected": "eth:<ch>@seq:<n> or eth:<ch>@e<epoch>/seq:<a>..<b>"}))
    };
    let rest = s.strip_prefix("eth:").ok_or_else(bad)?;
    let (ch, pos) = rest.split_once('@').ok_or_else(bad)?;
    if ch.is_empty() {
        return Err(bad());
    }
    let (epoch, pos) = match pos.strip_prefix('e') {
        Some(r) => {
            let (e, p) = r
                .split_once('/')
                .or_else(|| r.split_once(':').filter(|(_, p)| p.starts_with("seq:")))
                .ok_or_else(bad)?;
            (Some(e.parse::<u64>().map_err(|_| bad())?), p)
        }
        None => (None, pos),
    };
    let range = pos.strip_prefix("seq:").ok_or_else(bad)?;
    let (a, b) = match range.split_once("..") {
        Some((a, b)) => (
            a.parse::<u64>().map_err(|_| bad())?,
            b.parse::<u64>().map_err(|_| bad())?,
        ),
        None => {
            let a = range.parse::<u64>().map_err(|_| bad())?;
            (a, a)
        }
    };
    if b < a {
        return Err(bad());
    }
    Ok((ch.to_owned(), epoch, a, b))
}

/// REF of a published packet.
pub fn ref_of(rec: &PublishedRecord) -> String {
    format!("{DOMAIN}:{}@seq:{}", channel_of(rec), rec.seq.get())
}

/// Channel of a record (record channel, else `payload.ch`).
pub fn channel_of(rec: &PublishedRecord) -> &str {
    rec.channel
        .as_deref()
        .or_else(|| rec.payload.get("ch").and_then(Value::as_str))
        .unwrap_or("")
}

/// True when Loom itself sent this captured frame (never device evidence, TEST R-8).
pub fn is_attributed(rec: &PublishedRecord) -> bool {
    rec.payload.get("attributed_origin").is_some_and(|v| !v.is_null())
}

/// One query/trace row (schema v1 `eth.query` rows).
pub fn row(rec: &PublishedRecord, detail: bool) -> Value {
    let p = &rec.payload;
    let mut r = json!({
        "ref": ref_of(rec),
        "seq": rec.seq,
        "ts": rec.ts,
        "dir": p.get("dir").cloned().unwrap_or(Value::Null),
        "origin": rec.origin,
        "attributed_origin": p.get("attributed_origin").cloned().unwrap_or(Value::Null),
        "len": p.get("len").cloned().unwrap_or(Value::Null),
        "caplen": p.get("caplen").cloned().unwrap_or(Value::Null),
        "proto": p.get("proto").cloned().unwrap_or(Value::Null),
        "src": p.get("src").cloned().unwrap_or(Value::Null),
        "dst": p.get("dst").cloned().unwrap_or(Value::Null),
        "info": p.get("info").cloned().unwrap_or(Value::Null),
        "stream": p.get("stream").cloned().unwrap_or(Value::Null),
        "link": p.get("link").cloned().unwrap_or(Value::Null),
    });
    if detail && let Some(o) = r.as_object_mut() {
        o.insert("layers".into(), p.get("layers").cloned().unwrap_or(Value::Null));
    }
    r
}

/// Decodes the frame of an `eth.pkt` record with the same decoder the CLI and UI rely on.
/// `tcp.stream` is replaced by the worker's number (`payload.stream`) so filters match the rows.
pub fn decode_record(rec: &PublishedRecord, streams: &mut StreamTable) -> Option<Decoded> {
    let p = &rec.payload;
    let bytes = payload_bytes(p).ok()??;
    let lt = u16::try_from(p.get("linktype")?.as_u64()?).ok()?;
    let meta = FrameMeta {
        seq: rec.seq.get(),
        ts_ns: rec.ts.mono_ns.get(),
        dir: match p.get("dir").and_then(Value::as_str) {
            Some("rx") => Some(DDir::Rx),
            Some("tx") => Some(DDir::Tx),
            _ => None,
        },
        attributed_origin: match p.get("attributed_origin").and_then(Value::as_str) {
            Some("agent") => Some(DOrigin::Agent),
            Some("user") => Some(DOrigin::User),
            Some("script") => Some(DOrigin::Script),
            _ => None,
        },
        request_id: p
            .get("link")
            .and_then(|l| l.get("request_id"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        orig_len: p
            .get("len")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok()),
    };
    let mut d = decode(lt, &bytes, &meta, streams);
    if let Some(s) = p.get("stream").and_then(Value::as_u64) {
        for f in d.layers.iter_mut().flat_map(|l| l.fields.iter_mut()) {
            if f.name == "tcp.stream" {
                f.value = FieldValue::Uint(s);
            }
        }
    }
    Some(d)
}

/// Channel to use: explicit, else the only known channel, else `E_AMBIGUOUS_CHANNEL`.
pub fn resolve_channel(params: &Value, book: &ChannelBook) -> Result<String, DomainError> {
    if let Some(ch) = params.get("ch").and_then(Value::as_str).filter(|c| !c.is_empty()) {
        return Ok(ch.to_owned());
    }
    let names = book.names();
    match names.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(DomainError::new(ErrorCode::NotOpen, "no capture channel is open")
            .with_detail(json!({"hint": "loom eth capture start <iface>"}))),
        many => Err(
            DomainError::new(ErrorCode::AmbiguousChannel, "several capture channels; pass --ch")
                .with_detail(json!({"channels": many})),
        ),
    }
}

/// Parses a cursor parameter.
pub fn cursor_param(v: Option<&Value>) -> Result<Option<Cursor>, DomainError> {
    match v.and_then(Value::as_str) {
        None => Ok(None),
        Some(s) => s
            .parse::<Cursor>()
            .map(Some)
            .map_err(|e| DomainError::new(ErrorCode::Usage, format!("bad cursor {s:?}: {e}"))),
    }
}

/// Default look-back cursor.
pub fn lookback() -> Cursor {
    Cursor::Relative {
        amount: DEFAULT_LOOKBACK_SECS,
        unit: TimeUnit::Secs,
    }
}

/// Result of a bounded multi-call scan.
#[derive(Debug, Default)]
pub struct ScanSummary {
    /// Loss entries (`eth.batch` with unpublished packets, `source.gap`, `record.gap`, scan gaps).
    pub gaps: Vec<Value>,
    /// Gap entries not listed (over [`MAX_GAPS`]).
    pub gaps_omitted: usize,
    /// The whole range was scanned (no byte budget hit, no scan-level loss).
    pub complete: bool,
    /// Bytes scanned.
    pub scanned_bytes: u64,
    /// The byte budget stopped the scan early.
    pub budget_hit: bool,
}

fn count_value(c: &GapCount) -> Value {
    match c {
        GapCount::Exact(n) => json!(n.get()),
        GapCount::Unknown => json!("unknown"),
    }
}

fn gap_entry(kind: &str, g: &GapData, after: Option<u64>) -> Value {
    json!({
        "kind": kind,
        "cause": g.cause,
        "count": count_value(&g.count),
        "first_seq": g.seq_range.map(|r| r.from.get()),
        "last_seq": g.seq_range.map(|r| r.to.get()),
        "after_seq": after,
    })
}

/// Scans `[from, until]` of channel `ch` and calls `each` for every `eth.pkt` in stream order
/// (return `false` to stop). Losses are collected into the summary. Bounded by
/// [`SCAN_BUDGET_BYTES`].
pub async fn scan_packets(
    ctx: &CallCtx,
    ch: &str,
    from: Cursor,
    until: Option<Cursor>,
    budget_bytes: u64,
    each: &mut (dyn FnMut(&PublishedRecord) -> bool + Send),
) -> Result<ScanSummary, DomainError> {
    let mut sum = ScanSummary {
        complete: true,
        ..ScanSummary::default()
    };
    let mut from = from;
    let mut last_seq: Option<u64> = None;
    loop {
        let q = JournalQuery {
            from: from.clone(),
            until: until.clone(),
            filter: SubFilter {
                channels: vec![ch.to_owned()],
                kinds: SCAN_KINDS.iter().map(|k| (*k).to_owned()).collect(),
                ..SubFilter::default()
            },
            request_id: None,
            max_events: SCAN_EVENTS_PER_CALL,
        };
        let limit = SCAN_BYTES_PER_CALL
            .min(budget_bytes.saturating_sub(sum.scanned_bytes))
            .max(1);
        let r = ctx.journal().scan(&q, limit).await?;
        sum.scanned_bytes = sum.scanned_bytes.saturating_add(r.scanned_bytes);
        let push = |sum: &mut ScanSummary, v: Value| {
            if sum.gaps.len() < MAX_GAPS {
                sum.gaps.push(v);
            } else {
                sum.gaps_omitted += 1;
            }
        };
        for g in &r.gaps {
            push(&mut sum, gap_entry("scan", g, last_seq));
        }
        if !r.complete && r.resume_from.is_none() {
            sum.complete = false;
        }
        for ev in &r.events {
            match ev.kind.as_str() {
                KIND_PKT => {
                    last_seq = Some(ev.seq.get());
                    if !each(ev) {
                        return Ok(sum);
                    }
                }
                KIND_BATCH => {
                    let total = json_u64(ev.payload.get("total")).unwrap_or(0);
                    let shown = json_u64(ev.payload.get("shown")).unwrap_or(0);
                    if total > shown {
                        push(
                            &mut sum,
                            json!({"kind": "unpublished", "cause": "publish_budget", "count": total - shown,
                                   "first_src_seq": ev.payload.get("covered_first_src_seq"),
                                   "last_src_seq": ev.payload.get("covered_last_src_seq"),
                                   "after_seq": last_seq}),
                        );
                    }
                }
                k => {
                    let kind = if k == "source.gap" { "source" } else { "record" };
                    match serde_json::from_value::<GapData>(ev.payload.clone()) {
                        Ok(g) => push(&mut sum, gap_entry(kind, &g, last_seq)),
                        Err(_) => push(
                            &mut sum,
                            json!({"kind": kind, "cause": "unknown", "count": "unknown", "after_seq": last_seq}),
                        ),
                    }
                }
            }
        }
        match r.resume_from {
            Some(next) if sum.scanned_bytes < budget_bytes => from = next,
            Some(_) => {
                sum.budget_hit = true;
                sum.complete = false;
                return Ok(sum);
            }
            None => return Ok(sum),
        }
    }
}

/// True when a gap entry list contains loss that makes absence/count claims uncertain.
pub fn has_loss(gaps: &[Value]) -> bool {
    !gaps.is_empty()
}

// ---------------------------------------------------------------------------------------------
// Runtime-agnostic delay (the domain API has no timer; CORE ask P1-4 for a bounded wait API).

#[derive(Debug, Default)]
struct DelayShared {
    fired: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// Sleeps without depending on a specific async runtime: a short helper thread wakes the task.
#[derive(Debug)]
pub struct Delay {
    until: Instant,
    shared: Arc<DelayShared>,
    started: bool,
}

impl Delay {
    /// Delay for `d`.
    pub fn new(d: Duration) -> Delay {
        Delay {
            until: Instant::now() + d,
            shared: Arc::new(DelayShared::default()),
            started: false,
        }
    }
}

impl Future for Delay {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.shared.fired.load(Ordering::Acquire) || Instant::now() >= self.until {
            return Poll::Ready(());
        }
        if let Ok(mut w) = self.shared.waker.lock() {
            *w = Some(cx.waker().clone());
        }
        if !self.started {
            self.started = true;
            let shared = Arc::clone(&self.shared);
            let until = self.until;
            std::thread::spawn(move || {
                std::thread::sleep(until.saturating_duration_since(Instant::now()));
                shared.fired.store(true, Ordering::Release);
                if let Ok(mut w) = shared.waker.lock()
                    && let Some(w) = w.take()
                {
                    w.wake();
                }
            });
        }
        Poll::Pending
    }
}

// ---------------------------------------------------------------------------------------------
// Channel book: capture channel facts folded from `eth.capture_state` (reducer writes, hooks and
// handlers read). Bounded to MAX_BOOK_CHANNELS.

/// What the gateway knows about one capture channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChanInfo {
    /// `live|replay|sim`.
    pub backend: String,
    /// Interface name.
    pub iface: String,
    /// Capture state.
    pub state: String,
    /// Record generation of the last state event.
    pub generation: Option<u32>,
    /// Worker `started_mono_ns` of the current generation, exactly as reported (worker clock domain; no conversion).
    /// Reset by a new generation, kept when a later partial payload (e.g. `stopped` without it) omits it.
    pub started_mono_ns: Option<u64>,
    /// Last update order (for eviction).
    pub touched: u64,
}

impl ChanInfo {
    /// Loopback-like targets (sim, replay or a loopback interface): sends stay on this host.
    ///
    /// `stream` is never local by channel: its loopback-only *receive* peer does not constrain
    /// `eth send udp --to`, which uses a real OS socket, and its `iface` is a free-text label
    /// (`lo0` must not lower risk). Stream sends are classified by their destination only.
    pub fn is_local(&self) -> bool {
        if self.backend == "stream" {
            return false;
        }
        self.backend == "sim"
            || self.backend == "replay"
            || matches!(self.iface.as_str(), "lo" | "lo0")
            || self.iface.to_ascii_lowercase().contains("loopback")
    }
}

/// Shared channel book.
#[derive(Debug, Clone, Default)]
pub struct ChannelBook(Arc<Mutex<(BTreeMap<String, ChanInfo>, u64)>>);

impl ChannelBook {
    /// Updates a channel from an `eth.capture_state` payload.
    pub fn update(&self, ch: &str, payload: &Value, generation: Option<u32>) {
        let Ok(mut g) = self.0.lock() else { return };
        let (map, clock) = &mut *g;
        *clock += 1;
        let tick = *clock;
        if !map.contains_key(ch) && map.len() >= MAX_BOOK_CHANNELS {
            let oldest = map.iter().min_by_key(|(_, v)| v.touched).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                map.remove(&k);
            }
        }
        let e = map.entry(ch.to_owned()).or_default();
        let s = |k: &str| payload.get(k).and_then(Value::as_str).map(str::to_owned);
        if let Some(v) = s("backend") {
            e.backend = v;
        }
        if let Some(v) = s("iface") {
            e.iface = v;
        }
        if let Some(v) = s("state") {
            e.state = v;
        }
        if generation.is_some() && generation != e.generation {
            e.started_mono_ns = None; // a new capture generation never inherits the previous start anchor
        }
        if generation.is_some() {
            e.generation = generation;
        }
        if let Some(v) = json_u64(payload.get("started_mono_ns")) {
            e.started_mono_ns = Some(v);
        }
        e.touched = tick;
    }

    /// Facts about a channel.
    pub fn get(&self, ch: &str) -> Option<ChanInfo> {
        self.0.lock().ok().and_then(|g| g.0.get(ch).cloned())
    }

    /// Known channel names.
    pub fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|g| g.0.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// All channels.
    pub fn all(&self) -> Vec<(String, ChanInfo)> {
        self.0
            .lock()
            .map(|g| g.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }
}

/// `E_USAGE` helper.
pub fn usage(msg: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Usage, msg)
}

#[cfg(test)]
mod json_u64_tests {
    use super::json_u64;
    use serde_json::json;

    #[test]
    fn reads_canonical_strings_and_numbers() {
        assert_eq!(json_u64(Some(&json!("9"))), Some(9));
        assert_eq!(json_u64(Some(&json!(9))), Some(9));
        assert_eq!(json_u64(Some(&json!("18446744073709551615"))), Some(u64::MAX));
        assert_eq!(json_u64(Some(&json!("x"))), None);
        assert_eq!(json_u64(Some(&json!(-1))), None);
        assert_eq!(json_u64(None), None);
    }
}

#[cfg(test)]
mod stream_locality_tests {
    use super::ChanInfo;
    use crate::gateway::hooks::classify_send;
    use loom_proto::event::Risk;
    use serde_json::json;

    fn chan(backend: &str, iface: &str) -> ChanInfo {
        ChanInfo {
            backend: backend.into(),
            iface: iface.into(),
            ..Default::default()
        }
    }
    #[test]
    fn stream_channel_is_never_local_even_with_loopback_label() {
        assert!(!chan("stream", "ext").is_local());
        assert!(!chan("stream", "lo0").is_local());
        assert!(!chan("stream", "loopback-sim").is_local());
        // Existing behaviour unchanged.
        assert!(chan("sim", "sim").is_local());
        assert!(chan("replay", "fixture").is_local());
        assert!(chan("live", "lo0").is_local());
        assert!(!chan("live", "en5").is_local());
    }
    #[test]
    fn stream_send_is_classified_by_destination_without_sending() {
        let local = chan("stream", "lo0").is_local();
        let remote = json!({"kind":"udp","hex":"4142","to":"203.0.113.5:40001","ch":"ext-stream"});
        let (r, why) = classify_send(&remote, local);
        assert_eq!(r, Risk::R2, "{why}");
        let lo = json!({"kind":"udp","hex":"4142","to":"127.0.0.1:40001","ch":"ext-stream"});
        let (r, why) = classify_send(&lo, local);
        assert_eq!(r, Risk::R1, "{why}");
        assert!(why.contains("loopback"));
        let bcast = json!({"kind":"udp","hex":"4142","to":"192.0.2.255:40001","ch":"ext-stream"});
        assert_eq!(classify_send(&bcast, local).0, Risk::R3);
    }
}
