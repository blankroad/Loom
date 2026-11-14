//! Gateway piece: registration, strict parameter validation, FFmpeg resolution (`loom.toml
//! [video] ffmpeg` or PATH — never from RPC params), storage roots from the daemon
//! (`CallCtx::storage_roots`, never `~/.loom`), bounded journal read, status-based wait, reducer.

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

use crate::model::{self, OpenConfig};
use crate::{DOMAIN, KIND_CAPTURE_STATE, KIND_HEALTH, KIND_RECORD_STATE, KIND_SNAPSHOT, WORKER_KIND};

/// Default command wait.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(15);
/// `read --follow` bound.
pub const MAX_READ_WAIT_MS: u64 = 30_000;
/// `wait` bound.
pub const MAX_WAIT_MS: u64 = 600_000;
/// `read --follow` default.
pub const FOLLOW_DEFAULT_MS: u64 = 10_000;
/// Rows per read.
pub const MAX_ROWS: u32 = 1000;
/// Rows budget per read result (64 KiB envelope headroom).
pub const ROW_BUDGET: usize = 56 * 1024;
/// Journal bytes examined per read poll.
pub const SCAN_BUDGET: u64 = 4 << 20;
/// Poll period.
pub const POLL: Duration = Duration::from_millis(100);
/// Reducer bounds.
pub const SNAP_CAPTURES: usize = 4;
/// Snapshots kept for late joiners.
pub const SNAP_SNAPSHOTS: usize = 16;
/// Capture states that are terminal.
pub const TERMINAL: &[&str] = &["closed", "failed"];
/// All capture states.
pub const STATES: &[&str] = &["opening", "live", "stalled", "stopping", "closed", "failed"];

/// The domain module.
#[derive(Debug, Default)]
pub struct Video;

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

fn opt_bool(o: &Map<String, Value>, k: &str) -> Result<bool, DomainError> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(usage(format!("{k}: expected a boolean"))),
    }
}

fn capture_id(o: &Map<String, Value>) -> Result<Option<&str>, DomainError> {
    let c = opt_str(o, "capture_id", 64)?;
    if c.is_some_and(|c| !model::valid_capture_id(c)) {
        return Err(usage("capture_id: expected v + 12 lowercase hex"));
    }
    Ok(c)
}

fn frame_id(o: &Map<String, Value>) -> Result<Option<u64>, DomainError> {
    match opt_u64(o, "frame_id", u64::MAX)? {
        Some(0) => Err(usage("frame_id: expected a positive integer")),
        v => Ok(v),
    }
}

fn cursor(o: &Map<String, Value>) -> Result<Option<Cursor>, DomainError> {
    opt_str(o, "since", 128)?
        .map(|s| s.parse::<Cursor>().map_err(|e| usage(format!("since: {e}"))))
        .transpose()
}

/// `(path, error)` of the FFmpeg engine.
fn ffmpeg(ctx: &CallCtx) -> (Option<String>, Option<String>) {
    let cfg = ctx.config();
    let configured = cfg.get("ffmpeg").and_then(Value::as_str).map(str::to_owned);
    let path = std::env::var("PATH").ok();
    match model::resolve_ffmpeg(configured.as_deref(), path.as_deref()) {
        Ok(p) => (p.to_str().map(str::to_owned), None),
        Err(e) => (None, Some(e)),
    }
}

fn storage(ctx: &CallCtx) -> Result<Value, DomainError> {
    let r = ctx.storage_roots().ok_or_else(|| {
        DomainError::new(
            ErrorCode::Config,
            "daemon storage roots unavailable; video artifacts are never written to a default home",
        )
    })?;
    Ok(json!({"home": r.home.display().to_string(), "session": r.session.display().to_string()}))
}

/// Worker args: params plus daemon-owned fields (storage, engine). Callers cannot set these:
/// every handler validates params strictly before this.
fn with_daemon(ctx: &CallCtx, mut args: Value) -> Result<Value, DomainError> {
    let (path, error) = ffmpeg(ctx);
    args["storage"] = storage(ctx)?;
    args["ffmpeg"] = json!(path);
    args["ffmpeg_error"] = json!(error);
    Ok(args)
}

