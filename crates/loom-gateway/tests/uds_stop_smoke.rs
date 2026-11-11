//! CORE STOP → in-progress UDS cancel over the real gateway.
//!
//! Production `daemon::start` in an isolated temporary home, real WebSocket clients, and an ECU
//! that answers slowly: the bench virtual ECU with a `0x78`-pending fault behind a real loopback
//! DoIP entity, reached by the agent with `uds.connect {host, port}` (the real DoIP client and
//! runner). STOP comes from a viewer's `core.stop`, so the cancel goes through `Daemon::stop` →
//! `WriteHooks::on_latch` — no crate-internal `stop_latch` call. No hardware, no user daemon.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_gateway::daemon::{Config, Running, start};
use loom_gateway::home::Home;
use loom_uds::sim_support::loom_uds_doip::{DoipEntity, EntityConfig};
use loom_uds::sim_support::loom_uds_sim::{FaultAction, FaultMatch, FaultRule, SimEcu, SimNetwork};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const ENTITY_LA: u16 = 0x1000;
const ECU_LA: u16 = 0x1001;

struct D {
    rt: tokio::runtime::Runtime,
    run: Option<Running>,
    home: tempfile::TempDir,
}

impl D {
    fn start() -> D {
        let home = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.worker_program = Some(env!("CARGO_BIN_EXE_loomd").into());
        cfg.worker_orphan = Some(Duration::from_secs(2));
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
    fn viewer(&self, id: &str, subscribe: bool) -> Client {
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
            c.request("core.hello", hello(None, id))["result"]["role"],
            "viewer"
        );
        if subscribe {
            let r = c.request(
                "core.subscribe",
                json!({"filter": {}, "snapshot": false, "max_batch_ms": 5}),
            );
            assert!(r["result"]["sub_id"].is_string());
        }
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
            s.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
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
    /// Send without waiting; the reply is collected by [`Client::wait`].
    fn send(&mut self, m: &str, p: Value) -> i64 {
        self.id += 1;
        let id = self.id;
        self.ws
            .send(Message::Text(
                json!({"jsonrpc": "2.0", "id": id, "method": m, "params": p})
                    .to_string()
                    .into(),
            ))
            .unwrap();
        id
    }
    fn wait(&mut self, id: i64, within: Duration) -> Value {
        let end = Instant::now() + within;
        while Instant::now() < end {
            if let Some(i) = self.seen.iter().position(|v| v["id"] == json!(id)) {
                return self.seen.remove(i);
            }
            if let Some(v) = self.read() {
                if v["id"] == json!(id) {
                    return v;
                }
                self.seen.push(v);
            }
        }
        panic!("no reply to request {id} within {within:?}");
    }
    fn request(&mut self, m: &str, p: Value) -> Value {
        let id = self.send(m, p);
        self.wait(id, Duration::from_secs(15))
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
    /// Pump until `pred` holds on the events seen so far (bounded).
    fn wait_events(&mut self, what: &str, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            let ev = self.events();
            if pred(&ev) {
                return ev;
            }
            self.pump(Duration::from_millis(20));
        }
        panic!("viewer never saw {what}: {:?}", self.events());
    }
}

fn code(r: &Value) -> &Value {
    if !r["error"].is_null() {
        &r["error"]["data"]["code"]
    } else {
        &r["result"]["error"]["code"]
    }
}

fn detail(r: &Value) -> &Value {
    if !r["error"].is_null() {
        &r["error"]["data"]["detail"]
    } else {
        &r["result"]["error"]["detail"]
    }
}

fn rid_of(e: &Value) -> String {
    e["request_id"].as_str().unwrap_or_default().to_owned()
}

/// `(kind, write_state)` of the viewer events of `rid`, in order.
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

fn is_sent(e: &Value) -> bool {
    e["kind"] == "uds.tx" && e["payload"]["write_state"] == "sent"
}

fn sent_of(events: &[Value], rid: &str) -> usize {
    events
        .iter()
        .filter(|e| e["request_id"] == json!(rid) && is_sent(e))
        .count()
}

/// The bench ECU behind a real loopback DoIP entity; every `0x2E` write gets 63 × `0x78`, 200 ms
/// apart (≈ 12.6 s), before its answer — long enough to be in flight when STOP comes.
fn slow_write_ecu(rt: &tokio::runtime::Runtime) -> DoipEntity {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::always(
        FaultMatch::sid(0x2E),
        FaultAction::PendingThen {
            count: 63,
            every: Duration::from_millis(200),
        },
    ))
    .unwrap();
    let mut net = SimNetwork::new();
    net.insert(ECU_LA, ecu).unwrap();
    let net = Arc::new(Mutex::new(net));
    rt.block_on(DoipEntity::start(
        EntityConfig::loopback(ENTITY_LA, vec![ECU_LA]),
        move |la, now, req: &[u8]| {
            net.lock()
                .unwrap()
                .handle_physical(la, now, req)
                .map(|r| r.into_vec().into_iter().map(|x| (x.delay, x.bytes)).collect())
                .unwrap_or_default()
        },
    ))
    .unwrap()
}

