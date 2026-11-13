//! UDS transaction runner for Loom (plan U-06, D2 gate).
//!
//! * [`Transport`] — one diagnostic target on one link, dyn-compatible via [`BoxFuture`].
//!   Implementations: `DoipTransport` (feature `doip`, over `loom-uds-doip`) and [`FakeTransport`]
//!   (scripted, deterministic under tokio's paused clock).
//! * [`Connector`] / [`Link`] — transport-neutral link lifecycle (connect, reconnect with a new
//!   generation, link-loss signals, owned resources). `DoipConnector` (feature `doip`) is the DoIP
//!   implementation; CAN-TP or vendor Ethernet diagnostics attach as further connectors.
//! * [`Runner`] — one request at a time: P2 first response, `0x78` → P2* with a bounded count,
//!   total deadline, generation change → `outcome_unknown`, SPRMIB → `ok_suppressed`.
//! * [`Unresolved`] — late-response quarantine U (SID overlap, evidence-only release).
//! * [`RunnerHandle`] — per-target serial queues by [`Priority`] (abort > user > agent, each bounded,
//!   full → `E_BUSY`) and [`RunnerHandle::abort`] (waiting → `failed`, in flight → wait stopped,
//!   response quarantined).
//!
//! `write_state` mapping (contract 02 + P0 lead decision): handed to the socket = `sent`, ECU
//! positive response = `confirmed`, NRC = `failed` + `nrc`, read timeout = `failed` +
//! `timeout`, write timeout or link loss after sending = `outcome_unknown` (never re-sent).
//! Read-back verification (`effective_verified`) belongs to the domain.

mod abort;
#[cfg(feature = "doip")]
mod doip;
mod fake;
mod handle;
mod link;
pub mod quarantine;
mod runner;
mod transport;

pub use abort::{AbortKind, AbortScope, AbortSignal, Priority};
#[cfg(feature = "doip")]
pub use doip::{DoipConnector, DoipTransport};
pub use fake::{FakeAction, FakeControl, FakeStep, FakeTransport, MAX_FAKE_LOG, MAX_FAKE_SCHEDULED};
pub use handle::{
    AbortSummary, MAX_PENDING_ABORT, MAX_PENDING_ABORT_BYTES, MAX_PENDING_AGENT_BYTES,
    MAX_PENDING_PER_TARGET, MAX_PENDING_USER, MAX_PENDING_USER_BYTES, MAX_RETAINED_TARGET_BYTES, QueueDepth,
    RunnerHandle,
};
pub use link::{ConnectError, Connector, Link, LinkEvents, LinkSignal};
pub use quarantine::{MAX_UNRESOLVED_PER_TARGET, ReleaseCause, Unresolved, UnresolvedEntry};
pub use runner::{
    Access, DEFAULT_COMPLETION_LIMIT, DEFAULT_MAX_RESPONSE_PENDING, DEFAULT_P2, DEFAULT_P2_STAR,
    DEFAULT_QUARANTINE_QUIET, MAX_PROBE_CANDIDATES, MAX_REQUEST_ID_BYTES, MAX_RESPONSE_PENDING_HARD,
    MonoClock, NoopObserver, READ_RETRY_BUSY_DELAY, RejectReason, Runner, RunnerConfig, RunnerEvent,
    RunnerObserver, TimeoutPhase, TxnRequest, TxnResult, TxnStatus, TxnTimes,
};
pub use transport::{BoxFuture, Incoming, RecvError, Transport, TransportError};
