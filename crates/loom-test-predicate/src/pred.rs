//! Common predicates (contract 07 §7.3-5, TEST plan §3.1): `exists`, `absent`, `count`,
//! `order`, `within_ms`, `hold`, `measure` over a citation `{filter, pred}`.
//!
//! The caller (CORE session engine) re-reads the **whole** window from the journal and passes
//! every record plus the journal's `Coverage`; witnesses are outputs, never inputs (R4 P1-1).
//!
//! Rules applied here:
//! - coverage: `Complete`/`Measure` predicates are `Inconclusive` when the coverage is
//!   incomplete, a `source.gap`/`record.gap` of the channel is in the window, or the stream
//!   epoch changes inside it. `Positive` predicates still verify on a match.
//! - roles: observation origins default to `["device"]`; a record's `payload.attributed_origin`
//!   (Loom's own frame seen on the wire, eth) counts as that origin; only own sends → `agent_tx_only`.
//! - `request_id` in the filter pairs request/response; matches of other requests are reported
//!   as `request_id_mismatch`, never counted.
//! - a user write (origin user, `payload.write_state`) to the same domain·channel·target in the
//!   window → `Inconclusive{"user_intervention"}`.
//! - a missing field / wrong type → `Error` (never false).
//! - `EvalBudget` is charged per chunk; over → `Inconclusive{"eval_budget"}`.

use loom_domain_api::{
    Coverage, CoverageNeed, EvalBudget, EventRef, EvidenceVerifier, PointerSpec, Predicate, Verification,
};
use loom_proto::event::{Origin, PublishedRecord};
use serde_json::{Map, Value, json};

use crate::ast::{EvalError, Expr, PredError, perr};
use crate::canonical::canonical_sha256;
use crate::limits::*;
use crate::render;
use crate::units::{duration_ms, json_num};

/// Which records a citation looks at.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// Domain (`None` = any).
    pub domain: Option<String>,
    /// Channel (`None` = any).
    pub channel: Option<String>,
    /// Kinds (empty = any).
    pub kinds: Vec<String>,
    /// Request pairing.
    pub request_id: Option<String>,
    /// Allowed observation origins (default `["device"]`).
    pub origins: Vec<Origin>,
    /// Target inside the channel (ECU address …), compared with `payload.target` for the
    /// user-intervention rule.
    pub target: Option<String>,
}

/// Anchor of `within_ms`.
#[derive(Debug, Clone)]
pub enum Anchor {
    /// Monotonic time given by the engine (e.g. an earlier step's event).
    MonoNs(u64),
    /// First record in the window matching this (any origin: a stimulus may be the anchor).
    Where(Expr),
}

/// A common predicate.
#[derive(Debug, Clone)]
pub enum Pred {
    /// A matching observation exists.
    Exists {
        /// Condition.
        cond: Expr,
    },
    /// No matching observation in the whole window.
    Absent {
        /// Condition.
        cond: Expr,
    },
    /// `min ≤ n ≤ max` matching observations.
    Count {
        /// Condition.
        cond: Expr,
        /// Minimum.
        min: u64,
        /// Maximum.
        max: Option<u64>,
    },
    /// A match of `a` followed by a match of `b`.
    Order {
        /// First.
        a: Expr,
        /// Then.
        b: Expr,
    },
    /// A match within `max_ms` after the anchor.
    WithinMs {
        /// Anchor.
        anchor: Anchor,
        /// Step id of the anchor (rendering only).
        anchor_step: Option<String>,
        /// Bound.
        max_ms: u64,
        /// Condition.
        cond: Expr,
        /// Clock uncertainty (ms): a result this close to the bound is inconclusive.
        tolerance_ms: u64,
    },
    /// Consecutive samples meeting `cond` for `min_ms` (same source·generation, gaps ≤ max).
    Hold {
        /// Which records are samples of the signal.
        sample: Expr,
        /// Condition on each sample.
        cond: Expr,
        /// Required duration.
        min_ms: u64,
        /// Largest interval between samples.
        max_gap_ms: u64,
    },
    /// Time from the first `from` (any origin) to the next `to` observation.
    Measure {
        /// Start.
        from: Expr,
        /// End.
        to: Expr,
        /// Lower bound.
        min_ms: Option<u64>,
        /// Upper bound.
        max_ms: Option<u64>,
        /// Clock uncertainty.
        tolerance_ms: u64,
    },
}

/// A parsed citation template / check.
#[derive(Debug, Clone)]
pub struct Citation {
    /// Filter.
    pub filter: Filter,
    /// Predicate.
    pub pred: Pred,
    /// Input as given (canonical form goes into digests).
    pub raw: Value,
}

/// Check status (contract 07 §7.3-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Predicate holds with the evidence.
    Verified,
    /// Evidence violates it.
    Mismatch,
    /// Nothing matching in a complete window.
    NotFound,
    /// Cannot decide (coverage, user intervention, budget, clock).
    Inconclusive,
    /// Missing field / wrong type / bad citation.
    Error,
}

impl Status {
    /// Wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Verified => "verified",
            Status::Mismatch => "mismatch",
            Status::NotFound => "not_found",
            Status::Inconclusive => "inconclusive",
            Status::Error => "error",
        }
    }
}

