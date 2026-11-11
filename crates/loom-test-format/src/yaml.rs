//! Deserialize directly into bounded JSON; no unbounded intermediate YAML Value/alias expansion.
use crate::*;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{cell::Cell, fmt};
struct Seed<'a> {
    depth: usize,
    nodes: &'a Cell<usize>,
    bytes: &'a Cell<usize>,
}
impl Seed<'_> {
    fn charge<E: de::Error>(&self, n: usize) -> Result<(), E> {
        let n = self.bytes.get().saturating_add(n);
        if n > MAX_AST_BYTES {
            return Err(E::custom("YAML expanded AST byte limit"));
        }
        self.bytes.set(n);
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        self.charge::<D::Error>(CAP_AST_NODE_BYTES)?;
        let n = self.nodes.get() + 1;
        self.nodes.set(n);
        if self.depth > MAX_DEPTH || n > MAX_NODES {
            return Err(de::Error::custom("YAML AST depth/node limit"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON-compatible YAML (string keys, finite numbers, no tags)")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite YAML number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        if v.len() > MAX_SCALAR_BYTES {
            return Err(E::custom("YAML scalar byte limit"));
        }
        self.charge::<E>(v.len())?;
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        if v.len() > MAX_SCALAR_BYTES {
            return Err(E::custom("YAML scalar byte limit"));
        }
        self.charge::<E>(v.len())?;
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = a.next_element_seed(Seed {
            depth: self.depth + 1,
            nodes: self.nodes,
            bytes: self.bytes,
        })? {
            if out.len() >= MAX_COLLECTION_ITEMS {
                return Err(de::Error::custom("YAML collection limit"));
            }
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = a.next_key_seed(Seed {
            depth: self.depth + 1,
            nodes: self.nodes,
            bytes: self.bytes,
        })? {
            if out.len() >= MAX_COLLECTION_ITEMS {
                return Err(de::Error::custom("YAML collection limit"));
            }
            let Value::String(key) = key else {
                return Err(de::Error::custom("YAML keys must be strings"));
            };
            if out.contains_key(&key) {
                return Err(de::Error::custom("duplicate YAML mapping key"));
            }
            let value = a.next_value_seed(Seed {
                depth: self.depth + 1,
                nodes: self.nodes,
                bytes: self.bytes,
            })?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}
pub(crate) fn parse(text: &str, whole: &str, offset: usize) -> Result<Value, FormatError> {
    preflight(text, whole, offset)?;
    let nodes = Cell::new(0);
    let bytes = Cell::new(0);
    Seed {
        depth: 0,
        nodes: &nodes,
        bytes: &bytes,
    }
    .deserialize(serde_norway::Deserializer::from_str(text))
    .map_err(|e| {
        let local = e.location().map_or(0, |l| l.index());
        error(whole, offset + local, "/", "yaml", e.to_string())
    })
}

/// The backend retains a whole document's lexical events before visiting values. Bound
/// that allocation too, independently of the AST/alias expansion limits in Seed.
const MAX_YAML_LEXICAL_TOKENS: usize = MAX_NODES * 2;
const MAX_YAML_INDENT: usize = 256;
fn preflight(text: &str, whole: &str, base: usize) -> Result<(), FormatError> {
    let mut tokens = 0usize;
    let mut flow = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut block = None::<(usize, usize)>;
    let mut indents = vec![0usize];
    let mut offset = 0;
    let mut scalar_bytes = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if let Some((level, size)) = &mut block {
            if trimmed.trim().is_empty() || indent > *level {
                *size += line.len();
                if *size > MAX_SCALAR_BYTES {
                    return Err(error(
                        whole,
                        base + offset,
                        "/",
                        "scalar_limit",
                        "YAML block scalar exceeds 64 KiB",
                    ));
                }
                offset += line.len();
                continue;
            }
            block = None;
        }
        if quote.is_none() && flow == 0 && !trimmed.trim().is_empty() && !trimmed.starts_with('#') {
            if indent > MAX_YAML_INDENT {
                return Err(error(
                    whole,
                    base + offset,
                    "/",
                    "yaml_indent_limit",
                    "YAML indentation exceeds 256 columns",
                ));
            }
            while indents.last().is_some_and(|n| *n > indent) {
                indents.pop();
            }
            if indents.last().is_none_or(|n| *n < indent) {
                indents.push(indent);
            }
            if indents.len() > MAX_DEPTH {
                return Err(error(
                    whole,
                    base + offset,
                    "/",
                    "yaml_depth_limit",
                    "YAML block nesting limit",
                ));
            }
        }
        let b = line.as_bytes();
        let mut i = 0;
        let mut plain = false;
        let mut starts_block = false;
        while i < b.len() {
            let ch = b[i];
            if let Some(q) = quote {
                scalar_bytes += 1;
                if scalar_bytes > MAX_SCALAR_BYTES {
                    return Err(error(
                        whole,
                        base + offset + i,
                        "/",
                        "scalar_limit",
                        "YAML quoted scalar exceeds 64 KiB",
                    ));
                }
                if q == b'"' && escaped {
                    escaped = false;
                } else if q == b'"' && ch == b'\\' {
                    escaped = true;
                } else if ch == q {
                    if q == b'\'' && b.get(i + 1) == Some(&b'\'') {
                        i += 1;
                        scalar_bytes += 1;
                    } else {
                        quote = None;
                        scalar_bytes = 0;
                    }
                }
                i += 1;
                continue;
            }
            if ch == b'#' && (i == 0 || b[i - 1].is_ascii_whitespace()) {
                break;
            }
            if (ch == b'\'' || ch == b'"') && !plain {
                quote = Some(ch);
                scalar_bytes = 0;
                tokens += 1;
            } else if matches!(ch, b'[' | b'{') && (!plain || flow > 0) {
                flow += 1;
                tokens += 1;
                plain = false;
                if flow > MAX_DEPTH {
                    return Err(error(
                        whole,
                        base + offset + i,
                        "/",
                        "yaml_depth_limit",
                        "YAML flow nesting limit",
                    ));
                }
            } else if matches!(ch, b']' | b'}') && flow > 0 {
                flow -= 1;
                tokens += 1;
                plain = false;
            } else if (ch == b',' && flow > 0)
                || (ch == b':' && (flow > 0 || b.get(i + 1).is_none_or(|c| c.is_ascii_whitespace())))
            {
                tokens += 1;
                plain = false;
            } else if ch.is_ascii_whitespace() {
                plain = false;
                scalar_bytes = 0;
            } else {
                if !plain {
                    tokens += 1;
                    plain = true;
                    scalar_bytes = 0;
                    if matches!(ch, b'|' | b'>') && flow == 0 {
                        starts_block = true;
                    }
                }
                scalar_bytes += 1;
                if scalar_bytes > MAX_SCALAR_BYTES {
                    return Err(error(
                        whole,
                        base + offset + i,
                        "/",
                        "scalar_limit",
                        "YAML plain scalar token exceeds 64 KiB",
                    ));
                }
            }
            if tokens > MAX_YAML_LEXICAL_TOKENS {
                return Err(error(
                    whole,
                    base + offset + i,
                    "/",
                    "yaml_token_limit",
                    "YAML lexical event budget",
                ));
            }
            i += 1;
        }
        if starts_block && quote.is_none() {
            block = Some((indent, 0));
        }
        offset += line.len();
    }
    Ok(())
}
