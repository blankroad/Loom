//! Core-independent bounded capture, replay and synthetic tap. No privileged IO by default.
mod capture_loop;
#[cfg(feature = "live")]
mod interfaces;
#[cfg(feature = "live")]
mod live;
mod queue;
mod replay;
mod sim;
pub use capture_loop::{CaptureConfig, CaptureLoop, CaptureSummary, StopHandle};
#[cfg(feature = "live")]
pub use interfaces::{
    InterfaceInfo, MAX_INTERFACE_ADDRESSES, MAX_INTERFACE_DESCRIPTION, MAX_INTERFACE_REASON_BYTES,
    MAX_LIVE_INTERFACES, list_interfaces,
};
#[cfg(feature = "live")]
pub use live::{LiveConfig, LiveSource, MAX_BPF_BYTES};
pub use loom_eth_pcapng::{Direction, Interface, LoomMeta, TimestampResolution};
pub use queue::{BoundedFrameQueue, GapCause, GapCounts, GapKind, GapReport, QueueConfig};
pub use replay::{ReplayOptions, ReplaySource, ReplaySpeed};
pub use sim::{Connection, Endpoint, SimTapHandle, SimTapSource, SocketConnection, TcpFlags};
use std::{
    fmt, io,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

/// Frames larger than this are rejected, never silently truncated.
pub const MAX_FRAME_BYTES: usize = loom_eth_pcapng::MAX_PACKET_BYTES;
/// Source timeout contract: `next` must return/wake within this duration when requested.
pub const MAX_SOURCE_POLL: Duration = Duration::from_millis(20);
/// Driver counters are sampled at this interval, independently of traffic arrival.
pub const CAP_STATS_INTERVAL: Duration = Duration::from_millis(100);
/// Clamp individual condvar sleeps to avoid OS deadline overflow; callers may poll again.
pub const MAX_SOURCE_WAIT: Duration = Duration::from_secs(1);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampSource {
    Replay,
    Sim,
    PcapHost,
    PcapAdapter,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub ts_ns: u128,
    pub caplen: u32,
    pub origlen: u32,
    pub data: Vec<u8>,
    /// Preserve per-packet interface/section identity for mixed-interface replay.
    pub interface_id: u32,
    pub section_index: u64,
    pub interface: Interface,
    /// All ordinary EPB comments, including malformed/unknown-version loom comments.
    pub comments: Vec<String>,
    /// Valid X10 attribution, distinct from this replay session's capture source sequence.
    pub meta: Option<LoomMeta>,
}
impl RawFrame {
    pub fn validate(&self) -> Result<()> {
        if self.data.len() > MAX_FRAME_BYTES {
            return Err(CaptureError::Limit("frame bytes"));
        }
        if self.caplen as usize != self.data.len()
            || self.caplen > self.origlen
            || (self.interface.snaplen != 0 && self.caplen > self.interface.snaplen)
        {
            return Err(CaptureError::Invalid("frame lengths"));
        }
        if self.comments.len() + usize::from(self.meta.is_some()) > loom_eth_pcapng::MAX_COMMENTS
            || self
                .comments
                .iter()
                .any(|c| c.len() > loom_eth_pcapng::MAX_COMMENT_BYTES)
        {
            return Err(CaptureError::Limit("frame comments"));
        }
        if let Some(meta) = &self.meta {
            meta.to_comment()?;
        }
        self.interface.validate().map_err(CaptureError::Format)
    }
    pub(crate) fn heap_bytes(&self) -> usize {
        self.data
            .capacity()
            .saturating_add(self.interface.name.capacity())
            .saturating_add(self.comments.capacity() * std::mem::size_of::<String>())
            .saturating_add(self.comments.iter().map(String::capacity).sum::<usize>())
            .saturating_add(
                self.meta
                    .as_ref()
                    .and_then(|m| m.req.as_ref())
                    .map_or(0, String::capacity),
            )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub source_seq: u64,
    /// Monotonic nanoseconds since this capture session's origin, preserved on restart.
    pub host_mono_ns: u128,
    pub ts_source: TimestampSource,
    pub raw: RawFrame,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsAvailability {
    Available,
    Partial,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropStats {
    pub availability: StatsAvailability,
    pub recv: u64,
    pub kernel_drop: Option<u64>,
    pub if_drop: Option<u64>,
}
impl DropStats {
    pub const UNAVAILABLE: Self = Self {
        availability: StatsAvailability::Unavailable,
        recv: 0,
        kernel_drop: None,
        if_drop: None,
    };
    pub const ZERO: Self = Self {
        availability: StatsAvailability::Available,
        recv: 0,
        kernel_drop: Some(0),
        if_drop: Some(0),
    };
}
/// Implementations must honor `timeout`, keep all internal buffers bounded and return
/// `None` for a timeout. Finite sources also implement `is_exhausted` to distinguish EOF.
pub trait CaptureSource: Send + fmt::Debug {
    fn interface(&self) -> Interface;
    fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>>;
    fn stats(&mut self) -> DropStats;
    fn send(&mut self, _frame: &[u8]) -> Result<()> {
        Err(CaptureError::Unsupported("send"))
    }
    fn timestamp_source(&self) -> TimestampSource;
    fn is_exhausted(&self) -> bool {
        false
    }
    /// Wake interruptible waits when stopped. Custom sources may rely on the timeout contract.
    fn set_stop_signal(&mut self, _signal: StopSignal) {}
}
#[derive(Debug)]
pub enum CaptureError {
    Io(io::Error),
    Format(loom_eth_pcapng::Error),
    Permission(String),
    /// BPF syntax/compile error; callers map this to E_USAGE (exit 2).
    Bpf {
        msg: String,
    },
    Backend(String),
    Invalid(&'static str),
    Limit(&'static str),
    Unsupported(&'static str),
    ThreadPanic,
}
pub type Result<T> = std::result::Result<T, CaptureError>;
impl From<io::Error> for CaptureError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<loom_eth_pcapng::Error> for CaptureError {
    fn from(e: loom_eth_pcapng::Error) -> Self {
        Self::Format(e)
    }
}
impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CaptureError {}
#[derive(Debug, Clone, Default)]
pub struct StopSignal(Arc<(Mutex<bool>, Condvar)>);
impl StopSignal {
    pub fn is_stopped(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn stop(&self) {
        let mut flag = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        *flag = true;
        self.0.1.notify_all();
    }
    pub(crate) fn wake(&self) {
        let _guard = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        self.0.1.notify_all();
    }
    pub(crate) fn wait(&self, d: Duration) {
        let flag = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        if !*flag {
            let _result = self
                .0
                .1
                .wait_timeout(flag, d.min(MAX_SOURCE_WAIT))
                .unwrap_or_else(|p| p.into_inner());
        }
    }
}

pub use queue::{MAX_GAP_REPORTS, MAX_QUEUE_BYTES, MAX_QUEUE_FRAMES};
pub use sim::{MAX_SIM_BYTES, MAX_SIM_CONNECTIONS, MAX_SIM_FRAMES};
