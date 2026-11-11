//! `bridge` — bounded reader → channel-processing hand-off (UART plan §3.5, §3.13 first row).
//!
//! Byte **and** count limits at once. The producer (reader thread) never waits: when the bridge
//! is full it drops the **oldest undelivered** chunks and adds the dropped range (rx offsets,
//! byte count, source_seq range) to one fixed slot. The consumer receives that range as
//! [`BridgeItem::Lost`] before any later data (→ `record.gap` + `display.gap`,
//! cause `reader_bridge_overflow`; not `source.gap`, the host did read those bytes).
//!
//! Wakeups are coalesced: the producer signals the condvar only while the consumer is parked,
//! so a busy consumer is not woken per chunk.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Bridge payload limit (§3.13 row "reader→채널 reducer": 1 MiB or 256 chunks).
pub const BRIDGE_MAX_BYTES: usize = 1024 * 1024;
/// Bridge chunk-count limit (§3.13: 256 chunks).
pub const BRIDGE_MAX_CHUNKS: usize = 256;
/// Per-chunk metadata charged against [`BRIDGE_MAX_BYTES`] besides the payload
/// (§3.13: limits count metadata too, not payload only).
pub const BRIDGE_CHUNK_OVERHEAD: usize = 64;
/// `cause` of overflow losses (§3.13).
pub const CAUSE_READER_BRIDGE_OVERFLOW: &str = "reader_bridge_overflow";

/// One chunk read from the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RxChunk {
    /// Local hand-off id (shared with TX chunks). The worker replaces it with an Emitter
    /// reservation before ring ingestion; it is not a producer event id while in this bridge.
    pub source_seq: u64,
    /// Offset of `bytes[0]` in the port's RX byte stream.
    pub rx_offset: u64,
    /// Host monotonic time when the read completed (`ts.source = host_read_complete`).
    pub mono_ns: u64,
    /// Host wall time when the read completed.
    pub wall_ns: u64,
    /// Raw bytes, never modified.
    pub bytes: Vec<u8>,
}

impl RxChunk {
    fn cost(&self) -> usize {
        self.bytes.len() + BRIDGE_CHUNK_OVERHEAD
    }
}

/// A contiguous range of RX data dropped inside the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LostRange {
    /// First lost rx offset (inclusive).
    pub rx_from: u64,
    /// End of the lost range (exclusive).
    pub rx_to: u64,
    /// Lost payload bytes (`rx_to - rx_from`).
    pub bytes: u64,
    /// First lost source_seq (inclusive).
    pub seq_from: u64,
    /// Last lost source_seq (inclusive). Not every seq in between is an RX chunk: TX chunks share
    /// the sequence space.
    pub seq_to: u64,
    /// Number of dropped chunks.
    pub chunks: u64,
    /// Always [`CAUSE_READER_BRIDGE_OVERFLOW`] for now.
    pub cause: &'static str,
}

impl LostRange {
    fn of(c: &RxChunk) -> Self {
        let len = c.bytes.len() as u64;
        Self {
            rx_from: c.rx_offset,
            rx_to: c.rx_offset + len,
            bytes: len,
            seq_from: c.source_seq,
            seq_to: c.source_seq,
            chunks: 1,
            cause: CAUSE_READER_BRIDGE_OVERFLOW,
        }
    }

    fn merge(&mut self, c: &RxChunk) {
        let len = c.bytes.len() as u64;
        self.rx_from = self.rx_from.min(c.rx_offset);
        self.rx_to = self.rx_to.max(c.rx_offset + len);
        self.bytes += len;
        self.seq_from = self.seq_from.min(c.source_seq);
        self.seq_to = self.seq_to.max(c.source_seq);
        self.chunks += 1;
    }
}

/// What the consumer receives, in stream order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeItem {
    /// Data.
    Data(RxChunk),
    /// Data dropped by overflow (precedes every later `Data`).
    Lost(LostRange),
    /// The reader hit an I/O error and stopped; delivered after all queued data.
    Disconnected(io::ErrorKind),
}

/// Why [`BridgeRx::pop`] returned nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopError {
    /// Nothing arrived within the timeout.
    Timeout,
    /// The producer is gone and everything was delivered.
    Closed,
}

/// Counters for tests and status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BridgeStats {
    /// Chunks pushed by the producer.
    pub pushed_chunks: u64,
    /// Payload bytes pushed by the producer.
    pub pushed_bytes: u64,
    /// Chunks dropped by overflow.
    pub dropped_chunks: u64,
    /// Payload bytes dropped by overflow.
    pub dropped_bytes: u64,
    /// Condvar notifications actually sent (coalescing evidence).
    pub wakeups: u64,
}

