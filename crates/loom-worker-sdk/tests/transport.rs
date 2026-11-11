//! Frame I/O over a real socket pair: limits, truncation, unknown types, deadlines.
#![allow(clippy::unwrap_used)]
#![cfg(unix)]

use std::time::Duration;

use loom_proto::num::U64;
use loom_proto::worker::{Credit, HEADER_LEN, MAX_FRAME, WorkerMsg};
use loom_worker_sdk::codec::encode_frame;
use loom_worker_sdk::transport::{FrameReader, FrameWriter, WireError};
use tokio::io::AsyncWriteExt;

fn pair() -> (FrameReader, tokio::net::unix::OwnedWriteHalf, FrameWriter) {
    let (a, b) = tokio::net::UnixStream::pair().unwrap();
    let (ar, _aw) = a.into_split();
    let (_br, bw) = b.into_split();
    // A second pair for a FrameWriter → FrameReader round trip is built by callers when needed.
    let (c, _d) = tokio::net::UnixStream::pair().unwrap();
    let (_cr, cw) = c.into_split();
    (FrameReader::new(Box::new(ar)), bw, FrameWriter::new(Box::new(cw)))
}

fn credit(n: u64) -> WorkerMsg {
    WorkerMsg::Credit(Credit { bytes: U64(n) })
}

#[tokio::test]
async fn roundtrip_and_clean_eof() {
    let (a, b) = tokio::net::UnixStream::pair().unwrap();
    let mut r = FrameReader::new(Box::new(a.into_split().0));
    let mut w = FrameWriter::new(Box::new(b.into_split().1));
    let n = w.write(&credit(7)).await.unwrap();
    assert_eq!(n, encode_frame(&credit(7)).unwrap().len());
    let (m, used) = r.read(Some(Duration::from_secs(1))).await.unwrap().unwrap();
    assert_eq!(m, credit(7));
    assert_eq!(used, n);
    w.shutdown().await.unwrap();
    assert!(r.read(None).await.unwrap().is_none(), "EOF at a frame boundary");
}

#[tokio::test]
async fn oversize_header_is_rejected_before_allocation() {
    let (mut r, mut raw, _) = pair();
    let len = (MAX_FRAME as u32).to_le_bytes();
    raw.write_all(&[len[0], len[1], len[2], len[3], 19, 0, 0, 0])
        .await
        .unwrap();
    let e = r.read(Some(Duration::from_secs(1))).await.unwrap_err();
    assert!(matches!(e, WireError::Codec(_)), "{e:?}");
}

#[tokio::test]
async fn unknown_type_and_flags_are_errors() {
    let (mut r, mut raw, _) = pair();
    raw.write_all(&[0, 0, 0, 0, 99, 0, 0, 0]).await.unwrap();
    assert!(matches!(r.read(None).await.unwrap_err(), WireError::Codec(_)));
    let (mut r, mut raw, _) = pair();
    raw.write_all(&[0, 0, 0, 0, 19, 1, 0, 0]).await.unwrap();
    assert!(matches!(r.read(None).await.unwrap_err(), WireError::Codec(_)));
}

#[tokio::test]
async fn truncated_frame_is_an_error() {
    let (mut r, mut raw, _) = pair();
    let f = encode_frame(&credit(1)).unwrap();
    raw.write_all(&f[..f.len() - 2]).await.unwrap();
    drop(raw);
    assert!(matches!(r.read(None).await.unwrap_err(), WireError::Truncated));
    // Truncated inside the header too.
    let (mut r, mut raw, _) = pair();
    raw.write_all(&f[..HEADER_LEN - 3]).await.unwrap();
    drop(raw);
    assert!(matches!(r.read(None).await.unwrap_err(), WireError::Truncated));
}

#[tokio::test]
async fn stalled_frame_hits_the_read_deadline() {
    let (mut r, mut raw, _) = pair();
    let f = encode_frame(&credit(1)).unwrap();
    raw.write_all(&f[..3]).await.unwrap();
    let e = r.read(Some(Duration::from_millis(50))).await.unwrap_err();
    assert!(matches!(e, WireError::Deadline), "{e:?}");
}

#[tokio::test]
async fn garbage_payload_is_an_error() {
    let (mut r, mut raw, _) = pair();
    raw.write_all(&[3, 0, 0, 0, 19, 0, 0, 0, 0xc1, 0xc1, 0xc1])
        .await
        .unwrap();
    assert!(matches!(r.read(None).await.unwrap_err(), WireError::Codec(_)));
}

#[tokio::test]
async fn oversize_message_is_not_written() {
    let (_, _, mut w) = pair();
    let big = WorkerMsg::Reject(loom_proto::worker::Reject {
        reason: "x".repeat(MAX_FRAME),
    });
    assert!(matches!(w.write(&big).await.unwrap_err(), WireError::Codec(_)));
}
