//! Command handlers. Capture control and real sends go to the channel's worker; reads run here on
//! the journal with the same decoder and filter the CLI, UI and TEST use (one result for all).

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler};
use loom_eth_decode::{Decoded, FieldValue, StreamTable, field_def};
use loom_eth_filter::Filter;
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, payload_bytes};
use serde_json::{Value, json};

use super::common::*;
use super::hooks::{classify_send, send_frame};
use crate::WORKER_KIND;
use crate::model::{FOLLOW_MAX_BYTES, QUERY_DEFAULT_ROWS};

/// Longest `wait_for` (contract single-result wait).
pub const WAIT_MAX: Duration = Duration::from_secs(3600);
/// Poll period of `wait_for` / `query` follow (journal re-scan; CORE ask P1-4).
pub const WAIT_POLL: Duration = Duration::from_millis(50);
/// Bytes one wait poll may scan.
pub const WAIT_SCAN_BYTES: u64 = 4 * 1024 * 1024;
/// Longest `query` follow wait (contract `--follow` ≤ 600 s).
pub const FOLLOW_WAIT_MAX: Duration = Duration::from_secs(600);
/// Matched REFs listed in wait/evidence results.
pub const MAX_EVIDENCE_REFS: usize = 16;
/// Rows of `stats` tables.
pub const STATS_TOP: usize = 20;
/// Distinct keys counted by `stats` (over: the rest is counted as `other`).
pub const STATS_MAX_KEYS: usize = 4096;
/// Nodes in an `eth.show` tree.
pub const SHOW_MAX_NODES: usize = 2048;
/// Packets written by one export.
pub const EXPORT_MAX_PACKETS: usize = 100_000;
/// Bytes written by one export.
pub const EXPORT_MAX_BYTES: u64 = 256 * 1024 * 1024;

fn compile(filter: Option<&str>) -> Result<Option<Filter>, DomainError> {
    match filter.filter(|f| !f.trim().is_empty()) {
        None => Ok(None),
        Some(f) => Filter::compile(f).map(Some).map_err(|e| {
            DomainError::new(ErrorCode::Usage, format!("display filter: {e}"))
                .with_detail(json!({"pos": e.pos, "expected": e.expected, "msg": e.msg}))
        }),
    }
}

/// Packet passes the optional display filter (decodes only when a filter is given).
struct Matcher {
    filter: Option<Filter>,
    streams: StreamTable,
    decoded: usize,
}

impl Matcher {
    fn new(filter: Option<Filter>) -> Matcher {
        Matcher {
            filter,
            streams: StreamTable::new(),
            decoded: 0,
        }
    }
    fn matches(&mut self, rec: &PublishedRecord) -> bool {
        let Some(f) = &self.filter else { return true };
        self.decoded += 1;
        decode_record(rec, &mut self.streams).is_some_and(|d| f.matches(&d))
    }
    fn exhausted(&self) -> bool {
        self.decoded >= MAX_DECODE_PER_REQUEST
    }
}

fn str_param<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

// --- worker-forwarded commands ---------------------------------------------------------------

/// `eth.ifaces`, `eth.capture.start`, `eth.capture.stop`, `eth.capture.status` (with `ch`).
#[derive(Debug)]
pub struct Forward(pub &'static str);

impl Handler for Forward {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            ctx.worker(WORKER_KIND)?
                .call(self.0, params, ctx.deadline())
                .await
        })
    }
}

/// `eth.capture.status`: one channel → worker; no channel → the gateway's channel book.
#[derive(Debug)]
pub struct Status(pub ChannelBook);

impl Handler for Status {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            if str_param(&params, "ch").is_some() {
                return ctx
                    .worker(WORKER_KIND)?
                    .call("eth.capture.status", params, ctx.deadline())
                    .await;
            }
            let chans: Vec<Value> = self
                .0
                .all()
                .into_iter()
                .map(|(ch, i)| json!({"ch": ch, "state": i.state, "backend": i.backend, "iface": i.iface, "generation": i.generation}))
                .collect();
            Ok(json!({"channels": chans}))
        })
    }
}

