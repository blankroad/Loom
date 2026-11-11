//! Response classification and parsers.
//!
//! [`classify_response`] is the single function the transaction runner uses to decide what a
//! received PDU means for the request in flight. It never panics and never allocates.

use serde::Serialize;

use crate::db::DidLookup;
use crate::nrc::NRC_RESPONSE_PENDING;
use crate::sid::{ids, service};
use crate::{CodecError, MAX_PDU_LEN, NEGATIVE_RESPONSE_SID, POSITIVE_OFFSET};

/// What a received PDU means for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseKind {
    /// Positive response whose SID, sub-function and identifier echo match the request.
    Positive,
    /// `7F <sid> <nrc>` for this request, final (`nrc != 0x78`).
    Negative { nrc: u8 },
    /// `7F <sid> 78`: request accepted, final answer later (P2* applies).
    Pending,
    /// Well-formed response that belongs to some other request (other SID, sub-function or DID).
    /// The runner must not end the current transaction with it.
    Foreign { why: &'static str },
    /// Not a valid UDS response (empty, truncated, unknown response SID, bad negative length),
    /// or the request itself is not a valid UDS request.
    Malformed { why: &'static str },
}

impl ResponseKind {
    /// `Positive` or `Negative`: the transaction is over.
    pub fn is_final(&self) -> bool {
        matches!(self, ResponseKind::Positive | ResponseKind::Negative { .. })
    }
}

fn is_response_sid(b: u8) -> bool {
    b == NEGATIVE_RESPONSE_SID || b.checked_sub(POSITIVE_OFFSET).and_then(service).is_some()
}

/// Classify `resp` against the request `req` (both full UDS PDUs, SID first).
///
/// Echo checks on positive responses: sub-function (bit 7 masked) for sub-function services;
/// the first DID for `0x22`/`0x24`; DID for `0x2E`; DID and control parameter for `0x2F`;
/// routine id for `0x31`; block counter for `0x36`. A negative response must be exactly three
/// bytes and name this request's SID.
pub fn classify_response(req: &[u8], resp: &[u8]) -> ResponseKind {
    use ResponseKind::*;
    let Some(&req_sid) = req.first() else {
        return Malformed { why: "empty request" };
    };
    let Some(info) = service(req_sid) else {
        return Malformed {
            why: "request SID is not a UDS service",
        };
    };
    if req.len() < info.min_request_len {
        return Malformed {
            why: "request shorter than service minimum",
        };
    }
    let Some(&resp_sid) = resp.first() else {
        return Malformed {
            why: "empty response",
        };
    };
    if resp.len() > MAX_PDU_LEN {
        return Malformed {
            why: "response exceeds MAX_PDU_LEN",
        };
    }

    if resp_sid == NEGATIVE_RESPONSE_SID {
        if resp.len() != 3 {
            return Malformed {
                why: "negative response must be 3 bytes",
            };
        }
        if resp[1] != req_sid {
            return Foreign {
                why: "negative response for another SID",
            };
        }
        return if resp[2] == NRC_RESPONSE_PENDING {
            Pending
        } else {
            Negative { nrc: resp[2] }
        };
    }

    if !is_response_sid(resp_sid) {
        return Malformed {
            why: "unknown response SID",
        };
    }
    if resp_sid != req_sid.wrapping_add(POSITIVE_OFFSET) {
        return Foreign {
            why: "positive response for another SID",
        };
    }

    if info.has_subfunction {
        let Some(&sub) = resp.get(1) else {
            return Malformed {
                why: "positive response missing sub-function echo",
            };
        };
        if sub & 0x7F != req[1] & 0x7F {
            return Foreign {
                why: "sub-function echo differs",
            };
        }
    }

    match req_sid {
        ids::READ_DID | 0x24 => {
            if resp.len() < 3 {
                return Malformed {
                    why: "positive response missing DID echo",
                };
            }
            if resp[1..3] != req[1..3] {
                return Foreign {
                    why: "DID echo differs",
                };
            }
        }
        ids::WRITE_DID => {
            if resp.len() != 3 {
                return Malformed {
                    why: "WriteDataByIdentifier response must be 3 bytes",
                };
            }
            if resp[1..3] != req[1..3] {
                return Foreign {
                    why: "DID echo differs",
                };
            }
        }
        ids::IO_CONTROL => {
            if resp.len() < 4 {
                return Malformed {
                    why: "IO control response missing echo",
                };
            }
            if resp[1..4] != req[1..4] {
                return Foreign {
                    why: "DID or control parameter echo differs",
                };
            }
        }
        ids::ROUTINE_CONTROL => {
            if resp.len() < 4 {
                return Malformed {
                    why: "RoutineControl response missing routine id",
                };
            }
            if resp[2..4] != req[2..4] {
                return Foreign {
                    why: "routine id echo differs",
                };
            }
        }
        ids::TRANSFER_DATA => {
            if resp.len() < 2 {
                return Malformed {
                    why: "TransferData response missing block counter",
                };
            }
            if resp[1] != req[1] {
                return Foreign {
                    why: "block sequence counter differs",
                };
            }
        }
        ids::CLEAR_DTC => {
            if resp.len() != 1 {
                return Malformed {
                    why: "ClearDiagnosticInformation response must be 1 byte",
                };
            }
        }
        _ => {}
    }
    Positive
}

fn expect_sid(resp: &[u8], expected: u8) -> Result<(), CodecError> {
    match resp.first() {
        None => Err(CodecError::Truncated {
            what: "response",
            need: 1,
            got: 0,
        }),
        Some(&got) if got != expected => Err(CodecError::UnexpectedSid { expected, got }),
        Some(_) if resp.len() > MAX_PDU_LEN => Err(CodecError::TooLong {
            what: "response",
            len: resp.len(),
            max: MAX_PDU_LEN,
        }),
        Some(_) => Ok(()),
    }
}

/// Parsed `50 <session> [<P2:2> <P2*:2>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionResponse {
    pub session: u8,
    /// `(P2server_max ms, P2*server_max ms)`. `None` if the ECU sent the short 2-byte form.
    pub timing: Option<SessionTiming>,
}

