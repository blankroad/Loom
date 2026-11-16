//! B9 correction (ROOT-accepted findings from the PRE_FINAL7 PTY run). Contract: FUNCTIONS.md §B9 (lead-written).
//! F1 — the pacing budget is `min(RPC deadline, caller window = defer_timeout_ms + timeout_ms)`: an impossible estimate is refused
//!      before any byte, the TX deadline and the expect wait are capped by it, exact `written_bytes`, no later write.
//! F2 — DTR/RTS on a device without modem control (a PTY: ENOTTY) is `E_UNSUPPORTED`, not `E_DISCONNECTED`; a real hang-up is
//!      still a disconnect.
//! The F1 PTY case reproduces the PRE_FINAL7 run exactly: RPC deadline far away (3600 s), caller 5 s + 1 s, 200 B at 100 ms/char.

use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, WriteState};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::Write;
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::UartWorker;
use loom_worker_sdk::WorkerCtx;
use serde_json::{Value, json};

/// Bytes the slow driver accepted, with their write time.
type Wire = Arc<Mutex<Vec<(Instant, u8)>>>;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime")
}

/// An agent write with an explicit RPC deadline (the gateway's budget), independent of the caller's window in `op`.
fn write_with_rpc_deadline(cx: &WorkerCtx, rid: &str, op: Value, rpc: Duration) -> Write {
    Write {
        call_id: U64(1),
        request_id: rid.into(),
        write_class: WriteClass::Request,
        op,
        origin: Origin::Agent,
        client_id: Some("agent-b9".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(cx.clock().mono_ns() + u64::try_from(rpc.as_nanos()).expect("ns")),
    }
}

fn send_op(channel: &str, data: &[u8], extra: Value) -> Value {
    let mut op = json!({"method": "uart.send", "channel": channel, "data": hex::encode(data), "eol": "none"});
    if let (Value::Object(m), Value::Object(e)) = (&mut op, extra) {
        m.extend(e);
    }
    op
}

// ---------------------------------------------------------------- slow transport (F1 enforcement)

#[derive(Debug)]
struct Silent;
impl RxHandle for Silent {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
}

/// Every write takes `per_write` (a slow driver), then accepts the bytes.
#[derive(Debug)]
struct Slow {
    cfg: PortConfig,
    per_write: Duration,
    wire: Wire,
}
impl Transport for Slow {
    fn read(&mut self, _buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        thread::sleep(wait.min(Duration::from_millis(20)));
        Ok(0)
    }
    fn write(&mut self, buf: &[u8], _deadline: Instant) -> WriteResult {
        thread::sleep(self.per_write);
        let now = Instant::now();
        self.wire
            .lock()
            .expect("wire")
            .extend(buf.iter().map(|b| (now, *b)));
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

fn slow_rig(per_write: Duration) -> (FakeWorker<UartWorker>, Wire) {
    let mut fw = FakeWorker::new(UartWorker::new());
    let wire = Arc::new(Mutex::new(Vec::new()));
    let cx = fw.ctx().clone();
    fw.worker()
        .open_with_transport(
            &cx,
            &json!({"target": "slow", "alias": "dut", "profile": "linux-shell", "env": "sim"}),
            Box::new(Slow {
                cfg: PortConfig::default(),
                per_write,
                wire: Arc::clone(&wire),
            }),
        )
        .expect("open");
    (fw, wire)
}

fn write_now(fw: &mut FakeWorker<UartWorker>, w: Write) -> loom_worker_sdk::WriteOutcome {
    let cx = fw.ctx().clone();
    let h = fw.worker().clone();
    rt().block_on(h.write(&cx, w))
}

#[test]
fn the_caller_window_refuses_an_impossible_estimate_even_with_a_far_rpc_deadline() {
    let (mut fw, wire) = slow_rig(Duration::ZERO);
    let cx = fw.ctx().clone();
    let op = send_op(
        "dut",
        &[b'r'; 200],
        json!({"char_delay_ms": 100, "timeout_ms": 5000, "defer_timeout_ms": 1000}),
    );
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-a", op, Duration::from_secs(3600)),
    );
    assert_eq!(o.state, WriteState::Failed, "{:?}", o.error);
    let e = o.error.as_ref().expect("error");
    assert_eq!(e.code, ErrorCode::Usage);
    assert_eq!(e.detail["estimate_ms"], 19_900);
    let budget = e.detail["budget_ms"].as_u64().expect("budget_ms");
    assert!(
        budget <= 6000 && budget > 5000,
        "budget = caller window 6 s, not the RPC deadline: {budget}"
    );
    thread::sleep(Duration::from_millis(300));
    assert!(wire.lock().expect("wire").is_empty(), "no byte written");
}

#[test]
fn the_tx_deadline_is_capped_by_the_caller_window_with_exact_written_bytes() {
    // Estimate 30 × 50 ms ≈ 1.45 s fits the 2 s window, but the driver needs 100 ms per byte (≈ 4.5 s in total).
    let (mut fw, wire) = slow_rig(Duration::from_millis(100));
    let cx = fw.ctx().clone();
    let op = send_op(
        "dut",
        &[b'w'; 30],
        json!({"char_delay_ms": 50, "timeout_ms": 1000, "defer_timeout_ms": 1000}),
    );
    let t0 = Instant::now();
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-b", op, Duration::from_secs(3600)),
    );
    let took = t0.elapsed();
    assert!(
        took < Duration::from_millis(2600),
        "result after {took:?}, window 2 s"
    );
    assert_eq!(o.state, WriteState::OutcomeUnknown, "{:?}", o.error);
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Timeout));
    let written = o.detail["written_bytes"].as_u64().expect("written_bytes");
    assert!(written > 0 && written < 30, "cut mid-request: {written}");
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        wire.lock().expect("wire").len() as u64,
        written,
        "exact written_bytes, no later write"
    );
}

