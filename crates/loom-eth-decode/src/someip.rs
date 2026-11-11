//! SOME/IP and SOME/IP-SD decoder (direct implementation).
//!
//! `someip_parse` 0.7.2 was evaluated (P0 report): it parses headers and SD, but its SD iterators use
//! `unsafe`, `SdHeader` allocates owned vectors without our caps, and it does not expose byte offsets
//! for hex highlighting. The wire format is small, so it is parsed here with explicit caps.
//! Payload semantics (FIBEX/ARXML) are out of scope.

use std::net::{Ipv4Addr, Ipv6Addr};

use crate::model::{Decoded, Loc};
use crate::rd::{arr, be16, be24, be32, u8_at};
use crate::value::FieldValue;

/// SOME/IP-SD port.
pub const SOMEIP_SD_PORT: u16 = 30490;
/// SOME/IP header length (message id .. return code).
pub const HEADER_LEN: usize = 16;
/// Messages decoded from one UDP datagram. Over: ignored with a warning.
pub const MAX_MSGS_PER_DATAGRAM: usize = 64;
/// SD entries decoded per message. Over: ignored with a warning.
pub const MAX_SD_ENTRIES: usize = 64;
/// SD options decoded per message. Over: ignored with a warning.
pub const MAX_SD_OPTIONS: usize = 64;

const SD_SERVICE: u16 = 0xFFFF;
const SD_METHOD: u16 = 0x8100;

fn msg_type_name(t: u8) -> &'static str {
    match t {
        0x00 => "REQUEST",
        0x01 => "REQUEST_NO_RETURN",
        0x02 => "NOTIFICATION",
        0x80 => "RESPONSE",
        0x81 => "ERROR",
        0x20 => "TP_REQUEST",
        0x21 => "TP_REQUEST_NO_RETURN",
        0x22 => "TP_NOTIFICATION",
        0xA0 => "TP_RESPONSE",
        0xA1 => "TP_ERROR",
        _ => "UNKNOWN",
    }
}

fn entry_name(t: u8, ttl: u32) -> &'static str {
    match (t, ttl) {
        (0x00, _) => "FindService",
        (0x01, 0) => "StopOfferService",
        (0x01, _) => "OfferService",
        (0x06, 0) => "StopSubscribeEventgroup",
        (0x06, _) => "SubscribeEventgroup",
        (0x07, 0) => "SubscribeEventgroupNack",
        (0x07, _) => "SubscribeEventgroupAck",
        _ => "UnknownEntry",
    }
}

/// Stream framing: total length (header length field + 8), `Err` if implausible.
pub(crate) fn frame_len(b: &[u8]) -> Result<Option<usize>, &'static str> {
    let Some(len) = be32(b, 4) else { return Ok(None) };
    let len = len as usize;
    if len < 8 {
        return Err("SOME/IP length below 8");
    }
    // Over-limit lengths are returned as-is; the caller applies its own cap (stream: MAX_PDU_BYTES).
    Ok(Some(len.saturating_add(8)))
}

/// Strict check used before decoding an unknown-port payload as SOME/IP: every message must fit exactly.
pub(crate) fn looks_like(payload: &[u8]) -> bool {
    let mut off = 0usize;
    let mut n = 0usize;
    while off < payload.len() {
        let rest = payload.get(off..).unwrap_or(&[]);
        let Ok(Some(total)) = frame_len(rest) else {
            return false;
        };
        if total > rest.len() || total < HEADER_LEN || u8_at(rest, 12) != Some(1) {
            return false;
        }
        if msg_type_name(u8_at(rest, 14).unwrap_or(0xFF)) == "UNKNOWN" {
            return false;
        }
        off += total;
        n += 1;
        if n > MAX_MSGS_PER_DATAGRAM {
            return false;
        }
    }
    n > 0
}

