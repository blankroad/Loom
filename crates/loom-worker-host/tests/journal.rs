//! Worker-side journal (contract 06 §6.1): originals survive credit starvation, orphan seals,
//! journal failures are reported on ctl, crashed epochs are recovered by the host.
#![allow(clippy::unwrap_used)]
#![cfg(unix)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use loom_journal::segment::list_segments;
use loom_journal::{ScanLimits, ScanQuery, ScanRange, producer_dir, scan};
use loom_proto::event::{GapCount, GapData, SourceRecord};
use loom_proto::worker::Lane;
use loom_worker_host::WorkerState;
use serde_json::json;

const T: Duration = Duration::from_secs(5);

fn journal_records(root: &Path, instance: &str, epoch: u64) -> Vec<SourceRecord> {
    let r = scan(
        &producer_dir(root, instance, epoch),
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
    assert!(r.corruptions.is_empty(), "{:?}", r.corruptions);
    r.records
        .iter()
        .map(|r| r.decode_source_record().unwrap())
        .collect()
}

fn all_sealed(root: &Path, instance: &str, epoch: u64) -> bool {
    let segs = list_segments(&producer_dir(root, instance, epoch)).unwrap();
    !segs.is_empty() && segs.iter().all(|s| s.sealed)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_rx_producer_keeps_health_alive_and_all_originals() {
    let dir = sock_dir();
    let root = dir.path().join("rec");
    let mut cfg = config(dir.path(), "t-slow-rx", &[]);
    cfg.journal_dir = Some(root.clone());
    cfg.initial_credit = Some(0);
    cfg.regrant_credit = false;
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();
    let n = 10_000u64;
    // 일시적으로 느린 장비 생산자는 supervisor의 kill 1.2초 상한보다 오래 일한다.
    // SDK 제어 스레드는 계속 heartbeat를 보내야 한다. 타임아웃을 늘리지 않는다.
    let result = s
        .host
        .call(call("rx", json!({"n":n,"producer_delay_ms":2000})))
        .await
        .unwrap();
    assert_eq!(result["ok"], n);
    assert_eq!(result["err"], 0);
    assert_eq!(s.host.pid(), Some(pid));
    assert_eq!(s.host.producer_epoch(), 1);
    assert_eq!(s.states.seen(WorkerState::Suspect), 0);
    assert_eq!(s.states.seen(WorkerState::Killed), 0);
    s.host.shutdown(false).await;
    wait_until(T, || !alive(pid)).await;
    let rows = journal_records(&root, "t-slow-rx", 1);
    assert_eq!(rows.iter().filter(|r| r.kind == "test.rx").count() as u64, n);
    assert!(all_sealed(&root, "t-slow-rx", 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credit_starved_flood_is_fully_journaled_and_orphan_seals() {
    let dir = sock_dir();
    let jroot = dir.path().join("rec");
    let mut cfg = config(dir.path(), "t-jr", &[]);
    cfg.journal_dir = Some(jroot.clone());
    cfg.initial_credit = Some(0);
    cfg.regrant_credit = false;
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();

    let n = 10_000u64;
    let r = s.host.call(call("rx", json!({"n": n}))).await.unwrap();
    assert_eq!(r["ok"], json!(n));
    // Screen side: exactly the mirror overflow is reported as display.gap (screen only).
    let mut screen_lost = 0;
    wait_until(T, || {
        screen_lost = s
            .sink
            .kinds(Lane::Ctl, "display.gap")
            .iter()
            .map(|g| {
                let d: GapData = serde_json::from_value(g.payload.clone()).unwrap();
                let GapCount::Exact(c) = d.count else { panic!() };
                c.0
            })
            .sum::<u64>();
        screen_lost == n - 8192
    })
    .await;
    // Health carries the journal watermarks.
    wait_until(T, || {
        s.host.state()["journal"]["durable_through"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|d| d >= r["last"].as_u64().unwrap())
    })
    .await;
    assert_eq!(s.host.state()["journal"]["health"], "ok");

    // Gateway goes away: worker latches, keeps journaling, seals at orphan exit.
    s.host.shutdown(false).await;
    wait_until(T, || !alive(pid)).await;
    assert!(all_sealed(&jroot, "t-jr", 1), "orphan exit sealed the journal");
    let recs = journal_records(&jroot, "t-jr", 1);
    let rx: Vec<_> = recs.iter().filter(|r| r.kind == "test.rx").collect();
    assert_eq!(
        rx.len() as u64,
        n,
        "every original is on disk despite zero credit"
    );
    assert_eq!(rx[0].source_seq.0, r["first"].as_u64().unwrap());
    assert_eq!(rx[rx.len() - 1].source_seq.0, r["last"].as_u64().unwrap());
    // display.gap events are journaled too (they are events of this producer).
    let gaps_on_disk = recs.iter().filter(|r| r.kind == "display.gap").count();
    assert_eq!(gaps_on_disk, s.sink.kinds(Lane::Ctl, "display.gap").len());
    // Strictly increasing seqs on disk.
    assert!(recs.windows(2).all(|w| w[0].source_seq < w[1].source_seq));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unwritable_journal_dir_reports_failed_then_gap_on_ctl() {
    let dir = sock_dir();
    let jroot = dir.path().join("rec");
    std::fs::write(&jroot, b"not a directory").unwrap();
    let mut cfg = config(dir.path(), "t-jf", &["--journal-retry-ms", "100"]);
    cfg.journal_dir = Some(jroot.clone());
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;

    let r = s.host.call(call("rx", json!({"n": 100}))).await.unwrap();
    let (first, last) = (r["first"].as_u64().unwrap(), r["last"].as_u64().unwrap());
    wait_until(T, || {
        s.sink
            .kinds(Lane::Ctl, "record.health")
            .iter()
            .any(|e| e.payload["health"] == "failed")
    })
    .await;
    let failed = s
        .sink
        .kinds(Lane::Ctl, "record.health")
        .into_iter()
        .find(|e| e.payload["health"] == "failed")
        .unwrap();
    assert_eq!(failed.domain, "test");
    assert!(failed.payload["error"].as_str().is_some_and(|e| !e.is_empty()));
    wait_until(T, || s.host.state()["journal"]["health"] == "failed").await;

    // Disk comes back: the exact loss is reported as record.gap, health returns to ok.
    std::fs::remove_file(&jroot).unwrap();
    wait_until(T, || !s.sink.kinds(Lane::Ctl, "record.gap").is_empty()).await;
    let g = &s.sink.kinds(Lane::Ctl, "record.gap")[0];
    let d: GapData = serde_json::from_value(g.payload.clone()).unwrap();
    assert_eq!(d.cause, "disk_error");
    let pr = d.producer_range.unwrap();
    assert_eq!(pr.producer_id, "t-jf");
    assert!(
        pr.from.0 <= first && pr.to.0 >= last,
        "{pr:?} covers {first}..={last}"
    );
    let GapCount::Exact(c) = d.count else { panic!() };
    assert!(
        c.0 >= 100,
        "all 100 rx (plus records emitted while failed) are counted: {}",
        c.0
    );
    wait_until(T, || {
        s.sink
            .kinds(Lane::Ctl, "record.health")
            .last()
            .is_some_and(|e| e.payload["health"] == "ok")
    })
    .await;

    // Recording works again.
    let r2 = s.host.call(call("rx", json!({"n": 10}))).await.unwrap();
    let pid = s.host.pid().unwrap();
    s.host.shutdown(false).await;
    wait_until(T, || !alive(pid)).await;
    let recs = journal_records(&jroot, "t-jf", 1);
    let rx: Vec<u64> = recs
        .iter()
        .filter(|r| r.kind == "test.rx")
        .map(|r| r.source_seq.0)
        .collect();
    assert_eq!(rx.len(), 10);
    assert_eq!(rx[0], r2["first"].as_u64().unwrap());
    assert!(
        recs.iter()
            .any(|r| r.kind == "record.gap" || r.kind == "record.health")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crashed_worker_journal_is_recovered_by_host() {
    let dir = sock_dir();
    let jroot = dir.path().join("rec");
    let mut cfg = config(dir.path(), "t-jc", &[]);
    cfg.journal_dir = Some(jroot.clone());
    let s = start(cfg);
    wait_state(&s.host, WorkerState::Ready, T).await;
    let pid = s.host.pid().unwrap();
    let r = s.host.call(call("rx", json!({"n": 1000}))).await.unwrap();
    // Wait until it is written to the OS, then crash.
    wait_until(T, || {
        s.host.state()["journal"]["written_through"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|w| w >= r["last"].as_u64().unwrap())
    })
    .await;
    kill9(pid);
    wait_until(T, || {
        s.states.events().iter().any(|e| {
            e.state == WorkerState::Exited && e.detail["journal_recovery"]["segments"].as_u64() >= Some(1)
        })
    })
    .await;
    assert!(all_sealed(&jroot, "t-jc", 1), "host sealed the dead epoch");
    let rx = journal_records(&jroot, "t-jc", 1)
        .into_iter()
        .filter(|r| r.kind == "test.rx")
        .count();
    assert_eq!(rx, 1000, "written records survive kill -9");
    // The restarted worker journals into a new epoch.
    wait_until(T, || {
        s.host.worker_state() == WorkerState::Ready && s.host.producer_epoch() == 2
    })
    .await;
    s.host.shutdown(true).await;
}
