//! Pure model: strict start-config validation, sigrok-cli argv, edge compression, annotation
//! parsing and the synthetic UART/SPI waveform generator. No I/O here.
//!
//! First-slice invariants (CONTRACT §8): acquisition is always D0..D7 (8 channels, 1 byte per
//! sample, bit i = Di); the decoder reads `-I binary:numchannels=8`; user `channels` only select
//! which bits are shown as edges. No shell is ever used: every argv element is built from
//! validated, allow-listed tokens.

use std::fmt;
use std::path::{Path, PathBuf};

use loom_proto::error::ErrorCode;
use serde_json::{Map, Value, json};

/// Channels per sample (fixed, first slice).
pub const CHANNELS: u8 = 8;
/// Acquisition channel list (always all eight, in bit order).
pub const ACQ_CHANNELS: &str = "D0,D1,D2,D3,D4,D5,D6,D7";
/// Sample rate bounds (Hz).
pub const MIN_SAMPLERATE: u64 = 1_000;
/// Upper sample rate of the first slice (CPU/journal bound, not a hardware limit).
pub const MAX_SAMPLERATE: u64 = 10_000_000;
/// Default sample rate.
pub const DEFAULT_SAMPLERATE: u64 = 1_000_000;
/// libsigrok demo default rate; a demo `pattern` can only be set together with it (the demo
/// driver rejects `samplerate` when a channel group is selected — verified on 0.7.2).
pub const DEMO_PATTERN_SAMPLERATE: u64 = 200_000;
/// Demo logic patterns (libsigrok 0.5.2 `logic_pattern_str`).
pub const DEMO_PATTERNS: &[&str] = &[
    "sigrok",
    "random",
    "incremental",
    "walking-one",
    "walking-zero",
    "all-low",
    "all-high",
    "squid",
    "graycode",
];
/// Duration bounds (ms).
pub const DEFAULT_DURATION_MS: u64 = 60_000;
/// Maximum capture duration.
pub const MAX_DURATION_MS: u64 = 3_600_000;
/// Minimum capture duration.
pub const MIN_DURATION_MS: u64 = 100;
/// Maximum `max_samples`.
pub const MAX_SAMPLES_LIMIT: u64 = 100_000_000_000;
/// Synthetic text bound (bytes, after `{n}` expansion).
pub const MAX_TEXT_BYTES: usize = 256;
/// Default synthetic text (`{n}` = 4-digit frame counter).
pub const DEFAULT_TEXT: &str = "LOOM {n}\r\n";
/// Default synthetic frame period.
pub const DEFAULT_FRAME_PERIOD_MS: u64 = 100;
/// Edges kept per `logic.edges` event (the rest are counted in `dropped`). 1024 keeps the worst
/// payload (11-digit sample indices, all 8 bits) under the 24 KiB reducer snapshot budget, so a
/// late joiner never loses a whole raw chunk; CONTRACT 4096 is only a cap.
pub const MAX_EDGES_PER_EVENT: usize = 1024;
/// Serialized payload bound for every logic data event (journal reads are 64 KiB).
pub const MAX_PAYLOAD_BYTES: usize = 48 * 1024;
/// Decoders accepted (first slice; must also be compiled into the installed sigrok-cli).
pub const DECODERS: &[&str] = &["uart", "spi"];
/// Drivers for `sigrok-device` that pass validation (8-bit candidates; hardware unverified).
pub const DEVICE_DRIVERS: &[&str] = &["fx2lafw"];
/// Drivers known to libsigrok but refused: 16-channel unitsize conflicts with 1 byte/sample.
pub const REFUSED_DRIVERS: &[&str] = &["saleae-logic16", "saleae-logic-pro"];

/// Validation error (maps to `E_USAGE` or `E_NOT_IMPLEMENTED`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Error code.
    pub code: ErrorCode,
    /// Message.
    pub message: String,
}

impl ConfigError {
    fn usage(m: impl Into<String>) -> Self {
        ConfigError {
            code: ErrorCode::Usage,
            message: m.into(),
        }
    }
    fn unsupported(m: impl Into<String>) -> Self {
        ConfigError {
            code: ErrorCode::NotImplemented,
            message: m.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

type R<T> = Result<T, ConfigError>;

/// Where samples come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// libsigrok demo driver — real acquisition path, synthetic content.
    SigrokDemo,
    /// Real device through a libsigrok driver (first slice: fx2lafw only, unverified).
    SigrokDevice,
    /// Loom-generated waveform decoded by the real libsigrok decoder.
    Synthetic,
}

impl Source {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::SigrokDemo => "sigrok-demo",
            Source::SigrokDevice => "sigrok-device",
            Source::Synthetic => "synthetic",
        }
    }
    fn parse(s: &str) -> R<Source> {
        match s {
            "sigrok-demo" => Ok(Source::SigrokDemo),
            "sigrok-device" => Ok(Source::SigrokDevice),
            "synthetic" => Ok(Source::Synthetic),
            "saleae-hla" => Err(ConfigError::unsupported(
                "source saleae-hla is not implemented (first slice)",
            )),
            o => Err(ConfigError::usage(format!(
                "source {o:?}: expected sigrok-demo, sigrok-device or synthetic"
            ))),
        }
    }
    /// True when an acquisition `sigrok-cli` process is spawned.
    pub fn acquires(self) -> bool {
        !matches!(self, Source::Synthetic)
    }
}

/// UART decoder options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uart {
    /// Data channel (bit index).
    pub ch: u8,
    /// True = `tx` line, false = `rx`.
    pub tx: bool,
    /// Baud rate.
    pub baudrate: u32,
    /// Data bits.
    pub data_bits: u8,
    /// `none` | `odd` | `even`.
    pub parity: &'static str,
    /// `1` | `1.5` | `2`.
    pub stop_bits: &'static str,
    /// LSB first (UART default).
    pub lsb_first: bool,
}

