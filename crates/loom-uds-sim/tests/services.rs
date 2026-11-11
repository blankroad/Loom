//! Each service: positive and negative paths, S3 expiry, security lockout.

use std::time::Duration;

use loom_uds_codec::{ResponseKind, classify_response};
use loom_uds_sim::{SECURITY_XOR_DEFAULT, SimEcu, SimEcuConfig, SimError};

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

/// Single immediate reply.
fn one(ecu: &mut SimEcu, now: Duration, req: &[u8]) -> Vec<u8> {
    let r = ecu.handle(now, req);
    assert_eq!(r.len(), 1, "expected one reply for {req:02X?}");
    let reply = r.last().expect("one reply");
    assert_eq!(reply.delay, Duration::ZERO);
    reply.bytes.clone()
}

#[test]
fn read_did_and_session_did() {
    let mut ecu = SimEcu::bench();
    let r = one(&mut ecu, ms(0), &[0x22, 0xF1, 0x90]);
    assert_eq!(&r[..3], &[0x62, 0xF1, 0x90]);
    assert_eq!(&r[3..], b"LOOMSIM0000000001");
    assert_eq!(
        one(&mut ecu, ms(1), &[0x22, 0xF1, 0x86]),
        [0x62, 0xF1, 0x86, 0x01]
    );
    let multi = one(&mut ecu, ms(2), &[0x22, 0xF1, 0xA0, 0xF1, 0x86]);
    assert_eq!(multi, [0x62, 0xF1, 0xA0, 0x01, 0xF1, 0x86, 0x01]);
    assert_eq!(one(&mut ecu, ms(3), &[0x22, 0x12, 0x34]), [0x7F, 0x22, 0x31]);
    assert_eq!(one(&mut ecu, ms(4), &[0x22, 0xF1]), [0x7F, 0x22, 0x13]);
}

#[test]
fn session_timing_and_write_rules() {
    let mut ecu = SimEcu::bench();
    assert_eq!(
        one(&mut ecu, ms(0), &[0x2E, 0xF1, 0xA0, 0x02]),
        [0x7F, 0x2E, 0x7F],
        "default session"
    );
    assert_eq!(
        one(&mut ecu, ms(1), &[0x10, 0x03]),
        [0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]
    );
    assert_eq!(ecu.session(), 0x03);
    assert_eq!(
        one(&mut ecu, ms(2), &[0x2E, 0xF1, 0xA0, 0x02]),
        [0x6E, 0xF1, 0xA0]
    );
    assert_eq!(ecu.did(0xF1A0), Some(&[0x02][..]));
    assert_eq!(
        one(&mut ecu, ms(3), &[0x2E, 0xF1, 0x90, 0x41]),
        [0x7F, 0x2E, 0x31],
        "read-only DID"
    );
    assert_eq!(
        one(&mut ecu, ms(4), &[0x2E, 0xF1, 0xA0, 1, 2]),
        [0x7F, 0x2E, 0x13],
        "length kept"
    );
    assert_eq!(
        one(&mut ecu, ms(5), &[0x10, 0x02]),
        [0x7F, 0x10, 0x12],
        "programming unsupported"
    );
    assert!(
        ecu.handle(ms(6), &[0x10, 0x81]).is_empty(),
        "SPRMIB suppresses the positive reply"
    );
    assert_eq!(ecu.session(), 0x01);
}

#[test]
fn s3_expiry_returns_to_default() {
    let mut ecu = SimEcu::bench();
    one(&mut ecu, ms(0), &[0x10, 0x03]);
    one(&mut ecu, ms(4_000), &[0x3E, 0x00]);
    assert_eq!(ecu.session(), 0x03, "tester present keeps the session");
    assert!(ecu.handle(ms(8_500), &[0x3E, 0x80]).is_empty());
    assert_eq!(ecu.session(), 0x03);
    // 5.001 s of silence: the next request finds the default session.
    assert_eq!(
        one(&mut ecu, ms(13_501), &[0x2E, 0xF1, 0xA0, 0x02]),
        [0x7F, 0x2E, 0x7F]
    );
    assert_eq!(ecu.session(), 0x01);
}

#[test]
fn dtc_read_and_clear() {
    let mut ecu = SimEcu::bench();
    let all = one(&mut ecu, ms(0), &[0x19, 0x02, 0xFF]);
    assert_eq!(
        all,
        [0x59, 0x02, 0xFF, 0x92, 0x34, 0x00, 0x09, 0xC1, 0x00, 0x00, 0x2F]
    );
    let confirmed_only = one(&mut ecu, ms(1), &[0x19, 0x02, 0x20]);
    assert_eq!(confirmed_only, [0x59, 0x02, 0xFF, 0xC1, 0x00, 0x00, 0x2F]);
    assert_eq!(one(&mut ecu, ms(2), &[0x19, 0x0A]), [0x7F, 0x19, 0x12]);
    assert_eq!(
        one(&mut ecu, ms(3), &[0x14, 0x12, 0x34, 0x56]),
        [0x7F, 0x14, 0x31]
    );
    assert_eq!(one(&mut ecu, ms(4), &[0x14, 0x92, 0x34, 0x00]), [0x54]);
    assert_eq!(ecu.dtcs().len(), 1);
    assert_eq!(one(&mut ecu, ms(5), &[0x14, 0xFF, 0xFF, 0xFF]), [0x54]);
    assert_eq!(one(&mut ecu, ms(6), &[0x19, 0x02, 0xFF]), [0x59, 0x02, 0xFF]);
}

