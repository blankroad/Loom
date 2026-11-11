//! TC-1 live acceptance: the agent runs the canonical TC-1 plan
//! (`docs/impl/main/fixtures/TC-1.live.plan.yaml`) through the production daemon against one
//! powered virtual DUT, and the session is judged by TEST from the daemon journal only.
//!
//! Placement: `Config.workers = Inproc` (real PSU/UART/CAN worker backends in this process, real
//! journal; UDS is a gateway producer). IPC worker domains are reported `fence_unflushed` by
//! TEST and cannot PASS by design; their RPC paths are covered by the separate IPC smokes.
//!
//! The DUT is one shared power state, driven only by the PSU domain's own confirmed output
//! read-back (`psu.write`, `write_state: effective_verified`, `readback.out`) seen on the live
//! event stream:
//! * UART: a [`PoweredBootshell`] behind a real pty opened by `uart.open` — silent and deaf
//!   while off, a fresh boot (ROM delay → boot log → `login:`) on every power-on;
//! * CAN: when the console reached `login:` (+ application delay), a fixture-only agent
//!   identity opens a second sim channel on ch0's bus with the DUT node config (periodic
//!   `VehState`, PowerMode = 2) and closes it at power-off — real sim node, device-origin frames;
//! * UDS: a real loopback DoIP entity with a bench `SimEcu` behind the same power state —
//!   no answer while the application is down, volatile state lost at power-off, DTC memory kept.
//!
//! The fixture never publishes events or answers on the agent's behalf; it reacts to measured
//! power exactly like hardware would.
//!
//! Opt-in browser handshake (only with `LOOM_TC1_BROWSER_DIR`; without it the test is unchanged):
//! the driver hands over a fresh private directory. Before `test.start` the test writes
//! `ready.json` (0600: origin + one-time operate bootstrap URL to `/p/shell/test`) and waits ≤ 60 s
//! for the driver's `begin`; after `test.end` passed it writes `result.json` (result + session id,
//! no secrets) and waits ≤ 60 s for `release`, then finishes as usual. `LOOM_TC1_UI_DIR` (browser
//! mode only) serves that UI build instead of the embedded assets. Secrets are never printed.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::io::Write as _;
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use base64::Engine as _;
use loom_gateway::daemon::{Config, Running, start};
use loom_gateway::home::Home;
use loom_gateway::policy::WorkersMode;
use loom_uart_worker::sim::{Bootshell, Device, LOGIN_PROMPT, PtyDevice, PtyHandle, Signal, Splitter};
use loom_uds::sim_support::loom_uds_doip::{DoipEntity, EntityConfig};
use loom_uds::sim_support::loom_uds_sim::{SimEcu, SimEcuConfig};
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const TC: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md");
const PLAN: &str = include_str!("../../../docs/impl/main/fixtures/TC-1.live.plan.yaml");
const DBC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tc1_dut.dbc");
/// sha256 of `tests/fixtures/tc1_dut.dbc`, pinned in the plan's S3 expectation.
const DBC_SHA: &str = "ec6c18ea48b749dc60ac63352d7483fb044da9da4468ccc69bb9f3f56a3387bf";

/// DoIP addressing of the virtual DUT.
const ENTITY_LA: u16 = 0x1000;
const ECU_LA: u16 = 0x1001;
/// Power-on → first console byte (ROM / bootloader).
const ROM_DELAY: Duration = Duration::from_millis(1500);
/// Console `login:` → body-controller application up (CAN broadcast, UDS server).
const APP_DELAY: Duration = Duration::from_millis(300);
/// How often the unpowered device looks at the supply.
const POWER_POLL: Duration = Duration::from_millis(5);
/// DUT body controller on the CAN bus once its application runs.
const DUT_NODE: &str = r#"
[[node]]
name = "DUT_BCM"
channel = "default"
seed = 11

[[node.periodic]]
id = 0x3A0
period_ms = 100.0
offset_ms = 0.0
data = "02 00 00 00 00 00 00 00"
"#;

// ------------------------------------------------------------------------------------------
// Daemon + WebSocket clients (same shape as test_runtime_smoke.rs)
// ------------------------------------------------------------------------------------------

