//! Deterministic random-input runs: no panic on any input (≥ 100,000 cases each).

use loom_uds_codec::did::STANDARD_DID_NAMES;
use loom_uds_codec::response::{
    parse_dtc_by_status_response, parse_read_did_response, parse_seed_response, parse_session_response,
};
use loom_uds_codec::{DidLookup, Format, StandardDids, classify_response, load_db, nrc_name};

const RUNS: usize = 100_000;

/// xorshift64*: deterministic, no external crate.
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
    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let len = usize::try_from(self.next() % (max as u64 + 1)).expect("fits usize");
        (0..len).map(|_| self.byte()).collect()
    }
    /// Bias toward UDS-looking PDUs so the echo paths get exercised.
    fn pdu(&mut self) -> Vec<u8> {
        let mut v = self.bytes(12);
        if let Some(first) = v.first_mut() {
            *first = match self.next() % 4 {
                0 => 0x7F,
                1 => [0x22, 0x62, 0x10, 0x50, 0x2E, 0x6E, 0x31, 0x71, 0x19, 0x59][self.byte() as usize % 10],
                _ => *first,
            };
        }
        v
    }
}

#[test]
fn classify_and_parsers_never_panic() {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut finals = 0usize;
    for _ in 0..RUNS {
        let req = rng.pdu();
        let resp = rng.pdu();
        if classify_response(&req, &resp).is_final() {
            finals += 1;
        }
        let _ = parse_session_response(&resp);
        let _ = parse_read_did_response(&req, &resp, &StandardDids);
        let _ = parse_dtc_by_status_response(&resp);
        let _ = parse_seed_response(&resp);
        let _ = nrc_name(rng.byte());
    }
    assert!(finals > 0, "random input reached the positive/negative paths");
}

#[test]
fn did_decode_never_panics() {
    let mut rng = Rng(0x0DD_BA11);
    let defs: Vec<_> = STANDARD_DID_NAMES
        .iter()
        .filter_map(|(id, _)| StandardDids.did_def(*id))
        .collect();
    for i in 0..RUNS {
        let data = rng.bytes(24);
        let _ = defs[i % defs.len()].decode(&data);
    }
}

#[test]
fn db_loader_never_panics() {
    let mut rng = Rng(0xDB_10AD);
    let seeds: [&[u8]; 3] = [
        br#"{"loom_diag":1,"dids":[{"id":1,"name":"a","length":1,"codec":{"type":"uint"}}]}"#,
        b"{}",
        b"[]",
    ];
    for i in 0..RUNS {
        let mut doc = seeds[i % seeds.len()].to_vec();
        // Mutate a few bytes of a valid-ish document, or use pure noise.
        if i % 2 == 0 {
            for _ in 0..3 {
                if !doc.is_empty() {
                    let at = usize::try_from(rng.next() % doc.len() as u64).expect("fits");
                    doc[at] = rng.byte();
                }
            }
        } else {
            doc = rng.bytes(64);
        }
        let _ = load_db(&doc, Format::Json);
    }
}