/// SPI decoder options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spi {
    /// Clock channel.
    pub clk: u8,
    /// MOSI channel.
    pub mosi: Option<u8>,
    /// MISO channel.
    pub miso: Option<u8>,
    /// Chip-select channel.
    pub cs: Option<u8>,
    /// CPOL.
    pub cpol: u8,
    /// CPHA.
    pub cpha: u8,
    /// Word size (bits).
    pub wordsize: u8,
    /// MSB first.
    pub msb_first: bool,
    /// CS active low.
    pub cs_active_low: bool,
    /// Annotated line: true = MISO data, false = MOSI data.
    pub annotate_miso: bool,
}

/// One decoder stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoder {
    /// `uart`.
    Uart(Uart),
    /// `spi`.
    Spi(Spi),
}

fn chan(v: &str) -> R<u8> {
    let d = v.strip_prefix('D').unwrap_or(v);
    match d.parse::<u8>() {
        Ok(n) if n < CHANNELS && !d.starts_with('+') && d.len() == 1 => Ok(n),
        _ => Err(ConfigError::usage(format!("channel {v:?}: expected D0..D7"))),
    }
}

fn num<T: std::str::FromStr>(k: &str, v: &str, lo: u64, hi: u64) -> R<T> {
    let bad = || ConfigError::usage(format!("{k}={v:?}: expected integer {lo}..={hi}"));
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) || v.len() > 12 {
        return Err(bad());
    }
    let n: u64 = v.parse().map_err(|_| bad())?;
    if !(lo..=hi).contains(&n) {
        return Err(bad());
    }
    v.parse::<T>().map_err(|_| bad())
}

