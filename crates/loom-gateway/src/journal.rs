//! Recording in `loomd` (contract 06, P1 J1) on top of `loom-journal`.
//!
//! * Session directory `records/<YYYY-MM-DD>/s<stream_epoch>/` with `manifest.json`.
//! * **What the gateway writes**: events of producer `gateway` / `gateway.<domain>` (write rows,
//!   latch, policy, marks, test markers, activity …) and the raw events of **in-process** workers
//!   (they have no process of their own). IPC workers write their own journal (`WorkerHost::
//!   journals_itself`), so the gateway does not write them.
//! * **Mapping log**: the hub's [`MappingSink`] is the `loom-journal` mapping writer. It never drops:
//!   on backpressure the hub thread waits and retries (publishing slows down — contract 06 §6.3).
//! * **Reads** beyond the hub ring go mapping → segment scan ([`Journal::read_range`]).
//! * **Retention** over all sessions (age + quota, pins kept), see [`Journal::retain`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_journal::{
    AppendError, FaultHook, GapReport, IndexPolicy, JournalConfig, JournalHandle, JournalWriter,
    MappingEntry, MappingError, MappingLog, QuotaLevel, RetainPolicy, ScanLimits, ScanQuery, ScanRange,
    SegmentId, lookup_mappings, producer_dir, recover_root, retain, scan,
};
use loom_proto::envelope::RecordHealth;
use loom_proto::event::{GapCount, GapData, PublishedRecord, SeqRange, SourceRecord};
use loom_proto::num::U64;
use loom_proto::rpc::SubFilter;
use serde_json::{Value, json};

use crate::home::{Home, write_atomic};
use crate::hub::{MappingRecord, MappingSink, lock, matches};

/// Longest the hub waits for the mapping log before giving up on one entry.
pub const MAPPING_GIVE_UP: Duration = Duration::from_secs(2);

/// Producer ids the gateway itself writes.
pub fn is_gateway_producer(id: &str) -> bool {
    id == "gateway" || id.starts_with("gateway.")
}

/// `records/<date>/s<epoch>`.
pub fn session_dir(home: &Home, epoch: u64) -> PathBuf {
    home.root()
        .join("records")
        .join(date_of(epoch))
        .join(format!("s{epoch}"))
}

/// UTC date `YYYY-MM-DD` of a unix time.
pub fn date_of(unix_secs: u64) -> String {
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// All session directories (oldest first).
pub fn sessions(home: &Home) -> Vec<(u64, PathBuf)> {
    let mut out = Vec::new();
    let Ok(days) = fs::read_dir(home.root().join("records")) else {
        return out;
    };
    for day in days.flatten() {
        let Ok(ss) = fs::read_dir(day.path()) else {
            continue;
        };
        for s in ss.flatten() {
            if let Some(e) = s
                .file_name()
                .to_str()
                .and_then(|n| n.strip_prefix('s'))
                .and_then(|n| n.parse::<u64>().ok())
            {
                out.push((e, s.path()));
            }
        }
    }
    out.sort();
    out
}

fn dir_bytes(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Open segments younger than this may still be written by an orphaned worker of the previous
/// daemon (orphan time 30 s): their recovery waits for a later retention tick.
pub const RECOVER_QUIET: Duration = Duration::from_secs(60);

fn newest_mtime(p: &Path) -> Option<std::time::SystemTime> {
    let rd = fs::read_dir(p).ok()?;
    rd.flatten()
        .filter_map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => newest_mtime(&e.path()),
            Ok(_) => e.metadata().and_then(|m| m.modified()).ok(),
            Err(_) => None,
        })
        .max()
}

/// Crash-tail recovery of every session except `skip`, leaving recently written ones alone.
fn recover_quiet(home: &Home, skip: Option<&Path>) -> Vec<GapReport> {
    let mut gaps = Vec::new();
    for (_, dir) in sessions(home) {
        if skip == Some(dir.as_path()) {
            continue;
        }
        let recent = newest_mtime(&dir)
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < RECOVER_QUIET);
        if recent {
            continue;
        }
        match recover_root(&dir, &IndexPolicy::default()) {
            Ok(reports) => gaps.extend(reports.into_iter().filter_map(|r| r.gap)),
            Err(e) => tracing::warn!("journal recovery of {}: {e}", dir.display()),
        }
    }
    gaps
}

