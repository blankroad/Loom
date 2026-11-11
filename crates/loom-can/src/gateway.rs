//! Gateway piece (G1): what `loomd` loads for CAN — registration, write hooks, bounded queries
//! (`can.trace.query`, `can.last`, `can.expect`), the Fixed-view reducer and the evidence
//! verifier. Device access always goes through the `can_sim` worker (I1).
//!
//! Bounds (C2): rows ≤ 1,000 and ≤ 64 KiB per result; at most [`MAX_SCANS`] journal scans of
//! [`SCAN_BYTES`] each per query; `can.expect` ≤ [`MAX_EXPECT_MS`]; Fixed snapshot ≤
//! [`FIXED_MAX_CHANNELS`] channels × [`FIXED_MAX_IDS`] ids (oldest replaced, counted).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use loom_domain_api::*;
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, PublishedRecord, Risk, SeqRange, WriteState, kinds};
use loom_proto::evidence::{Evidence, EvidenceRef, RefString};
use loom_proto::policy::WriteClass;
use loom_proto::rpc::{Role, SubFilter};
use schemars::{Schema, json_schema, schema_for};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::schema::{FramePayload, MAX_EXPECT_MS, SendExpect, SendParams, TxPayload};
use crate::signals::{
    Dbs, LoadedDb, PRED_SIGNAL_RANGE, decode_db, decode_record, start_frame, verify_signal_range,
};
use crate::{
    DOMAIN, KIND_CHANNEL_STATE, KIND_FRAME, KIND_ISOTP_RX_COMPLETE, KIND_ISOTP_RX_ERROR, KIND_ISOTP_RX_START,
    KIND_ISOTP_STATE, KIND_ISOTP_TX_DONE, KIND_ISOTP_TX_ERROR, KIND_JOB_STATE, KIND_STATS, KIND_TX,
    RPC_EXPECT, RPC_ISOTP_ABORT, RPC_ISOTP_BARRIER, RPC_ISOTP_CANCEL, RPC_ISOTP_CLOSE, RPC_ISOTP_LIST,
    RPC_ISOTP_OPEN, RPC_ISOTP_RECV, RPC_ISOTP_SEND, RPC_LAST, RPC_PERIODIC_LIST, RPC_PERIODIC_RENEW,
    RPC_PERIODIC_START, RPC_PERIODIC_STOP, RPC_PERIODIC_TAKEOVER, RPC_PERIODIC_UPDATE, RPC_SEND, RPC_SIG_SET,
    RPC_STATUS, RPC_TRACE_QUERY, WORKER_KIND_SIM,
};

/// Default rows of `can.trace.query` (plan §4.1: CAN 100).
pub const DEFAULT_ROWS: u32 = 100;
/// Hard row limit (D5).
pub const MAX_ROWS: u32 = 1_000;
/// Hard result size (D5): serialized rows.
pub const MAX_RESULT_BYTES: usize = 64 * 1024;
/// Bytes per journal scan call.
pub const SCAN_BYTES: u64 = 4 * 1024 * 1024;
/// Journal scans per query (continuing with `resume_from`).
pub const MAX_SCANS: usize = 8;
/// `can.expect` journal poll interval (M1, until CORE offers `ctx.wait` — can-core.md 1).
pub const EXPECT_POLL: Duration = Duration::from_millis(10);
/// Default / maximum look-back of `can.last`.
pub const LAST_DEFAULT_WINDOW_MS: u32 = 10_000;
/// Maximum look-back of `can.last`.
pub const LAST_MAX_WINDOW_MS: u32 = 60_000;
/// Channels kept by the Fixed reducer and the channel table.
pub const FIXED_MAX_CHANNELS: usize = 8;
/// Ids per channel kept by the Fixed reducer / `can.last`.
pub const FIXED_MAX_IDS: usize = 2_048;
/// `count` above this is shown as R3 (finite burst, plan §10.2). Display only under F1.
pub const BURST_R3_ABOVE: u64 = 10;

pub(crate) fn usage(msg: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Usage, msg)
}

