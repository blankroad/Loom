//! Loom Desktop host lifecycle (pure Rust, no Tauri).
//!
//! [`connect`] finds the `loomd` that owns a Loom home (the same discovery file the CLI and agents
//! use, `<home>/run/daemon.json`), starts it from an explicit binary only when none is running, and
//! asks it — with the local agent token — for a fresh single-use viewer bootstrap URL.
//!
//! Lifecycle guarantees:
//! - an existing daemon is always reused; a daemon is started only when the home's instance lock
//!   (`run/loomd.lock`) is free, serialized with the CLI through `run/start.lock`;
//! - a started daemon runs in its own process group (Windows: detached, no console, out of the
//!   app's job object when allowed) with stdin null and stdout/stderr appended to
//!   `<home>/run/loomd.log` (0600 on Unix) — no pipes, so closing or killing the app never
//!   blocks on, signals or reaps-into-death the daemon;
//! - nothing here ever stops a daemon: [`DesktopSession`] has no `Drop` behaviour;
//! - the token, the bootstrap code and the viewer URL never appear in `Debug`/`Display` output or
//!   errors, and this crate does not log.

mod error;
mod http;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub use error::HostError;

/// Longest wait for `run/start.lock` before falling back to waiting for whoever holds it.
const START_LOCK_WAIT: Duration = Duration::from_secs(2);
/// Upper bound of a single HTTP probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Poll interval while waiting for a daemon.
const POLL: Duration = Duration::from_millis(25);

/// How to reach (or start) the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectOptions {
    /// Loom home (absolute). Use [`default_home`] to match the CLI.
    pub home: PathBuf,
    /// Exact `loomd` to start when no daemon owns `home` (never searched on `PATH`).
    pub daemon_bin: PathBuf,
    /// `--ui-dir` for a daemon this call starts (must contain `index.html`). Ignored when an
    /// existing daemon is reused. `None` = the daemon's embedded UI.
    pub ui_dir: Option<PathBuf>,
    /// Bound on the whole [`connect`] call.
    pub startup_timeout: Duration,
    /// Environment changes for a daemon this call starts (`None` removes the variable), applied
    /// before `LOOM_HOME`, which is always set to [`ConnectOptions::home`]. Used e.g. to keep a
    /// Linux AppImage's mount-relative variables out of the long-lived daemon. Empty = inherit.
    pub daemon_env: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
}

impl ConnectOptions {
    /// Suggested [`ConnectOptions::startup_timeout`].
    pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
}

/// Loom home as the CLI and `loomd` resolve it: `LOOM_HOME`, else `$HOME/.loom`, else
/// `%USERPROFILE%\.loom` (empty values count as unset here).
pub fn default_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("LOOM_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(h));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".loom"))
}

/// A daemon ready for a viewer, with a freshly minted single-use bootstrap URL.
///
/// Dropping it does nothing to the daemon. The bootstrap code expires quickly and is redeemed
/// once; call [`connect`] again for a new one (e.g. on reload or retry).
pub struct DesktopSession {
    url: String,
    port: u16,
    pid: u32,
    started: bool,
    version: String,
    log: PathBuf,
}

impl DesktopSession {
    /// Canonical `http://localhost:PORT/ui/#boot=CODE`. Secret: never log or display it.
    pub fn viewer_url(&self) -> &str {
        &self.url
    }
    /// Daemon port on 127.0.0.1 / localhost.
    pub fn port(&self) -> u16 {
        self.port
    }
    /// Daemon pid (from discovery).
    pub fn pid(&self) -> u32 {
        self.pid
    }
    /// `true` if this call started the daemon, `false` if an existing one was reused.
    pub fn started(&self) -> bool {
        self.started
    }
    /// Daemon version from discovery.
    pub fn daemon_version(&self) -> &str {
        &self.version
    }
    /// `<home>/run/loomd.log`.
    pub fn log_path(&self) -> &Path {
        &self.log
    }
    /// Canonical origin `http://localhost:PORT` (not secret) for navigation allow-lists.
    pub fn origin(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

/// Bytes of the UI page `/ui/` served by the daemon on `port` (public static asset, no token):
/// the exact document the desktop window loads (`http://localhost:<port>/ui/#boot=…`). Fetched on
/// the canonical `localhost` host; any non-200 (including a 308 redirect) is an error, never followed.
/// The desktop shell compares them with the UI build it was packaged with: a reused, older
/// daemon keeps serving its own embedded UI, which must never be shown silently.
pub fn daemon_ui_index(port: u16, timeout: Duration) -> io::Result<Vec<u8>> {
    let r = http::get_public(port, "/ui/", timeout)?;
    if r.status != 200 {
        return Err(io::Error::other(format!("GET /ui/ returned HTTP {}", r.status)));
    }
    Ok(r.body)
}

impl std::fmt::Debug for DesktopSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopSession")
            .field("viewer_url", &"<redacted>")
            .field("port", &self.port)
            .field("pid", &self.pid)
            .field("started", &self.started)
            .field("version", &self.version)
            .finish()
    }
}

