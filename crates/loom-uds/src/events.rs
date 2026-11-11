//! `EventDraft` builders for the `uds.*` kinds (the core fills origin, client, ts and seq).

use loom_domain_api::EventDraft;
use loom_proto::event::{Dir, Env};
use serde::Serialize;
use serde_json::Value;

use crate::payload::{RowPayload, RxPayload, StatePayload};
use crate::{KIND_RX, KIND_STATE};

fn value(p: &impl Serialize) -> Value {
    // The payload types serialize infallibly (no maps with non-string keys, no failing impls).
    serde_json::to_value(p).unwrap_or(Value::Null)
}

/// Row event (`uds.tx`, `uds.pending`, `uds.result`) for `request_id`.
pub fn row(kind: &str, request_id: &str, generation: u32, env: Env, p: &RowPayload) -> EventDraft {
    let mut d = EventDraft::new(kind, value(p));
    d.channel = Some(p.target.clone());
    d.dir = (kind == crate::KIND_TX).then_some(Dir::Tx);
    d.request_id = Some(request_id.to_owned());
    d.generation = Some(generation);
    d.risk = Some(p.risk);
    d.env = Some(env);
    d
}

/// Raw received PDU (`uds.rx`). `request_id` = the request it answers (or the timed-out request a
/// late response belongs to); `None` when unknown.
pub fn rx(request_id: Option<&str>, generation: u32, env: Env, p: &RxPayload) -> EventDraft {
    let mut d = EventDraft::new(KIND_RX, value(p));
    d.channel = Some(p.target.clone());
    d.dir = Some(Dir::Rx);
    d.request_id = request_id.map(str::to_owned);
    d.generation = Some(generation);
    d.env = Some(env);
    d
}

/// Target state (`uds.state`).
pub fn state(env: Env, p: &StatePayload) -> EventDraft {
    let mut d = EventDraft::new(KIND_STATE, value(p));
    d.channel = Some(p.target.clone());
    d.generation = Some(p.generation);
    d.env = Some(env);
    d
}
