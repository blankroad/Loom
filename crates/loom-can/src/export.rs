//! `can.export` (CAN-4): journal `can.frame` rows of one channel → a BLF or ASC file on the daemon
//! machine, plus the `loom-can-export` JSONL provenance sidecar.
//! Contract: work/reports/operator-upgrade/can/FUNCTIONS.md §B4-EXPORT.
//!
//! * Same selection rules as `can.trace.query` (channel, since/until, ids, dir, origins), but only
//!   `can.frame` rows: `can.tx` rows are send requests, not bus frames.
//! * Streaming and bounded: rows are written as they are scanned (no collection), ≤
//!   [`EXPORT_MAX_FRAMES`] frames and ≤ [`EXPORT_MAX_SCANS`] journal scans per call. A cut is reported
//!   (`truncated`, `truncated_reason`, `next_cursor`), never hidden.
//! * One clock per file: the first row's `ts.clock_id` fixes the file; a row from another clock
//!   (daemon restart) ends the file with `truncated_reason: "clock_change"` and `next_cursor` at it.
//! * Journal losses in the range become `Gap` records (count unknown stays unknown), so the file
//!   and the sidecar show where data is missing.
//! * Files: written as `<path>.partial` + `<path>.loom.jsonl.partial` (create_new), fsynced, then
//!   renamed sidecar first and primary last — the same commit order as
//!   `loom_can_export::export_file`. An existing target is refused, never overwritten. On failure
//!   both partials are removed (the journal still holds the data; the export can be repeated).
//! * Default path: a NEW file `loom-can-<channel>-<tag>[-n].<blf|asc>` in the daemon's temp dir.
//!   Explicit `path`: absolute, must not exist. The path is on the DAEMON machine (no download).
//!
//! SCAFFOLD (Opus): constants, params, types, signatures. Bodies are Sonnet batch-4 work.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Duration;

use loom_can_export::{
    ExportFrame, ExportPaths, ExportRecord, Exporter, Format, Gap, GapScope, Metadata, Options, SourceRef,
    Summary, TimeQuality,
};
use loom_domain_api::{BoxFuture, CallCtx, DomainError, Handler, ScanResult};
use loom_proto::cursor::Cursor;
use loom_proto::event::{Dir, GapCount, GapData, Origin, PublishedRecord};
use serde::Deserialize;
use serde_json::Value;

use crate::gateway::{IdArg, Shared};

/// Frames per export file (the call ends with `truncated_reason: "max_frames"` beyond).
pub const EXPORT_MAX_FRAMES: u64 = 1_000_000;
/// Journal scans per call (each ≤ `gateway::MAX_ROWS` rows / `gateway::SCAN_BYTES`); beyond →
/// `truncated_reason: "scan_budget"`.
pub const EXPORT_MAX_SCANS: usize = 4_096;
/// Gap records per file (distinct journal gaps; beyond → counted in `gaps_dropped`).
pub const EXPORT_MAX_GAPS: usize = 1_024;
/// Bound on the whole call (registered `max_wait`).
pub const EXPORT_MAX_WAIT: Duration = Duration::from_secs(600);
/// Fresh-name attempts for a default file before giving up.
pub const DEFAULT_FILE_ATTEMPTS: u32 = 64;
/// `since` when not given: the last 10 minutes (`-10m`), bounded like ETH's default lookback.
pub const EXPORT_DEFAULT_SINCE_MINS: u64 = 10;

/// Output format (`format` param).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Blf,
    Asc,
}

impl ExportFormat {
    /// File extension without the dot: `"blf"` / `"asc"`.
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Blf => "blf",
            ExportFormat::Asc => "asc",
        }
    }
    /// The library format.
    pub fn format(self) -> Format {
        match self {
            ExportFormat::Blf => Format::Blf,
            ExportFormat::Asc => Format::Asc,
        }
    }
}

/// `can.export` params (schema registered in gateway.rs, `additionalProperties: false`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportParams {
    #[serde(default)]
    pub(crate) channel: Option<String>,
    #[serde(default)]
    pub(crate) since: Option<Cursor>,
    #[serde(default)]
    pub(crate) until: Option<Cursor>,
    #[serde(default)]
    pub(crate) ids: Vec<IdArg>,
    #[serde(default)]
    pub(crate) origins: Vec<Origin>,
    #[serde(default)]
    pub(crate) dir: Option<Dir>,
    pub(crate) format: ExportFormat,
    #[serde(default)]
    pub(crate) path: Option<String>,
}