/// `run/daemon.json` (the token stays private and out of `Debug`).
#[derive(Deserialize)]
struct Discovery {
    port: u16,
    pid: u32,
    token: String,
    version: String,
    proto: String,
}

#[derive(Deserialize)]
struct Bootstrap {
    url: String,
}

/// What one look at the home showed.
enum Probe {
    Ready(DesktopSession),
    /// No (readable/valid) discovery file.
    NoDiscovery,
    /// Discovery names another pid than the daemon we are waiting for.
    OtherPid,
    /// Nothing usable answered on the announced port.
    Unreachable,
    /// 401/403 with the announced token.
    Auth,
    /// Announced protocol is not `loom/1.x`.
    Proto(String),
    /// Answered, but not a valid bootstrap.
    Bad(&'static str),
}

fn sanitize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_graphic()).take(32).collect()
}

fn probe(home: &Path, want_pid: Option<u32>, deadline: Instant) -> Probe {
    let Ok(bytes) = fs::read(home.join("run/daemon.json")) else {
        return Probe::NoDiscovery;
    };
    let Ok(d) = serde_json::from_slice::<Discovery>(&bytes) else {
        return Probe::NoDiscovery;
    };
    if want_pid.is_some_and(|p| p != d.pid) {
        return Probe::OtherPid;
    }
    if d.port == 0 || d.token.is_empty() {
        return Probe::NoDiscovery;
    }
    if !d.proto.starts_with("loom/1.") {
        return Probe::Proto(sanitize(&d.proto));
    }
    let left = deadline
        .saturating_duration_since(Instant::now())
        .min(PROBE_TIMEOUT);
    let resp = match http::get_bearer(d.port, "/api/v1/bootstrap/new", &d.token, left) {
        Ok(r) => r,
        Err(_) => return Probe::Unreachable,
    };
    match resp.status {
        200 => {}
        401 | 403 => return Probe::Auth,
        _ => return Probe::Bad("bootstrap request was refused"),
    }
    let Ok(b) = serde_json::from_slice::<Bootstrap>(&resp.body) else {
        return Probe::Bad("bootstrap answer is not the expected JSON");
    };
    let prefix = format!("http://localhost:{}/ui/#boot=", d.port);
    let code = b.url.strip_prefix(&prefix).unwrap_or_default();
    let code = code.split('&').next().unwrap_or_default();
    if code.is_empty() || b.url.chars().any(|c| !c.is_ascii_graphic()) {
        return Probe::Bad("bootstrap URL is not on the daemon's canonical origin");
    }
    Probe::Ready(DesktopSession {
        url: b.url,
        port: d.port,
        pid: d.pid,
        started: false,
        version: sanitize(&d.version),
        log: home.join("run/loomd.log"),
    })
}

fn home_err(path: &Path) -> impl FnOnce(io::Error) -> HostError + '_ {
    move |source| HostError::Home {
        path: path.to_path_buf(),
        source,
    }
}

/// Create `home` and `home/run` (0700 when we create them; existing modes are left alone).
fn ensure_run_dir(home: &Path) -> Result<PathBuf, HostError> {
    let run = home.join("run");
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        b.mode(0o700);
    }
    b.create(&run).map_err(home_err(&run))?;
    Ok(run)
}

fn lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// `run/start.lock` (same protocol as `loom`): `None` = someone else is starting the daemon.
fn start_lock(run: &Path, deadline: Instant) -> Result<Option<File>, HostError> {
    let path = run.join("start.lock");
    let f = lock_file(&path).map_err(home_err(&path))?;
    let end = deadline.min(Instant::now() + START_LOCK_WAIT);
    loop {
        match f.try_lock() {
            Ok(()) => return Ok(Some(f)),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < end => std::thread::sleep(POLL),
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => return Err(home_err(&path)(e)),
        }
    }
}

/// Whether a live `loomd` holds this home's instance lock. Only called under `start.lock`, and
/// the probe lock is released immediately.
fn daemon_lock_held(run: &Path) -> Result<bool, HostError> {
    let path = run.join("loomd.lock");
    let f = lock_file(&path).map_err(home_err(&path))?;
    match f.try_lock() {
        Ok(()) => {
            drop(f);
            Ok(false)
        }
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(e)) => Err(home_err(&path)(e)),
    }
}