/// `eth.send`: dry-run preview here (no row, no audit, no worker); otherwise the worker sends.
#[derive(Debug)]
pub struct Send(pub ChannelBook);

impl Handler for Send {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            check_send_options(&params)?;
            if params.get("dry_run").and_then(Value::as_bool).unwrap_or(false) {
                return preview(&params, &self.0);
            }
            let rid = ctx.request_id().unwrap_or("eth-send").to_owned();
            ctx.worker(WORKER_KIND)?.write(&rid, params, ctx.deadline()).await
        })
    }
}

/// Kind-specific options (schema v1.2): `keep_open` is tcp only (Loom's own connection, never
/// someone else's stream), `from_port` is udp only.
pub fn check_send_options(params: &Value) -> Result<(), DomainError> {
    let kind = str_param(params, "kind").unwrap_or("");
    if params.get("keep_open").and_then(Value::as_bool).unwrap_or(false) && kind != "tcp" {
        return Err(usage(format!("keep_open is for send tcp only (got {kind:?})")));
    }
    if params.get("from_port").is_some_and(|v| !v.is_null()) && kind != "udp" {
        return Err(usage(format!("from_port is for send udp only (got {kind:?})")));
    }
    Ok(())
}

/// Dry-run preview `{proto, info, len, risk, reason}`.
pub fn preview(params: &Value, book: &ChannelBook) -> Result<Value, DomainError> {
    let frame = send_frame(params).ok_or_else(|| {
        usage("send needs kind raw|udp|tcp, even-length hex, and to=IP:PORT (IPv4 preview) for udp/tcp")
    })?;
    let local = str_param(params, "ch")
        .and_then(|c| book.get(c))
        .is_some_and(|i| i.is_local());
    let (risk, reason) = classify_send(params, local);
    let mut st = StreamTable::new();
    let d = loom_eth_decode::decode(
        loom_eth_decode::linktype::ETHERNET,
        &frame,
        &Default::default(),
        &mut st,
    );
    let len = match str_param(params, "kind") {
        Some("raw") => frame.len(),
        _ => super::hooks::hex_bytes(str_param(params, "hex").unwrap_or(""))
            .map(|b| b.len())
            .unwrap_or(0),
    };
    Ok(
        json!({"proto": d.summary.proto, "info": d.summary.info, "len": len, "risk": risk, "reason": reason, "dry_run": true}),
    )
}

// --- eth.filter.check -------------------------------------------------------------------------

/// `eth.filter.check{expr}`.
#[derive(Debug)]
pub struct FilterCheck;

impl Handler for FilterCheck {
    fn call<'a>(&'a self, _ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let expr = str_param(&params, "expr").ok_or_else(|| usage("filter.check needs expr"))?;
            Ok(filter_check(expr))
        })
    }
}

/// Result of a filter check (also used by the CLI tests).
pub fn filter_check(expr: &str) -> Value {
    match Filter::compile(expr) {
        Ok(f) => json!({"ok": true, "fields_used": f.fields_used()}),
        Err(e) => {
            json!({"ok": false, "error": {"pos": e.pos, "expected": e.expected, "msg": e.msg}, "fields_used": []})
        }
    }
}

// --- eth.query --------------------------------------------------------------------------------

/// `eth.query{ch, range{from?, to?}, filter?, max, detail, wait_ms?}`.
#[derive(Debug)]
pub struct Query(pub ChannelBook);

