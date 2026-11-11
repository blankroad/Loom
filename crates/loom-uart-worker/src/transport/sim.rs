//! `SimTransport` — in-process fake device behind fixed-size buffers (UART plan §9.1).
//!
//! Identical on every OS (the Windows CI path). A device thread runs a [`Runner`]; host→device and
//! device→host bytes go through two bounded byte queues, DTR/RTS/break go to the device as
//! [`Signal`]s through a bounded signal queue.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{PortConfig, RxHandle, Transport, TransportKind, WriteOutcome, WriteResult};
use crate::sim::{Device, Runner, Signal, SimSpec, Splitter};

/// Capacity of each direction's byte buffer (task spec: 1 MiB each; §3.13 bounded buffers).
/// A full device→host buffer pauses the device (its pending output stays in the runner); a full
/// host→device buffer makes `write` wait until its deadline (partial write).
pub const SIM_BUFFER_BYTES: usize = 1024 * 1024;
/// Capacity of the pending signal queue; `set_dtr`/`set_rts`/`set_break` fail with
/// `WouldBlock` when it is full (§3.13).
pub const SIM_SIGNAL_QUEUE: usize = 64;
/// Longest the device thread sleeps without re-checking state.
const SIM_LOOP_MAX_WAIT: Duration = Duration::from_millis(10);
/// Most host bytes fed to the device per loop iteration.
const SIM_FEED_CHUNK: usize = 4096;

#[derive(Debug, Default)]
struct State {
    to_host: VecDeque<u8>,
    to_dev: VecDeque<u8>,
    signals: VecDeque<Signal>,
    closed: bool,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    /// Host side waits here (data available / space freed).
    host_cv: Condvar,
    /// Device thread waits here (input, signals, space freed, close).
    dev_cv: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A poisoned lock only means a panicking peer; the byte queues stay consistent.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn read(&self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let deadline = Instant::now() + wait;
        let mut st = self.lock();
        loop {
            if !st.to_host.is_empty() {
                let n = buf.len().min(st.to_host.len());
                for (dst, src) in buf.iter_mut().zip(st.to_host.drain(..n)) {
                    *dst = src;
                }
                drop(st);
                self.dev_cv.notify_one();
                return Ok(n);
            }
            if st.closed {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "sim device closed"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(0);
            }
            st = self
                .host_cv
                .wait_timeout(st, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

/// In-process transport to a fake device (`sim://<scenario>`).
pub struct SimTransport {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    config: PortConfig,
    spec: Option<SimSpec>,
}

impl fmt::Debug for SimTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimTransport")
            .field("spec", &self.spec)
            .field("config", &self.config)
            .finish()
    }
}

impl SimTransport {
    /// Opens the scenario described by `spec`. **Worker binary only** (contract 08 §8.3).
    pub fn open(spec: &SimSpec, config: &PortConfig) -> io::Result<Self> {
        let (device, split) = spec.build();
        let mut t = Self::with_device(device, split, config)?;
        t.spec = Some(spec.clone());
        Ok(t)
    }

    /// Opens a transport to an arbitrary device (tests, custom scenarios).
    pub fn with_device(device: Box<dyn Device>, split: Splitter, config: &PortConfig) -> io::Result<Self> {
        config.validate()?;
        let shared = Arc::new(Shared::default());
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("loom-sim-dev".into())
            .spawn(move || device_loop(&s, device, split))?;
        Ok(Self {
            shared,
            thread: Some(thread),
            config: *config,
            spec: None,
        })
    }

    fn signal(&mut self, s: Signal) -> io::Result<()> {
        let mut st = self.shared.lock();
        if st.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "sim device closed"));
        }
        if st.signals.len() >= SIM_SIGNAL_QUEUE {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "sim signal queue full"));
        }
        st.signals.push_back(s);
        drop(st);
        self.shared.dev_cv.notify_one();
        Ok(())
    }
}

impl Drop for SimTransport {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.dev_cv.notify_all();
        self.shared.host_cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Transport for SimTransport {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        self.shared.read(buf, wait)
    }

    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
        let mut written = 0usize;
        let mut st = self.shared.lock();
        loop {
            if st.closed {
                return WriteResult {
                    written,
                    outcome: WriteOutcome::Failed(io::ErrorKind::BrokenPipe),
                };
            }
            let room = SIM_BUFFER_BYTES.saturating_sub(st.to_dev.len());
            let n = room.min(buf.len() - written);
            st.to_dev.extend(&buf[written..written + n]);
            written += n;
            if n > 0 {
                self.shared.dev_cv.notify_one();
            }
            if written == buf.len() {
                return WriteResult {
                    written,
                    outcome: WriteOutcome::Complete,
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return WriteResult {
                    written,
                    outcome: WriteOutcome::TimedOut,
                };
            }
            st = self
                .shared
                .host_cv
                .wait_timeout(st, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }

    fn set_dtr(&mut self, on: bool) -> io::Result<()> {
        self.signal(Signal::Dtr(on))
    }

    fn set_rts(&mut self, on: bool) -> io::Result<()> {
        self.signal(Signal::Rts(on))
    }

    fn set_break(&mut self, on: bool) -> io::Result<()> {
        self.signal(Signal::Break(on))
    }

    fn config(&self) -> &PortConfig {
        &self.config
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }

    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
        Ok(Box::new(SimRx {
            shared: Arc::clone(&self.shared),
        }))
    }
}

/// Reader-thread handle for [`SimTransport`].
#[derive(Debug)]
struct SimRx {
    shared: Arc<Shared>,
}

impl RxHandle for SimRx {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        self.shared.read(buf, wait)
    }
}

