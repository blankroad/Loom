use crate::stats::Stats;
use crate::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Wake {
    at: u64,
    job: JobId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Paused(PauseReason),
}
#[derive(Debug, Clone, Copy)]
struct Pending {
    id: DispatchId,
    scheduled: u64,
    created: u64,
    frame: CanFrame,
    revision: u64,
}
#[derive(Debug)]
struct Job {
    id: JobId,
    spec: JobSpec,
    next: Option<u64>,
    lease_end: Option<u64>,
    max_end: u64,
    remaining: Option<u64>,
    revision: u64,
    effective_revision: u64,
    status: Status,
    pending: Option<Pending>,
    stats: Stats,
}
#[derive(Debug)]
struct PendingPatch {
    job: JobId,
    frame: CanFrame,
    revision: u64,
}
#[derive(Debug)]
struct PausedChannel {
    channel: String,
    reason: PauseReason,
}

/// Read-only job facts; querying does not advance time or retain history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSnapshot {
    pub id: JobId,
    pub spec: JobSpec,
    pub next_ns: Option<u64>,
    pub lease_end_ns: Option<u64>,
    pub max_end_ns: u64,
    pub remaining: Option<u64>,
    pub revision: u64,
    pub effective_revision: u64,
    pub paused: Option<PauseReason>,
    pub pending: Option<DispatchId>,
    pub stats: StatsSnapshot,
}
#[derive(Debug)]
enum Action {
    Stop(JobId, StopReason),
    Tick {
        job: JobId,
        scheduled: u64,
        next: Option<u64>,
        skips: u64,
        dispatch: bool,
    },
}

/// Single-thread-driven scheduler with at most one unacknowledged dispatch per job.
#[derive(Debug)]
pub struct Scheduler {
    limits: Limits,
    now: u64,
    job_serial: u64,
    dispatch_serial: u64,
    latched: bool,
    jobs: Vec<Job>,
    patches: Vec<PendingPatch>,
    paused: Vec<PausedChannel>,
    heap: BinaryHeap<Reverse<Wake>>,
    dues: VecDeque<Due>,
    events: VecDeque<Event>,
}
impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Advance the original absolute grid strictly beyond `now`, without multiplication overflow.
fn grid_after(deadline: u64, period: u64, now: u64) -> (Option<u64>, u64) {
    if deadline > now {
        return (Some(deadline), 0);
    }
    let ticks = (u128::from(now - deadline) / u128::from(period)) + 1;
    let next = u128::from(deadline) + ticks * u128::from(period);
    (u64::try_from(next).ok(), u64::try_from(ticks).unwrap_or(u64::MAX))
}
fn expiry(job: &Job, now: u64) -> Option<StopReason> {
    if job.lease_end.is_some_and(|at| at < job.max_end && at <= now) {
        Some(StopReason::LeaseExpired)
    } else if now >= job.max_end {
        Some(StopReason::MaxDuration)
    } else {
        None
    }
}
fn cancel(job: &mut Job) {
    if job.pending.take().is_some() {
        job.stats.cancelled = job.stats.cancelled.saturating_add(1);
    }
}

