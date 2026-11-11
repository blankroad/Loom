//! ISO 13400-2 generic header and payload codec.
//!
//! Pure functions. [`parse_header`] validates version, payload type and length **before** any
//! payload buffer exists, so a hostile `u32::MAX` length never reaches an allocator.
//! [`decode_frame`] is the streaming form used by fuzz tests and the UDP path.

use thiserror::Error;

/// Generic DoIP header length.
pub const HEADER_LEN: usize = 8;

/// Largest UDS PDU Loom sends or accepts over DoIP (ISO 15765-2 classic limit, plan §3.8).
pub const MAX_UDS_PDU: usize = 4095;

/// Largest DoIP payload accepted, in bytes: source + target address (4) + [`MAX_UDS_PDU`].
///
/// Every other payload type is shorter (vehicle announcement 33, routing activation 13), and a
/// diagnostic ACK/NACK may echo at most the previous diagnostic message, which is also bounded
/// by this value. Over the limit: generic header NACK `0x02` and the TCP connection is closed
/// (skipping an attacker-chosen length of up to 4 GiB would be unbounded work).
pub const MAX_DOIP_PAYLOAD: usize = 4 + MAX_UDS_PDU;

/// Largest DoIP frame (header + payload).
pub const MAX_DOIP_FRAME: usize = HEADER_LEN + MAX_DOIP_PAYLOAD;

/// UDP/TCP port for DoIP (ISO 13400-2 `UDP_DISCOVERY` / `TCP_DATA`).
pub const DOIP_PORT: u16 = 13400;

/// Default protocol version byte allowed only on vehicle identification requests.
pub const VERSION_DEFAULT_VIR: u8 = 0xFF;

/// DoIP protocol version written by Loom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolVersion {
    /// ISO 13400-2:2012 (`0x02`).
    #[default]
    Iso2012,
    /// ISO 13400-2:2019 (`0x03`).
    Iso2019,
}

impl ProtocolVersion {
    /// Wire byte.
    pub fn byte(self) -> u8 {
        match self {
            ProtocolVersion::Iso2012 => 0x02,
            ProtocolVersion::Iso2019 => 0x03,
        }
    }
}

/// Payload type values (ISO 13400-2 Table 17).
pub mod payload_type {
    pub const GENERIC_NACK: u16 = 0x0000;
    pub const VEHICLE_ID_REQUEST: u16 = 0x0001;
    pub const VEHICLE_ID_REQUEST_EID: u16 = 0x0002;
    pub const VEHICLE_ID_REQUEST_VIN: u16 = 0x0003;
    pub const VEHICLE_ANNOUNCEMENT: u16 = 0x0004;
    pub const ROUTING_ACTIVATION_REQUEST: u16 = 0x0005;
    pub const ROUTING_ACTIVATION_RESPONSE: u16 = 0x0006;
    pub const ALIVE_CHECK_REQUEST: u16 = 0x0007;
    pub const ALIVE_CHECK_RESPONSE: u16 = 0x0008;
    pub const ENTITY_STATUS_REQUEST: u16 = 0x4001;
    pub const ENTITY_STATUS_RESPONSE: u16 = 0x4002;
    pub const POWER_MODE_REQUEST: u16 = 0x4003;
    pub const POWER_MODE_RESPONSE: u16 = 0x4004;
    pub const DIAGNOSTIC_MESSAGE: u16 = 0x8001;
    pub const DIAGNOSTIC_ACK: u16 = 0x8002;
    pub const DIAGNOSTIC_NACK: u16 = 0x8003;
}

/// Generic header NACK codes (ISO 13400-2 Table 19).
pub mod header_nack {
    /// Incorrect pattern format — close socket.
    pub const INCORRECT_PATTERN: u8 = 0x00;
    /// Unknown payload type — discard message.
    pub const UNKNOWN_PAYLOAD_TYPE: u8 = 0x01;
    /// Message too large.
    pub const MESSAGE_TOO_LARGE: u8 = 0x02;
    /// Out of memory.
    pub const OUT_OF_MEMORY: u8 = 0x03;
    /// Invalid payload length — close socket.
    pub const INVALID_PAYLOAD_LENGTH: u8 = 0x04;
}

