//! Verdict rules (TEST plan §6.4 R-1..R-9, §3.1 aggregation, contract 07 §7.4).
//!
//! - [`judge_step`]: the agent's proposed verdict + the step's checks → final verdict or a
//!   rejection code (`E_EVIDENCE_MISSING`, `E_EVIDENCE_MISMATCH`, `E_OUT_OF_WINDOW`,
//!   `E_RECORDING_FAILED`, `E_PLAN_MISMATCH`, all exit 4).
//! - [`UserVerdict`] is a separate type: a user assertion is never disguised as an automatic pass.
//! - [`aggregate`]: session result `passed|failed|inconclusive|incomplete` and exit code
//!   0 / 11 / 8 / 6.

use loom_proto::error::ErrorCode;
use loom_proto::event::WriteState;

use crate::limits::MAX_STEPS;
use crate::pred::{Check, Status};

/// Kind of step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// Has `expect` citations.
    Assert,
    /// Only stimulates (no `expect`): verdict `done`.
    Stimulus,
    /// Judged by the user only (R-7).
    Manual,
}

/// `record.health` during the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordHealth {
    /// Recording fine.
    Ok,
    /// Backlog only (allowed + warning).
    Degraded,
    /// Original data lost: PASS is refused.
    Failed,
}

/// What the agent proposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposed {
    /// Pass.
    Pass,
    /// Fail.
    Fail,
    /// Inconclusive (with a cause).
    Inconclusive(String),
    /// Could not run (precondition, open failure, policy).
    Blocked(String),
    /// Skipped: the agent may only skip when the plan's `when` branch is unmet.
    Skipped {
        /// The step's `when` condition is false.
        when_unmet: bool,
    },
    /// Stimulus done.
    Done,
}

/// A step's check as the engine saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct StepCheck {
    /// The verifier result.
    pub check: Check,
    /// Citation id/hash matches the registered plan's `step.expect`.
    pub plan_match: bool,
    /// Evidence is inside the step's attempt window (anchor rule, T11).
    pub in_window: bool,
}

/// Final verdict of one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepVerdict {
    /// All checks verified with device evidence.
    Pass,
    /// Failed; `unsupported` when no check shows a violation (R-2).
    Fail {
        /// No violating evidence.
        unsupported: bool,
    },
    /// Cannot decide.
    Inconclusive(String),
    /// Could not run.
    Blocked(String),
    /// Skipped (`by_user` = the user skipped it).
    Skipped {
        /// The user skipped it.
        by_user: bool,
    },
    /// Stimulus applied (write ≥ confirmed). Not a pass.
    Done,
}

/// A user's judgment (R-5, R-7). Kept apart from automatic verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserVerdict {
    /// Pass or fail.
    pub pass: bool,
    /// Why.
    pub reason: String,
    /// The automatic verdict it overrides, if any (both appear in the report).
    pub overrides: Option<StepVerdict>,
}

/// Why a proposed verdict was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// No citation / evidence insufficient / agent tx only / manual step.
    EvidenceMissing,
    /// A check contradicts the proposal.
    EvidenceMismatch,
    /// Evidence outside the attempt window.
    OutOfWindow,
    /// Recording failed during the step.
    RecordingFailed,
    /// Citation not the registered plan's `expect`, or a skip outside the plan.
    PlanMismatch,
}

impl Reject {
    /// Error code (all exit 4).
    pub fn code(self) -> ErrorCode {
        match self {
            Reject::EvidenceMissing => ErrorCode::EvidenceMissing,
            Reject::EvidenceMismatch => ErrorCode::EvidenceMismatch,
            Reject::OutOfWindow => ErrorCode::OutOfWindow,
            Reject::RecordingFailed => ErrorCode::RecordingFailed,
            Reject::PlanMismatch => ErrorCode::PlanMismatch,
        }
    }
}

fn confirmed(s: WriteState) -> bool {
    matches!(
        s,
        WriteState::Confirmed | WriteState::EffectiveVerified | WriteState::EffectUnverified
    )
}

