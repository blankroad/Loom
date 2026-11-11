//! Hub invariants (G1): snapshot seam, priority routing, overload drop accounting, priority reserve.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use loom_gateway::hub::{ConnShared, Hub, HubBuilder, SubStart, core_source};
use loom_proto::event::{SourceRecord, kinds};
use loom_proto::rpc::{Backlog, SubFilter, Subscribe};
use serde_json::{Value, json};

fn data(hub: &Hub, n: u64) -> SourceRecord {
    let mut s = core_source(&hub.clock, "echo", "echo.data", json!({"n": n}));
    s.channel = Some("loop".into());
    s.producer_id = "test".into();
    s
}

fn sub_req(max_events: u32) -> Subscribe {
    let mut s: Subscribe = serde_json::from_value(json!({})).unwrap();
    s.filter = SubFilter::default();
    s.snapshot = true; // `snapshot:false` + no `after_seq` would be live-only (UI C-9)
    s.backlog = Some(Backlog {
        max_events,
        max_bytes: 64 * 1024 * 1024,
        max_age_ms: 3_600_000,
    });
    s
}

fn drain_sub(c: &ConnShared) -> Vec<u64> {
    let mut q = c.q.lock().unwrap();
    q.subs
        .iter_mut()
        .flat_map(|s| s.q.drain(..).map(|p| p.rec.seq.get()).collect::<Vec<_>>())
        .collect()
}

/// T1: a subscription taken while events flow gets backlog ≤ H and live > H with no loss or
/// duplicate at the seam.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_seam_no_loss_no_dup() {
    let hub = HubBuilder::new(7).start().unwrap();
    let h2 = hub.clone();
    let prod = tokio::spawn(async move {
        for i in 0..3000u64 {
            drop(h2.publish(vec![data(&h2, i)]));
            if i % 10 == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    });
    let mut conns = Vec::new();
    for k in 0..200u64 {
        let c = ConnShared::new(k + 1, true);
        if k % 8 == 0 && k > 0 {
            // stay under the per-hub connection cap
            for (c, _) in conns.drain(..) {
                let c: Arc<ConnShared> = c;
                hub.disconnect(c.id);
            }
        }
        hub.connect(c.clone()).await.unwrap();
        let start = hub.subscribe(c.clone(), "s1", sub_req(100_000)).await.unwrap();
        conns.push((c, start));
        tokio::task::yield_now().await;
        // check the seam right away for this subscription
        let (c, start) = conns.last_mut().unwrap();
        let SubStart::Snapshot { h, backlog, .. } =
            std::mem::replace(start, SubStart::Resume { events: vec![] })
        else {
            panic!("expected snapshot");
        };
        let seqs: Vec<u64> = backlog.iter().map(|p| p.rec.seq.get()).collect();
        assert!(seqs.iter().all(|s| *s <= h));
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "backlog contiguous");
        if let Some(l) = seqs.last() {
            assert_eq!(*l, h, "backlog ends at H");
        }
        hub.barrier(Duration::from_secs(2)).await;
        let live = drain_sub(c);
        assert!(live.iter().all(|s| *s > h), "no duplicate across the seam");
        if let Some(f) = live.first() {
            assert_eq!(*f, h + 1, "no loss across the seam");
        }
        assert!(live.windows(2).all(|w| w[1] == w[0] + 1), "live contiguous");
    }
    prod.await.unwrap();
}

/// T4 (hub part): priority kinds go only to the control queue of viewer connections, data only to
/// subscriptions; flood does not displace them.
#[tokio::test]
async fn priority_routed_once_to_control() {
    let hub = HubBuilder::new(1).start().unwrap();
    let c = ConnShared::new(1, true);
    hub.connect(c.clone()).await.unwrap();
    hub.subscribe(c.clone(), "s1", sub_req(0)).await.unwrap();
    for i in 0..5000 {
        drop(hub.publish(vec![data(&hub, i)]));
        if i % 1000 == 0 {
            let latch = core_source(&hub.clock, "core", kinds::CORE_LATCH, json!({"on": true}));
            drop(hub.publish(vec![latch]));
        }
    }
    assert!(hub.barrier(Duration::from_secs(5)).await);
    let q = c.q.lock().unwrap();
    let prio: Vec<&str> = q.prio.iter().map(|p| p.rec.kind.as_str()).collect();
    assert_eq!(prio.iter().filter(|k| **k == kinds::CORE_LATCH).count(), 5);
    assert!(
        q.subs[0].q.iter().all(|p| p.rec.kind != kinds::CORE_LATCH),
        "priority not duplicated into data"
    );
    assert!(!q.prio_overflow);
}

