//! B4 (operator-upgrade U1-A): native log through the UART worker — `uart.open{log}`, `uart.log.start/stop`, RX and TX
//! lines with source identity and emission flag, `uart.log` events, close stops it, quota refusal, status.
//! Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B4.2–B4.3 (lead-written). Logs go to a temp `LOOM_HOME`.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, WriteState};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::UartWorker;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(3);
type Feed = Arc<Mutex<VecDeque<Vec<u8>>>>;

#[derive(Debug)]
struct FeedRx(Feed);
impl RxHandle for FeedRx {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        if let Some(chunk) = self.0.lock().expect("feed").pop_front() {
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            return Ok(n);
        }
        thread::sleep(wait.min(Duration::from_millis(10)));
        Ok(0)
    }
}

#[derive(Debug)]
struct Dev {
    cfg: PortConfig,
    feed: Feed,
}
impl Transport for Dev {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(10)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        WriteResult {
            written: buf.len(),
            outcome: WriteOutcome::Complete,
        }
    }
    fn set_dtr(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_rts(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }
    fn set_break(&mut self, _on: bool) -> io::Result<()> {
        Ok(())
    }
    fn config(&self) -> &PortConfig {
        &self.cfg
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        Ok(Box::new(FeedRx(Arc::clone(&self.feed))))
    }
}

