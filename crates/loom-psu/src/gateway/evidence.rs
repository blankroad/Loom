//! Evidence predicates (`psu.current_lt`, `psu.current_gt`, `psu.voltage_between`), the
//! reducer for late-joining viewers, and the display summarizer.
//!
//! Hold = consecutive samples meeting the condition for `hold_ms`, every interval ≤
//! `max_gap_ms` (default 3 poll periods), no loss inside. If no qualifying run exists and the
//! window had a loss, a long interval or incomplete coverage, the answer is Inconclusive.

use std::collections::{BTreeMap, VecDeque};

use loom_domain_api::{
    Coverage, CoverageNeed, DisplaySummarizer, EvalBudget, EventRef, EvidenceVerifier, Predicate, Reducer,
    Verification,
};
use loom_proto::event::PublishedRecord;
use loom_proto::rpc::SubFilter;
use serde_json::{Value, json};

use super::common::{ChannelBook, DEFAULT_MAX_GAP_MS, Step, channel_of, find_hold, ref_range, series};
use crate::model::{Condition, Quantity, metric};
use crate::{KIND_MEAS, KIND_STATE};

/// Predicate names.
pub const PREDICATES: &[&str] = &["psu.current_lt", "psu.current_gt", "psu.voltage_between"];
/// Reducer measurement window.
pub const SUMMARY_WINDOW_NS: u64 = 60_000_000_000;
/// Samples kept per supply by the reducer (60 s at 5 Hz is 300).
pub const REDUCER_MAX_SAMPLES: usize = 600;
/// Budget is charged in chunks of this many events.
const SPEND_CHUNK: usize = 256;

/// Evidence verifier of the psu domain.
#[derive(Debug, Default)]
pub struct PsuEvidence;

