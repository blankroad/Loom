//! CAN event payloads and RPC parameters (schema v1 for M1).
//!
//! This is the shape promised to the UI in `docs/impl/requests/can-ui.md`; change it only
//! together with that file. Payloads are kind-specific objects (contract 02, E1): raw bytes in
//! `payload.bytes` (hex), write state in `payload.write_state` (E8). `dlc` is the **DLC code**
//! (0..=15); the byte count is `len`.

use loom_can_frame::{CanFrame, CanId, DeviceTimestamp, FrameError, TimestampSource, parse_frame};
use loom_proto::event::{Encoding, PayloadBytes, WriteState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Largest `count` of one `can.send` (finite burst). Longer streams are periodic jobs (M2).
pub const MAX_SEND_COUNT: u32 = 100;
/// Largest gap between burst frames.
pub const MAX_GAP_MS: u32 = 10_000;
/// Largest `expect.within_ms`.
pub const MAX_EXPECT_MS: u32 = 60_000;

/// Errors turning RPC parameters or payloads into frames.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchemaError {
    #[error("{0}")]
    Frame(#[from] FrameError),
    #[error("give either `frame` text or structured `id`/`data`, not both")]
    BothForms,
    #[error("missing frame: give `frame` text or `id`")]
    MissingFrame,
    #[error("`brs`/`esi` need an FD frame")]
    FdFlagOnClassic,
    #[error("bad hex data: {0}")]
    BadHex(String),
    #[error("{field} out of range (max {max})")]
    OutOfRange { field: &'static str, max: u32 },
    #[error("payload bytes: {0}")]
    Bytes(String),
}

/// Driver timestamp as carried in `payload.device_ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceTsPayload {
    /// Driver time in its own clock, decimal u64 string (contract 02: u64 as string).
    pub value_ns: String,
    /// `hardware` | `kernel_sw` | `host`.
    pub source: String,
    /// Clock domain id; never subtract values with different clock ids.
    pub clock_id: u32,
}

impl From<DeviceTimestamp> for DeviceTsPayload {
    fn from(t: DeviceTimestamp) -> Self {
        let source = match t.source {
            TimestampSource::Hardware => "hardware",
            TimestampSource::KernelSoftware => "kernel_sw",
            TimestampSource::Host => "host",
        };
        Self {
            value_ns: t.value_ns.to_string(),
            source: source.into(),
            clock_id: t.clock_id,
        }
    }
}

/// ISO-TP annotation on raw FF/CF/FC frames (R-TP-07, M2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IsoTpTag {
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// `can.frame` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FramePayload {
    /// CAN id (u32, ≤ 0x1FFFFFFF) as a JSON number.
    pub id: u32,
    /// 29-bit id.
    pub ext: bool,
    pub fd: bool,
    pub brs: bool,
    pub esi: bool,
    /// Classic remote frame.
    pub rtr: bool,
    /// DLC code 0..=15 (not the byte count).
    pub dlc: u8,
    /// Byte count 0..=64 (requested length for remote frames).
    pub len: u8,
    /// Data as lowercase hex (`""` for remote frames).
    pub bytes: PayloadBytes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_ts: Option<DeviceTsPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isotp: Option<IsoTpTag>,
    /// Index inside a multi-frame `can.send` (0-based).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_index: Option<u32>,
}

impl FramePayload {
    /// Payload for a frame. Flags are taken from the frame kind.
    pub fn from_frame(frame: &CanFrame, device_ts: Option<DeviceTimestamp>) -> Self {
        let (fd, brs, esi, rtr) = match frame.kind() {
            loom_can_frame::FrameKind::Classic { remote } => (false, false, false, remote),
            loom_can_frame::FrameKind::Fd { brs, esi } => (true, brs, esi, false),
        };
        Self {
            id: frame.id().raw(),
            ext: frame.id().is_extended(),
            fd,
            brs,
            esi,
            rtr,
            dlc: frame.dlc(),
            len: frame.len() as u8,
            bytes: PayloadBytes::hex(frame.data()),
            device_ts: device_ts.map(Into::into),
            isotp: None,
            frame_index: None,
        }
    }