/// Decodes one SOME/IP message (header + payload). Returns info text.
pub(crate) fn decode_msg(d: &mut Decoded, msg: &[u8], loc: Loc, first_seq: Option<u64>) -> String {
    if msg.len() < HEADER_LEN {
        d.warn(
            "malformed",
            "someip",
            format!("header needs 16 bytes, have {}", msg.len()),
        );
        return "SOME/IP [Malformed]".into();
    }
    let svc = be16(msg, 0).unwrap_or(0);
    let method = be16(msg, 2).unwrap_or(0);
    let len = be32(msg, 4).unwrap_or(0);
    let client = be16(msg, 8).unwrap_or(0);
    let session = be16(msg, 10).unwrap_or(0);
    let pv = u8_at(msg, 12).unwrap_or(0);
    let iv = u8_at(msg, 13).unwrap_or(0);
    let mt = u8_at(msg, 14).unwrap_or(0);
    let rc = u8_at(msg, 15).unwrap_or(0);
    let mtn = msg_type_name(mt);
    let l = d.push_layer(
        "someip",
        format!("SOME/IP 0x{svc:04X}.0x{method:04X} {mtn}"),
        loc.pos(0, msg.len()),
    );
    d.add(
        l,
        "someip.messageid",
        FieldValue::Uint(((u32::from(svc) << 16) | u32::from(method)).into()),
        loc.pos(0, 4),
    );
    d.add_full(
        l,
        "someip.serviceid",
        FieldValue::Uint(svc.into()),
        None,
        loc.pos(0, 2),
        1,
    );
    d.add_full(
        l,
        "someip.methodid",
        FieldValue::Uint(method.into()),
        None,
        loc.pos(2, 2),
        1,
    );
    d.add(l, "someip.length", FieldValue::Uint(len.into()), loc.pos(4, 4));
    d.add(
        l,
        "someip.clientid",
        FieldValue::Uint(client.into()),
        loc.pos(8, 2),
    );
    d.add(
        l,
        "someip.sessionid",
        FieldValue::Uint(session.into()),
        loc.pos(10, 2),
    );
    d.add(
        l,
        "someip.protoversion",
        FieldValue::Uint(pv.into()),
        loc.pos(12, 1),
    );
    d.add(
        l,
        "someip.interfaceversion",
        FieldValue::Uint(iv.into()),
        loc.pos(13, 1),
    );
    d.add_show(
        l,
        "someip.messagetype",
        FieldValue::Uint(mt.into()),
        format!("0x{mt:02X} ({mtn})"),
        loc.pos(14, 1),
    );
    d.add(
        l,
        "someip.returncode",
        FieldValue::Uint(rc.into()),
        loc.pos(15, 1),
    );
    if let Some(fs) = first_seq {
        d.add(l, "loom.pdu_first_seq", FieldValue::Uint(fs), None);
    }
    if (len as usize).saturating_add(8) != msg.len() {
        d.warn(
            "malformed",
            "someip",
            format!("length field {len}, message {}", msg.len()),
        );
    }
    if pv != 1 {
        d.warn("malformed", "someip", format!("protocol version {pv}"));
    }
    let payload = msg.get(HEADER_LEN..).unwrap_or(&[]);
    let ploc = Loc {
        buf: loc.buf,
        off: loc.off + HEADER_LEN,
    };
    if svc == SD_SERVICE && method == SD_METHOD {
        return decode_sd(d, payload, ploc);
    }
    if !payload.is_empty() {
        d.add(
            l,
            "someip.payload",
            FieldValue::Bytes(payload.to_vec()),
            ploc.pos(0, payload.len()),
        );
    }
    format!(
        "SOME/IP {mtn} service=0x{svc:04X} method=0x{method:04X} client=0x{client:04X} session=0x{session:04X}"
    )
}

