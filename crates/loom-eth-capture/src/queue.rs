use crate::*;
use std::{
    collections::VecDeque,
    mem::size_of,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};
/// Raw queue hard/default byte ceiling, including reserved pointer storage and owned frames.
/// Overflow drops the newest frame and emits a Record gap; never blocks capture.
pub const MAX_QUEUE_BYTES: usize = 128 * 1024 * 1024;
/// Raw queue hard/default frame count ceiling; overflow drops newest.
pub const MAX_QUEUE_FRAMES: usize = 512_000;
/// Gap channel ceiling; overflow coalesces all pending reports into one bounded summary.
pub const MAX_GAP_REPORTS: usize = 1024;
/// Consumer waits are capped to one second per call; callers may loop for longer waits.
pub const MAX_QUEUE_WAIT: Duration = Duration::from_secs(1);
#[derive(Debug, Clone, Copy)]
pub struct QueueConfig {
    pub max_bytes: usize,
    pub max_frames: usize,
}
impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_bytes: MAX_QUEUE_BYTES,
            max_frames: MAX_QUEUE_FRAMES,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapKind {
    Record,
    Source,
    Mixed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapCause {
    QueueOverflow,
    KernelDrop,
    IfDrop,
    StatsUnavailable,
    CaptureRestart,
    Coalesced,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GapCounts {
    pub record: u64,
    pub kernel_drop: u64,
    pub if_drop: u64,
    pub stats_unavailable: u64,
    pub capture_restart: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapReport {
    pub kind: GapKind,
    pub cause: GapCause,
    /// Contiguous for a normal Record report; bounding range when `coalesced` is true.
    pub first_seq: Option<u64>,
    pub last_seq: Option<u64>,
    pub count: Option<u64>,
    pub coalesced: bool,
    pub counts: GapCounts,
}
impl GapReport {
    pub(crate) fn record(seq: u64) -> Self {
        Self {
            kind: GapKind::Record,
            cause: GapCause::QueueOverflow,
            first_seq: Some(seq),
            last_seq: Some(seq),
            count: Some(1),
            coalesced: false,
            counts: GapCounts {
                record: 1,
                ..Default::default()
            },
        }
    }
    pub(crate) fn source(cause: GapCause, count: Option<u64>) -> Self {
        let mut counts = GapCounts::default();
        match cause {
            GapCause::KernelDrop => counts.kernel_drop = count.unwrap_or(0),
            GapCause::IfDrop => counts.if_drop = count.unwrap_or(0),
            GapCause::StatsUnavailable => counts.stats_unavailable = 1,
            GapCause::CaptureRestart => counts.capture_restart = 1,
            _ => (),
        }
        Self {
            kind: GapKind::Source,
            cause,
            first_seq: None,
            last_seq: None,
            count,
            coalesced: false,
            counts,
        }
    }
    fn merge(&mut self, other: Self) {
        self.first_seq = match (self.first_seq, other.first_seq) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.last_seq = match (self.last_seq, other.last_seq) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.count = self.count.zip(other.count).and_then(|(a, b)| a.checked_add(b));
        self.counts.record = self.counts.record.saturating_add(other.counts.record);
        self.counts.kernel_drop = self.counts.kernel_drop.saturating_add(other.counts.kernel_drop);
        self.counts.if_drop = self.counts.if_drop.saturating_add(other.counts.if_drop);
        self.counts.stats_unavailable = self
            .counts
            .stats_unavailable
            .saturating_add(other.counts.stats_unavailable);
        self.counts.capture_restart = self
            .counts
            .capture_restart
            .saturating_add(other.counts.capture_restart);
    }
}
#[derive(Debug)]
struct State {
    frames: VecDeque<Box<CapturedFrame>>,
    gaps: VecDeque<GapReport>,
    bytes: usize,
    base_bytes: usize,
    closed: bool,
}
#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    config: QueueConfig,
}
#[derive(Debug, Clone)]
pub struct BoundedFrameQueue(Arc<Shared>);
impl BoundedFrameQueue {
    pub fn new(config: QueueConfig) -> Result<Self> {
        if config.max_frames == 0
            || config.max_frames > MAX_QUEUE_FRAMES
            || config.max_bytes > MAX_QUEUE_BYTES
        {
            return Err(CaptureError::Invalid("queue limits"));
        }
        let requested = config
            .max_frames
            .checked_mul(size_of::<Box<CapturedFrame>>())
            .ok_or(CaptureError::Limit("queue allocation"))?;
        if requested >= config.max_bytes {
            return Err(CaptureError::Invalid("queue bytes cannot hold pointer ring"));
        }
        let frames = VecDeque::with_capacity(config.max_frames);
        let base_bytes = frames.capacity() * size_of::<Box<CapturedFrame>>();
        if base_bytes >= config.max_bytes {
            return Err(CaptureError::Invalid("queue pointer allocation exceeds budget"));
        }
        Ok(Self(Arc::new(Shared {
            config,
            ready: Condvar::new(),
            state: Mutex::new(State {
                frames,
                gaps: VecDeque::with_capacity(MAX_GAP_REPORTS),
                bytes: base_bytes,
                base_bytes,
                closed: false,
            }),
        })))
    }
    fn push_gap_locked(state: &mut State, report: GapReport) {
        if let Some(last) = state.gaps.back_mut()
            && last.kind == GapKind::Record
            && report.kind == GapKind::Record
            && !last.coalesced
            && last.last_seq.and_then(|n| n.checked_add(1)) == report.first_seq
        {
            last.merge(report);
            return;
        }
        if state.gaps.len() == MAX_GAP_REPORTS {
            let mut merged = report;
            while let Some(old) = state.gaps.pop_front() {
                merged.merge(old);
            }
            merged.kind = GapKind::Mixed;
            merged.cause = GapCause::Coalesced;
            merged.coalesced = true;
            state.gaps.push_back(merged);
        } else {
            state.gaps.push_back(report);
        }
    }
    pub(crate) fn report(&self, report: GapReport) {
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        Self::push_gap_locked(&mut s, report);
        self.0.ready.notify_all();
    }
    pub fn push(&self, frame: CapturedFrame) -> Result<bool> {
        frame.raw.validate()?;
        let charge = size_of::<CapturedFrame>().saturating_add(frame.raw.heap_bytes());
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.closed {
            return Err(CaptureError::Invalid("closed queue"));
        }
        if s.frames.len() >= self.0.config.max_frames
            || charge > self.0.config.max_bytes.saturating_sub(s.bytes)
        {
            Self::push_gap_locked(&mut s, GapReport::record(frame.source_seq));
            self.0.ready.notify_all();
            return Ok(false);
        }
        s.bytes += charge;
        s.frames.push_back(Box::new(frame));
        self.0.ready.notify_one();
        Ok(true)
    }
    pub fn pop_timeout(&self, timeout: Duration) -> Option<CapturedFrame> {
        let start = Instant::now();
        let timeout = timeout.min(MAX_QUEUE_WAIT);
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        while s.frames.is_empty() && !s.closed {
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            let (next, _) = self
                .0
                .ready
                .wait_timeout(s, remaining)
                .unwrap_or_else(|p| p.into_inner());
            s = next;
        }
        let frame = s.frames.pop_front()?;
        s.bytes -= size_of::<CapturedFrame>() + frame.raw.heap_bytes();
        Some(*frame)
    }
    pub fn try_gap(&self) -> Option<GapReport> {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .gaps
            .pop_front()
    }
    pub fn len(&self) -> usize {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .frames
            .len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn byte_usage(&self) -> usize {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).bytes
    }
    pub fn reserved_bytes(&self) -> usize {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).base_bytes
    }
    pub fn gap_len(&self) -> usize {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).gaps.len()
    }
    pub fn is_closed(&self) -> bool {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).closed
    }
    pub(crate) fn close(&self) {
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        s.closed = true;
        self.0.ready.notify_all();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gap_overflow_coalesces_mixed_causes_without_inventing_record_counts() {
        let q = BoundedFrameQueue::new(QueueConfig {
            max_bytes: 4096,
            max_frames: 2,
        })
        .expect("queue");
        for n in 0..3000 {
            q.report(GapReport::record(n * 2));
            q.report(GapReport::source(GapCause::KernelDrop, Some(3)));
            q.report(GapReport::source(GapCause::StatsUnavailable, None));
            assert!(q.gap_len() <= MAX_GAP_REPORTS);
        }
        let mut counts = GapCounts::default();
        let mut coalesced = false;
        while let Some(g) = q.try_gap() {
            counts.record += g.counts.record;
            counts.kernel_drop += g.counts.kernel_drop;
            counts.stats_unavailable += g.counts.stats_unavailable;
            if g.coalesced {
                coalesced = true;
                assert!(g.count.is_none());
            }
        }
        assert!(coalesced);
        assert_eq!(
            (counts.record, counts.kernel_drop, counts.stats_unavailable),
            (3000, 9000, 3000)
        );
    }
}