fn pick(k: &str, v: &str, allowed: &[&'static str]) -> R<&'static str> {
    allowed
        .iter()
        .find(|a| **a == v)
        .copied()
        .ok_or_else(|| ConfigError::usage(format!("{k}={v:?}: expected one of {}", allowed.join(", "))))
}

impl Decoder {
    /// Parse `uart:baudrate=115200:rx=D0` / `spi:clk=D0:mosi=D1:cs=D2` (strict, allow-listed).
    pub fn parse(spec: &str) -> R<Decoder> {
        if spec.len() > 256 {
            return Err(ConfigError::usage("decoder spec longer than 256 bytes"));
        }
        let mut parts = spec.split(':');
        let id = parts.next().unwrap_or_default();
        let mut kv: Vec<(&str, &str)> = Vec::new();
        for p in parts {
            let (k, v) = p
                .split_once('=')
                .ok_or_else(|| ConfigError::usage(format!("decoder option {p:?}: expected key=value")))?;
            if kv.iter().any(|(k2, _)| *k2 == k) {
                return Err(ConfigError::usage(format!("decoder option {k:?} given twice")));
            }
            kv.push((k, v));
        }
        match id {
            "uart" => Self::parse_uart(&kv),
            "spi" => Self::parse_spi(&kv),
            "" => Err(ConfigError::usage("empty decoder spec")),
            o => Err(ConfigError::unsupported(format!(
                "decoder {o:?} is not supported in the first slice (uart, spi)"
            ))),
        }
    }

    fn parse_uart(kv: &[(&str, &str)]) -> R<Decoder> {
        let mut u = Uart {
            ch: 0,
            tx: false,
            baudrate: 115_200,
            data_bits: 8,
            parity: "none",
            stop_bits: "1",
            lsb_first: true,
        };
        let (mut rx, mut tx) = (None, None);
        for (k, v) in kv {
            match *k {
                "rx" => rx = Some(chan(v)?),
                "tx" => tx = Some(chan(v)?),
                "baudrate" => u.baudrate = num(k, v, 50, 5_000_000)?,
                "data_bits" => u.data_bits = num(k, v, 5, 9)?,
                "parity" => u.parity = pick(k, v, &["none", "odd", "even"])?,
                "stop_bits" => u.stop_bits = pick(k, v, &["1", "1.5", "2"])?,
                "bit_order" => u.lsb_first = pick(k, v, &["lsb-first", "msb-first"])? == "lsb-first",
                o => {
                    return Err(ConfigError::usage(format!(
                        "uart option {o:?}: expected rx, tx, baudrate, data_bits, parity, stop_bits, bit_order"
                    )));
                }
            }
        }
        match (rx, tx) {
            (Some(c), None) => u.ch = c,
            (None, Some(c)) => {
                u.ch = c;
                u.tx = true;
            }
            _ => {
                return Err(ConfigError::usage(
                    "uart needs exactly one of rx= or tx= (one annotated line per capture)",
                ));
            }
        }
        Ok(Decoder::Uart(u))
    }

    fn parse_spi(kv: &[(&str, &str)]) -> R<Decoder> {
        let mut s = Spi {
            clk: 0,
            mosi: None,
            miso: None,
            cs: None,
            cpol: 0,
            cpha: 0,
            wordsize: 8,
            msb_first: true,
            cs_active_low: true,
            annotate_miso: false,
        };
        let mut clk = None;
        let mut annotate = None;
        for (k, v) in kv {
            match *k {
                "clk" => clk = Some(chan(v)?),
                "mosi" => s.mosi = Some(chan(v)?),
                "miso" => s.miso = Some(chan(v)?),
                "cs" => s.cs = Some(chan(v)?),
                "cpol" => s.cpol = num(k, v, 0, 1)?,
                "cpha" => s.cpha = num(k, v, 0, 1)?,
                "wordsize" => s.wordsize = num(k, v, 1, 32)?,
                "bitorder" => s.msb_first = pick(k, v, &["msb-first", "lsb-first"])? == "msb-first",
                "cs_polarity" => {
                    s.cs_active_low = pick(k, v, &["active-low", "active-high"])? == "active-low"
                }
                "annotate" => annotate = Some(pick(k, v, &["mosi", "miso"])?),
                o => {
                    return Err(ConfigError::usage(format!(
                        "spi option {o:?}: expected clk, mosi, miso, cs, cpol, cpha, wordsize, bitorder, cs_polarity, annotate"
                    )));
                }
            }
        }
        s.clk = clk.ok_or_else(|| ConfigError::usage("spi needs clk="))?;
        if s.mosi.is_none() && s.miso.is_none() {
            return Err(ConfigError::usage("spi needs mosi= and/or miso="));
        }
        s.annotate_miso = match annotate {
            Some("miso") if s.miso.is_none() => {
                return Err(ConfigError::usage("annotate=miso without miso="));
            }
            Some("mosi") if s.mosi.is_none() => {
                return Err(ConfigError::usage("annotate=mosi without mosi="));
            }
            Some(a) => a == "miso",
            None => s.mosi.is_none(),
        };
        let used: Vec<u8> = [Some(s.clk), s.mosi, s.miso, s.cs]
            .into_iter()
            .flatten()
            .collect();
        for (i, a) in used.iter().enumerate() {
            if used[i + 1..].contains(a) {
                return Err(ConfigError::usage(format!("spi channel D{a} used for two roles")));
            }
        }
        Ok(Decoder::Spi(s))
    }

    /// Decoder id (`uart`/`spi`).
    pub fn id(&self) -> &'static str {
        match self {
            Decoder::Uart(_) => "uart",
            Decoder::Spi(_) => "spi",
        }
    }

    /// Canonical spec with `Dn` channel names (stable; recorded in events).
    pub fn canonical(&self) -> String {
        match self {
            Decoder::Uart(u) => format!(
                "uart:{}=D{}:baudrate={}:data_bits={}:parity={}:stop_bits={}:bit_order={}",
                if u.tx { "tx" } else { "rx" },
                u.ch,
                u.baudrate,
                u.data_bits,
                u.parity,
                u.stop_bits,
                if u.lsb_first { "lsb-first" } else { "msb-first" }
            ),
            Decoder::Spi(s) => {
                let mut out = format!("spi:clk=D{}", s.clk);
                for (k, v) in [("mosi", s.mosi), ("miso", s.miso), ("cs", s.cs)] {
                    if let Some(c) = v {
                        out.push_str(&format!(":{k}=D{c}"));
                    }
                }
                out.push_str(&format!(
                    ":cpol={}:cpha={}:wordsize={}:bitorder={}:cs_polarity={}:annotate={}",
                    s.cpol,
                    s.cpha,
                    s.wordsize,
                    if s.msb_first { "msb-first" } else { "lsb-first" },
                    if s.cs_active_low {
                        "active-low"
                    } else {
                        "active-high"
                    },
                    if s.annotate_miso { "miso" } else { "mosi" }
                ));
                out
            }
        }
    }

    /// `-P` value for sigrok-cli (binary input channels are named `0`..`7`).
    pub fn pd_arg(&self) -> String {
        match self {
            Decoder::Uart(u) => {
                let stop = match u.stop_bits {
                    "1.5" => "1.5",
                    "2" => "2.0",
                    _ => "1.0",
                };
                format!(
                    "uart:{}={}:baudrate={}:data_bits={}:parity={}:stop_bits={}:bit_order={}",
                    if u.tx { "tx" } else { "rx" },
                    u.ch,
                    u.baudrate,
                    u.data_bits,
                    u.parity,
                    stop,
                    if u.lsb_first { "lsb-first" } else { "msb-first" }
                )
            }
            Decoder::Spi(s) => {
                let mut out = format!("spi:clk={}", s.clk);
                for (k, v) in [("mosi", s.mosi), ("miso", s.miso), ("cs", s.cs)] {
                    if let Some(c) = v {
                        out.push_str(&format!(":{k}={c}"));
                    }
                }
                out.push_str(&format!(
                    ":cpol={}:cpha={}:wordsize={}:bitorder={}:cs_polarity={}",
                    s.cpol,
                    s.cpha,
                    s.wordsize,
                    if s.msb_first { "msb-first" } else { "lsb-first" },
                    if s.cs_active_low {
                        "active-low"
                    } else {
                        "active-high"
                    }
                ));
                out
            }
        }
    }

    /// `-A` value: data words only (verified to suppress per-bit annotations).
    pub fn annotation_filter(&self) -> String {
        match self {
            Decoder::Uart(u) => format!("uart={}", if u.tx { "tx-data" } else { "rx-data" }),
            Decoder::Spi(s) => format!("spi={}", if s.annotate_miso { "miso-data" } else { "mosi-data" }),
        }
    }

    /// Channel bits the decoder reads.
    pub fn mask(&self) -> u8 {
        let bits: Vec<u8> = match self {
            Decoder::Uart(u) => vec![u.ch],
            Decoder::Spi(s) => [Some(s.clk), s.mosi, s.miso, s.cs]
                .into_iter()
                .flatten()
                .collect(),
        };
        bits.into_iter().fold(0, |m, b| m | (1 << b))
    }
}

/// Validated `logic.start` parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartConfig {
    /// Source.
    pub source: Source,
    /// libsigrok driver (`demo` for sigrok-demo).
    pub driver: Option<String>,
    /// Device `conn=` (USB `bus.address`) for sigrok-device.
    pub conn: Option<String>,
    /// Sample rate (Hz).
    pub samplerate: u64,
    /// Displayed channel mask (bit i = Di).
    pub mask: u8,
    /// Decoder (at most one in the first slice).
    pub decoder: Option<Decoder>,
    /// Duration bound (ms), enforced by the worker.
    pub duration_ms: u64,
    /// Sample bound.
    pub max_samples: Option<u64>,
    /// Demo pattern.
    pub pattern: Option<String>,
    /// Synthetic text template.
    pub text: String,
    /// Synthetic frame period (ms).
    pub frame_period_ms: u64,
    /// Synthetic SPI clock (Hz).
    pub clock_hz: Option<u64>,
}

const START_KEYS: &[&str] = &[
    "source",
    "driver",
    "conn",
    "samplerate",
    "channels",
    "decoders",
    "duration_ms",
    "max_samples",
    "pattern",
    "text",
    "frame_period_ms",
    "clock_hz",
];

fn get_u64(o: &Map<String, Value>, k: &str) -> R<Option<u64>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| ConfigError::usage(format!("{k}: expected a non-negative integer"))),
    }
}

fn get_str<'a>(o: &'a Map<String, Value>, k: &str) -> R<Option<&'a str>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(Some)
            .ok_or_else(|| ConfigError::usage(format!("{k}: expected a string"))),
    }
}

