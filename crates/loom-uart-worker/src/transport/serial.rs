//! `SerialTransport` — real serial ports and pty slaves (UART plan §7.2, §7.3, §9.1).
//!
//! Unix design (no `unsafe`): `serialport` only exposes the `TTYPort` fd as a `RawFd`, and turning
//! that into a `BorrowedFd` needs `unsafe`. So the data path uses its **own** descriptor: the path
//! is opened first with `O_NONBLOCK` (an `OwnedFd` from `nix::fcntl::open`), then `serialport`
//! opens the same path, applying `TIOCEXCL` + exclusive `flock`, raw termios and line settings to
//! the device. All later opens are refused by `TIOCEXCL`/`flock`; our data fd predates them.
//! Data I/O never touches `serialport`'s blocking `write` (§7.3): it is `poll` + nonblocking
//! read/write + partial progress + absolute deadline. Modem lines and break go through
//! `serialport` and their errors are returned.

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use super::{PortConfig, RxHandle, Transport, TransportKind, WriteResult};

/// Options for [`SerialTransport::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOpts {
    /// Exclusive open (Unix: `TIOCEXCL` + exclusive `flock`; Windows is always exclusive).
    /// Default `true` (§7.2).
    pub exclusive: bool,
    /// DTR level to apply explicitly after open; `None` leaves the line untouched (§7.3).
    pub dtr: Option<bool>,
    /// RTS level to apply explicitly after open; `None` leaves the line untouched (§7.3).
    pub rts: Option<bool>,
    /// Provenance: the path is a pty slave issued by Loom's own sim harness. This preserves
    /// its transport label on other platforms; macOS baud handling uses descriptor identity,
    /// independently of this flag. Never set for external ptys or real devices.
    pub sim_pty: bool,
}

impl Default for OpenOpts {
    fn default() -> Self {
        Self {
            exclusive: true,
            dtr: None,
            rts: None,
            sim_pty: false,
        }
    }
}

fn to_sp_config(
    builder: serialport::SerialPortBuilder,
    config: &PortConfig,
) -> io::Result<serialport::SerialPortBuilder> {
    use super::{FlowControl, Parity, StopBits};
    let data_bits = match config.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        8 => serialport::DataBits::Eight,
        n => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("data_bits {n} not in 5..=8"),
            ));
        }
    };
    let parity = match config.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
        Parity::Mark | Parity::Space => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "mark/space parity is not supported yet (UART plan §7.2)",
            ));
        }
    };
    let stop = match config.stop {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
        StopBits::OnePointFive => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "1.5 stop bits are not supported (UART plan §7.2)",
            ));
        }
    };
    let flow = match config.flow {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    };
    Ok(builder
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop)
        .flow_control(flow))
}

