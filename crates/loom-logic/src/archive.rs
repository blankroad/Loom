//! Capture archive (operator-upgrade logic B2, finding L1-R2): whole-capture evidence kept apart from
//! the bounded live preview (`logic.edges` / `logic.annotations`, which stay rate-capped).
//!
//! Layout: `<root>/<capture_id>/` with
//! * `manifest.json` — [`Manifest`], rewritten atomically (tmp + rename) at open, every
//!   [`CHECKPOINT`] while recording, and at finish;
//! * `raw.lgr` — [`RAW_HEADER`] then raw blocks ([`EncodedBlock`]): lossless run-length record of all
//!   eight acquisition bits (D0..D7), independent of the displayed channel mask;
//! * `raw.idx` — one [`INDEX_ENTRY_BYTES`]-byte [`RawIndexEntry`] per block, appended only after the
//!   block bytes were written+flushed (a reader trusts only indexed blocks);
//! * `annotations.jsonl` — every decoder line the reader parsed, BEFORE the live rate limit and the
//!   batcher queue (see [`ann_line`]);
//! * `annotations.idx` — [`ANN_INDEX_ENTRY_BYTES`]-byte entries `(row u64 LE, byte_offset u64 LE)`
//!   every [`ANN_INDEX_EVERY`] rows (row 0 included);
//! * `export-*.csv|txt` — files written by `logic.export`.
//!
//! Partial evidence is never reported complete: any queue overflow, quota hit or I/O error stops that
//! part for good (sticky) at an exact sample (raw) / row (annotations), recorded in the manifest and
//! in capture_state `archive`. Sample indices are absolute capture samples (no re-basing).
//!
//! Contracts: `work/reports/operator-upgrade/logic/FUNCTIONS.md` §B2a / §B2a.1.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use loom_proto::error::ErrorCode;
use serde_json::{Map, Value};

use crate::model::Annotation;

/// Manifest `format` tag; directories without it are never read as archives nor pruned.
pub const FORMAT: &str = "loom-logic-archive/1";
/// Manifest file name.
pub const MANIFEST: &str = "manifest.json";
/// Raw block file.
pub const RAW_FILE: &str = "raw.lgr";
/// Raw block index.
pub const RAW_INDEX: &str = "raw.idx";
/// Annotation rows (JSON lines).
pub const ANN_FILE: &str = "annotations.jsonl";
/// Annotation row index.
pub const ANN_INDEX: &str = "annotations.idx";
/// First 8 bytes of `raw.lgr`: magic `LGR1` + version 1 + 3 zero bytes.
pub const RAW_HEADER: [u8; 8] = *b"LGR1\x01\0\0\0";
/// Block header bytes: start_sample u64 LE · state u8 · n_records u32 LE · payload_len u32 LE.
pub const BLOCK_HEADER_BYTES: usize = 17;
/// A block closes after this many transition records…
pub const BLOCK_MAX_RECORDS: u32 = 4096;
/// …or once it spans this many samples (bounds a range seek to one block of scanning).
pub const BLOCK_MAX_SAMPLES: u64 = 1 << 20;
/// `raw.idx` entry: start_sample u64 · file_offset u64 · state u8 · n_records u32 · 3 zero bytes (all LE).
pub const INDEX_ENTRY_BYTES: usize = 24;
/// `annotations.idx` entry: row u64 · byte_offset u64 (LE).
pub const ANN_INDEX_ENTRY_BYTES: usize = 16;
/// Annotation index stride (rows).
pub const ANN_INDEX_EVERY: u64 = 1024;
/// Writer queue bound (raw blocks + annotation lines, bytes). Exceeding it truncates that part.
pub const SINK_QUEUE_BYTES: usize = 16 << 20;
/// Manifest checkpoint period while recording.
pub const CHECKPOINT: Duration = Duration::from_secs(1);
/// `[logic]` defaults.
pub const DEFAULT_MAX_CAPTURE_MB: u64 = 256;
/// Total archive quota (MiB).
pub const DEFAULT_MAX_TOTAL_MB: u64 = 2048;
/// Oldest kept archive (hours).
pub const DEFAULT_MAX_AGE_H: u64 = 168;
/// Archives kept (including the new one).
pub const DEFAULT_MAX_CAPTURES: u32 = 32;
/// Archive root under `$LOOM_HOME`.
pub const ROOT_DIR: &str = "logic-archive";
/// `logic.archive.read kind=edges`: edges per result (default / max). Each returned chunk holds
/// ≤ `model::MAX_EDGES_PER_EVENT` edges, so the UI ingests it like a live `logic.edges` payload.
pub const READ_EDGES_DEFAULT: usize = 1024;
/// Max edges per read result (worst row ≈ 18 B → ≈ 37 KiB, inside the 64 KiB result).
pub const READ_EDGES_MAX: usize = 2048;
/// `kind=annotations`: rows per result (default / max), further bounded by [`READ_BYTES`].
pub const READ_ROWS_DEFAULT: usize = 500;
/// Max rows per read result.
pub const READ_ROWS_MAX: usize = 1000;
/// Serialized rows/chunks budget per read result.
pub const READ_BYTES: usize = 48 * 1024;
/// File bytes examined per read call (then `complete:false` + `next`).
pub const READ_SCAN_BYTES: u64 = 16 << 20;
/// `logic.archive.list` entries.
pub const LIST_MAX: usize = 32;
/// Export output bound (bytes) …
pub const EXPORT_MAX_BYTES: u64 = 64 << 20;
/// … and rows.
pub const EXPORT_MAX_ROWS: u64 = 2_000_000;

/// Archive failure → RPC error (existing codes only: Usage, NotFound, Config, Internal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveError {
    /// Error code.
    pub code: ErrorCode,
    /// Message.
    pub message: String,
}

/// `[logic]` archive settings resolved by the gateway and passed to the worker as start/read args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveConfig {
    /// `<LOOM_HOME>/logic-archive`.
    pub root: PathBuf,
    /// Per-capture quota (raw + annotations + indexes + exports), bytes.
    pub max_capture_bytes: u64,
    /// Whole-root quota, bytes.
    pub max_total_bytes: u64,
    /// Max age of a kept archive, seconds.
    pub max_age_s: u64,
    /// Max archives kept, including the new one.
    pub max_captures: u32,
}

/// Archive arguments as they travel gateway → worker (`args["archive"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveArgs {
    /// Archive on.
    Enabled(ArchiveConfig),
    /// Archive off (config `archive=false`, or no daemon storage roots); the reason is user-visible.
    Disabled {
        /// Why (`"disabled by [logic] archive=false"` / `"daemon storage roots unavailable"`).
        reason: String,
    },
}

impl ArchiveArgs {
    /// B2-A1. Gateway: `[logic]` section + daemon home → args. Keys (all optional):
    /// `archive` bool, `archive_max_capture_mb` 1..=65536, `archive_max_total_mb` 1..=1048576,
    /// `archive_max_age_h` 1..=87600, `archive_max_captures` 1..=10000. Wrong type/range →
    /// `Err("[logic] <key>: expected …")` (start fails with E_CONFIG; nothing is clamped).
    pub fn from_config(cfg: &Value, home: Option<&Path>) -> Result<ArchiveArgs, String> {
        let empty = Map::new();
        let sec = cfg.as_object().unwrap_or(&empty);
        let enabled = match sec.get("archive") {
            None => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("[logic] archive: expected true or false".to_owned()),
        };
        let get_u64 = |key: &str, lo: u64, hi: u64| -> Result<Option<u64>, String> {
            match sec.get(key) {
                None => Ok(None),
                Some(v) => match v.as_u64() {
                    Some(n) if (lo..=hi).contains(&n) => Ok(Some(n)),
                    _ => Err(format!("[logic] {key}: expected integer {lo}..={hi}")),
                },
            }
        };
        let cap_mb = get_u64("archive_max_capture_mb", 1, 65_536)?.unwrap_or(DEFAULT_MAX_CAPTURE_MB);
        let tot_mb = get_u64("archive_max_total_mb", 1, 1_048_576)?.unwrap_or(DEFAULT_MAX_TOTAL_MB);
        let age_h = get_u64("archive_max_age_h", 1, 87_600)?.unwrap_or(DEFAULT_MAX_AGE_H);
        let caps = get_u64("archive_max_captures", 1, 10_000)?.unwrap_or(u64::from(DEFAULT_MAX_CAPTURES));
        if !enabled {
            return Ok(ArchiveArgs::Disabled {
                reason: "disabled by [logic] archive=false".to_owned(),
            });
        }
        let Some(home) = home else {
            return Ok(ArchiveArgs::Disabled {
                reason: "daemon storage roots unavailable; logic archive is never written to a default home"
                    .to_owned(),
            });
        };
        let mul = |key: &str, v: u64, f: u64| {
            v.checked_mul(f)
                .ok_or_else(|| format!("[logic] {key}: out of range"))
        };
        Ok(ArchiveArgs::Enabled(ArchiveConfig {
            root: home.join(ROOT_DIR),
            max_capture_bytes: mul("archive_max_capture_mb", cap_mb, 1 << 20)?,
            max_total_bytes: mul("archive_max_total_mb", tot_mb, 1 << 20)?,
            max_age_s: mul("archive_max_age_h", age_h, 3600)?,
            max_captures: u32::try_from(caps)
                .map_err(|_| "[logic] archive_max_captures: out of range".to_owned())?,
        }))
    }
    /// B2-A2. Wire form: `{"enabled":true,"root":…,"max_capture_bytes":…,"max_total_bytes":…,
    /// "max_age_s":…,"max_captures":…}` or `{"enabled":false,"reason":…}`.
    pub fn to_args(&self) -> Value {
        match self {
            ArchiveArgs::Enabled(c) => serde_json::json!({
                "enabled": true,
                "root": c.root.to_string_lossy(),
                "max_capture_bytes": c.max_capture_bytes,
                "max_total_bytes": c.max_total_bytes,
                "max_age_s": c.max_age_s,
                "max_captures": c.max_captures,
            }),
            ArchiveArgs::Disabled { reason } => serde_json::json!({"enabled": false, "reason": reason}),
        }
    }
    /// B2-A3. Inverse of [`ArchiveArgs::to_args`] (worker side). Missing/`null` → `Disabled
    /// {reason:"archive not configured"}`; malformed → `Err` (E_USAGE). `root` must be absolute.
    pub fn from_args(v: &Value) -> Result<ArchiveArgs, String> {
        if v.is_null() {
            return Ok(ArchiveArgs::Disabled {
                reason: "archive not configured".to_owned(),
            });
        }
        let o = v
            .as_object()
            .ok_or_else(|| "archive: expected an object".to_owned())?;
        match o.get("enabled") {
            Some(Value::Bool(false)) => {
                let reason = o
                    .get("reason")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "archive.reason: expected string".to_owned())?;
                return Ok(ArchiveArgs::Disabled {
                    reason: reason.to_owned(),
                });
            }
            Some(Value::Bool(true)) => {}
            _ => return Err("archive.enabled: expected true or false".to_owned()),
        }
        let root = o
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| "archive.root: expected string".to_owned())?;
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            return Err("archive.root: expected an absolute path".to_owned());
        }
        let n = |k: &str| {
            o.get(k)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("archive.{k}: expected non-negative integer"))
        };
        let max_captures =
            u32::try_from(n("max_captures")?).map_err(|_| "archive.max_captures: out of range".to_owned())?;
        Ok(ArchiveArgs::Enabled(ArchiveConfig {
            root,
            max_capture_bytes: n("max_capture_bytes")?,
            max_total_bytes: n("max_total_bytes")?,
            max_age_s: n("max_age_s")?,
            max_captures,
        }))
    }
}

