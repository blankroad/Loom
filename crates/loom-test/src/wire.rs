//! `PredicateEngine`: the production `VerifierEngine` (TEST plan §3.1, contract 07 §7.3–7.4).
//!
//! For every `expect` of the registered plan step (not only what the agent cited):
//! 1. attempt window = CORE **source fences**: per producer `(start, end]` of committed
//!    `producer_epoch/source_seq`, same stream epoch (an open attempt is checked up to a fresh
//!    fence). Domains the fence names as unflushed are `uncertain`;
//! 2. time constraint = `[anchor, anchor + within_ms]` (or start fence + `within_ms`), compared
//!    only inside one clock domain (`ts.clock_id`) — a different clock is `uncertain`;
//! 3. **re-read the whole window** from the journal (`ctx.journal().scan`, bounded: 64 MiB,
//!    2 s, 200k events) — `source.gap`/`record.gap`/`display.gap` come with it (any gap in the
//!    window makes coverage incomplete; no end fence also does);
//! 4. build `Coverage` (scan complete, gaps, scan limit) and evaluate: common predicates
//!    (`exists`, `hold` …) with `loom-test-predicate`, domain predicates (`uart.text_contains`,
//!    `eth.match` …) with the domain's registered `EvidenceVerifier`;
//! 5. check the agent's REFs only as witness candidates: unknown seq → refused, seq outside the
//!    window → `E_OUT_OF_WINDOW`. The verdict comes from the re-read only.
//!
//! Limits: no journal pin (results carry `observed.pinned:false` — verified evidence is not yet
//! protected from retention). Without a start source fence the window is unknown → `uncertain`.
//! A stimulus (`done`) citation must be a confirmed write inside the attempt's fences.

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_domain_api::{
    Coverage, CoverageNeed, EvidenceVerifier, JournalQuery, PointerSpec, Predicate, Registry, Verification,
};
use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, SeqRange, WriteState};
use loom_proto::evidence::RefString;
use loom_proto::num::U64;
use loom_proto::rpc::{Role, SubFilter};
use loom_test_format::Expectation;
use loom_test_predicate::canonical::canonical_sha256;
use loom_test_predicate::pred::{CommonVerifier, is_common, plan_pred_name};
use loom_test_predicate::verdict::{self, Proposed, RecordHealth, Reject, StepCheck, StepKind, StepVerdict};
use loom_test_predicate::{Check, Citation as PredCitation, Expr, Status};
use serde_json::{Value, json};

use crate::*;

/// Bytes one check may scan (plan §3.1-7).
pub const SCAN_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Wall time one check may spend scanning.
pub const SCAN_MAX_TIME: Duration = Duration::from_secs(2);
/// Events one check may read.
pub const SCAN_MAX_EVENTS: usize = 200_000;
/// Events per journal call.
pub const SCAN_EVENTS_PER_CALL: u32 = 10_000;
/// Read this much before the attempt start so cited seqs just before it are recognised as
/// out-of-window instead of unknown.
pub const SCAN_SLACK_MS: u64 = 1_000;
/// Look-back for anchor events of earlier steps.
pub const ANCHOR_LOOKBACK_MS: u64 = 30 * 60 * 1000;
/// Cause strings the decision maps to rejection codes.
pub const CAUSE_OUT_OF_WINDOW: &str = "out_of_window";
/// A cited REF that is not in the journal (invented or wrong filter).
pub const CAUSE_REF_UNKNOWN: &str = "ref_not_in_journal";
/// A cited REF not found while the re-read was incomplete (gap/eviction/scan limit).
pub const CAUSE_REF_UNCHECKED: &str = "ref_unchecked_incomplete_scan";

/// The production verifier engine.
#[derive(Clone, Default)]
pub struct PredicateEngine {
    domain: Vec<Arc<dyn EvidenceVerifier>>,
    common: CommonVerifier,
}

impl std::fmt::Debug for PredicateEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PredicateEngine")
            .field("domain_verifiers", &self.domain.len())
            .field("specs", &self.common.specs.len())
            .finish()
    }
}

impl PredicateEngine {
    /// Engine with the domains' verifiers and every `PointerSpec` they registered.
    pub fn new(domain: Vec<Arc<dyn EvidenceVerifier>>, specs: Vec<PointerSpec>) -> Self {
        Self {
            domain,
            common: CommonVerifier { specs },
        }
    }

