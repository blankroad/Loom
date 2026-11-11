//! Errors and load warnings.

use std::fmt;

use loom_can_frame::FrameError;

/// Which load limit was hit (plan §7 / §9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    FileBytes,
    Statements,
    Messages,
    Signals,
    StringBytes,
    MuxDepth,
    IrBytes,
    DroppedStatements,
}

impl fmt::Display for LimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LimitKind::FileBytes => "file size (bytes)",
            LimitKind::Statements => "statements",
            LimitKind::Messages => "messages",
            LimitKind::Signals => "signals",
            LimitKind::StringBytes => "string length (bytes)",
            LimitKind::MuxDepth => "multiplexer depth",
            LimitKind::IrBytes => "compiled database size (bytes)",
            LimitKind::DroppedStatements => "unparsable statements",
        };
        f.write_str(s)
    }
}

/// The database could not be loaded. Nothing is partially loaded.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadError {
    /// A size limit was exceeded.
    LimitExceeded {
        what: LimitKind,
        limit: u64,
        actual: u64,
    },
    /// The bytes are not valid in the requested encoding.
    Encoding { offset: usize, detail: String },
    /// The file is not a usable DBC (nothing parsed, or the cleaned text still failed).
    InvalidDbc { line: Option<u32>, reason: String },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::LimitExceeded { what, limit, actual } => {
                write!(f, "DBC limit exceeded: {what} {actual} > {limit}")
            }
            LoadError::Encoding { offset, detail } => {
                write!(f, "DBC text encoding error at byte {offset}: {detail}")
            }
            LoadError::InvalidDbc { line, reason } => match line {
                Some(l) => write!(f, "invalid DBC (line {l}): {reason}"),
                None => write!(f, "invalid DBC: {reason}"),
            },
        }
    }
}

impl std::error::Error for LoadError {}

/// Kind of a non-fatal load finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WarningKind {
    /// Encoding was not given; the loader picked one.
    EncodingGuessed,
    /// can-dbc could not parse this statement; it was removed before compiling.
    StatementDropped,
    /// Parsed (or known) section that Loom does not use.
    SectionIgnored,
    /// A message was left out (pseudo message, invalid id).
    MessageSkipped,
    /// A message is kept but encode (and maybe decode) is refused. See `Message::encode_blocked`.
    MessageBlocked,
    DuplicateMessageId,
    DuplicateMessageName,
    /// An attribute value was not usable (wrong type, does not fit).
    AttributeIgnored,
    /// More warnings than `Limits::max_warnings`; the rest were counted only.
    WarningsTruncated,
}

/// Non-fatal finding with the source line when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub line: Option<u32>,
    pub kind: WarningKind,
    pub detail: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "line {l}: {:?}: {}", self.kind, self.detail),
            None => write!(f, "{:?}: {}", self.kind, self.detail),
        }
    }
}

/// Bounded warning list (C2): at most `cap` entries, the rest are counted.
pub(crate) struct Warnings {
    list: Vec<Warning>,
    cap: usize,
    omitted: u64,
}

impl Warnings {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            list: Vec::new(),
            cap,
            omitted: 0,
        }
    }

    pub(crate) fn push(&mut self, line: Option<u32>, kind: WarningKind, detail: impl Into<String>) {
        if self.list.len() < self.cap {
            self.list.push(Warning {
                line,
                kind,
                detail: detail.into(),
            });
        } else {
            self.omitted += 1;
        }
    }

    pub(crate) fn finish(mut self) -> Vec<Warning> {
        if self.omitted > 0 {
            self.list.push(Warning {
                line: None,
                kind: WarningKind::WarningsTruncated,
                detail: format!("{} more warnings not listed", self.omitted),
            });
        }
        self.list
    }
}

