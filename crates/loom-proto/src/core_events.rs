//! Payloads of CORE event kinds and control RPCs (contract v1 §2.5, §4.4; ui-core 7–12), and the
//! shared priority classification used by the hub and the UI (§1.5).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{Origin, Risk, SeqRange, kinds, payload_write_state};
use crate::num::U64;
use crate::policy::{ApprovalKind, ApprovalScope, Presence, PresenceMethod};

/// Driver-queued units that STOP could not cancel, per channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Uncancellable {
    /// `<domain>:<channel>`.
    pub channel: String,
    /// Units already handed to the driver.
    pub count: U64,
}

/// An output kept on while STOP is latched (STOP never switches outputs off — A3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OutputHeld {
    /// Domain.
    pub domain: String,
    /// Channel.
    pub channel: String,
    /// What is held (`PSU out 13.5 V`, `periodic 0x123 stopped, last value held` …).
    pub description: String,
}

/// `core.latch` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LatchPayload {
    /// Latch state.
    pub on: bool,
    /// Who changed it (client id).
    pub by: String,
    /// Origin of the change.
    pub origin: Origin,
    /// Workers that acknowledged.
    #[serde(default)]
    pub acked: Vec<String>,
    /// Workers that did not acknowledge in time (shown as stalled, never as success).
    #[serde(default)]
    pub missing: Vec<String>,
    /// Jobs stopped.
    #[serde(default)]
    pub jobs_stopped: Vec<String>,
    /// Approvals cancelled.
    #[serde(default)]
    pub approvals_cancelled: Vec<String>,
    /// Units the driver already had.
    #[serde(default)]
    pub uncancellable: Vec<Uncancellable>,
    /// Outputs still on.
    #[serde(default)]
    pub outputs_held: Vec<OutputHeld>,
    /// Latch persisted to disk (`false` = valid but not yet durable).
    pub persisted: bool,
}

/// `core.agent_cursor` payload — the agent received user interventions up to `to_seq` (CR-UI-11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AgentCursorPayload {
    /// Agent client id.
    pub client_id: String,
    /// Previous cursor.
    pub from_seq: U64,
    /// New cursor.
    pub to_seq: U64,
    /// `others_since_last.count` delivered with that result.
    pub others_count: U64,
}

/// `core.activity` payload (1 Hz aggregate of agent RPCs, I4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityPayload {
    /// 도메인이 보고한 활동 종류.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// 활동 대상 도메인.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// 활동 대상 채널.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Agent client id.
    pub client_id: String,
    /// Method.
    pub method: String,
    /// Human summary.
    pub summary: String,
    /// Stream range the agent read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<SeqRange>,
    /// What it is waiting for (predicate, deadline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<Value>,
}

/// `core.approval_requested` payload (only when approvals are enabled by policy, F1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRequestedPayload {
    /// Approval id.
    pub approval_id: String,
    /// Kind.
    pub kind: ApprovalKind,
    /// Risk.
    pub risk: Risk,
    /// Method.
    pub method: String,
    /// Target description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Domain display object (`approval_binding().display`).
    #[serde(default)]
    pub display: Value,
    /// Scope.
    pub scope: ApprovalScope,
    /// Must be used before (wall ns).
    pub use_by_wall_ns: U64,
    /// Requesting client.
    pub requested_by: String,
    /// Presence check required, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceMethod>,
}

/// Outcome of an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    /// Approved.
    Approved,
    /// Denied.
    Denied,
    /// Expired.
    Expired,
    /// Invalidated (state changed, STOP …).
    Invalidated,
}

/// `core.approval_resolved` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalResolvedPayload {
    /// Approval id.
    pub approval_id: String,
    /// Outcome.
    pub outcome: ApprovalOutcome,
    /// Deciding client (viewer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Viewer decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Approve.
    Approve,
    /// Deny.
    Deny,
}

/// `core.approval.respond` params (viewer with approve rights).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRespond {
    /// Approval id.
    pub approval_id: String,
    /// Decision.
    pub decision: Decision,
    /// Narrower scope than requested, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ApprovalScope>,
    /// Presence proof when required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
}

/// One safe-state recovery action (`core.safe_state.list` result item, A15).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SafeStateKind {
    /// 일반 복구 동작(구버전 기본값).
    #[default]
    Recover,
    /// 명시적으로 선언한 비상 출력 차단.
    EmergencyOff,
}

