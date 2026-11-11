//! Deterministic random-input tests (no external fuzz crate): parsers must never panic and
//! never accept an over-limit length.

use std::time::Duration;

use loom_uds_doip::frame::payload_type;
use loom_uds_doip::{
    ClientConfig, DoipClient, DoipEntity, EntityConfig, HEADER_LEN, MAX_DOIP_PAYLOAD, Message, decode_frame,
    parse_header,
};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::Instant;

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.byte()).collect()
    }
}

const KNOWN: [u16; 16] = [
    payload_type::GENERIC_NACK,
    payload_type::VEHICLE_ID_REQUEST,
    payload_type::VEHICLE_ID_REQUEST_EID,
    payload_type::VEHICLE_ID_REQUEST_VIN,
    payload_type::VEHICLE_ANNOUNCEMENT,
    payload_type::ROUTING_ACTIVATION_REQUEST,
    payload_type::ROUTING_ACTIVATION_RESPONSE,
    payload_type::ALIVE_CHECK_REQUEST,
    payload_type::ALIVE_CHECK_RESPONSE,
    payload_type::ENTITY_STATUS_REQUEST,
    payload_type::ENTITY_STATUS_RESPONSE,
    payload_type::POWER_MODE_REQUEST,
    payload_type::POWER_MODE_RESPONSE,
    payload_type::DIAGNOSTIC_MESSAGE,
    payload_type::DIAGNOSTIC_ACK,
    payload_type::DIAGNOSTIC_NACK,
];

/// Random frame: either pure noise or a plausible header with a random known type and length.
fn random_frame(rng: &mut Rng) -> Vec<u8> {
    if rng.below(3) == 0 {
        let n = rng.below(48) as usize;
        return rng.bytes(n);
    }
    let version = [0x02u8, 0x03, 0xFF, rng.byte()][rng.below(4) as usize];
    let ty = KNOWN[rng.below(KNOWN.len() as u64) as usize];
    let len: u32 = match rng.below(5) {
        0 => rng.next() as u32,
        1 => u32::MAX,
        _ => rng.below(40) as u32,
    };
    let mut f = vec![version, !version];
    if rng.below(10) == 0 {
        f[1] = rng.byte();
    }
    f.extend_from_slice(&ty.to_be_bytes());
    f.extend_from_slice(&len.to_be_bytes());
    let body = rng.below(48) as usize;
    f.extend_from_slice(&rng.bytes(body));
    f
}

#[test]
fn random_bytes_never_panic_100k() {
    let mut rng = Rng(0x10_0D_1D_0E);
    let mut accepted = 0u32;
    for _ in 0..100_000 {
        let f = random_frame(&mut rng);
        if let Some(head) = f.first_chunk::<HEADER_LEN>()
            && let Ok(h) = parse_header(head)
        {
            assert!(h.payload_len <= MAX_DOIP_PAYLOAD);
        }
        if let Ok(Some((h, msg, used))) = decode_frame(&f) {
            accepted += 1;
            assert_eq!(used, HEADER_LEN + h.payload_len);
            // Accepted messages re-encode to the same bytes (routing activation has reserved
            // bytes that Loom writes as zero).
            let again = msg.encode_with_version(h.version).expect("re-encode");
            let reserved = matches!(
                msg,
                Message::RoutingActivationRequest { .. } | Message::RoutingActivationResponse { .. }
            );
            if !reserved {
                assert_eq!(&again[..], &f[..used]);
            }
        }
        let ty = KNOWN[rng.below(KNOWN.len() as u64) as usize];
        let n = rng.below(40) as usize;
        let payload = rng.bytes(n);
        // Must not panic for any (type, payload).
        if let Ok(m) = Message::decode(ty, &payload) {
            assert_eq!(m.payload_type(), ty);
        }
    }
    assert!(
        accepted > 1000,
        "generator must also produce valid frames ({accepted})"
    );
}

#[test]
fn every_length_header_is_bounded() {
    let mut rng = Rng(7);
    for _ in 0..100_000 {
        let mut h = [0u8; HEADER_LEN];
        h.copy_from_slice(&rng.bytes(HEADER_LEN));
        h[0] = 0x02;
        h[1] = 0xFD;
        h[2..4].copy_from_slice(&payload_type::DIAGNOSTIC_MESSAGE.to_be_bytes());
        if let Ok(header) = parse_header(&h) {
            assert!(header.payload_len <= MAX_DOIP_PAYLOAD);
        }
    }
}

/// Random garbage on many TCP connections must not stop the entity from serving.
#[tokio::test]
async fn entity_survives_random_streams() {
    let entity = DoipEntity::start(
        EntityConfig::loopback(0x1000, vec![0x0E80]),
        |_t, _n, req: &[u8]| vec![(Duration::ZERO, vec![req[0].wrapping_add(0x40)])],
    )
    .await
    .expect("entity");
    let mut rng = Rng(42);
    for _ in 0..200 {
        let mut s = TcpStream::connect(entity.tcp_addr()).await.expect("connect");
        let mut blob = Vec::new();
        for _ in 0..rng.below(6) + 1 {
            blob.extend_from_slice(&random_frame(&mut rng));
        }
        // The entity may close mid-write; that is fine.
        let _ignored = s.write_all(&blob).await;
        drop(s);
    }
    let (client, _ev) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), 0x0E00))
        .await
        .expect("entity still serves");
    let mut t = client.open_target(0x0E80).expect("open");
    let until = Instant::now() + Duration::from_secs(1);
    t.send(&[0x3E, 0x00], until).await.expect("send");
    assert_eq!(t.recv_until(until).await.expect("rx").data, vec![0x7E]);
}
