//! `stream` backend: an external software producer streams pcapng over a loopback TCP
//! connection. This is not packet capture (no BPF/NIC/raw socket) and not file replay.
//!
//! Bounds: one reader thread per source; every socket read has a short deadline; partial
//! block bytes are preserved across deadlines and only whole blocks reach the parser; the
//! hand-off queue is capped by block count, by summed bytes and by individual block size;
//! the reader hands off with a stop-aware non-blocking `try_send` loop, so a full queue
//! back-pressures the producer through TCP instead of dropping. Closing shuts the socket,
//! signals the reader and polls its completion for at most [`STREAM_JOIN_BOUND`]; a reader
//! that has not finished is reported as an error and kept in [`outstanding_stream_readers`]
//! until it is actually reaped — it is never counted as cleaned up.
use crate::*;
use loom_eth_pcapng::{Block, PcapNgReader};
use std::{
    collections::VecDeque,
    io::Read,
    net::{Shutdown, SocketAddr, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::Instant,
};

/// Hand-off queue: maximum whole blocks waiting for the capture loop.
pub const MAX_STREAM_QUEUE_BLOCKS: usize = 64;
/// Hand-off queue: maximum summed bytes of waiting blocks (individual cap is
/// `loom_eth_pcapng::MAX_BLOCK_BYTES`).
pub const MAX_STREAM_QUEUE_BYTES: usize = 8 * 1024 * 1024;
/// Deadline of every socket read; also the reader's stop-check cadence.
pub const STREAM_READ_DEADLINE: Duration = Duration::from_millis(20);
/// Back-pressure wait between non-blocking hand-off attempts (stop-aware).
pub const STREAM_HANDOFF_WAIT: Duration = Duration::from_millis(2);
/// Close: maximum time to wait for the reader thread to finish after shutdown.
pub const STREAM_JOIN_BOUND: Duration = Duration::from_millis(500);
/// Default and maximum connect + first SHB/IDB bound.
pub const DEFAULT_STREAM_START: Duration = Duration::from_secs(2);
pub const MAX_STREAM_START: Duration = Duration::from_secs(10);
/// Maximum idle timeout (no bytes from the producer).
pub const MAX_STREAM_IDLE: Duration = Duration::from_secs(600);
/// Socket read chunk; bounds the partial buffer to one block plus one chunk.
const READ_CHUNK: usize = 64 * 1024;
/// Metadata blocks processed per `next` call before yielding.
const MAX_METADATA_PER_POLL: usize = 32;
const SHB_TYPE: [u8; 4] = [0x0a, 0x0d, 0x0d, 0x0a];
const EPB_TYPE: u32 = 6;
const EPB_PACKETID: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptions {
    /// Producer address; must be loopback (127.0.0.0/8 or ::1) with a non-zero port.
    pub addr: SocketAddr,
    /// Bound for connect plus the first SHB and IDB.
    pub start_timeout: Duration,
    /// Error out when no byte arrives for this long; `None` disables.
    pub idle_timeout: Option<Duration>,
}
impl StreamOptions {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            start_timeout: DEFAULT_STREAM_START,
            idle_timeout: Some(Duration::from_secs(30)),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !self.addr.ip().is_loopback() || self.addr.port() == 0 {
            return Err(CaptureError::Invalid(
                "stream producer must be a loopback address with a port",
            ));
        }
        if self.start_timeout.is_zero() || self.start_timeout > MAX_STREAM_START {
            return Err(CaptureError::Invalid("stream start timeout"));
        }
        if self
            .idle_timeout
            .is_some_and(|d| d.is_zero() || d > MAX_STREAM_IDLE)
        {
            return Err(CaptureError::Invalid("stream idle timeout"));
        }
        Ok(())
    }
}

/// Why the reader ended. Only `Eof` is a clean end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEnd {
    /// Producer closed at a block boundary.
    Eof,
    /// Producer closed with this many bytes of an incomplete block buffered.
    Truncated(usize),
    /// Socket error.
    Io(String),
    /// No bytes for the idle timeout.
    Idle,
    /// Not a valid pcapng block stream.
    Framing(&'static str),
    /// Stopped by close/stop (not an error).
    Stopped,
}
impl StreamEnd {
    fn message(&self) -> String {
        match self {
            Self::Eof => "producer_eof".into(),
            Self::Truncated(n) => format!("stream truncated: {n} bytes after last complete block"),
            Self::Io(e) => format!("stream read error: {e}"),
            Self::Idle => "stream idle timeout".into(),
            Self::Framing(m) => format!("stream framing: {m}"),
            Self::Stopped => "stream stopped".into(),
        }
    }
}

