//! Frame → signal values.

use loom_can_frame::{CanFrame, fd_padded_len};

use crate::error::DecodeError;
use crate::model::{Message, RawValue, Signal};

/// Result of decoding one frame. Only signals of active mux branches are listed.
#[derive(Debug, Clone)]
pub struct DecodedMessage<'a> {
    pub message: &'a Message,
    /// In DBC order.
    pub signals: Vec<DecodedSignal<'a>>,
    /// Frame length differs from the DBC length (FD padding to the next valid length is not a mismatch).
    pub length_mismatch: Option<LengthMismatch>,
    /// Frame is Classic but the DBC says CAN FD, or the other way round.
    pub fd_mismatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthMismatch {
    pub expected: usize,
    pub actual: usize,
}

#[derive(Debug, Clone)]
pub struct DecodedSignal<'a> {
    /// Index in `Message::signals()`.
    pub index: u32,
    pub signal: &'a Signal,
    pub value: SignalValue<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignalValue<'a> {
    Value {
        raw: RawValue,
        physical: f64,
        /// `VAL_` label for the raw value, if any.
        label: Option<&'a str>,
    },
    /// The signal is active but its bits are not in the frame.
    Unavailable(Unavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    ShortFrame { needed: usize, actual: usize },
}

#[derive(Clone, Copy)]
enum State {
    Inactive,
    /// Branch unknown because the multiplexor could not be read.
    Unknown,
    Short,
    Value(u64),
}

impl Message {
    /// Decodes a frame with this message's layout (the frame id is not checked;
    /// use [`crate::Database::decode`] to look up by id). Never panics on short frames.
    pub fn decode<'a>(&'a self, frame: &CanFrame) -> Result<DecodedMessage<'a>, DecodeError> {
        if frame.is_remote() {
            return Err(DecodeError::RemoteFrame);
        }
        let mut d = self.decode_payload(frame.data())?;
        d.fd_mismatch = frame.is_fd() != self.fd;
        Ok(d)
    }

    /// Decodes raw payload bytes with this message's layout.
    pub fn decode_payload<'a>(&'a self, data: &[u8]) -> Result<DecodedMessage<'a>, DecodeError> {
        if let Some(reason) = &self.decode_blocked {
            return Err(DecodeError::Blocked {
                reason: reason.clone(),
            });
        }
        let n = self.signals.len();
        let mut state = vec![State::Inactive; n];
        for &i in &self.order {
            let sig = &self.signals[i as usize];
            let active = match &sig.condition {
                None => Some(true),
                Some(c) => match state[c.multiplexor as usize] {
                    State::Value(v) => Some(c.contains(v)),
                    State::Inactive => Some(false),
                    State::Unknown | State::Short => None,
                },
            };
            state[i as usize] = match active {
                Some(true) => sig.layout().extract(data).map_or(State::Short, State::Value),
                Some(false) => State::Inactive,
                None => State::Unknown,
            };
        }

        let mut signals = Vec::with_capacity(n);
        for (i, (sig, st)) in self.signals.iter().zip(&state).enumerate() {
            let value = match *st {
                State::Value(bits) => {
                    let raw = sig.raw_value(bits);
                    SignalValue::Value {
                        raw,
                        physical: raw.as_f64() * sig.factor + sig.offset,
                        label: raw.as_i64().and_then(|v| sig.label_for(v)),
                    }
                }
                State::Short => SignalValue::Unavailable(Unavailable::ShortFrame {
                    needed: sig.layout().required_len(),
                    actual: data.len(),
                }),
                State::Inactive | State::Unknown => continue,
            };
            signals.push(DecodedSignal {
                index: i as u32,
                signal: sig,
                value,
            });
        }

        let size = self.size();
        let padded = if self.fd { fd_padded_len(size) } else { None };
        let length_mismatch = (data.len() != size && Some(data.len()) != padded).then_some(LengthMismatch {
            expected: size,
            actual: data.len(),
        });
        Ok(DecodedMessage {
            message: self,
            signals,
            length_mismatch,
            fd_mismatch: false,
        })
    }
}
