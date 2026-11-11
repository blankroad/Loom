//! Hash convention shared with `loom-test-format` (TEST P1 format report): object keys sorted
//! recursively in UTF-8 byte order, arrays kept in order, serde_json number/string encoding,
//! no whitespace (a simple JCS, not full RFC 8785); SHA-256 as 64 lowercase hex digits.
//! Duplicated here (a few lines) so CORE can use this crate without the YAML/Markdown parsers;
//! a test checks both produce identical bytes.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// SHA-256 of `bytes`, lowercase hex without prefix.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// Canonical JSON bytes. Inputs here are already bounded (AST ≤ 512 nodes, ≤ 64 REFs), so no
/// separate size limit is needed.
pub fn canonical_json(v: &Value) -> Vec<u8> {
    serde_json::to_vec(&sorted(v)).unwrap_or_default()
}

/// SHA-256 of [`canonical_json`].
pub fn canonical_sha256(v: &Value) -> String {
    sha256_hex(&canonical_json(v))
}
