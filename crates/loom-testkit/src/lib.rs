//! Test kit for domain crates (contract v1 §8.4, `core-p0-d5`).
//!
//! * [`FakeWorker`] — drive a [`loom_worker_sdk::Worker`] in-process with a simulated clock and a
//!   collecting sink.
//! * [`RegistryProbe`] — run a [`loom_domain_api::DomainModule`]'s `register` and check it.
//! * [`HookHarness`] — call write hooks with owned fixture requests and time them (1 ms warn /
//!   10 ms fail).
//! * [`fixtures`] — the contract golden fixtures of `loom-proto` (embedded, not copied).
//! * [`asserts`] — write-row order, publish-before-reply trace shape, honest gaps.
//!
//! This kit does **not** reproduce the product hub or policy engine; those are tested on the real
//! `loomd` from P1.

pub mod asserts;
pub mod fake_worker;
pub mod fixtures;
pub mod hooks;
pub mod probe;

pub use asserts::{TraceItem, assert_no_gap_lies, assert_publish_before_reply, assert_write_rows};
pub use fake_worker::FakeWorker;
pub use hooks::{HookHarness, OwnedWriteRequest, Timed};
pub use probe::{ProbeReport, RegistryProbe};
