//! Error codes and exit codes (contract v1 §4.4, master D5·A6·E2·E3) plus the crate's own
//! decoding error type.
//!
//! The single table in the private `define_error_codes!` macro is the source of truth for the wire name
//! (`"E_TIMEOUT"`), the Rust variant and the CLI exit code. New codes are added only here
//! (append-only; never renumber an exit group).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Decoding / validation error for contract types. Every fallible parser in this crate
/// returns this instead of panicking (plan T15).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtoError {
    /// A `U64` string was empty, non-decimal, signed, had a leading zero or overflowed.
    #[error("invalid u64 string: {0}")]
    InvalidU64(String),
    /// Hex input had odd length or a non-hex character.
    #[error("invalid hex: {0}")]
    InvalidHex(String),
    /// Base64 input was not standard (RFC 4648 §4, padded) base64.
    #[error("invalid base64: {0}")]
    InvalidBase64(String),
    /// Decoded byte length exceeds the inline limit.
    #[error("byte payload too large: {len} > {max}")]
    TooLarge {
        /// Length that was found (or would be produced).
        len: usize,
        /// The applicable maximum.
        max: usize,
    },
    /// A cursor string did not match the grammar (§4.5).
    #[error("invalid cursor: {0}")]
    InvalidCursor(String),
    /// An evidence REF string did not match the grammar.
    #[error("invalid evidence ref: {0}")]
    InvalidRef(String),
    /// A JSON Pointer (RFC 6901) was malformed.
    #[error("invalid json pointer: {0}")]
    InvalidPointer(String),
    /// An RPC method or event kind name violated the naming rule (§4.7).
    #[error("invalid name: {0}")]
    InvalidName(String),
    /// A payload field had the wrong shape.
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    /// A byte payload is a `raw_ref` and cannot be returned inline.
    #[error("payload bytes are not inline (raw_ref)")]
    NotInline,
    /// Worker frame header problems (§3.3).
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    /// Frame input ended before a full header / payload was available.
    #[error("incomplete frame: need {need} more bytes")]
    Incomplete {
        /// Bytes still missing.
        need: usize,
    },
}

