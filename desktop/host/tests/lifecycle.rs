//! Lifecycle tests with local HTTP fixtures and a real executable daemon fixture.
//!
//! The process fixture is this test binary re-executed through a `#!/bin/sh` wrapper (written per
//! test into its own temp home) that runs only `fake_loomd_entry`. It behaves like `loomd` for the
//! parts the host relies on: instance lock, atomic `run/daemon.json`, bearer-checked
//! `GET /api/v1/bootstrap/new`. Every fixture daemon writes `<home>/fake-pid`, exits on its own
//! after a TTL, and is killed by the test that started it. Only fixture pids are ever signalled.

#![cfg(unix)]

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use loom_desktop_host::{ConnectOptions, DesktopSession, HostError, connect, connect_with};

const MODE_ENV: &str = "LOOM_FAKE_DAEMON_MODE";
const CLIENT_ENV: &str = "LOOM_FAKE_CLIENT";

// ---------------------------------------------------------------- fixture daemon (in-process)

#[derive(Clone, Copy, PartialEq, Eq)]
enum Serve {
    Ok,
    Unauthorized,
    BadUrl,
    Chunked,
}

static CODES: AtomicU64 = AtomicU64::new(1);

fn handle(mut s: TcpStream, port: u16, token: &str, mode: Serve) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut request_line = String::new();
    if r.read_line(&mut request_line).is_err() {
        return;
    }
    let mut auth_ok = false;
    let mut host_ok = false;
    loop {
        let mut l = String::new();
        if r.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
            break;
        }
        let lower = l.to_ascii_lowercase();
        if lower.trim_end() == format!("authorization: bearer {token}") {
            auth_ok = true;
        }
        if lower.trim_end() == format!("host: 127.0.0.1:{port}") {
            host_ok = true;
        }
    }
    let path_ok = request_line.starts_with("GET /api/v1/bootstrap/new ");
    let resp = if !(auth_ok && host_ok && path_ok) || mode == Serve::Unauthorized {
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 20\r\nConnection: close\r\n\r\nagent token required"
            .to_owned()
    } else {
        let code = format!("fixturecode{:08}", CODES.fetch_add(1, Ordering::Relaxed));
        let url = if mode == Serve::BadUrl {
            format!("http://evil.example/ui/#boot={code}")
        } else {
            format!("http://localhost:{port}/ui/#boot={code}")
        };
        let body = format!(r#"{{"code":"{code}","url":"{url}","rights":"approve","expires_in_ms":60000}}"#);
        if mode == Serve::Chunked {
            let (a, b) = body.split_at(10);
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                a.len(),
                b.len()
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        }
    };
    let _ = s.write_all(resp.as_bytes());
}

fn serve_forever(l: TcpListener, token: String, mode: Serve) {
    let port = l.local_addr().unwrap().port();
    for s in l.incoming().flatten() {
        handle(s, port, &token, mode);
    }
}

fn write_discovery(home: &Path, port: u16, pid: u32, token: &str, proto: &str) {
    let run = home.join("run");
    fs::create_dir_all(&run).unwrap();
    let tmp = run.join(format!(".daemon.json.{}.tmp", std::process::id()));
    let json = format!(
        r#"{{"port":{port},"pid":{pid},"token":"{token}","version":"0.1.0-fixture","proto":"{proto}"}}"#
    );
    fs::write(&tmp, json).unwrap();
    fs::rename(&tmp, run.join("daemon.json")).unwrap();
}

fn hold_instance_lock(home: &Path) -> File {
    fs::create_dir_all(home.join("run")).unwrap();
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("run/loomd.lock"))
        .unwrap();
    f.try_lock().expect("instance lock");
    f
}

/// A live in-process "daemon" owning `home` (instance lock held, discovery written).
struct InProc {
    _lock: File,
    port: u16,
    token: String,
}

fn in_proc(home: &Path, mode: Serve, proto: &str) -> InProc {
    let lock = hold_instance_lock(home);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let token = format!("tok{:016x}secretsecret", port as u64 * 7919);
    write_discovery(home, port, std::process::id(), &token, proto);
    let t = token.clone();
    std::thread::spawn(move || serve_forever(l, t, mode));
    InProc {
        _lock: lock,
        port,
        token,
    }
}

