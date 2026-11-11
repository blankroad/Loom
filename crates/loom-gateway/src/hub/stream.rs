//! Connection side of the stream protocol (shared by the real gateway and `--fixture-replay`):
//! subscription frames, adaptive batching over pre-serialized events, heartbeat, per-subscriber
//! gaps and jump-to-live (contract 01 §1.4–1.5).

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_proto::envelope::RecordHealth;
use loom_proto::event::{GapCount, GapData, SeqRange, kinds};
use loom_proto::num::U64;
use loom_proto::rpc::Subscribe;
use loom_proto::stream::{
    CONTROL_SUB, Heartbeat, NOTIFY_EVENTS, NOTIFY_HEARTBEAT, NOTIFY_SNAPSHOT_END, NOTIFY_SNAPSHOT_PART,
    SnapshotEnd, SnapshotItem, SubStats,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::{ConnShared, Hub, JUMP_TO_LIVE, Published, SubStart, core_source, lock};

/// Max events per batch (01 §1.4).
pub const BATCH_MAX_EVENTS: usize = 200;
/// Event bytes per batch (leaves room for the envelope under 64 KiB).
pub const BATCH_MAX_BYTES: usize = 60 * 1024;

/// JSON-RPC notification text.
pub fn notification<T: Serialize>(method: &str, params: &T) -> String {
    json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string()
}

/// `core.events` frame from pre-serialized events (no re-serialization).
pub fn batch_frame(
    hub: &Hub,
    sub: &str,
    events: &[Arc<Published>],
    gap: Option<Value>,
    health: RecordHealth,
) -> String {
    let now = hub.clock.mono_ns();
    let oldest = events.iter().map(|e| e.rec.ts.mono_ns.get()).min().unwrap_or(now);
    let first = events.first().map_or(0, |e| e.rec.seq.get());
    let last = events.last().map_or(0, |e| e.rec.seq.get());
    let mut s = String::with_capacity(events.iter().map(|e| e.json.len() + 1).sum::<usize>() + 256);
    let sub_json = serde_json::to_string(sub).unwrap_or_default();
    let _ = write!(
        s,
        r#"{{"jsonrpc":"2.0","method":"{NOTIFY_EVENTS}","params":{{"sub":{sub_json},"first":"{first}","last":"{last}","events":["#
    );
    for (i, e) in events.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&e.json);
    }
    let lag = now.saturating_sub(oldest) / 1_000_000;
    let health = serde_json::to_string(&health).unwrap_or_default();
    let _ = write!(
        s,
        r#"],"lag_ms":{lag},"hub_publish_mono_ns":"{now}","record_health":{health}"#
    );
    if let Some(g) = gap {
        let _ = write!(s, r#","display_gap":{g}"#);
    }
    s.push_str("}}");
    s
}

fn take_batch(q: &mut std::collections::VecDeque<Arc<Published>>) -> (Vec<Arc<Published>>, usize) {
    let mut out = Vec::new();
    let mut bytes = 0;
    while let Some(e) = q.front() {
        let n = e.json.len();
        if !out.is_empty() && (out.len() >= BATCH_MAX_EVENTS || bytes + n > BATCH_MAX_BYTES) {
            break;
        }
        bytes += n;
        if let Some(e) = q.pop_front() {
            out.push(e);
        }
    }
    (out, bytes)
}

/// Frames for a subscription start (send them **after** the RPC response, 01 §1.4 Q8).
pub fn start_frames(hub: &Hub, conn: &ConnShared, sub: &str, start: SubStart) -> Vec<String> {
    let mut frames = Vec::new();
    match start {
        SubStart::Snapshot {
            h,
            backlog,
            items,
            resync,
        } => {
            if let Some(cause) = resync {
                let kind = if cause == "epoch_changed" {
                    kinds::CORE_EPOCH_CHANGED
                } else {
                    kinds::DISPLAY_GAP
                };
                let payload = if cause == "epoch_changed" {
                    json!({"new": U64(hub.epoch)})
                } else {
                    serde_json::to_value(GapData {
                        seq_range: None,
                        producer_range: None,
                        count: GapCount::Unknown,
                        cause: cause.into(),
                        bytes: None,
                        detail: json!({"sub": sub}),
                    })
                    .unwrap_or(Value::Null)
                };
                drop(hub.publish_to(vec![core_source(&hub.clock, "core", kind, payload)], conn.id));
            }
            // Parts: state items first, then backlog events, each part ≤ ~60 KiB.
            let mut parts: Vec<(Vec<SnapshotItem>, Vec<Arc<Published>>)> = vec![(items, Vec::new())];
            let mut bytes = 0;
            for e in backlog {
                let n = e.json.len();
                let full = parts.last().is_some_and(|p| {
                    !p.1.is_empty() && (p.1.len() >= BATCH_MAX_EVENTS || bytes + n > BATCH_MAX_BYTES)
                });
                if full {
                    parts.push((Vec::new(), Vec::new()));
                    bytes = 0;
                }
                bytes += n;
                if let Some(p) = parts.last_mut() {
                    p.1.push(e);
                }
            }
            let total = parts.len();
            for (i, (items, evs)) in parts.into_iter().enumerate() {
                let items_json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
                let ev: Vec<&str> = evs.iter().map(|e| &*e.json).collect();
                frames.push(format!(
                    r#"{{"jsonrpc":"2.0","method":"{NOTIFY_SNAPSHOT_PART}","params":{{"sub":{},"n":{},"total":{total},"items":{items_json},"events":[{}]}}}}"#,
                    serde_json::to_string(sub).unwrap_or_default(),
                    i + 1,
                    ev.join(",")
                ));
            }
            frames.push(notification(
                NOTIFY_SNAPSHOT_END,
                &SnapshotEnd {
                    sub: sub.to_owned(),
                    stream_seq: U64(h),
                },
            ));
        }
        SubStart::Resume { events } => {
            for c in events.chunks(BATCH_MAX_EVENTS) {
                frames.push(batch_frame(hub, sub, c, None, RecordHealth::Ok));
            }
        }
    }
    frames
}

