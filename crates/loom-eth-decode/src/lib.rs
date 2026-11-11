//! `loom-eth-decode` — no-IO packet decoder for the Loom `eth` domain.
//!
//! One packet in → tshark-style [`Summary`], a layer tree ([`Layer`]), and filterable field values
//! ([`Decoded::field_values`]). Field names come from the registry [`FIELDS`] (Wireshark names + aliases).
//!
//! ```
//! use loom_eth_decode::{decode, linktype, FrameMeta, StreamTable};
//! let mut streams = StreamTable::new();
//! let d = decode(linktype::ETHERNET, &[0u8; 10], &FrameMeta::default(), &mut streams);
//! assert!(!d.warnings.is_empty()); // too short for Ethernet: warning, no panic
//! ```
//!
//! Every buffer, map and list is bounded (`MAX_*` constants in each module). No input panics.

mod doip;
pub mod fields;
mod json;
mod model;
mod net;
mod rd;
mod someip;
mod stream;
mod uds;
mod value;

pub use fields::{FIELDS, FieldDef, field_def, protocols};
pub use json::{CompactLayers, ValueJson};
pub use model::{
    DecodeWarning, Decoded, Dir, Field, FieldValues, FrameMeta, Layer, Loc, MAX_BUFFERS, MAX_FIELD_DEPTH,
    MAX_FIELDS, MAX_FRAME_BYTES, MAX_INFO_LEN, MAX_LAYERS, MAX_WARNINGS, Origin, Pos, Summary,
};
pub use stream::{
    MAX_OOO_DISTANCE, MAX_PDU_BYTES, MAX_PDUS_PER_PACKET, MAX_PENDING_SEGMENTS, MAX_STREAM_BUFFERED_BYTES,
    MAX_STREAMS, MAX_TOTAL_BUFFERED_BYTES, STREAM_IDLE_NS, SWEEP_INTERVAL_NS, StreamTable,
};
pub use value::{Base, FieldType, FieldValue, fmt_hex, fmt_mac};

/// Per-protocol caps, re-exported for documentation and tests.
pub mod limits {
    pub use crate::doip::MAX_MSGS_PER_DATAGRAM as DOIP_MAX_MSGS_PER_DATAGRAM;
    pub use crate::net::MAX_VLAN_TAGS;
    pub use crate::someip::{
        MAX_MSGS_PER_DATAGRAM as SOMEIP_MAX_MSGS_PER_DATAGRAM, MAX_SD_ENTRIES, MAX_SD_OPTIONS,
    };
    pub use crate::uds::MAX_DIDS_PER_REQUEST;
}

/// Well-known ports.
pub mod ports {
    pub use crate::doip::DOIP_PORT;
    pub use crate::someip::SOMEIP_SD_PORT;
}

/// pcap LINKTYPE values handled by [`decode`].
pub mod linktype {
    /// BSD loopback, host byte order family (macOS `lo0`, Npcap loopback).
    pub const NULL: u16 = 0;
    pub const ETHERNET: u16 = 1;
    /// Raw IP (version from the first nibble).
    pub const RAW: u16 = 101;
    /// OpenBSD loopback, network byte order family.
    pub const LOOP: u16 = 108;
    pub const LINUX_SLL: u16 = 113;
    pub const IPV4: u16 = 228;
    pub const IPV6: u16 = 229;
}

/// Decodes one packet. `streams` holds TCP state for one capture interface; pass the same table
/// for every packet of that interface, in capture order.
pub fn decode(linktype: u16, frame: &[u8], meta: &FrameMeta, streams: &mut StreamTable) -> Decoded {
    let mut d = Decoded::new(meta, frame);
    let kept = frame.get(..MAX_FRAME_BYTES).unwrap_or(frame);
    net::run(&mut d, linktype, kept, streams);
    d
}