    /// Collects verifiers and pointer specs from the domain registries (CORE wiring).
    pub fn from_registries(regs: &[&Registry]) -> Self {
        let domain = regs
            .iter()
            .flat_map(|r| r.evidence_verifiers().iter().cloned())
            .collect();
        let specs = regs
            .iter()
            .flat_map(|r| r.event_kinds().iter().flat_map(|k| k.pointers.iter().cloned()))
            .collect();
        Self::new(domain, specs)
    }

    fn verifier_for(&self, name: &str) -> Option<&Arc<dyn EvidenceVerifier>> {
        self.domain.iter().find(|v| v.predicates().contains(&name))
    }
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(h) => h.block_on(f),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_or_else(|_| panic!("no runtime for the verifier"), |rt| rt.block_on(f)),
    }
}

/// `r` was committed after fence `f` (per producer: newest epoch + highest committed source_seq).
fn after_fence(f: &Fence, r: &PublishedRecord) -> bool {
    let mut newer_epoch = false;
    for s in f.source_refs.iter().filter(|s| s.producer_id == r.producer_id) {
        if s.producer_epoch == r.producer_epoch {
            return r.source_seq.get() > s.source_seq.get();
        }
        newer_epoch |= s.producer_epoch.get() > r.producer_epoch.get();
    }
    // A newer epoch at the fence means this epoch had ended before it; an unknown producer
    // (or an older epoch at the fence) committed nothing of this epoch before the fence.
    !newer_epoch
}

/// Attempt window from CORE source fences: `(start, end]` per producer, same stream epoch.
#[derive(Debug, Clone, Copy)]
struct Attempt<'a> {
    start: &'a Fence,
    end: Option<&'a Fence>,
}

impl Attempt<'_> {
    fn contains(&self, r: &PublishedRecord) -> bool {
        r.epoch == self.start.stream_epoch
            && after_fence(self.start, r)
            && self.end.is_none_or(|e| !after_fence(e, r))
    }
    /// Not after the end fence (anchors of earlier steps may be before the start).
    fn not_after_end(&self, r: &PublishedRecord) -> bool {
        r.epoch == self.start.stream_epoch && self.end.is_none_or(|e| !after_fence(e, r))
    }
    fn unflushed(&self, domain: &str) -> bool {
        let named = |f: &Fence| f.unflushed_domains.iter().any(|d| d == domain);
        named(self.start) || self.end.is_some_and(named)
    }
}

/// Time constraint from `anchor`/`within_ms`, valid only inside one clock domain (`Ts::clock_id`).
#[derive(Debug, Clone)]
struct TimeWin {
    clock_id: String,
    lo: u64,
    hi: Option<u64>,
}

impl TimeWin {
    /// `Some(true/false)` when comparable, `None` when `r` uses another clock domain.
    fn contains(&self, r: &PublishedRecord) -> Option<bool> {
        if r.ts.clock_id != self.clock_id || self.clock_id.is_empty() {
            return None;
        }
        let t = r.ts.mono_ns.get();
        Some(t >= self.lo && self.hi.is_none_or(|h| t <= h))
    }
}

struct Scan {
    events: Vec<PublishedRecord>,
    complete: bool,
    causes: Vec<String>,
}

/// Bounded multi-call scan.
fn scan(
    ctx: &CheckCtx,
    filter: SubFilter,
    from: Cursor,
    count: &mut usize,
    started: Instant,
) -> Result<Scan, DomainError> {
    let mut out = Scan {
        events: Vec::new(),
        complete: true,
        causes: Vec::new(),
    };
    let mut from = from;
    let mut bytes = 0u64;
    loop {
        if started.elapsed() > SCAN_MAX_TIME {
            out.complete = false;
            out.causes.push("scan_limit(time)".into());
            return Ok(out);
        }
        let room = SCAN_MAX_EVENTS.saturating_sub(*count);
        if room == 0 {
            out.complete = false;
            out.causes.push("scan_limit(events)".into());
            return Ok(out);
        }
        let q = JournalQuery {
            from: from.clone(),
            until: None,
            filter: filter.clone(),
            request_id: None,
            max_events: SCAN_EVENTS_PER_CALL.min(u32::try_from(room).unwrap_or(u32::MAX)),
        };
        let r = block(
            ctx.journal()
                .scan(&q, SCAN_MAX_BYTES.saturating_sub(bytes).max(1)),
        )?;
        bytes = bytes.saturating_add(r.scanned_bytes);
        *count += r.events.len();
        if !r.gaps.is_empty() {
            out.complete = false;
            out.causes.push(format!("journal_gap({})", r.gaps.len()));
        }
        out.events.extend(r.events);
        match r.resume_from {
            Some(next) if bytes < SCAN_MAX_BYTES => from = next,
            Some(_) => {
                out.complete = false;
                out.causes.push("scan_limit(bytes)".into());
                return Ok(out);
            }
            None => {
                if !r.complete {
                    out.complete = false;
                    out.causes.push("journal_incomplete".into());
                }
                return Ok(out);
            }
        }
    }
}

