//! Evidence verifier for TEST steps: `uds.dtc_absent{mask}` (TEST plan §3.1-5, UDS opinion 3).
//!
//! Re-evaluated on the original `uds.rx` / row records of the window the core re-read — never
//! on an agent summary, the row's `dtcs` field alone or a cited REF. A response counts only if:
//!
//! * it is a non-late `uds.rx` positive `59 02 <availability> (<dtc:3> <status>)*` on the
//!   channel, whose role (`payload.attributed_origin`, else `origin`) is in `filter.origins`
//!   (default `["device"]`);
//! * a request row (`uds.tx` / `uds.pending` / `uds.result`) of the same `request_id`, channel and
//!   generation is in the window, its bytes are `19 02 <mask>` (SPRMIB bit ignored) and the
//!   request mask and the ECU availability mask both contain every bit of the predicate mask;
//! * the row's final `resp_bytes`, if present, equal the received bytes;
//! * no positive `54` (ClearDiagnosticInformation) response on the channel follows it in the
//!   window — a clear supersedes earlier lists (TC-1 S5: clear, then list).
//!
//! Verdict: any counted response listing a DTC whose status has a mask bit set → `Mismatch`
//! (final even over an incomplete window). Otherwise `Match` (witnesses = the counted responses)
//! only over complete coverage with no loss; no counted response → `Inconclusive`, never a pass.

use std::collections::BTreeMap;

use loom_domain_api::{
    Coverage, CoverageNeed, EvalBudget, EventRef, EvidenceVerifier, Predicate, Verification,
};
use loom_proto::event::{Dir, PayloadBytes, PublishedRecord};
use loom_uds_codec::response::parse_dtc_by_status_response;
use serde_json::{Map, Value, json};

use crate::{DOMAIN, KIND_PENDING, KIND_RESULT, KIND_RX, KIND_TX};

/// Predicate `uds.dtc_absent`.
pub const PRED_DTC_ABSENT: &str = "uds.dtc_absent";
/// Predicates of the UDS verifier.
pub const PREDICATES: &[&str] = &[PRED_DTC_ABSENT];
/// DTCs / notes listed in `observed` and reasons.
const MAX_LISTED: usize = 8;
/// Budget is charged in chunks of this many events.
const SPEND_CHUNK: usize = 256;
/// Later passes decode up to 64 KiB per record: time is checked every this many events.
const TIME_CHECK_EVENTS: usize = 64;
const SID_READ_DTC: u8 = 0x19;
const SUB_BY_STATUS_MASK: u8 = 0x02;
const POS_CLEAR_DTC: u8 = 0x54;

/// Evidence verifier of the UDS domain.
#[derive(Debug, Default)]
pub struct UdsEvidence;

struct Args {
    mask: u8,
    channel: Option<String>,
    origins: Vec<String>,
}

fn parse_mask(v: &Value) -> Result<u8, String> {
    let n = match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => {
            let t = s.trim();
            match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16).ok(),
                None => t.parse().ok(),
            }
        }
        _ => None,
    };
    match n.and_then(|n| u8::try_from(n).ok()) {
        Some(m) if m != 0 => Ok(m),
        _ => Err(format!("mask must be a status mask 0x01..=0xFF, got {v}")),
    }
}

/// Predicate arguments: `{op, mask}` arrives without `op` and with the core's `filter`/`ch`; the
/// single-key form `{"uds.dtc_absent": {mask}}` arrives nested.
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
    let mut mask = None;
    let mut ch = None;
    let mut filter = Map::new();
    for (k, v) in &fields {
        match k.as_str() {
            "mask" => mask = Some(parse_mask(v)?),
            "ch" => ch = Some(v.as_str().ok_or("ch must be a string")?.to_owned()),
            "filter" => filter = v.as_object().ok_or("filter must be an object")?.clone(),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let mask = mask.ok_or("mask missing")?;
    if let Some(d) = filter.get("domain").and_then(Value::as_str)
        && d != DOMAIN
    {
        return Err(format!("filter.domain {d:?} is not uds"));
    }
    if let Some(ks) = filter.get("kinds").and_then(Value::as_array)
        && !ks.is_empty()
        && !ks.iter().any(|k| k.as_str() == Some(KIND_RX))
    {
        return Err("filter.kinds must include uds.rx".into());
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
        mask,
        channel: fch.or(ch),
        origins,
    })
}

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

fn bytes_at(r: &PublishedRecord, key: &str) -> Option<Vec<u8>> {
    let spec: PayloadBytes = serde_json::from_value(r.payload.get(key)?.clone()).ok()?;
    spec.decode().ok()
}

