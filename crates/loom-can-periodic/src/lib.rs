//! Bounded periodic CAN scheduling without threads, IO or an owned clock.
//!
//! Call `poll`, drain due frames/events, check `is_due_current` immediately before
//! driver enqueue, then report submission/failure. Absolute grid deadlines never
//! drift and missed ticks never accumulate into a catch-up burst.
#![forbid(unsafe_code)]

mod scheduler;
mod stats;

use loom_can_frame::{CanFrame, FrameKind};
pub use scheduler::{JobSnapshot, Scheduler};
pub use stats::StatsSnapshot;
use std::fmt;

/// Worker admission limits from the CAN plan.
pub const MAX_JOBS: usize = 128;
/// Maximum accepted patches awaiting a real transmission tick.
pub const MAX_PATCHES: usize = 256;
/// Maximum bounded event output queue.
pub const MAX_EVENTS: usize = 256;
/// Default cumulative lifetime, including paused time (one hour).
pub const DEFAULT_MAX_DURATION_NS: u64 = 3_600_000_000_000;
/// Maximum channel identifier length in bytes.
pub const MAX_CHANNEL_BYTES: usize = 64;
/// Maximum owner identifier length in bytes.
pub const MAX_OWNER_BYTES: usize = 128;

/// Server-established request origin. Device simulation is not a periodic client job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Agent,
    User,
    Script,
}

/// Unique job identity. A takeover creates a new identity, never reuses the old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// Correlates an actual enqueue result with one specific due frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchId {
    pub job: JobId,
    pub sequence: u64,
}

/// One admitted periodic producer. The first tick is start + period + offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub channel: String,
    pub frame: CanFrame,
    pub period_ns: u64,
    pub offset_ns: u64,
    pub origin: Origin,
    pub owner: String,
    pub lease_ns: Option<u64>,
    pub max_duration_ns: u64,
    /// Number of successful driver submissions; skips/failures do not consume it.
    pub count: Option<u64>,
}
impl JobSpec {
    /// No lease, zero offset, unlimited count, one-hour maximum lifetime.
    pub fn new(
        channel: impl Into<String>,
        frame: CanFrame,
        period_ns: u64,
        origin: Origin,
        owner: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            frame,
            period_ns,
            offset_ns: 0,
            origin,
            owner: owner.into(),
            lease_ns: None,
            max_duration_ns: DEFAULT_MAX_DURATION_NS,
            count: None,
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        validate_name(&self.channel, MAX_CHANNEL_BYTES)?;
        validate_name(&self.owner, MAX_OWNER_BYTES)?;
        if self.period_ns == 0
            || self.max_duration_ns == 0
            || self.lease_ns == Some(0)
            || self.count == Some(0)
        {
            return Err(Error::InvalidSpec);
        }
        if self.lease_ns.is_some_and(|v| v > self.max_duration_ns) {
            return Err(Error::LeaseCapExceeded);
        }
        Ok(())
    }
}
pub(crate) fn validate_name(value: &str, max: usize) -> Result<(), Error> {
    if value.is_empty() || value.len() > max {
        Err(Error::InvalidName)
    } else {
        Ok(())
    }
}

