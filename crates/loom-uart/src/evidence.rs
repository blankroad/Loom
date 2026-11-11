//! Evidence verifier for TEST steps: `uart.text_contains` (UART plan §9.6.3, TEST plan §3.1).
//!
//! The predicate is re-evaluated on the original `uart.data` RX records of the window the core
//! re-read — never on an agent summary or a cited REF. Rules:
//!
//! * Only RX bytes whose record role (`payload.attributed_origin`, else `origin`) is in
//!   `filter.origins` count; the default is `["device"]` (TEST plan §3.1). TX is never evidence.
//! * RX chunks are joined into one byte stream per channel only while they stay in one
//!   `(stream epoch, producer, producer epoch, generation)` and `rx_offset` is contiguous. A gap
//!   record, a `uart.state` event, unreadable bytes or an offset jump starts a new stream, so a
//!   match never spans lost bytes.
//! * Search is a raw-byte literal search (`pattern` or `literal`). Escape sequences are not
//!   stripped: a miss over RX that contains ESC is `Inconclusive`, never `Mismatch`.
//! * A miss is `Mismatch` only over complete coverage with no loss seen in the window.
//! * `observed` carries counts, offsets and seqs only — no surrounding RX text (it may echo
//!   secrets).

use std::collections::{BTreeMap, VecDeque};

use loom_domain_api::{
    Coverage, CoverageNeed, EvalBudget, EventRef, EvidenceVerifier, Predicate, Verification,
};
use loom_proto::event::{Dir, PayloadBytes, PublishedRecord};
use serde_json::{Map, Value, json};

use crate::kinds;

/// Predicate `uart.text_contains`.
pub const PRED_TEXT_CONTAINS: &str = "uart.text_contains";
/// Predicates of the UART verifier.
pub const PREDICATES: &[&str] = &[PRED_TEXT_CONTAINS];
/// Longest literal accepted.
pub const MAX_PATTERN_BYTES: usize = 4096;
/// Loss notes kept in a reason / `observed`.
const MAX_NOTES: usize = 8;
/// Budget is charged in chunks of this many events.
const SPEND_CHUNK: usize = 256;
/// Inside a chunk the time budget is checked every this many scanned bytes.
const BYTES_PER_TIME_CHECK: usize = 4096;
const ESC: u8 = 0x1B;

/// Evidence verifier of the UART domain.
#[derive(Debug, Default)]
pub struct UartEvidence;

struct Args {
    pattern: Vec<u8>,
    channel: Option<String>,
    origins: Vec<String>,
}

fn arg_error(msg: impl std::fmt::Display) -> Verification {
    // A malformed predicate is an error, never a "false" (contract 07 §7.4).
    Verification::Inconclusive {
        reason: format!("error: invalid {PRED_TEXT_CONTAINS} args: {msg}"),
    }
}