/// Why a call wrote fewer rows than the selection holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    MaxFrames,
    ScanBudget,
    ClockChange,
}

impl Cut {
    /// Wire text of `truncated_reason`.
    pub fn as_str(self) -> &'static str {
        match self {
            Cut::MaxFrames => "max_frames",
            Cut::ScanBudget => "scan_budget",
            Cut::ClockChange => "clock_change",
        }
    }
}

/// The streaming writer: partial files + `Exporter`, committed by rename.
pub struct StagedExport {
    pub paths: ExportPaths,
    exporter: Exporter<BufWriter<File>, BufWriter<File>>,
}

impl std::fmt::Debug for StagedExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StagedExport")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl StagedExport {
    /// Refuse an existing primary/sidecar, create both `.partial` files (create_new) and start the
    /// exporter. On an error after a partial was created, remove what this call created.
    pub fn create(path: &Path, format: Format, options: Options) -> Result<Self, loom_can_export::Error> {
        use loom_can_export::Error;
        use std::fs::{self, OpenOptions};
        use std::io;

        let paths = ExportPaths::new(path)?;
        let absent = |p: &Path| -> Result<(), Error> {
            match fs::symlink_metadata(p) {
                Ok(_) => Err(Error::AlreadyExists),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        };
        absent(&paths.primary)?;
        absent(&paths.sidecar)?;
        let primary = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.primary_partial)?;
        let sidecar = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.sidecar_partial)
        {
            Ok(f) => f,
            Err(e) => {
                let _ = fs::remove_file(&paths.primary_partial);
                return Err(e.into());
            }
        };
        match Exporter::new(
            BufWriter::with_capacity(64 * 1024, primary),
            BufWriter::with_capacity(64 * 1024, sidecar),
            format,
            options,
        ) {
            Ok(exporter) => Ok(Self { paths, exporter }),
            Err(e) => {
                let _ = fs::remove_file(&paths.primary_partial);
                let _ = fs::remove_file(&paths.sidecar_partial);
                Err(e)
            }
        }
    }

    /// Append one record (validation/ordering errors come from the library).
    pub fn push(&mut self, record: &ExportRecord) -> Result<(), loom_can_export::Error> {
        self.exporter.push(record)
    }

    /// Records written so far.
    pub fn summary(&self) -> &Summary {
        self.exporter.summary()
    }

    /// finish → flush → fsync both → re-check targets absent → rename sidecar, then primary.
    pub fn commit(self) -> Result<Summary, loom_can_export::Error> {
        use loom_can_export::Error;
        use std::fs;
        use std::io::{self, Write};

        let Self { paths, exporter } = self;
        let remove_partials = |paths: &ExportPaths| {
            let _ = fs::remove_file(&paths.primary_partial);
            let _ = fs::remove_file(&paths.sidecar_partial);
        };
        let absent = |p: &Path| -> Result<(), Error> {
            match fs::symlink_metadata(p) {
                Ok(_) => Err(Error::AlreadyExists),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        };
        let finished = (|| -> Result<Summary, Error> {
            let (mut primary, mut sidecar, summary) = exporter.finish()?;
            primary.flush()?;
            sidecar.flush()?;
            primary.get_ref().sync_all()?;
            sidecar.get_ref().sync_all()?;
            drop(primary);
            drop(sidecar);
            absent(&paths.primary)?;
            absent(&paths.sidecar)?;
            fs::rename(&paths.sidecar_partial, &paths.sidecar)?;
            if let Err(e) = fs::rename(&paths.primary_partial, &paths.primary) {
                let _ = fs::rename(&paths.sidecar, &paths.sidecar_partial);
                return Err(e.into());
            }
            Ok(summary)
        })();
        if finished.is_err() {
            remove_partials(&paths);
        }
        finished
    }

    /// Drop the writers and remove both `.partial` files (best effort).
    pub fn abort(self) {
        let Self { paths, exporter } = self;
        drop(exporter);
        let _ = std::fs::remove_file(&paths.primary_partial);
        let _ = std::fs::remove_file(&paths.sidecar_partial);
    }
}

