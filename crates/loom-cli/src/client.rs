//! Synchronous daemon client (plan §4.6): discovery (`daemon.json`), WebSocket + `core.hello`,
//! one-shot RPCs with a deadline, and daemon autostart (contract 03 §3.5). No async runtime.

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use loom_proto::envelope::ErrorBody;
use loom_proto::error::ErrorCode;
use loom_proto::rpc::RpcError;
use serde::Deserialize;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::{Message, WebSocket};

/// Protocol spoken.
pub const PROTO: &str = "loom/1.0";
/// Longest wait for the start lock (contract 03 §3.5).
pub const START_LOCK_WAIT: Duration = Duration::from_secs(2);
/// Longest wait for a freshly spawned daemon to answer.
pub const START_WAIT: Duration = Duration::from_secs(10);
/// TCP connect timeout to 127.0.0.1.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Loom home: `LOOM_HOME`, else `~/.loom`.
pub fn loom_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("LOOM_HOME") {
        return Some(PathBuf::from(h));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".loom"))
}

/// `run/daemon.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DaemonInfo {
    /// Port on 127.0.0.1.
    pub port: u16,
    /// Daemon pid.
    pub pid: u32,
    /// Agent token.
    pub token: String,
    /// Daemon version.
    pub version: String,
    /// Protocol.
    pub proto: String,
}

impl DaemonInfo {
    /// Read `<home>/run/daemon.json`.
    pub fn read(home: &Path) -> io::Result<DaemonInfo> {
        serde_json::from_slice(&fs::read(home.join("run/daemon.json"))?).map_err(io::Error::other)
    }
    /// Socket address.
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

/// Error body helper.
pub fn body(code: ErrorCode, message: impl Into<String>, hint: Option<&str>) -> ErrorBody {
    ErrorBody {
        code,
        message: message.into(),
        hint: hint.map(str::to_owned),
        retryable: false,
        request_id: None,
        detail: Value::Null,
    }
}

/// A JSON-RPC error turned into a Loom error body.
pub fn rpc_error_body(e: &RpcError) -> ErrorBody {
    if let Some(b) = e
        .data
        .as_ref()
        .and_then(|d| serde_json::from_value::<ErrorBody>(d.clone()).ok())
    {
        return b;
    }
    let code = match e.code {
        -32601 => ErrorCode::UnknownMethod,
        -32602 | -32600 => ErrorCode::Usage,
        _ => ErrorCode::Internal,
    };
    body(code, e.message.clone(), None)
}

/// One authenticated connection.
#[derive(Debug)]
pub struct Conn {
    ws: WebSocket<TcpStream>,
    next_id: i64,
    /// `core.hello` result.
    pub hello: Value,
}

fn io_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Conn {
    /// Connect and say hello with the agent token.
    pub fn open(info: &DaemonInfo, client_id: &str, timeout: Duration) -> Result<Conn, String> {
        let stream = TcpStream::connect_timeout(&info.addr(), CONNECT_TIMEOUT).map_err(io_err)?;
        stream.set_nodelay(true).map_err(io_err)?;
        stream.set_read_timeout(Some(timeout)).map_err(io_err)?;
        let req = format!("ws://127.0.0.1:{}/rpc", info.port)
            .into_client_request()
            .map_err(io_err)?;
        let (ws, _) = tungstenite::client::client(req, stream).map_err(io_err)?;
        let mut c = Conn {
            ws,
            next_id: 0,
            hello: Value::Null,
        };
        let hello = json!({
            "proto": PROTO,
            "token": info.token,
            "client_id": client_id,
            "want_role": "agent",
            "client": {"kind": "cli", "version": env!("CARGO_PKG_VERSION"), "tty": io::stdout().is_terminal()},
        });
        match c.call("core.hello", hello, timeout)? {
            Ok(v) => {
                c.hello = v;
                Ok(c)
            }
            Err(e) => Err(format!("hello refused: {}", rpc_error_body(&e).message)),
        }
    }

    /// One RPC. Outer `Err` = transport failure/timeout; inner = JSON-RPC error.
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Result<Value, RpcError>, String> {
        self.next_id += 1;
        let id = self.next_id;
        let text = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        self.ws.send(Message::Text(text.into())).map_err(io_err)?;
        let end = Instant::now() + timeout;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("no answer to {method} within {} ms", timeout.as_millis()));
            }
            self.ws.get_ref().set_read_timeout(Some(left)).map_err(io_err)?;
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(t.as_str()).map_err(io_err)?;
                    if v["id"] != json!(id) {
                        continue; // notification
                    }
                    if let Some(e) = v.get("error") {
                        let e: RpcError = serde_json::from_value(e.clone()).map_err(io_err)?;
                        return Ok(Err(e));
                    }
                    return Ok(Ok(v["result"].clone()));
                }
                Ok(Message::Close(_)) => return Err("daemon closed the connection".into()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    /// Close politely.
    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

/// Result of [`connect`].
#[derive(Debug)]
pub struct Connected {
    /// Connection.
    pub conn: Conn,
    /// Discovery info.
    pub info: DaemonInfo,
    /// This call started the daemon.
    pub autostarted: bool,
    /// Windows: the daemon could not leave this process's job object.
    pub tied_to_job: bool,
}

fn try_existing(home: &Path, client_id: &str, timeout: Duration) -> Option<(Conn, DaemonInfo)> {
    let info = DaemonInfo::read(home).ok()?;
    Conn::open(&info, client_id, timeout).ok().map(|c| (c, info))
}

/// `loomd` executable: `LOOM_DAEMON_BIN` → next to `loom` → `PATH`.
pub fn daemon_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("LOOM_DAEMON_BIN") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) { "loomd.exe" } else { "loomd" };
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let p = dir.join(name);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from(name)
}

