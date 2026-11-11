//! UartWorker end to end on `sim://bootshell` through `FakeWorker` (task p1-opus-worker).
//!
//! Concurrency note: `FakeWorker` drives one callback at a time (`&mut self`). Viewer input that
//! must arrive *while* an agent write is waiting goes through a clone of the worker handle on a
//! second thread — the shape the SDK runtime needs for contract 05 §5.1 (see the report).

use std::io;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use loom_proto::bytes::Bytes;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Origin, PublishedRecord, WriteState};
use loom_proto::worker::Input;
use loom_testkit::FakeWorker;
use loom_testkit::asserts::{assert_no_gap_lies, assert_write_rows};
use loom_uart_worker::transport::{
    PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult,
};
use loom_uart_worker::worker::UartWorker;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(2);

fn hexs(s: &str) -> String {
    hex::encode(s.as_bytes())
}

fn new_fw() -> FakeWorker<UartWorker> {
    FakeWorker::new(UartWorker::new())
}

fn open(fw: &mut FakeWorker<UartWorker>) -> Value {
    let out = fw.write(
        "open-1",
        json!({"method": "uart.open", "target": "sim://bootshell?split=0", "alias": "dut", "profile": "linux-shell"}),
    );
    assert_eq!(out.state, WriteState::Sent, "open failed: {:?}", out.error);
    out.detail
}

fn viewer_input(
    fw: &mut FakeWorker<UartWorker>,
    viewer: &str,
    bytes: &[u8],
    priority: bool,
) -> Result<(), loom_proto::worker::WorkerError> {
    let cx = fw.ctx().clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("viewer input runtime")
        .block_on(fw.worker().input(
            &cx,
            Input {
                conn_id: Some(viewer.into()),
                channel: Some("dut".into()),
                input_seq: Some(loom_proto::num::U64(1)),
                call_id: None,
                lease_id: viewer.into(),
                bytes: Bytes(bytes.to_vec()),
                priority,
            },
        ))
}

fn send(
    fw: &mut FakeWorker<UartWorker>,
    rid: &str,
    text: &str,
    extra: Value,
) -> loom_worker_sdk::WriteOutcome {
    let mut op = json!({"method": "uart.send", "channel": "dut", "data": hexs(text), "eol": "cr"});
    if let (Value::Object(m), Value::Object(e)) = (&mut op, extra) {
        m.extend(e);
    }
    fw.write(rid, op)
}

fn expect_lit(fw: &mut FakeWorker<UartWorker>, pat: &str) -> Result<Value, loom_proto::worker::WorkerError> {
    fw.call(
        "uart.expect",
        json!({"channel": "dut", "patterns": [{"kind": "literal", "pattern": pat}], "timeout_ms": 2000}),
    )
}

/// Boot → login → shell prompt.
fn login(fw: &mut FakeWorker<UartWorker>) {
    expect_lit(fw, "login: ").expect("login prompt");
    let o = send(
        fw,
        "l-user",
        "root",
        json!({"expect": [{"kind": "literal", "pattern": "Password: "}], "timeout_ms": 2000}),
    );
    assert!(o.error.is_none(), "user: {:?}", o.error);
    let o = send(
        fw,
        "l-pass",
        "secret",
        json!({"secret": true, "expect": [{"kind": "prompt"}], "timeout_ms": 2000}),
    );
    assert!(o.error.is_none(), "password: {:?}", o.error);
}

fn rows_of<'a>(evs: &'a [PublishedRecord], rid: &str) -> Vec<&'a PublishedRecord> {
    evs.iter()
        .filter(|e| e.kind == "uart.write" && e.request_id.as_deref() == Some(rid))
        .collect()
}

fn write_states(evs: &[PublishedRecord], rid: &str) -> Vec<String> {
    rows_of(evs, rid)
        .iter()
        .map(|e| e.payload["write_state"].as_str().unwrap_or("?").to_owned())
        .collect()
}

/// Viewer input from another thread (the worker handle is `Clone`).
fn input_later(
    fw: &mut FakeWorker<UartWorker>,
    delay: Duration,
    lease: &str,
    bytes: &[u8],
) -> JoinHandle<()> {
    let h = fw.worker().clone();
    let cx = fw.ctx().clone();
    let lease = lease.to_owned();
    let bytes = bytes.to_vec();
    thread::spawn(move || {
        thread::sleep(delay);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        rt.block_on(h.input(
            &cx,
            Input {
                conn_id: Some(lease.clone()),
                channel: Some("dut".into()),
                input_seq: Some(loom_proto::num::U64(1)),
                call_id: None,
                lease_id: lease,
                bytes: Bytes(bytes),
                priority: false,
            },
        ))
        .expect("viewer input accepted");
    })
}

