//! Evidence predicates for TEST steps (`eth.match`, `eth.count`, `eth.absent`, `eth.period`),
//! the reducer behind late-joining viewers, and the display summarizer.
//!
//! Rules (TEST R-8, plan §7): every event of the window is evaluated (no sampling); packets Loom
//! sent itself (`attributed_origin`) are never evidence; a loss inside the window (`source.gap`,
//! `record.gap`, unpublished packets in `eth.batch`) makes absence/count/period claims
//! Inconclusive unless the answer is already decided by what was seen.

use std::collections::{BTreeMap, VecDeque};

use loom_domain_api::{
    Coverage, CoverageNeed, DisplaySummarizer, EvalBudget, EventRef, EvidenceVerifier, Predicate, Reducer,
    Verification,
};
use loom_eth_decode::StreamTable;
use loom_eth_filter::Filter;
use loom_proto::event::PublishedRecord;
use loom_proto::rpc::SubFilter;
use serde_json::{Value, json};

use super::common::{ChannelBook, channel_of, decode_record, is_attributed, json_u64, ref_of};
use crate::model::{KIND_BATCH, KIND_CAPTURE_STATE, KIND_PKT};

/// Predicate names.
pub const PREDICATES: &[&str] = &["eth.match", "eth.count", "eth.absent", "eth.period"];
/// Matched events listed in a `Match` result.
pub const MAX_MATCHED_REFS: usize = 64;
/// Packet summaries kept by the reducer.
pub const REDUCER_LAST_PKTS: usize = 200;
/// Distinct protocols listed by the display summarizer.
pub const SUMMARY_MAX_PROTOS: usize = 32;
/// Budget is charged in chunks of this many events.
const SPEND_CHUNK: usize = 256;

/// Evidence verifier of the eth domain.
#[derive(Debug, Default)]
pub struct EthEvidence;

struct Scan {
    count: u64,
    matched: Vec<EventRef>,
    refs: Vec<String>,
    times_ns: Vec<u64>,
    loss: Vec<String>,
    excluded_own: u64,
    packets: u64,
}

fn arg_u64(p: &Predicate, k: &str) -> Option<u64> {
    p.args.get(k).and_then(Value::as_u64)
}

fn scan(
    p: &Predicate,
    filter: &Filter,
    events: &[PublishedRecord],
    budget: &mut EvalBudget,
) -> Result<Scan, String> {
    let ch = p.args.get("ch").and_then(Value::as_str);
    let want_times = p.name == "eth.period";
    let mut s = Scan {
        count: 0,
        matched: Vec::new(),
        refs: Vec::new(),
        times_ns: Vec::new(),
        loss: Vec::new(),
        excluded_own: 0,
        packets: 0,
    };
    // One stream table per channel so TCP reassembly does not mix interfaces.
    let mut streams: BTreeMap<String, StreamTable> = BTreeMap::new();
    for (i, ev) in events.iter().enumerate() {
        if i % SPEND_CHUNK == 0 {
            budget
                .spend(SPEND_CHUNK.min(events.len() - i))
                .map_err(|e| format!("evaluation budget: {e}"))?;
        }
        let ev_ch = channel_of(ev);
        if ch.is_some_and(|c| !ev_ch.is_empty() && c != ev_ch) {
            continue;
        }
        match ev.kind.as_str() {
            KIND_PKT => {}
            KIND_BATCH => {
                let total = json_u64(ev.payload.get("total")).unwrap_or(u64::MAX);
                let shown = json_u64(ev.payload.get("shown")).unwrap_or(0);
                if total > shown {
                    s.loss.push(format!(
                        "{} packets not published (seq {})",
                        total - shown,
                        ev.seq.get()
                    ));
                }
                continue;
            }
            "source.gap" | "record.gap" => {
                s.loss.push(format!("{} at seq {}", ev.kind, ev.seq.get()));
                continue;
            }
            _ => continue,
        }
        s.packets += 1;
        if is_attributed(ev) {
            s.excluded_own += 1;
            continue;
        }
        let table = streams.entry(ev_ch.to_owned()).or_default();
        if decode_record(ev, table).is_some_and(|d| filter.matches(&d)) {
            s.count += 1;
            if want_times {
                s.times_ns.push(ev.ts.mono_ns.get());
            }
            if s.matched.len() < MAX_MATCHED_REFS {
                s.matched.push(EventRef {
                    epoch: ev.epoch,
                    seq: ev.seq,
                    source: ev.source_ref(),
                });
                s.refs.push(ref_of(ev));
            }
        }
    }
    Ok(s)
}

