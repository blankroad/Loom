//! DID definitions and value codecs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CodecError, MAX_PDU_LEN};

/// Largest DID data length (a `0x62` response is SID + DID + data).
pub const MAX_DID_DATA_LEN: usize = MAX_PDU_LEN - 3;
/// Longest DID name or unit string.
pub const MAX_NAME_LEN: usize = 128;
/// Most entries in one enum codec map.
pub const MAX_ENUM_ENTRIES: usize = 256;

/// Byte order for `uint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Endian {
    #[default]
    Big,
    Little,
}

/// One value of an enum codec (raw value read big-endian over the DID length).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumEntry {
    pub value: u64,
    pub name: String,
}

fn default_scale() -> f64 {
    1.0
}

/// How DID bytes map to a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DidCodec {
    /// Printable ASCII. Trailing `0x00`/`0xFF` padding is trimmed on decode.
    Ascii,
    /// Raw bytes shown as hex (with an ASCII hint when every byte is printable).
    Hex,
    /// Unsigned integer of the DID length (1–8 bytes): `value = raw * scale + offset`.
    Uint {
        #[serde(default)]
        endian: Endian,
        #[serde(default = "default_scale")]
        scale: f64,
        #[serde(default)]
        offset: f64,
        #[serde(default)]
        unit: Option<String>,
    },
    /// Packed BCD digits (two per byte).
    Bcd,
    /// Big-endian unsigned raw value mapped to names.
    Enum { map: Vec<EnumEntry> },
}

impl DidCodec {
    pub fn kind(&self) -> &'static str {
        match self {
            DidCodec::Ascii => "ascii",
            DidCodec::Hex => "hex",
            DidCodec::Uint { .. } => "uint",
            DidCodec::Bcd => "bcd",
            DidCodec::Enum { .. } => "enum",
        }
    }
}

/// One data identifier. There is deliberately no risk field: the DB cannot change risk grades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DidDef {
    pub id: u16,
    pub name: String,
    /// Data length in bytes. `None` = variable length (only readable alone).
    #[serde(default)]
    pub length: Option<u16>,
    pub codec: DidCodec,
    #[serde(default)]
    pub writable: bool,
}

/// A decoded DID value.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DidValue {
    Text {
        text: String,
    },
    Hex {
        hex: String,
        ascii: Option<String>,
    },
    Number {
        raw: u64,
        value: f64,
        unit: Option<String>,
    },
    Bcd {
        digits: String,
    },
    Enum {
        raw: u64,
        name: Option<String>,
    },
}

/// A value to encode for a write.
#[derive(Debug, Clone, PartialEq)]
pub enum DidInput {
    /// For `ascii` and `bcd` (digits).
    Text(String),
    /// Physical value for `uint` (`raw = round((value - offset) / scale)`).
    Number(f64),
    /// Name for `enum`.
    EnumName(String),
    /// Raw bytes; accepted by every codec as long as the length matches.
    Raw(Vec<u8>),
}

fn printable(b: u8) -> bool {
    (0x20..=0x7E).contains(&b)
}

fn uint_from(data: &[u8], endian: Endian) -> u64 {
    let mut v = 0u64;
    match endian {
        Endian::Big => data.iter().for_each(|b| v = (v << 8) | u64::from(*b)),
        Endian::Little => data.iter().rev().for_each(|b| v = (v << 8) | u64::from(*b)),
    }
    v
}

fn uint_to(raw: u64, len: usize, endian: Endian) -> Vec<u8> {
    let be = raw.to_be_bytes();
    let mut out = be[8 - len..].to_vec();
    if endian == Endian::Little {
        out.reverse();
    }
    out
}

fn max_for_len(len: usize) -> u64 {
    if len >= 8 {
        u64::MAX
    } else {
        (1u64 << (len * 8)) - 1
    }
}

impl DidDef {
    /// Check limits and internal consistency. The loader calls this for every DID.
    pub fn validate(&self) -> Result<(), CodecError> {
        let bad = |m: String| Err(CodecError::Db(format!("DID 0x{:04X}: {m}", self.id)));
        if self.name.is_empty() || self.name.len() > MAX_NAME_LEN {
            return bad(format!("name must be 1..={MAX_NAME_LEN} bytes"));
        }
        if let Some(len) = self.length
            && (len == 0 || usize::from(len) > MAX_DID_DATA_LEN)
        {
            return bad(format!("length must be 1..={MAX_DID_DATA_LEN}"));
        }
        let len = self.length.map(usize::from);
        match &self.codec {
            DidCodec::Uint {
                scale, offset, unit, ..
            } => {
                if !matches!(len, Some(1..=8)) {
                    return bad("uint needs length 1..=8".into());
                }
                if !scale.is_finite() || *scale == 0.0 || !offset.is_finite() {
                    return bad("uint scale must be finite and non-zero, offset finite".into());
                }
                if unit.as_ref().is_some_and(|u| u.len() > MAX_NAME_LEN) {
                    return bad("unit too long".into());
                }
            }
            DidCodec::Enum { map } => {
                let Some(n @ 1..=8) = len else {
                    return bad("enum needs length 1..=8".into());
                };
                if map.is_empty() || map.len() > MAX_ENUM_ENTRIES {
                    return bad(format!("enum map must have 1..={MAX_ENUM_ENTRIES} entries"));
                }
                if let Some(e) = map.iter().find(|e| e.value > max_for_len(n)) {
                    return bad(format!("enum value {} does not fit {n} bytes", e.value));
                }
                if map
                    .iter()
                    .any(|e| e.name.is_empty() || e.name.len() > MAX_NAME_LEN)
                {
                    return bad("enum names must be 1..=128 bytes".into());
                }
            }
            DidCodec::Ascii | DidCodec::Hex | DidCodec::Bcd => {}
        }
        Ok(())
    }