/// Overload: while the hub is stalled, data publishes beyond the budget are refused and later
/// reported as `display.gap{cause:"hub_overload"}`; priority publishes still get through.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overload_drops_counted_priority_reserved() {
    let hub = HubBuilder::new(1).start().unwrap();
    let c = ConnShared::new(1, true);
    hub.connect(c.clone()).await.unwrap();
    hub.stall_for_test(Duration::from_millis(800));
    let mut refused = Vec::new();
    for i in 0..20_000u64 {
        if let Err(r) = hub.try_publish(vec![data(&hub, i)]) {
            refused.extend(r);
        }
    }
    assert!(!refused.is_empty(), "budget must refuse some data");
    hub.count_drops(&refused);
    // Priority publish still admitted (reserve), resolves once the hub resumes.
    let latch = core_source(&hub.clock, "core", kinds::CORE_LATCH, json!({"on": true}));
    let got = tokio::time::timeout(Duration::from_secs(5), hub.publish(vec![latch]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.len(), 1);
    assert!(hub.barrier(Duration::from_secs(5)).await);
    // one more turn so the drop report is published
    drop(hub.publish(vec![data(&hub, 99_999)]));
    assert!(hub.barrier(Duration::from_secs(5)).await);
    let q = c.q.lock().unwrap();
    let gap: Vec<&Value> = q
        .prio
        .iter()
        .filter(|p| p.rec.kind == kinds::DISPLAY_GAP && p.rec.payload["cause"] == "hub_overload")
        .map(|p| &p.rec.payload)
        .collect();
    assert_eq!(gap.len(), 1, "one gap per producer");
    assert_eq!(gap[0]["count"], json!(refused.len().to_string()));
}

/// Barrier times out while the hub is stalled (basis of `E_PARTIAL{published:false}`).
#[tokio::test]
async fn barrier_deadline_on_stall() {
    let hub = HubBuilder::new(1).start().unwrap();
    hub.stall_for_test(Duration::from_millis(500));
    assert!(!hub.barrier(Duration::from_millis(100)).await);
    assert!(hub.barrier(Duration::from_secs(2)).await);
}

/// Resume: `after_seq` in ring → only newer events; other epoch → snapshot with resync cause.
#[tokio::test]
async fn resume_and_epoch_change() {
    let hub = HubBuilder::new(3).start().unwrap();
    let c = ConnShared::new(1, true);
    hub.connect(c.clone()).await.unwrap();
    for i in 0..10 {
        drop(hub.publish(vec![data(&hub, i)]));
    }
    let mut r = sub_req(100);
    r.snapshot = false; // resume
    r.after_seq = Some(loom_proto::num::U64(6));
    r.stream_epoch = Some(loom_proto::num::U64(3));
    match hub.subscribe(c.clone(), "s1", r).await.unwrap() {
        SubStart::Resume { events } => {
            assert_eq!(
                events.iter().map(|p| p.rec.seq.get()).collect::<Vec<_>>(),
                vec![7, 8, 9, 10]
            );
        }
        SubStart::Snapshot { .. } => panic!("expected resume"),
    }
    let mut r = sub_req(100);
    r.after_seq = Some(loom_proto::num::U64(6));
    r.stream_epoch = Some(loom_proto::num::U64(2));
    match hub.subscribe(c.clone(), "s2", r).await.unwrap() {
        SubStart::Snapshot { resync, h, .. } => {
            assert_eq!(resync, Some("epoch_changed"));
            assert_eq!(h, 10);
        }
        SubStart::Resume { .. } => panic!("expected snapshot"),
    }
}
