//! Time source. The sim never reads the OS clock directly, so tests are deterministic.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Monotonic nanoseconds. Must never go backwards.
pub trait Clock: Send {
    fn now_ns(&self) -> u64;
}

/// Real monotonic clock (ns since creation).
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos() as u64
    }
}

/// Test clock moved by hand. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    ns: Arc<AtomicU64>,
}

impl ManualClock {
    pub fn new(start_ns: u64) -> Self {
        Self {
            ns: Arc::new(AtomicU64::new(start_ns)),
        }
    }

    /// Moves forward by `ns`.
    pub fn advance_ns(&self, ns: u64) {
        self.ns.fetch_add(ns, Ordering::SeqCst);
    }

    pub fn advance_ms(&self, ms: u64) {
        self.advance_ns(ms * 1_000_000);
    }

    /// Sets the time; ignored if it would go backwards.
    pub fn set_ns(&self, ns: u64) {
        self.ns.fetch_max(ns, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ns(&self) -> u64 {
        self.ns.load(Ordering::SeqCst)
    }
}
