//! `flood` and `binary` scenarios (UART plan §9.2).
//!
//! `flood` writes `seq=N t=<ns>\r\n` lines at a fixed byte rate. `t` is the line's **nominal**
//! emission time (`offset * 1e9 / rate`, 0 when unthrottled) rather than a clock reading, so the
//! stream is a pure function of `(rate, length)` and can be verified with [`flood_bytes`] /
//! [`flood_digest`] (FNV-1a 64 — see the report for why not SHA-256).
//!
//! `binary` writes `frames` × (0x00..=0xFF) and then echoes every host byte back unchanged.

use std::time::Duration;

use super::{Device, Signal};

/// Default flood rate in bytes/s (≈ 921600 baud payload, §9.2).
pub const DEFAULT_FLOOD_RATE: u64 = 92_000;
/// Most bytes one flood tick generates (keeps the runner's pending buffer bounded, §3.13).
const FLOOD_TICK_BYTES: usize = 16 * 1024;
/// Shortest flood tick interval when throttled (batches lines instead of waking per line).
const FLOOD_MIN_TICK: Duration = Duration::from_millis(2);
/// Binary frames generated per tick.
const BINARY_FRAMES_PER_TICK: u32 = 16;

/// Incremental FNV-1a 64-bit hash used for stream integrity checks.
#[derive(Debug, Clone, Copy)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    /// Empty hash state.
    pub fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    /// Absorbs bytes.
    pub fn update(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Current digest.
    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// Deterministic flood line generator.
#[derive(Debug, Clone, Default)]
struct FloodGen {
    seq: u64,
    offset: u64,
}

impl FloodGen {
    fn nominal_ns(&self, rate: u64) -> u64 {
        if rate == 0 {
            return 0;
        }
        u64::try_from(u128::from(self.offset) * 1_000_000_000 / u128::from(rate)).unwrap_or(u64::MAX)
    }

    /// Appends the next line, truncated so the stream never exceeds `limit` bytes.
    fn push_line(&mut self, rate: u64, limit: Option<u64>, out: &mut Vec<u8>) -> usize {
        let line = format!("seq={} t={}\r\n", self.seq, self.nominal_ns(rate));
        let mut take = line.len();
        if let Some(l) = limit {
            take = take.min(usize::try_from(l.saturating_sub(self.offset)).unwrap_or(usize::MAX));
        }
        out.extend_from_slice(&line.as_bytes()[..take]);
        self.seq += 1;
        self.offset += take as u64;
        take
    }
}

/// The first `len` bytes of a flood stream at `rate` (same bytes the device emits).
pub fn flood_bytes(rate: u64, len: usize) -> Vec<u8> {
    let mut g = FloodGen::default();
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        g.push_line(rate, Some(len as u64), &mut out);
    }
    out
}

/// FNV-1a 64 of the first `len` bytes of a flood stream at `rate`.
pub fn flood_digest(rate: u64, len: usize) -> u64 {
    let mut g = FloodGen::default();
    let mut h = Fnv64::new();
    let mut buf = Vec::with_capacity(64);
    while g.offset < len as u64 {
        buf.clear();
        g.push_line(rate, Some(len as u64), &mut buf);
        h.update(&buf);
    }
    h.finish()
}

/// The `flood` fake device.
#[derive(Debug)]
pub struct Flood {
    rate: u64,
    limit: Option<u64>,
    gen_: FloodGen,
    hash: Fnv64,
}

impl Flood {
    /// `rate` bytes/s (`0` = unthrottled), optional total `limit` in bytes.
    pub fn new(rate: u64, limit: Option<u64>) -> Self {
        Self {
            rate,
            limit,
            gen_: FloodGen::default(),
            hash: Fnv64::new(),
        }
    }

    /// Bytes emitted so far.
    pub fn emitted(&self) -> u64 {
        self.gen_.offset
    }

    /// FNV-1a 64 of everything emitted so far.
    pub fn digest(&self) -> u64 {
        self.hash.finish()
    }

    fn done(&self) -> bool {
        self.limit.is_some_and(|l| self.gen_.offset >= l)
    }
}

impl Device for Flood {
    fn on_open(&mut self, _out: &mut Vec<u8>) {}

    fn on_bytes(&mut self, _b: &[u8], _now: Duration, _out: &mut Vec<u8>) {}