macro_rules! define_error_codes {
    ($( $(#[$doc:meta])* $variant:ident = $wire:literal => $exit:literal ),+ $(,)?) => {
        /// Loom error code (`error.code` in the CLI envelope). Wire form is `"E_…"`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
        pub enum ErrorCode {
            $( $(#[$doc])* #[serde(rename = $wire)] $variant, )+
        }

        impl ErrorCode {
            /// Every code, in table order.
            pub const ALL: &'static [ErrorCode] = &[ $( ErrorCode::$variant, )+ ];

            /// CLI exit code group for this error (D5 table + E2/E3).
            pub const fn exit_code(self) -> u8 {
                match self { $( ErrorCode::$variant => $exit, )+ }
            }

            /// Wire name, e.g. `"E_TIMEOUT"`.
            pub const fn as_str(self) -> &'static str {
                match self { $( ErrorCode::$variant => $wire, )+ }
            }

            /// Parse a wire name; `None` for unknown codes.
            pub fn from_wire(s: &str) -> Option<ErrorCode> {
                match s { $( $wire => Some(ErrorCode::$variant), )+ _ => None }
            }
        }
    };
}

define_error_codes! {
    /// Internal bug or unexpected state.
    Internal = "E_INTERNAL" => 1,
    /// Stub / not yet implemented path (P0 stubs never fake device success).
    NotImplemented = "E_NOT_IMPLEMENTED" => 1,
    /// Bad command-line usage or arguments.
    Usage = "E_USAGE" => 2,
    /// Channel name matches more than one candidate.
    AmbiguousChannel = "E_AMBIGUOUS_CHANNEL" => 2,
    /// Configuration (loom.toml / policy) error.
    Config = "E_CONFIG" => 2,
    /// Database / diagnostic DB error.
    Db = "E_DB" => 2,
    /// RPC method not registered.
    UnknownMethod = "E_UNKNOWN_METHOD" => 2,
    /// Parameters failed schema validation.
    Schema = "E_SCHEMA" => 2,
    /// Condition wait timed out (expect, wait-for, approval wait).
    Timeout = "E_TIMEOUT" => 3,
    /// Immediate check: condition not met (E2).
    NoMatch = "E_NO_MATCH" => 3,
    /// Immediate check: forbidden match was found (E2).
    UnexpectedMatch = "E_UNEXPECTED_MATCH" => 3,
    /// Immediate check: count outside the expected range (E2).
    CountOutOfRange = "E_COUNT_OUT_OF_RANGE" => 3,
    /// Policy denial (hard deny list, configured deny).
    Denied = "E_DENIED" => 4,
    /// Service Loom does not implement (`immutable_deny`, F1).
    Unsupported = "E_UNSUPPORTED" => 4,
    /// Rate limit exceeded.
    RateLimit = "E_RATE_LIMIT" => 4,
    /// Gateway admission full (RPC slots); retry later.
    Busy = "E_BUSY" => 4,
    /// Recording / audit unavailable: agent·script R1+ writes blocked (A12).
    RecordingFailed = "E_RECORDING_FAILED" => 4,
    /// Test verdict PASS without evidence.
    EvidenceMissing = "E_EVIDENCE_MISSING" => 4,
    /// Cited evidence does not match the claim.
    EvidenceMismatch = "E_EVIDENCE_MISMATCH" => 4,
    /// Cited evidence lies outside the step window.
    OutOfWindow = "E_OUT_OF_WINDOW" => 4,
    /// Cited predicate is not the registered plan expectation.
    PlanMismatch = "E_PLAN_MISMATCH" => 4,
    /// R2 confirmation token required (only when enabled by policy, F1).
    ConfirmRequired = "E_CONFIRM_REQUIRED" => 5,
    /// Viewer approval required (only when enabled by policy, F1).
    ApprovalRequired = "E_APPROVAL_REQUIRED" => 5,
    /// Approval required but no approving viewer is connected.
    NoApprover = "E_NO_APPROVER" => 5,
    /// The user denied the approval.
    ApprovalDenied = "E_APPROVAL_DENIED" => 5,
    /// Approval expired or already used.
    ApprovalExpired = "E_APPROVAL_EXPIRED" => 5,
    /// The user is typing / holds input.
    UserActive = "E_USER_ACTIVE" => 6,
    /// Resource locked by another client.
    Locked = "E_LOCKED" => 6,
    /// Agent lock was broken by the user.
    LockBroken = "E_LOCK_BROKEN" => 6,
    /// User hold in effect.
    HeldByUser = "E_HELD_BY_USER" => 6,
    /// User interrupted the operation.
    InterruptedByUser = "E_INTERRUPTED_BY_USER" => 6,
    /// Late-response quarantine (UDS).
    Quarantine = "E_QUARANTINE" => 6,
    /// STOP latch is on.
    Stopped = "E_STOPPED" => 6,
    /// Lease not held.
    Lease = "E_LEASE" => 6,
    /// Another test session is active.
    SessionActive = "E_SESSION_ACTIVE" => 6,
    /// Test session is paused (E3).
    SessionPaused = "E_SESSION_PAUSED" => 6,
    /// Channel / target not open.
    NotOpen = "E_NOT_OPEN" => 7,
    /// Device disconnected.
    Disconnected = "E_DISCONNECTED" => 7,
    /// Device identity / generation changed.
    ConnectionChanged = "E_CONNECTION_CHANGED" => 7,
    /// CAN bus-off.
    BusOff = "E_BUS_OFF" => 7,
    /// Device did not respond (UDS P2/P2*); writes are `outcome_unknown`.
    NoResponse = "E_NO_RESPONSE" => 7,
    /// Port in use by another program.
    PortBusy = "E_PORT_BUSY" => 7,
    /// OS permission denied (e.g. dialout).
    Permission = "E_PERMISSION" => 7,
    /// Device / target not found.
    NotFound = "E_NOT_FOUND" => 7,
    /// Owning worker process is down / quarantined.
    WorkerDown = "E_WORKER_DOWN" => 7,
    /// Worker control lane full; the write was not sent.
    WorkerBusy = "E_WORKER_BUSY" => 7,
    /// Cursor refers to data outside retention / another epoch.
    CursorGap = "E_CURSOR_GAP" => 8,
    /// Bounded scan limit reached; resume with `resume_from`.
    ScanLimit = "E_SCAN_LIMIT" => 8,
    /// Partial result (e.g. write sent but not yet published, E7).
    Partial = "E_PARTIAL" => 8,
    /// Daemon unreachable.
    Daemon = "E_DAEMON" => 9,
    /// Protocol major version mismatch.
    ProtoMismatch = "E_PROTO_MISMATCH" => 9,
    /// Device negative response (UDS NRC).
    Nrc = "E_NRC" => 10,
    /// Test session / `run --test` verdict failed (E3).
    TestFailed = "E_TEST_FAILED" => 11,
    /// 도메인이 정한 자원·운영 상한을 넘었다(기존 variant 순서는 유지).
    Limit = "E_LIMIT" => 4,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Exit code for success (no error).
pub const EXIT_OK: u8 = 0;