// ---------------------------------------------------------------- fixture daemon (process)

/// Entry of the process fixture. A no-op unless re-executed by a wrapper script.
#[test]
fn fake_loomd_entry() {
    let Ok(mode) = std::env::var(MODE_ENV) else {
        return;
    };
    let home = PathBuf::from(std::env::var_os("LOOM_HOME").expect("LOOM_HOME passed by the host"));
    fs::write(home.join("fake-pid"), std::process::id().to_string()).unwrap();
    eprintln!("fake-loomd-stderr-marker mode={mode}");
    println!("fake-loomd-stdout-marker");
    // Safety net: fixtures never outlive the test run.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        std::process::exit(0);
    });
    if mode == "exit" {
        std::process::exit(3);
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("run/loomd.lock"))
        .unwrap();
    if lock.try_lock().is_err() {
        eprintln!("fake-loomd: another loomd is running for this LOOM_HOME");
        std::process::exit(2);
    }
    if mode == "hang" {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if mode == "slow" {
        std::thread::sleep(Duration::from_millis(400));
    }
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let token = format!("proctok{:x}{}", std::process::id(), "f".repeat(40));
    write_discovery(&home, port, std::process::id(), &token, "loom/1.0");
    serve_forever(l, token, Serve::Ok);
}

/// Re-exec entry acting as a short-lived desktop app: connect, record, then exit or linger.
#[test]
fn fake_client_entry() {
    let Ok(spec) = std::env::var(CLIENT_ENV) else {
        return;
    };
    let (home, bin, out, linger) = {
        let p: Vec<&str> = spec.split('\u{1f}').collect();
        (
            PathBuf::from(p[0]),
            PathBuf::from(p[1]),
            PathBuf::from(p[2]),
            p[3] == "linger",
        )
    };
    let s = connect(&opts(&home, &bin)).expect("client connect");
    fs::write(&out, format!("{} {}", s.pid(), s.started())).unwrap();
    if linger {
        std::thread::sleep(Duration::from_secs(30));
    }
    std::process::exit(0);
}

/// Std-only temp dir under the OS temp dir; removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> TempDir {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let p = std::env::temp_dir().join(format!(
        "ldh-{}-{}-{nanos}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&p).unwrap();
    TempDir(p)
}