/// Result of one check.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// Status.
    pub status: Status,
    /// Cause for non-verified results (`user_intervention`, `eval_budget`, `agent_tx_only` …).
    pub cause: Option<String>,
    /// Observed values (≤ 2 KiB).
    pub observed: Value,
    /// Matched records (≤ 64).
    pub matched: Vec<EventRef>,
    /// All matches counted.
    pub matched_total: u64,
    /// At least one matched record is a device observation.
    pub device_evidence: bool,
    /// SHA-256 (canonical JSON) of the matched set, or of expect·window·filter·coverage.
    pub digest: String,
    /// Coverage class of the predicate.
    pub need: CoverageNeed,
}

impl Check {
    /// As the CORE verifier result.
    pub fn verification(&self) -> Verification {
        match self.status {
            Status::Verified => Verification::Match {
                matched: self.matched.clone(),
                observed: self.observed.clone(),
            },
            Status::Mismatch | Status::NotFound => Verification::Mismatch {
                observed: self.observed.clone(),
                reason: self.cause.clone().unwrap_or_else(|| self.status.as_str().into()),
            },
            Status::Inconclusive => Verification::Inconclusive {
                reason: self.cause.clone().unwrap_or_else(|| "inconclusive".into()),
            },
            Status::Error => Verification::Inconclusive {
                reason: format!("error: {}", self.cause.clone().unwrap_or_default()),
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing.

fn obj<'a>(v: &'a Value, path: &str) -> Result<&'a Map<String, Value>, PredError> {
    v.as_object()
        .ok_or_else(|| perr(path, "object", v, "not an object"))
}

fn known_keys(m: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), PredError> {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(perr(
                &format!("{path}.{k}"),
                &allowed.join(" | "),
                &json!(k),
                "unknown key",
            ));
        }
    }
    Ok(())
}

fn opt_str(m: &Map<String, Value>, k: &str, path: &str) -> Result<Option<String>, PredError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= MAX_STRING_LEN => Ok(Some(s.clone())),
        Some(v) => Err(perr(&format!("{path}.{k}"), "string", v, "not a string")),
    }
}

fn parse_origin(v: &Value, path: &str) -> Result<Origin, PredError> {
    match v.as_str() {
        Some("device") => Ok(Origin::Device),
        Some("agent") => Ok(Origin::Agent),
        Some("user") => Ok(Origin::User),
        Some("script") => Ok(Origin::Script),
        _ => Err(perr(path, "device | agent | user | script", v, "unknown origin")),
    }
}

impl Filter {
    /// Parses `{domain, channel, kinds[], request_id?, origins[]?, target?}`.
    pub fn parse(v: &Value, path: &str) -> Result<Filter, PredError> {
        let m = obj(v, path)?;
        known_keys(
            m,
            &["domain", "channel", "kinds", "request_id", "origins", "target"],
            path,
        )?;
        let kinds = match m.get("kinds") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= MAX_IN_ITEMS => a
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    k.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| perr(&format!("{path}.kinds[{i}]"), "kind string", k, "not a string"))
                })
                .collect::<Result<_, _>>()?,
            Some(other) => {
                return Err(perr(
                    &format!("{path}.kinds"),
                    "array of kinds",
                    other,
                    "not a list",
                ));
            }
        };
        let origins = match m.get("origins") {
            None | Some(Value::Null) => vec![Origin::Device],
            Some(Value::Array(a)) if !a.is_empty() && a.len() <= 4 => a
                .iter()
                .enumerate()
                .map(|(i, o)| parse_origin(o, &format!("{path}.origins[{i}]")))
                .collect::<Result<_, _>>()?,
            Some(other) => {
                return Err(perr(
                    &format!("{path}.origins"),
                    "non-empty list of origins",
                    other,
                    "bad origins",
                ));
            }
        };
        Ok(Filter {
            domain: opt_str(m, "domain", path)?,
            channel: opt_str(m, "channel", path)?,
            kinds,
            request_id: opt_str(m, "request_id", path)?,
            origins,
            target: opt_str(m, "target", path)?,
        })
    }

    fn in_scope(&self, r: &PublishedRecord) -> bool {
        self.domain.as_deref().is_none_or(|d| r.domain == d)
            && self
                .channel
                .as_deref()
                .is_none_or(|c| r.channel.as_deref() == Some(c))
            && (self.kinds.is_empty() || self.kinds.contains(&r.kind))
    }
}

fn expr_at(m: &Map<String, Value>, k: &str, path: &str, required: bool) -> Result<Expr, PredError> {
    match m.get(k) {
        Some(v) => Expr::parse(v, &format!("{path}.{k}")),
        None if required => Err(perr(
            path,
            &format!("`{k}`"),
            &Value::Object(m.clone()),
            format!("missing {k}"),
        )),
        None => Ok(Expr::All(Vec::new())),
    }
}

fn dur_at(m: &Map<String, Value>, k: &str, path: &str) -> Result<Option<u64>, PredError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => duration_ms(v)
            .map(Some)
            .map_err(|e| perr(&format!("{path}.{k}"), "ms or \"5 s\"", v, e)),
    }
}

fn count_at(m: &Map<String, Value>, k: &str, path: &str) -> Result<Option<u64>, PredError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => json_num(v)
            .and_then(|n| {
                u64::try_from(match n {
                    crate::units::Num::Int(i) => i,
                    crate::units::Num::Float(_) => -1,
                })
                .ok()
            })
            .map(Some)
            .ok_or_else(|| perr(&format!("{path}.{k}"), "non-negative integer", v, "bad count")),
    }
}

