//! Minimal UDS (ISO 14229) interpretation of DoIP diagnostic payloads — **temporary**.
//!
//! UDS 팀 공유 코덱(`_xteam/eth-uds.md` §1, `uds::decode`)이 나오면 교체한다. Here: service names,
//! positive/negative response, NRC names, the DID of 22/62/2E/6E, the routine id of 31/71,
//! and the DTC count of a 59 02 response.

use crate::model::{Decoded, Loc};
use crate::rd::{be16, hex_spaced, printable, u8_at};
use crate::value::FieldValue;

/// DIDs listed from one 22 request. Over: the rest is not listed (warning).
pub const MAX_DIDS_PER_REQUEST: usize = 64;

pub(crate) fn service_name(sid: u8) -> Option<&'static str> {
    Some(match sid {
        0x10 => "DiagnosticSessionControl",
        0x11 => "ECUReset",
        0x14 => "ClearDiagnosticInformation",
        0x19 => "ReadDTCInformation",
        0x22 => "ReadDataByIdentifier",
        0x23 => "ReadMemoryByAddress",
        0x24 => "ReadScalingDataByIdentifier",
        0x27 => "SecurityAccess",
        0x28 => "CommunicationControl",
        0x29 => "Authentication",
        0x2A => "ReadDataByPeriodicIdentifier",
        0x2C => "DynamicallyDefineDataIdentifier",
        0x2E => "WriteDataByIdentifier",
        0x2F => "InputOutputControlByIdentifier",
        0x31 => "RoutineControl",
        0x34 => "RequestDownload",
        0x35 => "RequestUpload",
        0x36 => "TransferData",
        0x37 => "RequestTransferExit",
        0x38 => "RequestFileTransfer",
        0x3D => "WriteMemoryByAddress",
        0x3E => "TesterPresent",
        0x83 => "AccessTimingParameter",
        0x84 => "SecuredDataTransmission",
        0x85 => "ControlDTCSetting",
        0x86 => "ResponseOnEvent",
        0x87 => "LinkControl",
        _ => return None,
    })
}

pub(crate) fn nrc_name(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "generalReject",
        0x11 => "serviceNotSupported",
        0x12 => "subFunctionNotSupported",
        0x13 => "incorrectMessageLengthOrInvalidFormat",
        0x14 => "responseTooLong",
        0x21 => "busyRepeatRequest",
        0x22 => "conditionsNotCorrect",
        0x24 => "requestSequenceError",
        0x25 => "noResponseFromSubnetComponent",
        0x26 => "failurePreventsExecutionOfRequestedAction",
        0x31 => "requestOutOfRange",
        0x33 => "securityAccessDenied",
        0x35 => "invalidKey",
        0x36 => "exceedNumberOfAttempts",
        0x37 => "requiredTimeDelayNotExpired",
        0x70 => "uploadDownloadNotAccepted",
        0x71 => "transferDataSuspended",
        0x72 => "generalProgrammingFailure",
        0x73 => "wrongBlockSequenceCounter",
        0x78 => "requestCorrectlyReceived-ResponsePending",
        0x7E => "subFunctionNotSupportedInActiveSession",
        0x7F => "serviceNotSupportedInActiveSession",
        _ => "unknownNrc",
    }
}

fn has_subfunction(sid: u8) -> bool {
    matches!(
        sid,
        0x10 | 0x11 | 0x19 | 0x27 | 0x28 | 0x29 | 0x31 | 0x3E | 0x83 | 0x85 | 0x86 | 0x87
    )
}

