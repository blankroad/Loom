//! UDS/TC-1 real gateway RPC wiring smoke; isolated temporary home.
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
        self.viewer_as(loom_proto::rpc::Rights::Operate)
    }
    fn viewer_as(&self, rights: loom_proto::rpc::Rights) -> Client {
        let port = self.run().addr.port();
        let code = self.run().daemon.auth.issue_code(rights);
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

fn code(r: &Value) -> &Value {
    if !r["error"].is_null() {
        &r["error"]["data"]["code"]
    } else {
        &r["result"]["error"]["code"]
    }
}

#[test]
fn previews_are_pure_reads_while_stop_blocks_real_operations() {
    let d = D::start(Duration::from_secs(2));
    let mut watch = d.viewer_as(loom_proto::rpc::Rights::Watch);
    assert_eq!(watch.request("core.stop", json!({}))["result"]["on"], true);
    for (method, args) in [
        (
            "uds.request",
            json!({"target":"ecu","bytes_hex":"2EF1A002","dry_run":true}),
        ),
        (
            "eth.send",
            json!({"ch":"cap","kind":"raw","hex":"ffffffffffff02000000000108060001080006040001020000000001c0a80001000000000000c0a80002","dry_run":true}),
        ),
        ("psu.set", json!({"ch":"bench","volt":5.0,"dry_run":true})),
        ("psu.out", json!({"ch":"bench","state":"off","dry_run":true})),
        ("psu.cycle", json!({"ch":"bench","off_ms":200,"dry_run":true})),
    ] {
        let r = watch.request(method, args);
        assert!(code(&r).is_null(), "{method}: {r}");
        assert_eq!(r["result"]["data"]["dry_run"], true, "{r}");
        assert!(
            r["result"]["state"].is_null(),
            "a preview is not a confirmed write: {r}"
        );
    }
    watch.pump(Duration::from_millis(100));
    assert!(
        !watch
            .events()
            .iter()
            .any(|e| e["payload"].get("write_state").is_some()),
        "no preview write rows"
    );
    drop(watch);
    d.shutdown();
}
#[test]
fn hello_declares_auto_policy_only_without_approval_rules() {
    for (policy, is_auto) in [
        ("", true),
        ("[policy.actions]\n\"can.send\"=\"deny\"\n", true),
        ("[policy.actions]\n\"can.send\"=\"confirm\"\n", false),
        ("[policy.actions]\n\"can.send\"=\"approve\"\n", false),
    ] {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("loom.toml"), policy).unwrap();
        let d = D::start_in(home, Duration::from_secs(2));
        let mut c = Client::connect(d.run().addr.port(), &[]);
        let r = c.request("core.hello", hello(Some(&d.run().info.token), "auto-policy-test"));
        let caps = r["result"]["capabilities"].as_array().unwrap();
        assert_eq!(caps.iter().any(|c| c == "policy/auto"), is_auto, "{r}");
        drop(c);
        d.shutdown();
    }
}
