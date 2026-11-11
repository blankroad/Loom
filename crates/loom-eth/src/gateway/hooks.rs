//! Write hooks (plan §10, F1: classes are shown and audited, nothing is blocked by default).
//! Synchronous, no I/O, well under 1 ms (decoding one frame takes microseconds).
//!
//! | operation | class |
//! |---|---|
//! | capture start/stop | R0 (observation, audited) |
//! | send → sim, replay or loopback | R1 |
//! | send → real NIC | R2 |
//! | broadcast / multicast destination | R3 |
//! | raw DoIP diagnostic message (0x8001) | the inner UDS service class ([`uds_risk`]) |

use std::net::{IpAddr, Ipv4Addr};

use loom_domain_api::{Classification, Grant, PresendError, WriteHooks, WriteRequest};
use loom_eth_decode::{FieldValue, StreamTable, decode, linktype};
use loom_proto::event::Risk;
use serde_json::Value;

use super::common::ChannelBook;

/// UDS service class for raw DoIP diagnostic frames sent through `eth.send`, so that `eth`
/// cannot be used to show a lower class than `loom uds` would (UDS plan §10.3 table, lower bound
/// without a diagnostic DB). `sub` is the sub-function byte when the service has one.
pub fn uds_risk(sid: u8, sub: Option<u8>, rest: &[u8]) -> (Risk, &'static str) {
    let sf = sub.map(|s| s & 0x7F);
    match sid {
        0x10 => match sf {
            Some(0x01 | 0x03) => (Risk::R1, "UDS 0x10 default/extended session"),
            _ => (Risk::R3, "UDS 0x10 programming/safety/OEM session"),
        },
        0x11 => match sf {
            Some(0x04 | 0x05) => (Risk::R3, "UDS 0x11 rapid power shutdown"),
            _ => (Risk::R2, "UDS 0x11 ECUReset"),
        },
        0x14 => (Risk::R2, "UDS 0x14 ClearDiagnosticInformation"),
        0x19 | 0x22 | 0x24 => (Risk::R0, "UDS read service"),
        0x23 => (Risk::R1, "UDS 0x23 ReadMemoryByAddress (audited)"),
        0x27 => match sf {
            Some(s) if s % 2 == 1 => (Risk::R1, "UDS 0x27 requestSeed"),
            _ => (Risk::R3, "UDS 0x27 sendKey"),
        },
        0x28 => match sf {
            Some(0x00) => (Risk::R1, "UDS 0x28 enableRxAndTx"),
            _ => (Risk::R2, "UDS 0x28 disable communication"),
        },
        0x29 => (Risk::R3, "UDS 0x29 Authentication"),
        0x2C => (Risk::R1, "UDS 0x2C DynamicallyDefineDataIdentifier"),
        0x2E => match rest.get(..2) {
            Some([0xF1, 0x90 | 0x8C]) => (Risk::R3, "UDS 0x2E identification DID"),
            _ => (Risk::R2, "UDS 0x2E WriteDataByIdentifier"),
        },
        0x2F => match rest.get(2) {
            Some(0x00) => (Risk::R1, "UDS 0x2F returnControlToECU"),
            _ => (Risk::R3, "UDS 0x2F IO control"),
        },
        0x31 => match rest.get(1..3) {
            Some([0xFF, 0x00]) => (Risk::R4, "UDS 0x31 FF00 eraseMemory"),
            Some([0xFF, 0x01]) => (Risk::R1, "UDS 0x31 FF01 checkProgrammingDependencies"),
            _ => (Risk::R3, "UDS 0x31 RoutineControl (no DB)"),
        },
        0x34..=0x38 => (Risk::R4, "UDS 0x34–0x38 transfer/flash"),
        0x3D => (Risk::R4, "UDS 0x3D WriteMemoryByAddress"),
        0x3E => (Risk::R0, "UDS 0x3E TesterPresent"),
        0x84 => (Risk::R3, "UDS 0x84 SecuredDataTransmission"),
        0x85 => match sf {
            Some(0x01) => (Risk::R1, "UDS 0x85 DTC setting on"),
            _ => (Risk::R2, "UDS 0x85 DTC setting off"),
        },
        0x87 => (Risk::R3, "UDS 0x87 LinkControl"),
        0xBA..=0xBE => (Risk::R3, "UDS supplier-defined service"),
        _ => (Risk::R4, "UDS reserved/undefined SID"),
    }
}

