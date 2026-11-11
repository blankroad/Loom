//! Link, network and transport layers (etherparse 0.21 header slices) and the application hand-off.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use etherparse::{
    ArpPacketSlice, Ethernet2HeaderSlice, Icmpv4Slice, Icmpv6Slice, Ipv4HeaderSlice, Ipv6ExtensionsSlice,
    Ipv6HeaderSlice, LinuxSllHeaderSlice, LinuxSllProtocolType, SingleVlanHeaderSlice, TcpHeaderSlice,
    UdpHeaderSlice,
};

use crate::linktype;
use crate::model::{Decoded, Loc, Pos};
use crate::rd::{be16, be32};
use crate::stream::{App, Segment, StreamTable};
use crate::value::{FieldValue, fmt_mac};
use crate::{doip, someip};

/// VLAN tags walked per frame (802.1Q / QinQ). Over: the rest is not decoded (warning).
pub const MAX_VLAN_TAGS: usize = 4;

#[derive(Debug, Default)]
struct Ctx {
    proto: String,
    src: String,
    dst: String,
    info: String,
    vlan: u16,
    vlan_tags: usize,
}

pub(crate) fn run(d: &mut Decoded, lt: u16, frame: &[u8], streams: &mut StreamTable) {
    let mut c = Ctx {
        proto: "Frame".into(),
        ..Ctx::default()
    };
    let wire_len = d
        .meta
        .orig_len
        .unwrap_or(u32::try_from(frame.len()).unwrap_or(u32::MAX));
    let fl = d.push_layer(
        "frame",
        format!("Frame {}: {} bytes", d.meta.seq, wire_len),
        Pos::frame(0, frame.len()),
    );
    d.add(fl, "frame.len", FieldValue::Uint(wire_len.into()), None);
    d.add(fl, "frame.cap_len", FieldValue::Uint(frame.len() as u64), None);
    let ll = d.push_layer("loom", "Loom".into(), None);
    let seq = d.meta.seq;
    d.add(ll, "loom.seq", FieldValue::Uint(seq), None);
    if let Some(dir) = d.meta.dir {
        d.add(ll, "loom.dir", FieldValue::Str(dir.as_str().into()), None);
    }
    let org = d.meta.attributed_origin.map(|o| o.as_str()).unwrap_or("device");
    d.add(ll, "loom.origin", FieldValue::Str(org.into()), None);
    if let Some(rid) = d.meta.request_id.clone() {
        d.add(ll, "loom.request_id", FieldValue::Str(rid), None);
    }

    match lt {
        linktype::ETHERNET => ethernet(d, &mut c, frame, 0, streams),
        linktype::NULL | linktype::LOOP => null_loop(d, &mut c, frame, lt, streams),
        linktype::RAW => match frame.first().map(|b| b >> 4) {
            Some(4) => ipv4(d, &mut c, frame, 0, frame.len(), streams),
            Some(6) => ipv6(d, &mut c, frame, 0, frame.len(), streams),
            _ => d.warn("malformed", "frame", "raw IP: unknown version nibble".into()),
        },
        linktype::IPV4 => ipv4(d, &mut c, frame, 0, frame.len(), streams),
        linktype::IPV6 => ipv6(d, &mut c, frame, 0, frame.len(), streams),
        linktype::LINUX_SLL => sll(d, &mut c, frame, streams),
        other => {
            d.warn(
                "unknown_linktype",
                "frame",
                format!("linktype {other} not decoded"),
            );
            c.info = format!("Linktype {other}");
        }
    }

    let proto = if d.has_protocol("uds") {
        "DoIP/UDS".to_string()
    } else if d.has_protocol("doip") {
        "DoIP".into()
    } else if d.has_protocol("someipsd") {
        "SOME/IP-SD".into()
    } else if d.has_protocol("someip") {
        "SOME/IP".into()
    } else {
        c.proto
    };
    d.summary.proto = proto;
    d.summary.src = c.src;
    d.summary.dst = c.dst;
    d.summary.len = wire_len;
    d.set_info(c.info);
}

