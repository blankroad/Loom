//! Gateway piece: registration, strict start validation, engine resolution (`loom.toml
//! [logic] sigrok_cli` or PATH — never from RPC params), bounded journal read/wait, reducer.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_domain_api::*;
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, Risk};
use loom_proto::policy::WriteClass;
use loom_proto::rpc::SubFilter;
use schemars::{Schema, json_schema};
use serde_json::{Map, Value, json};

use crate::model::{self, StartConfig};
use crate::{DOMAIN, KIND_ANNOTATION, KIND_ANNOTATIONS, KIND_CAPTURE_STATE, KIND_EDGES, WORKER_KIND};

/// Default command wait.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(15);
/// `read --follow` / `wait` upper bounds.
pub const MAX_READ_WAIT_MS: u64 = 30_000;
/// `logic.wait` timeout bound.
pub const MAX_WAIT_MS: u64 = 600_000;
/// `read --follow` default wait when `wait_ms` is not given.
pub const FOLLOW_DEFAULT_MS: u64 = 10_000;
/// Rows per read.
pub const MAX_ROWS: u32 = 1000;
/// Result bytes per read.
pub const MAX_RESULT_BYTES: usize = 64 * 1024;
/// Rows budget inside a read result (headroom for the wrapper fields and the envelope).
pub const ROW_BUDGET: usize = MAX_RESULT_BYTES - 8 * 1024;
/// Journal bytes examined per read/wait poll.
pub const SCAN_BUDGET: u64 = 4 << 20;
/// Edges shown per edges row in `logic.read` (the journal keeps all ≤ 4096).
pub const READ_EDGES_PER_ROW: usize = 64;
/// Wait poll period.
pub const POLL: Duration = Duration::from_millis(50);
/// Reducer bounds.
pub const SNAP_CAPTURES: usize = 4;
/// Annotations kept for late joiners.
pub const SNAP_ANNOTATIONS: usize = 200;
/// Edge payload bytes kept for late joiners.
pub const SNAP_EDGE_BYTES: usize = 24 * 1024;
/// Annotation row bytes kept for late joiners (rows are also capped at 200).
pub const SNAP_ANNOTATION_BYTES: usize = 16 * 1024;

/// The domain module.
#[derive(Debug, Default)]
pub struct Logic;

fn usage(m: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Usage, m)
}

fn strict<'a>(p: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, DomainError> {
    let o = p.as_object().ok_or_else(|| usage("params must be an object"))?;
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(usage(format!(
            "unknown parameter {k:?} (allowed: {})",
            allowed.join(", ")
        )));
    }
    Ok(o)
}

fn opt_str<'a>(o: &'a Map<String, Value>, k: &str, max: usize) -> Result<Option<&'a str>, DomainError> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= max => Ok(Some(s)),
        _ => Err(usage(format!("{k}: expected a string of 1..={max} bytes"))),
    }
}

fn opt_u64(o: &Map<String, Value>, k: &str, max: u64) -> Result<Option<u64>, DomainError> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) if n <= max => Ok(Some(n)),
            _ => Err(usage(format!("{k}: expected an integer 0..={max}"))),
        },
    }
}

fn cursor(o: &Map<String, Value>) -> Result<Option<Cursor>, DomainError> {
    opt_str(o, "since", 128)?
        .map(|s| s.parse::<Cursor>().map_err(|e| usage(format!("since: {e}"))))
        .transpose()
}

/// Engine path from `[logic] sigrok_cli` or PATH; error text when absent.
fn engine(ctx: &CallCtx) -> (Option<String>, Option<String>) {
    let cfg = ctx.config();
    let configured = cfg.get("sigrok_cli").and_then(Value::as_str).map(str::to_owned);
    let path = std::env::var("PATH").ok();
    match model::resolve_engine(configured.as_deref(), path.as_deref()) {
        Ok(p) => (p.to_str().map(str::to_owned), None),
        Err(e) => (None, Some(e)),
    }
}

async fn sleep(ctx: &CallCtx, d: Duration) -> Result<(), DomainError> {
    ctx.spawn_blocking(move || std::thread::sleep(d)).await
}

