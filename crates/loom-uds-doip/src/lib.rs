//! DoIP (ISO 13400-2) for Loom UDS over the OS TCP/IP stack (master C4.1: PC NIC + media
//! converter).
//!
//! * [`frame`] — generic header + payload codec. Length is checked against [`MAX_DOIP_PAYLOAD`]
//!   **before** any payload buffer is allocated; central security activation `0xE0` is accepted.
//! * [`client`] — bounded tokio client: connect + routing activation, one reader task (alive
//!   checks answered immediately), ACK/NACK wait, responses routed per source LA.
//! * [`discovery`] — UDP vehicle identification request and announcement collection.
//! * [`entity`] — server side for tests and `connect --sim`, with fault injection.
//!
//! UDS bytes are opaque here; this crate does not depend on the UDS codec.

pub mod client;
pub mod discovery;
pub mod entity;
pub mod frame;
mod io;

pub use client::{
    AckWait, ClientConfig, ClientHealth, ClientStats, DoipClient, DoipError, DoipTarget, Incoming, LinkEvent,
    LinkState, RecvError,
};
pub use discovery::{Discovered, DiscoveryRequest, VehicleIdFilter, collect_announcements, discover};
pub use entity::{
    DEFAULT_T_TCP_GENERAL_INACTIVITY, DoipEntity, EntityConfig, EntityError, EntityFault, EntityHandler,
    EntityStats,
};
pub use frame::{
    ActivationCode, ActivationType, DOIP_PORT, HEADER_LEN, Header, HeaderError, MAX_DOIP_FRAME,
    MAX_DOIP_PAYLOAD, MAX_UDS_PDU, Message, ProtocolVersion, VehicleAnnouncement, decode_frame, parse_header,
};
pub use io::MAX_FRAME_WRITE_TIME;