/// Result of a journal range read.
#[derive(Debug, Default)]
pub struct JournalRead {
    /// Highest stream coordinate consumed, including filtered records.
    pub scanned_through: u64,
    /// Events in stream order.
    pub events: Vec<PublishedRecord>,
    /// Row/byte cap reached: continue at `next_seq`.
    pub next_seq: Option<u64>,
    /// Scan limit (bytes/time) reached: `E_SCAN_LIMIT`, resume at `next_seq`.
    pub scan_limited: bool,
    /// No mapping covers the start (deleted by retention or never recorded).
    pub out_of_retention: bool,
    /// Bytes scanned.
    pub scanned_bytes: u64,
}

/// Read limits.
#[derive(Debug, Clone, Copy)]
pub struct ReadLimits {
    /// Rows.
    pub max: usize,
    /// Result JSON bytes.
    pub result_bytes: usize,
    /// Scanned bytes.
    pub scan_bytes: u64,
    /// Wall-clock budget.
    pub time: Duration,
}

/// Retention outcome summarized for health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainSummary {
    /// Bytes used by all sessions.
    pub bytes: u64,
    /// Quota.
    pub max_bytes: u64,
    /// Level.
    pub level: QuotaLevel,
    /// Segments deleted.
    pub deleted: usize,
    /// Pinned segments kept.
    pub pinned_kept: usize,
}

struct Writer {
    writer: JournalWriter,
    handle: JournalHandle,
}

/// The daemon's recording.
pub struct Journal {
    home: Home,
    /// This session's root.
    pub root: PathBuf,
    /// Stream epoch.
    pub epoch: u64,
    writers: Mutex<HashMap<(String, u64), Writer>>,
    mapping: Mutex<Option<MappingLog>>,
    fault: Option<FaultHook>,
    gw_seq: Mutex<HashMap<String, u64>>,
    lossy: AtomicBool,
    lost_maps: Mutex<Vec<(u64, u64)>>,
    pins: Mutex<HashSet<(PathBuf, SegmentId)>>,
}

