//! `bootshell` — boot log → `login: ` → `Password: ` (no echo) → `root@sim:~# ` (UART plan §9.2).
//!
//! Commands: `uname -a`, `cat /proc/meminfo`, `ping <host>` (1 Hz, ^C prints statistics),
//! `status`, `echo ...`, `reboot` (boot log again); anything else prints `sh: <cmd>: not found`.
//! Line editing: echo, BS (0x7F / 0x08), ^U kills the line, ^C prints `^C` and a fresh prompt.

use std::time::Duration;

use super::{Device, Rng, Signal};

/// Number of boot log lines emitted before the login prompt.
pub const BOOT_LOG_LINES: usize = 200;
/// Login prompt (no trailing newline).
pub const LOGIN_PROMPT: &str = "login: ";
/// Password prompt (no trailing newline, input is not echoed).
pub const PASSWORD_PROMPT: &str = "Password: ";
/// Shell prompt (no trailing newline).
pub const SHELL_PROMPT: &str = "root@sim:~# ";

/// Longest input line the fake shell keeps; extra bytes ring the bell (§3.13 bounded buffers).
const LINE_MAX: usize = 1024;
/// Boot lines emitted per tick.
const BOOT_LINES_PER_TICK: usize = 10;
/// Delay between boot ticks.
const BOOT_TICK: Duration = Duration::from_millis(2);
/// Ping interval.
const PING_INTERVAL: Duration = Duration::from_secs(1);
/// Ping samples kept for statistics (count is exact; min/avg/max use these).
const PING_SAMPLES_MAX: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Booting,
    Login,
    Password,
    Shell,
    Ping,
}

#[derive(Debug)]
struct Ping {
    host: String,
    sent: u64,
    next_due: Duration,
    samples_us: Vec<u32>,
}

/// The `bootshell` fake device.
#[derive(Debug)]
pub struct Bootshell {
    rng: Rng,
    mode: Mode,
    boot_log: Vec<Vec<u8>>,
    boot_next: usize,
    line: Vec<u8>,
    prev_cr: bool,
    ping: Option<Ping>,
    boots: u32,
}

