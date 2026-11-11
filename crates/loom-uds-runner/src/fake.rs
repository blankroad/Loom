//! Scripted fake transport for deterministic tests (works with tokio's paused clock).
//!
//! A script maps each submitted PDU to timed steps (`delay` after hand-off). Steps stay queued
//! across transactions, so a response scheduled after a timeout arrives as a late response.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::time::{Instant, sleep_until};

use crate::transport::{BoxFuture, Incoming, RecvError, Transport, TransportError};

/// Most scheduled steps. Full: further steps are dropped and counted ([`FakeControl::dropped`]).
pub const MAX_FAKE_SCHEDULED: usize = 256;
/// Sent-PDU log length. Full: the oldest entry is dropped.
pub const MAX_FAKE_LOG: usize = 256;

/// What happens at a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeAction {
    Respond(Vec<u8>),
    RxStart,
    BumpGeneration,
    Close,
}

/// One timed step, `delay` after the PDU was handed off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeStep {
    pub delay: Duration,
    pub action: FakeAction,
}

impl FakeStep {
    pub fn respond(delay_ms: u64, bytes: &[u8]) -> Self {
        FakeStep {
            delay: Duration::from_millis(delay_ms),
            action: FakeAction::Respond(bytes.to_vec()),
        }
    }

    pub fn at(delay_ms: u64, action: FakeAction) -> Self {
        FakeStep {
            delay: Duration::from_millis(delay_ms),
            action,
        }
    }
}

#[derive(Debug, Default)]
struct FakeShared {
    generation: u64,
    closed: bool,
    log: VecDeque<Vec<u8>>,
    queue: Vec<(Instant, u64, FakeAction)>,
    seq: u64,
    dropped: u64,
    submit_error: Option<TransportError>,
    confirm_error: Option<TransportError>,
}

impl FakeShared {
    fn schedule(&mut self, due: Instant, action: FakeAction) {
        if self.queue.len() >= MAX_FAKE_SCHEDULED {
            self.dropped += 1;
            return;
        }
        self.seq += 1;
        self.queue.push((due, self.seq, action));
    }

    /// Index of the earliest step.
    fn earliest(&self) -> Option<usize> {
        self.queue
            .iter()
            .enumerate()
            .min_by_key(|(_, (due, seq, _))| (*due, *seq))
            .map(|(i, _)| i)
    }
}

/// Test-side handle.
#[derive(Debug, Clone, Default)]
pub struct FakeControl {
    inner: Arc<Mutex<FakeShared>>,
}

impl FakeControl {
    fn lock(&self) -> MutexGuard<'_, FakeShared> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Simulate reconnect + re-activation now.
    pub fn bump_generation(&self) {
        self.lock().generation += 1;
    }

    pub fn close(&self) {
        self.lock().closed = true;
    }

    /// Schedule an action `delay` from now (e.g. an unsolicited late response).
    pub fn inject(&self, delay: Duration, action: FakeAction) {
        let due = Instant::now() + delay;
        self.lock().schedule(due, action);
    }

    /// PDUs handed off so far (oldest first, at most [`MAX_FAKE_LOG`]).
    pub fn sent(&self) -> Vec<Vec<u8>> {
        self.lock().log.iter().cloned().collect()
    }

    pub fn dropped(&self) -> u64 {
        self.lock().dropped
    }

    pub fn fail_next_submit(&self, e: TransportError) {
        self.lock().submit_error = Some(e);
    }

    pub fn fail_next_confirm(&self, e: TransportError) {
        self.lock().confirm_error = Some(e);
    }
}

type Script = Box<dyn FnMut(&[u8]) -> Vec<FakeStep> + Send>;

/// The fake transport.
pub struct FakeTransport {
    script: Script,
    ctl: FakeControl,
}

impl std::fmt::Debug for FakeTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTransport")
            .field("ctl", &self.ctl)
            .finish_non_exhaustive()
    }
}

impl FakeTransport {
    pub fn new(script: impl FnMut(&[u8]) -> Vec<FakeStep> + Send + 'static) -> (FakeTransport, FakeControl) {
        let ctl = FakeControl::default();
        (
            FakeTransport {
                script: Box::new(script),
                ctl: ctl.clone(),
            },
            ctl,
        )
    }

    /// Pop the earliest step if it is due at `now`; apply generation/close side effects.
    fn pop_due(&self, now: Instant) -> Option<Result<Incoming, RecvError>> {
        let mut sh = self.ctl.lock();
        let i = sh.earliest()?;
        if sh.queue[i].0 > now {
            return None;
        }
        let (due, _, action) = sh.queue.swap_remove(i);
        Some(match action {
            FakeAction::Respond(data) => Ok(Incoming::Pdu { data, at: due }),
            FakeAction::RxStart => Ok(Incoming::RxStart { at: due }),
            FakeAction::BumpGeneration => {
                sh.generation += 1;
                Err(RecvError::GenerationChanged)
            }
            FakeAction::Close => {
                sh.closed = true;
                Err(RecvError::Closed)
            }
        })
    }
}

impl Transport for FakeTransport {
    fn generation(&self) -> u64 {
        self.ctl.generation()
    }

    fn submit<'a>(
        &'a mut self,
        pdu: &'a [u8],
        _until: Instant,
    ) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move {
            {
                let mut sh = self.ctl.lock();
                if let Some(e) = sh.submit_error.take() {
                    return Err(e);
                }
                if sh.closed {
                    return Err(TransportError::Closed);
                }
                if sh.log.len() >= MAX_FAKE_LOG {
                    sh.log.pop_front();
                }
                sh.log.push_back(pdu.to_vec());
            }
            let now = Instant::now();
            let steps = (self.script)(pdu);
            let mut sh = self.ctl.lock();
            for s in steps {
                sh.schedule(now + s.delay, s.action);
            }
            Ok(now)
        })
    }

    fn confirm<'a>(&'a mut self, _until: Instant) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move {
            match self.ctl.lock().confirm_error.take() {
                Some(e) => Err(e),
                None => Ok(Instant::now()),
            }
        })
    }

    fn recv<'a>(&'a mut self, until: Instant) -> BoxFuture<'a, Result<Incoming, RecvError>> {
        Box::pin(async move {
            loop {
                let now = Instant::now();
                if let Some(r) = self.pop_due(now) {
                    return r;
                }
                let next = {
                    let sh = self.ctl.lock();
                    if sh.closed {
                        return Err(RecvError::Closed);
                    }
                    sh.earliest().map(|i| sh.queue[i].0)
                };
                if now >= until {
                    return Err(RecvError::Timeout);
                }
                let wake = next.map_or(until, |n| n.min(until));
                sleep_until(wake).await;
            }
        })
    }

    fn try_recv(&mut self) -> Option<Incoming> {
        let now = Instant::now();
        loop {
            match self.pop_due(now)? {
                Ok(i) => return Some(i),
                Err(_) => continue,
            }
        }
    }
}