/// Names of the common predicates (bare and `test.`-prefixed are accepted).
pub const PRED_NAMES: &[&str] = &[
    "exists",
    "absent",
    "count",
    "order",
    "within_ms",
    "hold",
    "measure",
];

impl Pred {
    /// Parses one predicate by name from its argument object.
    pub fn parse(name: &str, args: &Value, path: &str) -> Result<Pred, PredError> {
        let name = name.strip_prefix("test.").unwrap_or(name);
        let empty = Map::new();
        let m = match args {
            Value::Null => &empty,
            v => obj(v, path)?,
        };
        let keys: &[&str] = match name {
            "exists" | "absent" => &["where", "filter"],
            "count" => &["where", "min", "max", "filter"],
            "order" => &["a", "b", "filter"],
            "within_ms" => &["anchor", "max", "where", "tolerance_ms", "filter"],
            "hold" => &["sample", "where", "min_ms", "max_gap_ms", "filter"],
            "measure" => &["from", "to", "min_ms", "max_ms", "tolerance_ms", "filter"],
            other => {
                return Err(perr(
                    path,
                    &PRED_NAMES.join(" | "),
                    &json!(other),
                    "unknown predicate (domain predicates go to the domain verifier)",
                ));
            }
        };
        known_keys(m, keys, path)?;
        Ok(match name {
            "exists" => Pred::Exists {
                cond: expr_at(m, "where", path, false)?,
            },
            "absent" => Pred::Absent {
                cond: expr_at(m, "where", path, false)?,
            },
            "count" => {
                let min = count_at(m, "min", path)?;
                let max = count_at(m, "max", path)?;
                if min.is_none() && max.is_none() {
                    return Err(perr(path, "`min` and/or `max`", args, "count needs a bound"));
                }
                let min = min.unwrap_or(0);
                if max.is_some_and(|x| x < min) {
                    return Err(perr(path, "min ≤ max", args, "bad count range"));
                }
                Pred::Count {
                    cond: expr_at(m, "where", path, false)?,
                    min,
                    max,
                }
            }
            "order" => Pred::Order {
                a: expr_at(m, "a", path, true)?,
                b: expr_at(m, "b", path, true)?,
            },
            "within_ms" => {
                let ap = format!("{path}.anchor");
                let av = m
                    .get("anchor")
                    .ok_or_else(|| perr(path, "`anchor`", args, "missing anchor"))?;
                let am = obj(av, &ap)?;
                known_keys(am, &["mono_ns", "where", "step"], &ap)?;
                let anchor = match (am.get("mono_ns"), am.get("where")) {
                    (Some(t), None) => Anchor::MonoNs(
                        json_num(t)
                            .and_then(|n| match n {
                                crate::units::Num::Int(i) => u64::try_from(i).ok(),
                                crate::units::Num::Float(_) => None,
                            })
                            .ok_or_else(|| perr(&format!("{ap}.mono_ns"), "U64", t, "bad time"))?,
                    ),
                    (None, Some(w)) => Anchor::Where(Expr::parse(w, &format!("{ap}.where"))?),
                    _ => return Err(perr(&ap, "exactly one of mono_ns | where", av, "bad anchor")),
                };
                Pred::WithinMs {
                    anchor,
                    anchor_step: opt_str(am, "step", &ap)?,
                    max_ms: dur_at(m, "max", path)?
                        .ok_or_else(|| perr(path, "`max`", args, "missing max"))?,
                    cond: expr_at(m, "where", path, false)?,
                    tolerance_ms: dur_at(m, "tolerance_ms", path)?.unwrap_or(0),
                }
            }
            "hold" => Pred::Hold {
                sample: expr_at(m, "sample", path, false)?,
                cond: expr_at(m, "where", path, true)?,
                min_ms: dur_at(m, "min_ms", path)?
                    .ok_or_else(|| perr(path, "`min_ms`", args, "missing min_ms"))?,
                max_gap_ms: dur_at(m, "max_gap_ms", path)?.unwrap_or(DEFAULT_MAX_GAP_MS),
            },
            _ => {
                let (min_ms, max_ms) = (dur_at(m, "min_ms", path)?, dur_at(m, "max_ms", path)?);
                if let (Some(a), Some(b)) = (min_ms, max_ms)
                    && a > b
                {
                    return Err(perr(path, "min_ms ≤ max_ms", args, "bad bounds"));
                }
                Pred::Measure {
                    from: expr_at(m, "from", path, true)?,
                    to: expr_at(m, "to", path, true)?,
                    min_ms,
                    max_ms,
                    tolerance_ms: dur_at(m, "tolerance_ms", path)?.unwrap_or(0),
                }
            }
        })
    }

    /// Coverage class (contract 07 §7.3-3).
    pub fn need(&self) -> CoverageNeed {
        match self {
            Pred::Exists { .. } | Pred::Count { max: None, .. } => CoverageNeed::Positive,
            Pred::Absent { .. } | Pred::Count { .. } | Pred::Order { .. } | Pred::Hold { .. } => {
                CoverageNeed::Complete
            }
            Pred::WithinMs { .. } | Pred::Measure { .. } => CoverageNeed::Measure,
        }
    }