/// B2-A4. `^lc-[0-9a-f]{1,16}-[0-9]{1,20}$` (worker id format) — the only names ever joined to a root.
pub fn valid_capture_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("lc-") else {
        return false;
    };
    let Some((hex, dec)) = rest.split_once('-') else {
        return false;
    };
    (1..=16).contains(&hex.len())
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && (1..=20).contains(&dec.len())
        && dec.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------------------------
// Raw codec (pure).

/// One closed raw block, serialized: header ([`BLOCK_HEADER_BYTES`]) + records
/// (`varint(delta)` LEB128 u64, then the new 8-bit value). `delta` = sample − previous record sample
/// (block start for the first record), always ≥ 1. Covers `[start_sample, end_sample)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedBlock {
    /// First sample covered.
    pub start_sample: u64,
    /// Exclusive end (= next block's start).
    pub end_sample: u64,
    /// Value (all 8 bits) at `start_sample`.
    pub state: u8,
    /// Transition records.
    pub n_records: u32,
    /// Header + payload bytes as written to `raw.lgr`.
    pub bytes: Vec<u8>,
}

/// Streaming encoder of 1-byte samples into [`EncodedBlock`]s.
#[derive(Debug, Clone)]
pub struct RawEncoder {
    first: u64,
    sample: u64,
    started: bool,
    start: u64,
    /// Value at `start` (block header state).
    state: u8,
    /// Current value (after the last record).
    cur: u8,
    prev: u64,
    n: u32,
    payload: Vec<u8>,
}

impl RawEncoder {
    /// Encoder whose first fed byte is absolute sample `first_sample` (0 unless B5 pre-trigger).
    pub fn new(first_sample: u64) -> Self {
        RawEncoder {
            first: first_sample,
            sample: first_sample,
            started: false,
            start: first_sample,
            state: 0,
            cur: 0,
            prev: first_sample,
            n: 0,
            payload: Vec::new(),
        }
    }
    /// Next absolute sample index.
    pub fn sample(&self) -> u64 {
        self.sample
    }
    /// B2-R1. Feed samples; returns blocks closed during this call (in order). For each byte `b` at
    /// absolute sample `s`: if no block is open, OR the open block has `BLOCK_MAX_RECORDS` records,
    /// OR `s − start ≥ BLOCK_MAX_SAMPLES` → close the open block as `[start, s)` (if any) and open a
    /// new block at `s` with `state = cur = b`, `prev = s`, no record. Otherwise, if `b != cur` →
    /// record `varint(s − prev)` + `b`, `prev = s`, `cur = b`. Then `sample = s + 1`. Hence every
    /// block's `state` is the true value at its `start_sample` and every delta is ≥ 1.
    pub fn feed(&mut self, data: &[u8]) -> Vec<EncodedBlock> {
        let mut out = Vec::new();
        for &b in data {
            let s = self.sample;
            if !self.started || self.n >= BLOCK_MAX_RECORDS || s - self.start >= BLOCK_MAX_SAMPLES {
                if self.started {
                    let mut bytes = Vec::with_capacity(BLOCK_HEADER_BYTES + self.payload.len());
                    bytes.extend_from_slice(&self.start.to_le_bytes());
                    bytes.push(self.state);
                    bytes.extend_from_slice(&self.n.to_le_bytes());
                    bytes.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(&self.payload);
                    out.push(EncodedBlock {
                        start_sample: self.start,
                        end_sample: s,
                        state: self.state,
                        n_records: self.n,
                        bytes,
                    });
                }
                self.started = true;
                self.start = s;
                self.state = b;
                self.cur = b;
                self.prev = s;
                self.n = 0;
                self.payload.clear();
            } else if b != self.cur {
                put_varint(&mut self.payload, s - self.prev);
                self.payload.push(b);
                self.prev = s;
                self.cur = b;
                self.n += 1;
            }
            self.sample = s + 1;
        }
        out
    }
    /// B2-R2. Close the open block (if any sample was fed into it) and return it with the final
    /// exclusive end sample. Nothing fed at all → `(None, first_sample)`.
    pub fn finish(&mut self) -> (Option<EncodedBlock>, u64) {
        if !self.started {
            return (None, self.sample.max(self.first));
        }
        let mut bytes = Vec::with_capacity(BLOCK_HEADER_BYTES + self.payload.len());
        bytes.extend_from_slice(&self.start.to_le_bytes());
        bytes.push(self.state);
        bytes.extend_from_slice(&self.n.to_le_bytes());
        bytes.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.payload);
        let block = EncodedBlock {
            start_sample: self.start,
            end_sample: self.sample,
            state: self.state,
            n_records: self.n,
            bytes,
        };
        self.started = false;
        self.n = 0;
        self.payload.clear();
        (Some(block), self.sample)
    }
}

/// One `raw.idx` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawIndexEntry {
    /// Block start sample.
    pub start_sample: u64,
    /// Byte offset of the block header in `raw.lgr`.
    pub offset: u64,
    /// Value at start.
    pub state: u8,
    /// Records in the block.
    pub n_records: u32,
}

impl RawIndexEntry {
    /// B2-R3. 24-byte LE serialization (see [`INDEX_ENTRY_BYTES`]).
    pub fn to_bytes(&self) -> [u8; INDEX_ENTRY_BYTES] {
        let mut out = [0u8; INDEX_ENTRY_BYTES];
        out[0..8].copy_from_slice(&self.start_sample.to_le_bytes());
        out[8..16].copy_from_slice(&self.offset.to_le_bytes());
        out[16] = self.state;
        out[17..21].copy_from_slice(&self.n_records.to_le_bytes());
        out
    }
}

/// B2-R4. Parse `raw.idx`. A trailing partial entry (< 24 bytes, crash mid-append) is ignored and
/// reported as `true` in the second value. Entries must have strictly increasing `start_sample` and
/// `offset` (else `Err`, E_INTERNAL "archive index corrupt").
pub fn parse_raw_index(bytes: &[u8]) -> Result<(Vec<RawIndexEntry>, bool), ArchiveError> {
    let corrupt = || ArchiveError {
        code: ErrorCode::Internal,
        message: "archive index corrupt".to_owned(),
    };
    let mut out: Vec<RawIndexEntry> = Vec::with_capacity(bytes.len() / INDEX_ENTRY_BYTES);
    for c in bytes.chunks_exact(INDEX_ENTRY_BYTES) {
        let mut u8s = [0u8; 8];
        u8s.copy_from_slice(&c[0..8]);
        let start_sample = u64::from_le_bytes(u8s);
        u8s.copy_from_slice(&c[8..16]);
        let offset = u64::from_le_bytes(u8s);
        let state = c[16];
        let n_records = u32::from_le_bytes([c[17], c[18], c[19], c[20]]);
        if let Some(prev) = out.last()
            && (start_sample <= prev.start_sample || offset <= prev.offset)
        {
            return Err(corrupt());
        }
        out.push(RawIndexEntry {
            start_sample,
            offset,
            state,
            n_records,
        });
    }
    Ok((out, !bytes.len().is_multiple_of(INDEX_ENTRY_BYTES)))
}

/// A decoded block: transitions `(sample, value)` after `start_sample`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBlock {
    /// First sample covered.
    pub start_sample: u64,
    /// Value at start.
    pub state: u8,
    /// Transitions (absolute sample, new value), increasing samples.
    pub edges: Vec<(u64, u8)>,
}

/// B2-R5. Decode one block from the start of `bytes`; returns it and the bytes consumed. Truncated
/// header/payload, a varint longer than 10 bytes, `delta == 0` or `n_records` not matching the
/// payload → `Err` (E_INTERNAL "archive block corrupt at sample N").
pub fn decode_block(bytes: &[u8]) -> Result<(DecodedBlock, usize), ArchiveError> {
    let corrupt = |at: u64| ArchiveError {
        code: ErrorCode::Internal,
        message: format!("archive block corrupt at sample {at}"),
    };
    if bytes.len() < BLOCK_HEADER_BYTES {
        return Err(corrupt(0));
    }
    let mut w = [0u8; 8];
    w.copy_from_slice(&bytes[0..8]);
    let start = u64::from_le_bytes(w);
    let state = bytes[8];
    let n = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    let plen = u32::from_le_bytes([bytes[13], bytes[14], bytes[15], bytes[16]]) as usize;
    let total = BLOCK_HEADER_BYTES + plen;
    if bytes.len() < total {
        return Err(corrupt(start));
    }
    let payload = &bytes[BLOCK_HEADER_BYTES..total];
    let mut edges = Vec::new();
    let mut pos = 0usize;
    let mut prev = start;
    while pos < payload.len() {
        let Some((delta, used)) = get_varint(&payload[pos..]) else {
            return Err(corrupt(start));
        };
        pos += used;
        if delta == 0 || pos >= payload.len() {
            return Err(corrupt(start));
        }
        let value = payload[pos];
        pos += 1;
        let Some(sample) = prev.checked_add(delta) else {
            return Err(corrupt(start));
        };
        edges.push((sample, value));
        prev = sample;
    }
    if edges.len() as u64 != u64::from(n) {
        return Err(corrupt(start));
    }
    Ok((
        DecodedBlock {
            start_sample: start,
            state,
            edges,
        },
        total,
    ))
}

// ---------------------------------------------------------------------------------------------
// Annotation lines (pure).

/// B2-N1. One `annotations.jsonl` line incl. trailing `\n`:
/// parsed `{"s":START,"e":END,"d":"uart-1","t":"41"}`; unparsed `{"s":null,"e":null,"d":null,"t":"<raw>"}`
/// (keys in this order; `t` JSON-escaped). Loss markers (`Annotation::is_loss_marker`) are never archived.
pub fn ann_line(a: &Annotation) -> Vec<u8> {
    if a.is_loss_marker() {
        return Vec::new();
    }
    let num = |v: Option<u64>| v.map_or_else(|| "null".to_owned(), |n| n.to_string());
    let d = a
        .decoder
        .as_ref()
        .map_or_else(|| "null".to_owned(), |d| Value::String(d.clone()).to_string());
    let t = Value::String(a.text.clone()).to_string();
    format!(
        "{{\"s\":{},\"e\":{},\"d\":{},\"t\":{}}}\n",
        num(a.start),
        num(a.end),
        d,
        t
    )
    .into_bytes()
}

/// B2-N2. Archived line → the same row shape as `Annotation::to_row(samplerate)` plus
/// `"row": <row>` and `"source": "archive"`. Malformed line → `None` (counted by the caller).
pub fn ann_row(line: &str, row: u64, samplerate: u64) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    let o = v.as_object()?;
    let opt_num = |k: &str| -> Option<Option<u64>> {
        match o.get(k)? {
            Value::Null => Some(None),
            n => n.as_u64().map(Some),
        }
    };
    let start = opt_num("s")?;
    let end = opt_num("e")?;
    let decoder = match o.get("d")? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return None,
    };
    let text = o.get("t")?.as_str()?.to_owned();
    let a = Annotation {
        decoder,
        start,
        end,
        text,
        lost_before: 0,
    };
    let mut v = a.to_row(samplerate);
    v["row"] = serde_json::json!(row);
    v["source"] = serde_json::json!("archive");
    Some(v)
}

// ---------------------------------------------------------------------------------------------
// Manifest.