/// Mask → `["D0", …]`.
pub fn mask_channels(mask: u8) -> Vec<String> {
    (0..CHANNELS)
        .filter(|b| mask & (1 << b) != 0)
        .map(|b| format!("D{b}"))
        .collect()
}

impl StartConfig {
    /// Strict parse: unknown keys, wrong types, out-of-range values and non-allow-listed tokens
    /// are refused (nothing is clamped).
    pub fn from_params(p: &Value) -> R<StartConfig> {
        let o = p
            .as_object()
            .ok_or_else(|| ConfigError::usage("params must be an object"))?;
        if let Some(k) = o.keys().find(|k| !START_KEYS.contains(&k.as_str())) {
            return Err(ConfigError::usage(format!(
                "unknown parameter {k:?} (allowed: {})",
                START_KEYS.join(", ")
            )));
        }
        let source =
            Source::parse(get_str(o, "source")?.ok_or_else(|| ConfigError::usage("source is required"))?)?;
        let pattern = get_str(o, "pattern")?.map(str::to_owned);
        let driver = get_str(o, "driver")?;
        let conn = get_str(o, "conn")?;
        let driver = match source {
            Source::SigrokDemo => {
                if driver.is_some_and(|d| d != "demo") || conn.is_some() {
                    return Err(ConfigError::usage("sigrok-demo uses driver demo and no conn"));
                }
                Some("demo".to_owned())
            }
            Source::SigrokDevice => {
                let d = driver.ok_or_else(|| ConfigError::usage("sigrok-device needs driver"))?;
                if REFUSED_DRIVERS.contains(&d) {
                    return Err(ConfigError::unsupported(format!(
                        "driver {d}: 16-channel unitsize conflicts with the fixed 8-channel / 1-byte-per-sample first slice"
                    )));
                }
                if !DEVICE_DRIVERS.contains(&d) {
                    return Err(ConfigError::usage(format!(
                        "driver {d:?}: first slice accepts only {} (hardware not yet verified)",
                        DEVICE_DRIVERS.join(", ")
                    )));
                }
                Some(d.to_owned())
            }
            Source::Synthetic => {
                if driver.is_some() || conn.is_some() {
                    return Err(ConfigError::usage("synthetic takes no driver/conn"));
                }
                None
            }
        };
        let conn = match conn {
            Some(c) if source == Source::SigrokDevice => {
                let ok = c.split_once('.').is_some_and(|(a, b)| {
                    [a, b]
                        .iter()
                        .all(|x| !x.is_empty() && x.len() <= 3 && x.bytes().all(|b| b.is_ascii_digit()))
                });
                if !ok {
                    return Err(ConfigError::usage(format!(
                        "conn {c:?}: expected USB bus.address (e.g. 1.5)"
                    )));
                }
                Some(c.to_owned())
            }
            _ => None,
        };
        if source != Source::SigrokDemo && pattern.is_some() {
            return Err(ConfigError::usage("pattern applies only to sigrok-demo"));
        }
        if let Some(pt) = &pattern
            && !DEMO_PATTERNS.contains(&pt.as_str())
        {
            return Err(ConfigError::usage(format!(
                "pattern {pt:?}: expected one of {}",
                DEMO_PATTERNS.join(", ")
            )));
        }
        let samplerate = match (get_u64(o, "samplerate")?, &pattern) {
            (Some(r), Some(_)) if r != DEMO_PATTERN_SAMPLERATE => {
                return Err(ConfigError::usage(format!(
                    "demo pattern runs at the demo default {DEMO_PATTERN_SAMPLERATE} Hz only (libsigrok refuses samplerate with a channel group)"
                )));
            }
            (None, Some(_)) => DEMO_PATTERN_SAMPLERATE,
            (r, _) => r.unwrap_or(DEFAULT_SAMPLERATE),
        };
        if !(MIN_SAMPLERATE..=MAX_SAMPLERATE).contains(&samplerate) {
            return Err(ConfigError::usage(format!(
                "samplerate {samplerate}: expected {MIN_SAMPLERATE}..={MAX_SAMPLERATE} Hz"
            )));
        }
        let decoders = match o.get("decoders") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| ConfigError::usage("decoders: expected strings"))
                        .and_then(Decoder::parse)
                })
                .collect::<R<Vec<_>>>()?,
            Some(_) => return Err(ConfigError::usage("decoders: expected an array of strings")),
        };
        if decoders.len() > 1 {
            return Err(ConfigError::unsupported(
                "at most one decoder per capture in the first slice",
            ));
        }
        let decoder = decoders.into_iter().next();
        let mask = match o.get("channels") {
            None | Some(Value::Null) => match (&decoder, source) {
                (Some(d), Source::Synthetic) => d.mask(),
                _ => 0xFF,
            },
            Some(Value::Array(a)) if !a.is_empty() && a.len() <= usize::from(CHANNELS) => {
                let mut m = 0u8;
                for v in a {
                    let c = chan(
                        v.as_str()
                            .ok_or_else(|| ConfigError::usage("channels: expected strings"))?,
                    )?;
                    if m & (1 << c) != 0 {
                        return Err(ConfigError::usage(format!("channel D{c} listed twice")));
                    }
                    m |= 1 << c;
                }
                m
            }
            Some(_) => return Err(ConfigError::usage("channels: expected 1..8 of D0..D7")),
        };
        let duration_ms = get_u64(o, "duration_ms")?.unwrap_or(DEFAULT_DURATION_MS);
        if !(MIN_DURATION_MS..=MAX_DURATION_MS).contains(&duration_ms) {
            return Err(ConfigError::usage(format!(
                "duration_ms {duration_ms}: expected {MIN_DURATION_MS}..={MAX_DURATION_MS}"
            )));
        }
        let max_samples = get_u64(o, "max_samples")?;
        if max_samples.is_some_and(|n| n == 0 || n > MAX_SAMPLES_LIMIT) {
            return Err(ConfigError::usage(format!(
                "max_samples: expected 1..={MAX_SAMPLES_LIMIT}"
            )));
        }
        let text = get_str(o, "text")?;
        let frame_period_ms = get_u64(o, "frame_period_ms")?;
        let clock_hz = get_u64(o, "clock_hz")?;
        if source != Source::Synthetic && (text.is_some() || frame_period_ms.is_some() || clock_hz.is_some())
        {
            return Err(ConfigError::usage(
                "text, frame_period_ms and clock_hz apply only to synthetic",
            ));
        }
        let text = text.unwrap_or(DEFAULT_TEXT).to_owned();
        if text.is_empty() || expand(&text, 9999).len() > MAX_TEXT_BYTES {
            return Err(ConfigError::usage(format!("text: 1..={MAX_TEXT_BYTES} bytes")));
        }
        let frame_period_ms = frame_period_ms.unwrap_or(DEFAULT_FRAME_PERIOD_MS);
        if !(1..=10_000).contains(&frame_period_ms) {
            return Err(ConfigError::usage("frame_period_ms: expected 1..=10000"));
        }
        let cfg = StartConfig {
            source,
            driver,
            conn,
            samplerate,
            mask,
            decoder,
            duration_ms,
            max_samples,
            pattern,
            text,
            frame_period_ms,
            clock_hz,
        };
        if source == Source::Synthetic {
            // Validates generator parameters up front.
            Synth::new(&cfg)?;
        }
        Ok(cfg)
    }

    /// Normalized params (round-trips through [`StartConfig::from_params`]).
    pub fn to_params(&self) -> Value {
        let mut v = json!({
            "source": self.source.as_str(),
            "samplerate": self.samplerate,
            "channels": mask_channels(self.mask),
            "decoders": self.decoder.iter().map(Decoder::canonical).collect::<Vec<_>>(),
            "duration_ms": self.duration_ms,
        });
        if self.source == Source::SigrokDevice {
            v["driver"] = json!(self.driver);
        }
        for (k, x) in [("conn", json!(self.conn)), ("pattern", json!(self.pattern))] {
            if !x.is_null() {
                v[k] = x;
            }
        }
        if let Some(n) = self.max_samples {
            v["max_samples"] = json!(n);
        }
        if self.source == Source::Synthetic {
            v["text"] = json!(self.text);
            v["frame_period_ms"] = json!(self.frame_period_ms);
            if let Some(c) = self.clock_hz {
                v["clock_hz"] = json!(c);
            }
        }
        v
    }

    /// Acquisition argv (without the program). `None` for synthetic.
    pub fn acquisition_args(&self) -> Option<Vec<String>> {
        if !self.source.acquires() {
            return None;
        }
        let mut d = self.driver.clone().unwrap_or_else(|| "demo".into());
        if let Some(c) = &self.conn {
            d.push_str(":conn=");
            d.push_str(c);
        }
        let mut a = vec!["-d".to_owned(), d];
        if let Some(p) = &self.pattern {
            a.extend(["-g".into(), "Logic".into(), "-c".into(), format!("pattern={p}")]);
        } else {
            a.extend(["-c".into(), format!("samplerate={}", self.samplerate)]);
        }
        a.extend(
            ["-C", ACQ_CHANNELS, "--continuous", "-O", "binary", "-l", "2"]
                .iter()
                .map(|s| (*s).to_owned()),
        );
        Some(a)
    }

    /// Decoder argv (without the program). `None` without a decoder.
    pub fn decoder_args(&self) -> Option<Vec<String>> {
        let d = self.decoder.as_ref()?;
        Some(vec![
            "-i".into(),
            "-".into(),
            "-I".into(),
            format!("binary:numchannels={CHANNELS}:samplerate={}", self.samplerate),
            "-P".into(),
            d.pd_arg(),
            "--protocol-decoder-samplenum".into(),
            "-l".into(),
            "2".into(),
            "-A".into(),
            d.annotation_filter(),
        ])
    }
}

