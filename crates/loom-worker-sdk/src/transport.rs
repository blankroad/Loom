//! `loom-worker/1` transport (contract 05 §5.1–5.2): two named local connections per worker
//! (`ctl`, `data`) and async frame I/O with length checks before allocation and read deadlines.
//!
//! * Unix (Linux, macOS): Unix domain sockets `<sock_dir>/<instance>.{ctl,data}.sock`, directory
//!   mode 0700. Default `sock_dir` = `$LOOM_HOME/run/w` (`LOOM_HOME` defaults to `~/.loom`).
//! * Windows: named pipes `\\.\pipe\loom-<user hash 8>-<instance>-{ctl,data}`; the server rejects
//!   remote clients. (A DACL restricted to the current user needs raw security attributes, which
//!   are `unsafe` in tokio — deferred to P2, see the P1 report.)
//!
//! The gateway (`loom-worker-host`) listens; the worker connects.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use loom_proto::worker::{FrameHeader, HEADER_LEN, Lane, WorkerMsg};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::codec::{CodecError, decode_frame, encode_frame};

/// Read half of a lane.
pub type BoxRead = Box<dyn AsyncRead + Send + Unpin>;
/// Write half of a lane.
pub type BoxWrite = Box<dyn AsyncWrite + Send + Unpin>;

/// Longest Unix socket path we accept (macOS `sun_path` is 104 bytes incl. NUL).
pub const MAX_SOCK_PATH: usize = 100;

/// Transport / framing failure. Every variant except `Eof` is a connection error: the caller
/// closes that connection.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// Socket error.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// Bad header, bad payload, oversize, type mismatch.
    #[error("frame: {0}")]
    Codec(#[from] CodecError),
    /// Peer closed in the middle of a frame.
    #[error("truncated frame")]
    Truncated,
    /// No complete frame before the read deadline.
    #[error("read deadline expired")]
    Deadline,
    /// Message sent on the wrong lane.
    #[error("message {0} on the wrong lane")]
    WrongLane(&'static str),
}

/// `$LOOM_HOME` (default `~/.loom`).
pub fn loom_home() -> PathBuf {
    if let Some(h) = std::env::var_os("LOOM_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".loom")
}

/// Default socket directory `$LOOM_HOME/run/w`.
pub fn default_sock_dir() -> PathBuf {
    loom_home().join("run").join("w")
}

fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Ctl => "ctl",
        Lane::Data => "data",
    }
}

/// Validate an instance name used in socket/pipe names.
pub fn validate_instance(instance: &str) -> Result<(), String> {
    let ok = !instance.is_empty()
        && instance.len() <= 64
        && instance
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !instance.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(format!("invalid worker instance name {instance:?}"))
    }
}

/// Unix socket path of a lane.
pub fn socket_path(sock_dir: &Path, instance: &str, lane: Lane) -> PathBuf {
    sock_dir.join(format!("{instance}.{}.sock", lane_name(lane)))
}

/// 8 hex chars identifying the current OS user (FNV-1a of the user name).
pub fn user_hash8() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let mut h: u32 = 0x811c_9dc5;
    for b in user.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// Windows pipe name of a lane.
pub fn pipe_name(instance: &str, lane: Lane) -> String {
    format!(r"\\.\pipe\loom-{}-{instance}-{}", user_hash8(), lane_name(lane))
}

/// Connect one lane (worker side), retrying until `timeout` (the gateway binds before it spawns
/// the worker, so normally the first attempt succeeds).
pub async fn connect_lane(
    sock_dir: &Path,
    instance: &str,
    lane: Lane,
    timeout: Duration,
) -> io::Result<(BoxRead, BoxWrite)> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match connect_once(sock_dir, instance, lane).await {
            Ok(v) => return Ok(v),
            Err(e) if tokio::time::Instant::now() < deadline => {
                let _ = e;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
async fn connect_once(sock_dir: &Path, instance: &str, lane: Lane) -> io::Result<(BoxRead, BoxWrite)> {
    let path = socket_path(sock_dir, instance, lane);
    let s = tokio::net::UnixStream::connect(&path).await?;
    let (r, w) = s.into_split();
    Ok((Box::new(r), Box::new(w)))
}

#[cfg(windows)]
async fn connect_once(_sock_dir: &Path, instance: &str, lane: Lane) -> io::Result<(BoxRead, BoxWrite)> {
    let c = tokio::net::windows::named_pipe::ClientOptions::new().open(pipe_name(instance, lane))?;
    let (r, w) = tokio::io::split(c);
    Ok((Box::new(r), Box::new(w)))
}

/// Reads frames from one lane.
pub struct FrameReader {
    inner: BoxRead,
    buf: Vec<u8>,
}

impl std::fmt::Debug for FrameReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameReader").finish_non_exhaustive()
    }
}

impl FrameReader {
    /// Wrap a read half.
    pub fn new(inner: BoxRead) -> Self {
        FrameReader {
            inner,
            buf: Vec::with_capacity(HEADER_LEN + 4096),
        }
    }