fn null_loop(d: &mut Decoded, c: &mut Ctx, f: &[u8], lt: u16, streams: &mut StreamTable) {
    let Some(raw) = f.get(..4) else {
        d.warn("malformed", "null", "loopback header needs 4 bytes".into());
        return;
    };
    let be = be32(raw, 0).unwrap_or(0);
    let le = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    // NULL uses the capturing host's byte order, LOOP uses network order.
    let fam = if lt == linktype::LOOP || be <= 0xFF {
        be
    } else {
        le
    };
    let l = d.push_layer("null", format!("Null/Loopback family {fam}"), Pos::frame(0, 4));
    d.add(l, "null.family", FieldValue::Uint(fam.into()), Pos::frame(0, 4));
    c.proto = "Null".into();
    match fam {
        2 => ipv4(d, c, f, 4, f.len(), streams),
        24 | 28 | 30 => ipv6(d, c, f, 4, f.len(), streams),
        _ => c.info = format!("Loopback family {fam}"),
    }
}

fn sll(d: &mut Decoded, c: &mut Ctx, f: &[u8], streams: &mut StreamTable) {
    let h = match LinuxSllHeaderSlice::from_slice(f) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "sll", e.to_string());
            return;
        }
    };
    let n = h.slice().len();
    let l = d.push_layer("sll", "Linux cooked capture".into(), Pos::frame(0, n));
    d.add(
        l,
        "sll.pkttype",
        FieldValue::Uint(be16(f, 0).unwrap_or(0).into()),
        Pos::frame(0, 2),
    );
    d.add(
        l,
        "sll.hatype",
        FieldValue::Uint(h.arp_hardware_type().0.into()),
        Pos::frame(2, 2),
    );
    let et = be16(f, 14).unwrap_or(0);
    d.add(l, "sll.etype", FieldValue::Uint(et.into()), Pos::frame(14, 2));
    c.proto = "SLL".into();
    match h.protocol_type() {
        LinuxSllProtocolType::EtherType(t) => ethertype(d, c, f, n, t.0, streams),
        _ => c.info = format!("Linux cooked protocol 0x{et:04X}"),
    }
}

fn ethernet(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize, streams: &mut StreamTable) {
    let rest = f.get(off..).unwrap_or(&[]);
    let h = match Ethernet2HeaderSlice::from_slice(rest) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "eth", e.to_string());
            c.info = "[Malformed Ethernet]".into();
            return;
        }
    };
    let (dst, src, et) = (h.destination(), h.source(), h.ether_type().0);
    let l = d.push_layer(
        "eth",
        format!("Ethernet II, Src: {}, Dst: {}", fmt_mac(&src), fmt_mac(&dst)),
        Pos::frame(off, 14),
    );
    d.add(l, "eth.dst", FieldValue::Mac(dst), Pos::frame(off, 6));
    d.add(l, "eth.src", FieldValue::Mac(src), Pos::frame(off + 6, 6));
    d.add(
        l,
        "eth.type",
        FieldValue::Uint(et.into()),
        Pos::frame(off + 12, 2),
    );
    c.proto = "Ethernet".into();
    c.src = fmt_mac(&src);
    c.dst = fmt_mac(&dst);
    ethertype(d, c, f, off + 14, et, streams);
}

fn ethertype(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize, et: u16, streams: &mut StreamTable) {
    match et {
        0x8100 | 0x88A8 | 0x9100 => vlan(d, c, f, off, streams),
        0x0800 => ipv4(d, c, f, off, f.len(), streams),
        0x86DD => ipv6(d, c, f, off, f.len(), streams),
        0x0806 => arp(d, c, f, off),
        _ => c.info = format!("Ethertype 0x{et:04X}"),
    }
}