fn device_loop(shared: &Shared, device: Box<dyn Device>, split: Splitter) {
    let mut runner = Runner::start(device, split, Instant::now());
    let mut input = Vec::with_capacity(SIM_FEED_CHUNK);
    let mut signals = Vec::new();
    loop {
        // Collect input and signals.
        {
            let mut st = shared.lock();
            if st.closed {
                return;
            }
            let n = st.to_dev.len().min(SIM_FEED_CHUNK);
            input.clear();
            input.extend(st.to_dev.drain(..n));
            signals.clear();
            signals.extend(st.signals.drain(..));
        }
        if !input.is_empty() {
            shared.host_cv.notify_all(); // space freed for writers
        }
        let now = Instant::now();
        for s in signals.drain(..) {
            runner.signal(s, now);
        }
        if !input.is_empty() {
            runner.feed(&input, now);
        }
        runner.tick_if_due(now);

        // Deliver output pieces while there is room.
        let mut delivered = false;
        {
            let mut st = shared.lock();
            loop {
                let room = SIM_BUFFER_BYTES.saturating_sub(st.to_host.len());
                if room == 0 {
                    break;
                }
                let Some(piece) = runner.piece(now) else {
                    break;
                };
                let n = piece.len().min(room);
                st.to_host.extend(&piece[..n]);
                runner.consumed(n, now);
                delivered = true;
                runner.tick_if_due(now);
            }
        }
        if delivered {
            shared.host_cv.notify_all();
        }

        // Sleep until input/signal/space/close or the runner's next timed event. A host read
        // notifies `dev_cv`, so a full device→host buffer needs no timed wakeup.
        let st = shared.lock();
        if st.closed || !st.to_dev.is_empty() || !st.signals.is_empty() {
            continue;
        }
        let now = Instant::now();
        let has_room = st.to_host.len() < SIM_BUFFER_BYTES;
        let wait = runner.next_wakeup(has_room, now).map_or(SIM_LOOP_MAX_WAIT, |w| {
            w.saturating_duration_since(now).min(SIM_LOOP_MAX_WAIT)
        });
        if wait.is_zero() {
            continue;
        }
        let _ = shared
            .dev_cv
            .wait_timeout(st, wait)
            .unwrap_or_else(|p| p.into_inner());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Scenario;
    use std::sync::mpsc;

    fn read_until(t: &mut SimTransport, needle: &[u8], limit: Duration) -> Vec<u8> {
        let end = Instant::now() + limit;
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        while Instant::now() < end {
            let n = t.read(&mut buf, Duration::from_millis(20)).expect("sim read");
            got.extend_from_slice(&buf[..n]);
            if got.windows(needle.len()).any(|w| w == needle) {
                return got;
            }
        }
        panic!(
            "timed out waiting for {:?}; got tail {:?}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&got[got.len().saturating_sub(200)..])
        );
    }

    #[test]
    fn signals_reach_the_device() {
        struct Rec(mpsc::Sender<Signal>);
        impl Device for Rec {
            fn on_open(&mut self, _: &mut Vec<u8>) {}
            fn on_bytes(&mut self, _: &[u8], _: Duration, _: &mut Vec<u8>) {}
            fn tick(&mut self, _: Duration, _: &mut Vec<u8>) -> Option<Duration> {
                None
            }
            fn on_signal(&mut self, s: Signal, _: &mut Vec<u8>) {
                let _ = self.0.send(s);
            }
        }
        let (tx, rx) = mpsc::channel();
        let mut t = SimTransport::with_device(Box::new(Rec(tx)), Splitter::whole(), &PortConfig::default())
            .expect("open");
        t.set_dtr(false).expect("dtr");
        t.set_rts(true).expect("rts");
        t.set_break(true).expect("break on");
        t.set_break(false).expect("break off");
        let got: Vec<Signal> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(2)).expect("signal"))
            .collect();
        assert_eq!(
            got,
            [
                Signal::Dtr(false),
                Signal::Rts(true),
                Signal::Break(true),
                Signal::Break(false)
            ]
        );
    }

    #[test]
    fn bootshell_break_prints_marker() {
        let spec = SimSpec::new(Scenario::Bootshell, 1);
        let mut t = SimTransport::open(&spec, &PortConfig::default()).expect("open");
        read_until(&mut t, b"login: ", Duration::from_secs(2));
        t.set_break(true).expect("break");
        read_until(&mut t, b"<BREAK>", Duration::from_secs(2));
    }

    #[test]
    fn rejects_unsupported_config() {
        let cfg = PortConfig {
            parity: crate::transport::Parity::Mark,
            ..PortConfig::default()
        };
        let e =
            SimTransport::open(&SimSpec::new(Scenario::Binary, 0), &cfg).expect_err("mark parity rejected");
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    }
}