fn is_executable(p: &Path) -> bool {
    let Ok(m) = fs::metadata(p) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        m.is_file() && m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        m.is_file()
    }
}

/// Maps the configured [`ConnectOptions::daemon_bin`] to the file actually started (see
/// [`connect_with`]).
pub type PrepareDaemon<'a> = &'a dyn Fn(&Path) -> io::Result<PathBuf>;

fn spawn_daemon(
    o: &ConnectOptions,
    run: &Path,
    ui_dir: Option<&Path>,
    prepare: PrepareDaemon<'_>,
) -> Result<Child, HostError> {
    if !is_executable(&o.daemon_bin) {
        return Err(HostError::DaemonBinaryMissing {
            path: o.daemon_bin.clone(),
        });
    }
    let bin = prepare(&o.daemon_bin).map_err(|source| HostError::Spawn {
        path: o.daemon_bin.clone(),
        source,
    })?;
    if !is_executable(&bin) {
        return Err(HostError::DaemonBinaryMissing { path: bin });
    }
    let log_path = run.join("loomd.log");
    let mut lo = OpenOptions::new();
    lo.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        lo.mode(0o600);
    }
    let mut log = lo.open(&log_path).map_err(home_err(&log_path))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Non-secret marker so the log shows who started this daemon.
    let _ = writeln!(
        log,
        "loom-desktop-host: starting {} (unix time {now}, desktop pid {})",
        bin.display(),
        std::process::id()
    );
    let err_log = log.try_clone().map_err(home_err(&log_path))?;
    let mut cmd = Command::new(&bin);
    cmd.arg("run");
    if let Some(dir) = ui_dir {
        cmd.arg("--ui-dir").arg(dir);
    }
    for (k, v) in &o.daemon_env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    cmd.env("LOOM_HOME", &o.home)
        .current_dir(&o.home)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err_log);
    spawn_detached(&mut cmd).map_err(|source| HostError::Spawn { path: bin, source })
}

#[cfg(unix)]
fn spawn_detached(cmd: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt as _;
    // Own process group: signals to the app's group (terminal Ctrl-C, group kill) skip the daemon.
    cmd.process_group(0);
    cmd.spawn()
}

/// Same flags as `loom`'s `spawn_daemon`: no console, own process group, and out of the app's job
/// object when the job allows breakaway (a launcher job with kill-on-close would otherwise take
/// the daemon down with the app). A job that forbids breakaway fails the first spawn (access
/// denied); the daemon is then started inside it, exactly as the CLI does.
#[cfg(windows)]
fn spawn_detached(cmd: &mut Command) -> io::Result<Child> {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const ERROR_ACCESS_DENIED: i32 = 5;
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    match cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => cmd.creation_flags(base).spawn(),
        r => r,
    }
}