pub(crate) fn parse<T: DeserializeOwned>(v: Value) -> Result<T, DomainError> {
    serde_json::from_value(v).map_err(|e| usage(format!("bad parameters: {e}")))
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // State stays valid after a panic elsewhere; never take the hub thread down.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------------------------
// Channel table: shared by the reducer (writer, from `can.channel_state`), hooks (presend) and
// handlers (default channel, dry-run env).
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct ChannelInfo {
    state: String,
    backend: Option<String>,
    generation: Option<u32>,
    last_seq: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ChannelTable {
    channels: BTreeMap<String, ChannelInfo>,
}

impl ChannelTable {
    fn update(&mut self, ev: &PublishedRecord) {
        let Some(ch) = &ev.channel else { return };
        let state = ev.payload["state"].as_str().unwrap_or("unknown").to_owned();
        let generation = ev.generation.or_else(|| {
            ev.payload["generation"]
                .as_u64()
                .and_then(|g| u32::try_from(g).ok())
        });
        // Bounded: forget the channel with the oldest state change.
        if !self.channels.contains_key(ch)
            && self.channels.len() >= FIXED_MAX_CHANNELS
            && let Some(old) = self
                .channels
                .iter()
                .min_by_key(|(_, i)| i.last_seq)
                .map(|(k, _)| k.clone())
        {
            self.channels.remove(&old);
        }
        self.channels.insert(
            ch.clone(),
            ChannelInfo {
                state,
                backend: ev.payload["backend"].as_str().map(str::to_owned),
                generation,
                last_seq: ev.seq.get(),
            },
        );
    }

    fn open_channels(&self) -> Vec<String> {
        self.channels
            .iter()
            .filter(|(_, i)| i.state == "open")
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Env of a channel for display (dry-run). M1 only has the sim backend.
    pub(crate) fn env(&self, ch: &str) -> Env {
        match self.channels.get(ch).and_then(|i| i.backend.as_deref()) {
            Some("sim") | None => Env::Sim,
            Some(_) => Env::Bench,
        }
    }
}

pub(crate) type Shared = Arc<Mutex<ChannelTable>>;

/// `channel` param, or the only open channel. Several open → `E_AMBIGUOUS_CHANNEL`;
/// none known → `ch0` (plan default channel; the worker reports `E_NOT_OPEN` if it is not open).
pub(crate) fn resolve_channel(table: &Shared, given: Option<&str>) -> Result<String, DomainError> {
    if let Some(c) = given.filter(|c| !c.is_empty()) {
        return Ok(c.to_owned());
    }
    let open = lock(table).open_channels();
    match open.as_slice() {
        [one] => Ok(one.clone()),
        [] => Ok("ch0".into()),
        many => Err(DomainError::new(
            ErrorCode::AmbiguousChannel,
            format!(
                "several CAN channels are open ({}); pass --channel",
                many.join(", ")
            ),
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Ids, frame matching
// ---------------------------------------------------------------------------------------------

/// An id or id range argument: number, `"0x300"`, `"768"`, `"0x7E0-0x7EF"`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum IdArg {
    Num(u32),
    Text(String),
}

fn parse_u32(s: &str) -> Result<u32, DomainError> {
    let s = s.trim();
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse::<u32>(),
    };
    r.map_err(|_| usage(format!("bad CAN id {s:?}")))
}

impl IdArg {
    fn range(&self) -> Result<(u32, u32), DomainError> {
        let (lo, hi) = match self {
            IdArg::Num(n) => (*n, *n),
            IdArg::Text(t) => match t.split_once('-') {
                Some((a, b)) => (parse_u32(a)?, parse_u32(b)?),
                None => {
                    let v = parse_u32(t)?;
                    (v, v)
                }
            },
        };
        if lo > hi || hi > loom_can_frame::MAX_EXTENDED_ID {
            return Err(usage(format!("bad CAN id range {lo:#X}-{hi:#X}")));
        }
        Ok((lo, hi))
    }

    fn single(&self) -> Result<u32, DomainError> {
        match self.range()? {
            (lo, hi) if lo == hi => Ok(lo),
            _ => Err(usage("give one CAN id, not a range")),
        }
    }
}

/// Frame payload of a `can.frame` record, or the summary frame of a `can.tx` row.
pub(crate) fn frame_payload(rec: &PublishedRecord) -> Option<FramePayload> {
    let v = match rec.kind.as_str() {
        KIND_FRAME => &rec.payload,
        KIND_TX => &rec.payload["frame"],
        _ => return None,
    };
    serde_json::from_value(v.clone()).ok()
}

fn parse_hex(s: &str) -> Result<Vec<u8>, DomainError> {
    let clean: String = s.chars().filter(|c| !matches!(c, '.' | ' ' | ':')).collect();
    if !clean.len().is_multiple_of(2) || clean.len() / 2 > loom_can_frame::FD_MAX_LEN {
        return Err(usage(format!("bad hex {s:?}")));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| usage(format!("bad hex {s:?}"))))
        .collect()
}

/// What a frame must look like (`can.expect`, `can.frame_match`).
#[derive(Debug, Clone, PartialEq)]
struct FrameSpec {
    id: u32,
    ext: Option<bool>,
    data: Vec<u8>,
    mask: Vec<u8>,
    origin: Option<Origin>,
    dir: Option<Dir>,
}

impl FrameSpec {
    fn new(
        id: u32,
        ext: Option<bool>,
        data: Option<&str>,
        mask: Option<&str>,
        origin: Option<Origin>,
        dir: Option<Dir>,
    ) -> Result<Self, DomainError> {
        let data = data.map(parse_hex).transpose()?.unwrap_or_default();
        let mask = match mask {
            Some(m) => parse_hex(m)?,
            None => vec![0xFF; data.len()],
        };
        if mask.len() != data.len() {
            return Err(usage("mask must have the same length as data"));
        }
        Ok(FrameSpec {
            id,
            ext,
            data,
            mask,
            origin,
            dir,
        })
    }

    fn matches(&self, rec: &PublishedRecord) -> bool {
        if rec.kind != KIND_FRAME
            || self.origin.is_some_and(|o| o != rec.origin)
            || self.dir.is_some_and(|d| rec.dir != Some(d))
        {
            return false;
        }
        let Some(f) = frame_payload(rec) else { return false };
        if f.id != self.id || self.ext.is_some_and(|e| e != f.ext) || f.rtr && !self.data.is_empty() {
            return false;
        }
        let Ok(bytes) = f.bytes.decode() else { return false };
        bytes.len() >= self.data.len()
            && self
                .data
                .iter()
                .zip(&self.mask)
                .zip(&bytes)
                .all(|((d, m), b)| d & m == b & m)
    }

    fn expected(&self) -> Value {
        json!({
            "id": self.id,
            "ext": self.ext,
            "data": hex(&self.data),
            "mask": hex(&self.mask),
            "origin": self.origin,
            "dir": self.dir,
        })
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn observed(rec: &PublishedRecord) -> Value {
    let f = frame_payload(rec);
    json!({
        "id": f.as_ref().map(|f| f.id),
        "ext": f.as_ref().map(|f| f.ext),
        "data": f.as_ref().map(|f| f.bytes.data.clone()),
        "origin": rec.origin,
        "dir": rec.dir,
        "mono_ns": rec.ts.mono_ns,
    })
}

fn ref_string(channel: &str, epoch: u64, from: u64, to: u64) -> String {
    RefString {
        domain: DOMAIN.into(),
        channel: channel.into(),
        epoch: Some(epoch),
        from,
        to,
        pointer: None,
    }
    .to_string()
}

fn match_evidence(channel: &str, rec: &PublishedRecord, spec: &FrameSpec) -> (Evidence, String) {
    let ev = Evidence {
        r#ref: EvidenceRef {
            stream_epoch: rec.epoch,
            seq_range: SeqRange {
                from: rec.seq,
                to: rec.seq,
            },
            source_refs: vec![rec.source_ref()],
        },
        kind: KIND_FRAME.into(),
        pointer: Some("/payload/bytes/data".into()),
        predicate_id: None,
        observed: Some(observed(rec)),
        expected: Some(spec.expected()),
        request_id: rec.request_id.clone(),
        complete: true,
    };
    let r = ref_string(channel, rec.epoch.get(), rec.seq.get(), rec.seq.get());
    (ev, r)
}

// ---------------------------------------------------------------------------------------------
// Activity (I4): the user sees what the agent reads and waits for.
// ---------------------------------------------------------------------------------------------

pub(crate) fn publish_activity(
    ctx: &CallCtx,
    method: &str,
    channel: &str,
    summary: String,
    extra: Map<String, Value>,
) {
    if ctx.role() == Role::Viewer {
        return; // viewer reads are not agent activity
    }
    let mut payload = Map::new();
    payload.insert("client_id".into(), json!(ctx.client_id()));
    payload.insert("method".into(), json!(method));
    payload.insert("summary".into(), json!(summary));
    payload.insert("domain".into(), json!(DOMAIN));
    payload.insert("channel".into(), json!(channel));
    payload.extend(extra);
    let mut draft = EventDraft::new(kinds::CORE_ACTIVITY, Value::Object(payload));
    draft.channel = Some(channel.into());
    // Best effort: activity must never fail the read itself.
    let _ = ctx.publish(draft);
}

// ---------------------------------------------------------------------------------------------
// Sleeping without a runtime dependency (M1). One shared timer thread, bounded.
// ---------------------------------------------------------------------------------------------

pub(crate) mod timer {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    /// Pending timers kept at once; above this a sleep yields instead of waiting (C2).
    pub const MAX_PENDING: usize = 4_096;

    #[derive(Default)]
    struct State {
        heap: BinaryHeap<Reverse<(Instant, u64)>>,
        wakers: HashMap<u64, Waker>,
        next_id: u64,
    }

    struct Shared {
        state: Mutex<State>,
        cv: Condvar,
    }

    fn shared() -> &'static Arc<Shared> {
        static S: OnceLock<Arc<Shared>> = OnceLock::new();
        S.get_or_init(|| {
            let s = Arc::new(Shared {
                state: Mutex::new(State::default()),
                cv: Condvar::new(),
            });
            let t = s.clone();
            // Detached; lives for the process. Wakes futures whose deadline passed.
            let _ = std::thread::Builder::new()
                .name("loom-can-timer".into())
                .spawn(move || run(&t));
            s
        })
    }

    fn run(s: &Shared) {
        let mut st = s.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let now = Instant::now();
            while let Some(Reverse((at, id))) = st.heap.peek().copied() {
                if at > now {
                    break;
                }
                st.heap.pop();
                if let Some(w) = st.wakers.remove(&id) {
                    w.wake();
                }
            }
            st = match st.heap.peek() {
                Some(Reverse((at, _))) => {
                    let d = at.saturating_duration_since(Instant::now());
                    s.cv.wait_timeout(st, d)
                        .map(|(g, _)| g)
                        .unwrap_or_else(|e| e.into_inner().0)
                }
                None => s.cv.wait(st).unwrap_or_else(|e| e.into_inner()),
            };
        }
    }

    /// Future that completes at `until`.
    pub struct Sleep {
        until: Instant,
        id: Option<u64>,
    }

    pub fn sleep(d: Duration) -> Sleep {
        Sleep {
            until: Instant::now() + d,
            id: None,
        }
    }

    impl Future for Sleep {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if Instant::now() >= self.until {
                return Poll::Ready(());
            }
            let s = shared();
            let mut st = s.state.lock().unwrap_or_else(|e| e.into_inner());
            match self.id {
                Some(id) => {
                    st.wakers.insert(id, cx.waker().clone());
                }
                None if st.wakers.len() >= MAX_PENDING => {
                    // Too many sleepers: yield and retry rather than grow without bound.
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                None => {
                    st.next_id += 1;
                    let id = st.next_id;
                    st.heap.push(std::cmp::Reverse((self.until, id)));
                    st.wakers.insert(id, cx.waker().clone());
                    self.id = Some(id);
                    s.cv.notify_one();
                }
            }
            Poll::Pending
        }
    }

    impl Drop for Sleep {
        fn drop(&mut self) {
            if let Some(id) = self.id {
                let s = shared();
                s.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .wakers
                    .remove(&id);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Journal helpers
// ---------------------------------------------------------------------------------------------

pub(crate) fn frame_filter(channel: &str, kinds: &[&str], origins: Vec<Origin>) -> SubFilter {
    SubFilter {
        domains: vec![DOMAIN.into()],
        channels: vec![channel.into()],
        kinds: kinds.iter().map(|k| (*k).to_owned()).collect(),
        origins,
        ext: None,
    }
}

pub(crate) fn seq_cursor(rec: &PublishedRecord, plus: u64) -> Cursor {
    Cursor::Seq {
        epoch: Some(rec.epoch.get()),
        seq: rec.seq.get().saturating_add(plus),
    }
}

/// Where the next scan of a continuing read starts.
pub(crate) fn next_from(scan: &ScanResult) -> Option<Cursor> {
    scan.resume_from
        .clone()
        .or_else(|| scan.events.last().map(|e| seq_cursor(e, 1)))
}

/// `now`/relative cursors are re-evaluated by the journal on every scan, so a wait loop keeps
/// them anchored at the call start: offset + elapsed (rounded up to whole ms).
#[derive(Debug, Clone)]
pub(crate) enum Anchor {
    Fixed(Cursor),
    Before { offset: Duration, start: Instant },
}

impl Anchor {
    pub(crate) fn new(c: Cursor, start: Instant) -> Self {
        match &c {
            Cursor::Now => Anchor::Before {
                offset: Duration::ZERO,
                start,
            },
            Cursor::Relative { .. } => Anchor::Before {
                offset: c.relative_duration().unwrap_or_default(),
                start,
            },
            _ => Anchor::Fixed(c),
        }
    }

    pub(crate) fn cursor(&self) -> Cursor {
        match self {
            Anchor::Fixed(c) => c.clone(),
            Anchor::Before { offset, start } => {
                let ms = (offset.as_nanos() + start.elapsed().as_nanos()).div_ceil(1_000_000);
                Cursor::Relative {
                    amount: u64::try_from(ms).unwrap_or(u64::MAX).max(1),
                    unit: TimeUnit::Millis,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// can.expect (also used by can.send --expect)
// ---------------------------------------------------------------------------------------------

enum WaitOutcome {
    Matched {
        evidence: Evidence,
        r#ref: String,
        rec: Box<PublishedRecord>,
    },
    TimedOut {
        evidence: Vec<Evidence>,
        refs: Vec<String>,
        frames_scanned: u64,
    },
}

async fn wait_for(
    ctx: &CallCtx,
    channel: &str,
    spec: &FrameSpec,
    after: Cursor,
    within: Duration,
) -> Result<WaitOutcome, DomainError> {
    let start = Instant::now();
    let deadline = (start + within).min(ctx.deadline());
    let mut from = Anchor::new(after, start);
    let mut first: Option<PublishedRecord> = None;
    let mut last: Option<PublishedRecord> = None;
    let mut scanned: u64 = 0;
    let mut complete = true;
    loop {
        // Scan channel frames (any origin) so the timeout window covers what was on the bus.
        let q = JournalQuery {
            from: from.cursor(),
            until: None,
            filter: frame_filter(channel, &[KIND_FRAME], Vec::new()),
            request_id: None,
            max_events: MAX_ROWS,
        };
        let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
        complete &= scan.gaps.is_empty();
        for rec in &scan.events {
            scanned += 1;
            if first.is_none() {
                first = Some(rec.clone());
            }
            if spec.matches(rec) {
                let (evidence, r) = match_evidence(channel, rec, spec);
                return Ok(WaitOutcome::Matched {
                    evidence,
                    r#ref: r,
                    rec: Box::new(rec.clone()),
                });
            }
        }
        if let Some(l) = scan.events.last() {
            last = Some(l.clone());
        }
        if let Some(n) = next_from(&scan) {
            from = Anchor::Fixed(n);
        }
        if Instant::now() >= deadline {
            break;
        }
        // More already waiting (limit hit): scan again at once.
        if scan.resume_from.is_none() {
            timer::sleep(EXPECT_POLL.min(deadline.saturating_duration_since(Instant::now()))).await;
        }
    }
    // Timeout. Cite the scanned window only when we saw frames: never invent stream seqs.
    let (evidence, refs) = match (&first, &last) {
        (Some(f), Some(l)) => {
            let ev = Evidence {
                r#ref: EvidenceRef {
                    stream_epoch: l.epoch,
                    seq_range: SeqRange {
                        from: f.seq,
                        to: l.seq,
                    },
                    source_refs: Vec::new(),
                },
                kind: "can.wait_timeout".into(),
                pointer: None,
                predicate_id: None,
                observed: Some(json!({"frames_scanned": scanned})),
                expected: Some(spec.expected()),
                request_id: None,
                complete,
            };
            let r = ref_string(channel, l.epoch.get(), f.seq.get(), l.seq.get());
            (vec![ev], vec![r])
        }
        _ => (Vec::new(), Vec::new()),
    };
    Ok(WaitOutcome::TimedOut {
        evidence,
        refs,
        frames_scanned: scanned,
    })
}

fn waiting_summary(channel: &str, spec: &FrameSpec, within_ms: u32) -> String {
    let data = if spec.data.is_empty() {
        String::new()
    } else {
        format!(" data {}", hex(&spec.data))
    };
    format!(
        "waiting for {} {:#X}{data} on {channel} (≤ {within_ms} ms)",
        spec.origin
            .map_or("any".to_owned(), |o| format!("{o:?}").to_lowercase()),
        spec.id
    )
}

fn waiting_extra(spec: &FrameSpec, within_ms: u32) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("action".into(), json!("wait"));
    m.insert(
        "waiting".into(),
        json!({"predicate": "can.frame_match", "args": spec.expected(), "within_ms": within_ms}),
    );
    m
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectParams {
    #[serde(default)]
    channel: Option<String>,
    id: IdArg,
    #[serde(default)]
    ext: Option<bool>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    mask: Option<String>,
    /// Default `device`: a response is a device frame (contract 07 §7.3).
    #[serde(default)]
    origin: Option<Origin>,
    #[serde(default)]
    dir: Option<Dir>,
    /// Default `now`.
    #[serde(default)]
    after: Option<Cursor>,
    within_ms: u32,
}

#[derive(Debug)]
struct ExpectH {
    table: Shared,
}

impl Handler for ExpectH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: ExpectParams = parse(params)?;
            if p.within_ms == 0 || p.within_ms > MAX_EXPECT_MS {
                return Err(usage(format!("within_ms must be 1..={MAX_EXPECT_MS}")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let spec = FrameSpec::new(
                p.id.single()?,
                p.ext,
                p.data.as_deref(),
                p.mask.as_deref(),
                Some(p.origin.unwrap_or(Origin::Device)),
                p.dir,
            )?;
            publish_activity(
                ctx,
                RPC_EXPECT,
                &channel,
                waiting_summary(&channel, &spec, p.within_ms),
                waiting_extra(&spec, p.within_ms),
            );
            let within = Duration::from_millis(u64::from(p.within_ms));
            match wait_for(ctx, &channel, &spec, p.after.unwrap_or(Cursor::Now), within).await? {
                WaitOutcome::Matched { evidence, r#ref, rec } => Ok(json!({
                    "channel": channel,
                    "matched": true,
                    "frame": *rec,
                    "evidence": [evidence],
                    "refs": [r#ref],
                })),
                WaitOutcome::TimedOut {
                    evidence,
                    refs,
                    frames_scanned,
                } => Err(DomainError::new(
                    ErrorCode::Timeout,
                    format!(
                        "no matching frame {:#X} on {channel} within {} ms",
                        spec.id, p.within_ms
                    ),
                )
                .with_detail(json!({
                    "channel": channel,
                    "within_ms": p.within_ms,
                    "frames_scanned": frames_scanned,
                    "evidence": evidence,
                    "refs": refs,
                }))),
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.send
// ---------------------------------------------------------------------------------------------

/// Risk shown for a send (F1: display only, everything runs automatically).
pub(crate) fn classify_send(count: u64, env: Env) -> (Risk, String) {
    if count > BURST_R3_ABOVE {
        (Risk::R3, format!("finite burst > {BURST_R3_ABOVE} frames"))
    } else if env == Env::Sim {
        (Risk::R1, "sim channel".into())
    } else {
        // bench and vehicle are the same class (B4).
        (Risk::R2, "frame on a hardware channel".into())
    }
}

#[derive(Debug)]
struct SendH {
    table: Shared,
}

impl SendH {
    /// Stream seq right after this request's tx echo, so `--expect` cannot match a frame from
    /// before the send. Falls back to "since the send started".
    async fn after_echo(ctx: &CallCtx, channel: &str, rid: &str, start: Instant) -> Cursor {
        let since = Anchor::Before {
            offset: Duration::from_millis(1),
            start,
        };
        let q = JournalQuery {
            from: since.cursor(),
            until: None,
            filter: frame_filter(channel, &[KIND_FRAME], Vec::new()),
            request_id: Some(rid.to_owned()),
            max_events: u32::from(crate::schema::MAX_SEND_COUNT as u16),
        };
        match ctx.journal().scan(&q, SCAN_BYTES).await {
            Ok(s) => match s.events.last() {
                Some(echo) => seq_cursor(echo, 1),
                None => since.cursor(),
            },
            Err(_) => since.cursor(),
        }
    }

    fn publish_row(
        ctx: &CallCtx,
        channel: &str,
        rid: &str,
        p: &SendParams,
        frame: FramePayload,
        state: WriteState,
        risk: Risk,
    ) {
        let row = TxPayload {
            write_state: state,
            frame,
            count: p.count,
            sent: p.count,
            error: None,
        };
        let Ok(payload) = serde_json::to_value(&row) else {
            return;
        };
        let mut d = EventDraft::new(KIND_TX, payload);
        d.channel = Some(channel.into());
        d.dir = Some(Dir::Tx);
        d.request_id = Some(rid.into());
        d.risk = Some(risk);
        d.env = Some(Env::Sim); // M1: the only backend is sim
        d.purpose = Some("agent_send".into());
        let _ = ctx.publish(d);
    }
}

impl Handler for SendH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let mut params = match params {
                Value::Object(m) => m,
                _ => return Err(usage("params must be an object")),
            };
            let channel = resolve_channel(&self.table, params.get("channel").and_then(Value::as_str))?;
            params.insert("channel".into(), json!(channel));
            let p: SendParams = parse(Value::Object(params))?;
            let frame = p.resolve_frame().map_err(|e| usage(e.to_string()))?;
            let env = lock(&self.table).env(&channel);
            let (risk, reason) = classify_send(u64::from(p.count), env);

            if p.dry_run {
                // R0 preview: nothing reaches the worker.
                return Ok(json!({
                    "dry_run": true,
                    "channel": channel,
                    "frame": frame.to_string(),
                    "bytes": frame.data_hex().to_lowercase(),
                    "count": p.count,
                    "risk": risk,
                    "reason": reason,
                    "policy": "auto",
                }));
            }

            let rid = ctx.request_id().unwrap_or("can-send").to_owned();
            let start = Instant::now();
            let op = serde_json::to_value(&p).map_err(|e| usage(e.to_string()))?;
            let out = ctx
                .worker(WORKER_KIND_SIM)?
                .write(&rid, op, ctx.deadline())
                .await?;
            let mut out = match out {
                Value::Object(m) => m,
                other => {
                    let mut m = Map::new();
                    m.insert("worker".into(), other);
                    m
                }
            };
            out.insert("request_id".into(), json!(rid));
            out.insert("channel".into(), json!(channel));
            out.insert("frame".into(), json!(frame.to_string()));
            out.insert("risk".into(), json!(risk));
            out.insert("reason".into(), json!(reason));

            if let Some(SendExpect { id, ext, within_ms }) = p.expect.clone() {
                let spec = FrameSpec::new(id, Some(ext), None, None, Some(Origin::Device), None)?;
                publish_activity(
                    ctx,
                    RPC_SEND,
                    &channel,
                    waiting_summary(&channel, &spec, within_ms),
                    waiting_extra(&spec, within_ms),
                );
                let after = Self::after_echo(ctx, &channel, &rid, start).await;
                let within = Duration::from_millis(u64::from(within_ms.max(1)));
                let (state, expect) = match wait_for(ctx, &channel, &spec, after, within).await? {
                    WaitOutcome::Matched { evidence, r#ref, rec } => (
                        WriteState::EffectiveVerified,
                        json!({"matched": true, "frame": *rec, "evidence": [evidence], "refs": [r#ref]}),
                    ),
                    WaitOutcome::TimedOut {
                        evidence,
                        refs,
                        frames_scanned,
                    } => (
                        WriteState::EffectUnverified,
                        json!({"matched": false, "within_ms": within_ms, "frames_scanned": frames_scanned,
                               "evidence": evidence, "refs": refs}),
                    ),
                };
                Self::publish_row(
                    ctx,
                    &channel,
                    &rid,
                    &p,
                    FramePayload::from_frame(&frame, None),
                    state,
                    risk,
                );
                out.insert("write_state".into(), json!(state));
                out.insert("expect".into(), expect);
            }
            Ok(Value::Object(out))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.trace.query
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceParams {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    since: Option<Cursor>,
    #[serde(default)]
    until: Option<Cursor>,
    #[serde(default)]
    ids: Vec<IdArg>,
    #[serde(default)]
    origins: Vec<Origin>,
    #[serde(default)]
    dir: Option<Dir>,
    #[serde(default)]
    max: Option<u32>,
    /// Accepted and echoed; advancing `@last` is the core's job (no cursor API on `CallCtx` yet).
    #[serde(default)]
    peek: bool,
    /// Add `decoded` (DBC signals) to frame rows of the reply. Stored events are unchanged.
    #[serde(default)]
    decode: bool,
}

/// Reply row: the record, plus `decoded` when a DBC is given and knows the frame.
fn reply_row(rec: &PublishedRecord, db: Option<&LoadedDb>) -> Value {
    let mut row = serde_json::to_value(rec).unwrap_or(Value::Null);
    if let Some(d) = db.and_then(|d| decode_record(&d.db, rec)) {
        row["decoded"] = d;
    }
    row
}

fn row_matches(rec: &PublishedRecord, ids: &[(u32, u32)], dir: Option<Dir>) -> bool {
    if let Some(d) = dir {
        // `can.tx` rows describe sends; they belong to the tx side.
        let rd = if rec.kind == KIND_TX {
            Some(Dir::Tx)
        } else {
            rec.dir
        };
        if rd != Some(d) {
            return false;
        }
    }
    if ids.is_empty() {
        return true;
    }
    frame_payload(rec).is_some_and(|f| ids.iter().any(|(lo, hi)| (*lo..=*hi).contains(&f.id)))
}

#[derive(Debug)]
struct TraceH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for TraceH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: TraceParams = parse(params)?;
            let max = p.max.unwrap_or(DEFAULT_ROWS);
            if max == 0 || max > MAX_ROWS {
                return Err(usage(format!("max must be 1..={MAX_ROWS}")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let ids = p.ids.iter().map(IdArg::range).collect::<Result<Vec<_>, _>>()?;
            let db = p.decode.then(|| decode_db(&self.dbs, &channel)).transpose()?;
            let mut from = p.since.clone().unwrap_or(Cursor::Last);
            let mut rows: Vec<PublishedRecord> = Vec::new();
            let mut out: Vec<Value> = Vec::new();
            let mut bytes = 0usize;
            let mut truncated = false;
            let mut complete = true;
            let mut gaps = Vec::new();
            let mut next: Option<Cursor> = None;
            'scans: for i in 0..MAX_SCANS {
                let q = JournalQuery {
                    from: from.clone(),
                    until: p.until.clone(),
                    filter: frame_filter(&channel, &[KIND_FRAME, KIND_TX], p.origins.clone()),
                    request_id: None,
                    max_events: MAX_ROWS,
                };
                let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
                complete &= scan.complete && scan.gaps.is_empty();
                gaps.extend(scan.gaps.iter().cloned());
                for rec in &scan.events {
                    if !row_matches(rec, &ids, p.dir) {
                        continue;
                    }
                    if rows.len() as u32 == max {
                        truncated = true;
                        next = Some(seq_cursor(rec, 0));
                        break 'scans;
                    }
                    // Decoded only for rows that are returned (bounded work).
                    let row = reply_row(rec, db.as_deref());
                    let size = serde_json::to_vec(&row).map_or(0, |v| v.len());
                    if bytes + size > MAX_RESULT_BYTES {
                        truncated = true;
                        next = Some(seq_cursor(rec, 0));
                        break 'scans;
                    }
                    bytes += size;
                    rows.push(rec.clone());
                    out.push(row);
                }
                let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
                match next_from(&scan) {
                    Some(n) if more => {
                        if i + 1 == MAX_SCANS {
                            // Scan budget used up: say so instead of pretending the range is done.
                            truncated = true;
                            next = Some(n);
                            break 'scans;
                        }
                        from = n;
                    }
                    n => {
                        next = n.or(next);
                        break 'scans;
                    }
                }
            }
            let range = match (rows.first(), rows.last()) {
                (Some(a), Some(b)) => Some(SeqRange {
                    from: a.seq,
                    to: b.seq,
                }),
                _ => None,
            };
            let next = next.or_else(|| rows.last().map(|r| seq_cursor(r, 1)));
            let mut extra = Map::new();
            extra.insert("action".into(), json!("read"));
            if let Some(r) = range {
                extra.insert("range".into(), json!(r));
            }
            publish_activity(
                ctx,
                RPC_TRACE_QUERY,
                &channel,
                format!("read {channel} trace ({} rows)", rows.len()),
                extra,
            );
            Ok(json!({
                "channel": channel,
                "rows": out,
                "next_cursor": next.map(|c| c.to_string()),
                "truncated": truncated,
                "range": range,
                "complete": complete,
                "gaps": gaps,
                "peek": p.peek,
                "db_sha256": db.map(|d| d.sha.clone()),
            }))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.last (Fixed view on demand)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LastParams {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    id: Option<IdArg>,
    #[serde(default)]
    window_ms: Option<u32>,
    /// Add `decoded` to each row's last frame.
    #[serde(default)]
    decode: bool,
}

#[derive(Debug)]
struct LastH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for LastH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: LastParams = parse(params)?;
            let window = p.window_ms.unwrap_or(LAST_DEFAULT_WINDOW_MS);
            if window == 0 || window > LAST_MAX_WINDOW_MS {
                return Err(usage(format!("window_ms must be 1..={LAST_MAX_WINDOW_MS}")));
            }
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let only = p.id.as_ref().map(IdArg::range).transpose()?;
            let db = p.decode.then(|| decode_db(&self.dbs, &channel)).transpose()?;
            // The journal scans forward, so read a bounded recent window instead of backwards.
            let mut from = Cursor::Relative {
                amount: u64::from(window),
                unit: TimeUnit::Millis,
            };
            let mut last: BTreeMap<(u32, bool), (PublishedRecord, u64)> = BTreeMap::new();
            let mut evicted = 0u64;
            let mut complete = true;
            for i in 0..MAX_SCANS {
                let q = JournalQuery {
                    from: from.clone(),
                    until: None,
                    filter: frame_filter(&channel, &[KIND_FRAME], Vec::new()),
                    request_id: None,
                    max_events: MAX_ROWS,
                };
                let scan = ctx.journal().scan(&q, SCAN_BYTES).await?;
                complete &= scan.complete && scan.gaps.is_empty();
                for rec in &scan.events {
                    let Some(f) = frame_payload(rec) else { continue };
                    if only.is_some_and(|(lo, hi)| !(lo..=hi).contains(&f.id)) {
                        continue;
                    }
                    let key = (f.id, f.ext);
                    let full = last.len() >= FIXED_MAX_IDS;
                    match last.get_mut(&key) {
                        Some(e) => {
                            e.0 = rec.clone();
                            e.1 += 1;
                        }
                        None if full => evicted += 1,
                        None => {
                            last.insert(key, (rec.clone(), 1));
                        }
                    }
                }
                let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
                match next_from(&scan) {
                    Some(n) if more => {
                        if i + 1 == MAX_SCANS {
                            complete = false;
                        }
                        from = n;
                    }
                    _ => break,
                }
            }
            let mut rows = Vec::new();
            let mut bytes = 0usize;
            let mut truncated = false;
            for ((id, ext), (rec, count)) in &last {
                let row =
                    json!({"id": id, "ext": ext, "count": count, "last": reply_row(rec, db.as_deref())});
                let size = row.to_string().len();
                if rows.len() as u32 == MAX_ROWS || bytes + size > MAX_RESULT_BYTES {
                    truncated = true;
                    break;
                }
                bytes += size;
                rows.push(row);
            }
            Ok(json!({
                "channel": channel,
                "window_ms": window,
                "rows": rows,
                "truncated": truncated,
                "complete": complete,
                "ids_not_tracked": evicted,
                "db_sha256": db.map(|d| d.sha.clone()),
            }))
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.status
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct StatusH;

impl Handler for StatusH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            ctx.worker(WORKER_KIND_SIM)?
                .call(RPC_STATUS, params, ctx.deadline())
                .await
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.isotp.* (K10 minimal): the worker owns the engines; the gateway checks and forwards.
// ---------------------------------------------------------------------------------------------

/// Forwards a worker call unchanged (`can.isotp.close|abort|list`).
#[derive(Debug)]
struct WorkerCallH {
    method: &'static str,
}

impl Handler for WorkerCallH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            ctx.worker(WORKER_KIND_SIM)?
                .call(self.method, params, ctx.deadline())
                .await
        })
    }
}

/// `can.isotp.open`: fills the channel (only open channel / `ch0`), then the worker validates.
#[derive(Debug)]
struct IsoTpOpenH {
    table: Shared,
}

impl Handler for IsoTpOpenH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let Value::Object(mut m) = params else {
                return Err(usage("params must be an object"));
            };
            let channel = resolve_channel(&self.table, m.get("channel").and_then(Value::as_str))?;
            m.insert("channel".into(), json!(channel));
            // Fail fast on bad parameters (the worker checks again).
            let p: crate::isotp::OpenParams = parse(Value::Object(m.clone()))?;
            p.config().map_err(|(_, msg)| usage(msg))?;
            ctx.worker(WORKER_KIND_SIM)?
                .call(RPC_ISOTP_OPEN, Value::Object(m), ctx.deadline())
                .await
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IsoTpSendParams {
    endpoint: String,
    pdu: String,
}

/// `can.isotp.send` (write `Request`): one PDU → `{accepted:true, transfer}`. The UDS request row
/// owns `write_state`; transport progress arrives as `can.isotp_tx_done{tx_state}`.
#[derive(Debug)]
struct IsoTpSendH;

impl Handler for IsoTpSendH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: IsoTpSendParams = parse(params)?;
            let pdu = crate::isotp::parse_pdu(&p.pdu).map_err(usage)?;
            let rid = ctx.request_id().unwrap_or("can-isotp-send").to_owned();
            let op = json!({"method": RPC_ISOTP_SEND, "endpoint": p.endpoint, "pdu": hex(&pdu)});
            let mut out = ctx
                .worker(WORKER_KIND_SIM)?
                .write(&rid, op, ctx.deadline())
                .await?;
            if let Value::Object(m) = &mut out {
                m.insert("request_id".into(), json!(rid));
            }
            Ok(out)
        })
    }
}

// ---------------------------------------------------------------------------------------------
// can.periodic.* (K9 wiring, A1 job writes): the worker's scheduler owns the jobs.
// ---------------------------------------------------------------------------------------------

/// Longest period (ms).
pub const PERIODIC_MAX_PERIOD_MS: u32 = 60_000;
/// Longest job lifetime / lease (ms): 24 h.
pub const PERIODIC_MAX_LIFE_MS: u64 = 86_400_000;

/// Risk shown for periodic writes (F1: display only). Stopping only removes traffic.
pub(crate) fn classify_periodic(method: &str, env: Env) -> (Risk, String) {
    if method == RPC_PERIODIC_STOP {
        (Risk::R0, "stops a periodic job".into())
    } else if env == Env::Sim {
        (Risk::R1, "sim channel".into())
    } else {
        (Risk::R3, "periodic".into())
    }
}

fn is_periodic_write(method: &str) -> bool {
    matches!(
        method,
        RPC_PERIODIC_START
            | RPC_PERIODIC_UPDATE
            | RPC_PERIODIC_STOP
            | RPC_PERIODIC_RENEW
            | RPC_PERIODIC_TAKEOVER
    ) || method == RPC_SIG_SET
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeriodicStartParams {
    #[serde(default)]
    channel: Option<String>,
    /// Raw form: cansend text (`300#01`) or a `can.frame` payload object.
    #[serde(default)]
    frame: Option<Value>,
    #[serde(default)]
    ext: bool,
    /// DBC form: message name, signals on top of the DBC start values.
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    signals: Map<String, Value>,
    #[serde(default)]
    force_range: bool,
    /// Default for `msg`: the DBC `GenMsgCycleTime`.
    #[serde(default)]
    period_ms: Option<u32>,
    #[serde(default)]
    offset_ms: Option<u32>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    lease_ms: Option<u64>,
    #[serde(default)]
    max_duration_ms: Option<u64>,
    #[serde(default)]
    owner: Option<String>,
}

#[derive(Debug)]
struct PeriodicStartH {
    table: Shared,
    dbs: Dbs,
}

impl Handler for PeriodicStartH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let p: PeriodicStartParams = parse(params)?;
            let channel = resolve_channel(&self.table, p.channel.as_deref())?;
            let (frame, cycle, db_sha) = match (&p.frame, &p.msg) {
                (Some(_), Some(_)) | (None, None) => return Err(usage("give either `frame` or `msg`")),
                (Some(f), None) => {
                    if !p.signals.is_empty() {
                        return Err(usage("`signals` needs `msg`"));
                    }
                    let frame = match f {
                        Value::String(t) => {
                            loom_can_frame::parse_frame(t, p.ext).map_err(|e| usage(format!("frame: {e}")))?
                        }
                        obj => parse::<FramePayload>(obj.clone())?
                            .to_frame()
                            .map_err(|e| usage(format!("frame: {e}")))?,
                    };
                    (frame, None, None)
                }
                (None, Some(m)) => {
                    let (f, c, sha) = start_frame(&self.dbs, &channel, m, &p.signals, p.force_range)?;
                    (f, c, Some(sha))
                }
            };
            if frame.is_remote() {
                return Err(usage("remote frames cannot be sent periodically"));
            }
            let period = p
                .period_ms
                .or(cycle)
                .ok_or_else(|| usage("period_ms is required (the DBC message has no GenMsgCycleTime)"))?;
            if period == 0 || period > PERIODIC_MAX_PERIOD_MS {
                return Err(usage(format!("period_ms must be 1..={PERIODIC_MAX_PERIOD_MS}")));
            }
            if p.offset_ms.is_some_and(|o| o > PERIODIC_MAX_PERIOD_MS) {
                return Err(usage(format!("offset_ms must be ≤ {PERIODIC_MAX_PERIOD_MS}")));
            }
            if p.count == Some(0) {
                return Err(usage("count must be ≥ 1 (omit it to run until stopped)"));
            }
            for (k, v) in [("lease_ms", p.lease_ms), ("max_duration_ms", p.max_duration_ms)] {
                if v.is_some_and(|v| v == 0 || v > PERIODIC_MAX_LIFE_MS) {
                    return Err(usage(format!("{k} must be 1..={PERIODIC_MAX_LIFE_MS}")));
                }
            }
            if p.owner.as_ref().is_some_and(|o| o.len() > 128) {
                return Err(usage("owner is at most 128 bytes"));
            }
            let mut op = json!({
                "method": RPC_PERIODIC_START,
                "channel": channel,
                "frame": crate::signals::frame_payload_of(&frame),
                "period_ms": period,
            });
            for (k, v) in [
                ("offset_ms", p.offset_ms.map(Value::from)),
                ("count", p.count.map(Value::from)),
                ("lease_ms", p.lease_ms.map(Value::from)),
                ("max_duration_ms", p.max_duration_ms.map(Value::from)),
                ("owner", p.owner.clone().map(Value::from)),
            ] {
                if let Some(v) = v {
                    op[k] = v;
                }
            }
            let rid = ctx.request_id().unwrap_or("can-periodic-start").to_owned();
            let mut out = ctx
                .worker(WORKER_KIND_SIM)?
                .write(&rid, op, ctx.deadline())
                .await?;
            let env = lock(&self.table).env(&channel);
            let (risk, reason) = classify_periodic(RPC_PERIODIC_START, env);
            if let Value::Object(m) = &mut out {
                m.insert("request_id".into(), json!(rid));
                m.insert("channel".into(), json!(channel));
                m.insert("frame".into(), json!(frame.to_string()));
                m.insert("period_ms".into(), json!(period));
                m.insert(
                    "period_from_dbc".into(),
                    json!(p.period_ms.is_none() && cycle.is_some()),
                );
                m.insert("msg".into(), json!(p.msg));
                m.insert("db_sha256".into(), json!(db_sha));
                m.insert("risk".into(), json!(risk));
                m.insert("reason".into(), json!(reason));
            }
            Ok(out)
        })
    }
}

/// `can.periodic.update|stop|renew|takeover`: checked here, then a worker job write.
#[derive(Debug)]
struct JobWriteH {
    method: &'static str,
}

fn check_job_op(method: &str, p: &Map<String, Value>) -> Result<(), DomainError> {
    let job = p.get("job").and_then(Value::as_str).filter(|j| !j.is_empty());
    let allowed: &[&str] = match method {
        RPC_PERIODIC_UPDATE => &["job", "expected_rev", "patch"],
        RPC_PERIODIC_STOP => &["job", "all_mine"],
        _ => &["job"],
    };
    if let Some(k) = p.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(usage(format!("unknown parameter {k:?}")));
    }
    if method == RPC_PERIODIC_STOP {
        let all = p.get("all_mine").and_then(Value::as_bool).unwrap_or(false);
        return match (job, all) {
            (Some(_), false) | (None, true) => Ok(()),
            _ => Err(usage("give `job` or `all_mine: true`")),
        };
    }
    if job.is_none() {
        return Err(usage("missing `job`"));
    }
    if method == RPC_PERIODIC_UPDATE {
        if p.get("expected_rev").and_then(Value::as_u64).is_none() {
            return Err(usage("missing `expected_rev` (from `can.periodic.list`)"));
        }
        let patch = p
            .get("patch")
            .and_then(Value::as_object)
            .ok_or_else(|| usage("missing `patch`"))?;
        let hexlen = |k: &str| -> Result<Option<usize>, DomainError> {
            match patch.get(k) {
                None => Ok(None),
                Some(Value::String(h)) => parse_hex(h).map(|b| Some(b.len())),
                Some(_) => Err(usage(format!("patch.{k} must be hex"))),
            }
        };
        match (hexlen("data")?, hexlen("mask")?, hexlen("bytes")?, patch.len()) {
            (Some(n), None, None, 1) if n > 0 => {}
            (None, Some(m), Some(b), 2) if m == b && m > 0 => {}
            _ => {
                return Err(usage(
                    "patch is {data: hex} or {mask: hex, bytes: hex} of equal length",
                ));
            }
        }
    }
    Ok(())
}

impl Handler for JobWriteH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let Value::Object(mut m) = params else {
                return Err(usage("params must be an object"));
            };
            check_job_op(self.method, &m)?;
            m.insert("method".into(), json!(self.method));
            let rid = ctx.request_id().unwrap_or("can-periodic").to_owned();
            let mut out = ctx
                .worker(WORKER_KIND_SIM)?
                .write(&rid, Value::Object(m), ctx.deadline())
                .await?;
            if let Value::Object(o) = &mut out {
                o.insert("request_id".into(), json!(rid));
            }
            Ok(out)
        })
    }
}

fn register_periodic(reg: &mut Registry, table: &Shared, dbs: &Dbs) -> Result<(), RegisterError> {
    let any = json_schema!({"type": "object"});
    let job_write = |method: &'static str, params: Schema, handler: Arc<dyn Handler>| CommandSpec {
        write_class: Some(WriteClass::Job),
        roles: RoleSet::WRITE,
        ..read_spec(method, params, handler, Duration::from_secs(10))
    };
    let job_only = json_schema!({
        "type": "object", "additionalProperties": false, "required": ["job"],
        "properties": {"job": {"type": "string"}}
    });
    reg.command(job_write(
        RPC_PERIODIC_START,
        json_schema!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "channel": {"type": "string"}, "frame": {"type": ["string", "object"]},
                "ext": {"type": "boolean"}, "msg": {"type": "string"},
                "signals": {"type": "object", "maxProperties": crate::signals::MAX_SET_SIGNALS},
                "force_range": {"type": "boolean"},
                "period_ms": {"type": "integer", "minimum": 1, "maximum": PERIODIC_MAX_PERIOD_MS},
                "offset_ms": {"type": "integer", "minimum": 0, "maximum": PERIODIC_MAX_PERIOD_MS},
                "count": {"type": "integer", "minimum": 1},
                "lease_ms": {"type": "integer", "minimum": 1, "maximum": PERIODIC_MAX_LIFE_MS},
                "max_duration_ms": {"type": "integer", "minimum": 1, "maximum": PERIODIC_MAX_LIFE_MS},
                "owner": {"type": "string", "maxLength": 128}
            }
        }),
        Arc::new(PeriodicStartH {
            table: table.clone(),
            dbs: dbs.clone(),
        }),
    ))?
    .command(job_write(
        RPC_PERIODIC_UPDATE,
        any.clone(),
        Arc::new(JobWriteH {
            method: RPC_PERIODIC_UPDATE,
        }),
    ))?
    .command(job_write(
        RPC_PERIODIC_STOP,
        any,
        Arc::new(JobWriteH {
            method: RPC_PERIODIC_STOP,
        }),
    ))?
    .command(job_write(
        RPC_PERIODIC_RENEW,
        job_only.clone(),
        Arc::new(JobWriteH {
            method: RPC_PERIODIC_RENEW,
        }),
    ))?
    .command(job_write(
        RPC_PERIODIC_TAKEOVER,
        job_only,
        Arc::new(JobWriteH {
            method: RPC_PERIODIC_TAKEOVER,
        }),
    ))?
    .command(read_spec(
        RPC_PERIODIC_LIST,
        json_schema!({
            "type": "object", "additionalProperties": false, "properties": {"channel": {"type": "string"}}
        }),
        Arc::new(WorkerCallH {
            method: RPC_PERIODIC_LIST,
        }),
        Duration::from_secs(5),
    ))?
    .event_kind(EventKindSpec {
        kind: KIND_JOB_STATE.into(),
        payload_schema: json_schema!({
            "type": "object", "required": ["job", "state"],
            "properties": {
                "job": {"type": "string"}, "channel": {"type": "string"}, "id": {"type": "integer"},
                "state": {"enum": ["started", "patched", "renewed", "paused", "stopped"]},
                "reason": {"type": ["string", "null"]}, "revision": {"type": "integer"},
                "owner": {"type": ["string", "null"]}, "origin": {"type": "string"}
            }
        }),
        pointers: vec![
            ptr("/payload/job", ValueType::String),
            ptr(
                "/payload/state",
                ValueType::Enum(
                    ["started", "patched", "renewed", "paused", "stopped"]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                ),
            ),
        ],
        version: 1,
    })?;
    Ok(())
}

