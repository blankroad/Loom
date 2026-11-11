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
fn uds_sim_request_reaches_live_viewer() {
    let d = D::start(Duration::from_secs(2));
    let mut a = d.agent("uds-smoke");
    let mut viewer = d.viewer();
    let connected = a.request("uds.connect", json!({"name":"ecu","sim":true}));
    assert!(code(&connected).is_null(), "{connected}");
    let response = a.request("uds.request", json!({"target":"ecu","bytes_hex":"22F190"}));
    assert!(code(&response).is_null(), "{response}");
    assert_eq!(
        response["result"]["data"]["row"]["write_state"], "confirmed",
        "{response}"
    );
    viewer.pump(Duration::from_millis(300));
    let events = viewer.events();
    for kind in ["uds.tx", "uds.rx", "uds.result"] {
        assert!(
            events.iter().any(|event| event["kind"] == kind),
            "missing {kind}: {events:?}"
        );
    }
    assert_eq!(viewer.request("core.stop", json!({}))["result"]["on"], true);
    let blocked = a.request("uds.request", json!({"target":"ecu","bytes_hex":"22F190"}));
    assert_eq!(code(&blocked), "E_STOPPED", "{blocked}");
    let user = viewer.request("uds.request", json!({"target":"ecu","bytes_hex":"22F190"}));
    assert!(code(&user).is_null(), "{user}");
    assert_eq!(
        user["result"]["data"]["row"]["write_state"], "confirmed",
        "{user}"
    );
    let epoch = d.run().daemon.hub.epoch;
    let live_uds: Vec<_> = events
        .iter()
        .filter(|e| ["uds.tx", "uds.rx", "uds.result"].contains(&e["kind"].as_str().unwrap_or_default()))
        .cloned()
        .collect();
    drop((a, viewer));
    let home = d.shutdown();
    std::thread::sleep(Duration::from_millis(1100));
    let restarted = D::start_in(home, Duration::from_secs(2));
    let mut reader = restarted.agent("uds-journal-smoke");
    let journal = reader.request("core.events.query", json!({"since":format!("e{epoch}:seq:1"),"filter":{"kinds":["uds.tx","uds.rx","uds.result"]},"max":100,"peek":true}));
    assert_eq!(journal["result"]["source"], "journal", "{journal}");
    let recorded = journal["result"]["events"].as_array().unwrap();
    for event in live_uds {
        assert!(recorded.contains(&event), "missing journal event {event}");
    }
    drop(reader);
    restarted.shutdown();
}

#[test]
fn tc1_start_reports_current_gateway_wiring() {
    let d = D::start(Duration::from_secs(2));
    let mut a = d.agent("tc1-smoke");
    let r = a.request(
        "test.start",
        json!({
            "tc":{"text":include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md")},
            "plan":{"text":include_str!("../../loom-test-format/tests/fixtures/TC-1.plan.yaml")}
        }),
    );
    eprintln!("TC-1 start: {r}");
    // The real Registry command/risk table is injected: the fixture now fails the plan check on
    // names/arguments no domain registered (details: tests/test_services_smoke.rs).
    assert_eq!(
        code(&r),
        "E_USAGE",
        "TC-1 fixture must not start against the real registry: {r}"
    );
    drop(a);
    d.shutdown();
}

/// Only a domain-declared pure preview skips the write pipeline; arbitrary dry_run never does.
#[test]
fn registered_preview_is_read_only_even_during_stop() {
    let d = D::start(Duration::from_secs(2));
    let mut agent = d.agent("preview-smoke");
    let mut viewer = d.viewer_as(loom_proto::rpc::Rights::Watch);
    assert_eq!(viewer.request("core.stop", json!({}))["result"]["on"], true);
    for client in [&mut agent, &mut viewer] {
        let preview = client.request(
            "uds.request",
            json!({"target":"unopened","bytes_hex":"22F190","dry_run":true}),
        );
        assert!(code(&preview).is_null(), "{preview}");
        assert!(preview["result"]["data"]["risk"].is_string(), "{preview}");
    }
    let forged = agent.request("echo.send", json!({"text":"must not send","dry_run":true}));
    assert_eq!(code(&forged), "E_STOPPED", "{forged}");
    let write = viewer.request(
        "uds.request",
        json!({"target":"unopened","bytes_hex":"22F190","dry_run":false}),
    );
    assert_eq!(code(&write), "E_DENIED", "{write}");
    viewer.pump(Duration::from_millis(200));
    assert!(
        !viewer.events().iter().any(|event| event["domain"] == "uds"),
        "preview must publish no UDS rows"
    );
    drop((agent, viewer));
    d.shutdown();
}