/// Diagnostic message ACK/NACK codes (ISO 13400-2 Tables 27/29).
pub mod diag_code {
    /// Routing confirmation ACK.
    pub const ACK: u8 = 0x00;
    pub const INVALID_SOURCE_ADDRESS: u8 = 0x02;
    pub const UNKNOWN_TARGET_ADDRESS: u8 = 0x03;
    pub const MESSAGE_TOO_LARGE: u8 = 0x04;
    pub const OUT_OF_MEMORY: u8 = 0x05;
    pub const TARGET_UNREACHABLE: u8 = 0x06;
    pub const UNKNOWN_NETWORK: u8 = 0x07;
    pub const TRANSPORT_PROTOCOL_ERROR: u8 = 0x08;
}

/// Routing activation type (ISO 13400-2 Table 47). Stored as the raw byte so OEM values survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivationType(pub u8);

impl ActivationType {
    pub const DEFAULT: ActivationType = ActivationType(0x00);
    pub const WWH_OBD: ActivationType = ActivationType(0x01);
    /// Central security. **0xE0** — some published crates use `0x02`, which is wrong.
    pub const CENTRAL_SECURITY: ActivationType = ActivationType(0xE0);

    /// Types the Loom entity accepts (`0x00`, `0x01`, `0xE0`); others get response code `0x06`.
    pub fn is_supported(self) -> bool {
        matches!(self.0, 0x00 | 0x01 | 0xE0)
    }
}

/// Routing activation response code (ISO 13400-2:2019 Table 48). Raw byte; constants name the
/// ISO values. `0x02` is "SA differs from the SA already activated on this socket".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivationCode(pub u8);

impl ActivationCode {
    pub const DENIED_UNKNOWN_SA: ActivationCode = ActivationCode(0x00);
    pub const DENIED_ALL_SOCKETS_ACTIVE: ActivationCode = ActivationCode(0x01);
    pub const DENIED_SA_MISMATCH_ON_SOCKET: ActivationCode = ActivationCode(0x02);
    pub const DENIED_SA_ACTIVE_ON_OTHER_SOCKET: ActivationCode = ActivationCode(0x03);
    pub const DENIED_MISSING_AUTHENTICATION: ActivationCode = ActivationCode(0x04);
    pub const DENIED_REJECTED_CONFIRMATION: ActivationCode = ActivationCode(0x05);
    pub const DENIED_UNSUPPORTED_TYPE: ActivationCode = ActivationCode(0x06);
    pub const DENIED_REQUIRES_TLS: ActivationCode = ActivationCode(0x07);
    pub const SUCCESS: ActivationCode = ActivationCode(0x10);
    pub const CONFIRMATION_REQUIRED: ActivationCode = ActivationCode(0x11);

    /// Only `0x10` activates routing. `0x11` means "not yet: confirmation pending".
    pub fn is_success(self) -> bool {
        self.0 == 0x10
    }

    /// Short ISO name for logs and the UI.
    pub fn name(self) -> &'static str {
        match self.0 {
            0x00 => "denied_unknown_source_address",
            0x01 => "denied_all_sockets_active",
            0x02 => "denied_source_address_mismatch_on_socket",
            0x03 => "denied_source_address_active_on_other_socket",
            0x04 => "denied_missing_authentication",
            0x05 => "denied_rejected_confirmation",
            0x06 => "denied_unsupported_activation_type",
            0x07 => "denied_requires_tls",
            0x10 => "success",
            0x11 => "confirmation_required",
            0xE0..=0xFE => "vm_specific",
            _ => "reserved",
        }
    }
}

/// Validated generic header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    pub payload_type: u16,
    pub payload_len: usize,
}

/// Header rejection, mapped to a generic header NACK code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HeaderError {
    #[error("incorrect DoIP header pattern")]
    IncorrectPattern,
    #[error("unknown payload type 0x{payload_type:04X}")]
    UnknownPayloadType { payload_type: u16, payload_len: usize },
    #[error("payload length {payload_len} exceeds {MAX_DOIP_PAYLOAD}")]
    MessageTooLarge { payload_len: u64 },
    #[error("invalid payload length {payload_len} for type 0x{payload_type:04X}")]
    InvalidPayloadLength { payload_type: u16, payload_len: usize },
}

