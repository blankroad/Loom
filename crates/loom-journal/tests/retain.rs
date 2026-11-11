//! Retention GC: age, quota, pins, active segments, manifest.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashSet;
use std::fs::{self, File};
use std::time::{Duration, SystemTime};

use common::{EPOCH, PRODUCER, append_all, config};
use loom_journal::retain::{DeleteReason, RETENTION_MANIFEST, retain_at};
use loom_journal::segment::list_segments;
use loom_journal::{JournalWriter, QuotaLevel, RetainPolicy, SegmentId, producer_dir};

fn set_mtime(p: &std::path::Path, t: SystemTime) {
    File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn gc_by_age_and_quota_keeps_pins_and_active() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mut c = config(root);
    c.rotate_bytes = 16 * 1024;
    let w = JournalWriter::spawn(c).unwrap();
    append_all(&w.handle(), 0..1_000);
    w.close();
    let dir = producer_dir(root, PRODUCER, EPOCH);
    let segs = list_segments(&dir).unwrap();
    let n = segs.len();
    assert!(n >= 20, "{n} segments");

    // A second producer is still running: its active segment must survive.
    let mut c2 = config(root);
    c2.producer_id = "can-ch0".into();
    let w2 = JournalWriter::spawn(c2.clone()).unwrap();
    let h2 = w2.handle();
    for s in 0..50u64 {
        h2.try_append_raw(s, s, vec![7; 100]).unwrap();
    }
    w2.flush(Duration::from_secs(5)).unwrap();
    let active = list_segments(&producer_dir(root, "can-ch0", EPOCH)).unwrap();
    assert!(active.iter().all(|s| !s.sealed));

    // Segments 1..=5 are 40 days old, the rest progressively newer; seg 2 is pinned.
    let now = SystemTime::now();
    for (i, s) in segs.iter().enumerate() {
        let age = if s.seg_no <= 5 {
            Duration::from_secs(40 * 86_400)
        } else {
            Duration::from_secs(86_400) - Duration::from_secs(i as u64 * 60)
        };
        set_mtime(&s.path, now - age);
    }
    for s in &active {
        set_mtime(&s.path, now - Duration::from_secs(90 * 86_400));
    }
    let pinned = SegmentId {
        producer_id: PRODUCER.into(),
        producer_epoch: EPOCH,
        seg_no: 2,
    };
    let pins: HashSet<SegmentId> = [pinned.clone()].into_iter().collect();

    // Age only (huge quota).
    let rep = retain_at(
        root,
        &RetainPolicy {
            max_age: Duration::from_secs(30 * 86_400),
            max_bytes: u64::MAX / 2,
        },
        &pins,
        now,
    )
    .unwrap();
    let deleted: Vec<u32> = rep.deleted.iter().map(|d| d.id.seg_no).collect();
    assert_eq!(deleted, vec![1, 3, 4, 5]);
    assert!(rep.deleted.iter().all(|d| d.reason == DeleteReason::MaxAge));
    assert_eq!(rep.deleted[0].first_seq, Some(0));
    assert_eq!(rep.level, QuotaLevel::Ok);
    assert_eq!(rep.pinned_kept, 1);
    assert!(dir.join("000002.lj").exists());

    // Quota: set max so usage is ~95 % → prune oldest unpinned until < 90 %.
    let rep0 = retain_at(root, &RetainPolicy::default(), &pins, now).unwrap();
    let used = rep0.bytes_after;
    let max = used * 100 / 95;
    let rep = retain_at(
        root,
        &RetainPolicy {
            max_age: Duration::from_secs(30 * 86_400),
            max_bytes: max,
        },
        &pins,
        now,
    )
    .unwrap();
    assert!(!rep.deleted.is_empty());
    assert!(rep.deleted.iter().all(|d| d.reason == DeleteReason::Quota));
    assert_eq!(rep.deleted[0].id.seg_no, 6, "oldest unpinned first");
    assert!(rep.bytes_after * 10 < max * 9);
    assert!(rep.bytes_after * 10 >= max * 8 || rep.level == QuotaLevel::Ok);
    assert!(dir.join("000002.lj").exists(), "pinned kept");
    assert!(active.iter().all(|s| s.path.exists()), "active kept");

    // Quota smaller than what pins + active hold → Full (caller sets record.health=failed).
    let rep = retain_at(
        root,
        &RetainPolicy {
            max_age: Duration::from_secs(30 * 86_400),
            max_bytes: 1,
        },
        &pins,
        now,
    )
    .unwrap();
    assert_eq!(rep.level, QuotaLevel::Full);
    let left = list_segments(&dir).unwrap();
    assert_eq!(left.iter().map(|s| s.seg_no).collect::<Vec<_>>(), vec![2]);

    // Manifest has one line per deletion.
    let manifest = fs::read_to_string(root.join(RETENTION_MANIFEST)).unwrap();
    let lines: Vec<serde_json::Value> = manifest
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), n - 1);
    assert_eq!(lines[0]["reason"], "max_age");
    assert_eq!(lines[0]["seg_no"], 1);
    assert_eq!(lines[0]["producer_id"], PRODUCER);
    w2.close();
}