fn ms_cursor(ms: u64) -> Cursor {
    Cursor::Relative {
        amount: ms,
        unit: TimeUnit::Millis,
    }
}

fn ref_of(r: &PublishedRecord) -> String {
    let ch = r.channel.as_deref().filter(|c| !c.is_empty()).unwrap_or("_");
    format!("{}:{ch}@e{}:seq:{}", r.domain, r.epoch.get(), r.seq.get())
}

fn role(r: &PublishedRecord) -> Origin {
    match r.payload.get("attributed_origin").and_then(Value::as_str) {
        Some("agent") => Origin::Agent,
        Some("user") => Origin::User,
        Some("script") => Origin::Script,
        _ => r.origin,
    }
}

/// Losses that make a re-read incomplete. `display.gap` counts too: the gateway's live scan reads
/// the hub ring, so records the hub (or an IPC screen mirror) dropped are missing from the scan.
fn is_gap(r: &PublishedRecord) -> bool {
    matches!(r.kind.as_str(), "source.gap" | "record.gap" | "display.gap")
}

/// Domains a re-read asks for: the expectation's domain plus every gap reporter.
fn scan_domains(domain: &str) -> Vec<String> {
    vec![domain.into(), "source".into(), "record".into(), "display".into()]
}

fn with_pinned(observed: Value) -> Value {
    // No CORE pin before R1: say so on every result. Keep within the 2 KiB contract.
    let mut o = match observed {
        Value::Object(m) => Value::Object(m),
        Value::Null => json!({}),
        other => json!({"value": other}),
    };
    o["pinned"] = json!(false);
    if serde_json::to_vec(&o).map_or(usize::MAX, |b| b.len()) > MAX_OBSERVED_BYTES {
        json!({"truncated": true, "pinned": false})
    } else {
        o
    }
}

fn check_row(
    step: &str,
    i: usize,
    status: EvidenceStatus,
    cause: Option<String>,
    observed: Value,
) -> EvidenceCheck {
    EvidenceCheck {
        evidence_id: format!("{step}#{i}"),
        expectation: u32::try_from(i).unwrap_or(u32::MAX),
        reference: String::new(),
        status,
        observed: with_pinned(observed),
        cause,
        complete: false,
        gap_in_window: false,
        digest: None,
        matched_refs: Vec::new(),
        witness: Witness::Unknown,
    }
}

fn filter_json(f: &loom_test_format::Filter) -> Value {
    let mut v = json!({"domain": f.domain, "kinds": f.kinds});
    if let Some(c) = &f.channel {
        v["channel"] = json!(c);
    }
    if let Some(r) = &f.request_id {
        v["request_id"] = json!(r);
    }
    if !f.origins.is_empty() {
        v["origins"] = json!(f.origins);
    }
    v
}

