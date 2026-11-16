//! operator-upgrade B3a (ETH-03) — `udp.stream` conversation index (FUNCTIONS.md F3.1/F3.2).
mod common;

use common::*;
use loom_eth_decode::{MAX_UDP_CONVERSATIONS, StreamTable, field_def};

#[test]
fn udp_stream_is_registered_like_wireshark() {
    let def = field_def("udp.stream").expect("udp.stream registered");
    assert_eq!(def.desc, "Stream index");
}

#[test]
fn both_directions_share_one_number_and_new_pairs_count_up_from_zero() {
    let mut t = StreamTable::new();
    let req = dec(&mut t, 1, &udp4(TESTER, ECU, 40000, 65201, b"ping"));
    let resp = dec(&mut t, 2, &udp4(ECU, TESTER, 65201, 40000, b"pong"));
    let other_port = dec(&mut t, 3, &udp4(TESTER, ECU, 40001, 65201, b"ping"));
    let again = dec(&mut t, 4, &udp4(TESTER, ECU, 40000, 65201, b"ping2"));
    assert_eq!(first_u(&req, "udp.stream"), Some(0));
    assert_eq!(
        first_u(&resp, "udp.stream"),
        Some(0),
        "reverse direction = same conversation"
    );
    assert_eq!(first_u(&other_port, "udp.stream"), Some(1));
    assert_eq!(first_u(&again, "udp.stream"), Some(0));
    // VLAN is part of the key; TCP numbering is a separate index space.
    let vlan = dec(&mut t, 5, &udp4_vlan(7, TESTER, ECU, 40000, 65201, b"ping"));
    assert_eq!(first_u(&vlan, "udp.stream"), Some(2));
    let tcp = dec(&mut t, 6, &tcp4(true, 1000, Default::default(), b""));
    assert_eq!(first_u(&tcp, "tcp.stream"), Some(0));
    assert_eq!(first_u(&tcp, "udp.stream"), None);
    // DoIP / SOME/IP over UDP still decode and also carry the conversation index.
    let doip = dec(&mut t, 7, &udp4(TESTER, ECU, 50000, 13400, &read_vin_req()));
    assert!(count_layers(&doip, "doip") >= 1);
    assert_eq!(first_u(&doip, "udp.stream"), Some(3));
}

#[test]
fn malformed_length_still_numbered_and_no_panic_on_ipv6() {
    let mut t = StreamTable::new();
    let mut f = udp4(TESTER, ECU, 1, 2, b"abcd");
    // UDP length field below 8 (bytes 38..40 of Ethernet+IPv4+UDP)
    f[38] = 0;
    f[39] = 4;
    let d = dec(&mut t, 1, &f);
    assert_eq!(first_u(&d, "udp.stream"), Some(0));
    let v6 = dec(&mut t, 2, &udp6(b"x"));
    assert_eq!(first_u(&v6, "udp.stream"), Some(1));
}

#[test]
fn table_is_bounded_idle_first_then_lru_with_warning() {
    let mut t = StreamTable::new();
    // Fill the table at t≈0.
    for i in 0..MAX_UDP_CONVERSATIONS {
        let port = 1000 + u16::try_from(i).expect("port");
        let d = decode_at(
            &mut t,
            i as u64,
            1_000 + i as u64,
            &udp4(TESTER, ECU, port, 9, b"x"),
        );
        assert_eq!(first_u(&d, "udp.stream"), Some(i as u64));
    }
    // Keep conversation 0 recent; the others go idle.
    let recent = loom_eth_decode::STREAM_IDLE_NS + 10_000;
    decode_at(&mut t, 5000, recent, &udp4(TESTER, ECU, 1000, 9, b"x"));
    // Full + idle ones exist → idle eviction, no warning, new number.
    let d = decode_at(&mut t, 5001, recent + 1, &udp4(TESTER, ECU, 60000, 9, b"x"));
    assert_eq!(first_u(&d, "udp.stream"), Some(MAX_UDP_CONVERSATIONS as u64));
    assert!(d.warnings.iter().all(|w| w.code != "udp_conversation_evicted"));
    assert_eq!(t.udp_evicted(), (MAX_UDP_CONVERSATIONS - 1) as u64);
    // The kept conversation keeps its number.
    let d = decode_at(&mut t, 5002, recent + 2, &udp4(TESTER, ECU, 1000, 9, b"x"));
    assert_eq!(first_u(&d, "udp.stream"), Some(0));
    // Refill to the cap with fresh (non-idle) conversations (2 live now: #0 and the newcomer),
    // then one more → no idle entry left → LRU eviction + warning.
    for i in 0..MAX_UDP_CONVERSATIONS - 2 {
        let port = 20000 + u16::try_from(i).expect("port");
        decode_at(
            &mut t,
            6000 + i as u64,
            recent + 3,
            &udp4(TESTER, ECU, port, 9, b"x"),
        );
    }
    let d = decode_at(&mut t, 9000, recent + 5, &udp4(TESTER, ECU, 65000, 9, b"x"));
    let w: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.code == "udp_conversation_evicted")
        .collect();
    assert_eq!(w.len(), 1, "{:?}", d.warnings);
    assert!(
        w[0].detail
            .contains(&format!("table full ({MAX_UDP_CONVERSATIONS})")),
        "{}",
        w[0].detail
    );
    // LRU = smallest last_ts: the idle-eviction newcomer (#1024, seen at recent+1); #0 was refreshed at recent+2.
    assert!(
        w[0].detail
            .contains(&format!("evicted udp stream {MAX_UDP_CONVERSATIONS}")),
        "{}",
        w[0].detail
    );
    assert_eq!(t.udp_evicted(), MAX_UDP_CONVERSATIONS as u64);
}

fn decode_at(t: &mut StreamTable, seq: u64, ts: u64, frame: &[u8]) -> loom_eth_decode::Decoded {
    loom_eth_decode::decode(loom_eth_decode::linktype::ETHERNET, frame, &meta(seq, ts), t)
}