fn decode_sd(d: &mut Decoded, p: &[u8], loc: Loc) -> String {
    let l = d.push_layer(
        "someipsd",
        "SOME/IP Service Discovery".into(),
        loc.pos(0, p.len()),
    );
    let (Some(flags), Some(elen)) = (u8_at(p, 0), be32(p, 4)) else {
        d.warn("malformed", "someipsd", "SD header needs 8 bytes".into());
        return "SOME/IP-SD [Malformed]".into();
    };
    d.add(l, "someipsd.flags", FieldValue::Uint(flags.into()), loc.pos(0, 1));
    d.add_full(
        l,
        "someipsd.flags.reboot",
        FieldValue::Bool(flags & 0x80 != 0),
        None,
        loc.pos(0, 1),
        1,
    );
    d.add_full(
        l,
        "someipsd.flags.unicast",
        FieldValue::Bool(flags & 0x40 != 0),
        None,
        loc.pos(0, 1),
        1,
    );
    d.add(
        l,
        "someipsd.length_entriesarray",
        FieldValue::Uint(elen.into()),
        loc.pos(4, 4),
    );
    let elen = elen as usize;
    let estart = 8usize;
    let Some(eend) = estart.checked_add(elen).filter(|&e| e <= p.len()) else {
        d.warn(
            "malformed",
            "someipsd",
            format!("entries length {elen} exceeds message"),
        );
        return "SOME/IP-SD [Malformed]".into();
    };
    if !elen.is_multiple_of(16) {
        d.warn(
            "malformed",
            "someipsd",
            format!("entries length {elen} not a multiple of 16"),
        );
    }
    let mut infos = Vec::new();
    let mut e = estart;
    while e + 16 <= eend {
        if infos.len() >= MAX_SD_ENTRIES {
            d.warn(
                "limit_fields",
                "someipsd",
                format!("more than {MAX_SD_ENTRIES} entries"),
            );
            break;
        }
        let t = u8_at(p, e).unwrap_or(0);
        let svc = be16(p, e + 4).unwrap_or(0);
        let inst = be16(p, e + 6).unwrap_or(0);
        let major = u8_at(p, e + 8).unwrap_or(0);
        let ttl = be24(p, e + 9).unwrap_or(0);
        let name = entry_name(t, ttl);
        d.add_show(
            l,
            "someipsd.entry.type",
            FieldValue::Uint(t.into()),
            format!("0x{t:02X} ({name})"),
            loc.pos(e, 1),
        );
        d.add_full(
            l,
            "someipsd.entry.serviceid",
            FieldValue::Uint(svc.into()),
            None,
            loc.pos(e + 4, 2),
            1,
        );
        d.add_full(
            l,
            "someipsd.entry.instanceid",
            FieldValue::Uint(inst.into()),
            None,
            loc.pos(e + 6, 2),
            1,
        );
        d.add_full(
            l,
            "someipsd.entry.majorver",
            FieldValue::Uint(major.into()),
            None,
            loc.pos(e + 8, 1),
            1,
        );
        d.add_full(
            l,
            "someipsd.entry.ttl",
            FieldValue::Uint(ttl.into()),
            None,
            loc.pos(e + 9, 3),
            1,
        );
        match t {
            0x06 | 0x07 => {
                let eg = be16(p, e + 14).unwrap_or(0);
                d.add_full(
                    l,
                    "someipsd.entry.eventgroupid",
                    FieldValue::Uint(eg.into()),
                    None,
                    loc.pos(e + 14, 2),
                    1,
                );
                infos.push(format!("{name} 0x{svc:04X}.{inst} eg=0x{eg:04X} ttl={ttl}"));
            }
            _ => {
                let minor = be32(p, e + 12).unwrap_or(0);
                d.add_full(
                    l,
                    "someipsd.entry.minorver",
                    FieldValue::Uint(minor.into()),
                    None,
                    loc.pos(e + 12, 4),
                    1,
                );
                infos.push(format!("{name} 0x{svc:04X}.{inst} ttl={ttl}"));
            }
        }
        e += 16;
    }
    if let Some(olen) = be32(p, eend) {
        d.add(
            l,
            "someipsd.length_optionsarray",
            FieldValue::Uint(olen.into()),
            loc.pos(eend, 4),
        );
        let ostart = eend + 4;
        match ostart.checked_add(olen as usize).filter(|&x| x <= p.len()) {
            Some(oend) => decode_sd_options(d, l, p, ostart, oend, loc),
            None => d.warn(
                "malformed",
                "someipsd",
                format!("options length {olen} exceeds message"),
            ),
        }
    }
    if infos.is_empty() {
        "SOME/IP-SD (no entries)".into()
    } else {
        format!("SOME/IP-SD {}", infos.join(", "))
    }
}

