//! operator-upgrade B3b (ETH-06) — read-time Decode As (FUNCTIONS.md F5.1–F5.6).
mod common;

use common::*;
use loom_eth_decode::{
    AppProto, DecodeAs, DecodeAsError, DecodeAsRule, MAX_DECODE_AS_RULES, StreamTable, Transport,
};

const R: fn(Transport, u16, AppProto) -> DecodeAsRule = |transport, port, proto| DecodeAsRule {
    transport,
    port,
    proto,
};

#[test]
fn rule_sets_are_validated_and_deduplicated() {
    assert_eq!(Transport::Tcp.as_str(), "tcp");
    assert_eq!(Transport::Udp.as_str(), "udp");
    assert_eq!(AppProto::Doip.as_str(), "doip");
    assert_eq!(AppProto::SomeIp.as_str(), "someip");
    let empty = DecodeAs::new(&[]).expect("empty ok");
    assert!(empty.is_empty());
    assert_eq!(empty, DecodeAs::default());
    let d = DecodeAs::new(&[
        R(Transport::Tcp, 64681, AppProto::Doip),
        R(Transport::Udp, 64681, AppProto::Doip),
        R(Transport::Tcp, 64681, AppProto::Doip),
    ])
    .expect("dup is fine");
    assert_eq!(
        d.rules(),
        [
            R(Transport::Tcp, 64681, AppProto::Doip),
            R(Transport::Udp, 64681, AppProto::Doip)
        ]
    );
    assert_eq!(
        DecodeAs::new(&[R(Transport::Udp, 0, AppProto::Doip)]),
        Err(DecodeAsError::BadPort)
    );
    assert_eq!(
        DecodeAs::new(&[
            R(Transport::Udp, 30500, AppProto::Doip),
            R(Transport::Udp, 30500, AppProto::SomeIp)
        ]),
        Err(DecodeAsError::Conflict {
            transport: Transport::Udp,
            port: 30500
        })
    );
    let nine: Vec<_> = (1..=9)
        .map(|p| R(Transport::Udp, 40000 + p, AppProto::SomeIp))
        .collect();
    assert_eq!(
        DecodeAs::new(&nine),
        Err(DecodeAsError::TooMany {
            max: MAX_DECODE_AS_RULES
        })
    );
    assert!(DecodeAs::new(&nine[..8]).is_ok());
    // 9 entries that dedup to 8 are fine
    let mut dup = nine[..8].to_vec();
    dup.push(nine[0]);
    assert!(DecodeAs::new(&dup).is_ok());
    assert_eq!(
        DecodeAsError::BadPort.to_string(),
        "decode_as port must be 1..65535"
    );
    assert_eq!(
        DecodeAsError::TooMany { max: 8 }.to_string(),
        "decode_as allows at most 8 rules"
    );
    assert_eq!(
        DecodeAsError::Conflict {
            transport: Transport::Udp,
            port: 30500
        }
        .to_string(),
        "decode_as maps udp port 30500 to two protocols"
    );
}

#[test]
fn lookup_prefers_the_destination_port_and_matches_either_direction() {
    let d = DecodeAs::new(&[
        R(Transport::Udp, 64681, AppProto::Doip),
        R(Transport::Udp, 50000, AppProto::SomeIp),
    ])
    .expect("ok");
    assert_eq!(
        d.lookup(Transport::Udp, 50000, 64681),
        Some(R(Transport::Udp, 64681, AppProto::Doip))
    );
    assert_eq!(
        d.lookup(Transport::Udp, 64681, 40000),
        Some(R(Transport::Udp, 64681, AppProto::Doip))
    );
    assert_eq!(
        d.lookup(Transport::Udp, 50000, 1),
        Some(R(Transport::Udp, 50000, AppProto::SomeIp))
    );
    assert_eq!(
        d.lookup(Transport::Tcp, 50000, 64681),
        None,
        "transport must match"
    );
    assert_eq!(d.lookup(Transport::Udp, 1, 2), None);
}

#[test]
fn udp_rule_decodes_doip_on_a_fixture_port_and_marks_it() {
    let frame = udp4(TESTER, ECU, 50000, 64681, &read_vin_req());
    let plain = dec(&mut StreamTable::new(), 1, &frame);
    assert_eq!(count_layers(&plain, "doip"), 0);
    assert_eq!(plain.summary.proto, "UDP");
    assert!(plain.warnings.iter().all(|w| w.code != "decode_as"));
    let rules = DecodeAs::new(&[R(Transport::Udp, 64681, AppProto::Doip)]).expect("ok");
    let mut t = StreamTable::with_decode_as(rules.clone());
    assert_eq!(t.decode_as(), &rules);
    let d = dec(&mut t, 1, &frame);
    assert_eq!(count_layers(&d, "doip"), 1);
    assert_eq!(d.summary.proto, "DoIP/UDS");
    let w: Vec<_> = d.warnings.iter().filter(|w| w.code == "decode_as").collect();
    assert_eq!(w.len(), 1);
    assert_eq!(
        (w[0].layer, w[0].detail.as_str()),
        ("udp", "UDP port 64681 decoded as doip (Decode As)")
    );
    // the response direction (rule port is the source) too
    let back = dec(&mut t, 2, &udp4(ECU, TESTER, 64681, 50000, &read_vin_resp()));
    assert_eq!(count_layers(&back, "doip"), 1);
    // udp.stream numbering is unaffected
    assert_eq!(first_u(&d, "udp.stream"), first_u(&back, "udp.stream"));
}

#[test]
fn a_rule_overrides_the_well_known_port() {
    let frame = udp4(TESTER, ECU, 50000, 13400, &sd_offer());
    let d = dec(&mut StreamTable::new(), 1, &frame);
    assert_eq!(count_layers(&d, "someip"), 0, "13400 is DoIP by default");
    let mut t = StreamTable::with_decode_as(
        DecodeAs::new(&[R(Transport::Udp, 13400, AppProto::SomeIp)]).expect("ok"),
    );
    let d = dec(&mut t, 1, &frame);
    assert!(count_layers(&d, "someip") >= 1);
    assert_eq!(count_layers(&d, "doip"), 0);
}

#[test]
fn tcp_rule_reassembles_doip_on_a_non_standard_port() {
    let rules = DecodeAs::new(&[R(Transport::Tcp, 64681, AppProto::Doip)]).expect("ok");
    let syn = Flags {
        syn: true,
        ..Flags::default()
    };
    let ack = Flags {
        ack: Some(1),
        ..Flags::default()
    };
    let req = read_vin_req();
    let frames = [
        tcp4_ports(true, 50000, 64681, 100, syn, &[]),
        tcp4_ports(true, 50000, 64681, 101, ack, &req),
    ];
    let mut plain = StreamTable::new();
    let _ = dec(&mut plain, 1, &frames[0]);
    let p = dec(&mut plain, 2, &frames[1]);
    assert_eq!(count_layers(&p, "doip"), 0);
    let mut t = StreamTable::with_decode_as(rules);
    let _ = dec(&mut t, 1, &frames[0]);
    let d = dec(&mut t, 2, &frames[1]);
    assert_eq!(count_layers(&d, "doip"), 1);
    assert_eq!(d.summary.proto, "DoIP/UDS");
    assert!(first_u(&d, "tcp.stream").is_some());
    let w: Vec<_> = d.warnings.iter().filter(|w| w.code == "decode_as").collect();
    assert_eq!(
        (w[0].layer, w[0].detail.as_str()),
        ("tcp", "TCP port 64681 decoded as doip (Decode As)")
    );
}
