use thiserror::Error;

/// Codec errors. `code()` maps to the CLI error code family (`E_USAGE` for bad arguments,
/// `E_DB` for diagnostic DB problems, `E_DECODE` for bytes that do not match the expected shape).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CodecError {
    #[error("{what} is {len} bytes, limit {max}")]
    TooLong {
        what: &'static str,
        len: usize,
        max: usize,
    },
    #[error("too many {what}: {count}, limit {max}")]
    TooMany {
        what: &'static str,
        count: usize,
        max: usize,
    },
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("{what}: expected {expected} bytes, got {got}")]
    LengthMismatch {
        what: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("truncated {what}: need at least {need} bytes, got {got}")]
    Truncated {
        what: &'static str,
        need: usize,
        got: usize,
    },
    #[error("unexpected response SID 0x{got:02X} (expected 0x{expected:02X})")]
    UnexpectedSid { expected: u8, got: u8 },
    #[error("DID 0x{did:04X}: length unknown, cannot split a multi-DID response")]
    UnknownDidLength { did: u16 },
    #[error("DID 0x{expected:04X} expected in response, found 0x{got:04X}")]
    DidEcho { expected: u16, got: u16 },
    #[error("value cannot be decoded as {codec}: {detail}")]
    Decode { codec: &'static str, detail: String },
    #[error("value cannot be encoded as {codec}: {detail}")]
    Encode { codec: &'static str, detail: String },
    #[error("diagnostic DB: {0}")]
    Db(String),
}

impl CodecError {
    /// Stable error family for CLI envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            CodecError::Db(_) => "E_DB",
            CodecError::InvalidArgument(_)
            | CodecError::TooLong { .. }
            | CodecError::TooMany { .. }
            | CodecError::Encode { .. } => "E_USAGE",
            CodecError::LengthMismatch { .. }
            | CodecError::Truncated { .. }
            | CodecError::UnexpectedSid { .. }
            | CodecError::UnknownDidLength { .. }
            | CodecError::DidEcho { .. }
            | CodecError::Decode { .. } => "E_DECODE",
        }
    }

    pub(crate) fn arg(msg: impl Into<String>) -> Self {
        CodecError::InvalidArgument(msg.into())
    }
}
