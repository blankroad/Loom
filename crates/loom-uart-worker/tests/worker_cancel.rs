//! TX cancellation wired into the worker (task p1-opus-cancel-wire, TX API 168ac1c):
//! user preemption of any size, STOP, HOLD and close cut agent writes at a chunk boundary and
//! report the honest terminal state; user bytes keep flowing.
//!
//! A slow transport (16 B per 2 ms write) keeps a 4 KiB agent send on the wire for ~0.5 s, so
//! the cut point is deterministic. Concurrent actors use clones of the worker handle on other
//! threads (the SDK runtime serializes callbacks today — CORE request 6).

use std::io;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use loom_proto::bytes::Bytes;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Input, Write};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::UartWorker;
use loom_worker_sdk::{Worker, WorkerCtx};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(3);

/// Accepts 16 bytes per 2 ms write call and records what it accepted; never produces RX.
#[derive(Debug)]
struct SlowTransport {
    cfg: PortConfig,
    wire: Arc<Mutex<Vec<u8>>>,
}
#[derive(Debug)]
struct Silent;

impl RxHandle for Silent {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait);
        Ok(0)
    }
}

impl Transport for SlowTransport {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait);
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        thread::sleep(Duration::from_millis(2));
        let n = buf.len().min(16);
        self.wire.lock().expect("wire lock").extend_from_slice(&buf[..n]);
        WriteResult {
            written: n,
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

struct Rig {
    fw: FakeWorker<UartWorker>,
    wire: Arc<Mutex<Vec<u8>>>,
}

fn rig() -> Rig {
    let mut fw = FakeWorker::new(UartWorker::new());
    let wire = Arc::new(Mutex::new(Vec::new()));
    let cx = fw.ctx().clone();
    fw.worker()
        .open_with_transport(
            &cx,
            &json!({"target": "slow", "alias": "dut", "profile": "linux-shell", "env": "sim"}),
            Box::new(SlowTransport {
                cfg: PortConfig::default(),
                wire: Arc::clone(&wire),
            }),
        )
        .expect("open slow transport");
    Rig { fw, wire }
}

fn send_op(text_len: usize, byte: u8) -> Value {
    json!({"method": "uart.send", "channel": "dut", "data": hex::encode(vec![byte; text_len]), "eol": "none",
           "defer_timeout_ms": 3000})
}

fn write_msg(cx: &WorkerCtx, rid: &str, op: Value) -> Write {
    Write {
        call_id: U64(9),
        request_id: rid.into(),
        write_class: WriteClass::Request,
        op,
        origin: Origin::Agent,
        client_id: Some("agent-b".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + 5_000_000_000),
    }
}

fn call_msg(cx: &WorkerCtx, method: &str, args: Value) -> Call {
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

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
}

/// Run `f` on another thread after `delay` with a clone of the worker handle.
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

fn input(h: &UartWorker, cx: &WorkerCtx, lease: &str, bytes: &[u8]) {
    rt().block_on(h.input(
        cx,
        Input {
            conn_id: Some(lease.into()),
            channel: Some("dut".into()),
            input_seq: Some(U64(1)),
            call_id: None,
            lease_id: lease.into(),
            bytes: Bytes(bytes.to_vec()),
            priority: false,
        },
    ))
    .expect("viewer input accepted");
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

/// Wait until the transport accepted `n` bytes of `byte` (≤ 3 s).
fn wire_has(wire: &Arc<Mutex<Vec<u8>>>, byte: u8, n: usize) -> bool {
    let end = Instant::now() + WAIT;
    while Instant::now() < end {
        if wire.lock().expect("wire").iter().filter(|b| **b == byte).count() >= n {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

fn close(fw: &mut FakeWorker<UartWorker>) {
    let o = fw.write("close", json!({"method": "uart.close", "channel": "dut"}));
    assert_eq!(o.state, WriteState::Sent, "close: {:?}", o.error);
}

// ---------------------------------------------------------------- user preemption (no 64 B bypass)

#[test]
fn a_4_kib_user_paste_cuts_a_large_agent_send() {
    let Rig { mut fw, wire } = rig();
    let paste = vec![b'p'; 4096];
    let p2 = paste.clone();
    let t = later(&mut fw, Duration::from_millis(100), move |h, cx| {
        input(&h, &cx, "tab-a", &p2)
    });
    let o = fw.write("q-big", send_op(4096, b'a'));
    t.join().expect("paste thread");
    assert_eq!(o.state, WriteState::OutcomeUnknown, "detail {}", o.detail);
    assert_eq!(
        o.error.as_ref().map(|e| e.code),
        Some(ErrorCode::InterruptedByUser)
    );
    let written = o.detail["written_bytes"].as_u64().unwrap_or(0);
    assert!(
        written > 0 && written < 4096,
        "agent cut mid-transfer, written={written}"
    );

    // The whole paste reaches the device, after the agent's last byte (no interleaving).
    assert!(wire_has(&wire, b'p', 4096), "user paste not fully written");
    let w = wire.lock().expect("wire").clone();
    let first_user = w.iter().position(|b| *b == b'p').expect("user bytes");
    let last_agent = w.iter().rposition(|b| *b == b'a').expect("agent bytes");
    assert!(
        last_agent < first_user,
        "agent bytes after the user's paste started"
    );
    assert_eq!(
        w.iter().filter(|b| **b == b'a').count() as u64,
        written,
        "written_bytes is exact"
    );

    let evs = fw.drain_published();
    let last = terminal_row(&evs, "q-big");
    assert_eq!(last.payload["write_state"], "outcome_unknown");
    assert_eq!(last.payload["cause"], "interrupted_by_user");
    assert_eq!(last.payload["error"]["code"], "E_INTERRUPTED_BY_USER");
    close(&mut fw);
}

// ---------------------------------------------------------------- STOP

#[test]
fn stop_cuts_the_running_send_fails_the_waiting_one_and_user_keys_still_go() {
    let Rig { mut fw, wire } = rig();
    // Agent B arrives while A runs: it waits for admission (one agent request at a time).
    let b = later(&mut fw, Duration::from_millis(30), |h, cx| {
        rt().block_on(h.write(&cx, write_msg(&cx, "q-b", send_op(64, b'b'))))
    });
    // STOP at 120 ms, then a user key under STOP.
    let s = later(&mut fw, Duration::from_millis(120), |mut h, cx| {
        cx.latch().set(true);
        h.on_latch(true);
        input(&h, &cx, "tab-a", b"u");
    });
    let a = fw.write("q-a", send_op(4096, b'a'));
    let b = b.join().expect("agent b");
    s.join().expect("stop thread");

    assert_eq!(a.state, WriteState::OutcomeUnknown, "A {}", a.detail);
    assert_eq!(a.error.as_ref().map(|e| e.code), Some(ErrorCode::Stopped));
    let written = a.detail["written_bytes"].as_u64().unwrap_or(0);
    assert!(written > 0 && written < 4096, "A written={written}");
    assert_eq!(b.state, WriteState::Failed, "B must not run under STOP");
    assert_eq!(b.error.as_ref().map(|e| e.code), Some(ErrorCode::Stopped));
    assert!(
        wire_has(&wire, b'u', 1),
        "the user key must reach the device under STOP"
    );
    assert_eq!(
        wire.lock().expect("wire").iter().filter(|x| **x == b'b').count(),
        0
    );

    let evs = fw.drain_published();
    assert_eq!(terminal_row(&evs, "q-a").payload["cause"], "stopped");
    assert_eq!(rows(&evs, "q-b").last().map(String::as_str), Some("failed"));
    fw.latch(false);
    close(&mut fw);
}

// ---------------------------------------------------------------- HOLD

#[test]
fn hold_cuts_the_running_agent_send() {
    let Rig { mut fw, wire } = rig();
    let h = later(&mut fw, Duration::from_millis(100), |h, cx| {
        rt().block_on(h.call(&cx, call_msg(&cx, "uart.hold", json!({"channel": "dut"}))))
            .expect("hold")
    });
    let o = fw.write("q-held", send_op(4096, b'a'));
    let held = h.join().expect("hold thread");
    assert_eq!(held["hold"], true);
    assert_eq!(o.state, WriteState::OutcomeUnknown, "{}", o.detail);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::HeldByUser));
    let written = o.detail["written_bytes"].as_u64().unwrap_or(0);
    assert!(written > 0 && written < 4096);
    assert_eq!(
        wire.lock().expect("wire").len() as u64,
        written,
        "nothing after the cut"
    );
    // While HOLD stays on, new agent writes are refused at once.
    let again = fw.write("q-again", send_op(16, b'c'));
    assert_eq!(again.error.as_ref().map(|e| e.code), Some(ErrorCode::HeldByUser));
    let evs = fw.drain_published();
    assert_eq!(terminal_row(&evs, "q-held").payload["cause"], "held_by_user");
    close(&mut fw);
}

// ---------------------------------------------------------------- close

#[test]
fn close_ends_an_unfinished_agent_send_outcome_unknown() {
    let Rig { mut fw, .. } = rig();
    let c = later(&mut fw, Duration::from_millis(100), |h, cx| {
        let w = write_msg(&cx, "close-x", json!({"method": "uart.close", "channel": "dut"}));
        rt().block_on(h.write(&cx, w))
    });
    let o = fw.write("q-closing", send_op(4096, b'a'));
    let closed = c.join().expect("close thread");
    assert_eq!(closed.state, WriteState::Sent, "close: {:?}", closed.error);
    assert_eq!(o.state, WriteState::OutcomeUnknown, "{}", o.detail);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::NotOpen));
    let written = o.detail["written_bytes"].as_u64().unwrap_or(0);
    assert!(written > 0 && written < 4096);
    let evs = fw.drain_published();
    let last = terminal_row(&evs, "q-closing");
    assert_eq!(last.payload["cause"], "closed");
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.state" && e.payload["state"] == "closed" && e.dir.is_none()),
        "closed state event"
    );
}