fn num(p: &Predicate, k: &str) -> Option<f64> {
    p.args.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn condition(p: &Predicate) -> Result<Condition, String> {
    match p.name.as_str() {
        "psu.current_lt" => Ok(Condition {
            q: Quantity::Current,
            gt: None,
            lt: Some(num(p, "value").ok_or("args.value missing")?),
        }),
        "psu.current_gt" => Ok(Condition {
            q: Quantity::Current,
            gt: Some(num(p, "value").ok_or("args.value missing")?),
            lt: None,
        }),
        "psu.voltage_between" => {
            let (lo, hi) = (
                num(p, "lo").ok_or("args.lo missing")?,
                num(p, "hi").ok_or("args.hi missing")?,
            );
            if lo >= hi {
                return Err("lo must be < hi".into());
            }
            Ok(Condition {
                q: Quantity::Voltage,
                gt: Some(lo),
                lt: Some(hi),
            })
        }
        other => Err(format!("unknown predicate {other}")),
    }
}

impl EvidenceVerifier for PsuEvidence {
    fn predicates(&self) -> &[&'static str] {
        PREDICATES
    }

    fn coverage_need(&self, pred: &Predicate) -> CoverageNeed {
        match pred.name.as_str() {
            // "every sample in the window is in range" needs the whole window.
            "psu.voltage_between" if pred.args.get("hold_ms").is_none() => CoverageNeed::Complete,
            _ => CoverageNeed::Measure,
        }
    }

    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification {
        let cond = match condition(pred) {
            Ok(c) => c,
            Err(e) => {
                return Verification::Inconclusive {
                    reason: format!("{}: {e}", pred.name),
                };
            }
        };
        let ch = pred.args.get("ch").and_then(Value::as_str);
        let mut mine: Vec<PublishedRecord> = Vec::new();
        for (n, e) in events.iter().enumerate() {
            if n % SPEND_CHUNK == 0
                && let Err(b) = budget.spend(SPEND_CHUNK.min(events.len() - n))
            {
                return Verification::Inconclusive {
                    reason: format!("evaluation budget: {b}"),
                };
            }
            if ch.is_none_or(|c| channel_of(e) == c) {
                mine.push(e.clone());
            }
        }
        let steps = series(&mine, cond.q.metric());
        let refs = |a: u64, b: u64| {
            mine.iter()
                .filter(|e| e.kind == KIND_MEAS && (a..=b).contains(&e.seq.get()))
                .map(|e| EventRef {
                    epoch: e.epoch,
                    seq: e.seq,
                    source: e.source_ref(),
                })
                .collect::<Vec<_>>()
        };
        let chan = ch
            .map(str::to_owned)
            .or_else(|| mine.first().map(|e| channel_of(e).to_owned()))
            .unwrap_or_default();
        let max_gap = pred
            .args
            .get("max_gap_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_GAP_MS);
        let losses = steps.iter().filter(|s| matches!(s, Step::Loss(_))).count();
        let uncertain = |long: u64| !cov.complete || losses > 0 || long > 0;
        let why = |long: u64| {
            let mut c = cov.causes.clone();
            if losses > 0 {
                c.push(format!("{losses} losses (scpi_no_response or record gap)"));
            }
            if long > 0 {
                c.push(format!("{long} intervals > {max_gap} ms"));
            }
            if !cov.complete && c.is_empty() {
                c.push("coverage incomplete".into());
            }
            c.join("; ")
        };

        // "Every sample in range" form of voltage_between.
        if pred.name == "psu.voltage_between" && pred.args.get("hold_ms").is_none() {
            let samples: Vec<(u64, f64)> = steps
                .iter()
                .filter_map(|s| {
                    if let Step::Sample(seq, _, v) = s {
                        Some((*seq, *v))
                    } else {
                        None
                    }
                })
                .collect();
            if let Some((seq, v)) = samples.iter().find(|(_, v)| !cond.holds(*v)) {
                return Verification::Mismatch {
                    observed: json!({"seq": seq, "voltage": v, "ref": ref_range(&chan, *seq, *seq)}),
                    reason: format!("voltage {v} outside {}", cond.describe()),
                };
            }
            let (Some(a), Some(b)) = (samples.first(), samples.last()) else {
                return Verification::Inconclusive {
                    reason: "no voltage samples in the window".into(),
                };
            };
            if uncertain(0) {
                return Verification::Inconclusive {
                    reason: format!("all {} samples in range, but {}", samples.len(), why(0)),
                };
            }
            return Verification::Match {
                matched: refs(a.0, b.0),
                observed: json!({"samples": samples.len(), "ref": ref_range(&chan, a.0, b.0)}),
            };
        }

        let hold_ms = pred.args.get("hold_ms").and_then(Value::as_u64).unwrap_or(0);
        let h = find_hold(&steps, &cond, hold_ms, max_gap);
        let observed = json!({"condition": cond.describe(), "hold_ms": hold_ms, "best_ms": h.best_ms,
                              "samples": h.samples, "losses": h.losses, "last": h.last});
        match h.run {
            Some((a, b, held, n)) => Verification::Match {
                matched: refs(a, b),
                observed: json!({"condition": cond.describe(), "held_ms": held, "samples": n, "ref": ref_range(&chan, a, b)}),
            },
            None if h.samples == 0 => Verification::Inconclusive {
                reason: "no samples in the window".into(),
            },
            None if uncertain(h.long_intervals) => Verification::Inconclusive {
                reason: format!(
                    "{} not held {hold_ms} ms (best {} ms); {}",
                    cond.describe(),
                    h.best_ms,
                    why(h.long_intervals)
                ),
            },
            None => Verification::Mismatch {
                observed,
                reason: format!(
                    "{} never held for {hold_ms} ms (best {} ms)",
                    cond.describe(),
                    h.best_ms
                ),
            },
        }
    }
}

// --- reducer ----------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ChanView {
    state: Value,
    samples: VecDeque<(u64, f64, f64, Value)>, // mono, v, i, age_ms
}

/// Snapshot: latest state per supply + a 60 s measurement summary.
#[derive(Debug, Default)]
pub struct PsuReducer {
    pub(crate) book: ChannelBook,
    chans: BTreeMap<String, ChanView>,
}

impl PsuReducer {
    /// Reducer feeding the shared channel book.
    pub fn new(book: ChannelBook) -> PsuReducer {
        PsuReducer {
            book,
            chans: BTreeMap::new(),
        }
    }
}

fn stat(xs: impl Iterator<Item = f64>) -> Value {
    let v: Vec<f64> = xs.collect();
    if v.is_empty() {
        return Value::Null;
    }
    let min = v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    json!({"min": min, "max": max, "mean": v.iter().sum::<f64>() / v.len() as f64})
}

impl Reducer for PsuReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_STATE, KIND_MEAS]
    }

    fn apply(&mut self, ev: &PublishedRecord) {
        let ch = channel_of(ev).to_owned();
        if !self.chans.contains_key(&ch) && self.chans.len() >= super::common::MAX_BOOK_CHANNELS {
            return;
        }
        let view = self.chans.entry(ch.clone()).or_default();
        match ev.kind.as_str() {
            KIND_STATE => {
                view.state = ev.payload.clone();
                self.book.update(&ch, &ev.payload, ev.generation);
            }
            KIND_MEAS => {
                let (Some(v), Some(i)) = (metric(&ev.metrics, "voltage"), metric(&ev.metrics, "current"))
                else {
                    return;
                };
                let t = ev.ts.mono_ns.get();
                if view.samples.len() == REDUCER_MAX_SAMPLES {
                    view.samples.pop_front();
                }
                view.samples
                    .push_back((t, v, i, ev.payload.get("age_ms").cloned().unwrap_or(Value::Null)));
                while view
                    .samples
                    .front()
                    .is_some_and(|s| t.saturating_sub(s.0) > SUMMARY_WINDOW_NS)
                {
                    view.samples.pop_front();
                }
            }
            _ => {}
        }
    }

    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        self.chans
            .iter()
            .filter(|(ch, _)| filter.channels.is_empty() || filter.channels.iter().any(|c| c == *ch))
            .map(|(ch, v)| {
                let last = v.samples.back().map(|s| json!({"v": s.1, "i": s.2, "age_ms": s.3}));
                json!({"kind": KIND_STATE, "ch": ch, "state": v.state,
                       "meas_60s": {"samples": v.samples.len(), "voltage": stat(v.samples.iter().map(|s| s.1)),
                                    "current": stat(v.samples.iter().map(|s| s.2)), "last": last}})
            })
            .collect()
    }
}

// --- display summarizer -----------------------------------------------------------------------

/// Dropped display samples per supply: count and current range.
#[derive(Debug, Default)]
pub struct PsuSummarizer;

impl DisplaySummarizer for PsuSummarizer {
    fn summarize(&self, dropped: &[PublishedRecord]) -> Value {
        let mut per: BTreeMap<String, (u64, f64, f64)> = BTreeMap::new();
        for e in dropped.iter().filter(|e| e.kind == KIND_MEAS) {
            let Some(i) = metric(&e.metrics, "current") else {
                continue;
            };
            let ch = channel_of(e).to_owned();
            if !per.contains_key(&ch) && per.len() >= super::common::MAX_BOOK_CHANNELS {
                continue;
            }
            let x = per.entry(ch).or_insert((0, f64::INFINITY, f64::NEG_INFINITY));
            x.0 += 1;
            x.1 = x.1.min(i);
            x.2 = x.2.max(i);
        }
        let chans: Vec<Value> = per
            .into_iter()
            .map(|(ch, (n, lo, hi))| json!({"ch": ch, "samples": n, "current_min": lo, "current_max": hi}))
            .collect();
        json!({"samples": dropped.iter().filter(|e| e.kind == KIND_MEAS).count(), "channels": chans})
    }
}
