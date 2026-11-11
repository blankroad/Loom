//! DoIP (ISO 13400-2) decoder — **temporary**.
//!
//! UDS 팀 no-IO 코덱 crate가 나오면 이 모듈을 교체한다(`_xteam/eth-uds.md` §1). Until then this is the
//! minimal passive decoder for the eth packet view: header checks, the payload types listed below,
//! and UDS hand-off for diagnostic messages. Lengths are checked before any allocation.

use crate::model::{Decoded, Loc};
use crate::rd::{arr, be16, be32, printable, u8_at};
use crate::uds;
use crate::value::FieldValue;

/// TCP/UDP port for DoIP.
pub const DOIP_PORT: u16 = 13400;
/// Generic header length.
pub const HEADER_LEN: usize = 8;
/// DoIP messages decoded from one UDP datagram. Over: the rest is ignored with a warning.
pub const MAX_MSGS_PER_DATAGRAM: usize = 16;

pub(crate) fn type_name(t: u16) -> &'static str {
    match t {
        0x0000 => "Generic header NACK",
        0x0001 => "Vehicle identification request",
        0x0002 => "Vehicle identification request (EID)",
        0x0003 => "Vehicle identification request (VIN)",
        0x0004 => "Vehicle announcement",
        0x0005 => "Routing activation request",
        0x0006 => "Routing activation response",
        0x0007 => "Alive check request",
        0x0008 => "Alive check response",
        0x4001 => "Entity status request",
        0x4002 => "Entity status response",
        0x4003 => "Diagnostic power mode request",
        0x4004 => "Diagnostic power mode response",
        0x8001 => "Diagnostic message",
        0x8002 => "Diagnostic message ACK",
        0x8003 => "Diagnostic message NACK",
        _ => "Unknown payload type",
    }
}

/// Header check for stream framing: returns total PDU length (header + payload).
/// `Err` = not a DoIP header at this position (stream is out of sync).
pub(crate) fn frame_len(b: &[u8]) -> Result<Option<usize>, &'static str> {
    let (Some(v), Some(inv)) = (u8_at(b, 0), u8_at(b, 1)) else {
        return Ok(None);
    };
    if inv != !v {
        return Err("version/inverse mismatch");
    }
    let Some(len) = be32(b, 4) else { return Ok(None) };
    let len = len as usize;
    // Over-limit lengths are returned as-is; the caller applies its own cap (stream: MAX_PDU_BYTES).
    Ok(Some(HEADER_LEN.saturating_add(len)))
}

fn hex16(v: u16) -> String {
    format!("0x{v:04X}")
}

