//! `sim` — fake UART devices (UART plan §9.1, §9.2).
//!
//! A [`Device`] is a pure byte state machine. A [`Runner`] drives it (ticks, input, signals) and
//! hands its output out in seed-shaken pieces (splits land inside UTF-8 and ESC sequences on
//! purpose). Two hosts run a `Runner`:
//! * [`crate::transport::SimTransport`] — in-process, all OSes (`sim://<scenario>`).
//! * [`PtyDevice`] (Unix) — a real pty pair, so the worker opens the slave with the same
//!   `SerialTransport` path as a real port (M1 "direct access" proof).
//!
//! M1 scenarios: `bootshell`, `flood`, `binary`, `ansi`. Same seed + same input timeline ⇒ same
//! bytes and the same piece boundaries.

mod ansi;
mod bootshell;
mod flood;
#[cfg(unix)]
mod pty;

use std::io;
use std::time::{Duration, Instant};

pub use ansi::{ANSI_DONE_MARKER, Ansi, QUERY_SEQUENCES, QuerySeq, ansi_corpus};
pub use bootshell::{BOOT_LOG_LINES, Bootshell, LOGIN_PROMPT, PASSWORD_PROMPT, SHELL_PROMPT};
pub use flood::{Binary, DEFAULT_FLOOD_RATE, Flood, Fnv64, binary_frames, flood_bytes, flood_digest};
#[cfg(unix)]
pub use pty::{PtyDevice, PtyHandle, is_live_sim_pty};

/// Modem/line signal delivered to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// DTR level changed.
    Dtr(bool),
    /// RTS level changed.
    Rts(bool),
    /// Break asserted (`true`) or cleared (`false`).
    Break(bool),
}

/// A fake device: a deterministic byte state machine. `now` is time since the device opened.
pub trait Device: Send {
    /// Called once when the host side opens.
    fn on_open(&mut self, out: &mut Vec<u8>);
    /// Bytes from the host.
    fn on_bytes(&mut self, b: &[u8], now: Duration, out: &mut Vec<u8>);
    /// Periodic work; returns the delay until the next tick (`None` = only after input/signal).
    fn tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration>;
    /// A modem/line signal from the host.
    fn on_signal(&mut self, s: Signal, out: &mut Vec<u8>);
}

/// Device output kept in the runner before the host takes it; above this the runner stops
/// calling [`Device::tick`] (device-side pacing, §9.1).
pub const RUNNER_PENDING_HIGH_WATER: usize = 64 * 1024;
/// Hard cap of device output held in the runner (§3.13 no unbounded buffers). Output beyond it
/// (only possible from input-triggered bursts) is dropped and counted in [`Runner::dropped`].
pub const RUNNER_PENDING_MAX: usize = 1024 * 1024;
/// Maximum accepted `sim://` target length.
pub const SIM_TARGET_MAX: usize = 256;

/// Scenario names known in M1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scenario {
    /// Boot log → login → shell.
    Bootshell,
    /// `seq=N t=<ns>` lines at a fixed rate.
    Flood,
    /// 0x00–0xFF frames, then loopback.
    Binary,
    /// ANSI / OSC / terminal query corpus.
    Ansi,
}

impl Scenario {
    /// Scenario name as used in `sim://<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Bootshell => "bootshell",
            Scenario::Flood => "flood",
            Scenario::Binary => "binary",
            Scenario::Ansi => "ansi",
        }
    }
}

/// Parsed `sim://<scenario>[?seed=N&...]` target.
///
/// Keys: `seed` (all, default 0), `split` (all; `0` disables piece shaking, default 1),
/// `rate` (flood bytes/s, default [`DEFAULT_FLOOD_RATE`], `0` = unthrottled), `bytes` (flood total,
/// default unlimited), `frames` (binary, default 1). Unknown keys are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimSpec {
    /// Scenario.
    pub scenario: Scenario,
    /// Seed for content variation and piece splitting.
    pub seed: u64,
    /// Shake output into random pieces with short pauses.
    pub split: bool,
    /// Flood rate in bytes/s (`0` = as fast as the host reads).
    pub rate: u64,
    /// Flood total byte limit.
    pub bytes: Option<u64>,
    /// Binary frame count.
    pub frames: u32,
}

