//! Subscription stream notifications (contract v1 §1.4–1.5, ui-core Q1·Q2): snapshot parts, heartbeat,
//! close, clock and UI reports. Event batches are [`crate::rpc::EventsBatch`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::RecordHealth;
use crate::event::PublishedRecord;
use crate::num::U64;

/// Reserved `sub` id of the shell-wide control subscription. The server creates it automatically
/// for every viewer connection and delivers priority events as `core.events{sub:"control"}` (Q1).
pub const CONTROL_SUB: &str = "control";

/// Notification: event batch ([`crate::rpc::EventsBatch`]).
pub const NOTIFY_EVENTS: &str = "core.events";
/// Notification: [`SnapshotPart`].
pub const NOTIFY_SNAPSHOT_PART: &str = "core.snapshot_part";
/// Notification: [`SnapshotEnd`].
pub const NOTIFY_SNAPSHOT_END: &str = "core.snapshot_end";
/// Notification: [`Heartbeat`] (1 s, not recorded).
pub const NOTIFY_HEARTBEAT: &str = "core.heartbeat";
/// Notification: [`Closed`], sent right before the server closes the connection.
pub const NOTIFY_CLOSED: &str = "core.closed";

/// One domain state in a snapshot (reducer output).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotItem {
    /// Domain.
    pub domain: String,
    /// Channel, if the state is per channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// `<domain>.snapshot`.
    pub kind: String,
    /// Domain-defined state.
    pub state: Value,
}

/// `core.snapshot_part` — part `n` of `total` (1-based), captured atomically at hub turn H.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotPart {
    /// Subscription id.
    pub sub: String,
    /// Part number (1-based).
    pub n: u32,
    /// Total parts.
    pub total: u32,
    /// Domain states.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SnapshotItem>,
    /// Backlog events (seq ≤ H), oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<PublishedRecord>,
}

/// `core.snapshot_end` — live events continue with seq > `stream_seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotEnd {
    /// Subscription id.
    pub sub: String,
    /// High-water H of the snapshot.
    pub stream_seq: U64,
}

/// Per-subscription statistics in the heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubStats {
    /// Subscription id.
    pub sub: String,
    /// Queued events.
    pub depth: u32,
    /// Events dropped so far (display gaps).
    pub drops: U64,
    /// Last flush time (daemon monotonic ns).
    pub last_flush_mono_ns: U64,
}

/// `core.heartbeat` (1 s even without data) — UI computes LIVE/LAG/STALLED from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Heartbeat {
    /// Hub high-water stream seq.
    pub stream_seq_hw: U64,
    /// Daemon monotonic time (ns).
    pub daemon_mono_ns: U64,
    /// Daemon wall time (ns).
    pub daemon_wall_ns: U64,
    /// Recording health.
    pub record_health: RecordHealth,
    /// STOP latch.
    pub latch: bool,
    /// Subscription statistics of this connection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_stats: Vec<SubStats>,
}

/// Why the server closed a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// Priority queue overflow — reconnect and resubscribe with a snapshot.
    SlowConsumer,
    /// Protocol major mismatch.
    ProtoMismatch,
    /// Daemon shutting down.
    Shutdown,
    /// Viewer cookie / token expired.
    AuthExpired,
    /// Protocol violation (bad frames, oversize, no hello).
    ProtocolError,
}

/// `core.closed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Closed {
    /// Reason.
    pub reason: CloseReason,
    /// Human detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Suggested reconnect delay (UI Q12). Absent = client default backoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u32>,
}

/// `core.time` result: daemon receive (d1) and send (d2) monotonic times, so the client can remove
/// server processing time from the offset estimate (CR-UI-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimeResult {
    /// d1: daemon monotonic ns when the request was received.
    pub recv_mono_ns: U64,
    /// d2: daemon monotonic ns when the reply was sent.
    pub send_mono_ns: U64,
    /// Daemon wall time (ns).
    pub wall_ns: U64,
    /// Monotonic clock domain id.
    pub clock_id: String,
}

/// `core.viewer.presence` params (tab visibility, not proof of a human).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ViewerPresence {
    /// Channel the report is about (`None` = whole tab).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Tab/panel visible.
    pub visible: bool,
    /// Highest stream seq rendered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_through_seq: Option<U64>,
    /// Age of the oldest received-but-unrendered event (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_pending_age_ms: Option<u32>,
}

/// `core.metrics.report` params (UI render latency etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetricsReport {
    /// `ui.render` | `ui.render.sample` | …
    pub kind: String,
    /// Channel, if per channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// p50 (ms).
    pub p50: f64,
    /// p95 (ms).
    pub p95: f64,
    /// p99 (ms).
    pub p99: f64,
    /// Sample count.
    pub n: u64,
    /// Clock offset uncertainty (ms).
    pub skew_err_ms: f64,
    /// Highest stream seq rendered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_through_seq: Option<U64>,
    /// Extra fields.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}