/// Archive lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveState {
    /// Capture running (or crashed before finish — readers report it as not finalized).
    Recording,
    /// Finished, nothing truncated, raw covers every acquired sample.
    Complete,
    /// Finished with a truncated part or a raw end short of the acquired samples.
    Partial,
    /// Could not record at all (open failed after the dir existed).
    Failed,
}

impl ArchiveState {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveState::Recording => "recording",
            ArchiveState::Complete => "complete",
            ArchiveState::Partial => "partial",
            ArchiveState::Failed => "failed",
        }
    }
}

/// `manifest.json` contents (hand-serialized; key names are the wire names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// = [`FORMAT`].
    pub format: String,
    /// Capture id (= directory name).
    pub capture_id: String,
    /// `source` wire name.
    pub source: String,
    /// Sample rate (Hz).
    pub samplerate: u64,
    /// Canonical decoder spec(s).
    pub decoders: Vec<String>,
    /// Lifecycle.
    pub state: ArchiveState,
    /// Wall clock at open / last write (ns since epoch; informational).
    pub started_wall_ns: u64,
    /// Last manifest write.
    pub updated_wall_ns: u64,
    /// First archived sample (0 unless B5 pre-trigger).
    pub raw_first_sample: u64,
    /// Exclusive end of the indexed raw coverage.
    pub raw_end_sample: u64,
    /// Samples the capture acquired (final only; `None` while recording).
    pub acquired_samples: Option<u64>,
    /// Raw bytes on disk (`raw.lgr` + `raw.idx`).
    pub raw_bytes: u64,
    /// Indexed raw blocks.
    pub raw_blocks: u64,
    /// Sticky raw truncation (first sample NOT archived).
    pub raw_truncated_at_sample: Option<u64>,
    /// Raw part error text.
    pub raw_error: Option<String>,
    /// Archived annotation rows.
    pub ann_rows: u64,
    /// Annotation bytes on disk (`.jsonl` + `.idx`).
    pub ann_bytes: u64,
    /// Sticky annotation truncation (first row NOT archived).
    pub ann_truncated_at_row: Option<u64>,
    /// Annotation part error text.
    pub ann_error: Option<String>,
    /// Quota of this capture (bytes).
    pub max_capture_bytes: u64,
}

impl Manifest {
    /// B2-F1. JSON object: `format, capture_id, source, samplerate, decoders, state, started_wall_ns,
    /// updated_wall_ns, max_capture_bytes, raw:{first_sample,end_sample,acquired_samples,bytes,blocks,
    /// truncated_at_sample,error}, annotations:{rows,bytes,truncated_at_row,error}`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "format": self.format,
            "capture_id": self.capture_id,
            "source": self.source,
            "samplerate": self.samplerate,
            "decoders": self.decoders,
            "state": self.state.as_str(),
            "started_wall_ns": self.started_wall_ns,
            "updated_wall_ns": self.updated_wall_ns,
            "max_capture_bytes": self.max_capture_bytes,
            "raw": {
                "first_sample": self.raw_first_sample,
                "end_sample": self.raw_end_sample,
                "acquired_samples": self.acquired_samples,
                "bytes": self.raw_bytes,
                "blocks": self.raw_blocks,
                "truncated_at_sample": self.raw_truncated_at_sample,
                "error": self.raw_error,
            },
            "annotations": {
                "rows": self.ann_rows,
                "bytes": self.ann_bytes,
                "truncated_at_row": self.ann_truncated_at_row,
                "error": self.ann_error,
            },
        })
    }
    /// B2-F2. Inverse; `format != FORMAT`, missing/mistyped required keys → `Err(reason)`.
    pub fn from_json(v: &Value) -> Result<Manifest, String> {
        let req_u64 = |o: &Value, k: &str| -> Result<u64, String> {
            o.get(k)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("manifest.{k}: expected non-negative integer"))
        };
        let req_str = |o: &Value, k: &str| -> Result<String, String> {
            o.get(k)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("manifest.{k}: expected string"))
        };
        let opt_u64 = |o: &Value, k: &str| -> Result<Option<u64>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(n) => n
                    .as_u64()
                    .map(Some)
                    .ok_or_else(|| format!("manifest.{k}: expected integer or null")),
            }
        };
        let opt_str = |o: &Value, k: &str| -> Result<Option<String>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(format!("manifest.{k}: expected string or null")),
            }
        };
        let format = req_str(v, "format")?;
        if format != FORMAT {
            return Err(format!("manifest.format: expected {FORMAT}, found {format}"));
        }
        let state = match req_str(v, "state")?.as_str() {
            "recording" => ArchiveState::Recording,
            "complete" => ArchiveState::Complete,
            "partial" => ArchiveState::Partial,
            "failed" => ArchiveState::Failed,
            other => return Err(format!("manifest.state: unknown state {other:?}")),
        };
        let decoders = v
            .get("decoders")
            .and_then(Value::as_array)
            .ok_or_else(|| "manifest.decoders: expected array".to_owned())?
            .iter()
            .map(|d| {
                d.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "manifest.decoders: expected strings".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let raw = v
            .get("raw")
            .filter(|r| r.is_object())
            .ok_or_else(|| "manifest.raw: expected object".to_owned())?;
        let ann = v
            .get("annotations")
            .filter(|r| r.is_object())
            .ok_or_else(|| "manifest.annotations: expected object".to_owned())?;
        Ok(Manifest {
            format,
            capture_id: req_str(v, "capture_id")?,
            source: req_str(v, "source")?,
            samplerate: req_u64(v, "samplerate")?,
            decoders,
            state,
            started_wall_ns: req_u64(v, "started_wall_ns")?,
            updated_wall_ns: req_u64(v, "updated_wall_ns")?,
            raw_first_sample: req_u64(raw, "first_sample")?,
            raw_end_sample: req_u64(raw, "end_sample")?,
            acquired_samples: opt_u64(raw, "acquired_samples")?,
            raw_bytes: req_u64(raw, "bytes")?,
            raw_blocks: req_u64(raw, "blocks")?,
            raw_truncated_at_sample: opt_u64(raw, "truncated_at_sample")?,
            raw_error: opt_str(raw, "error")?,
            ann_rows: req_u64(ann, "rows")?,
            ann_bytes: req_u64(ann, "bytes")?,
            ann_truncated_at_row: opt_u64(ann, "truncated_at_row")?,
            ann_error: opt_str(ann, "error")?,
            max_capture_bytes: req_u64(v, "max_capture_bytes")?,
        })
    }
    /// B2-F3. Atomic write: `manifest.json.tmp` then rename over `manifest.json` in `dir`.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let tmp = dir.join("manifest.json.tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(self.to_json().to_string().as_bytes())?;
        f.flush()?;
        drop(f);
        std::fs::rename(&tmp, dir.join(MANIFEST))
    }
    /// B2-F4. Read + parse `dir/manifest.json` (I/O or parse error → `Err(reason)`).
    pub fn read(dir: &Path) -> Result<Manifest, String> {
        let path = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Manifest::from_json(&v)
    }
    /// B2-F5 (amended B2a.1). Evidence completeness as readers report it: `complete` iff state Complete,
    /// no truncation on either part, no error on either part, and `acquired_samples == Some(raw_end_sample)`.
    pub fn complete(&self) -> bool {
        self.state == ArchiveState::Complete
            && self.raw_truncated_at_sample.is_none()
            && self.ann_truncated_at_row.is_none()
            && self.raw_error.is_none()
            && self.ann_error.is_none()
            && self.acquired_samples == Some(self.raw_end_sample)
    }
}

/// Static facts written into a new manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestMeta {
    /// Capture id.
    pub capture_id: String,
    /// Source wire name.
    pub source: String,
    /// Sample rate.
    pub samplerate: u64,
    /// Canonical decoder specs.
    pub decoders: Vec<String>,
    /// First archived sample.
    pub first_sample: u64,
}

// ---------------------------------------------------------------------------------------------
// Rotation.

/// What [`prune`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Capture ids removed, oldest first.
    pub removed: Vec<String>,
    /// Bytes still used under the root after pruning (archives only).
    pub kept_bytes: u64,
    /// Non-fatal problems (unreadable dir etc.), user-visible in the start result.
    pub errors: Vec<String>,
}

/// B2-P1. Make room for one new capture under `cfg.root`. Candidates: direct subdirs whose name
/// passes [`valid_capture_id`], that are real directories (not symlinks), and whose manifest parses
/// with [`FORMAT`]; everything else is left untouched. Never removes `protect`. Oldest
/// (`started_wall_ns`) first, remove while ANY holds: count ≥ `max_captures`;
/// age (`now_wall_ns − started_wall_ns`) > `max_age_s`; used bytes + `max_capture_bytes` >
/// `max_total_bytes`. Removal = `remove_dir_all` of that one directory.
pub fn prune(cfg: &ArchiveConfig, protect: Option<&str>, now_wall_ns: u64) -> PruneReport {
    let mut report = PruneReport::default();
    let rd = match std::fs::read_dir(&cfg.root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return report,
        Err(e) => {
            report.errors.push(format!("{}: {e}", cfg.root.display()));
            return report;
        }
    };
    // (started_wall_ns, id, bytes)
    let mut cands: Vec<(u64, String, u64)> = Vec::new();
    for entry in rd.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid_capture_id(&name) {
            continue;
        }
        let path = entry.path();
        let is_dir = std::fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        if let Ok(m) = Manifest::read(&path) {
            cands.push((m.started_wall_ns, name, dir_bytes(&path)));
        }
    }
    cands.sort();
    let mut used: u64 = cands.iter().map(|c| c.2).sum();
    let mut remaining = cands.len();
    let max_age_ns = u128::from(cfg.max_age_s) * 1_000_000_000;
    for (started, id, bytes) in &cands {
        if protect == Some(id.as_str()) {
            continue;
        }
        let age_ns = u128::from(now_wall_ns.saturating_sub(*started));
        let remove = age_ns > max_age_ns
            || remaining >= cfg.max_captures as usize
            || used.saturating_add(cfg.max_capture_bytes) > cfg.max_total_bytes;
        if !remove {
            continue;
        }
        match std::fs::remove_dir_all(cfg.root.join(id)) {
            Ok(()) => {
                report.removed.push(id.clone());
                remaining -= 1;
                used = used.saturating_sub(*bytes);
            }
            Err(e) => report.errors.push(format!("{id}: {e}")),
        }
    }
    report.kept_bytes = used;
    report
}

// ---------------------------------------------------------------------------------------------
// Writer (worker side).

/// Shared progress of the writer thread (atomics read by [`ArchiveSink::status`]).
#[derive(Debug, Default)]
pub struct SinkShared {
    /// B5: first archived sample once known (set by [`ArchiveSink::set_first_sample`] before any block).
    pub first_sample: AtomicU64,
    /// B5: `first_sample` was set (else the manifest keeps `ManifestMeta::first_sample`).
    pub first_set: AtomicBool,
    /// Bytes queued but not yet written.
    pub queued: AtomicUsize,
    /// Raw coverage end written+indexed.
    pub raw_end_sample: AtomicU64,
    /// Raw bytes written.
    pub raw_bytes: AtomicU64,
    /// Raw blocks indexed.
    pub raw_blocks: AtomicU64,
    /// Annotation rows written.
    pub ann_rows: AtomicU64,
    /// Annotation bytes written.
    pub ann_bytes: AtomicU64,
    /// Raw truncated (sticky); sample in `trunc`.
    pub raw_truncated: AtomicBool,
    /// Annotations truncated (sticky).
    pub ann_truncated: AtomicBool,
    /// Rows offered to the archive (= row number of the next line).
    pub ann_offered: AtomicU64,
    /// Truncation points and error texts.
    pub trunc: Mutex<SinkTrunc>,
}