impl SimSpec {
    /// A spec with default parameters.
    pub fn new(scenario: Scenario, seed: u64) -> Self {
        Self {
            scenario,
            seed,
            split: true,
            rate: DEFAULT_FLOOD_RATE,
            bytes: None,
            frames: 1,
        }
    }

    /// Parses `sim://<scenario>[?k=v&...]`.
    pub fn parse(target: &str) -> io::Result<Self> {
        let bad = |msg: String| io::Error::new(io::ErrorKind::InvalidInput, msg);
        if target.len() > SIM_TARGET_MAX {
            return Err(bad(format!("sim target longer than {SIM_TARGET_MAX} bytes")));
        }
        let rest = target
            .strip_prefix(crate::transport::SIM_SCHEME)
            .ok_or_else(|| bad(format!("not a sim:// target: {target}")))?;
        let (name, query) = match rest.split_once('?') {
            Some((n, q)) => (n, Some(q)),
            None => (rest, None),
        };
        let scenario = match name {
            "bootshell" => Scenario::Bootshell,
            "flood" => Scenario::Flood,
            "binary" => Scenario::Binary,
            "ansi" => Scenario::Ansi,
            other => return Err(bad(format!("unknown sim scenario '{other}'"))),
        };
        let mut spec = Self::new(scenario, 0);
        for pair in query.into_iter().flat_map(|q| q.split('&')) {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair
                .split_once('=')
                .ok_or_else(|| bad(format!("sim parameter without value: '{pair}'")))?;
            let num = || {
                v.parse::<u64>()
                    .map_err(|_| bad(format!("sim parameter {k}: '{v}' is not a number")))
            };
            match (k, scenario) {
                ("seed", _) => spec.seed = num()?,
                ("split", _) => spec.split = num()? != 0,
                ("rate", Scenario::Flood) => spec.rate = num()?,
                ("bytes", Scenario::Flood) => spec.bytes = Some(num()?),
                ("frames", Scenario::Binary) => {
                    spec.frames = u32::try_from(num()?).map_err(|_| bad(format!("frames too large: {v}")))?;
                }
                _ => {
                    return Err(bad(format!(
                        "unknown parameter '{k}' for sim scenario '{}'",
                        scenario.name()
                    )));
                }
            }
        }
        Ok(spec)
    }

    /// Builds the device and its output splitter.
    pub fn build(&self) -> (Box<dyn Device>, Splitter) {
        let interactive = matches!(self.scenario, Scenario::Bootshell | Scenario::Ansi);
        let splitter = if !self.split {
            Splitter::whole()
        } else if interactive {
            Splitter::interactive(self.seed)
        } else {
            Splitter::bulk(self.seed)
        };
        let device: Box<dyn Device> = match self.scenario {
            Scenario::Bootshell => Box::new(Bootshell::new(self.seed)),
            Scenario::Flood => Box::new(Flood::new(self.rate, self.bytes)),
            Scenario::Binary => Box::new(Binary::new(self.frames)),
            Scenario::Ansi => Box::new(Ansi::new()),
        };
        (device, splitter)
    }
}

