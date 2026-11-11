//! ISO 14229-1 negative response codes.
//!
//! One table for UI, CLI and skills. Names follow ISO 14229-1 Table A.1 (2013) plus the
//! authentication NRCs added in the 2020 edition (0x34, 0x50–0x5D, 0x94). Ranges reserved by the
//! standard map to a range name; anything else is `None`.

/// NRC 0x78 requestCorrectlyReceived-ResponsePending: not a final answer.
pub const NRC_RESPONSE_PENDING: u8 = 0x78;

const NAMED: &[(u8, &str)] = &[
    (0x10, "generalReject"),
    (0x11, "serviceNotSupported"),
    (0x12, "subFunctionNotSupported"),
    (0x13, "incorrectMessageLengthOrInvalidFormat"),
    (0x14, "responseTooLong"),
    (0x21, "busyRepeatRequest"),
    (0x22, "conditionsNotCorrect"),
    (0x24, "requestSequenceError"),
    (0x25, "noResponseFromSubnetComponent"),
    (0x26, "failurePreventsExecutionOfRequestedAction"),
    (0x31, "requestOutOfRange"),
    (0x33, "securityAccessDenied"),
    (0x34, "authenticationRequired"),
    (0x35, "invalidKey"),
    (0x36, "exceededNumberOfAttempts"),
    (0x37, "requiredTimeDelayNotExpired"),
    (0x50, "certificateVerificationFailedInvalidTimePeriod"),
    (0x51, "certificateVerificationFailedInvalidSignature"),
    (0x52, "certificateVerificationFailedInvalidChainOfTrust"),
    (0x53, "certificateVerificationFailedInvalidType"),
    (0x54, "certificateVerificationFailedInvalidFormat"),
    (0x55, "certificateVerificationFailedInvalidContent"),
    (0x56, "certificateVerificationFailedInvalidScope"),
    (0x57, "certificateVerificationFailedInvalidCertificate"),
    (0x58, "ownershipVerificationFailed"),
    (0x59, "challengeCalculationFailed"),
    (0x5A, "settingAccessRightsFailed"),
    (0x5B, "sessionKeyCreationDerivationFailed"),
    (0x5C, "configurationDataUsageFailed"),
    (0x5D, "deAuthenticationFailed"),
    (0x70, "uploadDownloadNotAccepted"),
    (0x71, "transferDataSuspended"),
    (0x72, "generalProgrammingFailure"),
    (0x73, "wrongBlockSequenceCounter"),
    (0x78, "requestCorrectlyReceivedResponsePending"),
    (0x7E, "subFunctionNotSupportedInActiveSession"),
    (0x7F, "serviceNotSupportedInActiveSession"),
    (0x81, "rpmTooHigh"),
    (0x82, "rpmTooLow"),
    (0x83, "engineIsRunning"),
    (0x84, "engineIsNotRunning"),
    (0x85, "engineRunTimeTooLow"),
    (0x86, "temperatureTooHigh"),
    (0x87, "temperatureTooLow"),
    (0x88, "vehicleSpeedTooHigh"),
    (0x89, "vehicleSpeedTooLow"),
    (0x8A, "throttlePedalTooHigh"),
    (0x8B, "throttlePedalTooLow"),
    (0x8C, "transmissionRangeNotInNeutral"),
    (0x8D, "transmissionRangeNotInGear"),
    (0x8F, "brakeSwitchesNotClosed"),
    (0x90, "shifterLeverNotInPark"),
    (0x91, "torqueConverterClutchLocked"),
    (0x92, "voltageTooHigh"),
    (0x93, "voltageTooLow"),
    (0x94, "resourceTemporarilyNotAvailable"),
];

/// ISO name of a negative response code, e.g. `0x31 → "requestOutOfRange"`.
///
/// `0x38–0x4F` map to `reservedByExtendedDataLinkSecurityDocument` and `0xF0–0xFE` to
/// `vehicleManufacturerSpecificConditionsNotCorrect`; other unassigned values return `None`.
pub fn nrc_name(nrc: u8) -> Option<&'static str> {
    if let Ok(i) = NAMED.binary_search_by_key(&nrc, |(c, _)| *c) {
        return Some(NAMED[i].1);
    }
    match nrc {
        0x38..=0x4F => Some("reservedByExtendedDataLinkSecurityDocument"),
        0xF0..=0xFE => Some("vehicleManufacturerSpecificConditionsNotCorrect"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sorted_unique() {
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn known_and_unknown() {
        assert_eq!(nrc_name(0x31), Some("requestOutOfRange"));
        assert_eq!(nrc_name(0x78), Some("requestCorrectlyReceivedResponsePending"));
        assert_eq!(nrc_name(0x40), Some("reservedByExtendedDataLinkSecurityDocument"));
        assert_eq!(
            nrc_name(0xF3),
            Some("vehicleManufacturerSpecificConditionsNotCorrect")
        );
        assert_eq!(nrc_name(0x00), None);
        assert_eq!(nrc_name(0xFF), None);
        assert_eq!(nrc_name(0x15), None);
    }
}