// ---- can.isotp.cancel / recv / barrier (m2-isotp-contract "리드 연결 요청") ------------------------

/// Longest `timeout_ms` of `can.isotp.recv` / `can.isotp.barrier` (the worker's limit).
pub const ISOTP_MAX_WAIT_MS: u64 = 60_000;
/// Re-ask interval while a recv / barrier waits (the worker answers at once, never waits).
pub const ISOTP_POLL: Duration = Duration::from_millis(10);

/// Unsigned argument: integer or decimal string of at most 20 digits (worker rule).
fn u64_arg(p: &Map<String, Value>, key: &str, required: bool) -> Result<Option<u64>, DomainError> {
    match p.get(key) {
        None | Some(Value::Null) if !required => Ok(None),
        None | Some(Value::Null) => Err(usage(format!("missing `{key}`"))),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(Some)
            .ok_or_else(|| usage(format!("`{key}` must be an unsigned integer"))),
        Some(Value::String(t)) if !t.is_empty() && t.len() <= 20 && t.bytes().all(|b| b.is_ascii_digit()) => {
            t.parse()
                .map(Some)
                .map_err(|_| usage(format!("`{key}` overflows u64")))
        }
        Some(_) => Err(usage(format!(
            "`{key}` must be an unsigned integer or decimal string"
        ))),
    }
}

