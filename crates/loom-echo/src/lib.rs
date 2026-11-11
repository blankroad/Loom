//! `loom-echo` — the sample domain. A simulated loopback device: whatever is sent comes back as RX.
//!
//! **Domain teams: copy this crate and replace the parts marked `DOMAIN:`.** The three pieces are
//! [`gateway`] (registration, hooks, reducer — runs in `loomd`), [`worker`] (device owner — runs
//! in `loomd worker echo`) and [`cli`] (the `loom echo …` fragment linked into `loom`).

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "gateway")]
pub mod gateway;
#[cfg(feature = "worker")]
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`). DOMAIN: your domain name.
pub const DOMAIN: &str = "echo";
/// Worker kind. DOMAIN: your worker kind.
pub const WORKER_KIND: &str = "echo";
/// RX event kind.
pub const KIND_DATA: &str = "echo.data";
/// Write-row event kind (`payload.write_state`, E8).
pub const KIND_WRITE: &str = "echo.write";
/// An input lease ended (viewer gone / released) — reported by the worker.
pub const KIND_LEASE_END: &str = "echo.lease_end";
