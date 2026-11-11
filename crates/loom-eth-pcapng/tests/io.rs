use loom_eth_pcapng::*;
use std::io::Cursor;
fn interface(res: TimestampResolution) -> Interface {
    Interface {
        linktype: 1,
        snaplen: 65535,
        name: "eth-test".into(),
        tsresol: res,
    }
}
fn packet(n: usize, ts: u128) -> Packet {
    Packet {
        ts_ns: ts,
        caplen: n as u32,
        origlen: n as u32,
        data: vec![0xab; n],
        ..Packet::default()
    }
}
fn file(p: &Packet, res: TimestampResolution) -> Vec<u8> {
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface(res)).expect("IDB");
    w.write_packet(p).expect("EPB");
    w.into_inner()
}
#[test]
fn nanosecond_microsecond_multi_interface_comments_truncation_roundtrip() {
    for res in [TimestampResolution::MICRO, TimestampResolution::NANO] {
        for n in [0, 1, 3, 4, 64, 1500, 65_535] {
            let mut p = packet(n, 1_700_000_000_123_456_000);
            p.interface_id = 1;
            p.origlen += 12;
            p.comments = vec!["user note 한글".into(), "loom1 this is a user comment".into()];
            p.meta = Some(LoomMeta {
                seq: 42,
                dir: Direction::Tx,
                req: Some("t-000418".into()),
                org: Origin::Agent,
            });
            let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
            w.write_interface(&interface(TimestampResolution::NANO))
                .expect("IDB0");
            w.write_interface(&interface(res)).expect("IDB1");
            w.write_packet(&p).expect("EPB");
            let bytes = w.into_inner();
            let mut r = PcapNgReader::new(Cursor::new(&bytes));
            assert_eq!(r.next_packet().expect("read"), Some(p));
            assert_eq!(r.interfaces().len(), 2);
            assert!(r.next_packet().expect("EOF").is_none());
            assert_eq!(r.last_good_offset(), bytes.len() as u64);
        }
    }
}
#[test]
fn nano_wire_ticks_are_not_self_confirming_roundtrip() {
    let bytes = file(&packet(1, 1_700_000_000_123_456_789), TimestampResolution::NANO);
    let mut off = 0;
    while off < bytes.len() {
        let n = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().expect("len")) as usize;
        let kind = u32::from_le_bytes(bytes[off..off + 4].try_into().expect("kind"));
        if kind == 1 {
            assert!(
                bytes[off + 16..off + n - 4]
                    .windows(5)
                    .any(|w| w == [9, 0, 1, 0, 9])
            );
        }
        if kind == 6 {
            let hi = u32::from_le_bytes(bytes[off + 12..off + 16].try_into().expect("hi"));
            let lo = u32::from_le_bytes(bytes[off + 16..off + 20].try_into().expect("lo"));
            assert_eq!((u64::from(hi) << 32) | u64::from(lo), 1_700_000_000_123_456_789);
        }
        off += n;
    }
}
#[test]
fn metadata_validation_and_user_comment_preservation() {
    for dir in [Direction::Rx, Direction::Tx] {
        for org in [Origin::Agent, Origin::User, Origin::Script] {
            let m = LoomMeta {
                seq: u64::MAX,
                dir,
                req: None,
                org,
            };
            assert_eq!(LoomMeta::from_comment(&m.to_comment().expect("comment")), Some(m));
        }
    }
    for s in [
        "loom1 seq=1 dir=bad org=agent",
        "loom1 seq=1 seq=2 dir=rx org=user",
        "loom2 seq=1 dir=rx org=agent",
        "loom1 seq=1 dir=rx org=agent req=",
        "loom1 seq=1 dir=rx org=agent req=a\tb",
    ] {
        assert!(LoomMeta::from_comment(s).is_none());
    }
    let bad = LoomMeta {
        seq: 0,
        dir: Direction::Tx,
        req: Some("has space".into()),
        org: Origin::Agent,
    };
    assert!(bad.to_comment().is_err());
    let mut p = packet(1, 0);
    p.comments = vec!["loom1 seq=garbage".into()];
    let mut r = PcapNgReader::new(Cursor::new(file(&p, TimestampResolution::NANO)));
    assert_eq!(r.next_packet().expect("read"), Some(p));
}
#[test]
fn nanosecond_codec_export_copies_verbatim_and_serializes_header() {
    let header = SessionHeader {
        interfaces: vec![
            interface(TimestampResolution::MICRO),
            interface(TimestampResolution::NANO),
        ],
    };
    let json = serde_json::to_string(&header).expect("header serialize");
    assert!(json.contains("eth-test"));
    let mut p = packet(3, 1_700_000_000_123_456_789);
    p.interface_id = 1;
    let records = [
        encode_record(&packet(64, 1)).expect("encode0"),
        encode_record(&p).expect("encode1"),
    ];
    assert_eq!(decode_record(&records[1]).expect("decode"), p);
    let mut bytes = Vec::new();
    export(&header, records.iter().map(Vec::as_slice), &mut bytes).expect("export");
    let suffix: Vec<_> = records.iter().flatten().copied().collect();
    assert!(bytes.ends_with(&suffix));
    let mut r = PcapNgReader::new(Cursor::new(bytes));
    assert_eq!(r.next_packet().expect("p0"), Some(packet(64, 1)));
    assert_eq!(r.next_packet().expect("p1"), Some(p));
    assert!(r.next_packet().expect("EOF").is_none());
    assert!(
        r.interfaces()
            .iter()
            .all(|i| i.tsresol == TimestampResolution::NANO)
    );
}
fn put16(v: &mut Vec<u8>, n: u16, be: bool) {
    v.extend(if be { n.to_be_bytes() } else { n.to_le_bytes() });
}
fn put32(v: &mut Vec<u8>, n: u32, be: bool) {
    v.extend(if be { n.to_be_bytes() } else { n.to_le_bytes() });
}
fn block(kind: u32, body: &[u8], be: bool) -> Vec<u8> {
    let mut v = Vec::new();
    put32(&mut v, kind, be);
    put32(&mut v, (body.len() + 12) as u32, be);
    v.extend(body);
    put32(&mut v, (body.len() + 12) as u32, be);
    v
}
fn manual_ng(be: bool, res: u8, ticks: u64) -> Vec<u8> {
    let mut sh = Vec::new();
    put32(&mut sh, 0x1a2b3c4d, be);
    put16(&mut sh, 1, be);
    put16(&mut sh, 0, be);
    sh.extend([255; 8]);
    let mut out = block(0x0a0d0d0a, &sh, be);
    let mut id = Vec::new();
    put16(&mut id, 1, be);
    put16(&mut id, 0, be);
    put32(&mut id, 65535, be);
    put16(&mut id, 9, be);
    put16(&mut id, 1, be);
    id.extend([res, 0, 0, 0]);
    id.extend([0; 4]);
    out.extend(block(1, &id, be));
    out.extend(block(0xdeadbeef, &[1, 2, 3, 4], be));
    let mut ep = Vec::new();
    for n in [0, (ticks >> 32) as u32, ticks as u32, 3, 7] {
        put32(&mut ep, n, be);
    }
    ep.extend([1, 2, 3, 0]);
    put16(&mut ep, 1, be);
    put16(&mut ep, 4, be);
    ep.extend(b"note");
    ep.extend([0; 4]);
    out.extend(block(6, &ep, be));
    out
}
#[test]
fn handwritten_big_endian_binary_resolution_unknown_block() {
    let mut r = PcapNgReader::new(Cursor::new(manual_ng(true, 0x80 | 10, 1536)));
    let p = r.next_packet().expect("BE binary").expect("packet");
    assert_eq!(p.ts_ns, 1_500_000_000);
    assert_eq!(p.data, [1, 2, 3]);
    assert_eq!(p.origlen, 7);
    assert_eq!(p.comments, ["note"]);
}
#[test]
fn decimal_binary_extremes_and_precision_rejection() {
    for (res, ticks, want) in [
        (0, 2, 2_000_000_000u128),
        (6, 123, 123_000),
        (9, 123, 123),
        (10, 123, 12),
        (127, u64::MAX, 0),
        (0x80 | 127, u64::MAX, 0),
    ] {
        let mut r = PcapNgReader::new(Cursor::new(manual_ng(false, res, ticks)));
        assert_eq!(r.next_packet().expect("read").expect("p").ts_ns, want);
    }
    let bytes = manual_ng(false, 6, u64::MAX);
    let mut r = PcapNgReader::new(Cursor::new(bytes));
    assert_eq!(
        r.next_packet().expect("read").expect("p").ts_ns,
        u128::from(u64::MAX) * 1000
    );
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface(TimestampResolution::MICRO))
        .expect("IDB");
    assert!(w.write_packet(&packet(1, 1)).is_err());
    assert!(encode_record(&packet(1, u128::from(u64::MAX) + 1)).is_err());
}
#[test]
fn mixed_endian_sections_reset_interface_ids() {
    let mut bytes = manual_ng(true, 9, 11);
    bytes.extend(manual_ng(false, 6, 22));
    let mut r = PcapNgReader::new(Cursor::new(bytes));
    let p0 = r.next_packet().expect("read0").expect("p0");
    let p1 = r.next_packet().expect("read1").expect("p1");
    assert_eq!((p0.section_index, p0.ts_ns), (0, 11));
    assert_eq!((p1.section_index, p1.ts_ns), (1, 22_000));
    assert_eq!(r.interfaces().len(), 1);
}
#[test]
fn isb_drop_stat_and_new_section_writer() {
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface(TimestampResolution::NANO))
        .expect("IDB");
    let stats = InterfaceStatistics {
        interface_id: 0,
        ts_ns: 123,
        if_drop: Some(45),
    };
    w.write_statistics(&stats).expect("ISB");
    w.start_section().expect("SHB2");
    assert!(w.write_packet(&packet(1, 0)).is_err());
    let mut r = PcapNgReader::new(Cursor::new(w.into_inner()));
    let mut found = false;
    while let Some(b) = r.next_block().expect("read") {
        if let Block::Statistics(s) = b {
            assert_eq!(s, stats);
            found = true;
        }
    }
    assert!(found);
}
#[test]
fn classic_pcap_all_magic_variants() {
    for be in [false, true] {
        for nano in [false, true] {
            let mut v = Vec::new();
            put32(&mut v, if nano { 0xa1b23c4d } else { 0xa1b2c3d4 }, be);
            put16(&mut v, 2, be);
            put16(&mut v, 4, be);
            for n in [0, 0, 64, 1, 123, 456, 3, 7] {
                put32(&mut v, n, be);
            }
            v.extend([1, 2, 3]);
            let mut r = PcapNgReader::new(Cursor::new(v));
            let p = r.next_packet().expect("pcap").expect("p");
            assert_eq!(p.ts_ns, 123_000_000_000 + if nano { 456 } else { 456000 });
            assert_eq!(p.data, [1, 2, 3]);
            assert!(r.next_packet().expect("EOF").is_none());
        }
    }
}
#[test]
fn r1_sample_is_read_without_reinterpreting_its_timestamp_units() {
    let mut r = PcapNgReader::new(Cursor::new(include_bytes!("fixtures/r1-sol.pcapng")));
    let mut count = 0;
    while let Some(p) = r.next_packet().expect("sample") {
        if count == 0 {
            assert_eq!(p.ts_ns, 1_700_000_000_000_000_000_000);
        }
        count += 1;
    }
    assert_eq!(count, 9);
}
#[test]
fn truncated_files_report_recoverable_prefix_and_fuse() {
    let bytes = file(&packet(64, 123), TimestampResolution::NANO);
    for cut in 0..bytes.len() {
        let mut r = PcapNgReader::new(Cursor::new(&bytes[..cut]));
        let mut failed = false;
        loop {
            match r.next_block() {
                Ok(Some(_)) => (),
                Ok(None) => break,
                Err(e) => {
                    assert_eq!(e.last_good_offset, r.last_good_offset());
                    assert!(e.last_good_offset <= cut as u64);
                    assert!(r.next_block().expect("fused").is_none());
                    failed = true;
                    break;
                }
            }
        }
        // EOF exactly on a complete block boundary is a valid shorter file.
        if !failed {
            assert_eq!(r.last_good_offset(), cut as u64);
        }
    }
}
#[test]
fn allocation_ceiling_and_idb_cap_are_enforced() {
    let mut bytes = manual_ng(false, 9, 1);
    bytes.truncate(28);
    bytes.extend(6u32.to_le_bytes());
    bytes.extend(u32::MAX.to_le_bytes());
    let mut r = PcapNgReader::new(Cursor::new(bytes));
    r.next_block().expect("SHB");
    let e = r.next_block().expect_err("oversized");
    assert!(matches!(e.kind, ErrorKind::Limit(_)));
    assert_eq!(e.offset, 36);
    assert_eq!(e.last_good_offset, 28);
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    for _ in 0..MAX_INTERFACES {
        w.write_interface(&interface(TimestampResolution::NANO))
            .expect("IDB");
    }
    assert!(w.write_interface(&interface(TimestampResolution::NANO)).is_err());
    let mut all = w.into_inner();
    let extra = all[28..].get(..44).expect("IDB prefix").to_vec(); // use an actual full IDB below
    let len = u32::from_le_bytes(all[32..36].try_into().expect("IDB length")) as usize;
    let extra = if extra.len() == len {
        extra
    } else {
        all[28..28 + len].to_vec()
    };
    all.extend(extra);
    let mut r = PcapNgReader::new(Cursor::new(all));
    let mut got = 0;
    loop {
        match r.next_block() {
            Ok(Some(Block::Interface { .. })) => got += 1,
            Ok(Some(_)) => (),
            Err(e) => {
                assert!(matches!(e.kind, ErrorKind::Limit(_)));
                break;
            }
            Ok(None) => panic!("missing IDB cap error"),
        }
    }
    assert_eq!(got, MAX_INTERFACES);
}
#[test]
fn bad_lengths_interfaces_comments_and_metadata_are_rejected() {
    let mut p = packet(2, 0);
    p.caplen = 3;
    assert!(encode_record(&p).is_err());
    p = packet(2, 0);
    p.origlen = 1;
    assert!(encode_record(&p).is_err());
    p = packet(2, 0);
    p.comments = vec!["x".repeat(MAX_COMMENT_BYTES + 1)];
    assert!(encode_record(&p).is_err());
    let mut bytes = file(&packet(2, 0), TimestampResolution::NANO);
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    assert!(PcapNgReader::new(Cursor::new(bytes)).next_packet().is_err());
    let header = SessionHeader {
        interfaces: vec![interface(TimestampResolution::NANO)],
    };
    let mut p = packet(1, 0);
    p.interface_id = 2;
    let record = encode_record(&p).expect("encode id is session-local");
    assert!(export(&header, std::iter::once(record.as_slice()), Vec::new()).is_err());
}
#[test]
fn random_bytes_100000_no_panics_in_reader_record_and_metadata() {
    let mut seed = 0xc0ffee1234567890u64;
    for i in 0..100_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let n = (seed as usize) % 192;
        let mut data = vec![0; n];
        for b in &mut data {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *b = seed as u8;
        }
        if i % 3 == 0 {
            data = manual_ng(i % 2 == 0, 9, seed);
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let pos = seed as usize % data.len();
                data[pos] ^= (seed >> 32) as u8;
            }
        } else if i % 2 == 0 {
            let mut prefix = manual_ng(i % 4 == 0, 9, 0);
            prefix.truncate(28);
            prefix.extend(&data);
            data = prefix;
        }
        let mut reader = PcapNgReader::new(Cursor::new(&data));
        while matches!(reader.next_block(), Ok(Some(_))) {}
        let _ = decode_record(&data);
        if let Ok(s) = std::str::from_utf8(&data) {
            let _ = LoomMeta::from_comment(s);
        }
    }
}