impl Handler for Query {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0)?;
            let max = params
                .get("max")
                .and_then(Value::as_u64)
                .map_or(QUERY_DEFAULT_ROWS, |m| u32::try_from(m).unwrap_or(u32::MAX))
                .clamp(1, CONTRACT_MAX_ROWS) as usize;
            let detail = params.get("detail").and_then(Value::as_bool).unwrap_or(false);
            let filter = compile(str_param(&params, "filter"))?;
            let range = params.get("range").cloned().unwrap_or(Value::Null);
            let from = cursor_param(range.get("from"))?;
            let to = cursor_param(range.get("to"))?;
            let wait = params
                .get("wait_ms")
                .and_then(Value::as_u64)
                .map(|ms| Duration::from_millis(ms).min(FOLLOW_WAIT_MAX));
            let deadline = wait.map(|w| (Instant::now() + w).min(ctx.deadline()));
            loop {
                let r = query_once(ctx, &ch, from.clone(), to.clone(), filter.clone(), max, detail).await?;
                let empty = r["rows"].as_array().is_none_or(Vec::is_empty);
                match deadline {
                    Some(dl) if empty && Instant::now() + WAIT_POLL < dl => Delay::new(WAIT_POLL).await,
                    _ => return Ok(r),
                }
            }
        })
    }
}

async fn query_once(
    ctx: &CallCtx,
    ch: &str,
    from: Option<Cursor>,
    to: Option<Cursor>,
    filter: Option<Filter>,
    max: usize,
    detail: bool,
) -> Result<Value, DomainError> {
    let mut m = Matcher::new(filter);
    let mut rows: VecDeque<Value> = VecDeque::new();
    let mut matched = 0usize;
    let mut last_seq = None;
    let mut stopped_early = false;
    let sum = scan_packets(
        ctx,
        ch,
        from.unwrap_or_else(lookback),
        to,
        SCAN_BUDGET_BYTES,
        &mut |rec| {
            if m.exhausted() {
                stopped_early = true;
                return false;
            }
            if !m.matches(rec) {
                return true;
            }
            matched += 1;
            last_seq = Some(rec.seq.get());
            // schema v1: the latest `max` rows up to `to` (older ones are counted in `matched`).
            if rows.len() == max {
                rows.pop_front();
            }
            rows.push_back(row(rec, detail));
            true
        },
    )
    .await?;
    let first = rows.front().and_then(|r| r["seq"].as_str().map(str::to_owned));
    let last = rows.back().and_then(|r| r["seq"].as_str().map(str::to_owned));
    let next = last
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .or(last_seq)
        .map(|s| format!("seq:{}", s + 1));
    // Older matches were dropped: page backwards with `--until prev_cursor`.
    let prev = first
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0 && matched > rows.len())
        .map(|s| format!("seq:{}", s - 1));
    Ok(json!({
        "ch": ch,
        "rows": rows,
        "range": {"from": first, "to": last},
        "truncated": stopped_early || matched > rows.len() || sum.budget_hit,
        "matched": matched,
        "gaps": sum.gaps,
        "gaps_omitted": sum.gaps_omitted,
        "complete": sum.complete && !stopped_early,
        "next_cursor": next,
        "prev_cursor": prev,
    }))
}

// --- eth.show ---------------------------------------------------------------------------------

/// `eth.show{ref}`: tree + bytes of one packet.
#[derive(Debug)]
pub struct Show;

impl Handler for Show {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let r = str_param(&params, "ref").ok_or_else(|| usage("show needs ref"))?;
            let (ch, epoch, seq, _) = parse_ref(r)?;
            let at = Cursor::Seq { epoch, seq };
            let mut found: Option<PublishedRecord> = None;
            scan_packets(ctx, &ch, at.clone(), Some(at), SCAN_BYTES_PER_CALL, &mut |rec| {
                if rec.seq.get() == seq {
                    found = Some(rec.clone());
                    false
                } else {
                    true
                }
            })
            .await?;
            let rec = found
                .ok_or_else(|| DomainError::new(ErrorCode::NotFound, format!("{r} not in the journal")))?;
            let mut st = StreamTable::new();
            let d = decode_record(&rec, &mut st)
                .ok_or_else(|| DomainError::new(ErrorCode::Internal, "packet has no decodable bytes"))?;
            let p = &rec.payload;
            Ok(json!({
                "ref": ref_of(&rec),
                "len": p.get("len"),
                "caplen": p.get("caplen"),
                "linktype": p.get("linktype"),
                "bytes": p.get("bytes"),
                "truncated": p.get("bytes_truncated").and_then(Value::as_bool).unwrap_or(false),
                "summary": d.summary,
                "warnings": d.warnings,
                "tree": tree(&d),
            }))
        })
    }
}

