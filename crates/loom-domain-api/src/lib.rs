//! Gateway-side domain API (contract v1 §8, plan §3.9). Stable surface for domain crates.
//!
//! A domain crate implements [`DomainModule`] and registers everything it offers into a
//! [`Registry`]: RPC commands with handlers, event kinds (payload schema + fixed JSON Pointers),
//! risk table, write hooks, workers, reducers (snapshot providers), evidence verifiers, safe-state
//! and emergency-off actions, interlocks, UI panel manifests, policy section schema and help.
//!
//! P0 (`core-p0-d2`) is **compile-only**: the traits and the registry (with real name validation)
//! exist so domain teams can build against them; `loomd` drives them from P1. Nothing here depends
//! on an async runtime — async methods return [`BoxFuture`].

use std::future::Future;
use std::pin::Pin;

use loom_proto::error::ErrorCode;
use serde_json::Value;

pub mod ctx;
pub mod hooks;
pub mod registry;
pub mod verify;

pub use ctx::{
    ApprovalHandle, CallCtx, CtxServices, EventDraft, Handler, JournalQuery, JournalReader, ScanResult,
    WaitResult, WaitSpec, WorkerHandle,
};
pub use hooks::{
    Binding, Classification, Grant, PatternBinding, PresendError, TargetRef, WriteHooks, WriteRequest,
};
pub use registry::{
    Arch, Bounds, Budgets, CommandSpec, DisplaySummarizer, EmergencyOffSpec, EventKindSpec, HelpExample,
    HelpSpec, Instances, Interlock, MAX_WORKER_TICK, MIN_WORKER_TICK, ModuleRuntime, Owner, Platform,
    PointerSpec, Reducer, RegisterError, Registry, Reopen, RiskRow, RiskTable, RoleSet, SafeStateSpec,
    ValueType, WorkerSpec,
};
pub use verify::{
    BudgetExceeded, Coverage, CoverageNeed, EvalBudget, EventRef, EvidenceVerifier, Predicate, Verification,
};

/// Version of this API. A domain built against another major is disabled with a reason.
pub const DOMAIN_API_VERSION: u32 = 1;

/// Boxed `Send` future — dyn-compatible async without an external crate or runtime.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Error returned by domain handlers and services. `code` decides the CLI exit code.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct DomainError {
    /// Loom error code.
    pub code: ErrorCode,
    /// One-line message.
    pub message: String,
    /// Structured detail (e.g. `write_state`, `written_bytes`).
    pub detail: Value,
}

impl DomainError {
    /// New error without detail.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        DomainError {
            code,
            message: message.into(),
            detail: Value::Null,
        }
    }

    /// Attach detail.
    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    /// `E_NOT_IMPLEMENTED` for P0 stubs (never fake device success).
    pub fn not_implemented(what: &str) -> Self {
        DomainError::new(ErrorCode::NotImplemented, format!("not implemented yet: {what}"))
    }
}

/// A domain compiled into `loomd` (C3: compile-time registration, no dylib).
pub trait DomainModule: Send + Sync + 'static {
    /// Domain name — RPC prefix, CLI group, event `domain` (`uart`, `can`, `uds` …).
    const NAME: &'static str;
    /// API version this domain was built for (compare with [`DOMAIN_API_VERSION`]).
    const API_VERSION: u32;
    /// Register everything the domain offers. Called once at daemon start.
    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError>;
}

/// `[a-z][a-z0-9_]*`.
pub(crate) fn is_token(s: &str) -> bool {
    let mut it = s.bytes();
    matches!(it.next(), Some(b'a'..=b'z'))
        && it.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
