//! CORE integration contracts. Missing journal/policy/runtime services never claim verification.
use crate::*;
use loom_domain_api::{BoxFuture, CallCtx, DomainError, EvalBudget};
use loom_proto::{error::ErrorCode, num::U64, rpc::Role};
use serde_json::Value;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingHealth {
    Healthy,
    /// Recording is backlogged or intentionally off: nothing is claimed as lost, but PASS/done
    /// cannot rely on it either.
    Degraded,
    Failed,
    Unknown,
}
#[derive(Debug)]
pub struct CheckCtx {
    pub call: CallCtx,
    pub session_id: String,
    pub step_id: String,
    pub revision: U64,
    pub plan_hash: Option<String>,
    pub window: AttemptWindow,
    /// CORE clock (`CallCtx::now_mono_ns`) at the check; same base as `AttemptWindow::host_started_ns`
    /// and the gateway clock domain of `Fence::captured_mono_ns`.
    pub now_ns: U64,
    pub budget: EvalBudget,
    pub auto: bool,
    pub user_assertion: bool,
}
impl CheckCtx {
    pub fn journal(&self) -> &dyn loom_domain_api::JournalReader {
        self.call.journal()
    }
}
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub checks: Vec<EvidenceCheck>,
    pub all_expectations_verified: bool,
    pub stimulus_confirmed: bool,
    pub verified_iterations: u32,
    pub error: Option<DomainError>,
}
impl CheckOutcome {
    pub fn unavailable() -> Self {
        Self {
            checks: Vec::new(),
            all_expectations_verified: false,
            stimulus_confirmed: false,
            verified_iterations: 0,
            error: Some(DomainError::not_implemented(
                "TEST verifier/journal/fence adapter",
            )),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Decision {
    pub effective: Option<Verdict>,
    pub rejected: Option<Rejection>,
    pub unsupported: bool,
    pub error: Option<DomainError>,
}
impl Decision {
    pub fn accept(v: Verdict) -> Self {
        Self {
            effective: Some(v),
            rejected: None,
            unsupported: false,
            error: None,
        }
    }
    pub fn reject(code: ErrorCode, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            effective: None,
            rejected: Some(Rejection {
                code,
                message: message.clone(),
            }),
            unsupported: false,
            error: Some(err(code, message)),
        }
    }
}
/// Implementations requery the whole registered expectation window, verify roles/coverage/pins,
/// spend CheckCtx.budget cooperatively, and return all expectations, not only selected witnesses.
pub trait VerifierEngine: Send + Sync {
    fn check(&self, step: &StepSpec, citations: &[Citation], ctx: &mut CheckCtx) -> CheckOutcome;
    fn decide(
        &self,
        step: &StepSpec,
        proposed: Verdict,
        outcome: &CheckOutcome,
        ctx: &mut CheckCtx,
    ) -> Decision;
}
#[derive(Debug, Default)]
pub struct NotWired;
impl VerifierEngine for NotWired {
    fn check(&self, _: &StepSpec, _: &[Citation], _: &mut CheckCtx) -> CheckOutcome {
        CheckOutcome::unavailable()
    }
    fn decide(&self, _: &StepSpec, p: Verdict, o: &CheckOutcome, _: &mut CheckCtx) -> Decision {
        match p {
            Verdict::Pass | Verdict::Done => Decision::reject(
                ErrorCode::NotImplemented,
                "TEST verifier is NotWired; no PASS/done is confirmed",
            ),
            Verdict::Fail => {
                let mut d = Decision::accept(p);
                d.unsupported = o.checks.is_empty();
                d
            }
            _ => Decision::accept(p),
        }
    }
}
/// CORE owns source fences, recording state, STOP and real domain dispatch; no hidden device path.
pub trait SessionHooks: Send + Sync {
    /// Synchronous fence for simple fakes. The engine only calls [`SessionHooks::fence_async`].
    fn fence(&self, _session: &str, _step: &str, _attempt: u32) -> Result<Option<Fence>, DomainError> {
        Ok(None)
    }
    /// Source fence taken before a step transition (`attempt` = new attempt, 0 = end/check) or
    /// before a verification of an open window. CORE waits for its commit barrier here; the
    /// engine never holds its state lock across this call. `Ok(None)` = no fence (not wired);
    /// `Err` refuses the transition (fail closed). Default: [`SessionHooks::fence`].
    fn fence_async<'a>(
        &'a self,
        session: &'a str,
        step: &'a str,
        attempt: u32,
    ) -> BoxFuture<'a, Result<Option<Fence>, DomainError>> {
        Box::pin(async move { self.fence(session, step, attempt) })
    }
    fn recording_health(&self) -> RecordingHealth {
        RecordingHealth::Unknown
    }
    fn stop_latched(&self) -> bool {
        false
    }
    fn approval(
        &self,
        _session: &str,
        _revision: U64,
        _plan_hash: Option<&str>,
    ) -> Result<ApprovalStatus, DomainError> {
        Err(DomainError::not_implemented("plan approval adapter"))
    }
    fn finally<'a>(
        &'a self,
        _ctx: &'a mut CallCtx,
        _session: &'a str,
        _actions: Vec<loom_test_format::Step>,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async {
            Err(DomainError::not_implemented(
                "safe-state finally dispatcher through normal CORE RPC",
            ))
        })
    }
}
#[derive(Debug, Default)]
pub struct NotWiredHooks;
impl SessionHooks for NotWiredHooks {}
/// Gate all domain writes for session-bound agent/script clients. CORE must call this before
/// dispatching writes outside test.* too; the module cannot intercept unrelated registries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientBinding {
    pub client_id: String,
    pub role: Role,
    pub session_id: String,
}
