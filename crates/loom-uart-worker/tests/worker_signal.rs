//! B3 (operator-upgrade U6): `uart.signal` — break pulse, DTR / RTS set and pulse, STOP / HOLD rules, the
//! release always happens. Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B3.1 (lead-written).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Write};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::tx::ControlOp;
use loom_uart_worker::worker::{BREAK_DEFAULT_MS, SIGNAL_MAX_MS, UartWorker};
use loom_worker_sdk::{Worker, WorkerCtx};
use serde_json::{Value, json};

type Controls = Arc<Mutex<Vec<(Instant, ControlOp)>>>;

#[derive(Debug)]
struct Silent;
impl RxHandle for Silent {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
}

/// Records control ops; `fail_break_on` makes asserting break fail (release still recorded).
#[derive(Debug)]
struct Lines {
    cfg: PortConfig,
    controls: Controls,
    fail_break_on: Arc<AtomicBool>,
}

impl Transport for Lines {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        WriteResult {
            written: buf.len(),
            outcome: WriteOutcome::Complete,
        }
    }
    fn set_dtr(&mut self, on: bool) -> io::Result<()> {
        self.controls
            .lock()
            .expect("controls")
            .push((Instant::now(), ControlOp::Dtr(on)));
        Ok(())
    }
    fn set_rts(&mut self, on: bool) -> io::Result<()> {
        self.controls
            .lock()
            .expect("controls")
            .push((Instant::now(), ControlOp::Rts(on)));
        Ok(())
    }
    fn set_break(&mut self, on: bool) -> io::Result<()> {
        if on && self.fail_break_on.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "break not supported"));
        }
        self.controls
            .lock()
            .expect("controls")
            .push((Instant::now(), ControlOp::Break(on)));
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

struct Rig {
    fw: FakeWorker<UartWorker>,
    controls: Controls,
    fail_break_on: Arc<AtomicBool>,
}

fn rig() -> Rig {
    let mut fw = FakeWorker::new(UartWorker::new());
    let controls: Controls = Arc::new(Mutex::new(Vec::new()));
    let fail_break_on = Arc::new(AtomicBool::new(false));
    let cx = fw.ctx().clone();
    fw.worker()
        .open_with_transport(
            &cx,
            &json!({"target": "lines", "alias": "dut", "profile": "linux-shell", "env": "sim"}),
            Box::new(Lines {
                cfg: PortConfig::default(),
                controls: Arc::clone(&controls),
                fail_break_on: Arc::clone(&fail_break_on),
            }),
        )
        .expect("open");
    let _ = fw.drain_published();
    Rig {
        fw,
        controls,
        fail_break_on,
    }
}

impl Rig {
    fn signal(&mut self, rid: &str, args: Value) -> loom_worker_sdk::WriteOutcome {
        let mut op = json!({"method": "uart.signal", "channel": "dut"});
        if let (Value::Object(m), Value::Object(a)) = (&mut op, args) {
            m.extend(a);
        }
        self.fw.write(rid, op)
    }
    fn ops(&self) -> Vec<ControlOp> {
        self.controls
            .lock()
            .expect("controls")
            .iter()
            .map(|(_, o)| *o)
            .collect()
    }
    fn gap(&self) -> Duration {
        let c = self.controls.lock().expect("controls");
        c[1].0.duration_since(c[0].0)
    }
    fn lines(&mut self) -> Value {
        self.fw
            .call("uart.status", json!({"channel": "dut"}))
            .expect("status")["lines"]
            .clone()
    }
}

fn rows(evs: &[PublishedRecord], rid: &str) -> Vec<String> {
    evs.iter()
        .filter(|e| e.kind == "uart.write" && e.request_id.as_deref() == Some(rid))
        .map(|e| e.payload["write_state"].as_str().unwrap_or("?").to_owned())
        .collect()
}

fn signal_events(evs: &[PublishedRecord]) -> Vec<&PublishedRecord> {
    evs.iter().filter(|e| e.kind == "uart.signal").collect()
}

