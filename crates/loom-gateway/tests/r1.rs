//! P1 R1: input plumbing, bounded wait, channels reducer, PerChannel workers (IPC). Unix only.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::net::TcpStream;
use std::process::Command;
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

fn worker(a: &mut Client) -> Value {
    let st = a.request("core.status", json!({}));
    st["result"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "echo")
        .cloned()
        .unwrap()
}

fn wait_state(a: &mut Client, want: &str, within: Duration) -> Value {
    let end = Instant::now() + within;
    loop {
        let w = worker(a);
        if w["state"] == want || Instant::now() >= end {
            return w;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn alive(pid: u64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn kill9(pid: u64) {
    assert!(
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
}

fn events_of(c: &mut Client, kind: &str, d: Duration) -> Vec<Value> {
    c.pump(d);
    c.events().into_iter().filter(|e| e["kind"] == kind).collect()
}

/// `data_hex` alias, 0x03 → priority, `input_seq`/`conn_id` reach the worker; when the typing
/// viewer disconnects the worker gets `InputLeaseEnd{cause: conn_closed}`.
#[test]
fn input_fields_priority_and_lease_end() {
    let d = D::start(Duration::from_millis(300));
    let mut watcher = d.viewer();
    let mut typist = d.viewer();
    let r = typist.request(
        "echo.input",
        json!({"data_hex": "03", "input_seq": "18446744073709551615"}),
    );
    assert_eq!(r["result"]["sent"], 1, "{r}");
    let rx = events_of(&mut watcher, "echo.data", Duration::from_millis(500));
    let ctrl_c = rx
        .iter()
        .find(|e| e["payload"]["text"] == "\u{3}")
        .unwrap_or_else(|| panic!("{rx:?}"));
    assert_eq!(ctrl_c["payload"]["priority"], true, "0x03 is sent with priority");
    assert_eq!(ctrl_c["payload"]["input_seq"], "18446744073709551615");
    drop(typist); // viewer gone
    let ends = events_of(&mut watcher, "echo.lease_end", Duration::from_secs(2));
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0]["payload"]["cause"], "conn_closed");
    assert!(ends[0]["payload"]["conn_id"].as_str().unwrap().starts_with('c'));
    drop(watcher);
    d.shutdown();
}

/// `CallCtx::wait`: returns as soon as a matching event is published; without one it times out
/// with `scanned_through` = the high-water it examined.
#[test]
fn wait_returns_on_event_or_times_out() {
    use loom_domain_api::WaitSpec;
    use loom_gateway::daemon::Caller;
    use loom_proto::cursor::Cursor;
    use loom_proto::rpc::{Role, SubFilter};
    let d = D::start(Duration::from_millis(300));
    let daemon = d.run().daemon.clone();
    let agent = Caller {
        role: Role::Agent,
        rights: None,
        client_id: "w#1".into(),
        conn_id: 0,
    };
    let (elapsed, res) = d.rt.block_on(async {
        let d2 = daemon.clone();
        let a2 = agent.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = d2.call(&a2, "echo.send", Some(json!({"text": "wake"}))).await;
        });
        let t0 = Instant::now();
        let spec = WaitSpec {
            filter: SubFilter {
                kinds: vec!["echo.data".into()],
                ..SubFilter::default()
            },
            after: Cursor::Now,
            deadline: Instant::now() + Duration::from_secs(3),
            max_events: 1,
        };
        let r = daemon.wait(&spec).await.unwrap();
        (t0.elapsed(), r)
    });
    assert!(!res.timed_out, "{res:?}");
    assert_eq!(res.events[0].payload["text"], "wake");
    eprintln!(
        "wait returned {:?} after the send started (send issued at +200 ms)",
        elapsed
    );
    assert!(elapsed < Duration::from_millis(1200), "{elapsed:?}");

    let (elapsed, res) = d.rt.block_on(async {
        let t0 = Instant::now();
        let spec = WaitSpec {
            filter: SubFilter {
                kinds: vec!["echo.never".into()],
                ..SubFilter::default()
            },
            after: Cursor::Now,
            deadline: Instant::now() + Duration::from_millis(300),
            max_events: 1,
        };
        (t0.elapsed(), daemon.wait(&spec).await.unwrap())
    });
    let _ = elapsed;
    assert!(res.timed_out && res.events.is_empty());
    assert!(res.scanned_through >= daemon.hw().saturating_sub(5), "{res:?}");
    d.shutdown();
}

/// `core.channels`: a late viewer's snapshot lists open channels; `core.channels.list` too.
/// PerChannel: two psu channels = two worker processes; killing one leaves the other serving.
#[test]
fn channels_reducer_and_per_channel_workers() {
    let d = D::start(Duration::from_millis(300));
    let mut a = d.agent("agent-pc");
    for ch in ["pa", "pb"] {
        let r = a.request("psu.connect", json!({"ch": ch, "backend": "sim"}));
        assert_eq!(r["result"]["ok"], true, "{r}");
    }
    let st = a.request("core.status", json!({}));
    let ws: Vec<Value> = st["result"]["workers"].as_array().unwrap().clone();
    let pa = ws
        .iter()
        .find(|w| w["instance"] == "psu.pa")
        .unwrap_or_else(|| panic!("{ws:?}"))
        .clone();
    let pb = ws.iter().find(|w| w["instance"] == "psu.pb").unwrap().clone();
    assert_ne!(pa["pid"], pb["pid"], "one process per channel");

    // late viewer: control snapshot carries the open channels
    let port = d.run().addr.port();
    let mut v = d.viewer();
    let r = v.request(
        "core.subscribe",
        json!({"filter": {"domains": ["psu"]}, "snapshot": true}),
    );
    assert!(r["result"]["sub_id"].is_string());
    v.pump(Duration::from_millis(300));
    let items: Vec<Value> = v
        .seen
        .iter()
        .filter(|f| f["method"] == "core.snapshot_part")
        .flat_map(|f| f["params"]["items"].as_array().cloned().unwrap_or_default())
        .collect();
    let chans = items
        .iter()
        .find(|i| i["kind"] == "core.channels")
        .unwrap_or_else(|| panic!("{items:?}"));
    let names: Vec<&str> = chans["state"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["channel"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"pa") && names.contains(&"pb"), "{names:?}");
    let list = a.request("core.channels.list", json!({}));
    assert!(
        list["result"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["domain"] == "psu" && c["channel"] == "pb"),
        "{list}"
    );
    let _ = port;

    kill9(pa["pid"].as_u64().unwrap());
    let r = a.request("psu.meas", json!({"ch": "pb"}));
    assert_eq!(r["result"]["ok"], true, "the other channel is unaffected: {r}");
    let end = Instant::now() + Duration::from_secs(8);
    let mut back = Value::Null;
    while Instant::now() < end {
        let st = a.request("core.status", json!({}));
        back = st["result"]["workers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["instance"] == "psu.pa")
            .cloned()
            .unwrap_or(Value::Null);
        if back["state"] == "ready" && back["pid"] != pa["pid"] {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(back["state"], "ready", "{back}");
    drop((a, v));
    d.shutdown();
}

/// Domains registered in `loomd` answer over RPC through their workers: eth (sim capture → packets
/// recorded and queryable), can (sim worker status), uds (registered status).
#[test]
fn domain_smoke_rpc() {
    let d = D::start(Duration::from_millis(300));
    let mut a = d.agent("agent-smoke");
    let r = a.request(
        "eth.capture.start",
        json!({"iface": "sim0", "ch": "cap", "backend": "sim", "snaplen": 65535, "buffer_mib": 16}),
    );
    assert_eq!(r["result"]["ok"], true, "{r}");
    let end = Instant::now() + Duration::from_secs(5);
    let mut n = 0;
    while Instant::now() < end && n == 0 {
        let q = a.request(
            "core.events.query",
            json!({"filter": {"kinds": ["eth.pkt"]}, "max": 10, "peek": true}),
        );
        n = q["result"]["count"].as_u64().unwrap_or(0);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(n > 0, "eth sim packets reach the hub");
    let r = a.request("can.status", json!({}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    assert_eq!(r["result"]["data"]["backend"], "sim");
    let r = a.request("uds.status", json!({}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    assert!(r["result"]["data"]["targets"].is_array(), "{r}");
    let st = a.request("core.status", json!({}));
    let names: Vec<&str> = st["result"]["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    for want in ["can", "echo", "eth", "psu", "uart", "uds", "test"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    drop(a);
    d.shutdown();
}

/// Real IPC UART worker: agent TX/RX and viewer intervention share the live subscription.
/// This exercises the WebSocket protocol, not browser rendering.
#[test]
fn uart_sim_live_input_and_stop_smoke() {
    let d = D::start(Duration::from_secs(5));
    let mut a = d.agent("uart-smoke");
    let mut v = d.viewer();
    let opened = a.request(
        "uart.open",
        json!({"target":"sim://bootshell?split=0","alias":"dut","profile":"linux-shell"}),
    );
    assert_eq!(opened["result"]["ok"], true, "{opened}");
    let boot = a.request(
        "uart.expect",
        json!({"channel":"dut","patterns":[{"kind":"literal","pattern":"login: "}],"timeout_ms":3000}),
    );
    assert!(
        boot["error"].is_null() && boot["result"]["error"].is_null(),
        "{boot}"
    );
    let sent = a.request(
        "uart.send",
        json!({"channel":"dut","data":hex::encode("root"),"eol":"cr"}),
    );
    assert_eq!(sent["result"]["ok"], true, "{sent}");
    // Keep an agent expect outstanding while a viewer types: serial runtime would stall input.
    a.id += 1;
    let expect_id = a.id;
    a.ws.send(Message::Text(json!({"jsonrpc":"2.0","id":expect_id,"method":"uart.expect","params":{"channel":"dut","patterns":[{"kind":"literal","pattern":"# "}],"timeout_ms":3000}}).to_string().into())).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    let input = v.request(
        "uart.input",
        json!({"channel":"dut","data_hex":hex::encode("secret\r"),"input_seq":"1"}),
    );
    assert_eq!(input["result"]["sent"], 7, "{input}");
    let input_elapsed = t.elapsed();
    assert!(
        input_elapsed < Duration::from_secs(2),
        "input stalled behind expect"
    );
    v.pump(Duration::from_millis(500));
    let evs = v.events();
    for (dir, origin) in [("tx", "agent"), ("tx", "user"), ("rx", "device")] {
        assert!(
            evs.iter()
                .any(|e| e["kind"] == "uart.data" && e["dir"] == dir && e["origin"] == origin),
            "missing {dir}/{origin}: {evs:?}"
        );
    }
    // Drain pending expect before issuing the next agent RPC.
    let end = Instant::now() + Duration::from_secs(4);
    let mut matched = false;
    while Instant::now() < end {
        if let Some(r) = a.read()
            && r["id"] == expect_id
        {
            assert!(r["error"].is_null() && r["result"]["error"].is_null(), "{r}");
            matched = true;
            break;
        }
    }
    assert!(matched, "expect did not complete after viewer input");
    assert_eq!(v.request("core.stop", json!({}))["result"]["on"], true);
    let blocked = a.request("uart.send", json!({"channel":"dut","data":"78","eol":"cr"}));
    assert_eq!(blocked["result"]["error"]["code"], "E_STOPPED", "{blocked}");
    eprintln!(
        "UART smoke: live agent/user/device events, input during expect, STOP blocks agent; input RPC {:?}",
        input_elapsed
    );
    let epoch = d.run().daemon.hub.epoch;
    let live_uart: Vec<_> = evs.iter().filter(|e| e["kind"] == "uart.data").cloned().collect();
    drop((a, v));
    let home = d.shutdown();
    std::thread::sleep(Duration::from_millis(1100));
    let restarted = D::start_in(home, Duration::from_secs(5));
    let mut reader = restarted.agent("uart-journal-smoke");
    let journal = reader.request(
        "core.events.query",
        json!({"since":format!("e{epoch}:seq:1"),"filter":{"kinds":["uart.data"]},"max":100,"peek":true}),
    );
    assert_eq!(journal["result"]["source"], "journal", "{journal}");
    let recorded = journal["result"]["events"].as_array().unwrap();
    for event in live_uart {
        assert!(
            recorded.contains(&event),
            "live UART event absent from journal: {event}"
        );
    }
    eprintln!("UART journal smoke: live events recovered identically after restart");
    drop(reader);
    restarted.shutdown();
}

/// STOP keeps outputs on (A3) and `core.latch.outputs_held` reports what the PSU last reported
/// (hub channel cache), instead of an empty placeholder; STOP itself switches nothing off.
#[test]
fn stop_reports_observed_held_outputs_without_switching_them_off() {
    let d = D::start(Duration::from_millis(300));
    let mut a = d.agent("agent-held");
    let r = a.request("psu.connect", json!({"ch": "bench", "backend": "sim"}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    let r = a.request("psu.out", json!({"ch": "bench", "state": "on"}));
    assert_eq!(r["result"]["ok"], true, "{r}");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let held = loop {
        let s = a.request("core.stop", json!({"reason": "held-output check"}));
        let held = s["result"]["outputs_held"].clone();
        if held.as_array().is_some_and(|h| !h.is_empty()) || std::time::Instant::now() > deadline {
            break held;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let h = held.as_array().unwrap_or_else(|| panic!("{held}"));
    let bench = h.iter().find(|o| o["domain"] == "psu" && o["channel"] == "bench").unwrap_or_else(|| panic!("{held}"));
    let text = bench["description"].as_str().unwrap();
    assert!(text.contains("out ON") && text.contains("last reported") && text.contains("not re-measured"), "{text}");
    // STOP did not switch the output off: the device still reports it on
    let m = a.request("psu.meas", json!({"ch": "bench"}));
    assert_ne!(m["result"]["data"]["out"], false, "{m}");
}
