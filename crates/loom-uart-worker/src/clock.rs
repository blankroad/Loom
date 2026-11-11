//! `clock` — injectable time source for RX/TX chunk stamps (UART plan §3.5, D3).
//!
//! `mono_ns` is a monotonic nanosecond counter relative to the clock's own origin (P1 maps it to
//! the CORE `clock_id`); `wall_ns` is UNIX-epoch nanoseconds from [`std::time::SystemTime`].
//! Tests inject [`FixedClock`] to get deterministic stamps.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Time source used by the reader thread and the TX actor.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Monotonic nanoseconds since this clock's origin.
    fn mono_ns(&self) -> u64;
    /// Wall-clock nanoseconds since the UNIX epoch (0 if the system clock is before 1970).
    fn wall_ns(&self) -> u64;
}

/// Production clock: [`Instant`] for mono, [`SystemTime`] for wall.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// Creates a clock whose `mono_ns` origin is "now".
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
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
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn wall_ns(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

/// Test clock with values set explicitly (never advances by itself).
#[derive(Debug, Default)]
pub struct FixedClock {
    mono: AtomicU64,
    wall: AtomicU64,
}

impl FixedClock {
    /// Creates a clock reporting the given values.
    pub fn new(mono_ns: u64, wall_ns: u64) -> Self {
        Self {
            mono: AtomicU64::new(mono_ns),
            wall: AtomicU64::new(wall_ns),
        }
    }

    /// Sets both values.
    pub fn set(&self, mono_ns: u64, wall_ns: u64) {
        self.mono.store(mono_ns, Ordering::SeqCst);
        self.wall.store(wall_ns, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn mono_ns(&self) -> u64 {
        self.mono.load(Ordering::SeqCst)
    }

    fn wall_ns(&self) -> u64 {
        self.wall.load(Ordering::SeqCst)
    }
}

/// Local chunk allocator shared by reader and TX actor (UART plan §3.5).
/// The R1 worker adapter replaces these hand-off ids with an Emitter reservation after RX
/// coalescing; only that reserved id is an event/ring `source_seq`.
#[derive(Debug, Default)]
pub struct SeqSource {
    next: AtomicU64,
}

impl SeqSource {
    /// Starts allocating at `first`.
    pub fn starting_at(first: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
        }
    }

    /// Allocates the next sequence number.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    /// Allocates `n` consecutive numbers and returns the first one.
    pub fn reserve(&self, n: u64) -> u64 {
        self.next.fetch_add(n, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_reports_set_values() {
        let c = FixedClock::new(5, 7);
        assert_eq!((c.mono_ns(), c.wall_ns()), (5, 7));
        c.set(9, 11);
        assert_eq!((c.mono_ns(), c.wall_ns()), (9, 11));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::new();
        let a = c.mono_ns();
        let b = c.mono_ns();
        assert!(b >= a);
        assert!(c.wall_ns() > 1_600_000_000_000_000_000);
    }

    #[test]
    fn seq_source_allocates_in_order() {
        let s = SeqSource::starting_at(10);
        assert_eq!(s.next(), 10);
        assert_eq!(s.reserve(3), 11);
        assert_eq!(s.next(), 14);
    }
}