async fn sleep(ctx: &CallCtx, d: Duration) -> Result<(), DomainError> {
    ctx.spawn_blocking(move || std::thread::sleep(d)).await
}

macro_rules! forward {
    ($name:ident, $method:literal, [$($key:literal),*], $daemon:expr) => {
        #[derive(Debug)]
        struct $name;
        impl Handler for $name {
            fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
                Box::pin(async move {
                    let keys: &[&str] = &[$($key),*];
                    let o = strict(&params, keys)?;
                    #[allow(unused_mut)]
                    let mut args = json!({});
                    for &k in keys {
                        match k {
                            "capture_id" => { if let Some(c) = capture_id(o)? { args[k] = json!(c); } }
                            "frame_id" => { if let Some(f) = frame_id(o)? { args[k] = json!(f); } }
                            "local" => { args[k] = json!(opt_bool(o, k)?); }
                            _ => {}
                        }
                    }
                    let args = if $daemon { with_daemon(ctx, args)? } else { args };
                    ctx.worker(WORKER_KIND)?.call($method, args, ctx.deadline()).await
                })
            }
        }
    };
}

forward!(Close, "video.close", ["capture_id"], false);
forward!(Status, "video.status", [], false);
forward!(Frame, "video.frame", ["capture_id", "frame_id"], false);
forward!(Snapshot, "video.snapshot", ["capture_id", "frame_id"], false);
forward!(RecordStart, "video.record.start", ["capture_id"], true);
forward!(RecordStop, "video.record.stop", ["capture_id"], false);
forward!(Sources, "video.sources", ["local"], true);

#[derive(Debug)]
struct Open;
impl Handler for Open {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let cfg = OpenConfig::from_params(&params).map_err(|e| DomainError::new(e.code, e.message))?;
            let args = with_daemon(ctx, json!({"config": cfg.to_params()}))?;
            ctx.worker(WORKER_KIND)?
                .call("video.open", args, ctx.deadline())
                .await
        })
    }
}

#[derive(Debug)]
struct Capabilities;
impl Handler for Capabilities {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            strict(&params, &[])?;
            let args = with_daemon(ctx, json!({}))?;
            let w = ctx.worker(WORKER_KIND)?;
            let probe = w.call("video.probe", args.clone(), ctx.deadline()).await?;
            let quota = w.call("video.quota", args, ctx.deadline()).await?;
            let mut v = model::capabilities(&probe);
            v["quota_usage"] = quota;
            Ok(v)
        })
    }
}

fn kinds_for(kind: &str) -> Result<&'static [&'static str], DomainError> {
    Ok(match kind {
        "state" => &[KIND_CAPTURE_STATE],
        "health" => &[KIND_HEALTH],
        "snapshot" => &[KIND_SNAPSHOT],
        "record" => &[KIND_RECORD_STATE],
        "all" => &[KIND_CAPTURE_STATE, KIND_HEALTH, KIND_SNAPSHOT, KIND_RECORD_STATE],
        o => {
            return Err(usage(format!(
                "kind {o:?}: expected state, health, snapshot, record or all"
            )));
        }
    })
}

/// Bounded journal scan from `from` (same discipline as loom-logic).
async fn scan(
    ctx: &CallCtx,
    from: Cursor,
    kinds: &[&str],
    capture: Option<&str>,
) -> Result<(Vec<PublishedRecord>, Cursor, bool), DomainError> {
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
            Some(c) => return Ok((out, c, false)),
            None => return Ok((out, next, !gaps)),
        }
    }
}

/// Read rows: payload + `seq` + `type`.
pub fn rows(events: &[PublishedRecord]) -> Vec<Value> {
    events
        .iter()
        .map(|e| {
            let mut r = e.payload.clone();
            r["seq"] = json!(e.seq.get());
            r["type"] = json!(e.kind.strip_prefix("video.").unwrap_or(&e.kind));
            r
        })
        .collect()
}