    fn check_len(&self, got: usize) -> Result<(), CodecError> {
        if got > MAX_DID_DATA_LEN {
            return Err(CodecError::TooLong {
                what: "DID data",
                len: got,
                max: MAX_DID_DATA_LEN,
            });
        }
        match self.length {
            Some(n) if usize::from(n) != got => Err(CodecError::LengthMismatch {
                what: "DID data",
                expected: usize::from(n),
                got,
            }),
            _ => Ok(()),
        }
    }

    /// Decode response bytes. Length mismatch with a fixed-length definition is an error.
    pub fn decode(&self, data: &[u8]) -> Result<DidValue, CodecError> {
        self.check_len(data.len())?;
        match &self.codec {
            DidCodec::Ascii => {
                let trimmed_len = data
                    .iter()
                    .rposition(|b| *b != 0x00 && *b != 0xFF)
                    .map_or(0, |i| i + 1);
                let body = &data[..trimmed_len];
                if let Some(b) = body.iter().find(|b| !printable(**b)) {
                    return Err(CodecError::Decode {
                        codec: "ascii",
                        detail: format!("byte 0x{b:02X}"),
                    });
                }
                Ok(DidValue::Text {
                    text: body.iter().map(|b| char::from(*b)).collect(),
                })
            }
            DidCodec::Hex => Ok(DidValue::Hex {
                hex: hex::encode_upper(data),
                ascii: (!data.is_empty() && data.iter().all(|b| printable(*b)))
                    .then(|| data.iter().map(|b| char::from(*b)).collect()),
            }),
            DidCodec::Uint {
                endian,
                scale,
                offset,
                unit,
            } => {
                if data.is_empty() || data.len() > 8 {
                    return Err(CodecError::Decode {
                        codec: "uint",
                        detail: "needs 1..=8 bytes".into(),
                    });
                }
                let raw = uint_from(data, *endian);
                // u64 → f64 may round above 2^53; acceptable for a display value, raw is exact.
                #[allow(clippy::cast_precision_loss)]
                let value = raw as f64 * scale + offset;
                Ok(DidValue::Number {
                    raw,
                    value,
                    unit: unit.clone(),
                })
            }
            DidCodec::Bcd => {
                let mut digits = String::with_capacity(data.len() * 2);
                for b in data {
                    for nib in [b >> 4, b & 0x0F] {
                        if nib > 9 {
                            return Err(CodecError::Decode {
                                codec: "bcd",
                                detail: format!("byte 0x{b:02X}"),
                            });
                        }
                        digits.push(char::from(b'0' + nib));
                    }
                }
                Ok(DidValue::Bcd { digits })
            }
            DidCodec::Enum { map } => {
                if data.is_empty() || data.len() > 8 {
                    return Err(CodecError::Decode {
                        codec: "enum",
                        detail: "needs 1..=8 bytes".into(),
                    });
                }
                let raw = uint_from(data, Endian::Big);
                let name = map.iter().find(|e| e.value == raw).map(|e| e.name.clone());
                Ok(DidValue::Enum { raw, name })
            }
        }
    }

