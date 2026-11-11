//! What the bus delivers: one normalised record per frame, identical for every endpoint.

use loom_can_frame::{Direction, TimedFrame};

/// Who caused a frame (D3 origin enum). Set by the caller from the connection role,
/// never by the frame sender itself. SimNodes are always `Device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    Agent,
    User,
    Device,
    Script,
}

impl Origin {
    pub const ALL: [Origin; 4] = [Origin::Agent, Origin::User, Origin::Device, Origin::Script];

    /// Direction from the worker's point of view: frames from the bus side (devices)
    /// are `Rx`, frames Loom clients put on the bus are `Tx`. The same for every observer.
    pub fn direction(self) -> Direction {
        match self {
            Origin::Device => Direction::Rx,
            _ => Direction::Tx,
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// A bus (one channel name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BusId(pub(crate) u16);

impl BusId {
    pub fn index(self) -> u16 {
        self.0
    }
}

/// An attached endpoint. Contains a generation so a stale id never reaches a new endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EndpointId {
    pub(crate) bus: u16,
    pub(crate) slot: u16,
    pub(crate) generation: u32,
}

impl EndpointId {
    pub fn bus(self) -> BusId {
        BusId(self.bus)
    }
}

/// A started SimNode (slot + generation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) slot: u16,
    pub(crate) generation: u32,
}

/// Who put the frame on the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sender {
    Endpoint(EndpointId),
    Node(NodeId),
}

/// One frame as seen on a sim bus. Fixed size (no heap) so it fits fixed ring slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimRecord {
    pub bus: BusId,
    /// Per-bus, starts at 1, +1 per frame, never reused (the producer's `source_seq`).
    pub source_seq: u64,
    pub origin: Origin,
    pub sender: Sender,
    /// `dir` follows [`Origin::direction`]; `ingest_mono_ns` is the bus time of the frame;
    /// `device_ts` is `None` (no hardware clock in the sim).
    pub frame: TimedFrame,
}
