//! `loom-eth` — the Ethernet packet domain (tshark for the agent, Wireshark for the user).
//!
//! Three pieces as in the CORE contract (§8.1): [`gateway`] (registration, queries, hooks,
//! reducer — runs in `loomd`), [`worker`] (capture pipeline — runs in the eth worker process)
//! and [`cli`] (`loom eth …`). The payload model shared by all three is [`model`]
//! (schema v1, `docs/impl/eth/schema-v1.md`).

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "runtime")]
pub mod gateway;
pub mod model;
#[cfg(feature = "runtime")]
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "eth";
/// Worker kind (one instance per capture channel, PerChannel ≤ 4).
pub const WORKER_KIND: &str = "eth";
/// Maximum concurrent capture channels (worker instances).
pub const MAX_CHANNELS: u32 = 4;