/// Typed patch with fixed-size storage, independent of DBC/signal interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    kind: PatchKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchKind {
    Replace {
        data: [u8; 64],
        len: u8,
    },
    Masked {
        offset: u8,
        mask: [u8; 64],
        bytes: [u8; 64],
        len: u8,
    },
}
impl Patch {
    /// Replace payload only. CAN id and CC/FD/BRS/ESI flags remain unchanged.
    pub fn replace(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 64 {
            return Err(Error::InvalidPatch);
        }
        let mut data = [0; 64];
        data[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            kind: PatchKind::Replace {
                data,
                len: bytes.len() as u8,
            },
        })
    }
    /// Atomic byte mask: result = (old & !mask) | (bytes & mask).
    pub fn masked(offset: usize, mask: &[u8], bytes: &[u8]) -> Result<Self, Error> {
        if mask.is_empty() || mask.len() != bytes.len() || offset > 63 || mask.len() > 64 - offset {
            return Err(Error::InvalidPatch);
        }
        let mut masks = [0; 64];
        let mut data = [0; 64];
        masks[..mask.len()].copy_from_slice(mask);
        data[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            kind: PatchKind::Masked {
                offset: offset as u8,
                mask: masks,
                bytes: data,
                len: bytes.len() as u8,
            },
        })
    }
    pub(crate) fn apply(self, frame: CanFrame) -> Result<CanFrame, Error> {
        if frame.is_remote() {
            return Err(Error::InvalidPatch);
        }
        let mut data = [0; 64];
        data[..frame.len()].copy_from_slice(frame.data());
        let len = match self.kind {
            PatchKind::Replace {
                data: replacement,
                len,
            } => {
                data = replacement;
                usize::from(len)
            }
            PatchKind::Masked {
                offset,
                mask,
                bytes,
                len,
            } => {
                let start = usize::from(offset);
                let count = usize::from(len);
                if start + count > frame.len() {
                    return Err(Error::InvalidPatch);
                }
                for i in 0..count {
                    data[start + i] = (data[start + i] & !mask[i]) | (bytes[i] & mask[i]);
                }
                frame.len()
            }
        };
        match frame.kind() {
            FrameKind::Classic { .. } => CanFrame::classic(frame.id(), &data[..len]),
            FrameKind::Fd { brs, esi } => CanFrame::fd(frame.id(), &data[..len], brs, esi),
        }
        .map_err(|_| Error::InvalidPatch)
    }
}

/// Bounded output for a single tick. Submission is not a bus confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub job: JobId,
    pub dispatch: DispatchId,
    pub channel: String,
    pub frame: CanFrame,
    pub origin: Origin,
    pub owner: String,
    pub scheduled_ns: u64,
    pub lateness_ns: u64,
    pub revision: u64,
}

/// Why transmission stopped. ErrorPassive/BusOff can be explicitly resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    LeaseExpired,
    Completed,
    MaxDuration,
    Stopped,
    Latched,
    Takeover,
    BusOff,
    ErrorPassive,
}
/// Physical channel problem supplied by the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    ErrorPassive,
    BusOff,
}
impl PauseReason {
    pub(crate) fn stop_reason(self) -> StopReason {
        match self {
            Self::ErrorPassive => StopReason::ErrorPassive,
            Self::BusOff => StopReason::BusOff,
        }
    }
}
/// Fixed-size event facts apart from the bounded owner string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// resumable=true means a retained paused job, not a terminal deletion.
    Stopped {
        job: JobId,
        owner: String,
        reason: StopReason,
        resumable: bool,
        at_ns: u64,
        stats: StatsSnapshot,
    },
    Resumed {
        job: JobId,
        at_ns: u64,
    },
    SendFailed {
        dispatch: DispatchId,
        at_ns: u64,
    },
}

/// Validated limits may be lowered for a worker profile or pressure tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub jobs: usize,
    pub patches: usize,
    pub dues: usize,
    pub events: usize,
    pub paused_channels: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            jobs: MAX_JOBS,
            patches: MAX_PATCHES,
            dues: MAX_JOBS,
            events: MAX_EVENTS,
            paused_channels: MAX_JOBS,
        }
    }
}

/// Admission/clock/CAS errors. OutputFull never partially applies an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidSpec,
    InvalidName,
    InvalidPatch,
    InvalidLimits,
    UnknownJob,
    ResourceExhausted,
    OutputFull,
    Latched,
    TimeWentBackwards,
    TimeOverflow,
    RevisionConflict { current: u64 },
    Conflict { owner: String },
    LeaseCapExceeded,
    NoLease,
    Expired,
    StaleDispatch,
    SubmissionBeforeDispatch,
    CounterExhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
