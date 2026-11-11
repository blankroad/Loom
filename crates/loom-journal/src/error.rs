//! Error type of the journal crate.

use std::io;
use std::path::{Path, PathBuf};

/// Errors returned by journal APIs (writer spawn, recovery, scans, retention).
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// File system error on `path`.
    #[error("io error at {path}: {source}")]
    Io {
        /// File or directory involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// Segment or index header is not a valid Loom journal header.
    #[error("bad header in {path}: {reason}")]
    BadHeader {
        /// File involved.
        path: PathBuf,
        /// Why it was rejected.
        reason: String,
    },
    /// Invalid configuration value.
    #[error("invalid journal config: {0}")]
    Config(String),
    /// The writer thread has stopped.
    #[error("journal writer is closed")]
    Closed,
    /// An operation did not finish within its time bound.
    #[error("journal timeout: {0}")]
    Timeout(&'static str),
    /// The writer is in the failed state (disk error); the string is the last error.
    #[error("journal failed: {0}")]
    Failed(String),
}

impl JournalError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> JournalError + '_ {
        move |source| JournalError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}
