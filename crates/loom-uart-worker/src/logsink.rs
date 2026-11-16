//! `logsink` — opt-in native UART log files written by the worker (operator-upgrade U1-A, batch B4).
//!
//! Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B4.1. Provenance (PLAN v3): records are written in the worker
//! *before* hub publication, so a file carries the source identity `(producer_id, producer_epoch, source_seq)`, the generation,
//! `rx_offset` (RX), request/client ids (TX), host clocks and whether the journal emission of that chunk succeeded (`emit`) —
//! **never** a stream seq (the worker cannot know it). Every file header says so.
//!
//! Bounds: one writer thread per sink; a byte-accounted queue of [`LOG_QUEUE_BYTES`] (overflow is counted and written as a gap
//! line, never blocks the device path); files rotate at `rotate_bytes`; the channel directory's total (existing files
//! included) stops logging at `quota_bytes` — nothing is ever deleted.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Queued record bytes at most; more are dropped and counted.
pub const LOG_QUEUE_BYTES: usize = 4 * 1024 * 1024;
/// Default / bounds of `rotate_mib`.
pub const LOG_ROTATE_DEFAULT_MIB: u64 = 64;
/// Smallest `rotate_mib`.
pub const LOG_ROTATE_MIN_MIB: u64 = 1;
/// Largest `rotate_mib`.
pub const LOG_ROTATE_MAX_MIB: u64 = 1024;
/// Default `quota_mib` (per channel directory).
pub const LOG_QUOTA_DEFAULT_MIB: u64 = 1024;
/// Largest `quota_mib`.
pub const LOG_QUOTA_MAX_MIB: u64 = 8192;
/// Bytes kept free for the footer: a record or a new file is written only if `size + text + LOG_FOOTER_RESERVE` stays
/// within the rotate size (per file) and the quota (directory), so the footer always fits.
pub const LOG_FOOTER_RESERVE: u64 = 256;
/// A `.log` line without a newline is written in pieces of this many characters.
pub const LOG_LINE_MAX_CHARS: usize = 64 * 1024;
/// Sub-directory of `$LOOM_HOME` that holds the logs (`<home>/uart/logs/<channel>/`).
pub const LOG_SUBDIR: [&str; 2] = ["uart", "logs"];

/// File format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Readable lines (one per device line).
    Log,
    /// One JSON object per chunk (exact bytes as hex).
    Jsonl,
}

/// Sink settings (validated by the caller).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogCfg {
    /// Format.
    pub format: LogFormat,
    /// Rotate when a file would exceed this many bytes.
    pub rotate_bytes: u64,
    /// Stop when the channel directory would exceed this many bytes.
    pub quota_bytes: u64,
}

/// Direction of a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogDir {
    /// Device → host.
    Rx,
    /// Host → device.
    Tx,
}

/// One chunk to log (copied from the reader / TX actor chunk and its emission result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub dir: LogDir,
    /// `device` | `user` | `agent` | `script`.
    pub origin: String,
    pub generation: u32,
    /// RX only.
    pub rx_offset: Option<u64>,
    /// TX only (agent writes).
    pub request_id: Option<String>,
    /// TX only.
    pub client_id: Option<String>,
    pub producer_id: String,
    pub producer_epoch: u64,
    pub source_seq: u64,
    pub mono_ns: u64,
    pub wall_ns: u64,
    pub clock_id: String,
    /// Real bytes; never written when `secret` (length only).
    pub bytes: Vec<u8>,
    pub secret: bool,
    /// The journal emission of this chunk succeeded (`false` = the journal may lack it).
    pub emit_ok: bool,
}

/// Fixed facts written into every file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogHeader {
    pub channel: String,
    pub target: String,
    pub producer_id: String,
    pub producer_epoch: u64,
    pub generation: u32,
    pub clock_id: String,
}

/// Live counters (status / `uart.log` events).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogStats {
    /// Still accepting records.
    pub active: bool,
    pub dir: PathBuf,
    /// File being written (last file after stop).
    pub file: Option<PathBuf>,
    /// Files this sink created.
    pub files: u32,
    /// Bytes this sink wrote (all files).
    pub bytes_written: u64,
    /// Bytes in the channel directory (existing files + written).
    pub dir_bytes: u64,
    pub dropped_records: u64,
    pub dropped_bytes: u64,
    /// Why it stopped: `user` | `quota` | `closed` | `io_error: …` (`None` while active).
    pub stopped: Option<String>,
}