#[test]
fn core_stop_cancels_in_progress_agent_uds_not_user() {
    let d = D::start();
    let entity = slow_write_ecu(&d.rt);
    let port = entity.tcp_addr().port();
    let mut v = d.viewer("web:stop", true);
    let mut user = d.viewer("web:console", false);
    let mut writer = d.agent("uds-stop-writer");
    let mut reader = d.agent("uds-stop-reader");

    let r = writer.request(
        "uds.connect",
        json!({"name": "ecu1", "host": "127.0.0.1", "port": port, "la": ECU_LA}),
    );
    eprintln!("connect: {r}");
    assert!(r["error"].is_null() && code(&r).is_null(), "{r}");

    // 1. Agent write handed to the ECU and kept pending (0x78) — in flight.
    let w_id = writer.send(
        "uds.request",
        json!({"target": "ecu1", "text": "2E F1 A0 01", "timeout_ms": 20_000}),
    );
    let ev = v.wait_events("agent write sent + 0x78", |ev| {
        ev.iter()
            .any(|e| is_sent(e) && e["payload"]["bytes"]["data"] == "2ef1a001" && e["origin"] == "agent")
            && ev.iter().any(|e| e["kind"] == "uds.pending")
    });
    let wrid = rid_of(
        ev.iter()
            .find(|e| is_sent(e) && e["payload"]["bytes"]["data"] == "2ef1a001")
            .unwrap(),
    );

    // 2. Agent read accepted by the gateway and waiting behind it in the UDS target queue.
    let r_id = reader.send("uds.read_did", json!({"target": "ecu1", "dids": ["F18C"]}));
    let ev = v.wait_events("agent read accepted", |ev| {
        ev.iter().any(|e| {
            e["kind"] == "uds.tx"
                && e["payload"]["write_state"] == "accepted"
                && e["payload"]["method"] == "uds.read_did"
        })
    });
    let rrid = rid_of(
        ev.iter()
            .find(|e| e["payload"]["method"] == "uds.read_did")
            .unwrap(),
    );
    // 3. User read waiting in the user lane.
    let u_id = user.send(
        "uds.request",
        json!({"target": "ecu1", "text": "22 F1 90", "timeout_ms": 15_000}),
    );
    v.wait_events("user read accepted", |ev| {
        ev.iter().any(|e| {
            e["kind"] == "uds.tx" && e["payload"]["write_state"] == "accepted" && e["origin"] == "user"
        })
    });
    v.pump(Duration::from_millis(150)); // both handlers reach the target queue
    let before = v.events();
    assert_eq!(
        sent_of(&before, &rrid),
        0,
        "agent read still waiting: {:?}",
        rows(&before, &rrid)
    );

    // STOP from the viewer: Daemon::stop → WriteHooks::on_latch(true) → UDS cancel.
    let t = Instant::now();
    let s = v.request("core.stop", json!({"reason": "uds stop smoke"}));
    let stop_took = t.elapsed();
    eprintln!("core.stop ({stop_took:?}): {s}");
    assert_eq!(s["result"]["on"], true, "{s}");

    let t = Instant::now();
    let w = writer.wait(w_id, Duration::from_secs(5));
    let w_took = t.elapsed();
    eprintln!("agent write after STOP ({w_took:?}): {w}");
    assert_eq!(code(&w), "E_STOPPED", "{w}");
    assert_eq!(
        detail(&w)["write_state"],
        "outcome_unknown",
        "handed off → unknown: {w}"
    );
    assert_eq!(detail(&w)["row"]["write_state"], "outcome_unknown", "{w}");
    assert!(
        w_took < Duration::from_secs(2),
        "in-flight write stopped promptly, not after its 0x78 run: {w_took:?}"
    );
    let r = reader.wait(r_id, Duration::from_secs(5));
    eprintln!("agent read after STOP: {r}");
    assert_eq!(code(&r), "E_STOPPED", "{r}");
    assert_eq!(detail(&r)["row"]["write_state"], "failed", "{r}");

    // A new agent request while latched never reaches the domain.
    let r = reader.request("uds.request", json!({"target": "ecu1", "text": "3E 00"}));
    eprintln!("agent request while latched: {r}");
    assert_eq!(code(&r), "E_STOPPED", "{r}");

    // The user's waiting read is not touched (it waits out the aborted write's quarantine).
    let u = user.wait(u_id, Duration::from_secs(15));
    eprintln!("user read during STOP: {u}");
    assert!(code(&u).is_null(), "{u}");
    assert_eq!(u["result"]["data"]["row"]["write_state"], "confirmed", "{u}");

    let ev = v.wait_events("final rows", |ev| {
        rows(ev, &wrid).iter().any(|(k, _)| k == "uds.result")
            && rows(ev, &rrid).iter().any(|(k, _)| k == "uds.result")
    });
    eprintln!("viewer rows write {wrid}: {:?}", rows(&ev, &wrid));
    eprintln!("viewer rows read {rrid}: {:?}", rows(&ev, &rrid));
    let wrows = rows(&ev, &wrid);
    assert!(
        wrows.iter().all(|(_, s)| s != "confirmed"),
        "no false confirmed: {wrows:?}"
    );
    let wfinal: Vec<&Value> = ev
        .iter()
        .filter(|e| e["request_id"] == json!(wrid) && e["payload"]["final"] == true)
        .collect();
    assert_eq!(wfinal.len(), 1, "one final row: {wfinal:?}");
    assert_eq!(wfinal[0]["kind"], "uds.result");
    assert_eq!(wfinal[0]["payload"]["write_state"], "outcome_unknown");
    assert_eq!(sent_of(&ev, &wrid), 1);
    let rfinal: Vec<&Value> = ev
        .iter()
        .filter(|e| e["request_id"] == json!(rrid) && e["payload"]["final"] == true)
        .collect();
    assert_eq!(rfinal.len(), 1, "{rfinal:?}");
    assert_eq!(rfinal[0]["payload"]["write_state"], "failed");
    assert_eq!(sent_of(&ev, &rrid), 0, "waiting agent read never sent");
    assert!(
        ev.iter()
            .any(|e| e["kind"] == "core.latch" && e["payload"]["on"] == true),
        "viewer sees the latch"
    );

    // Release: nothing cancelled is resumed or resent.
    let rel = v.request("core.stop.release", json!({}));
    eprintln!("release: {rel}");
    assert_eq!(rel["result"]["on"], false, "{rel}");
    v.pump(Duration::from_millis(1500));
    let ev = v.events();
    assert_eq!(sent_of(&ev, &wrid), 1, "cancelled write not resent");
    assert_eq!(sent_of(&ev, &rrid), 0, "cancelled read not resent");
    let agent_writes = ev
        .iter()
        .filter(|e| is_sent(e) && e["payload"]["bytes"]["data"] == "2ef1a001")
        .count();
    assert_eq!(agent_writes, 1, "2E F1A0 01 sent exactly once in total");

    // Agents work again after release.
    let r = reader.request(
        "uds.request",
        json!({"target": "ecu1", "text": "3E 00", "timeout_ms": 10_000}),
    );
    eprintln!("agent after release: {r}");
    assert!(code(&r).is_null(), "{r}");
    assert_eq!(r["result"]["data"]["row"]["write_state"], "confirmed", "{r}");

    drop((v, user, writer, reader));
    let home = d.shutdown();
    drop(entity);

    // Audit records the aborted write as outcome_unknown, never failed/confirmed.
    let mut audit = String::new();
    for f in std::fs::read_dir(home.path().join("audit")).unwrap() {
        audit.push_str(&std::fs::read_to_string(f.unwrap().path()).unwrap_or_default());
    }
    let results: Vec<Value> = audit
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|l| l.to_string().contains("write.result") && l.to_string().contains(&wrid))
        .collect();
    eprintln!("audit write.result {wrid}: {results:?}");
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(
        results[0].to_string().contains("\"outcome_unknown\""),
        "{results:?}"
    );
}
