//! `~/.loom` layout (contract 01 §1.2): single-instance lock, discovery file, latch file, audit dir.
//! `LOOM_HOME` overrides the root (test isolation).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use rand::RngCore as _;
use serde::{Deserialize, Serialize};

/// Root of Loom's per-user state.
#[derive(Debug, Clone)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    /// `LOOM_HOME`, else `~/.loom` (`%USERPROFILE%\.loom` on Windows).
    pub fn from_env() -> io::Result<Home> {
        if let Some(h) = std::env::var_os("LOOM_HOME") {
            return Ok(Home::at(PathBuf::from(h)));
        }
        let base = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "neither LOOM_HOME nor HOME is set"))?;
        Ok(Home::at(PathBuf::from(base).join(".loom")))
    }

    /// Explicit root.
    pub fn at(root: impl Into<PathBuf>) -> Home {
        Home { root: root.into() }
    }

    /// Root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// `run/` (lock, discovery, latch).
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }
    /// `audit/`.
    pub fn audit_dir(&self) -> PathBuf {
        self.root.join("audit")
    }
    /// `run/loomd.lock`.
    pub fn lock_path(&self) -> PathBuf {
        self.run_dir().join("loomd.lock")
    }
    /// `run/daemon.json`.
    pub fn daemon_json(&self) -> PathBuf {
        self.run_dir().join("daemon.json")
    }
    /// `run/latch.json`.
    pub fn latch_json(&self) -> PathBuf {
        self.run_dir().join("latch.json")
    }
    /// `loom.toml` (policy; optional).
    pub fn config(&self) -> PathBuf {
        self.root.join("loom.toml")
    }

    /// Create `run/` and `audit/` (0700 on Unix).
    pub fn ensure(&self) -> io::Result<()> {
        for d in [self.root.clone(), self.run_dir(), self.audit_dir()] {
            fs::create_dir_all(&d)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&d, fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }
}

/// Held for the daemon's lifetime; another `loomd` with the same home fails to start.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Try to take the lock (non-blocking).
    pub fn acquire(home: &Home) -> io::Result<InstanceLock> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.lock_path())?;
        match file.try_lock() {
            Ok(()) => Ok(InstanceLock { _file: file }),
            Err(fs::TryLockError::WouldBlock) => Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another loomd is running for this LOOM_HOME",
            )),
            Err(fs::TryLockError::Error(e)) => Err(e),
        }
    }
}

/// Write `bytes` to `path` atomically (temp file + rename), owner-only (0600) on Unix.
pub fn write_atomic(path: &Path, bytes: &[u8], sync: bool) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut o = OpenOptions::new();
        o.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        if sync {
            f.sync_all()?;
        }
    }
    fs::rename(&tmp, path)
}

/// Discovery file `run/daemon.json` (contract 01 §1.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    /// Bound port on 127.0.0.1.
    pub port: u16,
    /// Daemon pid.
    pub pid: u32,
    /// Agent token (256-bit random, hex).
    pub token: String,
    /// Daemon version.
    pub version: String,
    /// Protocol (`loom/1.x`).
    pub proto: String,
}

impl DaemonInfo {
    /// Write atomically with 0600.
    pub fn write(&self, home: &Home) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(&home.daemon_json(), &bytes, true)
    }

    /// Read.
    pub fn read(home: &Home) -> io::Result<DaemonInfo> {
        serde_json::from_slice(&fs::read(home.daemon_json())?).map_err(io::Error::other)
    }
}

/// `n` random bytes as lowercase hex (OS-seeded CSPRNG).
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

/// Constant-time string comparison (tokens, cookies).
pub fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
