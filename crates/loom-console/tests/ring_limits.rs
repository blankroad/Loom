//! Backlog ring: limits, eviction, offsets and queries (plan §3.3, §3.6, §3.13 backlog 행).

use loom_console::ring::{
    Chunk, DEFAULT_MAX_ENTRIES, DEFAULT_MAX_META, DEFAULT_MAX_PAYLOAD, Entry, GapLayer, GapMark,
    MAX_META_FIELD_BYTES, Ring, RingError, RingLimits, RxItem,
};
use loom_proto::event::{Dir, Origin};

fn rx(seq: u64, off: u64, bytes: &[u8]) -> Entry {
    rx_gen(seq, off, 0, bytes)
}

fn rx_gen(seq: u64, off: u64, generation: u32, bytes: &[u8]) -> Entry {
    Entry::Data(Chunk {
        source_seq: seq,
        dir: Dir::Rx,
        origin: Origin::Device,
        client_id: None,
        request_id: None,
        generation,
        mono_ns: seq * 1000,
        wall_ns: seq * 1000,
        rx_offset: Some(off),
        bytes: bytes.to_vec(),
    })
}

fn tx(seq: u64, client: &str, bytes: &[u8]) -> Entry {
    Entry::Data(Chunk {
        source_seq: seq,
        dir: Dir::Tx,
        origin: Origin::Agent,
        client_id: Some(client.to_owned()),
        request_id: Some(format!("req-{seq}")),
        generation: 0,
        mono_ns: seq * 1000,
        wall_ns: seq * 1000,
        rx_offset: None,
        bytes: bytes.to_vec(),
    })
}

fn gap(cause: &str) -> Entry {
    Entry::Gap(GapMark {
        layer: GapLayer::Record,
        cause: cause.to_owned(),
        rx_offset_from: None,
        bytes: None,
        source_seq_range: None,
        generation: 0,
        mono_ns: 0,
    })
}

fn small(max_payload: usize, max_entries: usize, max_meta: usize) -> RingLimits {
    RingLimits {
        max_payload,
        max_entries,
        max_meta,
    }
}

#[test]
fn defaults_match_plan() {
    let l = RingLimits::default();
    assert_eq!(l.max_payload, 16 << 20);
    assert_eq!(l.max_entries, 32_768);
    assert_eq!(l.max_meta, 4 << 20);
    assert_eq!(
        (DEFAULT_MAX_PAYLOAD, DEFAULT_MAX_ENTRIES, DEFAULT_MAX_META),
        (16 << 20, 32_768, 4 << 20)
    );
}

#[test]
fn byte_limit_evicts_oldest_without_gap() {
    let mut r = Ring::new(small(1000, 1000, 1 << 20));
    for i in 0..20u64 {
        r.push(rx(i, i * 100, &[b'a'; 100])).expect("push");
    }
    let s = r.stats();
    assert!(s.payload_bytes <= 1000);
    assert_eq!(s.entries, 10);
    assert_eq!(s.evicted_entries, 10);
    assert_eq!(s.evicted_bytes, 1000);
    assert_eq!(s.retained_from_rx_offset, 1000);
    assert_eq!(s.retained_from_source_seq, 10);
    assert_eq!(s.rx_end, 2000);
    assert_eq!(s.oldest_mono_ns, Some(10_000));
    // Eviction is retention expiry, never a gap mark.
    assert!(r.entries().all(|e| matches!(e, Entry::Data(_))));
}

#[test]
fn entry_limit_evicts_oldest() {
    let mut r = Ring::new(small(1 << 20, 8, 1 << 20));
    for i in 0..100u64 {
        let out = r.push(rx(i, i, b"x")).expect("push");
        if i >= 8 {
            assert_eq!(out.evicted_entries, 1);
        }
    }
    assert_eq!(r.len(), 8);
    assert_eq!(r.stats().retained_from_rx_offset, 92);
}

#[test]
fn meta_limit_counts_strings() {
    let mut r = Ring::new(small(1 << 20, 1000, 1000));
    let client = "c".repeat(90);
    for i in 0..100u64 {
        r.push(tx(i, &client, b"ls\r")).expect("push");
    }
    let s = r.stats();
    assert!(s.meta_bytes <= 1000, "meta {}", s.meta_bytes);
    assert!((5..=10).contains(&s.entries), "entries {}", s.entries);
    assert_eq!(s.evicted_entries + s.entries as u64, 100);
}

#[test]
fn overlong_meta_is_rejected_not_truncated() {
    let mut r = Ring::new(RingLimits::default());
    let long = "x".repeat(MAX_META_FIELD_BYTES + 1);
    let err = r.push(tx(1, &long, b"a")).expect_err("must reject");
    assert!(matches!(
        err,
        RingError::MetaFieldTooLong {
            field: "client_id",
            ..
        }
    ));
    assert!(r.is_empty());
    assert!(matches!(
        r.push(gap(&long)),
        Err(RingError::MetaFieldTooLong { field: "cause", .. })
    ));
    r.push(tx(1, &"x".repeat(MAX_META_FIELD_BYTES), b"a"))
        .expect("128 B is fine");
}

