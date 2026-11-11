//! Bounded, sans-IO ISO-TP for normal 11/29-bit addressing.
//!
//! The caller owns the monotonic clock, driver, arbitration and raw-frame journal.
//! Drain outputs, submit each frame no earlier than `not_before_ns`, and report
//! actual submission/confirmation using its handle. No driver acceptance implies
//! a bus confirmation. See the crate README for generation fencing and bounds.
#![forbid(unsafe_code)]

mod codec;
mod engine;

pub use codec::{FlowStatus, Pci, decode, encode_cf, encode_fc, encode_ff, encode_sf, st_min_ns};
pub use engine::{AbortReport, Engine, Event, FrameHandle, FrameRequest, Stats, TransferId, TxState};
use loom_can_frame::{CanId, len_to_dlc};
use std::fmt;

/// Fixed protocol limit; 32-bit FF length encoding is outside P0.
pub const MAX_PDU: usize = 4095;
/// Maximum number of data frames for a 4095-byte PDU at TX_DL=8.
pub const MAX_DATA_FRAMES: usize = 586;
/// Hard cap on output entries, independently applied to frames and events.
pub const MAX_OUTPUTS: usize = 64;
/// Hard cap on transfers awaiting optional bus confirmations.
pub const MAX_TRANSFERS: usize = 4;
/// Hard cap on outstanding flow-control frame handles.
pub const MAX_CONTROLS: usize = 64;

/// Reserved extended/mixed address bytes are retained for the future API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressing {
    Normal,
    Extended { tx_address: u8, rx_address: u8 },
    Mixed { address_extension: u8 },
}

/// Explicit, disabled-by-default simulation faults. Hardware gating belongs to the worker.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FaultPlan {
    pub drop_cf_number: Option<u16>,
    pub wrong_sn_cf_number: Option<u16>,
    pub fc_delay_ms: u32,
    pub fc_wait_count: u8,
    pub fc_overflow: bool,
}

/// Endpoint parameters. RX CAN_DL is learned from each incoming FF, not TX_DL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoTpConfig {
    pub tx_id: CanId,
    pub rx_id: CanId,
    pub addressing: Addressing,
    pub can_fd: bool,
    pub bitrate_switch: bool,
    pub tx_dl: u8,
    pub padding: Option<u8>,
    pub block_size: u8,
    pub st_min: u8,
    pub n_bs_ms: u32,
    pub n_cr_ms: u32,
    pub wft_max: u8,
    pub max_pdu: usize,
    pub faults: FaultPlan,
}

impl IsoTpConfig {
    /// Classic CAN defaults: DL8, padding CC, BS8, STmin0, 1s timeouts, WAIT limit10.
    pub fn new(tx_id: CanId, rx_id: CanId) -> Self {
        Self {
            tx_id,
            rx_id,
            addressing: Addressing::Normal,
            can_fd: false,
            bitrate_switch: false,
            tx_dl: 8,
            padding: Some(0xCC),
            block_size: 8,
            st_min: 0,
            n_bs_ms: 1000,
            n_cr_ms: 1000,
            wft_max: 10,
            max_pdu: MAX_PDU,
            faults: FaultPlan::default(),
        }
    }

    /// Validate once at the endpoint boundary, before allocating buffers.
    pub fn validate(&self) -> Result<(), Error> {
        if self.addressing != Addressing::Normal {
            return Err(Error::Unsupported);
        }
        if self.tx_dl < 8
            || len_to_dlc(self.tx_dl as usize, self.can_fd).is_err()
            || (!self.can_fd && self.tx_dl != 8)
        {
            return Err(Error::InvalidConfig("TX_DL"));
        }
        if !self.can_fd && self.bitrate_switch {
            return Err(Error::InvalidConfig("BRS requires FD"));
        }
        if self.max_pdu == 0 || self.max_pdu > MAX_PDU {
            return Err(Error::InvalidConfig("max_pdu"));
        }
        // We send st_min in our FC; ISO 15765-2 reserves 0x80..=0xF0 and 0xFA..=0xFF for
        // senders. (Received reserved values are still accepted and mean 127 ms.)
        if !matches!(self.st_min, 0..=0x7F | 0xF1..=0xF9) {
            return Err(Error::InvalidConfig("st_min reserved value"));
        }
        if self.n_bs_ms == 0 || self.n_cr_ms == 0 {
            return Err(Error::InvalidConfig("zero timeout"));
        }
        if self.faults.fc_wait_count > 32
            || self.faults.fc_delay_ms > 60_000
            || self.faults.drop_cf_number == Some(0)
            || self.faults.wrong_sn_cf_number == Some(0)
        {
            return Err(Error::InvalidConfig("fault bound"));
        }
        Ok(())
    }

    /// Largest PDU that can use an SF at the configured transmit data length.
    pub fn single_frame_capacity(&self) -> usize {
        if self.can_fd && self.tx_dl > 8 {
            self.tx_dl as usize - 2
        } else {
            7
        }
    }
}

/// Queue limits are configurable downwards, never beyond the compile-time hard caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueLimits {
    pub frames: usize,
    pub events: usize,
    pub transfers: usize,
}

impl Default for QueueLimits {
    fn default() -> Self {
        Self {
            frames: 32,
            events: 16,
            transfers: MAX_TRANSFERS,
        }
    }
}

/// A failed admission or malformed protocol input; never a panic for wire input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidConfig(&'static str),
    InvalidLength,
    InvalidPci,
    InvalidFlowStatus,
    PduTooLarge { len: usize, max: usize },
    Busy,
    OutputFull,
    ResourceExhausted,
    NBsTimeout,
    NCrTimeout,
    WrongSequence { expected: u8, actual: u8 },
    UnexpectedPdu,
    FlowOverflow,
    WaitLimit,
    Aborted,
    TimeWentBackwards,
    SubmissionTooEarly,
    ConfirmationBeforeSubmission,
    InvalidHandle,
    CounterExhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
