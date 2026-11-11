//! Shared test harness: transport + reader + bridge + TX actor, i.e. the worker's device path.
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use loom_uart_worker::bridge::{BridgeItem, BridgeRx, PopError, bridge};
use loom_uart_worker::clock::{SeqSource, SystemClock};
use loom_uart_worker::reader::Reader;
use loom_uart_worker::transport::Transport;
use loom_uart_worker::tx::{TxActor, TxDone, TxEvent, TxHandle, TxLane, TxRequest, event_channel};

/// Default wait for device output in tests (§ common rules: sleeps/waits ≤ 2 s each).
pub const WAIT: Duration = Duration::from_secs(2);

pub struct Session {
    pub reader: Reader,
    pub rx: BridgeRx,
    pub actor: TxActor,
    pub tx: TxHandle,
    pub events: Receiver<TxEvent>,
    pub buf: Vec<u8>,
    next_id: u64,
}

impl Session {
    pub fn new(t: Box<dyn Transport>) -> Self {
        let seq = Arc::new(SeqSource::default());
        let clock = Arc::new(SystemClock::new());
        let (btx, rx) = bridge();
        let rxh = t.rx_handle().expect("rx handle");
        let reader = Reader::spawn(rxh, btx, Arc::clone(&seq), clock.clone(), 0).expect("reader");
        let (etx, events) = event_channel();
        let actor = TxActor::spawn(t, seq, clock, etx).expect("tx actor");
        let tx = actor.handle();
        Self {
            reader,
            rx,
            actor,
            tx,
            events,
            buf: Vec::new(),
            next_id: 1,
        }
    }

    /// Sends on `lane` and waits for the request's `TxDone`.
    pub fn send_lane(&mut self, lane: TxLane, bytes: &[u8]) -> TxDone {
        let id = self.next_id;
        self.next_id += 1;
        self.tx
            .submit(TxRequest {
                request_id: id,
                lane,
                client_id: "test".into(),
                bytes: bytes.to_vec(),
                deadline: Instant::now() + WAIT,
            })
            .expect("submit");
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left).expect("tx event") {
                TxEvent::Done(d) if d.request_id == id => return d,
                _ => {}
            }
        }
    }

    /// Agent send.
    pub fn send(&mut self, bytes: &[u8]) -> TxDone {
        let d = self.send_lane(TxLane::Agent, bytes);
        assert_eq!(d.state, loom_uart_worker::tx::TxState::Sent, "send failed: {d:?}");
        d
    }

    /// Reads until `needle` appears; returns everything up to and including it and keeps the
    /// rest buffered.
    pub fn expect(&mut self, needle: &[u8]) -> Vec<u8> {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(pos) = self.buf.windows(needle.len()).position(|w| w == needle) {
                let rest = self.buf.split_off(pos + needle.len());
                return std::mem::replace(&mut self.buf, rest);
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                panic!(
                    "timed out waiting for {:?}; buffered tail {:?}",
                    String::from_utf8_lossy(needle),
                    String::from_utf8_lossy(&self.buf[self.buf.len().saturating_sub(300)..])
                );
            }
            match self.rx.pop(left.min(Duration::from_millis(50))) {
                Ok(BridgeItem::Data(c)) => self.buf.extend_from_slice(&c.bytes),
                Ok(other) => panic!("unexpected bridge item {other:?}"),
                Err(PopError::Timeout) => {}
                Err(PopError::Closed) => panic!("bridge closed"),
            }
        }
    }

    /// Reads exactly `n` bytes.
    pub fn read_exact(&mut self, n: usize) -> Vec<u8> {
        let end = Instant::now() + WAIT;
        while self.buf.len() < n {
            let left = end.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out: {} of {n} bytes", self.buf.len());
            match self.rx.pop(left.min(Duration::from_millis(50))) {
                Ok(BridgeItem::Data(c)) => self.buf.extend_from_slice(&c.bytes),
                Ok(other) => panic!("unexpected bridge item {other:?}"),
                Err(PopError::Timeout) => {}
                Err(PopError::Closed) => panic!("bridge closed"),
            }
        }
        let rest = self.buf.split_off(n);
        std::mem::replace(&mut self.buf, rest)
    }
}

/// Logs in on a `bootshell` device and checks `uname -a` and `ping` + Ctrl-C.
pub fn bootshell_script(s: &mut Session) {
    use loom_uart_worker::sim::{LOGIN_PROMPT, PASSWORD_PROMPT, SHELL_PROMPT};
    let boot = s.expect(LOGIN_PROMPT.as_bytes());
    let boot_text = String::from_utf8_lossy(&boot);
    assert!(
        boot_text.contains("(boot #1)") && boot_text.contains("Started"),
        "boot log expected before login"
    );
    s.send(b"root\r");
    s.expect(PASSWORD_PROMPT.as_bytes());
    s.send(b"secret\r");
    let after = s.expect(SHELL_PROMPT.as_bytes());
    assert!(
        !after.windows(6).any(|w| w == b"secret"),
        "password must not echo"
    );

    s.send(b"uname -a\r");
    let out = s.expect(SHELL_PROMPT.as_bytes());
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("uname -a\r\n"), "echo: {text:?}");
    assert!(text.contains("Linux sim 6.6.0-loom #1 SMP PREEMPT"), "{text:?}");

    s.send(b"ping 10.0.0.1\r");
    s.expect(b"icmp_seq=1 ");
    let d = s.send_lane(TxLane::CtrlC, b"\x03");
    assert_eq!(d.state, loom_uart_worker::tx::TxState::Sent);
    let out = s.expect(SHELL_PROMPT.as_bytes());
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("--- 10.0.0.1 ping statistics ---"), "{text:?}");
    assert!(text.contains("packets transmitted"), "{text:?}");
}