/// Reap a daemon we started on a background thread so it never lingers as a zombie while the app
/// runs. The thread does not keep the app alive (the process exits without joining it) and never
/// signals the daemon.
fn reap_in_background(mut child: Child) {
    let _ = std::thread::Builder::new()
        .name("loom-desktop-host-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

fn validate(o: &ConnectOptions) -> Result<Option<PathBuf>, HostError> {
    if !o.home.is_absolute() {
        return Err(HostError::InvalidOptions(
            "the Loom home must be an absolute path".into(),
        ));
    }
    if o.startup_timeout.is_zero() {
        return Err(HostError::InvalidOptions(
            "the startup timeout must be positive".into(),
        ));
    }
    let Some(dir) = &o.ui_dir else {
        return Ok(None);
    };
    let ok = dir.join("index.html").is_file();
    match fs::canonicalize(dir) {
        Ok(c) if ok => Ok(Some(c)),
        _ => Err(HostError::InvalidOptions(format!(
            "the UI directory {} has no index.html",
            dir.display()
        ))),
    }
}

/// Reuse the daemon that owns `options.home`, or start `options.daemon_bin` if none does, then mint
/// a fresh viewer bootstrap URL. Bounded by `options.startup_timeout`. Never stops a daemon.
pub fn connect(options: &ConnectOptions) -> Result<DesktopSession, HostError> {
    connect_with(options, &|p| Ok(p.to_path_buf()))
}

/// [`connect`], with `prepare` turning the configured daemon binary into the file to start. It is
/// called only when this call is about to start a daemon (holding `run/start.lock`, after the
/// configured binary was found executable), never when an existing daemon is reused — e.g. the
/// Linux AppImage build copies its bundled daemon out of the app's mount here. An error is a
/// [`HostError::Spawn`] for the configured path, and nothing is started.
pub fn connect_with(
    options: &ConnectOptions,
    prepare: PrepareDaemon<'_>,
) -> Result<DesktopSession, HostError> {
    let ui_dir = validate(options)?;
    let home = options.home.as_path();
    let t0 = Instant::now();
    let deadline = t0 + options.startup_timeout;
    let log = home.join("run/loomd.log");

    // Fast path: a live daemon answers with its announced token. No locks touched.
    let mut last = match probe(home, None, deadline) {
        Probe::Ready(s) => return Ok(s),
        p => p,
    };

    let run = ensure_run_dir(home)?;
    // Held until the daemon answers so a concurrent `loom` waits instead of spawning a second one.
    let lock = start_lock(&run, deadline)?;
    let mut child: Option<Child> = None;
    let mut live_owner = false;
    if lock.is_some() {
        match probe(home, None, deadline) {
            Probe::Ready(s) => return Ok(s),
            p => last = p,
        }
        live_owner = daemon_lock_held(&run)?;
        if !live_owner {
            // Stale or absent discovery: the home has no daemon. Start ours.
            child = Some(spawn_daemon(options, &run, ui_dir.as_deref(), prepare)?);
        }
    }

    while Instant::now() < deadline {
        if let Some(c) = child.as_mut() {
            match c.try_wait() {
                Ok(Some(status)) => {
                    // Lost a race to another starter? Then its daemon is ready to reuse.
                    if let Probe::Ready(s) = probe(home, None, deadline) {
                        return Ok(s);
                    }
                    return Err(HostError::DaemonExited {
                        status: status.to_string(),
                        log,
                    });
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }
        let want = child.as_ref().map(Child::id);
        match probe(home, want, deadline) {
            Probe::Ready(mut s) => {
                drop(lock);
                if let Some(c) = child {
                    s.started = s.pid == c.id();
                    reap_in_background(c);
                }
                return Ok(s);
            }
            Probe::Proto(found) if live_owner => return Err(HostError::ProtocolMismatch { found }),
            p => last = p,
        }
        std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
    drop(lock);
    let waited = t0.elapsed();
    if let Some(c) = child {
        // Left running: it may still come up, and a retry will reuse it.
        reap_in_background(c);
        return Err(HostError::StartupTimeout { waited, log });
    }
    Err(match last {
        Probe::Auth => HostError::AuthRejected {
            discovery: home.join("run/daemon.json"),
        },
        Probe::Proto(found) => HostError::ProtocolMismatch { found },
        Probe::Bad(detail) => HostError::Protocol(detail),
        Probe::Ready(_) | Probe::NoDiscovery | Probe::OtherPid | Probe::Unreachable => {
            HostError::Unresponsive { waited, log }
        }
    })
}

#[cfg(test)]
mod ui_index_tests {
    use super::*;
    use std::io::{BufRead as _, BufReader};
    use std::net::TcpListener;

    /// One-shot local server: records the request head, answers with `reply`.
    fn serve(reply: &'static [u8]) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut head = String::new();
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            s.write_all(reply).unwrap();
            head
        });
        (port, h)
    }

    #[test]
    fn fetches_index_without_credentials() {
        let (port, h) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n<!doctype html>");
        let body = daemon_ui_index(port, Duration::from_secs(2)).unwrap();
        assert_eq!(body, b"<!doctype html>");
        let head = h.join().unwrap();
        assert!(head.starts_with("GET /ui/ HTTP/1.1\r\n"));
        assert!(!head.to_ascii_lowercase().contains("authorization"));
    }

    /// Regression (final QA, 2026-11-15): loomd redirects (308) any `Host: 127.0.0.1` GET outside
    /// /rpc and /api/ to its canonical `localhost` origin; the identity fetch must not be redirected.
    #[test]
    fn uses_canonical_localhost_host() {
        let (port, h) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        daemon_ui_index(port, Duration::from_secs(2)).unwrap();
        let head = h.join().unwrap();
        assert!(
            head.contains(&format!("\r\nHost: localhost:{port}\r\n")),
            "{head}"
        );
    }

    #[test]
    fn non_200_is_an_error() {
        let (port, h) = serve(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        let e = daemon_ui_index(port, Duration::from_secs(2)).unwrap_err();
        assert!(e.to_string().contains("404"));
        h.join().unwrap();
    }
}
