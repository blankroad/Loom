//! DBC signals on the gateway side (M2a, K6·G2·K7 part): per-channel DBC (`can.db.load`,
//! `can.db.info`), decoding of trace rows on request, `can.sig.get`, `can.sig.wait_for` and the
//! `can.signal_range` evidence predicate. Owner: can-opus.
//!
//! Bounds (C2): DBCs on at most [`MAX_DB_CHANNELS`] channels and [`MAX_DISTINCT_DBS`] different
//! files at once (each within `loom-can-dbc` default `Limits`: 16 MiB file, 48 MiB compiled);
//! one load at a time; load result lists ≤ [`MAX_LISTED_WARNINGS`] warnings and
//! ≤ [`MAX_LISTED_BLOCKED`] blocked messages (the rest counted); `sig.get` looks back
//! ≤ [`SIG_MAX_WINDOW_MS`]; `sig.wait_for` waits ≤ [`WAIT_MAX_MS`]; expressions ≤
//! [`MAX_EXPR_BYTES`]. Stored events are never changed: decoding only adds fields to replies.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_can_dbc::{
    Database, DecodeError, EncodeBase, EncodeError, EncodeOptions, Limits, LoadError, LoadOptions, Message,
    RawValue, Signal, SignalInput, SignalValue, TextEncoding, Unavailable, Warning,
};
use loom_can_frame::CanFrame;
use loom_domain_api::*;
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, SeqRange};
use loom_proto::evidence::{Evidence, EvidenceRef, RefString};
use loom_proto::policy::WriteClass;
use schemars::{Schema, json_schema};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::gateway::{
    Anchor, EXPECT_POLL, MAX_ROWS, SCAN_BYTES, Shared, frame_filter, frame_payload, lock, next_from, parse,
    ptr, publish_activity, read_spec, resolve_channel, timer, usage,
};
use crate::schema::FramePayload;
use crate::{
    DOMAIN, KIND_FRAME, RPC_DB_CATALOG, RPC_DB_INFO, RPC_DB_LOAD, RPC_ENCODE_PREVIEW, RPC_PERIODIC_LIST,
    RPC_PERIODIC_START, RPC_PERIODIC_UPDATE, RPC_SIG_GET, RPC_SIG_SET, RPC_SIG_WAIT_FOR, RPC_SIGNAL_AT,
    RPC_SIGNAL_QUERY, WORKER_KIND_SIM,
};

/// Published when a channel's DBC changes (first load or a different file).
pub const KIND_DB_CHANGED: &str = "can.db_changed";
/// Predicate `can.signal_range`.
pub const PRED_SIGNAL_RANGE: &str = "can.signal_range";
/// Channels that can have a DBC.
pub const MAX_DB_CHANNELS: usize = 8;
/// Different DBC files loaded at once (plan §7 "동시 DB 4").
pub const MAX_DISTINCT_DBS: usize = 4;
/// Warnings listed in a load result.
pub const MAX_LISTED_WARNINGS: usize = 20;
/// Blocked messages listed in a load / info result.
pub const MAX_LISTED_BLOCKED: usize = 50;
/// Default look-back of `can.sig.get`.
pub const SIG_DEFAULT_WINDOW_MS: u32 = 10_000;
/// Longest accepted condition expression.
pub use crate::condition::MAX_EXPR_BYTES;
/// Maximum look-back of `can.sig.get`.
pub use crate::condition::SIG_MAX_WINDOW_MS;
/// Maximum wait of `can.sig.wait_for` (1 h).
pub use crate::condition::WAIT_MAX_MS;
/// `sig.get` first looks this far back, then 4× wider up to the window (newest sample first).
const FIRST_WINDOW_MS: u32 = 250;
/// Journal scans per `sig.get` window step.
const SIG_MAX_SCANS: usize = 16;

fn db_err(msg: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Db, msg)
}

// ---------------------------------------------------------------------------------------------
// DB store
// ---------------------------------------------------------------------------------------------

/// One loaded DBC file.
#[derive(Debug)]
pub struct LoadedDb {
    pub db: Database,
    /// `db_sha256` (hex) — sessions and evidence pin this.
    pub sha: String,
    /// Absolute path it was read from.
    pub path: String,
    /// Loader warnings (bounded by `Limits::max_warnings`).
    pub warnings: Vec<Warning>,
}

/// DBC per channel. Shared by the db/sig handlers, trace decoding and the verifier.
#[derive(Debug, Default)]
pub struct DbStore {
    channels: BTreeMap<String, Arc<LoadedDb>>,
    loading: bool,
}

/// Shared handle to the store.
pub type Dbs = Arc<Mutex<DbStore>>;

impl DbStore {
    /// DBC of a channel.
    pub fn get(&self, channel: &str) -> Option<Arc<LoadedDb>> {
        self.channels.get(channel).cloned()
    }

    /// A loaded DBC with this hash (on any channel).
    pub fn by_sha(&self, sha: &str) -> Option<Arc<LoadedDb>> {
        self.channels.values().find(|d| d.sha == sha).cloned()
    }

    /// Puts `db` on `channel`; returns the DBC it replaced. Caps: channels and distinct files.
    fn install(&mut self, channel: &str, db: Arc<LoadedDb>) -> Result<Option<Arc<LoadedDb>>, DomainError> {
        if !self.channels.contains_key(channel) && self.channels.len() >= MAX_DB_CHANNELS {
            return Err(db_err(format!(
                "DBCs are loaded on {MAX_DB_CHANNELS} channels already (maximum)"
            )));
        }
        let distinct = {
            let mut shas: Vec<&str> = self
                .channels
                .iter()
                .filter(|(c, _)| c.as_str() != channel)
                .map(|(_, d)| d.sha.as_str())
                .collect();
            shas.push(&db.sha);
            shas.sort_unstable();
            shas.dedup();
            shas.len()
        };
        if distinct > MAX_DISTINCT_DBS {
            return Err(db_err(format!(
                "at most {MAX_DISTINCT_DBS} different DBC files can be loaded at once; \
                 load this file on a channel that already has a DBC to replace it"
            ))
            .with_detail(json!({"max_distinct_dbs": MAX_DISTINCT_DBS})));
        }
        Ok(self.channels.insert(channel.to_owned(), db))
    }
}

fn no_db(channel: &str) -> DomainError {
    db_err(format!("no DBC loaded on {channel}")).with_detail(json!({
        "channel": channel,
        "hint": format!("loom can db load <file.dbc> --channel {channel}"),
    }))
}

fn channel_db(dbs: &Dbs, channel: &str) -> Result<Arc<LoadedDb>, DomainError> {
    lock(dbs).get(channel).ok_or_else(|| no_db(channel))
}

/// DBC of a channel for decoding trace rows (`decode: true`).
pub(crate) fn decode_db(dbs: &Dbs, channel: &str) -> Result<Arc<LoadedDb>, DomainError> {
    channel_db(dbs, channel)
}

// ---------------------------------------------------------------------------------------------
// Loading (file I/O and compile run on their own thread so the hub is not blocked)
// ---------------------------------------------------------------------------------------------

fn read_dbc(path: &Path, limit: usize) -> Result<Vec<u8>, DomainError> {
    let shown = path.display();
    let f = std::fs::File::open(path).map_err(|e| db_err(format!("cannot open {shown}: {e}")))?;
    let meta = f
        .metadata()
        .map_err(|e| db_err(format!("cannot read {shown}: {e}")))?;
    if !meta.is_file() {
        return Err(db_err(format!("{shown} is not a file")));
    }
    let too_big = |actual: u64| {
        db_err(format!(
            "DBC limit exceeded: file size (bytes) {actual} > {limit}"
        ))
        .with_detail(json!({"limit": "file size (bytes)", "max": limit, "actual": actual}))
    };
    if meta.len() > limit as u64 {
        return Err(too_big(meta.len()));
    }
    let mut buf = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0).min(limit));
    f.take(limit as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| db_err(format!("cannot read {shown}: {e}")))?;
    if buf.len() > limit {
        return Err(too_big(buf.len() as u64));
    }
    Ok(buf)
}

fn load_error(e: &LoadError) -> DomainError {
    let detail = match e {
        LoadError::LimitExceeded { what, limit, actual } => {
            json!({"limit": what.to_string(), "max": limit, "actual": actual})
        }
        LoadError::Encoding { offset, .. } => json!({"offset": offset}),
        LoadError::InvalidDbc { line, .. } => json!({"line": line}),
    };
    db_err(e.to_string()).with_detail(detail)
}

fn load_file(path: String, encoding: TextEncoding) -> Result<LoadedDb, DomainError> {
    let options = LoadOptions {
        encoding,
        limits: Limits::default(),
    };
    let bytes = read_dbc(Path::new(&path), options.limits.max_file_bytes)?;
    let (db, warnings) = Database::load(&bytes, &options).map_err(|e| load_error(&e))?;
    Ok(LoadedDb {
        sha: db.sha256_hex(),
        db,
        path,
        warnings,
    })
}

/// Clears the "load running" flag when the loader finishes, panics or never starts.
struct LoadingGuard(Dbs);

impl Drop for LoadingGuard {
    fn drop(&mut self) {
        lock(&self.0).loading = false;
    }
}

/// Runs `f` on a new thread and awaits its result without a runtime dependency.
async fn off_thread<T: Send + 'static>(
    deadline: Instant,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, DomainError> {
    let slot: Arc<Mutex<Option<T>>> = Arc::default();
    let out = slot.clone();
    let handle = std::thread::Builder::new()
        .name("loom-can-dbc".into())
        .spawn(move || {
            let v = f();
            *lock(&out) = Some(v);
        })
        .map_err(|e| DomainError::new(ErrorCode::Internal, format!("cannot start the DBC loader: {e}")))?;
    loop {
        if let Some(v) = lock(&slot).take() {
            return Ok(v);
        }
        if handle.is_finished() {
            // Re-check: the thread may have stored its result just before finishing.
            return lock(&slot)
                .take()
                .ok_or_else(|| DomainError::new(ErrorCode::Internal, "the DBC loader stopped unexpectedly"));
        }
        if Instant::now() >= deadline {
            return Err(DomainError::new(
                ErrorCode::Timeout,
                "DBC load did not finish in time",
            ));
        }
        timer::sleep(Duration::from_millis(2)).await;
    }
}

