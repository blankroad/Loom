//! B3 (operator-upgrade U2): opt-in bounded reconnect — same target, same recorded USB identity, new
//! generation; STOP / close / window end it; nothing is resumed. Contract: FUNCTIONS.md §B3.2–B3.3 (lead-written).

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, WriteState};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    OpenOpts, PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::{
    PortIdentity, RECONNECT_POLL, RECONNECT_WINDOW_DEFAULT, RECONNECT_WINDOW_MAX, RECONNECT_WINDOW_MIN,
    Reopener, UartWorker,
};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(3);

/// One simulated USB device slot: plugged or not, and which device is in it.
#[derive(Debug)]
struct Bench {
    plugged: AtomicBool,
    identity: Mutex<Option<PortIdentity>>,
    opens: AtomicUsize,
    /// bytes written per transport generation (index = open number, 0 = the initial open)
    wires: Mutex<Vec<Arc<Mutex<Vec<u8>>>>>,
}

impl Bench {
    fn new(identity: Option<PortIdentity>) -> Arc<Self> {
        Arc::new(Self {
            plugged: AtomicBool::new(true),
            identity: Mutex::new(identity),
            opens: AtomicUsize::new(0),
            wires: Mutex::new(Vec::new()),
        })
    }
    fn transport(self: &Arc<Self>) -> Box<dyn Transport> {
        let wire = Arc::new(Mutex::new(Vec::new()));
        self.wires.lock().expect("wires").push(Arc::clone(&wire));
        Box::new(Dev {
            cfg: PortConfig::default(),
            bench: Arc::clone(self),
            wire,
        })
    }
    fn unplug(&self) {
        self.plugged.store(false, Ordering::SeqCst);
    }
    fn plug(&self, identity: Option<PortIdentity>) {
        *self.identity.lock().expect("id") = identity;
        self.plugged.store(true, Ordering::SeqCst);
    }
    fn wire(&self, n: usize) -> Vec<u8> {
        self.wires.lock().expect("wires")[n].lock().expect("wire").clone()
    }
}

#[derive(Debug)]
struct Dev {
    cfg: PortConfig,
    bench: Arc<Bench>,
    wire: Arc<Mutex<Vec<u8>>>,
}

#[derive(Debug)]
struct DevRx(Arc<Bench>);

impl RxHandle for DevRx {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        if !self.0.plugged.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "unplugged"));
        }
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
}

impl Transport for Dev {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        if !self.bench.plugged.load(Ordering::SeqCst) {
            return WriteResult {
                written: 0,
                outcome: WriteOutcome::Failed(io::ErrorKind::BrokenPipe),
            };
        }
        self.wire.lock().expect("wire").extend_from_slice(buf);
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
        TransportKind::Serial
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        Ok(Box::new(DevRx(Arc::clone(&self.bench))))
    }
}

#[derive(Debug)]
struct FakeReopener(Arc<Bench>);

impl Reopener for FakeReopener {
    fn identity(&self, target: &str) -> Option<PortIdentity> {
        assert_eq!(target, "/dev/ttyUSB7", "reopen uses the same target");
        if !self.0.plugged.load(Ordering::SeqCst) {
            return None;
        }
        Some(self.0.identity.lock().expect("id").clone().unwrap_or_default())
    }
    fn open(&self, target: &str, _config: &PortConfig, _opts: &OpenOpts) -> io::Result<Box<dyn Transport>> {
        assert_eq!(target, "/dev/ttyUSB7");
        if !self.0.plugged.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "absent"));
        }
        self.0.opens.fetch_add(1, Ordering::SeqCst);
        Ok(self.0.transport())
    }
}

fn usb(serial: &str) -> PortIdentity {
    PortIdentity {
        vid: Some(0x0403),
        pid: Some(0x6001),
        serial: Some(serial.into()),
    }
}

struct Rig {
    fw: FakeWorker<UartWorker>,
    bench: Arc<Bench>,
    /// every record drained so far
    seen: Vec<PublishedRecord>,
}

fn rig(identity: Option<PortIdentity>, extra: Value) -> Result<Rig, loom_proto::worker::WorkerError> {
    let mut fw = FakeWorker::new(UartWorker::new());
    let bench = Bench::new(identity);
    fw.worker()
        .set_reopener(Arc::new(FakeReopener(Arc::clone(&bench))));
    let mut op = json!({"target": "/dev/ttyUSB7", "alias": "dut", "profile": "linux-shell", "env": "bench"});
    if let (Value::Object(m), Value::Object(e)) = (&mut op, extra) {
        m.extend(e);
    }
    let cx = fw.ctx().clone();
    let t = bench.transport();
    fw.worker().open_with_transport(&cx, &op, t)?;
    Ok(Rig {
        fw,
        bench,
        seen: Vec::new(),
    })
}

fn enabled(window_ms: u64) -> Value {
    json!({"reconnect": {"enabled": true, "window_ms": window_ms}})
}