/// Drain until `pred` holds over everything seen so far (≤ 2 s).
fn drain_until(
    fw: &mut FakeWorker<UartWorker>,
    seen: &mut Vec<PublishedRecord>,
    pred: impl Fn(&[PublishedRecord]) -> bool,
) {
    let end = Instant::now() + WAIT;
    loop {
        seen.extend(fw.drain_published());
        if pred(seen) {
            return;
        }
        assert!(Instant::now() < end, "condition not reached within {WAIT:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn close(fw: &mut FakeWorker<UartWorker>) -> Value {
    let out = fw.write("close-1", json!({"method": "uart.close", "channel": "dut"}));
    assert_eq!(out.state, WriteState::Sent, "close failed: {:?}", out.error);
    out.detail
}

// ---------------------------------------------------------------- 1. core loop

#[test]
fn core_loop_open_expect_send_run_read() {
    let mut fw = new_fw();
    let info = open(&mut fw);
    assert_eq!(info["channel"], "dut");
    assert_eq!(info["env"], "sim");
    assert_eq!(info["generation"], 1);
    login(&mut fw);

    let run = fw.write(
        "run-1",
        json!({"method": "uart.run", "channel": "dut", "data": hexs("uname -a"), "eol": "cr", "timeout_ms": 2000}),
    );
    assert!(run.error.is_none(), "run: {:?}", run.error);
    let output = run.detail["output"].to_string();
    assert!(output.contains("Linux sim"), "run output: {output}");
    assert!(
        !output.contains("uname -a"),
        "command echo must be stripped: {output}"
    );

    let read = fw
        .call(
            "uart.read",
            json!({"channel": "dut", "mode": "peek", "from": "off:0"}),
        )
        .expect("read");
    assert!(
        read["lines"].as_array().is_some_and(|l| !l.is_empty()),
        "read: {read}"
    );
    // `run` committed the agent cursor to the prompt match: a summary read of the same client
    // does not hand the command output back again (plan §4.5).
    let fresh = fw
        .call("uart.read", json!({"channel": "dut", "mode": "summary"}))
        .expect("summary read");
    assert!(
        !fresh["lines"].to_string().contains("Linux sim"),
        "cursor not committed: {fresh}"
    );

    let evs = fw.drain_published();
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.data" && e.dir == Some(Dir::Rx))
    );
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.data" && e.dir == Some(Dir::Tx) && e.origin == Origin::Agent)
    );
    for rid in ["l-user", "l-pass", "run-1"] {
        let rows: Vec<PublishedRecord> = rows_of(&evs, rid).into_iter().cloned().collect();
        assert_write_rows(&rows);
        assert_eq!(
            write_states(&evs, rid),
            ["requested", "accepted", "sent"],
            "{rid}"
        );
        let last = rows.last().expect("rows");
        assert_eq!(
            last.payload["final"], true,
            "{rid}: terminal row carries final:true"
        );
    }
    // The password never appears in clear: the write row has `secret`, not `bytes`.
    let pass_req = rows_of(&evs, "l-pass")[0];
    assert!(pass_req.payload.get("bytes").is_none());
    assert_eq!(pass_req.payload["secret"]["len"], 7);
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.expect_result" && e.request_id.as_deref() == Some("run-1"))
    );
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.state" && e.payload["state"] == "open")
    );
    assert_no_gap_lies(&evs);
    close(&mut fw);
}

// ---------------------------------------------------------------- 2. user intervention

