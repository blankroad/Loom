//! Gateway-side listeners for the two lanes of one worker instance (contract 05 §5.1).
//!
//! Unix: the socket directory is created with mode 0700 and the socket files are removed once
//! both lanes are connected (nobody else can connect later). Each accepted peer must run as the
//! same OS user and, when the OS reports it, be the spawned child's pid; other peers are dropped.
//! Windows: two named pipe servers (`first_pipe_instance`, remote clients rejected).

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use loom_proto::worker::Lane;
use loom_worker_sdk::transport::{BoxRead, BoxWrite, validate_instance};

/// One lane's halves.
pub type Halves = (BoxRead, BoxWrite);

/// Create the socket directory (0700 on Unix).
pub fn prepare_sock_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// Listeners for `ctl` and `data` of one worker start.
pub struct LaneListeners {
    #[cfg(unix)]
    ctl: tokio::net::UnixListener,
    #[cfg(unix)]
    data: tokio::net::UnixListener,
    #[cfg(unix)]
    uid: u32,
    #[cfg(windows)]
    ctl: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(windows)]
    data: tokio::net::windows::named_pipe::NamedPipeServer,
    paths: PathGuard,
}

/// Removes the socket files when the listeners (or the accepted pair) are done.
#[derive(Debug)]
struct PathGuard(Vec<PathBuf>);

impl Drop for PathGuard {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl std::fmt::Debug for LaneListeners {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaneListeners")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

impl LaneListeners {
    /// Bind both lanes (stale socket files are replaced).
    #[cfg(unix)]
    pub fn bind(sock_dir: &Path, instance: &str) -> io::Result<Self> {
        use loom_worker_sdk::transport::{MAX_SOCK_PATH, socket_path};
        use std::os::unix::fs::MetadataExt;
        validate_instance(instance).map_err(invalid)?;
        prepare_sock_dir(sock_dir)?;
        let uid = std::fs::metadata(sock_dir)?.uid();
        let ctl_path = socket_path(sock_dir, instance, Lane::Ctl);
        let data_path = socket_path(sock_dir, instance, Lane::Data);
        for p in [&ctl_path, &data_path] {
            if p.as_os_str().len() > MAX_SOCK_PATH {
                return Err(invalid(format!(
                    "socket path too long ({} > {MAX_SOCK_PATH}): {}",
                    p.as_os_str().len(),
                    p.display()
                )));
            }
            match std::fs::remove_file(p) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        let ctl = tokio::net::UnixListener::bind(&ctl_path)?;
        let data = tokio::net::UnixListener::bind(&data_path)?;
        Ok(LaneListeners {
            ctl,
            data,
            uid,
            paths: PathGuard(vec![ctl_path, data_path]),
        })
    }

    /// Bind both lanes (named pipes).
    #[cfg(windows)]
    pub fn bind(_sock_dir: &Path, instance: &str) -> io::Result<Self> {
        use loom_worker_sdk::transport::pipe_name;
        use tokio::net::windows::named_pipe::ServerOptions;
        validate_instance(instance).map_err(invalid)?;
        let mk = |lane| {
            ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(pipe_name(instance, lane))
        };
        Ok(LaneListeners {
            ctl: mk(Lane::Ctl)?,
            data: mk(Lane::Data)?,
            paths: PathGuard(Vec::new()),
        })
    }

    /// Accept `ctl` then `data` within `timeout`.
    #[cfg(unix)]
    pub async fn accept(self, expected_pid: Option<u32>, timeout: Duration) -> io::Result<(Halves, Halves)> {
        let deadline = tokio::time::Instant::now() + timeout;
        let ctl = accept_one(&self.ctl, self.uid, expected_pid, deadline).await?;
        let data = accept_one(&self.data, self.uid, expected_pid, deadline).await?;
        let (cr, cw) = ctl.into_split();
        let (dr, dw) = data.into_split();
        Ok(((Box::new(cr), Box::new(cw)), (Box::new(dr), Box::new(dw))))
    }

    /// Accept `ctl` then `data` within `timeout`.
    #[cfg(windows)]
    pub async fn accept(self, _expected_pid: Option<u32>, timeout: Duration) -> io::Result<(Halves, Halves)> {
        let LaneListeners {
            ctl,
            data,
            paths: _paths,
        } = self;
        let deadline = tokio::time::Instant::now() + timeout;
        tokio::time::timeout_at(deadline, ctl.connect())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ctl connect timeout"))??;
        tokio::time::timeout_at(deadline, data.connect())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "data connect timeout"))??;
        let (cr, cw) = tokio::io::split(ctl);
        let (dr, dw) = tokio::io::split(data);
        Ok(((Box::new(cr), Box::new(cw)), (Box::new(dr), Box::new(dw))))
    }
}

#[cfg(unix)]
async fn accept_one(
    l: &tokio::net::UnixListener,
    uid: u32,
    expected_pid: Option<u32>,
    deadline: tokio::time::Instant,
) -> io::Result<tokio::net::UnixStream> {
    loop {
        let (s, _) = tokio::time::timeout_at(deadline, l.accept())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "worker did not connect in time"))??;
        let cred = s.peer_cred()?;
        let pid_ok = match (expected_pid, cred.pid()) {
            (Some(want), Some(got)) => i64::from(want) == i64::from(got),
            _ => true,
        };
        if cred.uid() == uid && pid_ok {
            return Ok(s);
        }
        tracing::warn!(peer_uid = cred.uid(), peer_pid = ?cred.pid(), "rejected worker socket peer");
    }
}