impl Scheduler {
    /// Default bounds; initial monotonic time is zero. Use start_at for an OS clock epoch.
    pub fn new() -> Self {
        Self::allocate(Limits::default())
    }
    /// Lower hard bounds for a profile or deterministic pressure test.
    pub fn with_limits(limits: Limits) -> Result<Self, Error> {
        if !(1..=MAX_JOBS).contains(&limits.jobs)
            || !(1..=MAX_PATCHES).contains(&limits.patches)
            || !(1..=MAX_JOBS).contains(&limits.dues)
            || !(1..=MAX_EVENTS).contains(&limits.events)
            || !(1..=MAX_JOBS).contains(&limits.paused_channels)
        {
            return Err(Error::InvalidLimits);
        }
        Ok(Self::allocate(limits))
    }
    fn allocate(limits: Limits) -> Self {
        Self {
            limits,
            now: 0,
            job_serial: 0,
            dispatch_serial: 0,
            latched: false,
            jobs: Vec::with_capacity(limits.jobs),
            patches: Vec::with_capacity(limits.patches),
            paused: Vec::with_capacity(limits.paused_channels),
            heap: BinaryHeap::with_capacity(limits.jobs),
            dues: VecDeque::with_capacity(limits.dues),
            events: VecDeque::with_capacity(limits.events),
        }
    }
    fn clock(&self, now: u64) -> Result<(), Error> {
        if now < self.now {
            Err(Error::TimeWentBackwards)
        } else {
            Ok(())
        }
    }
    fn event_space(&self, count: usize) -> Result<(), Error> {
        if self.events.len() + count > self.limits.events {
            Err(Error::OutputFull)
        } else {
            Ok(())
        }
    }
    fn index(&self, id: JobId) -> Result<usize, Error> {
        self.jobs.iter().position(|j| j.id == id).ok_or(Error::UnknownJob)
    }
    fn rebuild(&mut self) {
        // One node per admitted job; no accumulation of stale lazy-cancellation entries.
        self.heap.clear();
        for job in &self.jobs {
            let mut at = job.max_end;
            if let Some(lease) = job.lease_end {
                at = at.min(lease);
            }
            if job.status == Status::Running
                && let Some(next) = job.next
            {
                at = at.min(next);
            }
            self.heap.push(Reverse(Wake { at, job: job.id }));
        }
    }
    /// Admitted jobs, including resumable paused jobs.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }
    /// Whether there are no admitted jobs.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    /// Number of accepted, not yet applied patches.
    pub fn pending_patches(&self) -> usize {
        self.patches.len()
    }
    /// Whether all periodic starts are currently blocked by STOP.
    pub fn is_latched(&self) -> bool {
        self.latched
    }
    /// Remove one bounded due frame; revalidate it immediately before driver submission.
    pub fn pop_due(&mut self) -> Option<Due> {
        self.dues.pop_front()
    }
    /// Remove one event; terminal statistics live here, not in an ever-growing job archive.
    pub fn pop_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
    /// Current due/event output queue occupancy.
    pub fn output_len(&self) -> (usize, usize) {
        (self.dues.len(), self.events.len())
    }
    /// Current job facts, including admitted vs applied patch revision.
    pub fn job(&self, id: JobId) -> Result<JobSnapshot, Error> {
        let j = &self.jobs[self.index(id)?];
        Ok(JobSnapshot {
            id,
            spec: j.spec.clone(),
            next_ns: j.next,
            lease_end_ns: j.lease_end,
            max_end_ns: j.max_end,
            remaining: j.remaining,
            revision: j.revision,
            effective_revision: j.effective_revision,
            paused: match j.status {
                Status::Running => None,
                Status::Paused(p) => Some(p),
            },
            pending: j.pending.map(|p| p.id),
            stats: j.stats.snapshot(),
        })
    }
    /// Start using the most recently supplied monotonic time.
    pub fn start(&mut self, spec: JobSpec) -> Result<JobId, Error> {
        self.start_at(spec, self.now)
    }
    /// Admit a producer at an explicit monotonic time; all validation precedes mutation.
    pub fn start_at(&mut self, spec: JobSpec, now: u64) -> Result<JobId, Error> {
        self.clock(now)?;
        spec.validate()?;
        if self.latched {
            return Err(Error::Latched);
        }
        if let Some(j) = self
            .jobs
            .iter()
            .find(|j| j.spec.channel == spec.channel && j.spec.frame.id() == spec.frame.id())
        {
            return Err(Error::Conflict {
                owner: j.spec.owner.clone(),
            });
        }
        if self.jobs.len() == self.limits.jobs {
            return Err(Error::ResourceExhausted);
        }
        let next = now
            .checked_add(spec.period_ns)
            .and_then(|v| v.checked_add(spec.offset_ns))
            .ok_or(Error::TimeOverflow)?;
        let max_end = now.checked_add(spec.max_duration_ns).ok_or(Error::TimeOverflow)?;
        let lease_end = spec
            .lease_ns
            .map(|v| now.checked_add(v).ok_or(Error::TimeOverflow))
            .transpose()?;
        let status = self
            .paused
            .iter()
            .find(|p| p.channel == spec.channel && spec.origin != Origin::User)
            .map_or(Status::Running, |p| Status::Paused(p.reason));
        self.event_space(usize::from(status != Status::Running))?;
        let serial = self.job_serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        let id = JobId(serial);
        let job = Job {
            id,
            remaining: spec.count,
            spec,
            next: Some(next),
            lease_end,
            max_end,
            revision: 0,
            effective_revision: 0,
            status,
            pending: None,
            stats: Stats::default(),
        };
        if let Status::Paused(reason) = status {
            self.events.push_back(Event::Stopped {
                job: id,
                owner: job.spec.owner.clone(),
                reason: reason.stop_reason(),
                resumable: true,
                at_ns: now,
                stats: job.stats.snapshot(),
            });
        }
        self.jobs.push(job);
        self.job_serial = serial;
        self.now = now;
        self.rebuild();
        Ok(id)
    }
    /// Admit a byte patch with CAS; current/removed Due values remain immutable.
    pub fn patch(&mut self, id: JobId, expected_rev: u64, patch: Patch) -> Result<u64, Error> {
        let index = self.index(id)?;
        let job = &self.jobs[index];
        if let Some(reason) = expiry(job, self.now) {
            let _ = reason;
            return Err(Error::Expired);
        }
        if expected_rev != job.revision {
            return Err(Error::RevisionConflict {
                current: job.revision,
            });
        }
        if self.patches.len() == self.limits.patches {
            return Err(Error::ResourceExhausted);
        }
        let revision = job.revision.checked_add(1).ok_or(Error::CounterExhausted)?;
        let base = self
            .patches
            .iter()
            .rev()
            .find(|p| p.job == id)
            .map_or(job.spec.frame, |p| p.frame);
        let frame = patch.apply(base)?;
        self.patches.push(PendingPatch {
            job: id,
            frame,
            revision,
        });
        self.jobs[index].revision = revision;
        Ok(revision)
    }
    /// Extend to now+original lease length without changing the initial maximum lifetime.
    pub fn renew(&mut self, id: JobId, now: u64) -> Result<u64, Error> {
        self.clock(now)?;
        let index = self.index(id)?;
        let job = &self.jobs[index];
        if now >= job.max_end {
            return Err(Error::LeaseCapExceeded);
        }
        if expiry(job, now).is_some() {
            return Err(Error::Expired);
        }
        let lease = job.spec.lease_ns.ok_or(Error::NoLease)?;
        let end = now.checked_add(lease).ok_or(Error::LeaseCapExceeded)?;
        if end > job.max_end {
            return Err(Error::LeaseCapExceeded);
        }
        self.jobs[index].lease_end = Some(end);
        self.now = now;
        self.rebuild();
        Ok(end)
    }
    /// Earliest absolute grid tick or lifecycle deadline, including paused-job expiry.
    pub fn poll_at(&self) -> Option<u64> {
        self.heap.peek().map(|v| v.0.at)
    }
    /// Transactionally enqueue at most one due per job. Returns the number enqueued.
    /// Output pressure leaves the clock, revisions, deadlines and counters unchanged.
    pub fn poll(&mut self, now: u64) -> Result<usize, Error> {
        self.clock(now)?;
        let mut heap = self.heap.clone();
        let mut actions = Vec::with_capacity(self.limits.jobs);
        let mut due_count = 0usize;
        let mut stopped = 0usize;
        let mut removed_dues = 0usize;
        while let Some(Reverse(wake)) = heap.pop() {
            if wake.at > now {
                break;
            }
            let job = &self.jobs[self.index(wake.job)?];
            if let Some(reason) = expiry(job, now) {
                stopped += 1;
                removed_dues += self.dues.iter().filter(|d| d.job == job.id).count();
                actions.push(Action::Stop(job.id, reason));
            } else if job.status == Status::Running
                && let Some(scheduled) = job.next.filter(|at| *at <= now)
            {
                let (next, ticks) = grid_after(scheduled, job.spec.period_ns, now);
                let dispatch = job.pending.is_none();
                due_count += usize::from(dispatch);
                actions.push(Action::Tick {
                    job: job.id,
                    scheduled,
                    next,
                    skips: ticks.saturating_sub(u64::from(dispatch)),
                    dispatch,
                });
            }
        }
        self.event_space(stopped)?;
        if self.dues.len() - removed_dues + due_count > self.limits.dues {
            return Err(Error::OutputFull);
        }
        self.dispatch_serial
            .checked_add(due_count as u64)
            .ok_or(Error::CounterExhausted)?;
        self.now = now;
        for action in actions {
            match action {
                Action::Stop(id, reason) => self.retire(id, reason),
                Action::Tick {
                    job: id,
                    scheduled,
                    next,
                    skips,
                    dispatch,
                } => {
                    let index = self.index(id)?;
                    if dispatch {
                        let patch = self
                            .patches
                            .iter()
                            .rev()
                            .find(|p| p.job == id)
                            .map(|p| (p.frame, p.revision));
                        if let Some((frame, revision)) = patch {
                            self.jobs[index].spec.frame = frame;
                            self.jobs[index].effective_revision = revision;
                        }
                        self.patches.retain(|p| p.job != id);
                    }
                    let job = &mut self.jobs[index];
                    job.next = next;
                    job.stats.missed = job.stats.missed.saturating_add(skips);
                    if dispatch {
                        self.dispatch_serial += 1;
                        let handle = DispatchId {
                            job: id,
                            sequence: self.dispatch_serial,
                        };
                        job.pending = Some(Pending {
                            id: handle,
                            scheduled,
                            created: now,
                            frame: job.spec.frame,
                            revision: job.effective_revision,
                        });
                        self.dues.push_back(Due {
                            job: id,
                            dispatch: handle,
                            channel: job.spec.channel.clone(),
                            frame: job.spec.frame,
                            origin: job.spec.origin,
                            owner: job.spec.owner.clone(),
                            scheduled_ns: scheduled,
                            lateness_ns: now - scheduled,
                            revision: job.effective_revision,
                        });
                    }
                }
            }
        }
        self.rebuild();
        Ok(due_count)
    }
    /// Alias for the ISO-TP-style timer integration.
    pub fn on_timer(&mut self, now: u64) -> Result<usize, Error> {
        self.poll(now)
    }
    /// Recheck a removed Due on the TX thread immediately before enqueueing it.
    pub fn is_due_current(&self, due: &Due, now: u64) -> bool {
        !self.latched
            && now >= self.now
            && self.jobs.iter().any(|j| {
                j.id == due.job
                    && j.status == Status::Running
                    && expiry(j, now).is_none()
                    && j.spec.channel == due.channel
                    && j.spec.owner == due.owner
                    && j.spec.origin == due.origin
                    && j.pending.is_some_and(|p| {
                        p.id == due.dispatch
                            && p.frame == due.frame
                            && p.revision == due.revision
                            && now >= p.created
                    })
            })
    }
    /// Convenience callback for serialized integrations with no stale dispatch callbacks.
    /// Prefer on_dispatch_submitted when a driver queue can outlive pause/resume.
    pub fn on_submitted(&mut self, id: JobId, actual_ns: u64) -> Result<(), Error> {
        let dispatch = self.jobs[self.index(id)?].pending.ok_or(Error::StaleDispatch)?.id;
        self.on_dispatch_submitted(dispatch, actual_ns)
    }
    /// Record actual driver acceptance for exactly this dispatch, not bus confirmation.
    pub fn on_dispatch_submitted(&mut self, dispatch: DispatchId, actual_ns: u64) -> Result<(), Error> {
        self.finish_dispatch(dispatch, actual_ns, true)
    }
    /// Convenience failure callback; use the dispatch variant across asynchronous boundaries.
    pub fn on_send_failed(&mut self, id: JobId, actual_ns: u64) -> Result<(), Error> {
        let dispatch = self.jobs[self.index(id)?].pending.ok_or(Error::StaleDispatch)?.id;
        self.on_dispatch_failed(dispatch, actual_ns)
    }
    /// A failed attempt is not retried immediately and does not consume a successful count.
    pub fn on_dispatch_failed(&mut self, dispatch: DispatchId, actual_ns: u64) -> Result<(), Error> {
        self.finish_dispatch(dispatch, actual_ns, false)
    }
    fn finish_dispatch(&mut self, dispatch: DispatchId, actual: u64, success: bool) -> Result<(), Error> {
        self.clock(actual)?;
        let index = self.index(dispatch.job)?;
        let job = &self.jobs[index];
        let pending = job
            .pending
            .filter(|p| p.id == dispatch)
            .ok_or(Error::StaleDispatch)?;
        if actual < pending.created {
            return Err(Error::SubmissionBeforeDispatch);
        }
        let expired = expiry(job, actual);
        let stop = expired.or_else(|| (success && job.remaining == Some(1)).then_some(StopReason::Completed));
        self.event_space(usize::from(!success) + usize::from(stop.is_some()))?;
        self.now = actual;
        let job = &mut self.jobs[index];
        job.pending = None;
        if success {
            job.stats.submitted(actual - pending.scheduled);
            if expired.is_some() {
                job.stats.expired_submissions = job.stats.expired_submissions.saturating_add(1);
            }
            if let Some(left) = job.remaining.as_mut() {
                *left = left.saturating_sub(1);
            }
        } else {
            job.stats.failed = job.stats.failed.saturating_add(1);
            self.events.push_back(Event::SendFailed {
                dispatch,
                at_ns: actual,
            });
        }
        if let Some(next) = job.next {
            let (next, ticks) = grid_after(next, job.spec.period_ns, actual);
            job.next = next;
            job.stats.missed = job.stats.missed.saturating_add(ticks);
        }
        self.dues.retain(|d| d.dispatch != dispatch);
        if let Some(reason) = stop {
            self.retire(dispatch.job, reason);
        }
        self.rebuild();
        Ok(())
    }
    fn retire(&mut self, id: JobId, reason: StopReason) {
        if let Some(index) = self.jobs.iter().position(|j| j.id == id) {
            let mut job = self.jobs.remove(index);
            cancel(&mut job);
            self.events.push_back(Event::Stopped {
                job: id,
                owner: job.spec.owner,
                reason,
                resumable: false,
                at_ns: self.now,
                stats: job.stats.snapshot(),
            });
            self.dues.retain(|d| d.job != id);
            self.patches.retain(|p| p.job != id);
        }
    }
    /// Explicitly delete a job and invalidate queued/removed dispatches and patches.
    pub fn stop(&mut self, id: JobId) -> Result<(), Error> {
        self.index(id)?;
        self.event_space(1)?;
        self.retire(id, StopReason::Stopped);
        self.rebuild();
        Ok(())
    }
    /// Stop all origins, reject all new periodic starts; user one-shots belong elsewhere.
    /// If event space is full, returns OutputFull before setting the latch: the worker
    /// must set its independent final TX latch first, drain events and retry this call.
    pub fn latch(&mut self, on: bool) -> Result<(), Error> {
        self.latch_at(on, self.now)
    }
    /// STOP at explicit time. Clearing never recreates jobs or resumes paused jobs.
    pub fn latch_at(&mut self, on: bool, now: u64) -> Result<(), Error> {
        self.clock(now)?;
        self.event_space(if on { self.jobs.len() } else { 0 })?;
        self.now = now;
        self.latched = on;
        if on {
            while let Some(job) = self.jobs.first() {
                let id = job.id;
                self.retire(id, StopReason::Latched);
            }
        }
        self.rebuild();
        Ok(())
    }
    /// Replace the owner with a fresh job id; preserve effective bytes/grid/lifetime/count.
    /// Old-owner unapplied patches and outstanding dispatches are cancelled.
    pub fn takeover(
        &mut self,
        id: JobId,
        new_owner: impl Into<String>,
        origin: Origin,
    ) -> Result<JobId, Error> {
        let owner = new_owner.into();
        validate_name(&owner, MAX_OWNER_BYTES)?;
        if self.latched {
            return Err(Error::Latched);
        }
        let index = self.index(id)?;
        if expiry(&self.jobs[index], self.now).is_some() {
            return Err(Error::Expired);
        }
        self.event_space(1)?;
        let serial = self.job_serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        let new_id = JobId(serial);
        let old = &mut self.jobs[index];
        cancel(old);
        self.events.push_back(Event::Stopped {
            job: id,
            owner: old.spec.owner.clone(),
            reason: StopReason::Takeover,
            resumable: false,
            at_ns: self.now,
            stats: old.stats.snapshot(),
        });
        self.dues.retain(|d| d.job != id);
        self.patches.retain(|p| p.job != id);
        old.id = new_id;
        old.spec.owner = owner;
        old.spec.origin = origin;
        old.spec.count = old.remaining;
        old.revision = 0;
        old.effective_revision = 0;
        old.stats = Stats::default();
        if let Some(next) = old.next {
            old.next = grid_after(next, old.spec.period_ns, self.now).0;
        }
        if origin != Origin::User
            && let Some(ch) = self.paused.iter().find(|ch| ch.channel == old.spec.channel)
        {
            old.status = Status::Paused(ch.reason);
        }
        self.job_serial = serial;
        self.rebuild();
        Ok(new_id)
    }
    /// Retain agent/script jobs but stop their transmissions until explicit resume.
    /// The channel pause also applies to new agent/script jobs admitted afterwards.
    pub fn pause_channel(&mut self, channel: &str, reason: PauseReason) -> Result<usize, Error> {
        self.pause_channel_at(channel, reason, self.now)
    }
    /// Pause at explicit time. User-origin jobs remain worker/device-policy controlled.
    pub fn pause_channel_at(&mut self, channel: &str, reason: PauseReason, now: u64) -> Result<usize, Error> {
        self.clock(now)?;
        validate_name(channel, MAX_CHANNEL_BYTES)?;
        let position = self.paused.iter().position(|ch| ch.channel == channel);
        if position.is_none() && self.paused.len() == self.limits.paused_channels {
            return Err(Error::ResourceExhausted);
        }
        let affected = self
            .jobs
            .iter()
            .filter(|j| {
                j.spec.channel == channel
                    && j.spec.origin != Origin::User
                    && j.status != Status::Paused(reason)
            })
            .count();
        self.event_space(affected)?;
        self.now = now;
        if let Some(index) = position {
            self.paused[index].reason = reason;
        } else {
            self.paused.push(PausedChannel {
                channel: channel.into(),
                reason,
            });
        }
        for job in &mut self.jobs {
            if job.spec.channel == channel
                && job.spec.origin != Origin::User
                && job.status != Status::Paused(reason)
            {
                cancel(job);
                job.status = Status::Paused(reason);
                self.events.push_back(Event::Stopped {
                    job: job.id,
                    owner: job.spec.owner.clone(),
                    reason: reason.stop_reason(),
                    resumable: true,
                    at_ns: now,
                    stats: job.stats.snapshot(),
                });
            }
        }
        self.dues.retain(|d| {
            self.jobs
                .iter()
                .any(|j| j.id == d.job && j.status == Status::Running)
        });
        self.rebuild();
        Ok(affected)
    }
    /// Resume only by explicit request, preserving the grid and skipping paused ticks.
    pub fn resume_channel(&mut self, channel: &str) -> Result<usize, Error> {
        self.resume_channel_at(channel, self.now)
    }
    /// Expired paused jobs are deleted, never revived by a resume.
    pub fn resume_channel_at(&mut self, channel: &str, now: u64) -> Result<usize, Error> {
        self.clock(now)?;
        validate_name(channel, MAX_CHANNEL_BYTES)?;
        let ids: Vec<_> = self
            .jobs
            .iter()
            .filter(|j| j.spec.channel == channel && j.status != Status::Running)
            .map(|j| j.id)
            .collect();
        self.event_space(ids.len())?;
        self.now = now;
        self.paused.retain(|ch| ch.channel != channel);
        let mut resumed = 0;
        for id in ids {
            let index = self.index(id)?;
            if let Some(reason) = expiry(&self.jobs[index], now) {
                self.retire(id, reason);
                continue;
            }
            let job = &mut self.jobs[index];
            job.status = Status::Running;
            if let Some(next) = job.next {
                let (next, ticks) = grid_after(next, job.spec.period_ns, now);
                job.next = next;
                job.stats.paused_skipped = job.stats.paused_skipped.saturating_add(ticks);
            }
            self.events.push_back(Event::Resumed { job: id, at_ns: now });
            resumed += 1;
        }
        self.rebuild();
        Ok(resumed)
    }
}
