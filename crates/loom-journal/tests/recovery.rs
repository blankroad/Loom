//! Crash-tail recovery and sealed-segment corruption.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use common::{EPOCH, PRODUCER, append_all, config};
use loom_journal::gap::causes;
use loom_journal::segment::{list_segments, quarantine_path, segment_file_name};
use loom_journal::{
    IndexPolicy, JournalWriter, ScanLimits, ScanQuery, ScanRange, producer_dir, recover_producer_dir, scan,
    verify_producer_dir, verify_segment,
};
use loom_proto::event::GapCount;

/// Write `n` records into one sealed segment and return (dir, sealed path).
fn one_segment(root: &Path, n: u64) -> (PathBuf, PathBuf) {
    let mut c = config(root);
    c.index.every_records = 100;
    let w = JournalWriter::spawn(c).unwrap();
    append_all(&w.handle(), 0..n);
    w.close();
    let dir = producer_dir(root, PRODUCER, EPOCH);
    let segs = list_segments(&dir).unwrap();
    assert_eq!(segs.len(), 1);
    (dir, segs[0].path.clone())
}

/// Turn a sealed segment back into an "active" one (as if the writer crashed).
fn unseal(dir: &Path, sealed: &Path) -> PathBuf {
    let open = dir.join(segment_file_name(1, false));
    fs::rename(sealed, &open).unwrap();
    open
}

fn all_seqs(dir: &Path) -> Vec<u64> {
    let r = scan(
        dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 0,
                to: u64::MAX,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    r.records.iter().map(|r| r.source_seq).collect()
}

/// Offset of the last frame (via a full scan).
fn last_frame_offset(dir: &Path) -> u64 {
    let r = scan(
        dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 0,
                to: u64::MAX,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    r.records.last().unwrap().offset
}

#[test]
fn crash_tail_garbage_is_cut_on_writer_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, sealed) = one_segment(tmp.path(), 1_000);
    let good_len = fs::metadata(&sealed).unwrap().len();
    let open = unseal(&dir, &sealed);
    let garbage = [0x2a_u8; 37];
    OpenOptions::new()
        .append(true)
        .open(&open)
        .unwrap()
        .write_all(&garbage)
        .unwrap();

    // Same producer epoch restarts: the writer recovers first.
    let w = JournalWriter::spawn(config(tmp.path())).unwrap();
    let rec = w.recovered().to_vec();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].truncated_bytes, 37);
    assert_eq!(rec[0].records, 1_000);
    assert_eq!(rec[0].last_seq, Some(999));
    let gap = rec[0].gap.clone().unwrap();
    assert_eq!(gap.cause, causes::CRASH_TAIL);
    assert_eq!(gap.from, 1_000);
    assert_eq!(gap.to, None);
    assert_eq!(gap.count, GapCount::Unknown);
    assert_eq!(gap.bytes, Some(37));
    assert_eq!(
        fs::metadata(&sealed).unwrap().len(),
        good_len,
        "re-sealed at the valid length"
    );
    assert!(!open.exists());
    let h = w.handle();
    assert_eq!(
        h.take_gaps(),
        vec![gap.clone()],
        "crash_tail gap is also pending on the handle"
    );
    let gd = gap.to_gap_data();
    assert_eq!(gd.cause, "crash_tail");
    assert!(gd.producer_range.is_none());

    // Seq order continues after the recovered tail.
    assert!(h.try_append(&common::record(999)).is_err());
    append_all(&h, 1_000..1_010);
    w.close();
    assert_eq!(all_seqs(&dir), (0..1_010).collect::<Vec<_>>());
}

