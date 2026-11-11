//! pty harness (Unix): a fake device behind a real pty pair (UART plan §9.1).
//!
//! The master is put in raw + nonblocking mode and driven by a device thread with a `poll` loop
//! (a synchronous first write to the master can block before anyone reads the slave — §9.1).
//! The slave is set raw / echo off so the line discipline does not rewrite CR/LF, ^C or NUL. The
//! harness keeps one slave fd open for its lifetime so the master never sees a hang-up while the
//! worker reopens the slave path with `SerialTransport::open(.., sim_pty = true)`.

use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::PollFlags;
use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};

use super::{Device, Runner, SimSpec, Splitter};
use crate::transport::wait_fd;

/// Longest the device thread sleeps between stop-flag checks.
const PTY_LOOP_MAX_WAIT: Duration = Duration::from_millis(10);
/// Bytes read from the master per read call.
const PTY_READ_BUF: usize = 4096;
/// Most bytes read from the master per loop iteration before output gets a turn.
const PTY_READ_BUDGET: usize = 64 * 1024;

/// Slave paths of the pty devices this process is running right now (bounded, see
/// [`crate::worker::MAX_SIM_PTYS`]). Only [`PtyDevice::spawn`] adds to it and [`PtyHandle`]'s
/// drop removes from it, so a path here was issued by Loom's own sim harness in this process.
static LIVE_SIM_PTYS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Is `path` the slave of a pty device spawned by this process and still running? Lets an
/// in-process `UartWorker` open it as a sim pty (`OpenOpts::sim_pty`, DR-V18) without a
/// per-worker [`crate::worker::UartWorker::register_sim_pty`] call. Never true for a real port.
pub fn is_live_sim_pty(path: &str) -> bool {
    LIVE_SIM_PTYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|p| p == path)
}

/// Spawns fake devices on pty pairs.
#[derive(Debug)]
pub struct PtyDevice;

impl PtyDevice {
    /// Builds the scenario from `spec` and spawns it on a new pty pair.
    pub fn spawn_spec(spec: &SimSpec) -> io::Result<PtyHandle> {
        let (device, split) = spec.build();
        Self::spawn(device, split)
    }

    /// Spawns `device` on a new pty pair; the returned handle owns the device thread.
    pub fn spawn(device: Box<dyn Device>, split: Splitter) -> io::Result<PtyHandle> {
        let pty = nix::pty::openpty(None, None).map_err(io::Error::from)?;
        for fd in [&pty.slave, &pty.master] {
            let mut t = tcgetattr(fd).map_err(io::Error::from)?;
            cfmakeraw(&mut t);
            tcsetattr(fd, SetArg::TCSANOW, &t).map_err(io::Error::from)?;
        }
        let slave_path = nix::unistd::ttyname(&pty.slave).map_err(io::Error::from)?;
        let live = slave_path.to_string_lossy().into_owned();
        {
            let mut v = LIVE_SIM_PTYS.lock().unwrap_or_else(PoisonError::into_inner);
            if v.len() >= crate::worker::MAX_SIM_PTYS {
                return Err(io::Error::other("too many live sim ptys"));
            }
            v.push(live.clone());
        }
        let guard = LiveGuard(live);
        let flags =
            OFlag::from_bits_truncate(fcntl(&pty.master, FcntlArg::F_GETFL).map_err(io::Error::from)?);
        fcntl(&pty.master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).map_err(io::Error::from)?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let master = pty.master;
        let thread = std::thread::Builder::new()
            .name("loom-sim-pty".into())
            .spawn(move || device_loop(master, device, split, &stop_t))?;
        Ok(PtyHandle {
            slave_path,
            stop,
            thread: Some(thread),
            _live: guard,
            _slave: pty.slave,
        })
    }
}

/// A running pty fake device. Dropping it stops the device thread and closes the pty.
#[derive(Debug)]
pub struct PtyHandle {
    /// Path of the slave side, to be opened like a real port.
    pub slave_path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<io::Result<()>>>,
    _live: LiveGuard,
    _slave: OwnedFd,
}

/// Removes a slave path from [`LIVE_SIM_PTYS`] when its device goes away.
#[derive(Debug)]
struct LiveGuard(String);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut v = LIVE_SIM_PTYS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = v.iter().position(|p| *p == self.0) {
            v.swap_remove(i);
        }
    }
}

impl PtyHandle {
    /// Slave path as a string (lossy for non-UTF-8 paths).
    pub fn slave_path_str(&self) -> String {
        self.slave_path.to_string_lossy().into_owned()
    }

    /// Stops the device thread and returns its exit status.
    pub fn stop(mut self) -> io::Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| io::Error::other("pty device thread panicked"))?,
            None => Ok(()),
        }
    }
}

impl Drop for PtyHandle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn device_loop(
    master: OwnedFd,
    device: Box<dyn Device>,
    split: Splitter,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut runner = Runner::start(device, split, Instant::now());
    let mut buf = [0u8; PTY_READ_BUF];
    while !stop.load(Ordering::SeqCst) {
        let now = Instant::now();
        runner.tick_if_due(now);

        // Output: write pieces until the master would block.
        let mut want_out = false;
        while let Some(piece) = runner.piece(now) {
            match nix::unistd::write(master.as_fd(), piece) {
                Ok(n) => runner.consumed(n, now),
                Err(Errno::EAGAIN) | Err(Errno::EINTR) => {
                    want_out = true;
                    break;
                }
                Err(e) => return Err(io::Error::from(e)),
            }
            runner.tick_if_due(now);
        }

        // Wait for input, output space, or the next timed event.
        // With `want_out` the POLLOUT event wakes us; otherwise pending output is pausing.
        let mut wait = PTY_LOOP_MAX_WAIT;
        let now = Instant::now();
        if let Some(w) = runner.next_wakeup(!want_out, now) {
            wait = wait.min(w.saturating_duration_since(now));
        }
        let events = if want_out {
            PollFlags::POLLIN | PollFlags::POLLOUT
        } else {
            PollFlags::POLLIN
        };
        let ready = wait_fd(master.as_fd(), events, wait)?;
        if !ready.is_some_and(|r: PollFlags| r.intersects(PollFlags::POLLIN | PollFlags::POLLHUP)) {
            continue;
        }

        // Input: drain up to the budget.
        let mut taken = 0usize;
        while taken < PTY_READ_BUDGET {
            match nix::unistd::read(master.as_fd(), &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    taken += n;
                    runner.feed(&buf[..n], Instant::now());
                }
                Err(Errno::EAGAIN) | Err(Errno::EINTR) => break,
                // Linux reports EIO on the master while no slave is open; we hold one, but a
                // transient EIO must not kill the device.
                Err(Errno::EIO) => {
                    std::thread::sleep(Duration::from_millis(1));
                    break;
                }
                Err(e) => return Err(io::Error::from(e)),
            }
        }
    }
    Ok(())
}
