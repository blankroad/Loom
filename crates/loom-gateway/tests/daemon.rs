//! Real `loomd` invariants over WebSocket/HTTP (P1 tests T2·T3·T4·T5·T6·T8·T9·T10·T11 + echo E2E).
//! Each test uses its own `LOOM_HOME` temp dir and an OS-assigned port.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_gateway::daemon::{Config, Daemon, Running, start};
use loom_gateway::home::Home;
use loom_gateway::hub::core_source;
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
    fn start_in(home: tempfile::TempDir, tweak: impl FnOnce(&mut Config)) -> D {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.worker_program = Some(env!("CARGO_BIN_EXE_loomd").into());
        cfg.worker_orphan = Some(Duration::from_millis(500));
        tweak(&mut cfg);
        let run = rt.block_on(start(cfg)).unwrap();
        D {
            rt,
            run: Some(run),
            home,
        }
    }
    fn start() -> D {
        D::start_in(tempfile::tempdir().unwrap(), |_| {})
    }
    fn addr(&self) -> SocketAddr {
        self.run.as_ref().unwrap().addr
    }
    fn token(&self) -> String {
        self.run.as_ref().unwrap().info.token.clone()
    }
    fn d(&self) -> Arc<Daemon> {
        self.run.as_ref().unwrap().daemon.clone()
    }
    /// Stop serving and release the instance lock; returns the home for a restart.
    fn shutdown(mut self) -> tempfile::TempDir {
        if let Some(r) = self.run.take() {
            r.server.abort();
            drop(r);
        }
        let D { rt, home, .. } = self;
        rt.shutdown_timeout(Duration::from_secs(2));
        home
    }

    /// Raw HTTP/1.1 request → (status, headers, body).
    fn http(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &str,
    ) -> (u16, String, String) {
        let mut s = TcpStream::connect(self.addr()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            req.push_str(&format!("Host: 127.0.0.1:{}\r\n", self.addr().port()));
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(body);
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        (status, head.to_owned(), body.to_owned())
    }

    /// Bootstrap a viewer cookie (`rights` = `None` → daemon default).
    fn cookie(&self, rights: Option<&str>) -> String {
        let path = match rights {
            Some(r) => format!("/api/v1/bootstrap/new?rights={r}"),
            None => "/api/v1/bootstrap/new".into(),
        };
        let (st, _, body) = self.http(
            "GET",
            &path,
            &[("Authorization", format!("Bearer {}", self.token()))],
            "",
        );
        assert_eq!(st, 200, "{body}");
        let code =
            serde_json::from_str::<Value>(body.trim_end_matches(|c: char| !c.is_ascii_graphic() && c != '}'))
                .unwrap_or_else(|_| decode_chunked(&body))["code"]
                .as_str()
                .unwrap()
                .to_owned();
        let (st, head, _) = self.http(
            "POST",
            "/api/v1/bootstrap",
            &[("Content-Type", "application/json".into())],
            &json!({"code": code}).to_string(),
        );
        assert_eq!(st, 204, "UI Q18: 204 + Set-Cookie");
        let line = head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
            .unwrap();
        assert!(line.contains("HttpOnly") && line.contains("SameSite=Strict"));
        line.split_once(':')
            .unwrap()
            .1
            .trim()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }

    fn agent(&self, client_id: &str) -> Client {
        let mut c = Client::connect(self.addr(), &[]);
        let r = c.request("core.hello", hello(Some(&self.token()), client_id, "agent"));
        assert_eq!(r["result"]["role"], "agent", "{r}");
        c
    }

    fn viewer(&self, rights: Option<&str>) -> Client {
        let cookie = self.cookie(rights);
        let origin = format!("http://localhost:{}", self.addr().port());
        let mut c = Client::connect(self.addr(), &[("Cookie", cookie), ("Origin", origin)]);
        let r = c.request("core.hello", hello(None, "web:test", "viewer"));
        assert_eq!(r["result"]["role"], "viewer", "{r}");
        c
    }
}

fn decode_chunked(body: &str) -> Value {
    // axum may answer with chunked encoding: "<hex>\r\n<data>\r\n0\r\n\r\n".
    let data = body.split("\r\n").nth(1).unwrap_or_default();
    serde_json::from_str(data).unwrap()
}