fn code(o: &loom_worker_sdk::WriteOutcome) -> Option<ErrorCode> {
    o.error.as_ref().map(|e| e.code)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
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

#[test]
fn constants() {
    assert_eq!(BREAK_DEFAULT_MS, 250);
    assert_eq!(SIGNAL_MAX_MS, 5000);
}

#[test]
fn break_without_duration_is_a_250_ms_pulse_with_rows_and_an_event() {
    let mut r = rig();
    let o = r.signal("s-brk", json!({"kind": "break"}));
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    assert_eq!(r.ops(), vec![ControlOp::Break(true), ControlOp::Break(false)]);
    assert!(
        r.gap() + Duration::from_millis(5) >= Duration::from_millis(250),
        "held {:?}",
        r.gap()
    );
    assert_eq!(o.detail["kind"], "break");
    assert_eq!(o.detail["duration_ms"], 250);
    let evs = r.fw.drain_published();
    assert_eq!(rows(&evs, "s-brk"), vec!["requested", "accepted", "sent"]);
    let sig = signal_events(&evs);
    assert_eq!(sig.len(), 1);
    assert_eq!(sig[0].payload["kind"], "break");
    assert_eq!(sig[0].payload["duration_ms"], 250);
    assert_eq!(sig[0].payload["result"], "ok");
    assert_eq!(sig[0].request_id.as_deref(), Some("s-brk"));
    assert_eq!(r.lines(), json!({"dtr": null, "rts": null, "break": false}));
}

#[test]
fn break_duration_bounds() {
    let mut r = rig();
    let o = r.signal("s-50", json!({"kind": "break", "duration_ms": 50}));
    assert_eq!(o.state, WriteState::Sent);
    assert!(r.gap() + Duration::from_millis(5) >= Duration::from_millis(50));
    for bad in [json!(0), json!(5001), json!(-1), json!("100")] {
        let o = r.signal("s-bad", json!({"kind": "break", "duration_ms": bad.clone()}));
        assert_eq!(o.state, WriteState::Failed, "{bad}");
        assert_eq!(code(&o), Some(ErrorCode::Usage), "{bad}");
    }
    assert_eq!(r.ops().len(), 2, "bad requests touch no line");
}

#[test]
fn dtr_and_rts_set_and_keep() {
    let mut r = rig();
    assert_eq!(
        r.signal("s-d", json!({"kind": "dtr", "value": false})).state,
        WriteState::Sent
    );
    assert_eq!(
        r.signal("s-r", json!({"kind": "rts", "value": true})).state,
        WriteState::Sent
    );
    assert_eq!(r.ops(), vec![ControlOp::Dtr(false), ControlOp::Rts(true)]);
    assert_eq!(r.lines(), json!({"dtr": false, "rts": true, "break": false}));
}

#[test]
fn dtr_pulse_returns_to_the_opposite_level() {
    let mut r = rig();
    let o = r.signal("s-p", json!({"kind": "dtr", "value": false, "duration_ms": 100}));
    assert_eq!(o.state, WriteState::Sent);
    assert_eq!(r.ops(), vec![ControlOp::Dtr(false), ControlOp::Dtr(true)]);
    assert!(r.gap() + Duration::from_millis(5) >= Duration::from_millis(100));
    assert_eq!(r.lines()["dtr"], true);
}

#[test]
fn malformed_requests_are_usage_errors() {
    let mut r = rig();
    for args in [
        json!({"kind": "dtr"}),
        json!({"kind": "rts", "value": "on"}),
        json!({"kind": "reset"}),
        json!({}),
        json!({"kind": "break", "value": false}),
    ] {
        let o = r.signal("s-x", args.clone());
        assert_eq!(code(&o), Some(ErrorCode::Usage), "{args}");
    }
    assert!(r.ops().is_empty());
}

#[test]
fn stop_refuses_agent_signals_but_not_the_user() {
    let mut r = rig();
    r.fw.latch(true);
    let o = r.signal("s-stop", json!({"kind": "break", "duration_ms": 20}));
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(code(&o), Some(ErrorCode::Stopped));
    assert!(r.ops().is_empty());
    // The user's own Break (UI palette) is not latched, like user keys.
    let cx = r.fw.ctx().clone();
    let w = Write {
        call_id: U64(3),
        request_id: "s-user".into(),
        write_class: WriteClass::Request,
        op: json!({"method": "uart.signal", "channel": "dut", "kind": "break", "duration_ms": 20}),
        origin: Origin::User,
        client_id: Some("tab-a".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + 5_000_000_000),
    };
    let h = r.fw.worker().clone();
    let o = rt().block_on(h.write(&cx, w));
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    assert_eq!(r.ops(), vec![ControlOp::Break(true), ControlOp::Break(false)]);
    r.fw.latch(false);
}

#[test]
fn hold_refuses_agent_signals() {
    let mut r = rig();
    let cx = r.fw.ctx().clone();
    let h = r.fw.worker().clone();
    let call = Call {
        conn_id: Some("tab-a".into()),
        call_id: U64(7),
        method: "uart.hold".into(),
        args: json!({"channel": "dut"}),
        origin: Origin::User,
        client_id: Some("tab-a".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + 5_000_000_000),
        approval_ref: None,
    };
    rt().block_on(h.call(&cx, call)).expect("hold");
    let o = r.signal("s-held", json!({"kind": "rts", "value": true}));
    assert_eq!(code(&o), Some(ErrorCode::HeldByUser));
    assert!(r.ops().is_empty());
}

#[test]
fn stop_during_a_break_still_releases_it() {
    let mut r = rig();
    let s = later(&mut r.fw, Duration::from_millis(100), |mut h, cx| {
        cx.latch().set(true);
        h.on_latch(true);
    });
    let o = r.signal("s-long", json!({"kind": "break", "duration_ms": 300}));
    s.join().expect("stop thread");
    assert_eq!(
        o.state,
        WriteState::Sent,
        "the pulse was already running: {:?}",
        o.error
    );
    assert_eq!(r.ops(), vec![ControlOp::Break(true), ControlOp::Break(false)]);
    assert_eq!(r.lines()["break"], false);
    r.fw.latch(false);
}

#[test]
fn a_failed_assert_is_failed_and_the_release_is_still_attempted() {
    let mut r = rig();
    r.fail_break_on.store(true, Ordering::SeqCst);
    let o = r.signal("s-fail", json!({"kind": "break", "duration_ms": 20}));
    assert_eq!(o.state, WriteState::Failed);
    assert_eq!(code(&o), Some(ErrorCode::Unsupported));
    assert_eq!(r.ops(), vec![ControlOp::Break(false)], "release attempted");
    let evs = r.fw.drain_published();
    assert_eq!(signal_events(&evs)[0].payload["result"], "failed");
    assert_eq!(r.lines()["break"], false);
}