    /// Rebuild the frame (validates every field).
    pub fn to_frame(&self) -> Result<CanFrame, SchemaError> {
        let id = if self.ext {
            CanId::extended(self.id)?
        } else {
            CanId::standard(self.id)?
        };
        if self.rtr {
            return Ok(CanFrame::remote(id, self.len as usize)?);
        }
        if self.bytes.encoding == Encoding::RawRef {
            return Err(SchemaError::Bytes("raw_ref not allowed for a frame".into()));
        }
        let data = self
            .bytes
            .decode()
            .map_err(|e| SchemaError::Bytes(e.to_string()))?;
        let frame = if self.fd {
            CanFrame::fd(id, &data, self.brs, self.esi)?
        } else {
            if self.brs || self.esi {
                return Err(SchemaError::FdFlagOnClassic);
            }
            CanFrame::classic(id, &data)?
        };
        if frame.dlc() != self.dlc || frame.len() != self.len as usize {
            return Err(SchemaError::Bytes(format!(
                "dlc/len {}/{} do not match {} data bytes",
                self.dlc,
                self.len,
                frame.len()
            )));
        }
        Ok(frame)
    }
}

/// `can.tx` payload: write state of one `can.send` request (one Trace row per `request_id`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TxPayload {
    pub write_state: WriteState,
    /// First frame of the request (summary for rows without an echo yet).
    pub frame: FramePayload,
    /// Frames in the request.
    pub count: u32,
    /// Frames handed to the driver so far.
    pub sent: u32,
    /// Error code if `failed` / `outcome_unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `expect` part of `can.send`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SendExpect {
    pub id: u32,
    #[serde(default)]
    pub ext: bool,
    pub within_ms: u32,
}

/// `can.send` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct SendParams {
    pub channel: String,
    /// cansend-style text (`300#01`, `123##1AABB`, `300#R`). Either this or `id`/`data`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    #[serde(default)]
    pub ext: bool,
    #[serde(default)]
    pub fd: bool,
    /// FD only. Defaults to true for FD frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brs: Option<bool>,
    #[serde(default)]
    pub esi: bool,
    /// Hex data (dots/spaces allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Frames to send (1..=MAX_SEND_COUNT).
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default)]
    pub gap_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<SendExpect>,
    #[serde(default)]
    pub dry_run: bool,
}

fn one() -> u32 {
    1
}

impl SendParams {
    /// The frame to send. Validates the either/or form, flags and bounds.
    pub fn resolve_frame(&self) -> Result<CanFrame, SchemaError> {
        if self.count == 0 || self.count > MAX_SEND_COUNT {
            return Err(SchemaError::OutOfRange {
                field: "count",
                max: MAX_SEND_COUNT,
            });
        }
        if self.gap_ms > MAX_GAP_MS {
            return Err(SchemaError::OutOfRange {
                field: "gap_ms",
                max: MAX_GAP_MS,
            });
        }
        if let Some(e) = &self.expect
            && e.within_ms > MAX_EXPECT_MS
        {
            return Err(SchemaError::OutOfRange {
                field: "expect.within_ms",
                max: MAX_EXPECT_MS,
            });
        }
        match (&self.frame, self.id) {
            (Some(_), Some(_)) => Err(SchemaError::BothForms),
            (None, None) => Err(SchemaError::MissingFrame),
            (Some(text), None) => {
                if self.data.is_some() || self.fd || self.brs.is_some() || self.esi {
                    return Err(SchemaError::BothForms);
                }
                Ok(parse_frame(text, self.ext)?)
            }
            (None, Some(raw)) => {
                let id = if self.ext {
                    CanId::extended(raw)?
                } else {
                    CanId::auto(raw)?
                };
                let data = parse_hex(self.data.as_deref().unwrap_or(""))?;
                if self.fd {
                    Ok(CanFrame::fd(id, &data, self.brs.unwrap_or(true), self.esi)?)
                } else {
                    if self.brs.is_some() || self.esi {
                        return Err(SchemaError::FdFlagOnClassic);
                    }
                    Ok(CanFrame::classic(id, &data)?)
                }
            }
        }
    }
}