fn hello(token: Option<&str>, client_id: &str, role: &str) -> Value {
    let mut h = json!({"proto": "loom/1.0", "client_id": client_id, "want_role": role,
        "client": {"kind": "test", "version": "0", "tty": false},
        "origin": "user"}); // forged origin field: must be ignored (T5)
    if let Some(t) = token {
        h["token"] = json!(t);
    }
    h
}

struct Client {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: i64,
    /// Frames read while waiting for something else.
    seen: Vec<Value>,
}

impl Client {
    fn try_connect(addr: SocketAddr, headers: &[(&str, String)]) -> Result<Client, tungstenite::Error> {
        let mut req = format!("ws://127.0.0.1:{}/rpc", addr.port())
            .into_client_request()
            .unwrap();
        for (k, v) in headers {
            req.headers_mut().insert(
                tungstenite::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        let (ws, _) = tungstenite::connect(req)?;
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        }
        Ok(Client {
            ws,
            next_id: 0,
            seen: Vec::new(),
        })
    }
    fn connect(addr: SocketAddr, headers: &[(&str, String)]) -> Client {
        Client::try_connect(addr, headers).unwrap()
    }
    fn call(&mut self, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        let t = json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}).to_string();
        self.ws.send(Message::Text(t.into())).unwrap();
        self.next_id
    }
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
    /// Read until `pred`; frames before the match are kept in `seen`.
    fn wait(&mut self, within: Duration, pred: impl Fn(&Value) -> bool) -> Option<Value> {
        let end = Instant::now() + within;
        while Instant::now() < end {
            match self.recv(end.saturating_duration_since(Instant::now())) {
                Some(v) if pred(&v) => return Some(v),
                Some(v) => self.seen.push(v),
                None => {}
            }
        }
        None
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.call(method, params);
        self.wait(Duration::from_secs(10), |v| v["id"] == json!(id))
            .unwrap_or_else(|| panic!("no reply to {method}"))
    }
    /// All events seen so far (control and data), flattened.
    fn events(&self) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|f| f["method"] == "core.events")
            .flat_map(|f| f["params"]["events"].as_array().cloned().unwrap_or_default())
            .collect()
    }
}

fn subscribe_all(c: &mut Client) {
    let r = c.request(
        "core.subscribe",
        json!({"filter": {}, "snapshot": true, "max_batch_ms": 5}),
    );
    assert!(r["result"]["sub_id"].is_string(), "{r}");
    c.wait(Duration::from_secs(5), |v| v["method"] == "core.snapshot_end")
        .unwrap();
}