fn has_subfunction(sid: u8) -> bool {
    matches!(
        sid,
        0x10 | 0x11 | 0x19 | 0x27 | 0x28 | 0x29 | 0x31 | 0x3E | 0x83 | 0x85 | 0x86 | 0x87
    )
}

fn max_risk(a: Risk, b: Risk) -> Risk {
    if (b as u8) > (a as u8) { b } else { a }
}

/// Parsed send target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SockTo {
    pub ip: IpAddr,
    pub port: u16,
}

pub(crate) fn parse_to(s: &str) -> Option<SockTo> {
    let sa: std::net::SocketAddr = s.parse().ok()?;
    Some(SockTo {
        ip: sa.ip(),
        port: sa.port(),
    })
}

/// Largest frame or payload `eth.send` accepts (jumbo frame). Over: not classifiable → usage error.
pub const MAX_SEND_BYTES: usize = 9216;

pub(crate) fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if s.len() > MAX_SEND_BYTES * 3 {
        return None;
    }
    let clean: String = s.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if !clean.len().is_multiple_of(2) || clean.len() > MAX_SEND_BYTES * 2 {
        return None;
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| clean.get(i..i + 2).and_then(|g| u8::from_str_radix(g, 16).ok()))
        .collect()
}

/// Ethernet + IPv4 + UDP/TCP frame around a socket payload, for previews and classification.
/// Checksums are zero (decoding does not verify them).
pub(crate) fn synth_frame(kind: &str, to: SockTo, from_port: u16, payload: &[u8]) -> Option<Vec<u8>> {
    let IpAddr::V4(dst) = to.ip else { return None };
    let tcp = kind == "tcp";
    let l4 = if tcp { 20 } else { 8 };
    let total = u16::try_from(20 + l4 + payload.len()).ok()?;
    let mut f = Vec::with_capacity(14 + usize::from(total));
    f.extend([0xFF; 6]);
    f.extend([2, 0, 0, 0, 0, 1]);
    f.extend([0x08, 0x00]);
    f.extend([
        0x45,
        0,
        (total >> 8) as u8,
        total as u8,
        0,
        0,
        0x40,
        0,
        64,
        if tcp { 6 } else { 17 },
        0,
        0,
    ]);
    f.extend(Ipv4Addr::UNSPECIFIED.octets());
    f.extend(dst.octets());
    f.extend(from_port.to_be_bytes());
    f.extend(to.port.to_be_bytes());
    if tcp {
        f.extend([0, 0, 0, 1, 0, 0, 0, 0, 0x50, 0x18, 0xFF, 0xFF, 0, 0, 0, 0]);
    } else {
        let ul = u16::try_from(8 + payload.len()).ok()?;
        f.extend(ul.to_be_bytes());
        f.extend([0, 0]);
    }
    f.extend(payload);
    Some(f)
}

/// The frame a send will put on the wire (raw) or a synthetic one around a socket payload.
pub(crate) fn send_frame(args: &Value) -> Option<Vec<u8>> {
    let kind = args.get("kind").and_then(Value::as_str)?;
    let bytes = hex_bytes(args.get("hex").and_then(Value::as_str)?)?;
    match kind {
        "raw" => Some(bytes),
        "udp" | "tcp" => {
            let to = parse_to(args.get("to").and_then(Value::as_str)?)?;
            let fp = args
                .get("from_port")
                .and_then(Value::as_u64)
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(0);
            synth_frame(kind, to, fp, &bytes)
        }
        _ => None,
    }
}

