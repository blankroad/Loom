//! `reader` — per-port RX thread (UART plan §3.5).
//!
//! Waits up to [`READ_WAIT`] for the first byte, then drains whatever is immediately available up
//! to [`RX_CHUNK_MAX`], stamps the chunk (host mono/wall at read completion, worker `source_seq`,
//! cumulative `rx_offset`) and pushes it into the bridge without ever waiting on the consumer.
//! A stop flag ends the thread within one [`READ_WAIT`]; an I/O error is forwarded as
//! [`BridgeItem::Disconnected`](crate::bridge::BridgeItem::Disconnected) and ends the thread.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::bridge::{BridgeTx, RxChunk};
use crate::clock::{Clock, SeqSource};
use crate::transport::RxHandle;

/// Wait for the first byte of a chunk (§3.5: 20 ms); also bounds stop latency.
pub const READ_WAIT: Duration = Duration::from_millis(20);
/// Largest RX chunk (§3.5 / §3.13: chunks ≤ 16 KiB).
pub const RX_CHUNK_MAX: usize = 16 * 1024;

/// How the reader thread ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderExit {
    /// The stop flag was set.
    Stopped,
    /// An I/O error ended reading (already forwarded to the bridge).
    Disconnected(io::ErrorKind),
}

/// Running reader thread. Dropping it stops and joins the thread.
#[derive(Debug)]
pub struct Reader {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<ReaderExit>>,
}

impl Reader {
    /// Spawns the reader. `start_offset` is the rx offset of the first byte read.
    pub fn spawn(
        mut rx: Box<dyn RxHandle>,
        bridge: BridgeTx,
        seq: Arc<SeqSource>,
        clock: Arc<dyn Clock>,
        start_offset: u64,
    ) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("loom-uart-rx".into())
            .spawn(move || run(rx.as_mut(), &bridge, &seq, clock.as_ref(), start_offset, &stop_t))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Asks the thread to stop (returns immediately).
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Stops the thread and waits for it.
    pub fn stop(mut self) -> ReaderExit {
        self.join_inner()
    }

    /// `true` once the thread has ended (stop or disconnect).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn join_inner(&mut self) -> ReaderExit {
        self.request_stop();
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok(exit)) => exit,
            Some(Err(_)) => ReaderExit::Disconnected(io::ErrorKind::Other),
            None => ReaderExit::Stopped,
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.join_inner();
    }
}

fn run(
    rx: &mut dyn RxHandle,
    bridge: &BridgeTx,
    seq: &SeqSource,
    clock: &dyn Clock,
    start_offset: u64,
    stop: &AtomicBool,
) -> ReaderExit {
    let mut buf = vec![0u8; RX_CHUNK_MAX];
    let mut offset = start_offset;
    while !stop.load(Ordering::SeqCst) {
        let mut filled = match rx.read(&mut buf, READ_WAIT) {
            Ok(0) => continue,
            Ok(n) => n,
            Err(e) => {
                bridge.disconnect(e.kind());
                return ReaderExit::Disconnected(e.kind());
            }
        };
        let mut error = None;
        while filled < RX_CHUNK_MAX {
            match rx.read(&mut buf[filled..], Duration::ZERO) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => {
                    error = Some(e.kind());
                    break;
                }
            }
        }
        let chunk = RxChunk {
            source_seq: seq.next(),
            rx_offset: offset,
            mono_ns: clock.mono_ns(),
            wall_ns: clock.wall_ns(),
            bytes: buf[..filled].to_vec(),
        };
        offset += filled as u64;
        bridge.push(chunk);
        if let Some(kind) = error {
            bridge.disconnect(kind);
            return ReaderExit::Disconnected(kind);
        }
    }
    ReaderExit::Stopped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::{BridgeItem, bridge};
    use crate::clock::FixedClock;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Instant;

    type Steps = Arc<Mutex<VecDeque<Result<Vec<u8>, io::ErrorKind>>>>;

    /// Scripted RX: each entry is returned by one read call (data, or an error kind).
    #[derive(Debug)]
    struct Script(Steps);

    impl RxHandle for Script {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            let next = self.0.lock().expect("script lock").pop_front();
            match next {
                Some(Ok(b)) => {
                    let n = b.len().min(buf.len());
                    buf[..n].copy_from_slice(&b[..n]);
                    Ok(n)
                }
                Some(Err(k)) => Err(io::Error::from(k)),
                None => {
                    std::thread::sleep(wait);
                    Ok(0)
                }
            }
        }
    }

    #[test]
    fn drains_into_one_chunk_with_stamps_and_offsets() {
        let script = Arc::new(Mutex::new(VecDeque::from([
            Ok(b"ab".to_vec()),
            Ok(b"cd".to_vec()),
        ])));
        let (tx, rx) = bridge();
        let clock = Arc::new(FixedClock::new(111, 222));
        let seq = Arc::new(SeqSource::starting_at(5));
        let reader = Reader::spawn(
            Box::new(Script(Arc::clone(&script))),
            tx,
            Arc::clone(&seq),
            clock,
            1000,
        )
        .expect("spawn");
        let item = rx.pop(Duration::from_secs(2)).expect("chunk");
        assert_eq!(
            item,
            BridgeItem::Data(RxChunk {
                source_seq: 5,
                rx_offset: 1000,
                mono_ns: 111,
                wall_ns: 222,
                bytes: b"abcd".to_vec(),
            })
        );
        script.lock().expect("lock").push_back(Ok(b"e".to_vec()));
        match rx.pop(Duration::from_secs(2)) {
            Ok(BridgeItem::Data(c)) => assert_eq!((c.source_seq, c.rx_offset), (6, 1004)),
            other => panic!("unexpected {other:?}"),
        }
        let t = Instant::now();
        assert_eq!(reader.stop(), ReaderExit::Stopped);
        assert!(t.elapsed() < READ_WAIT * 3, "stop latency {:?}", t.elapsed());
    }

    #[test]
    fn error_after_data_forwards_data_then_disconnect() {
        let script = Arc::new(Mutex::new(VecDeque::from([
            Ok(b"xy".to_vec()),
            Err(io::ErrorKind::BrokenPipe),
        ])));
        let (tx, rx) = bridge();
        let reader = Reader::spawn(
            Box::new(Script(script)),
            tx,
            Arc::new(SeqSource::default()),
            Arc::new(FixedClock::default()),
            0,
        )
        .expect("spawn");
        assert!(matches!(
            rx.pop(Duration::from_secs(2)),
            Ok(BridgeItem::Data(c)) if c.bytes == b"xy"
        ));
        assert_eq!(
            rx.pop(Duration::from_secs(2)),
            Ok(BridgeItem::Disconnected(io::ErrorKind::BrokenPipe))
        );
        assert_eq!(reader.stop(), ReaderExit::Disconnected(io::ErrorKind::BrokenPipe));
    }

    #[test]
    fn chunks_are_capped_at_16k() {
        let script = Arc::new(Mutex::new(VecDeque::from([
            Ok(vec![1u8; 10_000]),
            Ok(vec![2u8; 10_000]),
        ])));
        let (tx, rx) = bridge();
        let _reader = Reader::spawn(
            Box::new(Script(script)),
            tx,
            Arc::new(SeqSource::default()),
            Arc::new(FixedClock::default()),
            0,
        )
        .expect("spawn");
        match rx.pop(Duration::from_secs(2)) {
            Ok(BridgeItem::Data(c)) => assert_eq!(c.bytes.len(), RX_CHUNK_MAX),
            other => panic!("unexpected {other:?}"),
        }
    }
}
