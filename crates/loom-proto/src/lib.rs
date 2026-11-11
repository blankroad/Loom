//! Loom contract v1 types (`docs/contract/`). This crate is the single source of truth (C3):
//! Rust types -> JSON Schema -> TS/Python. No tokio, no I/O.
//!
//! Module owners inside CORE: see `docs/impl/core/tasks/`.

pub mod bytes;
pub mod canonical;
pub mod core_events;
pub mod cursor;
pub mod envelope;
pub mod error;
pub mod event;
pub mod evidence;
pub mod num;
pub mod policy;
pub mod rpc;
pub mod schema;
pub mod stream;
pub mod worker;

/// Contract version carried in `core.hello` (`proto: "loom/1.0"`).
pub const PROTO_MAJOR: u16 = 1;
pub const PROTO_MINOR: u16 = 0;
/// `schema_version` field of every event record and CLI envelope.
pub const SCHEMA_VERSION: u16 = 1;