/// Predicate arguments: the plan form `{op, pattern|literal}` arrives without `op` and with the
/// core's `filter`/`ch`; the single-key form `{"uart.text_contains": {...}}` is accepted too.
fn parse_args(p: &Predicate) -> Result<Args, String> {
    let outer = p.args.as_object().ok_or("args must be an object")?;
    let mut fields: Map<String, Value> = outer.clone();
    if let Some(inner) = fields.remove(&p.name) {
        let inner = inner
            .as_object()
            .ok_or("nested predicate args must be an object")?;
        for (k, v) in inner {
            fields.insert(k.clone(), v.clone());
        }
    }
    let mut pattern = None;
    let mut ch = None;
    let mut filter = None;
    for (k, v) in &fields {
        match k.as_str() {
            "pattern" | "literal" => {
                let s = v.as_str().ok_or_else(|| format!("{k} must be a string"))?;
                if pattern.replace(s.to_owned()).is_some() {
                    return Err("give exactly one of pattern / literal".into());
                }
            }
            "ch" => ch = Some(v.as_str().ok_or("ch must be a string")?.to_owned()),
            "filter" => filter = Some(v.as_object().ok_or("filter must be an object")?.clone()),
            "regex" | "ignore_case" => return Err(format!("{k} is not supported (literal search only)")),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let pattern = pattern.ok_or("pattern (or literal) missing")?;
    if pattern.is_empty() || pattern.len() > MAX_PATTERN_BYTES {
        return Err(format!("pattern must be 1..={MAX_PATTERN_BYTES} bytes"));
    }
    let filter = filter.unwrap_or_default();
    if let Some(d) = filter.get("domain").and_then(Value::as_str)
        && d != crate::DOMAIN
    {
        return Err(format!("filter.domain {d:?} is not uart"));
    }
    if let Some(ks) = filter.get("kinds").and_then(Value::as_array)
        && !ks.is_empty()
        && !ks.iter().any(|k| k.as_str() == Some(kinds::DATA))
    {
        return Err("filter.kinds must include uart.data".into());
    }
    let fch = filter.get("channel").and_then(Value::as_str).map(str::to_owned);
    if let (Some(a), Some(b)) = (&fch, &ch)
        && a != b
    {
        return Err(format!("filter.channel {a:?} and ch {b:?} disagree"));
    }
    let mut origins: Vec<String> = filter
        .get("origins")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if origins.is_empty() {
        origins.push("device".into());
    }
    Ok(Args {
        pattern: pattern.into_bytes(),
        channel: fch.or(ch),
        origins,
    })
}

/// Role of a record: who caused it (`attributed_origin` wins over the transport origin).
fn role(r: &PublishedRecord) -> String {
    match r.payload.get("attributed_origin").and_then(Value::as_str) {
        Some(o) => o.to_owned(),
        None => serde_json::to_value(r.origin)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    }
}

fn eref(r: &PublishedRecord) -> EventRef {
    EventRef {
        epoch: r.epoch,
        seq: r.seq,
        source: r.source_ref(),
    }
}

fn is_gap(kind: &str) -> bool {
    matches!(kind, "source.gap" | "record.gap" | "display.gap")
}

fn rx_offset(r: &PublishedRecord) -> Option<u64> {
    match r.payload.get("rx_offset")? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// Decoded RX bytes, or `None` when they are not inline / not readable / redacted.
fn rx_bytes(r: &PublishedRecord) -> Option<Vec<u8>> {
    if r.payload.get("secret").is_some() || r.quality.iter().any(|q| q == "redacted") {
        return None;
    }
    let spec: PayloadBytes = serde_json::from_value(r.payload.get("bytes")?.clone()).ok()?;
    spec.decode().ok()
}

/// Identity a joined RX stream must keep.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamKey {
    epoch: u64,
    producer: String,
    producer_epoch: u64,
    generation: Option<u32>,
}

/// State of one channel's joined stream: the KMP automaton state and, for the last
/// `pattern.len()` bytes, the owning event index and `rx_offset` of each byte.
#[derive(Debug, Default)]
struct Tail {
    key: Option<StreamKey>,
    next_offset: Option<u64>,
    /// Bytes of the pattern currently matched (KMP state).
    q: usize,
    owner: VecDeque<(usize, Option<u64>)>,
}

impl Tail {
    fn reset(&mut self) {
        *self = Tail::default();
    }
}

#[derive(Debug, Default)]
struct Stats {
    rx_chunks: u64,
    rx_bytes: u64,
    tx_skipped: u64,
    origin_skipped: u64,
    escape_seen: bool,
    loss: Vec<String>,
    lost: u64,
}

impl Stats {
    fn note(&mut self, s: String) {
        self.lost += 1;
        if self.loss.len() < MAX_NOTES {
            self.loss.push(s);
        }
    }

    fn observed(&self, a: &Args, extra: Value) -> Value {
        let mut o = json!({
            "pattern_len": a.pattern.len(),
            "rx_chunks": self.rx_chunks, "rx_bytes": self.rx_bytes,
            "tx_skipped": self.tx_skipped, "origin_skipped": self.origin_skipped,
            "origins": a.origins, "losses": self.lost, "loss": self.loss,
        });
        if let (Some(o), Value::Object(e)) = (o.as_object_mut(), extra) {
            o.extend(e);
        }
        o
    }
}

/// KMP failure table: `fail[k]` = longest proper border of `pat[..=k]`.
fn failure(pat: &[u8]) -> Vec<usize> {
    let mut fail = vec![0; pat.len()];
    let mut k = 0;
    for i in 1..pat.len() {
        while k > 0 && pat[i] != pat[k] {
            k = fail[k - 1];
        }
        if pat[i] == pat[k] {
            k += 1;
        }
        fail[i] = k;
    }
    fail
}

/// Linear streaming literal search (KMP): `O(bytes + pattern)` whatever the input.
struct Kmp<'a> {
    pat: &'a [u8],
    fail: Vec<usize>,
}

impl Kmp<'_> {
    /// Advance the state by one byte; true when the whole pattern just matched.
    fn step(&self, q: &mut usize, b: u8) -> bool {
        while *q > 0 && self.pat[*q] != b {
            *q = self.fail[*q - 1];
        }
        if self.pat[*q] == b {
            *q += 1;
        }
        if *q == self.pat.len() {
            *q = self.fail[*q - 1];
            return true;
        }
        false
    }
}

/// First match: owning event indices (stream order) and the observed summary.
struct Hit {
    owners: Vec<usize>,
    observed: Value,
}