fn observed(s: &Scan, extra: Value) -> Value {
    let mut o = json!({"count": s.count, "packets": s.packets, "excluded_own": s.excluded_own,
                       "refs": s.refs, "loss": s.loss});
    if let (Some(o), Value::Object(e)) = (o.as_object_mut(), extra) {
        o.extend(e);
    }
    o
}

impl EvidenceVerifier for EthEvidence {
    fn predicates(&self) -> &[&'static str] {
        PREDICATES
    }

    fn coverage_need(&self, pred: &Predicate) -> CoverageNeed {
        match pred.name.as_str() {
            "eth.match" => CoverageNeed::Positive,
            "eth.period" => CoverageNeed::Measure,
            _ => CoverageNeed::Complete,
        }
    }

    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification {
        let Some(expr) = pred.args.get("filter").and_then(Value::as_str) else {
            return Verification::Inconclusive {
                reason: format!("{}: args.filter missing", pred.name),
            };
        };
        let filter = match Filter::compile(expr) {
            Ok(f) => f,
            Err(e) => {
                return Verification::Inconclusive {
                    reason: format!("{}: display filter: {e}", pred.name),
                };
            }
        };
        let s = match scan(pred, &filter, events, budget) {
            Ok(s) => s,
            Err(reason) => return Verification::Inconclusive { reason },
        };
        let lossy = !cov.complete || !s.loss.is_empty();
        let loss_reason = || {
            let mut c: Vec<String> = cov.causes.clone();
            c.extend(s.loss.iter().cloned());
            if c.is_empty() {
                c.push("coverage incomplete".into());
            }
            format!("losses in the window: {}", c.join("; "))
        };
        match pred.name.as_str() {
            "eth.match" => {
                if s.count > 0 {
                    Verification::Match {
                        matched: s.matched.clone(),
                        observed: observed(&s, json!({})),
                    }
                } else if lossy {
                    Verification::Inconclusive {
                        reason: format!("no match; {}", loss_reason()),
                    }
                } else {
                    Verification::Mismatch {
                        observed: observed(&s, json!({})),
                        reason: format!("no packet matched {expr:?}"),
                    }
                }
            }
            "eth.absent" => {
                if s.count > 0 {
                    Verification::Mismatch {
                        observed: observed(&s, json!({})),
                        reason: format!("{} packets matched {expr:?}", s.count),
                    }
                } else if lossy {
                    Verification::Inconclusive {
                        reason: loss_reason(),
                    }
                } else {
                    Verification::Match {
                        matched: Vec::new(),
                        observed: observed(&s, json!({})),
                    }
                }
            }
            "eth.count" => {
                let min = arg_u64(pred, "min").unwrap_or(1);
                let max = arg_u64(pred, "max");
                let range = json!({"min": min, "max": max});
                if max.is_some_and(|m| s.count > m) {
                    // Losses can only hide more packets: over the maximum is final.
                    Verification::Mismatch {
                        observed: observed(&s, json!({"expected": range})),
                        reason: format!("{} packets, expected ≤ {}", s.count, max.unwrap_or(0)),
                    }
                } else if s.count >= min && (max.is_none() || !lossy) {
                    Verification::Match {
                        matched: s.matched.clone(),
                        observed: observed(&s, json!({"expected": range})),
                    }
                } else if lossy {
                    Verification::Inconclusive {
                        reason: format!("{} packets counted; {}", s.count, loss_reason()),
                    }
                } else {
                    Verification::Mismatch {
                        observed: observed(&s, json!({"expected": range})),
                        reason: format!("{} packets, expected ≥ {min}", s.count),
                    }
                }
            }
            "eth.period" => verify_period(pred, &s, lossy, loss_reason),
            other => Verification::Inconclusive {
                reason: format!("unknown predicate {other}"),
            },
        }
    }
}