fn endpoint_of(p: &Map<String, Value>) -> Result<&str, DomainError> {
    p.get("endpoint")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty() && e.len() <= 64)
        .ok_or_else(|| usage("missing `endpoint`"))
}

/// `can.isotp.cancel` (worker write; R0 stop control — allowed under STOP, never latched).
#[derive(Debug)]
struct IsoTpCancelH;

impl Handler for IsoTpCancelH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let Value::Object(p) = params else {
                return Err(usage("params must be an object"));
            };
            if let Some(k) = p.keys().find(|k| !matches!(k.as_str(), "endpoint" | "transfer")) {
                return Err(usage(format!("unknown parameter {k:?}")));
            }
            let endpoint = endpoint_of(&p)?;
            let transfer = p
                .get("transfer")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty() && t.len() <= 64)
                .ok_or_else(|| usage("missing `transfer` (from the can.isotp.send result, ≤ 64 bytes)"))?;
            let rid = ctx.request_id().unwrap_or("can-isotp-cancel").to_owned();
            let op = json!({"method": RPC_ISOTP_CANCEL, "endpoint": endpoint, "transfer": transfer});
            let mut out = ctx
                .worker(WORKER_KIND_SIM)?
                .write(&rid, op, ctx.deadline())
                .await?;
            if let Value::Object(m) = &mut out {
                m.insert("request_id".into(), json!(rid));
            }
            Ok(out)
        })
    }
}

