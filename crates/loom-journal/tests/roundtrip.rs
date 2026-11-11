//! Round trip, rotation, watermarks, index rebuild, scan limits, opaque codec, throughput smoke.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::{Duration, Instant};

use common::{EPOCH, PRODUCER, append_all, config, record, wait_for};
use loom_journal::index::read_index;
use loom_journal::segment::{compute_index, index_path, list_segments};
use loom_journal::{
    Codec, IndexPolicy, JournalWriter, ScanLimits, ScanQuery, ScanRange, producer_dir, rebuild_index, scan,
};

fn all(range: ScanRange) -> ScanQuery {
    ScanQuery {
        range,
        limits: ScanLimits::unlimited(),
        resume: None,
    }
}

#[test]
fn roundtrip_10k_json_records() {
    let tmp = tempfile::tempdir().unwrap();
    let w = JournalWriter::spawn(config(tmp.path())).unwrap();
    let h = w.handle();
    append_all(&h, 0..10_000);
    let rep = w.close();
    assert!(rep.gaps.is_empty(), "{:?}", rep.gaps);
    assert_eq!(rep.status.written_through, Some(9_999));
    assert_eq!(rep.status.durable_through, Some(9_999));

    let dir = producer_dir(tmp.path(), PRODUCER, EPOCH);
    let segs = list_segments(&dir).unwrap();
    assert!(segs.iter().all(|s| s.sealed), "all sealed after close");
    let r = scan(
        &dir,
        &all(ScanRange::SourceSeq {
            from: 0,
            to: u64::MAX,
        }),
    )
    .unwrap();
    assert!(!r.truncated);
    assert_eq!(r.records.len(), 10_000);
    for (i, rec) in r.records.iter().enumerate() {
        assert_eq!(rec.source_seq, i as u64);
        assert_eq!(rec.decode_source_record().unwrap(), record(i as u64));
    }
}

#[test]
fn rotation_by_size_and_age() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = config(tmp.path());
    c.rotate_bytes = 64 * 1024;
    let w = JournalWriter::spawn(c).unwrap();
    append_all(&w.handle(), 0..3_000);
    let dir = w.dir().to_path_buf();
    w.close();
    let segs = list_segments(&dir).unwrap();
    assert!(segs.len() >= 10, "size rotation made {} segments", segs.len());
    for s in &segs {
        assert!(s.sealed);
        assert!(s.len <= 64 * 1024, "segment {} is {} bytes", s.seg_no, s.len);
    }
    let r = scan(&dir, &all(ScanRange::SourceSeq { from: 1234, to: 2345 })).unwrap();
    let seqs: Vec<u64> = r.records.iter().map(|r| r.source_seq).collect();
    assert_eq!(seqs, (1234..=2345).collect::<Vec<_>>());

    // Age rotation.
    let tmp2 = tempfile::tempdir().unwrap();
    let mut c = config(tmp2.path());
    c.rotate_age = Duration::from_millis(30);
    let w = JournalWriter::spawn(c).unwrap();
    let h = w.handle();
    append_all(&h, 0..10);
    let dir = w.dir().to_path_buf();
    wait_for(Duration::from_secs(5), || {
        list_segments(&dir).unwrap().iter().any(|s| s.sealed)
    });
    append_all(&h, 10..20);
    w.close();
    let segs = list_segments(&dir).unwrap();
    assert!(segs.len() >= 2);
    let r = scan(&dir, &all(ScanRange::SourceSeq { from: 0, to: 19 })).unwrap();
    assert_eq!(r.records.len(), 20);
}

#[test]
fn group_sync_watermarks_advance() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = config(tmp.path());
    c.sync_interval = Duration::from_millis(300);
    let w = JournalWriter::spawn(c).unwrap();
    let h = w.handle();
    append_all(&h, 0..100);
    wait_for(Duration::from_secs(5), || h.written_through() == Some(99));
    // Written to the OS long before the first group sync (300 ms after spawn).
    let d = h.durable_through();
    assert!(d.is_none_or(|d| d < 99), "durable {d:?} before group sync");
    wait_for(Duration::from_secs(5), || h.durable_through() == Some(99));
    let s = h.status();
    assert_eq!(s.to_proto().written_through.map(|v| v.0), Some(99));
    assert_eq!(s.to_proto().durable_through.map(|v| v.0), Some(99));

    // The active segment's written part is readable before seal.
    let r = scan(w.dir(), &all(ScanRange::SourceSeq { from: 50, to: 99 })).unwrap();
    assert_eq!(r.records.len(), 50);
    append_all(&h, 100..150);
    let st = w.flush(Duration::from_secs(5)).unwrap();
    assert_eq!(st.durable_through, Some(149));
    w.close();
}