fn vlan(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize, streams: &mut StreamTable) {
    if c.vlan_tags >= MAX_VLAN_TAGS {
        d.warn(
            "limit_layers",
            "vlan",
            format!("more than {MAX_VLAN_TAGS} VLAN tags"),
        );
        return;
    }
    let rest = f.get(off..).unwrap_or(&[]);
    let h = match SingleVlanHeaderSlice::from_slice(rest) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "vlan", e.to_string());
            return;
        }
    };
    let (pcp, dei, id, et) = (
        h.priority_code_point().value(),
        h.drop_eligible_indicator(),
        h.vlan_identifier().value(),
        h.ether_type().0,
    );
    let l = d.push_layer(
        "vlan",
        format!("802.1Q Virtual LAN, PRI: {pcp}, ID: {id}"),
        Pos::frame(off, 4),
    );
    d.add(
        l,
        "vlan.priority",
        FieldValue::Uint(pcp.into()),
        Pos::frame(off, 2),
    );
    d.add(l, "vlan.dei", FieldValue::Bool(dei), Pos::frame(off, 2));
    d.add(l, "vlan.id", FieldValue::Uint(id.into()), Pos::frame(off, 2));
    d.add(
        l,
        "vlan.etype",
        FieldValue::Uint(et.into()),
        Pos::frame(off + 2, 2),
    );
    if c.vlan_tags == 0 {
        c.vlan = id;
    }
    c.vlan_tags += 1;
    ethertype(d, c, f, off + 4, et, streams);
}

fn arp(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize) {
    let rest = f.get(off..).unwrap_or(&[]);
    let a = match ArpPacketSlice::from_slice(rest) {
        Ok(a) => a,
        Err(e) => {
            d.warn("malformed", "arp", e.to_string());
            return;
        }
    };
    let n = a.slice().len();
    let op = a.operation().0;
    let l = d.push_layer(
        "arp",
        format!("Address Resolution Protocol (op {op})"),
        Pos::frame(off, n),
    );
    d.add(
        l,
        "arp.hw.type",
        FieldValue::Uint(a.hw_addr_type().0.into()),
        Pos::frame(off, 2),
    );
    d.add(
        l,
        "arp.proto.type",
        FieldValue::Uint(a.proto_addr_type().0.into()),
        Pos::frame(off + 2, 2),
    );
    d.add(
        l,
        "arp.opcode",
        FieldValue::Uint(op.into()),
        Pos::frame(off + 6, 2),
    );
    let hs = usize::from(a.hw_addr_size());
    let ps = usize::from(a.proto_addr_size());
    let mac = |b: &[u8]| -> Option<[u8; 6]> { b.try_into().ok() };
    let ip4 = |b: &[u8]| -> Option<Ipv4Addr> { <[u8; 4]>::try_from(b).ok().map(Ipv4Addr::from) };
    let (sha, spa, tha, tpa) = (
        mac(a.sender_hw_addr()),
        ip4(a.sender_protocol_addr()),
        mac(a.target_hw_addr()),
        ip4(a.target_protocol_addr()),
    );
    let o_sha = off + 8;
    let o_spa = o_sha + hs;
    let o_tha = o_spa + ps;
    let o_tpa = o_tha + hs;
    if let Some(m) = sha {
        d.add(l, "arp.src.hw_mac", FieldValue::Mac(m), Pos::frame(o_sha, 6));
    }
    if let Some(i) = spa {
        d.add(l, "arp.src.proto_ipv4", FieldValue::Ipv4(i), Pos::frame(o_spa, 4));
    }
    if let Some(m) = tha {
        d.add(l, "arp.dst.hw_mac", FieldValue::Mac(m), Pos::frame(o_tha, 6));
    }
    if let Some(i) = tpa {
        d.add(l, "arp.dst.proto_ipv4", FieldValue::Ipv4(i), Pos::frame(o_tpa, 4));
    }
    c.proto = "ARP".into();
    let s = |x: Option<Ipv4Addr>| x.map(|v| v.to_string()).unwrap_or_else(|| "?".into());
    c.info = match op {
        1 => format!("Who has {}? Tell {}", s(tpa), s(spa)),
        2 => format!(
            "{} is at {}",
            s(spa),
            sha.map(|m| fmt_mac(&m)).unwrap_or_else(|| "?".into())
        ),
        _ => format!("ARP opcode {op}"),
    };
}