/// Decoding refused for the whole frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// No message with this id in the database.
    UnknownId,
    /// Several messages share this id; the database cannot pick one.
    AmbiguousId,
    /// The message layout is not trustworthy (see reason).
    Blocked { reason: String },
    /// Remote frames carry no data.
    RemoteFrame,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnknownId => write!(f, "no message with this id in the DBC"),
            DecodeError::AmbiguousId => write!(f, "several DBC messages share this id"),
            DecodeError::Blocked { reason } => write!(f, "decode blocked: {reason}"),
            DecodeError::RemoteFrame => write!(f, "remote frames carry no signal data"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Encoding refused. Nothing is sent on any error (fail closed).
#[derive(Debug, Clone, PartialEq)]
pub enum EncodeError {
    /// Layout of this message is not trustworthy (see reason).
    Blocked {
        reason: String,
    },
    UnknownSignal {
        name: String,
    },
    DuplicateSignal {
        name: String,
    },
    /// Signal is active, has no DBC start value and no value was given.
    MissingSignal {
        name: String,
    },
    /// Physical value outside DBC [min, max]. `force_range` skips this check.
    OutOfRange {
        signal: String,
        value: f64,
        min: f64,
        max: f64,
    },
    /// Raw value does not fit the signal bit width (never skippable).
    RawOverflow {
        signal: String,
        raw: i128,
        bits: u8,
        signed: bool,
    },
    /// Value is NaN/infinite or cannot be represented (e.g. f32 overflow).
    NotRepresentable {
        signal: String,
        detail: String,
    },
    UnknownLabel {
        signal: String,
        label: String,
    },
    /// A value was given for a signal whose mux branch is not active.
    InactiveMuxSignal {
        signal: String,
        multiplexor: String,
        /// `None` when the multiplexor itself is in an inactive branch.
        multiplexor_value: Option<u64>,
    },
    /// The branch needs a multiplexor value range; give the multiplexor explicitly.
    AmbiguousMultiplexor {
        signal: String,
        multiplexor: String,
    },
    /// Two given signals need different values of the same multiplexor.
    ConflictingMultiplexor {
        multiplexor: String,
        first: u64,
        second: u64,
    },
    /// `EncodeBase::Payload` length differs from the DBC message length.
    BaseLengthMismatch {
        expected: usize,
        actual: usize,
    },
    Frame(FrameError),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::Blocked { reason } => write!(f, "encode blocked: {reason}"),
            EncodeError::UnknownSignal { name } => write!(f, "unknown signal {name}"),
            EncodeError::DuplicateSignal { name } => write!(f, "signal {name} given twice"),
            EncodeError::MissingSignal { name } => {
                write!(f, "signal {name} has no DBC start value; give a value")
            }
            EncodeError::OutOfRange {
                signal,
                value,
                min,
                max,
            } => write!(f, "{signal} = {value} outside DBC range [{min}, {max}]"),
            EncodeError::RawOverflow {
                signal,
                raw,
                bits,
                signed,
            } => write!(
                f,
                "{signal} raw {raw} does not fit {bits}-bit {}",
                if *signed { "signed" } else { "unsigned" }
            ),
            EncodeError::NotRepresentable { signal, detail } => {
                write!(f, "{signal}: value not representable ({detail})")
            }
            EncodeError::UnknownLabel { signal, label } => {
                write!(f, "{signal} has no value label {label:?}")
            }
            EncodeError::InactiveMuxSignal {
                signal,
                multiplexor,
                multiplexor_value,
            } => match multiplexor_value {
                Some(v) => write!(f, "{signal} is not in the active branch ({multiplexor} = {v})"),
                None => write!(f, "{signal} is in a branch of inactive multiplexor {multiplexor}"),
            },
            EncodeError::AmbiguousMultiplexor { signal, multiplexor } => write!(
                f,
                "{signal} is valid for several values of {multiplexor}; set {multiplexor} explicitly"
            ),
            EncodeError::ConflictingMultiplexor {
                multiplexor,
                first,
                second,
            } => write!(
                f,
                "given signals need {multiplexor} = {first} and {multiplexor} = {second}"
            ),
            EncodeError::BaseLengthMismatch { expected, actual } => {
                write!(f, "base payload has {actual} bytes, DBC message has {expected}")
            }
            EncodeError::Frame(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EncodeError {}