/// `can.isotp.recv` / `can.isotp.barrier`: R0 reads. The worker answers with one snapshot and never
/// waits, so the gateway re-asks every [`ISOTP_POLL`] until there is a result (recv: a PDU; barrier:
/// `complete`) or `timeout_ms` / the call deadline passes. A timed-out barrier returns its last
/// `complete:false` unchanged (UDS reports "uncertain", never a P2 timeout from it).
#[derive(Debug)]
struct IsoTpPollH {
    method: &'static str,
}

impl IsoTpPollH {
    fn check(&self, p: &Map<String, Value>) -> Result<u64, DomainError> {
        let allowed: &[&str] = if self.method == RPC_ISOTP_RECV {
            &["endpoint", "after_pdu_seq", "timeout_ms", "max_pdus"]
        } else {
            &["endpoint", "epoch", "deadline_mono_ns", "timeout_ms"]
        };
        if let Some(k) = p.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(usage(format!("unknown parameter {k:?}")));
        }
        endpoint_of(p)?;
        let timeout = u64_arg(p, "timeout_ms", false)?.unwrap_or(0);
        if timeout > ISOTP_MAX_WAIT_MS {
            return Err(usage(format!("timeout_ms must be 0..={ISOTP_MAX_WAIT_MS}")));
        }
        if self.method == RPC_ISOTP_RECV {
            u64_arg(p, "after_pdu_seq", false)?;
            if u64_arg(p, "max_pdus", false)?.is_some_and(|m| !(1..=16).contains(&m)) {
                return Err(usage("max_pdus must be 1..=16"));
            }
        } else {
            u64_arg(p, "epoch", true)?;
            u64_arg(p, "deadline_mono_ns", true)?;
        }
        Ok(timeout)
    }

    fn done(&self, out: &Value) -> bool {
        if self.method == RPC_ISOTP_RECV {
            out["pdus"].as_array().is_some_and(|a| !a.is_empty())
        } else {
            out["complete"].as_bool() == Some(true)
        }
    }
}