/// echo E2E: agent `echo.send` → viewer sees the write rows (final on the last) and rx; viewer
/// `echo.input` → agent's next result reports it in `others_since_last`.
#[test]
fn echo_e2e() {
    let d = D::start();
    let mut viewer = d.viewer(None);
    subscribe_all(&mut viewer);
    let mut agent = d.agent("agent-1");
    let first = agent.request("echo.read", json!({"tail": 5}));
    assert_eq!(first["result"]["ok"], true, "{first}");
    assert_eq!(first["result"]["others_since_last"]["count"], "0");

    let r = agent.request("echo.send", json!({"text": "hi", "channel": "loop"}));
    let env = &r["result"];
    assert_eq!(env["ok"], true, "{r}");
    assert_eq!(env["data"]["state"], "sent");
    assert_eq!(env["meta"]["visibility"], "visible");
    let rid = env["data"]["request_id"].as_str().unwrap().to_owned();

    viewer.wait(Duration::from_secs(2), |_| false);
    let evs = viewer.events();
    let rows: Vec<&Value> = evs
        .iter()
        .filter(|e| e["kind"] == "echo.write" && e["request_id"] == json!(rid))
        .collect();
    let states: Vec<&str> = rows
        .iter()
        .map(|e| e["payload"]["write_state"].as_str().unwrap())
        .collect();
    assert_eq!(states, vec!["requested", "accepted", "sent"], "{rows:?}");
    assert!(rows.iter().all(|e| e["origin"] == "agent"));
    assert_eq!(rows.last().unwrap()["payload"]["final"], true);
    assert!(rows[..2].iter().all(|e| e["payload"].get("final").is_none()));
    assert!(
        evs.iter()
            .any(|e| e["kind"] == "echo.data" && e["payload"]["text"] == "hi"),
        "rx shown to viewer"
    );

    let inp = viewer.request("echo.input", json!({"channel": "loop", "text": "x\n"}));
    assert_eq!(inp["result"]["sent"], 1 + 1, "{inp}");
    let next = agent.request("echo.read", json!({"tail": 10}));
    let o = &next["result"]["others_since_last"];
    assert!(
        o["count"].as_str().unwrap().parse::<u64>().unwrap() >= 1,
        "{next}"
    );
    assert!(o["origins"].as_array().unwrap().contains(&json!("user")));
    assert!(
        next["result"]["data"]["lines"]
            .as_array()
            .unwrap()
            .contains(&json!("hi"))
    );
    // the agent cursor is shown to the viewer
    assert!(
        viewer
            .wait(Duration::from_secs(2), |v| v["params"]["events"]
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e["kind"] == "core.agent_cursor")))
            .is_some()
    );
    // daemon.json exists with 0600
    let dj = d.home.path().join("run/daemon.json");
    let info: Value = serde_json::from_slice(&std::fs::read(&dj).unwrap()).unwrap();
    assert_eq!(info["port"], d.addr().port());
    assert_eq!(info["token"].as_str().unwrap().len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&dj).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

/// T2: the final row reaches the viewer no later than the agent's reply; a stalled hub turns the
/// success into `E_PARTIAL{published:false}` (device acted — not `failed`).
#[test]
fn t2_publish_before_reply_and_partial() {
    let d = D::start_in(tempfile::tempdir().unwrap(), |c| {
        c.publish_deadline = Duration::from_millis(300)
    });
    let mut viewer = d.viewer(None);
    subscribe_all(&mut viewer);
    let addr = d.addr();
    let token = d.token();
    let watcher = std::thread::spawn(move || {
        let _ = addr;
        let got = viewer.wait(Duration::from_secs(5), |v| {
            v["params"]["events"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|e| e["kind"] == "echo.write" && e["payload"]["final"] == true)
            })
        });
        (got.is_some(), Instant::now(), viewer)
    });
    let mut agent = d.agent("agent-t2");
    let _ = token;
    std::thread::sleep(Duration::from_millis(100));
    let r = agent.request("echo.send", json!({"text": "t2"}));
    let replied = Instant::now();
    assert_eq!(r["result"]["ok"], true);
    let (ok, seen_at, _viewer) = watcher.join().unwrap();
    assert!(ok);
    assert!(
        seen_at <= replied + Duration::from_millis(50),
        "viewer saw the final row {:?} after the reply",
        seen_at - replied
    );

    d.d().hub.stall_for_test(Duration::from_millis(1500));
    let r = agent.request("echo.send", json!({"text": "stalled"}));
    let env = &r["result"];
    assert_eq!(env["ok"], false, "{r}");
    assert_eq!(env["error"]["code"], "E_PARTIAL");
    assert_eq!(env["error"]["detail"]["published"], false);
    assert_eq!(env["error"]["detail"]["write_state"], "sent");
    assert_eq!(env["data"]["state"], "sent");
}

fn flood(d: &D, n: usize, size: usize) -> u64 {
    let hub = d.d().hub.clone();
    let text = "x".repeat(size);
    let mut last = 0;
    d.rt.block_on(async {
        for chunk in 0..n / 100 {
            let recs = (0..100)
                .map(|i| {
                    let mut s = core_source(
                        &hub.clock,
                        "echo",
                        "echo.data",
                        json!({"text": text, "i": chunk * 100 + i}),
                    );
                    s.channel = Some("loop".into());
                    s.producer_id = "flood".into();
                    s
                })
                .collect();
            let p = hub.publish(recs).await.unwrap();
            last = p.last().unwrap().rec.seq.get();
        }
    });
    last
}