/// `[A-Za-z0-9._-]`, ≤ 64 chars, no leading '.', never empty (`"x"`).
pub fn safe_label(s: &str) -> String {
    let mapped: String = s
        .chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = mapped.trim_start_matches('.');
    if trimmed.is_empty() {
        "x".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Output path. Explicit: absolute and not existing (primary and sidecar), else `E_USAGE`.
/// Default: first `loom-can-<ch>-<tag>[-n].<ext>` in `dir` whose primary, sidecar and both
/// partials are all absent (≤ DEFAULT_FILE_ATTEMPTS names), else `E_INTERNAL`.
pub fn output_path(
    dir: &Path,
    explicit: Option<&str>,
    channel: &str,
    tag: &str,
    format: ExportFormat,
) -> Result<PathBuf, DomainError> {
    use crate::gateway::usage;
    use loom_proto::error::ErrorCode;

    let exists = |p: &Path| std::fs::symlink_metadata(p).is_ok();
    let with_suffix = |p: &Path, suffix: &str| {
        let mut name = p.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    if let Some(p) = explicit {
        let path = PathBuf::from(p);
        if !path.is_absolute() {
            return Err(usage(format!(
                "export path must be absolute (daemon machine): {p}"
            )));
        }
        if exists(&path) || exists(&with_suffix(&path, ".loom.jsonl")) {
            return Err(usage(format!("export path exists, refusing to overwrite: {p}")));
        }
        return Ok(path);
    }
    let stem = format!("loom-can-{}-{}", safe_label(channel), safe_label(tag));
    let ext = format.ext();
    for n in 0..DEFAULT_FILE_ATTEMPTS {
        let name = if n == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let path = dir.join(name);
        let taken = exists(&path)
            || exists(&with_suffix(&path, ".loom.jsonl"))
            || exists(&with_suffix(&path, ".partial"))
            || exists(&with_suffix(&path, ".loom.jsonl.partial"));
        if !taken {
            return Ok(path);
        }
    }
    Err(DomainError::new(
        ErrorCode::Internal,
        format!(
            "{DEFAULT_FILE_ATTEMPTS} names taken for {stem}*.{ext} in {}",
            dir.display()
        ),
    ))
}

/// Journal origin → library origin.
pub fn export_origin(o: Origin) -> loom_can_export::Origin {
    match o {
        Origin::Agent => loom_can_export::Origin::Agent,
        Origin::User => loom_can_export::Origin::User,
        Origin::Device => loom_can_export::Origin::Device,
        Origin::Script => loom_can_export::Origin::Script,
    }
}

/// `can.frame` row → `ExportRecord::Frame`; None when the row is not a readable frame.
pub fn frame_record(rec: &PublishedRecord) -> Option<ExportRecord> {
    use crate::schema::FramePayload;
    use loom_can_frame::Direction;

    if rec.kind != crate::KIND_FRAME {
        return None;
    }
    let fp: FramePayload = serde_json::from_value(rec.payload.clone()).ok()?;
    let frame = fp.to_frame().ok()?;
    let dir = match rec.dir {
        Some(Dir::Tx) => Direction::Tx,
        _ => Direction::Rx,
    };
    let wall = rec.ts.wall_ns.get();
    let label_ok = |l: &str| !l.is_empty() && l.len() <= loom_can_export::MAX_LABEL_BYTES;
    let mut out = ExportFrame::new(rec.ts.mono_ns.get(), 0, dir, frame, export_origin(rec.origin));
    out.wall_ns = (wall > 0).then_some(wall);
    out.metadata = Metadata {
        request_id: rec.request_id.clone().filter(|r| label_ok(r)),
        source: label_ok(&rec.producer_id).then(|| SourceRef {
            producer_id: rec.producer_id.clone(),
            producer_epoch: rec.producer_epoch.get(),
            source_seq: rec.source_seq.get(),
        }),
        time_quality: TimeQuality::Exact,
    };
    Some(ExportRecord::Frame(out))
}

/// Journal gap → `ExportRecord::Gap` at `at_mono_ns`.
pub fn gap_record(g: &GapData, at_mono_ns: u64) -> ExportRecord {
    let scope = if g.seq_range.is_none() && g.producer_range.is_some() {
        GapScope::Source
    } else {
        GapScope::Record
    };
    let count = match g.count {
        GapCount::Exact(n) if n.get() > 0 => Some(n.get()),
        _ => None,
    };
    let mut end = g.cause.len().min(256);
    while !g.cause.is_char_boundary(end) {
        end -= 1;
    }
    let reason = if end == 0 {
        "unknown".to_owned()
    } else {
        g.cause[..end].to_owned()
    };
    ExportRecord::Gap(Gap {
        ts_mono_ns: at_mono_ns,
        scope,
        count,
        reason,
        metadata: Metadata {
            time_quality: TimeQuality::Exact,
            ..Metadata::default()
        },
    })
}

/// Rows of one scan that belong in the file, in order (kind/ids/dir already applied).
pub fn wanted<'s>(scan: &'s ScanResult, ids: &[(u32, u32)], dir: Option<Dir>) -> Vec<&'s PublishedRecord> {
    scan.events
        .iter()
        .filter(|r| r.kind == crate::KIND_FRAME && crate::gateway::row_matches(r, ids, dir))
        .collect()
}

/// The whole call (see §B4-EXPORT for the algorithm and the result object).
pub(crate) async fn run_export(
    ctx: &mut CallCtx,
    table: &Shared,
    params: Value,
) -> Result<Value, DomainError> {
    use crate::gateway::{
        MAX_ROWS, SCAN_BYTES, frame_filter, next_from, parse, publish_activity, resolve_channel, seq_cursor,
    };
    use crate::{KIND_FRAME, RPC_EXPORT};
    use loom_domain_api::JournalQuery;
    use loom_proto::cursor::TimeUnit;
    use loom_proto::error::ErrorCode;
    use loom_proto::event::SeqRange;
    use loom_proto::num::U64;
    use serde_json::{Map, json};

    let p: ExportParams = parse(params)?;
    let channel = resolve_channel(table, p.channel.as_deref())?;
    let ids = p.ids.iter().map(IdArg::range).collect::<Result<Vec<_>, _>>()?;
    let tag = match ctx.request_id() {
        Some(r) => r.to_owned(),
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
            .to_string(),
    };
    let mut path = output_path(&std::env::temp_dir(), p.path.as_deref(), &channel, &tag, p.format)?;
    let mut from = p.since.clone().unwrap_or(Cursor::Relative {
        amount: EXPORT_DEFAULT_SINCE_MINS,
        unit: TimeUnit::Mins,
    });

    let mut writer: Option<StagedExport> = None;
    let mut clock: Option<String> = None;
    let mut anchor_wall: Option<u64> = None;
    let mut last_mono: u64 = 0;
    let mut frames: u64 = 0;
    let mut first_seq: Option<U64> = None;
    let mut last_seq: Option<U64> = None;
    let mut seen: Vec<GapData> = Vec::new();
    let mut pending: Vec<GapData> = Vec::new();
    let mut gaps_dropped: u64 = 0;
    let mut scans_complete = true;
    let mut cut: Option<Cut> = None;
    let mut next: Option<Cursor> = None;

    let abort = |w: Option<StagedExport>| {
        if let Some(w) = w {
            w.abort();
        }
    };
    let internal = |msg: String| DomainError::new(ErrorCode::Internal, msg);

    'scans: for i in 0..EXPORT_MAX_SCANS {
        let q = JournalQuery {
            from: from.clone(),
            until: p.until.clone(),
            filter: frame_filter(&channel, &[KIND_FRAME], p.origins.clone()),
            request_id: None,
            max_events: MAX_ROWS,
        };
        let scan = match ctx.journal().scan(&q, SCAN_BYTES).await {
            Ok(s) => s,
            Err(e) => {
                abort(writer.take());
                return Err(e);
            }
        };
        scans_complete &= scan.complete;
        for g in &scan.gaps {
            if seen.contains(g) {
                continue;
            }
            if seen.len() < EXPORT_MAX_GAPS {
                seen.push(g.clone());
                pending.push(g.clone());
            } else {
                gaps_dropped += 1;
            }
        }
        for rec in wanted(&scan, &ids, p.dir) {
            if frames == EXPORT_MAX_FRAMES {
                cut = Some(Cut::MaxFrames);
                next = Some(seq_cursor(rec, 0));
                break 'scans;
            }
            if clock.as_ref().is_some_and(|c| *c != rec.ts.clock_id) {
                cut = Some(Cut::ClockChange);
                next = Some(seq_cursor(rec, 0));
                break 'scans;
            }
            let Some(r) = frame_record(rec) else { continue };
            let mono = rec.ts.mono_ns.get();
            if writer.is_none() {
                clock = Some(rec.ts.clock_id.clone());
                let wall = rec.ts.wall_ns.get();
                anchor_wall = (wall > 0).then_some(wall);
                let options = Options {
                    anchor_mono_ns: mono,
                    anchor_wall_ns: anchor_wall,
                    compressed: false,
                };
                // A default name another export took between the pick and the create is re-picked
                // (bounded); an explicit path is never changed.
                let mut attempts = 0;
                loop {
                    match StagedExport::create(&path, p.format.format(), options) {
                        Ok(w) => {
                            writer = Some(w);
                            break;
                        }
                        Err(e) => {
                            let taken = matches!(&e, loom_can_export::Error::AlreadyExists)
                                || matches!(&e, loom_can_export::Error::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists);
                            attempts += 1;
                            if taken && p.path.is_none() && attempts < DEFAULT_FILE_ATTEMPTS {
                                match output_path(&std::env::temp_dir(), None, &channel, &tag, p.format) {
                                    Ok(np) => {
                                        path = np;
                                        continue;
                                    }
                                    Err(e2) => return Err(e2),
                                }
                            }
                            return Err(internal(format!("cannot create {}: {e}", path.display())));
                        }
                    }
                }
            }
            if let Some(w) = writer.as_mut() {
                let mut failed: Option<loom_can_export::Error> = None;
                for g in pending.drain(..) {
                    if let Err(e) = w.push(&gap_record(&g, mono)) {
                        failed = Some(e);
                        break;
                    }
                }
                if failed.is_none() {
                    failed = w.push(&r).err();
                }
                if let Some(e) = failed {
                    abort(writer.take());
                    return Err(internal(format!("export write: {e}")));
                }
            }
            frames += 1;
            last_mono = mono;
            first_seq.get_or_insert(rec.seq);
            last_seq = Some(rec.seq);
        }
        if let Some(w) = writer.as_mut() {
            let mut failed: Option<loom_can_export::Error> = None;
            for g in pending.drain(..) {
                if let Err(e) = w.push(&gap_record(&g, last_mono)) {
                    failed = Some(e);
                    break;
                }
            }
            if let Some(e) = failed {
                abort(writer.take());
                return Err(internal(format!("export write: {e}")));
            }
        }
        let more = scan.resume_from.is_some() || scan.events.len() as u32 == MAX_ROWS;
        match next_from(&scan) {
            Some(n) if more => {
                if i + 1 == EXPORT_MAX_SCANS {
                    cut = Some(Cut::ScanBudget);
                    next = Some(n);
                    break 'scans;
                }
                from = n;
            }
            _ => break 'scans,
        }
    }

    let sidecar = writer
        .as_ref()
        .map(|w| w.paths.sidecar.to_string_lossy().into_owned());
    let path_out = writer.as_ref().map(|_| path.to_string_lossy().into_owned());
    if let Some(w) = writer.take() {
        w.commit().map_err(|e| internal(format!("export commit: {e}")))?;
    }
    let gaps = seen.len() as u64;
    let range = match (first_seq, last_seq) {
        (Some(a), Some(b)) => Some(SeqRange { from: a, to: b }),
        _ => None,
    };
    let mut extra = Map::new();
    extra.insert("action".into(), json!("read"));
    if let Some(r) = range {
        extra.insert("range".into(), json!(r));
    }
    publish_activity(
        ctx,
        RPC_EXPORT,
        &channel,
        format!(
            "export {channel} ({frames} frames) → {}",
            path_out.as_deref().unwrap_or("no file")
        ),
        extra,
    );
    Ok(json!({
        "channel": channel,
        "format": p.format.ext(),
        "location": "daemon",
        "path": path_out,
        "sidecar": sidecar,
        "frames": frames,
        "gaps": gaps,
        "gaps_dropped": gaps_dropped,
        "truncated": cut.is_some(),
        "truncated_reason": cut.map(Cut::as_str),
        "next_cursor": if cut.is_some() { next.map(|c| c.to_string()) } else { None },
        "complete": cut.is_none() && scans_complete && gaps == 0 && gaps_dropped == 0,
        "range": range,
        "clock_id": clock,
        "wall_anchor_known": anchor_wall.is_some(),
    }))
}

/// `can.export` handler (role READ; manifest class `job`, right `watch`).
#[derive(Debug)]
pub(crate) struct ExportH {
    pub(crate) table: Shared,
}

impl Handler for ExportH {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { run_export(ctx, &self.table, params).await })
    }
}