#[derive(Debug, Default)]
struct State {
    queue: VecDeque<RxChunk>,
    cost: usize,
    lost: Option<LostRange>,
    terminal: Option<io::ErrorKind>,
    consumer_waiting: bool,
    producer_closed: bool,
    stats: BridgeStats,
}

#[derive(Debug, Default)]
struct Inner {
    state: Mutex<State>,
    cv: Condvar,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn wake(&self, st: &mut State) {
        if st.consumer_waiting {
            st.consumer_waiting = false;
            st.stats.wakeups += 1;
            self.cv.notify_one();
        }
    }
}

/// Creates a bridge with the §3.13 limits.
pub fn bridge() -> (BridgeTx, BridgeRx) {
    let inner = Arc::new(Inner::default());
    (
        BridgeTx {
            inner: Arc::clone(&inner),
        },
        BridgeRx { inner },
    )
}

/// Producer side (reader thread). Dropping it closes the bridge.
#[derive(Debug)]
pub struct BridgeTx {
    inner: Arc<Inner>,
}

impl BridgeTx {
    /// Queues a chunk without waiting; drops the oldest undelivered chunks if needed.
    /// Returns the number of chunks dropped by this push.
    pub fn push(&self, chunk: RxChunk) -> usize {
        let mut st = self.inner.lock();
        st.stats.pushed_chunks += 1;
        st.stats.pushed_bytes += chunk.bytes.len() as u64;
        let mut dropped = 0usize;
        while (st.cost + chunk.cost() > BRIDGE_MAX_BYTES || st.queue.len() >= BRIDGE_MAX_CHUNKS)
            && let Some(old) = st.queue.pop_front()
        {
            st.cost -= old.cost();
            record_loss(&mut st, &old);
            dropped += 1;
        }
        if chunk.cost() > BRIDGE_MAX_BYTES {
            // Cannot happen with ≤16 KiB reads; account rather than exceed the limit.
            record_loss(&mut st, &chunk);
            dropped += 1;
        } else {
            st.cost += chunk.cost();
            st.queue.push_back(chunk);
        }
        self.inner.wake(&mut st);
        dropped
    }

    /// Reports a terminal I/O error (delivered after queued data).
    pub fn disconnect(&self, kind: io::ErrorKind) {
        let mut st = self.inner.lock();
        st.terminal = Some(kind);
        self.inner.wake(&mut st);
    }
}

fn record_loss(st: &mut State, c: &RxChunk) {
    st.stats.dropped_chunks += 1;
    st.stats.dropped_bytes += c.bytes.len() as u64;
    match st.lost.as_mut() {
        Some(l) => l.merge(c),
        None => st.lost = Some(LostRange::of(c)),
    }
}

impl Drop for BridgeTx {
    fn drop(&mut self) {
        let mut st = self.inner.lock();
        st.producer_closed = true;
        st.consumer_waiting = false;
        self.inner.cv.notify_all();
    }
}

/// Consumer side (channel processing thread).
#[derive(Debug)]
pub struct BridgeRx {
    inner: Arc<Inner>,
}

