//! Golden table: filter text → (fixture, expected match) or expected error offset.

mod common;

use common::{Fx, decoded};
use loom_eth_filter::Filter;

#[derive(Debug, Clone, Copy)]
enum Exp {
    /// Compiles; result on the fixture.
    M(Fx, bool),
    /// Fails to compile at this byte offset.
    E(usize),
}
use Exp::{E, M};
use Fx::*;

const T: bool = true;
const F: bool = false;

#[rustfmt::skip]
const CASES: &[(&str, Exp)] = &[
    // presence
    ("doip", M(Req, T)), ("uds", M(Req, T)), ("uds", M(Ann, F)), ("tcp", M(Req, T)), ("udp", M(Req, F)),
    ("someipsd", M(Sd, T)), ("someip", M(Sd, T)), ("vlan", M(Sd, T)), ("vlan", M(Req, F)), ("arp", M(Arp, T)),
    ("icmp", M(Icmp, T)), ("ipv6", M(V6, T)), ("ip", M(V6, F)), ("frame", M(Req, T)), ("loom.request_id", M(Req, T)),
    ("loom.request_id", M(Ann, F)), ("tcp.flags.syn", M(Syn, T)), ("tcp.flags.syn", M(Req, T)),
    ("tcp.analysis.retransmission", M(Req, F)), ("not doip", M(Ann, F)), ("!arp", M(Req, T)), ("  doip  ", M(Req, T)),
    // integers
    ("doip.type == 0x8001", M(Req, T)), ("doip.type == 32769", M(Req, T)), ("doip.type eq 0x8001", M(Req, T)),
    ("doip.type == 0x0004", M(Ann, T)), ("doip.payload_type == 0x0004", M(Ann, T)), ("doip.type != 0x8001", M(Req, F)),
    ("doip.type != 0x8001", M(Ann, T)), ("doip.type != 0x8001", M(Arp, T)), ("doip.type ne 4", M(Ann, F)),
    ("doip.type == 0b100", M(Ann, T)), ("uds.sid == 0x22", M(Req, T)), ("uds.sid == 0x62", M(Resp, T)),
    ("uds.did == 0xf190", M(Req, T)), ("uds.did == 0xF190", M(Resp, T)), ("uds.nrc == 0x31", M(Nrc, T)),
    ("uds.err.code == 49", M(Nrc, T)), ("uds.err.sid == 0x22", M(Nrc, T)), ("uds.nrc == 0x31", M(Resp, F)),
    ("uds.reply == 1", M(Resp, T)), ("uds.reply == true", M(Req, F)), ("uds.reply == false", M(Req, T)),
    ("uds.reply == 0", M(Resp, F)), ("uds.reply == TRUE", M(Resp, T)), ("doip.source_address == 0x0e00", M(Req, T)),
    ("doip.target_address == 0x1000", M(Req, T)), ("doip.source_address == 0x1000", M(Resp, T)),
    ("doip.logical_address == 0x1000", M(Ann, T)), ("doip.further_action == 0", M(Ann, T)),
    ("doip.futher_action == 0", M(Ann, T)), ("doip.length == 7", M(Req, T)), ("doip.length > 6", M(Req, T)),
    ("doip.length < 7", M(Req, F)), ("doip.length >= 7", M(Req, T)), ("doip.length <= 6", M(Req, F)),
    ("doip.length gt 6 and doip.length lt 8", M(Req, T)), ("doip.length ge 8", M(Req, F)), ("doip.length le 7", M(Req, T)),
    ("doip.version == 2", M(Req, T)), ("doip.inverse_version == 0xfd", M(Req, T)),
    // strings
    ("doip.vin == \"LOOM0000000000001\"", M(Ann, T)), ("doip.vin == LOOM0000000000001", M(Ann, T)),
    ("doip.vin contains \"LOOM\"", M(Ann, T)), ("doip.vin contains \"XYZ\"", M(Ann, F)),
    ("doip.vin matches \"^LOOM0+1$\"", M(Ann, T)), ("doip.vin ~ \"^XYZ\"", M(Ann, F)), ("doip.vin > \"LOOM\"", M(Ann, T)),
    ("doip.vin < \"LOOM\"", M(Ann, F)), ("doip.vin[0:4] == \"LOOM\"", M(Ann, T)), ("doip.vin[0:4] == 4c:4f:4f:4d", M(Ann, T)),
    // ports, stream, flags
    ("tcp.port == 13400", M(Req, T)), ("tcp.port == 50000", M(Req, T)), ("tcp.srcport == 50000", M(Req, T)),
    ("tcp.dstport == 50000", M(Req, F)), ("tcp.dstport == 50000", M(Resp, T)), ("udp.port == 13400", M(Ann, T)),
    ("udp.port == 13400", M(Req, F)), ("tcp.stream == 0", M(Req, T)), ("tcp.len == 15", M(Req, T)),
    ("tcp.len > 100", M(Req, F)), ("tcp.flags.syn == 1", M(Syn, T)), ("tcp.flags.syn == 1", M(Req, F)),
    ("tcp.flags.push", M(Req, T)), ("tcp.flags.push == true", M(Req, T)), ("tcp.flags.rst == 0", M(Req, T)),
    ("tcp.flags.reset == 1", M(Req, F)), ("tcp.flags.ack == 1", M(Req, T)), ("tcp.seq == 1", M(Req, T)),
    ("tcp.seq == 0", M(Syn, T)), ("tcp.window_size_value == 65535", M(Req, T)), ("tcp.window == 65535", M(Req, T)),
    ("tcp.flags == 0x018", M(Req, T)), ("tcp.flags == 0x002", M(Syn, T)), ("tcp.port in {13400 50000}", M(Req, T)),
    ("udp.length == 48", M(Ann, T)), ("udp.srcport == 40000", M(V6, T)),
    // IPv4
    ("ip.src == 192.0.2.1", M(Req, T)), ("ip.dst == 192.0.2.10", M(Req, T)), ("ip.addr == 192.0.2.10", M(Req, T)),
    ("ip.addr == 192.0.2.10", M(Resp, T)), ("ip.addr == 10.0.0.1", M(Req, F)), ("ip.addr == 192.0.2.0/24", M(Req, T)),
    ("ip.addr == 10.0.0.0/8", M(Req, F)), ("ip.src == 192.0.2.0/30", M(Req, T)), ("ip.dst == 192.0.2.0/30", M(Req, F)),
    ("ip.addr != 192.0.2.1", M(Req, F)), ("ip.addr != 10.1.1.1", M(Req, T)), ("ip.ttl == 64", M(Req, T)),
    ("ip.ttl > 63 && ip.ttl < 65", M(Req, T)), ("ip.proto == 6", M(Req, T)), ("ip.proto == 17", M(Ann, T)),
    ("ip.dst == 255.255.255.255", M(Ann, T)), ("ip.src > 192.0.2.5", M(Resp, T)), ("ip.src < 192.0.2.5", M(Req, T)),
    ("ip.addr in {10.0.0.1, 192.0.2.10}", M(Req, T)), ("ip.addr in {10.0.0.0/8 172.16.0.0/12}", M(Req, F)),
    ("ip.addr in {192.0.2.0/24}", M(Req, T)), ("ip.src in {192.0.2.0..192.0.2.5}", M(Req, T)),
    ("ip.src in {192.0.2.2..192.0.2.5}", M(Req, F)), ("ip.flags.df == 1", M(Req, T)), ("ip.len == 55", M(Req, T)),
    ("ip.src[3] == 01", M(Req, T)), ("ip.src[0:2] == c0:00", M(Req, T)), ("ip.dst == 192.0.2.10/32", M(Req, T)),
    // IPv6
    ("ipv6.src == fd00::1", M(V6, T)), ("ipv6.addr == fd00::2", M(V6, T)), ("ipv6.addr == fd00::/8", M(V6, T)),
    ("ipv6.addr == fe80::/10", M(V6, F)), ("ipv6.dst == fd00::1", M(V6, F)), ("ipv6.hlim == 64", M(V6, T)),
    ("ipv6.nxt == 17", M(V6, T)), ("ipv6.src < fd00::2", M(V6, T)), ("ipv6.addr in {fd00::5..fd00::9}", M(V6, F)),
    // Ethernet, VLAN, slices
    ("eth.src == 02:00:00:00:00:01", M(Req, T)), ("eth.addr == 02:00:00:00:00:0a", M(Req, T)),
    ("eth.dst == 02-00-00-00-00-0a", M(Req, T)), ("eth.dst == 02.00.00.00.00.0a", M(Req, T)),
    ("eth.dst == 0200.0000.000a", E(11)), ("eth.src == 02:00:00:00:00:02", M(Req, F)),
    ("eth.src == 2:0:0:0:0:1", M(Req, T)), ("eth.type == 0x0800", M(Req, T)), ("eth.type == 0x8100", M(Sd, T)),
    ("eth.dst == ff:ff:ff:ff:ff:ff", M(Arp, T)), ("vlan.id == 42", M(Sd, T)), ("vlan.id in {1..41}", M(Sd, F)),
    ("vlan.id in {40..50}", M(Sd, T)), ("vlan.etype == 0x0800", M(Sd, T)), ("vlan.priority == 0", M(Sd, T)),
    ("eth.src[0:3] == 02:00:00", M(Req, T)), ("eth.src[0:3] == 02:00:01", M(Req, F)), ("eth.src[5] == 01", M(Req, T)),
    ("eth.src[-1:1] == 01", M(Req, T)), ("eth.src[4-5] == 00:01", M(Req, T)), ("eth.src[:2] == 02:00", M(Req, T)),
    ("eth.src[4:] == 00:01", M(Req, T)), ("eth.src[6:1] == 00", M(Req, F)), ("eth.src[0:3] > 01:ff:ff", M(Req, T)),
    ("frame[12:2] == 08:00", M(Req, T)), ("frame[12:2] == 81:00", M(Sd, T)), ("frame[12:2] == 0800", M(Req, T)),
    ("frame[0] == 02", M(Req, T)), ("frame contains 22:f1:90", M(Req, T)), ("frame contains \"LOOM\"", M(Resp, T)),
    ("frame contains \"LOOM\"", M(Req, F)), ("doip contains 22:f1:90", M(Req, T)), ("doip contains 62:f1:90", M(Req, F)),
    ("uds contains f1:90", M(Req, T)), ("uds == 22:f1:90", M(Req, T)), ("doip[0:2] == 02:fd", M(Req, T)),
    ("doip[2:2] == 80:01", M(Req, T)), ("frame matches \"LOOM0+1\"", M(Resp, T)), ("frame matches \"^\\x02\"", M(Req, T)),
    ("doip matches \"\\x22\\xf1\\x90$\"", M(Req, T)), ("frame contains 2b:2b:2b", M(Req, F)),
    // ARP, ICMP, SOME/IP
    ("arp.opcode == 1", M(Arp, T)), ("arp.opcode == 2", M(Arp, F)), ("arp.src.proto_ipv4 == 192.0.2.1", M(Arp, T)),
    ("arp.dst.proto_ipv4 == 192.0.2.10", M(Arp, T)), ("arp.src.hw_mac == 02:00:00:00:00:01", M(Arp, T)),
    ("icmp.type == 8", M(Icmp, T)), ("icmp.ident == 0x1234 and icmp.seq == 7", M(Icmp, T)), ("icmp.code == 0", M(Icmp, T)),
    ("someipsd.entry.serviceid == 0x1234", M(Sd, T)), ("someipsd.entry.service_id == 0x1234", M(Sd, T)),
    ("someipsd.entry.type == 0x01", M(Sd, T)), ("someipsd.entry.ttl == 3", M(Sd, T)),
    ("someipsd.entry.instanceid == 1", M(Sd, T)), ("someipsd.option.ipv4 == 192.0.2.2", M(Sd, T)),
    ("someipsd.option.port == 30500", M(Sd, T)), ("someip.serviceid == 0xffff and someip.methodid == 0x8100", M(Sd, T)),
    ("someip.service_id == 0xffff", M(Sd, T)), ("someip.messagetype == 0x02", M(Sd, T)),
    ("someipsd.flags.reboot == 1", M(Sd, T)), ("someipsd.flags.unicast == 1", M(Sd, T)),
    ("someipsd.entry.majorver == 1 && someipsd.entry.minorver == 1", M(Sd, T)), ("someip.protoversion == 1", M(Sd, T)),
    // Loom and frame fields
    ("loom.origin == \"agent\"", M(Req, T)), ("loom.origin == agent", M(Req, T)), ("loom.origin == device", M(Ann, T)),
    ("loom.origin == agent", M(Ann, F)), ("loom.dir == tx", M(Req, T)), ("loom.dir == rx", M(Req, F)),
    ("loom.request_id == \"t-000418\"", M(Req, T)), ("loom.request_id contains \"0418\"", M(Req, T)),
    ("loom.request_id matches \"^t-[0-9]+$\"", M(Req, T)), ("loom.seq == 41240", M(Req, T)), ("loom.seq > 41239", M(Req, T)),
    ("loom.seq in {1..100}", M(Ann, T)), ("frame.len == 69", M(Req, T)), ("frame.len >= 60 && frame.len <= 80", M(Req, T)),
    ("frame.cap_len == 69", M(Req, T)), ("frame.time_relative >= 2.5", M(Req, T)), ("frame.time_relative < 2.5", M(Req, F)),
    ("frame.time_relative == 2.5", M(Req, T)), ("frame.time_relative > 0.000000001", M(Req, T)),
    ("frame.time_delta == 0", M(Req, T)), ("frame.time_relative in {2..3}", M(Req, T)),
    // logic
    ("doip and uds", M(Req, T)), ("doip && !uds", M(Ann, T)), ("doip.type == 0x8001 || doip.type == 0x0004", M(Ann, T)),
    ("(doip.type == 0x8001 || doip.type == 4) and udp", M(Ann, T)), ("(doip.type == 0x8001 || doip.type == 4) and tcp", M(Ann, F)),
    ("not (tcp or udp)", M(Arp, T)), ("!(tcp || udp)", M(Req, F)), ("not not doip", M(Req, T)), ("!!!doip", M(Req, F)),
    ("doip.type in {0x8001 0x8002 0x8003}", M(Req, T)), ("doip.type in {0x8002, 0x8003}", M(Req, F)),
    ("doip.type in {0x0001..0x0008}", M(Ann, T)), ("doip.type in {0x0001 .. 0x0003}", M(Ann, F)),
    ("uds.sid in {0x22, 0x2e, 0x31}", M(Req, T)), ("uds.did in {0xf180..0xf19f}", M(Req, T)),
    ("doip.type == 5 and doip.type == 0x8001", M(Two, T)), ("doip.type != 5", M(Two, F)),
    ("uds.sid == 0x22 and ip.src == 192.0.2.1 and tcp.dstport == 13400 and loom.origin == agent", M(Req, T)),
    ("doip.type == 0x8001 AND uds", M(Req, T)), ("doip or arp", M(Arp, T)), ("doip and arp", M(Arp, F)),
    ("((doip))", M(Req, T)), ("a.b.c" , E(0)),
    ("tcp.port == 13400 and not tcp.flags.syn == 1", M(Req, T)), ("tcp.port == 13400 and not tcp.flags.syn == 1", M(Syn, F)),
    // errors (offset)
    ("", E(0)), ("doip.type ==", E(12)), ("doip.type == ", E(13)), ("doip.typo == 1", E(0)), ("DOIP", E(0)),
    ("doip.type == 0x10000", E(13)), ("uds.sid == 256", E(11)), ("ip.src == 300.1.1.1", E(10)), ("ip.src == fd00::1", E(10)),
    ("eth.src == 02:00:00", E(11)), ("doip.type == \"x\"", E(13)), ("uds.reply == maybe", E(13)),
    ("doip.vin matches LOOM", E(17)), ("doip.vin matches \"(\"", E(17)), ("doip.type contains 1", E(10)),
    ("doip.type matches \"1\"", E(10)), ("tcp.flags.syn > 0", E(14)), ("len(frame) > 3", E(3)), ("frame.len + 1 == 70", E(10)),
    ("tcp.flags & 0x02", E(10)), ("ip.addr#2 == 192.0.2.1", E(7)), ("ip.src == ip.dst", E(10)), ("doip.type === 1", E(10)),
    ("doip.type !== 1", E(10)), ("doip.type = 1", E(10)), ("doip.type == 1 or", E(17)), ("doip.type == 1 and", E(18)),
    ("(doip", E(5)), ("doip)", E(4)), ("doip.type in {}", E(13)), ("doip.type in {1, 2", E(18)), ("doip.type in 1", E(13)),
    ("doip.type {1}", E(10)), ("\"abc\"", E(0)), ("and", E(0)), ("doip.type == 1 doip", E(15)),
    ("any ip.addr == 1.1.1.1", E(0)), ("eth.src[0:x] == 02", E(8)), ("eth.src[0:3 == 02", E(12)),
    ("doip.type[0:1] == 01", E(9)), ("doip.vin == \"unterminated", E(12)), ("doip.vin == \"bad \\q\"", E(17)),
    ("$foo", E(0)), ("${foo}", E(0)), ("doip.type == 'a'", E(13)), ("doip.type == 1 ; uds", E(15)),
    ("ip.addr == 10.0.0.0/33", E(11)), ("frame.time_relative > abc", E(22)), ("frame.time_relative > 1.0000000001", E(22)),
    ("doip.type in {1..x}", E(17)), ("tcp.flags.syn in {0..1}", E(14)), ("frame contains", E(14)), ("!", E(1)),
    ("not", E(3)), ("((((((((((", E(10)), ("ip.addr == 192.0.2.1/24/1", E(11)), ("eth.dst == zz:00:00:00:00:00", E(11)),
    ("doip.type == -1", E(13)), ("doip.type == 1.5", E(13)), ("uds.sid ==0x22x", E(10)), ("ipv6.src == 192.0.2.1", E(12)),
    ("loom.seq == 18446744073709551616", E(12)), ("doip.type in {0x8001 0x8002", E(27)), ("doip.type == 1 )", E(15)),
    ("uds.sid ==", E(10)), ("eth.src[2-1] == 00", E(8)), ("doip.type matches", E(17)), ("doip.type == 0x", E(13)),
    ("ip.addr in {10.0.0.1, }", E(22)), ("ip.addr in {,10.0.0.1}", E(12)), ("tcp.port == 13400 &&", E(20)),
    ("eth.src contains", E(16)), ("eth.src[] == 01", E(8)), ("ip.addr == 192.0.2.1 or or doip", E(24)),
    ("ip.ttl < 64 || ip.ttl >", E(23)), ("loom.dir == \"tx", E(12)), ("frame[", E(6)), ("doip.type matches \"\"\"", E(20)),
];

