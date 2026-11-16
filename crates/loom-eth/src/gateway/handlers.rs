//! Command handlers. Capture control and real sends go to the channel's worker; reads run here on
//! the journal with the same decoder and filter the CLI, UI and TEST use (one result for all).

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler};
use loom_eth_decode::{
    Base, DecodeAs, Decoded, Field, FieldType, FieldValue, Layer, StreamTable, field_def, fmt_mac,
};
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
/// (C1, ETH-RT-02) `eth.show` of a TCP packet re-decodes the channel from this many journal seqs
/// before the packet, so relative TCP numbers use the same stream context as a list read.
pub const SHOW_CONTEXT_SEQS: u64 = 100_000;
/// (C1) Byte budget of that context scan; when it stops before the packet, the context is unknown.
pub const SHOW_CONTEXT_BYTES: u64 = 8 * 1024 * 1024;
/// (C1) Shown instead of a relative TCP number whose SYN was not inside the show window.
pub const TCP_REL_UNKNOWN: &str = "unknown (stream SYN not in the show window)";
/// (C1) Warning code added by [`mark_tcp_rel_unknown`].
pub const WARN_TCP_REL_UNKNOWN: &str = "tcp_relative_unknown";
/// (C1) Detail text of that warning.
pub const WARN_TCP_REL_UNKNOWN_DETAIL: &str =
    "relative tcp.seq/tcp.ack need the stream's SYN inside the show window; use tcp.seq_raw / tcp.ack_raw";
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

/// Packet passes the optional display filter (decodes only when a filter is given, or always when
/// the request has Decode As rules — B3b).
struct Matcher {
    filter: Option<Filter>,
    streams: StreamTable,
    decoded: usize,
    /// (B3b) the request has Decode As rules: every packet is decoded (rows/keys may be re-summarised).
    redecode: bool,
}