impl PredicateEngine {
    /// Anchor event: the latest match (kind, step tag, where) not after the attempt's end fence.
    /// Returns its timestamp `(clock_id, mono_ns)`.
    fn anchor_ts(
        &self,
        e: &Expectation,
        ctx: &CheckCtx,
        att: Attempt<'_>,
        count: &mut usize,
        started: Instant,
    ) -> Result<Option<(String, u64)>, String> {
        let Some(a) = &e.anchor else { return Ok(None) };
        let cond = match &a.where_clause {
            Some(w) => {
                let mut x = Expr::parse(w, "anchor.where").map_err(|e| e.to_string())?;
                x.check(&self.common.specs, "anchor.where")
                    .map_err(|e| e.to_string())?;
                Some(x)
            }
            None => None,
        };
        let filter = SubFilter {
            kinds: a.kind.iter().cloned().collect(),
            domains: a
                .kind
                .as_deref()
                .and_then(|k| k.split_once('.'))
                .map(|(d, _)| vec![d.to_owned()])
                .unwrap_or_default(),
            ..SubFilter::default()
        };
        let s = scan(ctx, filter, ms_cursor(ANCHOR_LOOKBACK_MS), count, started).map_err(|e| e.message)?;
        let mut best = None;
        for r in &s.events {
            if !att.not_after_end(r) {
                continue;
            }
            if r.test_session.as_deref().is_some_and(|x| x != ctx.session_id)
                || r.test_step.as_deref().is_some_and(|x| x != a.step)
            {
                continue;
            }
            if let Some(c) = &cond {
                let v = serde_json::to_value(r).unwrap_or(Value::Null);
                match c.eval(&v) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => return Err(format!("anchor seq={}: {e}", r.seq.get())),
                }
            }
            best = Some((r.ts.clock_id.clone(), r.ts.mono_ns.get()));
        }
        Ok(best)
    }

    #[allow(clippy::too_many_arguments)]
    fn one(
        &self,
        step: &StepSpec,
        i: usize,
        e: &Expectation,
        refs: &[&Citation],
        ctx: &mut CheckCtx,
        count: &mut usize,
        started: Instant,
    ) -> EvidenceCheck {
        let sid = step.definition.step_id.as_str();
        if e.legacy {
            return check_row(
                sid,
                i,
                EvidenceStatus::Error,
                Some("legacy UDS expect needs its adapter before verification".into()),
                json!({}),
            );
        }
        // 1. attempt window from CORE source fences (per-producer commit coverage).
        let Some(start) = &step.window.start else {
            return check_row(
                sid,
                i,
                EvidenceStatus::Uncertain,
                Some("window_unknown: no CORE source fence for this attempt".into()),
                json!({}),
            );
        };
        let att = Attempt {
            start,
            end: step.window.end.as_ref(),
        };
        if att.unflushed(&e.filter.domain) {
            return check_row(
                sid,
                i,
                EvidenceStatus::Uncertain,
                Some(format!(
                    "fence_unflushed({}): producer records may be in flight at the fence; attribution unknown",
                    e.filter.domain
                )),
                json!({}),
            );
        }
        // 2. time constraint (anchor/within) in one clock domain; the anchor may be an earlier
        //    step's event outside this attempt.
        let time = match self.anchor_ts(e, ctx, att, count, started) {
            Err(why) => return check_row(sid, i, EvidenceStatus::Error, Some(why), json!({})),
            Ok(None) if e.anchor.is_some() => {
                return check_row(
                    sid,
                    i,
                    EvidenceStatus::Uncertain,
                    Some("anchor_not_found".into()),
                    json!({}),
                );
            }
            Ok(Some((clock_id, t))) => Some(TimeWin {
                clock_id,
                lo: t,
                hi: e.within_ms.map(|w| t.saturating_add(w.saturating_mul(1_000_000))),
            }),
            Ok(None) => e.within_ms.map(|w| {
                let t = start.captured_mono_ns.get();
                TimeWin {
                    clock_id: start.clock_id.clone(),
                    lo: t,
                    hi: Some(t.saturating_add(w.saturating_mul(1_000_000))),
                }
            }),
        };
        // 3. re-read everything of the domain (and every gap reporter) from just before the
        //    attempt start; channel is filtered here so gap events (no channel) stay visible.
        let back_ms =
            ctx.now_ns.get().saturating_sub(step.window.host_started_ns.get()) / 1_000_000 + SCAN_SLACK_MS;
        let sub = SubFilter {
            domains: scan_domains(&e.filter.domain),
            ..SubFilter::default()
        };
        let s = match scan(ctx, sub, ms_cursor(back_ms), count, started) {
            Ok(s) => s,
            Err(err) => {
                return check_row(
                    sid,
                    i,
                    EvidenceStatus::Error,
                    Some(format!("journal: {}", err.message)),
                    json!({}),
                );
            }
        };
        let relevant = |r: &PublishedRecord| {
            is_gap(r)
                || (r.domain == e.filter.domain
                    && e.filter
                        .channel
                        .as_ref()
                        .is_none_or(|c| r.channel.as_ref() == Some(c)))
        };
        let in_window = |r: &PublishedRecord| -> Option<bool> {
            if !att.contains(r) {
                return Some(false);
            }
            match &time {
                None => Some(true),
                Some(t) if is_gap(r) => Some(t.contains(r).unwrap_or(true)),
                Some(t) => t.contains(r),
            }
        };
        let mut clock_mismatch = false;
        let window: Vec<PublishedRecord> = s
            .events
            .iter()
            .filter(|r| relevant(r))
            .filter(|r| match in_window(r) {
                Some(b) => b,
                None => {
                    clock_mismatch = true;
                    false
                }
            })
            .cloned()
            .collect();
        if clock_mismatch {
            return check_row(
                sid,
                i,
                EvidenceStatus::Uncertain,
                Some(
                    "clock_domain_mismatch: anchor/within time cannot be compared with the evidence clock"
                        .into(),
                ),
                json!({"time_clock": time.as_ref().map(|t| t.clock_id.clone())}),
            );
        }
        let gap_in_window = window.iter().any(is_gap);
        let (lo_seq, hi_seq) = (
            window.first().map_or(0, |r| r.seq.get()),
            window.last().map_or(0, |r| r.seq.get()),
        );
        let mut causes = s.causes.clone();
        if gap_in_window {
            causes.push("gap_in_window".into());
        }
        if att.end.is_none() {
            causes.push("window_open_no_end_fence".into());
        }
        let cov = Coverage {
            window: SeqRange {
                from: U64(lo_seq),
                to: U64(hi_seq),
            },
            complete: s.complete && !gap_in_window && att.end.is_some(),
            causes,
        };
        // 4. evaluate.
        let fj = filter_json(&e.filter);
        let name = plan_pred_name(&e.pred).unwrap_or("").to_owned();
        let mut row = if is_common(&name) {
            self.common_check(sid, i, &fj, e, &window, &cov, ctx)
        } else {
            self.domain_check(sid, i, &name, &fj, e, &window, &cov, ctx)
        };
        row.gap_in_window = gap_in_window;
        row.complete = row.complete && cov.complete;
        // 5. the agent's REFs: witness candidates only.
        for c in refs {
            let Some(r) = &c.reference else { continue };
            if row.reference.is_empty() && r.len() <= 512 && r.parse::<RefString>().is_ok() {
                row.reference = r.clone();
            }
            let Ok(rs) = r.parse::<RefString>() else {
                row.status = EvidenceStatus::Mismatch;
                row.cause = Some(CAUSE_REF_UNKNOWN.into());
                continue;
            };
            let found: Vec<&PublishedRecord> = s
                .events
                .iter()
                .filter(|x| {
                    (rs.from..=rs.to).contains(&x.seq.get())
                        && rs.epoch.is_none_or(|ep| ep == x.epoch.get())
                        && rs.domain == x.domain
                })
                .collect();
            if found.is_empty() && s.complete {
                row.status = EvidenceStatus::Mismatch;
                row.cause = Some(CAUSE_REF_UNKNOWN.into());
            } else if found.is_empty() {
                // The re-read lost part of the range: the REF may be real but cannot be checked.
                if row.status != EvidenceStatus::Mismatch {
                    row.status = EvidenceStatus::Uncertain;
                    row.cause = Some(CAUSE_REF_UNCHECKED.into());
                }
            } else if found.iter().any(|x| in_window(x) != Some(true)) {
                row.status = EvidenceStatus::Mismatch;
                row.cause = Some(CAUSE_OUT_OF_WINDOW.into());
            }
        }
        row
    }

    #[allow(clippy::too_many_arguments)]
    fn common_check(
        &self,
        sid: &str,
        i: usize,
        fj: &Value,
        e: &Expectation,
        window: &[PublishedRecord],
        cov: &Coverage,
        ctx: &mut CheckCtx,
    ) -> EvidenceCheck {
        let path = format!("steps[{sid}].expect[{i}]");
        let mut c = match PredCitation::from_plan(fj, &e.pred, &path) {
            Ok(c) => c,
            Err(err) => return check_row(sid, i, EvidenceStatus::Error, Some(err.to_string()), json!({})),
        };
        if let Err(err) = c.check(&self.common.specs, &path) {
            return check_row(sid, i, EvidenceStatus::Error, Some(err.to_string()), json!({}));
        }
        let k = c.evaluate(window, cov, &mut ctx.budget);
        let status = match k.status {
            Status::Verified => EvidenceStatus::Verified,
            Status::Mismatch => EvidenceStatus::Mismatch,
            Status::NotFound => EvidenceStatus::NotFound,
            Status::Inconclusive => EvidenceStatus::Uncertain,
            Status::Error => EvidenceStatus::Error,
        };
        let witness = match (k.status, k.need) {
            (Status::Verified, CoverageNeed::Measure) => Witness::Measurement,
            (Status::Verified, _) if k.matched_total > 0 && k.device_evidence => Witness::Device,
            (Status::Verified, _) if k.matched_total > 0 => Witness::OwnTxOnly,
            (Status::Verified, _) => Witness::CompleteAbsence,
            _ if k.cause.as_deref() == Some("agent_tx_only") => Witness::OwnTxOnly,
            _ => Witness::Unknown,
        };
        let matched_refs = k
            .matched
            .iter()
            .filter_map(|m| window.iter().find(|r| r.seq == m.seq && r.epoch == m.epoch))
            .map(ref_of)
            .collect();
        EvidenceCheck {
            evidence_id: format!("{sid}#{i}"),
            expectation: u32::try_from(i).unwrap_or(u32::MAX),
            reference: String::new(),
            status,
            observed: with_pinned(k.observed),
            cause: k.cause,
            complete: cov.complete,
            gap_in_window: false,
            digest: Some(k.digest),
            matched_refs,
            witness,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn domain_check(
        &self,
        sid: &str,
        i: usize,
        name: &str,
        fj: &Value,
        e: &Expectation,
        window: &[PublishedRecord],
        cov: &Coverage,
        ctx: &mut CheckCtx,
    ) -> EvidenceCheck {
        let Some(v) = self.verifier_for(name) else {
            return check_row(
                sid,
                i,
                EvidenceStatus::Error,
                Some(format!("no domain verifier registered for {name:?}")),
                json!({}),
            );
        };
        let mut args = e.pred.clone();
        if let Some(o) = args.as_object_mut() {
            o.remove("op");
            o.insert("filter".into(), fj.clone());
            if let Some(ch) = &e.filter.channel {
                o.entry("ch").or_insert_with(|| json!(ch));
            }
        }
        let pred = Predicate {
            name: name.to_owned(),
            args,
            expect_id: Some(format!("{sid}#{i}")),
        };
        let need = v.coverage_need(&pred);
        // The core role rule also binds domain predicates: a user write to the same channel.
        let user = window.iter().find(|r| {
            r.origin == Origin::User && r.payload.get("write_state").is_some() && r.domain == e.filter.domain
        });
        if let Some(u) = user {
            return check_row(
                sid,
                i,
                EvidenceStatus::Uncertain,
                Some("user_intervention".into()),
                json!({"user_write_seq": u.seq}),
            );
        }
        let verdict = v.verify(&pred, window, cov, &mut ctx.budget);
        let digest = canonical_sha256(&json!({"verification": verdict, "pred": pred.args, "name": name,
                                               "window": cov.window, "coverage": {"complete": cov.complete, "causes": cov.causes}}));
        let mut row = check_row(sid, i, EvidenceStatus::Error, None, json!({}));
        row.digest = Some(digest);
        row.complete = cov.complete;
        match verdict {
            Verification::Match { matched, observed } => {
                let recs: Vec<&PublishedRecord> = matched
                    .iter()
                    .filter_map(|m| window.iter().find(|r| r.seq == m.seq && r.epoch == m.epoch))
                    .collect();
                row.status = EvidenceStatus::Verified;
                row.observed = with_pinned(observed);
                row.matched_refs = recs.iter().take(MAX_CITATIONS).map(|r| ref_of(r)).collect();
                row.witness = if recs.is_empty() {
                    if need == CoverageNeed::Measure {
                        Witness::Measurement
                    } else {
                        Witness::CompleteAbsence
                    }
                } else if recs.iter().any(|r| role(r) == Origin::Device) {
                    Witness::Device
                } else {
                    Witness::OwnTxOnly
                };
                if !matched.is_empty() && recs.is_empty() {
                    // The domain matched records that are not in the re-read window.
                    row.status = EvidenceStatus::Mismatch;
                    row.cause = Some(CAUSE_OUT_OF_WINDOW.into());
                }
            }
            Verification::Mismatch { observed, reason } => {
                row.status = EvidenceStatus::Mismatch;
                row.observed = with_pinned(observed);
                row.cause = Some(reason);
            }
            Verification::Inconclusive { reason } => {
                row.status = if reason.starts_with("error:") {
                    EvidenceStatus::Error
                } else {
                    EvidenceStatus::Uncertain
                };
                row.cause = Some(reason);
            }
        }
        if row.cause.as_ref().is_some_and(|c| c.len() > MAX_TEXT_BYTES) {
            row.cause = row.cause.map(|c| preview(&c));
        }
        row
    }

    /// Stimulus-only step: a cited write REF whose final state is ≥ confirmed.
    fn stimulus(
        &self,
        step: &StepSpec,
        citations: &[Citation],
        ctx: &CheckCtx,
        count: &mut usize,
        started: Instant,
    ) -> (Vec<EvidenceCheck>, bool) {
        let sid = step.definition.step_id.as_str();
        let mut rows = Vec::new();
        let mut confirmed = false;
        for (i, c) in citations.iter().enumerate().take(MAX_CITATIONS) {
            let Some(rs) = c.reference.as_deref().and_then(|r| r.parse::<RefString>().ok()) else {
                continue;
            };
            let from = Cursor::Seq {
                epoch: rs.epoch,
                seq: rs.from,
            };
            let s = match scan(
                ctx,
                SubFilter {
                    domains: vec![rs.domain.clone()],
                    ..SubFilter::default()
                },
                from,
                count,
                started,
            ) {
                Ok(s) => s,
                Err(e) => {
                    rows.push(check_row(
                        sid,
                        i,
                        EvidenceStatus::Error,
                        Some(e.message),
                        json!({}),
                    ));
                    continue;
                }
            };
            let rows_of: Vec<&PublishedRecord> = s
                .events
                .iter()
                .filter(|r| {
                    (rs.from..=rs.to).contains(&r.seq.get()) && rs.epoch.is_none_or(|ep| ep == r.epoch.get())
                })
                .collect();
            // The confirmed write must belong to this attempt (CORE source fences), not to an
            // earlier attempt/step or another stream epoch.
            let att = step.window.start.as_ref().map(|start| Attempt {
                start,
                end: step.window.end.as_ref(),
            });
            let in_attempt =
                |r: &PublishedRecord| att.is_some_and(|a| a.contains(r) && !a.unflushed(&r.domain));
            let confirmed_row = |r: &PublishedRecord| {
                matches!(
                    r.write_state().ok().flatten(),
                    Some(
                        WriteState::Confirmed | WriteState::EffectiveVerified | WriteState::EffectUnverified
                    )
                )
            };
            let ok = rows_of.iter().any(|r| confirmed_row(r) && in_attempt(r));
            confirmed |= ok;
            let cause = if ok {
                None
            } else if rows_of.is_empty() {
                Some(CAUSE_REF_UNKNOWN.to_owned())
            } else if att.is_none() {
                Some("window_unknown: no CORE source fence for this attempt".to_owned())
            } else if att.is_some_and(|a| a.unflushed(&rs.domain)) {
                Some(format!(
                    "fence_unflushed({}): producer records may be in flight at the fence; attribution unknown",
                    rs.domain
                ))
            } else if rows_of.iter().any(|r| confirmed_row(r)) {
                Some(CAUSE_OUT_OF_WINDOW.to_owned())
            } else {
                Some("write_not_confirmed".to_owned())
            };
            let status = match cause.as_deref() {
                None => EvidenceStatus::Verified,
                Some(CAUSE_OUT_OF_WINDOW) => EvidenceStatus::Mismatch,
                Some(c) if c.starts_with("window_unknown") || c.starts_with("fence_unflushed") => {
                    EvidenceStatus::Uncertain
                }
                _ => EvidenceStatus::NotFound,
            };
            let mut row = check_row(sid, i, status, cause, json!({"rows": rows_of.len()}));
            row.reference = c.reference.clone().unwrap_or_default();
            row.matched_refs = rows_of.iter().take(4).map(|r| ref_of(r)).collect();
            row.witness = Witness::OwnTxOnly;
            row.complete = s.complete;
            row.digest = Some(canonical_sha256(
                &json!({"stimulus": row.matched_refs, "confirmed": ok}),
            ));
            rows.push(row);
        }
        (rows, confirmed)
    }
}

impl VerifierEngine for PredicateEngine {
    fn check(&self, step: &StepSpec, citations: &[Citation], ctx: &mut CheckCtx) -> CheckOutcome {
        let Some(t) = &step.template else {
            let mut o = CheckOutcome::unavailable();
            o.error = Some(err(
                ErrorCode::PlanMismatch,
                "automatic verification needs a registered plan step",
            ));
            return o;
        };
        let started = Instant::now();
        let mut count = 0usize;
        if t.expect.is_empty() {
            let (checks, confirmed) = self.stimulus(step, citations, ctx, &mut count, started);
            return CheckOutcome {
                checks,
                all_expectations_verified: false,
                stimulus_confirmed: confirmed,
                verified_iterations: u32::from(confirmed),
                error: None,
            };
        }
        let mut checks = Vec::with_capacity(t.expect.len());
        for (i, e) in t.expect.iter().enumerate().take(MAX_CITATIONS) {
            let refs: Vec<&Citation> = citations.iter().filter(|c| c.expectation as usize == i).collect();
            checks.push(self.one(step, i, e, &refs, ctx, &mut count, started));
        }
        let all = (0..t.expect.len()).all(|i| {
            checks
                .iter()
                .any(|c| c.expectation as usize == i && c.status == EvidenceStatus::Verified)
        });
        // Repeats need per-iteration windows (CORE R1 fences per iteration): one window = one iteration.
        CheckOutcome {
            checks,
            all_expectations_verified: all,
            stimulus_confirmed: false,
            verified_iterations: u32::from(all),
            error: None,
        }
    }

    fn decide(
        &self,
        step: &StepSpec,
        proposed: Verdict,
        outcome: &CheckOutcome,
        ctx: &mut CheckCtx,
    ) -> Decision {
        // A viewer's own judgment is a user assertion (separate type), not an automatic PASS.
        if ctx.user_assertion {
            return Decision::accept(proposed);
        }
        let kind = match &step.template {
            Some(t) if t.manual.is_some() => StepKind::Manual,
            Some(t) if t.expect.is_empty() => StepKind::Stimulus,
            _ => StepKind::Assert,
        };
        let p = match proposed {
            Verdict::Pass => Proposed::Pass,
            Verdict::Fail => Proposed::Fail,
            Verdict::Inconclusive => Proposed::Inconclusive("agent".into()),
            Verdict::Blocked => Proposed::Blocked("agent".into()),
            Verdict::Done => Proposed::Done,
            Verdict::Skipped if ctx.call.role() == Role::Viewer => return Decision::accept(Verdict::Skipped),
            Verdict::Skipped => Proposed::Skipped { when_unmet: false },
        };
        let checks: Vec<StepCheck> = outcome
            .checks
            .iter()
            .map(|c| StepCheck {
                check: Check {
                    status: match c.status {
                        EvidenceStatus::Verified => Status::Verified,
                        EvidenceStatus::Mismatch => Status::Mismatch,
                        EvidenceStatus::NotFound => Status::NotFound,
                        EvidenceStatus::Uncertain => Status::Inconclusive,
                        EvidenceStatus::Error => Status::Error,
                    },
                    cause: c.cause.clone(),
                    observed: Value::Null,
                    matched: Vec::new(),
                    matched_total: c.matched_refs.len() as u64,
                    device_evidence: c.witness == Witness::Device,
                    digest: c.digest.clone().unwrap_or_default(),
                    need: match c.witness {
                        Witness::CompleteAbsence => CoverageNeed::Complete,
                        Witness::Measurement => CoverageNeed::Measure,
                        _ => CoverageNeed::Positive,
                    },
                },
                plan_match: true,
                in_window: c.cause.as_deref() != Some(CAUSE_OUT_OF_WINDOW),
            })
            .collect();
        let writes = if outcome.stimulus_confirmed {
            vec![WriteState::Confirmed]
        } else {
            Vec::new()
        };
        let health = RecordHealth::Ok; // the engine guards record health before and after this call
        match verdict::judge_step(&p, kind, &checks, &writes, health) {
            Ok(v) => {
                let (eff, unsupported) = match v {
                    StepVerdict::Pass => (Verdict::Pass, false),
                    StepVerdict::Fail { unsupported } => (Verdict::Fail, unsupported),
                    StepVerdict::Inconclusive(_) => (Verdict::Inconclusive, false),
                    StepVerdict::Blocked(_) => (Verdict::Blocked, false),
                    StepVerdict::Skipped { .. } => (Verdict::Skipped, false),
                    StepVerdict::Done => (Verdict::Done, false),
                };
                let mut d = Decision::accept(eff);
                d.unsupported = unsupported;
                d
            }
            Err(r) => {
                let why = outcome
                    .checks
                    .iter()
                    .filter_map(|c| c.cause.clone())
                    .next()
                    .unwrap_or_default();
                let msg = match r {
                    Reject::EvidenceMissing => "evidence missing or not verified",
                    Reject::EvidenceMismatch => "the re-read window contradicts the verdict",
                    Reject::OutOfWindow => "cited evidence is outside the step's assertion window",
                    Reject::RecordingFailed => "recording failed",
                    Reject::PlanMismatch => "not the registered plan's expectation",
                };
                Decision::reject(
                    r.code(),
                    if why.is_empty() {
                        msg.to_owned()
                    } else {
                        format!("{msg} ({why})")
                    },
                )
            }
        }
    }
}