#[test]
fn the_expect_wait_after_a_paced_send_is_capped_by_the_remaining_window() {
    let (mut fw, _wire) = slow_rig(Duration::ZERO);
    let cx = fw.ctx().clone();
    // TX ≈ 0.9 s; window 3 s; an expect that never matches must end at the window, not 0.9 s + 3 s.
    let op = send_op(
        "dut",
        &[b'e'; 10],
        json!({"char_delay_ms": 100, "timeout_ms": 3000, "defer_timeout_ms": 0, "expect": [{"kind": "literal", "pattern": "NEVER-B9"}]}),
    );
    let t0 = Instant::now();
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-c", op, Duration::from_secs(3600)),
    );
    let took = t0.elapsed();
    assert_eq!(o.state, WriteState::Sent, "the bytes went out");
    assert_eq!(
        o.error.as_ref().map(|e| e.code),
        Some(ErrorCode::Timeout),
        "the wait timed out"
    );
    assert!(
        took >= Duration::from_millis(2800) && took < Duration::from_millis(3500),
        "ended after {took:?}"
    );
}

#[test]
fn the_rpc_deadline_still_wins_when_it_is_shorter() {
    let (mut fw, wire) = slow_rig(Duration::ZERO);
    let cx = fw.ctx().clone();
    let op = send_op(
        "dut",
        &[b's'; 40],
        json!({"char_delay_ms": 100, "timeout_ms": 60_000, "defer_timeout_ms": 0}),
    );
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-d", op, Duration::from_secs(2)),
    );
    assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Usage));
    assert!(
        o.error.as_ref().expect("e").detail["budget_ms"]
            .as_u64()
            .is_some_and(|b| b <= 2000)
    );
    assert!(wire.lock().expect("wire").is_empty());
}

#[test]
fn huge_caller_values_are_clamped_without_overflow_and_unpaced_sends_are_unchanged() {
    let (mut fw, wire) = slow_rig(Duration::ZERO);
    let cx = fw.ctx().clone();
    let op = send_op(
        "dut",
        b"ok",
        json!({"timeout_ms": u64::MAX, "defer_timeout_ms": u64::MAX, "char_delay_ms": 1}),
    );
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-e", op, Duration::from_secs(10)),
    );
    assert_eq!(o.state, WriteState::Sent, "{:?}", o.error);
    let plain = send_op("dut", b"plain", json!({"timeout_ms": 1, "defer_timeout_ms": 0}));
    let o = write_now(
        &mut fw,
        write_with_rpc_deadline(&cx, "f1-f", plain, Duration::from_secs(10)),
    );
    assert_eq!(
        o.state,
        WriteState::Sent,
        "an unpaced send needs no window to transmit: {:?}",
        o.error
    );
    assert_eq!(wire.lock().expect("wire").len(), 7);
}

// ---------------------------------------------------------------- real OS PTY (macOS, like external_pty.rs)

#[cfg(target_os = "macos")]
mod pty {
    use super::*;
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    use nix::pty::{OpenptyResult, openpty};
    use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
    use nix::unistd::{read, ttyname};

    fn external_pty() -> OpenptyResult {
        let pair = openpty(None, None).expect("OS PTY");
        let mut term = tcgetattr(&pair.slave).expect("termios");
        cfmakeraw(&mut term);
        tcsetattr(&pair.slave, SetArg::TCSANOW, &term).expect("raw slave");
        fcntl(&pair.master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).expect("nonblocking master");
        pair
    }

    fn open_pty(fw: &mut FakeWorker<UartWorker>, path: &str, alias: &str) {
        let o = fw.write(
            "open",
            json!({"method": "uart.open", "target": path, "alias": alias}),
        );
        assert_eq!(o.state, WriteState::Sent, "open: {:?}", o.error);
    }