/// T3 + T4: a viewer that does not read loses only its own data (gap reported to it); another
/// viewer gets every event; STOP during the flood reaches the reading viewer's control queue.
#[test]
fn t3_t4_slow_subscriber_isolated_priority_kept() {
    let d = D::start();
    let mut slow = d.viewer(None);
    subscribe_all(&mut slow);
    let mut fast = d.viewer(None);
    let r = fast.request(
        "core.subscribe",
        json!({"filter": {"kinds": ["echo.data"]}, "snapshot": false, "max_batch_ms": 5}),
    );
    assert!(r["result"]["sub_id"].is_string());
    // `snapshot:false` without `after_seq` = live only, no snapshot frames (UI C-9)
    assert!(
        fast.wait(Duration::from_millis(300), |v| v["method"] == "core.snapshot_end")
            .is_none()
    );
    let mut agent = d.agent("agent-t3");

    let reader = std::thread::spawn(move || {
        let mut seqs = Vec::new();
        let mut latch_seen = false;
        let mut gaps = 0;
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end && (seqs.len() < 2000 || !latch_seen) {
            let Some(f) = fast.recv(Duration::from_millis(200)) else {
                continue;
            };
            for e in f["params"]["events"].as_array().cloned().unwrap_or_default() {
                match e["kind"].as_str().unwrap_or_default() {
                    "echo.data" => seqs.push(e["seq"].as_str().unwrap().parse::<u64>().unwrap()),
                    "core.latch" => latch_seen = true,
                    "display.gap" => gaps += 1,
                    _ => {}
                }
            }
        }
        (seqs, latch_seen, gaps)
    });
    let half = flood(&d, 1000, 4000);
    let stop = agent.request("core.stop", json!({}));
    assert_eq!(stop["result"]["on"], true, "{stop}");
    let last = flood(&d, 1000, 4000);
    let (seqs, latch_seen, gaps) = reader.join().unwrap();
    assert!(latch_seen, "STOP delivered during flood");
    assert_eq!(gaps, 0, "fast viewer has no gap");
    assert_eq!(seqs.len(), 2000, "fast viewer got everything");
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
    assert!(half < last);

    // The slow viewer now reads: it lost data but was told so.
    let mut gap = false;
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end && !gap {
        if let Some(f) = slow.recv(Duration::from_millis(200)) {
            gap |= f["params"]["display_gap"].is_object()
                || f["params"]["events"].as_array().is_some_and(|a| {
                    a.iter().any(|e| {
                        e["kind"] == "display.gap"
                            && ["slow_client", "jump_to_live"]
                                .contains(&e["payload"]["cause"].as_str().unwrap_or(""))
                    })
                });
        }
    }
    assert!(gap, "slow viewer gets a display gap");
}