/// Result of draining a connection's queues.
#[derive(Debug, Default)]
pub struct Drain {
    /// Frames to send in order.
    pub frames: Vec<String>,
    /// Control queue overflowed: send `core.closed{slow_consumer}` and close.
    pub overflow: bool,
    /// When to wake for the next due batch.
    pub deadline: Option<Instant>,
}

/// Drain the control queue fully, then due data subscriptions (adaptive batching).
pub fn drain(hub: &Hub, conn: &ConnShared, only_prio: bool, health: RecordHealth) -> Drain {
    let mut d = Drain::default();
    let mut gaps = Vec::new();
    let mut released = 0;
    {
        let mut q = lock(&conn.q);
        if q.prio_overflow {
            d.overflow = true;
            return d;
        }
        while !q.prio.is_empty() {
            let (b, n) = take_batch(&mut q.prio);
            q.prio_bytes = q.prio_bytes.saturating_sub(n);
            d.frames.push(batch_frame(hub, CONTROL_SUB, &b, None, health));
        }
        if !only_prio {
            let now = Instant::now();
            for s in &mut q.subs {
                // Lag > 2 s: drop the backlog and jump to live (01 §1.4).
                if s.q
                    .front()
                    .is_some_and(|e| now.duration_since(e.at) > JUMP_TO_LIVE)
                {
                    let (a, b, n) = (
                        s.q.front().map_or(0, |e| e.rec.seq.get()),
                        s.q.back().map_or(0, |e| e.rec.seq.get()),
                        s.q.len() as u64,
                    );
                    released += s.bytes;
                    s.bytes = 0;
                    s.q.clear();
                    s.drops += n;
                    gaps.push((s.id.clone(), a, b, n, "jump_to_live"));
                    continue;
                }
                if s.q.is_empty() {
                    continue;
                }
                let due = s.last_flush + Duration::from_millis(u64::from(s.max_batch_ms));
                if now >= due || s.q.len() >= BATCH_MAX_EVENTS {
                    let gap = s.pending_gap.take().map(|(a, b, n, cause)| {
                        gaps.push((s.id.clone(), a, b, n, cause));
                        json!({"count": U64(n), "seq_range": {"from": U64(a), "to": U64(b)}, "causes": [cause]})
                    });
                    let (b, n) = take_batch(&mut s.q);
                    s.bytes = s.bytes.saturating_sub(n);
                    released += n;
                    d.frames.push(batch_frame(hub, &s.id, &b, gap, health));
                    s.last_flush = now;
                    if !s.q.is_empty() {
                        d.deadline = Some(d.deadline.map_or(now, |x| x.min(now)));
                    }
                } else {
                    d.deadline = Some(d.deadline.map_or(due, |x| x.min(due)));
                }
            }
        }
    }
    hub.release_sub_bytes(released);
    for (sub, a, b, n, cause) in gaps {
        // Per-subscriber loss goes to this connection only, with a real seq.
        let g = GapData {
            seq_range: Some(SeqRange {
                from: U64(a),
                to: U64(b),
            }),
            producer_range: None,
            count: GapCount::Exact(U64(n)),
            cause: cause.into(),
            bytes: None,
            detail: json!({"sub": sub}),
        };
        drop(hub.publish_to(
            vec![core_source(
                &hub.clock,
                "display",
                kinds::DISPLAY_GAP,
                serde_json::to_value(g).unwrap_or(Value::Null),
            )],
            conn.id,
        ));
    }
    d
}

/// Heartbeat frame for a connection.
pub fn heartbeat(hub: &Hub, conn: &ConnShared, hw: u64, latch: bool, health: RecordHealth) -> String {
    let now = hub.clock.mono_ns();
    let mut stats = Vec::new();
    {
        let q = lock(&conn.q);
        if conn.viewer {
            stats.push(SubStats {
                sub: CONTROL_SUB.into(),
                depth: u32::try_from(q.prio.len()).unwrap_or(u32::MAX),
                drops: U64(0),
                last_flush_mono_ns: U64(now),
            });
        }
        for s in &q.subs {
            let age = u64::try_from(s.last_flush.elapsed().as_nanos()).unwrap_or(0);
            stats.push(SubStats {
                sub: s.id.clone(),
                depth: u32::try_from(s.q.len()).unwrap_or(u32::MAX),
                drops: U64(s.drops),
                last_flush_mono_ns: U64(now.saturating_sub(age)),
            });
        }
    }
    let hb = Heartbeat {
        stream_seq_hw: U64(hw),
        daemon_mono_ns: U64(now),
        daemon_wall_ns: U64(hub.clock.wall_ns()),
        record_health: health,
        latch,
        sub_stats: stats,
    };
    notification(NOTIFY_HEARTBEAT, &hb)
}

/// Parse `core.subscribe` params with defaults.
pub fn parse_subscribe(p: Option<Value>) -> Result<Subscribe, serde_json::Error> {
    serde_json::from_value(p.unwrap_or_else(|| json!({})))
}
