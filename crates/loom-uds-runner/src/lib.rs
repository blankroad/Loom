//! UDS transaction runner for Loom (plan U-06, D2 gate).
//!
//! * [`Transport`] — one diagnostic target on one link, dyn-compatible via [`BoxFuture`].
//!   Implementations: [`DoipTransport`] (over `loom-uds-doip`) and [`FakeTransport`] (scripted,
//!   deterministic under tokio's paused clock).
//! * [`Runner`] — one request at a time: P2 first response, `0x78` → P2* with a bounded count,
//!   total deadline, generation change → `outcome_unknown`, SPRMIB → `ok_suppressed`.
//! * [`Unresolved`] — late-response quarantine U (SID overlap, evidence-only release).
//! * [`RunnerHandle`] — per-target serial queue (`MAX_PENDING_PER_TARGET`, full → `E_BUSY`).
//!
//! `write_state` mapping (contract 02 + P0 lead decision): handed to the socket = `sent`, ECU
//! positive response = `confirmed`, NRC = `failed` + `nrc`, read timeout = `failed` +
//! `timeout`, write timeout or link loss after sending = `outcome_unknown` (never re-sent).
//! Read-back verification (`effective_verified`) belongs to the domain.

mod doip;
mod fake;
mod handle;
pub mod quarantine;
mod runner;
mod transport;

pub use doip::DoipTransport;
pub use fake::{FakeAction, FakeControl, FakeStep, FakeTransport, MAX_FAKE_LOG, MAX_FAKE_SCHEDULED};
pub use handle::{MAX_PENDING_PER_TARGET, RunnerHandle};
pub use quarantine::{MAX_UNRESOLVED_PER_TARGET, ReleaseCause, Unresolved, UnresolvedEntry};
pub use runner::{
    Access, DEFAULT_COMPLETION_LIMIT, DEFAULT_MAX_RESPONSE_PENDING, DEFAULT_P2, DEFAULT_P2_STAR,
    DEFAULT_QUARANTINE_QUIET, MAX_PROBE_CANDIDATES, MAX_RESPONSE_PENDING_HARD, MonoClock, NoopObserver,
    RejectReason, Runner, RunnerConfig, RunnerEvent, RunnerObserver, TimeoutPhase, TxnRequest, TxnResult,
    TxnStatus, TxnTimes,
};
pub use transport::{BoxFuture, Incoming, RecvError, Transport, TransportError};