fn fake_bin(dir: &Path, mode: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let script = dir.join(format!("fake-loomd-{mode}"));
    let body = format!(
        "#!/bin/sh\necho spawn >> \"$LOOM_HOME/spawn-count\"\nprintf '%s\\n' \"$@\" > \"$LOOM_HOME/fake-args\"\npwd > \"$LOOM_HOME/fake-cwd\"\nprintf '%s|%s' \"${{LOOM_TEST_ENV_PROBE-unset}}\" \"${{HOME-unset}}\" > \"$LOOM_HOME/fake-env\"\n{MODE_ENV}={mode} exec '{}' fake_loomd_entry --exact --nocapture --test-threads=1\n",
        exe.display()
    );
    fs::write(&script, body).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn opts(home: &Path, bin: &Path) -> ConnectOptions {
    ConnectOptions {
        home: home.to_path_buf(),
        daemon_bin: bin.to_path_buf(),
        ui_dir: None,
        startup_timeout: Duration::from_secs(10),
        daemon_env: Vec::new(),
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn wait_dead(pid: u32, within: Duration) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        // A child of this test process may linger as a zombie until reaped: check state too.
        let st = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default();
        if !alive(pid) || st.starts_with('Z') || st.is_empty() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Kills the fixture daemon recorded in `<home>/fake-pid` (only ever our own fixture).
struct Fixture {
    home: TempDir,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture { home: tempdir() }
    }
    fn path(&self) -> &Path {
        self.home.path()
    }
    fn fake_pid(&self) -> Option<u32> {
        fs::read_to_string(self.path().join("fake-pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
    fn spawns(&self) -> usize {
        fs::read_to_string(self.path().join("spawn-count")).map_or(0, |s| s.lines().count())
    }
    fn log(&self) -> String {
        fs::read_to_string(self.path().join("run/loomd.log")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(pid) = self.fake_pid() {
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

fn assert_redacted(text: &str, secrets: &[&str]) {
    assert!(!text.contains("#boot="), "leaked boot fragment: {text}");
    assert!(!text.contains("fixturecode"), "leaked code: {text}");
    for s in secrets {
        assert!(!text.contains(s), "leaked secret in: {text}");
    }
}

fn assert_viewer_url(s: &DesktopSession) {
    let prefix = format!("http://localhost:{}/ui/#boot=", s.port());
    assert!(s.viewer_url().starts_with(&prefix));
    assert!(s.viewer_url().len() > prefix.len());
    assert_eq!(s.origin(), format!("http://localhost:{}", s.port()));
}

// ---------------------------------------------------------------- tests

#[test]
fn reuses_existing_daemon_without_spawning() {
    let f = Fixture::new();
    let d = in_proc(f.path(), Serve::Ok, "loom/1.0");
    // A daemon binary that does not exist proves no spawn is attempted.
    let s = connect(&opts(f.path(), Path::new("/nonexistent/loomd"))).unwrap();
    assert!(!s.started());
    assert_eq!(s.port(), d.port);
    assert_eq!(s.pid(), std::process::id());
    assert_eq!(s.daemon_version(), "0.1.0-fixture");
    assert_viewer_url(&s);
    // Each connect mints a fresh code.
    let s2 = connect(&opts(f.path(), Path::new("/nonexistent/loomd"))).unwrap();
    assert_ne!(s.viewer_url(), s2.viewer_url());
    assert_redacted(&format!("{s:?}"), &[&d.token]);
    assert_eq!(f.spawns(), 0);
}

#[test]
fn chunked_bootstrap_answer_is_accepted() {
    let f = Fixture::new();
    let _d = in_proc(f.path(), Serve::Chunked, "loom/1.0");
    let s = connect(&opts(f.path(), Path::new("/nonexistent/loomd"))).unwrap();
    assert_viewer_url(&s);
}

#[test]
fn starts_absent_daemon_detached_with_logged_stdio_and_survives_drop() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let t0 = Instant::now();
    let s = connect(&opts(f.path(), &bin)).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(10));
    assert!(s.started());
    assert_eq!(Some(s.pid()), f.fake_pid());
    assert_viewer_url(&s);
    // argv / env / cwd
    assert_eq!(fs::read_to_string(f.path().join("fake-args")).unwrap(), "run\n");
    let cwd = fs::canonicalize(fs::read_to_string(f.path().join("fake-cwd")).unwrap().trim()).unwrap();
    assert_eq!(cwd, fs::canonicalize(f.path()).unwrap());
    // own process group
    let pgid = Command::new("ps")
        .args(["-o", "pgid=", "-p", &s.pid().to_string()])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&pgid.stdout).trim(), s.pid().to_string());
    // stdio routed to run/loomd.log with protected modes
    let log = f.log();
    assert!(log.contains("loom-desktop-host: starting"), "{log}");
    assert!(log.contains("fake-loomd-stderr-marker"), "{log}");
    assert!(log.contains("fake-loomd-stdout-marker"), "{log}");
    assert_redacted(&log, &[]);
    let mode = |p: &str| fs::metadata(f.path().join(p)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode("run/loomd.log"), 0o600);
    assert_eq!(mode("run"), 0o700);
    // Dropping the session leaves the daemon alive; next connect reuses it.
    let pid = s.pid();
    drop(s);
    std::thread::sleep(Duration::from_millis(200));
    assert!(alive(pid));
    let again = connect(&opts(f.path(), &bin)).unwrap();
    assert!(!again.started());
    assert_eq!(again.pid(), pid);
    assert_eq!(f.spawns(), 1);
}

#[test]
fn passes_ui_dir_only_when_starting() {
    let f = Fixture::new();
    let ui = f.path().join("ui-dist");
    fs::create_dir_all(&ui).unwrap();
    fs::write(ui.join("index.html"), "<!doctype html>").unwrap();
    let bin = fake_bin(f.path(), "ok");
    let mut o = opts(f.path(), &bin);
    o.ui_dir = Some(ui.clone());
    let s = connect(&o).unwrap();
    assert!(s.started());
    let args = fs::read_to_string(f.path().join("fake-args")).unwrap();
    assert_eq!(
        args,
        format!("run\n--ui-dir\n{}\n", fs::canonicalize(&ui).unwrap().display())
    );
}

/// `daemon_env` reaches a daemon this call starts (set and remove), but can never redirect it to
/// another home: `LOOM_HOME` is always the selected home.
#[test]
fn daemon_env_overrides_apply_but_never_replace_loom_home() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let mut o = opts(f.path(), &bin);
    o.daemon_env = vec![
        ("LOOM_TEST_ENV_PROBE".into(), Some("set-by-desktop".into())),
        ("HOME".into(), None), // inherited from the test process: removal is real,
        ("LOOM_HOME".into(), Some("/nonexistent-bogus-home".into())),
    ];
    let s = connect(&o).unwrap();
    assert!(s.started());
    // The probe file is under the real home, so LOOM_HOME was the selected one.
    assert_eq!(
        fs::read_to_string(f.path().join("fake-env")).unwrap(),
        "set-by-desktop|unset"
    );
}

/// `connect_with`'s prepare hook (Linux AppImage copy-out) runs only when a daemon is started:
/// never on reuse (so a read-only home with a running daemon still connects), exactly once on a
/// start, and the prepared file is what runs.
#[test]
fn prepare_hook_runs_only_when_starting_and_its_binary_is_spawned() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);

    // Reuse: hook never called (it would fail loudly).
    let f = Fixture::new();
    let _d = in_proc(f.path(), Serve::Ok, "loom/1.0");
    let s = connect_with(&opts(f.path(), Path::new("/nonexistent/loomd")), &|_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(std::io::Error::other("must not be called on reuse"))
    })
    .unwrap();
    assert!(!s.started());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Start: hook called once with the configured binary; its result is what runs.
    let f = Fixture::new();
    let configured = fake_bin(f.path(), "ok");
    let copies = f.path().join("copies");
    fs::create_dir_all(&copies).unwrap();
    let prepared = fake_bin(&copies, "ok");
    let s = connect_with(&opts(f.path(), &configured), &|p| {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(p, configured.as_path());
        Ok(prepared.clone())
    })
    .unwrap();
    assert!(s.started());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(f.log().contains(&format!("starting {}", prepared.display())), "{}", f.log());

    // Hook failure: a spawn error naming the configured binary, and nothing started.
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let e = connect_with(&opts(f.path(), &bin), &|_| Err(std::io::Error::other("copy failed")))
        .unwrap_err();
    assert!(matches!(e, HostError::Spawn { .. }), "{e:?}");
    assert!(e.to_string().contains("copy failed"), "{e}");
    assert_eq!(f.spawns(), 0);
}

#[test]
fn invalid_options_fail_fast_without_spawning() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let mut o = opts(f.path(), &bin);
    o.ui_dir = Some(f.path().join("no-such-ui"));
    assert!(matches!(connect(&o), Err(HostError::InvalidOptions(_))));
    let mut o = opts(Path::new("relative/home"), &bin);
    assert!(matches!(connect(&o), Err(HostError::InvalidOptions(_))));
    o = opts(f.path(), &bin);
    o.startup_timeout = Duration::ZERO;
    let e = connect(&o).unwrap_err();
    assert!(matches!(e, HostError::InvalidOptions(_)) && !e.is_retryable());
    assert_eq!(f.spawns(), 0);
}

