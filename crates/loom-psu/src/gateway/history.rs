//! `psu.history` (PSU-03, operator upgrade B3): statistics and optional raw rows of the journaled
//! `psu.meas` samples of one supply over an explicit wall-clock window. One bounded page per call
//! (journal byte budget, row cap); the caller follows `next_cursor` with the same window. No new
//! logger: the records are the ones the poller already journals. See FUNCTIONS.md §Batch 3.

use std::time::{SystemTime, UNIX_EPOCH};

use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler};
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::event::PublishedRecord;
use serde_json::{Value, json};

use super::common::{
    ChannelBook, SCAN_BUDGET_BYTES, Seek, TimeIndex, cursor_param, resolve_channel, scan_page, usage,
};
use crate::KIND_MEAS;
use crate::model::{HISTORY_MAX_ROWS, HISTORY_MAX_SPAN_MS, metric};

/// Mergeable statistics of one quantity (pages merge exactly: `n`, `min`, `max`, `sum`, `last`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HistStat {
    /// Samples.
    pub n: u64,
    /// Minimum (meaningless when `n == 0`).
    pub min: f64,
    /// Maximum (meaningless when `n == 0`).
    pub max: f64,
    /// Sum.
    pub sum: f64,
    /// Last value in stream order.
    pub last: Option<f64>,
}

impl HistStat {
    /// B3-H1: adds one sample.
    pub fn add(&mut self, x: f64) {
        if self.n == 0 {
            self.min = x;
            self.max = x;
        } else {
            self.min = self.min.min(x);
            self.max = self.max.max(x);
        }
        self.n += 1;
        self.sum += x;
        self.last = Some(x);
    }

    /// B3-H2: merges a LATER page into this one (`last` comes from `o` when `o.n > 0`).
    pub fn merge(&mut self, o: &HistStat) {
        if o.n == 0 {
            return;
        }
        if self.n == 0 {
            *self = *o;
            return;
        }
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
        self.n += o.n;
        self.sum += o.sum;
        self.last = o.last;
    }

    /// B3-H3: `{"n","min","max","sum","mean","last"}`, or `null` when `n == 0`.
    pub fn json(&self) -> Value {
        if self.n == 0 {
            return Value::Null;
        }
        json!({"n": self.n, "min": self.min, "max": self.max, "sum": self.sum,
               "mean": self.sum / self.n as f64, "last": self.last})
    }
}

/// Why a page fold stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// An event at/after the window end was seen: the window is done.
    ReachedEnd,
    /// The row cap was reached; the next page starts at this (unconsumed) sample.
    RowCap {
        /// Its stream epoch.
        epoch: u64,
        /// Its stream seq.
        seq: u64,
    },
    /// Every given event was consumed.
    Exhausted,
}

/// Result of folding one page of events.
#[derive(Debug, Clone, PartialEq)]
pub struct PageFold {
    /// Voltage (V).
    pub v: HistStat,
    /// Current (A).
    pub i: HistStat,
    /// Power (W): reported, else V×I.
    pub p: HistStat,
    /// Samples inside the window.
    pub samples: u64,
    /// `source.gap` / `record.gap` events inside the window.
    pub losses: u64,
    /// First sample `(seq, wall_ns)`.
    pub first: Option<(u64, u64)>,
    /// Last sample `(seq, wall_ns)`.
    pub last: Option<(u64, u64)>,
    /// Raw rows `{"seq","wall_ms","v","i","p"}` (≤ max_rows; empty when max_rows == 0).
    pub rows: Vec<Value>,
    /// Why the fold stopped.
    pub stop: Stop,
}

/// Resolved window (wall ns, `[from, to)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Inclusive start.
    pub from_wall_ns: u64,
    /// Exclusive end.
    pub to_wall_ns: u64,
}

/// B3-W1: `{from_ms | last_ms, to_ms?}` → window (wall ms params, epoch based), checked.
pub fn history_window(p: &Value, now_wall_ns: u64) -> Result<Window, DomainError> {
    let int = |k: &str| -> Result<Option<u64>, DomainError> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| usage(format!("{k} must be a non-negative integer (ms)"))),
        }
    };
    let (from_ms, last_ms, to_ms) = (int("from_ms")?, int("last_ms")?, int("to_ms")?);
    if from_ms.is_some() == last_ms.is_some() {
        return Err(usage("give exactly one of from_ms, last_ms"));
    }
    if let Some(l) = last_ms
        && (l == 0 || l > HISTORY_MAX_SPAN_MS)
    {
        return Err(usage(format!("last_ms must be 1..={HISTORY_MAX_SPAN_MS}")));
    }
    let to = to_ms.map_or(now_wall_ns, |t| t.saturating_mul(1_000_000));
    let from = match (from_ms, last_ms) {
        (Some(f), _) => f.saturating_mul(1_000_000),
        (None, Some(l)) => to.saturating_sub(l.saturating_mul(1_000_000)),
        (None, None) => unreachable!("one of from_ms/last_ms is present"),
    };
    if from >= to {
        return Err(usage("empty window: from must be before to (to defaults to now)"));
    }
    if to - from > HISTORY_MAX_SPAN_MS * 1_000_000 {
        return Err(usage(format!("window longer than {HISTORY_MAX_SPAN_MS} ms")));
    }
    Ok(Window {
        from_wall_ns: from,
        to_wall_ns: to,
    })
}

