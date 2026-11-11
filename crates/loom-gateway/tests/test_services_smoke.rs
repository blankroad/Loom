//! TEST real Registry command/risk adapter through the production daemon; isolated temporary home.
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

fn code(r: &Value) -> &Value {
    if !r["error"].is_null() {
        &r["error"]["data"]["code"]
    } else {
        &r["result"]["error"]["code"]
    }
}

const TC1_TC: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md");
const TC1_PLAN: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.plan.yaml");

/// TC-1 steps rewritten with the commands/arguments the domains actually register. No `finally`:
/// the registered PSU safe state (`psu.out off`) is R3 and the format allows only R0–R1 there.
const REGISTERED_PLAN: &str = r#"loom_tc_plan: 1
env: sim
steps:
  - id: S1
    tc_step: 1
    do: [{psu.out: {ch: bench, state: "off"}}, {sleep: 2s}, {psu.out: {ch: bench, state: "on"}}]
  - id: S2
    tc_step: 2
    uart.expect: {channel: dut, patterns: ["login:"], timeout: 30s}
    expect:
      - filter: {domain: uart, channel: dut, kinds: [uart.data], origins: [device]}
        pred: {op: uart.text_contains, pattern: "login:"}
        within_ms: 30000
  - id: S3
    tc_step: 3
    can.sig.wait_for: {channel: ch0, expr: "VehState.PowerMode == 2", within_ms: 5000, timeout: 6s}
  - id: S4
    tc_step: 4
    uds.dtc.list: {target: bcm, mask: 0x09}
"#;

#[test]
fn tc1_fixture_is_checked_against_the_real_registry() {
    let d = D::start(Duration::from_secs(2));
    let mut a = d.agent("tc1-registry");
    let r = a.request(
        "test.start",
        json!({"tc":{"text":TC1_TC},"plan":{"text":TC1_PLAN}}),
    );
    eprintln!("TC-1 start: {r}");
    // Not E_NOT_IMPLEMENTED any more: the plan reaches the real command/risk table and fails on
    // the fixture's names/arguments that no domain registered.
    assert_eq!(code(&r), "E_USAGE", "{r}");
    let detail = if r["error"].is_null() {
        &r["result"]["error"]["detail"]
    } else {
        &r["error"]["data"]["detail"]
    };
    let errors = detail["errors"].as_array().cloned().unwrap_or_default();
    let has = |c: &str, needle: &str| {
        errors
            .iter()
            .any(|e| e["code"] == c && e.to_string().contains(needle))
    };
    assert!(has("unknown_command", "can.sig.wait-for"), "{detail}");
    assert!(has("unknown_command", "uds.dtc.clear"), "{detail}");
    assert!(has("arguments", "uart.expect"), "{detail}");
    assert!(has("unsafe_finally", "psu.out"), "{detail}");
    let st = a.request("test.status", json!({}));
    assert_eq!(
        st["result"]["data"]["sessions"],
        json!([]),
        "no session may start: {st}"
    );
    drop(a);
    d.shutdown();
}

#[test]
fn registered_plan_starts_but_cannot_claim_pass() {
    let d = D::start(Duration::from_secs(2));
    let mut a = d.agent("tc1-registered");
    let mut viewer = d.viewer();
    let r = a.request(
        "test.start",
        json!({"tc":{"text":TC1_TC},"plan":{"text":REGISTERED_PLAN}}),
    );
    eprintln!("registered start: {r}");
    assert!(code(&r).is_null(), "{r}");
    let data = &r["result"]["data"];
    assert_eq!(data["state"], "RUNNING", "{r}");
    let session = data["session_id"].as_str().unwrap().to_owned();
    // Risk summary comes from the domains' hooks (PSU output = R3), not a default.
    assert!(data["risk_summary"].to_string().contains("R3"), "{r}");

    // Ordered plan: S2 cannot start before S1.
    let early = a.request("test.step", json!({"session_id": session, "id": "S2"}));
    assert_eq!(code(&early), "E_DENIED", "{early}");
    // S1 begins without a CORE source fence (NotWiredHooks).
    let s1 = a.request("test.step", json!({"session_id": session, "id": "S1"}));
    assert!(code(&s1).is_null(), "{s1}");
    assert_eq!(s1["result"]["data"]["source_fence_available"], false, "{s1}");
    let done = a.request(
        "test.verdict",
        json!({"session_id": session, "step": "S1", "verdict": "done"}),
    );
    eprintln!("S1 done: {done}");
    assert_eq!(
        code(&done),
        "E_EVIDENCE_MISSING",
        "unconfirmed stimulus is not done: {done}"
    );
    let pass = a.request(
        "test.verdict",
        json!({"session_id": session, "step": "S1", "verdict": "pass"}),
    );
    eprintln!("S1 pass: {pass}");
    assert_eq!(
        code(&pass),
        "E_EVIDENCE_MISSING",
        "PASS needs verified evidence: {pass}"
    );
    let st = a.request("test.status", json!({"session_id": session}));
    let st_text = st.to_string().to_lowercase();
    assert!(!st_text.contains("\"effective\":\"pass\""), "{st}");
    assert!(!st_text.contains("\"effective\":\"done\""), "{st}");
    viewer.pump(Duration::from_millis(300));
    let events = viewer.events();
    assert!(
        events.iter().any(|e| e["kind"] == "test.session_started"),
        "viewer sees the session start live: {events:?}"
    );
    drop((a, viewer));
    d.shutdown();
}