#[test]
fn missing_or_non_executable_binary_is_explicit() {
    let f = Fixture::new();
    let e = connect(&opts(f.path(), &f.path().join("absent-loomd"))).unwrap_err();
    assert!(matches!(e, HostError::DaemonBinaryMissing { .. }), "{e}");
    assert!(e.to_string().contains("absent-loomd"));
    assert!(!e.is_retryable());
    let plain = f.path().join("not-exec");
    fs::write(&plain, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        connect(&opts(f.path(), &plain)),
        Err(HostError::DaemonBinaryMissing { .. })
    ));
}

#[test]
fn stale_discovery_with_dead_port_is_replaced() {
    let f = Fixture::new();
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    write_discovery(f.path(), port, 999_999, "stale-token-value", "loom/1.0");
    let s = connect(&opts(f.path(), &fake_bin(f.path(), "ok"))).unwrap();
    assert!(s.started());
    assert_ne!(s.port(), port);
    assert_eq!(Some(s.pid()), f.fake_pid());
}

#[test]
fn stale_discovery_pointing_at_foreign_daemon_is_replaced() {
    // Port reused by another home's daemon (401 for our stale token), instance lock free.
    let f = Fixture::new();
    let foreign = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = foreign.local_addr().unwrap().port();
    std::thread::spawn(move || serve_forever(foreign, "foreign-token".into(), Serve::Ok));
    write_discovery(f.path(), port, 999_998, "stale-token-value", "loom/1.0");
    let s = connect(&opts(f.path(), &fake_bin(f.path(), "ok"))).unwrap();
    assert!(s.started());
    assert_ne!(s.port(), port);
}

