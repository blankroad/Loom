mod common;

use common::*;
use loom_eth_decode::{
    Decoded, FrameMeta, MAX_FIELDS, MAX_STREAM_BUFFERED_BYTES, MAX_STREAMS, MAX_WARNINGS, STREAM_IDLE_NS,
    StreamTable, decode, linktype,
};

fn has_warning(d: &Decoded, code: &str) -> bool {
    d.warnings.iter().any(|w| w.code == code)
}

#[test]
fn stream_1025_evicts_lru() {
    let mut t = StreamTable::new();
    let mut last = None;
    for i in 0..=MAX_STREAMS {
        let port = 10_000 + u16::try_from(i).expect("port");
        let f = tcp4_ports(
            true,
            port,
            13400,
            1,
            Flags {
                syn: true,
                ..Flags::default()
            },
            &[],
        );
        // Same timestamp for all: nothing is idle, so the 1025th must evict the LRU.
        last = Some(decode(linktype::ETHERNET, &f, &meta(i as u64, 1_000), &mut t));
    }
    let d = last.expect("decoded");
    assert!(has_warning(&d, "stream_evicted"), "{:?}", d.warnings);
    assert_eq!(t.len(), MAX_STREAMS);
    assert_eq!(t.evicted(), 1);
    assert_eq!(first_u(&d, "tcp.stream"), Some(MAX_STREAMS as u64));
}

#[test]
fn idle_streams_are_evicted_by_timestamp() {
    let mut t = StreamTable::new();
    let s = Flags {
        syn: true,
        ..Flags::default()
    };
    decode(
        linktype::ETHERNET,
        &tcp4_ports(true, 1111, 13400, 1, s, &[]),
        &meta(1, 0),
        &mut t,
    );
    decode(
        linktype::ETHERNET,
        &tcp4_ports(true, 2222, 13400, 1, s, &[]),
        &meta(2, STREAM_IDLE_NS + 1),
        &mut t,
    );
    assert_eq!(t.len(), 1);
    assert_eq!(t.evicted(), 1);
}

#[test]
fn pdu_over_cap_is_truncated() {
    let mut t = StreamTable::new();
    // Header claims 1 MiB + 1 payload bytes.
    let mut hdr = vec![2, 0xFD, 0x80, 0x01];
    hdr.extend((1_048_577u32).to_be_bytes());
    hdr.extend([0u8; 64]);
    let d = dec(
        &mut t,
        1,
        &tcp4(
            true,
            1,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &hdr,
        ),
    );
    assert!(has_warning(&d, "reassembly_truncated"), "{:?}", d.warnings);
    assert_eq!(t.buffered_bytes(), 0);
    // Later segments of that direction are not app-decoded, but the packet still decodes.
    let d = dec(
        &mut t,
        2,
        &tcp4(
            true,
            1 + hdr.len() as u32,
            Flags {
                ack: Some(1),
                ..Flags::default()
            },
            &routing_req(),
        ),
    );
    assert!(!d.has_protocol("doip"));
    assert!(d.has_protocol("tcp"));
}

#[test]
fn out_of_order_bytes_over_4mib_are_truncated() {
    let mut t = StreamTable::new();
    let fl = Flags {
        ack: Some(1),
        ..Flags::default()
    };
    // Start the stream with a partial header so the direction is in app mode.
    dec(&mut t, 1, &tcp4(true, 1, fl, &[2, 0xFD]));
    let chunk = vec![0xAAu8; 60_000];
    let mut truncated = false;
    for i in 0..200u32 {
        // Each segment is ahead of the hole at seq 3 and adjacent to the previous one.
        let seq = 1000 + i * 60_000;
        let d = dec(&mut t, 2 + u64::from(i), &tcp4(true, seq, fl, &chunk));
        assert!(
            t.buffered_bytes() <= MAX_STREAM_BUFFERED_BYTES,
            "buffered {}",
            t.buffered_bytes()
        );
        if has_warning(&d, "reassembly_truncated") {
            truncated = true;
            break;
        }
    }
    assert!(truncated, "cap never hit");
    assert_eq!(t.buffered_bytes(), 0);
}

#[test]
fn many_layers_and_fields_stay_bounded() {
    // 64 SD entries + junk options: fields stay under MAX_FIELDS, warnings under MAX_WARNINGS.
    let mut sd = vec![0xC0, 0, 0, 0];
    sd.extend((64u32 * 16 + 16).to_be_bytes());
    for i in 0..65u16 {
        sd.extend([0x01, 0, 0, 0, 0x12, 0x34]);
        sd.extend(i.to_be_bytes());
        sd.extend([1, 0, 0, 3, 0, 0, 0, 1]);
    }
    sd.extend(0u32.to_be_bytes());
    let mut p = vec![0xFF, 0xFF, 0x81, 0x00];
    p.extend((8 + u32::try_from(sd.len()).expect("len")).to_be_bytes());
    p.extend([0, 0, 0, 1, 1, 1, 0x02, 0]);
    p.extend(sd);
    let mut t = StreamTable::new();
    let d = dec(&mut t, 1, &udp4(TESTER, ECU, 30490, 30490, &p));
    let n: usize = d.layers.iter().map(|l| l.fields.len()).sum();
    assert!(n <= MAX_FIELDS);
    assert!(d.warnings.len() <= MAX_WARNINGS);
    assert!(has_warning(&d, "limit_fields"));
}

#[test]
fn huge_frame_is_cut_not_panicking() {
    let mut t = StreamTable::new();
    let big = vec![0u8; loom_eth_decode::MAX_FRAME_BYTES + 10];
    let d = decode(linktype::ETHERNET, &big, &FrameMeta::default(), &mut t);
    assert!(has_warning(&d, "frame_too_large"));
    assert_eq!(d.frame().len(), loom_eth_decode::MAX_FRAME_BYTES);
}
