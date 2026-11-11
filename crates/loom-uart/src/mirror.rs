//! Channel mirror: the gateway's view of each UART channel, rebuilt from the domain's own
//! events. Serves two jobs:
//! - **snapshot** for late-joining viewers (`core.snapshot_part`, plan §5.2);
//! - **presend check** of the link generation in the write hooks (plan §10.2).
//!
//! The worker's InputArbiter stays the authority for hold/dirty/lock; the mirror only shows them.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use loom_domain_api::Reducer;
use loom_proto::event::PublishedRecord;
use loom_proto::rpc::SubFilter;
use serde_json::{Map, Value, json};

use crate::{MAX_PORTS, kinds};

/// Mirrored state of one channel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelView {
    /// Link generation (`generation` of the last `uart.state`).
    pub generation: Option<u32>,
    /// Last `uart.state` payload.
    pub state: Value,
    /// Last `uart.config` payload.
    pub config: Value,
    /// Last `uart.input_owner` payload.
    pub input_owner: Value,
    /// Last `uart.lock` payload.
    pub lock: Value,
    /// Last `uart.agent_activity` payload.
    pub agent_activity: Value,
}

/// Shared mirror (reducer writes, hooks read). Bounded: at most [`MAX_CHANNELS`] channels.
#[derive(Debug, Clone, Default)]
pub struct Mirror(Arc<RwLock<BTreeMap<String, ChannelView>>>);

/// Channel entries kept (ports are capped at 8; closed channels stay visible, so allow 4×).
pub const MAX_CHANNELS: usize = MAX_PORTS as usize * 4;

impl Mirror {
    /// Copy of one channel's view.
    pub fn get(&self, channel: &str) -> Option<ChannelView> {
        self.0.read().ok()?.get(channel).cloned()
    }

    /// Current generation of a channel.
    pub fn generation(&self, channel: &str) -> Option<u32> {
        self.get(channel)?.generation
    }

    fn apply(&self, ev: &PublishedRecord) {
        let Some(ch) = ev.channel.as_deref() else {
            return;
        };
        let Ok(mut map) = self.0.write() else {
            return;
        };
        if !map.contains_key(ch) && map.len() >= MAX_CHANNELS {
            // Evict the first closed channel; if none, ignore the newcomer (counted in status).
            let closed = map
                .iter()
                .find(|(_, v)| v.state.get("state").and_then(Value::as_str) == Some("closed"))
                .map(|(k, _)| k.clone());
            match closed {
                Some(k) => {
                    map.remove(&k);
                }
                None => return,
            }
        }
        let view = map.entry(ch.to_owned()).or_default();
        let p = ev.payload.clone();
        match ev.kind.as_str() {
            kinds::STATE => {
                view.state = p;
                if ev.generation.is_some() {
                    view.generation = ev.generation;
                }
            }
            kinds::CONFIG => view.config = p,
            kinds::INPUT_OWNER => view.input_owner = p,
            kinds::HOLD => {
                if let Value::Object(o) = &mut view.input_owner {
                    o.insert("hold".into(), p.get("on").cloned().unwrap_or(Value::Bool(false)));
                } else {
                    view.input_owner = json!({"owner": "none", "dirty": false, "hold": p.get("on").cloned().unwrap_or(Value::Bool(false))});
                }
            }
            kinds::LOCK => view.lock = p,
            kinds::AGENT_ACTIVITY => view.agent_activity = p,
            _ => {}
        }
    }

    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        let Ok(map) = self.0.read() else {
            return Vec::new();
        };
        map.iter()
            .filter(|(ch, _)| filter.channels.is_empty() || filter.channels.iter().any(|c| c == *ch))
            .map(|(ch, v)| {
                let mut o = Map::new();
                o.insert("channel".into(), json!(ch));
                o.insert("generation".into(), json!(v.generation));
                for (k, val) in [
                    ("state", &v.state),
                    ("config", &v.config),
                    ("input_owner", &v.input_owner),
                    ("lock", &v.lock),
                    ("agent_activity", &v.agent_activity),
                ] {
                    if !val.is_null() {
                        o.insert(k.into(), val.clone());
                    }
                }
                Value::Object(o)
            })
            .collect()
    }
}

/// Reducer adapter registered with the core.
#[derive(Debug, Clone, Default)]
pub struct MirrorReducer(pub Mirror);

const MIRROR_KINDS: [&str; 6] = [
    kinds::STATE,
    kinds::CONFIG,
    kinds::INPUT_OWNER,
    kinds::HOLD,
    kinds::LOCK,
    kinds::AGENT_ACTIVITY,
];

impl Reducer for MirrorReducer {
    fn kinds(&self) -> &[&'static str] {
        &MIRROR_KINDS
    }
    fn apply(&mut self, ev: &PublishedRecord) {
        self.0.apply(ev);
    }
    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        self.0.snapshot(filter)
    }
}