/// Truncation details.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SinkTrunc {
    /// First raw sample not archived.
    pub raw_at_sample: Option<u64>,
    /// Raw reason.
    pub raw_error: Option<String>,
    /// First annotation row not archived.
    pub ann_at_row: Option<u64>,
    /// Annotation reason.
    pub ann_error: Option<String>,
}

/// Writer-thread messages.
#[derive(Debug)]
pub enum SinkMsg {
    /// A closed raw block.
    Raw(EncodedBlock),
    /// One annotation line ([`ann_line`]) with its row number.
    Ann(u64, Vec<u8>),
    /// Finish: final raw end + acquired samples + whether the capture failed.
    Finish {
        /// Encoder end sample.
        end_sample: u64,
        /// Samples the capture acquired.
        acquired: u64,
    },
}

/// Per-capture archive writer: one thread, one byte-bounded queue, sticky truncation.
#[derive(Debug)]
pub struct ArchiveSink {
    dir: PathBuf,
    tx: Option<mpsc::Sender<SinkMsg>>,
    shared: Arc<SinkShared>,
    join: Option<JoinHandle<Manifest>>,
    prune: PruneReport,
}

/// Clonable handle the decoder-reader thread uses to archive every parsed line.
#[derive(Debug, Clone)]
pub struct AnnTap {
    tx: mpsc::Sender<SinkMsg>,
    shared: Arc<SinkShared>,
}

impl ArchiveSink {
    /// B2-S1. `prune(cfg, None, now)`, create `<root>/<capture_id>` (must not exist), write
    /// `raw.lgr` header, empty `raw.idx`/`annotations.jsonl`/`annotations.idx`, the Recording
    /// manifest, and spawn the writer thread (`logic-archive`). Writer loop: `recv_timeout(CHECKPOINT)`;
    /// on Raw/Ann write+flush (index entry after the data), update atomics, enforce
    /// `max_capture_bytes` (the write that would exceed it is not done → sticky truncation with
    /// error "archive quota <N> MiB reached"); I/O error → sticky truncation with the error text;
    /// rewrite the manifest each CHECKPOINT; on Finish write the final manifest (state Complete or
    /// Partial per [`Manifest::complete`] rules) and return it from the thread. Any setup error →
    /// `Err(text)`; the caller then records `archive.state = "failed"` and the capture continues.
    pub fn open(cfg: &ArchiveConfig, meta: &ManifestMeta, now_wall_ns: u64) -> Result<ArchiveSink, String> {
        use std::sync::atomic::Ordering::Relaxed;
        if !valid_capture_id(&meta.capture_id) {
            return Err(format!("archive: invalid capture id {:?}", meta.capture_id));
        }
        std::fs::create_dir_all(&cfg.root)
            .map_err(|e| format!("archive root {}: {e}", cfg.root.display()))?;
        let prune_report = prune(cfg, Some(&meta.capture_id), now_wall_ns);
        let dir = cfg.root.join(&meta.capture_id);
        if let Err(e) = std::fs::create_dir(&dir) {
            return Err(if e.kind() == std::io::ErrorKind::AlreadyExists {
                "archive dir exists".to_owned()
            } else {
                format!("archive dir {}: {e}", dir.display())
            });
        }
        std::fs::write(dir.join(RAW_FILE), RAW_HEADER).map_err(|e| format!("archive {RAW_FILE}: {e}"))?;
        for f in [RAW_INDEX, ANN_FILE, ANN_INDEX] {
            std::fs::File::create(dir.join(f)).map_err(|e| format!("archive {f}: {e}"))?;
        }
        let manifest = Manifest {
            format: FORMAT.to_owned(),
            capture_id: meta.capture_id.clone(),
            source: meta.source.clone(),
            samplerate: meta.samplerate,
            decoders: meta.decoders.clone(),
            state: ArchiveState::Recording,
            started_wall_ns: now_wall_ns,
            updated_wall_ns: now_wall_ns,
            raw_first_sample: meta.first_sample,
            raw_end_sample: meta.first_sample,
            acquired_samples: None,
            raw_bytes: RAW_HEADER.len() as u64,
            raw_blocks: 0,
            raw_truncated_at_sample: None,
            raw_error: None,
            ann_rows: 0,
            ann_bytes: 0,
            ann_truncated_at_row: None,
            ann_error: None,
            max_capture_bytes: cfg.max_capture_bytes,
        };
        manifest
            .write(&dir)
            .map_err(|e| format!("archive manifest: {e}"))?;
        let shared = Arc::new(SinkShared::default());
        shared.raw_end_sample.store(meta.first_sample, Relaxed);
        shared.raw_bytes.store(RAW_HEADER.len() as u64, Relaxed);
        let (tx, rx) = mpsc::channel();
        let (sh, wdir) = (Arc::clone(&shared), dir.clone());
        let join = std::thread::Builder::new()
            .name("logic-archive".to_owned())
            .spawn(move || writer_loop(wdir, rx, sh, manifest))
            .map_err(|e| format!("archive writer thread: {e}"))?;
        Ok(ArchiveSink {
            dir,
            tx: Some(tx),
            shared,
            join: Some(join),
            prune: prune_report,
        })
    }
    /// B2-S2. Queue closed raw blocks without blocking. Once truncated, drops everything. If
    /// `queued + bytes > SINK_QUEUE_BYTES` → truncate at that block's `start_sample` with error
    /// "archive writer behind (queue > 16 MiB)".
    pub fn raw(&self, blocks: Vec<EncodedBlock>) {
        use std::sync::atomic::Ordering::Relaxed;
        let cut = |at: u64, why: &str| {
            let mut t = self
                .shared
                .trunc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if t.raw_at_sample.is_none() {
                t.raw_at_sample = Some(at);
            }
            if t.raw_error.is_none() {
                t.raw_error = Some(why.to_owned());
            }
            drop(t);
            self.shared.raw_truncated.store(true, Relaxed);
        };
        for b in blocks {
            if self.shared.raw_truncated.load(Relaxed) {
                return;
            }
            let n = b.bytes.len();
            if self.shared.queued.load(Relaxed) + n > SINK_QUEUE_BYTES {
                cut(b.start_sample, "archive writer behind (queue > 16 MiB)");
                return;
            }
            let at = b.start_sample;
            self.shared.queued.fetch_add(n, Relaxed);
            let sent = self
                .tx
                .as_ref()
                .is_some_and(|tx| tx.send(SinkMsg::Raw(b)).is_ok());
            if !sent {
                self.shared.queued.fetch_sub(n, Relaxed);
                cut(at, "archive writer stopped");
                return;
            }
        }
    }
    /// B2-S3. Annotation tap for the reader thread.
    pub fn tap(&self) -> AnnTap {
        let tx = self.tx.clone().unwrap_or_else(|| mpsc::channel().0);
        AnnTap {
            tx,
            shared: Arc::clone(&self.shared),
        }
    }
    /// B2-S4. capture_state `archive` object from atomics (no I/O): `{"state":"recording","dir":…,
    /// "raw_end_sample","raw_bytes","annotation_rows","raw_truncated_at_sample","annotation_truncated_at_row",
    /// "error","pruned":[ids]}` (`error` = first of raw/ann error, else null).
    pub fn status(&self) -> Value {
        use std::sync::atomic::Ordering::Relaxed;
        let (raw_at, ann_at, error) = {
            let t = self
                .shared
                .trunc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                t.raw_at_sample,
                t.ann_at_row,
                t.raw_error.clone().or_else(|| t.ann_error.clone()),
            )
        };
        serde_json::json!({
            "state": "recording",
            "dir": self.dir.to_string_lossy(),
            "raw_end_sample": self.shared.raw_end_sample.load(Relaxed),
            "raw_bytes": self.shared.raw_bytes.load(Relaxed),
            "annotation_rows": self.shared.ann_rows.load(Relaxed),
            "raw_truncated_at_sample": raw_at,
            "annotation_truncated_at_row": ann_at,
            "error": error,
            "pruned": self.prune.removed,
        })
    }
    /// B2-S7 (B2a.1). The decoder did not see every acquired sample (tee failure, early exit, kill during
    /// drain): record `text` as the annotation part's error (first error wins; rows already archived stay
    /// readable, no row truncation is set). Call before [`ArchiveSink::finish`]; the final manifest is then
    /// Partial (F5) and `status()["error"]` names it when raw has no error.
    pub fn mark_decode_gap(&self, text: &str) {
        let mut t = self
            .shared
            .trunc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if t.ann_error.is_none() {
            t.ann_error = Some(text.to_owned());
        }
    }
    /// B5-S8. Pre-trigger start (or, for a trigger that never fired, the acquired end) becomes the
    /// archive's first sample: manifest `raw_first_sample` and — while no block was written — also
    /// `raw_end_sample`. Call before the first `raw()` (and for not-fired, before `finish`).
    pub fn set_first_sample(&self, sample: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.shared.first_sample.store(sample, Relaxed);
        if self.shared.raw_blocks.load(Relaxed) == 0 {
            self.shared.raw_end_sample.store(sample, Relaxed);
        }
        self.shared.first_set.store(true, Relaxed);
    }
    /// B2-S5. Send the encoder's last block + Finish, drop the sender, join (bounded by the queue
    /// drain), return the final status object (same keys as `status`, `state` from the final
    /// manifest). A panicked writer → state "failed", error "archive writer ended unexpectedly".
    pub fn finish(self, last: Option<EncodedBlock>, end_sample: u64, acquired: u64) -> Value {
        use std::sync::atomic::Ordering::Relaxed;
        let mut this = self;
        if let Some(b) = last {
            this.raw(vec![b]);
        }
        if let Some(tx) = this.tx.take() {
            let _ = tx.send(SinkMsg::Finish { end_sample, acquired });
        }
        let manifest = this.join.take().and_then(|j| j.join().ok());
        match manifest {
            Some(m) => serde_json::json!({
                "state": m.state.as_str(),
                "dir": this.dir.to_string_lossy(),
                "raw_end_sample": m.raw_end_sample,
                "raw_bytes": m.raw_bytes,
                "annotation_rows": m.ann_rows,
                "raw_truncated_at_sample": m.raw_truncated_at_sample,
                "annotation_truncated_at_row": m.ann_truncated_at_row,
                "error": m.raw_error.clone().or_else(|| m.ann_error.clone()),
                "pruned": this.prune.removed,
            }),
            None => serde_json::json!({
                "state": "failed",
                "dir": this.dir.to_string_lossy(),
                "raw_end_sample": this.shared.raw_end_sample.load(Relaxed),
                "raw_bytes": this.shared.raw_bytes.load(Relaxed),
                "annotation_rows": this.shared.ann_rows.load(Relaxed),
                "raw_truncated_at_sample": Value::Null,
                "annotation_truncated_at_row": Value::Null,
                "error": "archive writer ended unexpectedly",
                "pruned": this.prune.removed,
            }),
        }
    }
}