#[test]
fn reset_locks_and_returns_default() {
    let mut ecu = SimEcu::bench();
    one(&mut ecu, ms(0), &[0x10, 0x03]);
    unlock(&mut ecu, ms(1));
    assert_eq!(ecu.security_level(), Some(0x01));
    assert_eq!(one(&mut ecu, ms(2), &[0x11, 0x01]), [0x51, 0x01]);
    assert_eq!((ecu.session(), ecu.security_level()), (0x01, None));
    assert_eq!(one(&mut ecu, ms(3), &[0x11, 0x04]), [0x7F, 0x11, 0x12]);
}

fn unlock(ecu: &mut SimEcu, now: Duration) {
    let seed = one(ecu, now, &[0x27, 0x01]);
    assert_eq!(&seed[..2], &[0x67, 0x01]);
    let s = u32::from_be_bytes([seed[2], seed[3], seed[4], seed[5]]);
    let k = (s ^ SECURITY_XOR_DEFAULT).to_be_bytes();
    assert_eq!(one(ecu, now, &[0x27, 0x02, k[0], k[1], k[2], k[3]]), [0x67, 0x02]);
}

#[test]
fn security_access_flow_and_lockout() {
    let mut ecu = SimEcu::bench();
    assert_eq!(
        one(&mut ecu, ms(0), &[0x27, 0x01]),
        [0x7F, 0x27, 0x7F],
        "not in default session"
    );
    one(&mut ecu, ms(1), &[0x10, 0x03]);
    assert_eq!(
        one(&mut ecu, ms(2), &[0x27, 0x02, 0, 0, 0, 0]),
        [0x7F, 0x27, 0x24],
        "key before seed"
    );
    for attempt in 1..=3u64 {
        one(&mut ecu, ms(10 + attempt), &[0x27, 0x01]);
        let r = one(&mut ecu, ms(10 + attempt), &[0x27, 0x02, 0, 0, 0, 0]);
        let want = if attempt < 3 { 0x35 } else { 0x36 };
        assert_eq!(r, [0x7F, 0x27, want], "attempt {attempt}");
    }
    assert_eq!(
        one(&mut ecu, ms(1_000), &[0x27, 0x01]),
        [0x7F, 0x27, 0x37],
        "locked out"
    );
    // Keep the session alive past the 10 s lockout, then unlock normally.
    for t in [4_000, 8_000, 11_000] {
        assert!(ecu.handle(ms(t), &[0x3E, 0x80]).is_empty());
    }
    unlock(&mut ecu, ms(11_500));
    assert_eq!(
        one(&mut ecu, ms(11_600), &[0x27, 0x01]),
        [0x67, 0x01, 0, 0, 0, 0],
        "already unlocked"
    );
    one(&mut ecu, ms(11_700), &[0x10, 0x01]);
    assert_eq!(ecu.security_level(), None, "session change locks");
}

#[test]
fn unsupported_services() {
    let mut ecu = SimEcu::bench();
    for sid in [0x2A, 0x86, 0x31, 0x34, 0x99] {
        assert_eq!(one(&mut ecu, ms(0), &[sid, 0x01, 0x02, 0x03]), [0x7F, sid, 0x11]);
    }
    assert!(ecu.handle(ms(0), &[]).is_empty());
}

#[test]
fn replies_classify_against_their_request() {
    let mut ecu = SimEcu::bench();
    let reqs: [&[u8]; 6] = [
        &[0x22, 0xF1, 0x90],
        &[0x10, 0x03],
        &[0x2E, 0xF1, 0xA0, 0x07],
        &[0x19, 0x02, 0x09],
        &[0x14, 0xFF, 0xFF, 0xFF],
        &[0x3E, 0x00],
    ];
    for (i, req) in reqs.iter().enumerate() {
        let r = one(&mut ecu, ms(i as u64), req);
        assert_eq!(
            classify_response(req, &r),
            ResponseKind::Positive,
            "{req:02X?} → {r:02X?}"
        );
    }
}

#[test]
fn config_limits() {
    let mut cfg = SimEcuConfig::bench();
    cfg.p2_star_ms = 5001;
    assert!(matches!(SimEcu::new(cfg), Err(SimError::Config(_))));
    let mut cfg = SimEcuConfig::bench();
    cfg.dids.insert(0xF186, loom_uds_sim::SimDid::read_only(&[1]));
    assert!(SimEcu::new(cfg).is_err());
    let mut cfg = SimEcuConfig::bench();
    for i in 0..300u16 {
        cfg.dids.insert(i, loom_uds_sim::SimDid::read_only(&[1]));
    }
    assert!(matches!(SimEcu::new(cfg), Err(SimError::TooMany { .. })));
    assert!(SimEcu::new(SimEcuConfig::bench()).is_ok());
}