/// B3-F1: folds `events` (stream order) for the window; `max_rows == 0` = statistics only.
pub fn history_page(events: &[PublishedRecord], w: Window, max_rows: usize) -> PageFold {
    let mut f = PageFold {
        v: HistStat::default(),
        i: HistStat::default(),
        p: HistStat::default(),
        samples: 0,
        losses: 0,
        first: None,
        last: None,
        rows: Vec::new(),
        stop: Stop::Exhausted,
    };
    for e in events {
        match e.kind.as_str() {
            KIND_MEAS => {
                let (Some(v), Some(i)) = (metric(&e.metrics, "voltage"), metric(&e.metrics, "current"))
                else {
                    continue;
                };
                let wall = e.ts.wall_ns.get();
                if wall >= w.to_wall_ns {
                    f.stop = Stop::ReachedEnd;
                    break;
                }
                if wall < w.from_wall_ns {
                    continue;
                }
                if max_rows > 0 && f.rows.len() == max_rows {
                    f.stop = Stop::RowCap {
                        epoch: e.epoch.get(),
                        seq: e.seq.get(),
                    };
                    break;
                }
                let p = metric(&e.metrics, "power").unwrap_or(v * i);
                f.v.add(v);
                f.i.add(i);
                f.p.add(p);
                f.samples += 1;
                let seq = e.seq.get();
                f.first.get_or_insert((seq, wall));
                f.last = Some((seq, wall));
                if max_rows > 0 {
                    f.rows.push(json!({
                        "seq": seq, "wall_ms": wall / 1_000_000, "v": v, "i": i, "p": p,
                        "ref": Cursor::Seq { epoch: Some(e.epoch.get()), seq }.to_string(),
                        "generation": e.generation,
                        "clock_id": e.ts.clock_id,
                    }));
                }
            }
            "source.gap" | "record.gap" => {
                let wall = e.ts.wall_ns.get();
                if wall >= w.from_wall_ns && wall < w.to_wall_ns {
                    f.losses += 1;
                }
            }
            _ => {}
        }
    }
    f
}

/// `psu.history{ch?, from_ms|last_ms, to_ms?, rows?, cursor?}` (read).
#[derive(Debug)]
pub struct History(pub ChannelBook, pub TimeIndex);

impl Handler for History {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { history(ctx, &self.0, &self.1, &params).await })
    }
}

/// B3-Q1: one page of `psu.history`.
async fn history(
    ctx: &CallCtx,
    book: &ChannelBook,
    index: &TimeIndex,
    p: &Value,
) -> Result<Value, DomainError> {
    let ch = resolve_channel(p, book)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| usage("system clock before 1970"))
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))?;
    let w = history_window(p, now)?;
    let rows = match p.get("rows") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .filter(|r| *r <= u64::from(HISTORY_MAX_ROWS))
            .ok_or_else(|| usage(format!("rows must be 0..={HISTORY_MAX_ROWS}")))?,
    };
    let seek = index.seek(&ch, w.from_wall_ns);
    let before_index = !matches!(seek, Seek::At(_));
    let start = match cursor_param(p.get("cursor"))? {
        Some(c @ Cursor::Seq { .. }) => c,
        Some(_) => {
            return Err(usage(
                "cursor must be the next_cursor of a previous page (seq cursor)",
            ));
        }
        None => match seek {
            Seek::At(a) | Seek::BeforeIndex(a) => Cursor::Seq {
                epoch: Some(a.epoch),
                seq: a.seq,
            },
            Seek::Empty => Cursor::Relative {
                amount: now.saturating_sub(w.from_wall_ns) / 1_000_000 + 1,
                unit: TimeUnit::Millis,
            },
        },
    };
    let page = scan_page(ctx, &ch, start, SCAN_BUDGET_BYTES).await?;
    let f = history_page(&page.events, w, usize::try_from(rows).unwrap_or(usize::MAX));
    let next = match f.stop {
        Stop::ReachedEnd => None,
        Stop::RowCap { epoch, seq } => Some(Cursor::Seq {
            epoch: Some(epoch),
            seq,
        }),
        Stop::Exhausted => page.resume.clone(),
    };
    let (oldest, dropped) = index.coverage(&ch);
    let complete = next.is_none() && page.gaps.is_empty() && !before_index;
    let at =
        |x: Option<(u64, u64)>| x.map_or(Value::Null, |(s, t)| json!({"seq": s, "wall_ms": t / 1_000_000}));
    let mut out = json!({
        "ch": ch, "now_ms": now / 1_000_000,
        "range": {"from_ms": w.from_wall_ns / 1_000_000, "to_ms": w.to_wall_ns / 1_000_000},
        "samples": f.samples, "losses": f.losses,
        "stats": {"voltage": f.v.json(), "current": f.i.json(), "power": f.p.json()},
        "first": at(f.first), "last": at(f.last),
        "next_cursor": next.map(|c| c.to_string()),
        "before_index": before_index,
        "index": {"oldest_ms": oldest.map(|a| a.wall_ns / 1_000_000), "dropped": dropped},
        "journal_gaps": page.gaps, "scanned_bytes": page.scanned_bytes, "complete": complete,
    });
    if rows > 0 {
        out["rows"] = json!(f.rows);
    }
    Ok(out)
}
