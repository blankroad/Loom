//! UDS (ISO 14229-1) codec for Loom.
//!
//! Pure crate: no I/O, no async, no clocks. Everything here is a function of its inputs.
//!
//! * [`sid`] — service table (name, sub-function, minimum request length).
//! * [`nrc`] — negative response code names (one source for UI, CLI and skills).
//! * [`request`] — request builders with length limits.
//! * [`response`] — [`classify_response`] (the runner's single entry point) and response parsers.
//! * [`did`] — DID definitions, value codecs and the built-in ISO F180–F19E table.
//! * [`db`] — diagnostic DB file schema, loader and identity hash.
//!
//! The DB never carries a risk grade: risk is decided from the bytes by the `loom-uds` domain
//! (plan: "DB 하향 무시").

pub mod db;
pub mod did;
mod error;
pub mod nrc;
pub mod request;
pub mod response;
pub mod sid;

pub use db::{DiagDb, DidLookup, Format, LoadedDb, StandardDids, fnv1a64, load_db};
pub use did::{DidCodec, DidDef, DidInput, DidValue, Endian, EnumEntry};
pub use error::CodecError;
pub use nrc::{NRC_RESPONSE_PENDING, nrc_name};
pub use response::{ResponseKind, classify_response};
pub use sid::{ServiceInfo, service};

/// Largest UDS request or response Loom builds or parses, in bytes.
///
/// ISO 15765-2 classic addressing tops out at 4095 bytes per PDU; DoIP could carry more but
/// Loom does not need it for the services it implements. Longer input is rejected with
/// [`CodecError::TooLong`] before any allocation.
pub const MAX_PDU_LEN: usize = 4095;

/// Positive response SID offset (`resp[0] == req[0] + 0x40`).
pub const POSITIVE_OFFSET: u8 = 0x40;

/// Negative response SID.
pub const NEGATIVE_RESPONSE_SID: u8 = 0x7F;

/// Suppress-positive-response bit in the sub-function byte.
pub const SPRMIB: u8 = 0x80;