#[test]
fn caller_bugs_are_rejected() {
    let mut r = Ring::new(small(100, 10, 1000));
    assert_eq!(r.push(rx(1, 0, b"")), Err(RingError::EmptyChunk));
    assert!(matches!(
        r.push(rx(1, 0, &[0; 101])),
        Err(RingError::ChunkTooLarge { .. })
    ));
    r.push(rx(5, 0, b"abc")).expect("push");
    assert!(matches!(
        r.push(rx(5, 3, b"d")),
        Err(RingError::NonMonotonicSeq { .. })
    ));
    assert!(matches!(
        r.push(rx(6, 2, b"d")),
        Err(RingError::NonMonotonicRxOffset { .. })
    ));
    let mut bad_tx = tx(7, "c", b"x");
    if let Entry::Data(c) = &mut bad_tx {
        c.rx_offset = Some(3);
    }
    assert_eq!(r.push(bad_tx), Err(RingError::RxOffsetMismatch));
    let mut bad_rx = rx(7, 3, b"x");
    if let Entry::Data(c) = &mut bad_rx {
        c.rx_offset = None;
    }
    assert_eq!(r.push(bad_rx), Err(RingError::RxOffsetMismatch));
    assert_eq!(r.len(), 1);
}

#[test]
fn offsets_continuous_after_wrap_and_across_generations() {
    let mut r = Ring::new(small(4096, 64, 1 << 20));
    let mut off = 0u64;
    for i in 0..1000u64 {
        let len = (i % 37 + 1) as usize;
        r.push(rx_gen(i, off, (i / 300) as u32, &vec![b'z'; len]))
            .expect("push");
        off += len as u64;
    }
    let from = r.retained_from_rx_offset();
    let mut expect = from;
    let mut gens = Vec::new();
    for s in r.iter_from_rx_offset(from) {
        assert_eq!(s.rx_offset(), Some(expect));
        expect += s.bytes().len() as u64;
        gens.push(s.chunk.generation);
    }
    assert_eq!(expect, r.rx_end());
    assert_eq!(r.rx_end(), off);
    assert!(gens.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn partial_chunk_query_and_gaps_in_rx_order() {
    let mut r = Ring::new(RingLimits::default());
    r.push(rx(1, 0, b"hello ")).expect("push");
    r.push(tx(2, "agent#1", b"ls\r")).expect("push");
    r.push(gap("reader_bridge_overflow")).expect("push");
    r.push(rx(3, 50, b"world")).expect("push");

    let v: Vec<_> = r.iter_from_rx_offset(3).collect();
    assert_eq!(v.len(), 2);
    assert_eq!((v[0].bytes(), v[0].rx_offset()), (&b"lo "[..], Some(3)));
    assert_eq!((v[1].bytes(), v[1].rx_offset()), (&b"world"[..], Some(50)));

    // The gap sits at RX position 6: visible from offsets ≤ 6, not from inside "world".
    let kinds = |off| {
        r.rx_items_from(off)
            .map(|i| match i {
                RxItem::Data(_) => 'd',
                RxItem::Gap(_) => 'g',
            })
            .collect::<String>()
    };
    assert_eq!(kinds(0), "dgd");
    assert_eq!(kinds(6), "gd");
    assert_eq!(kinds(51), "d");
    assert_eq!(kinds(55), "");
}

#[test]
fn source_seq_query_and_last_n_bytes() {
    let mut r = Ring::new(RingLimits::default());
    r.push(rx(1, 0, b"abc")).expect("push");
    r.push(gap("x")).expect("push");
    r.push(tx(4, "a#1", b"TX")).expect("push");
    r.push(rx(7, 3, b"defg")).expect("push");

    let seqs = |s| {
        r.iter_from_source_seq(s)
            .map(|e| match e {
                Entry::Data(c) => c.source_seq.to_string(),
                Entry::Gap(_) => "g".into(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(seqs(0), ["1", "g", "4", "7"]);
    assert_eq!(seqs(2), ["g", "4", "7"]);
    assert_eq!(seqs(5), ["7"]);
    assert_eq!(r.rx_offset_for_source_seq(2), 3);
    assert_eq!(r.rx_offset_for_source_seq(99), 7);

    let tail = r.last_n_bytes(5, false);
    let got: Vec<u8> = tail.iter().flat_map(|s| s.bytes().to_vec()).collect();
    assert_eq!(got, b"cdefg");
    assert_eq!(tail[0].rx_offset(), Some(2));
    let tail = r.last_n_bytes(5, true);
    let got: Vec<u8> = tail.iter().flat_map(|s| s.bytes().to_vec()).collect();
    assert_eq!(got, b"TXdefg"[1..].to_vec());
}

#[test]
fn one_million_pushes_stay_within_limits() {
    // Default limits: the entry cap binds first for 48-byte chunks.
    let mut r = Ring::new(RingLimits::default());
    let payload = [b'q'; 48];
    let mut off = 0u64;
    for i in 0..1_000_000u64 {
        let e = if i % 10 == 0 {
            tx(i, "claude-7f3a#4121", &payload[..8])
        } else {
            let e = rx(i, off, &payload);
            off += payload.len() as u64;
            e
        };
        r.push(e).expect("push");
        if i % 100_000 == 0 {
            check(&r);
        }
    }
    check(&r);
    let s = r.stats();
    assert_eq!(s.entries, DEFAULT_MAX_ENTRIES);

    // Byte-bound configuration with 16 KiB chunks.
    let mut r = Ring::new(small(1 << 20, DEFAULT_MAX_ENTRIES, 64 << 10));
    let big = vec![b'b'; 16 << 10];
    let mut off = 0u64;
    for i in 0..2_000u64 {
        r.push(rx(i, off, &big)).expect("push");
        off += big.len() as u64;
    }
    check(&r);
    assert_eq!(r.stats().entries, 64);

    fn check(r: &Ring) {
        let s = r.stats();
        assert!(s.entries <= s.limits.max_entries);
        assert!(s.payload_bytes <= s.limits.max_payload);
        assert!(s.meta_bytes <= s.limits.max_meta);
        assert!(
            s.slot_capacity <= 2 * s.limits.max_entries,
            "slots {}",
            s.slot_capacity
        );
    }
}