struct D {
    rt: tokio::runtime::Runtime,
    run: Option<Running>,
    home: tempfile::TempDir,
}

impl D {
    fn inproc() -> D {
        D::inproc_ui(None)
    }
    /// `ui_dir`: serve this UI build (browser handshake only); `None` = the default config.
    fn inproc_ui(ui_dir: Option<PathBuf>) -> D {
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
        cfg.workers = Some(WorkersMode::Inproc);
        if ui_dir.is_some() {
            cfg.ui_dir = ui_dir;
        }
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
            c.request("core.hello", hello(None, "web:tc1"))["result"]["role"],
            "viewer"
        );
        c.subscribe(json!({}));
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
            s.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        }
        Client {
            ws,
            id: 0,
            seen: Vec::new(),
        }
    }
    fn subscribe(&mut self, filter: Value) {
        let r = self.request(
            "core.subscribe",
            json!({"filter": filter, "snapshot": false, "max_batch_ms": 5}),
        );
        assert!(r["result"]["sub_id"].is_string(), "{r}");
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
        let end = Instant::now() + Duration::from_secs(40);
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
    fn take_events(&mut self) -> Vec<Value> {
        let ev = self.events();
        self.seen.clear();
        ev
    }
    fn wait_event(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + Duration::from_secs(10);
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

fn ok(r: &Value) {
    assert!(code(r).is_null() && r["error"].is_null(), "{r}");
}

fn seq(e: &Value) -> u64 {
    e["seq"]
        .as_u64()
        .or_else(|| e["seq"].as_str().and_then(|s| s.parse().ok()))
        .expect("seq")
}

// ------------------------------------------------------------------------------------------
// Virtual DUT
// ------------------------------------------------------------------------------------------

/// Shared power state of the virtual DUT.
#[derive(Debug, Default)]
struct Dut {
    /// Supply output as the PSU itself confirmed it (read-back).
    power: AtomicBool,
    /// The console printed `login:` in the current power cycle.
    booted: AtomicBool,
    /// Body-controller application up (CAN broadcast, UDS server).
    app_up: AtomicBool,
    /// Power-on boots so far.
    boots: AtomicU32,
    /// UDS requests that met an unpowered ECU (no answer).
    uds_unanswered: AtomicU32,
    /// Fixture transitions, for the log.
    log: Mutex<Vec<String>>,
}

impl Dut {
    fn note(&self, s: String) {
        eprintln!("[dut] {s}");
        self.log.lock().unwrap().push(s);
    }
}

/// `bootshell` behind the DUT supply: nothing on the line and input lost while off; every
/// power-on edge starts a fresh boot after [`ROM_DELAY`].
struct PoweredBootshell {
    dut: Arc<Dut>,
    seed: u64,
    on_at: Option<Duration>,
    shell: Option<Bootshell>,
}

impl PoweredBootshell {
    fn new(dut: Arc<Dut>, seed: u64) -> Self {
        PoweredBootshell {
            dut,
            seed,
            on_at: None,
            shell: None,
        }
    }
    fn power_lost(&mut self) {
        if self.on_at.take().is_some() {
            self.shell = None;
            self.dut.booted.store(false, Ordering::SeqCst);
            self.dut.note("uart: power lost, console dead".into());
        }
    }
    fn rel(&self, now: Duration) -> Duration {
        now.saturating_sub(self.on_at.unwrap_or(now) + ROM_DELAY)
    }
}

impl Device for PoweredBootshell {
    fn on_open(&mut self, _out: &mut Vec<u8>) {}

    fn on_bytes(&mut self, b: &[u8], now: Duration, out: &mut Vec<u8>) {
        if !self.dut.power.load(Ordering::SeqCst) {
            self.power_lost();
            return;
        }
        let rel = self.rel(now);
        if let Some(s) = self.shell.as_mut() {
            s.on_bytes(b, rel, out);
        }
    }

    fn tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration> {
        if !self.dut.power.load(Ordering::SeqCst) {
            self.power_lost();
            return Some(POWER_POLL);
        }
        let on_at = *self.on_at.get_or_insert(now);
        if self.shell.is_none() {
            if now < on_at + ROM_DELAY {
                return Some(POWER_POLL);
            }
            let n = self.dut.boots.fetch_add(1, Ordering::SeqCst) + 1;
            let mut s = Bootshell::new(self.seed.wrapping_add(u64::from(n)));
            s.on_open(out);
            self.shell = Some(s);
            self.dut.note(format!("uart: boot #{n} started"));
        }
        let rel = self.rel(now);
        let from = out.len();
        let next = self.shell.as_mut().unwrap().tick(rel, out);
        let login = LOGIN_PROMPT.trim_end().as_bytes();
        if !self.dut.booted.load(Ordering::SeqCst) && out[from..].windows(login.len()).any(|w| w == login) {
            self.dut.booted.store(true, Ordering::SeqCst);
            self.dut.note("uart: login prompt printed".into());
        }
        Some(next.map_or(POWER_POLL, |d| d.min(POWER_POLL)))
    }

    fn on_signal(&mut self, s: Signal, out: &mut Vec<u8>) {
        if self.dut.power.load(Ordering::SeqCst)
            && let Some(sh) = self.shell.as_mut()
        {
            sh.on_signal(s, out);
        }
    }
}

/// Bench `SimEcu` behind the DUT supply. DTC memory is non-volatile; session/security are not.
struct Ecu {
    dut: Arc<Dut>,
    ecu: Mutex<Option<SimEcu>>,
    dtcs: Mutex<Vec<(u32, u8)>>,
}

impl Ecu {
    fn new(dut: Arc<Dut>) -> Self {
        Ecu {
            dut,
            ecu: Mutex::new(None),
            dtcs: Mutex::new(SimEcuConfig::bench().dtcs),
        }
    }
    fn power_lost(&self) {
        *self.ecu.lock().unwrap() = None;
    }
    fn handle(&self, la: u16, now: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)> {
        if la != ECU_LA {
            return Vec::new();
        }
        let mut g = self.ecu.lock().unwrap();
        if !self.dut.app_up.load(Ordering::SeqCst) {
            *g = None;
            self.dut.uds_unanswered.fetch_add(1, Ordering::SeqCst);
            return Vec::new();
        }
        let ecu = g.get_or_insert_with(|| {
            let mut cfg = SimEcuConfig::bench();
            cfg.dtcs = self.dtcs.lock().unwrap().clone();
            SimEcu::new(cfg).unwrap()
        });
        let r = ecu.handle(now, req);
        *self.dtcs.lock().unwrap() = ecu.dtcs().to_vec();
        r.into_vec().into_iter().map(|x| (x.delay, x.bytes)).collect()
    }
}

/// The DUT's reaction to the supply: one thread on a fixture-only agent connection.
struct Fixture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Fixture {
    fn start(d: &D, dut: Arc<Dut>, ecu: Arc<Ecu>, bus: String) -> Fixture {
        let mut c = d.agent("tc1-dut-fixture");
        c.subscribe(json!({"domains": ["psu"], "kinds": ["psu.write"], "channels": ["bench"]}));
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("tc1-dut".into())
            .spawn(move || fixture_loop(c, &dut, &ecu, &bus, &s2))
            .unwrap();
        Fixture {
            stop,
            thread: Some(thread),
        }
    }
    fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

fn fixture_loop(mut c: Client, dut: &Dut, ecu: &Ecu, bus: &str, stop: &AtomicBool) {
    let mut booted_at: Option<Instant> = None;
    let mut can_up = false;
    while !stop.load(Ordering::SeqCst) {
        c.pump(Duration::from_millis(20));
        for e in c.take_events() {
            let p = &e["payload"];
            if e["kind"] != "psu.write"
                || e["channel"] != "bench"
                || p["write_state"] != "effective_verified"
                || p["action"] != "out"
            {
                continue;
            }
            let Some(on) = p["readback"]["out"].as_bool() else {
                continue;
            };
            if dut.power.swap(on, Ordering::SeqCst) != on {
                dut.note(format!(
                    "power {} (psu.write seq {} effective_verified, readback.out={on})",
                    if on { "ON" } else { "OFF" },
                    seq(&e)
                ));
                if !on {
                    dut.app_up.store(false, Ordering::SeqCst);
                    ecu.power_lost();
                }
            }
        }
        if !dut.power.load(Ordering::SeqCst) {
            booted_at = None;
            dut.app_up.store(false, Ordering::SeqCst);
            if can_up {
                let r = c.request("can.close", json!({"channel": "dutnode"}));
                dut.note(format!("can: DUT node off (can.close dutnode → {})", code(&r)));
                can_up = false;
            }
            continue;
        }
        if dut.booted.load(Ordering::SeqCst) && !dut.app_up.load(Ordering::SeqCst) {
            let t = *booted_at.get_or_insert_with(Instant::now);
            if t.elapsed() >= APP_DELAY {
                if !can_up {
                    let r = c.request(
                        "can.open",
                        json!({"channel": "dutnode", "bus": bus, "nodes": DUT_NODE}),
                    );
                    dut.note(format!("can: DUT node on bus {bus} → {}", data(&r)));
                    assert!(code(&r).is_null() && r["error"].is_null(), "{r}");
                    can_up = true;
                }
                dut.app_up.store(true, Ordering::SeqCst);
                dut.note("app: UDS server + CAN broadcast up".into());
            }
        }
    }
    if can_up {
        let _ = c.request("can.close", json!({"channel": "dutnode"}));
    }
}

// ------------------------------------------------------------------------------------------
// Opt-in browser handshake (LOOM_TC1_BROWSER_DIR)
// ------------------------------------------------------------------------------------------

const BROWSER_READY: &str = "ready.json";
const BROWSER_BEGIN: &str = "begin";
const BROWSER_RESULT: &str = "result.json";
const BROWSER_RELEASE: &str = "release";
/// Bound on each wait for the driver (`begin`, `release`).
const BROWSER_WAIT: Duration = Duration::from_secs(60);
/// Shell route of the TEST panel (`next` of the bootstrap deep link, URL-encoded).
const BROWSER_ROUTE: &str = "/p/shell/test";
const BROWSER_NEXT: &str = "%2Fp%2Fshell%2Ftest";

/// File handshake with the browser driver. The driver owns the directory: it creates it fresh and
/// private, creates `begin`/`release`, and deletes `ready.json` after use.
struct Browser {
    dir: PathBuf,
    ui_dir: Option<PathBuf>,
}

impl Browser {
    /// `None` unless `LOOM_TC1_BROWSER_DIR` is set (the default test run).
    fn from_env() -> Option<Browser> {
        let dir = PathBuf::from(std::env::var_os("LOOM_TC1_BROWSER_DIR")?);
        assert!(dir.is_absolute(), "LOOM_TC1_BROWSER_DIR must be absolute");
        let m = std::fs::symlink_metadata(&dir).expect("LOOM_TC1_BROWSER_DIR must exist");
        assert!(
            m.is_dir(),
            "LOOM_TC1_BROWSER_DIR must be a directory (not a link)"
        );
        assert_eq!(m.mode() & 0o077, 0, "LOOM_TC1_BROWSER_DIR must be private (0700)");
        assert!(
            std::fs::read_dir(&dir).unwrap().next().is_none(),
            "LOOM_TC1_BROWSER_DIR must be a freshly created, empty directory"
        );
        let ui_dir = std::env::var_os("LOOM_TC1_UI_DIR").map(PathBuf::from);
        if let Some(u) = &ui_dir {
            assert!(
                u.join("index.html").is_file(),
                "LOOM_TC1_UI_DIR has no index.html"
            );
        }
        Some(Browser { dir, ui_dir })
    }

    /// Owner-only file, written aside and renamed so the driver never reads a partial file.
    fn write_private(&self, name: &str, v: &Value) {
        let tmp = self.dir.join(format!(".{name}.tmp"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .unwrap();
        f.write_all(v.to_string().as_bytes()).unwrap();
        f.sync_all().unwrap();
        drop(f);
        std::fs::rename(&tmp, self.dir.join(name)).unwrap();
    }

    /// `ready.json`: canonical origin + a one-time operate bootstrap URL opening the TEST panel
    /// (same form as `GET /api/v1/bootstrap/new?rights=operate&next=…`). Never printed.
    fn ready(&self, d: &D) {
        let origin = loom_gateway::server::canonical_origin(d.run().addr.port());
        let code = d.run().daemon.auth.issue_code(loom_proto::rpc::Rights::Operate);
        let url = format!("{origin}/ui/#boot={code}&next={BROWSER_NEXT}");
        self.write_private(
            BROWSER_READY,
            &json!({"origin": origin, "url": url, "route": BROWSER_ROUTE}),
        );
        eprintln!("[browser] {BROWSER_READY} written; waiting ≤ {BROWSER_WAIT:?} for {BROWSER_BEGIN}");
    }

    /// `result.json` after a passed `test.end`: result + session id only.
    fn result(&self, end: &Value, sid: &str) {
        self.write_private(
            BROWSER_RESULT,
            &json!({"result": data(end)["result"], "session_id": sid}),
        );
        eprintln!("[browser] {BROWSER_RESULT} written; waiting ≤ {BROWSER_WAIT:?} for {BROWSER_RELEASE}");
    }

    /// Wait (bounded) for the driver's marker file. The open WebSocket clients keep being read so
    /// their pongs reach the daemon (it closes a connection after 40 s of silence).
    fn wait(&self, name: &str, clients: &mut [&mut Client]) {
        let p = self.dir.join(name);
        let end = Instant::now() + BROWSER_WAIT;
        while std::fs::symlink_metadata(&p).is_err() {
            assert!(
                Instant::now() < end,
                "browser handshake: no `{name}` within {BROWSER_WAIT:?}"
            );
            for c in clients.iter_mut() {
                c.pump(Duration::from_millis(25));
            }
        }
        eprintln!("[browser] {name} seen");
    }
}

// ------------------------------------------------------------------------------------------
// Agent helpers
// ------------------------------------------------------------------------------------------

fn r#ref(d: &D, domain_ch: &str, e: &Value) -> String {
    format!("{domain_ch}@e{}:seq:{}", d.epoch(), seq(e))
}

/// `psu.out` and the REF of its final write row.
fn psu_out(d: &D, a: &mut Client, v: &mut Client, state: &str) -> String {
    let r = a.request("psu.out", json!({"ch": "bench", "state": state}));
    ok(&r);
    let rid = data(&r)["request_id"].as_str().unwrap().to_owned();
    let fin = v.wait_event("final psu write row", |e| {
        e["request_id"] == json!(rid) && e["payload"]["final"] == true
    });
    r#ref(d, "psu:bench", &fin)
}

fn wait_flag(f: &AtomicBool, want: bool, what: &str) {
    let end = Instant::now() + Duration::from_secs(10);
    while f.load(Ordering::SeqCst) != want {
        assert!(Instant::now() < end, "DUT never reached {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Device UART bytes after `after_seq`, as (seq, text) per event.
fn uart_rx(v: &Client, after_seq: u64) -> Vec<(Value, String)> {
    v.events()
        .into_iter()
        .filter(|e| {
            e["kind"] == "uart.data" && e["channel"] == "dut" && e["origin"] == "device" && seq(e) > after_seq
        })
        .map(|e| {
            let b = &e["payload"]["bytes"];
            let raw = match b["encoding"].as_str() {
                Some("base64") => base64::engine::general_purpose::STANDARD
                    .decode(b["data"].as_str().unwrap_or(""))
                    .unwrap_or_default(),
                Some("hex") => hex::decode(b["data"].as_str().unwrap_or("")).unwrap_or_default(),
                _ => Vec::new(),
            };
            (e, String::from_utf8_lossy(&raw).into_owned())
        })
        .collect()
}

/// The device UART event completing the first `needle` after `after_seq`.
fn uart_match(v: &Client, after_seq: u64, needle: &str) -> Option<Value> {
    let mut text = String::new();
    for (e, t) in uart_rx(v, after_seq) {
        text.push_str(&t);
        if text.contains(needle) {
            return Some(e);
        }
    }
    None
}

fn max_seq(v: &Client) -> u64 {
    v.events().iter().map(seq).max().unwrap_or(0)
}

fn verdict(a: &mut Client, sid: &str, step: &str, verdict: &str, refs: &[&str]) -> Value {
    let r = a.request(
        "test.verdict",
        json!({"session_id": sid, "step": step, "verdict": verdict, "evidence": refs, "reason": "observed"}),
    );
    eprintln!("{step} {verdict}: {r}");
    r
}

fn last_check(v: &mut Client, step: &str) -> Value {
    v.pump(Duration::from_millis(200));
    v.events()
        .into_iter()
        .rev()
        .find(|e| e["kind"] == "test.evidence_checked" && e["payload"]["step_id"] == step)
        .unwrap_or_else(|| panic!("no evidence check for {step}"))
}

fn step(a: &mut Client, sid: &str, id: &str) {
    let r = a.request("test.step", json!({"session_id": sid, "id": id}));
    eprintln!("step {id}: {r}");
    ok(&r);
    assert_eq!(data(&r)["source_fence_available"], true, "{r}");
}

/// The uds.rx (device) answer to `request_id`.
fn uds_rx(v: &mut Client, rid: &str) -> Value {
    v.wait_event("uds.rx", |e| {
        e["kind"] == "uds.rx" && e["request_id"] == json!(rid) && e["origin"] == "device"
    })
}

// ------------------------------------------------------------------------------------------
// The test
// ------------------------------------------------------------------------------------------

#[test]
fn tc1_live_powered_dut_judged_from_journal() {
    let browser = Browser::from_env();
    let d = D::inproc_ui(browser.as_ref().and_then(|b| b.ui_dir.clone()));
    let mut v = d.viewer();
    let mut a = d.agent("tc1-agent");
    let dut = Arc::new(Dut::default());

    // Bench wiring (setup, before the session): PSU sim, DUT console on a real pty, CAN ch0 with
    // the DUT DBC, UDS over a real loopback DoIP entity.
    ok(&a.request("psu.connect", json!({"ch": "bench", "backend": "sim"})));
    let pty: PtyHandle = PtyDevice::spawn(
        Box::new(PoweredBootshell::new(dut.clone(), 1)),
        Splitter::interactive(1),
    )
    .unwrap();
    let r = a.request(
        "uart.open",
        json!({"target": pty.slave_path_str(), "alias": "dut", "env": "sim"}),
    );
    eprintln!("uart.open pty: {r}");
    ok(&r);
    let r = a.request("can.open", json!({"channel": "ch0", "nodes": "none"}));
    ok(&r);
    let bus = data(&r)["bus"].as_str().unwrap().to_owned();
    let r = a.request("can.db.load", json!({"channel": "ch0", "path": DBC}));
    ok(&r);
    assert_eq!(data(&r)["db_sha256"], DBC_SHA, "plan pins this DBC: {r}");
    let ecu = Arc::new(Ecu::new(dut.clone()));
    let e2 = ecu.clone();
    let entity =
        d.rt.block_on(DoipEntity::start(
            EntityConfig::loopback(ENTITY_LA, vec![ECU_LA]),
            move |la, now, req: &[u8]| e2.handle(la, now, req),
        ))
        .unwrap();
    let r = a.request(
        "uds.connect",
        json!({"name": "bcm", "host": "127.0.0.1", "port": entity.tcp_addr().port(), "la": ECU_LA}),
    );
    ok(&r);
    let fx = Fixture::start(&d, dut.clone(), ecu.clone(), bus);

    // Precondition "DUT power OFF": no console, no CAN broadcast, no UDS answer.
    // TesterPresent, not 0x19: a timed-out SID stays quarantined for P2* (late answers), and
    // S4 needs 0x19.
    let r = a.request(
        "uds.request",
        json!({"target": "bcm", "bytes_hex": "3E00", "timeout_ms": 500}),
    );
    eprintln!("unpowered TesterPresent: {r}");
    assert_eq!(code(&r), "E_NO_RESPONSE", "an unpowered ECU must not answer: {r}");
    assert!(dut.uds_unanswered.load(Ordering::SeqCst) >= 1);
    let r = a.request(
        "can.sig.wait_for",
        json!({"channel": "ch0", "expr": "VehState.PowerMode == 2", "within_ms": 400}),
    );
    eprintln!("unpowered wait_for: {r}");
    assert!(data(&r)["matched"] != true, "no broadcast while off: {r}");
    v.pump(Duration::from_millis(200));
    assert!(uart_rx(&v, 0).is_empty(), "console silent while off");
    assert_eq!(dut.boots.load(Ordering::SeqCst), 0);

    // Browser handshake (opt-in): the driver's viewer opens the TEST panel and subscribes first.
    if let Some(b) = &browser {
        b.ready(&d);
        b.wait(BROWSER_BEGIN, &mut [&mut a, &mut v]);
    }

    // Session.
    let r = a.request("test.start", json!({"tc": {"text": TC}, "plan": {"text": PLAN}}));
    eprintln!("start: {r}");
    ok(&r);
    assert_eq!(data(&r)["state"], "RUNNING", "{r}");
    let sid = data(&r)["session_id"].as_str().unwrap().to_owned();

    // S1: power OFF, 2 s, ON (confirmed writes).
    step(&mut a, &sid, "S1");
    let off = psu_out(&d, &mut a, &mut v, "off");
    std::thread::sleep(Duration::from_secs(2));
    let on = psu_out(&d, &mut a, &mut v, "on");
    wait_flag(&dut.power, true, "power ON from the PSU read-back");
    let r = verdict(&mut a, &sid, "S1", "done", &[&off, &on]);
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "done", "{r}");

    // S2: console `login:` within 30 s of power ON.
    step(&mut a, &sid, "S2");
    let s2_from = max_seq(&v);
    let r = a.request(
        "uart.expect",
        json!({"channel": "dut", "patterns": [{"kind": "literal", "pattern": "login:"}], "timeout_ms": 30000}),
    );
    eprintln!("uart.expect: {r}");
    ok(&r);
    assert_eq!(dut.boots.load(Ordering::SeqCst), 1, "booted by this power-on");
    v.pump(Duration::from_millis(200));
    let login = uart_match(&v, s2_from, "login:").expect("device login: bytes on the stream");
    let login_ref = r#ref(&d, "uart:dut", &login);
    let r = verdict(&mut a, &sid, "S2", "pass", &[&login_ref]);
    let c = last_check(&mut v, "S2");
    eprintln!("S2 check: {c}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "pass", "{r}");
    assert_eq!(c["payload"]["status"], "verified", "{c}");
    assert_eq!(c["payload"]["witness"], "device", "{c}");

    // S3: PowerMode = 2 within 5 s of boot.
    step(&mut a, &sid, "S3");
    let r = a.request(
        "can.sig.wait_for",
        json!({"channel": "ch0", "expr": "VehState.PowerMode == 2", "within_ms": 5000}),
    );
    eprintln!("can.sig.wait_for: {r}");
    ok(&r);
    assert_eq!(data(&r)["matched"], true, "{r}");
    assert_eq!(data(&r)["origin"], "device", "{r}");
    let can_ref = data(&r)["frame_ref"].as_str().unwrap().to_owned();
    let frame_seq = seq(&data(&r).clone());
    let frame = v.wait_event("PowerMode frame", |e| {
        e["kind"] == "can.frame" && seq(e) == frame_seq
    });
    assert_eq!(frame["origin"], "device", "{frame}");
    assert_eq!(can_ref, r#ref(&d, "can:ch0", &frame));
    let r = verdict(&mut a, &sid, "S3", "pass", &[&can_ref]);
    let c = last_check(&mut v, "S3");
    eprintln!("S3 check: {c}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "pass", "{r}");
    assert_eq!(c["payload"]["witness"], "device", "{c}");

    // S4: DTC (mask 09) read — the bench ECU has stored DTCs, so the step fails.
    step(&mut a, &sid, "S4");
    let r = a.request("uds.dtc.list", json!({"target": "bcm", "mask": 9}));
    eprintln!("dtc.list: {r}");
    ok(&r);
    let rid = data(&r)["request_id"].as_str().unwrap_or_default().to_owned();
    let rx = uds_rx(&mut v, &rid);
    let rx_ref = r#ref(&d, "uds:bcm", &rx);
    let r = verdict(&mut a, &sid, "S4", "fail", &[&rx_ref]);
    let c = last_check(&mut v, "S4");
    eprintln!("S4 check: {c}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "fail", "{r}");
    assert_eq!(c["payload"]["status"], "mismatch", "{c}");

    // S5: clear all DTCs (14 FF FF FF), re-read → none.
    step(&mut a, &sid, "S5");
    let r = a.request("uds.request", json!({"target": "bcm", "bytes_hex": "14FFFFFF"}));
    eprintln!("clear: {r}");
    ok(&r);
    let r = a.request("uds.dtc.list", json!({"target": "bcm", "mask": 9}));
    eprintln!("dtc.list after clear: {r}");
    ok(&r);
    let rid = data(&r)["request_id"].as_str().unwrap_or_default().to_owned();
    let rx = uds_rx(&mut v, &rid);
    let rx_ref = r#ref(&d, "uds:bcm", &rx);
    let r = verdict(&mut a, &sid, "S5", "pass", &[&rx_ref]);
    let c = last_check(&mut v, "S5");
    eprintln!("S5 check: {c}");
    ok(&r);
    assert_eq!(data(&r)["verdict"]["effective"], "pass", "{r}");
    assert_eq!(c["payload"]["witness"], "device", "{c}");

    let end = a.request("test.end", json!({"session_id": sid}));
    eprintln!("end: {end}");
    ok(&end);
    assert_eq!(data(&end)["result"], "passed", "{end}");
    assert_eq!(data(&end)["exit_code"], 0, "{end}");
    let ended = v.wait_event("session ended", |e| e["kind"] == "test.session_ended");
    eprintln!("session_ended: {ended}");
    // Browser handshake (opt-in): time for the driver's screenshot of the ended session.
    if let Some(b) = &browser {
        b.result(&end, &sid);
        b.wait(BROWSER_RELEASE, &mut [&mut a, &mut v]);
    }

    // After the session: the same DUT follows the supply — off kills console/CAN/UDS, the next
    // power-on reboots, and the DTC memory stays cleared.
    let _ = psu_out(&d, &mut a, &mut v, "off");
    wait_flag(&dut.power, false, "power OFF");
    wait_flag(&dut.booted, false, "console dead");
    std::thread::sleep(Duration::from_millis(300));
    // Another SID than the pre-session probe (0x3E may still be quarantined) and than 0x19.
    let r = a.request(
        "uds.request",
        json!({"target": "bcm", "bytes_hex": "22F190", "timeout_ms": 500}),
    );
    assert_eq!(code(&r), "E_NO_RESPONSE", "ECU answered while off: {r}");
    let r = a.request(
        "can.sig.wait_for",
        json!({"channel": "ch0", "expr": "VehState.PowerMode == 2", "within_ms": 400}),
    );
    assert!(data(&r)["matched"] != true, "broadcast while off: {r}");
    let off_seq = max_seq(&v);
    let _ = psu_out(&d, &mut a, &mut v, "on");
    let r = a.request(
        "uart.expect",
        json!({"channel": "dut", "patterns": [{"kind": "literal", "pattern": "login:"}], "timeout_ms": 30000}),
    );
    ok(&r);
    assert_eq!(dut.boots.load(Ordering::SeqCst), 2, "second boot");
    v.pump(Duration::from_millis(200));
    assert!(
        uart_match(&v, off_seq, "U-Boot").is_some(),
        "reboot log after power-on"
    );
    wait_flag(&dut.app_up, true, "application up");
    // The unanswered off-probe holds the target in its quiet period (≥ P2* = 5 s): nothing is
    // sent until it ends. Retry only that refusal, bounded.
    let end = Instant::now() + Duration::from_secs(8);
    let r = loop {
        let r = a.request("uds.dtc.list", json!({"target": "bcm", "mask": 9}));
        if code(&r) != "E_QUARANTINE" || Instant::now() > end {
            break r;
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    eprintln!("dtc.list after reboot: {r}");
    ok(&r);
    assert_eq!(
        data(&r)["row"]["dtcs"],
        json!([]),
        "DTC memory kept across power: {r}"
    );
    let r = a.request(
        "can.sig.wait_for",
        json!({"channel": "ch0", "expr": "VehState.PowerMode == 2", "within_ms": 2000}),
    );
    assert_eq!(data(&r)["matched"], true, "{r}");

    eprintln!("dut log: {:#?}", dut.log.lock().unwrap());
    fx.stop();
    drop((a, v, entity, pty));
    d.shutdown();
}
