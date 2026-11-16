//! B2 (operator-upgrade U3): transmit pacing through the UART worker — `uart.open.pacing`,
//! the `uart.pacing` verb, per-request overrides, viewer input, STOP / HOLD / close / Ctrl-C
//! inside a paced gap. Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B2.2–B2.4.

use std::io;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use loom_proto::bytes::Bytes;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Input, WorkerError, Write};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::UartWorker;
use loom_worker_sdk::{Worker, WorkerCtx};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(3);
const SLACK: Duration = Duration::from_millis(5);

type Wire = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

#[derive(Debug)]
struct Silent;
impl RxHandle for Silent {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
}

/// Accepts every write whole and records `(entry time, bytes)`.
#[derive(Debug)]
struct Rec {
    cfg: PortConfig,
    wire: Wire,
}

impl Transport for Rec {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        self.wire
            .lock()
            .expect("wire")
            .push((Instant::now(), buf.to_vec()));
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
        Ok(Box::new(Silent))
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn open_op(pacing: Option<Value>) -> Value {
    let mut op = json!({"target": "rec", "alias": "dut", "profile": "linux-shell", "env": "sim"});
    if let Some(p) = pacing {
        op["pacing"] = p;
    }
    op
}

struct Rig {
    fw: FakeWorker<UartWorker>,
    wire: Wire,
}

fn try_open(
    fw: &mut FakeWorker<UartWorker>,
    wire: &Wire,
    pacing: Option<Value>,
) -> Result<Value, WorkerError> {
    let cx = fw.ctx().clone();
    fw.worker().open_with_transport(
        &cx,
        &open_op(pacing),
        Box::new(Rec {
            cfg: PortConfig::default(),
            wire: Arc::clone(wire),
        }),
    )
}

fn rig(pacing: Option<Value>) -> (Rig, Value) {
    let mut fw = FakeWorker::new(UartWorker::new());
    let wire: Wire = Arc::new(Mutex::new(Vec::new()));
    let opened = try_open(&mut fw, &wire, pacing).expect("open");
    (Rig { fw, wire }, opened)
}

impl Rig {
    fn bytes(&self) -> Vec<u8> {
        self.wire
            .lock()
            .expect("wire")
            .iter()
            .flat_map(|(_, b)| b.clone())
            .collect()
    }
    fn writes(&self) -> Vec<(Instant, Vec<u8>)> {
        self.wire.lock().expect("wire").clone()
    }
    fn wait_bytes(&self, n: usize) -> bool {
        let end = Instant::now() + WAIT;
        while Instant::now() < end {
            if self.bytes().len() >= n {
                return true;
            }
            thread::sleep(ms(2));
        }
        false
    }
    fn send(&mut self, rid: &str, text: &str, extra: Value) -> loom_worker_sdk::WriteOutcome {
        let mut op =
            json!({"method": "uart.send", "channel": "dut", "data": hex::encode(text), "eol": "none"});
        if let (Value::Object(m), Value::Object(e)) = (&mut op, extra) {
            m.extend(e);
        }
        self.fw.write(rid, op)
    }
}

fn spacing(w: &[(Instant, Vec<u8>)]) -> Vec<Duration> {
    w.windows(2).map(|p| p[1].0.duration_since(p[0].0)).collect()
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
}

fn viewer_input(h: &UartWorker, cx: &WorkerCtx, seq: u64, bytes: &[u8]) {
    rt().block_on(h.input(
        cx,
        Input {
            conn_id: Some("tab-a".into()),
            channel: Some("dut".into()),
            input_seq: Some(U64(seq)),
            call_id: None,
            lease_id: "tab-a".into(),
            bytes: Bytes(bytes.to_vec()),
            priority: false,
        },
    ))
    .expect("viewer input accepted");
}

fn later<T: Send + 'static>(
    fw: &mut FakeWorker<UartWorker>,
    delay: Duration,
    f: impl FnOnce(UartWorker, WorkerCtx) -> T + Send + 'static,
) -> JoinHandle<T> {
    let h = fw.worker().clone();
    let cx = fw.ctx().clone();
    thread::spawn(move || {
        thread::sleep(delay);
        f(h, cx)
    })
}

