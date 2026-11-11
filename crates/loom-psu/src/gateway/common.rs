//! Shared gateway helpers: channel book (reducer writes, hooks/handlers read), bounded journal
//! scan, a runtime-agnostic delay, and hold evaluation used by `wait_for` and evidence.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use loom_domain_api::{CallCtx, DomainError, JournalQuery};
use loom_proto::cursor::Cursor;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord};
use loom_proto::rpc::SubFilter;
use serde_json::{Value, json};

use crate::model::{Condition, Limits, POLL_PERIOD_NS, metric, param_ch};
use crate::{DOMAIN, KIND_MEAS, KIND_STATE, KIND_WRITE};

/// Supplies remembered by the gateway.
pub const MAX_BOOK_CHANNELS: usize = 16;
/// Events per journal scan call.
pub const SCAN_EVENTS_PER_CALL: u32 = 10_000;
/// Bytes per journal scan call.
pub const SCAN_BYTES_PER_CALL: u64 = 4 * 1024 * 1024;
/// Bytes one request may scan in total.
pub const SCAN_BUDGET_BYTES: u64 = 32 * 1024 * 1024;
/// Default largest interval between two samples of one hold (3 poll periods).
pub const DEFAULT_MAX_GAP_MS: u64 = 3 * POLL_PERIOD_NS / 1_000_000;
/// Kinds a psu scan reads.
pub const SCAN_KINDS: &[&str] = &[KIND_MEAS, KIND_STATE, KIND_WRITE, "source.gap", "record.gap"];

/// What the gateway knows about one supply (from `psu.state`).
#[derive(Debug, Clone, Default)]
pub struct ChanInfo {
    /// Last `psu.state` payload.
    pub state: Value,
    /// Record generation.
    pub generation: Option<u32>,
    touched: u64,
}

impl ChanInfo {
    /// Profile limits (default when unknown).
    pub fn limits(&self) -> Limits {
        Limits::from_value(&self.state)
    }
}

/// Shared channel book.
#[derive(Debug, Clone, Default)]
pub struct ChannelBook(Arc<Mutex<(BTreeMap<String, ChanInfo>, u64)>>);

impl ChannelBook {
    /// Fold a `psu.state` record.
    pub fn update(&self, ch: &str, state: &Value, generation: Option<u32>) {
        let Ok(mut g) = self.0.lock() else { return };
        let (map, clock) = &mut *g;
        *clock += 1;
        let tick = *clock;
        if !map.contains_key(ch)
            && map.len() >= MAX_BOOK_CHANNELS
            && let Some(k) = map.iter().min_by_key(|(_, v)| v.touched).map(|(k, _)| k.clone())
        {
            map.remove(&k);
        }
        let e = map.entry(ch.to_owned()).or_default();
        e.state = state.clone();
        if generation.is_some() {
            e.generation = generation;
        }
        e.touched = tick;
    }

    /// One supply.
    pub fn get(&self, ch: &str) -> Option<ChanInfo> {
        self.0.lock().ok().and_then(|g| g.0.get(ch).cloned())
    }

    /// Known supplies.
    pub fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|g| g.0.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Any supply in a power cycle (interlock `psu.cycling`).
    pub fn any_cycling(&self) -> bool {
        self.0
            .lock()
            .map(|g| {
                g.0.values()
                    .any(|c| c.state.get("cycling").and_then(Value::as_bool) == Some(true))
            })
            .unwrap_or(false)
    }
}

/// Channel of a request: explicit, else the only known supply.
pub fn resolve_channel(params: &Value, book: &ChannelBook) -> Result<String, DomainError> {
    if let Some(ch) = param_ch(params) {
        return Ok(ch.to_owned());
    }
    match book.names().as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(DomainError::new(ErrorCode::NotOpen, "no supply connected")
            .with_detail(json!({"hint": "loom psu connect <name>"}))),
        many => Err(
            DomainError::new(ErrorCode::AmbiguousChannel, "several supplies; name one")
                .with_detail(json!({"channels": many})),
        ),
    }
}

/// `E_USAGE`.
pub fn usage(msg: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::Usage, msg)
}

/// Channel of a record.
pub fn channel_of(rec: &PublishedRecord) -> &str {
    rec.channel
        .as_deref()
        .or_else(|| rec.payload.get("ch").and_then(Value::as_str))
        .unwrap_or("")
}

/// `psu:<ch>@seq:<a>..<b>`.
pub fn ref_range(ch: &str, a: u64, b: u64) -> String {
    if a == b {
        format!("{DOMAIN}:{ch}@seq:{a}")
    } else {
        format!("{DOMAIN}:{ch}@seq:{a}..{b}")
    }
}

/// Parses a cursor parameter.
pub fn cursor_param(v: Option<&Value>) -> Result<Option<Cursor>, DomainError> {
    match v.and_then(Value::as_str) {
        None => Ok(None),
        Some(s) => s
            .parse::<Cursor>()
            .map(Some)
            .map_err(|e| usage(format!("bad cursor {s:?}: {e}"))),
    }
}