/// Bounded scan from `from`. Returns events and the cursor to continue from.
async fn scan(
    ctx: &CallCtx,
    from: Cursor,
    kinds: &[&str],
    capture: Option<&str>,
) -> Result<(Vec<PublishedRecord>, Cursor, bool, bool), DomainError> {
    let mut out = Vec::new();
    let mut from = from;
    let mut used = 0u64;
    let mut gaps = false;
    let mut next = from.clone();
    loop {
        let q = JournalQuery {
            from: from.clone(),
            until: None,
            filter: SubFilter {
                domains: vec![DOMAIN.into()],
                channels: capture.map(|c| vec![c.to_owned()]).unwrap_or_default(),
                kinds: kinds.iter().map(|k| (*k).to_owned()).collect(),
                ..SubFilter::default()
            },
            request_id: None,
            max_events: MAX_ROWS,
        };
        let r = ctx.journal().scan(&q, (SCAN_BUDGET - used).max(1)).await?;
        used = used.saturating_add(r.scanned_bytes);
        gaps |= !r.gaps.is_empty();
        if r.scanned_through > 0 {
            next = Cursor::Seq {
                epoch: None,
                seq: r.scanned_through + 1,
            };
        }
        out.extend(r.events);
        match r.resume_from {
            Some(c) if used < SCAN_BUDGET => from = c,
            Some(c) => {
                return Ok((out, c, false, gaps));
            }
            None => return Ok((out, next, !gaps, gaps)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Handlers.

#[derive(Debug)]
struct Start;
impl Handler for Start {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let cfg = StartConfig::from_params(&params).map_err(|e| DomainError::new(e.code, e.message))?;
            let (path, error) = engine(ctx);
            let args = json!({"config": cfg.to_params(), "engine": path, "engine_error": error});
            ctx.worker(WORKER_KIND)?
                .call("logic.start", args, ctx.deadline())
                .await
        })
    }
}

#[derive(Debug)]
struct Stop;
impl Handler for Stop {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let o = strict(&params, &["capture_id"])?;
            let id = opt_str(o, "capture_id", 128)?;
            ctx.worker(WORKER_KIND)?
                .call("logic.stop", json!({"capture_id": id}), ctx.deadline())
                .await
        })
    }
}

#[derive(Debug)]
struct Status;
impl Handler for Status {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            strict(&params, &[])?;
            let (path, error) = engine(ctx);
            let w = ctx.worker(WORKER_KIND)?;
            let probe = if path.is_some() {
                w.call("logic.probe", json!({"engine": path}), ctx.deadline())
                    .await?
            } else {
                json!({"engine": {"path": null, "version": null, "error": error}})
            };
            let st = w.call("logic.status", json!({}), ctx.deadline()).await?;
            let compiled = probe.get("compiled").cloned().unwrap_or(Value::Null);
            let compiled_has = |list: &str, id: &str| {
                compiled[list]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(id)))
            };
            let decoders: Vec<&str> = model::DECODERS
                .iter()
                .copied()
                .filter(|d| compiled_has("decoders", d))
                .collect();
            let mut sources = vec!["synthetic"];
            if compiled_has("drivers", "demo") {
                sources.push("sigrok-demo");
            }
            if model::DEVICE_DRIVERS.iter().any(|d| compiled_has("drivers", d)) {
                sources.push("sigrok-device");
            }
            Ok(json!({
                "engine": probe["engine"],
                "compiled": compiled,
                "implemented": {
                    "sources": sources,
                    "decoders": decoders,
                    "device_drivers": model::DEVICE_DRIVERS,
                    "refused_drivers": model::REFUSED_DRIVERS,
                    "device_verified": false,
                },
                "raw_waveform": true,
                "decode": !decoders.is_empty(),
                "unit_size": 1,
                "channels": model::CHANNELS,
                "capture": st["capture"],
                "running": st["running"],
            }))
        })
    }
}