impl Matcher {
    /// (B3b) Matcher decoding with `rules` (FUNCTIONS.md F5.10).
    fn with_decode_as(filter: Option<Filter>, rules: DecodeAs) -> Matcher {
        Matcher {
            filter,
            redecode: !rules.is_empty(),
            streams: StreamTable::with_decode_as(rules),
            decoded: 0,
        }
    }
    /// (B3b) `(passes, decoded)`: decodes when a filter is set or `redecode`; counts toward
    /// [`MAX_DECODE_PER_REQUEST`] like `matches` (FUNCTIONS.md F5.10).
    fn check(&mut self, rec: &PublishedRecord) -> (bool, Option<Decoded>) {
        if self.filter.is_none() && !self.redecode {
            return (true, None);
        }
        self.decoded += 1;
        let d = decode_record(rec, &mut self.streams);
        let pass = match &self.filter {
            None => true,
            Some(f) => d.as_ref().is_some_and(|d| f.matches(d)),
        };
        (pass, d)
    }
    fn matches(&mut self, rec: &PublishedRecord) -> bool {
        self.check(rec).0
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
                .map(|(ch, i)| json!({"ch": ch, "state": i.state, "backend": i.backend, "iface": i.iface, "generation": i.generation,
                    "start_cursor": i.start_at.map(|(e, s)| Cursor::Seq { epoch: Some(e), seq: s }.to_string()),
                    "start_seq": i.start_at.map(|(_, s)| s.to_string())}))
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
            let scope = range_scope(&params)?;
            let decode_as = decode_as_param(&params)?;
            let wait = params
                .get("wait_ms")
                .and_then(Value::as_u64)
                .map(|ms| Duration::from_millis(ms).min(FOLLOW_WAIT_MAX));
            let deadline = wait.map(|w| (Instant::now() + w).min(ctx.deadline()));
            loop {
                let r = query_once(ctx, &ch, &scope, filter.clone(), &decode_as, max, detail).await?;
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
    scope: &ScanScope,
    filter: Option<Filter>,
    decode_as: &DecodeAs,
    max: usize,
    detail: bool,
) -> Result<Value, DomainError> {
    let mut m = Matcher::with_decode_as(filter, decode_as.clone());
    let mut rows: VecDeque<Value> = VecDeque::new();
    let mut matched = 0usize;
    let mut last_seq = None;
    let mut stopped_early = false;
    let sum = scan_packets(
        ctx,
        ch,
        scope.from.clone(),
        scope.to.clone(),
        SCAN_BUDGET_BYTES,
        &mut |rec| {
            if m.exhausted() {
                stopped_early = true;
                return false;
            }
            let (pass, d) = m.check(rec);
            if !pass {
                return true;
            }
            matched += 1;
            last_seq = Some(rec.seq.get());
            // schema v1: the latest `max` rows up to `to` (older ones are counted in `matched`).
            if rows.len() == max {
                rows.pop_front();
            }
            rows.push_back(match d {
                Some(d) if m.redecode => row_redecoded(rec, detail, &d),
                _ => row(rec, detail),
            });
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
    let decode_stopped = stopped_early;
    let caps = if matched > rows.len() {
        &[REASON_ROW_CAP][..]
    } else {
        &[]
    };
    let reasons = partial_reasons(scope, &sum, decode_stopped, caps);
    let mut out = json!({
        "ch": ch,
        "rows": rows,
        "range": {"from": first, "to": last},
        "truncated": stopped_early || matched > rows.len() || sum.budget_hit,
        "matched": matched,
        "gaps": sum.gaps,
        "gaps_omitted": sum.gaps_omitted,
        "complete": complete_from(&reasons),
        "scan_complete": scan_complete_from(&reasons),
        "scope": scope_json(scope),
        "scanned": scanned_json(&sum),
        "resume_cursor": resume_cursor_json(&sum, &reasons),
        "partial_reasons": reasons,
        "next_cursor": next,
        "prev_cursor": prev,
    });
    if !decode_as.is_empty() {
        out["decode_as"] = decode_as_json(decode_as);
    }
    Ok(out)
}

// --- eth.show ---------------------------------------------------------------------------------

/// `eth.show{ref}`: tree + bytes of one packet.
#[derive(Debug)]
pub struct Show;

impl Handler for Show {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let decode_as = decode_as_param(&params)?;
            let r = str_param(&params, "ref").ok_or_else(|| usage("show needs ref"))?;
            let (ch, epoch, seq, _) = parse_ref(r)?;
            let at = Cursor::Seq { epoch, seq };
            let mut found: Option<PublishedRecord> = None;
            scan_packets(
                ctx,
                &ch,
                at.clone(),
                Some(at.clone()),
                SCAN_BYTES_PER_CALL,
                &mut |rec| {
                    if rec.seq.get() == seq {
                        found = Some(rec.clone());
                        false
                    } else {
                        true
                    }
                },
            )
            .await?;
            let rec = found
                .ok_or_else(|| DomainError::new(ErrorCode::NotFound, format!("{r} not in the journal")))?;
            let mut st = StreamTable::with_decode_as(decode_as.clone());
            let mut d = decode_record(&rec, &mut st)
                .ok_or_else(|| DomainError::new(ErrorCode::Internal, "packet has no decodable bytes"))?;
            let mut tcp_rel: Option<Value> = None;
            if d.tcp_rel.is_some() {
                let from = Cursor::Seq {
                    epoch,
                    seq: seq.saturating_sub(SHOW_CONTEXT_SEQS).max(1),
                };
                let mut ctx_st = StreamTable::with_decode_as(decode_as.clone());
                let mut target: Option<Decoded> = None;
                let scanned = scan_packets(
                    ctx,
                    &ch,
                    from.clone(),
                    Some(at.clone()),
                    SHOW_CONTEXT_BYTES,
                    &mut |rec| {
                        let Some(dec) = decode_record(rec, &mut ctx_st) else {
                            return true;
                        };
                        if rec.seq.get() == seq {
                            target = Some(dec);
                            false
                        } else {
                            true
                        }
                    },
                )
                .await;
                let mut reached = false;
                if scanned.is_ok()
                    && let Some(full) = target
                {
                    d = full;
                    reached = true;
                }
                let b = d.tcp_rel.unwrap_or_default();
                let has_ack = d
                    .layers
                    .iter()
                    .any(|l| l.name == "tcp" && l.fields.iter().any(|f| f.name == "tcp.ack"));
                tcp_rel = Some(json!({
                    "window_from": from.to_string(),
                    "reached": reached,
                    "seq_known": b.seq_from_syn,
                    "ack_known": if has_ack { Some(b.ack_from_syn) } else { None },
                }));
                mark_tcp_rel_unknown(&mut d);
            }
            let p = &rec.payload;
            let mut out = json!({
                "ref": ref_of(&rec),
                "len": p.get("len"),
                "caplen": p.get("caplen"),
                "linktype": p.get("linktype"),
                "bytes": p.get("bytes"),
                "truncated": p.get("bytes_truncated").and_then(Value::as_bool).unwrap_or(false),
                "summary": d.summary,
                "warnings": d.warnings,
                "tree": tree(&d),
            });
            if let Some(v) = tcp_rel {
                out["tcp_rel"] = v;
            }
            if !decode_as.is_empty() {
                out["decode_as"] = decode_as_json(&decode_as);
            }
            Ok(out)
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
                              "off": fp.map(|p| p.off), "len": fp.map(|p| p.len), "children": [],
                              "filter": node_filter(l, f)});
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
        out.push(json!({"name": l.name, "label": l.title, "off": pos.map(|p| p.off), "len": pos.map(|p| p.len), "children": top,
                         "filter": filter_expr(l.name, None)}));
    }
    out
}

/// (C1, ETH-RT-02) Ready filter of one tree field node. Relative TCP numbers depend on where
/// decoding started, so `tcp.seq` / `tcp.ack` nodes select the packet by the raw header value of the
/// same layer (`tcp.seq_raw` / `tcp.ack_raw`); `None` when that raw field is missing. Every other
/// field: [`filter_expr`]`(f.name, Some(&f.value))`. FUNCTIONS.md FC1.3.
pub fn node_filter(layer: &Layer, f: &Field) -> Option<String> {
    let raw_name = match f.name {
        "tcp.seq" => "tcp.seq_raw",
        "tcp.ack" => "tcp.ack_raw",
        _ => return filter_expr(f.name, Some(&f.value)),
    };
    let raw = layer.fields.iter().find(|x| x.name == raw_name)?;
    filter_expr(raw_name, Some(&raw.value))
}

/// (C1, ETH-RT-02) Marks relative TCP numbers whose SYN base is unknown (`d.tcp_rel` flags false)
/// as unavailable instead of showing a decode-start-relative number. FUNCTIONS.md FC1.5.
pub fn mark_tcp_rel_unknown(d: &mut Decoded) {
    let Some(b) = d.tcp_rel else { return };
    // first `pat` followed by at least one ASCII digit → the digits become `?`
    let blank = |text: &mut String, pat: &str| {
        let Some(i) = text.find(pat) else { return };
        let start = i + pat.len();
        let digits = text[start..].bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            text.replace_range(start..start + digits, "?");
        }
    };
    let mut changed = false;
    if !b.seq_from_syn {
        for l in d.layers.iter_mut().filter(|l| l.name == "tcp") {
            for f in l.fields.iter_mut().filter(|f| f.name == "tcp.seq") {
                f.show = Some(TCP_REL_UNKNOWN.to_owned());
            }
            blank(&mut l.title, "Seq: ");
        }
        blank(&mut d.summary.info, "Seq=");
        changed = true;
    }
    if !b.ack_from_syn {
        let mut has_ack = false;
        for l in d.layers.iter_mut().filter(|l| l.name == "tcp") {
            for f in l.fields.iter_mut().filter(|f| f.name == "tcp.ack") {
                f.show = Some(TCP_REL_UNKNOWN.to_owned());
                has_ack = true;
            }
        }
        if has_ack {
            blank(&mut d.summary.info, "Ack=");
            changed = true;
        }
    }
    if changed && d.warnings.len() < loom_eth_decode::MAX_WARNINGS {
        d.warnings.push(loom_eth_decode::DecodeWarning {
            code: WARN_TCP_REL_UNKNOWN,
            layer: "tcp",
            detail: WARN_TCP_REL_UNKNOWN_DETAIL.to_owned(),
        });
    }
}