/// Judges one step.
///
/// `writes` = final states of the step's writes (stimulus evidence).
pub fn judge_step(
    proposed: &Proposed,
    kind: StepKind,
    checks: &[StepCheck],
    writes: &[WriteState],
    health: RecordHealth,
) -> Result<StepVerdict, Reject> {
    if checks.iter().any(|c| !c.plan_match) {
        return Err(Reject::PlanMismatch);
    }
    if checks.iter().any(|c| !c.in_window) {
        return Err(Reject::OutOfWindow);
    }
    match proposed {
        Proposed::Skipped { when_unmet: true } => return Ok(StepVerdict::Skipped { by_user: false }),
        Proposed::Skipped { when_unmet: false } => return Err(Reject::PlanMismatch),
        Proposed::Blocked(why) => return Ok(StepVerdict::Blocked(why.clone())),
        Proposed::Inconclusive(why) => return Ok(StepVerdict::Inconclusive(why.clone())),
        _ => {}
    }
    match kind {
        // R-7: the agent only asks.
        StepKind::Manual => Err(Reject::EvidenceMissing),
        // R-8: stimulus-only steps end as `done` when a write reached `confirmed`.
        StepKind::Stimulus => match proposed {
            Proposed::Fail => Ok(StepVerdict::Fail {
                unsupported: !writes.iter().any(|w| matches!(w, WriteState::Failed)),
            }),
            _ if writes.iter().copied().any(confirmed) => Ok(StepVerdict::Done),
            _ => Err(Reject::EvidenceMissing),
        },
        StepKind::Assert => match proposed {
            Proposed::Pass => {
                if health == RecordHealth::Failed {
                    return Err(Reject::RecordingFailed);
                }
                if checks.is_empty() {
                    return Err(Reject::EvidenceMissing);
                }
                if checks
                    .iter()
                    .any(|c| matches!(c.check.status, Status::Mismatch | Status::NotFound))
                {
                    return Err(Reject::EvidenceMismatch);
                }
                if checks.iter().any(|c| c.check.status != Status::Verified) {
                    return Err(Reject::EvidenceMissing);
                }
                // R-8 / §7.4: at least one device observation (absence checks carry none, but a
                // step of only own-send matches never passes).
                let positive = checks.iter().filter(|c| c.check.matched_total > 0).count();
                if positive > 0 && !checks.iter().any(|c| c.check.device_evidence) {
                    return Err(Reject::EvidenceMissing);
                }
                Ok(StepVerdict::Pass)
            }
            Proposed::Fail => {
                let shown = checks
                    .iter()
                    .any(|c| matches!(c.check.status, Status::Mismatch | Status::NotFound));
                Ok(StepVerdict::Fail { unsupported: !shown })
            }
            Proposed::Done => Err(Reject::EvidenceMissing),
            _ => Err(Reject::PlanMismatch),
        },
    }
}

/// A step's final state for aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Final {
    /// Automatic verdict.
    Auto(StepVerdict),
    /// User verdict.
    User(UserVerdict),
}

/// One step of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepState {
    /// Step id.
    pub id: String,
    /// Required (not optional / not a branch-only step).
    pub required: bool,
    /// Verdict (None = not judged / not run).
    pub verdict: Option<Final>,
    /// `on_fail: goto` recovery step ids handling a failure of this step.
    pub recovery: Vec<String>,
}

/// Session outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// All required steps pass or done.
    Passed,
    /// A required step failed (not recovered).
    Failed,
    /// Inconclusive / blocked / user-skipped required step.
    Inconclusive,
    /// A required step was not judged or not run.
    Incomplete,
}

impl Outcome {
    /// Wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::Inconclusive => "inconclusive",
            Outcome::Incomplete => "incomplete",
        }
    }
}

/// Aggregated result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    /// Outcome.
    pub outcome: Outcome,
    /// `loom test end` / `loom run --test` exit code: 0 / 11 / 8 / 6.
    pub exit_code: u8,
    /// Failures replaced by a successful `on_fail: goto` recovery (still reported).
    pub recovered_failures: Vec<String>,
    /// Steps deciding the outcome.
    pub decisive: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Ok,
    Inconclusive,
    Incomplete,
    Blocked,
    Failed,
}

fn rank(v: Option<&Final>) -> Rank {
    match v {
        None => Rank::Incomplete,
        Some(Final::User(u)) => {
            if u.pass {
                Rank::Ok
            } else {
                Rank::Failed
            }
        }
        Some(Final::Auto(a)) => match a {
            StepVerdict::Pass | StepVerdict::Done | StepVerdict::Skipped { by_user: false } => Rank::Ok,
            StepVerdict::Fail { .. } => Rank::Failed,
            StepVerdict::Blocked(_) => Rank::Blocked,
            StepVerdict::Inconclusive(_) | StepVerdict::Skipped { by_user: true } => Rank::Inconclusive,
        },
    }
}

/// Aggregates a session (R-6, R-9). `stopped_by_user` = global STOP / user abort → exit 6.
/// More than [`MAX_STEPS`] steps → `Err`.
pub fn aggregate(steps: &[StepState], stopped_by_user: bool) -> Result<SessionResult, String> {
    if steps.len() > MAX_STEPS {
        return Err(format!("{} steps > {MAX_STEPS}", steps.len()));
    }
    let by_id = |id: &str| steps.iter().find(|s| s.id == id);
    let mut worst = Rank::Ok;
    let mut decisive = Vec::new();
    let mut recovered = Vec::new();
    for s in steps.iter().filter(|s| s.required) {
        let mut r = rank(s.verdict.as_ref());
        // on_fail goto: replaced only if every recovery step exists, is required and passed.
        if r == Rank::Failed
            && !s.recovery.is_empty()
            && s.recovery
                .iter()
                .all(|id| by_id(id).is_some_and(|x| x.required && rank(x.verdict.as_ref()) == Rank::Ok))
        {
            recovered.push(s.id.clone());
            r = Rank::Ok;
        }
        if r > worst {
            worst = r;
            decisive.clear();
        }
        if r == worst && r != Rank::Ok {
            decisive.push(s.id.clone());
        }
    }
    let outcome = match worst {
        Rank::Ok => Outcome::Passed,
        Rank::Failed => Outcome::Failed,
        Rank::Incomplete => Outcome::Incomplete,
        Rank::Blocked | Rank::Inconclusive => Outcome::Inconclusive,
    };
    let exit_code = if stopped_by_user {
        6
    } else {
        match outcome {
            Outcome::Passed => 0,
            Outcome::Failed => 11,
            Outcome::Inconclusive | Outcome::Incomplete => 8,
        }
    };
    Ok(SessionResult {
        outcome,
        exit_code,
        recovered_failures: recovered,
        decisive,
    })
}
