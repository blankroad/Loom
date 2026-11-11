//! Bounded writer input queue (plan §9.2 Q2/Q18). No queue in this crate is unbounded.
//!
//! Every queue has an item cap and a byte cap, and may also draw from a shared
//! [`QueueBudget`] (the "worker total 16 MiB / 32k items" of Q2). Overflow policy:
//! * [`BoundedQueue::try_push`] never blocks; when a cap is hit it returns `Full` at once and the
//!   caller records the loss (`record.gap`, health `failed`) — journal data path.
//! * [`BoundedQueue::push_timeout`] waits up to its timeout for room, then returns `Full`; the
//!   caller must slow down (backpressure) and retry — mapping log, which is never dropped.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Default shared budget of all journal queues of one worker: 16 MiB.
pub const WORKER_BUDGET_BYTES: u64 = 16 * 1024 * 1024;
/// Default shared budget of all journal queues of one worker: 32k items.
pub const WORKER_BUDGET_ITEMS: u64 = 32 * 1024;

/// Byte/item budget shared by several writer queues of one process.
#[derive(Debug)]
pub struct QueueBudget {
    max_bytes: u64,
    max_items: u64,
    bytes: AtomicU64,
    items: AtomicU64,
}

impl QueueBudget {
    /// New budget.
    pub fn new(max_bytes: u64, max_items: u64) -> Arc<Self> {
        Arc::new(QueueBudget {
            max_bytes,
            max_items,
            bytes: AtomicU64::new(0),
            items: AtomicU64::new(0),
        })
    }

    /// Worker default (16 MiB / 32k items).
    pub fn worker_default() -> Arc<Self> {
        Self::new(WORKER_BUDGET_BYTES, WORKER_BUDGET_ITEMS)
    }

    /// `(bytes, items)` currently reserved.
    pub fn used(&self) -> (u64, u64) {
        (
            self.bytes.load(Ordering::Acquire),
            self.items.load(Ordering::Acquire),
        )
    }

    /// Reserve; on overflow nothing stays reserved. Two racing reservations may both fail
    /// near the cap (conservative, never over-commits).
    fn try_reserve(&self, bytes: u64) -> bool {
        let b = self
            .bytes
            .fetch_add(bytes, Ordering::AcqRel)
            .saturating_add(bytes);
        let i = self.items.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        if b > self.max_bytes || i > self.max_items {
            self.release(bytes);
            false
        } else {
            true
        }
    }