/// Why a sink could not start.
#[derive(Debug, thiserror::Error)]
pub enum LogStartError {
    /// The directory already holds `dir_bytes ≥ quota_bytes`.
    #[error("log quota already reached ({dir_bytes} of {quota_bytes} bytes in the channel log directory)")]
    Quota { dir_bytes: u64, quota_bytes: u64 },
    /// Creating the directory or the first file failed.
    #[error("log directory error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
enum Msg {
    Record(LogRecord),
    Stop(String),
}

/// Shared between the handle and the writer thread.
#[derive(Debug, Default)]
struct Shared {
    stats: Mutex<LogStats>,
    /// bytes queued and not yet written
    queued: Mutex<usize>,
    /// dropped since the last gap line (records, bytes)
    pending_gap: Mutex<(u64, u64)>,
    /// set by the writer when it stops by itself; taken once by `take_stop_notice`
    stop_notice: Mutex<Option<String>>,
}

/// A running sink: [`LogSink::record`] never blocks; [`LogSink::stop`] flushes and joins.
#[derive(Debug)]
pub struct LogSink {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl LogSink {
    /// Create `dir` (and parents), sum the existing regular files into `dir_bytes`, refuse when that is `≥ quota_bytes`,
    /// open file 1 with its header and spawn the writer thread `loom-uart-log`. See FUNCTIONS.md §B4.1 `start`.
    pub fn start(
        dir: &Path,
        header: LogHeader,
        cfg: LogCfg,
        started_wall_ns: u64,
    ) -> Result<LogSink, LogStartError> {
        std::fs::create_dir_all(dir)?;
        let mut dir_bytes = 0u64;
        for e in std::fs::read_dir(dir)?.flatten() {
            if let Ok(m) = e.metadata()
                && m.is_file()
            {
                dir_bytes += m.len();
            }
        }
        let head_text = header_text(&header, &cfg, 1, started_wall_ns);
        let head_len = head_text.len() as u64;
        if dir_bytes >= cfg.quota_bytes || dir_bytes + head_len + LOG_FOOTER_RESERVE > cfg.quota_bytes {
            return Err(LogStartError::Quota {
                dir_bytes,
                quota_bytes: cfg.quota_bytes,
            });
        }
        let path = dir.join(file_name(
            &header.channel,
            started_wall_ns,
            header.generation,
            1,
            cfg.format,
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut out = std::io::BufWriter::new(file);
        io::Write::write_all(&mut out, head_text.as_bytes())?;
        io::Write::flush(&mut out)?;
        let shared = Arc::new(Shared::default());
        *shared.stats.lock().unwrap_or_else(|p| p.into_inner()) = LogStats {
            active: true,
            dir: dir.to_path_buf(),
            file: Some(path.clone()),
            files: 1,
            bytes_written: head_len,
            dir_bytes: dir_bytes + head_len,
            ..LogStats::default()
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let st = WriterState {
            out,
            dir: dir.to_path_buf(),
            header,
            cfg,
            started_ns: started_wall_ns,
            n: 1,
            file_bytes: head_len,
            header_len: head_len,
            dir_bytes: dir_bytes + head_len,
            bytes_written: head_len,
            files: 1,
            records: 0,
            done: false,
            asm: [LineAssembler::default(), LineAssembler::default()],
        };
        let sh = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("loom-uart-log".into())
            .spawn(move || writer_loop(rx, sh, st))?;
        Ok(LogSink {
            tx,
            shared,
            thread: Some(thread),
        })
    }

    /// Queue one chunk, or count it dropped when the queue is over [`LOG_QUEUE_BYTES`] or the sink stopped. §B4.1.
    pub fn record(&self, r: LogRecord) {
        if !self.shared.stats.lock().unwrap_or_else(|p| p.into_inner()).active {
            return;
        }
        let cost = r.bytes.len();
        {
            let mut q = self.shared.queued.lock().unwrap_or_else(|p| p.into_inner());
            if *q + cost > LOG_QUEUE_BYTES {
                drop(q);
                let mut st = self.shared.stats.lock().unwrap_or_else(|p| p.into_inner());
                st.dropped_records += 1;
                st.dropped_bytes += cost as u64;
                drop(st);
                let mut g = self.shared.pending_gap.lock().unwrap_or_else(|p| p.into_inner());
                g.0 += 1;
                g.1 += cost as u64;
                return;
            }
            *q += cost;
        }
        if self.tx.send(Msg::Record(r)).is_err() {
            let mut q = self.shared.queued.lock().unwrap_or_else(|p| p.into_inner());
            *q = q.saturating_sub(cost);
        }
    }

    /// The stop reason, once, when the sink stopped by itself (`quota`, `io_error: …`); `None` otherwise / afterwards.
    /// The worker turns it into one `uart.log` event. §B4.1.
    pub fn take_stop_notice(&self) -> Option<String> {
        self.shared
            .stop_notice
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }

    /// Snapshot of the counters. §B4.1.
    pub fn stats(&self) -> LogStats {
        self.shared
            .stats
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Write what is queued, the footer (`reason`), close, join; returns the final counters. §B4.1.
    pub fn stop(self, reason: &str) -> LogStats {
        let mut this = self;
        if let Some(t) = this.thread.take() {
            let _ = this.tx.send(Msg::Stop(reason.to_owned()));
            let _ = t.join();
        }
        let mut st = this.shared.stats.lock().unwrap_or_else(|p| p.into_inner());
        st.active = false;
        if st.stopped.is_none() {
            st.stopped = Some(reason.to_owned());
        }
        st.clone()
    }
}

impl Drop for LogSink {
    /// Dropping without `stop` stops with reason `closed`. §B4.1.
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = self.tx.send(Msg::Stop("closed".into()));
            let _ = t.join();
        }
    }
}

/// Everything the writer thread owns (private state carrier for the three writer helpers below).
struct WriterState {
    out: std::io::BufWriter<std::fs::File>,
    dir: PathBuf,
    header: LogHeader,
    cfg: LogCfg,
    started_ns: u64,
    n: u32,
    file_bytes: u64,
    header_len: u64,
    dir_bytes: u64,
    bytes_written: u64,
    files: u32,
    records: u64,
    /// stopped by itself (quota / io error) or finalized: later records are ignored
    done: bool,
    asm: [LineAssembler; 2],
}

/// Writer thread body: drains the queue in order, writes with the rotation / quota limits, flushes when idle.
fn writer_loop(rx: std::sync::mpsc::Receiver<Msg>, shared: Arc<Shared>, mut st: WriterState) {
    use std::io::Write as _;
    use std::sync::mpsc::RecvTimeoutError;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(Msg::Record(r)) => {
                {
                    let mut q = shared.queued.lock().unwrap_or_else(|p| p.into_inner());
                    *q = q.saturating_sub(r.bytes.len());
                }
                if st.done {
                    continue;
                }
                let gap = std::mem::take(&mut *shared.pending_gap.lock().unwrap_or_else(|p| p.into_inner()));
                if gap != (0, 0) {
                    let text = gap_text(st.cfg.format, gap.0, gap.1, r.wall_ns);
                    if !write_limited(&mut st, &shared, &text) {
                        continue;
                    }
                }
                st.records += 1;
                let text = match st.cfg.format {
                    LogFormat::Log => st.asm[usize::from(r.dir == LogDir::Tx)].feed(&r),
                    LogFormat::Jsonl => jsonl_line(&r),
                };
                if !text.is_empty() {
                    write_limited(&mut st, &shared, &text);
                }
            }
            Ok(Msg::Stop(reason)) => {
                if !st.done {
                    if st.cfg.format == LogFormat::Log {
                        for i in 0..2 {
                            let tail = st.asm[i].flush();
                            if !tail.is_empty() && !write_limited(&mut st, &shared, &tail) {
                                break;
                            }
                        }
                    }
                    finish_writer(&mut st, &shared, &reason, false);
                }
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                if !st.done {
                    let _ = st.out.flush();
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !st.done {
                    finish_writer(&mut st, &shared, "closed", false);
                }
                return;
            }
        }
    }
}

/// Write `text` honoring rotation and quota; `false` = the sink stopped itself (quota / io error) instead.
fn write_limited(st: &mut WriterState, shared: &Shared, text: &str) -> bool {
    use std::io::Write as _;
    if st.done {
        return false;
    }
    let len = text.len() as u64;
    if st.file_bytes + len + LOG_FOOTER_RESERVE > st.cfg.rotate_bytes && st.file_bytes > st.header_len {
        let n = st.n + 1;
        let hdr = header_text(&st.header, &st.cfg, n, st.started_ns);
        let hlen = hdr.len() as u64;
        if st.dir_bytes + hlen + LOG_FOOTER_RESERVE > st.cfg.quota_bytes {
            finish_writer(st, shared, "quota", true);
            return false;
        }
        let path = st.dir.join(file_name(
            &st.header.channel,
            st.started_ns,
            st.header.generation,
            n,
            st.cfg.format,
        ));
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        let file = match opened {
            Ok(f) => f,
            Err(e) => {
                finish_writer(st, shared, &format!("io_error: {e}"), true);
                return false;
            }
        };
        let _ = st.out.flush();
        st.out = std::io::BufWriter::new(file);
        if let Err(e) = st.out.write_all(hdr.as_bytes()) {
            finish_writer(st, shared, &format!("io_error: {e}"), true);
            return false;
        }
        st.n = n;
        st.files += 1;
        st.file_bytes = hlen;
        st.header_len = hlen;
        st.dir_bytes += hlen;
        st.bytes_written += hlen;
        let mut s = shared.stats.lock().unwrap_or_else(|p| p.into_inner());
        s.file = Some(path);
        s.files = st.files;
        s.bytes_written = st.bytes_written;
        s.dir_bytes = st.dir_bytes;
    }
    if st.dir_bytes + len + LOG_FOOTER_RESERVE > st.cfg.quota_bytes {
        finish_writer(st, shared, "quota", true);
        return false;
    }
    if let Err(e) = st.out.write_all(text.as_bytes()) {
        finish_writer(st, shared, &format!("io_error: {e}"), true);
        return false;
    }
    st.file_bytes += len;
    st.dir_bytes += len;
    st.bytes_written += len;
    let mut s = shared.stats.lock().unwrap_or_else(|p| p.into_inner());
    s.bytes_written = st.bytes_written;
    s.dir_bytes = st.dir_bytes;
    true
}

/// Footer (when it fits), flush, close; publishes the final state. `self_stop` also leaves a one-shot notice.
fn finish_writer(st: &mut WriterState, shared: &Shared, reason: &str, self_stop: bool) {
    use std::io::Write as _;
    if st.done {
        return;
    }
    st.done = true;
    let (dr, db) = {
        let s = shared.stats.lock().unwrap_or_else(|p| p.into_inner());
        (s.dropped_records, s.dropped_bytes)
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    let footer = footer_text(st.cfg.format, reason, st.records, dr, db, now);
    let flen = footer.len() as u64;
    if st.dir_bytes + flen <= st.cfg.quota_bytes && st.out.write_all(footer.as_bytes()).is_ok() {
        st.file_bytes += flen;
        st.dir_bytes += flen;
        st.bytes_written += flen;
    }
    let _ = st.out.flush();
    let mut s = shared.stats.lock().unwrap_or_else(|p| p.into_inner());
    s.active = false;
    s.stopped = Some(reason.to_owned());
    s.bytes_written = st.bytes_written;
    s.dir_bytes = st.dir_bytes;
    s.files = st.files;
    drop(s);
    if self_stop {
        *shared.stop_notice.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason.to_owned());
    }
}

/// `<home>/uart/logs/<channel>`. §B4.1.
pub fn log_dir(home: &Path, channel: &str) -> PathBuf {
    let mut p = home.to_path_buf();
    for part in LOG_SUBDIR {
        p.push(part);
    }
    p.push(channel);
    p
}

/// `<channel>-<YYYYMMDD>T<HHMMSS>Z-g<gen>-<nnn>.<log|jsonl>` (UTC of `started_wall_ns`, `nnn` 3+ digits). §B4.1.
pub fn file_name(channel: &str, started_wall_ns: u64, generation: u32, n: u32, format: LogFormat) -> String {
    let iso = utc_iso(started_wall_ns);
    let date = iso[..10].replace('-', "");
    let time = iso[11..19].replace(':', "");
    let ext = match format {
        LogFormat::Log => "log",
        LogFormat::Jsonl => "jsonl",
    };
    format!("{channel}-{date}T{time}Z-g{generation}-{n:03}.{ext}")
}

/// UTC `YYYY-MM-DDTHH:MM:SS.mmmZ` (proleptic Gregorian, no external crate). §B4.1.
pub fn utc_iso(wall_ns: u64) -> String {
    let secs = wall_ns / 1_000_000_000;
    let ms = (wall_ns / 1_000_000) % 1000;
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX / 2);
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// The header text of file `n` (ends with `\n`). §B4.1.
pub fn header_text(h: &LogHeader, cfg: &LogCfg, n: u32, started_wall_ns: u64) -> String {
    let started = utc_iso(started_wall_ns);
    match cfg.format {
        LogFormat::Log => format!(
            "# Loom UART native log · channel {} · file {n} · started {started}\n\
             # provenance: native-worker-log; stream_seq: not available (assigned by the daemon journal after publication); cross-reference via (producer_id, producer_epoch, source_seq)\n\
             # target {} · producer_id {} · producer_epoch {} · generation {} · clock_id {}\n\
             # format log · rotate {} B · quota {} B · columns: <utc> g<gen> <RX|TX> src=<epoch>:<source_seq> off=<rx_offset|-> emit=<ok|FAILED> | text (\\r dropped, controls as \\xNN)\n",
            h.channel,
            h.target,
            h.producer_id,
            h.producer_epoch,
            h.generation,
            h.clock_id,
            cfg.rotate_bytes,
            cfg.quota_bytes
        ),
        LogFormat::Jsonl => format!(
            "{}\n",
            serde_json::json!({
                "type": "header",
                "provenance": "native-worker-log",
                "stream_seq": "not available",
                "channel": h.channel,
                "file_index": n,
                "started": started,
                "target": h.target,
                "producer_id": h.producer_id,
                "producer_epoch": h.producer_epoch.to_string(),
                "generation": h.generation,
                "clock_id": h.clock_id,
                "format": "jsonl",
                "rotate_bytes": cfg.rotate_bytes,
                "quota_bytes": cfg.quota_bytes,
            })
        ),
    }
}

/// The gap line written before the next record after drops (ends with `\n`). §B4.1.
pub fn gap_text(format: LogFormat, records: u64, bytes: u64, wall_ns: u64) -> String {
    let t = utc_iso(wall_ns);
    match format {
        LogFormat::Log => format!(
            "# GAP {t} dropped {records} records / {bytes} bytes (log queue full; the daemon journal may still have them)\n"
        ),
        LogFormat::Jsonl => format!(
            "{}\n",
            serde_json::json!({"type": "gap", "t": t, "dropped_records": records, "dropped_bytes": bytes})
        ),
    }
}

/// The footer written on stop / rotation (ends with `\n`). §B4.1.
pub fn footer_text(
    format: LogFormat,
    reason: &str,
    records: u64,
    dropped_records: u64,
    dropped_bytes: u64,
    wall_ns: u64,
) -> String {
    let t = utc_iso(wall_ns);
    match format {
        LogFormat::Log => format!(
            "# END {t} reason={reason} records={records} dropped_records={dropped_records} dropped_bytes={dropped_bytes}\n"
        ),
        LogFormat::Jsonl => format!(
            "{}\n",
            serde_json::json!({
                "type": "end",
                "t": t,
                "reason": reason,
                "records": records,
                "dropped_records": dropped_records,
                "dropped_bytes": dropped_bytes,
            })
        ),
    }
}

/// `.jsonl` object for one record (ends with `\n`). §B4.1.
pub fn jsonl_line(r: &LogRecord) -> String {
    let mut v = serde_json::json!({
        "t": utc_iso(r.wall_ns),
        "wall_ns": r.wall_ns.to_string(),
        "mono_ns": r.mono_ns.to_string(),
        "producer_epoch": r.producer_epoch.to_string(),
        "source_seq": r.source_seq.to_string(),
        "rx_offset": r.rx_offset.map(|o| o.to_string()),
        "clock_id": r.clock_id,
        "generation": r.generation,
        "dir": if r.dir == LogDir::Rx { "rx" } else { "tx" },
        "origin": r.origin,
        "request_id": r.request_id,
        "client_id": r.client_id,
        "producer_id": r.producer_id,
        "len": r.bytes.len(),
        "emit": if r.emit_ok { "ok" } else { "failed" },
    });
    if r.secret {
        v["secret"] = serde_json::json!({"len": r.bytes.len()});
    } else {
        v["hex"] = serde_json::Value::String(r.bytes.iter().map(|b| format!("{b:02x}")).collect());
    }
    format!("{v}\n")
}

/// `.log` line assembly for one direction: device text is split into lines; each output line carries the metadata of the
/// chunk that started it. See FUNCTIONS.md §B4.1 `LineAssembler`.
#[derive(Debug, Default)]
pub struct LineAssembler {
    /// incomplete UTF-8 sequence carried to the next chunk (≤ 3 bytes)
    carry: Vec<u8>,
    /// text of the unfinished line and the prefix of the record that started it
    pending: Option<(String, String)>,
    /// prefix of the last record fed (used when only carried bytes are left at flush)
    last_prefix: String,
}

impl LineAssembler {
    /// Feed one record of this direction; returns finished lines (each ends with `\n`). §B4.1.
    pub fn feed(&mut self, r: &LogRecord) -> String {
        let prefix = line_prefix(r);
        self.last_prefix.clone_from(&prefix);
        if r.secret {
            let mut out = self.flush();
            out.push_str(&format!("{prefix}[secret {} B]\n", r.bytes.len()));
            return out;
        }
        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(&r.bytes);
        // pieces: one char, an escape (`\xNN`), or "\n" (line end); `\r` is dropped
        let mut pieces: Vec<String> = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let (valid, bad, incomplete) = match std::str::from_utf8(&data[i..]) {
                Ok(v) => (v, 0, false),
                Err(e) => {
                    let (ok, rest) = data[i..].split_at(e.valid_up_to());
                    let _ = rest;
                    (
                        std::str::from_utf8(ok).unwrap_or(""),
                        e.error_len().unwrap_or(0),
                        e.error_len().is_none(),
                    )
                }
            };
            for ch in valid.chars() {
                match ch {
                    '\r' => {}
                    '\n' => pieces.push("\n".into()),
                    '\t' => pieces.push("\t".into()),
                    c if (c as u32) < 0x20 || c as u32 == 0x7f => pieces.push(format!("\\x{:02x}", c as u32)),
                    c => pieces.push(c.to_string()),
                }
            }
            i += valid.len();
            if incomplete {
                self.carry = data[i..].to_vec();
                break;
            }
            for b in &data[i..i + bad] {
                pieces.push(format!("\\x{b:02x}"));
            }
            i += bad;
            if valid.is_empty() && bad == 0 {
                break;
            }
        }
        let mut out = String::new();
        let mut count = self.pending.as_ref().map_or(0, |p| p.0.chars().count());
        for piece in pieces {
            if piece == "\n" {
                let (text, pfx) = self
                    .pending
                    .take()
                    .unwrap_or_else(|| (String::new(), prefix.clone()));
                out.push_str(&pfx);
                out.push_str(&text);
                out.push('\n');
                count = 0;
                continue;
            }
            let p = self
                .pending
                .get_or_insert_with(|| (String::new(), prefix.clone()));
            p.0.push_str(&piece);
            count += piece.chars().count();
            if count >= LOG_LINE_MAX_CHARS
                && let Some((text, pfx)) = self.pending.take()
            {
                out.push_str(&pfx);
                out.push_str(&text);
                out.push_str(" ⟨continued⟩\n");
                count = 0;
            }
        }
        out
    }

