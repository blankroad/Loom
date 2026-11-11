//! Deterministic random-input loops (no external fuzz crate): no panics, all caps respected.

mod common;

use common::*;
use loom_eth_decode::{
    MAX_FIELDS, MAX_LAYERS, MAX_STREAMS, MAX_TOTAL_BUFFERED_BYTES, MAX_WARNINGS, StreamTable, decode,
    linktype,
};

const LINKTYPES: [u16; 8] = [
    linktype::ETHERNET,
    linktype::NULL,
    linktype::LOOP,
    linktype::RAW,
    linktype::LINUX_SLL,
    linktype::IPV4,
    linktype::IPV6,
    7,
];

fn check(d: &loom_eth_decode::Decoded, t: &StreamTable) {
    assert!(d.layers.len() <= MAX_LAYERS);
    assert!(d.layers.iter().map(|l| l.fields.len()).sum::<usize>() <= MAX_FIELDS);
    assert!(d.warnings.len() <= MAX_WARNINGS);
    assert!(t.len() <= MAX_STREAMS);
    assert!(t.buffered_bytes() <= MAX_TOTAL_BUFFERED_BYTES);
}

#[test]
fn random_bytes_100k() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut t = StreamTable::new();
    for i in 0..100_000u64 {
        let len = rng.below(200) as usize;
        let frame = rng.bytes(len);
        let lt = LINKTYPES[rng.below(LINKTYPES.len() as u64) as usize];
        let d = decode(lt, &frame, &meta(i, i * 1000), &mut t);
        check(&d, &t);
    }
}

#[test]
fn mutated_fixtures_100k() {
    let seeds: Vec<Vec<u8>> = vec![
        udp4(ECU, [255; 4], 13400, 13400, &announcement()),
        tcp4(
            true,
            1,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &read_vin_req(),
        ),
        tcp4(
            false,
            1,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &read_vin_resp(),
        ),
        udp4_vlan(
            42,
            [192, 0, 2, 2],
            [224, 244, 224, 245],
            30490,
            30490,
            &sd_offer(),
        ),
        arp_request(),
        icmp_echo(),
        udp6(&routing_req()),
    ];
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut t = StreamTable::new();
    for i in 0..100_000u64 {
        let mut f = seeds[rng.below(seeds.len() as u64) as usize].clone();
        let flips = 1 + rng.below(6);
        for _ in 0..flips {
            if f.is_empty() {
                break;
            }
            let at = rng.below(f.len() as u64) as usize;
            if let Some(b) = f.get_mut(at) {
                *b ^= 1 << rng.below(8);
            }
        }
        if rng.below(10) == 0 {
            f.truncate(rng.below(f.len() as u64 + 1) as usize);
        }
        // Random TCP seq region so reassembly sees gaps, overlaps and wraps.
        let d = decode(linktype::ETHERNET, &f, &meta(i, i * 1000), &mut t);
        check(&d, &t);
    }
}

#[test]
fn random_tcp_stream_payloads() {
    // Random segments on DoIP ports with random seq: exercises reassembly gap/desync paths.
    let mut rng = Rng(42);
    let mut t = StreamTable::new();
    for i in 0..20_000u64 {
        let len = rng.below(300) as usize;
        let mut payload = rng.bytes(len);
        if rng.below(3) == 0 && payload.len() >= 8 {
            payload[0] = 2;
            payload[1] = 0xFD;
        }
        let seq = (rng.next() & 0xFFFF_FFFF) as u32;
        let port = 50_000 + (rng.below(4) as u16);
        let fl = Flags {
            ack: Some(1),
            syn: rng.below(50) == 0,
            rst: rng.below(80) == 0,
            fin: rng.below(80) == 0,
        };
        let f = tcp4_ports(rng.below(2) == 0, port, 13400, seq, fl, &payload);
        let d = decode(linktype::ETHERNET, &f, &meta(i, i * 10_000), &mut t);
        check(&d, &t);
    }
}