fn search(
    a: &Args,
    events: &[PublishedRecord],
    budget: &mut EvalBudget,
) -> Result<(Stats, Option<Hit>), String> {
    let mut st = Stats::default();
    let mut tails: BTreeMap<String, Tail> = BTreeMap::new();
    let m = a.pattern.len();
    let kmp = Kmp {
        pat: &a.pattern,
        fail: failure(&a.pattern),
    };
    let time = |budget: &mut EvalBudget| budget.spend(0).map_err(|e| format!("eval_budget: {e}"));
    for (i, ev) in events.iter().enumerate() {
        if i % SPEND_CHUNK == 0 {
            budget
                .spend(SPEND_CHUNK.min(events.len() - i))
                .map_err(|e| format!("eval_budget: {e}"))?;
        }
        let ch = ev.channel.as_deref().unwrap_or("");
        if is_gap(&ev.kind) {
            // A loss of unknown channel breaks every stream.
            if ev.channel.is_none() || a.channel.as_deref().is_none_or(|c| c == ch) {
                st.note(format!("{} at seq {}", ev.kind, ev.seq.get()));
            }
            match &ev.channel {
                Some(c) => tails.entry(c.clone()).or_default().reset(),
                None => tails.values_mut().for_each(Tail::reset),
            }
            continue;
        }
        if ev.domain != crate::DOMAIN || a.channel.as_deref().is_some_and(|c| c != ch) {
            continue;
        }
        if ev.kind == kinds::STATE {
            tails.entry(ch.to_owned()).or_default().reset();
            continue;
        }
        if ev.kind != kinds::DATA {
            continue;
        }
        if ev.dir != Some(Dir::Rx) {
            st.tx_skipped += 1;
            continue;
        }
        if !a.origins.contains(&role(ev)) {
            st.origin_skipped += 1;
            continue;
        }
        let tail = tails.entry(ch.to_owned()).or_default();
        let Some(bytes) = rx_bytes(ev) else {
            st.note(format!("unreadable uart.data bytes at seq {}", ev.seq.get()));
            tail.reset();
            continue;
        };
        st.rx_chunks += 1;
        st.rx_bytes += bytes.len() as u64;
        st.escape_seen |= bytes.contains(&ESC);
        let key = StreamKey {
            epoch: ev.epoch.get(),
            producer: ev.producer_id.clone(),
            producer_epoch: ev.producer_epoch.get(),
            generation: ev.generation,
        };
        let off = rx_offset(ev);
        if tail.key.as_ref() != Some(&key) {
            tail.reset();
        } else if tail.next_offset.is_none() || off != tail.next_offset {
            if let (Some(want), Some(got)) = (tail.next_offset, off) {
                st.note(format!(
                    "rx_offset jump {want} -> {got} at seq {} (bytes missing)",
                    ev.seq.get()
                ));
            }
            tail.reset();
        }
        tail.key = Some(key);
        tail.next_offset = off.map(|o| o + bytes.len() as u64);
        let mut matched = false;
        for (k, b) in bytes.iter().enumerate() {
            if k > 0 && k % BYTES_PER_TIME_CHECK == 0 {
                time(budget)?;
            }
            if tail.owner.len() == m {
                tail.owner.pop_front();
            }
            tail.owner.push_back((i, off.map(|o| o + k as u64)));
            if kmp.step(&mut tail.q, *b) {
                matched = true;
                break;
            }
        }
        if matched {
            let mut owners: Vec<usize> = tail.owner.iter().map(|o| o.0).collect();
            owners.dedup();
            let start = tail.owner.front().and_then(|o| o.1);
            let observed = json!({
                "matched_seqs": owners.iter().map(|k| events[*k].seq).collect::<Vec<_>>(),
                "rx_offset_start": start.map(|s| s.to_string()),
                "rx_offset_end": start.map(|s| (s + m as u64).to_string()),
                "channel": ch, "generation": ev.generation,
                "producer_id": ev.producer_id, "producer_epoch": ev.producer_epoch,
                "joined": owners.len() > 1,
            });
            return Ok((st, Some(Hit { owners, observed })));
        }
    }
    Ok((st, None))
}

impl EvidenceVerifier for UartEvidence {
    fn predicates(&self) -> &[&'static str] {
        PREDICATES
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
        if pred.name != PRED_TEXT_CONTAINS {
            return Verification::Inconclusive {
                reason: format!("error: unknown predicate {}", pred.name),
            };
        }
        let a = match parse_args(pred) {
            Ok(a) => a,
            Err(e) => return arg_error(e),
        };
        let (st, hit) = match search(&a, events, budget) {
            Ok(r) => r,
            Err(reason) => return Verification::Inconclusive { reason },
        };
        if let Some(h) = hit {
            return Verification::Match {
                matched: h.owners.iter().map(|k| eref(&events[*k])).collect(),
                observed: st.observed(&a, h.observed),
            };
        }
        if !cov.complete || st.lost > 0 {
            let mut causes = cov.causes.clone();
            causes.extend(st.loss.iter().cloned());
            if causes.is_empty() {
                causes.push("coverage incomplete".into());
            }
            return Verification::Inconclusive {
                reason: format!("no match; losses in the window: {}", causes.join("; ")),
            };
        }
        if st.escape_seen {
            return Verification::Inconclusive {
                reason: "no raw-byte match; RX contains escape sequences and ANSI-stripped matching is \
                         not evaluated"
                    .into(),
            };
        }
        Verification::Mismatch {
            observed: st.observed(&a, json!({})),
            reason: format!(
                "pattern not found in {} device RX chunks ({} bytes) of the complete window",
                st.rx_chunks, st.rx_bytes
            ),
        }
    }
}
