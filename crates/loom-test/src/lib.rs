//! TEST session module. CORE owns registration, journal/fences, policy and all device dispatch.
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "runtime")]
mod engine;
#[cfg(feature = "runtime")]
pub mod gateway;
pub mod model;
#[cfg(feature = "runtime")]
pub mod seams;
#[cfg(feature = "runtime")]
mod snapshot;
mod source;
#[cfg(feature = "runtime")]
pub mod wire;
#[cfg(feature = "runtime")]
pub use engine::{SessionConfig, TestModule};
pub use model::*;
#[cfg(feature = "runtime")]
pub use seams::*;
#[cfg(feature = "runtime")]
pub use snapshot::TestReducer;
#[cfg(feature = "runtime")]
pub use wire::PredicateEngine;
pub const DOMAIN: &str = "test";
/// Concurrent sessions default 1, configurable up to 4; reject new starts rather than evict active sessions.
pub const MAX_ACTIVE_SESSIONS: usize = 4;
/// Completed session snapshots retained in memory; oldest completed session is evicted on new start.
pub const MAX_RETAINED_SESSIONS: usize = 8;
/// Accepted/in-flight control requests; newest request gets E_LOCKED when full, never silently dropped.
pub const MAX_CONTROL_REQUESTS: usize = 1024;
/// Concurrent blocking verifier calls; refuse additional checks until permits are released.
pub const MAX_CHECKS: usize = 2;
/// Evidence citations per verdict/check/step; reject excess before cloning.
pub const MAX_CITATIONS: usize = 64;
/// Producers in one CORE source fence; a larger fence is refused (`E_PARTIAL`), never truncated.
/// Two fences per retained attempt × 500 steps stay inside `CAP_SESSION_BASE_BYTES`.
pub const MAX_FENCE_SOURCES: usize = 128;
/// Unflushed domains named by one fence.
pub const MAX_FENCE_DOMAINS: usize = 64;
/// Retained evidence rows per session; oldest display row is evicted, journal remains authoritative.
pub const MAX_RECENT_EVIDENCE: usize = 200;
/// Session notes/questions/answer text bytes; reject oversized prose, never silently truncate it.
pub const MAX_TEXT_BYTES: usize = 4096;
/// Retained notes per session; reject the newest on overflow.
pub const MAX_NOTES: usize = 1000;
/// Simultaneously open questions per session; reject the newest.
pub const MAX_OPEN_ASKS: usize = 16;
/// Options per question; reject excess entries (each option is at most 256 bytes).
pub const MAX_ASK_OPTIONS: usize = 16;
/// Retained step verdict audit rows; older audit stays in journal, ring drops oldest only after publication.
pub const MAX_VERDICT_HISTORY: usize = 8;
/// JSON control envelope (two <=1 MiB source texts with escaping); bounded serializer rejects overflow.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
/// Non-source arbitrary JSON/ref DTO bytes; reject rather than retain huge evidence.
pub const MAX_CITATION_BYTES: usize = 8192;
/// Engine logical retained-memory budget, not a measured RSS guarantee; additions are charged before commit.
pub const MAX_ENGINE_BYTES: usize = 64 * 1024 * 1024;
/// Conservative fixed allocation charge per parsed session/plan (AST, normalized args, DTOs and mirrors).
pub const CAP_SESSION_BASE_BYTES: usize = 12 * 1024 * 1024;
/// Bound per-step interpretation/command summaries so start/snapshot payloads remain bounded.
pub const MAX_PREVIEW_BYTES: usize = 1024;
/// Hard event payload ceiling; reject before publishing to CORE.
pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
/// Observed summary per checked citation; reject a verifier that exceeds its integration contract.
pub const MAX_OBSERVED_BYTES: usize = 2048;
/// Single question/approval wait ceiling.
pub const MAX_WAIT_MS: u64 = 3_600_000;
/// Number of end/step/retry changes retained as scalar revision counters is bounded by u32 exhaustion.
pub const MAX_ID_BYTES: usize = 128;
/// Long-lived waiters do not occupy control-queue slots, leaving room for answer/resume.
pub const MAX_WAITERS: usize = 64;
/// Charged mutation records retained per session; reject further mutations at the quota.
pub const MAX_SESSION_MUTATIONS: usize = 16_384;
use loom_domain_api::DomainError;
use loom_proto::error::ErrorCode;
use serde_json::Value;
#[cfg(feature = "runtime")]
use std::sync::{Mutex, MutexGuard};
#[cfg(feature = "runtime")]
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn err(code: ErrorCode, msg: impl Into<String>) -> DomainError {
    DomainError::new(code, msg)
}
fn usage(msg: impl Into<String>) -> DomainError {
    err(ErrorCode::Usage, msg)
}
#[cfg(feature = "runtime")]
#[derive(Debug)]
struct LimitedJson {
    bytes: Vec<u8>,
    limit: usize,
}
#[cfg(feature = "runtime")]
impl std::io::Write for LimitedJson {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("JSON byte limit"));
        }
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(feature = "runtime")]
fn json_len(v: &Value, limit: usize) -> Result<usize, DomainError> {
    validate_json(v)?;
    let mut out = LimitedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut out, v).map_err(|_| usage("JSON exceeds byte/depth budget"))?;
    Ok(out.bytes.len())
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, DomainError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or_else(|| usage(format!("{key} must be a nonempty string of at most {max} bytes")))
}
#[cfg(feature = "runtime")]
fn optional_text<'a>(v: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, DomainError> {
    v.get(key)
        .filter(|v| !v.is_null())
        .map(|_| text(v, key, max))
        .transpose()
}
#[cfg(feature = "runtime")]
fn bool_arg(v: &Value, key: &str, default: bool) -> Result<bool, DomainError> {
    match v.get(key) {
        None => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(usage(format!("{key} must be boolean"))),
    }
}
#[cfg(feature = "runtime")]
fn u64_arg(v: &Value, key: &str, default: Option<u64>) -> Result<u64, DomainError> {
    let n = match v.get(key) {
        Some(Value::String(s)) => loom_proto::num::parse_u64_canonical(s).ok(),
        Some(v) => v.as_u64(),
        None => default,
    };
    n.filter(|n| *n > 0 && *n <= MAX_WAIT_MS)
        .ok_or_else(|| usage(format!("{key} must be bounded positive milliseconds")))
}
fn preview(s: &str) -> String {
    let mut n = s.len().min(MAX_PREVIEW_BYTES);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    if n < s.len() {
        format!("{}…", &s[..n])
    } else {
        s.into()
    }
}
/// Iterative JSON preflight prevents recursion/huge DTOs before serialization or serde cloning.
#[cfg(feature = "runtime")]
fn validate_json(value: &Value) -> Result<(), DomainError> {
    let mut stack = vec![(value, 0usize)];
    let mut nodes = 0usize;
    while let Some((v, depth)) = stack.pop() {
        nodes += 1;
        if nodes > loom_test_format::MAX_NODES || depth > loom_test_format::MAX_DEPTH {
            return Err(usage("JSON AST depth/node budget exceeded"));
        }
        match v {
            Value::Array(a) => {
                if a.len() > loom_test_format::MAX_COLLECTION_ITEMS {
                    return Err(usage("JSON array bound"));
                }
                for x in a {
                    stack.push((x, depth + 1));
                }
            }
            Value::Object(m) => {
                if m.len() > loom_test_format::MAX_COLLECTION_ITEMS {
                    return Err(usage("JSON object bound"));
                }
                for x in m.values() {
                    stack.push((x, depth + 1));
                }
            }
            _ => (),
        }
    }
    Ok(())
}