/// What the request rows of one `request_id` say.
#[derive(Debug, Default)]
struct Req {
    channel: Option<String>,
    generation: Option<u32>,
    bytes: Option<Vec<u8>>,
    resp_bytes: Option<Vec<u8>>,
    conflict: bool,
}

#[derive(Debug, Default)]
struct Scan {
    loss: Vec<String>,
    notes: Vec<String>,
    origin_skipped: u64,
    late_skipped: u64,
    superseded: u64,
    counted: Vec<usize>,
    present: Vec<Value>,
}

impl Scan {
    fn note(&mut self, s: String) {
        if self.notes.len() < MAX_LISTED {
            self.notes.push(s);
        }
    }
}

fn scan(a: &Args, events: &[PublishedRecord], budget: &mut EvalBudget) -> Result<Scan, String> {
    let mut s = Scan::default();
    let on_channel = |r: &PublishedRecord| {
        r.domain == DOMAIN
            && a.channel
                .as_deref()
                .is_none_or(|c| r.channel.as_deref() == Some(c))
    };
    // Pass 1: request rows by request_id, gaps, the last positive clear per channel.
    let mut reqs: BTreeMap<&str, Req> = BTreeMap::new();
    for (i, ev) in events.iter().enumerate() {
        if i % SPEND_CHUNK == 0 {
            budget
                .spend(SPEND_CHUNK.min(events.len() - i))
                .map_err(|e| format!("eval_budget: {e}"))?;
        }
        if is_gap(&ev.kind) {
            if s.loss.len() < MAX_LISTED {
                s.loss.push(format!("{} at seq {}", ev.kind, ev.seq.get()));
            }
            continue;
        }
        if !on_channel(ev) || !matches!(ev.kind.as_str(), KIND_TX | KIND_PENDING | KIND_RESULT) {
            continue;
        }
        let Some(rid) = ev.request_id.as_deref() else {
            continue;
        };
        let r = reqs.entry(rid).or_default();
        let b = bytes_at(ev, "bytes");
        if r.channel.is_some() && r.channel != ev.channel
            || r.generation.is_some() && r.generation != ev.generation
            || r.bytes.is_some() && b.is_some() && r.bytes != b
        {
            r.conflict = true;
        }
        r.channel = ev.channel.clone();
        r.generation = r.generation.or(ev.generation);
        r.bytes = r.bytes.take().or(b);
        if ev.kind == KIND_RESULT {
            r.resp_bytes = bytes_at(ev, "resp_bytes");
        }
    }
    let rx_bytes = |ev: &PublishedRecord| -> Option<Vec<u8>> {
        if ev.kind != KIND_RX || ev.dir != Some(Dir::Rx) || !on_channel(ev) {
            return None;
        }
        if ev.payload.get("kind").and_then(Value::as_str) != Some("positive") {
            return None;
        }
        bytes_at(ev, "bytes")
    };
    let late = |ev: &PublishedRecord| ev.payload.get("late").and_then(Value::as_bool) != Some(false);
    let mut last_clear: BTreeMap<&str, usize> = BTreeMap::new();
    let time = |i: usize, budget: &mut EvalBudget| {
        if i.is_multiple_of(TIME_CHECK_EVENTS) {
            budget.spend(0).map_err(|e| format!("eval_budget: {e}"))
        } else {
            Ok(())
        }
    };
    for (i, ev) in events.iter().enumerate() {
        time(i, budget)?;
        if !late(ev)
            && rx_bytes(ev).is_some_and(|b| b.first() == Some(&POS_CLEAR_DTC))
            && let Some(ch) = ev.channel.as_deref()
        {
            last_clear.insert(ch, i);
        }
    }
    // Pass 2: the 19 02 responses.
    for (i, ev) in events.iter().enumerate() {
        time(i, budget)?;
        let Some(resp) = rx_bytes(ev) else { continue };
        if resp.first() != Some(&(SID_READ_DTC + 0x40)) {
            continue;
        }
        let seq = ev.seq.get();
        if late(ev) {
            s.late_skipped += 1;
            continue;
        }
        if !a.origins.contains(&role(ev)) {
            s.origin_skipped += 1;
            continue;
        }
        if last_clear
            .get(ev.channel.as_deref().unwrap_or(""))
            .is_some_and(|c| *c > i)
        {
            s.superseded += 1;
            continue;
        }
        let Some(req) = ev.request_id.as_deref().and_then(|rid| reqs.get(rid)) else {
            s.note(format!(
                "seq {seq}: no request row of its request_id in the window"
            ));
            continue;
        };
        if req.conflict || req.channel != ev.channel || req.generation != ev.generation {
            s.note(format!(
                "seq {seq}: request row channel/generation/bytes disagree"
            ));
            continue;
        }
        let Some(rq) = req.bytes.as_deref() else {
            s.note(format!("seq {seq}: request bytes unreadable"));
            continue;
        };
        if rq.len() < 3 || rq[0] != SID_READ_DTC || rq[1] & 0x7F != SUB_BY_STATUS_MASK {
            s.note(format!("seq {seq}: request is not 19 02 <mask>"));
            continue;
        }
        if rq[2] & a.mask != a.mask {
            s.note(format!(
                "seq {seq}: request mask 0x{:02X} does not include 0x{:02X}",
                rq[2], a.mask
            ));
            continue;
        }
        if req.resp_bytes.as_ref().is_some_and(|r| *r != resp) {
            s.note(format!(
                "seq {seq}: uds.result resp_bytes differ from uds.rx bytes"
            ));
            continue;
        }
        let rep = match parse_dtc_by_status_response(&resp) {
            Ok(r) => r,
            Err(e) => {
                s.note(format!("seq {seq}: malformed 59 02 response: {e}"));
                continue;
            }
        };
        if rep.availability_mask & a.mask != a.mask {
            s.note(format!(
                "seq {seq}: ECU availability mask 0x{:02X} lacks bits of 0x{:02X}",
                rep.availability_mask, a.mask
            ));
            continue;
        }
        s.counted.push(i);
        for d in rep.dtcs.iter().filter(|d| d.status & a.mask != 0) {
            if s.present.len() < MAX_LISTED {
                s.present
                    .push(json!({"dtc": format!("{:06X}", d.dtc), "status": d.status, "seq": ev.seq}));
            }
        }
    }
    Ok(s)
}