    /// Everything the device side (master) received within `wait`.
    fn drain_master(pair: &OpenptyResult, wait: Duration) -> Vec<u8> {
        let end = Instant::now() + wait;
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while Instant::now() < end {
            match read(&pair.master, &mut buf) {
                Ok(n) if n > 0 => out.extend_from_slice(&buf[..n]),
                _ => thread::sleep(Duration::from_millis(10)),
            }
        }
        out
    }

    #[test]
    fn pre_final7_case_is_refused_before_any_wire_byte_on_a_real_pty() {
        let pair = external_pty();
        let path = ttyname(&pair.slave).expect("slave name");
        let mut fw = FakeWorker::new(UartWorker::new());
        open_pty(&mut fw, path.to_str().expect("utf8"), "b9pty");
        let cx = fw.ctx().clone();
        let op = send_op(
            "b9pty",
            &[b'r'; 200],
            json!({"char_delay_ms": 100, "timeout_ms": 5000, "defer_timeout_ms": 1000}),
        );
        let o = write_now(
            &mut fw,
            write_with_rpc_deadline(&cx, "w11-replay", op, Duration::from_secs(3600)),
        );
        assert_eq!(o.state, WriteState::Failed);
        assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Usage));
        assert!(
            drain_master(&pair, Duration::from_millis(500)).is_empty(),
            "no byte reached the device"
        );
        let rows: Vec<String> = fw
            .drain_published()
            .iter()
            .filter(|e| e.kind == "uart.write" && e.request_id.as_deref() == Some("w11-replay"))
            .map(|e| e.payload["write_state"].as_str().unwrap_or("?").to_owned())
            .collect();
        assert_eq!(rows, vec!["requested", "failed"]);
        let _ = fw.write("close", json!({"method": "uart.close", "channel": "b9pty"}));
    }

    #[test]
    fn dtr_rts_on_a_pty_are_unsupported_and_break_still_works() {
        let pair = external_pty();
        let path = ttyname(&pair.slave).expect("slave name");
        let mut fw = FakeWorker::new(UartWorker::new());
        open_pty(&mut fw, path.to_str().expect("utf8"), "b9sig");
        let _ = fw.drain_published();
        for (rid, kind) in [("s-dtr", "dtr"), ("s-rts", "rts")] {
            let o = fw.write(
                rid,
                json!({"method": "uart.signal", "channel": "b9sig", "kind": kind, "value": true}),
            );
            assert_eq!(o.state, WriteState::Failed, "{kind}");
            assert_eq!(
                o.error.as_ref().map(|e| e.code),
                Some(ErrorCode::Unsupported),
                "{kind}: {:?}",
                o.error
            );
        }
        let b = fw.write(
            "s-brk",
            json!({"method": "uart.signal", "channel": "b9sig", "kind": "break", "duration_ms": 50}),
        );
        assert_eq!(b.state, WriteState::Sent, "break on a PTY: {:?}", b.error);
        let evs = fw.drain_published();
        let sig: Vec<&Value> = evs
            .iter()
            .filter(|e| e.kind == "uart.signal")
            .map(|e| &e.payload)
            .collect();
        assert_eq!(sig.len(), 3);
        assert_eq!(sig[0]["result"], "failed");
        assert_eq!(sig[0]["error"]["code"], "E_UNSUPPORTED");
        assert_eq!(sig[2]["result"], "ok");
        let st = fw
            .call("uart.status", json!({"channel": "b9sig"}))
            .expect("status");
        assert_eq!(st["connected"], true, "unsupported is not a disconnect");
        assert_eq!(
            st["lines"],
            json!({"dtr": null, "rts": null, "break": false}),
            "failed levels are not recorded"
        );
        let _ = fw.write("close", json!({"method": "uart.close", "channel": "b9sig"}));
    }

    #[test]
    fn a_real_hangup_is_still_a_disconnect() {
        let pair = external_pty();
        let path = ttyname(&pair.slave).expect("slave name");
        let mut fw = FakeWorker::new(UartWorker::new());
        open_pty(&mut fw, path.to_str().expect("utf8"), "b9hup");
        let OpenptyResult { master, slave } = pair;
        drop(master); // the device side goes away
        drop(slave);
        let end = Instant::now() + Duration::from_secs(3);
        let mut connected = true;
        while Instant::now() < end {
            connected = fw
                .call("uart.status", json!({"channel": "b9hup"}))
                .expect("status")["connected"]
                == true;
            if !connected {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert!(!connected, "hang-up seen as a disconnect");
        let o = fw.write(
            "s-after",
            json!({"method": "uart.signal", "channel": "b9hup", "kind": "rts", "value": true}),
        );
        assert_eq!(o.error.as_ref().map(|e| e.code), Some(ErrorCode::Disconnected));
        let _ = fw.write("close", json!({"method": "uart.close", "channel": "b9hup"}));
    }
}