/// Server timing from a `0x50` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionTiming {
    pub p2_ms: u16,
    /// Sent in 10 ms units, stored here in ms.
    pub p2_star_ms: u32,
}

/// Parse a DiagnosticSessionControl positive response.
pub fn parse_session_response(resp: &[u8]) -> Result<SessionResponse, CodecError> {
    expect_sid(resp, 0x50)?;
    match resp.len() {
        2 => Ok(SessionResponse {
            session: resp[1] & 0x7F,
            timing: None,
        }),
        6 => Ok(SessionResponse {
            session: resp[1] & 0x7F,
            timing: Some(SessionTiming {
                p2_ms: u16::from_be_bytes([resp[2], resp[3]]),
                p2_star_ms: u32::from(u16::from_be_bytes([resp[4], resp[5]])) * 10,
            }),
        }),
        got => Err(CodecError::LengthMismatch {
            what: "0x50 response",
            expected: 6,
            got,
        }),
    }
}

/// One DID record from a `0x62` response, undecoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DidRecord {
    pub did: u16,
    pub data: Vec<u8>,
}

/// Split a `0x62` response into the DIDs requested by `req` (a `0x22` request).
///
/// Every DID except the last needs a known length from `lookup`; the last DID takes the rest of
/// the response (and must match its known length, if any). With unknown lengths only a
/// single-DID request can be split.
pub fn parse_read_did_response(
    req: &[u8],
    resp: &[u8],
    lookup: &dyn DidLookup,
) -> Result<Vec<DidRecord>, CodecError> {
    if req.first() != Some(&ids::READ_DID) || req.len() < 3 || req.len().is_multiple_of(2) {
        return Err(CodecError::arg("not a ReadDataByIdentifier request"));
    }
    expect_sid(resp, 0x62)?;
    let wanted: Vec<u16> = req[1..]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let mut out = Vec::with_capacity(wanted.len());
    let mut pos = 1usize;
    for (i, &did) in wanted.iter().enumerate() {
        let Some(echo) = resp.get(pos..pos + 2) else {
            return Err(CodecError::Truncated {
                what: "0x62 response",
                need: pos + 2,
                got: resp.len(),
            });
        };
        let got = u16::from_be_bytes([echo[0], echo[1]]);
        if got != did {
            return Err(CodecError::DidEcho { expected: did, got });
        }
        pos += 2;
        let remaining = resp.len() - pos;
        let last = i + 1 == wanted.len();
        let len = match (lookup.did_len(did), last) {
            (Some(n), false) => n,
            (Some(n), true) if n == remaining => n,
            (Some(n), true) => {
                return Err(CodecError::LengthMismatch {
                    what: "DID data",
                    expected: n,
                    got: remaining,
                });
            }
            (None, true) => remaining,
            (None, false) => return Err(CodecError::UnknownDidLength { did }),
        };
        let Some(data) = resp.get(pos..pos + len) else {
            return Err(CodecError::Truncated {
                what: "DID data",
                need: pos + len,
                got: resp.len(),
            });
        };
        out.push(DidRecord {
            did,
            data: data.to_vec(),
        });
        pos += len;
    }
    Ok(out)
}

