//! 한 producer의 순서를 보존하면서 채널 A의 포화가 B의 원본을 버리지 않는지 검사한다.
#![allow(clippy::unwrap_used)]
mod common;

use common::{config, record, wait_for};
use loom_journal::{
    AppendError, FaultHook, FaultPoint, JournalWriter, ScanLimits, ScanQuery, ScanRange, scan,
};
use loom_proto::event::GapCount;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

struct Release(Arc<AtomicBool>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[test]
fn flooding_channel_alone_loses_records_and_gaps_keep_its_class() {
    let tmp = tempfile::tempdir().unwrap();
    let stall = Arc::new(AtomicBool::new(true));
    let entered = Arc::new(AtomicBool::new(false));
    let (s, e) = (stall.clone(), entered.clone());
    let hook: FaultHook = Arc::new(move |point| {
        if point == FaultPoint::Write {
            e.store(true, Ordering::SeqCst);
            while s.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    });
    let mut cfg = config(tmp.path());
    cfg.queue_bytes = 64 * 1024;
    cfg.queue_items = 128;
    cfg.class_queue_bytes = 1024;
    cfg.fault = Some(hook);
    let w = JournalWriter::spawn(cfg).unwrap();
    let release = Release(stall);
    let h = w.handle();
    let mut first = record(0);
    first.channel = Some("A".into());
    h.try_append_class("A", &first).unwrap();
    wait_for(Duration::from_secs(5), || entered.load(Ordering::SeqCst));
    let mut accepted = vec![0];
    let mut lost = Vec::new();
    for seq in 1..=20 {
        let mut r = record(seq);
        let normal = seq == 6 || seq == 16;
        r.channel = Some(if normal { "B" } else { "A" }.into());
        // 기존 API도 channel을 자동으로 분류한다.
        match h.try_append(&r) {
            Ok(()) => accepted.push(seq),
            Err(AppendError::Full { source_seq }) => {
                assert!(!normal, "B는 A의 채널 상한에 영향받지 않는다");
                lost.push(source_seq);
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(!lost.is_empty());
    assert!(h.class_queue_bytes("A") <= 1024);
    assert!(h.class_queue_bytes("B") <= 1024);
    drop(release);
    let dir = w.dir().to_path_buf();
    let closed = w.close();
    let count: u64 = closed
        .gaps
        .iter()
        .map(|g| {
            assert_eq!(g.class.as_deref(), Some("A"));
            assert_eq!(g.to_gap_data().detail["class"], "A");
            match g.count {
                GapCount::Exact(n) => n.0,
                GapCount::Unknown => panic!("단일 채널은 정확한 count"),
            }
        })
        .sum();
    assert_eq!(count, lost.len() as u64);
    let result = scan(
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
        result.records.iter().map(|r| r.source_seq).collect::<Vec<_>>(),
        accepted
    );
    assert!(accepted.contains(&6) && accepted.contains(&16));
    assert_eq!(h.class_queue_bytes("A"), 0);
    assert_eq!(h.class_queue_bytes("B"), 0);
}
