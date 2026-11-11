//! Queue overflow and disk-failure handling.
#![allow(clippy::unwrap_used)]

mod common;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{append_all, config, record, wait_for};
use loom_journal::gap::causes;
use loom_journal::{
    AppendError, FaultHook, FaultPoint, JournalWriter, QueueBudget, ScanLimits, ScanQuery, ScanRange, scan,
};
use loom_proto::envelope::RecordHealth;
use loom_proto::event::GapCount;
use loom_proto::num::U64;

fn seqs(dir: &std::path::Path) -> Vec<u64> {
    scan(
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
    .unwrap()
    .records
    .iter()
    .map(|r| r.source_seq)
    .collect()
}

#[test]
fn queue_full_returns_full_and_fails_health() {
    let tmp = tempfile::tempdir().unwrap();
    let stall = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(AtomicBool::new(false));
    let (s2, e2) = (stall.clone(), entered.clone());
    let hook: FaultHook = Arc::new(move |p| {
        if p == FaultPoint::Write {
            e2.store(true, Ordering::SeqCst);
            while s2.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    });
    let mut c = config(tmp.path());
    c.queue_items = 8;
    c.fault = Some(hook);
    let w = JournalWriter::spawn(c).unwrap();
    let h = w.handle();
    h.try_append(&record(0)).unwrap();
    wait_for(Duration::from_secs(5), || entered.load(Ordering::SeqCst));

    let mut accepted = vec![0u64];
    let mut dropped = Vec::new();
    for s in 1..100 {
        match h.try_append(&record(s)) {
            Ok(()) => accepted.push(s),
            Err(AppendError::Full { source_seq }) => dropped.push(source_seq),
            Err(e) => panic!("{e}"),
        }
    }
    assert_eq!(accepted.len(), 9, "writer holds 1, queue holds 8");
    assert_eq!(dropped, (9..100).collect::<Vec<_>>());
    let st = h.status();
    assert_eq!(st.health, RecordHealth::Failed);
    assert_eq!(st.drops, 91);
    assert_eq!(st.queue_depth, 8);
    // The dropped seq is consumed.
    assert!(matches!(
        h.try_append(&record(50)),
        Err(AppendError::OutOfOrder { .. })
    ));
    assert!(h.take_gaps().is_empty(), "overflow still open");

    stall.store(false, Ordering::SeqCst);
    wait_for(Duration::from_secs(5), || h.status().queue_depth == 0);
    h.try_append(&record(100)).unwrap();
    let gaps = h.take_gaps();
    assert_eq!(gaps.len(), 1);
    let g = &gaps[0];
    assert_eq!(g.cause, causes::QUEUE_FULL);
    assert_eq!((g.from, g.to), (9, Some(99)));
    assert_eq!(g.count, GapCount::Exact(U64(91)));
    let gd = g.to_gap_data();
    assert_eq!(gd.producer_range.unwrap().to, U64(99));
    w.flush(Duration::from_secs(5)).unwrap();
    assert_eq!(h.health(), RecordHealth::Ok, "loss taken, writer healthy");
    let dir = w.dir().to_path_buf();
    w.close();
    accepted.push(100);
    assert_eq!(seqs(&dir), accepted);
}

#[test]
fn shared_budget_caps_several_writers() {
    let tmp = tempfile::tempdir().unwrap();
    let budget = QueueBudget::new(1 << 20, 4);
    let stall = Arc::new(AtomicBool::new(true));
    let s2 = stall.clone();
    let hook: FaultHook = Arc::new(move |_| {
        while s2.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    });
    let mut a = config(tmp.path());
    a.budget = Some(budget.clone());
    a.fault = Some(hook.clone());
    a.write_interval = Duration::from_secs(10);
    let mut b = a.clone();
    b.producer_id = common::PRODUCER.to_string();
    b.producer_epoch = common::EPOCH + 1;
    let wa = JournalWriter::spawn(a).unwrap();
    let wb = JournalWriter::spawn(b).unwrap();
    // Writers have not popped yet only if we fill fast; count total accepted across both.
    let ha = wa.handle();
    let hb = wb.handle();
    let mut ok = 0;
    for s in 0..50u64 {
        if ha.try_append_raw(s, s, vec![0; 10]).is_ok() {
            ok += 1;
        }
        if hb.try_append_raw(s, s, vec![0; 10]).is_ok() {
            ok += 1;
        }
    }
    // Budget of 4 queued items; each writer may additionally hold popped entries in memory.
    assert!(budget.used().1 <= 4);
    assert!(ok < 100, "budget refused some appends");
    stall.store(false, Ordering::SeqCst);
    wa.close();
    wb.close();
    assert_eq!(budget.used(), (0, 0));
}

#[test]
fn injected_write_error_fails_then_recovers_with_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let f2 = fail.clone();
    let hook: FaultHook = Arc::new(move |p| {
        if p == FaultPoint::Write && f2.load(Ordering::SeqCst) {
            return Err(io::Error::other("injected EIO"));
        }
        Ok(())
    });
    let mut c = config(tmp.path());
    c.fault = Some(hook);
    let w = JournalWriter::spawn(c).unwrap();
    let h = w.handle();
    append_all(&h, 0..100);
    w.flush(Duration::from_secs(5)).unwrap();

    fail.store(true, Ordering::SeqCst);
    append_all(&h, 100..200);
    wait_for(Duration::from_secs(5), || {
        h.status().health == RecordHealth::Failed
    });
    let st = h.status();
    assert!(st.last_error.unwrap().contains("injected EIO"));
    assert!(matches!(
        w.flush(Duration::from_secs(5)),
        Err(loom_journal::JournalError::Failed(_))
    ));
    // Let a few retries fail.
    std::thread::sleep(Duration::from_millis(150));
    assert!(h.take_gaps().is_empty(), "gap is published on recovery");

    fail.store(false, Ordering::SeqCst);
    let mut gaps = Vec::new();
    wait_for(Duration::from_secs(5), || {
        gaps.extend(h.take_gaps());
        !gaps.is_empty()
    });
    assert_eq!(gaps.len(), 1);
    let g = &gaps[0];
    assert_eq!(g.cause, causes::DISK_ERROR);
    assert_eq!((g.from, g.to), (100, Some(199)));
    assert_eq!(g.count, GapCount::Exact(U64(100)));
    wait_for(Duration::from_secs(5), || h.health() == RecordHealth::Ok);

    append_all(&h, 200..300);
    let dir = w.dir().to_path_buf();
    let rep = w.close();
    assert!(rep.gaps.is_empty());
    let expect: Vec<u64> = (0..100).chain(200..300).collect();
    assert_eq!(seqs(&dir), expect);
}

#[test]
fn unwritable_root_fails_then_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("records");
    std::fs::write(&root, b"not a directory").unwrap();
    let w = JournalWriter::spawn(config(&root)).unwrap();
    let h = w.handle();
    append_all(&h, 0..10);
    wait_for(Duration::from_secs(5), || h.health() == RecordHealth::Failed);
    assert!(h.status().last_error.is_some());
    append_all(&h, 10..20);

    std::fs::remove_file(&root).unwrap();
    let mut gaps = Vec::new();
    wait_for(Duration::from_secs(5), || {
        gaps.extend(h.take_gaps());
        !gaps.is_empty()
    });
    assert_eq!((gaps[0].from, gaps[0].to), (0, Some(19)));
    assert_eq!(gaps[0].count, GapCount::Exact(U64(20)));
    append_all(&h, 20..30);
    let dir = w.dir().to_path_buf();
    w.close();
    assert_eq!(seqs(&dir), (20..30).collect::<Vec<_>>());
}