/// T5: origin is decided by credentials; foreign Origin/Host refused; 127.0.0.1 pages redirect;
/// security headers present; cookie-less browser WS and credential-less hello refused.
#[test]
fn t5_origin_rules() {
    let d = D::start();
    let port = d.addr().port();
    let mut viewer = d.viewer(None);
    subscribe_all(&mut viewer);
    let mut agent = d.agent("agent-t5");
    let r = agent.request("core.mark", json!({"label": "m1", "origin": "user"}));
    assert!(r["result"]["seq"].is_string(), "{r}");
    let mark = viewer
        .wait(Duration::from_secs(2), |v| {
            v["params"]["events"]
                .as_array()
                .is_some_and(|a| a.iter().any(|e| e["kind"] == "core.mark"))
        })
        .unwrap();
    let e = mark["params"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "core.mark")
        .unwrap()
        .clone();
    assert_eq!(e["origin"], "agent", "forged origin ignored");

    let cookie = d.cookie(None);
    assert!(
        Client::try_connect(
            d.addr(),
            &[
                ("Cookie", cookie.clone()),
                ("Origin", "http://evil.example".into())
            ]
        )
        .is_err()
    );
    assert!(
        Client::try_connect(d.addr(), &[("Origin", format!("http://localhost:{port}"))]).is_err(),
        "browser without cookie"
    );
    assert!(
        Client::try_connect(
            d.addr(),
            &[
                ("Cookie", "loom_session=forged".into()),
                ("Origin", format!("http://localhost:{port}"))
            ]
        )
        .is_err()
    );
    let mut anon = Client::connect(d.addr(), &[]);
    let r = anon.request("core.hello", hello(None, "anon", "agent"));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED", "{r}");
    let mut bad = Client::connect(d.addr(), &[]);
    let r = bad.request("core.hello", hello(Some(&"0".repeat(64)), "x", "agent"));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED");

    let (st, _, _) = d.http("GET", "/", &[("Host", "evil.example".into())], "");
    assert_eq!(st, 403, "DNS rebinding host");
    let (st, head, _) = d.http("GET", "/x?y=1", &[], "");
    assert_eq!(st, 308);
    assert!(head.contains(&format!("http://localhost:{port}/x?y=1")));
    let (st, head, _) = d.http("GET", "/", &[("Host", format!("localhost:{port}"))], "");
    assert_eq!(st, 307);
    assert!(head.contains("/ui/"));
    let (st, head, body) = d.http("GET", "/ui/", &[("Host", format!("localhost:{port}"))], "");
    assert_eq!(st, 200);
    assert!(body.contains("<html"), "embedded UI or build guidance");
    let h = head.to_ascii_lowercase();
    for k in [
        "content-security-policy",
        "permissions-policy: serial=(), usb=(), hid=(), bluetooth=()",
        "referrer-policy: no-referrer",
    ] {
        assert!(h.contains(k), "missing {k}");
    }
    let (st, _, _) = d.http("POST", "/api/v1/stop", &[], "");
    assert_eq!(st, 401);
    let (st, _, body) = d.http(
        "POST",
        "/api/v1/stop",
        &[("Authorization", format!("Bearer {}", d.token()))],
        "",
    );
    assert_eq!(st, 200, "{body}");
    assert!(d.d().latched());
}