impl Rig {
    fn drain(&mut self) {
        let evs = self.fw.drain_published();
        self.seen.extend(evs);
    }
    fn states(&self) -> Vec<String> {
        self.seen
            .iter()
            .filter(|e| e.kind == "uart.state")
            .map(|e| e.payload["state"].as_str().unwrap_or("?").to_owned())
            .collect()
    }
    fn last_state(&self) -> Option<&PublishedRecord> {
        self.seen.iter().rfind(|e| e.kind == "uart.state")
    }
    /// Drain until the latest uart.state is `state` (≤ `within`).
    fn wait_state(&mut self, state: &str, within: Duration) -> bool {
        let end = Instant::now() + within;
        while Instant::now() < end {
            self.drain();
            if self.last_state().is_some_and(|e| e.payload["state"] == state) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
    fn status(&mut self) -> Value {
        self.fw
            .call("uart.status", json!({"channel": "dut"}))
            .expect("status")
    }
}

#[test]
fn constants() {
    assert_eq!(RECONNECT_POLL, Duration::from_millis(300));
    assert_eq!(RECONNECT_WINDOW_DEFAULT, Duration::from_secs(10));
    assert_eq!(RECONNECT_WINDOW_MIN, Duration::from_secs(1));
    assert_eq!(RECONNECT_WINDOW_MAX, Duration::from_secs(120));
}

#[test]
fn bad_reconnect_options_are_usage_errors() {
    for bad in [
        json!({"enabled": true, "window_ms": 999}),
        json!({"enabled": true, "window_ms": 120_001}),
        json!({"enabled": "yes"}),
        json!({"enabled": true, "retries": 3}),
        json!(true),
    ] {
        let res = rig(Some(usb("A")), json!({"reconnect": bad.clone()}));
        assert_eq!(res.err().map(|e| e.code), Some(ErrorCode::Usage), "{bad}");
    }
}

#[test]
fn status_reports_the_setting_and_the_recorded_identity() {
    let mut r = rig(Some(usb("A")), enabled(2000)).expect("open");
    let st = r.status();
    assert_eq!(
        st["reconnect"],
        json!({"enabled": true, "window_ms": 2000, "active": false, "attempts": 0})
    );
    assert_eq!(
        st["identity"],
        json!({"vid": 0x0403, "pid": 0x6001, "serial": "A"})
    );
    let mut off = rig(None, json!({})).expect("open");
    assert_eq!(off.status()["reconnect"]["enabled"], false);
    assert_eq!(off.status()["reconnect"]["window_ms"], 10_000);
}

#[test]
fn without_the_option_a_disconnect_stays_disconnected() {
    let mut r = rig(Some(usb("A")), json!({})).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("disconnected", WAIT));
    r.bench.plug(Some(usb("A")));
    thread::sleep(Duration::from_millis(800));
    r.drain();
    assert_eq!(r.bench.opens.load(Ordering::SeqCst), 0, "never reopened");
    assert!(!r.states().iter().any(|s| s == "reconnecting"));
    assert_eq!(r.status()["connected"], false);
}

#[test]
fn the_same_device_is_reopened_as_a_new_generation() {
    let mut r = rig(Some(usb("A")), enabled(3000)).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT), "states {:?}", r.states());
    let rc = r.last_state().expect("state").payload["reconnect"].clone();
    assert_eq!(rc["enabled"], true);
    assert_eq!(rc["window_ms"], 3000);
    assert_eq!(rc["active"], true);
    assert_eq!(r.status()["reconnect"]["active"], true);
    thread::sleep(Duration::from_millis(400));
    r.bench.plug(Some(usb("A")));
    assert!(r.wait_state("open", WAIT), "states {:?}", r.states());
    let open = r.last_state().expect("open").clone();
    assert_eq!(open.generation, Some(2));
    let reopen = &open.payload["reopen"];
    assert!(reopen["attempt"].as_u64().is_some_and(|a| a >= 1));
    assert!(reopen["after_ms"].as_u64().is_some());
    assert_eq!(reopen["identity"], "usb");
    assert_eq!(reopen["previous_generation"], 1);
    assert!(
        r.seen
            .iter()
            .any(|e| e.kind == "uart.config" && e.generation == Some(2)),
        "config re-emitted for the new generation"
    );
    let st = r.status();
    assert_eq!(st["connected"], true);
    assert_eq!(st["generation"], 2);
    assert_eq!(st["reconnect"]["active"], false);
    assert_eq!(r.bench.opens.load(Ordering::SeqCst), 1);
    // The new port works and nothing from before was replayed onto it.
    assert!(r.bench.wire(1).is_empty(), "no automatic resend");
    let o = r.fw.write(
        "after",
        json!({"method": "uart.send", "channel": "dut", "data": hex::encode("ok"), "eol": "none"}),
    );
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    assert_eq!(r.bench.wire(1), b"ok");
}