impl EvidenceVerifier for UdsEvidence {
    fn predicates(&self) -> &[&'static str] {
        PREDICATES
    }

    fn coverage_need(&self, _pred: &Predicate) -> CoverageNeed {
        CoverageNeed::Complete
    }

    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification {
        if pred.name != PRED_DTC_ABSENT {
            return Verification::Inconclusive {
                reason: format!("error: unknown predicate {}", pred.name),
            };
        }
        let a = match parse_args(pred) {
            Ok(a) => a,
            Err(e) => {
                return Verification::Inconclusive {
                    reason: format!("error: invalid {PRED_DTC_ABSENT} args: {e}"),
                };
            }
        };
        let s = match scan(&a, events, budget) {
            Ok(s) => s,
            Err(reason) => return Verification::Inconclusive { reason },
        };
        let observed = |extra: Value| {
            let mut o = json!({
                "mask": a.mask, "responses": s.counted.len(),
                "response_seqs": s.counted.iter().take(MAX_LISTED).map(|i| events[*i].seq).collect::<Vec<_>>(),
                "origin_skipped": s.origin_skipped, "late_skipped": s.late_skipped,
                "superseded_by_clear": s.superseded, "notes": s.notes, "loss": s.loss,
            });
            if let (Some(o), Value::Object(e)) = (o.as_object_mut(), extra) {
                o.extend(e);
            }
            o
        };
        if !s.present.is_empty() {
            // A listed DTC is final: losses can only hide more.
            return Verification::Mismatch {
                observed: observed(json!({"dtcs": s.present})),
                reason: format!(
                    "{} DTC(s) with status mask 0x{:02X} bits reported",
                    s.present.len(),
                    a.mask
                ),
            };
        }
        if s.counted.is_empty() {
            let mut why = vec![format!(
                "no usable 59 02 response (request mask ⊇ 0x{:02X}, origin in {:?}) in the window",
                a.mask, a.origins
            )];
            if s.origin_skipped > 0 {
                why.push(format!("{} responses of another origin", s.origin_skipped));
            }
            why.extend(s.notes.iter().cloned());
            return Verification::Inconclusive {
                reason: why.join("; "),
            };
        }
        if !cov.complete || !s.loss.is_empty() {
            let mut c = cov.causes.clone();
            c.extend(s.loss.iter().cloned());
            if c.is_empty() {
                c.push("coverage incomplete".into());
            }
            return Verification::Inconclusive {
                reason: format!("no DTC listed, but the window is incomplete: {}", c.join("; ")),
            };
        }
        Verification::Match {
            matched: s.counted.iter().map(|i| eref(&events[*i])).collect(),
            observed: observed(json!({"dtcs": []})),
        }
    }
}