fn rts_conflict(config: &PortConfig) -> io::Result<()> {
    if config.flow == super::FlowControl::Hardware {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manual RTS conflicts with hardware flow control (UART plan §7.2)",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
mod imp {
    use super::*;
    use nix::errno::Errno;
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use serialport::SerialPort;
    use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

    /// Identify the macOS PTY slave driver from kernel descriptor properties, not a supplied
    /// filename or an ENOTTY baud failure. A fresh openpty supplies the platform's slave device
    /// major; its master has a different major. Symlinks to ptys therefore work too. Neither
    /// this probe nor the external pty is registered as an internal simulator.
    #[cfg(target_os = "macos")]
    fn native_pty_slave(fd: BorrowedFd<'_>) -> io::Result<bool> {
        use nix::libc::major;
        use nix::sys::stat::{SFlag, fstat};

        let candidate = fstat(fd)?;
        if SFlag::from_bits_truncate(candidate.st_mode & SFlag::S_IFMT.bits()) != SFlag::S_IFCHR {
            return Ok(false);
        }
        // If a reference cannot be allocated, retain ordinary serial baud/error handling.
        let Ok(reference) = nix::pty::openpty(None, None) else {
            return Ok(false);
        };
        let slave = fstat(&reference.slave)?;
        let master = fstat(&reference.master)?;
        let slave_major = major(slave.st_rdev);
        Ok(slave_major != major(master.st_rdev) && major(candidate.st_rdev) == slave_major)
    }

    #[cfg(not(target_os = "macos"))]
    fn native_pty_slave(_fd: BorrowedFd<'_>) -> io::Result<bool> {
        Ok(false)
    }

    #[cfg(all(test, target_os = "macos"))]
    #[test]
    fn pty_identity_excludes_master_and_other_descriptors() {
        let pair = nix::pty::openpty(None, None).expect("reference pair");
        assert!(native_pty_slave(pair.slave.as_fd()).expect("slave identity"));
        assert!(!native_pty_slave(pair.master.as_fd()).expect("master identity"));
        let null = std::fs::File::open("/dev/null").expect("non-PTY character device");
        assert!(!native_pty_slave(null.as_fd()).expect("other character driver"));
        let file =
            std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).expect("ordinary file");
        assert!(!native_pty_slave(file.as_fd()).expect("regular file"));
    }

    /// Real serial port or sim pty slave (Unix).
    pub struct SerialTransport {
        /// Owner of device settings, modem lines, break and the exclusive lock.
        port: serialport::TTYPort,
        /// Nonblocking data descriptor (opened before `port`, see module docs).
        data: OwnedFd,
        config: PortConfig,
        kind: TransportKind,
        path: String,
    }

    impl fmt::Debug for SerialTransport {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SerialTransport")
                .field("path", &self.path)
                .field("kind", &self.kind)
                .field("config", &self.config)
                .field("exclusive", &self.port.exclusive())
                .finish()
        }
    }

    impl SerialTransport {
        /// Opens `path`. **Worker binary only** (contract 08 §8.3): no CLI/UI path opens
        /// devices directly.
        ///
        /// Order: validate config → open nonblocking data fd → `serialport` open (exclusive lock,
        /// raw termios, line settings; DTR untouched) → explicit DTR/RTS (errors returned) →
        /// nonblocking invariant check.
        pub fn open(path: &str, config: &PortConfig, opts: &OpenOpts) -> io::Result<Self> {
            config.validate()?;
            if opts.rts.is_some() {
                rts_conflict(config)?;
            }
            let data = nix::fcntl::open(
                path,
                OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
                nix::sys::stat::Mode::empty(),
            )
            .map_err(|e| with_context(io::Error::from(e), path))?;
            if !nix::unistd::isatty(&data).unwrap_or(false) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{path} is not a tty"),
                ));
            }
            // macOS ptys reject IOSSIOSPEED. Skip it only after proving this descriptor is a
            // PTY slave; real serial devices still receive their requested baud and all errors.
            let native_pty = native_pty_slave(data.as_fd())?;
            let sp_baud = if native_pty { 0 } else { config.baud };
            let builder = serialport::new(path, sp_baud)
                .timeout(Duration::from_millis(20))
                .exclusive(opts.exclusive);
            let mut port = to_sp_config(builder, config)?.open_native().map_err(|e| {
                // Our own open above proved the path exists, so serialport's `NoDevice`
                // here means the exclusive lock (flock / TIOCEXCL) is held elsewhere.
                let busy = e.kind() == serialport::ErrorKind::NoDevice;
                let io = with_context(io::Error::from(e), path);
                if busy {
                    io::Error::new(io::ErrorKind::ResourceBusy, io.to_string())
                } else {
                    io
                }
            })?;
            if opts.exclusive && !port.exclusive() {
                return Err(io::Error::other(format!(
                    "{path}: exclusive access was not obtained"
                )));
            }
            if let Some(level) = opts.dtr {
                port.write_data_terminal_ready(level)
                    .map_err(|e| with_context(io::Error::from(e), &format!("{path}: set DTR")))?;
            }
            if let Some(level) = opts.rts {
                port.write_request_to_send(level)
                    .map_err(|e| with_context(io::Error::from(e), &format!("{path}: set RTS")))?;
            }
            ensure_nonblocking(data.as_fd())?;
            Ok(Self {
                port,
                data,
                config: *config,
                kind: if opts.sim_pty || native_pty {
                    TransportKind::Pty
                } else {
                    TransportKind::Serial
                },
                path: path.to_owned(),
            })
        }

        /// Device path this transport was opened with.
        pub fn path(&self) -> &str {
            &self.path
        }
    }

    impl Transport for SerialTransport {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            read_nonblocking(self.data.as_fd(), buf, wait)
        }

        fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
            write_nonblocking(self.data.as_fd(), buf, deadline)
        }

        fn set_dtr(&mut self, on: bool) -> io::Result<()> {
            self.port.write_data_terminal_ready(on).map_err(io::Error::from)
        }

        fn set_rts(&mut self, on: bool) -> io::Result<()> {
            rts_conflict(&self.config)?;
            self.port.write_request_to_send(on).map_err(io::Error::from)
        }

        fn set_break(&mut self, on: bool) -> io::Result<()> {
            let r = if on {
                self.port.set_break()
            } else {
                self.port.clear_break()
            };
            r.map_err(io::Error::from)
        }

        fn config(&self) -> &PortConfig {
            &self.config
        }

        fn kind(&self) -> TransportKind {
            self.kind
        }

        fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
            // dup shares the open file description, so O_NONBLOCK carries over.
            let fd = nix::unistd::dup(&self.data).map_err(io::Error::from)?;
            ensure_nonblocking(fd.as_fd())?;
            Ok(Box::new(FdRx { fd }))
        }
    }

    /// Reader-thread handle: a dup of the nonblocking data fd.
    #[derive(Debug)]
    pub struct FdRx {
        fd: OwnedFd,
    }

    impl RxHandle for FdRx {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            read_nonblocking(self.fd.as_fd(), buf, wait)
        }
    }

    fn with_context(e: io::Error, what: &str) -> io::Error {
        io::Error::new(e.kind(), format!("{what}: {e}"))
    }

    fn ensure_nonblocking(fd: BorrowedFd<'_>) -> io::Result<()> {
        let flags = OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?);
        if !flags.contains(OFlag::O_NONBLOCK) {
            fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
        }
        Ok(())
    }

    /// Poll timeout rounded **up** to whole milliseconds so sub-ms remainders do not spin.
    pub(crate) fn poll_timeout(remaining: Duration) -> PollTimeout {
        let ms = remaining.as_micros().div_ceil(1000);
        PollTimeout::from(u16::try_from(ms).unwrap_or(u16::MAX))
    }

    /// `poll` for `events`; `Ok(None)` on timeout or EINTR, `Ok(Some(revents))` otherwise.
    pub(crate) fn wait_fd(
        fd: BorrowedFd<'_>,
        events: PollFlags,
        remaining: Duration,
    ) -> io::Result<Option<PollFlags>> {
        let mut fds = [PollFd::new(fd, events)];
        match poll(&mut fds, poll_timeout(remaining)) {
            Ok(0) | Err(Errno::EINTR) => Ok(None),
            Ok(_) => {
                let rev = fds[0].revents().unwrap_or(PollFlags::empty());
                if rev.contains(PollFlags::POLLNVAL) {
                    return Err(io::Error::from(Errno::EBADF));
                }
                Ok(Some(rev))
            }
            Err(e) => Err(io::Error::from(e)),
        }
    }

    /// Nonblocking read with a bounded wait. `Ok(0)` = nothing within `wait`; EOF is an error.
    pub(crate) fn read_nonblocking(fd: BorrowedFd<'_>, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let deadline = Instant::now() + wait;
        loop {
            match nix::unistd::read(fd, buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "device closed (EOF)",
                    ));
                }
                Ok(n) => return Ok(n),
                Err(Errno::EAGAIN) | Err(Errno::EINTR) => {}
                Err(e) => return Err(io::Error::from(e)),
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(0);
            }
            // POLLHUP/POLLERR fall through to read(), which reports EOF or the error.
            if wait_fd(fd, PollFlags::POLLIN, remaining)?.is_none() && Instant::now() >= deadline {
                return Ok(0);
            }
        }
    }

    /// Nonblocking write with partial progress and an absolute deadline (§7.3).
    pub(crate) fn write_nonblocking(fd: BorrowedFd<'_>, buf: &[u8], deadline: Instant) -> WriteResult {
        use super::super::WriteOutcome;
        let mut written = 0usize;
        while written < buf.len() {
            match nix::unistd::write(fd, &buf[written..]) {
                Ok(n) if n > 0 => {
                    written += n;
                    continue;
                }
                Ok(_) | Err(Errno::EAGAIN) | Err(Errno::EINTR) => {}
                Err(e) => {
                    return WriteResult {
                        written,
                        outcome: WriteOutcome::Failed(io::Error::from(e).kind()),
                    };
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return WriteResult {
                    written,
                    outcome: WriteOutcome::TimedOut,
                };
            }
            if let Err(e) = wait_fd(fd, PollFlags::POLLOUT, remaining) {
                return WriteResult {
                    written,
                    outcome: WriteOutcome::Failed(e.kind()),
                };
            }
        }
        WriteResult {
            written,
            outcome: WriteOutcome::Complete,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Windows (compile-only in M1; run tests are M2)
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use super::*;
    use serialport::SerialPort;

    /// Smallest COMMTIMEOUTS slice used for reads/writes.
    const MIN_TIMEOUT: Duration = Duration::from_millis(1);

    /// Real serial port (Windows). Uses finite COMMTIMEOUTS via `set_timeout` (§7.3).
    pub struct SerialTransport {
        port: Box<dyn SerialPort>,
        config: PortConfig,
        path: String,
    }

    impl fmt::Debug for SerialTransport {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("SerialTransport")
                .field("path", &self.path)
                .field("config", &self.config)
                .finish()
        }
    }

    impl SerialTransport {
        /// Opens `path`. **Worker binary only** (contract 08 §8.3). Windows opens are always
        /// exclusive (share mode 0); `exclusive=false` is rejected rather than ignored.
        pub fn open(path: &str, config: &PortConfig, opts: &OpenOpts) -> io::Result<Self> {
            config.validate()?;
            if !opts.exclusive {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "non-exclusive open is not available on Windows",
                ));
            }
            if opts.rts.is_some() {
                rts_conflict(config)?;
            }
            let builder = serialport::new(path, config.baud).timeout(Duration::from_millis(20));
            let mut port = to_sp_config(builder, config)?.open().map_err(io::Error::from)?;
            if let Some(level) = opts.dtr {
                port.write_data_terminal_ready(level).map_err(io::Error::from)?;
            }
            if let Some(level) = opts.rts {
                port.write_request_to_send(level).map_err(io::Error::from)?;
            }
            Ok(Self {
                port,
                config: *config,
                path: path.to_owned(),
            })
        }

        /// Device path this transport was opened with.
        pub fn path(&self) -> &str {
            &self.path
        }
    }

    fn timed_read(port: &mut dyn SerialPort, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        port.set_timeout(wait.max(MIN_TIMEOUT)).map_err(io::Error::from)?;
        match port.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }

    impl Transport for SerialTransport {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            timed_read(self.port.as_mut(), buf, wait)
        }

        fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult {
            use super::super::WriteOutcome;
            let mut written = 0usize;
            while written < buf.len() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return WriteResult {
                        written,
                        outcome: WriteOutcome::TimedOut,
                    };
                }
                if let Err(e) = self.port.set_timeout(remaining.max(MIN_TIMEOUT)) {
                    return WriteResult {
                        written,
                        outcome: WriteOutcome::Failed(io::Error::from(e).kind()),
                    };
                }
                match self.port.write(&buf[written..]) {
                    Ok(n) => written += n,
                    Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        return WriteResult {
                            written,
                            outcome: WriteOutcome::Failed(e.kind()),
                        };
                    }
                }
            }
            WriteResult {
                written,
                outcome: WriteOutcome::Complete,
            }
        }

        fn set_dtr(&mut self, on: bool) -> io::Result<()> {
            self.port.write_data_terminal_ready(on).map_err(io::Error::from)
        }

        fn set_rts(&mut self, on: bool) -> io::Result<()> {
            rts_conflict(&self.config)?;
            self.port.write_request_to_send(on).map_err(io::Error::from)
        }

        fn set_break(&mut self, on: bool) -> io::Result<()> {
            let r = if on {
                self.port.set_break()
            } else {
                self.port.clear_break()
            };
            r.map_err(io::Error::from)
        }

        fn config(&self) -> &PortConfig {
            &self.config
        }

        fn kind(&self) -> TransportKind {
            TransportKind::Serial
        }

        fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>> {
            // NOTE(M2): COMMTIMEOUTS belong to the device, so the cloned handle's read timeout
            // and the TX handle's write timeout interfere; the Windows gate must settle this.
            let port = self.port.try_clone().map_err(io::Error::from)?;
            Ok(Box::new(WinRx { port }))
        }
    }

    /// Reader-thread handle (cloned COM handle).
    pub struct WinRx {
        port: Box<dyn SerialPort>,
    }

    impl fmt::Debug for WinRx {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("WinRx").finish()
        }
    }

    impl RxHandle for WinRx {
        fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            timed_read(self.port.as_mut(), buf, wait)
        }
    }
}

pub use imp::SerialTransport;

#[cfg(unix)]
pub(crate) use imp::wait_fd;