/// 복구 목록 RPC의 결과 봉투.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SafeStateList {
    /// 표시할 복구·비상 동작.
    pub items: Vec<SafeStateItem>,
}

/// 안전 상태 동작 하나.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SafeStateItem {
    /// 복구 또는 비상 출력 차단 동작.
    #[serde(default)]
    pub kind: SafeStateKind,
    /// Id (`<domain>.<name>`).
    pub id: String,
    /// Domain.
    pub domain: String,
    /// Channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Method executed (origin = user).
    pub method: String,
    /// Button text.
    pub description: String,
    /// An interlock currently blocks it.
    #[serde(default)]
    pub blocked: bool,
}

/// `core.safe_state.run` params — one key runs several items (CR-UI-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SafeStateRun {
    /// Item ids.
    pub ids: Vec<String>,
}

/// Per-item outcome of a safe-state run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SafeStateOutcome {
    /// Sent (see the write rows of `request_id`).
    Sent,
    /// Skipped (interlock / deny).
    Skipped,
    /// Failed.
    Failed,
}

/// One item of a [`SafeStateRunResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SafeStateRunItem {
    /// Item id.
    pub id: String,
    /// Outcome.
    pub outcome: SafeStateOutcome,
    /// Skipped because an interlock / deny blocked it (`finally_blocked`).
    #[serde(default)]
    pub finally_blocked: bool,
    /// Reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Request id of the write rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// `core.safe_state.run` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SafeStateRunResult {
    /// Per-item outcomes, in request order.
    pub items: Vec<SafeStateRunItem>,
}

/// CORE kinds that always travel on the priority (control) path.
const PRIORITY_CORE_KINDS: &[&str] = &[
    kinds::CORE_LATCH,
    kinds::CORE_APPROVAL_REQUESTED,
    kinds::CORE_APPROVAL_RESOLVED,
    kinds::CORE_CONFIRM_ISSUED,
    kinds::CORE_POLICY_DENIED,
    kinds::CORE_POLICY_CHANGED,
    kinds::CORE_WORKER_STATE,
    kinds::CORE_JOB_STATE,
    kinds::CORE_JOB_PROGRESS,
    kinds::CORE_ACTIVITY,
    kinds::CORE_AGENT_CURSOR,
    kinds::CORE_MARK,
    kinds::CORE_EPOCH_CHANGED,
    kinds::CORE_WRITE_RECOVERED,
    kinds::CORE_SESSION_CLOSED,
    kinds::CORE_ENV,
    kinds::DISPLAY_GAP,
    kinds::RECORD_GAP,
    kinds::RECORD_HEALTH,
    kinds::SOURCE_GAP,
    kinds::UI_FOCUS_REQUEST,
];

/// Domains whose every event is a control event (test/run progress, verdicts).
const PRIORITY_DOMAINS: &[&str] = &["test", "run"];

/// Domain event names that are state/error/alert reports (`uart.state`, `can.channel_state`, `uds.alert`).
fn is_state_like(event: &str) -> bool {
    matches!(event, "state" | "error" | "alert" | "timeout" | "late")
        || event.ends_with("_state")
        || event.ends_with("_error")
}

/// Whether an event goes on the priority (control) path — never dropped, sent before data (§1.5).
///
/// Rules: (1) the CORE kinds in this module's list; (2) every `test.*` / `run.*` event;
/// (3) any event whose `payload.write_state` is present — **all** write-row steps, not only the
/// final one, so `requested → … → confirmed` stay in order on one path; (4) domain state / error /
/// alert / timeout / late events (`<domain>.state`, `<domain>.*_state`, `<domain>.*_error`).
/// Bulk data (`uart.data`, `can.frame` without write state) is not priority.
pub fn is_priority_kind(kind: &str, payload: &Value) -> bool {
    if PRIORITY_CORE_KINDS.contains(&kind) {
        return true;
    }
    let (domain, event) = kind.split_once('.').unwrap_or((kind, ""));
    if PRIORITY_DOMAINS.contains(&domain) {
        return true;
    }
    if matches!(payload_write_state(payload), Ok(Some(_)) | Err(_)) {
        return true;
    }
    is_state_like(event)
}
