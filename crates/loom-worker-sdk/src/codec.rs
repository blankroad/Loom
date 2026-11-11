//! `loom-worker/1` frame codec: 8-byte header (`loom_proto::worker::FrameHeader`) + MessagePack
//! map payload (`rmp_serde::to_vec_named`). Pure functions; whole frame ≤ 64 KiB.

use loom_proto::error::ProtoError;
use loom_proto::worker::{FrameHeader, HEADER_LEN, MAX_PAYLOAD, WorkerMsg, split_frame};

/// Codec failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// Header / length problem, or more bytes needed ([`ProtoError::Incomplete`]).
    #[error(transparent)]
    Frame(#[from] ProtoError),
    /// Encoded message exceeds the frame limit — split it (chunks or `raw_ref`).
    #[error("message too large: {len} > {max} bytes")]
    TooLarge {
        /// Encoded payload length.
        len: usize,
        /// Maximum payload.
        max: usize,
    },
    /// Serialization failed.
    #[error("encode: {0}")]
    Encode(String),
    /// Payload is not a valid message.
    #[error("decode: {0}")]
    Decode(String),
    /// Header type and decoded message disagree.
    #[error("msg_type {header} in header but payload is type {body}")]
    TypeMismatch {
        /// From the header.
        header: u8,
        /// From the payload.
        body: u8,
    },
}

impl CodecError {
    /// True when the caller should read more bytes and retry.
    pub fn is_incomplete(&self) -> bool {
        matches!(self, CodecError::Frame(ProtoError::Incomplete { .. }))
    }
}

/// Encode one message into a complete frame.
pub fn encode_frame(msg: &WorkerMsg) -> Result<Vec<u8>, CodecError> {
    let payload = rmp_serde::to_vec_named(msg).map_err(|e| CodecError::Encode(e.to_string()))?;
    if payload.len() > MAX_PAYLOAD {
        return Err(CodecError::TooLarge {
            len: payload.len(),
            max: MAX_PAYLOAD,
        });
    }
    let header = FrameHeader::new(payload.len(), msg.msg_type())?;
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode one frame from the front of `buf`; returns the message and the bytes consumed.
/// [`CodecError::is_incomplete`] means "read more"; any other error is a connection error.
pub fn decode_frame(buf: &[u8]) -> Result<(WorkerMsg, usize), CodecError> {
    let (header, payload, _rest) = split_frame(buf)?;
    let msg: WorkerMsg = rmp_serde::from_slice(payload).map_err(|e| CodecError::Decode(e.to_string()))?;
    if msg.msg_type() != header.msg_type {
        return Err(CodecError::TypeMismatch {
            header: header.msg_type,
            body: msg.msg_type(),
        });
    }
    Ok((msg, HEADER_LEN + payload.len()))
}
