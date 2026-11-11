//! Async frame reader shared by client and entity.

use std::future::Future;
use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{Instant, sleep_until};

use crate::frame::{HEADER_LEN, HeaderError, MAX_DOIP_PAYLOAD, Message, parse_header};

/// Scratch size used to skip the body of an unknown payload type without allocating it.
const SKIP_CHUNK: usize = 512;

/// Socket writes (including control/entity frames) wait at most this long. Failure/cancellation
/// can leave a partial frame: callers must discard the connection, never resume another frame.
pub const MAX_FRAME_WRITE_TIME: Duration = Duration::from_secs(2);

/// Deadline/close win ties with writable readiness. A failed write may have emitted a prefix.
pub(crate) async fn write_bounded<W, F>(
    writer: &mut W,
    bytes: &[u8],
    until: Instant,
    closed: F,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    F: Future<Output = ()>,
{
    tokio::select! {
        biased;
        _ = closed => Err(io::Error::new(io::ErrorKind::ConnectionAborted, "DoIP write cancelled/closed")),
        _ = sleep_until(until) => Err(io::Error::new(io::ErrorKind::TimedOut, "DoIP write deadline")),
        result = writer.write_all(bytes) => {
            result?;
            if Instant::now() > until {
                Err(io::Error::new(io::ErrorKind::TimedOut, "DoIP write completed after deadline"))
            } else {
                Ok(())
            }
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test(start_paused = true)]
    async fn partial_write_timeout_discards_stream_instead_of_appending_a_frame() {
        let (mut writer, mut peer) = tokio::io::duplex(1);
        let task = tokio::spawn(async move {
            let result = write_bounded(
                &mut writer,
                &[1, 2, 3],
                Instant::now() + Duration::from_millis(10),
                std::future::pending(),
            )
            .await;
            // Same policy as client/entity: discard this writer, never append another frame.
            drop(writer);
            result
        });
        let error = tokio::time::timeout(Duration::from_millis(20), task)
            .await
            .expect("bounded write")
            .expect("task")
            .expect_err("stalled prefix must time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let mut bytes = [0; 8];
        assert_eq!(peer.read(&mut bytes).await.expect("prefix"), 1);
        assert_eq!(bytes[0], 1);
        assert_eq!(peer.read(&mut bytes).await.expect("closed"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_partial_write_observes_close_without_waiting_for_timeout() {
        let (mut writer, mut peer) = tokio::io::duplex(1);
        let (close, closed) = oneshot::channel();
        let task = tokio::spawn(async move {
            write_bounded(
                &mut writer,
                &[1, 2, 3],
                Instant::now() + MAX_FRAME_WRITE_TIME,
                async {
                    let _ = closed.await;
                },
            )
            .await
        });
        tokio::task::yield_now().await;
        close.send(()).expect("close listener");
        let error = tokio::time::timeout(Duration::from_millis(10), task)
            .await
            .expect("close is prompt")
            .expect("task")
            .expect_err("closed");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        let mut bytes = [0; 8];
        assert_eq!(peer.read(&mut bytes).await.expect("prefix"), 1);
        assert_eq!(peer.read(&mut bytes).await.expect("EOF"), 0);
    }
}