fn viewer_call(cx: &WorkerCtx, method: &str, args: Value) -> Call {
    Call {
        conn_id: Some("tab-a".into()),
        call_id: U64(7),
        method: method.into(),
        args,
        origin: Origin::User,
        client_id: Some("tab-a".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + 5_000_000_000),
        approval_ref: None,
    }
}

fn of_kind<'a>(evs: &'a [PublishedRecord], kind: &str) -> Vec<&'a PublishedRecord> {
    evs.iter().filter(|e| e.kind == kind).collect()
}

fn rows(evs: &[PublishedRecord], rid: &str) -> Vec<String> {
    evs.iter()
        .filter(|e| e.kind == "uart.write" && e.request_id.as_deref() == Some(rid))
        .map(|e| e.payload["write_state"].as_str().unwrap_or("?").to_owned())
        .collect()
}

fn terminal_row<'a>(evs: &'a [PublishedRecord], rid: &str) -> &'a PublishedRecord {
    evs.iter()
        .rfind(|e| e.kind == "uart.write" && e.request_id.as_deref() == Some(rid))
        .expect("terminal row")
}

fn code(r: &Result<Value, WorkerError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

// ---------------------------------------------------------------- open / status / config

#[test]
fn open_pacing_is_reported_in_result_config_event_and_status() {
    let (mut r, opened) = rig(Some(json!({"line_delay_ms": 50})));
    let want = json!({"char_delay_ms": 0, "line_delay_ms": 50});
    assert_eq!(opened["pacing"], want);
    let evs = r.fw.drain_published();
    let cfg = of_kind(&evs, "uart.config");
    assert_eq!(cfg.len(), 1);
    assert_eq!(cfg[0].payload["pacing"], want);
    assert!(cfg[0].payload["baud"].is_u64(), "existing config fields stay");
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(st["pacing"], want);
}

#[test]
fn default_open_reports_zero_pacing() {
    for p in [None, Some(Value::Null), Some(json!({}))] {
        let (mut r, opened) = rig(p);
        let zero = json!({"char_delay_ms": 0, "line_delay_ms": 0});
        assert_eq!(opened["pacing"], zero);
        let evs = r.fw.drain_published();
        assert_eq!(of_kind(&evs, "uart.config")[0].payload["pacing"], zero);
        assert_eq!(
            r.fw.call("uart.status", json!({"channel": "dut"}))
                .expect("status")["pacing"],
            zero
        );
    }
}

#[test]
fn bad_open_pacing_is_a_usage_error_and_opens_nothing() {
    let bad = [
        json!({"char_delay_ms": 1001}),
        json!({"line_delay_ms": 10_001}),
        json!({"char_delay_ms": -1}),
        json!({"char_delay_ms": "5"}),
        json!({"char_delay_ms": 1.5}),
        json!({"chr_delay_ms": 5}),
        json!("fast"),
        json!([]),
    ];
    for p in bad {
        let mut fw = FakeWorker::new(UartWorker::new());
        let wire: Wire = Arc::new(Mutex::new(Vec::new()));
        let res = try_open(&mut fw, &wire, Some(p.clone()));
        assert_eq!(code(&res), Some(ErrorCode::Usage), "pacing {p} → {res:?}");
        assert_eq!(fw.worker().live_threads(), 0, "no port opened for {p}");
    }
}

#[test]
fn boundary_values_are_accepted() {
    let (_r, opened) = rig(Some(json!({"char_delay_ms": 1000, "line_delay_ms": 10_000})));
    assert_eq!(
        opened["pacing"],
        json!({"char_delay_ms": 1000, "line_delay_ms": 10_000})
    );
}

#[test]
fn reopen_compares_pacing_only_when_given() {
    let (mut r, _) = rig(Some(json!({"line_delay_ms": 50})));
    let wire = Arc::clone(&r.wire);
    let other = try_open(&mut r.fw, &wire, Some(json!({"line_delay_ms": 60})));
    assert_eq!(
        code(&other),
        Some(ErrorCode::Config),
        "different pacing → close it first"
    );
    let same = try_open(&mut r.fw, &wire, Some(json!({"line_delay_ms": 50})));
    assert_eq!(
        same.expect("same pacing is idempotent")["pacing"]["line_delay_ms"],
        50
    );
    let absent = try_open(&mut r.fw, &wire, None);
    assert_eq!(
        absent.expect("absent pacing is idempotent")["pacing"]["line_delay_ms"],
        50,
        "reports the live pacing"
    );
}

// ---------------------------------------------------------------- uart.pacing verb

#[test]
fn pacing_verb_changes_reports_and_emits_config_once() {
    let (mut r, _) = rig(None);
    let _ = r.fw.drain_published();
    let out =
        r.fw.call("uart.pacing", json!({"channel": "dut", "char_delay_ms": 5}))
            .expect("pacing");
    assert_eq!(out["channel"], "dut");
    assert_eq!(out["generation"], 1);
    assert_eq!(out["changed"], true);
    assert_eq!(out["pacing"], json!({"char_delay_ms": 5, "line_delay_ms": 0}));
    let evs = r.fw.drain_published();
    let cfg = of_kind(&evs, "uart.config");
    assert_eq!(cfg.len(), 1, "one uart.config per change");
    assert_eq!(cfg[0].payload["pacing"], out["pacing"]);
    assert!(
        cfg[0].payload["baud"].is_u64(),
        "full config payload, not a delta"
    );
    assert_eq!(cfg[0].payload["profile"], "linux-shell");

    // A missing field keeps its current value.
    let out =
        r.fw.call("uart.pacing", json!({"channel": "dut", "line_delay_ms": 20}))
            .expect("pacing");
    assert_eq!(out["pacing"], json!({"char_delay_ms": 5, "line_delay_ms": 20}));
    let _ = r.fw.drain_published();

    // No change → changed:false and no event; no fields = query.
    for args in [
        json!({"channel": "dut", "char_delay_ms": 5}),
        json!({"channel": "dut"}),
    ] {
        let out = r.fw.call("uart.pacing", args).expect("pacing");
        assert_eq!(out["changed"], false);
        assert_eq!(out["pacing"], json!({"char_delay_ms": 5, "line_delay_ms": 20}));
    }
    assert!(of_kind(&r.fw.drain_published(), "uart.config").is_empty());
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(st["pacing"], json!({"char_delay_ms": 5, "line_delay_ms": 20}));
}

#[test]
fn pacing_verb_rejects_bad_values_without_changing_anything() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 3})));
    for args in [
        json!({"channel": "dut", "line_delay_ms": 10_001}),
        json!({"channel": "dut", "char_delay_ms": -5}),
        json!({"channel": "dut", "char_delay_ms": 2, "line_delay_ms": "x"}),
    ] {
        let res = r.fw.call("uart.pacing", args.clone());
        assert_eq!(code(&res), Some(ErrorCode::Usage), "{args}");
    }
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(
        st["pacing"],
        json!({"char_delay_ms": 3, "line_delay_ms": 0}),
        "all-or-nothing"
    );
}

