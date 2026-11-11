//! Small durable/shared state pieces: STOP latch file and idempotency table (policy: `policy.rs`).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_proto::event::Origin;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

use crate::home::{Home, write_atomic};
use crate::hub::lock;

// ---------------------------------------------------------------- latch file

/// `run/latch.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatchFile {
    /// Latched.
    pub on: bool,
    /// Who changed it (client id).
    pub by: String,
    /// Origin of the change.
    pub origin: Origin,
    /// Wall time (ns, decimal string).
    pub wall_ns: String,
}

/// Latch state at start: missing file = off; `on`, unreadable or corrupt = **latched** (fail safe).
pub fn load_latch(home: &Home) -> (bool, &'static str) {
    match std::fs::read(home.latch_json()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, "none"),
        Err(_) => (true, "unreadable"),
        Ok(b) => match serde_json::from_slice::<LatchFile>(&b) {
            Ok(f) if !f.on => (false, "off"),
            Ok(_) => (true, "on"),
            Err(_) => (true, "corrupt"),
        },
    }
}

/// Persist the latch. `false` = not persisted (STOP stays valid in memory).
pub fn persist_latch(home: &Home, on: bool, by: &str, origin: Origin) -> bool {
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let f = LatchFile {
        on,
        by: by.into(),
        origin,
        wall_ns: wall.to_string(),
    };
    match serde_json::to_vec(&f) {
        Ok(b) => write_atomic(&home.latch_json(), &b, true)
            .map_err(|e| tracing::error!("latch persist: {e}"))
            .is_ok(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- idempotency

/// Result TTL (contract 03 §3.1).
pub const IDEM_TTL: Duration = Duration::from_secs(24 * 3600);
/// Stored result bytes.
pub const IDEM_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
struct IdemEntry {
    tx: watch::Sender<Option<Value>>,
    at: Instant,
    bytes: usize,
}

/// Outcome of reserving `(client_id, key)`.
#[derive(Debug)]
pub enum Reserve {
    /// First request: run it and call [`Idempotency::complete`].
    New,
    /// Same key in progress: wait for its result.
    Pending(watch::Receiver<Option<Value>>),
    /// Already done: the stored result.
    Done(Value),
}

/// Entries, insertion order, stored bytes.
type IdemTable = (
    HashMap<(String, String), IdemEntry>,
    VecDeque<(String, String)>,
    usize,
);

/// `(client_id, idempotency key)` → result. A repeated key never sends again.
#[derive(Debug, Default)]
pub struct Idempotency {
    inner: Mutex<IdemTable>,
}

impl Idempotency {
    /// Reserve atomically.
    pub fn reserve(&self, client: &str, key: &str) -> Reserve {
        let mut g = lock(&self.inner);
        let (map, order, bytes) = &mut *g;
        // expire / evict
        while let Some(k) = order.front() {
            let expired = map.get(k).is_none_or(|e| e.at.elapsed() > IDEM_TTL);
            if !(expired || *bytes > IDEM_BYTES) {
                break;
            }
            if let Some(k) = order.pop_front()
                && let Some(e) = map.remove(&k)
            {
                *bytes = bytes.saturating_sub(e.bytes);
            }
        }
        let k = (client.to_owned(), key.to_owned());
        if let Some(e) = map.get(&k) {
            return match e.tx.borrow().clone() {
                Some(v) => Reserve::Done(v),
                None => Reserve::Pending(e.tx.subscribe()),
            };
        }
        let (tx, _) = watch::channel(None);
        map.insert(
            k.clone(),
            IdemEntry {
                tx,
                at: Instant::now(),
                bytes: 256,
            },
        );
        *bytes += 256;
        order.push_back(k);
        Reserve::New
    }

    /// Store the final result of a reserved key and wake waiters.
    pub fn complete(&self, client: &str, key: &str, result: Value) {
        let mut g = lock(&self.inner);
        let (map, _, bytes) = &mut *g;
        if let Some(e) = map.get_mut(&(client.to_owned(), key.to_owned())) {
            let n = result.to_string().len();
            *bytes += n;
            e.bytes += n;
            e.tx.send_replace(Some(result));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn idempotency_states() {
        let i = Idempotency::default();
        assert!(matches!(i.reserve("c", "k"), Reserve::New));
        assert!(matches!(i.reserve("c", "k"), Reserve::Pending(_)));
        assert!(matches!(i.reserve("d", "k"), Reserve::New));
        i.complete("c", "k", serde_json::json!({"ok": true}));
        assert!(matches!(i.reserve("c", "k"), Reserve::Done(_)));
    }
}