    fn exprs_mut(&mut self) -> Vec<(&'static str, &mut Expr)> {
        match self {
            Pred::Exists { cond } | Pred::Absent { cond } | Pred::Count { cond, .. } => vec![("where", cond)],
            Pred::Order { a, b } => vec![("a", a), ("b", b)],
            Pred::WithinMs { anchor, cond, .. } => {
                let mut v = vec![("where", cond)];
                if let Anchor::Where(e) = anchor {
                    v.push(("anchor.where", e));
                }
                v
            }
            Pred::Hold { sample, cond, .. } => vec![("sample", sample), ("where", cond)],
            Pred::Measure { from, to, .. } => vec![("from", from), ("to", to)],
        }
    }
}

impl Citation {
    /// Parses the plan form `{filter, pred: {<name>: {args}}}`.
    pub fn parse(v: &Value, path: &str) -> Result<Citation, PredError> {
        let m = obj(v, path)?;
        known_keys(m, &["filter", "pred", "id"], path)?;
        let filter = match m.get("filter") {
            Some(f) => Filter::parse(f, &format!("{path}.filter"))?,
            None => return Err(perr(path, "`filter`", v, "missing filter")),
        };
        let pv = m
            .get("pred")
            .ok_or_else(|| perr(path, "`pred`", v, "missing pred"))?;
        let pm = obj(pv, &format!("{path}.pred"))?;
        let [(name, args)] = pm.iter().collect::<Vec<_>>()[..] else {
            return Err(perr(
                &format!("{path}.pred"),
                "exactly one predicate",
                pv,
                "bad pred",
            ));
        };
        let pred = Pred::parse(name, args, &format!("{path}.pred.{name}"))?;
        Ok(Citation {
            filter,
            pred,
            raw: v.clone(),
        })
    }

    /// From a plan `expect` entry: `filter` object and `pred` in the plan form
    /// `{op: <name>, …args}` (also accepts `{<name>: {…args}}`). Only common predicates; use
    /// [`is_common`] first and send domain predicates to the domain verifier.
    pub fn from_plan(filter: &Value, pred: &Value, path: &str) -> Result<Citation, PredError> {
        let f = Filter::parse(filter, &format!("{path}.filter"))?;
        let pp = format!("{path}.pred");
        let pm = obj(pred, &pp)?;
        let (name, args) = match pm.get("op") {
            Some(Value::String(op)) => {
                let mut rest = pm.clone();
                rest.remove("op");
                (op.clone(), Value::Object(rest))
            }
            Some(other) => return Err(perr(&format!("{pp}.op"), "predicate name", other, "not a string")),
            None => match pm.iter().collect::<Vec<_>>()[..] {
                [(n, a)] => (n.clone(), a.clone()),
                _ => return Err(perr(&pp, "{op: <name>, …} or {<name>: {…}}", pred, "bad pred")),
            },
        };
        let p = Pred::parse(&name, &args, &format!("{pp}.{name}"))?;
        Ok(Citation {
            filter: f,
            pred: p,
            raw: json!({"filter": filter, "pred": pred}),
        })
    }

    /// From the CORE `Predicate{name: "test.exists", args: {filter, where, …}}`.
    pub fn from_predicate(p: &Predicate) -> Result<Citation, PredError> {
        let path = format!("pred[{}]", p.name);
        let fv = p
            .args
            .get("filter")
            .ok_or_else(|| perr(&path, "args.filter", &p.args, "missing filter"))?;
        let filter = Filter::parse(fv, &format!("{path}.filter"))?;
        let pred = Pred::parse(&p.name, &p.args, &path)?;
        Ok(Citation {
            filter,
            pred,
            raw: json!({"name": p.name, "args": p.args}),
        })
    }

    /// Type-checks every expression against the domain's fields (+ record builtins).
    pub fn check(&mut self, specs: &[PointerSpec], path: &str) -> Result<(), PredError> {
        let name = self.name();
        for (k, e) in self.pred.exprs_mut() {
            e.check(specs, &format!("{path}.pred.{name}.{k}"))?;
        }
        Ok(())
    }

