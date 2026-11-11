//! P1 W3: echo as an IPC worker (`loomd worker echo`) under supervision. Unix only (kill/pids).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

use loom_gateway::daemon::{Config, Running, start};
use loom_gateway::home::Home;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

struct D {
    rt: tokio::runtime::Runtime,
    run: Option<Running>,
    home: tempfile::TempDir,
}

impl D {
    fn start(orphan: Duration) -> D {
        D::start_in(tempfile::tempdir().unwrap(), orphan)
    }
    fn start_in(home: tempfile::TempDir, orphan: Duration) -> D {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.worker_program = Some(env!("CARGO_BIN_EXE_loomd").into());
        cfg.worker_orphan = Some(orphan);
        let run = rt.block_on(start(cfg)).unwrap();
        D {
            rt,
            run: Some(run),
            home,
        }
    }
    fn run(&self) -> &Running {
        self.run.as_ref().unwrap()
    }
    fn agent(&self, id: &str) -> Client {
        let mut c = Client::connect(self.run().addr.port(), &[]);
        let r = c.request("core.hello", hello(Some(&self.run().info.token), id));
        assert_eq!(r["result"]["role"], "agent", "{r}");
        c
    }
    fn viewer(&self) -> Client {
        let port = self.run().addr.port();
        let code = self
            .run()
            .daemon
            .auth
            .issue_code(loom_proto::rpc::Rights::Operate);
        let (session, _) = self.run().daemon.auth.redeem(&code).unwrap();
        let mut c = Client::connect(
            port,
            &[
                ("Cookie", format!("loom_session={session}")),
                ("Origin", format!("http://localhost:{port}")),
            ],
        );
        assert_eq!(
            c.request("core.hello", hello(None, "web:t"))["result"]["role"],
            "viewer"
        );
        let r = c.request(
            "core.subscribe",
            json!({"filter": {}, "snapshot": false, "max_batch_ms": 5}),
        );
        assert!(r["result"]["sub_id"].is_string());
        c
    }
    fn shutdown(mut self) -> tempfile::TempDir {
        if let Some(r) = self.run.take() {
            r.daemon.journal.close();
            r.server.abort();
            drop(r);
        }
        let D { rt, home, .. } = self;
        rt.shutdown_timeout(Duration::from_secs(2));
        home
    }
}

fn hello(token: Option<&str>, id: &str) -> Value {
    let mut h = json!({"proto": "loom/1.0", "client_id": id, "want_role": "agent", "client": {"kind": "test", "version": "0", "tty": false}});
    if let Some(t) = token {
        h["token"] = json!(t);
    }
    h
}

struct Client {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    id: i64,
    seen: Vec<Value>,
}

impl Client {
    fn connect(port: u16, headers: &[(&str, String)]) -> Client {
        let mut req = format!("ws://127.0.0.1:{port}/rpc")
            .into_client_request()
            .unwrap();
        for (k, v) in headers {
            req.headers_mut().insert(
                tungstenite::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        let (ws, _) = tungstenite::connect(req).unwrap();
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        }
        Client {
            ws,
            id: 0,
            seen: Vec::new(),
        }
    }
    fn read(&mut self) -> Option<Value> {
        match self.ws.read() {
            Ok(Message::Text(t)) => Some(serde_json::from_str(t.as_str()).unwrap()),
            _ => None,
        }
    }
    fn request(&mut self, m: &str, p: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.ws
            .send(Message::Text(
                json!({"jsonrpc": "2.0", "id": id, "method": m, "params": p})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if let Some(v) = self.read() {
                if v["id"] == json!(id) {
                    return v;
                }
                self.seen.push(v);
            }
        }
        panic!("no reply to {m}");
    }
    fn pump(&mut self, d: Duration) {
        let end = Instant::now() + d;
        while Instant::now() < end {
            if let Some(v) = self.read() {
                self.seen.push(v);
            }
        }
    }
    fn events(&self) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|f| f["method"] == "core.events")
            .flat_map(|f| f["params"]["events"].as_array().cloned().unwrap_or_default())
            .collect()
    }
}

fn worker(a: &mut Client) -> Value {
    let st = a.request("core.status", json!({}));
    st["result"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "echo")
        .cloned()
        .unwrap()
}

