//! `loom-logic` — logic-analyzer domain (CONTRACT: `work/reports/logic-live/CONTRACT.md`).
//!
//! The worker owns one external `sigrok-cli` acquisition process (or a Loom-generated synthetic
//! waveform, chosen explicitly), compresses the raw 1-byte-per-sample stream into bounded
//! `logic.edges` events and tees the same bytes into a second `sigrok-cli` running a real
//! libsigrok protocol decoder whose annotations become `logic.annotations` events. Everything
//! lands in the daemon journal before STOP, so the CLI agent and the UI read the same records.
//!
//! Pieces: [`model`] (validation, argv, edge/annotation codecs, synthetic generator — pure),
//! [`gateway`] (registration, read/wait, reducer), [`worker`] (process supervisor), [`cli`].

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "gateway")]
pub mod gateway;
pub mod model;
#[cfg(feature = "worker")]
pub mod worker;

/// Domain name (RPC prefix, CLI group, event `domain`).
pub const DOMAIN: &str = "logic";
/// Worker kind.
pub const WORKER_KIND: &str = "logic";

/// Capture lifecycle (priority lane).
pub const KIND_CAPTURE_STATE: &str = "logic.capture_state";
/// Raw transitions (data lane).
pub const KIND_EDGES: &str = "logic.edges";
/// Single decoder annotation (accepted by readers; the worker emits batches).
pub const KIND_ANNOTATION: &str = "logic.annotation";
/// Batched decoder annotations (data lane).
pub const KIND_ANNOTATIONS: &str = "logic.annotations";
