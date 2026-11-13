//! Transport-neutral link lifecycle: how a domain obtains (and re-obtains) a [`Transport`].
//!
//! A [`Connector`] knows one diagnostic target's addressing and link technology (DoIP, later
//! CAN-TP or a vendor Ethernet diagnostic adapter). Each [`Connector::connect`] yields one
//! [`Link`]: the transport the runner drives, a stream of [`LinkSignal`]s (link loss), and the
//! owned resources (sockets, reader/writer tasks) that must stay alive while the link is in use.
//!
//! The UDS rules (P2/P2*/`0x78`/deadline/quarantine/generation/`outcome_unknown`) stay in
//! [`crate::Runner`]; a connector never sees or rewrites UDS PDUs and never resends anything.
//!
//! # Contract for implementations
//! * `connect(generation)` returns a link whose `transport.generation() == generation`. The
//!   caller passes strictly increasing generations (reconnect = previous + 1) and rejects a link
//!   with any other value.
//! * A new generation is a claim of **response-epoch isolation**: no response to a request sent
//!   on an older generation can arrive on the new link. The runner relies on it to release its
//!   late-response quarantine U and to start the new link with nothing ambiguous outstanding.
//!   DoIP proves it by a new TCP connection plus routing activation (the old socket and its
//!   queued responses are gone). Dropping an old receiver or ISO-TP reassembly state is **not**
//!   such proof on CAN: the ECU may still send a delayed response to an earlier request on the
//!   same CAN IDs after the channel reopens. A future CAN-TP (or vendor) connector may return
//!   `Ok` with a new generation only under an adapter/vendor-specific, verified response-epoch
//!   fence or drain contract that covers every outstanding transaction. If it cannot establish
//!   that, it must fail the attempt with [`ConnectError`], so the target stays down and the
//!   ambiguity (`outcome_unknown`, quarantine) is retained. It must never return a fresh
//!   generation merely because the local receive path was rebuilt, and it must not issue UDS
//!   requests of its own (no automatic ECU reset or session change) to manufacture such proof.
//! * On `Err`, everything allocated by the attempt is already released (no socket, task or
//!   registration survives a failed attempt).
//! * Dropping [`Link::resources`] closes the link and stops its background tasks. The transport
//!   may outlive it briefly (it is swapped out of the runner afterwards); it must then fail with
//!   `Closed` rather than block.
//! * [`LinkEvents::next`] yields [`LinkSignal::Down`] when the link is lost. `None` (the signal
//!   source ended) is treated by the caller exactly like `Down`: the target goes link-down and the
//!   reconnect policy applies. A target is never left online without a live signal source.
//! * `Transport::confirm` resolves when the whole request has been handed over under the
//!   transport's own definition; P2 starts there (DoIP: the diagnostic ACK; ISO-TP: the last
//!   consecutive frame sent, not the first frame or the enqueue).
//! * Transports that segment responses (ISO-TP) emit [`crate::Incoming::RxStart`] at the first
//!   frame and [`crate::Incoming::Pdu`] with the reassembled PDU; the runner then applies its
//!   completion limit. Link framing (DoIP headers, ISO-TP PCI, vendor Ethernet framing) is the
//!   adapter's job and never reaches the runner.

use std::any::Any;
use std::fmt;

use thiserror::Error;
use tokio::sync::mpsc;

use crate::transport::{BoxFuture, Transport};

/// Something happened to the link (not data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkSignal {
    /// The link is lost (peer close, I/O or protocol error). The transport is dead.
    Down { reason: String },
    /// Received data was dropped because a bounded receive queue was full.
    RxOverflow { dropped_total: u64 },
}

/// Link signal source of one [`Link`].
pub trait LinkEvents: Send {
    /// Next signal; `None` when the source has ended (the caller treats this as link loss).
    fn next(&mut self) -> BoxFuture<'_, Option<LinkSignal>>;
}

/// A plain channel is a signal source (fakes, adapters that already produce [`LinkSignal`]s).
impl LinkEvents for mpsc::Receiver<LinkSignal> {
    fn next(&mut self) -> BoxFuture<'_, Option<LinkSignal>> {
        Box::pin(self.recv())
    }
}

/// One established link to one diagnostic target.
pub struct Link {
    pub transport: Box<dyn Transport>,
    pub events: Box<dyn LinkEvents>,
    /// Keeps the link's background resources alive; dropping it closes the link.
    pub resources: Box<dyn Any + Send>,
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link")
            .field("generation", &self.transport.generation())
            .finish_non_exhaustive()
    }
}

/// A connection attempt failed; nothing of it is left behind.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ConnectError(pub String);

/// Opens links to one diagnostic target (initial connect and every reconnect).
pub trait Connector: Send + Sync + fmt::Debug {
    /// Human-readable endpoint for `uds.state` (e.g. `DoIP 0x1001`).
    fn endpoint(&self) -> String;

    /// Establish a link with link generation `generation` (see the module contract).
    fn connect(&self, generation: u64) -> BoxFuture<'_, Result<Link, ConnectError>>;
}