#[test]
fn index_rebuild_matches_writer_json_and_opaque() {
    for codec in [Codec::Json, Codec::PcapEpb] {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = config(tmp.path());
        c.codec = codec;
        c.rotate_bytes = 256 * 1024;
        c.index = IndexPolicy {
            every_records: 100,
            every_mono_ns: 50_000_000,
        };
        let policy = c.index;
        let w = JournalWriter::spawn(c).unwrap();
        let h = w.handle();
        for s in 0..5_000u64 {
            match codec {
                Codec::Json => h.try_append(&record(s)).unwrap(),
                _ => {
                    // Opaque payload, seq gaps every 700, irregular mono steps.
                    let seq = s + s / 700 * 3;
                    let mono = s * 1_000_000 + (s % 13) * 7;
                    h.try_append_raw(seq, mono, vec![(s % 251) as u8; 40 + (s % 60) as usize])
                        .unwrap()
                }
            }
        }
        let dir = w.dir().to_path_buf();
        w.close();
        let segs = list_segments(&dir).unwrap();
        assert!(segs.len() > 1);
        let mut points = 0;
        for s in &segs {
            let ip = index_path(&dir, s.seg_no);
            let written = read_index(&ip).unwrap();
            assert!(!written.is_empty());
            points += written.len();
            assert_eq!(
                compute_index(&s.path, &policy).unwrap(),
                written,
                "{codec:?} seg {}",
                s.seg_no
            );
            std::fs::remove_file(&ip).unwrap();
            assert_eq!(rebuild_index(&s.path, &policy).unwrap(), written);
            assert_eq!(read_index(&ip).unwrap(), written);
        }
        assert!(points >= 50, "{codec:?}: {points} index points");

        if codec == Codec::PcapEpb {
            // Derived seqs across hints and gaps are exact.
            let r = scan(&dir, &all(ScanRange::SourceSeq { from: 690, to: 2_120 })).unwrap();
            let expect: Vec<u64> = (0..5_000u64)
                .map(|s| s + s / 700 * 3)
                .filter(|q| (690..=2_120).contains(q))
                .collect();
            let got: Vec<u64> = r.records.iter().map(|r| r.source_seq).collect();
            assert_eq!(got, expect);
            let first = &r.records[0];
            assert_eq!(first.payload.len(), 40 + (690 % 60) as usize);
        }
    }
}

#[test]
fn scan_limits_truncate_and_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c = config(tmp.path());
    c.rotate_bytes = 128 * 1024;
    c.index.every_records = 64;
    let w = JournalWriter::spawn(c).unwrap();
    append_all(&w.handle(), 0..4_000);
    let dir = w.dir().to_path_buf();
    w.close();

    // Result-byte limit: page through the whole range.
    let range = ScanRange::SourceSeq {
        from: 1_000,
        to: 3_500,
    };
    let mut q = ScanQuery {
        range,
        limits: ScanLimits {
            scan_bytes: u64::MAX,
            result_bytes: 16 * 1024,
            deadline: None,
        },
        resume: None,
    };
    let mut got = Vec::new();
    let mut pages = 0;
    loop {
        let r = scan(&dir, &q).unwrap();
        pages += 1;
        got.extend(r.records.iter().map(|r| r.source_seq));
        if !r.truncated {
            break;
        }
        assert!(r.records.iter().map(|r| r.payload.len()).sum::<usize>() <= 16 * 1024);
        q.resume = r.resume_from;
        assert!(q.resume.is_some());
    }
    assert!(pages > 5);
    assert_eq!(got, (1_000..=3_500).collect::<Vec<_>>());

    // Scanned-byte limit.
    let r = scan(
        &dir,
        &ScanQuery {
            range,
            limits: ScanLimits {
                scan_bytes: 50_000,
                result_bytes: u64::MAX,
                deadline: None,
            },
            resume: None,
        },
    )
    .unwrap();
    assert!(r.truncated);
    assert!(r.scanned_bytes >= 50_000 && r.scanned_bytes < 60_000);

    // Deadline already passed: nothing read, resume points at the start.
    let r = scan(
        &dir,
        &ScanQuery {
            range,
            limits: ScanLimits {
                scan_bytes: u64::MAX,
                result_bytes: u64::MAX,
                deadline: Some(Instant::now()),
            },
            resume: None,
        },
    )
    .unwrap();
    assert!(r.truncated && r.records.is_empty());
    let r2 = scan(
        &dir,
        &ScanQuery {
            range,
            limits: ScanLimits::unlimited(),
            resume: r.resume_from,
        },
    )
    .unwrap();
    assert_eq!(r2.records.len(), 2_501);

    // Mono range (mono = seq ms).
    let r = scan(
        &dir,
        &all(ScanRange::Mono {
            from: 2_000_000_000,
            to: 2_100_000_000,
        }),
    )
    .unwrap();
    let seqs: Vec<u64> = r.records.iter().map(|r| r.source_seq).collect();
    assert_eq!(seqs, (2_000..=2_100).collect::<Vec<_>>());
}

#[test]
fn throughput_smoke_100k() {
    let tmp = tempfile::tempdir().unwrap();
    let w = JournalWriter::spawn(config(tmp.path())).unwrap();
    let h = w.handle();
    let recs: Vec<_> = (0..100_000u64).map(record).collect();
    let t0 = Instant::now();
    let mut full = 0u64;
    for r in &recs {
        if h.try_append(r).is_err() {
            full += 1;
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    let appended = t0.elapsed();
    let st = w.flush(Duration::from_secs(60)).unwrap();
    let durable = t0.elapsed();
    let bytes: u64 = list_segments(w.dir()).unwrap().iter().map(|s| s.len).sum();
    w.close();
    let rate = 100_000.0 / durable.as_secs_f64();
    println!(
        "throughput_smoke: 100k records, append {:?}, durable {:?} ({:.0} rec/s, {:.1} MB/s), \
         {} bytes on disk ({:.0} B/rec), full={}, durable_through={:?}",
        appended,
        durable,
        rate,
        bytes as f64 / durable.as_secs_f64() / 1e6,
        bytes,
        bytes as f64 / 100_000.0,
        full,
        st.durable_through
    );
}
