//! P1 J1/J2: recording wired into `loomd` (journal, mapping, query fallback, health, retention)
//! and policy files. Real daemon over WebSocket, temp `LOOM_HOME`, port 0.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use loom_gateway::daemon::{Config, Running, start};
use loom_gateway::home::Home;
use loom_journal::{FaultPoint, JournalConfig, JournalWriter, RetainPolicy, SegmentId};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

struct D {
    rt: tokio::runtime::Runtime,
    run: Option<Running>,
}

impl D {
    fn start(home: &Path, tweak: impl FnOnce(&mut Config)) -> D {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = Config::new(Home::at(home));
        cfg.port = Some(0);
        cfg.worker_program = Some(env!("CARGO_BIN_EXE_loomd").into());
        cfg.worker_orphan = Some(Duration::from_millis(500));
        tweak(&mut cfg);
        let run = rt.block_on(start(cfg)).unwrap();
        D { rt, run: Some(run) }
    }
    fn run(&self) -> &Running {
        self.run.as_ref().unwrap()
    }
    fn shutdown(mut self) {
        if let Some(r) = self.run.take() {
            r.daemon.journal.close();
            r.server.abort();
            drop(r);
        }
        self.rt.shutdown_timeout(Duration::from_secs(2));
    }
    fn agent(&self) -> Client {
        let mut c = Client::connect(self.run().addr.port(), &[]);
        let r = c.request("core.hello", hello(Some(&self.run().info.token)));
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
        let r = c.request("core.hello", hello(None));
        assert_eq!(r["result"]["role"], "viewer", "{r}");
        c
    }
}

fn hello(token: Option<&str>) -> Value {
    let mut h = json!({"proto": "loom/1.0", "client_id": "t#1", "want_role": "agent", "client": {"kind": "test", "version": "0", "tty": false}});
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
            s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
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
    /// Drain frames for `d`, returning all events seen so far.
    fn events(&mut self, d: Duration) -> Vec<Value> {
        let end = Instant::now() + d;
        while Instant::now() < end {
            if let Some(v) = self.read() {
                self.seen.push(v);
            }
        }
        self.seen
            .iter()
            .filter(|f| f["method"] == "core.events")
            .flat_map(|f| f["params"]["events"].as_array().cloned().unwrap_or_default())
            .collect()
    }
}