impl std::fmt::Debug for Journal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journal")
            .field("root", &self.root)
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl Journal {
    /// Open this session's recording: recover crashed tails of earlier sessions, create the
    /// session dir and the mapping log. Returns crash-tail gaps found.
    pub fn open(
        home: &Home,
        epoch: u64,
        fault: Option<FaultHook>,
    ) -> std::io::Result<(Arc<Journal>, Vec<GapReport>)> {
        let mut gaps = Vec::new();
        gaps.extend(recover_quiet(home, None));
        let root = session_dir(home, epoch);
        if let Err(e) = fs::create_dir_all(&root) {
            tracing::error!("journal: cannot create {}: {e}", root.display());
        }
        let mut mcfg = JournalConfig::mapping(&root, epoch);
        mcfg.fault = fault.clone();
        let mapping = match MappingLog::spawn(mcfg) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::error!("journal: mapping log: {e}");
                None
            }
        };
        let j = Arc::new(Journal {
            home: home.clone(),
            lossy: AtomicBool::new(mapping.is_none()),
            root,
            epoch,
            writers: Mutex::new(HashMap::new()),
            mapping: Mutex::new(mapping),
            fault,
            gw_seq: Mutex::new(HashMap::new()),
            lost_maps: Mutex::new(Vec::new()),
            pins: Mutex::new(HashSet::new()),
        });
        Ok((j, gaps))
    }

    /// Write `manifest.json` (OS, build, domains, policy hash, env).
    pub fn write_manifest(&self, v: &Value) {
        let mut m = v.clone();
        m["os"] = json!(std::env::consts::OS);
        m["arch"] = json!(std::env::consts::ARCH);
        m["loomd_version"] = json!(env!("CARGO_PKG_VERSION"));
        m["stream_epoch"] = json!(U64(self.epoch));
        let bytes = serde_json::to_vec_pretty(&m).unwrap_or_default();
        if let Err(e) = write_atomic(&self.root.join("manifest.json"), &bytes, true) {
            tracing::warn!("journal manifest: {e}");
        }
    }

    /// Hub pre-publish hook for gateway producers: give the record a gap-free per-producer seq in
    /// publish order (the journal needs strictly increasing seqs) and write it.
    pub fn stamp_and_append_gateway(&self, rec: &mut SourceRecord) {
        if !is_gateway_producer(&rec.producer_id) {
            return;
        }
        rec.producer_epoch = U64(self.epoch);
        {
            let mut s = lock(&self.gw_seq);
            let n = s.entry(rec.producer_id.clone()).or_insert(0);
            *n += 1;
            rec.source_seq = U64(*n);
        }
        self.append(rec);
    }

    /// Append one record to its producer's journal (non-blocking).
    pub fn append(&self, rec: &SourceRecord) {
        let key = (rec.producer_id.clone(), rec.producer_epoch.get());
        let r = {
            let mut w = lock(&self.writers);
            if !w.contains_key(&key) {
                let mut cfg = JournalConfig::new(&self.root, &key.0, key.1);
                cfg.clock_id = rec.ts.clock_id.clone();
                cfg.fault = self.fault.clone();
                match JournalWriter::spawn(cfg) {
                    Ok(writer) => {
                        let handle = writer.handle();
                        w.insert(key.clone(), Writer { writer, handle });
                    }
                    Err(e) => {
                        tracing::error!("journal: writer {}/{}: {e}", key.0, key.1);
                        self.lossy.store(true, Ordering::Release);
                        return;
                    }
                }
            }
            w.get(&key).map(|x| x.handle.try_append(rec))
        };
        match r {
            Some(Ok(())) | None => {}
            Some(Err(AppendError::Full { .. })) => self.lossy.store(true, Ordering::Release),
            Some(Err(e)) => {
                tracing::warn!("journal append {}: {e}", rec.producer_id);
                self.lossy.store(true, Ordering::Release);
            }
        }
    }

    /// Health of all writers + gaps they report (taken). `lossy` latches until a poll sees every
    /// writer healthy again.
    pub fn poll(&self) -> (RecordHealth, Vec<GapData>) {
        let mut gaps: Vec<GapData> = std::mem::take(&mut *lock(&self.lost_maps))
            .into_iter()
            .map(|(a, b)| GapData {
                seq_range: Some(SeqRange { from: U64(a), to: U64(b) }),
                producer_range: None,
                count: GapCount::Exact(U64(b - a + 1)),
                cause: "mapping_backpressure".into(),
                bytes: None,
                detail: json!({"note": "stream mapping not recorded; events stay in their producer journals"}),
            })
            .collect();
        let mut worst = RecordHealth::Ok;
        for w in lock(&self.writers).values() {
            gaps.extend(w.handle.take_gaps().iter().map(GapReport::to_gap_data));
            worst = worse(worst, w.handle.health());
        }
        if let Some(m) = lock(&self.mapping).as_ref() {
            worst = worse(worst, m.status().health);
        } else {
            worst = RecordHealth::Failed;
        }
        if worst == RecordHealth::Failed {
            self.lossy.store(true, Ordering::Release);
        } else if gaps.is_empty() {
            self.lossy.store(false, Ordering::Release);
        }
        if self.lossy.load(Ordering::Acquire) {
            worst = RecordHealth::Failed;
        }
        (worst, gaps)
    }

    /// Recording lost data (agent/script writes must be refused).
    pub fn failed(&self) -> bool {
        self.lossy.load(Ordering::Acquire)
    }

    /// Flush and close all writers (daemon shutdown).
    pub fn close(&self) {
        for (_, w) in lock(&self.writers).drain() {
            let _ = w.writer.close();
        }
        if let Some(m) = lock(&self.mapping).take() {
            let _ = m.close();
        }
    }

    /// Wait until everything queued is on disk (tests, shutdown).
    pub fn flush(&self, timeout: Duration) {
        for w in lock(&self.writers).values() {
            let _ = w.writer.flush(timeout);
        }
        if let Some(m) = lock(&self.mapping).as_ref() {
            let _ = m.flush(timeout);
        }
    }

    /// Pin a segment so retention keeps it (test evidence; manual API in M1).
    pub fn pin(&self, session: &Path, id: SegmentId) {
        lock(&self.pins).insert((session.to_path_buf(), id));
    }

    fn root_for(&self, epoch: u64) -> Option<PathBuf> {
        if epoch == self.epoch {
            return Some(self.root.clone());
        }
        sessions(&self.home)
            .into_iter()
            .find(|(e, _)| *e == epoch)
            .map(|(_, p)| p)
    }

    /// Read stream seqs `[from, to]` of `epoch` from disk (mapping → segments), filtered.
    pub fn read_range(
        &self,
        epoch: u64,
        from: u64,
        to: u64,
        filter: &SubFilter,
        lim: ReadLimits,
    ) -> Result<JournalRead, String> {
        let mut out = JournalRead {
            scanned_through: from.saturating_sub(1),
            ..Default::default()
        };
        let Some(root) = self.root_for(epoch) else {
            out.out_of_retention = true;
            return Ok(out);
        };
        let deadline = Instant::now() + lim.time;
        let limits = ScanLimits {
            scan_bytes: lim.scan_bytes,
            result_bytes: u64::MAX,
            deadline: Some(deadline),
        };
        let map = lookup_mappings(&root, epoch, from, to, &limits, None).map_err(|e| e.to_string())?;
        if map.entries.first().is_none_or(|e| e.seq_first.get() > from) && map.entries.is_empty() {
            out.out_of_retention = true;
            return Ok(out);
        }
        let mut bytes = 0usize;
        let mut scanned = 0u64;
        for m in &map.entries {
            let (sf, sl) = (m.seq_first.get().max(from), m.seq_last.get().min(to));
            if sf > sl {
                continue;
            }
            let base = m.src_first.get();
            let src_from = base + (sf - m.seq_first.get());
            let src_to = base + (sl - m.seq_first.get());
            let dir = producer_dir(&root, &m.producer_id, m.producer_epoch.get());
            if !dir.is_dir() {
                continue; // producer not recorded (or deleted)
            }
            let left = ScanLimits {
                scan_bytes: lim.scan_bytes.saturating_sub(scanned),
                result_bytes: u64::MAX,
                deadline: Some(deadline),
            };
            let q = ScanQuery {
                range: ScanRange::SourceSeq {
                    from: src_from,
                    to: src_to,
                },
                limits: left,
                resume: None,
            };
            let r = scan(&dir, &q).map_err(|e| e.to_string())?;
            scanned += r.scanned_bytes;
            for rec in &r.records {
                let seq = m.seq_first.get() + (rec.source_seq - base);
                let Ok(src) = rec.decode_source_record() else {
                    continue;
                };
                let p = src.publish(U64(seq), U64(epoch));
                if !matches(filter, &p) {
                    out.scanned_through = out.scanned_through.max(seq);
                    continue;
                }
                let n = serde_json::to_string(&p).map(|s| s.len()).unwrap_or(0);
                if out.events.len() >= lim.max || (bytes + n > lim.result_bytes && !out.events.is_empty()) {
                    out.next_seq = Some(seq);
                    out.scanned_bytes = scanned;
                    return Ok(out);
                }
                bytes += n;
                out.scanned_through = out.scanned_through.max(seq);
                out.events.push(p);
            }
            if r.truncated {
                out.scan_limited = true;
                out.next_seq = r
                    .records
                    .last()
                    .map(|x| m.seq_first.get() + (x.source_seq - base) + 1)
                    .or(Some(sf));
                out.scanned_bytes = scanned;
                return Ok(out);
            }
        }
        if map.truncated {
            out.scan_limited = true;
            out.next_seq = map.entries.last().map(|e| e.seq_last.get() + 1).or(Some(from));
        }
        out.events.sort_by_key(|e| e.seq.get());
        out.scanned_bytes = scanned;
        Ok(out)
    }

    /// Recover crashed tails of earlier sessions that became quiet (retention tick).
    pub fn recover_earlier(&self) -> Vec<GapData> {
        recover_quiet(&self.home, Some(&self.root))
            .iter()
            .map(GapReport::to_gap_data)
            .collect()
    }

    /// Retention over every session (age, then quota ≥ 90 % → oldest unpinned sealed segments
    /// until < 90 %). Active segments and the mapping log are never deleted.
    pub fn retain(&self, policy: &RetainPolicy) -> RetainSummary {
        let pins = lock(&self.pins).clone();
        let pins_for = |s: &Path| {
            pins.iter()
                .filter(|(p, _)| p == s)
                .map(|(_, id)| id.clone())
                .collect::<HashSet<_>>()
        };
        let mut deleted = 0;
        let mut pinned_kept = 0;
        let age_only = RetainPolicy {
            max_age: policy.max_age,
            max_bytes: u64::MAX,
        };
        for (_, s) in sessions(&self.home) {
            match retain(&s, &age_only, &pins_for(&s)) {
                Ok(r) => deleted += r.deleted.len(),
                Err(e) => tracing::warn!("retain {}: {e}", s.display()),
            }
        }
        let max = policy.max_bytes.max(1);
        let mut usage: Vec<(PathBuf, u64)> = sessions(&self.home)
            .into_iter()
            .map(|(_, p)| {
                let b = dir_bytes(&p);
                (p, b)
            })
            .collect();
        let mut total: u64 = usage.iter().map(|(_, b)| b).sum();
        if total.saturating_mul(1000) / max >= 900 {
            for (s, b) in &mut usage {
                if total.saturating_mul(1000) / max < 900 {
                    break;
                }
                let other = total - *b;
                let target = (max / 10 * 9).saturating_sub(other); // bytes this session may keep
                let local = if target == 0 { 1 } else { target / 9 * 10 };
                match retain(
                    s,
                    &RetainPolicy {
                        max_age: Duration::MAX,
                        max_bytes: local,
                    },
                    &pins_for(s),
                ) {
                    Ok(r) => {
                        deleted += r.deleted.len();
                        pinned_kept += r.pinned_kept;
                        let now = dir_bytes(s);
                        total = total - *b + now;
                        *b = now;
                    }
                    Err(e) => tracing::warn!("retain {}: {e}", s.display()),
                }
            }
        }
        let pm = total.saturating_mul(1000) / max;
        let level = match pm {
            0..800 => QuotaLevel::Ok,
            800..900 => QuotaLevel::Warn,
            900..1000 => QuotaLevel::Prune,
            _ => QuotaLevel::Full,
        };
        RetainSummary {
            bytes: total,
            max_bytes: max,
            level,
            deleted,
            pinned_kept,
        }
    }
}