#[test]
fn golden_cases() {
    let mut failures = Vec::new();
    for (i, (src, exp)) in CASES.iter().enumerate() {
        let got = Filter::compile(src);
        match (exp, got) {
            (M(fx, want), Ok(f)) => {
                let d = decoded(*fx);
                let m = f.matches(&d);
                if m != *want {
                    failures.push(format!("#{i} `{src}` on {fx:?}: got {m}, want {want}"));
                }
            }
            (M(_, _), Err(e)) => failures.push(format!("#{i} `{src}`: unexpected error {e}")),
            (E(pos), Err(e)) => {
                if e.pos != *pos {
                    failures.push(format!("#{i} `{src}`: error at {} ({e}), want {pos}", e.pos));
                }
            }
            (E(pos), Ok(_)) => failures.push(format!("#{i} `{src}`: compiled, want error at {pos}")),
        }
    }
    assert!(CASES.len() >= 200, "only {} cases", CASES.len());
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fields_used_for_pushdown() {
    let f = Filter::compile("doip.payload_type == 4 && (ip.addr == 192.0.2.1 || doip.type == 5) && !arp")
        .expect("ok");
    assert_eq!(f.fields_used(), &["doip.type", "ip.addr", "arp"]);
    assert_eq!(
        f.as_str(),
        "doip.payload_type == 4 && (ip.addr == 192.0.2.1 || doip.type == 5) && !arp"
    );
}

#[test]
fn time_context() {
    let f = Filter::compile("frame.time_relative < 1 && frame.time_delta >= 0.5").expect("ok");
    let d = decoded(Fx::Req); // ts 2.5 s
    let cx = loom_eth_filter::EvalCtx {
        time_ref_ns: 2_000_000_000,
        prev_ts_ns: Some(1_900_000_000),
    };
    assert!(f.matches_with(&d, &cx));
    let cx2 = loom_eth_filter::EvalCtx {
        time_ref_ns: 0,
        prev_ts_ns: Some(1_900_000_000),
    };
    assert!(!f.matches_with(&d, &cx2));
}
