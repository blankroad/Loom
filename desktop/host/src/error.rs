//! [`HostError`]: redacted, user-actionable lifecycle errors.
//!
//! No variant ever carries the agent token, a bootstrap code or the viewer URL; `Display` and
//! `Debug` are safe to log and to show in the desktop error screen.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// Why [`crate::connect`] failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum HostError {
    /// The caller passed unusable [`crate::ConnectOptions`].
    InvalidOptions(String),
    /// The Loom home (or its `run/` directory / daemon log) could not be prepared.
    Home {
        /// Path that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// No daemon was running and the configured `loomd` is missing or not executable.
    DaemonBinaryMissing {
        /// Configured daemon binary.
        path: PathBuf,
    },
    /// The configured `loomd` could not be spawned.
    Spawn {
        /// Configured daemon binary.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The daemon this call started exited before it became ready.
    DaemonExited {
        /// Exit status as text (e.g. `exit status: 2`).
        status: String,
        /// `run/loomd.log`.
        log: PathBuf,
    },
    /// The daemon this call started did not become ready in time (it is left running).
    StartupTimeout {
        /// How long we waited.
        waited: Duration,
        /// `run/loomd.log`.
        log: PathBuf,
    },
    /// Another process is starting/running the daemon for this home but it never answered.
    Unresponsive {
        /// How long we waited.
        waited: Duration,
        /// `run/loomd.log`.
        log: PathBuf,
    },
    /// The live daemon for this home refused the agent token from `run/daemon.json`.
    AuthRejected {
        /// `run/daemon.json`.
        discovery: PathBuf,
    },
    /// The live daemon speaks a protocol this app does not support.
    ProtocolMismatch {
        /// Protocol announced in `run/daemon.json` (sanitized).
        found: String,
    },
    /// The daemon answered, but not with a usable bootstrap (detail never contains secrets).
    Protocol(&'static str),
}

impl HostError {
    /// Whether a plain "Retry" (without changing options or reinstalling) may succeed.
    pub fn is_retryable(&self) -> bool {
        !matches!(
            self,
            HostError::InvalidOptions(_)
                | HostError::DaemonBinaryMissing { .. }
                | HostError::ProtocolMismatch { .. }
        )
    }
}

fn secs(d: &Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::InvalidOptions(m) => write!(f, "Loom Desktop is misconfigured: {m}."),
            HostError::Home { path, source } => write!(
                f,
                "Cannot prepare the Loom home at {} ({source}). Check that it exists and is writable by you, then retry.",
                path.display()
            ),
            HostError::DaemonBinaryMissing { path } => write!(
                f,
                "The Loom daemon is not running and the bundled daemon {} is missing or not executable. Reinstall Loom Desktop, or start the daemon with `loom daemon start` and retry.",
                path.display()
            ),
            HostError::Spawn { path, source } => write!(
                f,
                "Cannot start the Loom daemon {} ({source}). Retry, or start it with `loom daemon start`.",
                path.display()
            ),
            HostError::DaemonExited { status, log } => write!(
                f,
                "The Loom daemon exited while starting ({status}). See {} for the reason, then retry.",
                log.display()
            ),
            HostError::StartupTimeout { waited, log } => write!(
                f,
                "The Loom daemon did not become ready within {}. It may still be starting; retry, or see {}.",
                secs(waited),
                log.display()
            ),
            HostError::Unresponsive { waited, log } => write!(
                f,
                "A Loom daemon owns this Loom home but did not answer within {}. It may be busy or starting; retry, or check it with `loom daemon status` (log: {}).",
                secs(waited),
                log.display()
            ),
            HostError::AuthRejected { discovery } => write!(
                f,
                "The running Loom daemon rejected the local credentials in {}. The file may be out of date; retry, or restart the daemon with `loom daemon stop` and retry.",
                discovery.display()
            ),
            HostError::ProtocolMismatch { found } => write!(
                f,
                "The running Loom daemon speaks protocol {found:?}, but this app needs loom/1.x. Update Loom Desktop or the daemon (stop the old one with `loom daemon stop`)."
            ),
            HostError::Protocol(detail) => write!(
                f,
                "The Loom daemon answered unexpectedly ({detail}). Retry; if it persists, restart the daemon with `loom daemon stop`."
            ),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HostError::Home { source, .. } | HostError::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}