fn parse_hex(s: &str) -> Result<Vec<u8>, SchemaError> {
    let clean: String = s.chars().filter(|c| !matches!(c, '.' | ' ' | ':')).collect();
    if !clean.len().is_multiple_of(2) || clean.len() / 2 > loom_can_frame::FD_MAX_LEN {
        return Err(SchemaError::BadHex(s.into()));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| SchemaError::BadHex(s.into())))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frame_payload_round_trip_and_shape() {
        let f = parse_frame("123##1000102030405060708090A0B", false).unwrap();
        let p = FramePayload::from_frame(&f, None);
        assert_eq!((p.dlc, p.len, p.fd, p.brs), (9, 12, true, true));
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["id"], json!(0x123));
        assert_eq!(
            v["bytes"],
            json!({"encoding": "hex", "data": "000102030405060708090a0b"})
        );
        assert!(v.get("device_ts").is_none());
        assert_eq!(p.to_frame().unwrap(), f);
    }

    #[test]
    fn remote_and_extended() {
        let f = parse_frame("18DAF110#R4", false).unwrap();
        let p = FramePayload::from_frame(&f, None);
        assert!(p.ext && p.rtr);
        assert_eq!((p.dlc, p.len), (4, 4));
        assert_eq!(p.to_frame().unwrap(), f);
    }

    #[test]
    fn payload_mismatch_is_rejected() {
        let mut p = FramePayload::from_frame(&parse_frame("300#0102", false).unwrap(), None);
        p.len = 3;
        assert!(p.to_frame().is_err());
        p.len = 2;
        p.brs = true;
        assert_eq!(p.to_frame(), Err(SchemaError::FdFlagOnClassic));
    }

    #[test]
    fn device_ts_is_string_u64() {
        let t = DeviceTimestamp {
            value_ns: u64::MAX,
            source: TimestampSource::KernelSoftware,
            clock_id: 7,
        };
        let v = serde_json::to_value(DeviceTsPayload::from(t)).unwrap();
        assert_eq!(
            v,
            json!({"value_ns": "18446744073709551615", "source": "kernel_sw", "clock_id": 7})
        );
    }

    #[test]
    fn send_params_forms() {
        let p: SendParams = serde_json::from_value(json!({"channel": "ch0", "frame": "300#01"})).unwrap();
        assert_eq!(p.count, 1);
        assert_eq!(p.resolve_frame().unwrap().to_string(), "300#01");

        let p: SendParams =
            serde_json::from_value(json!({"channel": "ch0", "id": 0x123, "fd": true, "data": "AA BB"}))
                .unwrap();
        let f = p.resolve_frame().unwrap();
        assert!(f.is_fd());
        assert_eq!(
            f.kind(),
            loom_can_frame::FrameKind::Fd {
                brs: true,
                esi: false
            }
        );

        for bad in [
            json!({"channel": "ch0"}),
            json!({"channel": "ch0", "frame": "300#01", "id": 1}),
            json!({"channel": "ch0", "frame": "300#01", "data": "01"}),
            json!({"channel": "ch0", "id": 1, "brs": true}),
            json!({"channel": "ch0", "frame": "300#01", "count": 0}),
            json!({"channel": "ch0", "frame": "300#01", "count": 101}),
            json!({"channel": "ch0", "id": 1, "data": "0"}),
            json!({"channel": "ch0", "frame": "300#01", "expect": {"id": 769, "within_ms": 60001}}),
        ] {
            let p: SendParams = serde_json::from_value(bad.clone()).unwrap();
            assert!(p.resolve_frame().is_err(), "{bad} should fail");
        }
    }
}