    /// Encode a value for `0x2E`. The result always has the definition's length (if fixed).
    pub fn encode(&self, input: &DidInput) -> Result<Vec<u8>, CodecError> {
        let kind = self.codec.kind();
        let enc_err = |detail: String| CodecError::Encode { codec: kind, detail };
        let fixed = self.length.map(usize::from);
        let bytes = match (&self.codec, input) {
            (_, DidInput::Raw(b)) => b.clone(),
            (DidCodec::Ascii, DidInput::Text(t)) => {
                if !t.bytes().all(printable) {
                    return Err(enc_err("text must be printable ASCII".into()));
                }
                t.as_bytes().to_vec()
            }
            (DidCodec::Bcd, DidInput::Text(t)) => {
                if t.len() % 2 != 0 || !t.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(enc_err("BCD needs an even count of decimal digits".into()));
                }
                t.as_bytes()
                    .chunks_exact(2)
                    .map(|p| ((p[0] - b'0') << 4) | (p[1] - b'0'))
                    .collect()
            }
            (
                DidCodec::Uint {
                    endian,
                    scale,
                    offset,
                    ..
                },
                DidInput::Number(v),
            ) => {
                let Some(len) = fixed else {
                    return Err(enc_err("uint without fixed length".into()));
                };
                let raw = ((v - offset) / scale).round();
                // max_for_len(8) as f64 rounds up to 2^64, so compare with `<` against that bound.
                #[allow(clippy::cast_precision_loss)]
                let limit = max_for_len(len) as f64;
                if !raw.is_finite() || raw < 0.0 || raw > limit {
                    return Err(enc_err(format!("{v} is outside the {len}-byte range")));
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let raw_u = raw as u64;
                uint_to(raw_u.min(max_for_len(len)), len, *endian)
            }
            (DidCodec::Enum { map }, DidInput::EnumName(name)) => {
                let Some(len) = fixed else {
                    return Err(enc_err("enum without fixed length".into()));
                };
                let Some(e) = map.iter().find(|e| &e.name == name) else {
                    return Err(enc_err(format!("unknown enum name {name:?}")));
                };
                uint_to(e.value, len, Endian::Big)
            }
            (_, other) => return Err(enc_err(format!("{other:?} does not fit this codec"))),
        };
        if bytes.is_empty() {
            return Err(enc_err("empty value".into()));
        }
        self.check_len(bytes.len())?;
        Ok(bytes)
    }
}

const fn std_did(id: u16, name: &'static str) -> (u16, &'static str) {
    (id, name)
}

/// ISO 14229-1 Annex C.1 identifiers F180–F19E (F18F is not assigned a name here).
pub const STANDARD_DID_NAMES: &[(u16, &str)] = &[
    std_did(0xF180, "bootSoftwareIdentification"),
    std_did(0xF181, "applicationSoftwareIdentification"),
    std_did(0xF182, "applicationDataIdentification"),
    std_did(0xF183, "bootSoftwareFingerprint"),
    std_did(0xF184, "applicationSoftwareFingerprint"),
    std_did(0xF185, "applicationDataFingerprint"),
    std_did(0xF186, "activeDiagnosticSession"),
    std_did(0xF187, "vehicleManufacturerSparePartNumber"),
    std_did(0xF188, "vehicleManufacturerECUSoftwareNumber"),
    std_did(0xF189, "vehicleManufacturerECUSoftwareVersionNumber"),
    std_did(0xF18A, "systemSupplierIdentifier"),
    std_did(0xF18B, "ECUManufacturingDate"),
    std_did(0xF18C, "ECUSerialNumber"),
    std_did(0xF18D, "supportedFunctionalUnits"),
    std_did(0xF18E, "vehicleManufacturerKitAssemblyPartNumber"),
    std_did(0xF190, "VIN"),
    std_did(0xF191, "vehicleManufacturerECUHardwareNumber"),
    std_did(0xF192, "systemSupplierECUHardwareNumber"),
    std_did(0xF193, "systemSupplierECUHardwareVersionNumber"),
    std_did(0xF194, "systemSupplierECUSoftwareNumber"),
    std_did(0xF195, "systemSupplierECUSoftwareVersionNumber"),
    std_did(0xF196, "exhaustRegulationOrTypeApprovalNumber"),
    std_did(0xF197, "systemNameOrEngineType"),
    std_did(0xF198, "repairShopCodeOrTesterSerialNumber"),
    std_did(0xF199, "programmingDate"),
    std_did(
        0xF19A,
        "calibrationRepairShopCodeOrCalibrationEquipmentSerialNumber",
    ),
    std_did(0xF19B, "calibrationDate"),
    std_did(0xF19C, "calibrationEquipmentSoftwareNumber"),
    std_did(0xF19D, "ECUInstallationDate"),
    std_did(0xF19E, "ODXFile"),
];

/// Built-in definition for a standard DID: VIN is 17 ASCII, F186 is a 1-byte session id,
/// everything else is variable-length hex (format is manufacturer specific).
pub fn standard_did(id: u16) -> Option<DidDef> {
    let i = STANDARD_DID_NAMES.binary_search_by_key(&id, |(d, _)| *d).ok()?;
    let name = STANDARD_DID_NAMES[i].1.to_string();
    let (length, codec) = match id {
        0xF190 => (Some(17), DidCodec::Ascii),
        0xF186 => (
            Some(1),
            DidCodec::Uint {
                endian: Endian::Big,
                scale: 1.0,
                offset: 0.0,
                unit: None,
            },
        ),
        _ => (None, DidCodec::Hex),
    };
    Some(DidDef {
        id,
        name,
        length,
        codec,
        writable: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_table_sorted() {
        assert!(STANDARD_DID_NAMES.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(standard_did(0xF18F).is_none());
        for (id, _) in STANDARD_DID_NAMES {
            let def = standard_did(*id).expect("listed DID has a definition");
            def.validate().expect("built-in definitions are valid");
        }
    }
}