impl Handler for IsoTpPollH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let Value::Object(p) = params else {
                return Err(usage("params must be an object"));
            };
            let timeout = self.check(&p)?;
            let start = Instant::now();
            let until = (start + Duration::from_millis(timeout)).min(ctx.deadline());
            let args = Value::Object(p);
            let mut polls = 0u32;
            loop {
                // Same arguments every time: recv keeps its cursor, barrier its epoch/deadline.
                let mut out = ctx
                    .worker(WORKER_KIND_SIM)?
                    .call(self.method, args.clone(), ctx.deadline())
                    .await?;
                polls += 1;
                let now = Instant::now();
                let done = self.done(&out);
                if done || now >= until {
                    if let Value::Object(m) = &mut out {
                        m.insert("polls".into(), json!(polls));
                        m.insert(
                            "waited_ms".into(),
                            json!(u64::try_from(now.duration_since(start).as_millis()).unwrap_or(u64::MAX)),
                        );
                        m.insert("timed_out".into(), json!(!done));
                    }
                    return Ok(out);
                }
                timer::sleep(ISOTP_POLL.min(until.saturating_duration_since(now))).await;
            }
        })
    }
}

/// Codes of `can.isotp_tx_error` / `rx_error` (worker `isotp::error_code` + `CANCELLED`).
const ISOTP_ERROR_CODES: &[&str] = &[
    "N_BS_TIMEOUT",
    "N_CR_TIMEOUT",
    "WRONG_SN",
    "UNEXPECTED_PDU",
    "FC_OVERFLOW",
    "OVERFLOW",
    "WFT_OVRN",
    "ABORTED",
    "CANCELLED",
    "INVALID_PCI",
    "INVALID_LENGTH",
    "INVALID_FLOW_STATUS",
    "BUSY",
    "INTERNAL",
];

fn isotp_codes() -> ValueType {
    ValueType::Enum(ISOTP_ERROR_CODES.iter().map(|c| (*c).to_owned()).collect())
}

fn isotp_kind(kind: &str, required: &[&str], mut pointers: Vec<PointerSpec>) -> EventKindSpec {
    pointers.insert(0, ptr("/payload/endpoint", ValueType::String));
    EventKindSpec {
        kind: kind.into(),
        payload_schema: Schema::try_from(json!({
            "type": "object",
            "required": required,
            "properties": {"endpoint": {"type": "string"}, "epoch": {"type": "integer"},
                           "channel": {"type": "string"}}
        }))
        .unwrap_or_else(|_| json_schema!({"type": "object"})),
        pointers,
        version: 1,
    }
}

