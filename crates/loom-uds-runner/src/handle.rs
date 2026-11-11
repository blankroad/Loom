//! Per-target serial executor: one task owns the [`Runner`]; requests wait in bounded
//! per-priority queues (plan §3.5: abort > user > agent/script, in flight ≤ 1).
//!
//! The request in flight is never pre-empted by priority; the next one taken is the oldest of
//! the highest non-empty priority. [`RunnerHandle::abort`] fails matching waiting requests
//! without sending them and stops the wait of the request in flight.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

use crate::abort::{AbortKind, AbortScope, AbortSignal, Priority};
use crate::runner::{MonoClock, RejectReason, Runner, TxnRequest, TxnResult};

/// Agent/script requests waiting behind the in-flight one, per target (plan §3.5: in flight 1 +
/// 32 waiting). Full: refused before sending with `E_BUSY` ([`RejectReason::QueueFull`]).
pub const MAX_PENDING_PER_TARGET: usize = 32;
/// User requests waiting, per target — capacity reserved for the user (plan §3.8). Full:
/// `E_BUSY`, not sent.
pub const MAX_PENDING_USER: usize = 8;
/// Abort-priority requests (safe-state recovery) waiting, per target — reserved. Full: `E_BUSY`,
/// not sent.
pub const MAX_PENDING_ABORT: usize = 16;
/// Agent lane payload + request-id bytes. Full: QueueFull before enqueue.
pub const MAX_PENDING_AGENT_BYTES: usize = 32 * 1024;
/// Reserved user lane bytes. Full: QueueFull before enqueue.
pub const MAX_PENDING_USER_BYTES: usize = 16 * 1024;
/// Reserved abort lane bytes. Full: QueueFull before enqueue.
pub const MAX_PENDING_ABORT_BYTES: usize = 16 * 1024;
/// Target payload + request-id bytes, including its in-flight request. Full: QueueFull.
/// The fixed-size job bookkeeping is additionally bounded by the per-lane count caps.
pub const MAX_RETAINED_TARGET_BYTES: usize = 64 * 1024;

#[derive(Debug)]
struct Job {
    req: TxnRequest,
    reply: oneshot::Sender<TxnResult>,
}

#[derive(Debug)]
struct InFlight {
    req: TxnRequest,
    signal: AbortSignal,
}

#[derive(Debug, Default)]
struct Queues {
    abort: VecDeque<Job>,
    user: VecDeque<Job>,
    agent: VecDeque<Job>,
    in_flight: Option<InFlight>,
    /// Every handle dropped: finish the queue, then stop.
    closed: bool,
    /// The task ended: refuse new requests.
    stopped: bool,
    /// Generation seen by the last execution (for results built outside the task).
    generation: u64,
}

impl Queues {
    fn bytes(&self) -> usize {
        self.abort
            .iter()
            .chain(&self.user)
            .chain(&self.agent)
            .map(|job| job.req.retained_bytes())
            .sum::<usize>()
            + self.in_flight.as_ref().map_or(0, |job| job.req.retained_bytes())
    }

    fn lane(&mut self, p: Priority) -> (&mut VecDeque<Job>, usize) {
        match p {
            Priority::Abort => (&mut self.abort, MAX_PENDING_ABORT),
            Priority::User => (&mut self.user, MAX_PENDING_USER),
            Priority::Agent => (&mut self.agent, MAX_PENDING_PER_TARGET),
        }
    }

    fn pop(&mut self) -> Option<Job> {
        self.abort
            .pop_front()
            .or_else(|| self.user.pop_front())
            .or_else(|| self.agent.pop_front())
    }
}

#[derive(Debug)]
struct Shared {
    q: Mutex<Queues>,
    wake: Notify,
    clock: MonoClock,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.q.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Dropped with the last handle clone: lets the task finish the queue and stop.
#[derive(Debug)]
struct Liveness(Arc<Shared>);

impl Drop for Liveness {
    fn drop(&mut self) {
        self.0.lock().closed = true;
        self.0.wake.notify_one();
    }
}

/// Marks the task stopped however it ends; waiting requests are dropped (their callers see
/// `RunnerStopped`).
struct StopGuard(Arc<Shared>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        let mut q = self.0.lock();
        q.stopped = true;
        q.in_flight = None;
        q.abort.clear();
        q.user.clear();
        q.agent.clear();
    }
}

/// What an abort hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AbortSummary {
    /// Waiting requests failed without being sent.
    pub cancelled_waiting: usize,
    /// The request in flight was signalled (its result arrives through its own receiver).
    pub in_flight: bool,
}

/// Queue depth snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueDepth {
    pub abort: usize,
    pub user: usize,
    pub agent: usize,
    pub in_flight: bool,
}

/// Handle to a spawned per-target runner. Cloneable; the task stops when every handle is dropped
/// (after finishing the waiting requests) and returns the [`Runner`].
#[derive(Debug, Clone)]
pub struct RunnerHandle {
    shared: Arc<Shared>,
    _live: Arc<Liveness>,
}

