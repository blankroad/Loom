//! Virtual UDS ECU for Loom simulation and tests.
//!
//! Pure and deterministic: [`SimEcu::handle`] takes the current monotonic time as an argument and
//! returns the reply schedule; it never sleeps, reads clocks or does I/O. Transport adapters
//! (DoIP entity in `loom-uds-doip`, CAN-TP node in the CAN worker) feed requests in and send the
//! replies out at `request time + Reply::delay`.

mod ecu;
mod fault;
mod network;

use std::time::Duration;

pub use ecu::{SECURITY_XOR_DEFAULT, SimDid, SimEcu, SimEcuConfig};
pub use fault::{FaultAction, FaultMatch, FaultRule};
pub use network::SimNetwork;

/// Most replies one request can produce (`0x78` pendings + final).
/// [`FaultAction::PendingThen`] counts above `MAX_REPLIES_PER_REQUEST - 1` are clamped.
pub const MAX_REPLIES_PER_REQUEST: usize = 64;
/// Most fault rules per ECU. [`SimEcu::add_fault`] returns [`SimError::TooMany`] beyond this.
pub const MAX_FAULT_RULES: usize = 32;
/// Most ECUs in one [`SimNetwork`]. [`SimNetwork::insert`] returns [`SimError::TooMany`] beyond this.
pub const MAX_SIM_ECUS: usize = 64;
/// Most DIDs per simulated ECU (config validation).
pub const MAX_SIM_DIDS: usize = 256;
/// Most stored DTCs per simulated ECU (config validation).
pub const MAX_SIM_DTCS: usize = 256;

/// One scheduled reply. `delay` is measured from the moment the request arrived, and the
/// replies of one request are sorted by `delay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub delay: Duration,
    pub bytes: Vec<u8>,
}

/// The reply schedule for one request (possibly empty: suppressed, dropped or no response).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Replies {
    items: Vec<Reply>,
}

impl Replies {
    pub fn none() -> Self {
        Self::default()
    }

    fn one(bytes: Vec<u8>) -> Self {
        Self {
            items: vec![Reply {
                delay: Duration::ZERO,
                bytes,
            }],
        }
    }

    /// Append, dropping anything past [`MAX_REPLIES_PER_REQUEST`].
    fn push(&mut self, reply: Reply) {
        if self.items.len() < MAX_REPLIES_PER_REQUEST {
            self.items.push(reply);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn as_slice(&self) -> &[Reply] {
        &self.items
    }

    pub fn into_vec(self) -> Vec<Reply> {
        self.items
    }

    /// The last reply (the final answer when the schedule is not empty).
    pub fn last(&self) -> Option<&Reply> {
        self.items.last()
    }
}

/// How the request reached the ECU. Functional requests suppress the NRCs ISO 14229-1 §7.5
/// lists for functional addressing (0x11, 0x12, 0x31, 0x7E, 0x7F).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Addressing {
    #[default]
    Physical,
    Functional,
}

/// Configuration and capacity errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    #[error("too many {what}: limit {max}")]
    TooMany { what: &'static str, max: usize },
    #[error("invalid sim config: {0}")]
    Config(String),
}
