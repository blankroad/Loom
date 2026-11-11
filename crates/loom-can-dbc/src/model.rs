//! Immutable compiled database (IR).

use std::collections::HashMap;

use loom_can_frame::{CanFrame, CanId};

use crate::bits::{BitLayout, ByteOrder};
use crate::error::DecodeError;
use crate::options::TextEncoding;

/// How the raw bits of a signal are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Unsigned,
    /// Two's complement.
    Signed,
    /// IEEE 754 single (`SIG_VALTYPE_ … 1`), always 32 bits.
    Float32,
    /// IEEE 754 double (`SIG_VALTYPE_ … 2`), always 64 bits.
    Float64,
}

/// Where a signal's start value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitialSource {
    /// `BA_ "GenSigStartValue" SG_ …` for this signal.
    Signal,
    /// `BA_DEF_DEF_ "GenSigStartValue" …` (database default).
    Default,
}

/// Mux branch condition: active when the multiplexor is active and its raw value
/// is in one of the inclusive ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxCondition {
    /// Index of the multiplexor signal in the same message.
    pub multiplexor: u32,
    pub ranges: Vec<(u64, u64)>,
}

impl MuxCondition {
    pub fn contains(&self, value: u64) -> bool {
        self.ranges.iter().any(|&(lo, hi)| lo <= value && value <= hi)
    }

    /// The single value if the condition allows exactly one.
    pub fn single_value(&self) -> Option<u64> {
        match self.ranges.as_slice() {
            [(lo, hi)] if lo == hi => Some(*lo),
            _ => None,
        }
    }
}

/// One signal of a message.
#[derive(Debug, Clone)]
pub struct Signal {
    pub(crate) name: String,
    pub(crate) layout: BitLayout,
    pub(crate) kind: ValueKind,
    pub(crate) factor: f64,
    pub(crate) offset: f64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) unit: String,
    /// Sorted by raw value.
    pub(crate) value_table: Vec<(i64, String)>,
    pub(crate) initial_bits: Option<u64>,
    pub(crate) initial_source: Option<InitialSource>,
    pub(crate) is_multiplexor: bool,
    pub(crate) condition: Option<MuxCondition>,
    pub(crate) receivers: Vec<String>,
    pub(crate) comment: Option<String>,
}

impl Signal {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// DBC start bit (Intel: LSB position, Motorola: MSB position).
    pub fn start_bit(&self) -> u16 {
        self.layout.start()
    }

    /// Bit length, 1..=64.
    pub fn size(&self) -> u8 {
        self.layout.size()
    }

    pub fn byte_order(&self) -> ByteOrder {
        self.layout.order()
    }

    pub fn kind(&self) -> ValueKind {
        self.kind
    }