fn kinds_for(kind: &str) -> Result<&'static [&'static str], DomainError> {
    Ok(match kind {
        "annotation" => &[KIND_ANNOTATION, KIND_ANNOTATIONS],
        "edges" => &[KIND_EDGES],
        "state" => &[KIND_CAPTURE_STATE],
        "all" => &[KIND_CAPTURE_STATE, KIND_EDGES, KIND_ANNOTATION, KIND_ANNOTATIONS],
        o => {
            return Err(usage(format!(
                "kind {o:?}: expected annotation, edges, state or all"
            )));
        }
    })
}

/// Flatten records into read rows (annotation batches → one row per annotation).
pub fn rows(events: &[PublishedRecord]) -> Vec<Value> {
    let mut out = Vec::new();
    for e in events {
        let p = &e.payload;
        let seq = e.seq.get();
        match e.kind.as_str() {
            KIND_ANNOTATIONS => {
                for a in p["annotations"].as_array().into_iter().flatten() {
                    let mut r = a.clone();
                    r["seq"] = json!(seq);
                    r["capture_id"] = p["capture_id"].clone();
                    r["type"] = json!("annotation");
                    out.push(r);
                }
                if p["dropped"].as_u64().is_some_and(|d| d > 0) {
                    out.push(json!({"type": "annotation_loss", "seq": seq, "capture_id": p["capture_id"], "dropped": p["dropped"]}));
                }
            }
            KIND_ANNOTATION => {
                let mut r = p.clone();
                r["seq"] = json!(seq);
                r["type"] = json!("annotation");
                out.push(r);
            }
            KIND_EDGES => {
                let edges = p["edges"].as_array().cloned().unwrap_or_default();
                out.push(json!({
                    "type": "edges", "seq": seq, "capture_id": p["capture_id"],
                    "sample_start": p["sample_start"], "sample_end": p["sample_end"],
                    "initial": p["initial"], "final": p["final"], "mask": p["mask"], "active": p["active"],
                    "n_edges": edges.len(), "dropped": p["dropped"],
                    "edges": edges.iter().take(READ_EDGES_PER_ROW).collect::<Vec<_>>(),
                    "edges_truncated": edges.len().saturating_sub(READ_EDGES_PER_ROW),
                }));
            }
            KIND_CAPTURE_STATE => out.push(json!({
                "type": "state", "seq": seq, "capture_id": p["capture_id"], "state": p["state"],
                "source": p["source"], "reason": p["reason"], "error": p["error"],
                "counters": p["counters"], "exit": p["exit"],
            })),
            _ => {}
        }
    }
    out
}

/// Keep the newest `tail` rows within `MAX_RESULT_BYTES`. Returns (rows, rows left out).
pub fn bound(mut rows: Vec<Value>, tail: usize) -> (Vec<Value>, usize) {
    let total = rows.len();
    let start = rows.len().saturating_sub(tail);
    rows.drain(..start);
    let mut bytes = 0usize;
    let mut keep = 0usize;
    for r in rows.iter().rev() {
        let n = serde_json::to_string(r).map_or(0, |s| s.len() + 1);
        if bytes + n > ROW_BUDGET {
            break;
        }
        bytes += n;
        keep += 1;
    }
    let cut = rows.len() - keep;
    rows.drain(..cut);
    let left = total - rows.len();
    (rows, left)
}