/// Worse of two healths.
pub fn worse(a: RecordHealth, b: RecordHealth) -> RecordHealth {
    let rank = |h: RecordHealth| match h {
        RecordHealth::Ok | RecordHealth::Disabled => 0,
        RecordHealth::Degraded => 1,
        RecordHealth::Failed => 2,
    };
    if rank(b) > rank(a) { b } else { a }
}

/// Hub mapping sink backed by the journal mapping log. Never drops: on backpressure the hub
/// thread retries (publishing slows), giving up only if the log is gone.
#[derive(Debug)]
pub struct JournalMapping(pub Arc<Journal>);

impl MappingSink for JournalMapping {
    fn record(&mut self, m: MappingRecord) {
        let e = MappingEntry {
            stream_epoch: U64(m.stream_epoch),
            seq_first: U64(m.seq_first),
            seq_last: U64(m.seq_last),
            producer_id: m.producer_id,
            producer_epoch: U64(m.producer_epoch),
            src_first: U64(m.src_first),
            src_last: U64(m.src_last),
            mono_first: U64(m.mono_first),
            mono_last: U64(m.mono_last),
        };
        let g = lock(&self.0.mapping);
        let Some(log) = g.as_ref() else { return };
        let start = Instant::now();
        loop {
            match log.append_mapping(&e) {
                Ok(()) => return,
                // append_mapping already waited ≤ 50 ms. Keep the hub waiting, but not forever: a
                // dead disk must not freeze STOP and the live view. After 2 s the entry is lost,
                // reported as a gap, and recording is failed (agent writes refused).
                Err(MappingError::Backpressure) if start.elapsed() < MAPPING_GIVE_UP => {}
                Err(MappingError::Backpressure) => {
                    lock(&self.0.lost_maps).push((m.seq_first, m.seq_last));
                    self.0.lossy.store(true, Ordering::Release);
                    return;
                }
                Err(err) => {
                    tracing::error!("mapping log: {err}");
                    self.0.lossy.store(true, Ordering::Release);
                    return;
                }
            }
        }
    }
}
