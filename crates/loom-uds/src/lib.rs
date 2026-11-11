//! Loom UDS domain — gateway piece (P0).
//!
//! * [`module::UdsModule`] — `DomainModule` registration: commands, event kinds, risk table,
//!   write hooks, reducer, help.
//! * [`risk`] — the UDS risk table as data and [`risk::classify_pdu`] (the `classify` hook).
//! * [`hooks`] — `WriteHooks` and the pure dry-run.
//! * [`session::TargetSession`] — what Loom believes about one ECU; source of `uds.state`.
//! * [`payload`] — `uds.tx` / `uds.rx` / `uds.result` / `uds.state` payload types.
//! * [`text`] — request text parser (UI console line, `loom uds raw`).
//!
//! Everything an agent sends to an ECU goes through `loomd` and is published before the reply,
//! so the user sees the same rows in the Loom UI at the same time (02 core principle).

pub mod events;
pub mod evidence;
pub mod handlers;
pub mod hooks;
pub mod lanes;
pub mod manager;
pub mod module;
pub mod payload;
pub mod policy;
pub mod risk;
pub mod session;
pub mod text;

pub use module::UdsModule;

/// Domain name.
pub const DOMAIN: &str = "uds";
/// Request handed to the transport.
pub const KIND_TX: &str = "uds.tx";
/// Row update while 0x78 pendings arrive (`write_state = sent`, `pending_count`).
pub const KIND_PENDING: &str = "uds.pending";
/// One received PDU (pendings, late responses included). No `write_state`: never touches a row.
pub const KIND_RX: &str = "uds.rx";
/// Final state of a request row.
pub const KIND_RESULT: &str = "uds.result";
/// Target state (session, security, link, lease).
pub const KIND_STATE: &str = "uds.state";

/// The DoIP entity and virtual-ECU crates behind `connect --sim`, re-exported so gateway
/// integration tests can put a slow or faulty ECU behind a real loopback DoIP entity (reached
/// with `uds.connect {host, port}`) without a manifest change. Not an API for domain code.
#[doc(hidden)]
pub mod sim_support {
    pub use loom_uds_doip;
    pub use loom_uds_sim;
}