#[test]
fn sync_failure_reports_unknown_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    let f2 = fail.clone();
    let hook: FaultHook = Arc::new(move |p| {
        if p == FaultPoint::Sync && f2.load(Ordering::SeqCst) {
            return Err(io::Error::other("injected fsync failure"));
        }
        Ok(())
    });
    let mut c = config(tmp.path());
    c.fault = Some(hook);
    let w = JournalWriter::spawn(c).unwrap();
    let h = w.handle();
    append_all(&h, 0..10);
    let mut gaps = Vec::new();
    wait_for(Duration::from_secs(5), || {
        gaps.extend(h.take_gaps());
        !gaps.is_empty()
    });
    assert_eq!(gaps[0].cause, causes::SYNC_FAILED);
    assert_eq!((gaps[0].from, gaps[0].to), (0, Some(9)));
    assert_eq!(gaps[0].count, GapCount::Unknown);
    assert_eq!(
        h.durable_through(),
        None,
        "durable never advances over unsynced data"
    );
    fail.store(false, Ordering::SeqCst);
    // Records offered while still failed would be dropped (and reported); wait for recovery.
    wait_for(Duration::from_secs(5), || {
        h.take_gaps();
        h.health() == RecordHealth::Ok
    });
    append_all(&h, 10..20);
    let st = w.flush(Duration::from_secs(5)).unwrap();
    assert_eq!(st.durable_through, Some(19));
    w.close();
}