/// Spawn `loomd run` detached (own process group / session, stdio to `run/loomd.log`).
/// Returns `(child pid, tied_to_job)`.
pub fn spawn_daemon(home: &Path) -> io::Result<(u32, bool)> {
    fs::create_dir_all(home.join("run"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("run/loomd.log"))?;
    let mut cmd = Command::new(daemon_bin());
    cmd.arg("run")
        .env("LOOM_HOME", home)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    detach(&mut cmd)
}

#[cfg(unix)]
fn detach(cmd: &mut Command) -> io::Result<(u32, bool)> {
    use std::os::unix::process::CommandExt as _;
    // New process group: terminal signals (Ctrl-C) to the agent's shell do not reach the daemon.
    cmd.process_group(0);
    Ok((cmd.spawn()?.id(), false))
}

#[cfg(windows)]
fn detach(cmd: &mut Command) -> io::Result<(u32, bool)> {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    // Try to leave the agent's job object; if the job forbids breakaway, start tied to it.
    match cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB).spawn() {
        Ok(c) => Ok((c.id(), false)),
        Err(_) => Ok((cmd.creation_flags(base).spawn()?.id(), true)),
    }
}

/// Take `run/start.lock` (waits ≤ 2 s). `None` = someone else holds it (they are starting it).
fn start_lock(home: &Path) -> io::Result<Option<File>> {
    fs::create_dir_all(home.join("run"))?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("run/start.lock"))?;
    let end = Instant::now() + START_LOCK_WAIT;
    loop {
        match f.try_lock() {
            Ok(()) => return Ok(Some(f)),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// Whether [`connect`] may start the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Start it unless `LOOM_NO_AUTOSTART=1` (normal commands).
    Auto,
    /// Always start it (`loom daemon start`).
    Always,
    /// Never (`loom daemon stop|status`).
    Never,
}

/// Connect, starting the daemon if needed and allowed.
pub fn connect(
    home: &Path,
    client_id: &str,
    timeout: Duration,
    start: Start,
) -> Result<Connected, ErrorBody> {
    if let Some((conn, info)) = try_existing(home, client_id, timeout) {
        return Ok(Connected {
            conn,
            info,
            autostarted: false,
            tied_to_job: false,
        });
    }
    let no_auto = start == Start::Auto && std::env::var("LOOM_NO_AUTOSTART").is_ok_and(|v| v == "1");
    if no_auto || start == Start::Never {
        return Err(body(
            ErrorCode::Daemon,
            "the Loom daemon is not running",
            Some(if no_auto {
                "LOOM_NO_AUTOSTART=1 is set — ask the user to run `loom daemon start`"
            } else {
                "run `loom daemon start`"
            }),
        ));
    }
    let lock = start_lock(home).map_err(|e| {
        body(
            ErrorCode::Daemon,
            format!("cannot take the start lock: {e}"),
            None,
        )
    })?;
    let mut spawned = None;
    if lock.is_some() {
        // Someone may have started it while we waited for the lock.
        if let Some((conn, info)) = try_existing(home, client_id, timeout) {
            return Ok(Connected {
                conn,
                info,
                autostarted: false,
                tied_to_job: false,
            });
        }
        let (pid, tied) = spawn_daemon(home).map_err(|e| {
            body(
                ErrorCode::Daemon,
                format!("cannot start {}: {e}", daemon_bin().display()),
                Some("set LOOM_DAEMON_BIN or put loomd next to loom"),
            )
        })?;
        spawned = Some((pid, tied));
    }
    let end = Instant::now() + START_WAIT;
    while Instant::now() < end {
        if let Ok(info) = DaemonInfo::read(home)
            && spawned.is_none_or(|(pid, _)| info.pid == pid)
            && let Ok(conn) = Conn::open(&info, client_id, timeout)
        {
            drop(lock);
            return Ok(Connected {
                conn,
                info,
                autostarted: spawned.is_some(),
                tied_to_job: spawned.is_some_and(|(_, t)| t),
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(body(
        ErrorCode::Daemon,
        "the Loom daemon did not come up",
        Some(&format!("see {}", home.join("run/loomd.log").display())),
    ))
}

/// Minimal HTTP/1.1 GET with the agent token (`/api/v1/*`). Returns `(status, body)`.
pub fn http_get(info: &DaemonInfo, path: &str) -> io::Result<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&info.addr(), CONNECT_TIMEOUT)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        info.port, info.token
    )?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        let mut b = String::new();
        let mut rest = body;
        while let Some((len, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
            if n == 0 || tail.len() < n {
                break;
            }
            b.push_str(&tail[..n]);
            rest = tail[n..].trim_start_matches("\r\n");
        }
        b
    } else {
        body.to_owned()
    };
    Ok((status, body))
}
