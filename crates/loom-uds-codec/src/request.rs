//! Request builders (M1 services). Every builder checks its length limits before allocating.

use crate::sid::ids;
use crate::{CodecError, MAX_PDU_LEN, SPRMIB};

/// Most DIDs in one `0x22` request. More is rejected with [`CodecError::TooMany`].
pub const MAX_DIDS_PER_READ: usize = 64;

/// Highest SecurityAccess requestSeed sub-function (ISO: odd values up to 0x7D).
pub const MAX_SECURITY_SEED_SUB: u8 = 0x7D;

fn with_sprmib(sub: u8, suppress: bool) -> Result<u8, CodecError> {
    if sub & SPRMIB != 0 {
        return Err(CodecError::arg(format!(
            "sub-function 0x{sub:02X} has bit 7 set; pass suppress=true instead"
        )));
    }
    Ok(if suppress { sub | SPRMIB } else { sub })
}

fn check_len(len: usize) -> Result<(), CodecError> {
    if len > MAX_PDU_LEN {
        return Err(CodecError::TooLong {
            what: "request",
            len,
            max: MAX_PDU_LEN,
        });
    }
    Ok(())
}

/// `10 <session>` DiagnosticSessionControl.
pub fn session_control(session: u8, suppress: bool) -> Result<Vec<u8>, CodecError> {
    if session == 0 {
        return Err(CodecError::arg("session 0x00 is reserved"));
    }
    Ok(vec![
        ids::DIAGNOSTIC_SESSION_CONTROL,
        with_sprmib(session, suppress)?,
    ])
}

/// `11 <reset type>` ECUReset.
pub fn ecu_reset(reset_type: u8, suppress: bool) -> Result<Vec<u8>, CodecError> {
    if reset_type == 0 {
        return Err(CodecError::arg("reset type 0x00 is reserved"));
    }
    Ok(vec![ids::ECU_RESET, with_sprmib(reset_type, suppress)?])
}

/// `22 <did> [<did> …]` ReadDataByIdentifier (1 to [`MAX_DIDS_PER_READ`] DIDs).
pub fn read_dids(dids: &[u16]) -> Result<Vec<u8>, CodecError> {
    if dids.is_empty() {
        return Err(CodecError::arg("read-did needs at least one DID"));
    }
    if dids.len() > MAX_DIDS_PER_READ {
        return Err(CodecError::TooMany {
            what: "DIDs",
            count: dids.len(),
            max: MAX_DIDS_PER_READ,
        });
    }
    let mut out = Vec::with_capacity(1 + dids.len() * 2);
    out.push(ids::READ_DID);
    for did in dids {
        out.extend_from_slice(&did.to_be_bytes());
    }
    Ok(out)
}

/// `2E <did> <data>` WriteDataByIdentifier.
pub fn write_did(did: u16, data: &[u8]) -> Result<Vec<u8>, CodecError> {
    if data.is_empty() {
        return Err(CodecError::arg("write-did needs at least one data byte"));
    }
    check_len(3 + data.len())?;
    let mut out = Vec::with_capacity(3 + data.len());
    out.push(ids::WRITE_DID);
    out.extend_from_slice(&did.to_be_bytes());
    out.extend_from_slice(data);
    Ok(out)
}

/// `19 02 <mask>` ReadDTCInformation / reportDTCByStatusMask.
pub fn read_dtc_by_status_mask(mask: u8) -> Vec<u8> {
    vec![ids::READ_DTC, 0x02, mask]
}

/// `14 <group:3>` ClearDiagnosticInformation. `0xFFFFFF` = all groups.
pub fn clear_dtc(group: u32) -> Result<Vec<u8>, CodecError> {
    if group > 0x00FF_FFFF {
        return Err(CodecError::arg(format!("DTC group 0x{group:X} exceeds 24 bits")));
    }
    let b = group.to_be_bytes();
    Ok(vec![ids::CLEAR_DTC, b[1], b[2], b[3]])
}

/// `3E 00` / `3E 80` TesterPresent.
pub fn tester_present(suppress: bool) -> Vec<u8> {
    vec![ids::TESTER_PRESENT, if suppress { SPRMIB } else { 0x00 }]
}

fn check_seed_sub(seed_sub: u8) -> Result<(), CodecError> {
    if seed_sub == 0 || seed_sub.is_multiple_of(2) || seed_sub > MAX_SECURITY_SEED_SUB {
        return Err(CodecError::arg(format!(
            "requestSeed sub-function must be odd 0x01..=0x{MAX_SECURITY_SEED_SUB:02X}, got 0x{seed_sub:02X}"
        )));
    }
    Ok(())
}

/// `27 <odd>` SecurityAccess requestSeed.
pub fn security_request_seed(seed_sub: u8) -> Result<Vec<u8>, CodecError> {
    check_seed_sub(seed_sub)?;
    Ok(vec![ids::SECURITY_ACCESS, seed_sub])
}

/// `27 <odd+1> <key>` SecurityAccess sendKey for the level opened by `seed_sub`.
pub fn security_send_key(seed_sub: u8, key: &[u8]) -> Result<Vec<u8>, CodecError> {
    check_seed_sub(seed_sub)?;
    if key.is_empty() {
        return Err(CodecError::arg("sendKey needs a key"));
    }
    check_len(2 + key.len())?;
    let mut out = Vec::with_capacity(2 + key.len());
    out.push(ids::SECURITY_ACCESS);
    out.push(seed_sub + 1);
    out.extend_from_slice(key);
    Ok(out)
}

/// RoutineControl sub-functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineOp {
    Start = 0x01,
    Stop = 0x02,
    RequestResults = 0x03,
}

/// `31 <op> <rid> [<option record>]` RoutineControl.
pub fn routine_control(
    op: RoutineOp,
    rid: u16,
    option: &[u8],
    suppress: bool,
) -> Result<Vec<u8>, CodecError> {
    check_len(4 + option.len())?;
    let mut out = Vec::with_capacity(4 + option.len());
    out.push(ids::ROUTINE_CONTROL);
    out.push(with_sprmib(op as u8, suppress)?);
    out.extend_from_slice(&rid.to_be_bytes());
    out.extend_from_slice(option);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders() -> Result<(), CodecError> {
        assert_eq!(session_control(0x03, false)?, [0x10, 0x03]);
        assert_eq!(session_control(0x03, true)?, [0x10, 0x83]);
        assert!(session_control(0x83, false).is_err());
        assert_eq!(read_dids(&[0xF190, 0xF18C])?, [0x22, 0xF1, 0x90, 0xF1, 0x8C]);
        assert!(read_dids(&[]).is_err());
        assert!(read_dids(&[0u16; MAX_DIDS_PER_READ + 1]).is_err());
        assert_eq!(write_did(0xF1A0, &[0x01])?, [0x2E, 0xF1, 0xA0, 0x01]);
        assert!(write_did(0xF1A0, &vec![0u8; MAX_PDU_LEN]).is_err());
        assert_eq!(clear_dtc(0xFF_FFFF)?, [0x14, 0xFF, 0xFF, 0xFF]);
        assert!(clear_dtc(0x100_0000).is_err());
        assert_eq!(tester_present(true), [0x3E, 0x80]);
        assert_eq!(security_request_seed(0x01)?, [0x27, 0x01]);
        assert!(security_request_seed(0x02).is_err());
        assert_eq!(security_send_key(0x01, &[1, 2])?, [0x27, 0x02, 1, 2]);
        assert_eq!(
            routine_control(RoutineOp::Start, 0xFF01, &[], false)?,
            [0x31, 0x01, 0xFF, 0x01]
        );
        Ok(())
    }
}
