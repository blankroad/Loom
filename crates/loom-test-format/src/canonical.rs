use crate::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
/// SHA-256 of the exact source bytes; lowercase hexadecimal without a prefix.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
/// Simple JCS: recursively sort object keys by UTF-8 lexical order, preserve array order,
/// serialize finite serde_json numbers/strings with serde_json, no whitespace. Not full RFC 8785.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>, FormatError> {
    let mut nodes = 0;
    let mut bytes = 0;
    let sorted = sort(value, 0, &mut nodes, &mut bytes)?;
    let mut output = BoundedJson(Vec::new());
    serde_json::to_writer(&mut output, &sorted).map_err(|e| {
        error(
            "",
            0,
            "/",
            "canonical_limit",
            format!("canonical JSON exceeds its bounded output: {e}"),
        )
    })?;
    Ok(output.0)
}
pub fn canonical_sha256(value: &Value) -> Result<String, FormatError> {
    Ok(sha256(&canonical_json(value)?))
}
fn sort(v: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Result<Value, FormatError> {
    *nodes += 1;
    *bytes = bytes
        .saturating_add(CAP_AST_NODE_BYTES)
        .saturating_add(v.as_str().map_or(0, str::len));
    if *bytes > MAX_AST_BYTES {
        return Err(error("", 0, "/", "ast_byte_limit", "canonical AST byte limit"));
    }
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(error("", 0, "/", "ast_limit", "canonical AST depth/node limit"));
    }
    match v {
        Value::Object(m) => {
            if m.len() > MAX_COLLECTION_ITEMS {
                return Err(error("", 0, "/", "collection_limit", "canonical object limit"));
            }
            let mut keys = m.keys().collect::<Vec<_>>();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                if k.len() > MAX_SCALAR_BYTES {
                    return Err(error("", 0, "/", "scalar_limit", "canonical key limit"));
                }
                *nodes += 1;
                *bytes = bytes.saturating_add(CAP_AST_NODE_BYTES).saturating_add(k.len());
                out.insert(k.clone(), sort(&m[k], depth + 1, nodes, bytes)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(a) => {
            if a.len() > MAX_COLLECTION_ITEMS {
                return Err(error("", 0, "/", "collection_limit", "canonical array limit"));
            }
            a.iter()
                .map(|x| sort(x, depth + 1, nodes, bytes))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        Value::String(s) if s.len() > MAX_SCALAR_BYTES => {
            Err(error("", 0, "/", "scalar_limit", "canonical string limit"))
        }
        _ => Ok(v.clone()),
    }
}

struct BoundedJson(Vec<u8>);
impl std::io::Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FILE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("canonical JSON exceeds 1 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