impl Bootshell {
    /// Creates the device; `seed` varies boot timestamps and ping times.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            mode: Mode::Booting,
            boot_log: Vec::new(),
            boot_next: 0,
            line: Vec::new(),
            prev_cr: false,
            ping: None,
            boots: 0,
        }
    }

    fn begin_boot(&mut self) {
        self.boots += 1;
        self.boot_log = boot_log(&mut self.rng, self.boots);
        self.boot_next = 0;
        self.mode = Mode::Booting;
        self.line.clear();
        self.ping = None;
    }

    fn prompt(&self, out: &mut Vec<u8>) {
        let p = match self.mode {
            Mode::Login => LOGIN_PROMPT,
            Mode::Password => PASSWORD_PROMPT,
            Mode::Shell => SHELL_PROMPT,
            Mode::Booting | Mode::Ping => "",
        };
        out.extend_from_slice(p.as_bytes());
    }

    fn echo_on(&self) -> bool {
        matches!(self.mode, Mode::Login | Mode::Shell)
    }

    fn submit(&mut self, now: Duration, out: &mut Vec<u8>) {
        let line = String::from_utf8_lossy(&self.line).into_owned();
        self.line.clear();
        out.extend_from_slice(b"\r\n");
        match self.mode {
            Mode::Login => {
                if line.trim() == "root" {
                    self.mode = Mode::Password;
                } else {
                    out.extend_from_slice(b"\r\nLogin incorrect\r\n");
                }
                self.prompt(out);
            }
            Mode::Password => {
                // Any password is accepted.
                out.extend_from_slice(b"Last login: Thu Jan  1 00:00:00 on ttyS0\r\n");
                self.mode = Mode::Shell;
                self.prompt(out);
            }
            Mode::Shell => self.run(&line, now, out),
            Mode::Booting | Mode::Ping => {}
        }
    }

    fn run(&mut self, line: &str, now: Duration, out: &mut Vec<u8>) {
        let mut words = line.split_whitespace();
        let Some(cmd) = words.next() else {
            self.prompt(out);
            return;
        };
        let args: Vec<&str> = words.collect();
        match cmd {
            "uname" => {
                if args.first() == Some(&"-a") {
                    out.extend_from_slice(
                        b"Linux sim 6.6.0-loom #1 SMP PREEMPT Thu Jan  1 00:00:00 UTC 1970 aarch64 GNU/Linux\r\n",
                    );
                } else {
                    out.extend_from_slice(b"Linux\r\n");
                }
            }
            "cat" => match args.first() {
                Some(&"/proc/meminfo") => out.extend_from_slice(MEMINFO.as_bytes()),
                Some(p) => {
                    out.extend_from_slice(format!("cat: {p}: No such file or directory\r\n").as_bytes())
                }
                None => {}
            },
            "status" => out.extend_from_slice(b"temp=41.5 rpm=1200\r\n"),
            "echo" => {
                out.extend_from_slice(args.join(" ").as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            "ping" => match args.last() {
                Some(host) => {
                    out.extend_from_slice(
                        format!("PING {host} ({host}) 56(84) bytes of data.\r\n").as_bytes(),
                    );
                    self.ping = Some(Ping {
                        host: (*host).to_owned(),
                        sent: 0,
                        next_due: now,
                        samples_us: Vec::new(),
                    });
                    self.mode = Mode::Ping;
                    return;
                }
                None => out.extend_from_slice(b"ping: usage error: Destination address required\r\n"),
            },
            "reboot" => {
                out.extend_from_slice(b"reboot: Restarting system\r\n");
                self.begin_boot();
                return;
            }
            other => out.extend_from_slice(format!("sh: {other}: not found\r\n").as_bytes()),
        }
        self.prompt(out);
    }

    fn ping_tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration> {
        let ping = self.ping.as_mut()?;
        while ping.next_due <= now {
            ping.sent += 1;
            let us = 300 + u32::try_from(self.rng.below(700)).unwrap_or(0);
            if ping.samples_us.len() < PING_SAMPLES_MAX {
                ping.samples_us.push(us);
            }
            out.extend_from_slice(
                format!(
                    "64 bytes from {}: icmp_seq={} ttl=64 time={}.{:03} ms\r\n",
                    ping.host,
                    ping.sent,
                    us / 1000,
                    us % 1000
                )
                .as_bytes(),
            );
            ping.next_due += PING_INTERVAL;
        }
        Some(ping.next_due.saturating_sub(now))
    }

    fn ping_stop(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"^C\r\n");
        if let Some(p) = self.ping.take() {
            let n = p.sent;
            out.extend_from_slice(format!("\r\n--- {} ping statistics ---\r\n", p.host).as_bytes());
            out.extend_from_slice(
                format!(
                    "{n} packets transmitted, {n} received, 0% packet loss, time {}ms\r\n",
                    n.saturating_sub(1) * 1000
                )
                .as_bytes(),
            );
            if !p.samples_us.is_empty() {
                let min = p.samples_us.iter().copied().min().unwrap_or(0);
                let max = p.samples_us.iter().copied().max().unwrap_or(0);
                let sum: u64 = p.samples_us.iter().map(|&v| u64::from(v)).sum();
                let avg = sum / p.samples_us.len() as u64;
                let f = |us: u64| format!("{}.{:03}", us / 1000, us % 1000);
                out.extend_from_slice(
                    format!(
                        "rtt min/avg/max/mdev = {}/{}/{}/{} ms\r\n",
                        f(u64::from(min)),
                        f(avg),
                        f(u64::from(max)),
                        f(u64::from(max - min) / 2)
                    )
                    .as_bytes(),
                );
            }
        }
        self.mode = Mode::Shell;
        self.prompt(out);
    }

    fn on_byte(&mut self, c: u8, now: Duration, out: &mut Vec<u8>) {
        let after_cr = std::mem::replace(&mut self.prev_cr, c == b'\r');
        match self.mode {
            Mode::Booting => return,
            Mode::Ping => {
                if c == 0x03 {
                    self.ping_stop(out);
                }
                return;
            }
            Mode::Login | Mode::Password | Mode::Shell => {}
        }
        match c {
            b'\r' => self.submit(now, out),
            b'\n' => {
                if !after_cr {
                    self.submit(now, out);
                }
            }
            0x03 => {
                self.line.clear();
                out.extend_from_slice(b"^C\r\n");
                self.prompt(out);
            }
            0x7F | 0x08 => {
                if let Some(n) = last_char_len(&self.line) {
                    self.line.truncate(self.line.len() - n);
                    if self.echo_on() {
                        out.extend_from_slice(b"\x08 \x08");
                    }
                }
            }
            0x15 => {
                let mut chars = 0;
                while let Some(n) = last_char_len(&self.line) {
                    self.line.truncate(self.line.len() - n);
                    chars += 1;
                }
                if self.echo_on() {
                    for _ in 0..chars {
                        out.extend_from_slice(b"\x08 \x08");
                    }
                }
            }
            b'\t' | 0x20.. => {
                if self.line.len() >= LINE_MAX {
                    out.push(0x07);
                } else {
                    self.line.push(c);
                    if self.echo_on() {
                        out.push(c);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Byte length of the last UTF-8 character of `line` (`None` if empty).
fn last_char_len(line: &[u8]) -> Option<usize> {
    if line.is_empty() {
        return None;
    }
    let mut n = 1;
    while n < line.len() && n < 4 && (line[line.len() - n] & 0xC0) == 0x80 {
        n += 1;
    }
    Some(n)
}

impl Device for Bootshell {
    fn on_open(&mut self, out: &mut Vec<u8>) {
        self.begin_boot();
        out.extend_from_slice(b"\r\nU-Boot SPL 2024.01-loom-sim\r\n");
    }

    fn on_bytes(&mut self, b: &[u8], now: Duration, out: &mut Vec<u8>) {
        for &c in b {
            self.on_byte(c, now, out);
        }
    }

    fn tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration> {
        match self.mode {
            Mode::Booting => {
                let end = (self.boot_next + BOOT_LINES_PER_TICK).min(self.boot_log.len());
                for l in &self.boot_log[self.boot_next..end] {
                    out.extend_from_slice(l);
                }
                self.boot_next = end;
                if self.boot_next >= self.boot_log.len() {
                    self.boot_log = Vec::new();
                    self.mode = Mode::Login;
                    out.extend_from_slice(b"\r\nLoom sim 1.0 sim ttyS0\r\n\r\n");
                    self.prompt(out);
                    None
                } else {
                    Some(BOOT_TICK)
                }
            }
            Mode::Ping => self.ping_tick(now, out),
            Mode::Login | Mode::Password | Mode::Shell => None,
        }
    }

    fn on_signal(&mut self, s: Signal, out: &mut Vec<u8>) {
        if s == Signal::Break(true) {
            out.extend_from_slice(b"\r\n<BREAK>\r\n");
        }
    }
}

const MEMINFO: &str = "MemTotal:        1015808 kB\r\n\
MemFree:          612344 kB\r\n\
MemAvailable:     803112 kB\r\n\
Buffers:           20480 kB\r\n\
Cached:           190464 kB\r\n\
SwapCached:            0 kB\r\n\
Active:           150212 kB\r\n\
Inactive:         120096 kB\r\n\
SwapTotal:             0 kB\r\n\
SwapFree:              0 kB\r\n";

const KERNEL_MSGS: &[&str] = &[
    "Booting Linux on physical CPU 0x0000000000 [0x410fd034]",
    "Machine model: Loom Sim Board rev B",
    "Memory: 1015808K/1048576K available",
    "SLUB: HWalign=64, Order=0-3, MinObjects=0, CPUs=4, Nodes=1",
    "rcu: Hierarchical RCU implementation.",
    "clocksource: arch_sys_counter: mask: 0xffffffffffffff max_cycles: 0x1cd42e208c",
    "Console: colour dummy device 80x25",
    "Calibrating delay loop (skipped), value calculated using timer frequency.. 48.00 BogoMIPS",
    "pid_max: default: 32768 minimum: 301",
    "Mount-cache hash table entries: 2048 (order: 2, 16384 bytes, linear)",
    "smp: Brought up 1 node, 4 CPUs",
    "devtmpfs: initialized",
    "NET: Registered PF_NETLINK/PF_ROUTE protocol family",
    "thermal_sys: Registered thermal governor 'step_wise' — zone0 41.5°C ✓",
    "serial: ttyS0 at MMIO 0x9000000 (irq = 33, base_baud = 1500000) is a 16550A",
    "usbcore: registered new interface driver usbfs",
    "mmc0: new high speed SDHC card at address aaaa",
    "EXT4-fs (mmcblk0p2): mounted filesystem with ordered data mode",
    "random: crng init done",
    "eth0: link up, 1000Mbps, full-duplex",
];

const SERVICES: &[&str] = &[
    "Journal Service",
    "udev Kernel Device Manager",
    "Network Configuration",
    "OpenSSH Daemon",
    "Login Service",
    "Loom Sim Agent",
];

/// Builds the boot log: kernel lines, systemd-style colored lines, `\r` progress bars, UTF-8.
fn boot_log(rng: &mut Rng, boot_no: u32) -> Vec<Vec<u8>> {
    let mut lines = Vec::with_capacity(BOOT_LOG_LINES);
    let mut t_us: u64 = 0;
    for i in 0..BOOT_LOG_LINES {
        t_us += 50 + rng.below(30_000);
        let stamp = format!("[{:5}.{:06}] ", t_us / 1_000_000, t_us % 1_000_000);
        let line = match i {
            100 => {
                // Progress bar rewritten with bare CRs, finished by CRLF.
                let mut s = String::new();
                for pct in (0..=100).step_by(10) {
                    let bar = "#".repeat(pct / 10);
                    s.push_str(&format!("fsck /dev/mmcblk0p2: [{bar:<10}] {pct:3}%\r"));
                }
                s.push('\n');
                s
            }
            150..=170 => {
                let svc = SERVICES[(i - 150) % SERVICES.len()];
                if i == 160 {
                    format!("[\x1b[0;1;31mFAILED\x1b[0m] Failed to start {svc}.\r\n")
                } else {
                    format!("[\x1b[0;32m  OK  \x1b[0m] Started {svc}.\r\n")
                }
            }
            _ => {
                let msg = KERNEL_MSGS[usize::try_from(rng.below(KERNEL_MSGS.len() as u64)).unwrap_or(0)];
                if i == 0 {
                    format!("{stamp}{msg} (boot #{boot_no})\r\n")
                } else {
                    format!("{stamp}{msg}\r\n")
                }
            }
        };
        lines.push(line.into_bytes());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_to_login(d: &mut Bootshell) -> Vec<u8> {
        let mut out = Vec::new();
        d.on_open(&mut out);
        let mut now = Duration::ZERO;
        while let Some(next) = d.tick(now, &mut out) {
            now += next;
        }
        out
    }

    fn send(d: &mut Bootshell, s: &[u8]) -> String {
        let mut out = Vec::new();
        d.on_bytes(s, Duration::ZERO, &mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn logged_in(seed: u64) -> Bootshell {
        let mut d = Bootshell::new(seed);
        run_to_login(&mut d);
        send(&mut d, b"root\r");
        send(&mut d, b"secret\r");
        d
    }

    #[test]
    fn boot_log_has_expected_shape() {
        let mut d = Bootshell::new(1);
        let out = run_to_login(&mut d);
        let text = String::from_utf8_lossy(&out);
        assert!(text.ends_with(LOGIN_PROMPT));
        assert!(text.matches("\r\n").count() >= BOOT_LOG_LINES);
        assert!(text.contains("\x1b[0;32m"));
        assert!(text.contains("100%\r\n"));
        assert!(text.contains("°C"));
    }

    #[test]
    fn login_password_is_not_echoed() {
        let mut d = Bootshell::new(1);
        run_to_login(&mut d);
        assert_eq!(send(&mut d, b"root\r"), format!("root\r\n{PASSWORD_PROMPT}"));
        let out = send(&mut d, b"hunter2\r\n");
        assert!(!out.contains("hunter2"));
        assert!(out.ends_with(SHELL_PROMPT));
    }

    #[test]
    fn commands() {
        let mut d = logged_in(2);
        assert!(send(&mut d, b"uname -a\r").contains("Linux sim 6.6.0-loom"));
        assert!(send(&mut d, b"cat /proc/meminfo\r").contains("MemTotal:"));
        assert!(send(&mut d, b"status\r").contains("temp=41.5 rpm=1200\r\n"));
        assert!(send(&mut d, b"frob\r").contains("sh: frob: not found\r\n"));
        assert_eq!(send(&mut d, b"\r"), format!("\r\n{SHELL_PROMPT}"));
    }

    #[test]
    fn line_editing() {
        let mut d = logged_in(3);
        // BS removes the 'x', ^U kills "garbage", then a clean command.
        let out = send(&mut d, b"statux\x7fs\r");
        assert!(out.contains("\x08 \x08"));
        assert!(out.contains("temp=41.5"));
        let out = send(&mut d, b"garbage\x15status\r");
        assert!(out.contains("temp=41.5"));
        let out = send(&mut d, b"half\x03");
        assert_eq!(out, format!("half^C\r\n{SHELL_PROMPT}"));
        // UTF-8 aware backspace.
        let out = send(&mut d, "echo é\x7fe\r".as_bytes());
        assert!(out.contains("\r\ne\r\n"), "{out:?}");
    }

    #[test]
    fn ping_runs_until_ctrl_c() {
        let mut d = logged_in(4);
        let mut out = Vec::new();
        d.on_bytes(b"ping 10.0.0.1\r", Duration::from_secs(10), &mut out);
        let next = d.tick(Duration::from_secs(10), &mut out);
        assert_eq!(next, Some(PING_INTERVAL));
        d.tick(Duration::from_secs(12), &mut out);
        let text = String::from_utf8_lossy(&out);
        assert_eq!(text.matches("icmp_seq=").count(), 3);
        let stop = send(&mut d, b"\x03");
        assert!(stop.contains("--- 10.0.0.1 ping statistics ---"));
        assert!(stop.contains("3 packets transmitted, 3 received"));
        assert!(stop.ends_with(SHELL_PROMPT));
    }

    #[test]
    fn reboot_replays_boot() {
        let mut d = logged_in(5);
        let mut out = Vec::new();
        d.on_bytes(b"reboot\r", Duration::ZERO, &mut out);
        let mut now = Duration::ZERO;
        while let Some(n) = d.tick(now, &mut out) {
            now += n;
        }
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("reboot: Restarting system"));
        assert!(text.contains("(boot #2)"));
        assert!(text.ends_with(LOGIN_PROMPT));
    }

    #[test]
    fn break_signal_is_visible() {
        let mut d = logged_in(6);
        let mut out = Vec::new();
        d.on_signal(Signal::Break(true), &mut out);
        assert_eq!(out, b"\r\n<BREAK>\r\n");
    }
}