fn wait_until(d: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Events of a previous daemon run come back from the recording (mapping → segments), with the
/// same stream seq; a tiny scan budget gives `E_SCAN_LIMIT` + `resume_from`.
#[test]
fn restart_reads_old_run_from_journal() {
    let home = tempfile::tempdir().unwrap();
    let d = D::start(home.path(), |_| {});
    let epoch1 = d.run().daemon.hub.epoch;
    let mut a = d.agent();
    for t in ["one", "two", "three"] {
        assert_eq!(a.request("echo.send", json!({"text": t}))["result"]["ok"], true);
    }
    // rx travels on the data lane: only write rows are guaranteed before the reply (contract 05)
    let mut live = Value::Null;
    wait_until(Duration::from_secs(3), || {
        live = a.request(
            "core.events.query",
            json!({"since": "seq:1", "peek": true, "filter": {"kinds": ["echo.data"]}}),
        );
        live["result"]["count"] == 3
    });
    let live_ev = live["result"]["events"].as_array().unwrap().clone();
    assert_eq!(live_ev.len(), 3, "{live}");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(d.run().daemon.journal.root.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["stream_epoch"], epoch1.to_string());
    assert!(manifest["policy_hash"].is_string());
    drop(a);
    d.shutdown();
    std::thread::sleep(Duration::from_millis(1100)); // next run gets another epoch (unix seconds)

    let d = D::start(home.path(), |_| {});
    assert_ne!(d.run().daemon.hub.epoch, epoch1);
    let mut a = d.agent();
    let r = a.request(
        "core.events.query",
        json!({"since": format!("e{epoch1}:seq:1"), "filter": {"kinds": ["echo.data"]}}),
    );
    let ev = r["result"]["events"]
        .as_array()
        .unwrap_or_else(|| panic!("{r}"))
        .clone();
    assert_eq!(r["result"]["source"], "journal");
    let texts: Vec<&str> = ev
        .iter()
        .map(|e| e["payload"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["one", "two", "three"]);
    let seqs: Vec<&Value> = ev.iter().map(|e| &e["seq"]).collect();
    let live_seqs: Vec<&Value> = live_ev.iter().map(|e| &e["seq"]).collect();
    assert_eq!(seqs, live_seqs, "same stream seqs as when live");
    // gateway rows were recorded too (write rows of the agent)
    let rows = a.request(
        "core.events.query",
        json!({"since": format!("e{epoch1}:seq:1"), "filter": {"kinds": ["echo.write"]}}),
    );
    let states: Vec<&str> = rows["result"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["payload"]["write_state"].as_str().unwrap())
        .collect();
    assert_eq!(
        states.iter().filter(|s| **s == "requested").count(),
        3,
        "{states:?}"
    );
    drop(a);
    d.shutdown();

    let d = D::start(home.path(), |c| c.scan_bytes = 1);
    let mut a = d.agent();
    let r = a.request("core.events.query", json!({"since": format!("e{epoch1}:seq:1")}));
    assert_eq!(r["error"]["data"]["code"], "E_SCAN_LIMIT", "{r}");
    assert!(
        r["error"]["data"]["detail"]["resume_from"]
            .as_str()
            .unwrap()
            .starts_with(&format!("e{epoch1}:seq:"))
    );
    let r = a.request("core.events.query", json!({"since": "e1:seq:1"}));
    assert_eq!(r["error"]["data"]["code"], "E_CURSOR_GAP", "unknown run: {r}");
    drop(a);
    d.shutdown();
}

/// Journal disk failure → `record.health=failed`, agent writes refused (exit 4), user input
/// allowed; after the disk recovers a `record.gap` is reported and health returns to ok.
#[test]
fn journal_failure_gates_agent_and_recovers() {
    let home = tempfile::tempdir().unwrap();
    let broken = Arc::new(AtomicBool::new(false));
    let b2 = broken.clone();
    let d = D::start(home.path(), move |c| {
        c.journal_fault = Some(Arc::new(move |p| {
            if b2.load(Ordering::Acquire) && p != FaultPoint::Sync {
                Err(std::io::Error::other("injected disk error"))
            } else {
                Ok(())
            }
        }));
    });
    let mut viewer = d.viewer();
    viewer.request(
        "core.subscribe",
        json!({"filter": {}, "snapshot": false, "max_batch_ms": 5}),
    );
    let mut a = d.agent();
    assert_eq!(
        a.request("echo.send", json!({"text": "before"}))["result"]["ok"],
        true
    );
    broken.store(true, Ordering::Release);
    let _ = a.request("echo.send", json!({"text": "during"})); // lands in the failing journal
    let failed = wait_until(Duration::from_secs(5), || {
        a.request("core.status", json!({}))["result"]["record_health"] == "failed"
    });
    assert!(failed, "health turns failed");
    let r = a.request("echo.send", json!({"text": "refused"}));
    assert_eq!(r["result"]["error"]["code"], "E_RECORDING_FAILED", "{r}");
    let r = viewer.request("echo.input", json!({"text": "user\n"}));
    assert!(r["result"]["sent"].is_number(), "user input still allowed: {r}");
    broken.store(false, Ordering::Release);
    let ok = wait_until(Duration::from_secs(12), || {
        a.request("core.status", json!({}))["result"]["record_health"] == "ok"
    });
    assert!(ok, "health recovers after the disk is back");
    let ev = viewer.events(Duration::from_millis(300));
    assert!(
        ev.iter().any(|e| e["kind"] == "record.gap"),
        "loss reported as record.gap"
    );
    assert!(
        ev.iter()
            .any(|e| e["kind"] == "record.health" && e["payload"]["health"] == "failed")
    );
    assert!(
        ev.iter()
            .any(|e| e["kind"] == "record.health" && e["payload"]["health"] == "ok")
    );
    assert_eq!(
        a.request("echo.send", json!({"text": "after"}))["result"]["ok"],
        true
    );
    drop((a, viewer));
    d.shutdown();
}

fn age(path: &Path, days: u64) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

/// Retention over sessions keeps pinned segments and the active session.
#[test]
fn retention_keeps_pins() {
    let home = tempfile::tempdir().unwrap();
    // Daemon first: its start-up retention must not race the pin below.
    let d = D::start(home.path(), |_| {});
    // An old session with two sealed segments (two producers).
    let old = home.path().join("records/2020-01-01/s100");
    for pid in ["keep", "drop"] {
        let w = JournalWriter::spawn(JournalConfig::new(&old, pid, 1)).unwrap();
        let h = w.handle();
        let mut rec: loom_proto::event::SourceRecord = serde_json::from_str(include_str!(
            "../../loom-proto/tests/fixtures/event_core_latch.json"
        ))
        .unwrap();
        rec.producer_id = pid.into();
        rec.producer_epoch = loom_proto::num::U64(1);
        h.try_append(&rec).unwrap();
        let _ = w.close();
    }
    let seg = |pid: &str| {
        let dir = loom_journal::producer_dir(&old, pid, 1);
        loom_journal::segment::list_segments(&dir)
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    };
    let (keep, dropme) = (seg("keep"), seg("drop"));
    assert!(keep.sealed && dropme.sealed);
    age(&keep.path, 40);
    age(&dropme.path, 40);

    let j = d.run().daemon.journal.clone();
    j.pin(
        &old,
        SegmentId {
            producer_id: "keep".into(),
            producer_epoch: 1,
            seg_no: keep.seg_no,
        },
    );
    let r = j.retain(&RetainPolicy::default());
    assert!(r.deleted >= 1, "{r:?}");
    assert!(keep.path.exists(), "pinned segment kept");
    assert!(!dropme.path.exists(), "old unpinned segment deleted");
    // quota: a tiny quota with only pinned/active data left → Full → recording failed
    let r = j.retain(&RetainPolicy {
        max_age: Duration::from_secs(365 * 86_400),
        max_bytes: 10,
    });
    assert_eq!(r.level, loom_journal::QuotaLevel::Full);
    assert!(keep.path.exists());
    d.shutdown();
}

/// Policy files: deny / actions (approve enforced as deny), user file merged stricter, broken
/// file keeps the previous policy with a `core.policy_changed{ok:false}` banner, env from file.
#[test]
fn policy_files() {
    let home = tempfile::tempdir().unwrap();
    let proj = home.path().join("loom.toml");
    std::fs::write(&proj, "[env]\nenv = \"vehicle\"\n").unwrap();
    std::fs::create_dir_all(home.path().join("config")).unwrap();
    std::fs::write(
        home.path().join("config/policy.toml"),
        "[policy]\ndeny = [\"echo.nothing\"]\n",
    )
    .unwrap();
    let d = D::start(home.path(), |_| {});
    let mut viewer = d.viewer();
    viewer.request(
        "core.subscribe",
        json!({"filter": {}, "snapshot": false, "max_batch_ms": 5}),
    );
    let mut a = d.agent();
    let r = a.request("echo.send", json!({"text": "ok"}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    assert_eq!(r["result"]["meta"]["env"], "vehicle");
    assert!(
        r["result"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "env_vehicle")
    );

    std::fs::write(
        &proj,
        "[env]\nenv = \"vehicle\"\n[policy.actions]\n\"echo.*\" = \"approve\"\n",
    )
    .unwrap();
    let r = a.request("echo.send", json!({"text": "needs approval"}));
    assert_eq!(
        r["result"]["error"]["code"], "E_DENIED",
        "approve is enforced as deny in M1: {r}"
    );
    assert_eq!(r["result"]["error"]["detail"]["action"], "approve");

    std::fs::write(
        &proj,
        "[policy]\ndeny = [\"echo.send\"]\n[env]\nenv = \"bench\"\n",
    )
    .unwrap();
    let r = a.request("echo.send", json!({"text": "denied"}));
    assert_eq!(r["result"]["error"]["code"], "E_DENIED");
    assert_eq!(r["result"]["meta"]["env"], "bench", "env follows the file");

    std::fs::write(&proj, "[policy\nbroken = ").unwrap();
    let r = a.request("echo.send", json!({"text": "still denied"}));
    assert_eq!(
        r["result"]["error"]["code"], "E_DENIED",
        "previous policy kept: {r}"
    );
    let st = a.request("core.status", json!({}));
    let deny: Vec<&str> = st["result"]["policy"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(deny, vec!["echo.send", "echo.nothing"], "project ∪ user");
    let ev = viewer.events(Duration::from_millis(300));
    let changes: Vec<&Value> = ev.iter().filter(|e| e["kind"] == "core.policy_changed").collect();
    assert!(
        changes.iter().any(|e| e["payload"]["ok"] == false),
        "broken file banner"
    );
    assert!(changes.iter().filter(|e| e["payload"]["ok"] == true).count() >= 2);
    assert!(ev.iter().any(|e| e["kind"] == "core.env"));
    drop((a, viewer));
    d.shutdown();

    // A broken file at start refuses to run with an unknown policy.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut cfg = Config::new(Home::at(home.path()));
    cfg.port = Some(0);
    assert!(rt.block_on(start(cfg)).is_err());
}