#[derive(Debug)]
enum Msg {
    Block(Vec<u8>),
    End(StreamEnd),
}

#[derive(Debug, Default)]
struct Link {
    /// Close requested (by `close`/Drop).
    shutdown: AtomicBool,
    /// Summed bytes of blocks in the hand-off queue.
    queued_bytes: AtomicUsize,
    /// Reader thread finished (set by a guard, also on panic).
    done: AtomicBool,
    /// Capture-loop stop signal, once attached.
    stop: Mutex<Option<StopSignal>>,
}
impl Link {
    fn halted(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
            || self
                .stop
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .is_some_and(StopSignal::is_stopped)
    }
}
#[derive(Debug)]
struct DoneGuard(Arc<Link>);
impl Drop for DoneGuard {
    fn drop(&mut self) {
        self.0.done.store(true, Ordering::Release);
    }
}

static OUTSTANDING: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
/// Stream reader threads that missed the close bound and are not yet reaped. Finished ones
/// are joined (reaped) by this call. Non-zero means cleanup is NOT complete.
pub fn outstanding_stream_readers() -> usize {
    let mut list = OUTSTANDING.lock().unwrap_or_else(|p| p.into_inner());
    let mut keep = Vec::with_capacity(list.len());
    for h in list.drain(..) {
        if h.is_finished() {
            let _ = h.join();
        } else {
            keep.push(h);
        }
    }
    *list = keep;
    list.len()
}

/// Little/big endian of the current section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Le,
    Be,
}
impl End {
    fn u16(self, b: &[u8]) -> Option<u16> {
        let a: [u8; 2] = b.get(..2)?.try_into().ok()?;
        Some(match self {
            Self::Le => u16::from_le_bytes(a),
            Self::Be => u16::from_be_bytes(a),
        })
    }
    fn u32(self, b: &[u8]) -> Option<u32> {
        let a: [u8; 4] = b.get(..4)?.try_into().ok()?;
        Some(match self {
            Self::Le => u32::from_le_bytes(a),
            Self::Be => u32::from_be_bytes(a),
        })
    }
    fn u64(self, b: &[u8]) -> Option<u64> {
        let a: [u8; 8] = b.get(..8)?.try_into().ok()?;
        Some(match self {
            Self::Le => u64::from_le_bytes(a),
            Self::Be => u64::from_be_bytes(a),
        })
    }
}
fn shb_endian(block: &[u8]) -> Option<End> {
    match block.get(8..12)? {
        [0x4d, 0x3c, 0x2b, 0x1a] => Some(End::Le),
        [0x1a, 0x2b, 0x3c, 0x4d] => Some(End::Be),
        _ => None,
    }
}

/// Splits a byte stream into whole pcapng blocks. Pure; used by the reader thread and tests.
#[derive(Debug, Default)]
pub(crate) struct Framer {
    partial: Vec<u8>,
    endian: Option<End>,
}
impl Framer {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.partial.extend_from_slice(bytes);
    }
    pub(crate) fn buffered(&self) -> usize {
        self.partial.len()
    }
    /// Next whole block, `Ok(None)` when more bytes are needed.
    pub(crate) fn next_block(&mut self) -> std::result::Result<Option<Vec<u8>>, &'static str> {
        if self.partial.len() < 8 {
            return Ok(None);
        }
        let shb = self.partial[..4] == SHB_TYPE;
        if shb {
            if self.partial.len() < 12 {
                return Ok(None);
            }
            self.endian = Some(shb_endian(&self.partial).ok_or("section byte order magic")?);
        } else if self.endian.is_none() {
            return Err("stream must start with a pcapng section header (classic pcap is not supported)");
        }
        let e = self.endian.ok_or("missing section")?;
        let n = e.u32(&self.partial[4..]).ok_or("block header")? as usize;
        if n < if shb { 28 } else { 12 } || !n.is_multiple_of(4) {
            return Err("block length");
        }
        if n > loom_eth_pcapng::MAX_BLOCK_BYTES {
            return Err("block exceeds individual block cap");
        }
        if self.partial.len() < n {
            return Ok(None);
        }
        if e.u32(&self.partial[n - 4..]).ok_or("block trailer")? as usize != n {
            return Err("block trailer length");
        }
        let rest = self.partial.split_off(n);
        Ok(Some(std::mem::replace(&mut self.partial, rest)))
    }
}

