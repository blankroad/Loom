//! Audit log `audit/audit-YYYY-MM.jsonl` with a hash chain (contract 04, plan §10.8).
//!
//! Each line is a JSON object with `prev` (previous `h`) and `h = sha256(prev ‖ canonical_json(record
//! without h))`. Canonical JSON = `serde_json::Value` serialization, whose object map is sorted
//! (`BTreeMap`; the workspace must not enable `serde_json/preserve_order` — a test pins this).
//!
//! One writer thread owns the file. Intents of R2+ are `sync_data`'d before the call returns;
//! everything else is grouped (sync at most every 100 ms). The queue holds 1024 records / 1 MiB;
//! when full a caller waits ≤ 200 ms and then fails. Any failure marks the audit **failed**, which
//! the gateway reports as `record.health = failed` (agent/script R1+ writes are then refused).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use tokio::sync::oneshot;

/// Queue capacity (records).
pub const QUEUE_RECORDS: usize = 1024;
/// Queue capacity (bytes).
pub const QUEUE_BYTES: usize = 1024 * 1024;
/// Longest a caller waits for queue space.
pub const QUEUE_WAIT: Duration = Duration::from_millis(200);
/// Group sync interval.
pub const GROUP_SYNC: Duration = Duration::from_millis(100);

/// Audit failure (`E_RECORDING_FAILED` for gated writers).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("audit log unavailable: {0}")]
pub struct AuditError(pub String);

/// Canonical JSON bytes of a value (sorted object keys, no whitespace).
pub fn canonical_json(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_default()
}

/// `h` of a record (`rec` must not contain `h`).
pub fn chain_hash(prev: &str, rec: &Value) -> String {
    let mut s = Sha256::new();
    s.update(prev.as_bytes());
    s.update(canonical_json(rec));
    hex::encode(s.finalize())
}

/// Civil (year, month) of a unix time (UTC).
fn year_month(unix_secs: u64) -> (i64, u32) {
    // Howard Hinnant's days-from-civil inverse.
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, u32::try_from(m).unwrap_or(1))
}

fn now_wall() -> (u64, u64) {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (d.as_secs(), u64::try_from(d.as_nanos()).unwrap_or(0))
}

/// File name for a unix time.
pub fn file_name(unix_secs: u64) -> String {
    let (y, m) = year_month(unix_secs);
    format!("audit-{y:04}-{m:02}.jsonl")
}

/// Last `(h, seq)` of the newest audit file in `dir` (chain continues across months/restarts).
fn chain_tail(dir: &Path) -> io::Result<(String, u64)> {
    let mut files: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("audit-"))
            })
            .collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    files.sort();
    let Some(last) = files.last() else {
        return Ok((String::new(), 0));
    };
    let mut tail = None;
    for line in BufReader::new(File::open(last)?).lines() {
        let line = line?;
        if !line.trim().is_empty() {
            tail = Some(line);
        }
    }
    let Some(line) = tail else {
        return Ok((String::new(), 0));
    };
    let v: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
    Ok((
        v["h"].as_str().unwrap_or_default().to_owned(),
        v["seq"].as_u64().unwrap_or(0),
    ))
}

/// Chain verification failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("audit chain broken at line {line}: {reason}")]
pub struct ChainError {
    /// 1-based line.
    pub line: usize,
    /// Why.
    pub reason: String,
}

/// Verify one file given the previous `h` (empty for the first file). Returns `(last h, lines)`.
pub fn verify_file(path: &Path, prev: &str) -> Result<(String, usize), ChainError> {
    let f = File::open(path).map_err(|e| ChainError {
        line: 0,
        reason: e.to_string(),
    })?;
    let mut prev = prev.to_owned();
    let mut n = 0;
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| ChainError {
            line: i + 1,
            reason: e.to_string(),
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let err = |reason: &str| ChainError {
            line: i + 1,
            reason: reason.into(),
        };
        let mut v: Value = serde_json::from_str(&line).map_err(|e| err(&e.to_string()))?;
        let obj = v.as_object_mut().ok_or_else(|| err("not an object"))?;
        let h = obj
            .remove("h")
            .and_then(|h| h.as_str().map(str::to_owned))
            .ok_or_else(|| err("missing h"))?;
        if obj.get("prev").and_then(Value::as_str) != Some(prev.as_str()) {
            return Err(err("prev does not match previous h"));
        }
        if chain_hash(&prev, &v) != h {
            return Err(err("h mismatch (record modified)"));
        }
        prev = h;
        n += 1;
    }
    Ok((prev, n))
}

