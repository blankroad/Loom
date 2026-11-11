//! Loom DBC: `can-dbc` parser → immutable compiled database (IR) → runtime codec.
//!
//! Plan D67: no code generation, because the DB is replaced while running (IG, CLI).
//! The gateway CAN module (decoder, SignalCache, IG preview, evidence signal pointers)
//! uses this crate.
//!
//! ```
//! use loom_can_dbc::{Database, EncodeBase, EncodeOptions, LoadOptions, SignalInput};
//! let dbc = b"VERSION \"\"\nBU_: ECU\nBO_ 256 Engine: 8 ECU\n SG_ Rpm : 0|16@1+ (0.25,0) [0|16000] \"rpm\" ECU\n";
//! let (db, _warnings) = Database::load(dbc, &LoadOptions::default()).unwrap();
//! let msg = db.message_by_name("Engine").unwrap();
//! let enc = msg
//!     .encode(&[("Rpm", SignalInput::Physical(3000.0))], EncodeBase::Initial, &EncodeOptions::default())
//!     .unwrap();
//! assert_eq!(enc.frame.data(), &[0xE0, 0x2E, 0, 0, 0, 0, 0, 0]);
//! let dec = db.decode(&enc.frame).unwrap();
//! assert_eq!(dec.signals.len(), 1);
//! ```
//!
//! Limits (C2) are checked while loading; every collection is fixed afterwards.
//! decode/encode allocate only per-signal vectors.

mod bits;
mod compile;
mod decode;
mod e2e;
mod encode;
mod error;
mod load;
mod model;
mod options;
mod text;

pub use bits::ByteOrder;
pub use decode::{DecodedMessage, DecodedSignal, LengthMismatch, SignalValue, Unavailable};
pub use e2e::{E2eReason, is_e2e_suspect};
pub use encode::{EncodeBase, EncodeOptions, Encoded, SignalInput, ValueSource};
pub use error::{DecodeError, EncodeError, LimitKind, LoadError, Warning, WarningKind};
pub use model::{Database, InitialSource, Message, MuxCondition, RawValue, Signal, SignalRef, ValueKind};
pub use options::{Limits, LoadOptions, TextEncoding};
