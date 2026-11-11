//! `loom-console` — the shared console engine (UART plan §3.2, A4).
//!
//! No device I/O and no async runtime: the UART worker, the web panel (WASM build of
//! [`sanitize`]) and later SSH/ADB/RTT consoles all use the same code.
//!
//! Ownership inside the team (docs/impl/uart/tasks/p0-common.md):
//! - [`ring`], [`cursor`], [`text`], [`matcher`] — T05/T06/T09 matcher.
//! - [`sanitize`], [`arbiter`] — T07/T07b/T10 core state machine.

pub mod arbiter;
pub mod cursor;
#[cfg(feature = "matcher")]
pub mod matcher;
pub mod ring;
pub mod sanitize;
pub mod text;
