//! Field values and types shared by the decoder and the display filter.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Serialize;

/// Type of a registered field. The display filter type-checks literals against this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    /// Protocol presence (`tcp`, `doip`). Value = the layer bytes.
    Protocol,
    U8,
    U16,
    U32,
    U64,
    Bool,
    Bytes,
    Str,
    Ipv4,
    Ipv6,
    Mac,
    /// Relative time in seconds. Not emitted by the decoder; the filter computes it
    /// from `FrameMeta::ts_ns` and its own time reference (`frame.time_relative`, `frame.time_delta`).
    TimeRel,
}

impl FieldType {
    /// Unsigned integer types.
    pub fn is_uint(self) -> bool {
        matches!(
            self,
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64
        )
    }

    /// Maximum value representable by an unsigned integer type.
    pub fn uint_max(self) -> Option<u64> {
        match self {
            FieldType::U8 => Some(u64::from(u8::MAX)),
            FieldType::U16 => Some(u64::from(u16::MAX)),
            FieldType::U32 => Some(u64::from(u32::MAX)),
            FieldType::U64 => Some(u64::MAX),
            _ => None,
        }
    }
}

/// Preferred display base for unsigned integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Base {
    Dec,
    Hex,
}

/// One value of a field. A field may occur several times in a packet (multi-value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Uint(u64),
    Bool(bool),
    Bytes(Vec<u8>),
    Str(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Mac([u8; 6]),
}

/// Formats a MAC as `aa:bb:cc:dd:ee:ff`.
pub fn fmt_mac(m: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0], m[1], m[2], m[3], m[4], m[5]
    )
}

/// Formats bytes as lowercase hex without separators.
pub fn fmt_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldValue::Uint(v) => write!(f, "{v}"),
            FieldValue::Bool(v) => write!(f, "{v}"),
            FieldValue::Bytes(b) => f.write_str(&fmt_hex(b)),
            FieldValue::Str(s) => f.write_str(s),
            FieldValue::Ipv4(a) => write!(f, "{a}"),
            FieldValue::Ipv6(a) => write!(f, "{a}"),
            FieldValue::Mac(m) => f.write_str(&fmt_mac(m)),
        }
    }
}
