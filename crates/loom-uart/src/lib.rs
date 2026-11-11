//! `loom-uart` — the UART domain's gateway piece (UART plan §3.2, contract 08 §8.1).
//!
//! Runs inside `loomd`. It owns no device: every device operation is forwarded to the
//! per-port `uart` worker (`loom-uart-worker`). What lives here: command specs, event kinds
//! (payload schemas = the UI panel contract), the risk table and write hooks, a channel
//! mirror reducer for late-joining viewers, and the `help --json` source (C8).

#[cfg(feature = "gateway")]
pub mod classify;
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "gateway")]
pub mod commands;
#[cfg(feature = "gateway")]
pub mod evidence;
#[cfg(feature = "gateway")]
pub mod hooks;
#[cfg(feature = "gateway")]
pub mod kinds;
#[cfg(feature = "gateway")]
pub mod mirror;
#[cfg(feature = "gateway")]
pub mod module;

#[cfg(feature = "gateway")]
pub use module::Uart;

/// Worker piece: what `loomd worker uart --instance <channel>` runs (one process per port).
#[cfg(feature = "worker")]
pub mod worker {
    pub use loom_uart_worker::worker::UartWorker;
}

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "uart";
/// Worker kind (`loomd worker uart --instance <channel>`).
pub const WORKER_KIND: &str = "uart";
/// Most UART worker processes (one per port, plan §3.13, DR-V13).
pub const MAX_PORTS: u16 = 8;