#[derive(Debug)]
struct Read;
impl Handler for Read {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let o = strict(
                &params,
                &["capture_id", "kind", "tail", "since", "wait_ms", "follow"],
            )?;
            let capture = opt_str(o, "capture_id", 128)?;
            let kinds = kinds_for(opt_str(o, "kind", 16)?.unwrap_or("annotation"))?;
            let tail = opt_u64(o, "tail", u64::from(MAX_ROWS))?.unwrap_or(100).max(1) as usize;
            let follow = match o.get("follow") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(usage("follow: expected a boolean")),
            };
            let wait_ms = match (opt_u64(o, "wait_ms", MAX_READ_WAIT_MS)?, follow) {
                (Some(0), false) => 0,
                (Some(_), false) => return Err(usage("wait_ms > 0 needs follow=true")),
                (Some(0), true) => return Err(usage("wait_ms: expected 1..=30000 with follow")),
                (Some(w), true) => w,
                (None, true) => FOLLOW_DEFAULT_MS,
                (None, false) => 0,
            };
            let mut from = cursor(o)?.unwrap_or(Cursor::Relative {
                amount: 10,
                unit: TimeUnit::Mins,
            });
            let deadline = (Instant::now() + Duration::from_millis(wait_ms)).min(ctx.deadline());
            loop {
                let (events, next, complete, _) = scan(ctx, from.clone(), kinds, capture).await?;
                let all = rows(&events);
                if !all.is_empty() || Instant::now() + POLL >= deadline {
                    let (rows, left) = bound(all, tail);
                    return Ok(json!({"rows": rows, "next": next.to_string(),
                                     "complete": complete && left == 0, "rows_left_out": left}));
                }
                from = next;
                sleep(ctx, POLL).await?;
            }
        })
    }
}

fn hex_byte(t: &str) -> Option<u8> {
    let t = t.trim();
    if t.is_empty() || t.len() > 2 {
        return None;
    }
    u8::from_str_radix(t, 16).ok()
}

/// Match state of `logic.wait`. `ascii` needs consecutive byte annotations of one decoder with
/// no loss between them: a row with `gap_before`, a loss row or a journal gap breaks the run.
#[derive(Debug)]
pub struct Matcher<'a> {
    text: Option<&'a str>,
    ascii: Option<&'a [u8]>,
    decoder: Option<&'a str>,
    window: std::collections::HashMap<String, Vec<u8>>,
}

impl<'a> Matcher<'a> {
    /// New matcher (exactly one of `text` / `ascii`).
    pub fn new(text: Option<&'a str>, ascii: Option<&'a [u8]>, decoder: Option<&'a str>) -> Self {
        Matcher {
            text,
            ascii,
            decoder,
            window: Default::default(),
        }
    }
    /// Forget partial runs.
    pub fn reset(&mut self) {
        self.window.clear();
    }
    /// Feed one read row; true on a match.
    pub fn push(&mut self, r: &Value) -> bool {
        let cap = r["capture_id"].as_str().unwrap_or("");
        if r["type"] == "annotation_loss" {
            self.window.retain(|k, _| !k.starts_with(&format!("{cap}/")));
            return false;
        }
        if r["type"] != "annotation" {
            return false;
        }
        let dec = r["decoder"].as_str().unwrap_or("");
        if self
            .decoder
            .is_some_and(|d| dec != d && !dec.starts_with(&format!("{d}-")))
        {
            return false;
        }
        let t = r["text"].as_str().unwrap_or("");
        if let Some(want) = self.text {
            return t.eq_ignore_ascii_case(want);
        }
        let Some(want) = self.ascii else { return false };
        let w = self.window.entry(format!("{cap}/{dec}")).or_default();
        if r["gap_before"].as_u64().is_some_and(|g| g > 0) {
            w.clear();
        }
        match hex_byte(t) {
            Some(b) => {
                w.push(b);
                let over = w.len().saturating_sub(want.len());
                w.drain(..over);
                w.as_slice() == want
            }
            None => {
                w.clear();
                false
            }
        }
    }
}

