//! Per-target serial executor: one task owns the [`Runner`]; requests wait in a bounded queue.

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::runner::{MonoClock, RejectReason, Runner, TxnRequest, TxnResult};

/// Requests waiting behind the in-flight one, per target (plan §3.5: in-flight 1 + 32 waiting).
/// Full: the request is refused before sending with `E_BUSY` ([`RejectReason::QueueFull`]).
pub const MAX_PENDING_PER_TARGET: usize = 32;

#[derive(Debug)]
struct Job {
    req: TxnRequest,
    reply: oneshot::Sender<TxnResult>,
}

/// Handle to a spawned per-target runner. Cloneable; the task stops when every handle is dropped
/// and returns the [`Runner`].
#[derive(Debug, Clone)]
pub struct RunnerHandle {
    tx: mpsc::Sender<Job>,
    clock: MonoClock,
}

impl RunnerHandle {
    /// Spawn the runner task.
    pub fn spawn(mut runner: Runner) -> (RunnerHandle, JoinHandle<Runner>) {
        let (tx, mut rx) = mpsc::channel::<Job>(MAX_PENDING_PER_TARGET);
        let clock = *runner.clock();
        let task = tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                // Once accepted, a request runs to completion even if the caller went away.
                let result = runner.execute(job.req).await;
                if job.reply.send(result).is_err() {
                    tracing::debug!("runner caller gone before the result");
                }
            }
            runner
        });
        (RunnerHandle { tx, clock }, task)
    }

    /// Queue a request. A full queue or stopped runner returns the rejection result immediately.
    pub fn try_submit(&self, req: TxnRequest) -> Result<oneshot::Receiver<TxnResult>, Box<TxnResult>> {
        let (reply, rx) = oneshot::channel();
        match self.tx.try_send(Job { req, reply }) {
            Ok(()) => Ok(rx),
            Err(mpsc::error::TrySendError::Full(job)) => Err(Box::new(TxnResult::rejected(
                &job.req,
                RejectReason::QueueFull,
                &self.clock,
                0,
            ))),
            Err(mpsc::error::TrySendError::Closed(job)) => Err(Box::new(TxnResult::rejected(
                &job.req,
                RejectReason::RunnerStopped,
                &self.clock,
                0,
            ))),
        }
    }

    /// Queue a request and wait for its result.
    pub async fn request(&self, req: TxnRequest) -> TxnResult {
        let probe = req.clone();
        match self.try_submit(req) {
            Ok(rx) => match rx.await {
                Ok(r) => r,
                Err(_) => TxnResult::rejected(&probe, RejectReason::RunnerStopped, &self.clock, 0),
            },
            Err(r) => *r,
        }
    }
}
