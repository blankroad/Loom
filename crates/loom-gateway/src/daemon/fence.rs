//! CORE source fences (contract 07 §7.3, core plan "source fence"): per-producer commit coverage.
//!
//! The hub observer records, for every producer id, the newest producer epoch and the highest
//! `source_seq` the hub committed (journaled, stream seq assigned, delivered to the ring). A fence
//! takes the gateway clock, waits for a hub barrier, then copies that table:
//!
//! * gateway producers and **in-process** workers push records into the hub queue synchronously
//!   when they emit, so every record they emitted before `captured_mono_ns` is covered (residual:
//!   two emitter threads of one producer racing between seq assignment and push, sub-µs);
//! * **IPC** workers send RX on a separate data lane (screen mirror, batched) and have no
//!   worker-level flush in `loom-worker/1`: their domains are listed as `unflushed`, and TEST
//!   treats expectations there as `uncertain` instead of guessing.
//!
//! Window membership is decided from these `(producer, epoch, source_seq)` refs; stream seqs and
//! cross-clock timestamps are never used as fences.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use loom_proto::event::{PublishedRecord, SourceRef};
use loom_proto::num::U64;

use crate::hub::HubClock;

/// Producers tracked; past it fences are refused (`E_PARTIAL`) rather than truncated.
pub const MAX_PRODUCERS: usize = 4096;
/// Hub barrier budget of one fence.
pub const FENCE_BARRIER: Duration = Duration::from_secs(1);

/// Committed watermark per producer id: `(newest producer epoch, highest source seq in it)`.
#[derive(Debug, Default)]
pub struct Commits {
    map: HashMap<String, (u64, u64)>,
    overflow: bool,
}

impl Commits {
    /// Hub observer: called on the hub thread for every committed (non-private) record.
    pub fn observe(&mut self, r: &PublishedRecord) {
        let (epoch, seq) = (r.producer_epoch.get(), r.source_seq.get());
        if let Some(e) = self.map.get_mut(r.producer_id.as_str()) {
            if epoch > e.0 {
                *e = (epoch, seq);
            } else if epoch == e.0 && seq > e.1 {
                e.1 = seq;
            }
        } else if self.map.len() < MAX_PRODUCERS {
            self.map.insert(r.producer_id.clone(), (epoch, seq));
        } else {
            self.overflow = true;
        }
    }

    /// Sorted refs, or `None` when a producer could not be tracked.
    pub fn snapshot(&self) -> Option<Vec<SourceRef>> {
        if self.overflow {
            return None;
        }
        let mut refs: Vec<SourceRef> = self
            .map
            .iter()
            .map(|(id, (epoch, seq))| SourceRef {
                producer_id: id.clone(),
                producer_epoch: U64(*epoch),
                source_seq: U64(*seq),
            })
            .collect();
        refs.sort_by(|a, b| a.producer_id.cmp(&b.producer_id));
        Some(refs)
    }
}

/// One source fence (converted to `loom_test::Fence` by the TEST runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFence {
    /// Hub stream epoch.
    pub stream_epoch: u64,
    /// Gateway clock at capture (taken before the barrier).
    pub captured_mono_ns: u64,
    /// Gateway clock domain.
    pub clock_id: String,
    /// Commit coverage per producer.
    pub source_refs: Vec<SourceRef>,
    /// Domains with producers that cannot be flushed (IPC workers).
    pub unflushed_domains: Vec<String>,
}

/// The gateway clock as a worker clock: in-process workers stamp `ts` in the same clock domain as
/// gateway events and fences (`loomd-<pid>`), so anchor/within comparisons are meaningful.
#[derive(Debug)]
pub struct HubClockSource(pub Arc<HubClock>);

impl loom_worker_sdk::Clock for HubClockSource {
    fn mono_ns(&self) -> u64 {
        self.0.mono_ns()
    }
    fn wall_ns(&self) -> u64 {
        self.0.wall_ns()
    }
    fn clock_id(&self) -> &str {
        &self.0.clock_id
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rec(producer: &str, epoch: u64, seq: u64) -> PublishedRecord {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1, "session_id": "s", "domain": "x", "kind": "x.y", "seq": "1", "epoch": "1",
            "producer_id": producer, "producer_epoch": epoch.to_string(), "source_seq": seq.to_string(),
            "ts": {"mono_ns": "1", "wall_ns": "1", "clock_id": "c", "source": "gateway"},
            "origin": "device", "payload": {}
        }))
        .unwrap()
    }

    #[test]
    fn newest_epoch_and_highest_seq_win() {
        let mut c = Commits::default();
        c.observe(&rec("w", 1, 5));
        c.observe(&rec("w", 1, 3));
        c.observe(&rec("g", 7, 1));
        assert_eq!(c.snapshot().unwrap()[1].source_seq, U64(5));
        c.observe(&rec("w", 2, 1));
        c.observe(&rec("w", 1, 9)); // late record of the dead epoch does not move the fence back
        let s = c.snapshot().unwrap();
        assert_eq!((s[1].producer_epoch, s[1].source_seq), (U64(2), U64(1)));
        assert_eq!(s[0].producer_id, "g");
    }

    #[test]
    fn overflow_refuses_the_fence() {
        let mut c = Commits::default();
        for i in 0..=MAX_PRODUCERS {
            c.observe(&rec(&format!("p{i}"), 1, 1));
        }
        assert!(c.snapshot().is_none());
    }
}