/// Most DTC records accepted from one `59 02` response (bounded by [`MAX_PDU_LEN`] anyway).
pub const MAX_DTCS: usize = (MAX_PDU_LEN - 3) / 4;

/// One DTC with its status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DtcRecord {
    /// 24-bit DTC value, raw.
    pub dtc: u32,
    pub status: u8,
}

/// Parsed `59 02 <availability mask> (<dtc:3> <status>)*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DtcReport {
    pub availability_mask: u8,
    pub dtcs: Vec<DtcRecord>,
}

/// Names of the DTC status bits, bit 0 first (ISO 14229-1 D.2).
pub const DTC_STATUS_BITS: [&str; 8] = [
    "testFailed",
    "testFailedThisOperationCycle",
    "pendingDTC",
    "confirmedDTC",
    "testNotCompletedSinceLastClear",
    "testFailedSinceLastClear",
    "testNotCompletedThisOperationCycle",
    "warningIndicatorRequested",
];

/// Parse a reportDTCByStatusMask positive response.
pub fn parse_dtc_by_status_response(resp: &[u8]) -> Result<DtcReport, CodecError> {
    expect_sid(resp, 0x59)?;
    if resp.len() < 3 {
        return Err(CodecError::Truncated {
            what: "0x59 02 response",
            need: 3,
            got: resp.len(),
        });
    }
    if resp[1] != 0x02 {
        return Err(CodecError::Decode {
            codec: "dtc",
            detail: format!("report type 0x{:02X}", resp[1]),
        });
    }
    let body = &resp[3..];
    if !body.len().is_multiple_of(4) {
        return Err(CodecError::Decode {
            codec: "dtc",
            detail: format!("{} record bytes is not a multiple of 4", body.len()),
        });
    }
    let dtcs = body
        .chunks_exact(4)
        .map(|c| DtcRecord {
            dtc: u32::from_be_bytes([0, c[0], c[1], c[2]]),
            status: c[3],
        })
        .collect();
    Ok(DtcReport {
        availability_mask: resp[2],
        dtcs,
    })
}

/// Parsed `67 <odd> <seed>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeedResponse {
    pub seed_sub: u8,
    pub seed: Vec<u8>,
}

impl SeedResponse {
    /// An all-zero seed means the level is already unlocked (ISO 14229-1 9.4).
    pub fn already_unlocked(&self) -> bool {
        self.seed.iter().all(|b| *b == 0)
    }
}

/// Parse a SecurityAccess requestSeed positive response.
pub fn parse_seed_response(resp: &[u8]) -> Result<SeedResponse, CodecError> {
    expect_sid(resp, 0x67)?;
    if resp.len() < 3 {
        return Err(CodecError::Truncated {
            what: "0x67 seed response",
            need: 3,
            got: resp.len(),
        });
    }
    if resp[1].is_multiple_of(2) {
        return Err(CodecError::Decode {
            codec: "seed",
            detail: "sub-function is not requestSeed".into(),
        });
    }
    Ok(SeedResponse {
        seed_sub: resp[1],
        seed: resp[2..].to_vec(),
    })
}