#[test]
fn a_different_device_at_the_path_is_refused() {
    let mut r = rig(Some(usb("A")), enabled(3000)).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    r.bench.plug(Some(usb("B")));
    assert!(r.wait_state("disconnected", WAIT), "states {:?}", r.states());
    let last = r.last_state().expect("state");
    assert_eq!(last.payload["reason"], "identity_mismatch");
    assert_eq!(last.payload["expected"]["serial"], "A");
    assert_eq!(last.payload["found"]["serial"], "B");
    thread::sleep(Duration::from_millis(700));
    assert_eq!(
        r.bench.opens.load(Ordering::SeqCst),
        0,
        "never opened the other device"
    );
    assert_eq!(r.status()["reconnect"]["active"], false);
}

#[test]
fn the_window_ends_the_episode() {
    let mut r = rig(Some(usb("A")), enabled(1000)).expect("open");
    let t0 = Instant::now();
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    assert!(r.wait_state("disconnected", WAIT), "states {:?}", r.states());
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_millis(2500),
        "gave up after {took:?}"
    );
    let last = r.last_state().expect("state");
    assert_eq!(last.payload["reason"], "gave_up");
    assert!(
        last.payload["reconnect"]["attempts"]
            .as_u64()
            .is_some_and(|a| a >= 2)
    );
    r.bench.plug(Some(usb("A")));
    thread::sleep(Duration::from_millis(700));
    assert_eq!(r.bench.opens.load(Ordering::SeqCst), 0, "nothing after giving up");
}

#[test]
fn close_while_reconnecting_ends_it_silently() {
    let mut r = rig(Some(usb("A")), enabled(5000)).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    let o =
        r.fw.write("close", json!({"method": "uart.close", "channel": "dut"}));
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    r.bench.plug(Some(usb("A")));
    thread::sleep(Duration::from_millis(900));
    r.drain();
    assert_eq!(r.bench.opens.load(Ordering::SeqCst), 0, "no reopen after close");
    assert_eq!(r.states().last().map(String::as_str), Some("closed"));
    assert_eq!(r.fw.worker().live_threads(), 0);
}

#[test]
fn stop_ends_a_reconnect_episode() {
    let mut r = rig(Some(usb("A")), enabled(5000)).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    r.fw.latch(true);
    assert!(r.wait_state("disconnected", WAIT), "states {:?}", r.states());
    assert_eq!(r.last_state().expect("state").payload["reason"], "stopped");
    r.bench.plug(Some(usb("A")));
    thread::sleep(Duration::from_millis(700));
    assert_eq!(r.bench.opens.load(Ordering::SeqCst), 0);
    r.fw.latch(false);
}

#[test]
fn pacing_and_hold_survive_a_reconnect() {
    let mut r = rig(
        Some(usb("A")),
        json!({"reconnect": {"enabled": true, "window_ms": 3000}, "pacing": {"line_delay_ms": 40}}),
    )
    .expect("open");
    let cx = r.fw.ctx().clone();
    let h = r.fw.worker().clone();
    let call = loom_proto::worker::Call {
        conn_id: Some("tab-a".into()),
        call_id: loom_proto::num::U64(7),
        method: "uart.hold".into(),
        args: json!({"channel": "dut"}),
        origin: loom_proto::event::Origin::User,
        client_id: Some("tab-a".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: loom_proto::num::U64(cx.clock().mono_ns() + 5_000_000_000),
        approval_ref: None,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("rt")
        .block_on(h.call(&cx, call))
        .expect("hold");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    r.bench.plug(Some(usb("A")));
    assert!(r.wait_state("open", WAIT));
    let st = r.status();
    assert_eq!(st["generation"], 2);
    assert_eq!(st["pacing"]["line_delay_ms"], 40);
    assert_eq!(
        st["hold"], true,
        "HOLD is a user safety state; a reconnect must not drop it"
    );
}

#[test]
fn an_unknown_identity_reopens_by_path_and_says_so() {
    let mut r = rig(None, enabled(3000)).expect("open");
    assert_eq!(r.status()["identity"], Value::Null);
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    r.bench.plug(None);
    assert!(r.wait_state("open", WAIT));
    assert_eq!(
        r.last_state().expect("open").payload["reopen"]["identity"],
        "path_only"
    );
}

#[test]
fn a_send_cut_by_the_unplug_is_not_resent() {
    let mut r = rig(Some(usb("A")), enabled(3000)).expect("open");
    r.bench.unplug();
    assert!(r.wait_state("reconnecting", WAIT));
    let o = r.fw.write(
        "during",
        json!({"method": "uart.send", "channel": "dut", "data": hex::encode("lost"), "eol": "none"}),
    );
    assert_ne!(o.state, WriteState::Sent, "no device to write to");
    r.bench.plug(Some(usb("A")));
    assert!(r.wait_state("open", WAIT));
    thread::sleep(Duration::from_millis(200));
    assert!(r.bench.wire(1).is_empty(), "the failed send is not replayed");
}
