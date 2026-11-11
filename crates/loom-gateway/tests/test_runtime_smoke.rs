//! TEST session runtime hooks through the production daemon (isolated temporary home, real sim
//! domains, real journal, WebSocket agent + viewer): CORE source fences, recording health, STOP,
//! the session-bound write gate and evidence acceptance. Real evidence can PASS; old/invented/
//! foreign-epoch REFs, missing evidence, STOP, recording failure and unflushed IPC domains fail
//! closed.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use loom_gateway::daemon::{Config, Running, start};
use loom_gateway::home::Home;
use loom_gateway::policy::WorkersMode;
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
    fn start(f: impl FnOnce(&mut Config)) -> D {
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
        f(&mut cfg);
        let run = rt.block_on(start(cfg)).unwrap();
        D {
            rt,
            run: Some(run),
            home,
        }
    }
    fn inproc() -> D {
        D::start(|c| c.workers = Some(WorkersMode::Inproc))
    }
    fn run(&self) -> &Running {
        self.run.as_ref().unwrap()
    }
    fn epoch(&self) -> u64 {
        self.run().daemon.hub.epoch
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
            c.request("core.hello", hello(None, "web:rt"))["result"]["role"],
            "viewer"
        );
        let r = c.request(
            "core.subscribe",
            json!({"filter": {}, "snapshot": false, "max_batch_ms": 5}),
        );
        assert!(r["result"]["sub_id"].is_string());
        c
    }
    fn shutdown(mut self) {
        if let Some(r) = self.run.take() {
            self.rt.block_on(r.daemon.shutdown_gracefully());
            r.server.abort();
            drop(r);
        }
        let D { rt, .. } = self;
        rt.shutdown_timeout(Duration::from_secs(2));
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
        let end = Instant::now() + Duration::from_secs(15);
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
    /// Viewer events until `pred` finds one (bounded).
    fn wait_event(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(8);
        while Instant::now() < end {
            if let Some(e) = self.events().into_iter().rev().find(|e| pred(e)) {
                return e;
            }
            self.pump(Duration::from_millis(50));
        }
        panic!("viewer never saw {what}");
    }
}

fn code(r: &Value) -> &Value {
    if !r["error"].is_null() {
        &r["error"]["data"]["code"]
    } else {
        &r["result"]["error"]["code"]
    }
}

fn data(r: &Value) -> &Value {
    &r["result"]["data"]
}

fn seq(e: &Value) -> u64 {
    e["seq"]
        .as_u64()
        .or_else(|| e["seq"].as_str().and_then(|s| s.parse().ok()))
        .expect("seq")
}

fn ok(r: &Value) {
    assert!(code(r).is_null(), "{r}");
}

const TC: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md");

/// Stimulus (PSU output on, confirmed write) then an observation step on the sim's device-origin
/// measurements, checked with the common `exists` predicate over the re-read window.
const PLAN: &str = r#"loom_tc_plan: 1
env: sim
steps:
  - id: S1
    tc_step: 1
    psu.out: {ch: pb, state: "on"}
  - id: S2
    tc_step: 2
    psu.meas: {ch: pb}
    expect:
      - filter: {domain: psu, channel: pb, kinds: [psu.meas], origins: [device]}
        pred: {op: exists}
"#;

struct Session {
    id: String,
}

fn start_session(a: &mut Client) -> Session {
    let r = a.request("test.start", json!({"tc":{"text":TC},"plan":{"text":PLAN}}));
    ok(&r);
    assert_eq!(data(&r)["state"], "RUNNING", "{r}");
    Session {
        id: data(&r)["session_id"].as_str().unwrap().to_owned(),
    }
}

