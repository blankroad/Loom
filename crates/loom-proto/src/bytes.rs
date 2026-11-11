//! Inline byte strings with a dual wire form (contract v1 §6.1).
//!
//! * [`Bytes`]: JSON = lowercase hex string (RPC arguments, small payloads). Uppercase hex is
//!   accepted on input; output is always lowercase.
//! * [`Base64Bytes`]: JSON = standard padded base64 (RFC 4648 §4) for larger event payloads.
//! * Binary formats (MessagePack): `bin` for both.
//!
//! Both decoders check the length **before** allocating and reject anything over
//! [`MAX_INLINE_BYTES`]; larger data travels as `raw_ref` (plan §4.8). Decoding never panics.

use std::borrow::Cow;
use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ProtoError;

/// Largest byte string carried inline in a message (64 KiB). Bigger data uses `raw_ref`.
pub const MAX_INLINE_BYTES: usize = 64 * 1024;

/// Decode a hex string (either case) with the inline size limit.
pub fn decode_hex(s: &str) -> Result<Vec<u8>, ProtoError> {
    if !s.len().is_multiple_of(2) {
        return Err(ProtoError::InvalidHex("odd length".into()));
    }
    let n = s.len() / 2;
    if n > MAX_INLINE_BYTES {
        return Err(ProtoError::TooLarge {
            len: n,
            max: MAX_INLINE_BYTES,
        });
    }
    if !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(ProtoError::InvalidHex("non-hex character".into()));
    }
    hex::decode(s).map_err(|e| ProtoError::InvalidHex(e.to_string()))
}

/// Decode a standard padded base64 string with the inline size limit.
pub fn decode_base64(s: &str) -> Result<Vec<u8>, ProtoError> {
    // Upper bound of decoded size, checked before allocation.
    let max_len = s.len() / 4 * 3;
    if max_len > MAX_INLINE_BYTES + 2 {
        return Err(ProtoError::TooLarge {
            len: max_len,
            max: MAX_INLINE_BYTES,
        });
    }
    if !s.is_ascii() {
        return Err(ProtoError::InvalidBase64("non-ASCII character".into()));
    }
    let v = B64
        .decode(s)
        .map_err(|e| ProtoError::InvalidBase64(e.to_string()))?;
    if v.len() > MAX_INLINE_BYTES {
        return Err(ProtoError::TooLarge {
            len: v.len(),
            max: MAX_INLINE_BYTES,
        });
    }
    Ok(v)
}

#[derive(Clone, Copy)]
enum TextForm {
    Hex,
    Base64,
}

struct BytesVisitor {
    form: TextForm,
    human: bool,
}

impl BytesVisitor {
    fn check_len<E: de::Error>(len: usize) -> Result<(), E> {
        if len > MAX_INLINE_BYTES {
            return Err(E::custom(ProtoError::TooLarge {
                len,
                max: MAX_INLINE_BYTES,
            }));
        }
        Ok(())
    }
}

impl<'de> Visitor<'de> for BytesVisitor {
    type Value = Vec<u8>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.human, self.form) {
            (true, TextForm::Hex) => f.write_str("a hex string"),
            (true, TextForm::Base64) => f.write_str("a standard base64 string"),
            (false, _) => f.write_str("a byte string"),
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<u8>, E> {
        if !self.human {
            return Err(E::invalid_type(de::Unexpected::Str(v), &self));
        }
        match self.form {
            TextForm::Hex => decode_hex(v),
            TextForm::Base64 => decode_base64(v),
        }
        .map_err(E::custom)
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
        if self.human {
            return Err(E::invalid_type(de::Unexpected::Bytes(v), &self));
        }
        Self::check_len(v.len())?;
        Ok(v.to_vec())
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
        if self.human {
            return Err(E::invalid_type(de::Unexpected::Bytes(&v), &self));
        }
        Self::check_len(v.len())?;
        Ok(v)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        // Some binary formats encode bytes as a sequence of u8. Bound before allocating.
        if self.human {
            return Err(de::Error::invalid_type(de::Unexpected::Seq, &self));
        }
        let hint = seq.size_hint().unwrap_or(0);
        Self::check_len::<A::Error>(hint)?;
        let mut out = Vec::with_capacity(hint);
        while let Some(b) = seq.next_element::<u8>()? {
            if out.len() == MAX_INLINE_BYTES {
                return Err(de::Error::custom(ProtoError::TooLarge {
                    len: MAX_INLINE_BYTES + 1,
                    max: MAX_INLINE_BYTES,
                }));
            }
            out.push(b);
        }
        Ok(out)
    }
}

fn de_bytes<'de, D: Deserializer<'de>>(d: D, form: TextForm) -> Result<Vec<u8>, D::Error> {
    if d.is_human_readable() {
        d.deserialize_str(BytesVisitor { form, human: true })
    } else {
        d.deserialize_bytes(BytesVisitor { form, human: false })
    }
}

/// Byte string; JSON = lowercase hex, binary = `bin`. Limit [`MAX_INLINE_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Bytes(pub Vec<u8>);

/// Byte string; JSON = standard base64, binary = `bin`. Limit [`MAX_INLINE_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Base64Bytes(pub Vec<u8>);

macro_rules! impl_bytes_common {
    ($t:ident, $form:expr, $enc:expr, $name:literal, $schema:tt) => {
        impl $t {
            /// Borrow the bytes.
            pub fn as_slice(&self) -> &[u8] {
                &self.0
            }
            /// Take the bytes.
            pub fn into_vec(self) -> Vec<u8> {
                self.0
            }
        }

        impl From<Vec<u8>> for $t {
            fn from(v: Vec<u8>) -> Self {
                $t(v)
            }
        }

        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                if s.is_human_readable() {
                    let enc: fn(&[u8]) -> String = $enc;
                    s.serialize_str(&enc(&self.0))
                } else {
                    s.serialize_bytes(&self.0)
                }
            }
        }

        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                de_bytes(d, $form).map($t)
            }
        }

        impl JsonSchema for $t {
            fn schema_name() -> Cow<'static, str> {
                $name.into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!($schema)
            }
        }
    };
}

impl_bytes_common!(Bytes, TextForm::Hex, |b| hex::encode(b), "HexBytes", {
    "type": "string",
    "pattern": "^([0-9a-fA-F]{2})*$",
    "maxLength": 131072,
    "description": "Bytes as hex (lowercase on output), at most 64 KiB decoded."
});

impl_bytes_common!(Base64Bytes, TextForm::Base64, |b| B64.encode(b), "Base64Bytes", {
    "type": "string",
    "contentEncoding": "base64",
    "maxLength": 87384,
    "description": "Bytes as standard padded base64 (RFC 4648 §4), at most 64 KiB decoded."
});