/// `epb_packetid` of an EPB block (section endian `e`), if present.
fn epb_packetid(block: &[u8], e: End) -> Option<u64> {
    if e.u32(block)? != EPB_TYPE {
        return None;
    }
    let caplen = e.u32(block.get(20..)?)? as usize;
    let mut opts = block.get(28 + caplen.div_ceil(4) * 4..block.len().checked_sub(4)?)?;
    while opts.len() >= 4 {
        let code = e.u16(opts)?;
        let n = e.u16(&opts[2..])? as usize;
        if code == 0 {
            return None;
        }
        let data = opts.get(4..4 + n)?;
        if code == EPB_PACKETID && n == 8 {
            return e.u64(data);
        }
        opts = opts.get(4 + n.div_ceil(4) * 4..)?;
    }
    None
}

/// Feeds exactly the queued whole blocks to the parser (never a partial block).
#[derive(Debug, Clone, Default)]
struct BlockFeed(Arc<Mutex<VecDeque<u8>>>);
impl Read for BlockFeed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut q = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let n = buf.len().min(q.len());
        for (dst, src) in buf.iter_mut().zip(q.drain(..n)) {
            *dst = src;
        }
        Ok(n)
    }
}

#[derive(Debug)]
pub struct StreamSource {
    rx: Receiver<Msg>,
    link: Arc<Link>,
    socket: Option<TcpStream>,
    thread: Option<JoinHandle<()>>,
    feed: BlockFeed,
    parser: PcapNgReader<BlockFeed>,
    endian: Option<End>,
    interface: Interface,
    exhausted: bool,
    recv: u64,
    last_packetid: Option<u64>,
    missing: u64,
    stop: StopSignal,
}
impl StreamSource {
    /// Connects to the producer and waits (bounded by `start_timeout`, in total) for the
    /// first SHB and IDB. Fails without leaving a running reader.
    pub fn connect(options: StreamOptions) -> Result<Self> {
        options.validate()?;
        // A reader that missed its close bound is unreaped evidence; never open on top of it.
        let outstanding = outstanding_stream_readers();
        if outstanding != 0 {
            return Err(CaptureError::Backend(format!(
                "{outstanding} earlier stream reader thread(s) still outstanding; cleanup incomplete, restart the eth worker"
            )));
        }
        let started = Instant::now();
        let socket = TcpStream::connect_timeout(&options.addr, options.start_timeout)?;
        socket.set_read_timeout(Some(STREAM_READ_DEADLINE))?;
        // Receive-only: Loom never writes into the producer's connection.
        socket.shutdown(Shutdown::Write)?;
        let reader_socket = socket.try_clone()?;
        let link = Arc::new(Link::default());
        let (tx, rx) = mpsc::sync_channel(MAX_STREAM_QUEUE_BLOCKS);
        let thread_link = link.clone();
        let idle = options.idle_timeout;
        let thread = std::thread::Builder::new()
            .name("loom-eth-stream".into())
            .spawn(move || reader(reader_socket, tx, thread_link, idle))?;
        let feed = BlockFeed::default();
        let mut source = Self {
            rx,
            link,
            socket: Some(socket),
            thread: Some(thread),
            parser: PcapNgReader::new(feed.clone()),
            feed,
            endian: None,
            interface: Interface {
                linktype: 1,
                snaplen: 0,
                name: String::new(),
                tsresol: TimestampResolution::NANO,
            },
            exhausted: false,
            recv: 0,
            last_packetid: None,
            missing: 0,
            stop: StopSignal::default(),
        };
        let deadline = started + options.start_timeout;
        loop {
            if !source.parser.interfaces().is_empty() {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                source.close_quietly();
                return Err(CaptureError::Backend(format!(
                    "stream start: no pcapng section+interface within {} ms",
                    options.start_timeout.as_millis()
                )));
            }
            match source.rx.recv_timeout(remaining) {
                Ok(Msg::Block(b)) => {
                    // An EPB before any IDB is rejected by the parser (EPB interface id).
                    if let Err(e) = source.parse(b) {
                        source.close_quietly();
                        return Err(e);
                    }
                }
                Ok(Msg::End(end)) => {
                    source.close_quietly();
                    return Err(CaptureError::Backend(format!("stream start: {}", end.message())));
                }
                Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => {
                    source.close_quietly();
                    return Err(CaptureError::Backend("stream start: reader exited".into()));
                }
            }
        }
        source.interface = source.parser.interfaces()[0].clone();
        Ok(source)
    }
    /// Parses one whole block; returns a packet if it was an EPB.
    fn parse(&mut self, block: Vec<u8>) -> Result<Option<loom_eth_pcapng::Packet>> {
        self.link.queued_bytes.fetch_sub(block.len(), Ordering::AcqRel);
        if block.get(..4) == Some(&SHB_TYPE[..]) {
            // MVP capability: one section, one interface. A second section could restart
            // packet numbering; reject it instead of silently resetting producer sequence state.
            if self.endian.is_some() {
                return Err(CaptureError::Unsupported(
                    "stream backend accepts one pcapng section (MVP); start a new capture for a new section",
                ));
            }
            self.endian = shb_endian(&block);
        }
        let packetid = self.endian.and_then(|e| epb_packetid(&block, e));
        self.feed
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(block);
        let parsed = self.parser.next_block()?;
        // A whole block must be consumed exactly; leftovers mean a framing/parser mismatch.
        if !self.feed.0.lock().unwrap_or_else(|p| p.into_inner()).is_empty() {
            return Err(CaptureError::Invalid("stream block not consumed by parser"));
        }
        match parsed {
            Some(Block::Packet(p)) => {
                if let Some(id) = packetid {
                    if let Some(last) = self.last_packetid {
                        if id <= last {
                            return Err(CaptureError::Invalid("producer packetid regressed"));
                        }
                        self.missing = self.missing.saturating_add(id - last - 1);
                    }
                    self.last_packetid = Some(id);
                }
                Ok(Some(p))
            }
            Some(Block::Interface { .. }) if self.parser.interfaces().len() > 1 => {
                Err(CaptureError::Unsupported(
                    "stream backend accepts one interface (MVP); use one IDB per producer connection",
                ))
            }
            Some(_) => Ok(None),
            None => Err(CaptureError::Invalid("stream parser ended unexpectedly")),
        }
    }
    fn to_raw(&self, p: loom_eth_pcapng::Packet) -> Result<RawFrame> {
        let interface = self
            .parser
            .interfaces()
            .get(p.interface_id as usize)
            .cloned()
            .ok_or(CaptureError::Invalid("stream interface"))?;
        let mut comments = p.comments;
        // External producers cannot claim Loom attribution: embedded loom metadata stays
        // visible as an ordinary comment only.
        if let Some(meta) = p.meta
            && let Ok(c) = meta.to_comment()
        {
            comments.push(c);
        }
        Ok(RawFrame {
            ts_ns: p.ts_ns,
            caplen: p.caplen,
            origlen: p.origlen,
            data: p.data,
            interface_id: p.interface_id,
            section_index: p.section_index,
            interface,
            comments,
            meta: None,
        })
    }
    /// Shut the socket, signal the reader and wait for it at most [`STREAM_JOIN_BOUND`].
    /// Err when the reader did not finish; it is then kept in the outstanding registry.
    pub fn close(&mut self) -> Result<()> {
        self.link.shutdown.store(true, Ordering::Release);
        if let Some(s) = self.socket.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let started = Instant::now();
        while !self.link.done.load(Ordering::Acquire) && started.elapsed() < STREAM_JOIN_BOUND {
            // Drain so a reader blocked on hand-off sees space (it also checks shutdown).
            while let Ok(m) = self.rx.try_recv() {
                if let Msg::Block(b) = m {
                    self.link.queued_bytes.fetch_sub(b.len(), Ordering::AcqRel);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.link.done.load(Ordering::Acquire) {
            thread.join().map_err(|_| CaptureError::ThreadPanic)
        } else {
            OUTSTANDING.lock().unwrap_or_else(|p| p.into_inner()).push(thread);
            Err(CaptureError::Backend(format!(
                "stream reader not finished within {} ms; kept as outstanding",
                STREAM_JOIN_BOUND.as_millis()
            )))
        }
    }
    fn close_quietly(&mut self) {
        let _ = self.close();
    }
    /// Cumulative producer packets missing according to `epb_packetid`.
    pub fn producer_missing(&self) -> u64 {
        self.missing
    }
}
impl Drop for StreamSource {
    fn drop(&mut self) {
        self.close_quietly();
    }
}
impl CaptureSource for StreamSource {
    fn interface(&self) -> Interface {
        self.interface.clone()
    }
    fn timestamp_source(&self) -> TimestampSource {
        TimestampSource::External
    }
    fn is_exhausted(&self) -> bool {
        self.exhausted
    }
    fn set_stop_signal(&mut self, s: StopSignal) {
        *self.link.stop.lock().unwrap_or_else(|p| p.into_inner()) = Some(s.clone());
        self.stop = s;
    }
    fn producer_seq_missing(&self) -> Option<u64> {
        Some(self.missing)
    }
    fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        if self.exhausted || self.stop.is_stopped() {
            return Ok(None);
        }
        let started = Instant::now();
        let wait = timeout.min(MAX_SOURCE_POLL);
        for _ in 0..MAX_METADATA_PER_POLL {
            let remaining = wait.saturating_sub(started.elapsed());
            match self.rx.recv_timeout(remaining) {
                Ok(Msg::Block(b)) => {
                    if let Some(p) = self.parse(b)? {
                        self.recv = self.recv.saturating_add(1);
                        return self.to_raw(p).map(Some);
                    }
                }
                Ok(Msg::End(StreamEnd::Eof)) => {
                    self.exhausted = true;
                    return Ok(None);
                }
                Ok(Msg::End(StreamEnd::Stopped)) => return Ok(None),
                Ok(Msg::End(end)) => return Err(CaptureError::Backend(end.message())),
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    if self.stop.is_stopped() || self.link.shutdown.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    return Err(CaptureError::Backend("stream reader exited".into()));
                }
            }
        }
        Ok(None)
    }
    fn stats(&mut self) -> DropStats {
        // TCP + back-pressure: nothing is dropped inside the source; producer-side gaps
        // are reported separately from epb_packetid.
        DropStats {
            recv: self.recv,
            ..DropStats::ZERO
        }
    }
}

/// Stop-aware non-blocking hand-off. `false` when halted or the receiver is gone.
fn hand_off(tx: &SyncSender<Msg>, link: &Link, mut msg: Msg) -> bool {
    let charge = match &msg {
        Msg::Block(b) => b.len(),
        Msg::End(_) => 0,
    };
    loop {
        if link.halted() {
            return false;
        }
        let fits = link
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(charge).filter(|v| *v <= MAX_STREAM_QUEUE_BYTES)
            })
            .is_ok();
        if fits {
            match tx.try_send(msg) {
                Ok(()) => return true,
                Err(TrySendError::Full(m)) => {
                    link.queued_bytes.fetch_sub(charge, Ordering::AcqRel);
                    msg = m;
                }
                Err(TrySendError::Disconnected(_)) => {
                    link.queued_bytes.fetch_sub(charge, Ordering::AcqRel);
                    return false;
                }
            }
        }
        let stop = link.stop.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match stop {
            Some(s) => s.wait(STREAM_HANDOFF_WAIT),
            None => std::thread::sleep(STREAM_HANDOFF_WAIT),
        }
    }
}