impl BridgeRx {
    /// Takes the next item, waiting up to `timeout`.
    pub fn pop(&self, timeout: Duration) -> Result<BridgeItem, PopError> {
        let deadline = Instant::now() + timeout;
        let mut st = self.inner.lock();
        loop {
            if let Some(item) = take_one(&mut st) {
                return Ok(item);
            }
            if st.producer_closed {
                return Err(PopError::Closed);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                st.consumer_waiting = false;
                return Err(PopError::Timeout);
            }
            st.consumer_waiting = true;
            st = self
                .inner
                .cv
                .wait_timeout(st, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    /// Waits up to `timeout` for at least one item, then moves up to `max` items into `out`
    /// under a single lock (one wakeup per batch). Returns the number moved.
    pub fn pop_batch(
        &self,
        out: &mut Vec<BridgeItem>,
        max: usize,
        timeout: Duration,
    ) -> Result<usize, PopError> {
        let first = self.pop(timeout)?;
        out.push(first);
        let mut n = 1;
        let mut st = self.inner.lock();
        while n < max {
            match take_one(&mut st) {
                Some(item) => {
                    out.push(item);
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }

    /// Bytes currently charged (payload + per-chunk overhead).
    pub fn queued_cost(&self) -> usize {
        self.inner.lock().cost
    }

    /// Counters.
    pub fn stats(&self) -> BridgeStats {
        self.inner.lock().stats
    }
}

fn take_one(st: &mut State) -> Option<BridgeItem> {
    if let Some(l) = st.lost.take() {
        return Some(BridgeItem::Lost(l));
    }
    if let Some(c) = st.queue.pop_front() {
        st.cost -= c.cost();
        return Some(BridgeItem::Data(c));
    }
    st.terminal.take().map(BridgeItem::Disconnected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn chunk(seq: u64, off: u64, len: usize) -> RxChunk {
        RxChunk {
            source_seq: seq,
            rx_offset: off,
            mono_ns: seq,
            wall_ns: seq,
            bytes: vec![b'x'; len],
        }
    }

    #[test]
    fn count_limit_drops_oldest_and_reports_range() {
        let (tx, rx) = bridge();
        for i in 0..(BRIDGE_MAX_CHUNKS as u64 + 10) {
            tx.push(chunk(i, i * 10, 10));
        }
        assert_eq!(
            rx.pop(Duration::ZERO),
            Ok(BridgeItem::Lost(LostRange {
                rx_from: 0,
                rx_to: 100,
                bytes: 100,
                seq_from: 0,
                seq_to: 9,
                chunks: 10,
                cause: CAUSE_READER_BRIDGE_OVERFLOW,
            }))
        );
        match rx.pop(Duration::ZERO) {
            Ok(BridgeItem::Data(c)) => assert_eq!((c.source_seq, c.rx_offset), (10, 100)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn byte_limit_drops_oldest() {
        let (tx, rx) = bridge();
        let len = 16 * 1024;
        let fit = BRIDGE_MAX_BYTES / (len + BRIDGE_CHUNK_OVERHEAD);
        for i in 0..(fit as u64 + 1) {
            tx.push(chunk(i, i * len as u64, len));
        }
        assert!(rx.queued_cost() <= BRIDGE_MAX_BYTES);
        match rx.pop(Duration::ZERO) {
            Ok(BridgeItem::Lost(l)) => {
                assert_eq!((l.seq_from, l.seq_to, l.rx_from), (0, 0, 0));
                assert_eq!(l.bytes, len as u64);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn disconnect_after_data_then_closed() {
        let (tx, rx) = bridge();
        tx.push(chunk(0, 0, 3));
        tx.disconnect(io::ErrorKind::BrokenPipe);
        drop(tx);
        assert!(matches!(rx.pop(Duration::ZERO), Ok(BridgeItem::Data(_))));
        assert_eq!(
            rx.pop(Duration::ZERO),
            Ok(BridgeItem::Disconnected(io::ErrorKind::BrokenPipe))
        );
        assert_eq!(rx.pop(Duration::from_millis(5)), Err(PopError::Closed));
    }

    #[test]
    fn timeout_when_empty() {
        let (_tx, rx) = bridge();
        let t = Instant::now();
        assert_eq!(rx.pop(Duration::from_millis(20)), Err(PopError::Timeout));
        assert!(t.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn wakeups_are_coalesced() {
        let (tx, rx) = bridge();
        // Consumer not waiting: pushes do not notify.
        for i in 0..100 {
            tx.push(chunk(i, i, 1));
        }
        assert_eq!(rx.stats().wakeups, 0);
        let mut out = Vec::new();
        assert_eq!(rx.pop_batch(&mut out, 1000, Duration::ZERO), Ok(100));
        // Parked consumer gets exactly one wakeup for a push.
        let h = std::thread::spawn(move || {
            let item = rx.pop(Duration::from_secs(2));
            (item, rx.stats().wakeups)
        });
        std::thread::sleep(Duration::from_millis(30));
        tx.push(chunk(100, 100, 1));
        let (item, wakeups) = h.join().expect("consumer thread");
        assert!(matches!(item, Ok(BridgeItem::Data(_))));
        assert_eq!(wakeups, 1);
    }

    proptest! {
        /// Whatever the push/pop interleaving, delivered data + lost ranges tile the stream
        /// exactly once and in order, and limits always hold.
        #[test]
        fn delivered_plus_lost_tiles_the_stream(ops in proptest::collection::vec((1usize..20_000, 0usize..4), 1..400)) {
            let (tx, rx) = bridge();
            let mut off = 0u64;
            let mut next_expected = 0u64;
            let check = |item: BridgeItem, next: &mut u64| match item {
                BridgeItem::Data(c) => { assert_eq!(c.rx_offset, *next); *next += c.bytes.len() as u64; }
                BridgeItem::Lost(l) => { assert_eq!(l.rx_from, *next); assert_eq!(l.rx_to - l.rx_from, l.bytes); *next = l.rx_to; }
                BridgeItem::Disconnected(_) => unreachable!(),
            };
            for (seq, (len, pops)) in (0u64..).zip(ops) {
                tx.push(chunk(seq, off, len));
                off += len as u64;
                prop_assert!(rx.queued_cost() <= BRIDGE_MAX_BYTES);
                for _ in 0..pops {
                    if let Ok(item) = rx.pop(Duration::ZERO) { check(item, &mut next_expected); }
                }
            }
            drop(tx);
            while let Ok(item) = rx.pop(Duration::ZERO) { check(item, &mut next_expected); }
            prop_assert_eq!(next_expected, off);
        }
    }
}
