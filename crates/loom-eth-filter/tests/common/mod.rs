//! Packet fixtures for filter tests (Rust only).
#![allow(dead_code)]

use etherparse::{PacketBuilder, VlanId};
use loom_eth_decode::{Decoded, Dir, FrameMeta, Origin, StreamTable, decode, linktype};

pub const MAC_T: [u8; 6] = [2, 0, 0, 0, 0, 1];
pub const MAC_E: [u8; 6] = [2, 0, 0, 0, 0, 0x0a];
pub const TESTER: [u8; 4] = [192, 0, 2, 1];
pub const ECU: [u8; 4] = [192, 0, 2, 10];
pub const VIN: &[u8; 17] = b"LOOM0000000000001";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fx {
    Req,
    Resp,
    Nrc,
    Ann,
    Sd,
    Arp,
    Icmp,
    V6,
    Syn,
    Two,
}

fn doip(ty: u16, body: &[u8]) -> Vec<u8> {
    let mut p = vec![2, 0xFD];
    p.extend(ty.to_be_bytes());
    p.extend(u32::try_from(body.len()).expect("small").to_be_bytes());
    p.extend(body);
    p
}

fn diag(sa: u16, ta: u16, uds: &[u8]) -> Vec<u8> {
    let mut b = sa.to_be_bytes().to_vec();
    b.extend(ta.to_be_bytes());
    b.extend(uds);
    doip(0x8001, &b)
}

fn tcp(to_ecu: bool, seq: u32, syn: bool, payload: &[u8]) -> Vec<u8> {
    let (sm, dm, s, d, sp, dp) = if to_ecu {
        (MAC_T, MAC_E, TESTER, ECU, 50000, 13400)
    } else {
        (MAC_E, MAC_T, ECU, TESTER, 13400, 50000)
    };
    let mut b = PacketBuilder::ethernet2(sm, dm)
        .ipv4(s, d, 64)
        .tcp(sp, dp, seq, 65535);
    if syn {
        b = b.syn();
    } else {
        b = b.ack(1);
    }
    if !payload.is_empty() {
        b = b.psh();
    }
    let mut out = Vec::new();
    b.write(&mut out, payload).expect("tcp");
    out
}

fn udp(src: [u8; 4], dst: [u8; 4], port: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    PacketBuilder::ethernet2(MAC_E, [0xFF; 6])
        .ipv4(src, dst, 64)
        .udp(port, port)
        .write(&mut out, payload)
        .expect("udp");
    out
}

fn sd_offer() -> Vec<u8> {
    let mut sd = vec![0xC0, 0, 0, 0, 0, 0, 0, 16];
    sd.extend([
        0x01, 0, 0, 0x10, 0x12, 0x34, 0x00, 0x01, 0x01, 0x00, 0x00, 0x03, 0, 0, 0, 1,
    ]);
    sd.extend(12u32.to_be_bytes());
    sd.extend([0, 9, 0x04, 0, 192, 0, 2, 2, 0, 0x11, 0x77, 0x24]);
    let mut p = vec![0xFF, 0xFF, 0x81, 0x00];
    p.extend((8 + u32::try_from(sd.len()).expect("small")).to_be_bytes());
    p.extend([0, 0, 0, 1, 1, 1, 0x02, 0]);
    p.extend(sd);
    p
}

pub fn frame(fx: Fx) -> Vec<u8> {
    match fx {
        Fx::Req => tcp(true, 100, false, &diag(0x0E00, 0x1000, &[0x22, 0xF1, 0x90])),
        Fx::Resp => {
            let mut u = vec![0x62, 0xF1, 0x90];
            u.extend(VIN);
            tcp(false, 500, false, &diag(0x1000, 0x0E00, &u))
        }
        Fx::Nrc => tcp(false, 500, false, &diag(0x1000, 0x0E00, &[0x7F, 0x22, 0x31])),
        Fx::Ann => {
            let mut b = VIN.to_vec();
            b.extend([0x10, 0x00, 2, 0, 0, 0, 0, 10, 2, 0, 0, 0, 0, 11, 0]);
            udp(ECU, [255; 4], 13400, &doip(0x0004, &b))
        }
        Fx::Sd => {
            let mut out = Vec::new();
            PacketBuilder::ethernet2(MAC_T, [1, 0, 0x5E, 0x74, 0xE0, 0xF5])
                .single_vlan(VlanId::try_from(42).expect("vlan"))
                .ipv4([192, 0, 2, 2], [224, 244, 224, 245], 64)
                .udp(30490, 30490)
                .write(&mut out, &sd_offer())
                .expect("sd");
            out
        }
        Fx::Arp => {
            let mut f = vec![0xFF; 6];
            f.extend(MAC_T);
            f.extend([0x08, 0x06, 0x00, 0x01, 0x08, 0x00, 6, 4, 0x00, 0x01]);
            f.extend(MAC_T);
            f.extend(TESTER);
            f.extend([0; 6]);
            f.extend(ECU);
            f
        }
        Fx::Icmp => {
            let mut out = Vec::new();
            PacketBuilder::ethernet2(MAC_T, MAC_E)
                .ipv4(TESTER, ECU, 64)
                .icmpv4_echo_request(0x1234, 7)
                .write(&mut out, b"ping")
                .expect("icmp");
            out
        }
        Fx::V6 => {
            let mut a = [0u8; 16];
            a[0] = 0xFD;
            a[15] = 1;
            let mut b = a;
            b[15] = 2;
            let mut out = Vec::new();
            PacketBuilder::ethernet2(MAC_T, MAC_E)
                .ipv6(a, b, 64)
                .udp(40000, 13400)
                .write(&mut out, &doip(0x0005, &[0x0E, 0, 0, 0, 0, 0, 0]))
                .expect("v6");
            out
        }
        Fx::Syn => tcp(true, 1000, true, &[]),
        Fx::Two => {
            let mut two = doip(0x0005, &[0x0E, 0, 0, 0, 0, 0, 0]);
            two.extend(diag(0x0E00, 0x1000, &[0x22, 0xF1, 0x90]));
            tcp(true, 100, false, &two)
        }
    }
}

pub fn decoded(fx: Fx) -> Decoded {
    let meta = match fx {
        Fx::Req => FrameMeta {
            seq: 41240,
            ts_ns: 2_500_000_000,
            dir: Some(Dir::Tx),
            attributed_origin: Some(Origin::Agent),
            request_id: Some("t-000418".into()),
            orig_len: None,
        },
        _ => FrameMeta {
            seq: 7,
            ts_ns: 1_000_000_000,
            ..FrameMeta::default()
        },
    };
    let mut t = StreamTable::new();
    decode(linktype::ETHERNET, &frame(fx), &meta, &mut t)
}

/// Deterministic xorshift64* PRNG.
#[derive(Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}