/// `{n}` → zero-padded 4-digit frame counter.
pub fn expand(template: &str, n: u64) -> Vec<u8> {
    template
        .replace("{n}", &format!("{:04}", n % 10_000))
        .into_bytes()
}

/// Sample index → ns since capture start (sample clock).
pub fn sample_ns(sample: u64, samplerate: u64) -> u64 {
    let ns = u128::from(sample) * 1_000_000_000 / u128::from(samplerate.max(1));
    u64::try_from(ns).unwrap_or(u64::MAX)
}

// ---------------------------------------------------------------------------------------------
// Edge compression.

/// Accumulates raw 1-byte samples into one bounded `logic.edges` payload.
#[derive(Debug, Clone)]
pub struct EdgeAcc {
    mask: u8,
    /// Next absolute sample index.
    pub sample: u64,
    start: u64,
    initial: u8,
    state: u8,
    active: u8,
    edges: Vec<(u64, u8)>,
    dropped: u64,
    started: bool,
}

impl EdgeAcc {
    /// Accumulator for displayed `mask`.
    pub fn new(mask: u8) -> Self {
        EdgeAcc {
            mask,
            sample: 0,
            start: 0,
            initial: 0,
            state: 0,
            active: 0,
            edges: Vec::new(),
            dropped: 0,
            started: false,
        }
    }

    /// Feed samples.
    pub fn feed(&mut self, data: &[u8]) {
        for &b in data {
            let v = b & self.mask;
            if !self.started {
                self.started = true;
                self.initial = v;
                self.state = v;
                self.start = self.sample;
            } else if v != self.state {
                self.active |= v ^ self.state;
                if self.edges.len() < MAX_EDGES_PER_EVENT {
                    self.edges.push((self.sample, v));
                } else {
                    self.dropped += 1;
                }
                self.state = v;
            }
            self.sample += 1;
        }
    }

    /// Edges currently held.
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// No samples since the last take.
    pub fn is_empty(&self) -> bool {
        self.sample == self.start || !self.started
    }

