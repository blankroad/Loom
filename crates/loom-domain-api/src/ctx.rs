//! Command handlers and their call context (plan §3.9). The gateway builds a [`CallCtx`] per RPC;
//! P0 only fixes the signatures — the services behind [`CtxServices`] arrive in P1.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use loom_proto::cursor::Cursor;
use loom_proto::event::{Dir, Env, GapData, Metric, Origin, PublishedRecord, Risk, SeqRange};
use loom_proto::rpc::{Rights, Role, SubFilter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{BoxFuture, DomainError};

/// An event a gateway module wants published. Origin, client, producer identity, timestamps and
/// stream position are filled in by the core from the call context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDraft {
    /// Kind `<domain>.<event>` (must be registered).
    pub kind: String,
    /// Channel.
    pub channel: Option<String>,
    /// Direction. `Rx` denotes bytes received from the device: CORE assigns device origin,
    /// while keeping the initiating request/client linkage. Do not use it for agent summaries.
    pub dir: Option<Dir>,
    /// Request id (write rows).
    pub request_id: Option<String>,
    /// Job id.
    pub job_id: Option<String>,
    /// Device/link generation.
    pub generation: Option<u32>,
    /// Purpose.
    pub purpose: Option<String>,
    /// Risk class (writes).
    pub risk: Option<Risk>,
    /// Environment (required on writes).
    pub env: Option<Env>,
    /// Kind-specific payload (`payload.write_state`, `payload.bytes` …).
    pub payload: Value,
    /// Numeric samples.
    pub metrics: Vec<Metric>,
    /// Quality flags.
    pub quality: Vec<String>,
}

impl EventDraft {
    /// Draft with only kind and payload set.
    pub fn new(kind: impl Into<String>, payload: Value) -> Self {
        EventDraft {
            kind: kind.into(),
            channel: None,
            dir: None,
            request_id: None,
            job_id: None,
            generation: None,
            purpose: None,
            risk: None,
            env: None,
            payload,
            metrics: Vec::new(),
            quality: Vec::new(),
        }
    }
}

/// Bounded journal query (links beyond the ring go to disk; never unbounded).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalQuery {
    /// Start position.
    pub from: Cursor,
    /// End position (inclusive), `None` = up to now.
    pub until: Option<Cursor>,
    /// Event filter.
    pub filter: SubFilter,
    /// Only rows of this request.
    pub request_id: Option<String>,
    /// Maximum events returned.
    pub max_events: u32,
}

/// Result of a bounded journal scan.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ScanResult {
    /// Matching events in stream order.
    pub events: Vec<PublishedRecord>,
    /// The whole requested range was scanned with no loss.
    pub complete: bool,
    /// Losses inside the range (record/source gaps, retention).
    pub gaps: Vec<GapData>,
    /// Where to continue when a limit was hit (`E_SCAN_LIMIT`).
    pub resume_from: Option<Cursor>,
    /// Last stream coordinate examined, including rows excluded by the filter.
    #[serde(default)]
    pub scanned_through: u64,
    /// Bytes scanned.
    pub scanned_bytes: u64,
}

/// Read-only access to the journal (original data on disk).
pub trait JournalReader: Send + Sync {
    /// Scan at most `byte_limit` bytes for `query`.
    fn scan<'a>(
        &'a self,
        query: &'a JournalQuery,
        byte_limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>>;
}

/// Handle to a worker of this domain (ctl lane).
pub trait WorkerHandle: Send + Sync {
    /// Generic call (open, close, query …).
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>>;
    /// Write that already passed the pipeline (request class). Returns the worker result.
    fn write<'a>(
        &'a self,
        request_id: &'a str,
        op: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>>;
}

/// Approval services visible to domains.
pub trait ApprovalHandle: Send + Sync {
    /// Invalidate an approval (same DID written, session/security changed — uds C-2).
    fn invalidate(&self, approval_id: &str, reason: &str) -> Result<(), DomainError>;
}

