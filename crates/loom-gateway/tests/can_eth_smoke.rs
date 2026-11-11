//! CAN/Ethernet sim smoke through real gateway WS and subprocess IPC workers.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::net::TcpStream;
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
            self.rt.block_on(r.daemon.shutdown_gracefully());
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

#[test]
fn can_sim_gateway_open_smoke() {
    let d = D::start(Duration::from_secs(3));
    let mut a = d.agent("can-smoke");
    let status = a.request("can.status", json!({}));
    assert_eq!(status["result"]["ok"], true, "{status}");
    let open = a.request("can.open", json!({"channel":"ch0","backend":"sim"}));
    eprintln!("CAN status={status}; open={open}");
    assert_eq!(
        open["result"]["ok"], true,
        "CAN open is required for send/RX: {open}"
    );
    let mut v = d.viewer();
    let sent = a.request(
        "can.send",
        json!({"channel":"ch0","frame":"300#01","expect":{"id":769,"within_ms":1000}}),
    );
    assert_eq!(sent["result"]["ok"], true, "{sent}");
    v.pump(Duration::from_millis(200));
    let frames = v.events();
    assert!(
        frames
            .iter()
            .any(|e| e["kind"] == "can.frame" && e["payload"]["id"] == 769),
        "ECU reply must reach live viewer: {frames:?}"
    );
    assert_eq!(v.request("core.stop", json!({}))["result"]["on"], true);
    let blocked_open = a.request("can.open", json!({"channel":"ch1"}));
    assert_eq!(
        blocked_open["result"]["error"]["code"], "E_STOPPED",
        "{blocked_open}"
    );
    let blocked = a.request("can.send", json!({"channel":"ch0","frame":"300#00"}));
    assert_eq!(blocked["result"]["error"]["code"], "E_STOPPED", "{blocked}");
    let user = v.request("can.send", json!({"channel":"ch0","frame":"300#02"}));
    assert_eq!(user["result"]["ok"], true, "{user}");
    v.pump(Duration::from_millis(100));
    assert!(
        v.events()
            .iter()
            .any(|e| e["kind"] == "can.frame" && e["origin"] == "user"),
        "user TX reaches same live stream"
    );
    let closed = v.request("can.close", json!({"channel":"ch0"}));
    assert_eq!(closed["result"]["ok"], true, "{closed}");
    eprintln!("CAN smoke PASS: open, TX/ECU reply/live stream, STOP agent gate, user TX, close");
    drop((a, v));
    d.shutdown();
}

#[test]
fn eth_sim_live_send_and_stop_smoke() {
    let d = D::start(Duration::from_secs(3));
    let mut a = d.agent("eth-smoke");
    let mut v = d.viewer();
    let start = a.request(
        "eth.capture.start",
        json!({"iface":"sim0","ch":"cap","backend":"sim"}),
    );
    assert_eq!(start["result"]["ok"], true, "{start}");
    v.pump(Duration::from_millis(300));
    assert!(
        v.events().iter().any(|e| e["kind"] == "eth.pkt"),
        "live sim packets on viewer subscription"
    );
    let query = a.request("eth.query", json!({"ch":"cap","max":5,"detail":true}));
    eprintln!("ETH query={query}");
    assert_eq!(query["result"]["ok"], true, "{query}");
    let send_params = json!({"ch":"cap","kind":"raw","hex":"ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002"});
    let sent = a.request("eth.send", send_params.clone());
    eprintln!("ETH send={sent}");
    assert_eq!(sent["result"]["ok"], true, "{sent}");
    v.pump(Duration::from_millis(300));
    assert!(
        v.events()
            .iter()
            .any(|e| e["kind"] == "eth.pkt" && e["payload"]["attributed_origin"] == "agent"),
        "agent TX on same live stream"
    );
    assert_eq!(v.request("core.stop", json!({}))["result"]["on"], true);
    let blocked = a.request("eth.send", send_params.clone());
    assert_eq!(blocked["result"]["error"]["code"], "E_STOPPED", "{blocked}");
    let user = v.request("eth.send", send_params);
    assert_eq!(user["result"]["ok"], true, "{user}");
    v.pump(Duration::from_millis(300));
    assert!(
        v.events()
            .iter()
            .any(|e| e["kind"] == "eth.pkt" && e["payload"]["attributed_origin"] == "user"),
        "user TX on same live stream under STOP"
    );
    eprintln!("ETH smoke PASS: capture, query/detail, agent TX, viewer stream, STOP gate and user TX");
    drop((a, v));
    d.shutdown();
}

/// Recreate the actual IPC process: its producer epoch changes and stale channel generations
/// must not be reused. This only terminates the disposable worker started by this test.
#[test]
fn can_generation_survives_actual_worker_recreation() {
    let d = D::start(Duration::from_secs(2));
    let mut a = d.agent("can-restart-smoke");
    let opened = a.request("can.open", json!({"channel":"ch0","nodes":"none"}));
    assert_eq!(opened["result"]["ok"], true, "{opened}");
    let old_generation = opened["result"]["data"]["generation"].as_u64().unwrap();
    let status = a.request("core.status", json!({}));
    let worker = status["result"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == loom_can::WORKER_KIND_SIM)
        .unwrap();
    let pid = worker["pid"].as_u64().unwrap();
    let epoch = worker["producer_epoch"].as_str().unwrap().parse::<u64>().unwrap();
    assert!(
        std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let until = Instant::now() + Duration::from_secs(8);
    let mut ready = false;
    while Instant::now() < until {
        let status = a.request("core.status", json!({}));
        ready = status["result"]["workers"].as_array().unwrap().iter().any(|w| {
            w["kind"] == loom_can::WORKER_KIND_SIM
                && w["state"] == "ready"
                && w["producer_epoch"]
                    .as_str()
                    .and_then(|x| x.parse::<u64>().ok())
                    .is_some_and(|e| e > epoch)
        });
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(ready, "CAN worker failed to restart");
    let reopened = a.request("can.open", json!({"channel":"ch0","nodes":"none"}));
    assert_eq!(reopened["result"]["ok"], true, "{reopened}");
    assert!(
        reopened["result"]["data"]["generation"].as_u64().unwrap() > old_generation,
        "{opened} -> {reopened}"
    );
    drop(a);
    d.shutdown();
}