fn ipv4(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize, end: usize, streams: &mut StreamTable) {
    let rest = f.get(off..end).unwrap_or(&[]);
    let h = match Ipv4HeaderSlice::from_slice(rest) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "ip", e.to_string());
            c.info = "[Malformed IPv4]".into();
            return;
        }
    };
    let hl = usize::from(h.ihl()) * 4;
    let total = usize::from(h.total_len());
    let (src, dst) = (h.source_addr(), h.destination_addr());
    let proto = h.protocol().0;
    let l = d.push_layer(
        "ip",
        format!("Internet Protocol Version 4, Src: {src}, Dst: {dst}"),
        Pos::frame(off, total.min(rest.len())),
    );
    d.add(l, "ip.version", FieldValue::Uint(4), Pos::frame(off, 1));
    d.add(l, "ip.hdr_len", FieldValue::Uint(hl as u64), Pos::frame(off, 1));
    d.add(
        l,
        "ip.dsfield",
        FieldValue::Uint(rest.get(1).copied().unwrap_or(0).into()),
        Pos::frame(off + 1, 1),
    );
    d.add(
        l,
        "ip.len",
        FieldValue::Uint(total as u64),
        Pos::frame(off + 2, 2),
    );
    d.add(
        l,
        "ip.id",
        FieldValue::Uint(h.identification().into()),
        Pos::frame(off + 4, 2),
    );
    d.add(
        l,
        "ip.flags.df",
        FieldValue::Bool(h.dont_fragment()),
        Pos::frame(off + 6, 1),
    );
    d.add(
        l,
        "ip.flags.mf",
        FieldValue::Bool(h.more_fragments()),
        Pos::frame(off + 6, 1),
    );
    let frag = u32::from(h.fragments_offset().value()) * 8;
    d.add(
        l,
        "ip.frag_offset",
        FieldValue::Uint(frag.into()),
        Pos::frame(off + 6, 2),
    );
    d.add(
        l,
        "ip.ttl",
        FieldValue::Uint(h.ttl().into()),
        Pos::frame(off + 8, 1),
    );
    d.add(
        l,
        "ip.proto",
        FieldValue::Uint(proto.into()),
        Pos::frame(off + 9, 1),
    );
    d.add(
        l,
        "ip.checksum",
        FieldValue::Uint(h.header_checksum().into()),
        Pos::frame(off + 10, 2),
    );
    d.add(l, "ip.src", FieldValue::Ipv4(src), Pos::frame(off + 12, 4));
    d.add(l, "ip.dst", FieldValue::Ipv4(dst), Pos::frame(off + 16, 4));
    c.proto = "IPv4".into();
    c.src = src.to_string();
    c.dst = dst.to_string();
    if total < hl {
        d.warn(
            "malformed",
            "ip",
            format!("total length {total} below header length {hl}"),
        );
        return;
    }
    if total > rest.len() {
        d.warn(
            "truncated",
            "ip",
            format!("total length {total}, captured {}", rest.len()),
        );
    }
    if h.more_fragments() || frag > 0 {
        d.warn(
            "ip_fragment",
            "ip",
            format!("fragment offset {frag}, MF {}", h.more_fragments()),
        );
        c.info = format!(
            "Fragmented IP protocol (proto={proto}, off={frag}, ID=0x{:04x})",
            h.identification()
        );
        return;
    }
    let pend = off + total.min(rest.len());
    transport(
        d,
        c,
        f,
        proto,
        off + hl,
        pend,
        IpAddr::V4(src),
        IpAddr::V4(dst),
        streams,
    );
}