struct Item {
    rec: Map<String, Value>,
    bytes: usize,
    sync: bool,
    ack: Option<oneshot::Sender<Result<(), AuditError>>>,
}

#[derive(Debug, Default)]
struct Shared {
    failed: AtomicBool,
    queued_bytes: AtomicUsize,
}

/// Handle to the audit writer.
#[derive(Debug, Clone)]
pub struct Audit {
    tx: SyncSender<Item>,
    shared: Arc<Shared>,
    dir: PathBuf,
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("bytes", &self.bytes)
            .field("sync", &self.sync)
            .finish_non_exhaustive()
    }
}

impl Audit {
    /// Start the writer for `dir`. Never fails: an unusable directory starts in the failed state.
    pub fn start(dir: PathBuf) -> Audit {
        let (tx, rx) = sync_channel(QUEUE_RECORDS);
        let shared = Arc::new(Shared::default());
        let (prev, seq) = match chain_tail(&dir) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("audit: cannot read chain tail: {e}");
                shared.failed.store(true, Ordering::Release);
                (String::new(), 0)
            }
        };
        let w = Writer {
            dir: dir.clone(),
            file: None,
            month: String::new(),
            prev,
            seq,
            shared: shared.clone(),
            dirty: false,
            last_sync: Instant::now(),
        };
        if let Err(e) = std::thread::Builder::new()
            .name("loom-audit".into())
            .spawn(move || w.run(rx))
        {
            tracing::error!("audit: writer thread: {e}");
            shared.failed.store(true, Ordering::Release);
        }
        Audit { tx, shared, dir }
    }

    /// Audit directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The audit log failed (recording health).
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    async fn enqueue(
        &self,
        rec: Map<String, Value>,
        sync: bool,
        ack: Option<oneshot::Sender<Result<(), AuditError>>>,
    ) -> Result<(), AuditError> {
        let bytes = canonical_json(&Value::Object(rec.clone())).len() + 160;
        let deadline = Instant::now() + QUEUE_WAIT;
        let mut item = Item {
            rec,
            bytes,
            sync,
            ack,
        };
        loop {
            let over = self.shared.queued_bytes.load(Ordering::Acquire) + bytes > QUEUE_BYTES;
            if !over {
                self.shared.queued_bytes.fetch_add(bytes, Ordering::AcqRel);
                match self.tx.try_send(item) {
                    Ok(()) => return Ok(()),
                    Err(TrySendError::Full(i)) => {
                        self.shared.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                        item = i;
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        self.shared.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
                        self.shared.failed.store(true, Ordering::Release);
                        return Err(AuditError("writer stopped".into()));
                    }
                }
            }
            if Instant::now() >= deadline {
                self.shared.failed.store(true, Ordering::Release);
                return Err(AuditError("audit queue full for 200 ms".into()));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// Append and wait until written (and `sync_data`'d when `sync`).
    pub async fn append(&self, kind: &str, fields: Value, sync: bool) -> Result<(), AuditError> {
        if self.failed() {
            return Err(AuditError("audit log failed earlier".into()));
        }
        let (tx, rx) = oneshot::channel();
        self.enqueue(record(kind, fields), sync, Some(tx)).await?;
        rx.await
            .unwrap_or_else(|_| Err(AuditError("writer stopped".into())))
    }

    /// Append without waiting (results, input batches). Failures still mark the log failed.
    pub async fn append_nowait(&self, kind: &str, fields: Value) {
        if self.failed() {
            return;
        }
        let _ = self.enqueue(record(kind, fields), false, None).await;
    }
}

fn record(kind: &str, fields: Value) -> Map<String, Value> {
    let mut m = match fields {
        Value::Object(m) => m,
        Value::Null => Map::new(),
        other => {
            let mut m = Map::new();
            m.insert("data".into(), other);
            m
        }
    };
    m.insert("kind".into(), Value::String(kind.into()));
    m
}

struct Writer {
    dir: PathBuf,
    file: Option<File>,
    month: String,
    prev: String,
    seq: u64,
    shared: Arc<Shared>,
    dirty: bool,
    last_sync: Instant,
}

impl Writer {
    fn run(mut self, rx: Receiver<Item>) {
        loop {
            let item = if self.dirty {
                match rx.recv_timeout(GROUP_SYNC.saturating_sub(self.last_sync.elapsed())) {
                    Ok(i) => Some(i),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(i) => Some(i),
                    Err(_) => break,
                }
            };
            if let Some(item) = item {
                self.shared.queued_bytes.fetch_sub(
                    item.bytes.min(self.shared.queued_bytes.load(Ordering::Acquire)),
                    Ordering::AcqRel,
                );
                let r = self.write(item.rec, item.sync);
                if let Err(e) = &r {
                    tracing::error!("audit write failed: {}", e.0);
                    self.shared.failed.store(true, Ordering::Release);
                }
                if let Some(a) = item.ack {
                    let _ = a.send(r);
                }
            }
            if self.dirty && self.last_sync.elapsed() >= GROUP_SYNC {
                self.sync();
            }
        }
        self.sync();
    }

    fn sync(&mut self) {
        if let Some(f) = &self.file
            && self.dirty
            && let Err(e) = f.sync_data()
        {
            tracing::error!("audit sync failed: {e}");
            self.shared.failed.store(true, Ordering::Release);
        }
        self.dirty = false;
        self.last_sync = Instant::now();
    }

    fn write(&mut self, mut rec: Map<String, Value>, sync: bool) -> Result<(), AuditError> {
        if self.shared.failed.load(Ordering::Acquire) {
            return Err(AuditError("audit log failed earlier".into()));
        }
        let (secs, wall_ns) = now_wall();
        let name = file_name(secs);
        if self.file.is_none() || self.month != name {
            self.sync();
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(&name))
                .map_err(|e| AuditError(format!("open {name}: {e}")))?;
            self.file = Some(f);
            self.month = name;
        }
        self.seq += 1;
        rec.insert("seq".into(), Value::from(self.seq));
        rec.insert("wall_ns".into(), Value::String(wall_ns.to_string()));
        rec.insert("prev".into(), Value::String(self.prev.clone()));
        let v = Value::Object(rec);
        let h = chain_hash(&self.prev, &v);
        let Value::Object(mut rec) = v else {
            return Err(AuditError("internal".into()));
        };
        rec.insert("h".into(), Value::String(h.clone()));
        let mut line = canonical_json(&Value::Object(rec));
        line.push(b'\n');
        let f = self.file.as_mut().ok_or_else(|| AuditError("no file".into()))?;
        f.write_all(&line)
            .map_err(|e| AuditError(format!("write: {e}")))?;
        self.prev = h;
        self.dirty = true;
        if sync {
            f.sync_data().map_err(|e| AuditError(format!("sync: {e}")))?;
            self.dirty = false;
            self.last_sync = Instant::now();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn canonical_json_sorts_keys() {
        // Pins the canonical form: serde_json's default map is sorted. If someone enables
        // `preserve_order`, this fails and the hash chain must get its own canonicalizer.
        let v: Value = serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":3}}"#).unwrap();
        assert_eq!(
            String::from_utf8(canonical_json(&v)).unwrap(),
            r#"{"a":{"c":3,"d":2},"b":1}"#
        );
    }

    #[test]
    fn month_names() {
        assert_eq!(file_name(0), "audit-1970-01.jsonl");
        assert_eq!(file_name(1_762_819_200), "audit-2025-11.jsonl"); // 2025-11-11
        assert_eq!(file_name(951_782_400), "audit-2000-02.jsonl"); // 2000-02-29
    }
}