/// SplitMix64 — tiny deterministic PRNG (no external crate needed).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// Seeded generator.
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish value in `0..n` (`0` when `n == 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

/// Decides output piece sizes and short pauses between pieces (seeded).
#[derive(Debug, Clone)]
pub struct Splitter {
    rng: Rng,
    max_piece: usize,
    /// Pause after a piece with probability 1/`pause_one_in` (`0` = never).
    pause_one_in: u64,
    pause: Duration,
}

impl Splitter {
    /// Interactive scenarios: pieces of 1..=48 bytes, a 1 ms pause after ~1 in 12 pieces so the
    /// reader really observes the splits.
    pub fn interactive(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed.wrapping_add(1)),
            max_piece: 48,
            pause_one_in: 12,
            pause: Duration::from_millis(1),
        }
    }

    /// Bulk scenarios: pieces of 1..=4096 bytes, no pauses (throughput).
    pub fn bulk(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed.wrapping_add(1)),
            max_piece: 4096,
            pause_one_in: 0,
            pause: Duration::ZERO,
        }
    }

    /// No shaking: pieces as large as possible.
    pub fn whole() -> Self {
        Self {
            rng: Rng::new(0),
            max_piece: usize::MAX,
            pause_one_in: 0,
            pause: Duration::ZERO,
        }
    }

    fn next_len(&mut self, avail: usize) -> usize {
        let cap = self.max_piece.min(avail).max(1);
        if self.max_piece == usize::MAX {
            return cap;
        }
        1 + usize::try_from(self.rng.below(cap as u64)).unwrap_or(0)
    }

    fn pause_after(&mut self) -> Option<Duration> {
        if self.pause_one_in == 0 || self.rng.below(self.pause_one_in) != 0 {
            None
        } else {
            Some(self.pause)
        }
    }
}

/// Drives one [`Device`]: input, signals, ticks, and piecewise output.
pub struct Runner {
    device: Box<dyn Device>,
    split: Splitter,
    origin: Instant,
    scratch: Vec<u8>,
    pending: Vec<u8>,
    head: usize,
    piece_left: usize,
    next_tick: Option<Instant>,
    hold_until: Option<Instant>,
    dropped: u64,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner")
            .field("pending", &self.pending_len())
            .field("dropped", &self.dropped)
            .finish()
    }
}

impl Runner {
    /// Creates a runner and calls [`Device::on_open`].
    pub fn start(device: Box<dyn Device>, split: Splitter, now: Instant) -> Self {
        let mut r = Self {
            device,
            split,
            origin: now,
            scratch: Vec::new(),
            pending: Vec::new(),
            head: 0,
            piece_left: 0,
            next_tick: Some(now),
            hold_until: None,
            dropped: 0,
        };
        r.device.on_open(&mut r.scratch);
        r.absorb();
        r
    }

    fn rel(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.origin)
    }

    fn absorb(&mut self) {
        if self.head > 0 && self.head * 2 >= self.pending.len() {
            self.pending.drain(..self.head);
            self.head = 0;
        }
        let room = RUNNER_PENDING_MAX.saturating_sub(self.pending_len());
        let take = self.scratch.len().min(room);
        self.pending.extend_from_slice(&self.scratch[..take]);
        self.dropped += (self.scratch.len() - take) as u64;
        self.scratch.clear();
    }

    /// Output bytes waiting for the host.
    pub fn pending_len(&self) -> usize {
        self.pending.len() - self.head
    }

    /// Device output dropped because [`RUNNER_PENDING_MAX`] was reached.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Feeds host bytes to the device.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        let rel = self.rel(now);
        self.device.on_bytes(bytes, rel, &mut self.scratch);
        self.absorb();
        self.next_tick = Some(now);
    }

    /// Delivers a signal to the device.
    pub fn signal(&mut self, s: Signal, now: Instant) {
        self.device.on_signal(s, &mut self.scratch);
        self.absorb();
        self.next_tick = Some(now);
    }

    /// Runs [`Device::tick`] if due and the pending output is below the high-water mark.
    pub fn tick_if_due(&mut self, now: Instant) {
        if self.pending_len() >= RUNNER_PENDING_HIGH_WATER {
            return;
        }
        if let Some(t) = self.next_tick
            && t <= now
        {
            let rel = self.rel(now);
            self.next_tick = self.device.tick(rel, &mut self.scratch).map(|d| now + d);
            self.absorb();
        }
    }

    /// The next output piece, or `None` if nothing is ready (empty or pausing).
    pub fn piece(&mut self, now: Instant) -> Option<&[u8]> {
        if self.pending_len() == 0 {
            return None;
        }
        if let Some(h) = self.hold_until {
            if h > now {
                return None;
            }
            self.hold_until = None;
        }
        if self.piece_left == 0 {
            self.piece_left = self.split.next_len(self.pending_len());
        }
        Some(&self.pending[self.head..self.head + self.piece_left])
    }

    /// Marks `n` bytes of the current piece as delivered.
    pub fn consumed(&mut self, n: usize, now: Instant) {
        let n = n.min(self.piece_left);
        self.head += n;
        self.piece_left -= n;
        if self.piece_left == 0
            && n > 0
            && let Some(p) = self.split.pause_after()
        {
            self.hold_until = Some(now + p);
        }
        if self.head == self.pending.len() {
            self.pending.clear();
            self.head = 0;
        }
    }

    /// Earliest instant at which the runner has work: a due tick, or — when the host side can
    /// take output (`can_output`) — the next piece (now, or the end of a split pause).
    pub fn next_wakeup(&self, can_output: bool, now: Instant) -> Option<Instant> {
        let tick = if self.pending_len() < RUNNER_PENDING_HIGH_WATER {
            self.next_tick
        } else {
            None
        };
        let out = if can_output && self.pending_len() > 0 {
            Some(self.hold_until.unwrap_or(now))
        } else {
            None
        };
        match (tick, out) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Escapes bytes for human-readable device messages (`\xNN` for non-printables).
pub(crate) fn escape_bytes(b: &[u8], out: &mut Vec<u8>) {
    for &c in b {
        match c {
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x20..=0x7E => out.push(c),
            _ => out.extend_from_slice(format!("\\x{c:02x}").as_bytes()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_targets() {
        let s = SimSpec::parse("sim://bootshell?seed=7").expect("valid");
        assert_eq!((s.scenario, s.seed, s.split), (Scenario::Bootshell, 7, true));
        let s = SimSpec::parse("sim://flood?rate=0&bytes=100&split=0").expect("valid");
        assert_eq!((s.rate, s.bytes, s.split), (0, Some(100), false));
        let s = SimSpec::parse("sim://binary?frames=3").expect("valid");
        assert_eq!(s.frames, 3);
        assert!(SimSpec::parse("sim://ansi").is_ok());
        for bad in [
            "sim://nope",
            "sim://bootshell?rate=1",
            "sim://flood?seed=x",
            "sim://flood?seed",
            "/dev/tty",
        ] {
            assert!(SimSpec::parse(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(3);
        let mut b = Rng::new(3);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(3).next_u64(), Rng::new(4).next_u64());
    }

    /// Collects runner output by draining pieces with a fake clock that jumps past pauses.
    fn drain_pieces(spec: &SimSpec, rounds: usize) -> (Vec<u8>, Vec<usize>) {
        let (dev, split) = spec.build();
        let t0 = Instant::now();
        let mut now = t0;
        let mut r = Runner::start(dev, split, now);
        let mut out = Vec::new();
        let mut sizes = Vec::new();
        for _ in 0..rounds {
            r.tick_if_due(now);
            while let Some(p) = r.piece(now) {
                out.extend_from_slice(p);
                let n = p.len();
                sizes.push(n);
                r.consumed(n, now);
            }
            now += Duration::from_millis(2);
        }
        (out, sizes)
    }

    #[test]
    fn same_seed_same_bytes_and_splits() {
        let spec = SimSpec::new(Scenario::Bootshell, 42);
        let a = drain_pieces(&spec, 200);
        let b = drain_pieces(&spec, 200);
        assert!(!a.0.is_empty());
        assert_eq!(a, b);
        let other = drain_pieces(&SimSpec::new(Scenario::Bootshell, 43), 200);
        assert_ne!(a.1, other.1, "different seed shakes differently");
    }

    #[test]
    fn interactive_split_cuts_inside_escape_sequences() {
        let spec = SimSpec::new(Scenario::Ansi, 1);
        let (bytes, sizes) = drain_pieces(&spec, 50);
        let mut pos = 0usize;
        let mut cut_in_esc = false;
        for n in sizes {
            pos += n;
            // A cut right after ESC or inside `ESC [ ... final` counts.
            if pos < bytes.len() && bytes[pos - 1] == 0x1b {
                cut_in_esc = true;
            }
        }
        assert!(cut_in_esc, "some piece boundary must split an ESC sequence");
    }

    #[test]
    fn runner_caps_pending_output() {
        struct Burst;
        impl Device for Burst {
            fn on_open(&mut self, _: &mut Vec<u8>) {}
            fn on_bytes(&mut self, _: &[u8], _: Duration, out: &mut Vec<u8>) {
                out.resize(out.len() + 700 * 1024, b'x');
            }
            fn tick(&mut self, _: Duration, _: &mut Vec<u8>) -> Option<Duration> {
                None
            }
            fn on_signal(&mut self, _: Signal, _: &mut Vec<u8>) {}
        }
        let now = Instant::now();
        let mut r = Runner::start(Box::new(Burst), Splitter::whole(), now);
        r.feed(b"a", now);
        r.feed(b"b", now);
        assert_eq!(r.pending_len(), RUNNER_PENDING_MAX);
        assert_eq!(r.dropped(), (1400 * 1024 - RUNNER_PENDING_MAX) as u64);
    }
}