/// `psu.out on` inside S1 and its final (terminal) write row REF.
fn power_on_ref(d: &D, a: &mut Client, v: &mut Client) -> String {
    let r = a.request("psu.out", json!({"ch": "pb", "state": "on"}));
    ok(&r);
    let rid = data(&r)["request_id"].as_str().unwrap().to_owned();
    let fin = v.wait_event("final psu write row", |e| {
        e["request_id"] == json!(rid) && e["payload"]["final"] == true
    });
    format!("psu:pb@e{}:seq:{}", d.epoch(), seq(&fin))
}

/// A fresh `psu.meas` sample and its REF.
fn meas_ref(d: &D, a: &mut Client, v: &mut Client) -> String {
    let before = v
        .events()
        .iter()
        .filter(|e| e["kind"] == "psu.meas")
        .map(seq)
        .max()
        .unwrap_or(0);
    ok(&a.request("psu.meas", json!({"ch": "pb"})));
    let e = v.wait_event("psu.meas", |e| e["kind"] == "psu.meas" && seq(e) > before);
    assert_eq!(e["origin"], "device", "{e}");
    format!("psu:pb@e{}:seq:{}", d.epoch(), seq(&e))
}

fn verdict(a: &mut Client, s: &Session, step: &str, v: &str, refs: &[&str]) -> Value {
    a.request(
        "test.verdict",
        json!({"session_id": s.id, "step": step, "verdict": v, "evidence": refs, "reason": "observed"}),
    )
}

fn last_check(v: &mut Client, step: &str) -> Value {
    v.pump(Duration::from_millis(200));
    v.events()
        .into_iter()
        .rev()
        .find(|e| e["kind"] == "test.evidence_checked" && e["payload"]["step_id"] == step)
        .unwrap_or_else(|| panic!("no evidence check for {step}"))
}

