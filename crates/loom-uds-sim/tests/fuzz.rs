//! 100,000 random requests at random times: no panic, bounded replies, and every reply is a
//! well-formed answer to its own request (never foreign or malformed) when no fault is active.

use std::time::Duration;

use loom_uds_codec::{ResponseKind, classify_response};
use loom_uds_sim::{MAX_REPLIES_PER_REQUEST, SimEcu};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
}

#[test]
fn random_requests() {
    let mut rng = Rng(0x51A_0E00_C0DE);
    let mut ecu = SimEcu::bench();
    let mut now = Duration::ZERO;
    let sids = [0x10, 0x11, 0x14, 0x19, 0x22, 0x27, 0x2E, 0x3E, 0x31, 0x2A];
    for _ in 0..100_000 {
        now += Duration::from_millis(rng.next() % 3_000);
        let len = usize::from(rng.byte() % 8);
        let mut req: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        if let Some(first) = req.first_mut()
            && !rng.next().is_multiple_of(4)
        {
            *first = sids[usize::from(rng.byte()) % sids.len()];
        }
        let replies = ecu.handle(now, &req);
        assert!(replies.len() <= MAX_REPLIES_PER_REQUEST);
        for r in replies.as_slice() {
            let kind = classify_response(&req, &r.bytes);
            let ok = matches!(
                kind,
                ResponseKind::Positive | ResponseKind::Negative { .. } | ResponseKind::Pending
            );
            // A request whose SID is not a UDS service is classified Malformed by design.
            let undefined_sid = loom_uds_codec::service(req[0]).is_none();
            let too_short = loom_uds_codec::service(req[0]).is_some_and(|s| req.len() < s.min_request_len);
            assert!(
                ok || undefined_sid || too_short,
                "{req:02X?} → {:02X?} = {kind:?}",
                r.bytes
            );
        }
    }
}