/// Layer tree `[{name, label, off, len, children}]` (`off` from byte 0 of the stored capture).
pub fn tree(d: &Decoded) -> Vec<Value> {
    let mut nodes = 0usize;
    let mut out = Vec::new();
    for l in &d.layers {
        if l.name == "loom" {
            continue;
        }
        nodes += 1;
        if nodes > SHOW_MAX_NODES {
            break;
        }
        let pos = l.pos.filter(|p| p.buf == 0);
        // Build field nodes from the flat (depth-ordered) list.
        let mut stack: Vec<(u8, Value)> = Vec::new();
        let mut top: Vec<Value> = Vec::new();
        for f in &l.fields {
            nodes += 1;
            if nodes > SHOW_MAX_NODES {
                break;
            }
            let desc = field_def(f.name).map_or(f.name, |def| def.desc);
            let shown = f.show.clone().unwrap_or_else(|| value_text(f.name, &f.value));
            let fp = f.pos.filter(|p| p.buf == 0);
            let node = json!({"name": f.name, "label": format!("{desc}: {shown}"),
                              "off": fp.map(|p| p.off), "len": fp.map(|p| p.len), "children": []});
            while let Some((dep, _)) = stack.last() {
                if *dep >= f.depth {
                    let Some((_, done)) = stack.pop() else { break };
                    attach(&mut stack, &mut top, done);
                } else {
                    break;
                }
            }
            stack.push((f.depth, node));
        }
        while let Some((_, done)) = stack.pop() {
            attach(&mut stack, &mut top, done);
        }
        out.push(json!({"name": l.name, "label": l.title, "off": pos.map(|p| p.off), "len": pos.map(|p| p.len), "children": top}));
    }
    out
}

fn attach(stack: &mut [(u8, Value)], top: &mut Vec<Value>, node: Value) {
    match stack
        .last_mut()
        .and_then(|(_, p)| p.get_mut("children"))
        .and_then(Value::as_array_mut)
    {
        Some(c) => c.push(node),
        None => top.push(node),
    }
}

fn value_text(name: &str, v: &FieldValue) -> String {
    match (v, field_def(name).map(|d| d.base)) {
        (FieldValue::Uint(n), Some(loom_eth_decode::Base::Hex)) => format!("0x{n:X}"),
        _ => v.to_string(),
    }
}

// --- eth.stats --------------------------------------------------------------------------------

/// `eth.stats{ch, by, filter?, range?}`.
#[derive(Debug)]
pub struct Stats(pub ChannelBook);

