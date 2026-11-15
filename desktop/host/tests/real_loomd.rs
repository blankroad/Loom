//! Against a real `loomd` in an isolated temp `LOOM_HOME`. Skipped (passes trivially) unless
//! `LOOM_DESKTOP_HOST_LOOMD=/abs/path/to/loomd` is set; optional `LOOM_DESKTOP_HOST_LOOM=/abs/loom`
//! adds a CLI shared-daemon check. Only the daemon this test started is stopped (SIGTERM).
//! Nothing secret (token, code, cookie, viewer URL) is printed.

#![cfg(unix)]

use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use loom_desktop_host::{ConnectOptions, connect};

struct Home(PathBuf, Option<u32>);

impl Drop for Home {
    fn drop(&mut self) {
        if let Some(pid) = self.1 {
            let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end && alive(pid) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if alive(pid) {
                let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
            }
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Raw HTTP/1.1 request to the canonical origin; returns (status, lowercase head, body).
fn request(port: u16, method: &str, path: &str, body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, head.to_ascii_lowercase(), body.to_owned())
}

fn code_of(url: &str) -> &str {
    url.split("#boot=").nth(1).unwrap().split('&').next().unwrap()
}

#[test]
fn real_loomd_lifecycle() {
    let Some(bin) = std::env::var_os("LOOM_DESKTOP_HOST_LOOMD").map(PathBuf::from) else {
        eprintln!("skipped: set LOOM_DESKTOP_HOST_LOOMD to a built loomd");
        return;
    };
    let root = std::env::temp_dir().join(format!("ldh-real-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut home = Home(fs::canonicalize(&root).unwrap(), None);
    let ui = home.0.join("ui-dist");
    fs::create_dir_all(&ui).unwrap();
    fs::write(
        ui.join("index.html"),
        "<!doctype html><title>ldh-ui-dir-marker</title>",
    )
    .unwrap();
    let o = ConnectOptions {
        home: home.0.join("home"),
        daemon_bin: bin,
        ui_dir: Some(ui),
        startup_timeout: Duration::from_secs(30),
        daemon_env: Vec::new(),
    };

    // 1. absent → started from the explicit binary
    let t0 = Instant::now();
    let s = connect(&o).expect("connect starts loomd");
    home.1 = Some(s.pid());
    eprintln!(
        "started={} pid={} port={} version={} in {:?}",
        s.started(),
        s.pid(),
        s.port(),
        s.daemon_version(),
        t0.elapsed()
    );
    assert!(s.started());
    assert!(
        s.viewer_url()
            .starts_with(&format!("http://localhost:{}/ui/#boot=", s.port()))
    );
    assert!(!format!("{s:?}").contains(code_of(s.viewer_url())));

    // 2. the daemon serves the selected --ui-dir at /ui/ on the canonical origin
    let (st, _, body) = request(s.port(), "GET", "/ui/", "");
    assert_eq!(st, 200);
    assert!(body.contains("ldh-ui-dir-marker"));
    // 2b. the desktop UI-identity fetch reads exactly those bytes from the REAL daemon (canonical
    //     host rule: a `Host: 127.0.0.1` GET outside /api would be a 308; regression 2026-11-15)
    let served = loom_desktop_host::daemon_ui_index(s.port(), Duration::from_secs(5))
        .expect("identity fetch of /ui/ from the real daemon");
    assert_eq!(
        served,
        b"<!doctype html><title>ldh-ui-dir-marker</title>".to_vec(),
        "identity bytes must equal the served index.html exactly"
    );

    // 3. the minted code is a real, single-use viewer bootstrap
    let code = code_of(s.viewer_url()).to_owned();
    let (st, head, _) = request(
        s.port(),
        "POST",
        "/api/v1/bootstrap",
        &format!(r#"{{"code":"{code}"}}"#),
    );
    assert_eq!(st, 204);
    assert!(head.contains("set-cookie: loom_session="));
    let (st, _, _) = request(
        s.port(),
        "POST",
        "/api/v1/bootstrap",
        &format!(r#"{{"code":"{code}"}}"#),
    );
    assert_eq!(st, 410, "code must be single-use");

    // 4. drop never stops the daemon; reconnect reuses it with a fresh code
    let (pid, port, first) = (s.pid(), s.port(), s.viewer_url().to_owned());
    drop(s);
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(pid));
    let again = connect(&o).expect("reuse");
    assert!(!again.started());
    assert_eq!((again.pid(), again.port()), (pid, port));
    assert_ne!(again.viewer_url(), first);

    // 5. the CLI sees the very same daemon through the same home
    if let Some(loom) = std::env::var_os("LOOM_DESKTOP_HOST_LOOM") {
        let out = Command::new(loom)
            .args(["--json", "daemon", "status"])
            .env("LOOM_HOME", &o.home)
            .env("LOOM_NO_AUTOSTART", "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "loom daemon status failed");
        assert!(text.contains(&pid.to_string()), "CLI reports another daemon");
        eprintln!("cli daemon status: ok, same pid");
    }

    // 6. log routed to the home, 0600, no secrets
    let log_path = o.home.join("run/loomd.log");
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("loom-desktop-host: starting"));
    assert!(!log.contains(&code));
    let tok: serde_json::Value =
        serde_json::from_slice(&fs::read(o.home.join("run/daemon.json")).unwrap()).unwrap();
    assert!(!log.contains(tok["token"].as_str().unwrap()));
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        fs::metadata(&log_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
