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
use loom_eth_decode::{
    AppProto, DecodeAs, DecodeAsRule, Decoded, Dir as DDir, FieldValue, FrameMeta, Origin as DOrigin,
    StreamTable, Transport, decode,
};
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
        "stream_proto": p.get("stream_proto").cloned().unwrap_or(Value::Null),
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
    let stream = p.get("stream").and_then(Value::as_u64);
    let proto = p.get("stream_proto").and_then(Value::as_str);
    if let Some(s) = stream {
        let target = match proto {
            Some("udp") => Some("udp.stream"),
            Some("tcp") | None => Some("tcp.stream"),
            Some(_) => None,
        };
        if let Some(target) = target {
            for f in d.layers.iter_mut().flat_map(|l| l.fields.iter_mut()) {
                if f.name == target {
                    f.value = FieldValue::Uint(s);
                }
            }
        }
    }
    if !(stream.is_some() && proto == Some("udp")) {
        for l in d.layers.iter_mut() {
            l.fields.retain(|f| f.name != "udp.stream");
        }
    }
    Some(d)
}

// ---------------------------------------------------------------------------------------------
// Read-time Decode As (operator-upgrade B3b, ETH-06): optional `decode_as` request parameter of
// eth.query / eth.show / eth.stats / eth.export. Applied only to this request's decoding.

/// `params.decode_as` → validated rules (absent/null → empty). Errors are `E_USAGE` (FUNCTIONS.md F5.7).
pub fn decode_as_param(params: &Value) -> Result<DecodeAs, DomainError> {
    let Some(v) = params.get("decode_as").filter(|v| !v.is_null()) else {
        return Ok(DecodeAs::default());
    };
    let Some(arr) = v.as_array() else {
        return Err(usage("decode_as must be an array of {transport, port, proto}"));
    };
    let mut rules: Vec<DecodeAsRule> = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let bad = |msg: &str| usage(format!("decode_as[{i}]: {msg}")).with_detail(json!({"index": i}));
        let Some(o) = item.as_object() else {
            return Err(bad("expected an object {transport, port, proto}"));
        };
        let transport = match o.get("transport").and_then(Value::as_str) {
            Some("tcp") => Transport::Tcp,
            Some("udp") => Transport::Udp,
            _ => return Err(bad("transport must be tcp or udp")),
        };
        let port = o
            .get("port")
            .and_then(Value::as_u64)
            .filter(|p| (1..=65535).contains(p))
            .and_then(|p| u16::try_from(p).ok())
            .ok_or_else(|| bad("port must be an integer 1..65535"))?;
        let proto = match o.get("proto").and_then(Value::as_str) {
            Some("doip") => AppProto::Doip,
            Some("someip") => AppProto::SomeIp,
            _ => return Err(bad("proto must be doip or someip")),
        };
        rules.push(DecodeAsRule {
            transport,
            port,
            proto,
        });
    }
    DecodeAs::new(&rules).map_err(|e| usage(e.to_string()))
}

/// Rules as the wire echo `[{"transport", "port", "proto"}]` (FUNCTIONS.md F5.8).
pub fn decode_as_json(d: &DecodeAs) -> Value {
    Value::Array(
        d.rules()
            .iter()
            .map(|r| json!({"transport": r.transport.as_str(), "port": r.port, "proto": r.proto.as_str()}))
            .collect(),
    )
}