    /// Predicate name.
    pub fn name(&self) -> &'static str {
        match self.pred {
            Pred::Exists { .. } => "exists",
            Pred::Absent { .. } => "absent",
            Pred::Count { .. } => "count",
            Pred::Order { .. } => "order",
            Pred::WithinMs { .. } => "within_ms",
            Pred::Hold { .. } => "hold",
            Pred::Measure { .. } => "measure",
        }
    }

    /// Korean sentence for the approval screen.
    pub fn render(&self) -> String {
        let f = &self.filter;
        let mut scope = Vec::new();
        if let Some(d) = &f.domain {
            scope.push(d.clone());
        }
        if let Some(c) = &f.channel {
            scope.push(c.clone());
        }
        if !f.kinds.is_empty() {
            scope.push(f.kinds.join("|"));
        }
        let scope = if scope.is_empty() {
            "모든 이벤트".to_owned()
        } else {
            scope.join(" ")
        };
        let who = if f.origins == [Origin::Device] {
            String::new()
        } else {
            format!(
                " (출처 {})",
                f.origins
                    .iter()
                    .map(|o| format!("{o:?}").to_lowercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let req = f
            .request_id
            .as_ref()
            .map(|r| format!(" 요청 {r}의"))
            .unwrap_or_default();
        let c = |e: &Expr| render::expr(e);
        match &self.pred {
            Pred::Exists { cond } => format!("{scope}{req}{who}에서 {} 이벤트가 있음", c(cond)),
            Pred::Absent { cond } => format!("{scope}{req}{who}에서 {} 이벤트가 창 전체에 없음", c(cond)),
            Pred::Count { cond, min, max } => {
                let n = match max {
                    Some(x) if x == min => format!("정확히 {min}개"),
                    Some(x) => format!("{min}~{x}개"),
                    None => format!("{min}개 이상"),
                };
                format!("{scope}{req}{who}에서 {} 이벤트가 {n}", c(cond))
            }
            Pred::Order { a, b } => format!("{scope}{who}에서 {} 다음에 {}", c(a), c(b)),
            Pred::WithinMs {
                anchor,
                anchor_step,
                max_ms,
                cond,
                ..
            } => {
                let base = match (anchor_step, anchor) {
                    (Some(s), _) => format!("{s} 매치"),
                    (None, Anchor::Where(e)) => c(e),
                    (None, Anchor::MonoNs(_)) => "기준 시각".to_owned(),
                };
                format!(
                    "{base} 후 {} 안에 {scope}{who}에서 {}",
                    render::ms(*max_ms),
                    c(cond)
                )
            }
            Pred::Hold {
                cond,
                min_ms,
                max_gap_ms,
                ..
            } => format!(
                "{scope}{who}에서 {}{} {} 동안 유지(같은 출처·세대, 샘플 간격 ≤ {})",
                c(cond),
                render::josa(&c(cond), "이", "가"),
                render::ms(*min_ms),
                render::ms(*max_gap_ms)
            ),
            Pred::Measure {
                from,
                to,
                min_ms,
                max_ms,
                ..
            } => {
                let b = match (min_ms, max_ms) {
                    (Some(a), Some(z)) => format!(" — {} 이상 {} 이하", render::ms(*a), render::ms(*z)),
                    (None, Some(z)) => format!(" — {} 이하", render::ms(*z)),
                    (Some(a), None) => format!(" — {} 이상", render::ms(*a)),
                    (None, None) => String::new(),
                };
                format!("{}부터 {}까지 시간 측정{b}", c(from), c(to))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Evaluation.

struct Rec<'a> {
    r: &'a PublishedRecord,
    v: Value,
}

impl Rec<'_> {
    fn seq(&self) -> u64 {
        self.r.seq.get()
    }
    fn t(&self) -> u64 {
        self.r.ts.mono_ns.get()
    }
    /// Origin for role checks: Loom's own frame on the wire counts as the sender's origin.
    fn role(&self) -> Origin {
        match self.r.payload.get("attributed_origin").and_then(Value::as_str) {
            Some("agent") => Origin::Agent,
            Some("user") => Origin::User,
            Some("script") => Origin::Script,
            _ => self.r.origin,
        }
    }
    fn eref(&self) -> EventRef {
        EventRef {
            epoch: self.r.epoch,
            seq: self.r.seq,
            source: self.r.source_ref(),
        }
    }
}

fn eval_err(rec: &Rec<'_>, e: &EvalError) -> String {
    format!("seq={} {e}", rec.seq())
}

struct Ctx<'a> {
    cit: &'a Citation,
    cov: &'a Coverage,
    all: Vec<Rec<'a>>,
}

enum Out {
    Done(Status, Option<String>, Value, Vec<usize>, u64),
}

fn done(s: Status, cause: Option<&str>, observed: Value, matched: Vec<usize>, total: u64) -> Out {
    Out::Done(s, cause.map(str::to_owned), observed, matched, total)
}

fn bounded(v: Value) -> Value {
    let n = serde_json::to_vec(&v).map_or(0, |b| b.len());
    if n <= MAX_OBSERVED_BYTES {
        v
    } else {
        json!({"truncated": true, "bytes": n})
    }
}

impl Citation {
    /// Evaluates over the whole window. Never panics; budget, coverage and role rules applied.
    pub fn evaluate(&self, events: &[PublishedRecord], cov: &Coverage, budget: &mut EvalBudget) -> Check {
        let need = self.pred.need();
        if events.len() > MAX_EVAL_EVENTS {
            return self.finish(
                done(
                    Status::Inconclusive,
                    Some("eval_budget"),
                    json!({"events": events.len()}),
                    vec![],
                    0,
                ),
                need,
                cov,
                &[],
            );
        }
        let mut all = Vec::with_capacity(events.len());
        for (i, r) in events.iter().enumerate() {
            if i % BUDGET_CHUNK == 0 && budget.spend(BUDGET_CHUNK.min(events.len() - i)).is_err() {
                return self.finish(
                    done(
                        Status::Inconclusive,
                        Some("eval_budget"),
                        json!({"evaluated": i}),
                        vec![],
                        0,
                    ),
                    need,
                    cov,
                    &[],
                );
            }
            all.push(Rec {
                r,
                v: serde_json::to_value(r).unwrap_or(Value::Null),
            });
        }
        let cx = Ctx { cit: self, cov, all };
        let out = cx.run(need);
        let recs = cx.all;
        self.finish(out, need, cov, &recs)
    }

    fn finish(&self, out: Out, need: CoverageNeed, cov: &Coverage, recs: &[Rec<'_>]) -> Check {
        let Out::Done(status, cause, observed, matched, total) = out;
        let device_evidence = matched.iter().any(|&i| recs[i].role() == Origin::Device);
        let refs: Vec<EventRef> = matched
            .iter()
            .take(MAX_MATCHED_REFS)
            .map(|&i| recs[i].eref())
            .collect();
        let digest = if status == Status::Verified && !refs.is_empty() {
            canonical_sha256(&json!({"kind": "match", "citation": self.raw, "matched": refs}))
        } else {
            canonical_sha256(
                &json!({"kind": status.as_str(), "citation": self.raw, "window": cov.window,
                                      "coverage": {"complete": cov.complete, "causes": cov.causes}, "cause": cause}),
            )
        };
        Check {
            status,
            cause,
            observed: bounded(observed),
            matched: refs,
            matched_total: total,
            device_evidence,
            digest,
            need,
        }
    }
}

impl Ctx<'_> {
    fn f(&self) -> &Filter {
        &self.cit.filter
    }

    /// Loss / epoch problems in the window that make Complete/Measure claims uncertain.
    fn coverage_problems(&self) -> Vec<String> {
        let mut out: Vec<String> = if self.cov.complete {
            vec![]
        } else if self.cov.causes.is_empty() {
            vec!["coverage_incomplete".into()]
        } else {
            self.cov.causes.clone()
        };
        let ch = self.f().channel.as_deref();
        let gaps = self
            .all
            .iter()
            .filter(|x| matches!(x.r.kind.as_str(), "source.gap" | "record.gap"))
            .filter(|x| ch.is_none() || x.r.channel.is_none() || x.r.channel.as_deref() == ch)
            .count();
        if gaps > 0 {
            out.push(format!("gap_in_window({gaps})"));
        }
        let mut epochs: Vec<u64> = self.all.iter().map(|x| x.r.epoch.get()).collect();
        epochs.sort_unstable();
        epochs.dedup();
        if epochs.len() > 1 {
            out.push("epoch_changed".into());
        }
        out
    }

    fn user_intervention(&self) -> Option<u64> {
        let f = self.f();
        self.all
            .iter()
            .find(|x| {
                x.r.origin == Origin::User
                    && x.r.payload.get("write_state").is_some()
                    && f.domain.as_deref().is_none_or(|d| x.r.domain == d)
                    && f.channel
                        .as_deref()
                        .is_none_or(|c| x.r.channel.as_deref() == Some(c))
                    && f.target.as_deref().is_none_or(|t| {
                        x.r.payload
                            .get("target")
                            .and_then(Value::as_str)
                            .is_none_or(|pt| pt == t)
                    })
            })
            .map(Rec::seq)
    }

    /// Records of the filter (domain/channel/kinds), split by role and request pairing.
    fn candidates(&self) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let f = self.f();
        let (mut ok, mut own, mut other_req) = (vec![], vec![], vec![]);
        for (i, x) in self.all.iter().enumerate() {
            if !f.in_scope(x.r) {
                continue;
            }
            if let Some(rq) = &f.request_id
                && x.r.request_id.as_deref() != Some(rq.as_str())
            {
                other_req.push(i);
                continue;
            }
            if f.origins.contains(&x.role()) {
                ok.push(i);
            } else {
                own.push(i);
            }
        }
        (ok, own, other_req)
    }

    fn matches(&self, e: &Expr, idx: &[usize]) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for &i in idx {
            match e.eval(&self.all[i].v) {
                Ok(true) => out.push(i),
                Ok(false) => {}
                Err(err) => return Err(eval_err(&self.all[i], &err)),
            }
        }
        Ok(out)
    }

    /// Matches among records that failed only the role/request checks (for diagnostics; a
    /// type error there is not the citation's error).
    fn soft_matches(&self, e: &Expr, idx: &[usize]) -> usize {
        idx.iter()
            .filter(|&&i| e.eval(&self.all[i].v).unwrap_or(false))
            .count()
    }

    fn not_found(
        &self,
        cond: &Expr,
        own: &[usize],
        other: &[usize],
        positive_only: bool,
        problems: &[String],
    ) -> Out {
        if self.soft_matches(cond, own) > 0 {
            return done(
                Status::Mismatch,
                Some("agent_tx_only"),
                json!({"own_matches": self.soft_matches(cond, own)}),
                vec![],
                0,
            );
        }
        if self.soft_matches(cond, other) > 0 {
            return done(
                Status::Mismatch,
                Some("request_id_mismatch"),
                json!({"other_request_matches": self.soft_matches(cond, other)}),
                vec![],
                0,
            );
        }
        if positive_only && !problems.is_empty() {
            return done(
                Status::Inconclusive,
                Some(&problems.join("; ")),
                json!({}),
                vec![],
                0,
            );
        }
        done(
            Status::NotFound,
            Some("not_found"),
            json!({"candidates": 0}),
            vec![],
            0,
        )
    }

    fn run(&self, need: CoverageNeed) -> Out {
        let problems = self.coverage_problems();
        if need != CoverageNeed::Positive && !problems.is_empty() {
            return done(
                Status::Inconclusive,
                Some(&problems.join("; ")),
                json!({"problems": problems}),
                vec![],
                0,
            );
        }
        if let Some(seq) = self.user_intervention() {
            return done(
                Status::Inconclusive,
                Some("user_intervention"),
                json!({"user_write_seq": seq.to_string()}),
                vec![],
                0,
            );
        }
        let (ok, own, other) = self.candidates();
        macro_rules! m {
            ($e:expr, $idx:expr) => {
                match self.matches($e, $idx) {
                    Ok(v) => v,
                    Err(why) => return done(Status::Error, Some(&why), json!({}), vec![], 0),
                }
            };
        }
        match &self.cit.pred {
            Pred::Exists { cond } => {
                let hits = m!(cond, &ok);
                match hits.first() {
                    Some(&first) => {
                        let total = hits.len() as u64;
                        done(
                            Status::Verified,
                            None,
                            json!({"first_seq": self.all[first].seq().to_string(), "matches": total}),
                            hits,
                            total,
                        )
                    }
                    None => self.not_found(cond, &own, &other, true, &problems),
                }
            }
            Pred::Absent { cond } => {
                let hits = m!(cond, &ok);
                let total = hits.len() as u64;
                if hits.is_empty() {
                    done(
                        Status::Verified,
                        None,
                        json!({"matches": 0, "candidates": ok.len()}),
                        vec![],
                        0,
                    )
                } else {
                    let first = self.all[hits[0]].seq().to_string();
                    done(
                        Status::Mismatch,
                        Some("present"),
                        json!({"matches": total, "first_seq": first}),
                        hits,
                        total,
                    )
                }
            }
            Pred::Count { cond, min, max } => {
                let hits = m!(cond, &ok);
                let n = hits.len() as u64;
                let obs = json!({"count": n, "min": min, "max": max});
                if n >= *min && max.is_none_or(|x| n <= x) {
                    done(Status::Verified, None, obs, hits, n)
                } else if n < *min && max.is_none() && !problems.is_empty() {
                    done(Status::Inconclusive, Some(&problems.join("; ")), obs, vec![], n)
                } else if n == 0 && *min > 0 {
                    match self.not_found(cond, &own, &other, false, &problems) {
                        Out::Done(s, c, _, _, _) if s == Status::Mismatch => Out::Done(s, c, obs, vec![], 0),
                        _ => done(Status::Mismatch, Some("count_out_of_range"), obs, vec![], 0),
                    }
                } else {
                    done(Status::Mismatch, Some("count_out_of_range"), obs, hits, n)
                }
            }
            Pred::Order { a, b } => {
                let ha = m!(a, &ok);
                let hb = m!(b, &ok);
                let first_a = ha.first().map(|&i| self.all[i].seq());
                match first_a.and_then(|sa| hb.iter().copied().find(|&i| self.all[i].seq() > sa)) {
                    Some(bi) => {
                        let ai = ha[0];
                        done(
                            Status::Verified,
                            None,
                            json!({"a_seq": self.all[ai].seq().to_string(), "b_seq": self.all[bi].seq().to_string()}),
                            vec![ai, bi],
                            2,
                        )
                    }
                    None => done(
                        Status::Mismatch,
                        Some(if first_a.is_none() {
                            "a_not_found"
                        } else {
                            "b_not_after_a"
                        }),
                        json!({"a_matches": ha.len(), "b_matches": hb.len()}),
                        vec![],
                        0,
                    ),
                }
            }
            Pred::WithinMs {
                anchor,
                max_ms,
                cond,
                tolerance_ms,
                ..
            } => {
                let all_idx: Vec<usize> = (0..self.all.len()).collect();
                let t0 = match anchor {
                    Anchor::MonoNs(t) => *t,
                    Anchor::Where(e) => match m!(e, &all_idx).first() {
                        Some(&i) => self.all[i].t(),
                        None => {
                            return done(
                                Status::Inconclusive,
                                Some("anchor_not_found"),
                                json!({}),
                                vec![],
                                0,
                            );
                        }
                    },
                };
                let hits = m!(cond, &ok);
                match hits.iter().copied().find(|&i| self.all[i].t() >= t0) {
                    Some(i) => {
                        let dt = (self.all[i].t() - t0) / 1_000_000;
                        let obs =
                            json!({"delta_ms": dt, "max_ms": max_ms, "seq": self.all[i].seq().to_string()});
                        if dt.abs_diff(*max_ms) <= *tolerance_ms && *tolerance_ms > 0 {
                            done(Status::Inconclusive, Some("clock_uncertainty"), obs, vec![i], 1)
                        } else if dt <= *max_ms {
                            done(Status::Verified, None, obs, vec![i], 1)
                        } else {
                            done(Status::Mismatch, Some("too_late"), obs, vec![i], 1)
                        }
                    }
                    None => self.not_found(cond, &own, &other, false, &problems),
                }
            }
            Pred::Measure {
                from,
                to,
                min_ms,
                max_ms,
                tolerance_ms,
            } => {
                let all_idx: Vec<usize> = (0..self.all.len()).collect();
                let Some(&fi) = m!(from, &all_idx).first() else {
                    return done(Status::NotFound, Some("from_not_found"), json!({}), vec![], 0);
                };
                let t0 = self.all[fi].t();
                let hits = m!(to, &ok);
                let Some(ti) = hits.iter().copied().find(|&i| self.all[i].t() >= t0 && i != fi) else {
                    return match self.not_found(to, &own, &other, false, &problems) {
                        Out::Done(Status::NotFound, ..) => {
                            done(Status::NotFound, Some("to_not_found"), json!({}), vec![], 0)
                        }
                        o => o,
                    };
                };
                let dt_ns = self.all[ti].t() - t0;
                let dt = dt_ns as f64 / 1e6;
                let obs = json!({"ms": dt, "from_seq": self.all[fi].seq().to_string(), "to_seq": self.all[ti].seq().to_string()});
                let near = |b: u64| (dt - b as f64).abs() <= *tolerance_ms as f64 && *tolerance_ms > 0;
                if min_ms.is_some_and(near) || max_ms.is_some_and(near) {
                    return done(
                        Status::Inconclusive,
                        Some("clock_uncertainty"),
                        obs,
                        vec![fi, ti],
                        2,
                    );
                }
                if min_ms.is_some_and(|b| dt < b as f64) || max_ms.is_some_and(|b| dt > b as f64) {
                    done(Status::Mismatch, Some("out_of_bounds"), obs, vec![fi, ti], 2)
                } else {
                    done(Status::Verified, None, obs, vec![fi, ti], 2)
                }
            }
            Pred::Hold {
                sample,
                cond,
                min_ms,
                max_gap_ms,
            } => self.hold(sample, cond, *min_ms, *max_gap_ms, &ok),
        }
    }

    /// Sampled hold (spike `v2-evidence`): consecutive samples of one source and generation,
    /// strictly increasing time, intervals ≤ `max_gap_ms`; any break restarts the run and makes a
    /// negative answer uncertain. Not a proof of continuity between samples.
    fn hold(&self, sample: &Expr, cond: &Expr, min_ms: u64, max_gap_ms: u64, ok: &[usize]) -> Out {
        let samples = match self.matches(sample, ok) {
            Ok(v) => v,
            Err(why) => return done(Status::Error, Some(&why), json!({}), vec![], 0),
        };
        let gap_ns = max_gap_ms.saturating_mul(1_000_000);
        let mut run: Vec<usize> = Vec::new();
        let mut prev: Option<usize> = None;
        let mut uncertain = Vec::new();
        let mut best = 0u64;
        for &i in &samples {
            let x = &self.all[i];
            if let Some(p) = prev {
                let y = &self.all[p];
                let broken = if x.r.generation != y.r.generation {
                    Some("generation_changed")
                } else if x.r.producer_id != y.r.producer_id || x.r.producer_epoch != y.r.producer_epoch {
                    Some("source_changed")
                } else if x.t() <= y.t() || x.t() - y.t() > gap_ns {
                    Some("stale_sample")
                } else {
                    None
                };
                if let Some(why) = broken {
                    run.clear();
                    if !uncertain.contains(&why) {
                        uncertain.push(why);
                    }
                }
            }
            prev = Some(i);
            match cond.eval(&x.v) {
                Err(e) => return done(Status::Error, Some(&eval_err(x, &e)), json!({}), vec![], 0),
                Ok(false) => run.clear(),
                Ok(true) => {
                    run.push(i);
                    let held = (x.t() - self.all[run[0]].t()) / 1_000_000;
                    best = best.max(held);
                    if held >= min_ms {
                        let first = self.all[run[0]].seq().to_string();
                        let n = run.len() as u64;
                        return done(
                            Status::Verified,
                            None,
                            json!({"held_ms": held, "from_seq": first, "to_seq": x.seq().to_string(), "samples": n, "max_gap_ms": max_gap_ms}),
                            run,
                            n,
                        );
                    }
                }
            }
        }
        let obs = json!({"best_ms": best, "min_ms": min_ms, "samples": samples.len()});
        if uncertain.is_empty() {
            done(Status::Mismatch, Some("hold_too_short"), obs, vec![], 0)
        } else {
            done(Status::Inconclusive, Some(&uncertain.join("; ")), obs, vec![], 0)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// CORE adapter.

/// The common predicates as a CORE `EvidenceVerifier` (`test.exists` …), registered by the CORE
/// `test` module. `specs` = every `PointerSpec` the domains registered (for units/types).
#[derive(Debug, Clone, Default)]
pub struct CommonVerifier {
    /// Field specs used to type-check citations before evaluation.
    pub specs: Vec<PointerSpec>,
}

/// Names registered by [`CommonVerifier`].
pub const VERIFIER_NAMES: &[&str] = &[
    "test.exists",
    "test.absent",
    "test.count",
    "test.order",
    "test.within_ms",
    "test.hold",
    "test.measure",
];

impl EvidenceVerifier for CommonVerifier {
    fn predicates(&self) -> &[&'static str] {
        VERIFIER_NAMES
    }

    fn coverage_need(&self, pred: &Predicate) -> CoverageNeed {
        // Unparseable → the strictest class; verify() reports the error.
        Citation::from_predicate(pred).map_or(CoverageNeed::Complete, |c| c.pred.need())
    }

    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification {
        let mut c = match Citation::from_predicate(pred) {
            Ok(c) => c,
            Err(e) => {
                return Verification::Inconclusive {
                    reason: format!("error: {e}"),
                };
            }
        };
        if let Err(e) = c.check(&self.specs, &format!("pred[{}]", pred.name)) {
            return Verification::Inconclusive {
                reason: format!("error: {e}"),
            };
        }
        c.evaluate(events, cov, budget).verification()
    }
}

/// True for a common predicate name (`exists`, `test.hold` …); anything else (`uart.text_contains`,
/// `eth.match` …) belongs to a domain verifier.
pub fn is_common(name: &str) -> bool {
    PRED_NAMES.contains(&name.strip_prefix("test.").unwrap_or(name))
}

/// Name of a plan `pred` (`{op: <name>}` or `{<name>: {…}}`).
pub fn plan_pred_name(pred: &Value) -> Option<&str> {
    let m = pred.as_object()?;
    match m.get("op") {
        Some(Value::String(s)) => Some(s.as_str()),
        Some(_) => None,
        None if m.len() == 1 => m.keys().next().map(String::as_str),
        None => None,
    }
}