fn reader(mut socket: TcpStream, tx: SyncSender<Msg>, link: Arc<Link>, idle: Option<Duration>) {
    let _done = DoneGuard(link.clone());
    let mut framer = Framer::default();
    let mut chunk = vec![0u8; READ_CHUNK];
    let mut last_rx = Instant::now();
    let end = loop {
        if link.halted() {
            break StreamEnd::Stopped;
        }
        match socket.read(&mut chunk) {
            Ok(0) => {
                break if framer.buffered() == 0 {
                    StreamEnd::Eof
                } else {
                    StreamEnd::Truncated(framer.buffered())
                };
            }
            Ok(n) => {
                framer.push(&chunk[..n]);
                let mut failed = None;
                loop {
                    match framer.next_block() {
                        Ok(Some(block)) => {
                            if !hand_off(&tx, &link, Msg::Block(block)) {
                                failed = Some(StreamEnd::Stopped);
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(m) => {
                            failed = Some(StreamEnd::Framing(m));
                            break;
                        }
                    }
                }
                if let Some(f) = failed {
                    break f;
                }
                // Hand-off time under back-pressure is not producer idleness.
                last_rx = Instant::now();
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                if idle.is_some_and(|d| last_rx.elapsed() >= d) {
                    break StreamEnd::Idle;
                }
            }
            Err(e) => break StreamEnd::Io(format!("{:?}: {e}", e.kind())),
        }
    };
    if end != StreamEnd::Stopped {
        let _ = hand_off(&tx, &link, Msg::End(end));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn block(kind: u32, body: &[u8]) -> Vec<u8> {
        let pad = (4 - body.len() % 4) % 4;
        let n = (12 + body.len() + pad) as u32;
        let mut b = kind.to_le_bytes().to_vec();
        b.extend(n.to_le_bytes());
        b.extend(body);
        b.extend(vec![0; pad]);
        b.extend(n.to_le_bytes());
        b
    }
    fn shb() -> Vec<u8> {
        let mut body = 0x1A2B_3C4Du32.to_le_bytes().to_vec();
        body.extend(1u16.to_le_bytes());
        body.extend(0u16.to_le_bytes());
        body.extend((-1i64).to_le_bytes());
        block(0x0A0D_0D0A, &body)
    }
    fn epb(id: Option<u64>, data: &[u8]) -> Vec<u8> {
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend(0u32.to_le_bytes());
        body.extend(7u32.to_le_bytes());
        body.extend((data.len() as u32).to_le_bytes());
        body.extend((data.len() as u32).to_le_bytes());
        body.extend(data);
        body.extend(vec![0; (4 - data.len() % 4) % 4]);
        if let Some(id) = id {
            body.extend(5u16.to_le_bytes());
            body.extend(8u16.to_le_bytes());
            body.extend(id.to_le_bytes());
            body.extend([0; 4]);
        }
        block(6, &body)
    }
    #[test]
    fn framer_preserves_partial_bytes_across_one_byte_pushes() {
        let mut f = Framer::default();
        let mut all = shb();
        all.extend(epb(Some(1), b"abc"));
        let mut out = Vec::new();
        for b in &all {
            f.push(std::slice::from_ref(b));
            while let Some(x) = f.next_block().expect("frame") {
                out.push(x);
            }
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out.concat(), all);
        assert_eq!(f.buffered(), 0);
    }
    #[test]
    fn framer_rejects_classic_pcap_oversize_and_bad_trailer() {
        let mut f = Framer::default();
        f.push(&[0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0]);
        assert!(f.next_block().is_err());
        let mut f = Framer::default();
        let mut s = shb();
        let big = (loom_eth_pcapng::MAX_BLOCK_BYTES as u32 + 4).to_le_bytes();
        s[4..8].copy_from_slice(&big);
        f.push(&s);
        assert!(f.next_block().is_err());
        let mut f = Framer::default();
        let mut s = shb();
        let last = s.len() - 4;
        s[last] ^= 0x04;
        f.push(&s);
        assert!(f.next_block().is_err());
    }
    #[test]
    fn packetid_is_read_from_epb_options() {
        assert_eq!(epb_packetid(&epb(Some(42), b"hello"), End::Le), Some(42));
        assert_eq!(epb_packetid(&epb(None, b"hello"), End::Le), None);
    }
    #[test]
    fn non_loopback_and_zero_port_rejected() {
        let bad = |s: &str| StreamOptions::new(s.parse().expect("addr")).validate().is_err();
        assert!(bad("192.0.2.1:1"));
        assert!(bad("127.0.0.1:0"));
        assert!(!bad("127.0.0.1:9"));
        assert!(!bad("[::1]:9"));
    }
    fn idb() -> Vec<u8> {
        let mut body = 1u16.to_le_bytes().to_vec();
        body.extend(0u16.to_le_bytes());
        body.extend(65535u32.to_le_bytes());
        body.extend(9u16.to_le_bytes());
        body.extend(1u16.to_le_bytes());
        body.extend([9, 0, 0, 0]);
        body.extend([0; 4]);
        block(1, &body)
    }
    fn epb_comment(data: &[u8], comment: &str) -> Vec<u8> {
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend(0u32.to_le_bytes());
        body.extend(7u32.to_le_bytes());
        body.extend((data.len() as u32).to_le_bytes());
        body.extend((data.len() as u32).to_le_bytes());
        body.extend(data);
        body.extend(vec![0; (4 - data.len() % 4) % 4]);
        body.extend(1u16.to_le_bytes());
        body.extend((comment.len() as u16).to_le_bytes());
        body.extend(comment.as_bytes());
        body.extend(vec![0; (4 - comment.len() % 4) % 4]);
        body.extend([0; 4]);
        block(6, &body)
    }
    fn header() -> Vec<u8> {
        let mut v = shb();
        v.extend(idb());
        v
    }
    /// Independent loopback producer: runs `serve` on the accepted connection.
    fn producer(
        serve: impl FnOnce(std::net::TcpStream) + Send + 'static,
    ) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = l.local_addr().expect("addr");
        let t = std::thread::spawn(move || {
            let (c, _) = l.accept().expect("accept");
            serve(c);
        });
        (addr, t)
    }
    fn options(addr: SocketAddr) -> StreamOptions {
        let mut o = StreamOptions::new(addr);
        o.start_timeout = Duration::from_millis(1000);
        o.idle_timeout = Some(Duration::from_secs(5));
        o
    }
    /// Polls `next` for at most `bound`; returns frames and the terminal result.
    fn drain(s: &mut StreamSource, bound: Duration) -> (Vec<RawFrame>, Option<CaptureError>) {
        let start = Instant::now();
        let mut out = Vec::new();
        while start.elapsed() < bound {
            match s.next(Duration::from_millis(20)) {
                Ok(Some(f)) => out.push(f),
                Ok(None) if s.is_exhausted() => return (out, None),
                Ok(None) => (),
                Err(e) => return (out, Some(e)),
            }
        }
        (out, None)
    }
    #[test]
    fn one_byte_writes_with_pauses_yield_identical_frame_then_clean_eof() {
        let frame = b"external-frame-bytes".to_vec();
        let mut wire = header();
        wire.extend(epb(Some(1), &frame));
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            let (head, rest) = wire.split_at(header().len());
            c.write_all(head).expect("header");
            for (i, b) in rest.iter().enumerate() {
                c.write_all(std::slice::from_ref(b)).expect("byte");
                if i % 8 == 0 {
                    // Longer than the 20 ms read deadline: partial bytes must survive.
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
            std::thread::sleep(Duration::from_millis(30));
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        assert_eq!(s.timestamp_source(), TimestampSource::External);
        let (frames, err) = drain(&mut s, Duration::from_secs(5));
        assert!(err.is_none(), "{err:?}");
        assert!(s.is_exhausted(), "clean EOF");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, frame);
        // Producer EPB timestamp (ticks 7 at ns resolution), not a host clock.
        assert_eq!(frames[0].ts_ns, 7);
        assert!(frames[0].meta.is_none());
        t.join().expect("producer");
        assert!(s.close().is_ok());
    }
    #[test]
    fn eof_mid_block_is_truncated_error_with_partial_count() {
        let mut wire = header();
        let e = epb(None, b"0123456789");
        wire.extend(&e[..10]);
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            c.write_all(&wire).expect("write");
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        let (frames, err) = drain(&mut s, Duration::from_secs(5));
        assert!(frames.is_empty());
        let msg = format!("{:?}", err.expect("truncated error"));
        assert!(msg.contains("truncated: 10 bytes"), "{msg}");
        t.join().expect("producer");
    }
    #[test]
    fn close_while_producer_hangs_mid_block_is_bounded() {
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            let mut wire = header();
            wire.extend(&epb(None, b"abcdef")[..9]);
            c.write_all(&wire).expect("write");
            // Hold the connection open with no EOF (Loom half-closes its side, so a read
            // here would return EOF at once); bounded hold.
            std::thread::sleep(Duration::from_millis(1500));
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        let (frames, err) = drain(&mut s, Duration::from_millis(100));
        assert!(frames.is_empty() && err.is_none());
        let started = Instant::now();
        assert!(s.close().is_ok(), "reader joined");
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(outstanding_stream_readers(), 0);
        t.join().expect("producer");
    }
    #[test]
    fn capture_loop_stop_with_hung_producer_joins_within_bound() {
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            let mut wire = header();
            wire.extend(epb(Some(1), b"one"));
            wire.extend(&epb(Some(2), b"two")[..7]);
            c.write_all(&wire).expect("write");
            // Hold the connection open with no EOF (Loom half-closes its side, so a read
            // here would return EOF at once); bounded hold.
            std::thread::sleep(Duration::from_millis(1500));
        });
        let s = StreamSource::connect(options(addr)).expect("connect");
        let capture = CaptureLoop::start(s, CaptureConfig::default()).expect("loop");
        let q = capture.queue();
        let first = q.pop_timeout(Duration::from_secs(2)).expect("first frame");
        assert_eq!(first.raw.data, b"one");
        assert_eq!(first.ts_source, TimestampSource::External);
        let started = Instant::now();
        capture.stop_handle().stop();
        let summary = capture.join().expect("join");
        assert!(
            started.elapsed() < Duration::from_millis(600),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(summary.frames_seen, 1);
        assert_eq!(outstanding_stream_readers(), 0);
        t.join().expect("producer");
    }
    #[test]
    fn packetid_jump_counts_missing_and_regression_errors() {
        let mut wire = header();
        for id in [1u64, 2, 5, 4] {
            wire.extend(epb(Some(id), &[id as u8; 4]));
        }
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            c.write_all(&wire).expect("write");
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        let (frames, err) = drain(&mut s, Duration::from_secs(5));
        assert_eq!(frames.len(), 3);
        assert_eq!(s.producer_seq_missing(), Some(2));
        let msg = format!("{:?}", err.expect("regression"));
        assert!(msg.contains("regressed"), "{msg}");
        t.join().expect("producer");
    }
    #[test]
    fn producer_seq_gap_reaches_queue_gap_report() {
        let mut wire = header();
        for id in [1u64, 4] {
            wire.extend(epb(Some(id), &[id as u8; 4]));
        }
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            c.write_all(&wire).expect("write");
        });
        let s = StreamSource::connect(options(addr)).expect("connect");
        let capture = CaptureLoop::start(s, CaptureConfig::default()).expect("loop");
        let q = capture.queue();
        let summary = capture.join().expect("join at EOF");
        assert_eq!(summary.frames_seen, 2);
        let mut producer_seq = 0;
        while let Some(g) = q.try_gap() {
            producer_seq += g.counts.producer_seq;
        }
        assert_eq!(producer_seq, 2);
        t.join().expect("producer");
    }
    #[test]
    fn second_section_and_second_interface_are_rejected_not_reset() {
        for extra in [shb(), idb()] {
            let mut wire = header();
            wire.extend(epb(Some(1), b"a"));
            wire.extend(extra);
            wire.extend(epb(Some(1), b"b"));
            let (addr, t) = producer(move |mut c| {
                use std::io::Write;
                c.write_all(&wire).expect("write");
            });
            let mut s = StreamSource::connect(options(addr)).expect("connect");
            let (frames, err) = drain(&mut s, Duration::from_secs(5));
            assert_eq!(frames.len(), 1);
            let msg = format!("{:?}", err.expect("unsupported"));
            assert!(msg.contains("MVP"), "{msg}");
            t.join().expect("producer");
        }
    }
    #[test]
    fn embedded_loom_meta_is_not_attribution() {
        let meta = loom_eth_pcapng::LoomMeta {
            seq: 3,
            dir: Direction::Tx,
            req: Some("w9".into()),
            org: loom_eth_pcapng::Origin::Agent,
        };
        let comment = meta.to_comment().expect("comment");
        let mut wire = header();
        wire.extend(epb_comment(b"claims-agent", &comment));
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            c.write_all(&wire).expect("write");
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        let (frames, _) = drain(&mut s, Duration::from_secs(5));
        assert_eq!(frames.len(), 1);
        assert!(frames[0].meta.is_none());
        assert!(frames[0].comments.iter().any(|c| c == &comment));
        t.join().expect("producer");
    }
    #[test]
    fn start_without_header_times_out_within_bound_and_leaves_no_reader() {
        let (addr, t) = producer(move |c| {
            std::thread::sleep(Duration::from_millis(600));
            drop(c);
        });
        let mut o = options(addr);
        o.start_timeout = Duration::from_millis(200);
        let started = Instant::now();
        let err = StreamSource::connect(o).expect_err("no header");
        assert!(
            started.elapsed() < Duration::from_millis(800),
            "{:?}",
            started.elapsed()
        );
        assert!(format!("{err:?}").contains("no pcapng section"));
        assert_eq!(outstanding_stream_readers(), 0);
        t.join().expect("producer");
    }
    #[test]
    fn connect_refused_is_an_io_error() {
        let addr = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            l.local_addr().expect("addr")
        };
        assert!(matches!(
            StreamSource::connect(options(addr)),
            Err(CaptureError::Io(_))
        ));
    }
    #[test]
    fn full_queue_backpressures_producer_without_loss() {
        // Far beyond queue cap (8 MiB) plus loopback socket buffers.
        const N: usize = 1000;
        let payload = vec![0x5a; 60 * 1024];
        let written = Arc::new(AtomicUsize::new(0));
        let w = written.clone();
        let (addr, t) = producer(move |mut c| {
            use std::io::Write;
            c.write_all(&header()).expect("header");
            for i in 0..N {
                c.write_all(&epb(Some(i as u64 + 1), &payload)).expect("block");
                w.store(i + 1, Ordering::Release);
            }
        });
        let mut s = StreamSource::connect(options(addr)).expect("connect");
        std::thread::sleep(Duration::from_millis(400));
        // Nobody consumed: queue bytes stay capped and the producer is held back by TCP.
        assert!(s.link.queued_bytes.load(Ordering::Acquire) <= MAX_STREAM_QUEUE_BYTES);
        assert!(
            written.load(Ordering::Acquire) < N,
            "producer must be back-pressured"
        );
        let (frames, err) = drain(&mut s, Duration::from_secs(20));
        assert!(err.is_none(), "{err:?}");
        assert_eq!(frames.len(), N);
        assert_eq!(s.producer_seq_missing(), Some(0));
        t.join().expect("producer");
    }
}