/// A query row for a request with Decode As rules: summary columns from the re-decode when a rule
/// applied to this packet, plus `decode_as_applied` (FUNCTIONS.md F5.9).
pub fn row_redecoded(rec: &PublishedRecord, detail: bool, d: &Decoded) -> Value {
    let mut r = row(rec, detail);
    let applied = d.warnings.iter().any(|w| w.code == "decode_as");
    if applied {
        let cut = |s: &str, n: usize, empty: &str| -> String {
            if s.is_empty() {
                empty.to_owned()
            } else {
                s.chars().take(n).collect()
            }
        };
        r["proto"] = json!(d.summary.proto);
        r["src"] = json!(cut(&d.summary.src, 128, "?"));
        r["dst"] = json!(cut(&d.summary.dst, 128, "?"));
        r["info"] = json!(cut(&d.summary.info, 512, "undecoded frame"));
    }
    r["decode_as_applied"] = json!(applied);
    r
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
    /// (operator-upgrade B1) Where a continuation scan starts: the journal's `resume_from` when the
    /// byte budget stopped the scan, or `seq:<X>` of the packet whose callback returned `false`
    /// (that packet was not consumed). `None` when the scan reached the end of the range.
    pub resume_from: Option<Cursor>,
    /// (B1) Seq of the first `eth.pkt` whose callback returned `true`.
    pub first_pkt_seq: Option<u64>,
    /// (B1) Seq of the last `eth.pkt` whose callback returned `true`.
    pub last_pkt_seq: Option<u64>,
    /// (B1) The callback returned `false` (caller-side stop: found, decode budget, output cap).
    pub stopped_by_callback: bool,
}

// ---------------------------------------------------------------------------------------------
// Range scope (operator-upgrade B1, finding ETH-01): every journal read states which range it
// scanned and why it is partial. A missing `from` falls back to [`DEFAULT_LOOKBACK_SECS`] and is
// never reported as complete history.

/// Where the start of a scan range came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FromSource {
    /// The caller passed `range.from`.
    Explicit,
    /// No `range.from`: [`lookback`] was applied.
    DefaultLookback,
}

impl FromSource {
    /// Wire text: `"explicit"` | `"default_lookback"`.
    pub fn as_str(self) -> &'static str {
        match self {
            FromSource::Explicit => "explicit",
            FromSource::DefaultLookback => "default_lookback",
        }
    }
}

/// Resolved scan range of one read request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanScope {
    /// Start cursor actually used (explicit or the default look-back).
    pub from: Cursor,
    /// End cursor (`None` = up to the newest record).
    pub to: Option<Cursor>,
    /// Where `from` came from.
    pub source: FromSource,
}

/// `partial_reasons` entry: no `range.from`, only the default look-back was scanned.
pub const REASON_DEFAULT_LOOKBACK: &str = "default_lookback";
/// `partial_reasons` entry: [`SCAN_BUDGET_BYTES`] (or the caller's budget) stopped the scan.
pub const REASON_SCAN_BUDGET: &str = "scan_budget";
/// `partial_reasons` entry: the journal reported an incomplete scan without a resume point.
pub const REASON_SCAN_INCOMPLETE: &str = "scan_incomplete";
/// `partial_reasons` entry: [`MAX_DECODE_PER_REQUEST`] stopped decoding.
pub const REASON_DECODE_BUDGET: &str = "decode_budget";
/// `partial_reasons` entry: loss entries (gaps) exist in the scanned range.
pub const REASON_LOSS: &str = "loss";
/// `partial_reasons` entry: more rows matched than returned (`eth.query` `max`).
pub const REASON_ROW_CAP: &str = "row_cap";
/// `partial_reasons` entry: an output cap stopped the result (export packets/bytes, follow bytes).
pub const REASON_OUTPUT_CAP: &str = "output_cap";

/// Resolves `params.range{from?, to?}` into a [`ScanScope`]. Bad cursors → `E_USAGE` (same message
/// as [`cursor_param`]).
pub fn range_scope(params: &Value) -> Result<ScanScope, DomainError> {
    let range = params.get("range").filter(|r| r.is_object());
    let from = cursor_param(range.and_then(|r| r.get("from")))?;
    let to = cursor_param(range.and_then(|r| r.get("to")))?;
    Ok(match from {
        Some(from) => ScanScope {
            from,
            to,
            source: FromSource::Explicit,
        },
        None => ScanScope {
            from: lookback(),
            to,
            source: FromSource::DefaultLookback,
        },
    })
}