/// Keep the newest `tail` rows within [`ROW_BUDGET`]. Returns (rows, rows left out).
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
            let capture = capture_id(o)?;
            let kinds = kinds_for(opt_str(o, "kind", 16)?.unwrap_or("all"))?;
            let tail = opt_u64(o, "tail", u64::from(MAX_ROWS))?.unwrap_or(100).max(1) as usize;
            let follow = opt_bool(o, "follow")?;
            let wait_ms = match (opt_u64(o, "wait_ms", MAX_READ_WAIT_MS)?, follow) {
                (Some(0) | None, false) => 0,
                (Some(_), false) => return Err(usage("wait_ms > 0 needs follow=true")),
                (Some(0), true) => return Err(usage("wait_ms: expected 1..=30000 with follow")),
                (Some(w), true) => w,
                (None, true) => FOLLOW_DEFAULT_MS,
            };
            let mut from = cursor(o)?.unwrap_or(Cursor::Relative {
                amount: 10,
                unit: TimeUnit::Mins,
            });
            let deadline = (Instant::now() + Duration::from_millis(wait_ms)).min(ctx.deadline());
            loop {
                let (events, next, complete) = scan(ctx, from.clone(), kinds, capture).await?;
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

/// `video.wait`: poll worker status until `frames` (latest worker frame id ≥ N) and/or
/// `state` holds for the current capture.
#[derive(Debug)]
struct Wait;
impl Handler for Wait {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let o = strict(&params, &["capture_id", "frames", "state", "timeout_ms"])?;
            let capture = capture_id(o)?.map(str::to_owned);
            let frames = opt_u64(o, "frames", u64::MAX)?;
            let state = opt_str(o, "state", 16)?.map(str::to_owned);
            if state.as_deref().is_some_and(|s| !STATES.contains(&s)) {
                return Err(usage(format!("state: expected one of {}", STATES.join(", "))));
            }
            if frames.is_none() && state.is_none() {
                return Err(usage("give frames and/or state"));
            }
            let timeout_ms = match opt_u64(o, "timeout_ms", MAX_WAIT_MS)? {
                Some(0) | None => return Err(usage(format!("timeout_ms: required, 1..={MAX_WAIT_MS}"))),
                Some(t) => t,
            };
            let deadline = (Instant::now() + Duration::from_millis(timeout_ms)).min(ctx.deadline());
            loop {
                let st = ctx
                    .worker(WORKER_KIND)?
                    .call("video.status", json!({}), ctx.deadline())
                    .await?;
                let same = capture
                    .as_deref()
                    .is_none_or(|c| st["capture_id"].as_str() == Some(c));
                let cur_state = st["capture"]["state"].as_str().unwrap_or("");
                let last = st["health"]["last_frame_id"].as_u64().unwrap_or(0);
                let ok_frames = frames.is_none_or(|n| last >= n);
                let ok_state = state.as_deref().is_none_or(|s| cur_state == s);
                if same && ok_frames && ok_state {
                    return Ok(json!({"matched": true, "status": st}));
                }
                // A terminal capture can no longer reach a frame/state goal it has not reached.
                if same && TERMINAL.contains(&cur_state) && state.as_deref().is_none_or(|s| s != cur_state) {
                    return Err(DomainError::new(
                        ErrorCode::NotOpen,
                        format!("capture ended ({cur_state}) before the wait condition"),
                    )
                    .with_detail(json!({"status": st})));
                }
                if Instant::now() + POLL >= deadline {
                    return Err(DomainError::new(ErrorCode::Timeout, "video wait timed out")
                        .with_detail(json!({"status": st, "timeout_ms": timeout_ms})));
                }
                sleep(ctx, POLL).await?;
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Hooks, reducer, registration.

/// Write hooks: observation-only capture control (never drives a device line).
#[derive(Debug)]
pub struct VideoHooks;
impl WriteHooks for VideoHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        let (risk, reason) = match req.method {
            "video.open" => (Risk::R1, "starts an FFmpeg acquisition process (observation only)"),
            "video.record.start" => (Risk::R1, "writes a bounded recording artifact"),
            "video.snapshot" => (Risk::R1, "writes a bounded snapshot artifact"),
            _ => (Risk::R0, "stops capture or recording"),
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

/// Late-joiner snapshot: recent capture states, latest health per capture, recent snapshots and
/// record states (metadata only).
#[derive(Debug, Default)]
pub struct VideoReducer {
    captures: VecDeque<(u64, Value)>,
    health: VecDeque<Value>,
    records: VecDeque<Value>,
    snapshots: VecDeque<Value>,
}

fn upsert(q: &mut VecDeque<Value>, key: &str, v: &Value, cap: usize) {
    if let Some(slot) = q.iter_mut().find(|x| x[key] == v[key]) {
        *slot = v.clone();
    } else {
        q.push_back(v.clone());
        while q.len() > cap {
            q.pop_front();
        }
    }
}

impl Reducer for VideoReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_CAPTURE_STATE, KIND_HEALTH, KIND_SNAPSHOT, KIND_RECORD_STATE]
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
            KIND_HEALTH => upsert(&mut self.health, "capture_id", p, SNAP_CAPTURES),
            KIND_RECORD_STATE => upsert(&mut self.records, "recording_id", p, SNAP_CAPTURES * 2),
            KIND_SNAPSHOT => {
                self.snapshots.push_back(p.clone());
                while self.snapshots.len() > SNAP_SNAPSHOTS {
                    self.snapshots.pop_front();
                }
            }
            _ => {}
        }
    }
    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        let keep = |v: &&Value| {
            filter.channels.is_empty()
                || v["capture_id"]
                    .as_str()
                    .is_some_and(|c| filter.channels.iter().any(|f| f == c))
        };
        vec![json!({
            "captures": self.captures.iter().map(|(_, v)| v).filter(keep).collect::<Vec<_>>(),
            "health": self.health.iter().filter(keep).collect::<Vec<_>>(),
            "records": self.records.iter().filter(keep).collect::<Vec<_>>(),
            "snapshots": self.snapshots.iter().filter(keep).collect::<Vec<_>>(),
        })]
    }
}