    fn release(&self, bytes: u64) {
        self.bytes.fetch_sub(bytes, Ordering::AcqRel);
        self.items.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) enum PushError<T> {
    Full(T),
    Closed(T),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct QueueStats {
    pub depth: usize,
    pub bytes: u64,
    pub max_depth: usize,
    pub max_items: usize,
    pub max_bytes: u64,
}

struct State<T> {
    q: VecDeque<(T, u64, Option<String>)>,
    classes: HashMap<String, u64>,
    bytes: u64,
    closed: bool,
    wake: bool,
    max_depth: usize,
}

/// Multi-producer, single-consumer queue with item and byte caps.
pub(crate) struct BoundedQueue<T> {
    st: Mutex<State<T>>,
    not_empty: Condvar,
    not_full: Condvar,
    max_items: usize,
    max_bytes: u64,
    budget: Option<Arc<QueueBudget>>,
    class_bytes: Option<u64>,
}

impl<T> BoundedQueue<T> {
    pub(crate) fn new(max_items: usize, max_bytes: u64, budget: Option<Arc<QueueBudget>>) -> Self {
        BoundedQueue {
            st: Mutex::new(State {
                q: VecDeque::new(),
                classes: HashMap::new(),
                bytes: 0,
                closed: false,
                wake: false,
                max_depth: 0,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            max_items,
            max_bytes,
            budget,
            class_bytes: None,
        }
    }

    /// 채널별 계정도 전체 계정과 같은 lock 안에서 예약·반납한다.
    pub(crate) fn with_class_limit(mut self, bytes: u64) -> Self {
        self.class_bytes = Some(bytes);
        self
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn try_insert(&self, st: &mut State<T>, item: T, cost: u64, class: Option<&str>) -> Result<(), T> {
        if st.q.len() >= self.max_items || st.bytes.saturating_add(cost) > self.max_bytes {
            return Err(item);
        }
        if let (Some(class), Some(limit)) = (class, self.class_bytes)
            && st.classes.get(class).copied().unwrap_or(0).saturating_add(cost) > limit
        {
            return Err(item);
        }
        if let Some(b) = &self.budget
            && !b.try_reserve(cost)
        {
            return Err(item);
        }
        let class = class.filter(|_| self.class_bytes.is_some());
        if let Some(class) = class {
            *st.classes.entry(class.to_owned()).or_default() += cost;
        }
        st.q.push_back((item, cost, class.map(str::to_owned)));
        st.bytes += cost;
        st.max_depth = st.max_depth.max(st.q.len());
        Ok(())
    }

    /// Non-blocking push; `Full` when any cap is hit.
    pub(crate) fn try_push(&self, item: T, cost: u64) -> Result<(), PushError<T>> {
        self.try_push_class(item, cost, None)
    }

    pub(crate) fn try_push_class(&self, item: T, cost: u64, class: Option<&str>) -> Result<(), PushError<T>> {
        let mut st = self.lock();
        if st.closed {
            return Err(PushError::Closed(item));
        }
        match self.try_insert(&mut st, item, cost, class) {
            Ok(()) => {
                drop(st);
                self.not_empty.notify_one();
                Ok(())
            }
            Err(item) => Err(PushError::Full(item)),
        }
    }

    /// Push, waiting at most `timeout` for room.
    pub(crate) fn push_timeout(&self, item: T, cost: u64, timeout: Duration) -> Result<(), PushError<T>> {
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        let mut item = item;
        loop {
            if st.closed {
                return Err(PushError::Closed(item));
            }
            match self.try_insert(&mut st, item, cost, None) {
                Ok(()) => {
                    drop(st);
                    self.not_empty.notify_one();
                    return Ok(());
                }
                Err(back) => item = back,
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(PushError::Full(item));
            }
            let (g, _) = self
                .not_full
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            st = g;
        }
    }

    /// Pop up to `max` items into `out`, waiting at most `timeout` for the first one.
    /// Returns true only when the queue is closed **and** empty.
    pub(crate) fn pop_batch(&self, timeout: Duration, max: usize, out: &mut Vec<T>) -> bool {
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        loop {
            if !st.q.is_empty() {
                let n = st.q.len().min(max.max(1));
                let mut freed = 0u64;
                for _ in 0..n {
                    let Some((item, cost, class)) = st.q.pop_front() else {
                        break;
                    };
                    freed += cost;
                    if let Some(class) = class {
                        let used = st.classes.get_mut(&class).expect("queued class is reserved");
                        *used -= cost;
                        if *used == 0 {
                            st.classes.remove(&class);
                        }
                    }
                    if let Some(b) = &self.budget {
                        b.release(cost);
                    }
                    out.push(item);
                }
                st.bytes -= freed;
                drop(st);
                self.not_full.notify_all();
                return false;
            }
            if st.closed {
                return true;
            }
            if st.wake {
                st.wake = false;
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (g, _) = self
                .not_empty
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            st = g;
        }
    }

    /// Wait without popping until woken, closed or `timeout`. Returns true if closed.
    pub(crate) fn park(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        loop {
            if st.closed {
                return true;
            }
            if st.wake {
                st.wake = false;
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (g, _) = self
                .not_empty
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(PoisonError::into_inner);
            st = g;
        }
    }

    /// Wake the consumer (control request pending).
    pub(crate) fn wake(&self) {
        self.lock().wake = true;
        self.not_empty.notify_all();
    }

    /// Refuse further pushes; the consumer drains what is left.
    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }

    pub(crate) fn stats(&self) -> QueueStats {
        let st = self.lock();
        QueueStats {
            depth: st.q.len(),
            bytes: st.bytes,
            max_depth: st.max_depth,
            max_items: self.max_items,
            max_bytes: self.max_bytes,
        }
    }

    pub(crate) fn class_used(&self, class: &str) -> u64 {
        self.lock().classes.get(class).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_and_worker_budgets_are_independent_and_released() {
        let mib = 1024 * 1024;
        let budget = QueueBudget::worker_default();
        let q = BoundedQueue::new(32 * 1024, 16 * mib, Some(budget.clone())).with_class_limit(8 * mib);
        assert!(q.try_push_class(1, 8 * mib, Some("A")).is_ok());
        assert!(matches!(
            q.try_push_class(2, 1, Some("A")),
            Err(PushError::Full(2))
        ));
        assert!(q.try_push_class(3, 8 * mib, Some("B")).is_ok());
        assert!(matches!(
            q.try_push_class(4, 1, Some("C")),
            Err(PushError::Full(4))
        ));
        assert_eq!(budget.used(), (16 * mib, 2));
        let mut out = Vec::new();
        q.pop_batch(Duration::ZERO, 1, &mut out);
        assert_eq!(out, [1]);
        assert_eq!(q.class_used("A"), 0);
        assert!(q.try_push_class(5, 8 * mib, Some("A")).is_ok());
        out.clear();
        q.pop_batch(Duration::ZERO, 32, &mut out);
        assert_eq!(out, [3, 5]);
        assert_eq!(budget.used(), (0, 0));
        assert!(q.lock().classes.is_empty());
    }

    #[test]
    fn caps_and_budget() {
        let budget = QueueBudget::new(100, 10);
        let q = BoundedQueue::new(3, 60, Some(budget.clone()));
        assert!(q.try_push(1, 20).is_ok());
        assert!(q.try_push(2, 20).is_ok());
        assert!(matches!(q.try_push(3, 30), Err(PushError::Full(3))));
        assert!(q.try_push(3, 20).is_ok());
        assert!(matches!(q.try_push(4, 1), Err(PushError::Full(4))));
        assert_eq!(budget.used(), (60, 3));
        let mut out = Vec::new();
        assert!(!q.pop_batch(Duration::ZERO, 2, &mut out));
        assert_eq!(out, vec![1, 2]);
        assert_eq!(budget.used(), (20, 1));
        assert!(matches!(
            q.push_timeout(9, 100, Duration::from_millis(5)),
            Err(PushError::Full(9))
        ));
        q.close();
        assert!(matches!(q.try_push(5, 1), Err(PushError::Closed(5))));
        out.clear();
        assert!(!q.pop_batch(Duration::ZERO, 10, &mut out));
        assert!(q.pop_batch(Duration::ZERO, 10, &mut out));
    }
}