#[test]
fn crash_tail_partial_frame_and_bad_crc() {
    // Partial last frame: cut 5 bytes off the end.
    let tmp = tempfile::tempdir().unwrap();
    let (dir, sealed) = one_segment(tmp.path(), 500);
    let last_off = last_frame_offset(&dir);
    let len = fs::metadata(&sealed).unwrap().len();
    let open = unseal(&dir, &sealed);
    OpenOptions::new()
        .write(true)
        .open(&open)
        .unwrap()
        .set_len(len - 5)
        .unwrap();
    let rep = recover_producer_dir(&dir, &IndexPolicy::default()).unwrap();
    assert_eq!(rep.len(), 1);
    assert_eq!(rep[0].truncated_bytes, len - 5 - last_off);
    assert_eq!(rep[0].last_seq, Some(498));
    assert_eq!(rep[0].gap.as_ref().unwrap().from, 499);
    assert_eq!(fs::metadata(&sealed).unwrap().len(), last_off);
    assert_eq!(all_seqs(&dir), (0..499).collect::<Vec<_>>());

    // Bad CRC in the last frame: the whole frame is cut.
    let tmp = tempfile::tempdir().unwrap();
    let (dir, sealed) = one_segment(tmp.path(), 500);
    let last_off = last_frame_offset(&dir);
    let len = fs::metadata(&sealed).unwrap().len();
    let mut bytes = fs::read(&sealed).unwrap();
    let i = bytes.len() - 3;
    bytes[i] ^= 0xff;
    let open = unseal(&dir, &sealed);
    fs::write(&open, &bytes).unwrap();
    let rep = recover_producer_dir(&dir, &IndexPolicy::default()).unwrap();
    assert_eq!(rep[0].truncated_bytes, len - last_off);
    assert_eq!(rep[0].records, 499);

    // Header-only file (crash right after create) → removed, nothing kept.
    let tmp = tempfile::tempdir().unwrap();
    let dir = producer_dir(tmp.path(), PRODUCER, EPOCH);
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join(segment_file_name(4, false));
    fs::write(&p, b"LOOMJ1").unwrap();
    let rep = recover_producer_dir(&dir, &IndexPolicy::default()).unwrap();
    assert_eq!(rep[0].truncated_bytes, 6);
    assert!(!p.exists());
}

#[test]
fn sealed_mid_corruption_is_reported_and_quarantined() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, sealed) = one_segment(tmp.path(), 3_000);
    let r = scan(
        &dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 1_500,
                to: 1_500,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    let bad_off = r.records[0].offset;
    let len = fs::metadata(&sealed).unwrap().len();
    let mut bytes = fs::read(&sealed).unwrap();
    bytes[(bad_off + 9 + 20) as usize] ^= 0x01;
    fs::write(&sealed, &bytes).unwrap();

    let v = verify_segment(&sealed).unwrap();
    let c = v.corruption.clone().unwrap();
    assert_eq!(c.offset, bad_off);
    assert_eq!(c.last_seq, Some(1_499));
    let g = c.gap();
    assert_eq!(g.cause, causes::CORRUPTION);
    assert_eq!(g.from, 1_500);
    assert_eq!(g.count, GapCount::Unknown);

    let vs = verify_producer_dir(&dir).unwrap();
    assert_eq!(vs.len(), 1);
    assert!(quarantine_path(&dir, 1).exists());
    assert_eq!(
        fs::metadata(&sealed).unwrap().len(),
        len,
        "sealed file is not truncated"
    );

    // Reads stop at the damage, even when the index would start past it.
    let full = scan(
        &dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 0,
                to: u64::MAX,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    assert_eq!(
        full.records.iter().map(|r| r.source_seq).collect::<Vec<_>>(),
        (0..1_500).collect::<Vec<_>>()
    );
    assert_eq!(full.corruptions.len(), 1);
    assert!(!full.truncated);
    let after = scan(
        &dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 2_000,
                to: 2_100,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    assert!(after.records.is_empty());
    assert_eq!(after.corruptions[0].reason, "quarantined");

    // Recovery never truncates sealed files.
    let rep = recover_producer_dir(&dir, &IndexPolicy::default()).unwrap();
    assert!(rep.is_empty());
    assert_eq!(fs::metadata(&sealed).unwrap().len(), len);
}

#[test]
fn unquarantined_corruption_found_by_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, sealed) = one_segment(tmp.path(), 300);
    let mut bytes = fs::read(&sealed).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x55;
    fs::write(&sealed, &bytes).unwrap();
    let r = scan(
        &dir,
        &ScanQuery {
            range: ScanRange::SourceSeq {
                from: 0,
                to: u64::MAX,
            },
            limits: ScanLimits::unlimited(),
            resume: None,
        },
    )
    .unwrap();
    assert_eq!(r.corruptions.len(), 1);
    assert!(r.records.len() < 300);
    assert!(
        quarantine_path(&dir, 1).exists(),
        "scan quarantines what it finds"
    );
}