fn id_text(id: loom_can_frame::CanId) -> String {
    if id.is_extended() {
        format!("0x{:08X}", id.raw())
    } else {
        format!("0x{:03X}", id.raw())
    }
}

fn summary(channel: &str, d: &LoadedDb) -> Value {
    let msgs = d.db.messages();
    let signals: usize = msgs.iter().map(|m| m.signals().len()).sum();
    let blocked: Vec<&Message> = msgs
        .iter()
        .filter(|m| m.decode_blocked().is_some() || m.encode_blocked().is_some())
        .collect();
    let warnings: Vec<Value> = d
        .warnings
        .iter()
        .take(MAX_LISTED_WARNINGS)
        .map(|w| json!({"line": w.line, "kind": format!("{:?}", w.kind), "detail": w.detail}))
        .collect();
    json!({
        "channel": channel,
        "loaded": true,
        "path": d.path,
        "db_sha256": d.sha,
        "messages": msgs.len(),
        "signals": signals,
        "nodes": d.db.nodes().len(),
        "encoding": format!("{:?}", d.db.encoding()).to_lowercase(),
        "encoding_guessed": d.db.encoding_guessed(),
        "warnings": warnings,
        "warnings_total": d.warnings.len(),
        "blocked": blocked.iter().take(MAX_LISTED_BLOCKED).map(|m| json!({
            "message": m.name(),
            "id": id_text(m.id()),
            "decode": m.decode_blocked(),
            "encode": m.encode_blocked(),
        })).collect::<Vec<_>>(),
        "blocked_total": blocked.len(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadParams {
    #[serde(default)]
    channel: Option<String>,
    path: String,
    /// `utf8` | `windows1252` | `auto` (default: UTF-8 if valid, else Windows-1252, marked).
    #[serde(default)]
    encoding: Option<String>,
}

#[derive(Debug)]
struct DbLoadH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for DbLoadH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: LoadParams = parse(params)?;
            if !Path::new(&p.path).is_absolute() {
                return Err(usage(format!(
                    "path must be absolute (the daemon has its own working directory): {}",
                    p.path
                )));
            }
            let encoding = match p.encoding.as_deref().unwrap_or("auto") {
                "auto" => TextEncoding::Auto,
                "utf8" | "utf-8" => TextEncoding::Utf8,
                "windows1252" | "cp1252" | "latin1" => TextEncoding::Windows1252,
                other => {
                    return Err(usage(format!(
                        "encoding must be utf8|windows1252|auto, not {other:?}"
                    )));
                }
            };
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            {
                let mut st = lock(&self.dbs);
                if st.loading {
                    return Err(DomainError::new(
                        ErrorCode::Busy,
                        "another DBC load is running; retry",
                    ));
                }
                st.loading = true;
            }
            let guard = LoadingGuard(self.dbs.clone());
            let path = p.path.clone();
            let loaded = off_thread(ctx.deadline(), move || {
                let _guard = guard;
                load_file(path, encoding)
            })
            .await??;
            let new = Arc::new(loaded);
            let prev = lock(&self.dbs).install(&channel, new.clone())?;
            let changed = prev.as_ref().is_none_or(|p| p.sha != new.sha);
            if changed {
                let mut d = EventDraft::new(
                    KIND_DB_CHANGED,
                    json!({
                        "channel": channel,
                        "db_sha256": new.sha,
                        "previous_sha256": prev.as_ref().map(|p| p.sha.clone()),
                        "path": new.path,
                    }),
                );
                d.channel = Some(channel.clone());
                let _ = ctx.publish(d);
            }
            let mut out = summary(&channel, &new);
            out["changed"] = json!(changed);
            out["previous_sha256"] = json!(prev.map(|p| p.sha.clone()));
            Ok(out)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InfoParams {
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Debug)]
struct DbInfoH {
    dbs: Dbs,
}

impl Handler for DbInfoH {
    fn call<'a>(&'a self, _ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: InfoParams = parse(params)?;
            let st = lock(&self.dbs);
            let channels: Vec<Value> = match p.channel.as_deref() {
                Some(ch) => vec![match st.get(ch) {
                    Some(d) => summary(ch, &d),
                    None => json!({"channel": ch, "loaded": false}),
                }],
                None => st.channels.iter().map(|(ch, d)| summary(ch, d)).collect(),
            };
            Ok(json!({"channels": channels}))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------------------------

/// Integers beyond JSON's exact range are sent as strings.
fn raw_json(r: RawValue) -> Value {
    const EXACT: u64 = 1 << 53;
    match r {
        RawValue::Unsigned(u) if u <= EXACT => json!(u),
        RawValue::Unsigned(u) => json!(u.to_string()),
        RawValue::Signed(i) if i.unsigned_abs() <= EXACT => json!(i),
        RawValue::Signed(i) => json!(i.to_string()),
        RawValue::Float32(f) => json!(f),
        RawValue::Float64(f) => json!(f),
    }
}

fn signal_json(sig: &Signal, v: &SignalValue<'_>) -> Value {
    match v {
        SignalValue::Value { raw, physical, label } => {
            let mut m =
                json!({"name": sig.name(), "value": physical, "unit": sig.unit(), "raw": raw_json(*raw)});
            if let Some(l) = label {
                m["label"] = json!(l);
            }
            m
        }
        SignalValue::Unavailable(Unavailable::ShortFrame { needed, actual }) => json!({
            "name": sig.name(), "unavailable": "short_frame", "needed": needed, "actual": actual,
        }),
    }
}

/// `decoded` field for one reply row: `None` for non-frames and ids the DBC does not know.
pub(crate) fn decode_record(db: &Database, rec: &PublishedRecord) -> Option<Value> {
    if rec.kind != KIND_FRAME {
        return None;
    }
    let f = frame_payload(rec)?;
    let frame = match f.to_frame() {
        Ok(fr) => fr,
        Err(e) => return Some(json!({"error": e.to_string()})),
    };
    match db.decode(&frame) {
        Err(DecodeError::UnknownId) => None,
        Err(e) => Some(json!({
            "message": db.message_by_id(frame.id()).map(Message::name),
            "error": e.to_string(),
        })),
        Ok(d) => {
            let mut v = json!({
                "message": d.message.name(),
                "signals": d.signals.iter().map(|s| signal_json(s.signal, &s.value)).collect::<Vec<_>>(),
            });
            if let Some(m) = d.length_mismatch {
                v["length_mismatch"] = json!({"expected": m.expected, "actual": m.actual});
            }
            if d.fd_mismatch {
                v["fd_mismatch"] = json!(true);
            }
            Some(v)
        }
    }
}

/// A signal resolved in a DBC.
#[derive(Debug, Clone, Copy)]
struct SigRef<'a> {
    msg: &'a Message,
    index: u32,
    sig: &'a Signal,
}

impl SigRef<'_> {
    fn path(&self) -> String {
        format!("{}.{}", self.msg.name(), self.sig.name())
    }
}

/// `"Msg.Sig"` or a signal name that is unique in the DBC.
fn resolve_signal<'a>(db: &'a Database, name: &str) -> Result<SigRef<'a>, DomainError> {
    let Some((msg, sig)) = db.signal_by_path(name) else {
        let cands = db.signals_named(name);
        if cands.len() > 1 {
            let list: Vec<String> = cands
                .iter()
                .take(10)
                .filter_map(|r| db.signal(*r))
                .map(|(m, s)| format!("{}.{}", m.name(), s.name()))
                .collect();
            return Err(usage(format!(
                "signal {name:?} is in several messages; use Message.Signal ({})",
                list.join(", ")
            )));
        }
        return Err(usage(format!(
            "no signal {name:?} in the loaded DBC (see `loom can db info`)"
        )));
    };
    let (index, _) = msg
        .signal_by_name(sig.name())
        .ok_or_else(|| DomainError::new(ErrorCode::Internal, "signal index lookup failed"))?;
    if db.is_ambiguous_id(msg.id()) {
        return Err(db_err(format!(
            "id {} is used by several DBC messages; its signals cannot be decoded",
            id_text(msg.id())
        )));
    }
    if let Some(reason) = msg.decode_blocked() {
        return Err(db_err(format!(
            "message {} cannot be decoded: {reason}",
            msg.name()
        )));
    }
    Ok(SigRef { msg, index, sig })
}

/// One decoded value of the signal.
#[derive(Debug, Clone)]
struct Sample {
    physical: f64,
    raw: RawValue,
    label: Option<String>,
}

/// The signal's value in this record, if it is a frame of the message with the signal active.
fn sample(s: SigRef<'_>, rec: &PublishedRecord) -> Option<Sample> {
    if rec.kind != KIND_FRAME {
        return None;
    }
    let f = frame_payload(rec)?;
    if f.rtr || f.id != s.msg.id().raw() || f.ext != s.msg.id().is_extended() {
        return None;
    }
    let frame = f.to_frame().ok()?;
    let d = s.msg.decode(&frame).ok()?;
    let ds = d.signals.iter().find(|x| x.index == s.index)?;
    match &ds.value {
        SignalValue::Value { raw, physical, label } => Some(Sample {
            physical: *physical,
            raw: *raw,
            label: label.map(str::to_owned),
        }),
        SignalValue::Unavailable(_) => None,
    }
}

fn value_json(s: SigRef<'_>, v: &Sample) -> Value {
    json!({
        "value": v.physical,
        "unit": s.sig.unit(),
        "raw": raw_json(v.raw),
        "label": v.label,
    })
}

fn age_ms(rec: &PublishedRecord) -> Option<u64> {
    let wall = rec.ts.wall_ns.get();
    if wall == 0 {
        return None;
    }
    let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_nanos()).ok()?;
    Some(now.saturating_sub(wall) / 1_000_000)
}

fn sig_ref_string(channel: &str, epoch: u64, from: u64, to: u64, sig: &str) -> String {
    RefString {
        domain: DOMAIN.into(),
        channel: channel.into(),
        epoch: Some(epoch),
        from,
        to,
        pointer: Some(format!("/signals/{sig}")),
    }
    .to_string()
}

fn frame_ref(channel: &str, rec: &PublishedRecord) -> String {
    RefString {
        domain: DOMAIN.into(),
        channel: channel.into(),
        epoch: Some(rec.epoch.get()),
        from: rec.seq.get(),
        to: rec.seq.get(),
        pointer: None,
    }
    .to_string()
}

