mod common;

use common::*;
use loom_eth_decode::{
    CompactLayers, Decoded, Dir, FIELDS, FieldValue, FrameMeta, Origin, StreamTable, decode, field_def,
    linktype,
};

fn assert_registered(d: &Decoded) {
    for l in &d.layers {
        assert!(
            field_def(l.name).is_some(),
            "layer {} not a registered protocol",
            l.name
        );
        for f in &l.fields {
            assert!(field_def(f.name).is_some(), "field {} not in registry", f.name);
        }
    }
}

#[test]
fn vehicle_announcement_udp() {
    let mut t = StreamTable::new();
    let d = dec(&mut t, 1, &udp4(ECU, [255; 4], 13400, 13400, &announcement()));
    assert_registered(&d);
    assert_eq!(d.summary.proto, "DoIP");
    assert_eq!(
        d.summary.info,
        "Vehicle announcement (0x0004) VIN=LOOM0000000000001 LA=0x1000"
    );
    assert_eq!(first_u(&d, "doip.type"), Some(4));
    assert_eq!(first_u(&d, "doip.payload_type"), Some(4), "alias");
    assert_eq!(values(&d, "doip.vin"), vec!["LOOM0000000000001"]);
    assert_eq!(first_u(&d, "doip.further_action"), Some(0));
    assert_eq!(
        first_u(&d, "doip.futher_action"),
        Some(0),
        "old Wireshark spelling"
    );
    assert_eq!(d.summary.src, "192.0.2.10");
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn routing_activation_and_uds_over_tcp() {
    let mut t = StreamTable::new();
    let a = Flags {
        ack: Some(1),
        ..Flags::default()
    };
    let syn = dec(
        &mut t,
        1,
        &tcp4(
            true,
            1000,
            Flags {
                syn: true,
                ..Flags::default()
            },
            &[],
        ),
    );
    assert_eq!(syn.summary.info, "50000 → 13400 [SYN] Seq=0 Win=65535 Len=0");
    let sa = dec(
        &mut t,
        2,
        &tcp4(
            false,
            5000,
            Flags {
                syn: true,
                ack: Some(1001),
                ..Flags::default()
            },
            &[],
        ),
    );
    assert_eq!(
        sa.summary.info,
        "13400 → 50000 [SYN, ACK] Seq=0 Ack=1 Win=65535 Len=0"
    );

    let rr = routing_req();
    let d = dec(&mut t, 3, &tcp4(true, 1001, Flags { ack: Some(5001), ..a }, &rr));
    assert_registered(&d);
    assert_eq!(
        d.summary.info,
        "Routing activation request (0x0005) SA=0x0E00 type=0x00"
    );
    assert_eq!(first_u(&d, "tcp.stream"), Some(0));
    assert_eq!(first_u(&d, "tcp.seq"), Some(1));

    let rs = routing_resp();
    let d = dec(
        &mut t,
        4,
        &tcp4(
            false,
            5001,
            Flags {
                ack: Some(1001 + rr.len() as u32),
                ..a
            },
            &rs,
        ),
    );
    assert_eq!(
        d.summary.info,
        "Routing activation response (0x0006) SA=0x0E00 code=0x10 OK"
    );
    assert_eq!(first_u(&d, "doip.response_code"), Some(0x10));

    let rq = read_vin_req();
    let seq_c = 1001 + rr.len() as u32;
    let d = dec(&mut t, 5, &tcp4(true, seq_c, a, &rq));
    assert_eq!(d.summary.proto, "DoIP/UDS");
    assert_eq!(d.summary.info, "Diag 0x0E00→0x1000 22 F1 90 ReadDataByIdentifier");
    assert_eq!(first_u(&d, "uds.sid"), Some(0x22));
    assert_eq!(first_u(&d, "uds.did"), Some(0xF190));
    assert_eq!(first_u(&d, "doip.source_address"), Some(0x0E00));
    // Positions point into the frame (PDU fully inside this segment).
    let doip = d.layers.iter().find(|l| l.name == "doip").expect("doip layer");
    let pos = doip.pos.expect("doip pos");
    assert_eq!(pos.buf, 0);
    assert_eq!(d.bytes_at(pos), rq.as_slice());

    let rp = read_vin_resp();
    let d = dec(&mut t, 6, &tcp4(false, 5001 + rs.len() as u32, a, &rp));
    assert_eq!(
        d.summary.info,
        "Diag 0x1000→0x0E00 62 F1 90 \"LOOM0000000000001\""
    );
    assert_eq!(values(&d, "uds.reply"), vec!["true"]);

    let d = dec(
        &mut t,
        7,
        &tcp4(false, 5001 + (rs.len() + rp.len()) as u32, a, &nrc_resp()),
    );
    assert_eq!(
        d.summary.info,
        "Diag 0x1000→0x0E00 7F 22 31 NRC requestOutOfRange"
    );
    assert_eq!(first_u(&d, "uds.nrc"), Some(0x31));
    assert_eq!(first_u(&d, "uds.err.code"), Some(0x31), "alias");
    assert_eq!(first_u(&d, "uds.err.sid"), Some(0x22));
}

#[test]
fn vlan_someip_sd_offer() {
    let mut t = StreamTable::new();
    let d = dec(
        &mut t,
        1,
        &udp4_vlan(
            42,
            [192, 0, 2, 2],
            [224, 244, 224, 245],
            30490,
            30490,
            &sd_offer(),
        ),
    );
    assert_registered(&d);
    assert_eq!(d.summary.proto, "SOME/IP-SD");
    assert_eq!(d.summary.info, "SOME/IP-SD OfferService 0x1234.1 ttl=3");
    assert_eq!(first_u(&d, "vlan.id"), Some(42));
    assert_eq!(first_u(&d, "someipsd.entry.serviceid"), Some(0x1234));
    assert_eq!(first_u(&d, "someipsd.entry.service_id"), Some(0x1234), "alias");
    assert_eq!(first_u(&d, "someip.serviceid"), Some(0xFFFF));
    assert_eq!(values(&d, "someipsd.option.ipv4"), vec!["192.0.2.2"]);
    assert_eq!(first_u(&d, "someipsd.option.port"), Some(0x7724));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[test]
fn arp_icmp_ipv6() {
    let mut t = StreamTable::new();
    let d = dec(&mut t, 1, &arp_request());
    assert_registered(&d);
    assert_eq!(d.summary.proto, "ARP");
    assert_eq!(d.summary.info, "Who has 192.0.2.10? Tell 192.0.2.1");
    assert_eq!(first_u(&d, "arp.opcode"), Some(1));

    let d = dec(&mut t, 2, &icmp_echo());
    assert_registered(&d);
    assert_eq!(d.summary.proto, "ICMP");
    assert_eq!(d.summary.info, "Echo (ping) request id=0x1234, seq=7");

    let d = dec(&mut t, 3, &udp6(&routing_req()));
    assert_registered(&d);
    assert_eq!(d.summary.proto, "DoIP");
    assert_eq!(values(&d, "ipv6.addr"), vec!["fd00::1", "fd00::2"]);
    assert_eq!(values(&d, "udp.port"), vec!["40000", "13400"]);
}

#[test]
fn loopback_raw_and_sll_linktypes() {
    let eth = udp4(TESTER, ECU, 40000, 13400, &routing_req());
    let mut t = StreamTable::new();
    let d = decode(linktype::NULL, &null_loopback(&eth), &meta(1, 0), &mut t);
    assert_registered(&d);
    assert_eq!(first_u(&d, "null.family"), Some(2));
    assert_eq!(d.summary.proto, "DoIP");

    let raw = eth.get(14..).expect("raw").to_vec();
    let d = decode(linktype::RAW, &raw, &meta(2, 0), &mut t);
    assert_eq!(d.summary.proto, "DoIP");
    let d = decode(linktype::IPV4, &raw, &meta(3, 0), &mut t);
    assert_eq!(d.summary.proto, "DoIP");

    let mut sll = vec![0, 0, 0, 1, 0, 6, 2, 0, 0, 0, 0, 1, 0, 0, 0x08, 0x00];
    sll.extend(&raw);
    let d = decode(linktype::LINUX_SLL, &sll, &meta(4, 0), &mut t);
    assert_registered(&d);
    assert_eq!(first_u(&d, "sll.etype"), Some(0x0800));
    assert_eq!(d.summary.proto, "DoIP");

    let d = decode(999, &raw, &meta(5, 0), &mut t);
    assert_eq!(d.warnings.first().map(|w| w.code), Some("unknown_linktype"));
    assert!(d.has_protocol("frame"));
}

#[test]
fn tcp_pdu_split_over_three_segments() {
    let mut t = StreamTable::new();
    let pdu = read_vin_resp();
    let (a, b) = (pdu.len() / 3, 2 * pdu.len() / 3);
    let fl = Flags {
        ack: Some(1),
        ..Flags::default()
    };
    let s1 = dec(&mut t, 10, &tcp4(false, 7000, fl, &pdu[..a]));
    assert!(!s1.has_protocol("doip"));
    assert!(
        s1.summary.info.ends_with("[TCP segment of a reassembled PDU]"),
        "{}",
        s1.summary.info
    );
    let s2 = dec(&mut t, 11, &tcp4(false, 7000 + a as u32, fl, &pdu[a..b]));
    assert!(!s2.has_protocol("doip"));
    let s3 = dec(&mut t, 12, &tcp4(false, 7000 + b as u32, fl, &pdu[b..]));
    assert_registered(&s3);
    assert_eq!(count_layers(&s3, "doip"), 1);
    assert_eq!(first_u(&s3, "loom.pdu_first_seq"), Some(10));
    assert_eq!(
        s3.summary.info,
        "Diag 0x1000→0x0E00 62 F1 90 \"LOOM0000000000001\""
    );
    let doip = s3.layers.iter().find(|l| l.name == "doip").expect("doip");
    let pos = doip.pos.expect("pos");
    assert_ne!(pos.buf, 0, "reassembled PDU lives in its own buffer");
    assert_eq!(s3.bytes_at(pos), pdu.as_slice());
}

#[test]
fn tcp_two_pdus_in_one_segment() {
    let mut t = StreamTable::new();
    let mut two = routing_req();
    two.extend(read_vin_req());
    let d = dec(
        &mut t,
        1,
        &tcp4(
            true,
            100,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &two,
        ),
    );
    assert_eq!(count_layers(&d, "doip"), 2);
    assert_eq!(count_layers(&d, "uds"), 1);
    assert_eq!(
        d.summary.info,
        "Routing activation request (0x0005) SA=0x0E00 type=0x00; Diag 0x0E00→0x1000 22 F1 90 ReadDataByIdentifier"
    );
    let second = d
        .layers
        .iter()
        .filter(|l| l.name == "doip")
        .nth(1)
        .expect("second doip");
    assert_eq!(second.pos.map(|p| p.buf), Some(0));
    assert_eq!(values(&d, "doip.type"), vec!["5", "32769"]);
}

#[test]
fn tcp_out_of_order_and_retransmission() {
    let mut t = StreamTable::new();
    let pdu = read_vin_resp();
    let h = pdu.len() / 2;
    let fl = Flags {
        ack: Some(1),
        ..Flags::default()
    };
    // Establish the next expected seq with an empty ACK, then deliver the second half first.
    dec(&mut t, 1, &tcp4(false, 9000, fl, &[]));
    let late = dec(&mut t, 2, &tcp4(false, 9000 + h as u32, fl, &pdu[h..]));
    assert_eq!(values(&late, "tcp.analysis.lost_segment"), vec!["true"]);
    assert!(!late.has_protocol("doip"));
    let fill = dec(&mut t, 3, &tcp4(false, 9000, fl, &pdu[..h]));
    assert_eq!(values(&fill, "tcp.analysis.out_of_order"), vec!["true"]);
    assert_eq!(count_layers(&fill, "doip"), 1);
    assert_eq!(
        first_u(&fill, "loom.pdu_first_seq"),
        None,
        "first byte arrived in this packet"
    );
    let re = dec(&mut t, 4, &tcp4(false, 9000, fl, &pdu[..h]));
    assert_eq!(values(&re, "tcp.analysis.retransmission"), vec!["true"]);
    assert!(
        re.summary.info.starts_with("[TCP Retransmission] "),
        "{}",
        re.summary.info
    );
    assert!(!re.has_protocol("doip"));
    // Overlap: re-sent bytes plus new PDU — only the new part is used.
    let mut more = pdu[h..].to_vec();
    more.extend(routing_req());
    let ov = dec(&mut t, 5, &tcp4(false, 9000 + h as u32, fl, &more));
    assert_eq!(count_layers(&ov, "doip"), 1);
    assert_eq!(first_u(&ov, "doip.type"), Some(5));
}

#[test]
fn new_syn_after_reset_gets_new_stream_number() {
    let mut t = StreamTable::new();
    let s = Flags {
        syn: true,
        ..Flags::default()
    };
    let a = dec(&mut t, 1, &tcp4(true, 100, s, &[]));
    dec(
        &mut t,
        2,
        &tcp4(
            true,
            101,
            Flags {
                rst: true,
                ..Flags::default()
            },
            &[],
        ),
    );
    let b = dec(&mut t, 3, &tcp4(true, 500, s, &[]));
    assert_eq!(first_u(&a, "tcp.stream"), Some(0));
    assert_eq!(first_u(&b, "tcp.stream"), Some(1));
}

#[test]
fn meta_fields_and_json_shapes() {
    let mut t = StreamTable::new();
    let m = FrameMeta {
        seq: 41240,
        ts_ns: 5,
        dir: Some(Dir::Tx),
        attributed_origin: Some(Origin::Agent),
        request_id: Some("t-000418".into()),
        orig_len: None,
    };
    let d = decode(
        linktype::ETHERNET,
        &tcp4(
            true,
            1,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &read_vin_req(),
        ),
        &m,
        &mut t,
    );
    assert_eq!(values(&d, "loom.origin"), vec!["agent"]);
    assert_eq!(values(&d, "loom.request_id"), vec!["t-000418"]);
    assert_eq!(values(&d, "loom.dir"), vec!["tx"]);
    assert_eq!(values(&d, "ip.addr"), vec!["192.0.2.1", "192.0.2.10"]);
    let proto: Vec<FieldValue> = d.field_values("doip").collect();
    assert_eq!(proto.len(), 1);
    assert_eq!(proto.first(), Some(&FieldValue::Bytes(read_vin_req())));

    let compact = serde_json::to_value(CompactLayers(&d.layers)).expect("json");
    assert_eq!(compact["doip"]["type"], "0x8001");
    assert_eq!(compact["doip"]["source_address"], "0x0E00");
    assert_eq!(compact["tcp"]["srcport"], 50000);
    assert_eq!(compact["uds"]["sid"], "0x22");
    assert_eq!(compact["loom"]["seq"], "41240");
    let tree = serde_json::to_value(&d).expect("tree");
    let tcp = tree["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|l| l["name"] == "tcp")
        .expect("tcp");
    let flags_parent = tcp["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .find(|f| f["name"] == "tcp.flags")
        .expect("flags");
    assert_eq!(flags_parent["children"].as_array().map(Vec::len), Some(6));
    assert!(tcp["off"].is_u64());
}

#[test]
fn registry_is_consistent() {
    for (i, f) in FIELDS.iter().enumerate() {
        assert!(
            FIELDS.iter().skip(i + 1).all(|g| g.name != f.name),
            "duplicate {}",
            f.name
        );
        for a in f.aliases {
            assert_eq!(field_def(a).map(|d| d.name), Some(f.name), "alias {a}");
        }
        for u in f.union {
            assert!(field_def(u).is_some(), "union member {u}");
        }
        if f.name.contains('.') {
            let proto = f.name.split('.').next().unwrap_or_default();
            assert!(field_def(proto).is_some(), "protocol of {}", f.name);
        }
    }
}
