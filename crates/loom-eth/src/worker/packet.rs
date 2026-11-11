//! Minimal bounded simulator framing; no OS capture and no alternative UDS interpretation.
use super::shared::MAX_SEND_BYTES;
use std::net::{IpAddr, SocketAddr};
/// Largest simulator message; builders return None above this rather than allocate.
pub const MAX_SIM_MESSAGE_BYTES: usize = 1024;
const SRC_MAC: [u8; 6] = [2, 0, 0, 0, 0, 2];
fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for c in data.chunks(2) {
        sum += u32::from(c[0]) * 256 + u32::from(*c.get(1).unwrap_or(&0));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}
fn transport_checksum(src: IpAddr, dst: IpAddr, proto: u8, transport: &[u8]) -> Option<u16> {
    if transport.len() > MAX_SEND_BYTES + 20 {
        return None;
    }
    let mut pseudo = Vec::with_capacity(40 + transport.len());
    match (src, dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => {
            pseudo.extend(s.octets());
            pseudo.extend(d.octets());
            pseudo.extend([0, proto]);
            pseudo.extend((transport.len() as u16).to_be_bytes());
        }
        (IpAddr::V6(s), IpAddr::V6(d)) => {
            pseudo.extend(s.octets());
            pseudo.extend(d.octets());
            pseudo.extend((transport.len() as u32).to_be_bytes());
            pseudo.extend([0, 0, 0, proto]);
        }
        _ => return None,
    };
    pseudo.extend(transport);
    Some(checksum(&pseudo))
}
fn ethernet_ip(src: IpAddr, dst: IpAddr, proto: u8, transport: &[u8]) -> Option<Vec<u8>> {
    if transport.len() > MAX_SEND_BYTES + 20 {
        return None;
    }
    let mut v = Vec::with_capacity(54 + transport.len());
    let dest_mac = match dst {
        IpAddr::V4(a) if a.octets() == [255; 4] => [255; 6],
        IpAddr::V4(a) if a.is_multicast() => {
            let b = a.octets();
            [1, 0, 0x5e, b[1] & 0x7f, b[2], b[3]]
        }
        IpAddr::V6(a) if a.is_multicast() => {
            let b = a.octets();
            [0x33, 0x33, b[12], b[13], b[14], b[15]]
        }
        _ => [2, 0, 0, 0, 0, 1],
    };
    v.extend(dest_mac);
    v.extend(SRC_MAC);
    match (src, dst) {
        (IpAddr::V4(s), IpAddr::V4(d)) => {
            v.extend([8, 0]);
            let mut ip = [0u8; 20];
            ip[0] = 0x45;
            ip[2..4].copy_from_slice(&((20 + transport.len()) as u16).to_be_bytes());
            ip[8] = 64;
            ip[9] = proto;
            ip[12..16].copy_from_slice(&s.octets());
            ip[16..20].copy_from_slice(&d.octets());
            let check = checksum(&ip);
            ip[10..12].copy_from_slice(&check.to_be_bytes());
            v.extend(ip);
        }
        (IpAddr::V6(s), IpAddr::V6(d)) => {
            v.extend([0x86, 0xdd]);
            let mut ip = [0; 40];
            ip[0] = 0x60;
            ip[4..6].copy_from_slice(&(transport.len() as u16).to_be_bytes());
            ip[6] = proto;
            ip[7] = 64;
            ip[8..24].copy_from_slice(&s.octets());
            ip[24..40].copy_from_slice(&d.octets());
            v.extend(ip);
        }
        _ => return None,
    };
    v.extend(transport);
    Some(v)
}
pub(crate) fn udp(src: SocketAddr, dst: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() > MAX_SEND_BYTES {
        return None;
    }
    let mut t = Vec::with_capacity(8 + payload.len());
    t.extend(src.port().to_be_bytes());
    t.extend(dst.port().to_be_bytes());
    t.extend(((8 + payload.len()) as u16).to_be_bytes());
    t.extend([0, 0]);
    t.extend(payload);
    let c = transport_checksum(src.ip(), dst.ip(), 17, &t)?;
    t[6..8].copy_from_slice(&(if c == 0 { 0xffff } else { c }).to_be_bytes());
    ethernet_ip(src.ip(), dst.ip(), 17, &t)
}
fn doip(kind: u16, payload: &[u8]) -> Vec<u8> {
    assert!(
        payload.len() <= MAX_SIM_MESSAGE_BYTES,
        "bounded internal simulator payload"
    );
    let mut p = Vec::with_capacity(8 + payload.len());
    p.extend([2, 0xfd]);
    p.extend(kind.to_be_bytes());
    p.extend((payload.len() as u32).to_be_bytes());
    p.extend(payload);
    p
}
pub(crate) fn announcement() -> Vec<u8> {
    let mut b = Vec::with_capacity(32);
    b.extend(b"LOOM0000000000001");
    b.extend([0x10, 0]);
    b.extend([2, 0, 0, 0, 0, 2]);
    b.extend([2, 0, 0, 0, 0, 3]);
    b.push(0);
    udp(
        "192.0.2.2:13400".parse().expect("constant IPv4"),
        "255.255.255.255:13400".parse().expect("constant IPv4"),
        &doip(4, &b),
    )
    .expect("bounded announcement")
}
pub(crate) fn offer() -> Vec<u8> {
    let mut b = vec![0xc0, 0, 0, 0, 0, 0, 0, 16];
    b.extend([1, 0, 0, 0x10, 0x12, 0x34, 0, 1, 1, 0, 0, 3, 0, 0, 0, 1]);
    b.extend(12u32.to_be_bytes());
    b.extend([0, 9, 4, 0, 192, 0, 2, 2, 0, 0x11, 0x77, 0x24]);
    let mut p = vec![0xff, 0xff, 0x81, 0];
    p.extend((8 + b.len() as u32).to_be_bytes());
    p.extend([0, 0, 0, 1, 1, 1, 2, 0]);
    p.extend(b);
    udp(
        "192.0.2.2:30490".parse().expect("constant IPv4"),
        "224.244.224.245:30490".parse().expect("constant IPv4"),
        &p,
    )
    .expect("bounded SD offer")
}
/// Only a complete Ethernet/IPv4/TCP DoIP routing activation request is answered.
/// VLAN, segmentation, and diagnostic services are not guessed or interpreted here.
pub(crate) fn routing_response(frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() > MAX_SEND_BYTES || frame.get(12..14)? != [8, 0] {
        return None;
    }
    let ip = frame.get(14..)?;
    if ip.len() < 20 || ip[0] >> 4 != 4 || ip[9] != 6 {
        return None;
    }
    let ihl = usize::from(ip[0] & 15) * 4;
    if ihl < 20 || ip.len() < ihl + 20 || u16::from_be_bytes([ip[6], ip[7]]) & 0x3fff != 0 {
        return None;
    }
    let total = usize::from(u16::from_be_bytes([ip[2], ip[3]]));
    let ip = ip.get(..total)?;
    let tcp = ip.get(ihl..)?;
    if tcp.len() < 20 {
        return None;
    }
    let th = usize::from(tcp[12] >> 4) * 4;
    if th < 20 {
        return None;
    }
    let p = tcp.get(th..)?;
    if p.len() < 15
        || p[..4] != [2, 0xfd, 0, 5]
        || p[4..8] != [0, 0, 0, 7]
        || tcp[2..4] != 13400u16.to_be_bytes()
    {
        return None;
    }
    let src = IpAddr::from(<[u8; 4]>::try_from(&ip[16..20]).ok()?);
    let dst = IpAddr::from(<[u8; 4]>::try_from(&ip[12..16]).ok()?);
    let payload = doip(6, &[p[8], p[9], 0x10, 0, 0x10, 0, 0, 0, 0]);
    let mut t = vec![0; 20];
    t[..2].copy_from_slice(&tcp[2..4]);
    t[2..4].copy_from_slice(&tcp[..2]);
    t[4..8].copy_from_slice(&tcp[8..12]);
    let seq = u32::from_be_bytes(tcp[4..8].try_into().ok()?);
    t[8..12].copy_from_slice(&seq.wrapping_add(p.len() as u32).to_be_bytes());
    t[12] = 0x50;
    t[13] = 0x18;
    t[14..16].copy_from_slice(&65535u16.to_be_bytes());
    t.extend(payload);
    let c = transport_checksum(src, dst, 6, &t)?;
    t[16..18].copy_from_slice(&c.to_be_bytes());
    let mut out = ethernet_ip(src, dst, 6, &t)?;
    out[..6].copy_from_slice(&frame[6..12]);
    out[6..12].copy_from_slice(&frame[..6]);
    Some(out)
}