    /// Read one frame. `Ok(None)` = clean EOF at a frame boundary. The header is validated (size
    /// ≤ 64 KiB, known type, zero flags) **before** the payload buffer grows. `deadline` bounds
    /// the whole frame once its first byte arrived (`None` = wait forever for the first byte).
    pub async fn read(
        &mut self,
        deadline: Option<Duration>,
    ) -> Result<Option<(WorkerMsg, usize)>, WireError> {
        let mut hdr = [0u8; HEADER_LEN];
        // First byte: may wait (idle connection); then the rest is bounded by the deadline.
        let n = self.inner.read(&mut hdr[..1]).await?;
        if n == 0 {
            return Ok(None);
        }
        let body = async {
            self.inner.read_exact(&mut hdr[1..]).await?;
            let h = FrameHeader::decode(&hdr).map_err(CodecError::from)?;
            let len = h.len as usize;
            self.buf.clear();
            self.buf.extend_from_slice(&hdr);
            self.buf.resize(HEADER_LEN + len, 0);
            self.inner.read_exact(&mut self.buf[HEADER_LEN..]).await?;
            let (msg, used) = decode_frame(&self.buf)?;
            Ok::<_, WireError>((msg, used))
        };
        let res = match deadline {
            Some(d) => tokio::time::timeout(d, body)
                .await
                .map_err(|_| WireError::Deadline)?,
            None => body.await,
        };
        match res {
            Err(WireError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof => Err(WireError::Truncated),
            other => other.map(Some),
        }
    }
}

/// Writes frames to one lane.
pub struct FrameWriter {
    inner: BoxWrite,
}

impl std::fmt::Debug for FrameWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameWriter").finish_non_exhaustive()
    }
}

impl FrameWriter {
    /// Wrap a write half.
    pub fn new(inner: BoxWrite) -> Self {
        FrameWriter { inner }
    }

    /// Encode and write one message; returns the frame length. Fails (without writing) if the
    /// encoded frame exceeds 64 KiB.
    pub async fn write(&mut self, msg: &WorkerMsg) -> Result<usize, WireError> {
        let frame = encode_frame(msg)?;
        self.write_raw(&frame).await?;
        Ok(frame.len())
    }

    /// Write already-encoded bytes (tests use this to inject damaged frames).
    pub async fn write_raw(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        self.inner.write_all(bytes).await?;
        self.inner.flush().await?;
        Ok(())
    }

    /// Shut the write side down.
    pub async fn shutdown(&mut self) -> Result<(), WireError> {
        self.inner.shutdown().await?;
        Ok(())
    }
}

/// Lane check: `msg` must travel on `lane`.
pub fn check_lane(msg: &WorkerMsg, lane: Lane) -> Result<(), WireError> {
    if msg.lane() == lane {
        Ok(())
    } else {
        Err(WireError::WrongLane(msg_name(msg)))
    }
}

/// Short name of a message (logs, errors).
pub fn msg_name(msg: &WorkerMsg) -> &'static str {
    match msg {
        WorkerMsg::Hello(_) => "hello",
        WorkerMsg::Welcome(_) => "welcome",
        WorkerMsg::Reject(_) => "reject",
        WorkerMsg::Call(_) => "call",
        WorkerMsg::Write(_) => "write",
        WorkerMsg::JobStart(_) => "job_start",
        WorkerMsg::JobUpdate(_) => "job_update",
        WorkerMsg::JobRenew(_) => "job_renew",
        WorkerMsg::JobStop(_) => "job_stop",
        WorkerMsg::InputLease(_) => "input_lease",
        WorkerMsg::InputLeaseEnd(_) => "input_lease_end",
        WorkerMsg::Input(_) => "input",
        WorkerMsg::Latch(_) => "latch",
        WorkerMsg::Credit(_) => "credit",
        WorkerMsg::Reply(_) => "reply",
        WorkerMsg::PriorityEvents(_) => "priority_events",
        WorkerMsg::Health(_) => "health",
        WorkerMsg::LatchAck(_) => "latch_ack",
        WorkerMsg::Segment(_) => "segment",
        WorkerMsg::Events(_) => "events",
    }
}

/// Reply 대상 프레임은 명시적 id 유무와 무관하게 소켓 순서 번호도 증가시킨다.
/// v1.0과 id 없는 Job/Input/InputLease는 그 번호를 응답 식별자로 사용한다.
pub fn is_request(msg: &WorkerMsg) -> bool {
    matches!(
        msg,
        WorkerMsg::Call(_)
            | WorkerMsg::Write(_)
            | WorkerMsg::JobStart(_)
            | WorkerMsg::JobUpdate(_)
            | WorkerMsg::JobRenew(_)
            | WorkerMsg::JobStop(_)
            | WorkerMsg::InputLease(_)
            | WorkerMsg::Input(_)
    )
}

/// v1.1 명시 식별자. minor 0으로 협상한 연결은 이 값 대신 순서 번호를 사용한다.
pub fn request_call_id(msg: &WorkerMsg) -> Option<u64> {
    match msg {
        WorkerMsg::Call(c) => Some(c.call_id.0),
        WorkerMsg::Write(w) => Some(w.call_id.0),
        WorkerMsg::JobStart(j) => j.call_id,
        WorkerMsg::JobUpdate(j) => j.call_id,
        WorkerMsg::JobRenew(j) => j.call_id,
        WorkerMsg::JobStop(j) => j.call_id,
        WorkerMsg::Input(i) => i.call_id,
        _ => None,
    }
}
