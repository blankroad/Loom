//! `classify_response` behaviour, including the counter-examples the runner depends on.

use loom_uds_codec::{ResponseKind, classify_response};

fn kind(req: &[u8], resp: &[u8]) -> ResponseKind {
    classify_response(req, resp)
}

#[test]
fn positive_and_negative() {
    assert_eq!(
        kind(&[0x22, 0xF1, 0x90], &[0x62, 0xF1, 0x90, b'L']),
        ResponseKind::Positive
    );
    assert_eq!(
        kind(&[0x22, 0xF1, 0x90], &[0x7F, 0x22, 0x31]),
        ResponseKind::Negative { nrc: 0x31 }
    );
    assert_eq!(
        kind(&[0x10, 0x03], &[0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]),
        ResponseKind::Positive
    );
    assert_eq!(
        kind(&[0x2E, 0xF1, 0xA0, 0x02], &[0x6E, 0xF1, 0xA0]),
        ResponseKind::Positive
    );
    assert_eq!(kind(&[0x14, 0xFF, 0xFF, 0xFF], &[0x54]), ResponseKind::Positive);
}

#[test]
fn pending_is_not_final() {
    let k = kind(&[0x31, 0x01, 0xFF, 0x01], &[0x7F, 0x31, 0x78]);
    assert_eq!(k, ResponseKind::Pending);
    assert!(!k.is_final());
}

#[test]
fn other_did_echo_is_foreign() {
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x62, 0xF1, 0x8C, 1]),
        ResponseKind::Foreign { .. }
    ));
    assert!(matches!(
        kind(&[0x2E, 0xF1, 0xA0, 1], &[0x6E, 0xF1, 0xA1]),
        ResponseKind::Foreign { .. }
    ));
}

#[test]
fn positive_for_other_sid_after_pending_is_foreign() {
    // 0x78 for our request, then a positive answer that belongs to an earlier 0x22.
    let req = [0x31, 0x01, 0xFF, 0x01];
    assert_eq!(kind(&req, &[0x7F, 0x31, 0x78]), ResponseKind::Pending);
    assert!(matches!(
        kind(&req, &[0x62, 0xF1, 0x90, 0x41]),
        ResponseKind::Foreign { .. }
    ));
}

#[test]
fn negative_for_other_sid_is_foreign() {
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x7F, 0x2E, 0x31]),
        ResponseKind::Foreign { .. }
    ));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x7F, 0x2E, 0x78]),
        ResponseKind::Foreign { .. }
    ));
}

#[test]
fn subfunction_and_routine_echo() {
    assert!(matches!(
        kind(&[0x10, 0x03], &[0x50, 0x01]),
        ResponseKind::Foreign { .. }
    ));
    // SPRMIB set in request: echo compared without bit 7.
    assert_eq!(kind(&[0x3E, 0x80], &[0x7E, 0x00]), ResponseKind::Positive);
    assert!(matches!(
        kind(&[0x31, 0x01, 0xFF, 0x01], &[0x71, 0x01, 0xFF, 0x02]),
        ResponseKind::Foreign { .. }
    ));
    assert!(matches!(
        kind(&[0x36, 0x05, 1, 2], &[0x76, 0x04]),
        ResponseKind::Foreign { .. }
    ));
}

#[test]
fn malformed() {
    assert!(matches!(kind(&[], &[0x62]), ResponseKind::Malformed { .. }));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[]),
        ResponseKind::Malformed { .. }
    ));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x7F, 0x22]),
        ResponseKind::Malformed { .. }
    ));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x7F, 0x22, 0x31, 0x00]),
        ResponseKind::Malformed { .. }
    ));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x00]),
        ResponseKind::Malformed { .. }
    ));
    assert!(matches!(
        kind(&[0x22, 0xF1, 0x90], &[0x62, 0xF1]),
        ResponseKind::Malformed { .. }
    ));
    assert!(matches!(kind(&[0x99], &[0xD9]), ResponseKind::Malformed { .. }));
    assert!(matches!(
        kind(&[0x22, 0xF1], &[0x62, 0xF1, 0x90]),
        ResponseKind::Malformed { .. }
    ));
}
