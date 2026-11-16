//! operator-upgrade C1 (ETH-RT-02) — `Decoded::tcp_rel`: relative TCP numbers are marked as
//! SYN-based only when the decoding table saw that SYN itself (FUNCTIONS.md FC1.1/FC1.2).
mod common;

use common::*;
use loom_eth_decode::{StreamTable, TcpRelBase};

const C: u16 = 50000;
const S: u16 = 40500; // no app protocol: plain TCP
const C_ISN: u32 = 1000;
const S_ISN: u32 = 5000;

fn syn() -> Flags {
    Flags {
        syn: true,
        ..Flags::default()
    }
}
fn ack(a: u32) -> Flags {
    Flags {
        ack: Some(a),
        ..Flags::default()
    }
}
fn synack(a: u32) -> Flags {
    Flags {
        syn: true,
        ack: Some(a),
        ..Flags::default()
    }
}
fn rel(seq_from_syn: bool, ack_from_syn: bool) -> Option<TcpRelBase> {
    Some(TcpRelBase {
        seq_from_syn,
        ack_from_syn,
    })
}

/// Full handshake + data: client 15 B, server 17 B, client 32 B.
fn flow() -> Vec<Vec<u8>> {
    vec![
        tcp4_ports(true, C, S, C_ISN, syn(), &[]),
        tcp4_ports(false, C, S, S_ISN, synack(C_ISN + 1), &[]),
        tcp4_ports(true, C, S, C_ISN + 1, ack(S_ISN + 1), &[]),
        tcp4_ports(true, C, S, C_ISN + 1, ack(S_ISN + 1), &[0xAA; 15]),
        tcp4_ports(false, C, S, S_ISN + 1, ack(C_ISN + 16), &[0xBB; 17]),
        tcp4_ports(true, C, S, C_ISN + 16, ack(S_ISN + 18), &[0xCC; 32]),
    ]
}

#[test]
fn full_context_marks_every_number_syn_based() {
    let mut t = StreamTable::new();
    let f = flow();
    let d: Vec<_> = f
        .iter()
        .enumerate()
        .map(|(i, fr)| dec(&mut t, i as u64 + 1, fr))
        .collect();
    // SYN: seq from its own SYN, no ack field.
    assert_eq!(d[0].tcp_rel, rel(true, false));
    assert_eq!(first_u(&d[0], "tcp.ack"), None);
    // SYN-ACK: own SYN + client SYN seen.
    assert_eq!(d[1].tcp_rel, rel(true, true));
    for x in &d[2..] {
        assert_eq!(x.tcp_rel, rel(true, true));
    }
    assert_eq!(first_u(&d[5], "tcp.seq"), Some(16));
    assert_eq!(first_u(&d[5], "tcp.ack"), Some(18));
    assert_eq!(first_u(&d[5], "tcp.seq_raw"), Some(u64::from(C_ISN + 16)));
}

#[test]
fn mid_stream_start_is_not_syn_based_and_values_are_unchanged() {
    // Decoding starts at the 4th packet (as a show of one packet, or a capture started mid-stream).
    let f = flow();
    let mut t = StreamTable::new();
    let a = dec(&mut t, 4, &f[3]);
    assert_eq!(a.tcp_rel, rel(false, false));
    // Pre-C1 numbers are kept for the list path: first seen − 1 → relative 1; no ack base → no tcp.ack.
    assert_eq!(first_u(&a, "tcp.seq"), Some(1));
    assert_eq!(first_u(&a, "tcp.ack"), None);
    let b = dec(&mut t, 5, &f[4]);
    // Server direction first seen here; client's base is first-seen too → ack not SYN based.
    assert_eq!(b.tcp_rel, rel(false, false));
    assert_eq!(
        first_u(&b, "tcp.ack"),
        Some(16),
        "ack relative to the client's first-seen base"
    );
    let c = dec(&mut t, 6, &f[5]);
    assert_eq!(c.tcp_rel, rel(false, false));
}

#[test]
fn only_one_direction_syn_seen() {
    // Server SYN-ACK seen, client SYN not (capture started between them).
    let f = flow();
    let mut t = StreamTable::new();
    let sa = dec(&mut t, 2, &f[1]);
    assert_eq!(
        sa.tcp_rel,
        rel(true, false),
        "client ISN unknown → ack not SYN based (and absent)"
    );
    let c = dec(&mut t, 3, &f[2]);
    assert_eq!(
        c.tcp_rel,
        rel(false, true),
        "client seq first-seen; ack relative to the seen server SYN"
    );
    assert_eq!(first_u(&c, "tcp.ack"), Some(1));
}

#[test]
fn new_generation_after_fin_restarts_from_its_own_syn() {
    let mut t = StreamTable::new();
    let f = flow();
    for (i, fr) in f.iter().enumerate() {
        dec(&mut t, i as u64 + 1, fr);
    }
    let fin = Flags {
        fin: true,
        ack: Some(S_ISN + 18),
        ..Flags::default()
    };
    dec(&mut t, 7, &tcp4_ports(true, C, S, C_ISN + 48, fin, &[]));
    let sfin = Flags {
        fin: true,
        ack: Some(C_ISN + 49),
        ..Flags::default()
    };
    dec(&mut t, 8, &tcp4_ports(false, C, S, S_ISN + 18, sfin, &[]));
    // Reused 4-tuple, new SYN with another ISN: new stream, SYN based again.
    let n = dec(&mut t, 9, &tcp4_ports(true, C, S, 9000, syn(), &[]));
    assert_eq!(n.tcp_rel, rel(true, false));
    let d = dec(&mut t, 10, &tcp4_ports(true, C, S, 9001, ack(1), &[0xDD; 3]));
    assert_eq!(
        d.tcp_rel,
        rel(true, false),
        "server of the new generation not seen"
    );
    assert_eq!(first_u(&d, "tcp.seq"), Some(1));
}

#[test]
fn non_tcp_packets_have_no_tcp_rel() {
    let mut t = StreamTable::new();
    let u = dec(&mut t, 1, &udp4(TESTER, ECU, 40000, 65201, b"x"));
    assert_eq!(u.tcp_rel, None);
}
