//! CLI / RPC result envelope (contract v1 §4.3, master D5·E8).
//!
//! Every `loom … --json` result and every domain RPC result uses [`Envelope`].
//! `others_since_last` is present on **every** result, including errors (D5, I5).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cursor::Cursor;
use crate::error::{EXIT_OK, ErrorCode};
use crate::event::{Env, GapSummary, Origin, SeqRange};
use crate::num::U64;

/// Recording health (A12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordHealth {
    /// Recording normally.
    #[default]
    Ok,
    /// Backlog building up, nothing lost yet.
    Degraded,
    /// Original data lost or audit unavailable: agent·script R1+ writes blocked.
    Failed,
    /// Intentionally off (`--no-record`; audit still on).
    Disabled,
}

/// Viewer counts (uart-core #13). `visible` is a tab-visibility report, not proof of a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Viewers {
    /// Viewer connections with an active subscription.
    pub subscribed: u16,
    /// Of those, viewers reporting a visible panel.
    pub visible: u16,
}

/// Summary of whether the user can currently see results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// At least one visible viewer.
    Visible,
    /// Viewers connected but none visible (`no_visible_viewer`).
    NotVisible,
    /// No viewer connected (`no_viewer_attached`).
    NoViewer,
}

/// Envelope metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Meta {
    /// Output was cut by size/row limits.
    #[serde(default)]
    pub truncated: bool,
    /// Cursor to continue reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Stream range covered by this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<SeqRange>,
    /// Screen-only drops within the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_gap: Option<GapSummary>,
    /// Recording health at reply time.
    #[serde(default)]
    pub record_health: RecordHealth,
    /// Environment badge (B4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Env>,
    /// STOP latch state.
    #[serde(default)]
    pub latch: bool,
    /// Viewer counts.
    #[serde(default)]
    pub viewers: Viewers,
    /// Stream epoch at reply time.
    #[serde(default)]
    pub stream_epoch: U64,
    /// Visibility summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

/// Warning item (`warnings[]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    /// Warning code (see [`warnings`]).
    pub code: String,
    /// Optional detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

/// Well-known warning codes.
pub mod warnings {
    /// No viewer connected — the user cannot see this.
    pub const NO_VIEWER_ATTACHED: &str = "no_viewer_attached";
    /// Viewers connected but none visible.
    pub const NO_VISIBLE_VIEWER: &str = "no_visible_viewer";
    /// The CLI started the daemon for this call.
    pub const DAEMON_AUTOSTARTED: &str = "daemon_autostarted";
    /// Recording backlog (no loss yet).
    pub const RECORD_DEGRADED: &str = "record_degraded";
    /// Running against a real vehicle.
    pub const ENV_VEHICLE: &str = "env_vehicle";
    /// Windows: the auto-started daemon could not break away from the caller's Job Object.
    pub const DAEMON_TIED_TO_PARENT_JOB: &str = "daemon_tied_to_parent_job";
}

/// User / other-actor activity since this client's previous call (D5, I5).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct OthersSinceLast {
    /// Number of counted events.
    pub count: U64,
    /// Distinct origins among them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<Origin>,
    /// Cursor the count starts from (`None` on the first call).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_cursor: Option<Cursor>,
    /// Count per event kind.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kinds: BTreeMap<String, U64>,
    /// Count per channel (`<domain>:<channel>`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_channel: BTreeMap<String, U64>,
}

/// Error body of a failed result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorBody {
    /// Error code (decides the exit code).
    pub code: ErrorCode,
    /// One-line message.
    pub message: String,
    /// What the agent should do next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Safe to retry as-is (never true for writes with unknown outcome).
    #[serde(default)]
    pub retryable: bool,
    /// Request id, for `loom status --request`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Structured detail.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
}

/// Result envelope of every command / RPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Envelope<T> {
    /// Envelope schema version ([`crate::SCHEMA_VERSION`]).
    pub schema_version: u16,
    /// Success flag.
    pub ok: bool,
    /// Command / method (`uart.send`).
    pub cmd: String,
    /// Result data on success.
    #[serde(default, skip_serializing_if = "Option::<T>::is_none")]
    pub data: Option<T>,
    /// Metadata.
    #[serde(default)]
    pub meta: Meta,
    /// Warnings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
    /// User / other activity since the last call (always present).
    pub others_since_last: OthersSinceLast,
    /// Error on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl<T> Envelope<T> {
    /// Success envelope.
    pub fn success(cmd: impl Into<String>, data: T, meta: Meta, others: OthersSinceLast) -> Self {
        Envelope {
            schema_version: crate::SCHEMA_VERSION,
            ok: true,
            cmd: cmd.into(),
            data: Some(data),
            meta,
            warnings: Vec::new(),
            others_since_last: others,
            error: None,
        }
    }

    /// Error envelope.
    pub fn failure(cmd: impl Into<String>, error: ErrorBody, meta: Meta, others: OthersSinceLast) -> Self {
        Envelope {
            schema_version: crate::SCHEMA_VERSION,
            ok: false,
            cmd: cmd.into(),
            data: None,
            meta,
            warnings: Vec::new(),
            others_since_last: others,
            error: Some(error),
        }
    }

    /// CLI exit code: 0 on success, else the error's group; a failure without a body is 1.
    pub fn exit_code(&self) -> u8 {
        match (&self.error, self.ok) {
            (Some(e), _) => e.code.exit_code(),
            (None, true) => EXIT_OK,
            (None, false) => ErrorCode::Internal.exit_code(),
        }
    }
}