impl Handler for Stats {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0)?;
            let by = str_param(&params, "by").unwrap_or("proto").to_owned();
            if !matches!(by.as_str(), "proto" | "conv" | "endpoint" | "io") {
                return Err(usage("stats --by proto|conv|endpoint|io"));
            }
            let mut m = Matcher::new(compile(str_param(&params, "filter"))?);
            let range = params.get("range").cloned().unwrap_or(Value::Null);
            let from = cursor_param(range.get("from"))?.unwrap_or_else(lookback);
            let to = cursor_param(range.get("to"))?;
            let mut table: BTreeMap<String, (u64, u64)> = BTreeMap::new();
            let (mut packets, mut bytes, mut other) = (0u64, 0u64, 0u64);
            let mut t0: Option<u64> = None;
            let mut partial = false;
            let sum = scan_packets(ctx, &ch, from, to, SCAN_BUDGET_BYTES, &mut |rec| {
                if m.exhausted() {
                    partial = true;
                    return false;
                }
                if !m.matches(rec) {
                    return true;
                }
                let p = &rec.payload;
                let len = p.get("len").and_then(Value::as_u64).unwrap_or(0);
                let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("?").to_owned();
                let keys: Vec<String> = match by.as_str() {
                    "proto" => vec![s("proto")],
                    "conv" => {
                        let (a, b) = (s("src"), s("dst"));
                        let (a, b) = if a <= b { (a, b) } else { (b, a) };
                        let st = p
                            .get("stream")
                            .and_then(Value::as_u64)
                            .map(|n| format!(" #{n}"))
                            .unwrap_or_default();
                        vec![format!("{a} ↔ {b}{st}")]
                    }
                    "endpoint" => vec![s("src"), s("dst")],
                    _ => {
                        let t = rec.ts.mono_ns.get();
                        let base = *t0.get_or_insert(t);
                        vec![format!("{:>6}s", t.saturating_sub(base) / 1_000_000_000)]
                    }
                };
                packets += 1;
                bytes += len;
                for k in keys {
                    if let Some(e) = table.get_mut(&k) {
                        e.0 += 1;
                        e.1 += len;
                    } else if table.len() < STATS_MAX_KEYS {
                        table.insert(k, (1, len));
                    } else {
                        other += 1;
                    }
                }
                true
            })
            .await?;
            let mut rows: Vec<(String, (u64, u64))> = table.into_iter().collect();
            if by != "io" {
                rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
            }
            let shown: Vec<Value> = rows
                .iter()
                .take(STATS_TOP.max(if by == "io" { 120 } else { 0 }))
                .map(|(k, (n, b))| json!({"key": k, "packets": n, "bytes": b}))
                .collect();
            Ok(
                json!({"ch": ch, "by": by, "rows": shown, "keys": rows.len(), "other": other,
                      "total_packets": packets, "total_bytes": bytes,
                      "truncated": partial || sum.budget_hit || rows.len() > shown.len(),
                      "gaps": sum.gaps, "complete": sum.complete && !partial}),
            )
        })
    }
}

// --- eth.follow -------------------------------------------------------------------------------

/// `eth.follow{ch, proto, stream, max_bytes}`: payload bytes of one TCP stream / UDP conversation.
#[derive(Debug)]
pub struct Follow(pub ChannelBook);

impl Handler for Follow {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let ch = resolve_channel(&params, &self.0)?;
            let proto = str_param(&params, "proto").unwrap_or("tcp").to_owned();
            if proto != "tcp" && proto != "udp" {
                return Err(usage("follow proto must be tcp or udp"));
            }
            let stream = params
                .get("stream")
                .and_then(Value::as_u64)
                .ok_or_else(|| usage("follow needs stream"))?;
            let max_bytes = params
                .get("max_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(u64::from(FOLLOW_MAX_BYTES))
                .min(u64::from(FOLLOW_MAX_BYTES)) as usize;
            let range = params.get("range").cloned().unwrap_or(Value::Null);
            let from = cursor_param(range.get("from"))?.unwrap_or_else(lookback);
            let to = cursor_param(range.get("to"))?;
            let mut st = StreamTable::new();
            let mut segs: Vec<Value> = Vec::new();
            let mut used = 0usize;
            let mut truncated = false;
            let mut client: Option<String> = None;
            let mut offs = [0usize; 2];
            let mut decoded = 0usize;
            let sum = scan_packets(ctx, &ch, from, to, SCAN_BUDGET_BYTES, &mut |rec| {
                if rec.payload.get("stream").and_then(Value::as_u64) != Some(stream) {
                    return true;
                }
                decoded += 1;
                if decoded > MAX_DECODE_PER_REQUEST {
                    truncated = true;
                    return false;
                }
                let Some(d) = decode_record(rec, &mut st) else {
                    return true;
                };
                if !d.has_protocol(&proto) || d.field_values("tcp.analysis.retransmission").next().is_some() {
                    return true;
                }
                let Some(data) = transport_payload(&d, &proto) else {
                    return true;
                };
                if data.is_empty() {
                    return true;
                }
                let src = rec
                    .payload
                    .get("src")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let c = client.get_or_insert_with(|| src.clone());
                let dir = usize::from(*c != src);
                let room = max_bytes.saturating_sub(used);
                let take = data.len().min(room);
                segs.push(
                    json!({"dir": if dir == 0 { "c2s" } else { "s2c" }, "off": offs[dir], "seq": rec.seq,
                                 "data": loom_eth_decode::fmt_hex(&data[..take])}),
                );
                offs[dir] += take;
                used += take;
                if take < data.len() {
                    truncated = true;
                    return false;
                }
                true
            })
            .await?;
            Ok(
                json!({"ch": ch, "proto": proto, "stream": stream, "segments": segs, "bytes": used,
                      "truncated": truncated || sum.budget_hit, "gaps": sum.gaps, "complete": sum.complete}),
            )
        })
    }
}