impl HeaderError {
    /// Generic header NACK code to send.
    pub fn nack_code(&self) -> u8 {
        match self {
            HeaderError::IncorrectPattern => header_nack::INCORRECT_PATTERN,
            HeaderError::UnknownPayloadType { .. } => header_nack::UNKNOWN_PAYLOAD_TYPE,
            HeaderError::MessageTooLarge { .. } => header_nack::MESSAGE_TOO_LARGE,
            HeaderError::InvalidPayloadLength { .. } => header_nack::INVALID_PAYLOAD_LENGTH,
        }
    }

    /// TCP: whether the connection must be closed after the NACK. Only an unknown payload type
    /// (whose bounded body can be skipped) keeps the stream in sync.
    pub fn closes_connection(&self) -> bool {
        !matches!(self, HeaderError::UnknownPayloadType { .. })
    }
}

/// Valid payload lengths per known type; `None` = unknown type.
fn length_valid(payload_type: u16, len: usize) -> Option<bool> {
    use payload_type::*;
    Some(match payload_type {
        GENERIC_NACK => len == 1,
        VEHICLE_ID_REQUEST | ALIVE_CHECK_REQUEST | ENTITY_STATUS_REQUEST | POWER_MODE_REQUEST => len == 0,
        VEHICLE_ID_REQUEST_EID => len == 6,
        VEHICLE_ID_REQUEST_VIN => len == 17,
        VEHICLE_ANNOUNCEMENT => len == 32 || len == 33,
        ROUTING_ACTIVATION_REQUEST => len == 7 || len == 11,
        ROUTING_ACTIVATION_RESPONSE => len == 9 || len == 13,
        ALIVE_CHECK_RESPONSE => len == 2,
        ENTITY_STATUS_RESPONSE => len == 3 || len == 7,
        POWER_MODE_RESPONSE => len == 1,
        DIAGNOSTIC_MESSAGE | DIAGNOSTIC_ACK | DIAGNOSTIC_NACK => len >= 5,
        _ => return None,
    })
}

fn is_vehicle_id_request(payload_type: u16) -> bool {
    matches!(
        payload_type,
        payload_type::VEHICLE_ID_REQUEST
            | payload_type::VEHICLE_ID_REQUEST_EID
            | payload_type::VEHICLE_ID_REQUEST_VIN
    )
}

/// Validate an 8-byte generic header. Check order follows ISO 13400-2 Figure 7: pattern →
/// payload type → maximum length → type-specific length. Never allocates.
pub fn parse_header(b: &[u8; HEADER_LEN]) -> Result<Header, HeaderError> {
    let version = b[0];
    if b[1] != !version {
        return Err(HeaderError::IncorrectPattern);
    }
    let payload_type = u16::from_be_bytes([b[2], b[3]]);
    let version_ok = matches!(version, 0x02 | 0x03)
        || (version == VERSION_DEFAULT_VIR && is_vehicle_id_request(payload_type));
    if !version_ok {
        return Err(HeaderError::IncorrectPattern);
    }
    let raw_len = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
    let Ok(payload_len) = usize::try_from(raw_len) else {
        return Err(HeaderError::MessageTooLarge {
            payload_len: u64::from(raw_len),
        });
    };
    let Some(valid) = length_valid(payload_type, payload_len) else {
        return Err(HeaderError::UnknownPayloadType {
            payload_type,
            payload_len,
        });
    };
    if payload_len > MAX_DOIP_PAYLOAD {
        return Err(HeaderError::MessageTooLarge {
            payload_len: u64::from(raw_len),
        });
    }
    if !valid {
        return Err(HeaderError::InvalidPayloadLength {
            payload_type,
            payload_len,
        });
    }
    Ok(Header {
        version,
        payload_type,
        payload_len,
    })
}

/// Vehicle identification response / announcement (payload type `0x0004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleAnnouncement {
    pub vin: [u8; 17],
    pub logical_address: u16,
    pub eid: [u8; 6],
    pub gid: [u8; 6],
    pub further_action: u8,
    /// VIN/GID sync status (optional byte).
    pub sync_status: Option<u8>,
}