#[test]
fn agent_send_waits_for_the_users_line_then_goes() {
    let mut fw = new_fw();
    open(&mut fw);
    login(&mut fw);
    viewer_input(&mut fw, "tab-a", b"ifcon", false).expect("typing");
    // The user finishes the line 300 ms later; the shell prints a new prompt.
    let t = input_later(&mut fw, Duration::from_millis(300), "tab-a", b"fig\r");
    let o = send(&mut fw, "q-defer", "echo hi", json!({"defer_timeout_ms": 3000}));
    t.join().expect("input thread");
    assert!(o.error.is_none(), "deferred send failed: {:?}", o.error);
    assert_eq!(o.state, WriteState::Sent);
    let waited = o.detail["deferred_ms"].as_u64().unwrap_or(0);
    assert!(
        waited >= 200,
        "agent must have waited for the user, deferred_ms={waited}"
    );
    // Events from the pump threads can land after the reply: wait for them (bounded).
    let mut evs = Vec::new();
    drain_until(&mut fw, &mut evs, |e| {
        e.iter()
            .any(|e| e.kind == "uart.agent_activity" && e.payload["kind"] == "deferred_send")
            && e.iter()
                .any(|e| e.kind == "uart.data" && e.dir == Some(Dir::Tx) && e.origin == Origin::Agent)
    });
    // The device saw the user's whole line before the agent's bytes.
    let tx: Vec<&PublishedRecord> = evs
        .iter()
        .filter(|e| e.kind == "uart.data" && e.dir == Some(Dir::Tx))
        .collect();
    let user_last = tx
        .iter()
        .rposition(|e| e.origin == Origin::User)
        .expect("user tx");
    let agent_first = tx
        .iter()
        .position(|e| e.request_id.as_deref() == Some("q-defer"))
        .expect("agent tx");
    assert!(user_last < agent_first, "agent bytes mixed into the user's line");
    close(&mut fw);
}

#[test]
fn unfinished_user_line_makes_the_agent_fail_user_active() {
    let mut fw = new_fw();
    open(&mut fw);
    login(&mut fw);
    viewer_input(&mut fw, "tab-a", b"ifcon", false).expect("typing");
    let o = send(&mut fw, "q-busy", "echo hi", json!({"defer_timeout_ms": 300}));
    let e = o.error.expect("must fail");
    assert_eq!(e.code, ErrorCode::UserActive);
    assert_eq!(o.state, WriteState::Failed);
    let evs = fw.drain_published();
    assert_eq!(write_states(&evs, "q-busy"), ["requested", "failed"]);
    assert!(
        !evs.iter()
            .any(|e| e.kind == "uart.data" && e.request_id.as_deref() == Some("q-busy")),
        "nothing of the agent request may reach the device"
    );
    close(&mut fw);
}

#[test]
fn hold_refuses_agent_writes() {
    let mut fw = new_fw();
    open(&mut fw);
    login(&mut fw);
    fw.call("uart.hold", json!({"channel": "dut"})).expect("hold");
    let o = send(&mut fw, "q-held", "echo hi", json!({}));
    assert_eq!(o.error.expect("held").code, ErrorCode::HeldByUser);
    fw.call("uart.unhold", json!({"channel": "dut"})).expect("unhold");
    let o = send(&mut fw, "q-free", "echo hi", json!({}));
    assert!(o.error.is_none(), "{:?}", o.error);
    close(&mut fw);
}

#[test]
fn stop_latch_blocks_agent_but_not_the_viewer() {
    let mut fw = new_fw();
    open(&mut fw);
    login(&mut fw);
    fw.drain_published();
    fw.latch(true);
    let o = send(&mut fw, "q-stop", "echo hi", json!({}));
    assert_eq!(o.error.expect("stopped").code, ErrorCode::Stopped);
    viewer_input(&mut fw, "tab-a", b"echo user\r", false).expect("viewer input under STOP");
    let mut seen = Vec::new();
    drain_until(&mut fw, &mut seen, |evs| {
        evs.iter()
            .any(|e| e.kind == "uart.data" && e.dir == Some(Dir::Tx) && e.origin == Origin::User)
    });
    assert_eq!(write_states(&seen, "q-stop"), ["requested", "failed"]);
    fw.latch(false);
    close(&mut fw);
}

// ---------------------------------------------------------------- Ctrl-C preemption

/// Accepts 16 bytes per 2 ms write call; never produces RX.
#[derive(Debug)]
struct SlowTransport(PortConfig);
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
        WriteResult {
            written: buf.len().min(16),
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
        &self.0
    }
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        Ok(Box::new(Silent))
    }
}

