//! Async frame reader shared by client and entity.

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::frame::{HEADER_LEN, HeaderError, MAX_DOIP_PAYLOAD, Message, parse_header};

/// Scratch size used to skip the body of an unknown payload type without allocating it.
const SKIP_CHUNK: usize = 512;

/// Result of reading one frame that did not yield a message.
#[derive(Debug)]
pub(crate) enum ReadError {
    /// Peer closed or the socket failed.
    Closed(std::io::Error),
    /// Unknown payload type whose (bounded) body was skipped: NACK `0x01`, keep the connection.
    Discarded(HeaderError),
    /// Header or payload invalid: NACK and close.
    Fatal(HeaderError),
}

/// Read one frame. The header is validated before the payload buffer is allocated, so the
/// allocation is at most [`MAX_DOIP_PAYLOAD`] bytes.
pub(crate) async fn read_message<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message, ReadError> {
    let mut head = [0u8; HEADER_LEN];
    r.read_exact(&mut head).await.map_err(ReadError::Closed)?;
    let header = match parse_header(&head) {
        Ok(h) => h,
        Err(e @ HeaderError::UnknownPayloadType { payload_len, .. }) if payload_len <= MAX_DOIP_PAYLOAD => {
            let mut left = payload_len;
            let mut scratch = [0u8; SKIP_CHUNK];
            while left > 0 {
                let n = left.min(SKIP_CHUNK);
                r.read_exact(&mut scratch[..n]).await.map_err(ReadError::Closed)?;
                left -= n;
            }
            return Err(ReadError::Discarded(e));
        }
        Err(e) => return Err(ReadError::Fatal(e)),
    };
    let mut payload = vec![0u8; header.payload_len];
    r.read_exact(&mut payload).await.map_err(ReadError::Closed)?;
    Message::decode(header.payload_type, &payload).map_err(ReadError::Fatal)
}