/// One decoded DoIP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    GenericNack {
        code: u8,
    },
    VehicleIdRequest,
    VehicleIdRequestEid {
        eid: [u8; 6],
    },
    VehicleIdRequestVin {
        vin: [u8; 17],
    },
    VehicleAnnouncement(VehicleAnnouncement),
    RoutingActivationRequest {
        source: u16,
        activation_type: ActivationType,
        oem: Option<[u8; 4]>,
    },
    RoutingActivationResponse {
        tester: u16,
        entity: u16,
        code: ActivationCode,
        oem: Option<[u8; 4]>,
    },
    AliveCheckRequest,
    AliveCheckResponse {
        source: u16,
    },
    EntityStatusRequest,
    EntityStatusResponse {
        node_type: u8,
        max_sockets: u8,
        open_sockets: u8,
        max_data_size: Option<u32>,
    },
    PowerModeRequest,
    PowerModeResponse {
        mode: u8,
    },
    /// Diagnostic message; `data` is the opaque UDS PDU.
    DiagnosticMessage {
        source: u16,
        target: u16,
        data: Vec<u8>,
    },
    DiagnosticAck {
        source: u16,
        target: u16,
        code: u8,
        previous: Vec<u8>,
    },
    DiagnosticNack {
        source: u16,
        target: u16,
        code: u8,
        previous: Vec<u8>,
    },
}

/// Encoding failure (only oversize payloads).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("DoIP payload of {len} bytes exceeds {MAX_DOIP_PAYLOAD}")]
pub struct EncodeError {
    pub len: usize,
}

fn u16_at(p: &[u8], i: usize) -> u16 {
    match (p.get(i), p.get(i + 1)) {
        (Some(&a), Some(&b)) => u16::from_be_bytes([a, b]),
        _ => 0,
    }
}

fn array_at<const N: usize>(p: &[u8], i: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(src) = p.get(i..i + N) {
        out.copy_from_slice(src);
    }
    out
}

impl Message {
    /// Payload type of this message.
    pub fn payload_type(&self) -> u16 {
        use payload_type::*;
        match self {
            Message::GenericNack { .. } => GENERIC_NACK,
            Message::VehicleIdRequest => VEHICLE_ID_REQUEST,
            Message::VehicleIdRequestEid { .. } => VEHICLE_ID_REQUEST_EID,
            Message::VehicleIdRequestVin { .. } => VEHICLE_ID_REQUEST_VIN,
            Message::VehicleAnnouncement(_) => VEHICLE_ANNOUNCEMENT,
            Message::RoutingActivationRequest { .. } => ROUTING_ACTIVATION_REQUEST,
            Message::RoutingActivationResponse { .. } => ROUTING_ACTIVATION_RESPONSE,
            Message::AliveCheckRequest => ALIVE_CHECK_REQUEST,
            Message::AliveCheckResponse { .. } => ALIVE_CHECK_RESPONSE,
            Message::EntityStatusRequest => ENTITY_STATUS_REQUEST,
            Message::EntityStatusResponse { .. } => ENTITY_STATUS_RESPONSE,
            Message::PowerModeRequest => POWER_MODE_REQUEST,
            Message::PowerModeResponse { .. } => POWER_MODE_RESPONSE,
            Message::DiagnosticMessage { .. } => DIAGNOSTIC_MESSAGE,
            Message::DiagnosticAck { .. } => DIAGNOSTIC_ACK,
            Message::DiagnosticNack { .. } => DIAGNOSTIC_NACK,
        }
    }