/// (operator-upgrade B3a, ETH-02) Display-filter expression selecting this field value (or, with
/// `value: None`, this protocol layer), only when [`Filter::compile`] accepts it; `None` otherwise
/// (unknown field, unsupported value, over the size caps). FUNCTIONS.md F3.7.
pub fn filter_expr(name: &str, value: Option<&FieldValue>) -> Option<String> {
    let def = field_def(name)?;
    let expr = match value {
        None => {
            if def.ty != FieldType::Protocol {
                return None;
            }
            name.to_owned()
        }
        Some(FieldValue::Uint(n)) => {
            if def.base == Base::Hex {
                format!("{name} == 0x{n:x}")
            } else {
                format!("{name} == {n}")
            }
        }
        Some(FieldValue::Bool(b)) => format!("{name} == {}", u8::from(*b)),
        Some(FieldValue::Ipv4(a)) => format!("{name} == {a}"),
        Some(FieldValue::Ipv6(a)) => format!("{name} == {a}"),
        Some(FieldValue::Mac(m)) => format!("{name} == {}", fmt_mac(m)),
        Some(FieldValue::Bytes(b)) => {
            if b.is_empty() || b.len() > 64 {
                return None;
            }
            let hex: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("{name} == {}", hex.join(":"))
        }
        Some(FieldValue::Str(s)) => {
            if s.chars().count() > 128 {
                return None;
            }
            let mut esc = String::new();
            for ch in s.chars() {
                match ch {
                    '\\' => esc.push_str("\\\\"),
                    '"' => esc.push_str("\\\""),
                    '\n' => esc.push_str("\\n"),
                    '\t' => esc.push_str("\\t"),
                    '\r' => esc.push_str("\\r"),
                    '\0' => esc.push_str("\\0"),
                    c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                        esc.push_str(&format!("\\x{:02x}", c as u32))
                    }
                    c => esc.push(c),
                }
            }
            format!("{name} == \"{esc}\"")
        }
    };
    Filter::compile(&expr).is_ok().then_some(expr)
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
            let decode_as = decode_as_param(&params)?;
            let mut m = Matcher::with_decode_as(compile(str_param(&params, "filter"))?, decode_as.clone());
            let scope = range_scope(&params)?;
            let mut table: BTreeMap<String, (u64, u64)> = BTreeMap::new();
            let (mut packets, mut bytes, mut other) = (0u64, 0u64, 0u64);
            let mut t0: Option<u64> = None;
            let mut partial = false;
            let sum = scan_packets(
                ctx,
                &ch,
                scope.from.clone(),
                scope.to.clone(),
                SCAN_BUDGET_BYTES,
                &mut |rec| {
                    if m.exhausted() {
                        partial = true;
                        return false;
                    }
                    let (pass, d) = m.check(rec);
                    if !pass {
                        return true;
                    }
                    let p = &rec.payload;
                    let len = p.get("len").and_then(Value::as_u64).unwrap_or(0);
                    let redec = d
                        .as_ref()
                        .filter(|d| d.warnings.iter().any(|w| w.code == "decode_as"));
                    let s = |k: &str| match (redec, k) {
                        (Some(d), "proto") => d.summary.proto.clone(),
                        (Some(d), "src") => d.summary.src.clone(),
                        (Some(d), "dst") => d.summary.dst.clone(),
                        _ => p.get(k).and_then(Value::as_str).unwrap_or("?").to_owned(),
                    };
                    let s = |k: &str| {
                        let v = s(k);
                        if v.is_empty() { "?".to_owned() } else { v }
                    };
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
                },
            )
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
            let reasons = partial_reasons(&scope, &sum, partial, &[]);
            let mut out = json!({"ch": ch, "by": by, "rows": shown, "keys": rows.len(), "other": other,
                      "total_packets": packets, "total_bytes": bytes,
                      "truncated": partial || sum.budget_hit || rows.len() > shown.len(),
                      "gaps": sum.gaps, "complete": complete_from(&reasons),
                      "scan_complete": scan_complete_from(&reasons),
                      "scope": scope_json(&scope),
                      "scanned": scanned_json(&sum),
                      "resume_cursor": resume_cursor_json(&sum, &reasons),
                      "partial_reasons": reasons});
            if !decode_as.is_empty() {
                out["decode_as"] = decode_as_json(&decode_as);
            }
            Ok(out)
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
            let scope = range_scope(&params)?;
            let mut st = StreamTable::new();
            let mut segs: Vec<Value> = Vec::new();
            let mut used = 0usize;
            let mut truncated = false;
            let mut decode_stopped = false;
            let mut output_cap = false;
            let mut client: Option<String> = None;
            let mut offs = [0usize; 2];
            let mut decoded = 0usize;
            let sum = scan_packets(
                ctx,
                &ch,
                scope.from.clone(),
                scope.to.clone(),
                SCAN_BUDGET_BYTES,
                &mut |rec| {
                    if rec.payload.get("stream").and_then(Value::as_u64) != Some(stream) {
                        return true;
                    }
                    if rec
                        .payload
                        .get("stream_proto")
                        .and_then(Value::as_str)
                        .unwrap_or("tcp")
                        != proto
                    {
                        return true;
                    }
                    decoded += 1;
                    if decoded > MAX_DECODE_PER_REQUEST {
                        truncated = true;
                        decode_stopped = true;
                        return false;
                    }
                    let Some(d) = decode_record(rec, &mut st) else {
                        return true;
                    };
                    if !d.has_protocol(&proto)
                        || d.field_values("tcp.analysis.retransmission").next().is_some()
                    {
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
                        output_cap = true;
                        return false;
                    }
                    true
                },
            )
            .await?;
            let caps = if output_cap { &[REASON_OUTPUT_CAP][..] } else { &[] };
            let reasons = partial_reasons(&scope, &sum, decode_stopped, caps);
            Ok(
                json!({"ch": ch, "proto": proto, "stream": stream, "segments": segs, "bytes": used,
                      "truncated": truncated || sum.budget_hit, "gaps": sum.gaps,
                      "complete": complete_from(&reasons),
                      "scan_complete": scan_complete_from(&reasons),
                      "scope": scope_json(&scope),
                      "scanned": scanned_json(&sum),
                      "resume_cursor": resume_cursor_json(&sum, &reasons),
                      "partial_reasons": reasons}),
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
            let filter_expr = compile(str_param(&params, "filter"))?;
            let decode_as = decode_as_param(&params)?;
            let mut m = Matcher::with_decode_as(filter_expr, decode_as.clone());
            let scope = range_scope(&params)?;
            // Explicit path: create/truncate (CLI `-o FILE`). Default: a fresh daemon-side temp file
            // per export (create_new + bounded suffixes), never an earlier export's file.
            let explicit = str_param(&params, "path").map(std::path::PathBuf::from);
            let opened = match &explicit {
                Some(p) => std::fs::File::create(p).map(|f| (p.clone(), f)),
                None => {
                    let tag = ctx
                        .request_id()
                        .map_or_else(|| wall_now_ns().to_string(), str::to_owned);
                    super::export::create_default_file(&std::env::temp_dir(), &ch, &tag)
                }
            };
            let (path, file) = opened.map_err(|e| {
                let at = explicit.as_ref().map_or_else(std::env::temp_dir, Clone::clone);
                DomainError::new(
                    ErrorCode::Internal,
                    format!("cannot create {}: {e}", at.display()),
                )
            })?;
            // A failed default export removes its own half-written file (never a user's path).
            let fail = |e: DomainError| {
                if explicit.is_none() {
                    let _ = std::fs::remove_file(&path);
                }
                e
            };
            let io = |e: loom_eth_pcapng::Error| {
                DomainError::new(ErrorCode::Internal, format!("pcapng write: {e}"))
            };
            let mut w = PcapNgWriter::new(std::io::BufWriter::new(file))
                .map_err(io)
                .map_err(fail)?;
            let mut ifaces: BTreeMap<u16, u32> = BTreeMap::new();
            let (mut packets, mut bytes) = (0usize, 0u64);
            let mut comments_dropped = 0usize;
            let mut err: Option<DomainError> = None;
            let mut truncated = false;
            let mut decode_stopped = false;
            let mut output_cap = false;
            let sum = scan_packets(
                ctx,
                &ch,
                scope.from.clone(),
                scope.to.clone(),
                SCAN_BUDGET_BYTES,
                &mut |rec| {
                    if packets >= EXPORT_MAX_PACKETS || bytes >= EXPORT_MAX_BYTES {
                        truncated = true;
                        output_cap = true;
                        return false;
                    }
                    if m.exhausted() {
                        truncated = true;
                        decode_stopped = true;
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
                },
            )
            .await;
            let sum = match sum {
                Ok(s) => s,
                Err(e) => {
                    drop(w);
                    return Err(fail(e));
                }
            };
            if let Some(e) = err {
                drop(w);
                return Err(fail(e));
            }
            w.flush().map_err(io).map_err(fail)?;
            let caps = if output_cap { &[REASON_OUTPUT_CAP][..] } else { &[] };
            let reasons = partial_reasons(&scope, &sum, decode_stopped, caps);
            let filter = str_param(&params, "filter")
                .filter(|f| !f.trim().is_empty())
                .map(str::to_owned);
            let mut out = json!({"path": path.display().to_string(), "packets": packets, "bytes": bytes,
                      "truncated": truncated || sum.budget_hit, "comments_dropped": comments_dropped, "gaps": sum.gaps,
                      "filter": filter,
                      "scope": scope_json(&scope),
                      "scanned": scanned_json(&sum),
                      "partial_reasons": reasons,
                      "resume_cursor": resume_cursor_json(&sum, &reasons),
                      "complete": complete_from(&reasons),
                      "scan_complete": scan_complete_from(&reasons)});
            if !decode_as.is_empty() {
                out["decode_as"] = decode_as_json(&decode_as);
            }
            Ok(out)
        })
    }
}

/// Wall clock (ns since the Unix epoch), the same clock as `ts.wall_ns` on this host.
fn wall_now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}