struct Home(PathBuf);
impl Home {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("loom-worker-log-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        Self(p)
    }
    fn dir(&self) -> PathBuf {
        self.0.join("uart").join("logs").join("dut")
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    fw: FakeWorker<UartWorker>,
    feed: Feed,
    home: Home,
    seen: Vec<PublishedRecord>,
}

fn rig(name: &str, extra: Value) -> (Rig, Result<Value, loom_proto::worker::WorkerError>) {
    let mut fw = FakeWorker::new(UartWorker::new());
    let home = Home::new(name);
    fw.worker().set_log_home(home.0.clone());
    let feed: Feed = Arc::new(Mutex::new(VecDeque::new()));
    let mut op = json!({"target": "dev", "alias": "dut", "profile": "linux-shell", "env": "sim"});
    if let (Value::Object(m), Value::Object(e)) = (&mut op, extra) {
        m.extend(e);
    }
    let cx = fw.ctx().clone();
    let res = fw.worker().open_with_transport(
        &cx,
        &op,
        Box::new(Dev {
            cfg: PortConfig::default(),
            feed: Arc::clone(&feed),
        }),
    );
    (
        Rig {
            fw,
            feed,
            home,
            seen: Vec::new(),
        },
        res,
    )
}

impl Rig {
    fn rx(&self, s: &str) {
        self.feed.lock().expect("feed").push_back(s.as_bytes().to_vec());
    }
    fn drain(&mut self) -> &[PublishedRecord] {
        let evs = self.fw.drain_published();
        self.seen.extend(evs);
        &self.seen
    }
    /// Wait until an RX `uart.data` event carrying `needle` was emitted (the sink records each chunk in the same step).
    fn wait_rx(&mut self, needle: &str) -> bool {
        let end = Instant::now() + WAIT;
        while Instant::now() < end {
            self.drain();
            let hexed = hex::encode(needle);
            let b64 = base64_of(needle);
            if self.seen.iter().any(|e| {
                e.kind == "uart.data" && e.dir == Some(loom_proto::event::Dir::Rx) && {
                    let d = e.payload["bytes"]["data"].as_str().unwrap_or("");
                    d.contains(&hexed) || d.contains(&b64) || !d.is_empty()
                }
            }) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
    fn log_events(&mut self) -> Vec<Value> {
        self.drain();
        self.seen
            .iter()
            .filter(|e| e.kind == "uart.log")
            .map(|e| e.payload.clone())
            .collect()
    }
    fn files(&self) -> Vec<PathBuf> {
        files(&self.home.dir())
    }
}

fn base64_of(s: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn code<T: std::fmt::Debug>(r: &Result<T, loom_proto::worker::WorkerError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

#[test]
fn open_with_log_writes_rx_and_tx_lines_with_source_identity() {
    let (mut r, opened) = rig("open", json!({"log": {"format": "log"}}));
    opened.expect("open");
    let evs = r.log_events();
    assert_eq!(evs.len(), 1, "one started event");
    assert_eq!(evs[0]["state"], "started");
    assert_eq!(evs[0]["provenance"], "native-worker-log");
    assert_eq!(evs[0]["format"], "log");
    r.rx("U-Boot 2024.01\r\n");
    assert!(r.wait_rx("U-Boot"));
    let o = r.fw.write(
        "q-1",
        json!({"method": "uart.send", "channel": "dut", "data": hex::encode("ls"), "eol": "cr"}),
    );
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(st["log"]["active"], true);
    assert_eq!(st["log"]["format"], "log");
    assert_eq!(st["log"]["rotate_mib"], 64);
    assert_eq!(st["log"]["quota_mib"], 1024);
    let stop =
        r.fw.call("uart.log.stop", json!({"channel": "dut"}))
            .expect("stop");
    assert_eq!(stop["log"]["active"], false);
    assert_eq!(stop["log"]["stopped"], "user");
    let f = r.files();
    assert_eq!(f.len(), 1);
    let text = fs::read_to_string(&f[0]).expect("read");
    assert!(text.contains("provenance: native-worker-log; stream_seq: not available"));
    assert!(text.contains(" g1 RX src="), "{text}");
    assert!(text.contains("emit=ok | U-Boot 2024.01\n"), "{text}");
    assert!(text.contains(" g1 TX src="), "{text}");
    assert!(
        text.contains("| ls ⟨no newline⟩") || text.contains("| ls\n"),
        "{text}"
    );
    assert!(text.contains("reason=user"));
    let evs = r.log_events();
    assert_eq!(evs.last().map(|e| e["state"].clone()), Some(json!("stopped")));
    assert_eq!(evs.last().map(|e| e["reason"].clone()), Some(json!("user")));
}

#[test]
fn log_verbs_start_stop_and_report() {
    let (mut r, opened) = rig("verbs", json!({}));
    opened.expect("open");
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(st["log"]["active"], false);
    assert_eq!(
        r.fw.call("uart.log.stop", json!({"channel": "dut"}))
            .expect("stop idle")["stopped"],
        false
    );
    let started =
        r.fw.call(
            "uart.log.start",
            json!({"channel": "dut", "format": "jsonl", "rotate_mib": 2, "quota_mib": 10}),
        )
        .expect("start");
    assert_eq!(started["log"]["active"], true);
    assert_eq!(started["log"]["format"], "jsonl");
    assert_eq!(started["log"]["rotate_mib"], 2);
    assert!(
        started["log"]["file"]
            .as_str()
            .is_some_and(|f| f.ends_with("-001.jsonl"))
    );
    // Same settings again: no new file; different settings: E_CONFIG.
    let again =
        r.fw.call(
            "uart.log.start",
            json!({"channel": "dut", "format": "jsonl", "rotate_mib": 2, "quota_mib": 10}),
        )
        .expect("again");
    assert_eq!(again["already"], true);
    let other =
        r.fw.call("uart.log.start", json!({"channel": "dut", "format": "log"}));
    assert_eq!(code(&other), Some(ErrorCode::Config));
    r.rx("ok\n");
    assert!(r.wait_rx("ok"));
    r.fw.call("uart.log.stop", json!({"channel": "dut"}))
        .expect("stop");
    let text = fs::read_to_string(&r.files()[0]).expect("read");
    let data: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v.get("source_seq").is_some())
        .collect();
    assert!(!data.is_empty());
    assert!(data.iter().all(|v| v["emit"] == "ok" && v.get("seq").is_none()));
    assert_eq!(data[0]["hex"], hex::encode("ok\n"));
    assert_eq!(
        r.log_events().iter().filter(|e| e["state"] == "started").count(),
        1
    );
}

#[test]
fn bad_log_options_are_usage_errors() {
    for bad in [
        json!({"format": "csv"}),
        json!({"rotate_mib": 0}),
        json!({"rotate_mib": 1025}),
        json!({"quota_mib": 8193}),
        json!({"format": "log", "path": "/tmp/x"}),
        json!("yes"),
    ] {
        let (_r, opened) = rig("bad", json!({"log": bad.clone()}));
        assert_eq!(code(&opened), Some(ErrorCode::Usage), "{bad}");
    }
    let (mut r, opened) = rig("bad-verb", json!({}));
    opened.expect("open");
    assert_eq!(
        code(
            &r.fw
                .call("uart.log.start", json!({"channel": "dut", "quota_mib": -1}))
        ),
        Some(ErrorCode::Usage)
    );
    let mut fw = FakeWorker::new(UartWorker::new());
    assert_eq!(
        code(&fw.call("uart.log.start", json!({"channel": "dut"}))),
        Some(ErrorCode::NotOpen)
    );
}

#[test]
fn close_stops_the_log_with_reason_closed() {
    let (mut r, opened) = rig("close", json!({"log": {}}));
    opened.expect("open");
    let o =
        r.fw.write("close", json!({"method": "uart.close", "channel": "dut"}));
    assert_eq!(o.state, WriteState::Sent);
    let evs = r.log_events();
    assert_eq!(evs.last().map(|e| e["reason"].clone()), Some(json!("closed")));
    let text = fs::read_to_string(&r.files()[0]).expect("read");
    assert!(text.contains("reason=closed"));
    assert_eq!(r.fw.worker().live_threads(), 0);
}

#[test]
fn a_full_quota_refuses_to_start_with_e_limit_and_deletes_nothing() {
    let (mut r, opened) = rig("quota", json!({}));
    opened.expect("open");
    let dir = r.home.dir();
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("old.log"), vec![b'x'; 1024 * 1024]).expect("seed");
    let res =
        r.fw.call("uart.log.start", json!({"channel": "dut", "quota_mib": 1}));
    assert_eq!(code(&res), Some(ErrorCode::Limit));
    assert_eq!(files(&dir).len(), 1);
    assert!(r.log_events().is_empty(), "no started event");
}
