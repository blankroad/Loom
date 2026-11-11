//! Write hooks (contract v1 §8.2, master A2). All hooks are **synchronous, non-blocking,
//! ≤ 1 ms per call, no I/O**. The core times them: > 1 ms is a warning, > 10 ms a domain bug.

use std::collections::BTreeMap;

use loom_proto::error::ErrorCode;
use loom_proto::event::{Env, Origin, Risk};
use loom_proto::policy::{ApprovalKind, ApprovalScope, BlockClass, WriteClass};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a write is aimed at.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TargetRef {
    /// Domain channel (`dut`, `ch0`, `ecu1`).
    pub channel: Option<String>,
    /// Finer target inside the channel (ECU address, CAN id …), domain format.
    pub target: Option<String>,
    /// Device/link generation the request was made against.
    pub generation: Option<u32>,
}

/// Borrowed view of a write request handed to hooks.
#[derive(Debug, Clone, Copy)]
pub struct WriteRequest<'a> {
    /// Domain.
    pub domain: &'a str,
    /// RPC method.
    pub method: &'a str,
    /// Target.
    pub target: &'a TargetRef,
    /// Bytes that will actually be sent, when the domain knows them before the backend.
    pub bytes: Option<&'a [u8]>,
    /// Parsed RPC params.
    pub args: &'a Value,
    /// Origin decided by connection credentials.
    pub origin: Origin,
    /// Client id.
    pub client_id: &'a str,
    /// Environment badge (never changes the class — B4).
    pub env: Env,
    /// Write class.
    pub write_class: WriteClass,
    /// Job plan for job-class writes.
    pub job: Option<&'a Value>,
}

/// Result of `classify`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    /// Risk class (shown and recorded even though F1 runs everything automatically).
    pub risk: Risk,
    /// Human reason (`UDS 0x2E WriteDataByIdentifier`).
    pub reason: String,
    /// Block layer when the write must not run.
    pub block: Option<BlockClass>,
}

/// Extra approval digest fields + what the approval screen shows (`approval_binding`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Binding {
    /// Fields bound into the approval digest (physical address, generation, request hash …).
    pub digest_fields: BTreeMap<String, Value>,
    /// Display object for the approval UI (`approval.display` schema).
    pub display: Value,
}

/// An approval (or bundle approval) being used for this write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    /// Approval id.
    pub approval_id: String,
    /// Kind.
    pub kind: ApprovalKind,
    /// Scope.
    pub scope: ApprovalScope,
    /// Digest the approval was bound to.
    pub digest: String,
    /// Remaining uses after the current reservation.
    pub uses_left: u32,
}

/// A domain pattern from a test/run bundle approval (E4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternBinding {
    /// Domain.
    pub domain: String,
    /// Domain-defined pattern (targets, services, DIDs, value ranges, rule …).
    pub pattern: Value,
    /// Maximum uses.
    pub max_uses: u32,
    /// Highest risk the pattern may cover (R4 is never bundled).
    pub risk_max: Risk,
}

/// `presend_check` failure. Invalidates the approval and releases the reservation (exit 6).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresendError {
    /// Late-response quarantine still covers this request.
    #[error("quarantine: {0}")]
    Quarantine(String),
    /// Target locked / lease not held.
    #[error("locked: {0}")]
    Locked(String),
    /// Value shown at approval time changed.
    #[error("stale value: {0}")]
    StaleValue(String),
    /// Link epoch / generation changed since approval.
    #[error("epoch changed: {0}")]
    EpochChanged(String),
}

impl PresendError {
    /// Error code for the client (all exit 6).
    pub const fn code(&self) -> ErrorCode {
        match self {
            PresendError::Quarantine(_) => ErrorCode::Quarantine,
            PresendError::Locked(_) | PresendError::StaleValue(_) | PresendError::EpochChanged(_) => {
                ErrorCode::Locked
            }
        }
    }
}

/// Domain write hooks (A2). All synchronous; see module docs for the budget.
pub trait WriteHooks: Send + Sync {
    /// Risk of the actual bytes/operation and target. No env adjustment (B4).
    fn classify(&self, req: &WriteRequest<'_>) -> Classification;

    /// Extra digest fields and display for approvals (only used when approvals are enabled).
    fn approval_binding(&self, _req: &WriteRequest<'_>) -> Binding {
        Binding::default()
    }

    /// Re-check right before the backend (after policy/approval).
    fn presend_check(&self, req: &WriteRequest<'_>, grant: Option<&Grant>) -> Result<(), PresendError>;

    /// Whether `req` falls inside a bundle-approval pattern. Default: no (individual approval).
    fn within_pattern(&self, _req: &WriteRequest<'_>, _pattern: &PatternBinding) -> bool {
        false
    }
}