    pub fn factor(&self) -> f64 {
        self.factor
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// DBC `[min|max]` as written (physical).
    pub fn min_max(&self) -> (f64, f64) {
        (self.min, self.max)
    }

    /// Range used for checks. DBC `[0|0]` means "no range" and gives `None`.
    pub fn range(&self) -> Option<(f64, f64)> {
        if (self.min == 0.0 && self.max == 0.0) || self.min > self.max {
            None
        } else {
            Some((self.min, self.max))
        }
    }

    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// `VAL_` labels sorted by raw value.
    pub fn value_table(&self) -> &[(i64, String)] {
        &self.value_table
    }

    pub fn label_for(&self, raw: i64) -> Option<&str> {
        self.value_table
            .binary_search_by_key(&raw, |(v, _)| *v)
            .ok()
            .map(|i| self.value_table[i].1.as_str())
    }

    pub fn raw_for_label(&self, label: &str) -> Option<i64> {
        self.value_table.iter().find(|(_, l)| l == label).map(|(v, _)| *v)
    }

    /// Start value as raw bits (`GenSigStartValue` is a raw value).
    pub fn initial_raw_bits(&self) -> Option<u64> {
        self.initial_bits
    }

    pub fn initial_source(&self) -> Option<InitialSource> {
        self.initial_source
    }

    /// Start value as a physical value.
    pub fn initial_physical(&self) -> Option<f64> {
        self.initial_bits.map(|b| self.physical(b))
    }

    pub fn is_multiplexor(&self) -> bool {
        self.is_multiplexor
    }

    /// Branch condition for multiplexed signals.
    pub fn mux_condition(&self) -> Option<&MuxCondition> {
        self.condition.as_ref()
    }

    pub fn receivers(&self) -> &[String] {
        &self.receivers
    }

    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    pub(crate) fn layout(&self) -> &BitLayout {
        &self.layout
    }

    /// Raw bits → typed raw value.
    pub fn raw_value(&self, bits: u64) -> RawValue {
        match self.kind {
            ValueKind::Unsigned => RawValue::Unsigned(bits),
            ValueKind::Signed => {
                let size = self.layout.size() as u32;
                let v = if size < 64 && bits & (1u64 << (size - 1)) != 0 {
                    (bits | !self.layout.mask()) as i64
                } else {
                    bits as i64
                };
                RawValue::Signed(v)
            }
            ValueKind::Float32 => RawValue::Float32(f32::from_bits(bits as u32)),
            ValueKind::Float64 => RawValue::Float64(f64::from_bits(bits)),
        }
    }

    /// Raw bits → physical value (`raw * factor + offset`).
    pub fn physical(&self, bits: u64) -> f64 {
        self.raw_value(bits).as_f64() * self.factor + self.offset
    }
}

/// A signal's raw value. Integers are exact; floats are the IEEE value before scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawValue {
    Unsigned(u64),
    Signed(i64),
    Float32(f32),
    Float64(f64),
}

impl RawValue {
    pub fn as_f64(self) -> f64 {
        match self {
            RawValue::Unsigned(v) => v as f64,
            RawValue::Signed(v) => v as f64,
            RawValue::Float32(v) => v as f64,
            RawValue::Float64(v) => v,
        }
    }

    /// Integer value for value-table lookup.
    pub fn as_i64(self) -> Option<i64> {
        match self {
            RawValue::Unsigned(v) => i64::try_from(v).ok(),
            RawValue::Signed(v) => Some(v),
            _ => None,
        }
    }
}

/// One DBC message (frame definition).
#[derive(Debug, Clone)]
pub struct Message {
    pub(crate) id: CanId,
    pub(crate) name: String,
    pub(crate) size: u8,
    pub(crate) fd: bool,
    pub(crate) transmitter: Option<String>,
    pub(crate) cycle_time_ms: Option<u32>,
    pub(crate) comment: Option<String>,
    pub(crate) signals: Vec<Signal>,
    pub(crate) signal_index: HashMap<Box<str>, u32>,
    /// Signal indices with every multiplexor before the signals it controls.
    pub(crate) order: Vec<u32>,
    pub(crate) encode_blocked: Option<String>,
    pub(crate) decode_blocked: Option<String>,
    pub(crate) e2e_attributes: Vec<String>,
    pub(crate) line: Option<u32>,
}

impl Message {
    pub fn id(&self) -> CanId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// DBC length in bytes (0..=64).
    pub fn size(&self) -> usize {
        self.size as usize
    }

    /// CAN FD: `VFrameFormat` says FD, or the length is above 8.
    pub fn is_fd(&self) -> bool {
        self.fd
    }

    pub fn transmitter(&self) -> Option<&str> {
        self.transmitter.as_deref()
    }

    /// `GenMsgCycleTime` in ms; `None` when absent or 0.
    pub fn cycle_time_ms(&self) -> Option<u32> {
        self.cycle_time_ms
    }

    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    pub fn signals(&self) -> &[Signal] {
        &self.signals
    }

    pub fn signal(&self, index: u32) -> Option<&Signal> {
        self.signals.get(index as usize)
    }

    pub fn signal_by_name(&self, name: &str) -> Option<(u32, &Signal)> {
        let i = *self.signal_index.get(name)?;
        Some((i, &self.signals[i as usize]))
    }

