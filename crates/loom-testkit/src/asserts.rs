//! Contract assertions. Each `check_*` returns a readable error; `assert_*` panics with it.

use loom_proto::event::{GapCount, GapData, PublishedRecord, WriteState, kinds, payload_write_state};

fn rank(s: WriteState) -> u8 {
    match s {
        WriteState::Requested => 0,
        WriteState::Accepted => 1,
        WriteState::Sent => 2,
        WriteState::Confirmed => 3,
        WriteState::EffectiveVerified | WriteState::EffectUnverified => 4,
        WriteState::Failed | WriteState::OutcomeUnknown => 5,
    }
}

fn terminal(s: WriteState) -> bool {
    matches!(
        s,
        WriteState::Failed
            | WriteState::OutcomeUnknown
            | WriteState::EffectiveVerified
            | WriteState::EffectUnverified
    )
}

/// Rows of one request: same `request_id`, every row has `payload.write_state`, stream seq
/// increasing, states only move forward (`requested → accepted → sent → confirmed →
/// effective_verified|effect_unverified`; `failed`/`outcome_unknown` may end it at any point),
/// nothing after a terminal state (A14, E8).
pub fn check_write_rows(rows: &[PublishedRecord]) -> Result<(), String> {
    let first = rows.first().ok_or("no rows")?;
    let rid = first.request_id.as_deref().ok_or("row without request_id (E8)")?;
    let mut prev: Option<(u64, WriteState)> = None;
    for (i, r) in rows.iter().enumerate() {
        if r.request_id.as_deref() != Some(rid) {
            return Err(format!("row {i}: request_id {:?} != {rid:?}", r.request_id));
        }
        let st = payload_write_state(&r.payload)
            .map_err(|e| format!("row {i}: {e}"))?
            .ok_or_else(|| format!("row {i} ({}): missing payload.write_state", r.kind))?;
        if let Some((pseq, pst)) = prev {
            if r.seq.get() <= pseq {
                return Err(format!("row {i}: seq {} not after {pseq}", r.seq));
            }
            if terminal(pst) {
                return Err(format!("row {i}: {st:?} after terminal {pst:?}"));
            }
            if rank(st) < rank(pst) {
                return Err(format!("row {i}: {st:?} goes backwards from {pst:?}"));
            }
        }
        prev = Some((r.seq.get(), st));
    }
    Ok(())
}

/// Panicking form of [`check_write_rows`].
pub fn assert_write_rows(rows: &[PublishedRecord]) {
    if let Err(e) = check_write_rows(rows) {
        panic!("write rows violate the contract: {e}");
    }
}

/// One observed item on a client connection, in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceItem {
    /// An event delivered to the client.
    Event(Box<PublishedRecord>),
    /// The RPC reply for a write request.
    Reply {
        /// Request id of the write.
        request_id: String,
    },
}

/// Shape check of publish-before-reply (E7) on a client trace: every reply's request has at least
/// one row delivered **before** the reply, and no row of that request arrives after it.
pub fn check_publish_before_reply(trace: &[TraceItem]) -> Result<(), String> {
    for (i, item) in trace.iter().enumerate() {
        let TraceItem::Reply { request_id } = item else {
            continue;
        };
        let is_row =
            |t: &TraceItem| matches!(t, TraceItem::Event(e) if e.request_id.as_deref() == Some(request_id));
        if !trace[..i].iter().any(is_row) {
            return Err(format!("reply for {request_id:?} at {i} has no row before it"));
        }
        if let Some(j) = trace[i + 1..].iter().position(is_row) {
            return Err(format!(
                "row of {request_id:?} at {} arrived after its reply",
                i + 1 + j
            ));
        }
    }
    Ok(())
}

/// Panicking form of [`check_publish_before_reply`].
pub fn assert_publish_before_reply(trace: &[TraceItem]) {
    if let Err(e) = check_publish_before_reply(trace) {
        panic!("publish-before-reply violated: {e}");
    }
}

/// Gap events are honest (A13): payload parses as `GapData`, a cause is given, ranges are ordered,
/// an exact count fits its stream range, `display.gap` names a range, and stream seqs increase.
pub fn check_no_gap_lies(events: &[PublishedRecord]) -> Result<(), String> {
    let mut last = None;
    for e in events {
        if let Some(p) = last
            && e.seq.get() <= p
        {
            return Err(format!("seq {} not after {p}", e.seq));
        }
        last = Some(e.seq.get());
        if ![kinds::DISPLAY_GAP, kinds::RECORD_GAP, kinds::SOURCE_GAP].contains(&e.kind.as_str()) {
            continue;
        }
        let g: GapData = serde_json::from_value(e.payload.clone())
            .map_err(|x| format!("{} seq {}: {x}", e.kind, e.seq))?;
        if g.cause.is_empty() {
            return Err(format!("{} seq {}: empty cause", e.kind, e.seq));
        }
        if let Some(r) = &g.seq_range {
            if r.from > r.to {
                return Err(format!("{} seq {}: seq_range reversed", e.kind, e.seq));
            }
            if let GapCount::Exact(n) = g.count
                && n.get() > r.to.get() - r.from.get() + 1
            {
                return Err(format!("{} seq {}: count {n} exceeds range size", e.kind, e.seq));
            }
        }
        if let Some(r) = &g.producer_range
            && r.from > r.to
        {
            return Err(format!("{} seq {}: producer_range reversed", e.kind, e.seq));
        }
        if e.kind == kinds::DISPLAY_GAP && g.seq_range.is_none() && g.producer_range.is_none() {
            return Err(format!("display.gap seq {}: no range", e.seq));
        }
    }
    Ok(())
}

/// Panicking form of [`check_no_gap_lies`].
pub fn assert_no_gap_lies(events: &[PublishedRecord]) {
    if let Err(e) = check_no_gap_lies(events) {
        panic!("gap reporting is not honest: {e}");
    }
}