#[test]
fn stale_discovery_with_unsupported_proto_is_replaced_when_unowned() {
    let f = Fixture::new();
    write_discovery(f.path(), 1, 999_997, "stale", "loom/2.0");
    let s = connect(&opts(f.path(), &fake_bin(f.path(), "ok"))).unwrap();
    assert!(s.started());
}

#[test]
fn live_daemon_rejecting_token_is_auth_error() {
    let f = Fixture::new();
    let d = in_proc(f.path(), Serve::Unauthorized, "loom/1.0");
    let mut o = opts(f.path(), &fake_bin(f.path(), "ok"));
    o.startup_timeout = Duration::from_millis(600);
    let t0 = Instant::now();
    let e = connect(&o).unwrap_err();
    assert!(t0.elapsed() < Duration::from_secs(3));
    assert!(matches!(e, HostError::AuthRejected { .. }), "{e}");
    assert!(e.is_retryable());
    assert_redacted(&format!("{e} {e:?}"), &[&d.token]);
    assert_eq!(f.spawns(), 0, "never start a second daemon over a live one");
}

#[test]
fn live_daemon_with_unsupported_proto_is_mismatch() {
    let f = Fixture::new();
    let _d = in_proc(f.path(), Serve::Ok, "loom/2.0");
    let e = connect(&opts(f.path(), &fake_bin(f.path(), "ok"))).unwrap_err();
    assert!(
        matches!(&e, HostError::ProtocolMismatch { found } if found == "loom/2.0"),
        "{e}"
    );
    assert!(!e.is_retryable());
    assert_eq!(f.spawns(), 0);
}

#[test]
fn bootstrap_url_off_canonical_origin_is_protocol_error() {
    let f = Fixture::new();
    let d = in_proc(f.path(), Serve::BadUrl, "loom/1.0");
    let mut o = opts(f.path(), &fake_bin(f.path(), "ok"));
    o.startup_timeout = Duration::from_millis(500);
    let e = connect(&o).unwrap_err();
    assert!(matches!(e, HostError::Protocol(_)), "{e}");
    assert_redacted(&format!("{e} {e:?}"), &[&d.token, "evil.example"]);
}

#[test]
fn daemon_exiting_during_startup_is_reported_with_log() {
    let f = Fixture::new();
    let e = connect(&opts(f.path(), &fake_bin(f.path(), "exit"))).unwrap_err();
    match &e {
        HostError::DaemonExited { status, log } => {
            assert!(status.contains('3'), "{status}");
            assert_eq!(log, &f.path().join("run/loomd.log"));
        }
        other => panic!("unexpected {other}"),
    }
    assert!(e.to_string().contains("loomd.log"));
    assert!(f.log().contains("fake-loomd-stderr-marker mode=exit"));
}

