//! Endpoints receive bus records. The bus only calls [`Endpoint::offer`]; a later
//! process-to-process endpoint (local socket, after CORE IPC exists) implements the same trait.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::record::SimRecord;

/// Default receive queue per endpoint (plan §9.1: 8,192 frames).
pub const DEFAULT_QUEUE_FRAMES: usize = 8_192;

/// Consumer side of a bus endpoint.
pub trait Endpoint: Send {
    /// Takes one record, in bus order. Must not block and must not wait for the consumer.
    /// Returns `false` when there is no room: the bus then counts a drop for this endpoint
    /// only (other endpoints and the bus are not affected).
    fn offer(&mut self, record: &SimRecord) -> bool;
}

struct Queue {
    records: VecDeque<SimRecord>,
    capacity: usize,
    /// Fault: at most this many records per `recv` call (slow consumer).
    throttle: Option<usize>,
}

fn lock(q: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    // A panic elsewhere must not take the bus down; the queue data stays valid.
    q.lock().unwrap_or_else(|e| e.into_inner())
}

/// In-process endpoint with a fixed-capacity queue.
pub struct LocalEndpoint {
    queue: Arc<Mutex<Queue>>,
}

/// Reading side of a [`LocalEndpoint`]. `Send + Sync`, may live on another thread.
#[derive(Clone)]
pub struct LocalReceiver {
    queue: Arc<Mutex<Queue>>,
}

impl std::fmt::Debug for LocalEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let q = lock(&self.queue);
        f.debug_struct("LocalEndpoint")
            .field("queued", &q.records.len())
            .field("capacity", &q.capacity)
            .finish()
    }
}

impl std::fmt::Debug for LocalReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let q = lock(&self.queue);
        f.debug_struct("LocalReceiver")
            .field("queued", &q.records.len())
            .field("capacity", &q.capacity)
            .field("throttle", &q.throttle)
            .finish()
    }
}

/// Creates a local endpoint pair. `capacity` is clamped to 1..=[`DEFAULT_QUEUE_FRAMES`] × 8.
pub fn local_endpoint(capacity: usize) -> (LocalEndpoint, LocalReceiver) {
    let capacity = capacity.clamp(1, DEFAULT_QUEUE_FRAMES * 8);
    let queue = Arc::new(Mutex::new(Queue {
        records: VecDeque::new(),
        capacity,
        throttle: None,
    }));
    (LocalEndpoint { queue: queue.clone() }, LocalReceiver { queue })
}

impl Endpoint for LocalEndpoint {
    fn offer(&mut self, record: &SimRecord) -> bool {
        let mut q = lock(&self.queue);
        if q.records.len() >= q.capacity {
            return false;
        }
        q.records.push_back(*record);
        true
    }
}

impl LocalReceiver {
    /// Takes up to `max` records (fewer under the slow-consumer fault).
    pub fn recv(&self, max: usize) -> Vec<SimRecord> {
        let mut out = Vec::new();
        self.recv_into(&mut out, max);
        out
    }

    /// Appends up to `max` records to `out`; returns how many.
    pub fn recv_into(&self, out: &mut Vec<SimRecord>, max: usize) -> usize {
        let mut q = lock(&self.queue);
        let n = max.min(q.throttle.unwrap_or(usize::MAX)).min(q.records.len());
        out.extend(q.records.drain(..n));
        n
    }

    pub fn len(&self) -> usize {
        lock(&self.queue).records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        lock(&self.queue).capacity
    }

    /// Fault injection (sim only): read at most `n` records per call; `None` = normal.
    pub fn set_throttle(&self, n: Option<usize>) {
        lock(&self.queue).throttle = n;
    }
}