impl AnnTap {
    /// B2-S6. Archive one parsed line (never blocks; loss markers ignored). Row number = the
    /// `ann_offered` counter (incremented for every offered non-marker line, even after truncation).
    /// Over the queue bound → sticky truncation at that row ("archive writer behind (queue > 16 MiB)").
    pub fn push(&self, a: &Annotation) {
        use std::sync::atomic::Ordering::Relaxed;
        if a.is_loss_marker() {
            return;
        }
        let row = self.shared.ann_offered.fetch_add(1, Relaxed);
        if self.shared.ann_truncated.load(Relaxed) {
            return;
        }
        let line = ann_line(a);
        let n = line.len();
        let cut = |why: &str| {
            let mut t = self
                .shared
                .trunc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if t.ann_at_row.is_none() {
                t.ann_at_row = Some(row);
            }
            if t.ann_error.is_none() {
                t.ann_error = Some(why.to_owned());
            }
            drop(t);
            self.shared.ann_truncated.store(true, Relaxed);
        };
        if self.shared.queued.load(Relaxed) + n > SINK_QUEUE_BYTES {
            cut("archive writer behind (queue > 16 MiB)");
            return;
        }
        self.shared.queued.fetch_add(n, Relaxed);
        if self.tx.send(SinkMsg::Ann(row, line)).is_err() {
            self.shared.queued.fetch_sub(n, Relaxed);
            cut("archive writer stopped");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Readers / export (worker side, blocking file I/O on a helper thread).

/// `logic.archive.read` request (strictly parsed; shared by gateway and worker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadReq {
    /// Capture id ([`valid_capture_id`]).
    pub capture_id: String,
    /// `edges` | `annotations`.
    pub kind: ReadKind,
    /// Inclusive start sample (default: archive first sample).
    pub from_sample: Option<u64>,
    /// Exclusive end sample (default: archive end).
    pub to_sample: Option<u64>,
    /// Edges or rows per result (defaults/max per kind).
    pub limit: Option<usize>,
    /// Edges: bits kept (1..=255, default 255).
    pub mask: Option<u8>,
    /// Annotations: continue after this row (exclusive) — the previous result's `next_row`.
    pub after_row: Option<u64>,
}

/// What to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKind {
    /// Raw edges (logic.edges payload shape).
    Edges,
    /// Annotation rows.
    Annotations,
}

impl ReadReq {
    /// B2-Q1. Strict params: allowed keys `capture_id` (required), `kind` (required),
    /// `from_sample`, `to_sample`, `limit`, `mask`, `after_row`; `to_sample > from_sample` when both;
    /// `limit` 1..=READ_*_MAX for the kind; `mask` only for edges, `after_row` only for annotations.
    pub fn from_params(p: &Value) -> Result<ReadReq, String> {
        const ALLOWED: [&str; 7] = [
            "capture_id",
            "kind",
            "from_sample",
            "to_sample",
            "limit",
            "mask",
            "after_row",
        ];
        let o = p
            .as_object()
            .ok_or_else(|| "params: expected an object".to_owned())?;
        for k in o.keys() {
            if !ALLOWED.contains(&k.as_str()) {
                return Err(format!(
                    "unknown parameter {k:?} (allowed: {})",
                    ALLOWED.join(", ")
                ));
            }
        }
        let capture_id = o
            .get("capture_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "capture_id: expected string".to_owned())?;
        if !valid_capture_id(capture_id) {
            return Err(format!("capture_id {capture_id:?}: invalid"));
        }
        let kind = match o.get("kind").and_then(Value::as_str) {
            Some("edges") => ReadKind::Edges,
            Some("annotations") => ReadKind::Annotations,
            _ => return Err("kind: expected \"edges\" or \"annotations\"".to_owned()),
        };
        let opt_u64 = |k: &str| -> Result<Option<u64>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .map(Some)
                    .ok_or_else(|| format!("{k}: expected non-negative integer")),
            }
        };
        let from_sample = opt_u64("from_sample")?;
        let to_sample = opt_u64("to_sample")?;
        if let (Some(f), Some(t)) = (from_sample, to_sample)
            && t <= f
        {
            return Err("to_sample: must be greater than from_sample".to_owned());
        }
        let limit_max = if kind == ReadKind::Edges {
            READ_EDGES_MAX
        } else {
            READ_ROWS_MAX
        };
        let limit = match opt_u64("limit")? {
            None => None,
            Some(n) if n >= 1 && n <= limit_max as u64 => Some(n as usize),
            Some(_) => return Err(format!("limit: expected integer 1..={limit_max}")),
        };
        let mask = match opt_u64("mask")? {
            None => None,
            Some(_) if kind != ReadKind::Edges => return Err("mask: only valid for kind=edges".to_owned()),
            Some(n) => match u8::try_from(n) {
                Ok(m) if m >= 1 => Some(m),
                _ => return Err("mask: expected integer 1..=255".to_owned()),
            },
        };
        let after_row = opt_u64("after_row")?;
        if after_row.is_some() && kind != ReadKind::Annotations {
            return Err("after_row: only valid for kind=annotations".to_owned());
        }
        Ok(ReadReq {
            capture_id: capture_id.to_owned(),
            kind,
            from_sample,
            to_sample,
            limit,
            mask,
            after_row,
        })
    }
    /// B2-Q2. Normalized params (round-trips through `from_params`).
    pub fn to_params(&self) -> Value {
        let mut m = Map::new();
        m.insert("capture_id".to_owned(), serde_json::json!(self.capture_id));
        let kind = match self.kind {
            ReadKind::Edges => "edges",
            ReadKind::Annotations => "annotations",
        };
        m.insert("kind".to_owned(), serde_json::json!(kind));
        if let Some(v) = self.from_sample {
            m.insert("from_sample".to_owned(), serde_json::json!(v));
        }
        if let Some(v) = self.to_sample {
            m.insert("to_sample".to_owned(), serde_json::json!(v));
        }
        if let Some(v) = self.limit {
            m.insert("limit".to_owned(), serde_json::json!(v));
        }
        if let Some(v) = self.mask {
            m.insert("mask".to_owned(), serde_json::json!(v));
        }
        if let Some(v) = self.after_row {
            m.insert("after_row".to_owned(), serde_json::json!(v));
        }
        Value::Object(m)
    }
}