#[test]
fn pacing_verb_needs_an_open_channel() {
    let mut fw = FakeWorker::new(UartWorker::new());
    let res = fw.call("uart.pacing", json!({"channel": "dut", "char_delay_ms": 5}));
    assert_eq!(code(&res), Some(ErrorCode::NotOpen));
}

// ---------------------------------------------------------------- agent writes

#[test]
fn a_line_paced_agent_send_leaves_one_line_per_gap() {
    let (mut r, _) = rig(Some(json!({"line_delay_ms": 60})));
    let o = r.send("q-lines", "l1\rl2\rl3", json!({"eol": "cr"}));
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    assert_eq!(
        o.detail["pacing"],
        json!({"char_delay_ms": 0, "line_delay_ms": 60})
    );
    let w = r.writes();
    let chunks: Vec<&[u8]> = w.iter().map(|(_, b)| b.as_slice()).collect();
    assert_eq!(chunks, vec![&b"l1\r"[..], b"l2\r", b"l3\r"]);
    for gap in spacing(&w) {
        assert!(gap + SLACK >= ms(60), "lines {gap:?} apart");
    }
    let evs = r.fw.drain_published();
    let requested = evs
        .iter()
        .find(|e| e.kind == "uart.write" && e.payload["write_state"] == "requested")
        .expect("requested row");
    assert_eq!(requested.payload["pacing"], o.detail["pacing"]);
}