    /// Decode a payload of a known type. The length rules of [`parse_header`] are re-checked, so
    /// this is safe on any input.
    pub fn decode(payload_type: u16, p: &[u8]) -> Result<Message, HeaderError> {
        use payload_type::*;
        let payload_len = p.len();
        match length_valid(payload_type, payload_len) {
            None => {
                return Err(HeaderError::UnknownPayloadType {
                    payload_type,
                    payload_len,
                });
            }
            Some(false) => {
                return Err(HeaderError::InvalidPayloadLength {
                    payload_type,
                    payload_len,
                });
            }
            Some(true) if payload_len > MAX_DOIP_PAYLOAD => {
                return Err(HeaderError::MessageTooLarge {
                    payload_len: payload_len as u64,
                });
            }
            Some(true) => {}
        }
        let oem4 = |at: usize| (p.len() >= at + 4).then(|| array_at::<4>(p, at));
        Ok(match payload_type {
            GENERIC_NACK => Message::GenericNack { code: p[0] },
            VEHICLE_ID_REQUEST => Message::VehicleIdRequest,
            VEHICLE_ID_REQUEST_EID => Message::VehicleIdRequestEid { eid: array_at(p, 0) },
            VEHICLE_ID_REQUEST_VIN => Message::VehicleIdRequestVin { vin: array_at(p, 0) },
            VEHICLE_ANNOUNCEMENT => Message::VehicleAnnouncement(VehicleAnnouncement {
                vin: array_at(p, 0),
                logical_address: u16_at(p, 17),
                eid: array_at(p, 19),
                gid: array_at(p, 25),
                further_action: p[31],
                sync_status: p.get(32).copied(),
            }),
            ROUTING_ACTIVATION_REQUEST => Message::RoutingActivationRequest {
                source: u16_at(p, 0),
                activation_type: ActivationType(p[2]),
                oem: oem4(7),
            },
            ROUTING_ACTIVATION_RESPONSE => Message::RoutingActivationResponse {
                tester: u16_at(p, 0),
                entity: u16_at(p, 2),
                code: ActivationCode(p[4]),
                oem: oem4(9),
            },
            ALIVE_CHECK_REQUEST => Message::AliveCheckRequest,
            ALIVE_CHECK_RESPONSE => Message::AliveCheckResponse { source: u16_at(p, 0) },
            ENTITY_STATUS_REQUEST => Message::EntityStatusRequest,
            ENTITY_STATUS_RESPONSE => Message::EntityStatusResponse {
                node_type: p[0],
                max_sockets: p[1],
                open_sockets: p[2],
                max_data_size: oem4(3).map(u32::from_be_bytes),
            },
            POWER_MODE_REQUEST => Message::PowerModeRequest,
            POWER_MODE_RESPONSE => Message::PowerModeResponse { mode: p[0] },
            DIAGNOSTIC_MESSAGE => Message::DiagnosticMessage {
                source: u16_at(p, 0),
                target: u16_at(p, 2),
                data: p[4..].to_vec(),
            },
            DIAGNOSTIC_ACK => Message::DiagnosticAck {
                source: u16_at(p, 0),
                target: u16_at(p, 2),
                code: p[4],
                previous: p[5..].to_vec(),
            },
            DIAGNOSTIC_NACK => Message::DiagnosticNack {
                source: u16_at(p, 0),
                target: u16_at(p, 2),
                code: p[4],
                previous: p[5..].to_vec(),
            },
            _ => {
                return Err(HeaderError::UnknownPayloadType {
                    payload_type,
                    payload_len,
                });
            }
        })
    }

