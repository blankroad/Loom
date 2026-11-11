//! `HookHarness`: call write hooks with owned requests and enforce the time budget
//! (contract 08 §8.2: > 1 ms warning, > 10 ms domain bug).

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_domain_api::{
    Binding, Classification, Grant, PatternBinding, PresendError, TargetRef, WriteHooks, WriteRequest,
};
use loom_proto::event::{Env, Origin};
use loom_proto::policy::WriteClass;
use serde_json::Value;

/// Warning threshold.
pub const WARN: Duration = Duration::from_millis(1);
/// Failure threshold.
pub const FAIL: Duration = Duration::from_millis(10);

/// Owned write request for tests; borrow it with [`OwnedWriteRequest::as_req`].
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedWriteRequest {
    /// Domain.
    pub domain: String,
    /// Method.
    pub method: String,
    /// Target.
    pub target: TargetRef,
    /// Bytes to send.
    pub bytes: Option<Vec<u8>>,
    /// Params.
    pub args: Value,
    /// Origin.
    pub origin: Origin,
    /// Client id.
    pub client_id: String,
    /// Env.
    pub env: Env,
    /// Write class.
    pub write_class: WriteClass,
    /// Job plan.
    pub job: Option<Value>,
}

impl OwnedWriteRequest {
    /// Agent request-class write on `channel` with no bytes and empty args.
    pub fn new(method: &str, channel: &str) -> Self {
        OwnedWriteRequest {
            domain: method.split('.').next().unwrap_or_default().to_owned(),
            method: method.to_owned(),
            target: TargetRef {
                channel: Some(channel.to_owned()),
                target: None,
                generation: Some(1),
            },
            bytes: None,
            args: Value::Object(Default::default()),
            origin: Origin::Agent,
            client_id: "test#1".into(),
            env: Env::Sim,
            write_class: WriteClass::Request,
            job: None,
        }
    }

    /// With bytes.
    pub fn bytes(mut self, b: &[u8]) -> Self {
        self.bytes = Some(b.to_vec());
        self
    }

    /// With params.
    pub fn args(mut self, a: Value) -> Self {
        self.args = a;
        self
    }

    /// With origin.
    pub fn origin(mut self, o: Origin) -> Self {
        self.origin = o;
        self
    }

    /// Borrowed view for hooks.
    pub fn as_req(&self) -> WriteRequest<'_> {
        WriteRequest {
            domain: &self.domain,
            method: &self.method,
            target: &self.target,
            bytes: self.bytes.as_deref(),
            args: &self.args,
            origin: self.origin,
            client_id: &self.client_id,
            env: self.env,
            write_class: self.write_class,
            job: self.job.as_ref(),
        }
    }
}

/// A hook result with its duration.
#[derive(Debug, Clone, PartialEq)]
pub struct Timed<T> {
    /// Result.
    pub value: T,
    /// Time taken.
    pub elapsed: Duration,
    /// Exceeded the 1 ms warning budget.
    pub slow: bool,
}

/// Calls hooks and checks their time budget.
pub struct HookHarness {
    hooks: Arc<dyn WriteHooks>,
}

impl std::fmt::Debug for HookHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookHarness").finish_non_exhaustive()
    }
}

fn time<T>(name: &str, f: impl FnOnce() -> T) -> Timed<T> {
    let t = Instant::now();
    let value = f();
    let elapsed = t.elapsed();
    assert!(
        elapsed <= FAIL,
        "hook {name} took {elapsed:?} (> {FAIL:?}): hooks must be non-blocking, no I/O"
    );
    Timed {
        value,
        elapsed,
        slow: elapsed > WARN,
    }
}

impl HookHarness {
    /// Wrap hooks (e.g. `reg.hooks_ref().cloned()` after a probe).
    pub fn new(hooks: Arc<dyn WriteHooks>) -> Self {
        HookHarness { hooks }
    }

    /// `classify`.
    pub fn classify(&self, r: &OwnedWriteRequest) -> Timed<Classification> {
        time("classify", || self.hooks.classify(&r.as_req()))
    }

    /// `approval_binding`.
    pub fn approval_binding(&self, r: &OwnedWriteRequest) -> Timed<Binding> {
        time("approval_binding", || self.hooks.approval_binding(&r.as_req()))
    }

    /// `presend_check`.
    pub fn presend_check(
        &self,
        r: &OwnedWriteRequest,
        grant: Option<&Grant>,
    ) -> Timed<Result<(), PresendError>> {
        time("presend_check", || self.hooks.presend_check(&r.as_req(), grant))
    }

    /// `within_pattern`.
    pub fn within_pattern(&self, r: &OwnedWriteRequest, p: &PatternBinding) -> Timed<bool> {
        time("within_pattern", || self.hooks.within_pattern(&r.as_req(), p))
    }
}