/// Decodes UDS bytes into a `uds` layer. Returns the info text (`22 F1 90 ReadDataByIdentifier`).
pub(crate) fn decode(d: &mut Decoded, data: &[u8], loc: Loc) -> String {
    let Some(sid) = u8_at(data, 0) else {
        d.warn("malformed", "uds", "empty diagnostic payload".into());
        return "UDS [empty]".into();
    };
    let negative = sid == 0x7F;
    // Responses are SID + 0x40 (0x50..0x7E, 0xC3..0xC7); requests never collide with that set.
    let positive = !negative && sid >= 0x40 && service_name(sid - 0x40).is_some();
    let req_sid = if positive { sid - 0x40 } else { sid };
    let svc = if negative {
        "NegativeResponse"
    } else {
        service_name(req_sid).unwrap_or("UnknownService")
    };
    let title = if positive {
        format!("UDS {svc} positive response")
    } else {
        format!("UDS {svc}")
    };
    let l = d.push_layer("uds", title, loc.pos(0, data.len()));
    d.add_show(
        l,
        "uds.sid",
        FieldValue::Uint(sid.into()),
        format!("0x{sid:02X} ({svc})"),
        loc.pos(0, 1),
    );
    d.add(l, "uds.reply", FieldValue::Bool(positive || negative), None);

    if negative {
        let rej = u8_at(data, 1);
        let nrc = u8_at(data, 2);
        if let Some(r) = rej {
            let n = service_name(r).unwrap_or("UnknownService");
            d.add_show(
                l,
                "uds.err.sid",
                FieldValue::Uint(r.into()),
                format!("0x{r:02X} ({n})"),
                loc.pos(1, 1),
            );
        }
        match nrc {
            Some(c) => {
                let name = nrc_name(c);
                d.add_show(
                    l,
                    "uds.nrc",
                    FieldValue::Uint(c.into()),
                    format!("0x{c:02X} ({name})"),
                    loc.pos(2, 1),
                );
                return format!("{} NRC {name}", head(data, 3));
            }
            None => {
                d.warn(
                    "malformed",
                    "uds",
                    "negative response shorter than 3 bytes".into(),
                );
                return format!("{} NRC [Malformed]", head(data, 3));
            }
        }
    }

    if has_subfunction(req_sid)
        && let Some(sf) = u8_at(data, 1)
    {
        d.add(
            l,
            "uds.subfunction",
            FieldValue::Uint((sf & 0x7F).into()),
            loc.pos(1, 1),
        );
    }

    match (req_sid, positive) {
        (0x22, false) => {
            let mut n = 0usize;
            let mut off = 1usize;
            while let Some(did) = be16(data, off) {
                if n >= MAX_DIDS_PER_REQUEST {
                    d.warn(
                        "limit_fields",
                        "uds",
                        format!("more than {MAX_DIDS_PER_REQUEST} DIDs"),
                    );
                    break;
                }
                d.add(l, "uds.did", FieldValue::Uint(did.into()), loc.pos(off, 2));
                n += 1;
                off += 2;
            }
            format!("{} {svc}", hex_spaced(data, 1 + 2 * n.min(4)))
        }
        (0x22, true) => {
            if let Some(did) = be16(data, 1) {
                d.add(l, "uds.did", FieldValue::Uint(did.into()), loc.pos(1, 2));
            }
            let rest = data.get(3..).unwrap_or(&[]);
            match printable(rest) {
                Some(txt) => format!("{} \"{txt}\"", head(data, 3)),
                None if rest.is_empty() => head(data, 3),
                None => format!("{} {}", head(data, 3), hex_spaced(rest, 8)),
            }
        }
        (0x2E, _) => {
            if let Some(did) = be16(data, 1) {
                d.add(l, "uds.did", FieldValue::Uint(did.into()), loc.pos(1, 2));
            }
            let suffix = if positive { " positive" } else { "" };
            format!("{} {svc}{suffix}", head(data, 3))
        }
        (0x31, _) => {
            if let Some(rid) = be16(data, 2) {
                d.add(l, "uds.rid", FieldValue::Uint(rid.into()), loc.pos(2, 2));
            }
            let suffix = if positive { " positive" } else { "" };
            format!("{} {svc}{suffix}", head(data, 4))
        }
        (0x19, true) if u8_at(data, 1) == Some(0x02) => {
            let recs = data.len().saturating_sub(3);
            if recs.is_multiple_of(4) {
                let n = recs / 4;
                d.add(l, "uds.dtc_count", FieldValue::Uint(n as u64), None);
                format!("{} {svc} positive, {n} DTC", head(data, 3))
            } else {
                d.warn(
                    "malformed",
                    "uds",
                    format!("59 02 record bytes {recs} not a multiple of 4"),
                );
                format!("{} {svc} positive [Malformed]", head(data, 3))
            }
        }
        _ => {
            let suffix = if positive { " positive" } else { "" };
            format!("{} {svc}{suffix}", hex_spaced(data, 4))
        }
    }
}

/// Hex of the first `n` bytes without an ellipsis (the info text continues with decoded data).
fn head(data: &[u8], n: usize) -> String {
    hex_spaced(data.get(..n).unwrap_or(data), n)
}