fn ipv6(d: &mut Decoded, c: &mut Ctx, f: &[u8], off: usize, end: usize, streams: &mut StreamTable) {
    let rest = f.get(off..end).unwrap_or(&[]);
    let h = match Ipv6HeaderSlice::from_slice(rest) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "ipv6", e.to_string());
            c.info = "[Malformed IPv6]".into();
            return;
        }
    };
    let (src, dst): (Ipv6Addr, Ipv6Addr) = (h.source_addr(), h.destination_addr());
    let plen = usize::from(h.payload_length());
    let l = d.push_layer(
        "ipv6",
        format!("Internet Protocol Version 6, Src: {src}, Dst: {dst}"),
        Pos::frame(off, (40 + plen).min(rest.len())),
    );
    d.add(l, "ipv6.version", FieldValue::Uint(6), Pos::frame(off, 1));
    d.add(
        l,
        "ipv6.tclass",
        FieldValue::Uint(h.traffic_class().into()),
        Pos::frame(off, 2),
    );
    d.add(
        l,
        "ipv6.flow",
        FieldValue::Uint(h.flow_label().value().into()),
        Pos::frame(off + 1, 3),
    );
    d.add(
        l,
        "ipv6.plen",
        FieldValue::Uint(plen as u64),
        Pos::frame(off + 4, 2),
    );
    d.add(
        l,
        "ipv6.nxt",
        FieldValue::Uint(h.next_header().0.into()),
        Pos::frame(off + 6, 1),
    );
    d.add(
        l,
        "ipv6.hlim",
        FieldValue::Uint(h.hop_limit().into()),
        Pos::frame(off + 7, 1),
    );
    d.add(l, "ipv6.src", FieldValue::Ipv6(src), Pos::frame(off + 8, 16));
    d.add(l, "ipv6.dst", FieldValue::Ipv6(dst), Pos::frame(off + 24, 16));
    c.proto = "IPv6".into();
    c.src = src.to_string();
    c.dst = dst.to_string();
    let pstart = off + 40;
    let avail = rest.len().saturating_sub(40);
    if plen > avail {
        d.warn(
            "truncated",
            "ipv6",
            format!("payload length {plen}, captured {avail}"),
        );
    }
    let pend = pstart + if plen == 0 { avail } else { plen.min(avail) };
    let payload = f.get(pstart..pend).unwrap_or(&[]);
    match Ipv6ExtensionsSlice::from_slice(h.next_header(), payload) {
        Ok((exts, next, tail)) => {
            if exts.is_fragmenting_payload() {
                d.warn("ip_fragment", "ipv6", "fragmented IPv6 payload".into());
                c.info = "Fragmented IPv6 payload".into();
                return;
            }
            let toff = pend - tail.len();
            transport(
                d,
                c,
                f,
                next.0,
                toff,
                pend,
                IpAddr::V6(src),
                IpAddr::V6(dst),
                streams,
            );
        }
        Err(e) => d.warn("malformed", "ipv6", e.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn transport(
    d: &mut Decoded,
    c: &mut Ctx,
    f: &[u8],
    proto: u8,
    off: usize,
    end: usize,
    src: IpAddr,
    dst: IpAddr,
    streams: &mut StreamTable,
) {
    let seg = f.get(off..end).unwrap_or(&[]);
    match proto {
        1 => icmp4(d, c, seg, off),
        58 => icmp6(d, c, seg, off),
        17 => udp(d, c, seg, off),
        6 => tcp(d, c, seg, off, src, dst, streams),
        p => c.info = format!("IP protocol {p}"),
    }
}

fn icmp4(d: &mut Decoded, c: &mut Ctx, seg: &[u8], off: usize) {
    let s = match Icmpv4Slice::from_slice(seg) {
        Ok(s) => s,
        Err(e) => {
            d.warn("malformed", "icmp", e.to_string());
            return;
        }
    };
    let (t, code) = (s.type_u8(), s.code_u8());
    let l = d.push_layer(
        "icmp",
        "Internet Control Message Protocol".into(),
        Pos::frame(off, seg.len()),
    );
    d.add(l, "icmp.type", FieldValue::Uint(t.into()), Pos::frame(off, 1));
    d.add(
        l,
        "icmp.code",
        FieldValue::Uint(code.into()),
        Pos::frame(off + 1, 1),
    );
    d.add(
        l,
        "icmp.checksum",
        FieldValue::Uint(s.checksum().into()),
        Pos::frame(off + 2, 2),
    );
    c.proto = "ICMP".into();
    let b = s.bytes5to8();
    let (id, sq) = (u16::from_be_bytes([b[0], b[1]]), u16::from_be_bytes([b[2], b[3]]));
    c.info = match t {
        0 | 8 => {
            d.add(
                l,
                "icmp.ident",
                FieldValue::Uint(id.into()),
                Pos::frame(off + 4, 2),
            );
            d.add(l, "icmp.seq", FieldValue::Uint(sq.into()), Pos::frame(off + 6, 2));
            let k = if t == 8 { "request" } else { "reply" };
            format!("Echo (ping) {k} id=0x{id:04x}, seq={sq}")
        }
        3 => format!("Destination unreachable (code {code})"),
        11 => format!("Time-to-live exceeded (code {code})"),
        _ => format!("ICMP type {t} code {code}"),
    };
}

fn icmp6(d: &mut Decoded, c: &mut Ctx, seg: &[u8], off: usize) {
    let s = match Icmpv6Slice::from_slice(seg) {
        Ok(s) => s,
        Err(e) => {
            d.warn("malformed", "icmpv6", e.to_string());
            return;
        }
    };
    let (t, code) = (s.type_u8(), s.code_u8());
    let l = d.push_layer(
        "icmpv6",
        "Internet Control Message Protocol v6".into(),
        Pos::frame(off, seg.len()),
    );
    d.add(l, "icmpv6.type", FieldValue::Uint(t.into()), Pos::frame(off, 1));
    d.add(
        l,
        "icmpv6.code",
        FieldValue::Uint(code.into()),
        Pos::frame(off + 1, 1),
    );
    d.add(
        l,
        "icmpv6.checksum",
        FieldValue::Uint(s.checksum().into()),
        Pos::frame(off + 2, 2),
    );
    c.proto = "ICMPv6".into();
    let b = s.bytes5to8();
    let (id, sq) = (u16::from_be_bytes([b[0], b[1]]), u16::from_be_bytes([b[2], b[3]]));
    c.info = match t {
        128 | 129 => {
            d.add(
                l,
                "icmpv6.echo.identifier",
                FieldValue::Uint(id.into()),
                Pos::frame(off + 4, 2),
            );
            d.add(
                l,
                "icmpv6.echo.sequence_number",
                FieldValue::Uint(sq.into()),
                Pos::frame(off + 6, 2),
            );
            let k = if t == 128 { "request" } else { "reply" };
            format!("Echo (ping) {k} id=0x{id:04x}, seq={sq}")
        }
        133 => "Router Solicitation".into(),
        134 => "Router Advertisement".into(),
        135 => "Neighbor Solicitation".into(),
        136 => "Neighbor Advertisement".into(),
        _ => format!("ICMPv6 type {t} code {code}"),
    };
}

fn udp(d: &mut Decoded, c: &mut Ctx, seg: &[u8], off: usize) {
    let h = match UdpHeaderSlice::from_slice(seg) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "udp", e.to_string());
            return;
        }
    };
    let (sp, dp, ul) = (h.source_port(), h.destination_port(), usize::from(h.length()));
    let l = d.push_layer(
        "udp",
        format!("User Datagram Protocol, Src Port: {sp}, Dst Port: {dp}"),
        Pos::frame(off, seg.len().min(ul.max(8))),
    );
    d.add(l, "udp.srcport", FieldValue::Uint(sp.into()), Pos::frame(off, 2));
    d.add(
        l,
        "udp.dstport",
        FieldValue::Uint(dp.into()),
        Pos::frame(off + 2, 2),
    );
    d.add(
        l,
        "udp.length",
        FieldValue::Uint(ul as u64),
        Pos::frame(off + 4, 2),
    );
    d.add(
        l,
        "udp.checksum",
        FieldValue::Uint(h.checksum().into()),
        Pos::frame(off + 6, 2),
    );
    c.proto = "UDP".into();
    if ul < 8 {
        d.warn("malformed", "udp", format!("length {ul} below 8"));
        return;
    }
    if ul > seg.len() {
        d.warn("truncated", "udp", format!("length {ul}, captured {}", seg.len()));
    }
    let payload = seg.get(8..ul.min(seg.len())).unwrap_or(&[]);
    c.info = format!("{sp} → {dp} Len={}", ul - 8);
    if payload.is_empty() {
        return;
    }
    let loc = Loc { buf: 0, off: off + 8 };
    let infos = if sp == doip::DOIP_PORT || dp == doip::DOIP_PORT {
        doip::decode_datagram(d, payload, loc)
    } else if sp == someip::SOMEIP_SD_PORT
        || dp == someip::SOMEIP_SD_PORT
        || (sp.min(dp) >= 1024 && someip::looks_like(payload))
    {
        someip::decode_datagram(d, payload, loc)
    } else {
        Vec::new()
    };
    if !infos.is_empty() {
        c.info = infos.join("; ");
    }
}