/// Bounded scan of `[from, now]` for one supply. Returns events in order, the resume cursor
/// (seq after the last event, if any) and whether the byte budget cut it short.
pub async fn scan(
    ctx: &CallCtx,
    ch: &str,
    from: Cursor,
    budget: u64,
) -> Result<(Vec<PublishedRecord>, Option<u64>, bool), DomainError> {
    let mut out = Vec::new();
    let mut from = from;
    let mut used = 0u64;
    let mut gaps_seen = false;
    loop {
        let q = JournalQuery {
            from: from.clone(),
            until: None,
            filter: SubFilter {
                channels: vec![ch.to_owned()],
                kinds: SCAN_KINDS.iter().map(|k| (*k).to_owned()).collect(),
                ..SubFilter::default()
            },
            request_id: None,
            max_events: SCAN_EVENTS_PER_CALL,
        };
        let r = ctx
            .journal()
            .scan(&q, SCAN_BYTES_PER_CALL.min(budget.saturating_sub(used)).max(1))
            .await?;
        used = used.saturating_add(r.scanned_bytes);
        gaps_seen |= !r.gaps.is_empty();
        out.extend(r.events);
        match r.resume_from {
            Some(next) if used < budget => from = next,
            Some(_) => {
                let last = out.last().map(|e| e.seq.get() + 1);
                return Ok((out, last, true));
            }
            None => {
                let last = out.last().map(|e| e.seq.get() + 1);
                return Ok((out, last, gaps_seen));
            }
        }
    }
}

/// True for a user-origin write that switched an output off (emergency OFF or panel toggle).
pub fn is_user_off(rec: &PublishedRecord) -> bool {
    rec.kind == KIND_WRITE
        && rec.origin == Origin::User
        && matches!(
            rec.payload.get("action").and_then(Value::as_str),
            Some("out" | "cycle")
        )
        && rec.payload.get("state").and_then(Value::as_str) != Some("on")
        && rec.payload.get("write_state").and_then(Value::as_str) == Some("sent")
}

// ---------------------------------------------------------------------------------------------
// Hold evaluation.

/// One step of the measured series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// A sample: seq, mono ns, value.
    Sample(u64, u64, f64),
    /// A loss (`source.gap`, `record.gap`).
    Loss(u64),
}

/// The measured series of `metric_name` in a record list.
pub fn series(events: &[PublishedRecord], metric_name: &str) -> Vec<Step> {
    events
        .iter()
        .filter_map(|e| match e.kind.as_str() {
            KIND_MEAS => {
                metric(&e.metrics, metric_name).map(|v| Step::Sample(e.seq.get(), e.ts.mono_ns.get(), v))
            }
            "source.gap" | "record.gap" => Some(Step::Loss(e.seq.get())),
            _ => None,
        })
        .collect()
}

/// Result of a hold search.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hold {
    /// First satisfying run `(first seq, last seq, held ms, samples)`.
    pub run: Option<(u64, u64, u64, u64)>,
    /// Samples seen.
    pub samples: u64,
    /// Losses seen.
    pub losses: u64,
    /// Intervals longer than the allowed gap.
    pub long_intervals: u64,
    /// Longest qualifying run so far (ms).
    pub best_ms: u64,
    /// Last value.
    pub last: Option<f64>,
}

/// Finds the first run of samples meeting `cond` for `hold_ms`: consecutive samples, every
/// interval ≤ `max_gap_ms`, no loss inside. `hold_ms == 0` = one sample.
pub fn find_hold(steps: &[Step], cond: &Condition, hold_ms: u64, max_gap_ms: u64) -> Hold {
    let mut h = Hold::default();
    let mut run: Option<(u64, u64, u64)> = None; // (first seq, first t, samples)
    let mut prev_t: Option<u64> = None;
    let max_gap_ns = max_gap_ms.saturating_mul(1_000_000);
    for s in steps {
        match *s {
            Step::Loss(_) => {
                h.losses += 1;
                run = None;
                prev_t = None;
            }
            Step::Sample(seq, t, v) => {
                h.samples += 1;
                h.last = Some(v);
                let contiguous = prev_t.is_some_and(|p| t.saturating_sub(p) <= max_gap_ns);
                if prev_t.is_some() && !contiguous {
                    h.long_intervals += 1;
                }
                prev_t = Some(t);
                if !cond.holds(v) {
                    run = None;
                    continue;
                }
                let (a, t0, n) = match run {
                    Some((a, t0, n)) if contiguous => (a, t0, n + 1),
                    _ => (seq, t, 1),
                };
                run = Some((a, t0, n));
                let held = t.saturating_sub(t0) / 1_000_000;
                h.best_ms = h.best_ms.max(held);
                if held >= hold_ms {
                    h.run = Some((a, seq, held, n));
                    return h;
                }
            }
        }
    }
    h
}

// ---------------------------------------------------------------------------------------------
// Runtime-agnostic delay (the domain API has no timer — CORE ask).

#[derive(Debug, Default)]
struct DelayShared {
    fired: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// Sleeps without a specific async runtime: a helper thread wakes the task.
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