/// `{"from": "<cursor>", "to": "<cursor>"|null, "from_source": "explicit"|"default_lookback",
///   "lookback_secs": 600|null}`.
pub fn scope_json(s: &ScanScope) -> Value {
    json!({
        "from": s.from.to_string(),
        "to": s.to.as_ref().map(ToString::to_string),
        "from_source": s.source.as_str(),
        "lookback_secs": if s.source == FromSource::DefaultLookback { Some(DEFAULT_LOOKBACK_SECS) } else { None },
    })
}

/// `{"first_seq": "<n>"|null, "last_seq": "<n>"|null, "bytes": <u64>}`.
pub fn scanned_json(sum: &ScanSummary) -> Value {
    json!({
        "first_seq": sum.first_pkt_seq.map(|n| n.to_string()),
        "last_seq": sum.last_pkt_seq.map(|n| n.to_string()),
        "bytes": sum.scanned_bytes,
    })
}

/// Ordered, de-duplicated partial reasons of one read (see FUNCTIONS.md F1.6 for the order).
/// `caps` are caller-side caps ([`REASON_ROW_CAP`], [`REASON_OUTPUT_CAP`]) in the caller's order.
pub fn partial_reasons(
    scope: &ScanScope,
    sum: &ScanSummary,
    decode_stopped: bool,
    caps: &[&'static str],
) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    if scope.source == FromSource::DefaultLookback {
        v.push(REASON_DEFAULT_LOOKBACK);
    }
    if sum.budget_hit {
        v.push(REASON_SCAN_BUDGET);
    }
    if !sum.complete && !sum.budget_hit {
        v.push(REASON_SCAN_INCOMPLETE);
    }
    if decode_stopped {
        v.push(REASON_DECODE_BUDGET);
    }
    if !sum.gaps.is_empty() || sum.gaps_omitted > 0 {
        v.push(REASON_LOSS);
    }
    for c in caps {
        if !v.contains(c) {
            v.push(c);
        }
    }
    v
}

