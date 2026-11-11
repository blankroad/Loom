//! Clocks. Hot paths take timestamps through [`Clock`] so simulations can drive time.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use loom_proto::event::{Ts, TsSource};
use loom_proto::num::U64;

/// Time source for event timestamps.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Monotonic nanoseconds.
    fn mono_ns(&self) -> u64;
    /// Wall-clock nanoseconds since the Unix epoch (UTC).
    fn wall_ns(&self) -> u64;
    /// Monotonic clock domain id.
    fn clock_id(&self) -> &str;
    /// Build an event timestamp.
    fn ts(&self, source: TsSource) -> Ts {
        Ts {
            mono_ns: U64(self.mono_ns()),
            wall_ns: U64(self.wall_ns()),
            clock_id: self.clock_id().to_owned(),
            source,
            quality: None,
        }
    }
}

fn wall_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// System clock.
///
/// **P0 limitation:** `mono_ns` is measured from this clock's creation (`std::time::Instant` has
/// no portable absolute value), so it is only comparable within one process; `clock_id` says so
/// (`proc-<pid>`). P1 switches to the host-wide monotonic clock (QPC / `CLOCK_MONOTONIC` /
/// `mach_absolute_time`) so gateway and workers share one time base.
#[derive(Debug)]
pub struct SystemClock {
    base: Instant,
    clock_id: String,
}

impl SystemClock {
    /// New clock.
    pub fn new() -> Self {
        SystemClock {
            base: Instant::now(),
            clock_id: format!("proc-{}", std::process::id()),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn mono_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    fn wall_ns(&self) -> u64 {
        wall_now_ns()
    }
    fn clock_id(&self) -> &str {
        &self.clock_id
    }
}

/// Simulated clock: time moves only when [`SimClock::advance_ns`] is called. Clones share time.
#[derive(Debug, Clone)]
pub struct SimClock {
    mono: Arc<AtomicU64>,
    wall_base_ns: u64,
}

impl SimClock {
    /// Start at `mono_ns = 0` with the given wall time.
    pub fn new(wall_base_ns: u64) -> Self {
        SimClock {
            mono: Arc::new(AtomicU64::new(0)),
            wall_base_ns,
        }
    }

    /// Advance simulated time.
    pub fn advance_ns(&self, ns: u64) {
        self.mono.fetch_add(ns, Ordering::AcqRel);
    }
}

impl Clock for SimClock {
    fn mono_ns(&self) -> u64 {
        self.mono.load(Ordering::Acquire)
    }
    fn wall_ns(&self) -> u64 {
        self.wall_base_ns.saturating_add(self.mono_ns())
    }
    fn clock_id(&self) -> &str {
        "sim"
    }
}