fn register_isotp(reg: &mut Registry, table: &Shared) -> Result<(), RegisterError> {
    let open = json_schema!({
        "type": "object", "required": ["tx_id", "rx_id"],
        "properties": {
            "channel": {"type": "string"}, "tx_id": {"type": "integer", "minimum": 0},
            "rx_id": {"type": "integer", "minimum": 0}, "ext": {"type": "boolean"},
            "fd": {"type": "boolean"}, "brs": {"type": "boolean"},
            "tx_dl": {"enum": [8, 12, 16, 20, 24, 32, 48, 64]},
            "padding": {"type": ["integer", "null", "string"]},
            "block_size": {"type": "integer", "minimum": 0, "maximum": 255},
            "st_min": {"type": "integer", "minimum": 0, "maximum": 255},
            "n_bs_ms": {"type": "integer", "minimum": 1, "maximum": crate::isotp::MAX_TIMEOUT_MS},
            "n_cr_ms": {"type": "integer", "minimum": 1, "maximum": crate::isotp::MAX_TIMEOUT_MS},
            "max_pdu": {"type": "integer", "minimum": 1, "maximum": 4095},
            "role": {"enum": ["client", "server"]}, "owner": {"type": "string"},
            "model": {"type": "string"}, "faults": {"type": "object"}
        }
    });
    let endpoint = json_schema!({
        "type": "object", "additionalProperties": false, "required": ["endpoint"],
        "properties": {"endpoint": {"type": "string"}}
    });
    let send = json_schema!({
        "type": "object", "additionalProperties": false, "required": ["endpoint", "pdu"],
        "properties": {"endpoint": {"type": "string"}, "pdu": {"type": "string", "maxLength": 8192}}
    });
    let list = json_schema!({
        "type": "object", "additionalProperties": false, "properties": {"channel": {"type": "string"}}
    });
    let control = |method: &str, params: Schema, handler: Arc<dyn Handler>| CommandSpec {
        roles: RoleSet::WRITE,
        ..read_spec(method, params, handler, Duration::from_secs(5))
    };
    let tx_states = vec!["driver_accepted".to_owned(), "bus_confirmed".to_owned()];
    reg.command(control(
        RPC_ISOTP_OPEN,
        open,
        Arc::new(IsoTpOpenH { table: table.clone() }),
    ))?
    .command(CommandSpec {
        write_class: Some(WriteClass::Request),
        ..control(RPC_ISOTP_SEND, send, Arc::new(IsoTpSendH))
    })?
    .command(CommandSpec {
        write_class: Some(WriteClass::Request),
        ..control(
            RPC_ISOTP_CANCEL,
            json_schema!({
                "type": "object", "additionalProperties": false, "required": ["endpoint", "transfer"],
                "properties": {
                    "endpoint": {"type": "string", "maxLength": 64},
                    "transfer": {"type": "string", "minLength": 1, "maxLength": 64}
                }
            }),
            Arc::new(IsoTpCancelH),
        )
    })?
    .command(read_spec(
        RPC_ISOTP_RECV,
        json_schema!({
            "type": "object", "additionalProperties": false, "required": ["endpoint"],
            "properties": {
                "endpoint": {"type": "string", "maxLength": 64},
                "after_pdu_seq": {"type": ["integer", "string"]},
                "timeout_ms": {"type": "integer", "minimum": 0, "maximum": ISOTP_MAX_WAIT_MS},
                "max_pdus": {"type": "integer", "minimum": 1, "maximum": 16}
            }
        }),
        Arc::new(IsoTpPollH {
            method: RPC_ISOTP_RECV,
        }),
        // Request timeout plus IPC margin.
        Duration::from_millis(ISOTP_MAX_WAIT_MS) + Duration::from_secs(5),
    ))?
    .command(read_spec(
        RPC_ISOTP_BARRIER,
        json_schema!({
            "type": "object", "additionalProperties": false,
            "required": ["endpoint", "epoch", "deadline_mono_ns"],
            "properties": {
                "endpoint": {"type": "string", "maxLength": 64},
                "epoch": {"type": ["integer", "string"]},
                "deadline_mono_ns": {"type": ["integer", "string"]},
                "timeout_ms": {"type": "integer", "minimum": 0, "maximum": ISOTP_MAX_WAIT_MS}
            }
        }),
        Arc::new(IsoTpPollH {
            method: RPC_ISOTP_BARRIER,
        }),
        Duration::from_millis(ISOTP_MAX_WAIT_MS) + Duration::from_secs(5),
    ))?
    .command(control(
        RPC_ISOTP_ABORT,
        endpoint.clone(),
        Arc::new(WorkerCallH {
            method: RPC_ISOTP_ABORT,
        }),
    ))?
    .command(control(
        RPC_ISOTP_CLOSE,
        endpoint,
        Arc::new(WorkerCallH {
            method: RPC_ISOTP_CLOSE,
        }),
    ))?
    .command(read_spec(
        RPC_ISOTP_LIST,
        list,
        Arc::new(WorkerCallH {
            method: RPC_ISOTP_LIST,
        }),
        Duration::from_secs(5),
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_TX_DONE,
        &["endpoint", "tx_state"],
        vec![ptr("/payload/tx_state", ValueType::Enum(tx_states))],
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_TX_ERROR,
        &["endpoint", "code"],
        vec![ptr("/payload/code", isotp_codes())],
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_RX_START,
        &["endpoint", "expected_length"],
        vec![ptr("/payload/expected_length", ValueType::U32)],
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_RX_COMPLETE,
        &["endpoint", "pdu"],
        vec![
            ptr("/payload/pdu/data", ValueType::Hex),
            ptr("/payload/len", ValueType::U32),
            ptr("/payload/pdu_seq", ValueType::U64String),
            ptr("/payload/ts_last_frame", ValueType::U64String),
        ],
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_RX_ERROR,
        &["endpoint", "code"],
        vec![ptr("/payload/code", isotp_codes())],
    ))?
    .event_kind(isotp_kind(
        KIND_ISOTP_STATE,
        &["endpoint", "state"],
        vec![ptr("/payload/state", ValueType::String)],
    ))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Write hooks
// ---------------------------------------------------------------------------------------------

/// CAN write hooks: risk display (F1) and the channel generation check.
#[derive(Debug)]
pub struct CanHooks {
    table: Shared,
}

impl WriteHooks for CanHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        // Periodic `count` is a job length, not a burst: classify by method first.
        let (risk, reason) = if req.method == RPC_ISOTP_CANCEL {
            // Stop control: only removes traffic (never latched or approval-gated as a new send).
            (Risk::R0, "stops an ISO-TP transfer".to_owned())
        } else if is_periodic_write(req.method) {
            classify_periodic(req.method, req.env)
        } else {
            let count = req.args.get("count").and_then(Value::as_u64).unwrap_or(1);
            classify_send(count, req.env)
        };
        Classification {
            risk,
            reason,
            block: None,
        }
    }

    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        if req.target.generation == Some(0) {
            return Err(PresendError::EpochChanged("channel not open".into()));
        }
        let Some(ch) = req.target.channel.as_deref() else {
            return Ok(());
        };
        let t = lock(&self.table);
        let Some(info) = t.channels.get(ch) else {
            return Ok(()); // unknown here: the worker answers E_NOT_OPEN if it is not open
        };
        if info.state != "open" {
            return Err(PresendError::EpochChanged(format!(
                "channel {ch} is {}",
                info.state
            )));
        }
        match (req.target.generation, info.generation) {
            (Some(want), Some(now)) if want != now => Err(PresendError::EpochChanged(format!(
                "channel {ch} reopened (generation {want} → {now})"
            ))),
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reducer: Fixed view + channel state for late-joining viewers
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct FixedChannel {
    ids: HashMap<(u32, bool), (Value, u64, u64)>, // (record summary, count, last seq)
    replaced: u64,
    last_seq: u64,
    state: Option<Value>,
}

/// Snapshot provider: per channel, the last frame of every id (Fixed) and the channel state.
#[derive(Debug)]
pub struct FixedReducer {
    table: Shared,
    channels: BTreeMap<String, FixedChannel>,
    channels_replaced: u64,
}

impl FixedReducer {
    fn channel(&mut self, ch: &str, seq: u64) -> &mut FixedChannel {
        if !self.channels.contains_key(ch)
            && self.channels.len() >= FIXED_MAX_CHANNELS
            && let Some(old) = self
                .channels
                .iter()
                .min_by_key(|(_, c)| c.last_seq)
                .map(|(k, _)| k.clone())
        {
            self.channels.remove(&old);
            self.channels_replaced += 1;
        }
        let c = self.channels.entry(ch.to_owned()).or_default();
        c.last_seq = c.last_seq.max(seq);
        c
    }
}

impl Reducer for FixedReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_FRAME, KIND_CHANNEL_STATE]
    }

    fn apply(&mut self, ev: &PublishedRecord) {
        let Some(ch) = ev.channel.clone() else { return };
        let seq = ev.seq.get();
        if ev.kind == KIND_CHANNEL_STATE {
            lock(&self.table).update(ev);
            let state = json!({"payload": ev.payload, "generation": ev.generation, "seq": ev.seq});
            self.channel(&ch, seq).state = Some(state);
            return;
        }
        let Some(f) = frame_payload(ev) else { return };
        let summary = json!({
            "seq": ev.seq, "epoch": ev.epoch, "ts": ev.ts, "dir": ev.dir, "origin": ev.origin,
            "external_actor": ev.external_actor, "request_id": ev.request_id, "payload": ev.payload,
        });
        let c = self.channel(&ch, seq);
        let key = (f.id, f.ext);
        if let Some(e) = c.ids.get_mut(&key) {
            *e = (summary, e.1 + 1, seq);
            return;
        }
        if c.ids.len() >= FIXED_MAX_IDS {
            // Bounded: replace the id seen longest ago.
            if let Some(old) = c.ids.iter().min_by_key(|(_, v)| v.2).map(|(k, _)| *k) {
                c.ids.remove(&old);
                c.replaced += 1;
            }
        }
        c.ids.insert(key, (summary, 1, seq));
    }

    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        self.channels
            .iter()
            .filter(|(ch, _)| filter.channels.is_empty() || filter.channels.contains(ch))
            .map(|(ch, c)| {
                let mut ids: Vec<_> = c.ids.iter().collect();
                ids.sort_by_key(|(k, _)| **k);
                let ids: Vec<Value> = ids
                    .into_iter()
                    .map(|((id, ext), (last, count, _))| json!({"id": id, "ext": ext, "count": count, "last": last}))
                    .collect();
                json!({
                    "kind": "can.fixed",
                    "channel": ch,
                    "state": c.state,
                    "ids": ids,
                    "ids_replaced": c.replaced,
                    "channels_replaced": self.channels_replaced,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Evidence verifier
// ---------------------------------------------------------------------------------------------

/// Predicate `can.frame_match`.
pub const PRED_FRAME_MATCH: &str = "can.frame_match";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchArgs {
    id: IdArg,
    #[serde(default)]
    ext: Option<bool>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    mask: Option<String>,
    #[serde(default)]
    dir: Option<Dir>,
    #[serde(default)]
    origin: Option<Origin>,
}

/// Re-evaluates `can.frame_match{id, ext?, data?, mask?, dir?, origin?}` and
/// `can.signal_range{…}` (see [`crate::signals`]) on the original events.
/// Meaning: "a matching frame / sample exists in the window" (coverage `Positive`).
#[derive(Debug, Default)]
pub struct CanVerifier {
    dbs: Dbs,
}

impl EvidenceVerifier for CanVerifier {
    fn predicates(&self) -> &[&'static str] {
        &[PRED_FRAME_MATCH, PRED_SIGNAL_RANGE]
    }

    fn coverage_need(&self, _pred: &Predicate) -> CoverageNeed {
        CoverageNeed::Positive
    }

    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification {
        if pred.name == PRED_SIGNAL_RANGE {
            return verify_signal_range(&self.dbs, pred, events, cov, budget);
        }
        let spec = serde_json::from_value::<MatchArgs>(pred.args.clone())
            .map_err(|e| e.to_string())
            .and_then(|a| {
                let id = a.id.single().map_err(|e| e.message)?;
                FrameSpec::new(id, a.ext, a.data.as_deref(), a.mask.as_deref(), a.origin, a.dir)
                    .map_err(|e| e.message)
            });
        let spec = match spec {
            Ok(s) => s,
            // A malformed predicate is an error, never a "false" (contract 07 §7.4).
            Err(e) => {
                return Verification::Inconclusive {
                    reason: format!("invalid {PRED_FRAME_MATCH} args: {e}"),
                };
            }
        };
        let mut checked = 0usize;
        for chunk in events.chunks(1024) {
            if budget.spend(chunk.len()).is_err() {
                return Verification::Inconclusive {
                    reason: "eval_budget".into(),
                };
            }
            for rec in chunk {
                checked += 1;
                if spec.matches(rec) {
                    return Verification::Match {
                        matched: vec![EventRef {
                            epoch: rec.epoch,
                            seq: rec.seq,
                            source: rec.source_ref(),
                        }],
                        observed: observed(rec),
                    };
                }
            }
        }
        if cov.complete {
            Verification::Mismatch {
                observed: json!({"frames_checked": checked}),
                reason: format!("no matching frame {:#X} in the window", spec.id),
            }
        } else {
            // Absence over an incomplete window proves nothing.
            Verification::Inconclusive {
                reason: format!("window incomplete: {}", cov.causes.join(", ")),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------------------------

/// The CAN domain module.
#[derive(Debug, Default)]
pub struct Can;

/// Allow omitting `channel` (resolved by the gateway) in a derived params schema.
fn channel_optional(schema: Schema) -> Schema {
    let mut v = schema.to_value();
    if let Some(req) = v.get_mut("required").and_then(Value::as_array_mut) {
        req.retain(|r| r != "channel");
    }
    Schema::try_from(v).unwrap_or_else(|_| json_schema!({"type": "object"}))
}

pub(crate) fn read_spec(
    method: &str,
    params: Schema,
    handler: Arc<dyn Handler>,
    max_wait: Duration,
) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: None,
        params,
        result: json_schema!({"type": "object"}),
        roles: RoleSet::READ,
        handler,
        bounds: Bounds {
            max_wait,
            default_rows: DEFAULT_ROWS,
            max_rows: MAX_ROWS,
        },
        default_action: None,
    }
}

pub(crate) fn ptr(path: &str, ty: ValueType) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: None,
        ty,
    }
}

fn write_states() -> Vec<String> {
    [
        "requested",
        "accepted",
        "sent",
        "confirmed",
        "effective_verified",
        "effect_unverified",
        "failed",
        "outcome_unknown",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

impl DomainModule for Can {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let table: Shared = Arc::default();
        let dbs: Dbs = Arc::default();
        let cursor = json_schema!({"type": "string"});
        let id_arg = json_schema!({"oneOf": [{"type": "integer", "minimum": 0}, {"type": "string"}]});
        let trace_params = json_schema!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "channel": {"type": "string"}, "since": cursor, "until": cursor,
                "ids": {"type": "array", "items": id_arg, "maxItems": 64},
                "origins": {"type": "array", "items": {"enum": ["agent", "user", "device", "script"]}},
                "dir": {"enum": ["rx", "tx"]},
                "max": {"type": "integer", "minimum": 1, "maximum": MAX_ROWS},
                "peek": {"type": "boolean"}
            }
        });
        let last_params = json_schema!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "channel": {"type": "string"}, "id": id_arg,
                "window_ms": {"type": "integer", "minimum": 1, "maximum": LAST_MAX_WINDOW_MS}
            }
        });
        let expect_params = json_schema!({
            "type": "object", "additionalProperties": false, "required": ["id", "within_ms"],
            "properties": {
                "channel": {"type": "string"}, "id": id_arg, "ext": {"type": "boolean"},
                "data": {"type": "string"}, "mask": {"type": "string"},
                "origin": {"enum": ["agent", "user", "device", "script"]},
                "dir": {"enum": ["rx", "tx"]}, "after": cursor,
                "within_ms": {"type": "integer", "minimum": 1, "maximum": MAX_EXPECT_MS}
            }
        });
        let expect_wait = Duration::from_millis(u64::from(MAX_EXPECT_MS) + 1_000);

        reg.command(CommandSpec {
            method: RPC_SEND.into(),
            write_class: Some(WriteClass::Request),
            params: channel_optional(schema_for!(SendParams)),
            result: json_schema!({"type": "object"}),
            roles: RoleSet::WRITE,
            handler: Arc::new(SendH { table: table.clone() }),
            bounds: Bounds {
                max_wait: expect_wait + Duration::from_secs(10),
                default_rows: DEFAULT_ROWS,
                max_rows: MAX_ROWS,
            },
            default_action: None, // F1: automatic
        })?
        .command(read_spec(
            RPC_TRACE_QUERY,
            trace_params,
            Arc::new(TraceH {
                table: table.clone(),
                dbs: dbs.clone(),
            }),
            Duration::from_secs(10),
        ))?
        .command(read_spec(
            RPC_LAST,
            last_params,
            Arc::new(LastH {
                table: table.clone(),
                dbs: dbs.clone(),
            }),
            Duration::from_secs(10),
        ))?
        .command(read_spec(
            RPC_EXPECT,
            expect_params,
            Arc::new(ExpectH { table: table.clone() }),
            expect_wait,
        ))?
        .command(read_spec(
            RPC_STATUS,
            json_schema!({"type": "object"}),
            Arc::new(StatusH),
            Duration::from_secs(5),
        ))?
        .event_kind(EventKindSpec {
            kind: KIND_FRAME.into(),
            payload_schema: schema_for!(FramePayload),
            pointers: vec![
                ptr("/payload/id", ValueType::U32),
                ptr("/payload/ext", ValueType::Bool),
                ptr("/payload/fd", ValueType::Bool),
                ptr("/payload/dlc", ValueType::U8),
                ptr("/payload/len", ValueType::U8),
                ptr("/payload/bytes/data", ValueType::Hex),
            ],
            version: 1,
        })?
        .event_kind(EventKindSpec {
            kind: KIND_TX.into(),
            payload_schema: schema_for!(TxPayload),
            pointers: vec![
                ptr("/payload/write_state", ValueType::Enum(write_states())),
                ptr("/payload/frame/id", ValueType::U32),
                ptr("/payload/count", ValueType::U32),
            ],
            version: 1,
        })?
        .event_kind(EventKindSpec {
            kind: KIND_CHANNEL_STATE.into(),
            payload_schema: json_schema!({"type": "object", "required": ["state"],
                "properties": {"state": {"type": "string"}, "backend": {"type": "string"}}}),
            pointers: vec![
                ptr("/payload/state", ValueType::String),
                ptr("/payload/backend", ValueType::String),
            ],
            version: 1,
        })?
        .event_kind(EventKindSpec {
            kind: KIND_STATS.into(),
            payload_schema: json_schema!({"type": "object"}),
            pointers: vec![ptr("/payload/fps", ValueType::F64)],
            version: 1,
        })?
        .risk_table(RiskTable {
            rows: vec![
                RiskRow {
                    method: RPC_SEND.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "sim channel (M1)".into(),
                },
                RiskRow {
                    method: RPC_SEND.into(),
                    selector: Some(format!("count>{BURST_R3_ABOVE}")),
                    risk: Risk::R3,
                    reason: format!("finite burst > {BURST_R3_ABOVE} frames"),
                },
                RiskRow {
                    method: RPC_PERIODIC_START.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "periodic job on a sim channel (M1); R3 \"periodic\" on hardware".into(),
                },
                RiskRow {
                    method: RPC_PERIODIC_UPDATE.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "changes a periodic job's bytes (sim); R3 on hardware".into(),
                },
                RiskRow {
                    method: RPC_PERIODIC_STOP.into(),
                    selector: None,
                    risk: Risk::R0,
                    reason: "stops a periodic job".into(),
                },
                RiskRow {
                    method: RPC_PERIODIC_RENEW.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "extends a periodic job (sim); R3 on hardware".into(),
                },
                RiskRow {
                    method: RPC_PERIODIC_TAKEOVER.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "takes over a periodic job (sim); R3 on hardware".into(),
                },
                RiskRow {
                    method: RPC_SIG_SET.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "IG: changes a signal of a periodic job (sim); R3 on hardware".into(),
                },
                RiskRow {
                    method: RPC_ISOTP_CANCEL.into(),
                    selector: None,
                    risk: Risk::R0,
                    reason: "stops an ISO-TP transfer".into(),
                },
                RiskRow {
                    method: RPC_ISOTP_SEND.into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "ISO-TP PDU on a sim channel (M1); PDU meaning is judged by UDS".into(),
                },
            ],
            ..RiskTable::default()
        })?
        .hooks(Arc::new(CanHooks { table: table.clone() }))?
        .worker(WorkerSpec {
            kind: WORKER_KIND_SIM.into(),
            instances: Instances::Shared,
            budgets: Budgets {
                mem_soft: 192 << 20,
                mem_hard: 256 << 20,
                credit: 2 << 20,
            },
            reopen_after_crash: Reopen::RxOnly,
            arch: Arch::Native,
            platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
        })?
        .reducer(Box::new(FixedReducer {
            table: table.clone(),
            channels: BTreeMap::new(),
            channels_replaced: 0,
        }))?
        .evidence_verifier(Arc::new(CanVerifier { dbs: dbs.clone() }))?
        .capability("sim")?
        .help(help())?;
        register_isotp(reg, &table)?;
        register_periodic(reg, &table, &dbs)?;
        crate::signals::register(reg, &table, &dbs)
    }
}