#[test]
fn per_request_overrides_replace_the_port_pacing_for_that_request_only() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 50})));
    let o = r.send("q-fast", "abcdef", json!({"char_delay_ms": 0}));
    assert_eq!(o.state, WriteState::Sent);
    assert!(
        o.detail.get("pacing").is_none(),
        "unpaced result has no pacing field"
    );
    assert_eq!(r.writes().len(), 1, "override 0 → one unpaced write");
    let st =
        r.fw.call("uart.status", json!({"channel": "dut"}))
            .expect("status");
    assert_eq!(st["pacing"]["char_delay_ms"], 50, "port pacing untouched");

    let (mut r, _) = rig(None);
    let o = r.send("q-slow", "abc", json!({"char_delay_ms": 20}));
    assert_eq!(o.state, WriteState::Sent);
    assert_eq!(
        o.detail["pacing"],
        json!({"char_delay_ms": 20, "line_delay_ms": 0})
    );
    assert_eq!(r.writes().len(), 3);
}

#[test]
fn a_bad_override_fails_before_any_byte() {
    let (mut r, _) = rig(None);
    let o = r.send("q-bad", "abc", json!({"char_delay_ms": 5000}));
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Usage));
    thread::sleep(ms(100));
    assert!(r.bytes().is_empty());
}

#[test]
fn an_agent_estimate_over_the_deadline_is_refused_before_admission() {
    // char 100 ms × 79 inner gaps ≈ 7.9 s > the 5 s FakeWorker deadline.
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 100})));
    let o = r.send("q-long", &"x".repeat(80), json!({}));
    assert_eq!(o.state, WriteState::Failed);
    let e = o.error.as_ref().expect("error");
    assert_eq!(e.code, ErrorCode::Usage);
    assert_eq!(e.detail["estimate_ms"], 7900);
    assert!(e.detail["budget_ms"].as_u64().is_some_and(|b| b <= 5000));
    thread::sleep(ms(100));
    assert!(r.bytes().is_empty(), "nothing written");
    let evs = r.fw.drain_published();
    assert_eq!(
        rows(&evs, "q-long"),
        vec!["requested", "failed"],
        "never admitted"
    );
    assert!(
        !evs.iter()
            .any(|e| e.kind == "uart.agent_activity" && e.payload["kind"] == "deferred"),
        "no admission activity"
    );
}

// ---------------------------------------------------------------- STOP / HOLD / close in a gap

fn assert_cut(r: &Rig, o: &loom_worker_sdk::WriteOutcome, want: ErrorCode) {
    assert_eq!(o.state, WriteState::OutcomeUnknown, "{}", o.detail);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(want));
    let written = o.detail["written_bytes"].as_u64().expect("written_bytes");
    assert!(written > 0 && written < 30, "cut mid-request, written={written}");
    thread::sleep(ms(500));
    assert_eq!(r.bytes().len() as u64, written, "no delayed write after the cut");
}

#[test]
fn stop_inside_a_paced_gap_leaves_no_later_write() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 100})));
    let s = later(&mut r.fw, ms(250), |mut h, cx| {
        cx.latch().set(true);
        h.on_latch(true);
    });
    let o = r.send("q-stop", &"s".repeat(30), json!({}));
    s.join().expect("stop thread");
    assert_cut(&r, &o, ErrorCode::Stopped);
    let evs = r.fw.drain_published();
    assert_eq!(terminal_row(&evs, "q-stop").payload["cause"], "stopped");
    r.fw.latch(false);
}

#[test]
fn hold_inside_a_paced_gap_leaves_no_later_write() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 100})));
    let h = later(&mut r.fw, ms(250), |h, cx| {
        rt().block_on(h.call(&cx, viewer_call(&cx, "uart.hold", json!({"channel": "dut"}))))
            .expect("hold")
    });
    let o = r.send("q-hold", &"h".repeat(30), json!({}));
    h.join().expect("hold thread");
    assert_cut(&r, &o, ErrorCode::HeldByUser);
}