fn tcp_flags(h: &TcpHeaderSlice<'_>) -> String {
    let mut v: Vec<&str> = Vec::new();
    for (on, n) in [
        (h.fin(), "FIN"),
        (h.syn(), "SYN"),
        (h.rst(), "RST"),
        (h.psh(), "PSH"),
        (h.ack(), "ACK"),
        (h.urg(), "URG"),
    ] {
        if on {
            v.push(n);
        }
    }
    v.join(", ")
}

fn tcp(
    d: &mut Decoded,
    c: &mut Ctx,
    seg: &[u8],
    off: usize,
    src: IpAddr,
    dst: IpAddr,
    streams: &mut StreamTable,
) {
    let h = match TcpHeaderSlice::from_slice(seg) {
        Ok(h) => h,
        Err(e) => {
            d.warn("malformed", "tcp", e.to_string());
            return;
        }
    };
    let hl = h.slice().len();
    let (sp, dp) = (h.source_port(), h.destination_port());
    let payload = seg.get(hl..).unwrap_or(&[]);
    let r = streams.feed(&Segment {
        vlan: c.vlan,
        src: (src, sp),
        dst: (dst, dp),
        seq: h.sequence_number(),
        ack: h.acknowledgment_number(),
        syn: h.syn(),
        ack_flag: h.ack(),
        fin: h.fin(),
        rst: h.rst(),
        payload,
        pkt_seq: d.meta.seq,
        ts_ns: d.meta.ts_ns,
    });
    for (code, msg) in &r.warnings {
        d.warn(code, "tcp", msg.clone());
    }
    let flags16 = be16(seg, 12).unwrap_or(0) & 0x0FFF;
    let l = d.push_layer(
        "tcp",
        format!(
            "Transmission Control Protocol, Src Port: {sp}, Dst Port: {dp}, Seq: {}, Len: {}",
            r.rel_seq,
            payload.len()
        ),
        Pos::frame(off, hl),
    );
    d.add(l, "tcp.srcport", FieldValue::Uint(sp.into()), Pos::frame(off, 2));
    d.add(
        l,
        "tcp.dstport",
        FieldValue::Uint(dp.into()),
        Pos::frame(off + 2, 2),
    );
    d.add(l, "tcp.stream", FieldValue::Uint(r.stream), None);
    d.add(l, "tcp.len", FieldValue::Uint(payload.len() as u64), None);
    d.add(
        l,
        "tcp.seq",
        FieldValue::Uint(r.rel_seq.into()),
        Pos::frame(off + 4, 4),
    );
    d.add(
        l,
        "tcp.seq_raw",
        FieldValue::Uint(h.sequence_number().into()),
        Pos::frame(off + 4, 4),
    );
    if let Some(a) = r.rel_ack {
        d.add(l, "tcp.ack", FieldValue::Uint(a.into()), Pos::frame(off + 8, 4));
    }
    if h.ack() {
        d.add(
            l,
            "tcp.ack_raw",
            FieldValue::Uint(h.acknowledgment_number().into()),
            Pos::frame(off + 8, 4),
        );
    }
    d.add(
        l,
        "tcp.hdr_len",
        FieldValue::Uint(hl as u64),
        Pos::frame(off + 12, 1),
    );
    d.add(
        l,
        "tcp.flags",
        FieldValue::Uint(flags16.into()),
        Pos::frame(off + 12, 2),
    );
    for (n, v) in [
        ("tcp.flags.fin", h.fin()),
        ("tcp.flags.syn", h.syn()),
        ("tcp.flags.reset", h.rst()),
        ("tcp.flags.push", h.psh()),
        ("tcp.flags.ack", h.ack()),
        ("tcp.flags.urg", h.urg()),
    ] {
        d.add_full(l, n, FieldValue::Bool(v), None, Pos::frame(off + 13, 1), 1);
    }
    d.add(
        l,
        "tcp.window_size_value",
        FieldValue::Uint(h.window_size().into()),
        Pos::frame(off + 14, 2),
    );
    d.add(
        l,
        "tcp.checksum",
        FieldValue::Uint(h.checksum().into()),
        Pos::frame(off + 16, 2),
    );
    let mut tags: Vec<&str> = Vec::new();
    if r.retransmission {
        d.add(l, "tcp.analysis.retransmission", FieldValue::Bool(true), None);
        tags.push("[TCP Retransmission] ");
    }
    if r.out_of_order {
        d.add(l, "tcp.analysis.out_of_order", FieldValue::Bool(true), None);
        tags.push("[TCP Out-Of-Order] ");
    }
    if r.lost_segment {
        d.add(l, "tcp.analysis.lost_segment", FieldValue::Bool(true), None);
        tags.push("[TCP Previous segment not captured] ");
    }
    c.proto = "TCP".into();
    let ack = r.rel_ack.map(|a| format!(" Ack={a}")).unwrap_or_default();
    c.info = format!(
        "{}{sp} → {dp} [{}] Seq={}{ack} Win={} Len={}",
        tags.concat(),
        tcp_flags(&h),
        r.rel_seq,
        h.window_size(),
        payload.len()
    );
    let mut infos = Vec::new();
    for p in r.pdus {
        let loc = match p.payload_off {
            Some(po) => Loc {
                buf: 0,
                off: off + hl + po,
            },
            None => Loc {
                buf: d.add_buffer(p.bytes.clone()).unwrap_or(u8::MAX),
                off: 0,
            },
        };
        let first = (p.first_pkt != d.meta.seq).then_some(p.first_pkt);
        infos.push(match p.app {
            App::Doip => doip::decode(d, &p.bytes, loc, first),
            App::SomeIp => someip::decode_msg(d, &p.bytes, loc, first),
        });
    }
    if !infos.is_empty() {
        c.info = format!("{}{}", tags.concat(), infos.join("; "));
    } else if r.partial && !payload.is_empty() {
        c.info.push_str(" [TCP segment of a reassembled PDU]");
    }
}