    pub fn is_multiplexed(&self) -> bool {
        self.signals.iter().any(|s| s.is_multiplexor)
    }

    /// Reason encode is refused (fail closed), if any.
    pub fn encode_blocked(&self) -> Option<&str> {
        self.encode_blocked.as_deref()
    }

    /// Reason decode is refused, if any.
    pub fn decode_blocked(&self) -> Option<&str> {
        self.decode_blocked.as_deref()
    }

    /// Message/signal attribute names that hint at E2E protection.
    pub fn e2e_attributes(&self) -> &[String] {
        &self.e2e_attributes
    }

    /// Line of the `BO_` statement in the source text, when known.
    pub fn line(&self) -> Option<u32> {
        self.line
    }
}

/// Reference to a signal in a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalRef {
    pub message: u32,
    pub signal: u32,
}

/// Compiled, immutable DBC database. Replace it as a whole to change the DB.
#[derive(Debug, Clone)]
pub struct Database {
    pub(crate) messages: Vec<Message>,
    pub(crate) by_id: HashMap<CanId, u32>,
    pub(crate) ambiguous_ids: Vec<CanId>,
    pub(crate) by_name: HashMap<Box<str>, u32>,
    pub(crate) signals_by_name: HashMap<Box<str>, Vec<SignalRef>>,
    pub(crate) nodes: Vec<String>,
    pub(crate) sha256: [u8; 32],
    pub(crate) encoding: TextEncoding,
    pub(crate) encoding_guessed: bool,
    pub(crate) approx_bytes: usize,
}

impl Database {
    /// SHA-256 of the file bytes as loaded (`db_sha256` for session pinning).
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }

    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Encoding used to read the text.
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// True when `TextEncoding::Auto` had to choose.
    pub fn encoding_guessed(&self) -> bool {
        self.encoding_guessed
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn message(&self, index: u32) -> Option<&Message> {
        self.messages.get(index as usize)
    }

    /// `None` if unknown or ambiguous (see [`Database::is_ambiguous_id`]).
    pub fn message_by_id(&self, id: CanId) -> Option<&Message> {
        self.by_id.get(&id).map(|&i| &self.messages[i as usize])
    }

    pub fn is_ambiguous_id(&self, id: CanId) -> bool {
        self.ambiguous_ids.contains(&id)
    }

    /// `None` if unknown or if several messages share the name.
    pub fn message_by_name(&self, name: &str) -> Option<&Message> {
        self.by_name.get(name).map(|&i| &self.messages[i as usize])
    }

    /// All signals with this name, in any message.
    pub fn signals_named(&self, name: &str) -> &[SignalRef] {
        self.signals_by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn signal(&self, r: SignalRef) -> Option<(&Message, &Signal)> {
        let m = self.messages.get(r.message as usize)?;
        Some((m, m.signals.get(r.signal as usize)?))
    }

    /// `"Message.Signal"`, or a bare signal name when it is unique in the database.
    pub fn signal_by_path(&self, path: &str) -> Option<(&Message, &Signal)> {
        if let Some((m, s)) = path.split_once('.')
            && let Some(msg) = self.message_by_name(m)
        {
            return msg.signal_by_name(s).map(|(_, sig)| (msg, sig));
        }
        match self.signals_named(path) {
            [one] => self.signal(*one),
            _ => None,
        }
    }

    /// Node names from `BU_`.
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// Estimated heap size of the compiled database in bytes.
    pub fn approx_heap_bytes(&self) -> usize {
        self.approx_bytes
    }

    /// Looks up the frame id and decodes. See [`Message::decode`].
    pub fn decode(&self, frame: &CanFrame) -> Result<crate::DecodedMessage<'_>, DecodeError> {
        if self.is_ambiguous_id(frame.id()) {
            return Err(DecodeError::AmbiguousId);
        }
        self.message_by_id(frame.id())
            .ok_or(DecodeError::UnknownId)?
            .decode(frame)
    }
}