/// `logic.wait`: first annotation whose text equals `text` (ASCII case-insensitive), or the end
/// of a run of consecutive byte annotations of one decoder spelling `ascii`.
#[derive(Debug)]
struct Wait;
impl Handler for Wait {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let o = strict(
                &params,
                &["text", "ascii", "decoder", "capture_id", "since", "timeout_ms"],
            )?;
            let text = opt_str(o, "text", 256)?;
            let ascii = opt_str(o, "ascii", 256)?;
            if ascii.is_some_and(|a| !a.is_ascii()) {
                return Err(usage("ascii: expected ASCII text"));
            }
            let ascii = ascii.map(str::as_bytes);
            if text.is_some() == ascii.is_some() {
                return Err(usage("give exactly one of text or ascii"));
            }
            let decoder = opt_str(o, "decoder", 64)?;
            let capture = opt_str(o, "capture_id", 128)?;
            let timeout_ms = match opt_u64(o, "timeout_ms", MAX_WAIT_MS)? {
                Some(0) | None => return Err(usage(format!("timeout_ms: required, 1..={MAX_WAIT_MS}"))),
                Some(t) => t,
            };
            let mut from = cursor(o)?.unwrap_or(Cursor::Relative {
                amount: 2,
                unit: TimeUnit::Secs,
            });
            let deadline = (Instant::now() + Duration::from_millis(timeout_ms)).min(ctx.deadline());
            let mut matcher = Matcher::new(text, ascii, decoder);
            let mut seen_seq = 0u64;
            loop {
                let (events, next, _, gaps) =
                    scan(ctx, from.clone(), &[KIND_ANNOTATION, KIND_ANNOTATIONS], capture).await?;
                if gaps {
                    // Journal loss inside the range: no contiguous-byte proof across it.
                    matcher.reset();
                }
                for r in rows(&events) {
                    let seq = r["seq"].as_u64().unwrap_or(0);
                    if seq < seen_seq {
                        continue;
                    }
                    if matcher.push(&r) {
                        return Ok(
                            json!({"matched": true, "annotation": r, "capture_id": r["capture_id"],
                                         "seq": seq, "next": next.to_string(),
                                         "match": if text.is_some() { "text" } else { "ascii_contiguous" }}),
                        );
                    }
                }
                if let Some(last) = events.last() {
                    seen_seq = last.seq.get() + 1;
                }
                if Instant::now() + POLL >= deadline {
                    return Err(DomainError::new(
                        ErrorCode::Timeout,
                        "no matching annotation before timeout",
                    )
                    .with_detail(json!({"next": next.to_string(), "timeout_ms": timeout_ms})));
                }
                from = next;
                sleep(ctx, POLL).await?;
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Hooks, reducer, registration.

/// Write hooks: capture control is observation-only (never drives a device line).
#[derive(Debug)]
pub struct LogicHooks;
impl WriteHooks for LogicHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        let (risk, reason) = match req.method {
            "logic.start" => (Risk::R1, "starts sigrok-cli capture processes (observation only)"),
            _ => (Risk::R0, "stops a capture"),
        };
        Classification {
            risk,
            reason: reason.into(),
            block: None,
        }
    }
    fn presend_check(&self, _req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        Ok(())
    }
}

/// Late-joiner snapshot: recent capture states, annotations and edges (bounded).
#[derive(Debug, Default)]
pub struct LogicReducer {
    captures: VecDeque<(u64, Value)>,
    annotations: VecDeque<(usize, Value)>,
    annotation_bytes: usize,
    edges: VecDeque<(usize, Value)>,
    edge_bytes: usize,
    evicted_annotations: u64,
    evicted_edges: u64,
}