fn wait_state(a: &mut Client, want: &str, within: Duration) -> Value {
    let end = Instant::now() + within;
    loop {
        let w = worker(a);
        if w["state"] == want || Instant::now() >= end {
            return w;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: u64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn kill9(pid: u64) {
    assert!(
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
}

/// kill -9 during a write: `outcome_unknown` (final row), worker crashed → ready again with a new
/// pid, the rest of the daemon keeps working, the write is not resumed.
#[test]
fn kill9_inflight_write_outcome_unknown_and_restart() {
    let d = D::start(Duration::from_millis(300));
    let mut viewer = d.viewer();
    let mut a = d.agent("agent-a");
    let w = worker(&mut a);
    assert_eq!(w["host"], "ipc");
    assert_eq!(w["state"], "ready");
    let pid = w["pid"].as_u64().unwrap();

    let token = d.run().info.token.clone();
    let port = d.run().addr.port();
    let inflight = std::thread::spawn(move || {
        let mut b = Client::connect(port, &[]);
        b.request("core.hello", hello(Some(&token), "agent-b"));
        b.request("echo.send", json!({"text": "inflight", "delay_ms": 1500}))
    });
    std::thread::sleep(Duration::from_millis(400));
    kill9(pid);
    let r = inflight.join().unwrap();
    let env = &r["result"];
    assert_eq!(env["ok"], false, "{r}");
    assert_eq!(env["error"]["detail"]["write_state"], "outcome_unknown", "{r}");
    let rid = env["error"]["request_id"].as_str().unwrap().to_owned();

    // other streams unaffected while the worker is down
    let m = a.request("core.mark", json!({"label": "during-outage"}));
    assert!(m["result"]["seq"].is_string(), "{m}");

    let w2 = wait_state(&mut a, "ready", Duration::from_secs(8));
    assert_eq!(w2["state"], "ready", "{w2}");
    assert_ne!(w2["pid"].as_u64().unwrap(), pid, "a new process");
    assert_eq!(w2["producer_epoch"], "2");
    assert_eq!(
        a.request("echo.send", json!({"text": "after"}))["result"]["ok"],
        true
    );

    viewer.pump(Duration::from_millis(600));
    let ev = viewer.events();
    let rows: Vec<&str> = ev
        .iter()
        .filter(|e| e["request_id"] == json!(rid))
        .map(|e| e["payload"]["write_state"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(rows.last(), Some(&"outcome_unknown"), "{rows:?}");
    assert!(!rows.contains(&"sent"), "not resumed after restart: {rows:?}");
    let last = ev.iter().rev().find(|e| e["request_id"] == json!(rid)).unwrap();
    assert_eq!(last["payload"]["final"], true);
    let states: Vec<&str> = ev
        .iter()
        .filter(|e| e["kind"] == "core.worker_state")
        .map(|e| e["payload"]["state"].as_str().unwrap())
        .collect();
    let crash = states
        .iter()
        .position(|s| matches!(*s, "exited" | "killed"))
        .expect("crash state");
    assert!(states[crash..].contains(&"ready"), "{states:?}");
    assert!(ev.iter().any(|e| e["kind"] == "core.mark"), "hub kept delivering");
    for g in ev.iter().filter(|e| e["kind"] == "record.gap") {
        assert_eq!(g["payload"]["cause"], "crash_tail", "{g}");
    }
    drop((a, viewer));
    d.shutdown();
}

/// STOP reaches the IPC worker (ack listed), agent writes are refused while latched.
#[test]
fn stop_latches_ipc_worker() {
    let d = D::start(Duration::from_millis(300));
    let mut viewer = d.viewer();
    let mut a = d.agent("agent-s");
    let r = a.request("core.stop", json!({"reason": "test"}));
    assert!(
        r["result"]["acked"].as_array().unwrap().contains(&json!("echo")),
        "{r}"
    );
    assert_eq!(r["result"]["missing"], json!([]));
    assert_eq!(worker(&mut a)["latch"], true);
    let r = a.request("echo.send", json!({"text": "no"}));
    assert_eq!(r["result"]["error"]["code"], "E_STOPPED");
    assert_eq!(
        viewer.request("core.stop.release", json!({}))["result"]["on"],
        false
    );
    assert_eq!(worker(&mut a)["latch"], false);
    assert_eq!(
        a.request("echo.send", json!({"text": "yes"}))["result"]["ok"],
        true
    );
    let list = a.request("core.worker.list", json!({}));
    assert!(
        list["result"]["workers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["kind"] == "echo")
    );
    drop((a, viewer));
    d.shutdown();
}

/// Gateway gone → the worker latches, seals its journal and exits after the orphan time.
#[test]
fn gateway_exit_orphans_worker() {
    let d = D::start(Duration::from_millis(400));
    let mut a = d.agent("agent-o");
    assert_eq!(
        a.request("echo.send", json!({"text": "rec"}))["result"]["ok"],
        true
    );
    let w = worker(&mut a);
    let pid = w["pid"].as_u64().unwrap();
    let session = d.run().daemon.journal.root.clone();
    drop(a);
    let home = d.shutdown();
    let end = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(pid), "orphan worker exited");
    let dir = session.join("echo").join("1");
    let files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(files.iter().any(|f| f.ends_with(".lj")), "sealed: {files:?}");
    assert!(
        !files.iter().any(|f| f.ends_with(".lj.open")),
        "nothing left open: {files:?}"
    );
    drop(home);
}

/// Five crashes in the window → quarantined: writes refused `E_WORKER_DOWN{quarantined}`; a viewer
/// `core.worker.restart` brings it back.
#[test]
fn crash_loop_quarantine_and_viewer_restart() {
    let d = D::start(Duration::from_millis(300));
    let mut viewer = d.viewer();
    let mut a = d.agent("agent-q");
    for i in 0..5 {
        let w = wait_state(&mut a, "ready", Duration::from_secs(8));
        assert_eq!(w["state"], "ready", "round {i}: {w}");
        kill9(w["pid"].as_u64().unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
    let w = wait_state(&mut a, "quarantined", Duration::from_secs(3));
    assert_eq!(w["state"], "quarantined", "{w}");
    let r = a.request("echo.send", json!({"text": "q"}));
    assert_eq!(r["result"]["error"]["code"], "E_WORKER_DOWN", "{r}");
    assert_eq!(r["result"]["error"]["detail"]["quarantined"], true);
    let r = a.request("core.worker.restart", json!({"kind": "echo"}));
    assert_eq!(
        r["error"]["data"]["code"], "E_DENIED",
        "agents cannot restart workers"
    );
    let r = viewer.request("core.worker.restart", json!({"kind": "echo"}));
    assert_eq!(r["result"]["restarted"], json!(["echo"]), "{r}");
    let w = wait_state(&mut a, "ready", Duration::from_secs(5));
    assert_eq!(w["state"], "ready");
    assert_eq!(
        a.request("echo.send", json!({"text": "back"}))["result"]["ok"],
        true
    );
    drop((a, viewer));
    d.shutdown();
}

/// Graceful stop: shutdown latch acked, the worker exits at once (orphan time is 10 s here) with a
/// sealed journal, new requests are refused, and `latch.json` is not written.
#[test]
fn graceful_shutdown_releases_worker_now() {
    let d = D::start(Duration::from_secs(10));
    let mut a = d.agent("agent-g");
    assert_eq!(
        a.request("echo.send", json!({"text": "rec"}))["result"]["ok"],
        true
    );
    let pid = worker(&mut a)["pid"].as_u64().unwrap();
    let session = d.run().daemon.journal.root.clone();
    let t0 = Instant::now();
    let summary = d.rt.block_on(d.run().daemon.shutdown_gracefully());
    assert!(
        summary["acked"].as_array().unwrap().contains(&json!("echo")),
        "{summary}"
    );
    let end = Instant::now() + Duration::from_secs(1);
    while alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !alive(pid),
        "worker released within 1 s (not the 10 s orphan path)"
    );
    eprintln!("graceful release took {:?}", t0.elapsed());
    let r = a.request("echo.send", json!({"text": "late"}));
    assert_eq!(
        r["error"]["data"]["code"], "E_DAEMON",
        "new requests refused: {r}"
    );
    assert!(
        !d.home.path().join("run/latch.json").exists(),
        "shutdown latch is not a user STOP"
    );
    let files: Vec<String> = std::fs::read_dir(session.join("echo").join("1"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        files.iter().any(|f| f.ends_with(".lj")) && !files.iter().any(|f| f.ends_with(".lj.open")),
        "{files:?}"
    );
    drop(a);
    d.shutdown();
}

/// A user STOP survives a graceful stop + restart; without one the restart is not latched.
#[test]
fn graceful_shutdown_keeps_user_stop_only() {
    let d = D::start(Duration::from_secs(10));
    let mut a = d.agent("agent-k");
    assert_eq!(a.request("core.stop", json!({}))["result"]["persisted"], true);
    d.rt.block_on(d.run().daemon.shutdown_gracefully());
    drop(a);
    let home = d.shutdown();
    let d = D::start_in(home, Duration::from_secs(10));
    let mut a = d.agent("agent-k2");
    assert_eq!(
        a.request("core.status", json!({}))["result"]["latch"],
        true,
        "user STOP kept"
    );
    let mut v = d.viewer();
    assert_eq!(v.request("core.stop.release", json!({}))["result"]["on"], false);
    d.rt.block_on(d.run().daemon.shutdown_gracefully());
    drop((a, v));
    let home = d.shutdown();
    let d = D::start_in(home, Duration::from_secs(10));
    let mut a = d.agent("agent-k3");
    assert_eq!(
        a.request("core.status", json!({}))["result"]["latch"],
        false,
        "not latched by the shutdown"
    );
    assert_eq!(
        a.request("echo.send", json!({"text": "ok"}))["result"]["ok"],
        true
    );
    d.rt.block_on(d.run().daemon.shutdown_gracefully());
    drop(a);
    d.shutdown();
}
