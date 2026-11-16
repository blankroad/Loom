//! operator-upgrade C1 (ETH-RT-02) — `eth.show` decodes a TCP packet in its stream context, marks
//! relative numbers it cannot know, and every ready tree `filter` selects the packet it came from
//! (FUNCTIONS.md FC1.3–FC1.6). Runtime finding: show of a mid-stream packet printed relative
//! `tcp.seq` 1 and offered `tcp.seq == 1`, which selected other packets.

mod support;

use std::time::Duration;

use loom_eth::gateway::Eth;
use loom_eth::gateway::handlers::{
    SHOW_CONTEXT_SEQS, TCP_REL_UNKNOWN, WARN_TCP_REL_UNKNOWN, WARN_TCP_REL_UNKNOWN_DETAIL, filter_check,
};
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

use support::*;

const C: u16 = 50000;
const S: u16 = 40500;
const C_ISN: u32 = 1000;
const S_ISN: u32 = 5000;
const SYN: u8 = 0x02;
const ACK: u8 = 0x10;
const PSH_ACK: u8 = 0x18;

/// Ethernet + IPv4 + TCP (20-byte header, checksums zero).
fn tcp4(to_srv: bool, seq: u32, ack: u32, flags: u8, payload: &[u8]) -> Vec<u8> {
    let (s, d, sp, dp) = if to_srv {
        (TESTER, ECU, C, S)
    } else {
        (ECU, TESTER, S, C)
    };
    let total = (20 + 20 + payload.len()) as u16;
    let mut f = vec![0x02, 0, 0, 0, 0, 0x0A, 0x02, 0, 0, 0, 0, 0x02, 0x08, 0x00];
    f.extend([
        0x45,
        0,
        (total >> 8) as u8,
        total as u8,
        0,
        1,
        0x40,
        0,
        64,
        6,
        0,
        0,
    ]);
    f.extend(s);
    f.extend(d);
    f.extend(sp.to_be_bytes());
    f.extend(dp.to_be_bytes());
    f.extend(seq.to_be_bytes());
    f.extend(ack.to_be_bytes());
    f.extend([0x50, flags, 0xFF, 0xFF, 0, 0, 0, 0]);
    f.extend(payload);
    f
}

/// Handshake + data: client 15 B, server 17 B, client 32 B (raw seqs from the ISNs above).
fn flow() -> Vec<Vec<u8>> {
    vec![
        tcp4(true, C_ISN, 0, SYN, &[]),
        tcp4(false, S_ISN, C_ISN + 1, SYN | ACK, &[]),
        tcp4(true, C_ISN + 1, S_ISN + 1, ACK, &[]),
        tcp4(true, C_ISN + 1, S_ISN + 1, PSH_ACK, &[0xAA; 15]),
        tcp4(false, S_ISN + 1, C_ISN + 16, PSH_ACK, &[0xBB; 17]),
        tcp4(true, C_ISN + 16, S_ISN + 18, PSH_ACK, &[0xCC; 32]),
    ]
}

/// Journal with `(seq, frame)` records on channel `qa`.
fn journal(recs: &[(u64, Vec<u8>)]) -> std::sync::Arc<Services> {
    let s = std::sync::Arc::new(Services::default());
    let base = wall_now() - 60_000_000_000;
    *s.journal.events.lock().expect("lock") = recs
        .iter()
        .map(|(seq, f)| pkt(*seq, "qa", base + seq * 1_000, f, None))
        .collect();
    s
}

fn show(svc: &std::sync::Arc<Services>, seq: u64) -> Value {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    call(
        &reg,
        svc,
        "eth.show",
        json!({"ref": format!("eth:qa@seq:{seq}")}),
        Duration::from_secs(5),
    )
    .expect("show")
}

fn nodes(v: &Value, out: &mut Vec<Value>) {
    for n in v.as_array().into_iter().flatten() {
        out.push(n.clone());
        nodes(&n["children"], out);
    }
}

fn node(r: &Value, name: &str) -> Value {
    let mut all = Vec::new();
    nodes(&r["tree"], &mut all);
    all.into_iter()
        .find(|n| n["name"] == name)
        .unwrap_or_else(|| panic!("node {name}"))
}

fn tcp_title(r: &Value) -> String {
    r["tree"]
        .as_array()
        .expect("tree")
        .iter()
        .find(|l| l["name"] == "tcp")
        .and_then(|l| l["label"].as_str())
        .expect("tcp layer")
        .to_owned()
}