/// Transport payload bytes of a decoded frame (TCP after the header for `tcp.len` bytes; UDP after 8).
fn transport_payload(d: &Decoded, proto: &str) -> Option<Vec<u8>> {
    let l = d.layers.iter().find(|l| l.name == proto)?;
    let pos = l.pos.filter(|p| p.buf == 0)?;
    let frame = d.frame();
    let start = (pos.off + pos.len) as usize;
    if proto == "tcp" {
        let n = d.field_values("tcp.len").find_map(|v| match v {
            FieldValue::Uint(n) => usize::try_from(n).ok(),
            _ => None,
        })?;
        frame.get(start..start.checked_add(n)?).map(<[u8]>::to_vec)
    } else {
        frame.get(pos.off as usize + 8..start).map(<[u8]>::to_vec)
    }
}

// --- eth.wait_for -----------------------------------------------------------------------------

/// `eth.wait_for{ch, filter, count{min, max?}, within_ms?, anchor?, timeout_ms, include_own?}`.
#[derive(Debug)]
pub struct WaitFor(pub ChannelBook);

impl Handler for WaitFor {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { wait_for(ctx, &self.0, &params).await })
    }
}

async fn wait_for(ctx: &CallCtx, book: &ChannelBook, params: &Value) -> Result<Value, DomainError> {
    let ch = resolve_channel(params, book)?;
    let expr = str_param(params, "filter")
        .ok_or_else(|| usage("wait_for needs a display filter"))?
        .to_owned();
    let filter = compile(Some(&expr))?.ok_or_else(|| usage("wait_for needs a display filter"))?;
    let cmin = params.pointer("/count/min").and_then(Value::as_u64).unwrap_or(1);
    let cmax = params.pointer("/count/max").and_then(Value::as_u64);
    if cmax.is_some_and(|m| m < cmin) {
        return Err(usage("count max < min"));
    }
    let include_own = params
        .get("include_own")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let timeout = Duration::from_millis(params.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30_000))
        .min(WAIT_MAX);
    let started = Instant::now();
    let deadline = (started + timeout).min(ctx.deadline());
    let within = params
        .get("within_ms")
        .and_then(Value::as_u64)
        .map(Duration::from_millis);
    // Window start: the anchor packet, else "now" (time-based cursor until the first event is seen).
    let anchor = match str_param(params, "anchor") {
        Some(r) => {
            let (_, epoch, seq, _) = parse_ref(r)?;
            Some((epoch, seq))
        }
        None => None,
    };
    let mut next_from: Option<Cursor> = anchor.map(|(epoch, seq)| Cursor::Seq { epoch, seq });
    let mut seen_upto: Option<u64> = anchor.map(|(_, s)| s.saturating_sub(1));
    // Packets stamped before the command started are history, not something we waited for.
    let start_wall = wall_now_ns();
    let within_ns = within.map(|w| u64::try_from(w.as_nanos()).unwrap_or(u64::MAX));
    let mut window_start_ns: Option<u64> = if anchor.is_none() { Some(start_wall) } else { None };
    let mut streams = StreamTable::new();
    let mut matched: Vec<String> = Vec::new();
    let mut count = 0u64;
    let mut gaps: Vec<Value> = Vec::new();
    let mut first_seq: Option<u64> = None;
    let mut last_seq: Option<u64> = None;
    loop {
        let from = next_from.clone().unwrap_or_else(|| Cursor::Relative {
            amount: u64::try_from(started.elapsed().as_millis())
                .unwrap_or(u64::MAX)
                .saturating_add(1000),
            unit: TimeUnit::Millis,
        });
        let mut window_over = false;
        let sum = scan_packets(ctx, &ch, from, None, WAIT_SCAN_BYTES, &mut |rec| {
            let s = rec.seq.get();
            if seen_upto.is_some_and(|u| s <= u) {
                return true;
            }
            seen_upto = Some(s);
            let t = rec.ts.wall_ns.get();
            if anchor.is_none() && t < start_wall {
                return true;
            }
            let ws = *window_start_ns.get_or_insert(t);
            if let Some(w) = within_ns
                && t.saturating_sub(ws) > w
            {
                window_over = true;
                return false;
            }
            first_seq.get_or_insert(s);
            last_seq = Some(s);
            if !include_own && is_attributed(rec) {
                return true;
            }
            if decode_record(rec, &mut streams).is_some_and(|d| filter.matches(&d)) {
                count += 1;
                if matched.len() < MAX_EVIDENCE_REFS {
                    matched.push(ref_of(rec));
                }
            }
            true
        })
        .await?;
        gaps.extend(sum.gaps.into_iter().take(MAX_GAPS.saturating_sub(gaps.len())));
        if let Some(s) = seen_upto {
            next_from = Some(Cursor::Seq {
                epoch: None,
                seq: s + 1,
            });
        }
        let now = Instant::now();
        let window_done = window_over
            || matches!((within_ns, window_start_ns), (Some(w), Some(ws)) if wall_now_ns() >= ws.saturating_add(w));
        let decided = match cmax {
            None => count >= cmin,
            Some(mx) => count > mx || window_done,
        };
        if decided || now + WAIT_POLL >= deadline {
            let ok = count >= cmin && cmax.is_none_or(|mx| count <= mx);
            let window = json!({"from_seq": first_seq, "to_seq": last_seq});
            let ev = json!({
                "ref": match (first_seq, last_seq) {
                    (Some(a), Some(b)) => Value::from(format!("eth:{ch}@seq:{a}..{b}")),
                    _ => Value::Null,
                },
                "kind": "eth.match", "filter": expr, "count": count,
                "matched_refs": matched, "gaps": gaps.len(), "complete": gaps.is_empty(),
            });
            let result =
                json!({"matched": ok, "count": count, "window": window, "evidence": [ev], "gaps": gaps});
            if ok {
                return Ok(result);
            }
            let (code, msg) = if !gaps.is_empty() {
                (
                    ErrorCode::Partial,
                    format!("{count} matching packets; losses in the window make the result inconclusive"),
                )
            } else if cmax.is_some_and(|mx| count > mx) || (decided && count < cmin) {
                (
                    ErrorCode::CountOutOfRange,
                    format!(
                        "{count} matching packets, expected {cmin}..{}",
                        cmax.map_or("∞".into(), |m| m.to_string())
                    ),
                )
            } else {
                (
                    ErrorCode::Timeout,
                    format!("{count} matching packets before the timeout (need ≥ {cmin})"),
                )
            };
            return Err(DomainError::new(code, msg).with_detail(result));
        }
        Delay::new(WAIT_POLL.min(deadline.saturating_duration_since(now))).await;
    }
}