fn spec(method: &str, write: bool, handler: Arc<dyn Handler>, params: Value, max_wait: Duration) -> CommandSpec {
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

/// Help (`loom video help --json`).
pub fn help() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "Video capture through a daemon-owned FFmpeg: live metadata (state, 1 s health) in the shared \
                  journal, frames on a separate authenticated media plane, bounded snapshot/recording artifacts."
            .into(),
        examples: vec![
            ex(
                "loom video open --source http-mjpeg --url http://127.0.0.1:8090/stream --size 1280x720 --fps 30",
                "open an explicit loopback MJPEG source (no default device)",
            ),
            ex("loom video wait --frames 30 --timeout 10s", "wait until 30 frames were received"),
            ex("loom video frame --id 120", "metadata + sha256 of one ring frame (no bytes)"),
            ex("loom video snapshot --frame 120", "save that exact JPEG as an artifact"),
            ex("loom video record start", "start an MJPEG-copy Matroska recording (bounded)"),
            ex("loom video close", "stop acquisition and reap FFmpeg"),
        ],
        notes: vec![
            "Frames never enter the journal; agents use health/status/frame metadata and snapshots.".into(),
            "frame_id is the worker's receive counter, not a source frame number; source_pts is null.".into(),
            "No automatic reconnect: after a disconnect, open again explicitly.".into(),
            "avfoundation needs an exact device name; it is implemented but unverified on hardware; audio, RTSP and DeckLink are unavailable."
                .into(),
            "Storage is capped (artifacts/bytes per session, home-wide bytes, recording size/duration); nothing is auto-deleted."
                .into(),
            "Recordings copy the preview JPEGs into Matroska (large files, not H.264).".into(),
        ],
    }
}