    /// Take the payload for `[sample_start, sample_end)` and start the next range at the current
    /// state. Returns `(payload, edges, dropped)`; `None` when no samples arrived.
    pub fn take(&mut self, capture_id: &str, seq: u64, samplerate: u64) -> Option<(Value, u64, u64)> {
        if self.is_empty() {
            return None;
        }
        let n = self.edges.len() as u64;
        let dropped = self.dropped;
        let p = json!({
            "capture_id": capture_id,
            "seq": seq,
            "samplerate": samplerate,
            "sample_start": self.start,
            "sample_end": self.sample,
            "mask": self.mask,
            "initial": self.initial,
            "final": self.state,
            "active": self.active,
            "edges": self.edges.iter().map(|(s, v)| json!([s, v])).collect::<Vec<_>>(),
            "dropped": dropped,
        });
        self.edges.clear();
        self.dropped = 0;
        self.active = 0;
        self.initial = self.state;
        self.start = self.sample;
        Some((p, n, dropped))
    }
}

// ---------------------------------------------------------------------------------------------
// Annotation lines.

/// One parsed `--protocol-decoder-samplenum` line: `START-END ID: TEXT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// Decoder instance id as printed (`uart-1`); `None` for unparsed lines.
    pub decoder: Option<String>,
    /// Start sample.
    pub start: Option<u64>,
    /// End sample.
    pub end: Option<u64>,
    /// Text.
    pub text: String,
    /// Annotations lost (queue/rate bound) immediately before this one.
    pub lost_before: u64,
}

/// Parse one line (no trailing newline). Unparseable lines keep their text with no samples.
pub fn parse_annotation(line: &str) -> Annotation {
    let raw = || Annotation {
        decoder: None,
        start: None,
        end: None,
        text: line.to_owned(),
        lost_before: 0,
    };
    let Some((span, rest)) = line.split_once(' ') else {
        return raw();
    };
    let Some((a, b)) = span.split_once('-') else {
        return raw();
    };
    let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        return raw();
    };
    if b < a {
        return raw();
    }
    let Some((id, text)) = rest.split_once(": ") else {
        return raw();
    };
    if id.is_empty() || id.contains(' ') {
        return raw();
    }
    Annotation {
        decoder: Some(id.to_owned()),
        start: Some(a),
        end: Some(b),
        text: text.to_owned(),
        lost_before: 0,
    }
}

impl Annotation {
    /// Marker carrying only a trailing loss count (never emitted as a row).
    pub fn loss_marker(n: u64) -> Self {
        Annotation {
            decoder: None,
            start: None,
            end: None,
            text: String::new(),
            lost_before: n,
        }
    }

    /// True for [`Annotation::loss_marker`].
    pub fn is_loss_marker(&self) -> bool {
        self.decoder.is_none() && self.start.is_none() && self.text.is_empty() && self.lost_before > 0
    }