/// Bounded wait for matching events (hub subscription in the gateway).
#[derive(Debug, Clone, PartialEq)]
pub struct WaitSpec {
    /// Which events.
    pub filter: SubFilter,
    /// Only events after this cursor (`seq:N` = after N, `now` = from the call on).
    pub after: Cursor,
    /// Give up at this instant (`timed_out = true`).
    pub deadline: Instant,
    /// Return as soon as this many matched (≥ 1).
    pub max_events: u32,
}

/// Result of [`CallCtx::wait`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitResult {
    /// Matching events in stream order (≤ `max_events`).
    pub events: Vec<PublishedRecord>,
    /// Continue after this cursor.
    pub cursor: Cursor,
    /// The deadline passed before `max_events` matched.
    pub timed_out: bool,
    /// Stream high-water that was examined (seq) — nothing up to it matched beyond `events`.
    pub scanned_through: u64,
}

/// Borrowed worker handle as a box (default [`CtxServices::worker_for`]).
struct Forward<'a>(&'a dyn WorkerHandle);

impl WorkerHandle for Forward<'_> {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        self.0.call(method, args, deadline)
    }
    fn write<'a>(
        &'a self,
        request_id: &'a str,
        op: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        self.0.write(request_id, op, deadline)
    }
}

fn process_mono_ns() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Daemon storage roots a module may hand to its worker (video artifacts, media sockets). The
/// gateway fills these from `Config.home` and the current journal session; a module must never
/// fall back to `~/.loom` when they are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRoots {
    /// `$LOOM_HOME` root of the running daemon.
    pub home: PathBuf,
    /// Current journal session directory (`records/<date>/s<epoch>`).
    pub session: PathBuf,
}

/// Services behind a [`CallCtx`] (implemented by the gateway, by `loom-testkit` fakes in P0 D5).
pub trait CtxServices: Send + Sync {
    /// Worker of `kind` serving this call's channel.
    fn worker(&self, kind: &str) -> Result<&dyn WorkerHandle, DomainError>;
    /// Publish an event through the hub (publish-before-reply is the core's job).
    fn publish(&self, draft: EventDraft) -> Result<(), DomainError>;
    /// Journal access.
    fn journal(&self) -> &dyn JournalReader;
    /// Approval services.
    fn approvals(&self) -> &dyn ApprovalHandle;

    /// Worker instance serving `channel` (PerChannel workers: started on first use). Default:
    /// [`CtxServices::worker`].
    fn worker_for(&self, kind: &str, channel: &str) -> Result<Box<dyn WorkerHandle + '_>, DomainError> {
        let _ = channel;
        self.worker(kind)
            .map(|w| Box::new(Forward(w)) as Box<dyn WorkerHandle + '_>)
    }
    /// Bounded wait for events. Default: not available.
    fn wait<'a>(&'a self, spec: &'a WaitSpec) -> BoxFuture<'a, Result<WaitResult, DomainError>> {
        let _ = spec;
        Box::pin(async { Err(DomainError::not_implemented("wait")) })
    }
    /// Host monotonic time (ns), same clock as the hub (`ts.mono_ns` of gateway events).
    fn now_mono_ns(&self) -> u64 {
        process_mono_ns()
    }
    /// Run `f` off the async workers; the future resolves when it finished. Default: inline.
    fn run_blocking(&self, f: Box<dyn FnOnce() + Send>) -> BoxFuture<'static, ()> {
        f();
        Box::pin(async {})
    }
    /// The domain's merged `loom.toml [<domain>]` section (`Null` when absent).
    fn config(&self) -> Value {
        Value::Null
    }
    /// Storage roots of the running daemon. Default: unavailable (`None`).
    fn storage_roots(&self) -> Option<StorageRoots> {
        None
    }
    /// Tell the user what the agent is doing (`core.activity` with `action`, the domain, the
    /// call's channel, the read range and the wait predicate). Default: ignored.
    fn activity(
        &self,
        action: &str,
        range: Option<SeqRange>,
        predicate: Option<Value>,
    ) -> Result<(), DomainError> {
        let _ = (action, range, predicate);
        Ok(())
    }
}