impl DomainModule for Video {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let obj = |props: Value| json!({"type": "object", "properties": props});
        let s = json!({"type": "string"});
        let i = json!({"type": "integer"});
        let b = json!({"type": "boolean"});
        let cap = json!({"capture_id": s});
        let commands = vec![
            spec("video.capabilities", false, Arc::new(Capabilities), obj(json!({})), DEFAULT_WAIT),
            spec("video.sources", false, Arc::new(Sources), obj(json!({"local": b})), DEFAULT_WAIT),
            spec(
                "video.open",
                true,
                Arc::new(Open),
                json!({"type": "object", "required": ["source"], "properties": {
                    "source": {"enum": model::SOURCE_KINDS}, "url": s, "name": s, "device": s,
                    "width": i, "height": i, "fps": i, "quality": i}}),
                DEFAULT_WAIT,
            ),
            spec("video.status", false, Arc::new(Status), obj(json!({})), DEFAULT_WAIT),
            spec(
                "video.frame",
                false,
                Arc::new(Frame),
                obj(json!({"capture_id": s, "frame_id": i})),
                DEFAULT_WAIT,
            ),
            spec(
                "video.snapshot",
                true,
                Arc::new(Snapshot),
                obj(json!({"capture_id": s, "frame_id": i})),
                DEFAULT_WAIT,
            ),
            spec("video.record.start", true, Arc::new(RecordStart), obj(cap.clone()), DEFAULT_WAIT),
            spec("video.record.stop", true, Arc::new(RecordStop), obj(cap.clone()), DEFAULT_WAIT),
            spec("video.close", true, Arc::new(Close), obj(cap), DEFAULT_WAIT),
            spec(
                "video.read",
                false,
                Arc::new(Read),
                obj(json!({"capture_id": s, "kind": {"enum": ["state", "health", "snapshot", "record", "all"]},
                           "tail": i, "since": s, "wait_ms": i, "follow": b})),
                Duration::from_millis(MAX_READ_WAIT_MS) + DEFAULT_WAIT,
            ),
            spec(
                "video.wait",
                false,
                Arc::new(Wait),
                obj(json!({"capture_id": s, "frames": i, "state": {"enum": STATES}, "timeout_ms": i})),
                Duration::from_millis(MAX_WAIT_MS) + DEFAULT_WAIT,
            ),
        ];
        for mut c in commands {
            // Safe-direction stops are allowed to watchers too (like logic.stop / core STOP).
            if c.method == "video.close" || c.method == "video.record.stop" {
                c.roles = RoleSet::READ;
            }
            reg.command(c)?;
        }
        let states = ValueType::Enum(STATES.iter().map(|s| (*s).to_owned()).collect());
        reg.event_kind(kind(
            KIND_CAPTURE_STATE,
            &["capture_id", "state"],
            vec![
                ptr("/payload/state", states),
                ptr("/payload/reason", ValueType::String),
            ],
        ))?
        .event_kind(kind(KIND_HEALTH, &["capture_id", "frames_total"], vec![]))?
        .event_kind(kind(
            KIND_SNAPSHOT,
            &["capture_id", "artifact_id", "frame_id", "sha256"],
            vec![ptr("/payload/sha256", ValueType::String)],
        ))?
        .event_kind(kind(
            KIND_RECORD_STATE,
            &["capture_id", "state", "recording_id"],
            vec![ptr("/payload/state", ValueType::String)],
        ))?
        .risk_table(RiskTable {
            rows: vec![
                RiskRow {
                    method: "video.open".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "observation-only capture (spawns ffmpeg)".into(),
                },
                RiskRow {
                    method: "video.snapshot".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "bounded artifact write".into(),
                },
                RiskRow {
                    method: "video.record.start".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "bounded artifact write".into(),
                },
                RiskRow {
                    method: "video.record.stop".into(),
                    selector: None,
                    risk: Risk::R0,
                    reason: "finalizes a recording".into(),
                },
                RiskRow {
                    method: "video.close".into(),
                    selector: None,
                    risk: Risk::R0,
                    reason: "stops a capture".into(),
                },
            ],
            ..RiskTable::default()
        })?
        .hooks(Arc::new(VideoHooks))?
        .worker(WorkerSpec {
            kind: WORKER_KIND.into(),
            instances: Instances::Shared,
            budgets: Budgets {
                mem_soft: 48 << 20,
                mem_hard: 96 << 20,
                credit: 4 << 20,
            },
            reopen_after_crash: Reopen::None,
            arch: Arch::Native,
            platforms: vec![Platform::Linux, Platform::Macos],
        })?
        .reducer(Box::new(VideoReducer::default()))?
        .capability("ffmpeg")?
        .help(help())?;
        Ok(())
    }
}
