//! Byte fixtures built in Rust (etherparse PacketBuilder). No external files, no Python.
#![allow(dead_code)]

use etherparse::{PacketBuilder, VlanId};
use loom_eth_decode::{Decoded, FrameMeta, StreamTable, decode, linktype};

pub const VIN: &[u8; 17] = b"LOOM0000000000001";
pub const TESTER: [u8; 4] = [192, 0, 2, 1];
pub const ECU: [u8; 4] = [192, 0, 2, 10];
pub const MAC_T: [u8; 6] = [2, 0, 0, 0, 0, 1];
pub const MAC_E: [u8; 6] = [2, 0, 0, 0, 0, 10];

pub fn doip(ty: u16, body: &[u8]) -> Vec<u8> {
    let mut p = vec![2, 0xFD];
    p.extend(ty.to_be_bytes());
    p.extend(u32::try_from(body.len()).expect("small body").to_be_bytes());
    p.extend(body);
    p
}

pub fn announcement() -> Vec<u8> {
    let mut b = VIN.to_vec();
    b.extend([0x10, 0x00]); // LA 0x1000
    b.extend([2, 0, 0, 0, 0, 10]); // EID
    b.extend([2, 0, 0, 0, 0, 11]); // GID
    b.push(0); // further action
    doip(0x0004, &b)
}

pub fn routing_req() -> Vec<u8> {
    doip(0x0005, &[0x0E, 0x00, 0x00, 0, 0, 0, 0])
}

pub fn routing_resp() -> Vec<u8> {
    doip(0x0006, &[0x0E, 0x00, 0x10, 0x00, 0x10, 0, 0, 0, 0])
}

pub fn diag(sa: u16, ta: u16, uds: &[u8]) -> Vec<u8> {
    let mut b = sa.to_be_bytes().to_vec();
    b.extend(ta.to_be_bytes());
    b.extend(uds);
    doip(0x8001, &b)
}

pub fn read_vin_req() -> Vec<u8> {
    diag(0x0E00, 0x1000, &[0x22, 0xF1, 0x90])
}

pub fn read_vin_resp() -> Vec<u8> {
    let mut u = vec![0x62, 0xF1, 0x90];
    u.extend(VIN);
    diag(0x1000, 0x0E00, &u)
}

pub fn nrc_resp() -> Vec<u8> {
    diag(0x1000, 0x0E00, &[0x7F, 0x22, 0x31])
}

pub fn sd_offer() -> Vec<u8> {
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

pub fn udp4(src: [u8; 4], dst: [u8; 4], sp: u16, dp: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    PacketBuilder::ethernet2(MAC_T, MAC_E)
        .ipv4(src, dst, 64)
        .udp(sp, dp)
        .write(&mut out, payload)
        .expect("build udp4");
    out
}

pub fn udp4_vlan(vlan: u16, src: [u8; 4], dst: [u8; 4], sp: u16, dp: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    PacketBuilder::ethernet2(MAC_T, [1, 0, 0x5E, 0x74, 0xE0, 0xF5])
        .single_vlan(VlanId::try_from(vlan).expect("vlan id"))
        .ipv4(src, dst, 64)
        .udp(sp, dp)
        .write(&mut out, payload)
        .expect("build vlan udp");
    out
}

pub fn udp6(payload: &[u8]) -> Vec<u8> {
    let mut a = [0u8; 16];
    a[0] = 0xFD;
    a[15] = 1;
    let mut b = a;
    b[15] = 2;
    let mut out = Vec::new();
    PacketBuilder::ethernet2(MAC_T, MAC_E)
        .ipv6(a, b, 64)
        .udp(40000, 13400)
        .write(&mut out, payload)
        .expect("build udp6");
    out
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub syn: bool,
    pub ack: Option<u32>,
    pub fin: bool,
    pub rst: bool,
}

/// TCP frame. `to_ecu` = tester → ECU (50000 → 13400), else ECU → tester.
pub fn tcp4(to_ecu: bool, seq: u32, fl: Flags, payload: &[u8]) -> Vec<u8> {
    tcp4_ports(to_ecu, 50000, 13400, seq, fl, payload)
}

pub fn tcp4_ports(to_ecu: bool, cport: u16, sport: u16, seq: u32, fl: Flags, payload: &[u8]) -> Vec<u8> {
    let (smac, dmac, s, d, sp, dp) = if to_ecu {
        (MAC_T, MAC_E, TESTER, ECU, cport, sport)
    } else {
        (MAC_E, MAC_T, ECU, TESTER, sport, cport)
    };
    let mut b = PacketBuilder::ethernet2(smac, dmac)
        .ipv4(s, d, 64)
        .tcp(sp, dp, seq, 65535);
    if fl.syn {
        b = b.syn();
    }
    if let Some(a) = fl.ack {
        b = b.ack(a);
    }
    if fl.fin {
        b = b.fin();
    }
    if fl.rst {
        b = b.rst();
    }
    if !payload.is_empty() {
        b = b.psh();
    }
    let mut out = Vec::new();
    b.write(&mut out, payload).expect("build tcp4");
    out
}

pub fn arp_request() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend([0xFF; 6]);
    f.extend(MAC_T);
    f.extend([0x08, 0x06]);
    f.extend([0x00, 0x01, 0x08, 0x00, 6, 4, 0x00, 0x01]);
    f.extend(MAC_T);
    f.extend(TESTER);
    f.extend([0; 6]);
    f.extend(ECU);
    f
}

pub fn icmp_echo() -> Vec<u8> {
    let mut out = Vec::new();
    PacketBuilder::ethernet2(MAC_T, MAC_E)
        .ipv4(TESTER, ECU, 64)
        .icmpv4_echo_request(0x1234, 7)
        .write(&mut out, b"ping")
        .expect("build icmp");
    out
}

/// BSD NULL loopback (little-endian family 2) + raw IPv4 from an Ethernet frame.
pub fn null_loopback(eth_frame: &[u8]) -> Vec<u8> {
    let mut f = 2u32.to_le_bytes().to_vec();
    f.extend(eth_frame.get(14..).expect("eth frame"));
    f
}

pub fn meta(seq: u64, ts_ns: u64) -> FrameMeta {
    FrameMeta {
        seq,
        ts_ns,
        ..FrameMeta::default()
    }
}

pub fn dec(t: &mut StreamTable, seq: u64, frame: &[u8]) -> Decoded {
    decode(linktype::ETHERNET, frame, &meta(seq, seq * 1_000_000), t)
}

pub fn values(d: &Decoded, name: &str) -> Vec<String> {
    d.field_values(name).map(|v| v.to_string()).collect()
}

pub fn first_u(d: &Decoded, name: &str) -> Option<u64> {
    d.field_values(name).find_map(|v| match v {
        loom_eth_decode::FieldValue::Uint(x) => Some(x),
        _ => None,
    })
}

pub fn count_layers(d: &Decoded, name: &str) -> usize {
    d.layers.iter().filter(|l| l.name == name).count()
}

/// Deterministic xorshift64* PRNG for fuzz-like loops.
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
    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| (self.next() >> 24) as u8).collect()
    }
}
