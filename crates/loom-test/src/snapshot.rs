use crate::*;
use loom_domain_api::Reducer;
use loom_proto::{event::PublishedRecord, num::U64, rpc::SubFilter};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
/// Compact display rows never carry the full verifier witness arrays/history; the journal does.
pub(crate) fn evidence_row(p: &Value) -> Value {
    let observed = if json_len(&p["observed"], MAX_OBSERVED_BYTES).is_ok() {
        p["observed"].clone()
    } else {
        json!("[observed summary exceeds snapshot limit]")
    };
    json!({"step_id":p["step_id"].as_str().map(preview),"evidence_id":p["evidence_id"].as_str().map(preview),"ref":p["ref"].as_str().map(preview),"status":p["status"].as_str().map(preview),"observed":observed,"cause":p["cause"].as_str().map(preview),"complete":p["complete"].as_bool(),"gap_in_window":p["gap_in_window"].as_bool(),"digest":p["digest"].as_str().map(preview),"matched_ref_count":p["matched_refs"].as_array().map_or(0,Vec::len)})
}
pub(crate) fn verdict_row(p: &Value) -> Value {
    if p.is_null() {
        return Value::Null;
    }
    json!({"proposed":p["proposed"].as_str().map(preview),"effective":p["effective"].as_str().map(preview),"by":p["by"].as_str().map(preview),"reason":p["reason"].as_str().map(preview),"rejected":if p["rejected"].is_null(){Value::Null}else{json!({"code":p["rejected"]["code"].as_str().map(preview),"message":p["rejected"]["message"].as_str().map(preview)})},"override":p["override"].as_bool(),"user_assertion":p["user_assertion"].as_bool()})
}
fn step_row(p: &Value) -> Value {
    json!({"step_id":p["step_id"].as_str().map(preview),"n":p["n"].as_u64(),"title":p["title"].as_str().map(preview),"status":p["status"].as_str().map(preview).unwrap_or_else(||"pending".into()),"timeout_ms":u64_display(&p["timeout_ms"])})
}
fn u64_display(v: &Value) -> Value {
    v.as_str()
        .and_then(|s| loom_proto::num::parse_u64_canonical(s).ok())
        .map(|n| json!(U64(n)))
        .unwrap_or(Value::Null)
}
/// Bounded late-join state is folded from published test.* records, never RPC-only side data.
#[derive(Debug, Default)]
pub struct TestReducer {
    sessions: BTreeMap<String, Value>,
    order: VecDeque<String>,
    last: Option<(U64, U64)>,
}
impl Reducer for TestReducer {
    fn kinds(&self) -> &[&'static str] {
        gateway::EVENT_KINDS
    }
    fn apply(&mut self, ev: &PublishedRecord) {
        if ev.domain != DOMAIN {
            return;
        }
        if self
            .last
            .is_some_and(|(e, s)| ev.epoch < e || (ev.epoch == e && ev.seq <= s))
        {
            return;
        }
        let p = &ev.payload;
        let Some(id) = p
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= MAX_ID_BYTES)
        else {
            return;
        };
        if json_len(p, MAX_EVENT_BYTES).is_err() {
            return;
        }
        self.last = Some((ev.epoch, ev.seq));
        if ev.kind == "test.session_started" {
            if !self.sessions.contains_key(id) {
                if self.order.len() >= MAX_RETAINED_SESSIONS + MAX_ACTIVE_SESSIONS
                    && let Some(old) = self.order.pop_front()
                {
                    self.sessions.remove(&old);
                }
                self.order.push_back(id.into());
            }
            let steps = p
                .get("steps")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(loom_test_format::MAX_STEPS)
                        .map(step_row)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            self.sessions.insert(id.into(),json!({"session_id":id,"title":p["title"].as_str().map(preview),"state":p["state"].as_str().map(preview),"revision":u64_display(&p["revision"]),"current_step":null,"steps":steps,"recent_evidence":[],"asks":[],"result":null}));
            return;
        }
        let Some(s) = self.sessions.get_mut(id) else {
            return;
        };
        match ev.kind.as_str() {
            "test.session_state" => {
                s["state"] = json!(p["state"].as_str().map(preview));
                s["revision"] = u64_display(&p["revision"]);
                if p["plan_changed"] == true {
                    s["steps"] = json!(
                        p["steps"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .take(loom_test_format::MAX_STEPS)
                            .map(step_row)
                            .collect::<Vec<_>>()
                    );
                    s["recent_evidence"] = json!([]);
                    s["current_step"] = Value::Null;
                    s["asks"] = json!([]);
                }
            }
            "test.session_ended" => {
                s["state"] = json!(p["state"].as_str().map(preview));
                s["result"] = json!(p["result"].as_str().map(preview));
                s["current_step"] = Value::Null;
                s["asks"] = json!([]);
            }
            "test.step_started" | "test.step_ended" | "test.verdict" => {
                let Some(step) = p
                    .get("step_id")
                    .and_then(Value::as_str)
                    .filter(|s| s.len() <= MAX_ID_BYTES)
                else {
                    return;
                };
                if ev.kind == "test.step_started"
                    && let Some(items) = s.get_mut("steps").and_then(Value::as_array_mut)
                    && !items.iter().any(|x| x["step_id"] == step)
                    && items.len() < loom_test_format::MAX_STEPS
                {
                    items.push(step_row(p));
                }
                if let Some(items) = s.get_mut("steps").and_then(Value::as_array_mut)
                    && let Some(d) = items.iter_mut().find(|x| x["step_id"] == step)
                {
                    match ev.kind.as_str() {
                        "test.step_started" => {
                            d["status"] = json!("running");
                            d["attempt"] = json!(p["attempt"].as_u64().filter(|n| *n <= u32::MAX as u64));
                            d["verdict"] = Value::Null;
                            d["accepted"] = Value::Null;
                        }
                        "test.step_ended" => d["status"] = json!("ended"),
                        _ => {
                            d["verdict"] = verdict_row(p);
                            if p["rejected"].is_null() {
                                d["accepted"] = verdict_row(p);
                            }
                        }
                    }
                }
                if ev.kind == "test.step_started" {
                    s["current_step"] = json!(step);
                } else if ev.kind == "test.step_ended" && s["current_step"] == step {
                    s["current_step"] = Value::Null;
                }
            }
            "test.evidence_checked" => {
                if let Some(a) = s.get_mut("recent_evidence").and_then(Value::as_array_mut) {
                    if a.len() >= MAX_RECENT_EVIDENCE {
                        a.remove(0);
                    }
                    a.push(evidence_row(p));
                }
            }
            "test.ask" => {
                if let Some(a) = s.get_mut("asks").and_then(Value::as_array_mut)
                    && a.len() < MAX_OPEN_ASKS
                    && json_len(p, MAX_TEXT_BYTES * 3).is_ok()
                {
                    a.push(p.clone());
                }
            }
            "test.answer" => {
                if let Some(a) = s.get_mut("asks").and_then(Value::as_array_mut) {
                    a.retain(|a| a["ask_id"] != p["ask_id"]);
                }
            }
            _ => (),
        }
    }
    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        if !filter.domains.is_empty() && !filter.domains.iter().any(|d| d == DOMAIN) {
            return Vec::new();
        }
        let chosen = self
            .order
            .iter()
            .rev()
            .filter_map(|id| self.sessions.get(id))
            .find(|s| s["state"] != "DONE" && s["state"] != "STOPPED")
            .or_else(|| self.order.back().and_then(|id| self.sessions.get(id)));
        chosen.map(|state|vec![json!({"domain":DOMAIN,"state":state,"sessions":self.order.iter().filter_map(|id|self.sessions.get(id)).map(|s|json!({"session_id":s["session_id"],"state":s["state"]})).collect::<Vec<_>>()})]).unwrap_or_default()
    }
}