impl RunnerHandle {
    /// Spawn the runner task.
    pub fn spawn(mut runner: Runner) -> (RunnerHandle, JoinHandle<Runner>) {
        let shared = Arc::new(Shared {
            q: Mutex::new(Queues {
                generation: runner.generation(),
                ..Queues::default()
            }),
            wake: Notify::new(),
            clock: *runner.clock(),
        });
        let live = Arc::new(Liveness(Arc::clone(&shared)));
        let task_shared = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            let _stop = StopGuard(Arc::clone(&task_shared));
            loop {
                let next = {
                    let mut q = task_shared.lock();
                    match q.pop() {
                        Some(job) => {
                            let signal = AbortSignal::new();
                            q.in_flight = Some(InFlight {
                                req: job.req.clone(),
                                signal: signal.clone(),
                            });
                            Some(Some((job, signal)))
                        }
                        None if q.closed => Some(None),
                        None => None,
                    }
                };
                let (job, signal) = match next {
                    Some(Some(j)) => j,
                    Some(None) => break,
                    None => {
                        // `notify_one` stores a permit, so a push between unlock and here is
                        // not lost.
                        task_shared.wake.notified().await;
                        continue;
                    }
                };
                // Once taken, a request runs to completion even if the caller went away.
                let result = runner.execute_abortable(job.req, &signal).await;
                {
                    let mut q = task_shared.lock();
                    q.in_flight = None;
                    q.generation = result.generation;
                }
                if job.reply.send(result).is_err() {
                    tracing::debug!("runner caller gone before the result");
                }
            }
            runner
        });
        (RunnerHandle { shared, _live: live }, task)
    }

    /// Queue a request in its priority's queue. A full queue or stopped runner returns the
    /// rejection result immediately.
    pub fn try_submit(&self, mut req: TxnRequest) -> Result<oneshot::Receiver<TxnResult>, Box<TxnResult>> {
        let mut q = self.shared.lock();
        let reason = if let Some(reason) = req.admission_error() {
            Some(reason)
        } else if q.stopped {
            Some(RejectReason::RunnerStopped)
        } else {
            let total_bytes = q.bytes();
            let (lane, cap) = q.lane(req.priority);
            let byte_cap = match req.priority {
                Priority::Abort => MAX_PENDING_ABORT_BYTES,
                Priority::User => MAX_PENDING_USER_BYTES,
                Priority::Agent => MAX_PENDING_AGENT_BYTES,
            };
            let lane_bytes: usize = lane.iter().map(|job| job.req.retained_bytes()).sum();
            (lane.len() >= cap
                || lane_bytes + req.retained_bytes() > byte_cap
                || total_bytes + req.retained_bytes() > MAX_RETAINED_TARGET_BYTES)
                .then_some(RejectReason::QueueFull)
        };
        if let Some(reason) = reason {
            let generation = q.generation;
            drop(q);
            return Err(Box::new(TxnResult::rejected(
                &req,
                reason,
                &self.shared.clock,
                generation,
            )));
        }
        // A small logical length must not retain caller-provided giant allocations.
        req.pdu.shrink_to_fit();
        if let Some(id) = req.request_id.as_mut() {
            id.shrink_to_fit();
        }
        let (reply, rx) = oneshot::channel();
        q.lane(req.priority).0.push_back(Job { req, reply });
        drop(q);
        self.shared.wake.notify_one();
        Ok(rx)
    }

    /// Queue a request and wait for its result.
    pub async fn request(&self, req: TxnRequest) -> TxnResult {
        if let Some(reason) = req.admission_error() {
            return TxnResult::rejected(&req, reason, &self.shared.clock, self.shared.lock().generation);
        }
        let probe = req.clone();
        match self.try_submit(req) {
            Ok(rx) => match rx.await {
                Ok(r) => r,
                Err(_) => TxnResult::rejected(&probe, RejectReason::RunnerStopped, &self.shared.clock, 0),
            },
            Err(r) => *r,
        }
    }

    /// User abort (`E_INTERRUPTED_BY_USER`). See [`RunnerHandle::abort_with`].
    pub fn abort(&self, scope: AbortScope) -> AbortSummary {
        self.abort_with(scope, AbortKind::User)
    }

    /// Abort matching requests. Waiting ones are failed at once without being sent (`failed` +
    /// `E_INTERRUPTED_BY_USER` / `E_STOPPED`); the one in flight is signalled and its result —
    /// read `failed`, write `outcome_unknown`, response quarantined — arrives through its own
    /// receiver. Never waits: one short lock, no await (measured ≤ 10 ms).
    pub fn abort_with(&self, scope: AbortScope, kind: AbortKind) -> AbortSummary {
        let mut guard = self.shared.lock();
        let q = &mut *guard;
        let generation = q.generation;
        let mut hit: Vec<Job> = Vec::new();
        for lane in [&mut q.abort, &mut q.user, &mut q.agent] {
            let mut i = 0;
            while i < lane.len() {
                if lane.get(i).is_some_and(|j| scope.matches(&j.req)) {
                    if let Some(job) = lane.remove(i) {
                        hit.push(job);
                    }
                } else {
                    i += 1;
                }
            }
        }
        let in_flight = match &q.in_flight {
            Some(f) if scope.matches(&f.req) => {
                f.signal.abort(kind);
                true
            }
            _ => false,
        };
        drop(guard);
        let cancelled_waiting = hit.len();
        for job in hit {
            let res = TxnResult::aborted(&job.req, kind, &self.shared.clock, generation);
            if job.reply.send(res).is_err() {
                tracing::debug!("aborted request's caller already gone");
            }
        }
        AbortSummary {
            cancelled_waiting,
            in_flight,
        }
    }

    /// Current queue depth per priority.
    pub fn queue_depth(&self) -> QueueDepth {
        let q = self.shared.lock();
        QueueDepth {
            abort: q.abort.len(),
            user: q.user.len(),
            agent: q.agent.len(),
            in_flight: q.in_flight.is_some(),
        }
    }
}