#[test]
fn hanging_daemon_times_out_bounded_and_is_left_running() {
    let f = Fixture::new();
    let mut o = opts(f.path(), &fake_bin(f.path(), "hang"));
    o.startup_timeout = Duration::from_millis(800);
    let t0 = Instant::now();
    let e = connect(&o).unwrap_err();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_millis(800) && took < Duration::from_secs(3),
        "{took:?}"
    );
    assert!(matches!(e, HostError::StartupTimeout { .. }), "{e}");
    assert!(e.is_retryable());
    let pid = f.fake_pid().expect("fixture started");
    assert!(alive(pid), "connect never kills a daemon");
}

#[test]
fn slow_daemon_within_timeout_succeeds() {
    let f = Fixture::new();
    let s = connect(&opts(f.path(), &fake_bin(f.path(), "slow"))).unwrap();
    assert!(s.started());
}

#[test]
fn concurrent_connects_start_exactly_one_daemon() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "slow");
    let o = opts(f.path(), &bin);
    let sessions: Vec<DesktopSession> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..4).map(|_| sc.spawn(|| connect(&o).unwrap())).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(f.spawns(), 1);
    let pid = sessions[0].pid();
    assert!(sessions.iter().all(|s| s.pid() == pid));
    assert_eq!(sessions.iter().filter(|s| s.started()).count(), 1);
}

fn run_client(f: &Fixture, bin: &Path, linger: bool) -> (std::process::Child, PathBuf) {
    let out = f.path().join("client-out");
    let spec = [
        f.path().to_str().unwrap(),
        bin.to_str().unwrap(),
        out.to_str().unwrap(),
        if linger { "linger" } else { "exit" },
    ]
    .join("\u{1f}");
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["fake_client_entry", "--exact", "--nocapture", "--test-threads=1"])
        .env(CLIENT_ENV, spec)
        .stdout(std::process::Stdio::null());
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    (cmd.spawn().unwrap(), out)
}

fn wait_file(p: &Path, within: Duration) -> String {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if let Ok(s) = fs::read_to_string(p)
            && !s.is_empty()
        {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{} never written", p.display());
}

#[test]
fn daemon_outlives_app_process_exit() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let (mut app, out) = run_client(&f, &bin, false);
    assert!(app.wait().unwrap().success());
    let rec = fs::read_to_string(out).unwrap();
    let (pid, started) = rec.split_once(' ').unwrap();
    assert_eq!(started, "true");
    let pid: u32 = pid.parse().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(pid), "daemon must survive the app exiting");
    let s = connect(&opts(f.path(), &bin)).unwrap();
    assert!(!s.started());
    assert_eq!(s.pid(), pid);
}

#[test]
fn daemon_outlives_app_process_group_kill() {
    let f = Fixture::new();
    let bin = fake_bin(f.path(), "ok");
    let (mut app, out) = run_client(&f, &bin, true);
    let rec = wait_file(&out, Duration::from_secs(10));
    let pid: u32 = rec.split_once(' ').unwrap().0.parse().unwrap();
    // SIGTERM then SIGKILL the whole app process group (like a terminal Ctrl-C / force quit).
    let group = format!("-{}", app.id());
    let _ = Command::new("kill")
        .args(["-TERM", "--", &group])
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stderr(std::process::Stdio::null())
        .status();
    let _ = app.wait();
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        alive(pid),
        "daemon must survive the app's process group being killed"
    );
    assert!(!wait_dead(pid, Duration::from_millis(100)));
    let s = connect(&opts(f.path(), &bin)).unwrap();
    assert_eq!(s.pid(), pid);
}

#[test]
fn errors_display_is_actionable_and_redacted() {
    let f = Fixture::new();
    let d = in_proc(f.path(), Serve::Unauthorized, "loom/1.0");
    let mut o = opts(f.path(), Path::new("/nonexistent/loomd"));
    o.startup_timeout = Duration::from_millis(300);
    let e = connect(&o).unwrap_err();
    let text = e.to_string();
    assert!(text.contains("loom daemon stop"), "{text}");
    assert!(text.contains("daemon.json"), "{text}");
    let raw = fs::read_to_string(f.path().join("run/daemon.json")).unwrap();
    assert!(raw.contains(&d.token));
    assert_redacted(&format!("{text} {e:?}"), &[&d.token]);
    let _ = d.port;
}