fn verify_period(pred: &Predicate, s: &Scan, lossy: bool, loss_reason: impl Fn() -> String) -> Verification {
    let Some(period) = arg_u64(pred, "period_ms") else {
        return Verification::Inconclusive {
            reason: "eth.period: args.period_ms missing".into(),
        };
    };
    let tol = arg_u64(pred, "tol_ms").unwrap_or(period / 10);
    if s.times_ns.len() < 2 {
        return if lossy {
            Verification::Inconclusive {
                reason: format!("{} matches; {}", s.count, loss_reason()),
            }
        } else {
            Verification::Mismatch {
                observed: observed(s, json!({})),
                reason: format!("{} matches; need ≥ 2 to measure a period", s.count),
            }
        };
    }
    let (lo, hi) = (period.saturating_sub(tol) as f64, (period + tol) as f64);
    let gaps: Vec<f64> = s
        .times_ns
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]) as f64 / 1e6)
        .collect();
    let n = gaps.len() as f64;
    let mean = gaps.iter().sum::<f64>() / n;
    let min = gaps.iter().copied().fold(f64::INFINITY, f64::min);
    let max = gaps.iter().copied().fold(0.0, f64::max);
    let outside = gaps.iter().filter(|g| **g < lo || **g > hi).count();
    let obs = observed(
        s,
        json!({"intervals": gaps.len(), "mean_ms": mean, "min_ms": min, "max_ms": max,
                                 "outside_tol": outside, "period_ms": period, "tol_ms": tol}),
    );
    if outside == 0 {
        Verification::Match {
            matched: s.matched.clone(),
            observed: obs,
        }
    } else if lossy {
        // A lost packet stretches an interval; do not call it a period violation.
        Verification::Inconclusive {
            reason: format!("{outside} intervals outside {period}±{tol} ms; {}", loss_reason()),
        }
    } else {
        Verification::Mismatch {
            observed: obs,
            reason: format!("{outside} of {} intervals outside {period}±{tol} ms", gaps.len()),
        }
    }
}

// --- reducer ----------------------------------------------------------------------------------

/// Snapshot for late-joining viewers: capture state per channel + the last packets.
#[derive(Debug, Default)]
pub struct EthReducer {
    pub(crate) book: ChannelBook,
    last: VecDeque<Value>,
}

impl EthReducer {
    /// Reducer writing into the shared channel book.
    pub fn new(book: ChannelBook) -> EthReducer {
        EthReducer {
            book,
            last: VecDeque::with_capacity(REDUCER_LAST_PKTS),
        }
    }
}

impl Reducer for EthReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_CAPTURE_STATE, KIND_PKT]
    }

    fn apply(&mut self, ev: &PublishedRecord) {
        match ev.kind.as_str() {
            KIND_CAPTURE_STATE => self.book.update(channel_of(ev), &ev.payload, ev.generation),
            KIND_PKT => {
                if self.last.len() == REDUCER_LAST_PKTS {
                    self.last.pop_front();
                }
                let p = &ev.payload;
                self.last.push_back(json!({
                    "ref": ref_of(ev), "seq": ev.seq, "ch": channel_of(ev), "ts": ev.ts.wall_ns,
                    "proto": p.get("proto"), "src": p.get("src"), "dst": p.get("dst"),
                    "len": p.get("len"), "info": p.get("info"), "org": p.get("attributed_origin"),
                }));
            }
            _ => {}
        }
    }

    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        let want = |ch: &str| filter.channels.is_empty() || filter.channels.iter().any(|c| c == ch);
        let mut out: Vec<Value> = self
            .book
            .all()
            .into_iter()
            .filter(|(ch, _)| want(ch))
            .map(|(ch, i)| {
                json!({"kind": KIND_CAPTURE_STATE, "ch": ch, "state": i.state, "backend": i.backend,
                       "iface": i.iface, "generation": i.generation})
            })
            .collect();
        let pkts: Vec<&Value> = self
            .last
            .iter()
            .filter(|v| v["ch"].as_str().is_some_and(want))
            .collect();
        out.push(json!({"kind": "eth.last_packets", "rows": pkts}));
        out
    }
}

// --- display summarizer -----------------------------------------------------------------------

/// Summary of packets the display lane dropped: counts per protocol and the seq range.
#[derive(Debug, Default)]
pub struct EthSummarizer;

impl DisplaySummarizer for EthSummarizer {
    fn summarize(&self, dropped: &[PublishedRecord]) -> Value {
        let mut by: BTreeMap<String, u64> = BTreeMap::new();
        let mut other = 0u64;
        let mut bytes = 0u64;
        let mut own = 0u64;
        for ev in dropped.iter().filter(|e| e.kind == KIND_PKT) {
            let p = ev.payload.get("proto").and_then(Value::as_str).unwrap_or("?");
            if let Some(n) = by.get_mut(p) {
                *n += 1;
            } else if by.len() < SUMMARY_MAX_PROTOS {
                by.insert(p.to_owned(), 1);
            } else {
                other += 1;
            }
            bytes += ev.payload.get("len").and_then(Value::as_u64).unwrap_or(0);
            own += u64::from(is_attributed(ev));
        }
        json!({
            "packets": dropped.iter().filter(|e| e.kind == KIND_PKT).count(),
            "bytes": bytes, "own": own, "by_proto": by, "other": other,
            "first_seq": dropped.first().map(|e| e.seq), "last_seq": dropped.last().map(|e| e.seq),
        })
    }
}
