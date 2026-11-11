use crate::*;
use std::{
    thread::{self, JoinHandle},
    time::Instant,
};
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub queue: QueueConfig,
    pub first_seq: u64,
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            queue: QueueConfig::default(),
            first_seq: 1,
        }
    }
}
#[derive(Debug, Clone)]
pub struct CaptureSummary {
    pub frames_seen: u64,
    pub frames_enqueued: u64,
    pub record_dropped: u64,
    pub next_seq: u64,
    pub final_stats: DropStats,
}
#[derive(Debug, Clone)]
pub struct StopHandle(pub(crate) StopSignal);
impl StopHandle {
    pub fn stop(&self) {
        self.0.stop();
    }
    pub fn is_stopped(&self) -> bool {
        self.0.is_stopped()
    }
}
#[derive(Debug)]
pub struct CaptureLoop {
    queue: BoundedFrameQueue,
    stop: StopHandle,
    thread: Option<JoinHandle<Result<CaptureSummary>>>,
    config: CaptureConfig,
    origin: Instant,
}
impl CaptureLoop {
    pub fn start(source: impl CaptureSource + 'static, config: CaptureConfig) -> Result<Self> {
        Self::start_at(source, config, Instant::now(), false)
    }
    fn start_at(
        mut source: impl CaptureSource + 'static,
        config: CaptureConfig,
        origin: Instant,
        restarted: bool,
    ) -> Result<Self> {
        if config.first_seq == 0 {
            return Err(CaptureError::Invalid("source_seq starts at 1 or later"));
        }
        source.interface().validate()?;
        let queue = BoundedFrameQueue::new(config.queue)?;
        let signal = StopSignal::default();
        source.set_stop_signal(signal.clone());
        let worker_queue = queue.clone();
        let worker_signal = signal.clone();
        let first = config.first_seq;
        let thread = thread::Builder::new()
            .name("loom-eth-capture".into())
            .spawn(move || {
                let result = run(
                    &mut source,
                    &worker_queue,
                    &worker_signal,
                    first,
                    origin,
                    restarted,
                );
                worker_queue.close();
                result
            })?;
        Ok(Self {
            queue,
            stop: StopHandle(signal),
            thread: Some(thread),
            config,
            origin,
        })
    }
    pub fn queue(&self) -> BoundedFrameQueue {
        self.queue.clone()
    }
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
    pub fn join(mut self) -> Result<CaptureSummary> {
        self.thread
            .take()
            .ok_or(CaptureError::Invalid("capture already joined"))?
            .join()
            .map_err(|_| CaptureError::ThreadPanic)?
    }
    pub fn restart(self, source: impl CaptureSource + 'static) -> Result<Self> {
        let mut config = self.config.clone();
        let origin = self.origin;
        self.stop.stop();
        let summary = self.join()?;
        config.first_seq = summary.next_seq;
        Self::start_at(source, config, origin, true)
    }
}
impl Drop for CaptureLoop {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn unknown(s: DropStats) -> bool {
    s.availability != StatsAvailability::Available || s.kernel_drop.is_none() || s.if_drop.is_none()
}
#[derive(Debug)]
struct StatsTracker {
    stats: DropStats,
    kernel: Option<u64>,
    interface: Option<u64>,
    recv: Option<u64>,
}
impl StatsTracker {
    fn new() -> Self {
        Self {
            stats: DropStats::ZERO,
            kernel: Some(0),
            interface: Some(0),
            recv: Some(0),
        }
    }
}
fn sample(previous: &mut StatsTracker, current: DropStats, queue: &BoundedFrameQueue) {
    if unknown(current)
        && (!unknown(previous.stats)
            || previous.stats.availability != current.availability
            || previous.stats.kernel_drop.is_some() != current.kernel_drop.is_some()
            || previous.stats.if_drop.is_some() != current.if_drop.is_some())
    {
        queue.report(GapReport::source(GapCause::StatsUnavailable, None));
    }
    let mut reset = false;
    for (old, new, cause) in [
        (&mut previous.kernel, current.kernel_drop, GapCause::KernelDrop),
        (&mut previous.interface, current.if_drop, GapCause::IfDrop),
    ] {
        if let Some(new) = new {
            if let Some(last) = *old {
                if new < last {
                    reset = true;
                } else if new > last {
                    queue.report(GapReport::source(cause, Some(new - last)));
                }
            } else if new > 0 {
                queue.report(GapReport::source(cause, Some(new)));
            }
            *old = Some(new);
        }
    }
    if current.availability != StatsAvailability::Unavailable {
        if previous.recv.is_some_and(|last| current.recv < last) {
            reset = true;
        }
        previous.recv = Some(current.recv);
    }
    if reset {
        queue.report(GapReport::source(GapCause::CaptureRestart, None));
    }
    previous.stats = current;
}
fn run(
    source: &mut impl CaptureSource,
    queue: &BoundedFrameQueue,
    stop: &StopSignal,
    first: u64,
    origin: Instant,
    restarted: bool,
) -> Result<CaptureSummary> {
    let mut summary = CaptureSummary {
        frames_seen: 0,
        frames_enqueued: 0,
        record_dropped: 0,
        next_seq: first,
        final_stats: source.stats(),
    };
    if restarted {
        queue.report(GapReport::source(GapCause::CaptureRestart, None));
    }
    let mut tracker = StatsTracker::new();
    sample(&mut tracker, summary.final_stats, queue);
    let mut sampled = Instant::now();
    let ts_source = source.timestamp_source();
    while !stop.is_stopped() {
        let wait = CAP_STATS_INTERVAL
            .saturating_sub(sampled.elapsed())
            .min(MAX_SOURCE_POLL);
        let frame = source.next(wait)?;
        if let Some(raw) = frame {
            let seq = summary.next_seq;
            summary.next_seq = seq
                .checked_add(1)
                .ok_or(CaptureError::Limit("source_seq exhausted"))?;
            raw.validate()?;
            summary.frames_seen += 1;
            if queue.push(CapturedFrame {
                source_seq: seq,
                host_mono_ns: origin.elapsed().as_nanos(),
                ts_source,
                raw,
            })? {
                summary.frames_enqueued += 1;
            } else {
                summary.record_dropped += 1;
            }
        } else if source.is_exhausted() {
            break;
        } else {
            stop.wait(Duration::from_millis(1).min(wait));
        }
        if sampled.elapsed() >= CAP_STATS_INTERVAL {
            sample(&mut tracker, source.stats(), queue);
            summary.final_stats = tracker.stats;
            sampled = Instant::now();
        }
    }
    sample(&mut tracker, source.stats(), queue);
    summary.final_stats = tracker.stats;
    Ok(summary)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drop_deltas_unknown_and_counter_reset() {
        let q = BoundedFrameQueue::new(QueueConfig {
            max_bytes: 4096,
            max_frames: 2,
        })
        .expect("queue");
        let mut old = StatsTracker::new();
        sample(
            &mut old,
            DropStats {
                recv: 4,
                kernel_drop: Some(3),
                if_drop: Some(2),
                ..DropStats::ZERO
            },
            &q,
        );
        sample(
            &mut old,
            DropStats {
                recv: 5,
                kernel_drop: Some(3),
                if_drop: Some(2),
                ..DropStats::ZERO
            },
            &q,
        );
        sample(&mut old, DropStats::ZERO, &q);
        sample(&mut old, DropStats::UNAVAILABLE, &q);
        sample(&mut old, DropStats::UNAVAILABLE, &q);
        let mut counts = GapCounts::default();
        while let Some(g) = q.try_gap() {
            counts.kernel_drop += g.counts.kernel_drop;
            counts.if_drop += g.counts.if_drop;
            counts.stats_unavailable += g.counts.stats_unavailable;
            counts.capture_restart += g.counts.capture_restart;
        }
        assert_eq!(
            (
                counts.kernel_drop,
                counts.if_drop,
                counts.stats_unavailable,
                counts.capture_restart
            ),
            (3, 2, 1, 1)
        );
    }
    #[test]
    fn unavailable_counter_recovery_counts_only_new_drops() {
        let q = BoundedFrameQueue::new(QueueConfig {
            max_bytes: 4096,
            max_frames: 2,
        })
        .expect("queue");
        let mut tracker = StatsTracker::new();
        sample(
            &mut tracker,
            DropStats {
                recv: 10,
                kernel_drop: Some(5),
                if_drop: None,
                availability: StatsAvailability::Partial,
            },
            &q,
        );
        sample(&mut tracker, DropStats::UNAVAILABLE, &q);
        sample(
            &mut tracker,
            DropStats {
                recv: 20,
                kernel_drop: Some(7),
                if_drop: Some(3),
                ..DropStats::ZERO
            },
            &q,
        );
        let mut kernel = 0;
        let mut interface = 0;
        let mut restart = 0;
        while let Some(g) = q.try_gap() {
            kernel += g.counts.kernel_drop;
            interface += g.counts.if_drop;
            restart += g.counts.capture_restart;
        }
        assert_eq!((kernel, interface, restart), (7, 3, 0));
    }
}
