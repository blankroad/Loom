//! The [`Transport`] trait: one diagnostic target on one link.
//!
//! Dyn-compatible without `async-trait`: async methods return [`BoxFuture`] (same shape as
//! `loom_domain_api::BoxFuture`). A transport is owned by exactly one runner (`&mut self`), so it
//! needs `Send` but not `Sync`.

use std::future::Future;
use std::pin::Pin;

use thiserror::Error;
use tokio::time::Instant;

/// Boxed `Send` future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Something received from the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A response started arriving (ISO-TP first frame). Satisfies P2; the PDU must then complete
    /// within the completion limit. DoIP never emits this (its PDUs arrive whole).
    RxStart { at: Instant },
    /// A complete response PDU and the instant the transport received it.
    Pdu { data: Vec<u8>, at: Instant },
}

/// Send-side failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransportError {
    /// Link closed (before hand-off for `submit`).
    #[error("link closed")]
    Closed,
    /// The link generation changed (reconnect / re-activation).
    #[error("link generation changed")]
    GenerationChanged,
    /// Transmit queue full; nothing was sent.
    #[error("transmit queue full")]
    TxFull,
    /// Not handed to the link before the deadline; it may still go out later.
    #[error("not handed to the link before the deadline")]
    TxTimeout,
    /// The transport refused to forward (DoIP diagnostic NACK code).
    #[error("transport NACK 0x{0:02X}")]
    Nack(u8),
    /// No transport acknowledgement (DoIP diagnostic ACK) in time; the request may have arrived.
    #[error("transport acknowledgement timed out")]
    AckTimeout,
    /// PDU empty or too long; nothing was sent.
    #[error("invalid PDU")]
    InvalidPdu,
    /// Transport-level busy (e.g. previous ACK still outstanding); nothing was sent.
    #[error("transport busy")]
    Busy,
    #[error("{0}")]
    Other(String),
}

impl TransportError {
    /// `submit` failed with nothing on the wire.
    pub fn nothing_sent(&self) -> bool {
        matches!(
            self,
            TransportError::Closed
                | TransportError::TxFull
                | TransportError::InvalidPdu
                | TransportError::Busy
        )
    }
}

/// Receive-side outcome other than data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RecvError {
    #[error("nothing received before the deadline")]
    Timeout,
    #[error("link closed")]
    Closed,
    #[error("link generation changed")]
    GenerationChanged,
}

/// One diagnostic target on one link.
pub trait Transport: Send {
    /// Link generation (epoch). Changes on every reconnect + re-activation; responses can never
    /// cross generations.
    fn generation(&self) -> u64;

    /// Hand `pdu` to the driver/socket (`write_state = sent`). Resolves with the hand-off instant.
    fn submit<'a>(
        &'a mut self,
        pdu: &'a [u8],
        until: Instant,
    ) -> BoxFuture<'a, Result<Instant, TransportError>>;

    /// Wait for transport confirmation of the last submitted PDU (DoIP diagnostic ACK; immediate
    /// for transports without one). Resolves with the instant the P2 timer starts.
    fn confirm<'a>(&'a mut self, until: Instant) -> BoxFuture<'a, Result<Instant, TransportError>>;

    /// Next received item before `until`.
    fn recv<'a>(&'a mut self, until: Instant) -> BoxFuture<'a, Result<Incoming, RecvError>>;

    /// An item already received, without waiting.
    fn try_recv(&mut self) -> Option<Incoming>;
}
