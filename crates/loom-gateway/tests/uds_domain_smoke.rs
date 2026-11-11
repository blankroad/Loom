//! UDS domain smoke over the real gateway: production `daemon::start` in an isolated temporary
//! home, real WebSocket clients (agent + viewer), `uds.connect {sim:true}` = virtual ECU behind a
//! loopback DoIP entity reached by the real DoIP client and runner. No hardware, no user daemon.
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

/// Events of `rid` seen by the viewer, as `(kind, write_state)`.
fn rows(events: &[Value], rid: &str) -> Vec<(String, String)> {
    events
        .iter()
        .filter(|e| e["request_id"] == json!(rid))
        .map(|e| {
            (
                e["kind"].as_str().unwrap_or("").to_owned(),
                e["payload"]["write_state"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn result_data(r: &Value) -> &Value {
    &r["result"]["data"]
}

/// Viewer events until `pred` holds (bounded).
fn wait_events(v: &mut Client, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        let ev = v.events();
        if pred(&ev) {
            return ev;
        }
        v.pump(Duration::from_millis(50));
    }
    v.events()
}

#[test]
fn uds_sim_connect_request_reply_and_live_events() {
    let d = D::start(Duration::from_secs(2));
    let mut v = d.viewer();
    let mut a = d.agent("uds-domain-smoke");

    // Nothing connected yet: explicit error, never a fake success.
    let r = a.request("uds.status", json!({"target": "ecu1"}));
    eprintln!("status before connect: {r}");
    assert_eq!(code(&r), "E_NOT_OPEN", "{r}");
    let r = a.request("uds.request", json!({"target": "ecu1", "text": "22 F1 90"}));
    eprintln!("request before connect: {r}");
    assert_eq!(code(&r), "E_NOT_OPEN", "{r}");

    let r = a.request("uds.connect", json!({"name": "ecu1", "sim": "lab"}));
    assert_eq!(code(&r), "E_USAGE", "unknown sim profile: {r}");

    // connect → virtual ECU over loopback DoIP (`--sim bench`).
    let r = a.request("uds.connect", json!({"name": "ecu1", "sim": "bench"}));
    eprintln!("connect: {r}");
    let c = result_data(&r);
    assert_eq!(c["existing"], false, "{r}");
    assert_eq!(c["state"]["generation"], 1, "{r}");
    assert_eq!(c["state"]["backend"], "sim", "{r}");
    assert!(c["entity"].as_str().unwrap().starts_with("127.0.0.1:"), "{r}");

    let r = a.request("uds.status", json!({"target": "ecu1"}));
    eprintln!("status: {r}");
    assert_eq!(result_data(&r)["targets"][0]["target"], "ecu1", "{r}");

    // request → exact positive response, decoded, row confirmed.
    let r = a.request("uds.request", json!({"target": "ecu1", "text": "22 F1 90"}));
    eprintln!("request 22F190: {r}");
    let data = result_data(&r);
    let rid = data["request_id"].as_str().expect("request_id").to_owned();
    let row = &data["row"];
    assert_eq!(row["write_state"], "confirmed", "{r}");
    assert_eq!(
        row["resp_bytes"]["data"],
        format!("62f190{}", hex(b"LOOMSIM0000000001")),
        "{r}"
    );
    assert_eq!(row["decoded"][0]["value"], "LOOMSIM0000000001", "{r}");

    // The viewer sees the same request live: tx (bytes) → rx (response) → final result.
    // Control rows can overtake the batched data lane on the wire. Both records must arrive;
    // stream seq (the journal order) must still put device RX before the final result.
    let mut ev = wait_events(&mut v, |ev| {
        let r = rows(ev, &rid);
        r.iter().any(|(k, _)| k == "uds.result") && r.iter().any(|(k, _)| k == "uds.rx")
    });
    ev.sort_by_key(|e| {
        e["seq"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| e["seq"].as_u64())
            .expect("stream seq")
    });
    let mine: Vec<&Value> = ev.iter().filter(|e| e["request_id"] == json!(rid)).collect();
    eprintln!("viewer rows for {rid}: {:?}", rows(&ev, &rid));
    assert_eq!(
        rows(&ev, &rid),
        [
            ("uds.tx".into(), "requested".into()),
            ("uds.tx".into(), "accepted".into()),
            ("uds.tx".into(), "sent".into()),
            ("uds.rx".into(), String::new()),
            ("uds.result".into(), "confirmed".into()),
        ],
        "one row, monotonic states, no duplicates"
    );
    let sent = mine
        .iter()
        .find(|e| e["kind"] == "uds.tx" && e["payload"]["write_state"] == "sent")
        .expect("uds.tx sent");
    assert_eq!(sent["payload"]["bytes"]["data"], "22f190");
    assert_eq!(sent["origin"], "agent");
    let rx = mine.iter().find(|e| e["kind"] == "uds.rx").expect("uds.rx");
    assert_eq!(rx["payload"]["bytes"]["data"], row["resp_bytes"]["data"]);
    assert_eq!(rx["payload"]["kind"], "positive");
    let fin = mine
        .iter()
        .rfind(|e| e["kind"] == "uds.result")
        .expect("uds.result");
    assert_eq!(fin["payload"]["write_state"], "confirmed");
    assert_eq!(fin["payload"]["final"], true, "core marks the terminal row");
    let seq = |e: &Value| -> u64 {
        e["seq"]
            .as_u64()
            .or_else(|| e["seq"].as_str().and_then(|x| x.parse().ok()))
            .expect("seq")
    };
    assert!(seq(sent) < seq(rx) && seq(rx) < seq(fin), "tx < rx < result");
    // Row times are on the hub clock (same base as the events' `ts.mono_ns`).
    let ns = |v: &Value| -> i128 {
        v.as_u64()
            .map(i128::from)
            .or_else(|| v.as_str().and_then(|x| x.parse().ok()))
            .expect("ns")
    };
    let skew = ns(&row["t_send_mono_ns"]) - ns(&sent["ts"]["mono_ns"]);
    assert!(skew.abs() < 1_000_000_000, "row vs event time skew {skew} ns");

    // Negative response: E_NRC with the exact NRC, the row is not confirmed.
    let r = a.request("uds.request", json!({"target": "ecu1", "text": "22 12 34"}));
    eprintln!("request 221234: {r}");
    assert_eq!(code(&r), "E_NRC", "{r}");
    let msg = r["error"]["message"]
        .as_str()
        .or_else(|| r["result"]["error"]["message"].as_str())
        .unwrap_or("");
    assert!(msg.contains("0x31"), "{r}");

    // User intervention: a viewer write takes the lease; the agent's R1 write is refused
    // before sending; after the viewer releases it, the agent may write again.
    let r = v.request("uds.session", json!({"target": "ecu1", "session": "extended"}));
    eprintln!("viewer session extended: {r}");
    assert_eq!(result_data(&r)["row"]["write_state"], "confirmed", "{r}");
    let r = a.request("uds.session", json!({"target": "ecu1", "session": "default"}));
    eprintln!("agent session default while user holds: {r}");
    let c = code(&r).as_str().unwrap_or("").to_owned();
    assert!(c == "E_HELD_BY_USER" || c == "E_LOCKED", "{r}");
    let r = v.request("uds.lease.release", json!({"target": "ecu1"}));
    eprintln!("viewer release: {r}");
    assert!(r["error"].is_null() && r["result"]["ok"] != false, "{r}");
    let r = a.request("uds.session", json!({"target": "ecu1", "session": "default"}));
    eprintln!("agent session default after release: {r}");
    assert_eq!(result_data(&r)["row"]["write_state"], "confirmed", "{r}");

    let r = a.request("uds.status", json!({"target": "ecu1"}));
    eprintln!("final status: {r}");
    drop((a, v));
    d.shutdown();
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