/// Per-call context handed to a [`Handler`].
#[derive(Clone)]
pub struct CallCtx {
    role: Role,
    rights: Option<Rights>,
    client_id: String,
    request_id: Option<String>,
    deadline: Instant,
    services: Arc<dyn CtxServices>,
}

impl fmt::Debug for CallCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallCtx")
            .field("role", &self.role)
            .field("rights", &self.rights)
            .field("client_id", &self.client_id)
            .field("request_id", &self.request_id)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl CallCtx {
    /// Built by the gateway (or test harness).
    pub fn new(
        role: Role,
        rights: Option<Rights>,
        client_id: impl Into<String>,
        request_id: Option<String>,
        deadline: Instant,
        services: Arc<dyn CtxServices>,
    ) -> Self {
        CallCtx {
            role,
            rights,
            client_id: client_id.into(),
            request_id,
            deadline,
            services,
        }
    }

    /// Origin of this call (decided by the connection role, never by params).
    pub fn origin(&self) -> Origin {
        self.role.origin()
    }
    /// Connection role.
    pub fn role(&self) -> Role {
        self.role
    }
    /// Viewer rights, if a viewer.
    pub fn rights(&self) -> Option<Rights> {
        self.rights
    }
    /// Client id.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    /// Request id.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
    /// Absolute deadline of the call.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Worker of `kind`.
    pub fn worker(&self, kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        self.services.worker(kind)
    }
    /// Publish an event.
    pub fn publish(&self, draft: EventDraft) -> Result<(), DomainError> {
        self.services.publish(draft)
    }
    /// Journal reader.
    pub fn journal(&self) -> &dyn JournalReader {
        self.services.journal()
    }
    /// Approval services.
    pub fn approvals(&self) -> &dyn ApprovalHandle {
        self.services.approvals()
    }
    /// Worker instance for `channel` (PerChannel kinds get one process per channel).
    pub fn worker_for(&self, kind: &str, channel: &str) -> Result<Box<dyn WorkerHandle + '_>, DomainError> {
        self.services.worker_for(kind, channel)
    }
    /// Wait (bounded) for events matching `spec` — subscription-based, no polling.
    pub fn wait(&self, spec: WaitSpec) -> BoxFuture<'_, Result<WaitResult, DomainError>> {
        Box::pin(async move { self.services.wait(&spec).await })
    }
    /// Host monotonic time (ns) on the hub's clock.
    pub fn now_mono_ns(&self) -> u64 {
        self.services.now_mono_ns()
    }
    /// Run blocking work (file parsing, DB lookups) off the async workers.
    pub fn spawn_blocking<R: Send + 'static>(
        &self,
        f: impl FnOnce() -> R + Send + 'static,
    ) -> BoxFuture<'static, Result<R, DomainError>> {
        let slot = Arc::new(std::sync::Mutex::new(None));
        let s2 = slot.clone();
        let done = self.services.run_blocking(Box::new(move || {
            let r = f();
            *s2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(r);
        }));
        Box::pin(async move {
            done.await;
            slot.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .ok_or_else(|| {
                    DomainError::new(
                        loom_proto::error::ErrorCode::Internal,
                        "blocking task did not finish",
                    )
                })
        })
    }
    /// The domain's merged `loom.toml [<domain>]` section.
    pub fn config(&self) -> Value {
        self.services.config()
    }
    /// Storage roots of the running daemon (`None` when the host provides none).
    pub fn storage_roots(&self) -> Option<StorageRoots> {
        self.services.storage_roots()
    }
    /// Report an agent action (`core.activity`): what, which range was read, what is awaited.
    pub fn activity(
        &self,
        action: &str,
        range: Option<SeqRange>,
        predicate: Option<Value>,
    ) -> Result<(), DomainError> {
        self.services.activity(action, range, predicate)
    }
}

/// RPC command handler.
pub trait Handler: Send + Sync {
    /// Handle one call. Params are already schema-checked; for write commands the pipeline
    /// (classify → policy → reserve → audit → presend) has already run.
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>>;
}