/// B2-Q3. `logic.archive.list`: newest first (`started_wall_ns`), ≤ [`LIST_MAX`]:
/// `{"root":…,"archives":[{"capture_id","state","complete","started_wall_ns","samplerate","decoders",
/// "raw_end_sample","acquired_samples","annotation_rows","bytes","raw_truncated_at_sample",
/// "annotation_truncated_at_row","error"}],"left_out":N}`. Missing root → empty list.
pub fn list(root: &Path) -> Value {
    let mut found: Vec<(u64, String, Manifest, u64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !valid_capture_id(&name) {
                continue;
            }
            let path = entry.path();
            let is_dir = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_dir())
                .unwrap_or(false);
            if !is_dir {
                continue;
            }
            if let Ok(m) = Manifest::read(&path) {
                let bytes = dir_bytes(&path);
                found.push((m.started_wall_ns, name, m, bytes));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let total = found.len();
    let archives: Vec<Value> = found
        .iter()
        .take(LIST_MAX)
        .map(|(_, id, m, bytes)| {
            serde_json::json!({
                "capture_id": id,
                "state": m.state.as_str(),
                "complete": m.complete(),
                "started_wall_ns": m.started_wall_ns,
                "samplerate": m.samplerate,
                "decoders": m.decoders,
                "raw_end_sample": m.raw_end_sample,
                "acquired_samples": m.acquired_samples,
                "annotation_rows": m.ann_rows,
                "bytes": bytes,
                "raw_truncated_at_sample": m.raw_truncated_at_sample,
                "annotation_truncated_at_row": m.ann_truncated_at_row,
                "error": m.raw_error.clone().or_else(|| m.ann_error.clone()),
            })
        })
        .collect();
    let left_out = total - archives.len();
    serde_json::json!({"root": root.to_string_lossy(), "archives": archives, "left_out": left_out})
}

/// B2-Q4. `logic.archive.read`. Edges: chunks in the `logic.edges` payload shape plus
/// `"seq":null,"source":"archive"`, ≤ `model::MAX_EDGES_PER_EVENT` edges each, `dropped:0`,
/// covering `[from, min(to, raw_end_sample))` with no hole; values masked; a chunk ends early when
/// `limit` edges are reached (then `next_from` = that chunk's `sample_end`). Annotations: rows via
/// [`ann_row`] with `start_sample` in `[from, to)` after `after_row`, ≤ limit, ≤ READ_BYTES.
/// Result: `{"capture_id","kind","samplerate","archive_state","range":{"requested":[from,to],
/// "returned":[a,b]},"chunks"|"rows":[…],"next_from"|"next_row":…|null,"complete":bool,
/// "truncated_at":…|null,"scanned_bytes":…}`. `complete` = the whole requested range is covered
/// by this result AND the archive part is not truncated before `to` AND the archive is finalized.
/// Unknown capture → E_NOT_FOUND; range outside coverage → empty result, `complete:false`.
pub fn read(root: &Path, req: &ReadReq) -> Result<Value, ArchiveError> {
    use std::io::{BufRead, Read, Seek, SeekFrom};
    let (dir, m) = capture_dir(root, &req.capture_id)?;
    let recording = m.state == ArchiveState::Recording;
    let internal = |msg: String| ArchiveError {
        code: ErrorCode::Internal,
        message: msg,
    };
    match req.kind {
        ReadKind::Edges => {
            let idx = read_raw_index(&dir)?;
            let mask = req.mask.unwrap_or(0xFF);
            let first_start = idx.first().map_or(m.raw_first_sample, |e| e.start_sample);
            let last_end = match idx.last() {
                None => m.raw_first_sample,
                Some(l) if recording && m.raw_end_sample < l.start_sample => l.start_sample,
                Some(l) => m.raw_end_sample.max(l.start_sample),
            };
            let from = req.from_sample.unwrap_or(m.raw_first_sample).max(first_start);
            let to = req
                .to_sample
                .unwrap_or_else(|| m.acquired_samples.unwrap_or(0).max(last_end));
            let cover_end = to.min(last_end);
            let limit = req.limit.unwrap_or(READ_EDGES_DEFAULT).clamp(1, READ_EDGES_MAX);
            let mk = |start: u64, end: u64, initial: u8, fin: u8, active: u8, edges: Vec<Value>| {
                serde_json::json!({
                    "capture_id": req.capture_id,
                    "seq": Value::Null,
                    "source": "archive",
                    "samplerate": m.samplerate,
                    "sample_start": start,
                    "sample_end": end,
                    "mask": mask,
                    "initial": initial,
                    "final": fin,
                    "active": active,
                    "edges": edges,
                    "dropped": 0,
                })
            };
            let mut chunks: Vec<Value> = Vec::new();
            let mut next_from: Option<u64> = None;
            let mut scanned: u64 = 0;
            let mut returned_end = from;
            if !idx.is_empty() && from < cover_end {
                let bi = idx.partition_point(|e| e.start_sample <= from).saturating_sub(1);
                let mut file = std::fs::File::open(dir.join(RAW_FILE))
                    .map_err(|e| internal(format!("archive raw open: {e}")))?;
                file.seek(SeekFrom::Start(idx[bi].offset))
                    .map_err(|e| internal(format!("archive raw seek: {e}")))?;
                let mut rd = std::io::BufReader::new(file);
                let mut cur_m = 0u8;
                let mut chunk_start = from;
                let mut chunk_initial = 0u8;
                let mut have_initial = false;
                let mut cedges: Vec<Value> = Vec::new();
                let mut active = 0u8;
                let mut total = 0usize;
                // (sample_end of the last chunk, next_from) once this page is complete
                let mut stop: Option<(u64, u64)> = None;
                // limit reached after the edge at this sample: the page ends before the first edge that is
                // not adjacent to it, so `next_from = last + 1` can never skip an edge at that sample
                let mut want_stop: Option<u64> = None;
                let mut bidx = bi;
                'blocks: while bidx < idx.len() {
                    let e = idx[bidx];
                    if e.start_sample >= cover_end {
                        break;
                    }
                    let block_end = idx.get(bidx + 1).map_or(last_end, |n| n.start_sample);
                    let mut hdr = [0u8; BLOCK_HEADER_BYTES];
                    rd.read_exact(&mut hdr).map_err(|er| {
                        internal(format!("archive block read at sample {}: {er}", e.start_sample))
                    })?;
                    let plen = u32::from_le_bytes([hdr[13], hdr[14], hdr[15], hdr[16]]) as usize;
                    if plen > (1 << 22) {
                        return Err(internal(format!(
                            "archive block corrupt at sample {}",
                            e.start_sample
                        )));
                    }
                    let mut buf = vec![0u8; BLOCK_HEADER_BYTES + plen];
                    buf[..BLOCK_HEADER_BYTES].copy_from_slice(&hdr);
                    rd.read_exact(&mut buf[BLOCK_HEADER_BYTES..]).map_err(|er| {
                        internal(format!("archive block read at sample {}: {er}", e.start_sample))
                    })?;
                    scanned += buf.len() as u64;
                    let (blk, _) = decode_block(&buf)?;
                    if blk.start_sample != e.start_sample {
                        return Err(internal(format!(
                            "archive block corrupt at sample {}",
                            e.start_sample
                        )));
                    }
                    // events of this block; a block's own `state` is the transition AT its start sample
                    // (it is not a record) whenever it differs from the previous block's last value
                    let mut evs: Vec<(u64, u8)> = Vec::with_capacity(blk.edges.len() + 1);
                    if have_initial {
                        evs.push((blk.start_sample, blk.state));
                    } else {
                        let mut v = blk.state;
                        for &(s, val) in &blk.edges {
                            if s <= from {
                                v = val;
                            } else {
                                break;
                            }
                        }
                        cur_m = v & mask;
                        chunk_initial = cur_m;
                        have_initial = true;
                    }
                    evs.extend(blk.edges.iter().copied());
                    for &(s, val) in &evs {
                        if s <= from {
                            continue;
                        }
                        if s >= cover_end {
                            break 'blocks;
                        }
                        let mv = val & mask;
                        if mv == cur_m {
                            continue;
                        }
                        if let Some(last) = want_stop {
                            if s != last + 1 {
                                stop = Some((last + 1, last + 1));
                                break 'blocks;
                            }
                            if total >= limit + crate::model::MAX_EDGES_PER_EVENT {
                                // pathological every-sample toggling: end here, the next page re-reads sample `last`
                                stop = Some((last + 1, last));
                                break 'blocks;
                            }
                        }
                        active |= mv ^ cur_m;
                        cur_m = mv;
                        cedges.push(serde_json::json!([s, mv]));
                        total += 1;
                        if cedges.len() >= crate::model::MAX_EDGES_PER_EVENT {
                            chunks.push(mk(
                                chunk_start,
                                s + 1,
                                chunk_initial,
                                cur_m,
                                active,
                                std::mem::take(&mut cedges),
                            ));
                            returned_end = s + 1;
                            chunk_start = s + 1;
                            chunk_initial = cur_m;
                            active = 0;
                        }
                        if want_stop.is_some() || total >= limit {
                            want_stop = Some(s);
                        }
                    }
                    bidx += 1;
                    if scanned >= READ_SCAN_BYTES && bidx < idx.len() && idx[bidx].start_sample < cover_end {
                        // the next block's state is a transition at block_end when it differs from `cur_m`
                        let next = if idx[bidx].state & mask != cur_m {
                            block_end - 1
                        } else {
                            block_end
                        };
                        stop = Some((block_end, next));
                        break;
                    }
                }
                let end_pos = stop.map_or(cover_end, |t| t.0);
                if chunk_start < end_pos {
                    chunks.push(mk(chunk_start, end_pos, chunk_initial, cur_m, active, cedges));
                    returned_end = end_pos;
                }
                next_from = stop.map(|t| t.1);
            }
            let complete = next_from.is_none()
                && !idx.is_empty()
                && from < cover_end
                && cover_end == to
                && match m.raw_truncated_at_sample {
                    None => true,
                    Some(t) => to <= t,
                }
                && !recording;
            Ok(serde_json::json!({
                "capture_id": req.capture_id,
                "kind": "edges",
                "samplerate": m.samplerate,
                "archive_state": m.state.as_str(),
                "range": {"requested": [from, to], "returned": [from, returned_end]},
                "chunks": chunks,
                "next_from": next_from,
                "complete": complete,
                "truncated_at": m.raw_truncated_at_sample,
                "scanned_bytes": scanned,
            }))
        }
        ReadKind::Annotations => {
            let aidx = read_ann_index(&dir);
            let limit = req.limit.unwrap_or(READ_ROWS_DEFAULT).clamp(1, READ_ROWS_MAX);
            let start_row = req.after_row.map_or(0, |r| r.saturating_add(1));
            let (mut row, off) = aidx
                .iter()
                .rev()
                .find(|(r, _)| *r <= start_row)
                .copied()
                .unwrap_or((0, 0));
            let filtered = req.from_sample.is_some() || req.to_sample.is_some();
            let lo = req.from_sample.unwrap_or(0);
            let hi = req.to_sample.unwrap_or(u64::MAX);
            let mut rows: Vec<Value> = Vec::new();
            let mut malformed = 0u64;
            let mut scanned = 0u64;
            let mut bytes = 0usize;
            let mut next_row: Option<u64> = None;
            let mut last_start: Option<u64> = None;
            if let Ok(mut file) = std::fs::File::open(dir.join(ANN_FILE)) {
                file.seek(SeekFrom::Start(off))
                    .map_err(|e| internal(format!("archive annotations seek: {e}")))?;
                let mut rd = std::io::BufReader::new(file);
                let mut line = String::new();
                loop {
                    line.clear();
                    let n = match rd.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => return Err(internal(format!("archive annotations read: {e}"))),
                    };
                    if !line.ends_with('\n') {
                        break; // partial last line (writer still appending)
                    }
                    scanned += n as u64;
                    let this_row = row;
                    row += 1;
                    if this_row < start_row {
                        continue;
                    }
                    match ann_row(line.trim_end(), this_row, m.samplerate) {
                        None => malformed += 1,
                        Some(v) => {
                            let keep = if filtered {
                                v.get("start_sample")
                                    .and_then(Value::as_u64)
                                    .is_some_and(|s| s >= lo && s < hi)
                            } else {
                                true
                            };
                            if keep {
                                bytes += v.to_string().len();
                                last_start = v.get("start_sample").and_then(Value::as_u64).or(last_start);
                                rows.push(v);
                            }
                        }
                    }
                    if rows.len() >= limit || bytes >= READ_BYTES || scanned >= READ_SCAN_BYTES {
                        next_row = Some(this_row);
                        break;
                    }
                }
            }
            let from_eff = req.from_sample.unwrap_or(m.raw_first_sample);
            let to_eff = req
                .to_sample
                .unwrap_or_else(|| m.acquired_samples.unwrap_or(0).max(m.raw_end_sample));
            let returned_end = if next_row.is_none() {
                to_eff
            } else {
                last_start.map_or(from_eff, |s| s.max(from_eff))
            };
            let complete =
                next_row.is_none() && m.ann_truncated_at_row.is_none() && m.ann_error.is_none() && !recording;
            Ok(serde_json::json!({
                "capture_id": req.capture_id,
                "kind": "annotations",
                "samplerate": m.samplerate,
                "archive_state": m.state.as_str(),
                "range": {"requested": [from_eff, to_eff], "returned": [from_eff, returned_end]},
                "rows": rows,
                "next_row": next_row,
                "complete": complete,
                "truncated_at": m.ann_truncated_at_row,
                "malformed": malformed,
                "scanned_bytes": scanned,
            }))
        }
    }
}

/// `logic.export` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReq {
    /// Capture id.
    pub capture_id: String,
    /// `edges` (csv only) | `annotations` (csv|txt).
    pub kind: ReadKind,
    /// `csv` | `txt`.
    pub format: ExportFormat,
    /// Inclusive start sample.
    pub from_sample: Option<u64>,
    /// Exclusive end sample.
    pub to_sample: Option<u64>,
}

/// Export file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// RFC 4180 CSV preceded by `# key=value` metadata lines.
    Csv,
    /// Annotations only: `<t_start_s> <decoder>: <text>` per line, same metadata lines.
    Txt,
}

impl ExportReq {
    /// B2-E1. Strict params: `capture_id`, `kind`, `format` (required), `from_sample`, `to_sample`;
    /// edges + txt → Err.
    pub fn from_params(p: &Value) -> Result<ExportReq, String> {
        const ALLOWED: [&str; 5] = ["capture_id", "kind", "format", "from_sample", "to_sample"];
        let o = p
            .as_object()
            .ok_or_else(|| "params: expected an object".to_owned())?;
        for k in o.keys() {
            if !ALLOWED.contains(&k.as_str()) {
                return Err(format!(
                    "unknown parameter {k:?} (allowed: {})",
                    ALLOWED.join(", ")
                ));
            }
        }
        let capture_id = o
            .get("capture_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "capture_id: expected string".to_owned())?;
        if !valid_capture_id(capture_id) {
            return Err(format!("capture_id {capture_id:?}: invalid"));
        }
        let kind = match o.get("kind").and_then(Value::as_str) {
            Some("edges") => ReadKind::Edges,
            Some("annotations") => ReadKind::Annotations,
            _ => return Err("kind: expected \"edges\" or \"annotations\"".to_owned()),
        };
        let format = match o.get("format").and_then(Value::as_str) {
            Some("csv") => ExportFormat::Csv,
            Some("txt") => ExportFormat::Txt,
            _ => return Err("format: expected \"csv\" or \"txt\"".to_owned()),
        };
        if kind == ReadKind::Edges && format == ExportFormat::Txt {
            return Err("format: edges can only be exported as csv".to_owned());
        }
        let opt_u64 = |k: &str| -> Result<Option<u64>, String> {
            match o.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .map(Some)
                    .ok_or_else(|| format!("{k}: expected non-negative integer")),
            }
        };
        let from_sample = opt_u64("from_sample")?;
        let to_sample = opt_u64("to_sample")?;
        if let (Some(f), Some(t)) = (from_sample, to_sample)
            && t <= f
        {
            return Err("to_sample: must be greater than from_sample".to_owned());
        }
        Ok(ExportReq {
            capture_id: capture_id.to_owned(),
            kind,
            format,
            from_sample,
            to_sample,
        })
    }
    /// B2-E2. Normalized params.
    pub fn to_params(&self) -> Value {
        let mut m = Map::new();
        m.insert("capture_id".to_owned(), serde_json::json!(self.capture_id));
        m.insert(
            "kind".to_owned(),
            serde_json::json!(match self.kind {
                ReadKind::Edges => "edges",
                ReadKind::Annotations => "annotations",
            }),
        );
        m.insert(
            "format".to_owned(),
            serde_json::json!(match self.format {
                ExportFormat::Csv => "csv",
                ExportFormat::Txt => "txt",
            }),
        );
        if let Some(v) = self.from_sample {
            m.insert("from_sample".to_owned(), serde_json::json!(v));
        }
        if let Some(v) = self.to_sample {
            m.insert("to_sample".to_owned(), serde_json::json!(v));
        }
        Value::Object(m)
    }
}