impl Reducer for LogicReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_CAPTURE_STATE, KIND_EDGES, KIND_ANNOTATION, KIND_ANNOTATIONS]
    }
    fn apply(&mut self, ev: &PublishedRecord) {
        let seq = ev.seq.get();
        let p = &ev.payload;
        match ev.kind.as_str() {
            KIND_CAPTURE_STATE => {
                let id = &p["capture_id"];
                if let Some(slot) = self.captures.iter_mut().find(|(_, v)| &v["capture_id"] == id) {
                    if slot.0 < seq {
                        *slot = (seq, p.clone());
                    }
                } else {
                    self.captures.push_back((seq, p.clone()));
                    if self.captures.len() > SNAP_CAPTURES {
                        self.captures.pop_front();
                    }
                }
            }
            KIND_EDGES => {
                let n = serde_json::to_string(p).map_or(usize::MAX, |s| s.len() + 1);
                if n > SNAP_EDGE_BYTES {
                    self.evicted_edges += 1;
                    return;
                }
                self.edges.push_back((n, p.clone()));
                self.edge_bytes += n;
                while self.edge_bytes > SNAP_EDGE_BYTES {
                    match self.edges.pop_front() {
                        Some((m, _)) => {
                            self.edge_bytes -= m;
                            self.evicted_edges += 1;
                        }
                        None => break,
                    }
                }
            }
            KIND_ANNOTATIONS | KIND_ANNOTATION => {
                for mut r in rows(std::slice::from_ref(ev)) {
                    if r["type"] != "annotation" {
                        continue;
                    }
                    if let Some(o) = r.as_object_mut() {
                        o.remove("type");
                    }
                    let n = serde_json::to_string(&r).map_or(usize::MAX, |s| s.len() + 1);
                    if n > SNAP_ANNOTATION_BYTES {
                        self.evicted_annotations += 1;
                        continue;
                    }
                    self.annotations.push_back((n, r));
                    self.annotation_bytes += n;
                    while self.annotations.len() > SNAP_ANNOTATIONS
                        || self.annotation_bytes > SNAP_ANNOTATION_BYTES
                    {
                        match self.annotations.pop_front() {
                            Some((m, _)) => {
                                self.annotation_bytes -= m;
                                self.evicted_annotations += 1;
                            }
                            None => break,
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        let keep = |v: &Value| {
            filter.channels.is_empty()
                || v["capture_id"]
                    .as_str()
                    .is_some_and(|c| filter.channels.iter().any(|f| f == c))
        };
        vec![json!({
            "captures": self.captures.iter().map(|(_, v)| v).filter(|v| keep(v)).collect::<Vec<_>>(),
            "annotations": self.annotations.iter().map(|(_, v)| v).filter(|v| keep(v)).collect::<Vec<_>>(),
            "edges": self.edges.iter().map(|(_, v)| v).filter(|v| keep(v)).collect::<Vec<_>>(),
            "evicted": {"annotations": self.evicted_annotations, "edges": self.evicted_edges},
        })]
    }
}

fn spec(
    method: &str,
    write: bool,
    handler: Arc<dyn Handler>,
    params: Value,
    max_wait: Duration,
) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: write.then_some(WriteClass::Request),
        params: Schema::try_from(params).unwrap_or_else(|_| json_schema!({"type": "object"})),
        result: json_schema!({"type": "object"}),
        roles: if write { RoleSet::WRITE } else { RoleSet::READ },
        handler,
        bounds: Bounds {
            max_wait,
            default_rows: 100,
            max_rows: MAX_ROWS,
        },
        default_action: None,
    }
}

fn kind(k: &str, required: &[&str], pointers: Vec<PointerSpec>) -> EventKindSpec {
    EventKindSpec {
        kind: k.into(),
        payload_schema: Schema::try_from(json!({"type": "object", "required": required}))
            .unwrap_or_else(|_| json_schema!({"type": "object"})),
        pointers,
        version: 1,
    }
}

fn ptr(path: &str, ty: ValueType) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: None,
        ty,
    }
}

/// Help (`loom logic help --json`).
pub fn help() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "Logic analyzer through sigrok-cli: live raw edges (D0..D7, 1 byte/sample) and real libsigrok \
                  decoder annotations (uart, spi) in the shared journal before STOP."
            .into(),
        examples: vec![
            ex(
                "loom logic start --source synthetic --decode uart:rx=D0:baudrate=115200 --duration 10s",
                "Loom-generated UART waveform decoded by libsigrok (marked synthetic)",
            ),
            ex(
                "loom logic start --source sigrok-demo --pattern graycode --duration 5s",
                "libsigrok demo driver acquisition (real engine, demo content)",
            ),
            ex("loom logic wait --ascii LOOM --timeout 5s", "wait for decoded bytes spelling LOOM"),
            ex("loom logic read --kind annotation --tail 50", "recent decoded words"),
            ex("loom logic stop", "stop and reap the capture processes"),
        ],
        notes: vec![
            "Acquisition is always D0..D7; --channels only selects displayed edge bits.".into(),
            "Edges are bounded (≤4096 per event, ≤10 events/s): `dropped` is real loss, use `final` to resync."
                .into(),
            "Annotation class is null (sigrok-cli 0.7.2 does not print it); decoder ids are e.g. uart-1.".into(),
            "sigrok-device accepts fx2lafw only and is unverified on hardware; saleae-logic16/-pro are refused."
                .into(),
            "No automatic fallback: synthetic is used only when --source synthetic is given.".into(),
        ],
    }
}

