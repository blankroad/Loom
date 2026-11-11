//! ISO 14229-1 service table.

use serde::Serialize;

/// One UDS service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ServiceInfo {
    pub sid: u8,
    /// ISO 14229-1 service name.
    pub name: &'static str,
    /// The second byte is a sub-function whose bit 7 is the suppress-positive-response bit.
    pub has_subfunction: bool,
    /// Minimum valid request length in bytes, SID included.
    pub min_request_len: usize,
}

const fn s(sid: u8, name: &'static str, has_subfunction: bool, min_request_len: usize) -> ServiceInfo {
    ServiceInfo {
        sid,
        name,
        has_subfunction,
        min_request_len,
    }
}

/// Every request service defined by ISO 14229-1 (2013/2020), sorted by SID.
pub const SERVICES: &[ServiceInfo] = &[
    s(0x10, "DiagnosticSessionControl", true, 2),
    s(0x11, "ECUReset", true, 2),
    s(0x14, "ClearDiagnosticInformation", false, 4),
    s(0x19, "ReadDTCInformation", true, 2),
    s(0x22, "ReadDataByIdentifier", false, 3),
    s(0x23, "ReadMemoryByAddress", false, 4),
    s(0x24, "ReadScalingDataByIdentifier", false, 3),
    s(0x27, "SecurityAccess", true, 2),
    s(0x28, "CommunicationControl", true, 3),
    s(0x29, "Authentication", true, 2),
    s(0x2A, "ReadDataByPeriodicIdentifier", false, 2),
    s(0x2C, "DynamicallyDefineDataIdentifier", true, 2),
    s(0x2E, "WriteDataByIdentifier", false, 4),
    s(0x2F, "InputOutputControlByIdentifier", false, 4),
    s(0x31, "RoutineControl", true, 4),
    s(0x34, "RequestDownload", false, 5),
    s(0x35, "RequestUpload", false, 5),
    s(0x36, "TransferData", false, 2),
    s(0x37, "RequestTransferExit", false, 1),
    s(0x38, "RequestFileTransfer", false, 5),
    s(0x3D, "WriteMemoryByAddress", false, 5),
    s(0x3E, "TesterPresent", true, 2),
    s(0x83, "AccessTimingParameter", true, 2),
    s(0x84, "SecuredDataTransmission", false, 2),
    s(0x85, "ControlDTCSetting", true, 2),
    s(0x86, "ResponseOnEvent", true, 2),
    s(0x87, "LinkControl", true, 2),
];

/// Look up a request SID. Undefined SIDs (including response SIDs) return `None`.
pub fn service(sid: u8) -> Option<&'static ServiceInfo> {
    SERVICES
        .binary_search_by_key(&sid, |s| s.sid)
        .ok()
        .map(|i| &SERVICES[i])
}

/// Well-known SIDs used by builders and parsers.
pub mod ids {
    pub const DIAGNOSTIC_SESSION_CONTROL: u8 = 0x10;
    pub const ECU_RESET: u8 = 0x11;
    pub const CLEAR_DTC: u8 = 0x14;
    pub const READ_DTC: u8 = 0x19;
    pub const READ_DID: u8 = 0x22;
    pub const SECURITY_ACCESS: u8 = 0x27;
    pub const WRITE_DID: u8 = 0x2E;
    pub const IO_CONTROL: u8 = 0x2F;
    pub const ROUTINE_CONTROL: u8 = 0x31;
    pub const TRANSFER_DATA: u8 = 0x36;
    pub const TESTER_PRESENT: u8 = 0x3E;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(SERVICES.windows(2).all(|w| w[0].sid < w[1].sid));
    }

    #[test]
    fn lookup_every_byte() {
        let defined: Vec<u8> = (0..=255u8).filter(|b| service(*b).is_some()).collect();
        assert_eq!(defined.len(), SERVICES.len());
        assert!(service(0x62).is_none(), "response SIDs are not request services");
        assert_eq!(service(0x22).map(|s| s.name), Some("ReadDataByIdentifier"));
    }
}
