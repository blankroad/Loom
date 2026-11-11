//! `U64`: 64-bit unsigned integer with a dual wire form (contract v1 §6.1, D3).
//!
//! * Human-readable formats (JSON): canonical decimal **string**, e.g. `"18446744073709551615"`.
//!   Canonical means: ASCII digits only, no sign, no whitespace, **no leading zero** except the
//!   single value `"0"`. Rejecting leading zeros keeps "same string ⇔ same number", which matters
//!   for JCS digests (approval binding, audit chain). JSON numbers are rejected.
//! * Binary formats (MessagePack, worker protocol): native unsigned integer.
//!
//! The branch is taken with `Serializer::is_human_readable()`. Decoding never panics.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ProtoError;

/// JSON Schema pattern for the canonical decimal string form.
pub const U64_PATTERN: &str = "^(0|[1-9][0-9]{0,19})$";

/// Unsigned 64-bit integer; JSON = canonical decimal string, binary = integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct U64(pub u64);

impl U64 {
    /// Zero.
    pub const ZERO: U64 = U64(0);
    /// Largest value.
    pub const MAX: U64 = U64(u64::MAX);

    /// The inner value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for U64 {
    fn from(v: u64) -> Self {
        U64(v)
    }
}

impl From<U64> for u64 {
    fn from(v: U64) -> Self {
        v.0
    }
}

impl fmt::Display for U64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Strictly parse a canonical decimal string (see module docs).
pub fn parse_u64_canonical(s: &str) -> Result<u64, ProtoError> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 20 {
        return Err(ProtoError::InvalidU64(truncate(s)));
    }
    if !b.iter().all(u8::is_ascii_digit) {
        return Err(ProtoError::InvalidU64(truncate(s)));
    }
    if b.len() > 1 && b[0] == b'0' {
        return Err(ProtoError::InvalidU64(format!("leading zero: {}", truncate(s))));
    }
    // All ASCII digits and ≤ 20 chars: the only failure left is overflow.
    s.parse::<u64>()
        .map_err(|_| ProtoError::InvalidU64(format!("overflow: {}", truncate(s))))
}

fn truncate(s: &str) -> String {
    s.chars().take(32).collect()
}

impl FromStr for U64 {
    type Err = ProtoError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_u64_canonical(s).map(U64)
    }
}

impl Serialize for U64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.0)
        } else {
            s.serialize_u64(self.0)
        }
    }
}

struct U64Visitor {
    human: bool,
}

impl Visitor<'_> for U64Visitor {
    type Value = U64;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.human {
            f.write_str("a canonical decimal string (u64)")
        } else {
            f.write_str("an unsigned 64-bit integer")
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<U64, E> {
        if !self.human {
            return Err(E::invalid_type(de::Unexpected::Str(v), &self));
        }
        parse_u64_canonical(v).map(U64).map_err(E::custom)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<U64, E> {
        if self.human {
            return Err(E::invalid_type(de::Unexpected::Unsigned(v), &self));
        }
        Ok(U64(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<U64, E> {
        if self.human {
            return Err(E::invalid_type(de::Unexpected::Signed(v), &self));
        }
        u64::try_from(v)
            .map(U64)
            .map_err(|_| E::invalid_value(de::Unexpected::Signed(v), &self))
    }
}

impl<'de> Deserialize<'de> for U64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            d.deserialize_str(U64Visitor { human: true })
        } else {
            d.deserialize_u64(U64Visitor { human: false })
        }
    }
}

impl JsonSchema for U64 {
    fn schema_name() -> Cow<'static, str> {
        "U64".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": U64_PATTERN,
            "description": "Unsigned 64-bit integer as canonical decimal string (no sign, no leading zero)."
        })
    }
}