fn observed(s: SigRef<'_>, v: &Sample, rec: &PublishedRecord, sha: &str) -> Value {
    let mut o = value_json(s, v);
    o["signal"] = json!(s.path());
    o["message"] = json!(s.msg.name());
    o["id"] = json!(id_text(s.msg.id()));
    o["origin"] = json!(rec.origin);
    o["db_sha256"] = json!(sha);
    o
}

fn sample_evidence(
    channel: &str,
    s: SigRef<'_>,
    v: &Sample,
    rec: &PublishedRecord,
    sha: &str,
    expected: Option<Value>,
) -> (Evidence, String) {
    let ev = Evidence {
        r#ref: EvidenceRef {
            stream_epoch: rec.epoch,
            seq_range: SeqRange {
                from: rec.seq,
                to: rec.seq,
            },
            source_refs: vec![rec.source_ref()],
        },
        kind: "can.signal".into(),
        pointer: Some(format!("/signals/{}", s.sig.name())),
        predicate_id: None,
        observed: Some(observed(s, v, rec, sha)),
        expected,
        request_id: rec.request_id.clone(),
        complete: true,
    };
    let r = sig_ref_string(
        channel,
        rec.epoch.get(),
        rec.seq.get(),
        rec.seq.get(),
        s.sig.name(),
    );
    (ev, r)
}

// ---------------------------------------------------------------------------------------------
// Conditions: `<Sig> <op> <number>` or `<Sig> between <a> [and] <b>` (hand parser, no eval)
// ---------------------------------------------------------------------------------------------

pub use crate::condition::{Condition, Op, parse_condition};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeArgs {
    signal: String,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    value: Option<f64>,
    #[serde(default)]
    between: Option<[f64; 2]>,
    #[serde(default)]
    tol: Option<f64>,
    db_sha256: String,
    #[serde(default)]
    origin: Option<Origin>,
}

fn condition_from_args(args: &Value) -> Result<(Condition, String, Option<Origin>), String> {
    let a: RangeArgs = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    let tol = a.tol.unwrap_or(0.0);
    if !tol.is_finite() || tol < 0.0 {
        return Err("tol must be a number ≥ 0".into());
    }
    let (op, value, hi) = match (a.op.as_deref(), a.value, a.between) {
        (None | Some("between"), None, Some([lo, hi])) if lo <= hi => (Op::Between, lo, hi),
        (Some(op), Some(v), None) => {
            let op = Op::from_str(op)
                .filter(|o| *o != Op::Between)
                .ok_or_else(|| format!("unknown op {op:?}"))?;
            (op, v, v)
        }
        _ => return Err("give `op` + `value`, or `between: [lo, hi]` with lo ≤ hi".into()),
    };
    if !value.is_finite() || !hi.is_finite() {
        return Err("values must be finite numbers".into());
    }
    Ok((
        Condition {
            signal: a.signal,
            op,
            value,
            hi,
            tol,
        },
        a.db_sha256,
        a.origin,
    ))
}

// ---------------------------------------------------------------------------------------------
// can.sig.get
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SigGetParams {
    #[serde(default)]
    channel: Option<String>,
    signal: String,
    #[serde(default)]
    window_ms: Option<u32>,
    /// Default: any origin (the value currently on the bus).
    #[serde(default)]
    origin: Option<Origin>,
}

/// Newest sample in the last `window_ms`, and whether the scan saw the whole window.
async fn latest_sample(
    ctx: &CallCtx,
    channel: &str,
    s: SigRef<'_>,
    origin: Option<Origin>,
    window_ms: u32,
) -> Result<(Option<(PublishedRecord, Sample)>, bool), DomainError> {
    let mut from = Cursor::Relative {
        amount: u64::from(window_ms),
        unit: TimeUnit::Millis,
    };
    let mut best = None;
    let mut complete = true;
    for i in 0..SIG_MAX_SCANS {
        let q = JournalQuery {
            from: from.clone(),
            until: None,
            filter: frame_filter(channel, &[KIND_FRAME], origin.into_iter().collect()),
            request_id: None,
            max_events: MAX_ROWS,
        };
        let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
        complete &= scan.gaps.is_empty();
        for rec in &scan.events {
            if let Some(v) = sample(s, rec) {
                best = Some((rec.clone(), v));
            }
        }
        let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
        match next_from(&scan) {
            Some(n) if more => {
                if i + 1 == SIG_MAX_SCANS {
                    complete = false; // newer frames were not read
                }
                from = n;
            }
            _ => break,
        }
    }
    Ok((best, complete))
}