fn help() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "CAN / CAN FD through the Loom daemon (M1: sim bus with a fake BCM). The user sees every \
                  frame live in the Loom UI CAN Trace."
            .into(),
        examples: vec![
            ex("loom can status", "channels, generation, drops, STOP latch, fake ECU nodes"),
            ex("loom can send 300#01 --channel ch0", "send one frame (R1 on sim)"),
            ex(
                "loom can send 300#01 --expect 0x301 --within 100",
                "send and wait for the response; result has evidence refs",
            ),
            ex("loom can send 300#01 --dry-run", "risk and bytes only, nothing is sent"),
            ex("loom can trace --since @last --ids 0x300-0x30F --max 50", "bounded trace read"),
            ex("loom can last --id 0x310", "last frame of an id (Fixed view)"),
            ex("loom can db load /abs/path/body.dbc -c ch0", "load a DBC for a channel (signals, decode)"),
            ex("loom can trace --decode --max 20", "trace rows with decoded signals"),
            ex(
                "loom can isotp open --tx 0x7E0 --rx 0x7E8",
                "ISO-TP endpoint for transport debugging (diagnostics: use `loom uds`)",
            ),
            ex(
                "loom can sig get EngineData.EngRPM",
                "current value, age and evidence ref (exit 3 if none in the window)",
            ),
            ex(
                "loom can sig wait-for \"EngRPM > 3000\" --timeout 10",
                "wait until a new sample satisfies the condition; exit 3 on timeout",
            ),
            ex(
                "loom can expect --id 0x301 --data 01 --after seq:5013 --within 200",
                "wait for a device frame; exit 3 on timeout",
            ),
        ],
        notes: vec![
            "Use only `loom can …`. Never open the CAN device with other tools or scripts while Loom owns it."
                .into(),
            "If the result warns that no viewer is attached, ask the user to open the Loom UI (`loom ui open can`)."
                .into(),
            "Cite `data.evidence[]` / `refs` from expect results; never invent sequence numbers.".into(),
            "Outputs are bounded: 100 rows by default, 1,000 / 64 KiB at most; continue with `next_cursor`.".into(),
            "Signals need a DBC on the channel (`loom can db load`); name them `Message.Signal` when a name is \
             not unique."
                .into(),
            "`loom can isotp …` is for ISO-TP transport debugging; for UDS diagnostics use `loom uds`. An open \
             endpoint reserves its tx id: raw sends on it are refused (E_DENIED tx_reserved)."
                .into(),
            "M2b: `check`, `stats`, periodic sends, `sig set`.".into(),
        ],
    }
}