/// B2-E3. Write `<root>/<id>/export-<kind>-<from>-<to>-<k>.<ext>` (`k` = first free 1..=999) and
/// return `{"path": <absolute>, "bytes", "rows", "complete", "range":{"requested","returned"},
/// "archive_state", "truncated_at", "limit_hit": bool}`. Metadata lines (exact keys, in order):
/// `# loom logic export`, `# capture_id=…`, `# kind=…`, `# samplerate=…`, `# from_sample=…`,
/// `# to_sample=…`, `# archive_state=…`, `# complete=true|false`, `# truncated_at=…|none`.
/// CSV headers: annotations `start_sample,end_sample,t_start_s,t_end_s,decoder,text`;
/// edges `sample,time_s,D0,D1,D2,D3,D4,D5,D6,D7` (first row = state at from_sample, then one row per
/// transition). Stops at EXPORT_MAX_BYTES / EXPORT_MAX_ROWS (`limit_hit`, complete false). The file
/// counts toward the capture quota (refused with E_USAGE "archive quota" when it would exceed it).
pub fn export(root: &Path, req: &ExportReq, max_capture_bytes: u64) -> Result<Value, ArchiveError> {
    use std::io::Write;
    let (dir, m) = capture_dir(root, &req.capture_id)?;
    let usage = |msg: String| ArchiveError {
        code: ErrorCode::Usage,
        message: msg,
    };
    let internal = |msg: String| ArchiveError {
        code: ErrorCode::Internal,
        message: msg,
    };
    let used0 = dir_bytes(&dir);
    if used0 >= max_capture_bytes {
        return Err(usage(format!(
            "archive quota {} MiB reached; export refused",
            max_capture_bytes >> 20
        )));
    }
    if req.kind == ReadKind::Edges && req.format == ExportFormat::Txt {
        return Err(usage("format: edges can only be exported as csv".to_owned()));
    }
    let from_eff = req.from_sample.unwrap_or(m.raw_first_sample);
    let to_eff = req
        .to_sample
        .unwrap_or_else(|| m.acquired_samples.unwrap_or(0).max(m.raw_end_sample));
    // rows + the metadata header must fit the remaining capture quota and the export bound
    // (H3(dir) + part bytes + header ≤ quota; 1024 bytes are reserved for the header)
    let budget = EXPORT_MAX_BYTES.min(max_capture_bytes - used0);
    let row_budget = budget.saturating_sub(1024);
    let kind_name = match req.kind {
        ReadKind::Edges => "edges",
        ReadKind::Annotations => "annotations",
    };
    let ext = match req.format {
        ExportFormat::Csv => "csv",
        ExportFormat::Txt => "txt",
    };
    // first free k: neither the final file nor its part exists; the part is created exclusively
    let mut picked: Option<(PathBuf, PathBuf, std::fs::File)> = None;
    for k in 1..=999u32 {
        let fin = dir.join(format!("export-{kind_name}-{from_eff}-{to_eff}-{k}.{ext}"));
        if fin.exists() {
            continue;
        }
        let part = dir.join(format!("export-{kind_name}-{from_eff}-{to_eff}-{k}.{ext}.part"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
        {
            Ok(f) => {
                picked = Some((fin, part, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(internal(format!("export create: {e}"))),
        }
    }
    let Some((final_path, part_path, part_file)) = picked else {
        return Err(usage("too many exports".to_owned()));
    };
    let mut final_created = false;
    let result = (|| -> Result<Value, ArchiveError> {
        let mut w = std::io::BufWriter::with_capacity(1 << 20, part_file);
        let mut body_bytes = 0u64;
        let mut rows = 0u64;
        let mut limit_hit = false;
        let mut complete: bool;
        let mut returned_end = from_eff;
        let mut io_err: Option<std::io::Error> = None;
        // Append one line to the part; false = the row/byte bound was hit (nothing written) or I/O failed.
        let mut put = |line: &str, count_row: bool, hit: &mut bool| -> bool {
            if io_err.is_some() {
                return false;
            }
            if count_row && (body_bytes + line.len() as u64 > row_budget || rows >= EXPORT_MAX_ROWS) {
                *hit = true;
                return false;
            }
            match w.write_all(line.as_bytes()) {
                Ok(()) => {
                    body_bytes += line.len() as u64;
                    if count_row {
                        rows += 1;
                    }
                    true
                }
                Err(e) => {
                    io_err = Some(e);
                    false
                }
            }
        };
        match req.kind {
            ReadKind::Edges => {
                let _ = put("sample,time_s,D0,D1,D2,D3,D4,D5,D6,D7\n", false, &mut limit_hit);
                let bits = |v: u64| {
                    (0..8)
                        .map(|i| ((v >> i) & 1).to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let mut cur_from = from_eff;
                let mut first_chunk = true;
                'pages: loop {
                    let r = read(
                        root,
                        &ReadReq {
                            capture_id: req.capture_id.clone(),
                            kind: ReadKind::Edges,
                            from_sample: Some(cur_from),
                            to_sample: Some(to_eff),
                            limit: Some(READ_EDGES_MAX),
                            mask: Some(0xFF),
                            after_row: None,
                        },
                    )?;
                    complete = r["complete"].as_bool().unwrap_or(false);
                    for ch in r["chunks"].as_array().map_or(&[][..], Vec::as_slice) {
                        if first_chunk {
                            first_chunk = false;
                            let s = ch["sample_start"].as_u64().unwrap_or(cur_from);
                            let line = format!(
                                "{s},{},{}\n",
                                fmt_seconds(s, m.samplerate),
                                bits(ch["initial"].as_u64().unwrap_or(0))
                            );
                            if !put(&line, false, &mut limit_hit) {
                                break 'pages;
                            }
                        }
                        for e in ch["edges"].as_array().map_or(&[][..], Vec::as_slice) {
                            let (s, v) = (e[0].as_u64().unwrap_or(0), e[1].as_u64().unwrap_or(0));
                            let line = format!("{s},{},{}\n", fmt_seconds(s, m.samplerate), bits(v));
                            if !put(&line, true, &mut limit_hit) {
                                break 'pages;
                            }
                        }
                        returned_end = ch["sample_end"].as_u64().unwrap_or(returned_end);
                    }
                    match r["next_from"].as_u64() {
                        Some(n) if n > cur_from => cur_from = n,
                        _ => break,
                    }
                }
            }
            ReadKind::Annotations => {
                if req.format == ExportFormat::Csv {
                    let _ = put(
                        "start_sample,end_sample,t_start_s,t_end_s,decoder,text\n",
                        false,
                        &mut limit_hit,
                    );
                }
                let mut after: Option<u64> = None;
                'pages: loop {
                    let r = read(
                        root,
                        &ReadReq {
                            capture_id: req.capture_id.clone(),
                            kind: ReadKind::Annotations,
                            from_sample: req.from_sample,
                            to_sample: req.to_sample,
                            limit: Some(READ_ROWS_MAX),
                            mask: None,
                            after_row: after,
                        },
                    )?;
                    complete = r["complete"].as_bool().unwrap_or(false);
                    for row in r["rows"].as_array().map_or(&[][..], Vec::as_slice) {
                        let text = row["text"].as_str().unwrap_or("");
                        let dec = row["decoder"].as_str();
                        let (s, e) = (row["start_sample"].as_u64(), row["end_sample"].as_u64());
                        let ts = s.map(|v| fmt_seconds(v, m.samplerate));
                        let line = match req.format {
                            ExportFormat::Csv => format!(
                                "{},{},{},{},{},{}\n",
                                s.map_or_else(String::new, |v| v.to_string()),
                                e.map_or_else(String::new, |v| v.to_string()),
                                ts.clone().unwrap_or_default(),
                                e.map_or_else(String::new, |v| fmt_seconds(v, m.samplerate)),
                                csv_field(dec.unwrap_or("?")),
                                csv_field(text)
                            ),
                            ExportFormat::Txt => {
                                format!(
                                    "{} {}: {}\n",
                                    ts.clone().unwrap_or_else(|| "?".to_owned()),
                                    dec.unwrap_or("?"),
                                    text
                                )
                            }
                        };
                        if !put(&line, true, &mut limit_hit) {
                            break 'pages;
                        }
                        if let Some(v) = e {
                            returned_end = returned_end.max(v);
                        }
                    }
                    match r["next_row"].as_u64() {
                        Some(n) if after.is_none_or(|a| n > a) => after = Some(n),
                        _ => break,
                    }
                }
                if complete && !limit_hit {
                    returned_end = to_eff;
                }
            }
        }
        if let Some(e) = io_err.take() {
            return Err(internal(format!("export write: {e}")));
        }
        w.flush().map_err(|e| internal(format!("export write: {e}")))?;
        drop(w);
        if limit_hit {
            complete = false;
        }
        let truncated_at = match req.kind {
            ReadKind::Edges => m.raw_truncated_at_sample,
            ReadKind::Annotations => m.ann_truncated_at_row,
        };
        let mut header = String::new();
        header.push_str("# loom logic export\n");
        header.push_str(&format!("# capture_id={}\n", req.capture_id));
        header.push_str(&format!("# kind={kind_name}\n"));
        header.push_str(&format!("# samplerate={}\n", m.samplerate));
        header.push_str(&format!("# from_sample={from_eff}\n"));
        header.push_str(&format!("# to_sample={to_eff}\n"));
        header.push_str(&format!("# archive_state={}\n", m.state.as_str()));
        header.push_str(&format!("# complete={complete}\n"));
        header.push_str(&format!(
            "# truncated_at={}\n",
            truncated_at.map_or_else(|| "none".to_owned(), |t| t.to_string())
        ));
        let mut fin = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&final_path)
            .map_err(|e| internal(format!("export create: {e}")))?;
        final_created = true;
        let mut part =
            std::fs::File::open(&part_path).map_err(|e| internal(format!("export read part: {e}")))?;
        fin.write_all(header.as_bytes())
            .and_then(|()| std::io::copy(&mut part, &mut fin).map(|_| ()))
            .and_then(|()| fin.flush())
            .map_err(|e| internal(format!("export write: {e}")))?;
        let abs = if final_path.is_absolute() {
            final_path.clone()
        } else {
            std::env::current_dir().map_or_else(|_| final_path.clone(), |c| c.join(&final_path))
        };
        Ok(serde_json::json!({
            "path": abs.to_string_lossy(),
            "bytes": header.len() as u64 + body_bytes,
            "rows": rows,
            "complete": complete,
            "range": {"requested": [from_eff, to_eff], "returned": [from_eff, returned_end]},
            "archive_state": m.state.as_str(),
            "truncated_at": truncated_at,
            "limit_hit": limit_hit,
        }))
    })();
    let _ = std::fs::remove_file(&part_path);
    if result.is_err() && final_created {
        let _ = std::fs::remove_file(&final_path);
    }
    result
}

/// Parsed `Map` helper type for strict param parsers.
pub type Params<'a> = &'a Map<String, Value>;

// ---------------------------------------------------------------------------------------------
// Private helpers (declared by the lead; bodies are B2a allowlist items).

/// B2-H1. Append unsigned LEB128.
fn put_varint(out: &mut Vec<u8>, v: u64) {
    let mut v = v;
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

/// B2-H2. Read unsigned LEB128 from the start of `b`: `(value, bytes used)`; `None` when truncated,
/// longer than 10 bytes or overflowing u64.
fn get_varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut val: u64 = 0;
    for (i, &byte) in b.iter().enumerate() {
        if i >= 10 {
            return None;
        }
        let part = u64::from(byte & 0x7f);
        if i == 9 && part > 1 {
            return None;
        }
        val |= part << (7 * i);
        if byte & 0x80 == 0 {
            return Some((val, i + 1));
        }
    }
    None
}

/// B2-H3. Sum of regular-file sizes directly inside `dir` (no recursion, symlinks not followed);
/// unreadable → 0 for that entry.
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut sum = 0u64;
    for e in rd.flatten() {
        let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file {
            sum = sum.saturating_add(e.metadata().map_or(0, |m| m.len()));
        }
    }
    sum
}

/// B2-H4. `sample / samplerate` seconds via integer ns (`sample * 1e9 / rate`, u128), printed with
/// 9 decimals then trailing zeros and a trailing '.' trimmed: 10 @ 1 MHz → "0.00001", 0 → "0",
/// 1_500_000 @ 1 MHz → "1.5".
fn fmt_seconds(sample: u64, samplerate: u64) -> String {
    if samplerate == 0 {
        return "0".to_owned();
    }
    let ns = u128::from(sample) * 1_000_000_000 / u128::from(samplerate);
    let s = format!("{}.{:09}", ns / 1_000_000_000, ns % 1_000_000_000);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// B2-H5. RFC 4180 field: quoted (with `"` doubled) iff it contains `,` `"` `\r` or `\n`.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// B2-H6. Writer thread body (see [`ArchiveSink::open`] / FUNCTIONS §B2a-S): owns the four data
/// files of `dir`, applies messages in order, returns the final manifest after `Finish` (or after
/// the channel closes without Finish → state Partial, raw/ann error "archive writer stopped early").
fn writer_loop(
    dir: PathBuf,
    rx: mpsc::Receiver<SinkMsg>,
    shared: Arc<SinkShared>,
    manifest: Manifest,
) -> Manifest {
    use std::io::Write;
    use std::sync::atomic::Ordering::Relaxed;
    let now_ns = || -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
    };
    let mut manifest = manifest;
    let mut open_err: Option<String> = None;
    let mut open = |name: &str| match std::fs::OpenOptions::new().append(true).open(dir.join(name)) {
        Ok(f) => Some(f),
        Err(e) => {
            if open_err.is_none() {
                open_err = Some(format!("archive write: {e}"));
            }
            None
        }
    };
    let mut raw_f = open(RAW_FILE);
    let mut ridx_f = open(RAW_INDEX);
    let mut ann_f = open(ANN_FILE);
    let mut aidx_f = open(ANN_INDEX);
    let cut_raw = |at: u64, err: String| {
        let mut t = shared
            .trunc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if t.raw_at_sample.is_none() {
            t.raw_at_sample = Some(at);
        }
        if t.raw_error.is_none() {
            t.raw_error = Some(err);
        }
        drop(t);
        shared.raw_truncated.store(true, Relaxed);
    };
    let cut_ann = |at: u64, err: String| {
        let mut t = shared
            .trunc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if t.ann_at_row.is_none() {
            t.ann_at_row = Some(at);
        }
        if t.ann_error.is_none() {
            t.ann_error = Some(err);
        }
        drop(t);
        shared.ann_truncated.store(true, Relaxed);
    };
    let sync = |m: &mut Manifest| {
        m.raw_end_sample = shared.raw_end_sample.load(Relaxed);
        if shared.first_set.load(Relaxed) {
            // B5: the archive starts at the pre-trigger sample (or, never fired, at the acquired end)
            let first = shared.first_sample.load(Relaxed);
            m.raw_first_sample = first;
            if shared.raw_blocks.load(Relaxed) == 0 {
                m.raw_end_sample = first;
            }
        }
        m.raw_bytes = shared.raw_bytes.load(Relaxed);
        m.raw_blocks = shared.raw_blocks.load(Relaxed);
        m.ann_rows = shared.ann_rows.load(Relaxed);
        m.ann_bytes = shared.ann_bytes.load(Relaxed);
        let t = shared
            .trunc
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if m.raw_truncated_at_sample.is_none() {
            m.raw_truncated_at_sample = t.raw_at_sample;
        }
        if m.raw_error.is_none() {
            m.raw_error.clone_from(&t.raw_error);
        }
        if m.ann_truncated_at_row.is_none() {
            m.ann_truncated_at_row = t.ann_at_row;
        }
        if m.ann_error.is_none() {
            m.ann_error.clone_from(&t.ann_error);
        }
    };
    let max = manifest.max_capture_bytes;
    let (mut raw_len, mut ridx_len, mut ann_len, mut aidx_len) = (RAW_HEADER.len() as u64, 0u64, 0u64, 0u64);
    let mut raw_cut = raw_f.is_none() || ridx_f.is_none();
    let mut ann_cut = ann_f.is_none() || aidx_f.is_none();
    if let Some(e) = open_err {
        if raw_cut {
            cut_raw(manifest.raw_first_sample, e.clone());
        }
        if ann_cut {
            cut_ann(0, e);
        }
    }
    let mut next_row = 0u64;
    let mut last_write = std::time::Instant::now();
    loop {
        match rx.recv_timeout(CHECKPOINT) {
            Ok(SinkMsg::Raw(b)) => {
                shared.queued.fetch_sub(b.bytes.len(), Relaxed);
                if !raw_cut {
                    let cost = b.bytes.len() as u64 + INDEX_ENTRY_BYTES as u64;
                    if raw_len + ridx_len + ann_len + aidx_len + cost > max {
                        raw_cut = true;
                        cut_raw(b.start_sample, format!("archive quota {} MiB reached", max >> 20));
                    } else if let (Some(rf), Some(xf)) = (raw_f.as_mut(), ridx_f.as_mut()) {
                        let entry = RawIndexEntry {
                            start_sample: b.start_sample,
                            offset: raw_len,
                            state: b.state,
                            n_records: b.n_records,
                        }
                        .to_bytes();
                        let res = rf
                            .write_all(&b.bytes)
                            .and_then(|()| rf.flush())
                            .and_then(|()| xf.write_all(&entry))
                            .and_then(|()| xf.flush());
                        match res {
                            Ok(()) => {
                                raw_len += b.bytes.len() as u64;
                                ridx_len += INDEX_ENTRY_BYTES as u64;
                                shared.raw_bytes.store(raw_len + ridx_len, Relaxed);
                                shared.raw_blocks.fetch_add(1, Relaxed);
                                shared.raw_end_sample.store(b.end_sample, Relaxed);
                            }
                            Err(e) => {
                                raw_cut = true;
                                cut_raw(b.start_sample, format!("archive write: {e}"));
                            }
                        }
                    }
                }
            }
            Ok(SinkMsg::Ann(row, line)) => {
                shared.queued.fetch_sub(line.len(), Relaxed);
                if !ann_cut && row == next_row {
                    let idx_cost = if row.is_multiple_of(ANN_INDEX_EVERY) {
                        ANN_INDEX_ENTRY_BYTES as u64
                    } else {
                        0
                    };
                    let cost = line.len() as u64 + idx_cost;
                    if raw_len + ridx_len + ann_len + aidx_len + cost > max {
                        ann_cut = true;
                        cut_ann(row, format!("archive quota {} MiB reached", max >> 20));
                    } else if let (Some(af), Some(xf)) = (ann_f.as_mut(), aidx_f.as_mut()) {
                        let mut res: std::io::Result<()> = Ok(());
                        if idx_cost > 0 {
                            let mut e = [0u8; ANN_INDEX_ENTRY_BYTES];
                            e[0..8].copy_from_slice(&row.to_le_bytes());
                            e[8..16].copy_from_slice(&ann_len.to_le_bytes());
                            res = xf.write_all(&e).and_then(|()| xf.flush());
                        }
                        if res.is_ok() {
                            res = af.write_all(&line).and_then(|()| af.flush());
                        }
                        match res {
                            Ok(()) => {
                                aidx_len += idx_cost;
                                ann_len += line.len() as u64;
                                next_row = row + 1;
                                shared.ann_rows.store(next_row, Relaxed);
                                shared.ann_bytes.store(ann_len + aidx_len, Relaxed);
                            }
                            Err(e) => {
                                ann_cut = true;
                                cut_ann(row, format!("archive write: {e}"));
                            }
                        }
                    }
                }
            }
            Ok(SinkMsg::Finish { end_sample, acquired }) => {
                sync(&mut manifest);
                if manifest.raw_truncated_at_sample.is_none() {
                    manifest.raw_end_sample = end_sample;
                    shared.raw_end_sample.store(end_sample, Relaxed);
                }
                manifest.acquired_samples = Some(acquired);
                manifest.state = ArchiveState::Complete;
                if !manifest.complete() {
                    manifest.state = ArchiveState::Partial;
                }
                manifest.updated_wall_ns = now_ns();
                if let Err(e) = manifest.write(&dir) {
                    if manifest.raw_error.is_none() {
                        manifest.raw_error = Some(format!("archive manifest write: {e}"));
                    }
                    manifest.state = ArchiveState::Partial;
                }
                return manifest;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                sync(&mut manifest);
                manifest.state = ArchiveState::Partial;
                if manifest.raw_error.is_none() {
                    manifest.raw_error = Some("archive writer stopped early".to_owned());
                }
                if manifest.ann_error.is_none() {
                    manifest.ann_error = Some("archive writer stopped early".to_owned());
                }
                manifest.updated_wall_ns = now_ns();
                let _ = manifest.write(&dir);
                return manifest;
            }
        }
        if last_write.elapsed() >= CHECKPOINT {
            sync(&mut manifest);
            manifest.updated_wall_ns = now_ns();
            let _ = manifest.write(&dir);
            last_write = std::time::Instant::now();
        }
    }
}

/// B2-H7. `<root>/<id>` after [`valid_capture_id`] (else E_USAGE "capture_id …: invalid") and an
/// existing manifest (else E_NOT_FOUND "no archive for <id>").
fn capture_dir(root: &Path, id: &str) -> Result<(PathBuf, Manifest), ArchiveError> {
    if !valid_capture_id(id) {
        return Err(ArchiveError {
            code: ErrorCode::Usage,
            message: format!("capture_id {id:?}: invalid"),
        });
    }
    let dir = root.join(id);
    if !dir.join(MANIFEST).is_file() {
        return Err(ArchiveError {
            code: ErrorCode::NotFound,
            message: format!("no archive for {id}"),
        });
    }
    match Manifest::read(&dir) {
        Ok(m) => Ok((dir, m)),
        Err(e) => Err(ArchiveError {
            code: ErrorCode::Internal,
            message: format!("archive manifest unreadable: {e}"),
        }),
    }
}

/// B2-H8. `raw.idx` of `dir` via [`parse_raw_index`] (missing file → empty).
fn read_raw_index(dir: &Path) -> Result<Vec<RawIndexEntry>, ArchiveError> {
    match std::fs::read(dir.join(RAW_INDEX)) {
        Ok(bytes) => Ok(parse_raw_index(&bytes)?.0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(ArchiveError {
            code: ErrorCode::Internal,
            message: format!("archive index read: {e}"),
        }),
    }
}

/// B2-H9. `annotations.idx` of `dir` → `(row, byte_offset)` pairs (trailing partial entry ignored).
fn read_ann_index(dir: &Path) -> Vec<(u64, u64)> {
    let Ok(bytes) = std::fs::read(dir.join(ANN_INDEX)) else {
        return Vec::new();
    };
    bytes
        .chunks_exact(ANN_INDEX_ENTRY_BYTES)
        .map(|c| {
            let mut a = [0u8; 8];
            let mut b = [0u8; 8];
            a.copy_from_slice(&c[0..8]);
            b.copy_from_slice(&c[8..16]);
            (u64::from_le_bytes(a), u64::from_le_bytes(b))
        })
        .collect()
}