#[derive(Debug)]
struct SigGetH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for SigGetH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: SigGetParams = parse(params)?;
            let window = p.window_ms.unwrap_or(SIG_DEFAULT_WINDOW_MS);
            if window == 0 || window > SIG_MAX_WINDOW_MS {
                return Err(usage(format!("window_ms must be 1..={SIG_MAX_WINDOW_MS}")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let s = resolve_signal(&db.db, &p.signal)?;
            // Newest first: a short window, then wider, so a busy bus does not hide the latest value.
            let mut w = FIRST_WINDOW_MS.min(window);
            let mut extra = Map::new();
            extra.insert("action".into(), json!("read"));
            publish_activity(
                ctx,
                RPC_SIG_GET,
                &channel,
                format!("read {} on {channel}", s.path()),
                extra,
            );
            loop {
                let (found, complete) = latest_sample(ctx, &channel, s, p.origin, w).await?;
                if let Some((rec, v)) = found {
                    let (mut ev, r) = sample_evidence(&channel, s, &v, &rec, &db.sha, None);
                    ev.complete = complete;
                    let mut out = value_json(s, &v);
                    let extra = json!({
                        "channel": channel,
                        "signal": s.path(),
                        "name": s.sig.name(),
                        "message": s.msg.name(),
                        "id": id_text(s.msg.id()),
                        "origin": rec.origin,
                        "seq": rec.seq,
                        "age_ms": age_ms(&rec),
                        "frame_ref": frame_ref(&channel, &rec),
                        "db_sha256": db.sha,
                        "complete": complete,
                        "evidence": [ev],
                        "refs": [r],
                    });
                    if let (Some(o), Value::Object(e)) = (out.as_object_mut(), extra) {
                        o.extend(e);
                    }
                    return Ok(out);
                }
                if w >= window {
                    break;
                }
                w = w.saturating_mul(4).min(window);
            }
            Err(DomainError::new(
                ErrorCode::NoMatch,
                format!("no {} value on {channel} in the last {window} ms", s.path()),
            )
            .with_detail(json!({
                "channel": channel,
                "signal": s.path(),
                "message": s.msg.name(),
                "id": id_text(s.msg.id()),
                "window_ms": window,
                "db_sha256": db.sha,
            })))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.sig.wait_for
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitParams {
    #[serde(default)]
    channel: Option<String>,
    expr: String,
    /// Default `now`: only samples after the call starts count.
    #[serde(default)]
    after: Option<Cursor>,
    within_ms: u32,
    /// Default `device` (like `can.expect`): own frames do not satisfy a device condition.
    #[serde(default)]
    origin: Option<Origin>,
    /// Any origin (overrides the `device` default).
    #[serde(default)]
    any_origin: bool,
}

#[derive(Debug)]
struct SigWaitH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for SigWaitH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: WaitParams = parse(params)?;
            if p.within_ms == 0 || p.within_ms > WAIT_MAX_MS {
                return Err(usage(format!("within_ms must be 1..={WAIT_MAX_MS}")));
            }
            let cond = parse_condition(&p.expr).map_err(usage)?;
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let s = resolve_signal(&db.db, &cond.signal)?;
            let cond = Condition {
                signal: s.path(),
                ..cond
            };
            let origin = if p.any_origin {
                None
            } else {
                Some(p.origin.unwrap_or(Origin::Device))
            };
            let args = cond.args(&db.sha, origin);
            let mut extra = Map::new();
            extra.insert("action".into(), json!("wait"));
            extra.insert(
                "waiting".into(),
                json!({"predicate": PRED_SIGNAL_RANGE, "args": args, "within_ms": p.within_ms}),
            );
            publish_activity(
                ctx,
                RPC_SIG_WAIT_FOR,
                &channel,
                format!("waiting for {} on {channel} (≤ {} ms)", cond.text(), p.within_ms),
                extra,
            );

            let start = Instant::now();
            let deadline = (start + Duration::from_millis(u64::from(p.within_ms))).min(ctx.deadline());
            let mut from = Anchor::new(p.after.clone().unwrap_or(Cursor::Now), start);
            let mut first: Option<PublishedRecord> = None;
            let mut last: Option<(PublishedRecord, Sample)> = None;
            let mut samples: u64 = 0;
            let mut complete = true;
            loop {
                let q = JournalQuery {
                    from: from.cursor(),
                    until: None,
                    filter: frame_filter(&channel, &[KIND_FRAME], origin.into_iter().collect()),
                    request_id: None,
                    max_events: MAX_ROWS,
                };
                let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
                complete &= scan.gaps.is_empty();
                for rec in &scan.events {
                    let Some(v) = sample(s, rec) else { continue };
                    samples += 1;
                    if first.is_none() {
                        first = Some(rec.clone());
                    }
                    if cond.holds(v.physical) {
                        let (ev, r) = sample_evidence(&channel, s, &v, rec, &db.sha, Some(args.clone()));
                        let mut out = value_json(s, &v);
                        let more = json!({
                            "channel": channel,
                            "matched": true,
                            "expr": cond.text(),
                            "signal": s.path(),
                            "name": s.sig.name(),
                            "message": s.msg.name(),
                            "id": id_text(s.msg.id()),
                            "origin": rec.origin,
                            "seq": rec.seq,
                            "frame_ref": frame_ref(&channel, rec),
                            "db_sha256": db.sha,
                            "samples_before": samples - 1,
                            "previous": last.as_ref().map(|(_, pv)| value_json(s, pv)),
                            "predicate": {"name": PRED_SIGNAL_RANGE, "args": args},
                            "evidence": [ev],
                            "refs": [r],
                        });
                        if let (Some(o), Value::Object(m)) = (out.as_object_mut(), more) {
                            o.extend(m);
                        }
                        return Ok(out);
                    }
                    last = Some((rec.clone(), v));
                }
                if let Some(n) = next_from(&scan) {
                    from = Anchor::Fixed(n);
                }
                if Instant::now() >= deadline {
                    break;
                }
                if scan.resume_from.is_none() {
                    timer::sleep(EXPECT_POLL.min(deadline.saturating_duration_since(Instant::now()))).await;
                }
            }
            // Timeout: cite only samples that were seen.
            let mut detail = json!({
                "channel": channel,
                "expr": cond.text(),
                "within_ms": p.within_ms,
                "samples": samples,
                "db_sha256": db.sha,
                "evidence": [],
                "refs": [],
            });
            if let (Some(f), Some((l, lv))) = (&first, &last) {
                let r = sig_ref_string(&channel, l.epoch.get(), f.seq.get(), l.seq.get(), s.sig.name());
                let ev = Evidence {
                    r#ref: EvidenceRef {
                        stream_epoch: l.epoch,
                        seq_range: SeqRange {
                            from: f.seq,
                            to: l.seq,
                        },
                        source_refs: Vec::new(),
                    },
                    kind: "can.signal_wait_timeout".into(),
                    pointer: Some(format!("/signals/{}", s.sig.name())),
                    predicate_id: None,
                    observed: Some(json!({"samples": samples, "last": value_json(s, lv)})),
                    expected: Some(args.clone()),
                    request_id: None,
                    complete,
                };
                let mut last_v = value_json(s, lv);
                last_v["seq"] = json!(l.seq);
                last_v["name"] = json!(s.sig.name());
                last_v["ref"] = json!(sig_ref_string(
                    &channel,
                    l.epoch.get(),
                    l.seq.get(),
                    l.seq.get(),
                    s.sig.name()
                ));
                detail["last"] = last_v;
                detail["evidence"] = json!([ev]);
                detail["refs"] = json!([r]);
            }
            Err(DomainError::new(
                ErrorCode::Timeout,
                format!(
                    "{} did not become true on {channel} within {} ms",
                    cond.text(),
                    p.within_ms
                ),
            )
            .with_detail(detail))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Evidence predicate can.signal_range
// ---------------------------------------------------------------------------------------------

/// Re-evaluates `can.signal_range{signal, op+value | between:[a,b], tol?, db_sha256, origin?}` on
/// the cited frames with the DBC pinned by `db_sha256` ("a sample satisfies it in the window").
pub(crate) fn verify_signal_range(
    dbs: &Dbs,
    pred: &Predicate,
    events: &[PublishedRecord],
    cov: &Coverage,
    budget: &mut EvalBudget,
) -> Verification {
    let (cond, sha, origin) = match condition_from_args(&pred.args) {
        Ok(v) => v,
        Err(e) => {
            return Verification::Inconclusive {
                reason: format!("invalid {PRED_SIGNAL_RANGE} args: {e}"),
            };
        }
    };
    let Some(db) = lock(dbs).by_sha(&sha) else {
        // The DBC that produced the claim is not loaded any more: never judge with another one.
        return Verification::Inconclusive {
            reason: "db_changed".into(),
        };
    };
    let s = match resolve_signal(&db.db, &cond.signal) {
        Ok(s) => s,
        Err(e) => {
            return Verification::Inconclusive {
                reason: format!("invalid {PRED_SIGNAL_RANGE} args: {}", e.message),
            };
        }
    };
    let mut samples = 0u64;
    let mut last: Option<Sample> = None;
    for chunk in events.chunks(256) {
        if budget.spend(chunk.len()).is_err() {
            return Verification::Inconclusive {
                reason: "eval_budget".into(),
            };
        }
        for rec in chunk {
            if origin.is_some_and(|o| o != rec.origin) {
                continue;
            }
            let Some(v) = sample(s, rec) else { continue };
            samples += 1;
            if cond.holds(v.physical) {
                return Verification::Match {
                    matched: vec![EventRef {
                        epoch: rec.epoch,
                        seq: rec.seq,
                        source: rec.source_ref(),
                    }],
                    observed: observed(s, &v, rec, &db.sha),
                };
            }
            last = Some(v);
        }
    }
    if cov.complete {
        Verification::Mismatch {
            observed: json!({"samples": samples, "last": last.map(|v| value_json(s, &v))}),
            reason: format!("no sample of {} satisfies {}", s.path(), cond.text()),
        }
    } else {
        Verification::Inconclusive {
            reason: format!("window incomplete: {}", cov.causes.join(", ")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding (periodic start `--msg`, IG `sig set`)
// ---------------------------------------------------------------------------------------------

/// Signals given per encode call.
pub const MAX_SET_SIGNALS: usize = 64;

fn encode_error(msg: &Message, e: &EncodeError) -> DomainError {
    let hint = match e {
        EncodeError::MissingSignal { .. } => " (give it with --set; it has no DBC start value)",
        EncodeError::OutOfRange { .. } => " (outside the DBC range; nothing was sent)",
        _ => "",
    };
    usage(format!("cannot encode {}: {e}{hint}", msg.name())).with_detail(json!({"message": msg.name()}))
}

/// Encodes `values` (`{Sig: number | "label"}`) into `msg` on top of `base`. Nothing is half-built.
fn encode_values(
    msg: &Message,
    values: &Map<String, Value>,
    base: EncodeBase<'_>,
    force_range: bool,
) -> Result<CanFrame, DomainError> {
    if values.len() > MAX_SET_SIGNALS {
        return Err(usage(format!("at most {MAX_SET_SIGNALS} signals per call")));
    }
    let mut inputs: Vec<(&str, SignalInput<'_>)> = Vec::with_capacity(values.len());
    for (name, v) in values {
        // A number is a physical value, a string a value-table label (`"D"`).
        let input = match v {
            Value::String(label) => SignalInput::Label(label.as_str()),
            Value::Number(n) => {
                SignalInput::Physical(n.as_f64().ok_or_else(|| usage(format!("{name}: bad number")))?)
            }
            _ => {
                return Err(usage(format!(
                    "{name}: value must be a number or a value-table label"
                )));
            }
        };
        inputs.push((name.as_str(), input));
    }
    let options = EncodeOptions {
        force_range,
        ..EncodeOptions::default()
    };
    msg.encode(&inputs, base, &options)
        .map(|e| e.frame)
        .map_err(|e| encode_error(msg, &e))
}

/// A DBC message by name (unique), with its encode guard.
fn message<'a>(db: &'a Database, name: &str) -> Result<&'a Message, DomainError> {
    let msg = db.message_by_name(name).ok_or_else(|| {
        usage(format!(
            "no message {name:?} in the loaded DBC (or the name is not unique)"
        ))
    })?;
    if let Some(reason) = msg.encode_blocked() {
        return Err(db_err(format!("message {name} cannot be encoded: {reason}")));
    }
    if db.is_ambiguous_id(msg.id()) {
        return Err(db_err(format!(
            "id {} is used by several DBC messages; it cannot be sent by name",
            id_text(msg.id())
        )));
    }
    Ok(msg)
}

/// `can.periodic.start{msg, signals}`: the frame from DBC start values plus `signals`, and the
/// DBC cycle time (`GenMsgCycleTime`) if the message has one.
pub(crate) fn start_frame(
    dbs: &Dbs,
    channel: &str,
    msg_name: &str,
    values: &Map<String, Value>,
    force_range: bool,
) -> Result<(CanFrame, Option<u32>, String), DomainError> {
    let db = channel_db(dbs, channel)?;
    let msg = message(&db.db, msg_name)?;
    let frame = encode_values(msg, values, EncodeBase::Initial, force_range)?;
    Ok((frame, msg.cycle_time_ms().filter(|c| *c > 0), db.sha.clone()))
}

// ---------------------------------------------------------------------------------------------
// can.sig.set (IG): change a signal of a running periodic job
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SigSetParams {
    #[serde(default)]
    channel: Option<String>,
    /// `Msg.Sig` (or a unique signal name).
    signal: String,
    /// Physical value or value-table label.
    value: Value,
    /// Job to change (default: the running job of the signal's message).
    #[serde(default)]
    job: Option<String>,
    #[serde(default)]
    force_range: bool,
}

/// The job sending `msg` on `channel` (from `can.periodic.list`).
async fn find_job(
    ctx: &CallCtx,
    channel: &str,
    msg: &Message,
    want: Option<&str>,
) -> Result<Value, DomainError> {
    let list = ctx
        .worker(WORKER_KIND_SIM)?
        .call(RPC_PERIODIC_LIST, json!({"channel": channel}), ctx.deadline())
        .await?;
    let (id, ext) = (msg.id().raw(), msg.id().is_extended());
    let jobs = list["jobs"].as_array().cloned().unwrap_or_default();
    let mine =
        |j: &Value| j["id"].as_u64() == Some(u64::from(id)) && j["ext"].as_bool().unwrap_or(false) == ext;
    let found = match want {
        Some(w) => {
            let j = jobs.into_iter().find(|j| j["job"].as_str() == Some(w));
            match j {
                Some(j) if mine(&j) => Some(j),
                Some(_) => {
                    return Err(usage(format!(
                        "job {w} does not send {} ({})",
                        msg.name(),
                        id_text(msg.id())
                    )));
                }
                None => None,
            }
        }
        None => jobs.into_iter().find(mine),
    };
    found.ok_or_else(|| {
        DomainError::new(
            ErrorCode::NotFound,
            format!(
                "no running periodic job sends {} ({}) on {channel}",
                msg.name(),
                id_text(msg.id())
            ),
        )
        .with_detail(json!({
            "message": msg.name(),
            "id": id_text(msg.id()),
            "hint": format!("loom can periodic start --msg {} --channel {channel} first", msg.name()),
        }))
    })
}

/// Last bytes the job sent, from the journal (only when `list` gives no `data`).
async fn journal_bytes(ctx: &CallCtx, channel: &str, job: &str) -> Result<Option<Vec<u8>>, DomainError> {
    let mut from = Cursor::Relative {
        amount: u64::from(SIG_MAX_WINDOW_MS),
        unit: TimeUnit::Millis,
    };
    let mut last = None;
    for _ in 0..SIG_MAX_SCANS {
        let q = JournalQuery {
            from: from.clone(),
            until: None,
            filter: frame_filter(channel, &[KIND_FRAME], Vec::new()),
            request_id: None,
            max_events: MAX_ROWS,
        };
        let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
        for rec in &scan.events {
            if rec.job_id.as_deref() == Some(job)
                && let Some(b) = frame_payload(rec).and_then(|f| f.bytes.decode().ok())
            {
                last = Some(b);
            }
        }
        match next_from(&scan) {
            Some(n) if scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS => from = n,
            _ => break,
        }
    }
    Ok(last)
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Revision conflict from the worker (`detail.current_rev`; the error code is the worker's).
fn is_conflict(e: &DomainError) -> bool {
    e.detail.get("current_rev").is_some()
}

#[derive(Debug)]
struct SigSetH {
    table: Shared,
    dbs: Dbs,
}

impl SigSetH {
    /// One read-encode-update round. `Ok(Err(e))` = the update was refused (maybe a conflict).
    async fn attempt(
        &self,
        ctx: &CallCtx,
        p: &SigSetParams,
        channel: &str,
        db: &LoadedDb,
        s: SigRef<'_>,
        rid: &str,
    ) -> Result<Result<Value, DomainError>, DomainError> {
        let job = find_job(ctx, channel, s.msg, p.job.as_deref()).await?;
        let job_id = job["job"].as_str().unwrap_or_default().to_owned();
        let rev = job["revision"]
            .as_u64()
            .ok_or_else(|| DomainError::new(ErrorCode::Internal, "periodic.list gave no revision"))?;
        // Base = the job's bytes at its latest revision, so other signals keep their values.
        let effective = job["effective_revision"].as_u64();
        if effective.is_some_and(|e| e < rev) {
            // A patch is waiting for the next tick: the listed frame is older than `rev`.
            return Err(DomainError::new(
                ErrorCode::Busy,
                format!("job {job_id} has a change not sent yet (rev {rev}); retry after its next period"),
            )
            .with_detail(json!({"job": job_id, "revision": rev, "effective_revision": effective})));
        }
        let listed = job["data"]
            .as_str()
            .or_else(|| job["frame"]["bytes"]["data"].as_str())
            .and_then(hex_bytes);
        let (mut base, source) = match listed {
            Some(b) => (b, "job"),
            None => match journal_bytes(ctx, channel, &job_id).await? {
                Some(b) => (b, "journal"),
                None => {
                    return Err(DomainError::new(
                        ErrorCode::NotFound,
                        format!("job {job_id} has not sent a frame yet; retry after its first period"),
                    ));
                }
            },
        };
        let size = s.msg.size();
        if base.len() < size {
            return Err(db_err(format!(
                "job {job_id} sends {} bytes but {} has {size}",
                base.len(),
                s.msg.name()
            )));
        }
        base.truncate(size); // FD padding beyond the DBC length is not part of the message
        let mut values = Map::new();
        values.insert(s.sig.name().to_owned(), p.value.clone());
        let frame = encode_values(s.msg, &values, EncodeBase::Payload(&base), p.force_range)?;
        let new = frame.data().to_vec();
        let changed: Vec<Value> = new
            .iter()
            .zip(&base)
            .enumerate()
            .filter(|(_, (n, o))| n != o)
            .map(|(i, (n, o))| json!({"byte": i, "old": format!("{o:02x}"), "new": format!("{n:02x}")}))
            .collect();
        let op = json!({
            "method": RPC_PERIODIC_UPDATE,
            "job": job_id,
            "expected_rev": rev,
            "patch": {"data": hex(&new)},
        });
        let res = ctx.worker(WORKER_KIND_SIM)?.write(rid, op, ctx.deadline()).await;
        Ok(res.map(|out| {
            json!({
                "job": job_id,
                "revision": out["revision"].clone(),
                "previous_revision": rev,
                "data": hex(&new),
                "changed": changed,
                "base": source,
                "db_sha256": db.sha,
            })
        }))
    }
}

impl Handler for SigSetH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: SigSetParams = parse(params)?;
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let s = resolve_signal(&db.db, &p.signal)?;
            message(&db.db, s.msg.name())?;
            let rid = ctx.request_id().unwrap_or("can-sig-set").to_owned();
            let mut out = match self.attempt(ctx, &p, &channel, &db, s, &rid).await? {
                Ok(v) => v,
                // Someone changed the job between our read and update: read again once.
                Err(e) if is_conflict(&e) => self.attempt(ctx, &p, &channel, &db, s, &rid).await??,
                Err(e) => return Err(e),
            };
            if out["base"] == json!("journal") {
                out["warning"] = json!(
                    "base bytes came from the last sent frame; a change not sent yet could be overwritten"
                );
            }
            out["channel"] = json!(channel);
            out["signal"] = json!(s.path());
            out["name"] = json!(s.sig.name());
            out["value"] = p.value.clone();
            out["unit"] = json!(s.sig.unit());
            out["message"] = json!(s.msg.name());
            out["id"] = json!(id_text(s.msg.id()));
            out["request_id"] = json!(rid);
            let env = lock(&self.table).env(&channel);
            let (risk, reason) = crate::gateway::classify_periodic(RPC_SIG_SET, env);
            out["risk"] = json!(risk);
            out["reason"] = json!(reason);
            Ok(out)
        })
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The `can.frame` payload sent to the worker for a periodic start.
pub(crate) fn frame_payload_of(frame: &CanFrame) -> Value {
    serde_json::to_value(FramePayload::from_frame(frame, None)).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------------------------
// UI data queries (G2, can-ui.md §5.4–5.6): catalog, encode preview, signal series, cursor value.
// All R0: they read the DBC and the journal, never the bus.
// ---------------------------------------------------------------------------------------------

/// Catalog page: messages per page (default / maximum).
pub const CATALOG_DEFAULT_MESSAGES: u32 = 200;
/// Catalog page: hard maximum of messages.
pub const CATALOG_MAX_MESSAGES: u32 = 1_000;
/// Value-table entries listed per signal (the rest counted).
pub const CATALOG_MAX_VALUES: usize = 256;
/// Comment bytes kept per message / signal.
pub const CATALOG_MAX_COMMENT: usize = 512;
/// Signals per `can.signal.query`.
pub const QUERY_MAX_SIGNALS: usize = 16;
/// Pixel buckets per series (UI x resolution).
pub const QUERY_MAX_PX: u32 = 4_000;
/// Frames scanned per query (raw scan bound; more → `scan_limited` + resume cursor).
pub const QUERY_MAX_FRAMES: u64 = 2_000_000;
/// Gaps listed per query (the rest counted).
pub const QUERY_MAX_GAPS: usize = 256;
/// Reply size bound (D5): larger results are cut by time with a resume point.
pub const QUERY_MAX_BYTES: usize = 64 * 1024;

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn e2e_text(r: &loom_can_dbc::E2eReason) -> String {
    match r {
        loom_can_dbc::E2eReason::MessageName { pattern } => format!("message name contains {pattern:?}"),
        loom_can_dbc::E2eReason::SignalName { signal, pattern } => {
            format!("signal {signal} contains {pattern:?}")
        }
        loom_can_dbc::E2eReason::Attribute { name } => format!("attribute {name}"),
    }
}

fn signal_meta(msg: &Message, sig: &Signal) -> Value {
    let (min, max) = sig.min_max();
    let table = sig.value_table();
    let values: Vec<Value> = table
        .iter()
        .take(CATALOG_MAX_VALUES)
        .map(|(raw, label)| json!([raw, label]))
        .collect();
    let mux = sig.mux_condition().map(|c| {
        json!({
            "multiplexor": msg.signal(c.multiplexor).map(Signal::name),
            "ranges": c.ranges.iter().map(|(lo, hi)| json!([lo, hi])).collect::<Vec<_>>(),
        })
    });
    json!({
        "name": sig.name(),
        "start_bit": sig.start_bit(),
        "size": sig.size(),
        "byte_order": match sig.byte_order() {
            loom_can_dbc::ByteOrder::Intel => "intel",
            loom_can_dbc::ByteOrder::Motorola => "motorola",
        },
        "kind": format!("{:?}", sig.kind()).to_lowercase(),
        "factor": sig.factor(),
        "offset": sig.offset(),
        "min": min,
        "max": max,
        "range": sig.range().map(|(lo, hi)| json!([lo, hi])),
        "unit": sig.unit(),
        "values": values,
        "values_total": table.len(),
        "initial": sig.initial_physical(),
        "multiplexor": sig.is_multiplexor(),
        "mux": mux,
        "receivers": sig.receivers(),
        "comment": sig.comment().map(|c| clip(c, CATALOG_MAX_COMMENT)),
    })
}

fn message_meta(db: &Database, index: usize, msg: &Message) -> Value {
    let e2e = loom_can_dbc::is_e2e_suspect(msg).map(|r| r.iter().map(e2e_text).collect::<Vec<_>>());
    json!({
        "index": index,
        "name": msg.name(),
        "id": id_text(msg.id()),
        "id_raw": msg.id().raw(),
        "ext": msg.id().is_extended(),
        "size": msg.size(),
        "fd": msg.is_fd(),
        "cycle_ms": msg.cycle_time_ms().filter(|c| *c > 0),
        "transmitter": msg.transmitter(),
        "comment": msg.comment().map(|c| clip(c, CATALOG_MAX_COMMENT)),
        "multiplexed": msg.is_multiplexed(),
        "ambiguous_id": db.is_ambiguous_id(msg.id()),
        "encode_blocked": msg.encode_blocked(),
        "decode_blocked": msg.decode_blocked(),
        "e2e_suspect": e2e,
        "signals": msg.signals().iter().map(|s| signal_meta(msg, s)).collect::<Vec<_>>(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogParams {
    #[serde(default)]
    channel: Option<String>,
    /// Message index to start from (`next_cursor` of the previous page).
    #[serde(default)]
    cursor: Option<u32>,
    #[serde(default)]
    max: Option<u32>,
    /// Case-insensitive substring of a message or signal name (autocomplete).
    #[serde(default)]
    filter: Option<String>,
}

#[derive(Debug)]
struct CatalogH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for CatalogH {
    fn call<'a>(&'a self, _ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: CatalogParams = parse(params)?;
            let max = p.max.unwrap_or(CATALOG_DEFAULT_MESSAGES);
            if max == 0 || max > CATALOG_MAX_MESSAGES {
                return Err(usage(format!("max must be 1..={CATALOG_MAX_MESSAGES}")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let filter = p.filter.as_deref().map(str::to_ascii_lowercase);
            let hit = |m: &Message| {
                filter.as_deref().is_none_or(|f| {
                    m.name().to_ascii_lowercase().contains(f)
                        || m.signals()
                            .iter()
                            .any(|s| s.name().to_ascii_lowercase().contains(f))
                })
            };
            let all = db.db.messages();
            let start = p.cursor.unwrap_or(0) as usize;
            let (mut out, mut bytes, mut next, mut oversize) = (Vec::new(), 0usize, None, false);
            for (i, m) in all.iter().enumerate().skip(start) {
                if !hit(m) {
                    continue;
                }
                let v = message_meta(&db.db, i, m);
                let size = v.to_string().len();
                // Always at least one message per page, even if it alone is above the bound.
                if !out.is_empty() && (out.len() as u32 == max || bytes + size > QUERY_MAX_BYTES) {
                    next = Some(i);
                    break;
                }
                oversize |= size > QUERY_MAX_BYTES;
                bytes += size;
                out.push(v);
            }
            Ok(json!({
                "channel": channel,
                "db_sha256": db.sha,
                "messages": out,
                "total_messages": all.len(),
                "next_cursor": next,
                "truncated": next.is_some(),
                "oversize_message": oversize,
            }))
        })
    }
}

// ---- can.encode.preview -----------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewParams {
    #[serde(default)]
    channel: Option<String>,
    msg: String,
    /// `{Sig: physical | "label" | {"raw": n}}`.
    #[serde(default)]
    signals: Map<String, Value>,
    /// `"initial"` (default), `"last"` (running job, else last frame on the bus) or hex bytes.
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    force_range: bool,
}

fn encode_kind(e: &EncodeError) -> &'static str {
    match e {
        EncodeError::Blocked { .. } => "blocked",
        EncodeError::UnknownSignal { .. } => "unknown_signal",
        EncodeError::DuplicateSignal { .. } => "duplicate_signal",
        EncodeError::MissingSignal { .. } => "missing_signal",
        EncodeError::OutOfRange { .. } => "out_of_range",
        EncodeError::RawOverflow { .. } => "raw_overflow",
        EncodeError::NotRepresentable { .. } => "not_representable",
        EncodeError::UnknownLabel { .. } => "unknown_label",
        EncodeError::InactiveMuxSignal { .. } => "inactive_mux_signal",
        EncodeError::AmbiguousMultiplexor { .. } => "ambiguous_multiplexor",
        EncodeError::ConflictingMultiplexor { .. } => "conflicting_multiplexor",
        EncodeError::BaseLengthMismatch { .. } => "base_length",
        EncodeError::Frame(_) => "frame",
    }
}

/// Newest frame of `msg` on the bus within the last `window_ms` (newest-first window steps).
async fn last_bus_bytes(ctx: &CallCtx, channel: &str, msg: &Message) -> Result<Option<Vec<u8>>, DomainError> {
    let (id, ext) = (msg.id().raw(), msg.id().is_extended());
    let mut w = FIRST_WINDOW_MS;
    loop {
        let q = JournalQuery {
            from: Cursor::Relative {
                amount: u64::from(w),
                unit: TimeUnit::Millis,
            },
            until: None,
            filter: frame_filter(channel, &[KIND_FRAME], Vec::new()),
            request_id: None,
            max_events: MAX_ROWS,
        };
        let mut from = q.from.clone();
        let mut last = None;
        for _ in 0..SIG_MAX_SCANS {
            let scan = ctx
                .journal()
                .scan(
                    &JournalQuery {
                        from: from.clone(),
                        ..q.clone()
                    },
                    SCAN_BYTES,
                )
                .await?;
            for rec in &scan.events {
                if let Some(f) = frame_payload(rec)
                    && f.id == id
                    && f.ext == ext
                    && !f.rtr
                {
                    last = f.bytes.decode().ok();
                }
            }
            match next_from(&scan) {
                Some(n) if scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS => from = n,
                _ => break,
            }
        }
        if last.is_some() || w >= SIG_MAX_WINDOW_MS {
            return Ok(last);
        }
        w = w.saturating_mul(4).min(SIG_MAX_WINDOW_MS);
    }
}

#[derive(Debug)]
struct PreviewH {
    table: Shared,
    dbs: Dbs,
}

impl PreviewH {
    /// Base bytes and where they came from.
    async fn base(
        &self,
        ctx: &CallCtx,
        channel: &str,
        msg: &Message,
        base: Option<&str>,
    ) -> Result<(Option<Vec<u8>>, &'static str, Option<String>), DomainError> {
        match base.unwrap_or("initial") {
            "initial" => Ok((None, "initial", None)),
            "last" => {
                // A running job's bytes win (IG edits that job); else what the bus last carried.
                if let Ok(job) = find_job(ctx, channel, msg, None).await {
                    let rev = job["revision"].as_u64();
                    let pending = job["effective_revision"].as_u64().is_some_and(|e| Some(e) < rev);
                    let bytes = job["data"]
                        .as_str()
                        .or_else(|| job["frame"]["bytes"]["data"].as_str())
                        .and_then(hex_bytes);
                    if let (Some(b), false) = (bytes, pending) {
                        return Ok((Some(b), "job", job["job"].as_str().map(str::to_owned)));
                    }
                }
                match last_bus_bytes(ctx, channel, msg).await? {
                    Some(b) => Ok((Some(b), "bus", None)),
                    None => Ok((None, "initial", None)),
                }
            }
            hexs => {
                let b = hex_bytes(&hexs.replace([' ', '.', ':'], "").to_ascii_lowercase())
                    .ok_or_else(|| usage("base must be \"initial\", \"last\" or hex bytes"))?;
                Ok((Some(b), "given", None))
            }
        }
    }
}

impl Handler for PreviewH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let started = Instant::now();
            let p: PreviewParams = parse(params)?;
            if p.signals.len() > MAX_SET_SIGNALS {
                return Err(usage(format!("at most {MAX_SET_SIGNALS} signals per call")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let msg = db.db.message_by_name(&p.msg).ok_or_else(|| {
                usage(format!(
                    "no message {:?} in the loaded DBC (or the name is not unique)",
                    p.msg
                ))
            })?;
            // Every range problem at once (encode itself stops at the first one).
            let mut range_errors = Vec::new();
            let mut inputs: Vec<(&str, SignalInput<'_>)> = Vec::new();
            for (name, v) in &p.signals {
                let input = match v {
                    Value::String(l) => SignalInput::Label(l.as_str()),
                    Value::Number(n) => SignalInput::Physical(n.as_f64().unwrap_or(f64::NAN)),
                    Value::Object(o) if o.len() == 1 && o.get("raw").is_some_and(Value::is_i64) => {
                        SignalInput::Raw(i128::from(o["raw"].as_i64().unwrap_or(0)))
                    }
                    _ => return Err(usage(format!("{name}: a number, a label or {{\"raw\": n}}"))),
                };
                // Raw integers are checked on their physical value too (raw × factor + offset).
                let physical = match (input, msg.signal_by_name(name)) {
                    (SignalInput::Physical(x), Some((_, sig))) => Some((x, sig)),
                    (SignalInput::Raw(r), Some((_, sig)))
                        if matches!(
                            sig.kind(),
                            loom_can_dbc::ValueKind::Unsigned | loom_can_dbc::ValueKind::Signed
                        ) =>
                    {
                        Some((r as f64 * sig.factor() + sig.offset(), sig))
                    }
                    _ => None,
                };
                if let Some((x, sig)) = physical
                    && let Some((lo, hi)) = sig.range()
                {
                    let tol = 1e-9 * lo.abs().max(hi.abs()).max(1.0);
                    if x < lo - tol || x > hi + tol {
                        range_errors.push(json!({"signal": name, "value": x, "min": lo, "max": hi}));
                    }
                }
                inputs.push((name.as_str(), input));
            }
            let (base_bytes, base_source, base_job) =
                self.base(ctx, &channel, msg, p.base.as_deref()).await?;
            let mut base_note = None;
            let base = match &base_bytes {
                Some(b) if b.len() >= msg.size() => EncodeBase::Payload(&b[..msg.size()]),
                Some(b) => {
                    return Err(usage(format!(
                        "base has {} bytes, {} needs {}",
                        b.len(),
                        msg.name(),
                        msg.size()
                    )));
                }
                None => {
                    if p.base.as_deref() == Some("last") {
                        base_note = Some("no running job or recent frame: DBC start values used");
                    }
                    EncodeBase::Initial
                }
            };
            let env = lock(&self.table).env(&channel);
            let (risk, reason) = crate::gateway::classify_send(1, env);
            let (risk_periodic, _) = crate::gateway::classify_periodic(RPC_PERIODIC_START, env);
            let mut out = json!({
                "channel": channel,
                "msg": msg.name(),
                "id": id_text(msg.id()),
                "range_errors": range_errors,
                "base_source": base_source,
                "base_job": base_job,
                "base_note": base_note,
                "risk": risk,
                "reason": reason,
                "risk_periodic": risk_periodic,
                "db_sha256": db.sha,
            });
            // Out-of-range values still get bytes (marked not sendable); other errors get none.
            let options = EncodeOptions {
                force_range: p.force_range || !range_errors.is_empty(),
                ..EncodeOptions::default()
            };
            match msg.encode(&inputs, base, &options) {
                Ok(enc) => {
                    let decoded = msg.decode(&enc.frame).ok();
                    let value_of = |i: u32| {
                        decoded
                            .as_ref()
                            .and_then(|d| d.signals.iter().find(|s| s.index == i))
                            .map(|s| match &s.value {
                                SignalValue::Value { physical, label, .. } => {
                                    json!({"value": physical, "label": label})
                                }
                                SignalValue::Unavailable(_) => Value::Null,
                            })
                    };
                    let sources: Vec<Value> = enc
                        .sources
                        .iter()
                        .filter_map(|(i, src)| {
                            let sig = msg.signal(*i)?;
                            Some(json!({
                                "signal": sig.name(),
                                "source": match src {
                                    loom_can_dbc::ValueSource::Given => "given",
                                    loom_can_dbc::ValueSource::Initial => "initial",
                                    loom_can_dbc::ValueSource::Base => "base",
                                    loom_can_dbc::ValueSource::AutoMultiplexor => "auto_multiplexor",
                                },
                                "decoded": value_of(*i),
                            }))
                        })
                        .collect();
                    let mux: Vec<Value> = decoded
                        .iter()
                        .flat_map(|d| d.signals.iter())
                        .filter(|s| s.signal.is_multiplexor())
                        .filter_map(|s| match &s.value {
                            SignalValue::Value { raw, .. } => {
                                Some(json!({"multiplexor": s.signal.name(), "value": raw_json(*raw)}))
                            }
                            SignalValue::Unavailable(_) => None,
                        })
                        .collect();
                    out["ok"] = json!(true);
                    out["bytes"] = json!(hex(enc.frame.data()));
                    out["frame"] = json!(enc.frame.to_string());
                    out["padded"] = json!(enc.padded);
                    out["sources"] = json!(sources);
                    out["mux"] = json!(mux);
                    out["sendable"] =
                        json!(p.force_range || out["range_errors"].as_array().is_none_or(Vec::is_empty));
                }
                Err(e) => {
                    out["ok"] = json!(false);
                    out["sendable"] = json!(false);
                    out["error"] = json!({"kind": encode_kind(&e), "message": e.to_string()});
                }
            }
            out["elapsed_us"] = json!(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
            Ok(out)
        })
    }
}

// ---- can.signal.query (Graphics) ---------------------------------------------------------------

/// One pixel bucket: first/last/min/max of the raw samples in it (extremes kept exactly).
#[derive(Debug, Clone, Copy)]
struct Bucket {
    first: f64,
    last: f64,
    min: f64,
    max: f64,
    n: u32,
    /// A gap lies before this bucket's first sample: the line must break here.
    brk: bool,
}

impl Bucket {
    fn add(&mut self, v: f64) {
        self.last = v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        self.n = self.n.saturating_add(1);
    }

    /// `self` is earlier than `o`.
    fn merge(self, o: Bucket) -> Bucket {
        Bucket {
            first: self.first,
            last: o.last,
            min: self.min.min(o.min),
            max: self.max.max(o.max),
            n: self.n.saturating_add(o.n),
            brk: self.brk || o.brk,
        }
    }
}

/// Shared x grid: `px` buckets of `width` ns from `start`. Without a fixed range it adapts: the
/// width doubles (adjacent buckets merge, exactly) whenever a sample lands past the end.
#[derive(Debug)]
struct Grid {
    start: Option<u64>,
    width: u64,
    px: usize,
    fixed_end: Option<u64>,
}

impl Grid {
    /// Bucket index for time `t`, growing the grid if needed (`series` are re-merged).
    fn index(&mut self, t: u64, series: &mut [Series]) -> Option<usize> {
        let start = *self.start.get_or_insert(t);
        if t < start || self.fixed_end.is_some_and(|e| t >= e) {
            return None;
        }
        loop {
            let i = (t - start) / self.width;
            if (i as usize) < self.px {
                return Some(i as usize);
            }
            if self.fixed_end.is_some() {
                return None;
            }
            self.width = self.width.saturating_mul(2);
            for s in series.iter_mut() {
                s.halve();
            }
        }
    }
}

#[derive(Debug)]
struct Series {
    buckets: Vec<Option<Bucket>>,
    raw: u64,
    brk_pending: bool,
}

impl Series {
    fn halve(&mut self) {
        let n = self.buckets.len();
        let mut out = vec![None; n];
        for (i, slot) in out.iter_mut().enumerate().take(n.div_ceil(2)) {
            let a = self.buckets.get(2 * i).copied().flatten();
            let b = self.buckets.get(2 * i + 1).copied().flatten();
            *slot = match (a, b) {
                (Some(a), Some(b)) => Some(a.merge(b)),
                (a, b) => a.or(b),
            };
        }
        self.buckets = out;
    }

    fn add(&mut self, i: usize, v: f64) {
        self.raw += 1;
        let brk = std::mem::take(&mut self.brk_pending);
        match &mut self.buckets[i] {
            Some(b) => {
                b.add(v);
                b.brk |= brk;
            }
            slot @ None => {
                *slot = Some(Bucket {
                    first: v,
                    last: v,
                    min: v,
                    max: v,
                    n: 1,
                    brk,
                })
            }
        }
    }
}

/// `5000`, `"5000"` (mono ns as the UI has them).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Ns {
    Num(u64),
    Text(String),
}

impl Ns {
    fn get(&self) -> Result<u64, DomainError> {
        match self {
            Ns::Num(n) => Ok(*n),
            Ns::Text(t) => t.parse().map_err(|_| usage(format!("bad mono ns {t:?}"))),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryParams {
    #[serde(default)]
    channel: Option<String>,
    signals: Vec<String>,
    /// Scan range as cursors (`seq:…`, `mark:…`, `-30s`, `12:00:01` …).
    t0: Cursor,
    #[serde(default)]
    t1: Option<Cursor>,
    /// Optional fixed grid on the UI's time axis (event `ts.mono_ns`); samples outside are left out.
    #[serde(default)]
    t0_ns: Option<Ns>,
    #[serde(default)]
    t1_ns: Option<Ns>,
    px: u32,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug)]
struct QueryH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for QueryH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: QueryParams = parse(params)?;
            if p.mode.as_deref().is_some_and(|m| m != "minmax") {
                return Err(usage("mode must be \"minmax\""));
            }
            if p.px == 0 || p.px > QUERY_MAX_PX {
                return Err(usage(format!("px must be 1..={QUERY_MAX_PX}")));
            }
            if p.signals.is_empty() || p.signals.len() > QUERY_MAX_SIGNALS {
                return Err(usage(format!("give 1..={QUERY_MAX_SIGNALS} signals")));
            }
            let fixed = match (&p.t0_ns, &p.t1_ns) {
                (Some(a), Some(b)) => {
                    let (a, b) = (a.get()?, b.get()?);
                    if b <= a {
                        return Err(usage("t1_ns must be after t0_ns"));
                    }
                    Some((a, b))
                }
                (None, None) => None,
                _ => return Err(usage("give both t0_ns and t1_ns, or neither")),
            };
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let refs = p
                .signals
                .iter()
                .map(|n| resolve_signal(&db.db, n))
                .collect::<Result<Vec<_>, _>>()?;
            let px = p.px as usize;
            let mut grid = match fixed {
                Some((a, b)) => Grid {
                    start: Some(a),
                    width: (b - a).div_ceil(p.px as u64).max(1),
                    px,
                    fixed_end: Some(b),
                },
                None => Grid {
                    start: None,
                    width: 1_000, // 1 µs, doubled as the data needs
                    px,
                    fixed_end: None,
                },
            };
            let mut series: Vec<Series> = refs
                .iter()
                .map(|_| Series {
                    buckets: vec![None; px],
                    raw: 0,
                    brk_pending: false,
                })
                .collect();
            let kinds = [
                KIND_FRAME,
                loom_proto::event::kinds::SOURCE_GAP,
                loom_proto::event::kinds::RECORD_GAP,
            ];
            let mut from = p.t0.clone();
            let (mut frames, mut complete, mut scan_limited) = (0u64, true, false);
            let (mut gaps, mut gaps_total) = (Vec::new(), 0u64);
            let mut resume: Option<Cursor> = None;
            let (mut first_seq, mut last_seq) = (None, None);
            loop {
                let q = JournalQuery {
                    from: from.clone(),
                    until: p.t1.clone(),
                    filter: frame_filter(&channel, &kinds, Vec::new()),
                    request_id: None,
                    max_events: MAX_ROWS,
                };
                let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
                if !scan.gaps.is_empty() {
                    // Journal-level loss inside the range: break every line, say incomplete.
                    complete = false;
                    gaps_total += scan.gaps.len() as u64;
                    for s in &mut series {
                        s.brk_pending = true;
                    }
                }
                for rec in &scan.events {
                    let t = rec.ts.mono_ns.get();
                    if rec.kind != KIND_FRAME {
                        gaps_total += 1;
                        if gaps.len() < QUERY_MAX_GAPS {
                            gaps.push(json!({"t_ns": t.to_string(), "kind": rec.kind, "cause": rec.payload["cause"], "seq": rec.seq}));
                        }
                        for s in &mut series {
                            s.brk_pending = true;
                        }
                        continue;
                    }
                    frames += 1;
                    first_seq.get_or_insert(rec.seq);
                    last_seq = Some(rec.seq);
                    for (k, r) in refs.iter().enumerate() {
                        let Some(v) = sample(*r, rec) else { continue };
                        if !v.physical.is_finite() {
                            continue;
                        }
                        if let Some(i) = grid.index(t, &mut series) {
                            series[k].add(i, v.physical);
                        }
                    }
                }
                let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
                match next_from(&scan) {
                    Some(n) if more => {
                        if frames >= QUERY_MAX_FRAMES {
                            scan_limited = true;
                            complete = false;
                            resume = Some(n);
                            break;
                        }
                        from = n;
                    }
                    _ => break,
                }
            }
            // Columns per series, cut by time when the reply would pass the size bound.
            let start = grid.start.unwrap_or(0);
            let mut col_data: Vec<[Vec<Value>; 7]> = (0..series.len()).map(|_| Default::default()).collect();
            let (mut bytes, mut cut_at, mut cut_i) = (0usize, None, 0usize);
            'grid: for i in 0..px {
                let t = start.saturating_add(grid.width.saturating_mul(i as u64));
                for (k, s) in series.iter().enumerate() {
                    let Some(b) = s.buckets[i] else { continue };
                    let row = [
                        json!(i),
                        json!(b.first),
                        json!(b.last),
                        json!(b.min),
                        json!(b.max),
                        json!(b.n),
                        json!(b.brk),
                    ];
                    let size: usize = row.iter().map(|v| v.to_string().len() + 1).sum();
                    if bytes + size > QUERY_MAX_BYTES {
                        cut_at = Some(t);
                        cut_i = i;
                        break 'grid;
                    }
                    bytes += size;
                    for (c, v) in col_data[k].iter_mut().zip(row) {
                        c.push(v);
                    }
                }
            }
            if cut_at.is_some() {
                // Drop the partial instant so every series ends at the same time.
                for c in &mut col_data {
                    while c[0].last().and_then(Value::as_u64) == Some(cut_i as u64) {
                        for col in c.iter_mut() {
                            col.pop();
                        }
                    }
                }
            }
            let out_series: Vec<Value> = refs
                .iter()
                .zip(series.iter())
                .zip(col_data)
                .map(|((r, s), c)| {
                    let mut m = Map::new();
                    let points: u64 = s.buckets.iter().flatten().map(|b| u64::from(b.n.min(4))).sum();
                    let [i, first, last, min, max, n, brk] = c;
                    m.insert("signal".into(), json!(r.path()));
                    m.insert("unit".into(), json!(r.sig.unit()));
                    m.insert("columns".into(), json!({"i": i, "first": first, "last": last, "min": min, "max": max, "n": n, "brk": brk}));
                    m.insert("decimated".into(), json!({"raw": s.raw, "points": points}));
                    Value::Object(m)
                })
                .collect();
            let mut extra = Map::new();
            extra.insert("action".into(), json!("read"));
            publish_activity(
                ctx,
                RPC_SIGNAL_QUERY,
                &channel,
                format!("graphed {} signals on {channel}", refs.len()),
                extra,
            );
            Ok(json!({
                "channel": channel,
                "db_sha256": db.sha,
                "mode": "minmax",
                "grid": {"start_ns": grid.start.map(|v| v.to_string()), "width_ns": grid.width.to_string(), "px": p.px, "fixed": fixed.is_some()},
                "series": out_series,
                "gaps": gaps,
                "gaps_total": gaps_total,
                "frames_scanned": frames,
                "range": match (first_seq, last_seq) {
                    (Some(a), Some(b)) => json!({"from": a, "to": b}),
                    _ => Value::Null,
                },
                "complete": complete,
                "scan_limited": scan_limited,
                "resume": resume.map(|c| c.to_string()),
                "truncated": cut_at.is_some(),
                "resume_t0_ns": cut_at.map(|v: u64| v.to_string()),
            }))
        })
    }
}

// ---- can.signal.at (Graphics cursor) -----------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AtParams {
    #[serde(default)]
    channel: Option<String>,
    signal: String,
    /// Cursor position on the UI time axis (event `ts.mono_ns`).
    t: Ns,
    /// Where the scan starts (default `-60s`); the UI passes its query's `t0`.
    #[serde(default)]
    from: Option<Cursor>,
}

#[derive(Debug)]
struct AtH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for AtH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: AtParams = parse(params)?;
            let t = p.t.get()?;
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let db = channel_db(&self.dbs, &channel)?;
            let s = resolve_signal(&db.db, &p.signal)?;
            let kinds = [
                KIND_FRAME,
                loom_proto::event::kinds::SOURCE_GAP,
                loom_proto::event::kinds::RECORD_GAP,
            ];
            let mut from = p.from.clone().unwrap_or(Cursor::Relative {
                amount: 60,
                unit: TimeUnit::Secs,
            });
            let mut before: Option<(PublishedRecord, Sample)> = None;
            let mut after: Option<(PublishedRecord, Sample)> = None;
            let (mut frames, mut complete, mut gap_between) = (0u64, true, false);
            'scan: loop {
                let q = JournalQuery {
                    from: from.clone(),
                    until: None,
                    filter: frame_filter(&channel, &kinds, Vec::new()),
                    request_id: None,
                    max_events: MAX_ROWS,
                };
                let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
                complete &= scan.gaps.is_empty();
                for rec in &scan.events {
                    let rt = rec.ts.mono_ns.get();
                    if rec.kind != KIND_FRAME {
                        // A gap between the two samples: the cursor value is not continuous.
                        if before.is_some() {
                            gap_between = true;
                        }
                        continue;
                    }
                    frames += 1;
                    let Some(v) = sample(s, rec) else { continue };
                    if rt <= t {
                        before = Some((rec.clone(), v));
                        gap_between = false;
                    } else {
                        after = Some((rec.clone(), v));
                        break 'scan;
                    }
                }
                let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
                match next_from(&scan) {
                    Some(n) if more && frames < QUERY_MAX_FRAMES => from = n,
                    Some(_) if more => {
                        complete = false;
                        break;
                    }
                    _ => break,
                }
            }
            let mut evidence = Vec::new();
            let mut point = |hit: &Option<(PublishedRecord, Sample)>| -> Value {
                let Some((rec, v)) = hit else { return Value::Null };
                let (ev, r) = sample_evidence(&channel, s, v, rec, &db.sha, None);
                evidence.push(ev);
                let mut o = value_json(s, v);
                o["t_ns"] = json!(rec.ts.mono_ns.get().to_string());
                o["dt_ns"] = json!(i128::from(rec.ts.mono_ns.get()) - i128::from(t));
                o["seq"] = json!(rec.seq);
                o["origin"] = json!(rec.origin);
                o["ref"] = json!(r);
                o
            };
            let (b, a) = (point(&before), point(&after));
            Ok(json!({
                "channel": channel,
                "signal": s.path(),
                "unit": s.sig.unit(),
                "t_ns": t.to_string(),
                "before": b,
                "after": a,
                "gap_between": gap_between,
                "complete": complete,
                "frames_scanned": frames,
                "db_sha256": db.sha,
                "evidence": evidence,
            }))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Registration (called from `gateway::Can::register`)
// ---------------------------------------------------------------------------------------------

fn schemas() -> [Schema; 4] {
    let origin = json_schema!({"enum": ["agent", "user", "device", "script"]});
    [
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["path"],
            "properties": {
                "channel": {"type": "string"},
                "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                "encoding": {"enum": ["utf8", "utf-8", "windows1252", "cp1252", "latin1", "auto"]}
            }
        }),
        json_schema!({
            "type": "object", "additionalProperties": false,
            "properties": {"channel": {"type": "string"}}
        }),
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["signal"],
            "properties": {
                "channel": {"type": "string"},
                "signal": {"type": "string", "minLength": 1, "maxLength": 256},
                "window_ms": {"type": "integer", "minimum": 1, "maximum": SIG_MAX_WINDOW_MS},
                "origin": origin
            }
        }),
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["expr", "within_ms"],
            "properties": {
                "channel": {"type": "string"},
                "expr": {"type": "string", "minLength": 1, "maxLength": MAX_EXPR_BYTES},
                "after": {"type": "string"},
                "within_ms": {"type": "integer", "minimum": 1, "maximum": WAIT_MAX_MS},
                "origin": origin,
                "any_origin": {"type": "boolean"}
            }
        }),
    ]
}

