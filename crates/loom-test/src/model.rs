use loom_proto::{error::ErrorCode, event::SourceRef, num::U64};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Planned,
    WaitingApproval,
    Running,
    Paused,
    Stopped,
    Done,
}
impl SessionState {
    pub fn active(self) -> bool {
        !matches!(self, Self::Stopped | Self::Done)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    Inconclusive,
    Blocked,
    Skipped,
    Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionResult {
    Passed,
    Failed,
    Inconclusive,
    Incomplete,
}
impl SessionResult {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::Failed => 11,
            Self::Inconclusive | Self::Incomplete => 8,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Ended,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepDomain {
    pub domain: String,
    pub channel: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepDefinition {
    pub step_id: String,
    pub n: u32,
    pub title: String,
    pub expect_text: String,
    pub source_text: String,
    pub domains: Vec<StepDomain>,
    pub timeout_ms: Option<U64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fence {
    pub stream_epoch: U64,
    pub source_refs: Vec<SourceRef>,
    pub captured_mono_ns: U64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptWindow {
    pub attempt: u32,
    pub host_started_ns: U64,
    pub host_ended_ns: Option<U64>,
    pub start: Option<Fence>,
    pub end: Option<Fence>,
}
#[derive(Debug, Clone)]
pub struct StepSpec {
    pub definition: StepDefinition,
    pub template: Option<loom_test_format::Step>,
    pub plan_hash: Option<String>,
    pub revision: U64,
    pub window: AttemptWindow,
}
/// An input citation is a witness candidate, never an already-verified claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    pub input: Value,
    pub reference: Option<String>,
    pub expectation: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Verified,
    Mismatch,
    NotFound,
    Uncertain,
    Error,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Witness {
    Device,
    CompleteAbsence,
    Measurement,
    UserAssertion,
    OwnTxOnly,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceCheck {
    pub evidence_id: String,
    pub expectation: u32,
    #[serde(rename = "ref")]
    pub reference: String,
    pub status: EvidenceStatus,
    pub observed: Value,
    pub cause: Option<String>,
    pub complete: bool,
    pub gap_in_window: bool,
    pub digest: Option<String>,
    pub matched_refs: Vec<String>,
    pub witness: Witness,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejection {
    pub code: ErrorCode,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerdictRecord {
    pub proposed: Verdict,
    pub effective: Option<Verdict>,
    pub by: String,
    pub reason: Option<String>,
    pub evidence_refs: Vec<String>,
    #[serde(rename = "override")]
    pub overridden: bool,
    pub previous: Option<Value>,
    pub rejected: Option<Rejection>,
    pub unsupported: bool,
    pub user_assertion: bool,
    pub attempt: u32,
    pub revision: U64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepSnapshot {
    #[serde(flatten)]
    pub definition: StepDefinition,
    pub status: StepStatus,
    pub attempt: u32,
    pub verdict: Option<VerdictRecord>,
    pub history: Vec<VerdictRecord>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub ask_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub timeout_ms: U64,
    pub step_id: Option<String>,
    pub revision: U64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    Auto,
    Required,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Granted { approval_id: String },
}
