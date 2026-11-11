use crate::stream::source_ref;
use crate::*;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const FILE_BUFFER_BYTES: usize = 64 * 1024;
/// Primary and sidecar names. Deterministic partial names make interrupted work discoverable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPaths {
    pub primary: PathBuf,
    pub sidecar: PathBuf,
    pub primary_partial: PathBuf,
    pub sidecar_partial: PathBuf,
}
impl ExportPaths {
    /// Append suffixes without losing an existing extension or requiring a UTF-8 filename.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        if path.file_name().is_none() || path.as_os_str().len() > 4096 {
            return Err(Error::Invalid("output path"));
        }
        let suffix = |s: &str| {
            let mut name = path.as_os_str().to_os_string();
            name.push(s);
            PathBuf::from(name)
        };
        Ok(Self {
            primary: path.into(),
            sidecar: suffix(".loom.jsonl"),
            primary_partial: suffix(".partial"),
            sidecar_partial: suffix(".loom.jsonl.partial"),
        })
    }
}
/// Failure retains partial files and last staged source coordinates for regeneration.
#[derive(Debug)]
pub struct ExportFailure {
    pub error: Error,
    pub paths: Option<ExportPaths>,
    pub staged: Summary,
}
impl fmt::Display for ExportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "export failed: {}; partials: {:?}", self.error, self.paths)
    }
}
impl std::error::Error for ExportFailure {}
fn absent(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error::AlreadyExists),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
/// Export already validated input frames from a bounded iterator. No history is collected.
pub fn export_frames(
    path: impl AsRef<Path>,
    format: Format,
    options: Options,
    frames: impl IntoIterator<Item = ExportFrame>,
) -> Result<Summary, Box<ExportFailure>> {
    export_file(
        path,
        format,
        options,
        frames.into_iter().map(|frame| Ok(ExportRecord::Frame(frame))),
    )
}
/// Write, finish, fsync, then rename sidecar first and primary last (commit marker).
/// The caller must own/serialize the destination path. This is not a two-file atomic
/// transaction or a cross-process no-clobber primitive. Existing paths are refused.
pub fn export_file(
    path: impl AsRef<Path>,
    format: Format,
    options: Options,
    records: impl IntoIterator<Item = Result<ExportRecord, Error>>,
) -> Result<Summary, Box<ExportFailure>> {
    let paths = ExportPaths::new(path).map_err(|error| {
        Box::new(ExportFailure {
            error,
            paths: None,
            staged: Summary::default(),
        })
    })?;
    let mut staged = Summary::default();
    let mut own_sidecar = false;
    let result = (|| -> Result<Summary, Error> {
        absent(&paths.primary)?;
        absent(&paths.sidecar)?;
        let primary = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.primary_partial)?;
        let sidecar = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&paths.sidecar_partial)?;
        own_sidecar = true;
        let mut exporter = Exporter::new(
            BufWriter::with_capacity(FILE_BUFFER_BYTES, primary),
            BufWriter::with_capacity(FILE_BUFFER_BYTES, sidecar),
            format,
            options,
        )?;
        for record in records {
            let record = match record {
                Ok(record) => record,
                Err(error) => {
                    staged = exporter.summary().clone();
                    return Err(error);
                }
            };
            if let Err(error) = exporter.push(&record) {
                staged = exporter.summary().clone();
                return Err(error);
            }
        }
        staged = exporter.summary().clone();
        let (mut primary, mut sidecar, summary) = exporter.finish()?;
        primary.flush()?;
        sidecar.flush()?;
        primary.get_ref().sync_all()?;
        sidecar.get_ref().sync_all()?;
        drop(primary);
        drop(sidecar);
        // Detect an already-created target again; callers still serialize competing exporters.
        absent(&paths.primary)?;
        absent(&paths.sidecar)?;
        fs::rename(&paths.sidecar_partial, &paths.sidecar)?;
        if let Err(error) = fs::rename(&paths.primary_partial, &paths.primary) {
            // Best effort: return sidecar to partial if the primary commit fails.
            let _ = fs::rename(&paths.sidecar, &paths.sidecar_partial);
            return Err(error.into());
        }
        Ok(summary)
    })();
    match result {
        Ok(summary) => Ok(summary),
        Err(error) => {
            if own_sidecar {
                append_failure(&paths, &staged, &error);
            }
            Err(Box::new(ExportFailure {
                error,
                paths: Some(paths),
                staged,
            }))
        }
    }
}
fn append_failure(paths: &ExportPaths, summary: &Summary, error: &Error) {
    // No input/error message is copied into this bounded recovery row.
    let kind = match error {
        Error::Io(_) => "io",
        Error::Blf(_) => "blf",
        Error::Json(_) => "json",
        Error::Invalid(_) => "invalid",
        Error::OutOfOrder => "out_of_order",
        Error::TimestampOverflow => "timestamp_overflow",
        Error::CountOverflow => "count_overflow",
        Error::RowLimit => "row_limit",
        Error::AlreadyExists => "already_exists",
        Error::Input(_) => "input",
    };
    let row = json!({"kind":"failed","error_class":kind,"staged_records":summary.records.to_string(),"staged_objects":summary.objects.to_string(),
        "first_source":source_ref(summary.first_source.as_ref()),"last_source":source_ref(summary.last_source.as_ref())});
    // All row fields are fixed/bounded labels; the explicit cap is checked before disk append.
    if let Ok(mut bytes) = serde_json::to_vec(&row)
        && bytes.len() < MAX_SIDECAR_LINE
    {
        bytes.push(b'\n');
        if let Ok(mut file) = OpenOptions::new().append(true).open(&paths.sidecar_partial) {
            let _ = file.write_all(&bytes);
            let _ = file.sync_all();
        }
    }
}