pub(crate) fn register(reg: &mut Registry, table: &Shared, dbs: &Dbs) -> Result<(), RegisterError> {
    let [load, info, get, wait] = schemas();
    reg.command(read_spec(
        RPC_DB_LOAD,
        load,
        Arc::new(DbLoadH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(30),
    ))?
    .command(read_spec(
        RPC_DB_INFO,
        info,
        Arc::new(DbInfoH { dbs: dbs.clone() }),
        Duration::from_secs(5),
    ))?
    .command(read_spec(
        RPC_SIG_GET,
        get,
        Arc::new(SigGetH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(10),
    ))?
    .command(read_spec(
        RPC_SIG_WAIT_FOR,
        wait,
        Arc::new(SigWaitH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_millis(u64::from(WAIT_MAX_MS)),
    ))?
    .command(CommandSpec {
        write_class: Some(WriteClass::Job),
        roles: RoleSet::WRITE,
        ..read_spec(
            RPC_SIG_SET,
            json_schema!({
                "type": "object", "additionalProperties": false, "required": ["signal", "value"],
                "properties": {
                    "channel": {"type": "string"},
                    "signal": {"type": "string", "minLength": 1, "maxLength": 256},
                    "value": {"type": ["number", "string"]},
                    "job": {"type": "string"},
                    "force_range": {"type": "boolean"}
                }
            }),
            Arc::new(SigSetH {
                table: table.clone(),
                dbs: dbs.clone(),
            }),
            Duration::from_secs(10),
        )
    })?
    .command(read_spec(
        RPC_DB_CATALOG,
        json_schema!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "channel": {"type": "string"}, "cursor": {"type": "integer", "minimum": 0},
                "max": {"type": "integer", "minimum": 1, "maximum": CATALOG_MAX_MESSAGES},
                "filter": {"type": "string", "maxLength": 128}
            }
        }),
        Arc::new(CatalogH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(5),
    ))?
    .command(read_spec(
        RPC_ENCODE_PREVIEW,
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["msg"],
            "properties": {
                "channel": {"type": "string"}, "msg": {"type": "string"},
                "signals": {"type": "object", "maxProperties": MAX_SET_SIGNALS},
                "base": {"type": "string", "maxLength": 200}, "force_range": {"type": "boolean"}
            }
        }),
        Arc::new(PreviewH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(2),
    ))?
    .command(read_spec(
        RPC_SIGNAL_QUERY,
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["signals", "t0", "px"],
            "properties": {
                "channel": {"type": "string"},
                "signals": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": QUERY_MAX_SIGNALS},
                "t0": {"type": "string"}, "t1": {"type": "string"},
                "t0_ns": {"type": ["integer", "string"]}, "t1_ns": {"type": ["integer", "string"]},
                "px": {"type": "integer", "minimum": 1, "maximum": QUERY_MAX_PX},
                "mode": {"enum": ["minmax"]}
            }
        }),
        Arc::new(QueryH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(30),
    ))?
    .command(read_spec(
        RPC_SIGNAL_AT,
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["signal", "t"],
            "properties": {
                "channel": {"type": "string"}, "signal": {"type": "string"},
                "t": {"type": ["integer", "string"]}, "from": {"type": "string"}
            }
        }),
        Arc::new(AtH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
        Duration::from_secs(10),
    ))?
    .event_kind(EventKindSpec {
        kind: KIND_DB_CHANGED.into(),
        payload_schema: json_schema!({
            "type": "object", "required": ["db_sha256"],
            "properties": {
                "channel": {"type": "string"},
                "db_sha256": {"type": "string"},
                "previous_sha256": {"type": ["string", "null"]},
                "path": {"type": "string"}
            }
        }),
        pointers: vec![ptr("/payload/db_sha256", ValueType::String)],
        version: 1,
    })?;
    Ok(())
}