/// Class and reason of an `eth.send`. `local` = the channel stays on this host (sim/replay/loopback).
pub fn classify_send(args: &Value, local: bool) -> (Risk, String) {
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
    let to = args.get("to").and_then(Value::as_str).and_then(parse_to);
    let mut risk = if local { Risk::R1 } else { Risk::R2 };
    let mut reasons: Vec<String> = vec![if local {
        "send on sim/replay/loopback".into()
    } else {
        "send on a real NIC".into()
    }];
    if let Some(t) = to {
        if t.ip.is_loopback() {
            risk = Risk::R1;
            reasons = vec!["send to loopback".into()];
        }
        let bcast = match t.ip {
            IpAddr::V4(a) => a.is_broadcast() || a.is_multicast() || a.octets()[3] == 255,
            IpAddr::V6(a) => a.is_multicast(),
        };
        if bcast {
            risk = max_risk(risk, Risk::R3);
            reasons.push("broadcast/multicast destination".into());
        }
    }
    let Some(frame) = send_frame(args) else {
        return (risk, reasons.join("; "));
    };
    if kind == "raw" && frame.first().is_some_and(|b| b & 1 == 1) {
        risk = max_risk(risk, Risk::R3);
        reasons.push("broadcast/multicast MAC".into());
    }
    // Raw DoIP diagnostic frames: class of the UDS service inside (never lower than `loom uds`).
    let mut streams = StreamTable::new();
    let d = decode(linktype::ETHERNET, &frame, &Default::default(), &mut streams);
    let mut uds: Option<(Risk, &'static str)> = None;
    for l in d.layers.iter().filter(|l| l.name == "uds") {
        let bytes = d.layer_bytes(l);
        let Some(&sid) = bytes.first() else { continue };
        let sub = if has_subfunction(sid) {
            bytes.get(1).copied()
        } else {
            None
        };
        let (r, why) = uds_risk(sid, sub, bytes.get(1..).unwrap_or(&[]));
        if uds.is_none_or(|(u, _)| (r as u8) > (u as u8)) {
            uds = Some((r, why));
        }
    }
    if let Some((r, why)) = uds {
        risk = max_risk(risk, r);
        reasons.push(format!("raw DoIP diagnostic message: {why}"));
    } else if d.field_values("doip.type").any(|v| v == FieldValue::Uint(0x8001)) {
        reasons.push("raw DoIP diagnostic message without UDS payload".into());
    }
    (risk, reasons.join("; "))
}

/// Write hooks of the eth domain.
#[derive(Debug, Clone, Default)]
pub struct EthHooks {
    pub(crate) book: ChannelBook,
}

impl WriteHooks for EthHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        match req.method {
            "eth.send" => {
                let local = req
                    .target
                    .channel
                    .as_deref()
                    .and_then(|c| self.book.get(c))
                    .is_some_and(|i| i.is_local());
                let (risk, reason) = classify_send(req.args, local);
                Classification {
                    risk,
                    reason,
                    block: None,
                }
            }
            m => Classification {
                risk: Risk::R0,
                reason: format!("{m}: capture control (observation, audited)"),
                block: None,
            },
        }
    }

    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        if req.method != "eth.send" {
            return Ok(());
        }
        let Some(ch) = req.target.channel.as_deref() else {
            return Ok(());
        };
        let Some(info) = self.book.get(ch) else {
            return Err(PresendError::Locked(format!("capture channel {ch} is not open")));
        };
        if matches!(info.state.as_str(), "stopped" | "error" | "permission_denied") {
            return Err(PresendError::Locked(format!(
                "capture channel {ch} is {}",
                info.state
            )));
        }
        match (req.target.generation, info.generation) {
            (Some(want), Some(have)) if want != have => Err(PresendError::EpochChanged(format!(
                "channel {ch} generation {have}, request made against {want}"
            ))),
            _ => Ok(()),
        }
    }
}