#[test]
fn real_evidence_passes_and_bad_refs_fail_closed() {
    let d = D::inproc();
    let mut v = d.viewer();
    let mut a = d.agent("rt-pass");
    ok(&a.request("psu.connect", json!({"ch": "pb", "backend": "sim"})));
    // A real device sample from before the session: valid data, wrong window.
    let old = meas_ref(&d, &mut a, &mut v);
    let s = start_session(&mut a);

    let s1 = a.request("test.step", json!({"session_id": s.id, "id": "S1"}));
    ok(&s1);
    assert_eq!(data(&s1)["source_fence_available"], true, "{s1}");
    let started = v.wait_event("S1 started", |e| {
        e["kind"] == "test.step_started" && e["payload"]["step_id"] == "S1"
    });
    assert_eq!(started["payload"]["fence_available"], true, "{started}");
    let on = power_on_ref(&d, &mut a, &mut v);
    let r = verdict(&mut a, &s, "S1", "done", &[&on]);
    eprintln!("S1 done: {r}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "done", "{r}");

    ok(&a.request("test.step", json!({"session_id": s.id, "id": "S2"})));
    // No evidence → refused.
    let r = verdict(&mut a, &s, "S2", "pass", &[]);
    assert_eq!(code(&r), "E_EVIDENCE_MISSING", "{r}");
    // Real sample, but committed before this attempt's start fence.
    let r = verdict(&mut a, &s, "S2", "pass", &[&old]);
    eprintln!("S2 pass with pre-session sample: {r}");
    assert_eq!(code(&r), "E_OUT_OF_WINDOW", "{r}");
    // S1's write row is a real REF of this session, but of the previous attempt window.
    let r = verdict(&mut a, &s, "S2", "pass", &[&on]);
    assert_eq!(code(&r), "E_OUT_OF_WINDOW", "{r}");
    // Invented seq and a foreign stream epoch.
    let invented = format!("psu:pb@e{}:seq:999999", d.epoch());
    let r = verdict(&mut a, &s, "S2", "pass", &[&invented]);
    assert_eq!(code(&r), "E_EVIDENCE_MISMATCH", "{r}");
    let fresh = meas_ref(&d, &mut a, &mut v);
    let foreign = fresh.replace(&format!("@e{}:", d.epoch()), &format!("@e{}:", d.epoch() - 1));
    let r = verdict(&mut a, &s, "S2", "pass", &[&foreign]);
    assert_eq!(code(&r), "E_EVIDENCE_MISMATCH", "{r}");

    // The fresh in-window device sample passes; the check re-read the window.
    let r = verdict(&mut a, &s, "S2", "pass", &[&fresh]);
    eprintln!("S2 pass: {r}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "pass", "{r}");
    let c = last_check(&mut v, "S2");
    assert_eq!(c["payload"]["status"], "verified", "{c}");
    assert_eq!(c["payload"]["witness"], "device", "{c}");
    assert_eq!(c["payload"]["complete"], true, "{c}");

    let end = a.request("test.end", json!({"session_id": s.id}));
    eprintln!("end: {end}");
    ok(&end);
    assert_eq!(data(&end)["result"], "passed", "{end}");
    assert_eq!(data(&end)["exit_code"], 0, "{end}");
    drop((a, v));
    d.shutdown();
}

#[test]
fn stop_and_session_gate_fail_closed() {
    let d = D::inproc();
    let mut v = d.viewer();
    let mut a = d.agent("rt-stop");
    ok(&a.request("psu.connect", json!({"ch": "pb", "backend": "sim"})));
    let s = start_session(&mut a);
    ok(&a.request("test.step", json!({"session_id": s.id, "id": "S1"})));

    // Paused session: every domain write of its agent is refused by CORE; the viewer keeps control.
    ok(&a.request("test.pause", json!({"session_id": s.id})));
    let r = a.request("psu.out", json!({"ch": "pb", "state": "on"}));
    eprintln!("paused agent write: {r}");
    assert_eq!(code(&r), "E_SESSION_PAUSED", "{r}");
    assert_eq!(r["result"]["error"]["detail"]["test_session_gate"], true, "{r}");
    ok(&v.request("psu.out", json!({"ch": "pb", "state": "off"})));
    ok(&a.request("test.resume", json!({"session_id": s.id})));
    let on = power_on_ref(&d, &mut a, &mut v);

    // Global STOP: the running session becomes STOPPED (live event), PASS/done and writes refused.
    let r = a.request("core.stop", json!({"reason": "test"}));
    assert_eq!(r["result"]["on"], true, "{r}");
    v.wait_event("STOPPED", |e| {
        e["kind"] == "test.session_state" && e["payload"]["state"] == "STOPPED"
    });
    let st = a.request("test.status", json!({"session_id": s.id}));
    assert_eq!(data(&st)["state"], "STOPPED", "{st}");
    let r = verdict(&mut a, &s, "S1", "done", &[&on]);
    assert!(
        ["E_LOCKED", "E_STOPPED"].contains(&code(&r).as_str().unwrap_or("")),
        "{r}"
    );
    let r = a.request("psu.out", json!({"ch": "pb", "state": "on"}));
    assert_eq!(code(&r), "E_STOPPED", "{r}");
    // Release (viewer) does not resume or run cleanup.
    let r = v.request("core.stop.release", json!({}));
    assert_eq!(r["result"]["on"], false, "{r}");
    let st = a.request("test.status", json!({"session_id": s.id}));
    assert_eq!(data(&st)["state"], "STOPPED", "{st}");
    let end = a.request("test.end", json!({"session_id": s.id}));
    eprintln!("end after STOP: {end}");
    assert!(!end.to_string().contains("\"passed\""), "{end}");
    drop((a, v));
    d.shutdown();
}

#[test]
fn recording_failure_refuses_pass() {
    let broken = Arc::new(AtomicBool::new(false));
    let b2 = broken.clone();
    let d = D::start(move |c| {
        c.workers = Some(WorkersMode::Inproc);
        c.journal_fault = Some(Arc::new(move |p| {
            if b2.load(Ordering::Acquire) && p != loom_journal::FaultPoint::Sync {
                Err(std::io::Error::other("injected disk error"))
            } else {
                Ok(())
            }
        }));
    });
    let mut v = d.viewer();
    let mut a = d.agent("rt-record");
    ok(&a.request("psu.connect", json!({"ch": "pb", "backend": "sim"})));
    let s = start_session(&mut a);
    ok(&a.request("test.step", json!({"session_id": s.id, "id": "S1"})));
    let on = power_on_ref(&d, &mut a, &mut v);
    ok(&verdict(&mut a, &s, "S1", "done", &[&on]));
    ok(&a.request("test.step", json!({"session_id": s.id, "id": "S2"})));
    broken.store(true, Ordering::Release);
    let fresh = meas_ref(&d, &mut a, &mut v); // journaled into the failing recording
    let end = Instant::now() + Duration::from_secs(5);
    while a.request("core.status", json!({}))["result"]["record_health"] != "failed" {
        assert!(Instant::now() < end, "record health never failed");
        std::thread::sleep(Duration::from_millis(50));
    }
    let r = verdict(&mut a, &s, "S2", "pass", &[&fresh]);
    eprintln!("pass during recording failure: {r}");
    assert_eq!(code(&r), "E_RECORDING_FAILED", "{r}");
    broken.store(false, Ordering::Release);
    drop((a, v));
    d.shutdown();
}

#[test]
fn ipc_worker_domains_are_unflushed_and_cannot_pass() {
    // Default placement: PSU runs as an IPC worker. Its data lane has no worker-level flush, so
    // the fence names `psu` unflushed and attribution there stays uncertain.
    let d = D::start(|_| {});
    let mut v = d.viewer();
    let mut a = d.agent("rt-ipc");
    ok(&a.request("psu.connect", json!({"ch": "pb", "backend": "sim"})));
    let s = start_session(&mut a);
    let s1 = a.request("test.step", json!({"session_id": s.id, "id": "S1"}));
    ok(&s1);
    assert_eq!(data(&s1)["source_fence_available"], true, "{s1}");
    let on = power_on_ref(&d, &mut a, &mut v);
    let r = verdict(&mut a, &s, "S1", "done", &[&on]);
    eprintln!("IPC done: {r}");
    assert!(!code(&r).is_null(), "IPC stimulus must not be confirmed: {r}");
    let c = last_check(&mut v, "S1");
    assert!(
        c["payload"]["cause"]
            .as_str()
            .is_some_and(|x| x.starts_with("fence_unflushed(psu)")),
        "{c}"
    );
    ok(&a.request("test.step", json!({"session_id": s.id, "id": "S2"})));
    let fresh = meas_ref(&d, &mut a, &mut v);
    let r = verdict(&mut a, &s, "S2", "pass", &[&fresh]);
    eprintln!("IPC pass: {r}");
    assert!(!code(&r).is_null(), "{r}");
    let c = last_check(&mut v, "S2");
    assert_eq!(c["payload"]["status"], "uncertain", "{c}");
    let st = a.request("test.status", json!({"session_id": s.id}));
    assert!(!st.to_string().contains("\"effective\":\"pass\""), "{st}");
    drop((a, v));
    d.shutdown();
}

#[test]
fn finally_runs_through_the_normal_write_pipeline() {
    let d = D::inproc();
    let mut v = d.viewer();
    let mut a = d.agent("rt-finally");
    ok(&a.request("uds.connect", json!({"name": "ecu1", "sim": "bench"})));
    let plan = r#"loom_tc_plan: 1
env: sim
steps:
  - id: S1
    tc_step: 1
    uds.session: {target: ecu1, session: extended}
finally: [{uds.session: {target: ecu1, session: default, recovery: true}}]
"#;
    let r = a.request("test.start", json!({"tc":{"text":TC},"plan":{"text":plan}}));
    eprintln!("finally plan start: {r}");
    ok(&r);
    let id = data(&r)["session_id"].as_str().unwrap().to_owned();
    ok(&a.request("test.step", json!({"session_id": id, "id": "S1"})));
    ok(&a.request("uds.session", json!({"target": "ecu1", "session": "extended"})));
    let end = a.request("test.end", json!({"session_id": id}));
    eprintln!("finally end: {end}");
    let ended = v.wait_event("session ended", |e| e["kind"] == "test.session_ended");
    eprintln!("session_ended: {ended}");
    assert_eq!(ended["payload"]["finally"]["complete"], true, "{ended}");
    let act = &ended["payload"]["finally"]["actions"][0];
    assert_eq!(act["method"], "uds.session", "{ended}");
    let rid = act["request_id"].as_str().expect("finally request id").to_owned();
    // The cleanup write went through CORE's pipeline as a script write, visible live.
    let row = v.wait_event("finally write row", |e| {
        e["request_id"] == json!(rid) && e["payload"]["write_state"] == "requested"
    });
    assert_eq!(row["origin"], "script", "{row}");
    drop((a, v));
    d.shutdown();
}

/// Absence needs a complete window. A clean window passes; when the live ring evicted part of
/// the channel inside the attempt (heavy traffic), the re-read is incomplete and PASS is refused.
#[test]
fn absence_is_not_claimed_over_an_evicted_ring() {
    const ABSENT: &str = r#"loom_tc_plan: 1
env: sim
steps:
  - id: S1
    tc_step: 1
    psu.meas: {ch: pb}
    expect:
      - filter: {domain: psu, channel: pb, kinds: [psu.fault]}
        pred: {op: absent}
"#;
    for flood in [false, true] {
        let d = D::inproc();
        let mut v = d.viewer();
        let mut a = d.agent(if flood { "rt-evict" } else { "rt-absent" });
        ok(&a.request("psu.connect", json!({"ch": "pb", "backend": "sim"})));
        let r = a.request("test.start", json!({"tc":{"text":TC},"plan":{"text":ABSENT}}));
        ok(&r);
        let s = Session {
            id: data(&r)["session_id"].as_str().unwrap().to_owned(),
        };
        ok(&a.request("test.step", json!({"session_id": s.id, "id": "S1"})));
        let witness = meas_ref(&d, &mut a, &mut v);
        if flood {
            // ~5.6 MiB on the same ring (psu/pb) > 4 MiB per-channel ring: the oldest in-window
            // events are evicted from the live ring.
            let daemon = d.run().daemon.clone();
            d.rt.block_on(async {
                for i in 0..700 {
                    let mut s = loom_gateway::hub::core_source(
                        &daemon.hub.clock,
                        "psu",
                        "psu.noise",
                        json!({"n": i, "pad": "x".repeat(8192)}),
                    );
                    s.channel = Some("pb".into());
                    drop(daemon.hub.publish(vec![s]).await);
                }
                assert!(daemon.hub.barrier(Duration::from_secs(5)).await);
            });
        }
        let r = verdict(&mut a, &s, "S1", "pass", &[&witness]);
        eprintln!(
            "absence (flood={flood}): {}",
            r.to_string().chars().take(600).collect::<String>()
        );
        let c = last_check(&mut v, "S1");
        if flood {
            assert!(!code(&r).is_null(), "PASS over an evicted ring: {r}");
            assert_eq!(c["payload"]["complete"], false, "{c}");
            assert_eq!(c["payload"]["status"], "uncertain", "{c}");
            assert!(
                c["payload"]["observed"]["problems"]
                    .to_string()
                    .contains("journal_gap"),
                "{c}"
            );
        } else {
            ok(&r);
            assert_eq!(data(&r)["verdict"]["effective"], "pass", "{r}");
            assert_eq!(c["payload"]["witness"], "complete_absence", "{c}");
        }
        drop((a, v));
        d.shutdown();
    }
}