#[test]
fn ctrl_c_cuts_a_large_agent_send() {
    let mut fw = new_fw();
    let cx = fw.ctx().clone();
    fw.worker()
        .open_with_transport(
            &cx,
            &json!({"target": "slow", "alias": "dut", "profile": "linux-shell", "env": "sim"}),
            Box::new(SlowTransport(PortConfig::default())),
        )
        .expect("open slow transport");
    let big = "x".repeat(4096);
    let t = input_later(&mut fw, Duration::from_millis(100), "tab-a", b"\x03");
    let o = send(&mut fw, "q-big", &big, json!({"eol": "none"}));
    t.join().expect("input thread");
    assert_eq!(o.state, WriteState::OutcomeUnknown, "detail {}", o.detail);
    assert_eq!(
        o.error.as_ref().map(|e| e.code),
        Some(ErrorCode::InterruptedByUser)
    );
    let written = o.detail["written_bytes"].as_u64().unwrap_or(0);
    assert!(written > 0 && written < 4096, "written_bytes={written}");
    let mut evs = Vec::new();
    // The 0x03 is written right after the agent request is cancelled; it is not part of the
    // agent's reply, so it may be published just after it.
    drain_until(&mut fw, &mut evs, |evs| {
        evs.iter()
            .any(|e| e.kind == "uart.data" && e.origin == Origin::User && e.dir == Some(Dir::Tx))
    });
    let states = write_states(&evs, "q-big");
    assert_eq!(states.last().map(String::as_str), Some("outcome_unknown"));
    close(&mut fw);
}

// ---------------------------------------------------------------- 3. publish-before-reply

#[test]
fn rows_and_tx_bytes_are_published_before_the_reply() {
    let mut fw = new_fw();
    open(&mut fw);
    login(&mut fw);
    fw.drain_published();
    let o = send(&mut fw, "q-pbr", "echo hi", json!({}));
    assert!(o.error.is_none(), "{:?}", o.error);
    // Nothing more is drained between the reply and this point.
    let evs = fw.drain_published();
    let rows = rows_of(&evs, "q-pbr");
    let last = rows.last().expect("rows before reply");
    assert_eq!(last.payload["write_state"], "sent");
    assert_eq!(last.payload["final"], true);
    let tx_pos = evs
        .iter()
        .position(|e| {
            e.kind == "uart.data" && e.dir == Some(Dir::Tx) && e.request_id.as_deref() == Some("q-pbr")
        })
        .expect("tx bytes published before the reply");
    let final_pos = evs
        .iter()
        .position(|e| {
            e.kind == "uart.write" && e.request_id.as_deref() == Some("q-pbr") && e.payload["final"] == true
        })
        .expect("final row");
    assert!(tx_pos < final_pos, "tx bytes come before the terminal row");
    close(&mut fw);
}

// ---------------------------------------------------------------- 4. lifecycle

#[test]
fn close_joins_threads_and_reopen_bumps_generation() {
    let mut fw = new_fw();
    open(&mut fw);
    expect_lit(&mut fw, "login: ").expect("login prompt");
    let c = close(&mut fw);
    assert_eq!(c["threads_left"], 0);
    assert_eq!(c["refs_left"], 0, "a thread still holds the port");
    assert_eq!(fw.worker().live_threads(), 0);
    // Reads still work on the kept ring after close.
    fw.call("uart.read", json!({"channel": "dut", "mode": "peek"}))
        .expect("read after close");
    let again = open(&mut fw);
    assert_eq!(again["generation"], 2);
    let evs = fw.drain_published();
    assert!(
        evs.iter()
            .any(|e| e.kind == "uart.state" && e.payload["state"] == "closed")
    );
    close(&mut fw);
}

#[test]
fn open_is_idempotent_and_refuses_other_settings() {
    let mut fw = new_fw();
    let a = open(&mut fw);
    let b = open(&mut fw);
    assert_eq!(a, b);
    let o = fw.write(
        "open-2",
        json!({"method": "uart.open", "target": "sim://bootshell?split=0", "alias": "dut", "profile": "linux-shell",
               "config": {"baud": 921600}}),
    );
    assert_eq!(o.error.expect("config conflict").code, ErrorCode::Config);
    close(&mut fw);
}

#[test]
fn unsupported_line_settings_and_bad_targets_map_to_codes() {
    let mut fw = new_fw();
    let o = fw.write(
        "open-x",
        json!({"method": "uart.open", "target": "sim://bootshell", "config": {"stop_bits": "1.5"}}),
    );
    assert_eq!(o.error.expect("1.5 stop bits").code, ErrorCode::Unsupported);
    let o = fw.write("open-y", json!({"method": "uart.open", "target": "sim://nope"}));
    assert_eq!(o.error.expect("unknown scenario").code, ErrorCode::Usage);
    let o = fw.write(
        "open-z",
        json!({"method": "uart.open", "target": "/dev/loom-does-not-exist"}),
    );
    assert_eq!(o.error.expect("missing device").code, ErrorCode::NotFound);
}