fn has_warning(r: &Value) -> bool {
    r["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .any(|w| w["code"] == WARN_TCP_REL_UNKNOWN)
}

/// Every ready filter of the shown packets compiles and, over `[from, …]`, selects that packet.
fn assert_self_matching(svc: &std::sync::Arc<Services>, seqs: &[u64], from: u64) {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    for &seq in seqs {
        let r = show(svc, seq);
        let mut all = Vec::new();
        nodes(&r["tree"], &mut all);
        for n in &all {
            let Some(f) = n["filter"].as_str() else {
                assert!(n["filter"].is_null(), "{n}");
                continue;
            };
            assert_eq!(filter_check(f)["ok"], true, "{f} must compile");
            let q = call(
                &reg,
                svc,
                "eth.query",
                json!({"ch": "qa", "filter": f, "range": {"from": format!("seq:{from}")}, "max": 1000}),
                Duration::from_secs(5),
            )
            .expect("query");
            let hit: Vec<&str> = q["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .filter_map(|x| x["seq"].as_str())
                .collect();
            assert!(
                hit.contains(&seq.to_string().as_str()),
                "filter {f:?} of seq {seq} selected {hit:?}"
            );
        }
    }
}

#[test]
fn mid_stream_show_uses_the_stream_context() {
    let f = flow();
    let svc = journal(
        &f.iter()
            .cloned()
            .enumerate()
            .map(|(i, x)| (i as u64 + 1, x))
            .collect::<Vec<_>>(),
    );
    let r = show(&svc, 6);
    let seq = node(&r, "tcp.seq");
    assert_eq!(seq["label"], "Relative sequence number: 16");
    assert_eq!(seq["filter"], "tcp.seq_raw == 1016");
    let ack = node(&r, "tcp.ack");
    assert_eq!(ack["label"], "Relative acknowledgment number: 18");
    assert_eq!(ack["filter"], "tcp.ack_raw == 5018");
    assert_eq!(node(&r, "tcp.seq_raw")["filter"], "tcp.seq_raw == 1016");
    assert!(tcp_title(&r).contains("Seq: 16,"), "{}", tcp_title(&r));
    assert!(
        r["summary"]["info"]
            .as_str()
            .expect("info")
            .contains("Seq=16 Ack=18"),
        "{}",
        r["summary"]["info"]
    );
    assert!(!has_warning(&r));
    assert_eq!(
        r["tcp_rel"],
        json!({"window_from": "seq:1", "reached": true, "seq_known": true, "ack_known": true})
    );
    assert_self_matching(&svc, &[1, 2, 3, 4, 5, 6], 1);
}

#[test]
fn capture_started_mid_stream_marks_relative_numbers_unknown() {
    let f = flow();
    let svc = journal(&[(10, f[3].clone()), (11, f[4].clone()), (12, f[5].clone())]);
    let r = show(&svc, 12);
    let seq = node(&r, "tcp.seq");
    assert_eq!(
        seq["label"],
        format!("Relative sequence number: {TCP_REL_UNKNOWN}")
    );
    assert_eq!(
        seq["filter"], "tcp.seq_raw == 1016",
        "raw value still selects exactly this packet"
    );
    let ack = node(&r, "tcp.ack");
    assert_eq!(
        ack["label"],
        format!("Relative acknowledgment number: {TCP_REL_UNKNOWN}")
    );
    assert_eq!(ack["filter"], "tcp.ack_raw == 5018");
    // Raw numbers are context free and stay as they are.
    assert_eq!(node(&r, "tcp.seq_raw")["label"], "Sequence number (raw): 1016");
    assert!(tcp_title(&r).contains("Seq: ?,"), "{}", tcp_title(&r));
    let info = r["summary"]["info"].as_str().expect("info");
    assert!(info.contains("Seq=? Ack=?"), "{info}");
    let w: Vec<&Value> = r["warnings"]
        .as_array()
        .expect("w")
        .iter()
        .filter(|w| w["code"] == WARN_TCP_REL_UNKNOWN)
        .collect();
    assert_eq!(w.len(), 1);
    assert_eq!(
        (w[0]["layer"].as_str(), w[0]["detail"].as_str()),
        (Some("tcp"), Some(WARN_TCP_REL_UNKNOWN_DETAIL))
    );
    assert_eq!(
        r["tcp_rel"],
        json!({"window_from": "seq:1", "reached": true, "seq_known": false, "ack_known": false})
    );
    assert_self_matching(&svc, &[10, 11, 12], 10);
}

#[test]
fn syn_outside_the_bounded_window_is_unknown() {
    let f = flow();
    let far = SHOW_CONTEXT_SEQS + 6;
    let svc = journal(&[
        (1, f[0].clone()),
        (2, f[1].clone()),
        (3, f[2].clone()),
        (far - 2, f[3].clone()),
        (far - 1, f[4].clone()),
        (far, f[5].clone()),
    ]);
    let r = show(&svc, far);
    assert_eq!(
        node(&r, "tcp.seq")["label"],
        format!("Relative sequence number: {TCP_REL_UNKNOWN}")
    );
    assert_eq!(r["tcp_rel"]["window_from"], "seq:6");
    assert_eq!(r["tcp_rel"]["reached"], true);
    assert_eq!(r["tcp_rel"]["seq_known"], false);
    assert!(has_warning(&r));
    // Inside the window the same stream is known.
    let r = show(&svc, 3);
    assert_eq!(node(&r, "tcp.seq")["label"], "Relative sequence number: 1");
    assert_eq!(r["tcp_rel"]["seq_known"], true);
}

#[test]
fn a_lone_syn_is_known_and_has_no_ack() {
    let f = flow();
    let svc = journal(&[(1, f[0].clone())]);
    let r = show(&svc, 1);
    assert_eq!(node(&r, "tcp.seq")["label"], "Relative sequence number: 0");
    assert_eq!(
        r["tcp_rel"],
        json!({"window_from": "seq:1", "reached": true, "seq_known": true, "ack_known": null})
    );
    assert!(!has_warning(&r));
}

#[test]
fn non_tcp_show_is_unchanged() {
    let svc = journal(&[(1, udp4(TESTER, ECU, 40000, 65201, b"ping"))]);
    let r = show(&svc, 1);
    assert!(r.get("tcp_rel").is_none(), "{r}");
    assert!(!has_warning(&r));
    assert_eq!(node(&r, "udp.dstport")["filter"], "udp.dstport == 65201");
}