    fn tick(&mut self, now: Duration, out: &mut Vec<u8>) -> Option<Duration> {
        let start = out.len();
        let now_ns = u64::try_from(now.as_nanos()).unwrap_or(u64::MAX);
        while !self.done()
            && out.len() - start < FLOOD_TICK_BYTES
            && (self.rate == 0 || self.gen_.nominal_ns(self.rate) <= now_ns)
        {
            self.gen_.push_line(self.rate, self.limit, out);
        }
        self.hash.update(&out[start..]);
        if self.done() {
            return None;
        }
        if self.rate == 0 {
            return Some(Duration::ZERO);
        }
        let due = self.gen_.nominal_ns(self.rate);
        if due <= now_ns {
            Some(Duration::ZERO)
        } else {
            Some(Duration::from_nanos(due - now_ns).max(FLOOD_MIN_TICK))
        }
    }

    fn on_signal(&mut self, _s: Signal, _out: &mut Vec<u8>) {}
}

/// `frames` × (0x00..=0xFF) — the bytes the `binary` device emits first.
pub fn binary_frames(frames: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(frames as usize * 256);
    for _ in 0..frames {
        v.extend(0..=255u8);
    }
    v
}

/// The `binary` fake device: emits frames, then loops host bytes back unchanged.
#[derive(Debug)]
pub struct Binary {
    frames_left: u32,
}

impl Binary {
    /// Emits `frames` frames of 0x00..=0xFF after open.
    pub fn new(frames: u32) -> Self {
        Self { frames_left: frames }
    }
}

impl Device for Binary {
    fn on_open(&mut self, _out: &mut Vec<u8>) {}

    fn on_bytes(&mut self, b: &[u8], _now: Duration, out: &mut Vec<u8>) {
        out.extend_from_slice(b);
    }

    fn tick(&mut self, _now: Duration, out: &mut Vec<u8>) -> Option<Duration> {
        let n = self.frames_left.min(BINARY_FRAMES_PER_TICK);
        for _ in 0..n {
            out.extend(0..=255u8);
        }
        self.frames_left -= n;
        (self.frames_left > 0).then_some(Duration::ZERO)
    }

    fn on_signal(&mut self, _s: Signal, _out: &mut Vec<u8>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_stream_shape_and_digest() {
        let b = flood_bytes(DEFAULT_FLOOD_RATE, 1000);
        assert_eq!(b.len(), 1000);
        assert!(b.starts_with(b"seq=0 t=0\r\nseq=1 t=119565\r\n"));
        let mut h = Fnv64::new();
        h.update(&b);
        assert_eq!(h.finish(), flood_digest(DEFAULT_FLOOD_RATE, 1000));
    }

    #[test]
    fn flood_device_matches_reference_and_paces() {
        let mut d = Flood::new(DEFAULT_FLOOD_RATE, Some(5000));
        let mut out = Vec::new();
        // At t=0 only the first line is due.
        let next = d.tick(Duration::ZERO, &mut out);
        assert_eq!(out, b"seq=0 t=0\r\n");
        assert!(next.is_some_and(|n| n >= FLOOD_MIN_TICK));
        while d.tick(Duration::from_secs(1), &mut out).is_some() {}
        assert_eq!(out, flood_bytes(DEFAULT_FLOOD_RATE, 5000));
        assert_eq!(d.digest(), flood_digest(DEFAULT_FLOOD_RATE, 5000));
        assert_eq!(d.emitted(), 5000);
    }

    #[test]
    fn unthrottled_flood_is_bounded_per_tick() {
        let mut d = Flood::new(0, None);
        let mut out = Vec::new();
        assert_eq!(d.tick(Duration::ZERO, &mut out), Some(Duration::ZERO));
        assert!(out.len() < FLOOD_TICK_BYTES + 64);
        assert!(out.starts_with(b"seq=0 t=0\r\n"));
    }

    #[test]
    fn binary_frames_then_loopback() {
        let mut d = Binary::new(20);
        let mut out = Vec::new();
        while d.tick(Duration::ZERO, &mut out).is_some() {}
        assert_eq!(out, binary_frames(20));
        let mut echo = Vec::new();
        d.on_bytes(&[0, 1, 0xff], Duration::ZERO, &mut echo);
        assert_eq!(echo, [0, 1, 0xff]);
    }
}
