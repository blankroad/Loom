//! Loss reports produced by the journal (feed `record.gap`, contract 02 A13).

use loom_proto::event::{GapCount, GapData, ProducerRange};
use loom_proto::num::U64;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// `cause` strings used by this crate.
pub mod causes {
    /// Open segment tail cut after a crash (contract 06 §6.2).
    pub const CRASH_TAIL: &str = "crash_tail";
    /// CRC error inside a sealed segment (file quarantined).
    pub const CORRUPTION: &str = "corruption";
    /// Writer input queue full; the caller's `try_append` got `Full` (plan Q2).
    pub const QUEUE_FULL: &str = "journal_queue_full";
    /// Write failed with "no space".
    pub const DISK_FULL: &str = "disk_full";
    /// Other write/open failure.
    pub const DISK_ERROR: &str = "disk_error";
    /// `sync_data` failed: records already written may or may not be on disk.
    pub const SYNC_FAILED: &str = "sync_failed";
    /// Writer closed while still failed; buffered records were never written.
    pub const CLOSED_FAILED: &str = "closed_while_failed";
    /// Several gaps merged because the pending-gap list hit its cap.
    pub const MULTIPLE: &str = "multiple";
}

/// One loss of journal records of one producer epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapReport {
    /// Producer id.
    pub producer_id: String,
    /// Producer epoch.
    pub producer_epoch: u64,
    /// First lost `source_seq` (inclusive). For tail/corruption: last good seq + 1.
    pub from: u64,
    /// Last lost `source_seq` (inclusive); `None` = unknown end (crash tail, corruption).
    pub to: Option<u64>,
    /// Lost record count, or unknown.
    pub count: GapCount,
    /// See [`causes`].
    pub cause: String,
    /// Lost bytes when known.
    pub bytes: Option<u64>,
    /// 채널별 큐 손실의 계정 이름. 디스크·복구 손실처럼 채널을 확정할 수 없으면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

impl GapReport {
    /// Payload for a `record.gap` event (`domain`/`channel` are chosen by the caller).
    pub fn to_gap_data(&self) -> GapData {
        let producer_range = self.to.map(|to| ProducerRange {
            producer_id: self.producer_id.clone(),
            producer_epoch: U64(self.producer_epoch),
            from: U64(self.from),
            to: U64(to),
        });
        let mut detail = if self.to.is_none() {
            json!({
                "producer_id": self.producer_id,
                "producer_epoch": self.producer_epoch.to_string(),
                "from": self.from.to_string(),
                "to": "unknown",
            })
        } else {
            serde_json::Value::Null
        };
        if let Some(class) = &self.class {
            if detail.is_null() {
                detail = json!({});
            }
            detail["class"] = json!(class);
        }
        GapData {
            seq_range: None,
            producer_range,
            count: self.count,
            cause: self.cause.clone(),
            bytes: self.bytes.map(U64),
            detail,
        }
    }

    /// Merge `other` into `self` (used when the pending list is full).
    pub(crate) fn absorb(&mut self, other: &GapReport) {
        let same_class = self.class == other.class;
        self.from = self.from.min(other.from);
        self.to = match (self.to, other.to) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
        self.count = match (self.count, other.count) {
            (GapCount::Exact(a), GapCount::Exact(b)) => GapCount::Exact(U64(a.0.saturating_add(b.0))),
            _ => GapCount::Unknown,
        };
        self.bytes = match (self.bytes, other.bytes) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        };
        if !same_class {
            self.class = None;
            self.count = GapCount::Unknown;
        }
        if self.cause != other.cause || !same_class {
            self.cause = causes::MULTIPLE.to_string();
        }
    }
}

/// Marker frame payload written into the journal itself when a loss is closed.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GapMarker<'a> {
    pub marker: &'a str,
    pub gap: &'a GapReport,
}