impl DomainModule for Logic {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let obj = |props: Value| json!({"type": "object", "properties": props});
        let s = json!({"type": "string"});
        let i = json!({"type": "integer"});
        let commands = vec![
            spec(
                "logic.status",
                false,
                Arc::new(Status),
                obj(json!({})),
                DEFAULT_WAIT,
            ),
            spec(
                "logic.start",
                true,
                Arc::new(Start),
                json!({"type": "object", "required": ["source"], "properties": {
                    "source": {"enum": ["sigrok-demo", "sigrok-device", "synthetic"]},
                    "driver": s, "conn": s, "samplerate": i, "channels": {"type": "array", "items": s},
                    "decoders": {"type": "array", "items": s}, "duration_ms": i, "max_samples": i,
                    "pattern": s, "text": s, "frame_period_ms": i, "clock_hz": i}}),
                DEFAULT_WAIT,
            ),
            spec(
                "logic.stop",
                true,
                Arc::new(Stop),
                obj(json!({"capture_id": s})),
                DEFAULT_WAIT,
            ),
            spec(
                "logic.read",
                false,
                Arc::new(Read),
                obj(
                    json!({"capture_id": s, "kind": {"enum": ["annotation", "edges", "state", "all"]},
                           "tail": i, "since": s, "wait_ms": i, "follow": {"type": "boolean"}}),
                ),
                Duration::from_millis(MAX_READ_WAIT_MS) + DEFAULT_WAIT,
            ),
            spec(
                "logic.wait",
                false,
                Arc::new(Wait),
                obj(
                    json!({"text": s, "ascii": s, "decoder": s, "capture_id": s, "since": s, "timeout_ms": i}),
                ),
                Duration::from_millis(MAX_WAIT_MS) + DEFAULT_WAIT,
            ),
        ];
        for mut c in commands {
            // STOP of a capture is allowed to watchers too (UI stop action is `watch`): request
            // class for audit, READ roles.
            if c.method == "logic.stop" {
                c.roles = RoleSet::READ;
            }
            reg.command(c)?;
        }
        let states = ValueType::Enum(
            ["starting", "running", "stopping", "stopped", "failed"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
        );
        reg.event_kind(kind(
            KIND_CAPTURE_STATE,
            &["capture_id", "state", "source"],
            vec![
                ptr("/payload/state", states),
                ptr("/payload/reason", ValueType::String),
            ],
        ))?
        .event_kind(kind(
            KIND_EDGES,
            &["capture_id", "sample_start", "sample_end", "initial", "edges"],
            vec![],
        ))?
        .event_kind(kind(
            KIND_ANNOTATION,
            &["capture_id", "text"],
            vec![ptr("/payload/text", ValueType::String)],
        ))?
        .event_kind(kind(KIND_ANNOTATIONS, &["capture_id", "annotations"], vec![]))?
        .risk_table(RiskTable {
            rows: vec![
                RiskRow {
                    method: "logic.start".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "observation-only capture (spawns sigrok-cli)".into(),
                },
                RiskRow {
                    method: "logic.stop".into(),
                    selector: None,
                    risk: Risk::R0,
                    reason: "stops a capture".into(),
                },
            ],
            ..RiskTable::default()
        })?
        .hooks(Arc::new(LogicHooks))?
        .worker(WorkerSpec {
            kind: WORKER_KIND.into(),
            instances: Instances::Shared,
            budgets: Budgets {
                mem_soft: 32 << 20,
                mem_hard: 64 << 20,
                credit: 4 << 20,
            },
            reopen_after_crash: Reopen::None,
            arch: Arch::Native,
            platforms: vec![Platform::Linux, Platform::Macos],
        })?
        .reducer(Box::new(LogicReducer::default()))?
        .capability("sigrok-cli")?
        .help(help())?;
        Ok(())
    }
}
