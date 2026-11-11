//! `loomd --fixture-replay` over a real WebSocket (tungstenite client). Each test < 10 s.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use loom_gateway::replay::{ReplayConfig, Scenario, serve};
use loom_proto::rpc::Rights;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

fn start(cfg: ReplayConfig) -> SocketAddr {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let (addr, handle) = serve(cfg).await.unwrap();
            tx.send(addr).unwrap();
            let _ = handle.await;
        });
    });
    rx.recv_timeout(Duration::from_secs(5)).unwrap()
}

struct Client {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: i64,
}

impl Client {
    fn connect(addr: SocketAddr) -> Self {
        let (ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{}/rpc", addr.port())).unwrap();
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        }
        Client { ws, next_id: 0 }
    }

    fn send_raw(&mut self, text: String) {
        self.ws.send(Message::Text(text.into())).unwrap();
    }

    fn call(&mut self, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        self.send_raw(
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}).to_string(),
        );
        self.next_id
    }

    /// Next JSON text frame, or `None` after `within`.
    fn recv(&mut self, within: Duration) -> Option<Value> {
        let end = Instant::now() + within;
        while Instant::now() < end {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Some(serde_json::from_str(t.as_str()).unwrap()),
                Ok(Message::Close(_)) => return None,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Read frames until `pred` matches; returns all frames read (the match last).
    fn until(&mut self, within: Duration, pred: impl Fn(&Value) -> bool) -> Vec<Value> {
        let end = Instant::now() + within;
        let mut seen = Vec::new();
        while Instant::now() < end {
            if let Some(v) = self.recv(end - Instant::now()) {
                let hit = pred(&v);
                seen.push(v);
                if hit {
                    return seen;
                }
            }
        }
        panic!(
            "condition not met in {within:?}; last frames: {:?}",
            seen.iter().rev().take(3).collect::<Vec<_>>()
        );
    }

    fn response(&mut self, id: i64) -> (Value, Vec<Value>) {
        let mut frames = self.until(Duration::from_secs(5), |v| v["id"] == json!(id));
        let r = frames.pop().unwrap();
        (r, frames)
    }

    fn hello(&mut self) -> Value {
        let id = self.call("core.hello", json!({"proto": "loom/1.0", "client_id": "test#1", "want_role": "viewer", "client": {"kind": "test", "version": "0"}}));
        self.response(id).0["result"].clone()
    }
}

fn cfg(scenario: Scenario, speed: f64) -> ReplayConfig {
    ReplayConfig {
        scenario,
        speed,
        ..ReplayConfig::default()
    }
}

fn is_note(v: &Value, method: &str) -> bool {
    v["method"] == json!(method)
}

#[test]
fn hello_subscribe_snapshot_events_heartbeat_time() {
    let addr = start(cfg(Scenario::Normal, 5.0));
    let mut c = Client::connect(addr);
    let h = c.hello();
    assert_eq!(
        (h["role"].as_str(), h["rights"].as_str(), h["env"].as_str()),
        (Some("viewer"), Some("operate"), Some("sim"))
    );
    let id = c.call(
        "core.subscribe",
        json!({"filter": {"domains": ["uart", "can"]}, "snapshot": true, "max_batch_ms": 10}),
    );
    let (r, _) = c.response(id);
    let sub = r["result"]["sub_id"].as_str().unwrap().to_owned();
    let frames = c.until(Duration::from_secs(3), |v| is_note(v, "core.snapshot_end"));
    assert!(frames.iter().any(|v| is_note(v, "core.snapshot_part")));
    let h_seq: u64 = frames.last().unwrap()["params"]["stream_seq"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let ev = c.until(Duration::from_secs(3), |v| {
        is_note(v, "core.events") && v["params"]["sub"] == json!(sub)
    });
    let batch = &ev.last().unwrap()["params"];
    let events = batch["events"].as_array().unwrap();
    assert!(!events.is_empty() && events.len() <= 200);
    for e in events {
        assert!(
            e["seq"].as_str().unwrap().parse::<u64>().unwrap() > h_seq,
            "live after snapshot H"
        );
        assert!(["uart", "can"].contains(&e["domain"].as_str().unwrap()));
    }
    assert!(batch["hub_publish_mono_ns"].is_string());
    c.until(Duration::from_secs(3), |v| {
        is_note(v, "core.events") && v["params"]["sub"] == json!("control")
    });
    c.until(Duration::from_secs(2), |v| is_note(v, "core.heartbeat"));
    let id = c.call("core.time", json!({}));
    let t = c.response(id).0["result"].clone();
    let d1: u64 = t["recv_mono_ns"].as_str().unwrap().parse().unwrap();
    let d2: u64 = t["send_mono_ns"].as_str().unwrap().parse().unwrap();
    assert!(d1 <= d2);
}

#[test]
fn echo_write_rows_arrive_before_reply() {
    let addr = start(ReplayConfig {
        echo_writes: true,
        ..cfg(Scenario::Normal, 1.0)
    });
    let mut c = Client::connect(addr);
    c.hello();
    let id = c.call("uart.send", json!({"channel": "dut", "data": "6c730a"}));
    let (r, before) = c.response(id);
    let rid = r["result"]["request_id"].as_str().unwrap().to_owned();
    let states: Vec<String> = before
        .iter()
        .filter(|v| is_note(v, "core.events") && v["params"]["sub"] == json!("control"))
        .flat_map(|v| v["params"]["events"].as_array().unwrap().clone())
        .filter(|e| e["request_id"] == json!(rid))
        .map(|e| {
            assert_eq!(e["origin"], json!("user"));
            assert_eq!(e["quality"], json!(["simulated"]));
            e["payload"]["write_state"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        states,
        ["requested", "accepted", "sent", "confirmed"],
        "publish-before-reply"
    );

    let id = c.call("core.stop", json!({}));
    let (_, before) = c.response(id);
    assert!(before.iter().any(|v| {
        v["params"]["events"]
            .as_array()
            .is_some_and(|a| a.iter().any(|e| e["kind"] == json!("core.latch")))
    }));
    let id = c.call("core.safe_state.run", json!({"ids": ["uds.recover", "nope"]}));
    let items = c.response(id).0["result"]["items"].clone();
    assert_eq!(items[0]["outcome"], json!("sent"));
    assert_eq!(items[1]["outcome"], json!("skipped"));
    let id = c.call("uart.frobnicate", json!({}));
    assert_eq!(
        c.response(id).0["error"]["data"]["code"],
        json!("E_NOT_IMPLEMENTED")
    );
}

#[test]
fn watch_rights_cannot_write() {
    let addr = start(ReplayConfig {
        echo_writes: true,
        rights: Rights::Watch,
        ..cfg(Scenario::Normal, 1.0)
    });
    let mut c = Client::connect(addr);
    assert_eq!(c.hello()["rights"], json!("watch"));
    let id = c.call("uart.send", json!({}));
    assert_eq!(c.response(id).0["error"]["data"]["code"], json!("E_DENIED"));
}

#[test]
fn saturate_closes_then_reconnect_snapshots() {
    let addr = start(cfg(Scenario::Saturate, 10.0));
    let mut c = Client::connect(addr);
    c.hello();
    let id = c.call("core.subscribe", json!({"snapshot": true}));
    c.response(id);
    let f = c.until(Duration::from_secs(5), |v| is_note(v, "core.closed"));
    assert_eq!(f.last().unwrap()["params"]["reason"], json!("slow_consumer"));
    let mut c2 = Client::connect(addr);
    c2.hello();
    let id = c2.call("core.subscribe", json!({"snapshot": true}));
    c2.response(id);
    c2.until(Duration::from_secs(3), |v| is_note(v, "core.snapshot_end"));
}

#[test]
fn late_join_gets_backlog() {
    let addr = start(cfg(Scenario::Late, 1.0));
    let mut c = Client::connect(addr);
    c.hello();
    let id = c.call("core.subscribe", json!({"snapshot": true}));
    c.response(id);
    let frames = c.until(Duration::from_secs(3), |v| is_note(v, "core.snapshot_end"));
    let n: usize = frames
        .iter()
        .filter(|v| is_note(v, "core.snapshot_part"))
        .map(|v| v["params"]["events"].as_array().map_or(0, Vec::len))
        .sum();
    assert!(n >= 1000, "backlog {n}");
}

#[test]
fn protocol_errors() {
    let addr = start(cfg(Scenario::Normal, 1.0));
    let mut c = Client::connect(addr);
    let id = c.call("core.subscribe", json!({}));
    assert_eq!(c.response(id).0["error"]["code"], json!(-32600), "hello first");
    c.hello();
    c.send_raw("{".into());
    assert_eq!(
        c.until(Duration::from_secs(2), |v| v.get("error").is_some())
            .last()
            .unwrap()["error"]["code"],
        json!(-32700)
    );
    c.send_raw(json!([{"jsonrpc": "2.0", "id": 1, "method": "core.time"}]).to_string());
    assert_eq!(
        c.until(Duration::from_secs(2), |v| v.get("error").is_some())
            .last()
            .unwrap()["error"]["code"],
        json!(-32600)
    );
    c.send_raw(
        json!({"jsonrpc": "2.0", "id": 99, "method": "core.time", "params": {"pad": "x".repeat(70 * 1024)}})
            .to_string(),
    );
    assert_eq!(
        c.until(Duration::from_secs(2), |v| v.get("error").is_some())
            .last()
            .unwrap()["error"]["code"],
        json!(-32600)
    );
    let id = c.call("nope", json!({}));
    assert_eq!(c.response(id).0["error"]["code"], json!(-32601));
    for _ in 0..8 {
        let id = c.call("core.subscribe", json!({}));
        c.response(id);
    }
    let id = c.call("core.subscribe", json!({}));
    assert_eq!(c.response(id).0["error"]["data"]["code"], json!("E_BUSY"));
}

#[test]
fn origin_allowlist() {
    let addr = start(ReplayConfig {
        allow_origins: vec!["http://localhost:5173".into()],
        ..cfg(Scenario::Normal, 1.0)
    });
    let url = format!("ws://127.0.0.1:{}/rpc", addr.port());
    let mut bad = url.as_str().into_client_request().unwrap();
    bad.headers_mut()
        .insert("Origin", "http://evil.example".parse().unwrap());
    assert!(tungstenite::connect(bad).is_err());
    let mut ok = url.as_str().into_client_request().unwrap();
    ok.headers_mut()
        .insert("Origin", "http://localhost:5173".parse().unwrap());
    assert!(tungstenite::connect(ok).is_ok());
}

#[test]
fn highload_slow_client_gets_honest_gap() {
    let addr = start(cfg(Scenario::Highload, 1.0));
    let mut c = Client::connect(addr);
    c.hello();
    let id = c.call(
        "core.subscribe",
        json!({"filter": {"domains": ["can"]}, "snapshot": false}),
    );
    c.response(id);
    std::thread::sleep(Duration::from_secs(3)); // do not read: the server must drop, not grow
    let f = c.until(Duration::from_secs(5), |v| {
        v["params"]["display_gap"].is_object()
            || v["params"]["events"]
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e["kind"] == json!("display.gap")))
    });
    let last = f.last().unwrap();
    let gap_cause = last["params"]["display_gap"]["causes"][0]
        .as_str()
        .or_else(|| {
            last["params"]["events"].as_array().and_then(|a| {
                a.iter()
                    .find(|e| e["kind"] == json!("display.gap"))
                    .and_then(|e| e["payload"]["cause"].as_str())
            })
        })
        .unwrap();
    assert_eq!(gap_cause, "slow_client");
}