    /// Append the payload bytes to `out`.
    fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Message::GenericNack { code } => out.push(*code),
            Message::VehicleIdRequest
            | Message::AliveCheckRequest
            | Message::EntityStatusRequest
            | Message::PowerModeRequest => {}
            Message::VehicleIdRequestEid { eid } => out.extend_from_slice(eid),
            Message::VehicleIdRequestVin { vin } => out.extend_from_slice(vin),
            Message::VehicleAnnouncement(a) => {
                out.extend_from_slice(&a.vin);
                out.extend_from_slice(&a.logical_address.to_be_bytes());
                out.extend_from_slice(&a.eid);
                out.extend_from_slice(&a.gid);
                out.push(a.further_action);
                if let Some(s) = a.sync_status {
                    out.push(s);
                }
            }
            Message::RoutingActivationRequest {
                source,
                activation_type,
                oem,
            } => {
                out.extend_from_slice(&source.to_be_bytes());
                out.push(activation_type.0);
                out.extend_from_slice(&[0; 4]);
                if let Some(o) = oem {
                    out.extend_from_slice(o);
                }
            }
            Message::RoutingActivationResponse {
                tester,
                entity,
                code,
                oem,
            } => {
                out.extend_from_slice(&tester.to_be_bytes());
                out.extend_from_slice(&entity.to_be_bytes());
                out.push(code.0);
                out.extend_from_slice(&[0; 4]);
                if let Some(o) = oem {
                    out.extend_from_slice(o);
                }
            }
            Message::AliveCheckResponse { source } => out.extend_from_slice(&source.to_be_bytes()),
            Message::EntityStatusResponse {
                node_type,
                max_sockets,
                open_sockets,
                max_data_size,
            } => {
                out.extend_from_slice(&[*node_type, *max_sockets, *open_sockets]);
                if let Some(m) = max_data_size {
                    out.extend_from_slice(&m.to_be_bytes());
                }
            }
            Message::PowerModeResponse { mode } => out.push(*mode),
            Message::DiagnosticMessage { source, target, data } => {
                out.extend_from_slice(&source.to_be_bytes());
                out.extend_from_slice(&target.to_be_bytes());
                out.extend_from_slice(data);
            }
            Message::DiagnosticAck {
                source,
                target,
                code,
                previous,
            }
            | Message::DiagnosticNack {
                source,
                target,
                code,
                previous,
            } => {
                out.extend_from_slice(&source.to_be_bytes());
                out.extend_from_slice(&target.to_be_bytes());
                out.push(*code);
                out.extend_from_slice(previous);
            }
        }
    }

    fn payload_len(&self) -> usize {
        match self {
            Message::DiagnosticMessage { data, .. } => 4 + data.len(),
            Message::DiagnosticAck { previous, .. } | Message::DiagnosticNack { previous, .. } => {
                5 + previous.len()
            }
            Message::VehicleAnnouncement(a) => 32 + usize::from(a.sync_status.is_some()),
            _ => 13,
        }
    }

    /// Encode header + payload with the given version byte. Fails only if the payload would
    /// exceed [`MAX_DOIP_PAYLOAD`] (checked before the frame buffer is allocated).
    pub fn encode_with_version(&self, version: u8) -> Result<Vec<u8>, EncodeError> {
        let estimate = self.payload_len();
        if estimate > MAX_DOIP_PAYLOAD {
            return Err(EncodeError { len: estimate });
        }
        let mut out = Vec::with_capacity(HEADER_LEN + estimate);
        out.extend_from_slice(&[version, !version]);
        out.extend_from_slice(&self.payload_type().to_be_bytes());
        out.extend_from_slice(&[0; 4]);
        self.write_payload(&mut out);
        let len = out.len() - HEADER_LEN;
        if len > MAX_DOIP_PAYLOAD {
            return Err(EncodeError { len });
        }
        let len32 = u32::try_from(len).map_err(|_| EncodeError { len })?;
        out[4..8].copy_from_slice(&len32.to_be_bytes());
        Ok(out)
    }

    /// Encode with a protocol version.
    pub fn encode(&self, version: ProtocolVersion) -> Result<Vec<u8>, EncodeError> {
        self.encode_with_version(version.byte())
    }
}

/// Streaming decode of the first frame in `buf`.
///
/// * `Ok(None)` — need more bytes (the header, if complete, was already validated).
/// * `Ok(Some((header, message, consumed)))` — one frame.
/// * `Err(e)` — header or payload invalid; see [`HeaderError::closes_connection`].
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Header, Message, usize)>, HeaderError> {
    let Some(head) = buf.first_chunk::<HEADER_LEN>() else {
        return Ok(None);
    };
    let header = parse_header(head)?;
    let end = HEADER_LEN + header.payload_len;
    let Some(payload) = buf.get(HEADER_LEN..end) else {
        return Ok(None);
    };
    let message = Message::decode(header.payload_type, payload)?;
    Ok(Some((header, message, end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn central_security_is_e0() {
        assert_eq!(ActivationType::CENTRAL_SECURITY.0, 0xE0);
        assert!(ActivationType::CENTRAL_SECURITY.is_supported());
        assert!(!ActivationType(0x02).is_supported());
    }

    #[test]
    fn u32_max_length_rejected_from_header_alone() {
        let h = [0x02, 0xFD, 0x80, 0x01, 0xFF, 0xFF, 0xFF, 0xFF];
        let e = parse_header(&h).expect_err("u32::MAX must be rejected");
        assert_eq!(e.nack_code(), header_nack::MESSAGE_TOO_LARGE);
        assert!(e.closes_connection());
        // decode_frame on the bare header must error, not wait for 4 GiB.
        assert!(decode_frame(&h).is_err());
    }
}