// --- eth.export -------------------------------------------------------------------------------

/// `eth.export{ch, format:"pcapng", filter?, range?, path?}` — synchronous and bounded in M1
/// (no CORE job API for domains yet).
#[derive(Debug)]
pub struct Export(pub ChannelBook);

impl Handler for Export {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            use loom_eth_pcapng::{Interface, PcapNgWriter, TimestampResolution};
            let ch = resolve_channel(&params, &self.0)?;
            if str_param(&params, "format").is_some_and(|f| f != "pcapng") {
                return Err(usage("export supports format pcapng only"));
            }
            let mut m = Matcher::new(compile(str_param(&params, "filter"))?);
            let range = params.get("range").cloned().unwrap_or(Value::Null);
            let from = cursor_param(range.get("from"))?.unwrap_or_else(lookback);
            let to = cursor_param(range.get("to"))?;
            let path = match str_param(&params, "path") {
                Some(p) => std::path::PathBuf::from(p),
                None => std::env::temp_dir().join(format!(
                    "loom-eth-{ch}-{}.pcapng",
                    ctx.request_id().unwrap_or("export")
                )),
            };
            let file = std::fs::File::create(&path).map_err(|e| {
                DomainError::new(
                    ErrorCode::Internal,
                    format!("cannot create {}: {e}", path.display()),
                )
            })?;
            let io = |e: loom_eth_pcapng::Error| {
                DomainError::new(ErrorCode::Internal, format!("pcapng write: {e}"))
            };
            let mut w = PcapNgWriter::new(std::io::BufWriter::new(file)).map_err(io)?;
            let mut ifaces: BTreeMap<u16, u32> = BTreeMap::new();
            let (mut packets, mut bytes) = (0usize, 0u64);
            let mut comments_dropped = 0usize;
            let mut err: Option<DomainError> = None;
            let mut truncated = false;
            let sum = scan_packets(ctx, &ch, from, to, SCAN_BUDGET_BYTES, &mut |rec| {
                if packets >= EXPORT_MAX_PACKETS || bytes >= EXPORT_MAX_BYTES || m.exhausted() {
                    truncated = true;
                    return false;
                }
                if !m.matches(rec) {
                    return true;
                }
                let p = &rec.payload;
                let (Ok(Some(data)), Some(lt)) = (
                    payload_bytes(p),
                    p.get("linktype")
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok()),
                ) else {
                    return true;
                };
                let id = match ifaces.get(&lt) {
                    Some(id) => *id,
                    None => match w.write_interface(&Interface {
                        linktype: lt,
                        snaplen: 65535,
                        name: ch.clone(),
                        tsresol: TimestampResolution::NANO,
                    }) {
                        Ok(id) => {
                            ifaces.insert(lt, id);
                            id
                        }
                        Err(e) => {
                            err = Some(io(e));
                            return false;
                        }
                    },
                };
                let (pkt, dropped) = super::export::to_packet(rec, id, data);
                comments_dropped += dropped;
                let caplen = pkt.caplen;
                if let Err(e) = w.write_packet(&pkt) {
                    err = Some(io(e));
                    return false;
                }
                packets += 1;
                bytes += u64::from(caplen);
                true
            })
            .await?;
            if let Some(e) = err {
                return Err(e);
            }
            w.flush().map_err(io)?;
            Ok(
                json!({"path": path.display().to_string(), "packets": packets, "bytes": bytes,
                      "truncated": truncated || sum.budget_hit, "comments_dropped": comments_dropped, "gaps": sum.gaps}),
            )
        })
    }
}

/// Wall clock (ns since the Unix epoch), the same clock as `ts.wall_ns` on this host.
fn wall_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}