/// `"<cursor>"` to continue a scan that stopped on [`REASON_SCAN_BUDGET`] or
/// [`REASON_DECODE_BUDGET`]; `null` otherwise.
pub fn resume_cursor_json(sum: &ScanSummary, reasons: &[&'static str]) -> Value {
    let budget = reasons
        .iter()
        .any(|r| *r == REASON_SCAN_BUDGET || *r == REASON_DECODE_BUDGET);
    match &sum.resume_from {
        Some(c) if budget => Value::String(c.to_string()),
        _ => Value::Null,
    }
}

/// `complete` of a read result — the ONLY flag that may be shown as complete/whole evidence:
/// `true` iff `reasons` is empty (explicit range, fully scanned and decoded, no loss, nothing cut
/// by a row/output cap).
pub fn complete_from(reasons: &[&'static str]) -> bool {
    reasons.is_empty()
}

/// `scan_complete` of a read result — scan coverage of the APPLIED range only (which may be the
/// default look-back; see `scope.from_source`): none of scan_budget / scan_incomplete /
/// decode_budget. Never evidence of completeness by itself.
pub fn scan_complete_from(reasons: &[&'static str]) -> bool {
    !reasons
        .iter()
        .any(|r| *r == REASON_SCAN_BUDGET || *r == REASON_SCAN_INCOMPLETE || *r == REASON_DECODE_BUDGET)
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

/// (operator-upgrade C2, ETH-RT-04) A journal `E_SCAN_LIMIT` error that is really one bounded scan
/// step: the core reads the recording for a seq cursor below its ring and, at the byte limit, answers
/// `E_SCAN_LIMIT` with `detail {partial: [], resume_from, scanned_bytes}`. Returns
/// `(resume_from, scanned_bytes)` only when that step is well formed and makes progress from `from`;
/// `None` (the caller keeps the error) otherwise. FUNCTIONS.md FC2.1.
pub fn scan_limit_resume(err: &DomainError, from: &Cursor) -> Option<(Cursor, u64)> {
    if err.code != ErrorCode::ScanLimit {
        return None;
    }
    match err.detail.get("partial") {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.is_empty() => {}
        _ => return None,
    }
    let Cursor::Seq { epoch: re, seq: rs } =
        err.detail.get("resume_from")?.as_str()?.parse::<Cursor>().ok()?
    else {
        return None;
    };
    let bytes = match err.detail.get("scanned_bytes")? {
        Value::Number(n) => n.as_u64()?,
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse::<u64>().ok()?
        }
        _ => return None,
    };
    if let Cursor::Seq { epoch: fe, seq: fs } = from {
        if rs <= *fs {
            return None;
        }
        if let (Some(a), Some(b)) = (fe, re.as_ref())
            && a != b
        {
            return None;
        }
    }
    Some((Cursor::Seq { epoch: re, seq: rs }, bytes))
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
        let r = match ctx.journal().scan(&q, limit).await {
            Ok(r) => r,
            Err(e) => match scan_limit_resume(&e, &from) {
                Some((next, bytes)) => {
                    sum.scanned_bytes = sum.scanned_bytes.saturating_add(bytes);
                    if sum.scanned_bytes < budget_bytes {
                        from = next;
                        continue;
                    }
                    sum.resume_from = Some(next);
                    sum.budget_hit = true;
                    sum.complete = false;
                    return Ok(sum);
                }
                None => return Err(e),
            },
        };
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
                    if each(ev) {
                        sum.first_pkt_seq.get_or_insert(ev.seq.get());
                        sum.last_pkt_seq = Some(ev.seq.get());
                    } else {
                        sum.stopped_by_callback = true;
                        sum.resume_from = Some(Cursor::Seq {
                            epoch: None,
                            seq: ev.seq.get(),
                        });
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
            Some(next) => {
                sum.resume_from = Some(next.clone());
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
    /// (B1) Journal position `(epoch, seq)` of the `starting` capture_state record of the current
    /// generation, as observed by the reducer. `None` = not observed (joined mid-generation, daemon
    /// restarted, or no generation yet) — never guessed from packets or the current state.
    pub start_at: Option<(u64, u64)>,
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
        self.update_at(ch, payload, generation, None);
    }

    /// (B1) [`ChannelBook::update`] plus the record position `at = (epoch, seq)` of that
    /// capture_state record; maintains [`ChanInfo::start_at`] (FUNCTIONS.md F1.8).
    pub fn update_at(&self, ch: &str, payload: &Value, generation: Option<u32>, at: Option<(u64, u64)>) {
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
            e.start_at = None;
        }
        if generation.is_some() {
            e.generation = generation;
        }
        if let Some(v) = json_u64(payload.get("started_mono_ns")) {
            e.started_mono_ns = Some(v);
        }
        if payload.get("state").and_then(Value::as_str) == Some("starting") && at.is_some() {
            e.start_at = at;
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

/// operator-upgrade B1 (ETH-01) specification tests — authored with the contract (FUNCTIONS.md F1.*).
#[cfg(test)]
mod range_scope_tests {
    use super::*;
    use serde_json::json;

    fn sum() -> ScanSummary {
        ScanSummary {
            complete: true,
            ..ScanSummary::default()
        }
    }

    #[test]
    fn missing_from_is_the_default_lookback_and_says_so() {
        let s = range_scope(&json!({"ch": "x"})).expect("scope");
        assert_eq!(s.source, FromSource::DefaultLookback);
        assert_eq!(s.from, lookback());
        assert_eq!(s.to, None);
        assert_eq!(
            scope_json(&s),
            json!({"from": "-600s", "to": null, "from_source": "default_lookback", "lookback_secs": 600})
        );
        // `range` present but without `from` is the same default.
        let s = range_scope(&json!({"range": {"to": "seq:9"}})).expect("scope");
        assert_eq!(s.source, FromSource::DefaultLookback);
        assert_eq!(scope_json(&s)["to"], "seq:9");
    }

    #[test]
    fn explicit_from_keeps_epoch_and_has_no_lookback() {
        let s = range_scope(&json!({"range": {"from": "e3:seq:1370", "to": "seq:1390"}})).expect("scope");
        assert_eq!(s.source, FromSource::Explicit);
        assert_eq!(
            s.from,
            Cursor::Seq {
                epoch: Some(3),
                seq: 1370
            }
        );
        assert_eq!(
            scope_json(&s),
            json!({"from": "e3:seq:1370", "to": "seq:1390", "from_source": "explicit", "lookback_secs": null})
        );
        let s = range_scope(&json!({"range": {"from": "-1h"}})).expect("relative is explicit");
        assert_eq!(s.source, FromSource::Explicit);
        assert_eq!(scope_json(&s)["from"], "-1h");
        assert_eq!(FromSource::Explicit.as_str(), "explicit");
        assert_eq!(FromSource::DefaultLookback.as_str(), "default_lookback");
    }

    #[test]
    fn bad_cursor_is_usage_error() {
        let e = range_scope(&json!({"range": {"from": "1380"}})).expect_err("bare number is a clock");
        assert_eq!(e.code, ErrorCode::Usage);
        let e = range_scope(&json!({"range": {"from": "seq:1", "to": "bogus cursor"}})).expect_err("bad to");
        assert_eq!(e.code, ErrorCode::Usage);
    }

    #[test]
    fn reasons_are_ordered_unique_and_drive_complete() {
        let explicit = range_scope(&json!({"range": {"from": "seq:1"}})).expect("scope");
        let default = range_scope(&json!({})).expect("scope");
        let clean = sum();
        assert!(partial_reasons(&explicit, &clean, false, &[]).is_empty());
        assert!(complete_from(&partial_reasons(&explicit, &clean, false, &[])));
        assert!(scan_complete_from(&partial_reasons(
            &explicit,
            &clean,
            false,
            &[]
        )));

        // ETH-01 regression: the default look-back alone is never complete — its window may be
        // fully scanned (scan_complete) but that is scan coverage, not evidence.
        let r = partial_reasons(&default, &clean, false, &[]);
        assert_eq!(r, [REASON_DEFAULT_LOOKBACK]);
        assert!(!complete_from(&r));
        assert!(scan_complete_from(&r));

        let mut s = sum();
        s.budget_hit = true;
        s.complete = false;
        s.gaps.push(json!({"kind": "source"}));
        let r = partial_reasons(
            &default,
            &s,
            true,
            &[REASON_ROW_CAP, REASON_OUTPUT_CAP, REASON_ROW_CAP],
        );
        assert_eq!(
            r,
            [
                REASON_DEFAULT_LOOKBACK,
                REASON_SCAN_BUDGET,
                REASON_DECODE_BUDGET,
                REASON_LOSS,
                REASON_ROW_CAP,
                REASON_OUTPUT_CAP
            ]
        );

        let mut s = sum();
        s.complete = false; // journal incomplete, no budget hit
        assert_eq!(
            partial_reasons(&explicit, &s, false, &[]),
            [REASON_SCAN_INCOMPLETE]
        );

        let mut s = sum();
        s.gaps_omitted = 3; // listed gaps empty but some were omitted → still loss
        let r = partial_reasons(&explicit, &s, false, &[REASON_ROW_CAP]);
        assert_eq!(r, [REASON_LOSS, REASON_ROW_CAP]);
        // ROOT review: loss or a row cap must never read as complete evidence.
        assert!(!complete_from(&r), "any reason clears `complete`");
        assert!(scan_complete_from(&r), "loss/row_cap do not reduce scan coverage");
        for one in [
            REASON_LOSS,
            REASON_ROW_CAP,
            REASON_OUTPUT_CAP,
            REASON_DEFAULT_LOOKBACK,
        ] {
            assert!(!complete_from(&[one]), "{one}");
            assert!(scan_complete_from(&[one]), "{one}");
        }
        for one in [REASON_SCAN_BUDGET, REASON_SCAN_INCOMPLETE, REASON_DECODE_BUDGET] {
            assert!(!complete_from(&[one]), "{one}");
            assert!(!scan_complete_from(&[one]), "{one}");
        }
        assert!(complete_from(&[]) && scan_complete_from(&[]));
    }

    #[test]
    fn resume_cursor_only_for_budget_stops() {
        let mut s = sum();
        s.resume_from = Some(Cursor::Seq {
            epoch: Some(1),
            seq: 77,
        });
        assert_eq!(resume_cursor_json(&s, &[REASON_SCAN_BUDGET]), json!("e1:seq:77"));
        assert_eq!(
            resume_cursor_json(&s, &[REASON_LOSS, REASON_DECODE_BUDGET]),
            json!("e1:seq:77")
        );
        assert_eq!(resume_cursor_json(&s, &[REASON_OUTPUT_CAP]), Value::Null);
        assert_eq!(resume_cursor_json(&sum(), &[REASON_SCAN_BUDGET]), Value::Null);
    }

    #[test]
    fn scanned_reports_packet_seqs_as_strings() {
        let mut s = sum();
        assert_eq!(
            scanned_json(&s),
            json!({"first_seq": null, "last_seq": null, "bytes": 0})
        );
        s.first_pkt_seq = Some(1380);
        s.last_pkt_seq = Some(u64::MAX);
        s.scanned_bytes = 4096;
        assert_eq!(
            scanned_json(&s),
            json!({"first_seq": "1380", "last_seq": "18446744073709551615", "bytes": 4096})
        );
    }

    #[test]
    fn start_anchor_is_observed_never_guessed() {
        let b = ChannelBook::default();
        // Joined mid-generation (first record of gen 2 is `live`): unknown.
        b.update_at(
            "qa",
            &json!({"state": "live", "backend": "sim", "iface": "sim"}),
            Some(2),
            Some((1, 40)),
        );
        assert_eq!(b.get("qa").expect("qa").start_at, None);
        // New generation seen from its `starting` record.
        b.update_at(
            "qa",
            &json!({"state": "starting", "started_mono_ns": "5"}),
            Some(3),
            Some((1, 50)),
        );
        assert_eq!(b.get("qa").expect("qa").start_at, Some((1, 50)));
        // Later states of the same generation keep the anchor (also a partial payload / no position).
        b.update_at("qa", &json!({"state": "live"}), Some(3), Some((1, 51)));
        b.update_at("qa", &json!({"state": "stopped"}), Some(3), Some((1, 60)));
        b.update("qa", &json!({"state": "stopped"}), Some(3));
        let i = b.get("qa").expect("qa");
        assert_eq!(i.start_at, Some((1, 50)));
        assert_eq!(i.state, "stopped");
        assert_eq!(i.started_mono_ns, Some(5));
        // A newer generation first seen mid-way resets to unknown (never inherits gen 3's start).
        b.update_at("qa", &json!({"state": "live"}), Some(4), Some((1, 90)));
        assert_eq!(b.get("qa").expect("qa").start_at, None);
        // `update` (no position) on a new generation also resets it.
        b.update_at("qb", &json!({"state": "starting"}), Some(1), Some((2, 7)));
        assert_eq!(b.get("qb").expect("qb").start_at, Some((2, 7)));
        b.update("qb", &json!({"state": "starting"}), Some(2));
        assert_eq!(b.get("qb").expect("qb").start_at, None);
        // Without generation: a first-seen `starting` record anchors; nothing else does.
        b.update_at("qc", &json!({"state": "starting"}), None, Some((1, 3)));
        assert_eq!(b.get("qc").expect("qc").start_at, Some((1, 3)));
        b.update_at("qd", &json!({"state": "live"}), None, Some((1, 4)));
        assert_eq!(b.get("qd").expect("qd").start_at, None);
    }
}