    /// Event row (class is `null`: sigrok-cli 0.7.2 does not print it; `_raw` for unparsed).
    pub fn to_row(&self, samplerate: u64) -> Value {
        match (self.start, self.end) {
            (Some(a), Some(b)) => json!({
                "decoder": self.decoder, "class": null,
                "start_sample": a, "end_sample": b,
                "t_start_ns": sample_ns(a, samplerate), "t_end_ns": sample_ns(b, samplerate),
                "text": self.text,
            }),
            _ => json!({"decoder": null, "class": "_raw", "start_sample": null, "end_sample": null,
                        "t_start_ns": null, "t_end_ns": null, "text": self.text}),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Synthetic waveform (run-length segments, never materialized per sample).

/// Deterministic synthetic waveform matching the configured decoder (UART 8N1 / SPI bytes),
/// one frame every `frame_period_ms`.
#[derive(Debug, Clone)]
pub struct Synth {
    kind: SynthKind,
    samplerate: u64,
    template: String,
    period: u64,
    frame: u64,
    segs: Vec<(u8, u64)>,
    seg: usize,
    left: u64,
}

#[derive(Debug, Clone)]
enum SynthKind {
    Uart { ch: u8, baud: u64 },
    Spi { s: Spi, half: u64 },
}

impl Synth {
    /// Generator for `cfg` (synthetic source with a decoder).
    pub fn new(cfg: &StartConfig) -> R<Synth> {
        let d = cfg.decoder.as_ref().ok_or_else(|| {
            ConfigError::usage("synthetic needs a decoder (uart or spi) to shape the waveform")
        })?;
        let kind = match d {
            Decoder::Uart(u) => {
                if u.data_bits != 8 || u.parity != "none" || u.stop_bits != "1" || !u.lsb_first {
                    return Err(ConfigError::unsupported(
                        "synthetic uart generates 8N1 LSB-first only",
                    ));
                }
                if cfg.samplerate / u64::from(u.baudrate) < 4 {
                    return Err(ConfigError::usage(
                        "synthetic uart needs samplerate ≥ 4 × baudrate",
                    ));
                }
                SynthKind::Uart {
                    ch: u.ch,
                    baud: u64::from(u.baudrate),
                }
            }
            Decoder::Spi(s) => {
                if s.wordsize != 8 {
                    return Err(ConfigError::unsupported(
                        "synthetic spi generates 8-bit words only",
                    ));
                }
                let clk = cfg.clock_hz.unwrap_or(cfg.samplerate / 10);
                // Range check before any arithmetic (no overflow / division by zero).
                if clk == 0 || clk > cfg.samplerate / 2 {
                    return Err(ConfigError::usage(
                        "synthetic spi needs 0 < clock_hz ≤ samplerate / 2",
                    ));
                }
                SynthKind::Spi {
                    s: s.clone(),
                    half: cfg.samplerate / clk / 2,
                }
            }
        };
        if cfg.clock_hz.is_some() && matches!(kind, SynthKind::Uart { .. }) {
            return Err(ConfigError::usage("clock_hz applies only to synthetic spi"));
        }
        let mut g = Synth {
            kind,
            samplerate: cfg.samplerate,
            template: cfg.text.clone(),
            period: (cfg.samplerate.saturating_mul(cfg.frame_period_ms) / 1000).max(1),
            frame: 0,
            segs: Vec::new(),
            seg: 0,
            left: 0,
        };
        g.build_frame();
        Ok(g)
    }

    fn idle(&self) -> u8 {
        match &self.kind {
            SynthKind::Uart { ch, .. } => 1 << ch,
            SynthKind::Spi { s, .. } => {
                let mut v = 0;
                if s.cpol == 1 {
                    v |= 1 << s.clk;
                }
                if let Some(cs) = s.cs
                    && s.cs_active_low
                {
                    v |= 1 << cs;
                }
                v
            }
        }
    }

    fn build_frame(&mut self) {
        let bytes = expand(&self.template, self.frame);
        self.frame += 1;
        let idle = self.idle();
        let mut segs: Vec<(u8, u64)> = Vec::new();
        let push = |v: u8, n: u64, segs: &mut Vec<(u8, u64)>| {
            if n == 0 {
                return;
            }
            match segs.last_mut() {
                Some((lv, ln)) if *lv == v => *ln += n,
                _ => segs.push((v, n)),
            }
        };
        let mut used = 0u64;
        match &self.kind {
            SynthKind::Uart { ch, baud } => {
                // Bit k starts at round(k * rate / baud) relative to the frame start.
                let (rate, baud) = (u128::from(self.samplerate), u128::from(*baud));
                let lead = u64::try_from(rate * 2 / baud).unwrap_or(0); // two idle bit-times
                push(idle, lead, &mut segs);
                used += lead;
                let mut k: u128 = 0;
                let edge = |k: u128| u64::try_from(k * rate / baud).unwrap_or(0);
                for b in bytes {
                    let mut bits = vec![0u8];
                    bits.extend((0..8).map(|i| (b >> i) & 1));
                    bits.push(1);
                    for bit in bits {
                        let n = edge(k + 1) - edge(k);
                        push(if bit == 1 { 1 << ch } else { 0 }, n, &mut segs);
                        used += n;
                        k += 1;
                    }
                }
            }
            SynthKind::Spi { s, half } => {
                let half = *half;
                let cs_on = |v: u8| match s.cs {
                    Some(cs) if s.cs_active_low => v & !(1 << cs),
                    Some(cs) => v | (1 << cs),
                    None => v,
                };
                let lead = 4 * half;
                push(idle, lead, &mut segs);
                push(cs_on(idle), 2 * half, &mut segs);
                used += lead + 2 * half;
                let first = s.cpol ^ s.cpha;
                for b in bytes {
                    for i in 0..8 {
                        let bit = if s.msb_first {
                            (b >> (7 - i)) & 1
                        } else {
                            (b >> i) & 1
                        };
                        let mut base = cs_on(0);
                        for line in [s.mosi, s.miso].into_iter().flatten() {
                            if bit == 1 {
                                base |= 1 << line;
                            }
                        }
                        for clk in [first, first ^ 1] {
                            push(base | (clk << s.clk), half, &mut segs);
                            used += half;
                        }
                    }
                }
                push(cs_on(idle), 2 * half, &mut segs);
                used += 2 * half;
            }
        }
        let rest = self.period.saturating_sub(used).max(1);
        push(idle, rest, &mut segs);
        self.segs = segs;
        self.seg = 0;
        self.left = self.segs.first().map_or(0, |s| s.1);
    }

    /// Append exactly `n` samples to `out`.
    pub fn fill(&mut self, out: &mut Vec<u8>, mut n: u64) {
        while n > 0 {
            if self.left == 0 {
                self.seg += 1;
                if self.seg >= self.segs.len() {
                    self.build_frame();
                } else {
                    self.left = self.segs[self.seg].1;
                }
                continue;
            }
            let take = self.left.min(n);
            let v = self.segs[self.seg].0;
            out.extend(std::iter::repeat_n(v, usize::try_from(take).unwrap_or(0)));
            self.left -= take;
            n -= take;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Engine discovery.

/// Locations searched after `PATH` (the daemon may run without a login PATH).
pub const ENGINE_FALLBACK_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

/// Resolve `sigrok-cli`: an explicit configured path (absolute, must be a file) or a PATH
/// search. Never falls back to anything else.
pub fn resolve_engine(configured: Option<&str>, path_env: Option<&str>) -> Result<PathBuf, String> {
    if let Some(c) = configured {
        let p = Path::new(c);
        if !p.is_absolute() {
            return Err(format!("[logic] sigrok_cli {c:?} must be an absolute path"));
        }
        return if p.is_file() {
            Ok(p.to_owned())
        } else {
            Err(format!("[logic] sigrok_cli {c:?} does not exist"))
        };
    }
    let dirs = path_env
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default();
    dirs.into_iter()
        .chain(ENGINE_FALLBACK_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join("sigrok-cli"))
        .find(|p| p.is_file())
        .ok_or_else(|| "sigrok-cli not found (install it, or set [logic] sigrok_cli in loom.toml)".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn start(p: Value) -> R<StartConfig> {
        StartConfig::from_params(&p)
    }

    #[test]
    fn strict_validation() {
        assert!(start(json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "x": 1})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["uart:rx=D8"]})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["uart:rx=D0;rm -rf"]})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["uart:rx=D0:baudrate=1e5"]})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["i2c:scl=D0"]})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["uart:rx=D0:tx=D1"]})).is_err());
        assert!(start(json!({"source": "synthetic", "decoders": ["spi:clk=D0:mosi=D0"]})).is_err());
        assert!(start(json!({"source": "sigrok-demo", "samplerate": 999})).is_err());
        assert!(start(json!({"source": "sigrok-demo", "samplerate": "1M"})).is_err());
        assert!(
            start(json!({"source": "sigrok-demo", "pattern": "graycode", "samplerate": 1_000_000})).is_err()
        );
        assert!(start(json!({"source": "sigrok-demo", "duration_ms": 50})).is_err());
        assert!(start(json!({"source": "sigrok-demo", "text": "x"})).is_err());
        assert!(
            start(json!({"source": "synthetic"})).is_err(),
            "synthetic needs a decoder"
        );
        let e = start(json!({"source": "sigrok-device", "driver": "saleae-logic-pro"})).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotImplemented);
        assert!(start(json!({"source": "sigrok-device", "driver": "demo:conn=x"})).is_err());
        assert!(start(json!({"source": "sigrok-device", "driver": "fx2lafw", "conn": "1.5:x=y"})).is_err());
        assert!(start(json!({"source": "sigrok-device", "driver": "fx2lafw", "conn": "1.5"})).is_ok());
        assert_eq!(
            start(json!({"source": "saleae-hla"})).unwrap_err().code,
            ErrorCode::NotImplemented
        );
    }

    #[test]
    fn argv_fixed_eight_channels() {
        let c = start(
            json!({"source": "sigrok-demo", "channels": ["D7", "D5"], "decoders": ["spi:clk=D0:mosi=D1"]}),
        )
        .unwrap();
        assert_eq!(c.mask, 0b1010_0000);
        let a = c.acquisition_args().unwrap();
        assert_eq!(
            a,
            [
                "-d",
                "demo",
                "-c",
                "samplerate=1000000",
                "-C",
                ACQ_CHANNELS,
                "--continuous",
                "-O",
                "binary",
                "-l",
                "2"
            ]
        );
        let d = c.decoder_args().unwrap();
        assert_eq!(d[3], "binary:numchannels=8:samplerate=1000000");
        assert_eq!(
            d[5],
            "spi:clk=0:mosi=1:cpol=0:cpha=0:wordsize=8:bitorder=msb-first:cs_polarity=active-low"
        );
        assert_eq!(d[10], "spi=mosi-data");
        let g = start(json!({"source": "sigrok-demo", "pattern": "graycode"})).unwrap();
        assert_eq!(g.samplerate, DEMO_PATTERN_SAMPLERATE);
        assert!(
            g.acquisition_args()
                .unwrap()
                .contains(&"pattern=graycode".to_owned())
        );
        // Normalized params round-trip.
        assert_eq!(StartConfig::from_params(&c.to_params()).unwrap(), c);
    }

    #[test]
    fn edges_are_bounded_and_absolute() {
        let mut e = EdgeAcc::new(0b0000_0011);
        e.feed(&[0, 0, 1, 1, 0b111, 2]);
        let (p, n, d) = e.take("lc", 0, 1000).unwrap();
        assert_eq!((n, d), (3, 0));
        assert_eq!(p["edges"], json!([[2, 1], [4, 3], [5, 2]]));
        assert_eq!(
            (p["sample_start"].as_u64(), p["sample_end"].as_u64()),
            (Some(0), Some(6))
        );
        e.feed(&[2, 0]);
        let (p, ..) = e.take("lc", 1, 1000).unwrap();
        assert_eq!((p["initial"].as_u64(), p["final"].as_u64()), (Some(2), Some(0)));
        assert_eq!(p["edges"], json!([[7, 0]]));
        let mut big = EdgeAcc::new(1);
        let toggles: Vec<u8> = (0..10_000u32).map(|i| (i & 1) as u8).collect();
        big.feed(&toggles);
        let (p, n, d) = big.take("lc", 0, 1000).unwrap();
        assert_eq!(n as usize, MAX_EDGES_PER_EVENT);
        assert_eq!(n + d, 9_999);
        assert_eq!(p["final"].as_u64(), Some(1));
        assert!(big.take("lc", 1, 1000).is_none());
    }

    #[test]
    fn annotation_lines() {
        let a = parse_annotation("210-290 uart-1: 55");
        assert_eq!(
            (a.decoder.as_deref(), a.start, a.end, a.text.as_str()),
            (Some("uart-1"), Some(210), Some(290), "55")
        );
        assert_eq!(a.to_row(1_000_000)["t_end_ns"], json!(290_000));
        assert!(a.to_row(1)["class"].is_null());
        let r = parse_annotation("garbage line");
        assert_eq!(r.to_row(1)["class"], json!("_raw"));
        assert_eq!(
            parse_annotation("300-200 uart-1: 55").to_row(1)["class"],
            json!("_raw")
        );
        assert_eq!(
            parse_annotation("18446744073709551616-1 uart-1: 55").to_row(1)["class"],
            json!("_raw")
        );
    }

    #[test]
    fn synth_spi_clock_bounds_never_panic() {
        let base =
            json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["spi:clk=D0:mosi=D1"]});
        for clk in [0u64, 500_001, 9_223_372_036_854_775_808, u64::MAX] {
            let mut p = base.clone();
            p["clock_hz"] = json!(clk);
            let e = start(p).unwrap_err();
            assert_eq!(e.code, ErrorCode::Usage, "clock_hz={clk}");
        }
        let mut ok = base.clone();
        ok["clock_hz"] = json!(500_000);
        assert!(start(ok).is_ok());
    }

    #[test]
    fn synth_uart_shape() {
        let c = start(json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:baudrate=100000"], "text": "U"})).unwrap();
        let mut g = Synth::new(&c).unwrap();
        let mut out = Vec::new();
        g.fill(&mut out, 120);
        // 2 idle bits (20), start bit low (10), then 0x55 LSB first: 1,0,1,0…
        assert!(out[..20].iter().all(|&v| v == 1));
        assert!(out[20..30].iter().all(|&v| v == 0));
        assert!(out[30..40].iter().all(|&v| v == 1));
        assert!(out[40..50].iter().all(|&v| v == 0));
        let mut more = Vec::new();
        g.fill(&mut more, 250_000);
        assert_eq!(more.len(), 250_000);
    }

    #[test]
    fn worst_case_edges_payload_fits() {
        // 10 MS/s × 1 h = 3.6e10 samples: 11-digit indices, every sample toggles all 8 bits.
        let mut e = EdgeAcc::new(0xFF);
        e.sample = MAX_SAMPLERATE * (MAX_DURATION_MS / 1000) - 10_000;
        let data: Vec<u8> = (0..8_000u32)
            .map(|i| if i % 2 == 0 { 0xFF } else { 0x00 })
            .collect();
        e.feed(&data);
        let (p, n, d) = e
            .take("lc-ffffffffffff-18446744073709551615", u64::MAX, MAX_SAMPLERATE)
            .unwrap();
        assert_eq!(n as usize, MAX_EDGES_PER_EVENT);
        assert!(d > 0);
        let bytes = serde_json::to_string(&p).unwrap().len();
        assert!(bytes < MAX_PAYLOAD_BYTES, "{bytes}");
    }
}