/// T6: the agent cannot release STOP; a watch viewer can STOP but not input or release.
#[test]
fn t6_rights() {
    let d = D::start();
    let mut agent = d.agent("agent-t6");
    let mut watch = d.viewer(Some("watch"));
    let mut op = d.viewer(Some("operate"));
    assert_eq!(watch.request("core.stop", json!({}))["result"]["on"], true);
    let r = agent.request("core.stop.release", json!({}));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED", "{r}");
    let r = watch.request("core.stop.release", json!({}));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED");
    let r = watch.request("echo.input", json!({"text": "a"}));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED");
    let r = agent.request("echo.input", json!({"text": "a"}));
    assert_eq!(r["error"]["data"]["code"], "E_DENIED", "input is viewer-only");
    // STOP blocks the agent's write (rows still visible) but not the user's.
    let r = agent.request("echo.send", json!({"text": "blocked"}));
    assert_eq!(r["result"]["error"]["code"], "E_STOPPED", "{r}");
    let r = op.request("echo.send", json!({"text": "user"}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    assert_eq!(op.request("core.stop.release", json!({}))["result"]["on"], false);
    assert_eq!(
        agent.request("echo.send", json!({"text": "ok"}))["result"]["ok"],
        true
    );
}

fn audit_month_file(home: &std::path::Path) -> std::path::PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    home.join("audit").join(loom_gateway::audit::file_name(secs))
}

/// T8: audit unwritable → agent R1+ refused (exit 4, `E_RECORDING_FAILED`), user input allowed.
#[test]
fn t8_audit_failure_gates_agent_only() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(audit_month_file(home.path())).unwrap(); // a directory where the file goes
    let d = D::start_in(home, |_| {});
    let mut viewer = d.viewer(None);
    subscribe_all(&mut viewer);
    let mut agent = d.agent("agent-t8");
    let r = agent.request("echo.send", json!({"text": "x"}));
    let env = &r["result"];
    assert_eq!(env["error"]["code"], "E_RECORDING_FAILED", "{r}");
    assert_eq!(env["meta"]["record_health"], "failed");
    let r = viewer.request("echo.input", json!({"text": "user\n"}));
    assert!(r["result"]["sent"].is_number(), "{r}");
    let r = viewer.request("echo.send", json!({"text": "user send"}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    viewer.wait(Duration::from_millis(300), |_| false);
    assert!(
        viewer
            .events()
            .iter()
            .any(|e| e["kind"] == "record.health" && e["payload"]["health"] == "failed")
    );
    assert_eq!(
        agent.request("echo.send", json!({"text": "again"}))["result"]["error"]["code"],
        "E_RECORDING_FAILED"
    );
}

/// T9: latch persists across restart; a corrupt latch file starts latched; second instance refused.
#[test]
fn t9_latch_persistence() {
    let d = D::start();
    let second = d.rt.block_on(start({
        let mut c = Config::new(Home::at(d.home.path()));
        c.port = Some(0);
        c
    }));
    assert_eq!(second.unwrap_err().kind(), std::io::ErrorKind::AddrInUse);
    let mut agent = d.agent("agent-t9");
    let r = agent.request("core.stop", json!({}));
    assert_eq!(r["result"]["persisted"], true, "{r}");
    drop(agent);
    let home = d.shutdown();
    let latch: Value =
        serde_json::from_slice(&std::fs::read(home.path().join("run/latch.json")).unwrap()).unwrap();
    assert_eq!(latch["on"], true);

    let d = D::start_in(home, |_| {});
    let mut a = d.agent("agent-t9b");
    assert_eq!(
        a.request("core.status", json!({}))["result"]["latch"],
        true,
        "latched after restart"
    );
    drop(a);
    let home = d.shutdown();
    std::fs::write(home.path().join("run/latch.json"), b"{not json").unwrap();
    let d = D::start_in(home, |_| {});
    assert!(d.d().latched(), "corrupt latch file → latched");
    let home = d.shutdown();
    std::fs::remove_file(home.path().join("run/latch.json")).unwrap();
    let d = D::start_in(home, |_| {});
    assert!(!d.d().latched());
}

/// T10: two concurrent requests with the same idempotency key send once.
#[test]
fn t10_idempotency() {
    let d = D::start();
    let addr = d.addr();
    let token = d.token();
    let go = |text: &'static str| {
        let token = token.clone();
        std::thread::spawn(move || {
            let mut c = Client::connect(addr, &[]);
            c.request("core.hello", hello(Some(&token), "agent-same", "agent"));
            c.request(
                "echo.send",
                json!({"text": text, "_loom": {"idempotency_key": "k1"}}),
            )
        })
    };
    let (a, b) = (go("once"), go("once"));
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!(a["result"]["ok"], true, "{a}");
    assert_eq!(
        a["result"]["data"]["request_id"],
        b["result"]["data"]["request_id"]
    );
    let mut c = d.agent("agent-other");
    let st = c.request("core.status", json!({}));
    let echo = st["result"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "echo")
        .unwrap()
        .clone();
    assert_eq!(echo["writes"], 1, "{st}");
    // a later repeat returns the stored result without sending
    let r = c.request(
        "echo.send",
        json!({"text": "once", "_loom": {"idempotency_key": "k1"}}),
    );
    assert_eq!(r["result"]["ok"], true);
    let st = c.request("core.status", json!({}));
    let echo = st["result"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "echo")
        .unwrap()
        .clone();
    assert_eq!(echo["writes"], 2, "different client = different key space");
}

/// T11: the audit chain verifies and detects tampering.
#[test]
fn t11_audit_chain() {
    let d = D::start();
    let mut agent = d.agent("agent-t11");
    for i in 0..3 {
        assert_eq!(
            agent.request("echo.send", json!({"text": format!("a{i}")}))["result"]["ok"],
            true
        );
    }
    agent.request("core.stop", json!({}));
    std::thread::sleep(Duration::from_millis(300));
    let f = audit_month_file(d.home.path());
    let (_, n) = loom_gateway::audit::verify_file(&f, "").unwrap();
    assert!(n >= 7, "intent+result per write, stop: {n}");
    let text = std::fs::read_to_string(&f).unwrap();
    let tampered = text.replacen("\"a1\"", "\"zz\"", 1);
    assert_ne!(text, tampered);
    std::fs::write(&f, tampered).unwrap();
    let e = loom_gateway::audit::verify_file(&f, "").unwrap_err();
    assert!(e.reason.contains("h mismatch"), "{e}");
}