    /// The unfinished line (if any) with ` ⟨no newline⟩`, for stop / rotation. §B4.1.
    pub fn flush(&mut self) -> String {
        let carry = std::mem::take(&mut self.carry);
        let mut tail: String = carry.iter().map(|b| format!("\\x{b:02x}")).collect();
        match self.pending.take() {
            Some((mut text, pfx)) => {
                text.push_str(&tail);
                format!("{pfx}{text} ⟨no newline⟩\n")
            }
            None if !tail.is_empty() => {
                tail.insert_str(0, &self.last_prefix);
                format!("{tail} ⟨no newline⟩\n")
            }
            None => String::new(),
        }
    }
}

/// The per-line prefix `<utc_iso> g<gen> <RX|TX> src=<producer_epoch>:<source_seq> off=<rx_offset|-> emit=<ok|FAILED> | `. §B4.1.
pub fn line_prefix(r: &LogRecord) -> String {
    format!(
        "{} g{} {} src={}:{} off={} emit={} | ",
        utc_iso(r.wall_ns),
        r.generation,
        if r.dir == LogDir::Rx { "RX" } else { "TX" },
        r.producer_epoch,
        r.source_seq,
        r.rx_offset.map_or_else(|| "-".to_owned(), |o| o.to_string()),
        if r.emit_ok { "ok" } else { "FAILED" },
    )
}