fn decode_sd_options(d: &mut Decoded, l: Option<usize>, p: &[u8], start: usize, end: usize, loc: Loc) {
    let mut o = start;
    let mut n = 0usize;
    while o + 3 <= end {
        if n >= MAX_SD_OPTIONS {
            d.warn(
                "limit_fields",
                "someipsd",
                format!("more than {MAX_SD_OPTIONS} options"),
            );
            return;
        }
        let olen = be16(p, o).unwrap_or(0) as usize;
        let ot = u8_at(p, o + 2).unwrap_or(0);
        let Some(next) = (o + 3).checked_add(olen).filter(|&x| x <= end) else {
            d.warn(
                "malformed",
                "someipsd",
                format!("option length {olen} exceeds options array"),
            );
            return;
        };
        d.add(
            l,
            "someipsd.option.type",
            FieldValue::Uint(ot.into()),
            loc.pos(o + 2, 1),
        );
        match ot {
            0x04 | 0x14 | 0x24 if olen >= 9 => {
                if let Some(a) = arr::<4>(p, o + 4) {
                    d.add_full(
                        l,
                        "someipsd.option.ipv4",
                        FieldValue::Ipv4(Ipv4Addr::from(a)),
                        None,
                        loc.pos(o + 4, 4),
                        1,
                    );
                }
                endpoint_tail(d, l, p, o + 9, loc);
            }
            0x06 | 0x16 | 0x26 if olen >= 21 => {
                if let Some(a) = arr::<16>(p, o + 4) {
                    d.add_full(
                        l,
                        "someipsd.option.ipv6",
                        FieldValue::Ipv6(Ipv6Addr::from(a)),
                        None,
                        loc.pos(o + 4, 16),
                        1,
                    );
                }
                endpoint_tail(d, l, p, o + 21, loc);
            }
            _ => {}
        }
        o = next;
        n += 1;
    }
}

/// `reserved(1) l4proto(1) port(2)` at `at - 1 ..`: proto at `at`, port at `at + 1`.
fn endpoint_tail(d: &mut Decoded, l: Option<usize>, p: &[u8], at: usize, loc: Loc) {
    if let Some(proto) = u8_at(p, at) {
        d.add_full(
            l,
            "someipsd.option.proto",
            FieldValue::Uint(proto.into()),
            None,
            loc.pos(at, 1),
            1,
        );
    }
    if let Some(port) = be16(p, at + 1) {
        d.add_full(
            l,
            "someipsd.option.port",
            FieldValue::Uint(port.into()),
            None,
            loc.pos(at + 1, 2),
            1,
        );
    }
}

/// Decodes every SOME/IP message in a UDP datagram.
pub(crate) fn decode_datagram(d: &mut Decoded, payload: &[u8], loc: Loc) -> Vec<String> {
    let mut infos = Vec::new();
    let mut off = 0usize;
    while off < payload.len() {
        if infos.len() >= MAX_MSGS_PER_DATAGRAM {
            d.warn(
                "limit_pdus",
                "someip",
                format!("more than {MAX_MSGS_PER_DATAGRAM} messages"),
            );
            break;
        }
        let rest = payload.get(off..).unwrap_or(&[]);
        let take = match frame_len(rest) {
            Ok(Some(t)) => t.min(rest.len()),
            _ => rest.len(),
        };
        infos.push(decode_msg(
            d,
            rest.get(..take).unwrap_or(rest),
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
