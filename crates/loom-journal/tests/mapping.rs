//! Stream mapping log: round trip, lookup, never-drop backpressure.
#![allow(clippy::unwrap_used)]

mod common;

use common::wait_for;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use loom_journal::{
    FaultHook, FaultPoint, JournalConfig, MappingEntry, MappingError, MappingLog, ScanLimits,
    lookup_mappings, lookup_seq,
};
use loom_proto::envelope::RecordHealth;
use loom_proto::num::U64;

const EPOCH: u64 = 5;

fn entry(i: u64) -> MappingEntry {
    MappingEntry {
        stream_epoch: U64(EPOCH),
        seq_first: U64(i * 10),
        seq_last: U64(i * 10 + 9),
        producer_id: if i.is_multiple_of(2) {
            "uart-dut".into()
        } else {
            "can-ch0".into()
        },
        producer_epoch: U64(1),
        src_first: U64(i * 5),
        src_last: U64(i * 5 + 9),
        mono_first: U64(i * 1_000_000),
        mono_last: U64(i * 1_000_000 + 999),
    }
}

fn cfg(root: &std::path::Path) -> JournalConfig {
    let mut c = JournalConfig::mapping(root, EPOCH);
    c.write_interval = Duration::from_millis(2);
    c.sync_interval = Duration::from_millis(20);
    c.retry_interval = Duration::from_millis(50);
    c.rotate_bytes = 64 * 1024;
    c.index.every_records = 32;
    c
}

fn all(root: &std::path::Path) -> Vec<MappingEntry> {
    lookup_mappings(root, EPOCH, 0, u64::MAX, &ScanLimits::unlimited(), None)
        .unwrap()
        .entries
}

#[test]
fn mapping_roundtrip_and_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    let log = MappingLog::spawn(cfg(tmp.path())).unwrap();
    for i in 0..2_000 {
        log.append_mapping(&entry(i)).unwrap();
    }
    let st = log.flush(Duration::from_secs(5)).unwrap();
    assert_eq!(st.durable_through, Some(19_990), "durable seq_first");
    assert!(matches!(
        log.append_mapping(&entry(5)),
        Err(MappingError::Invalid(_))
    ));
    let mut wrong = entry(3_000);
    wrong.stream_epoch = U64(EPOCH + 1);
    assert!(matches!(
        log.append_mapping(&wrong),
        Err(MappingError::Invalid(_))
    ));
    log.close();

    // Range lookup includes the entry that straddles the start.
    let r = lookup_mappings(tmp.path(), EPOCH, 105, 234, &ScanLimits::unlimited(), None).unwrap();
    assert_eq!(r.entries, (10..=23).map(entry).collect::<Vec<_>>());
    assert_eq!(lookup_seq(tmp.path(), EPOCH, 4_567).unwrap(), Some(entry(456)));
    assert_eq!(lookup_seq(tmp.path(), EPOCH, 19_999).unwrap(), Some(entry(1_999)));
    assert_eq!(lookup_seq(tmp.path(), EPOCH, 20_000).unwrap(), None);
    assert_eq!(all(tmp.path()), (0..2_000).map(entry).collect::<Vec<_>>());

    // Paged lookup.
    let mut resume = None;
    let mut got = Vec::new();
    loop {
        let limits = ScanLimits {
            scan_bytes: u64::MAX,
            result_bytes: 4 * 1024,
            deadline: None,
        };
        let r = lookup_mappings(tmp.path(), EPOCH, 0, u64::MAX, &limits, resume.as_ref()).unwrap();
        got.extend(r.entries);
        if !r.truncated {
            break;
        }
        resume = r.resume_from;
    }
    assert_eq!(got.len(), 2_000);
}

#[test]
fn mapping_is_never_dropped_on_disk_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let f2 = fail.clone();
    let hook: FaultHook = Arc::new(move |p| {
        if p == FaultPoint::Write && f2.load(Ordering::SeqCst) {
            return Err(io::Error::other("injected EIO"));
        }
        Ok(())
    });
    let mut c = cfg(tmp.path());
    c.queue_items = 4;
    c.fault = Some(hook);
    let log = MappingLog::spawn(c).unwrap();
    for i in 0..10 {
        log.append_mapping(&entry(i)).unwrap();
    }
    log.flush(Duration::from_secs(5)).unwrap();

    fail.store(true, Ordering::SeqCst);
    let mut next = 10;
    let mut blocked_for = None;
    while next < 1_000 {
        let t = Instant::now();
        match log.append_mapping(&entry(next)) {
            Ok(()) => next += 1,
            Err(MappingError::Backpressure) => {
                blocked_for = Some(t.elapsed());
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    let blocked_for = blocked_for.unwrap();
    assert!(
        blocked_for >= Duration::from_millis(45) && blocked_for < Duration::from_secs(1),
        "blocked {blocked_for:?}"
    );
    wait_for(Duration::from_secs(5), || {
        log.status().health == RecordHealth::Failed
    });

    fail.store(false, Ordering::SeqCst);
    // The hub retries the same entry until it is accepted.
    let t0 = Instant::now();
    loop {
        match log.append_mapping(&entry(next)) {
            Ok(()) => break,
            Err(MappingError::Backpressure) => assert!(t0.elapsed() < Duration::from_secs(5)),
            Err(e) => panic!("{e}"),
        }
    }
    for i in next + 1..next + 20 {
        log.append_mapping(&entry(i)).unwrap();
    }
    let rep = log.close();
    assert!(rep.gaps.is_empty(), "mapping never reports loss: {:?}", rep.gaps);
    assert_eq!(all(tmp.path()), (0..next + 20).map(entry).collect::<Vec<_>>());
}
