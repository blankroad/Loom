//! Per-target priority gate in front of the runner.
//!
//! The runner now queues by priority itself, but its events carry no request tag, so only one of
//! our requests may be inside the runner at a time for events to map to a `request_id`. This gate
//! keeps that rule while honouring the same order: Control (reconnect/restart) > Abort
//! (safe-state recovery) > User (console) > Agent (agent and script). FIFO within a lane.
//! Remove it when `RunnerEvent` carries the request id; the runner priorities already match.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use loom_proto::event::Origin;
use loom_uds_runner::{AbortKind, MAX_PENDING_ABORT, MAX_PENDING_PER_TARGET, MAX_PENDING_USER};
use tokio::sync::oneshot;

/// Internal reconnect / runner restart waiters (one per target in practice).
pub const MAX_WAITING_CONTROL: usize = 4;

/// Lane of a waiter (higher runs first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lane {
    Agent = 0,
    User = 1,
    Abort = 2,
    Control = 3,
}

impl Lane {
    fn cap(self) -> usize {
        match self {
            Lane::Agent => MAX_PENDING_PER_TARGET,
            Lane::User => MAX_PENDING_USER,
            Lane::Abort => MAX_PENDING_ABORT,
            Lane::Control => MAX_WAITING_CONTROL,
        }
    }
}

/// Why a waiter did not get the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    /// Lane full (`E_BUSY`, not sent).
    Busy,
    /// Cancelled by an abort / STOP while waiting (not sent).
    Aborted(AbortKind),
}

#[derive(Debug)]
struct Waiter {
    lane: Lane,
    origin: Origin,
    request_id: String,
    tx: oneshot::Sender<Result<(), AbortKind>>,
}

#[derive(Debug, Default)]
struct Queue {
    busy: bool,
    seq: u64,
    /// Key `(255 - lane, seq)`: the first entry is the highest lane, oldest first.
    waiters: BTreeMap<(u8, u64), Waiter>,
}

/// The gate. Hold the returned [`Permit`] while the request is in the runner.
#[derive(Debug, Default)]
pub struct PriorityGate {
    q: Mutex<Queue>,
}

/// Exclusive right to use the runner; passes it on when dropped.
#[derive(Debug)]
pub struct Permit<'a> {
    gate: &'a PriorityGate,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.gate.release();
    }
}

fn lock(m: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PriorityGate {
    pub async fn acquire(
        &self,
        lane: Lane,
        origin: Origin,
        request_id: &str,
    ) -> Result<Permit<'_>, GateError> {
        let rx = {
            let mut q = lock(&self.q);
            if !q.busy {
                q.busy = true;
                return Ok(Permit { gate: self });
            }
            if q.waiters.values().filter(|w| w.lane == lane).count() >= lane.cap() {
                return Err(GateError::Busy);
            }
            let (tx, rx) = oneshot::channel();
            q.seq += 1;
            let key = (255 - lane as u8, q.seq);
            q.waiters.insert(
                key,
                Waiter {
                    lane,
                    origin,
                    request_id: request_id.to_owned(),
                    tx,
                },
            );
            rx
        };
        match rx.await {
            Ok(Ok(())) => Ok(Permit { gate: self }),
            Ok(Err(kind)) => Err(GateError::Aborted(kind)),
            // Sender dropped without a decision only if the gate itself went away.
            Err(_) => Err(GateError::Aborted(AbortKind::User)),
        }
    }

    /// Hand the gate to the next waiter whose caller is still waiting, or mark it free.
    fn release(&self) {
        let mut q = lock(&self.q);
        while let Some((_, w)) = q.waiters.pop_first() {
            if w.tx.send(Ok(())).is_ok() {
                return; // busy stays true: ownership moved to that waiter
            }
        }
        q.busy = false;
    }

    /// Cancel waiting requests matching `pred` (Abort and Control lanes only by request id).
    /// Returns how many were cancelled.
    pub fn cancel(
        &self,
        kind: AbortKind,
        pred: impl Fn(Origin, &str) -> bool,
        include_abort_lane: bool,
    ) -> usize {
        let mut q = lock(&self.q);
        let keys: Vec<(u8, u64)> = q
            .waiters
            .iter()
            .filter(|(_, w)| (w.lane < Lane::Abort || include_abort_lane) && w.lane != Lane::Control)
            .filter(|(_, w)| pred(w.origin, &w.request_id))
            .map(|(k, _)| *k)
            .collect();
        let mut n = 0;
        for k in keys {
            if let Some(w) = q.waiters.remove(&k)
                && w.tx.send(Err(kind)).is_ok()
            {
                n += 1;
            }
        }
        n
    }

    /// Waiters per lane `(agent, user, abort, control)`.
    pub fn depth(&self) -> (usize, usize, usize, usize) {
        let q = lock(&self.q);
        let c = |l: Lane| q.waiters.values().filter(|w| w.lane == l).count();
        (c(Lane::Agent), c(Lane::User), c(Lane::Abort), c(Lane::Control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn order_cancel_and_caps() {
        let gate = Arc::new(PriorityGate::default());
        let first = gate
            .acquire(Lane::Agent, Origin::Agent, "a0")
            .await
            .expect("free gate");
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for (lane, origin, id) in [
            (Lane::Agent, Origin::Agent, "a1"),
            (Lane::Agent, Origin::Agent, "a2"),
            (Lane::User, Origin::User, "u1"),
            (Lane::Abort, Origin::User, "x1"),
        ] {
            let (g, o) = (gate.clone(), order.clone());
            tasks.push(tokio::spawn(async move {
                let r = g.acquire(lane, origin, id).await;
                if r.is_ok() {
                    o.lock().expect("lock").push(id);
                }
                r.map(|_| ()).err()
            }));
            tokio::task::yield_now().await;
        }
        while gate.depth().0 + gate.depth().1 + gate.depth().2 < 4 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            gate.cancel(AbortKind::User, |o, _| o == Origin::Agent, false),
            2,
            "agents cancelled, abort lane kept"
        );
        drop(first);
        let mut errs = Vec::new();
        for t in tasks {
            errs.push(t.await.expect("task"));
        }
        assert_eq!(
            *order.lock().expect("lock"),
            ["x1", "u1"],
            "abort lane, then user"
        );
        assert_eq!(
            errs.iter()
                .filter(|e| **e == Some(GateError::Aborted(AbortKind::User)))
                .count(),
            2
        );

        let _hold = gate.acquire(Lane::Agent, Origin::Agent, "h").await.expect("free");
        let mut waiting = Vec::new();
        for i in 0..MAX_PENDING_USER {
            let g = gate.clone();
            waiting.push(tokio::spawn(async move {
                g.acquire(Lane::User, Origin::User, &format!("u{i}"))
                    .await
                    .map(|_| ())
            }));
        }
        while gate.depth().1 < MAX_PENDING_USER {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            gate.acquire(Lane::User, Origin::User, "over").await.map(|_| ()),
            Err(GateError::Busy)
        );
        gate.cancel(AbortKind::Stop, |_, _| true, true);
        for w in waiting {
            assert_eq!(w.await.expect("task"), Err(GateError::Aborted(AbortKind::Stop)));
        }
    }
}