/// Decodes one complete DoIP PDU (`pdu` = header + payload). Returns the info string.
pub(crate) fn decode(d: &mut Decoded, pdu: &[u8], loc: Loc, first_seq: Option<u64>) -> String {
    let (Some(ver), Some(inv), Some(ty), Some(len)) =
        (u8_at(pdu, 0), u8_at(pdu, 1), be16(pdu, 2), be32(pdu, 4))
    else {
        d.warn(
            "malformed",
            "doip",
            format!("header needs 8 bytes, have {}", pdu.len()),
        );
        return "DoIP [Malformed]".into();
    };
    let name = type_name(ty);
    let l = d.push_layer(
        "doip",
        format!("DoIP v{ver} {name} (0x{ty:04X})"),
        loc.pos(0, pdu.len()),
    );
    d.add(l, "doip.version", FieldValue::Uint(ver.into()), loc.pos(0, 1));
    d.add(
        l,
        "doip.inverse_version",
        FieldValue::Uint(inv.into()),
        loc.pos(1, 1),
    );
    d.add_show(
        l,
        "doip.type",
        FieldValue::Uint(ty.into()),
        format!("0x{ty:04X} ({name})"),
        loc.pos(2, 2),
    );
    d.add(l, "doip.length", FieldValue::Uint(len.into()), loc.pos(4, 4));
    if let Some(fs) = first_seq {
        d.add(l, "loom.pdu_first_seq", FieldValue::Uint(fs), None);
    }
    if inv != !ver {
        d.warn("malformed", "doip", "version/inverse mismatch".into());
        return format!("DoIP [Malformed] {name}");
    }
    let body = pdu.get(HEADER_LEN..).unwrap_or(&[]);
    if body.len() != len as usize {
        d.warn(
            "malformed",
            "doip",
            format!("length field {len}, payload {}", body.len()),
        );
    }
    let b = Loc {
        buf: loc.buf,
        off: loc.off + HEADER_LEN,
    };
    let short = |d: &mut Decoded, need: usize| -> bool {
        if body.len() < need {
            d.warn(
                "malformed",
                "doip",
                format!("{name}: needs {need} bytes, have {}", body.len()),
            );
            true
        } else {
            false
        }
    };
    match ty {
        0x0000 => {
            if short(d, 1) {
                return format!("{name} (0x0000) [Malformed]");
            }
            let c = u8_at(body, 0).unwrap_or(0);
            d.add(
                l,
                "doip.header_nack_code",
                FieldValue::Uint(c.into()),
                b.pos(0, 1),
            );
            format!("{name} (0x0000) code=0x{c:02X}")
        }
        0x0002 => {
            if let Some(e) = arr::<6>(body, 0) {
                d.add(l, "doip.eid", FieldValue::Bytes(e.to_vec()), b.pos(0, 6));
            }
            format!("{name} (0x0002)")
        }
        0x0003 => {
            if let Some(v) = body.get(..17) {
                d.add(
                    l,
                    "doip.vin",
                    FieldValue::Str(String::from_utf8_lossy(v).into_owned()),
                    b.pos(0, 17),
                );
            }
            format!("{name} (0x0003)")
        }
        0x0004 => {
            if short(d, 32) {
                return format!("{name} (0x0004) [Malformed]");
            }
            let vin = body
                .get(..17)
                .map(|v| printable(v).unwrap_or_else(|| String::from_utf8_lossy(v).into_owned()));
            let la = be16(body, 17).unwrap_or(0);
            if let Some(v) = &vin {
                d.add(l, "doip.vin", FieldValue::Str(v.clone()), b.pos(0, 17));
            }
            d.add(
                l,
                "doip.logical_address",
                FieldValue::Uint(la.into()),
                b.pos(17, 2),
            );
            if let Some(e) = arr::<6>(body, 19) {
                d.add(l, "doip.eid", FieldValue::Bytes(e.to_vec()), b.pos(19, 6));
            }
            if let Some(g) = arr::<6>(body, 25) {
                d.add(l, "doip.gid", FieldValue::Bytes(g.to_vec()), b.pos(25, 6));
            }
            if let Some(fa) = u8_at(body, 31) {
                d.add(
                    l,
                    "doip.further_action",
                    FieldValue::Uint(fa.into()),
                    b.pos(31, 1),
                );
            }
            if let Some(s) = u8_at(body, 32) {
                d.add(l, "doip.vin_gid_sync", FieldValue::Uint(s.into()), b.pos(32, 1));
            }
            format!(
                "Vehicle announcement (0x0004) VIN={} LA={}",
                vin.unwrap_or_default(),
                hex16(la)
            )
        }
        0x0005 => {
            if short(d, 7) {
                return format!("{name} (0x0005) [Malformed]");
            }
            let sa = be16(body, 0).unwrap_or(0);
            let at = u8_at(body, 2).unwrap_or(0);
            d.add(l, "doip.source_address", FieldValue::Uint(sa.into()), b.pos(0, 2));
            d.add(
                l,
                "doip.activation_type",
                FieldValue::Uint(at.into()),
                b.pos(2, 1),
            );
            format!(
                "Routing activation request (0x0005) SA={} type=0x{at:02X}",
                hex16(sa)
            )
        }
        0x0006 => {
            if short(d, 9) {
                return format!("{name} (0x0006) [Malformed]");
            }
            let tester = be16(body, 0).unwrap_or(0);
            let entity = be16(body, 2).unwrap_or(0);
            let code = u8_at(body, 4).unwrap_or(0);
            d.add(
                l,
                "doip.target_address",
                FieldValue::Uint(tester.into()),
                b.pos(0, 2),
            );
            d.add(
                l,
                "doip.source_address",
                FieldValue::Uint(entity.into()),
                b.pos(2, 2),
            );
            d.add(
                l,
                "doip.response_code",
                FieldValue::Uint(code.into()),
                b.pos(4, 1),
            );
            let ok = if code == 0x10 { " OK" } else { "" };
            format!(
                "Routing activation response (0x0006) SA={} code=0x{code:02X}{ok}",
                hex16(tester)
            )
        }
        0x0008 => {
            if let Some(sa) = be16(body, 0) {
                d.add(l, "doip.source_address", FieldValue::Uint(sa.into()), b.pos(0, 2));
            }
            format!("{name} (0x0008)")
        }
        0x4002 => {
            if short(d, 3) {
                return format!("{name} (0x4002) [Malformed]");
            }
            let nt = u8_at(body, 0).unwrap_or(0);
            let mx = u8_at(body, 1).unwrap_or(0);
            let cur = u8_at(body, 2).unwrap_or(0);
            d.add(l, "doip.node_type", FieldValue::Uint(nt.into()), b.pos(0, 1));
            d.add(l, "doip.max_sockets", FieldValue::Uint(mx.into()), b.pos(1, 1));
            d.add(
                l,
                "doip.current_sockets",
                FieldValue::Uint(cur.into()),
                b.pos(2, 1),
            );
            if let Some(m) = be32(body, 3) {
                d.add(l, "doip.max_data_size", FieldValue::Uint(m.into()), b.pos(3, 4));
            }
            format!("{name} (0x4002) sockets {cur}/{mx}")
        }
        0x4004 => {
            if short(d, 1) {
                return format!("{name} (0x4004) [Malformed]");
            }
            let pm = u8_at(body, 0).unwrap_or(0);
            d.add(l, "doip.power_mode", FieldValue::Uint(pm.into()), b.pos(0, 1));
            format!("{name} (0x4004) mode=0x{pm:02X}")
        }
        0x8001..=0x8003 => {
            if short(d, if ty == 0x8001 { 4 } else { 5 }) {
                return format!("{name} (0x{ty:04X}) [Malformed]");
            }
            let sa = be16(body, 0).unwrap_or(0);
            let ta = be16(body, 2).unwrap_or(0);
            d.add(l, "doip.source_address", FieldValue::Uint(sa.into()), b.pos(0, 2));
            d.add(l, "doip.target_address", FieldValue::Uint(ta.into()), b.pos(2, 2));
            let arrow = format!("{}→{}", hex16(sa), hex16(ta));
            match ty {
                0x8001 => {
                    let data = body.get(4..).unwrap_or(&[]);
                    let uinfo = uds::decode(
                        d,
                        data,
                        Loc {
                            buf: b.buf,
                            off: b.off + 4,
                        },
                    );
                    format!("Diag {arrow} {uinfo}")
                }
                0x8002 => {
                    let c = u8_at(body, 4).unwrap_or(0);
                    d.add(l, "doip.ack_code", FieldValue::Uint(c.into()), b.pos(4, 1));
                    format!("Diag ACK (0x8002) {arrow}")
                }
                _ => {
                    let c = u8_at(body, 4).unwrap_or(0);
                    d.add(l, "doip.nack_code", FieldValue::Uint(c.into()), b.pos(4, 1));
                    format!("Diag NACK (0x8003) {arrow} code=0x{c:02X}")
                }
            }
        }
        _ => format!("{name} (0x{ty:04X})"),
    }
}

/// Decodes all DoIP messages in one UDP datagram payload.
pub(crate) fn decode_datagram(d: &mut Decoded, payload: &[u8], loc: Loc) -> Vec<String> {
    let mut infos = Vec::new();
    let mut off = 0usize;
    while off < payload.len() {
        if infos.len() >= MAX_MSGS_PER_DATAGRAM {
            d.warn(
                "limit_pdus",
                "doip",
                format!("more than {MAX_MSGS_PER_DATAGRAM} messages in datagram"),
            );
            break;
        }
        let rest = payload.get(off..).unwrap_or(&[]);
        let total = match frame_len(rest) {
            Ok(Some(t)) => t,
            Ok(None) | Err(_) => rest.len(),
        };
        let take = total.min(rest.len());
        let pdu = rest.get(..take).unwrap_or(rest);
        infos.push(decode(
            d,
            pdu,
            Loc {
                buf: loc.buf,
                off: loc.off + off,
            },
            None,
        ));
        if take == 0 {
            break;
        }
        off += take;
    }
    infos
}