#[test]
fn close_inside_a_paced_gap_leaves_no_later_write() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 100})));
    let c = later(&mut r.fw, ms(250), |h, cx| {
        let w = Write {
            call_id: U64(9),
            request_id: "close-x".into(),
            write_class: WriteClass::Request,
            op: json!({"method": "uart.close", "channel": "dut"}),
            origin: Origin::Agent,
            client_id: Some("agent-b".into()),
            plan_digest: None,
            approval_ref: None,
            deadline_mono_ns: U64(cx.clock().mono_ns() + 5_000_000_000),
        };
        rt().block_on(h.write(&cx, w))
    });
    let o = r.send("q-close", &"c".repeat(30), json!({}));
    let closed = c.join().expect("close thread");
    assert_eq!(closed.state, WriteState::Sent, "close: {:?}", closed.error);
    assert_cut(&r, &o, ErrorCode::NotOpen);
}

// ---------------------------------------------------------------- viewer input

#[test]
fn paced_viewer_input_is_spaced_per_line() {
    let (mut r, _) = rig(Some(json!({"line_delay_ms": 60})));
    let h = r.fw.worker().clone();
    let cx = r.fw.ctx().clone();
    viewer_input(&h, &cx, 1, b"a\rb\rc\r");
    assert!(r.wait_bytes(6));
    let w = r.writes();
    assert_eq!(w.len(), 3);
    for gap in spacing(&w) {
        assert!(gap + SLACK >= ms(60), "lines {gap:?} apart");
    }
}

#[test]
fn viewer_input_beyond_the_pacing_cap_is_discarded_with_an_ack() {
    // 700 bytes × 1 s ≈ 699 s > USER_PACING_MAX (600 s).
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 1000})));
    let h = r.fw.worker().clone();
    let cx = r.fw.ctx().clone();
    viewer_input(&h, &cx, 41, &[b'p'; 700]);
    thread::sleep(ms(200));
    assert!(r.bytes().is_empty(), "nothing written");
    let evs = r.fw.drain_published();
    let ack = evs
        .iter()
        .find(|e| e.kind == "uart.input_ack" && e.payload["state"] == "discarded")
        .expect("discarded ack");
    assert_eq!(ack.payload["input_seq"], "41");
    assert_eq!(ack.payload["reason"], "pacing_too_long");
}

#[test]
fn ctrl_c_cuts_a_paced_paste_and_the_ack_says_partial() {
    let (mut r, _) = rig(Some(json!({"char_delay_ms": 100})));
    let h = r.fw.worker().clone();
    let cx = r.fw.ctx().clone();
    viewer_input(&h, &cx, 1, b"pppppppppp");
    assert!(r.wait_bytes(1));
    let t0 = Instant::now();
    viewer_input(&h, &cx, 2, &[3]);
    let end = Instant::now() + WAIT;
    while !r.bytes().contains(&3) && Instant::now() < end {
        thread::sleep(ms(2));
    }
    assert!(t0.elapsed() < ms(150), "Ctrl-C waited behind the paced paste");
    thread::sleep(ms(400));
    let b = r.bytes();
    assert_eq!(b.last(), Some(&3), "nothing of the paste after Ctrl-C: {b:?}");
    let written = b.iter().filter(|x| **x == b'p').count();
    assert!((1..10).contains(&written));
    let evs = r.fw.drain_published();
    let ack = evs
        .iter()
        .find(|e| {
            e.kind == "uart.input_ack" && e.payload["input_seq"] == "1" && e.payload["state"] == "partial"
        })
        .expect("partial ack for the cut paste");
    assert_eq!(ack.payload["reason"], "interrupted_by_user");
    assert_eq!(ack.payload["written_bytes"], written as u64);
    assert_eq!(ack.payload["remaining_bytes"], (10 - written) as u64);
}

#[test]
fn unpaced_viewer_input_has_no_extra_acks() {
    let (mut r, _) = rig(None);
    let h = r.fw.worker().clone();
    let cx = r.fw.ctx().clone();
    viewer_input(&h, &cx, 5, b"ls\r");
    assert!(r.wait_bytes(3));
    thread::sleep(ms(50));
    let evs = r.fw.drain_published();
    let acks: Vec<&str> = of_kind(&evs, "uart.input_ack")
        .iter()
        .filter_map(|e| e.payload["state"].as_str())
        .collect();
    assert_eq!(acks, vec!["sent"], "only the arbiter's ack");
}
