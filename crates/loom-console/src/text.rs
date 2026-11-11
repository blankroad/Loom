//! `text` — display text pipeline for agent `read` (UART plan §3.6, §7.5).
//!
//! Turns raw device bytes into [`Line`]s for the agent. Raw bytes and rx offsets are never
//! changed: every line carries the raw `[rx_offset_start, rx_offset_end)` range it came from.
//!
//! Stages (all incremental, all with fixed-size state, all fed one byte at a time so the
//! result does not depend on how the input was split into chunks):
//!
//! 1. [`AnsiFilter`] — classifies each byte as text or as part of an escape sequence
//!    (CSI/OSC/DCS/APC/PM/SOS/plain ESC). It never buffers sequence bytes, so its state is a few
//!    words no matter how long a sequence is (§3.13 "escape 4 KiB/채널").
//! 2. [`Utf8Decoder`] (or direct Latin-1 mapping) — holds at most 3 bytes of an incomplete
//!    UTF-8 character; definitely invalid bytes become `\xNN`.
//! 3. [`LineAssembler`] — splits on `\r\n`, `\n`, lone `\r`; collapses carriage-return
//!    overwrites (progress bars); caps a partial line at 64 KiB of raw bytes and the display
//!    text at 512 characters (`…[+N chars]`).
//! 4. [`BinaryDetector`] — flags `likely_binary_or_wrong_baud` when more than 30 % of the last
//!    4 KiB were non-printable.
//!
//! [`TextPipeline`] glues the stages together and resets them on gaps and generation changes.
//!
//! Display conventions (agent text only, never the raw stream):
//! - invalid UTF-8 byte, ASCII control (except TAB) and Latin-1 C1 byte → `\xNN` (lower-case hex);
//! - a C1 control that arrived as valid UTF-8 (U+0080..U+009F) → `\u{NN}`;
//! - a backslash printed by the device is *not* escaped (display only; use `--hex` for exact).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Longest escape sequence / control string tracked before it is declared overlong
/// (plan §7.5 "모든 escape/string 길이 4 KiB 제한", §3.13 텍스트 행 "escape 4 KiB/채널").
pub const MAX_ESCAPE_BYTES: usize = 4096;

/// Raw bytes in one partial line before it is force-broken and marked `truncated`
/// (plan §3.13 텍스트 행 "부분 줄 64 KiB").
pub const MAX_PARTIAL_LINE_BYTES: usize = 64 * 1024;

/// Smallest partial-line cap a caller may configure (keeps the force-break logic sane).
pub const MIN_PARTIAL_LINE_BYTES: usize = 16;

/// Display characters kept per line before `…[+N chars]` (plan §3.6 "한 줄 최대 512자").
pub const MAX_LINE_CHARS: u32 = 512;

/// Smallest per-line display cap a caller may configure.
pub const MIN_LINE_CHARS: u32 = 16;

/// Window of recent raw bytes the binary detector looks at (plan §3.6 "최근 창", task: 4 KiB).
pub const BINARY_WINDOW_BYTES: usize = 4096;

/// Non-printable share (percent) above which the window is flagged (plan §3.6 "> 30%").
pub const BINARY_THRESHOLD_PERCENT: usize = 30;

/// Bytes the detector must have seen before it reports anything (avoids flagging a single
/// stray control byte).
pub const BINARY_MIN_SAMPLE_BYTES: usize = 64;

const BEL: u8 = 0x07;
const LF: u8 = 0x0a;
const CR: u8 = 0x0d;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const ESC: u8 = 0x1b;
const DEL: u8 = 0x7f;

// ---------------------------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------------------------

/// Character encoding of the channel (plan §3.6 "인코딩", §7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    /// UTF-8 with incremental decoding; invalid bytes shown as `\xNN`.
    #[default]
    Utf8,
    /// ISO-8859-1: every byte maps directly to U+0000..U+00FF.
    Latin1,
}

/// How escape sequences appear in agent text (plan §3.6 `--ansi strip|keep|escape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnsiMode {
    /// Remove CSI/OSC/DCS/APC/PM/SOS and plain ESC sequences (default).
    #[default]
    Strip,
    /// Keep sequences verbatim (raw ESC in the text; non-ASCII sequence bytes as `\xNN`).
    Keep,
    /// Keep sequences but render ESC as `\e` and other sequence controls as `\xNN`.
    Escape,
}

/// Options of a [`TextPipeline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextOptions {
    /// Channel encoding.
    pub encoding: Encoding,
    /// Escape-sequence handling.
    pub ansi: AnsiMode,
    /// `true` = every lone-CR segment becomes its own line (`--keep-cr`); `false` = a run of
    /// CR overwrites collapses to its last state.
    pub keep_cr: bool,
    /// Display characters per line, clamped to `MIN_LINE_CHARS..=MAX_LINE_CHARS`.
    pub line_chars: u32,
    /// Raw bytes per partial line, clamped to `MIN_PARTIAL_LINE_BYTES..=MAX_PARTIAL_LINE_BYTES`.
    pub max_line_bytes: usize,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            encoding: Encoding::Utf8,
            ansi: AnsiMode::Strip,
            keep_cr: false,
            line_chars: MAX_LINE_CHARS,
            max_line_bytes: MAX_PARTIAL_LINE_BYTES,
        }
    }
}

impl TextOptions {
    fn clamped(mut self) -> Self {
        self.line_chars = self.line_chars.clamp(MIN_LINE_CHARS, MAX_LINE_CHARS);
        self.max_line_bytes = self
            .max_line_bytes
            .clamp(MIN_PARTIAL_LINE_BYTES, MAX_PARTIAL_LINE_BYTES);
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Line
// ---------------------------------------------------------------------------------------------

/// One display line for the agent. `seq` (stream seq) is assigned by the gateway in P1; this
/// crate only knows `source_seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// Display text (escaped, capped; ends with `…[+N chars]` when [`Line::elided_chars`] > 0).
    pub text: String,
    /// Raw rx offset of the first byte of the line.
    pub rx_offset_start: u64,
    /// Raw rx offset just past the line terminator (exclusive).
    pub rx_offset_end: u64,
    /// `source_seq` of the chunk that holds the first byte of the line.
    pub source_seq_first: u64,
    /// Port generation of the line.
    pub generation: u32,
    /// `true` when ended by a terminator (or a forced 64 KiB break); `false` for a line still
    /// in progress or cut by a gap / generation change.
    pub complete: bool,
    /// The raw line exceeded the partial-line cap and was force-broken here.
    pub truncated: bool,
    /// Display characters dropped by the per-line cap.
    pub elided_chars: u64,
}

// ---------------------------------------------------------------------------------------------
// AnsiFilter
// ---------------------------------------------------------------------------------------------

/// Classification of one raw byte by [`AnsiFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteClass {
    /// Ordinary data or a C0 control to execute (CR/LF inside a CSI still breaks lines).
    Text,
    /// Part of an escape sequence or control string.
    Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrKind {
    /// OSC: ends with BEL or ST.
    Osc,
    /// DCS/APC/PM/SOS: ends with ST.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Ground,
    Esc,
    EscIntermediate,
    Csi,
    Str(StrKind),
    StrEsc(StrKind),
    /// Overlong sequence: drop bytes until a terminator (ST/BEL/CAN/SUB/ESC) or a line feed.
    Discard,
}

/// Fixed-state escape-sequence classifier (plan §7.5). Never buffers sequence bytes.
///
/// An overlong (> [`MAX_ESCAPE_BYTES`]) sequence is counted and then discarded until a
/// terminator. Unlike the render sanitizer, agent text also resynchronises at the next LF so a
/// stray unterminated OSC on a noisy line cannot hide all later output.
///
/// Also unlike a terminal, CR and LF inside a pending ESC / CSI sequence abort that sequence
/// (they are still executed as line breaks). This makes every line boundary a clean resync
/// point, so a cursor that restarts decoding at a line end sees exactly what a one-shot decode
/// saw (a terminal would keep the sequence open and swallow the next byte after `ESC LF`).
#[derive(Debug, Clone)]
pub struct AnsiFilter {
    state: AnsiState,
    len: usize,
    overlong: u64,
}

impl Default for AnsiFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnsiFilter {
    /// New filter in the ground state.
    pub fn new() -> Self {
        Self {
            state: AnsiState::Ground,
            len: 0,
            overlong: 0,
        }
    }

    /// Back to the ground state (gap / generation boundary). Counters are kept.
    pub fn reset(&mut self) {
        self.state = AnsiState::Ground;
        self.len = 0;
    }

    /// `true` while inside an escape sequence or control string.
    pub fn in_sequence(&self) -> bool {
        self.state != AnsiState::Ground
    }

    /// Number of sequences that exceeded [`MAX_ESCAPE_BYTES`].
    pub fn overlong_count(&self) -> u64 {
        self.overlong
    }

    /// Classify one byte and advance the state machine.
    pub fn classify(&mut self, b: u8) -> ByteClass {
        let class = self.step(b);
        if class == ByteClass::Sequence && self.state != AnsiState::Ground {
            self.len = self.len.saturating_add(1);
            if self.len > MAX_ESCAPE_BYTES && self.state != AnsiState::Discard {
                self.overlong = self.overlong.saturating_add(1);
                self.state = AnsiState::Discard;
            }
        }
        class
    }

    fn enter(&mut self, s: AnsiState) {
        self.state = s;
        if s == AnsiState::Ground {
            self.len = 0;
        }
    }

    fn start_escape(&mut self) {
        self.state = AnsiState::Esc;
        self.len = 0;
    }

    fn step(&mut self, b: u8) -> ByteClass {
        use AnsiState as S;
        match self.state {
            S::Ground => {
                if b == ESC {
                    self.start_escape();
                    ByteClass::Sequence
                } else {
                    ByteClass::Text
                }
            }
            S::Esc | S::EscIntermediate => match b {
                ESC => {
                    self.start_escape();
                    ByteClass::Sequence
                }
                CR | LF => {
                    // Line boundaries are resync points (see type docs).
                    self.enter(S::Ground);
                    ByteClass::Text
                }
                CAN | SUB => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                0x00..=0x1f => ByteClass::Text,
                b'[' if self.state == S::Esc => {
                    self.enter(S::Csi);
                    ByteClass::Sequence
                }
                b']' if self.state == S::Esc => {
                    self.enter(S::Str(StrKind::Osc));
                    ByteClass::Sequence
                }
                b'P' | b'_' | b'^' | b'X' if self.state == S::Esc => {
                    self.enter(S::Str(StrKind::Other));
                    ByteClass::Sequence
                }
                0x20..=0x2f => {
                    self.enter(S::EscIntermediate);
                    ByteClass::Sequence
                }
                0x30..=0x7e => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                DEL => ByteClass::Sequence,
                _ => {
                    // Non-ASCII aborts the sequence; the byte is data.
                    self.enter(S::Ground);
                    ByteClass::Text
                }
            },
            S::Csi => match b {
                ESC => {
                    self.start_escape();
                    ByteClass::Sequence
                }
                CR | LF => {
                    self.enter(S::Ground);
                    ByteClass::Text
                }
                CAN | SUB => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                0x00..=0x1f => ByteClass::Text,
                0x20..=0x3f | DEL => ByteClass::Sequence,
                0x40..=0x7e => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                _ => {
                    self.enter(S::Ground);
                    ByteClass::Text
                }
            },
            S::Str(kind) => match b {
                BEL if kind == StrKind::Osc => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                ESC => {
                    self.state = S::StrEsc(kind);
                    ByteClass::Sequence
                }
                CAN | SUB => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                _ => ByteClass::Sequence,
            },
            S::StrEsc(kind) => match b {
                b'\\' => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                ESC => {
                    self.state = S::StrEsc(kind);
                    ByteClass::Sequence
                }
                _ => {
                    // The ESC ended the string and starts a new sequence; re-run `b` there.
                    self.start_escape();
                    self.step(b)
                }
            },
            S::Discard => match b {
                ESC => {
                    self.start_escape();
                    ByteClass::Sequence
                }
                BEL | CAN | SUB => {
                    self.enter(S::Ground);
                    ByteClass::Sequence
                }
                LF => {
                    self.enter(S::Ground);
                    ByteClass::Text
                }
                _ => ByteClass::Sequence,
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Utf8Decoder
// ---------------------------------------------------------------------------------------------

/// One decoded unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded {
    /// A complete character.
    Char(char),
    /// A byte that can never be part of valid UTF-8 at this position.
    Invalid(u8),
}

/// Incremental UTF-8 decoder (plan §7.5): holds at most 3 bytes of an incomplete character.
///
/// Decoding is byte-by-byte with the exact well-formedness table of the Unicode standard, so the
/// output is identical however the input is split (equivalent to `std::str::from_utf8` on the
/// valid prefix plus `\xNN` for each invalid byte).
#[derive(Debug, Clone, Default)]
pub struct Utf8Decoder {
    buf: [u8; 4],
    len: u8,
    need: u8,
}

impl Utf8Decoder {
    /// New, empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes of the incomplete character currently held (≤ 3).
    pub fn pending(&self) -> &[u8] {
        &self.buf[..usize::from(self.len)]
    }

    /// Drop the incomplete character, reporting each held byte as invalid.
    pub fn flush(&mut self, out: &mut impl FnMut(Decoded)) {
        for &b in &self.buf[..usize::from(self.len)] {
            out(Decoded::Invalid(b));
        }
        self.len = 0;
        self.need = 0;
    }

    /// Decode a slice.
    pub fn decode(&mut self, bytes: &[u8], out: &mut impl FnMut(Decoded)) {
        for &b in bytes {
            self.push(b, out);
        }
    }

    /// Push one byte.
    pub fn push(&mut self, b: u8, out: &mut impl FnMut(Decoded)) {
        if self.len > 0 {
            if self.continuation_ok(b) {
                self.buf[usize::from(self.len)] = b;
                self.len += 1;
                if self.len == self.need {
                    let n = usize::from(self.len);
                    let ch = std::str::from_utf8(&self.buf[..n])
                        .ok()
                        .and_then(|s| s.chars().next());
                    match ch {
                        Some(c) => {
                            out(Decoded::Char(c));
                            self.len = 0;
                            self.need = 0;
                        }
                        // Unreachable with the table above; stay safe anyway.
                        None => self.flush(out),
                    }
                }
                return;
            }
            self.flush(out);
        }
        match b {
            0x00..=0x7f => out(Decoded::Char(char::from(b))),
            0xc2..=0xdf => self.start(b, 2),
            0xe0..=0xef => self.start(b, 3),
            0xf0..=0xf4 => self.start(b, 4),
            _ => out(Decoded::Invalid(b)),
        }
    }

    fn start(&mut self, b: u8, need: u8) {
        self.buf[0] = b;
        self.len = 1;
        self.need = need;
    }

    fn continuation_ok(&self, b: u8) -> bool {
        if self.len == 1 {
            match self.buf[0] {
                0xe0 => (0xa0..=0xbf).contains(&b),
                0xed => (0x80..=0x9f).contains(&b),
                0xf0 => (0x90..=0xbf).contains(&b),
                0xf4 => (0x80..=0x8f).contains(&b),
                _ => (0x80..=0xbf).contains(&b),
            }
        } else {
            (0x80..=0xbf).contains(&b)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// BinaryDetector
// ---------------------------------------------------------------------------------------------

const BINARY_WORDS: usize = BINARY_WINDOW_BYTES / 64;

/// Sliding-window non-printable ratio (plan §3.6 `likely_binary_or_wrong_baud`). Fixed 512 B
/// bitmap state.
#[derive(Debug, Clone)]
pub struct BinaryDetector {
    bits: [u64; BINARY_WORDS],
    pos: usize,
    filled: usize,
    bad: usize,
}

impl Default for BinaryDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryDetector {
    /// New, empty detector.
    pub fn new() -> Self {
        Self {
            bits: [0; BINARY_WORDS],
            pos: 0,
            filled: 0,
            bad: 0,
        }
    }

    /// Forget the window.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Record one raw byte: `bad` = non-printable / undecodable.
    pub fn record(&mut self, bad: bool) {
        let (w, bit) = (self.pos / 64, 1u64 << (self.pos % 64));
        let was_bad = self.bits[w] & bit != 0;
        if self.filled == BINARY_WINDOW_BYTES {
            if was_bad {
                self.bad -= 1;
            }
        } else {
            self.filled += 1;
        }
        if bad {
            self.bits[w] |= bit;
            self.bad += 1;
        } else {
            self.bits[w] &= !bit;
        }
        self.pos = (self.pos + 1) % BINARY_WINDOW_BYTES;
    }

    /// Record `n` bytes with the same classification.
    pub fn record_n(&mut self, n: usize, bad: bool) {
        for _ in 0..n {
            self.record(bad);
        }
    }

    /// Bytes in the window and how many were non-printable.
    pub fn counts(&self) -> (usize, usize) {
        (self.filled, self.bad)
    }

    /// `true` when the window looks like binary data or a wrong baud rate.
    pub fn likely_binary(&self) -> bool {
        self.filled >= BINARY_MIN_SAMPLE_BYTES && self.bad * 100 > self.filled * BINARY_THRESHOLD_PERCENT
    }
}

// ---------------------------------------------------------------------------------------------
// LineAssembler
// ---------------------------------------------------------------------------------------------

/// One overwrite segment of a line: capped display text plus elided character count.
#[derive(Debug, Clone, Default)]
struct Seg {
    text: String,
    chars: u32,
    elided: u64,
}

impl Seg {
    fn is_empty(&self) -> bool {
        self.chars == 0 && self.elided == 0
    }

    fn clear(&mut self) {
        self.text.clear();
        self.chars = 0;
        self.elided = 0;
    }

    fn fits(&mut self, n: u32, cap: u32) -> bool {
        if self.elided == 0 && self.chars + n <= cap {
            self.chars += n;
            true
        } else {
            self.elided += u64::from(n);
            false
        }
    }

    fn push_char(&mut self, c: char, cap: u32) {
        if self.fits(1, cap) {
            self.text.push(c);
        }
    }

    fn push_hex(&mut self, b: u8, cap: u32) {
        if self.fits(4, cap) {
            let _ = write!(self.text, "\\x{b:02x}");
        }
    }

    fn push_unicode_escape(&mut self, c: char, cap: u32) {
        let s = format!("\\u{{{:x}}}", u32::from(c));
        let n = u32::try_from(s.chars().count()).unwrap_or(u32::MAX);
        if self.fits(n, cap) {
            self.text.push_str(&s);
        }
    }

    fn push_ascii(&mut self, s: &str, cap: u32) {
        let n = u32::try_from(s.len()).unwrap_or(u32::MAX);
        if self.fits(n, cap) {
            self.text.push_str(s);
        }
    }

    fn render(&self) -> (String, u64) {
        let mut t = self.text.clone();
        if self.elided > 0 {
            let _ = write!(t, "…[+{} chars]", self.elided);
        }
        (t, self.elided)
    }
}

/// Display unit handed to [`LineAssembler::unit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Printable character (or TAB).
    Char(char),
    /// Render as `\xNN`.
    Hex(u8),
    /// Render as `\u{NN}`.
    Unicode(char),
    /// Render as `\e`.
    EscapeE,
}

/// Splits display units into [`Line`]s (plan §3.6 줄 분리).
///
/// Terminators: `\n`, `\r\n` (any run of `\r` followed by `\n`), lone `\r`.
/// With `keep_cr = false` a lone `\r` does not end the line: it starts a new overwrite segment
/// and the line finally shows the **last non-empty segment** (segment-wise, not column-wise
/// terminal merge: `abc\rX\n` → `X`). With `keep_cr = true` each lone-`\r` segment is its own
/// line.
#[derive(Debug, Clone)]
pub struct LineAssembler {
    keep_cr: bool,
    line_chars: u32,
    max_line_bytes: usize,
    cur: Seg,
    last_nonempty: Seg,
    started: bool,
    start: u64,
    start_seq: u64,
    generation: u32,
    cr_pending: bool,
    cr_end: u64,
    after_cr_seq: Option<u64>,
    truncated_lines: u64,
}

impl LineAssembler {
    /// New assembler. `line_chars` / `max_line_bytes` are clamped to the module limits.
    pub fn new(keep_cr: bool, line_chars: u32, max_line_bytes: usize) -> Self {
        Self {
            keep_cr,
            line_chars: line_chars.clamp(MIN_LINE_CHARS, MAX_LINE_CHARS),
            max_line_bytes: max_line_bytes.clamp(MIN_PARTIAL_LINE_BYTES, MAX_PARTIAL_LINE_BYTES),
            cur: Seg::default(),
            last_nonempty: Seg::default(),
            started: false,
            start: 0,
            start_seq: 0,
            generation: 0,
            cr_pending: false,
            cr_end: 0,
            after_cr_seq: None,
            truncated_lines: 0,
        }
    }

    /// Lines force-broken by the partial-line cap so far.
    pub fn truncated_lines(&self) -> u64 {
        self.truncated_lines
    }

    /// Must be called for every raw byte, before its units, with its offset and chunk seq.
    pub fn note_byte(&mut self, offset: u64, source_seq: u64, generation: u32, is_cr: bool) {
        if !self.started {
            self.started = true;
            self.start = offset;
            self.start_seq = source_seq;
            self.generation = generation;
        } else if self.cr_pending && !is_cr && self.after_cr_seq.is_none() {
            self.after_cr_seq = Some(source_seq);
        }
    }

    /// A carriage return at `offset`.
    pub fn cr(&mut self, offset: u64) {
        self.cr_pending = true;
        self.cr_end = offset + 1;
    }

    /// A line feed at `offset`; returns the completed line.
    pub fn lf(&mut self, offset: u64) -> Option<Line> {
        self.cr_pending = false;
        self.after_cr_seq = None;
        Some(self.finish(offset + 1, true, false))
    }

    /// A display unit; may complete a lone-CR line first (`keep_cr = true`).
    pub fn unit(&mut self, u: Unit) -> Option<Line> {
        let done = self.resolve_bare_cr();
        let cap = self.line_chars;
        match u {
            Unit::Char(c) => self.cur.push_char(c, cap),
            Unit::Hex(b) => self.cur.push_hex(b, cap),
            Unit::Unicode(c) => self.cur.push_unicode_escape(c, cap),
            Unit::EscapeE => self.cur.push_ascii("\\e", cap),
        }
        done
    }

    /// Force-break the line if it reached the raw cap; `next_offset` = offset after the byte
    /// just processed.
    pub fn check_limit(&mut self, next_offset: u64) -> Option<Line> {
        if self.started && next_offset - self.start >= self.max_line_bytes as u64 {
            self.cr_pending = false;
            self.after_cr_seq = None;
            self.truncated_lines += 1;
            return Some(self.finish(next_offset, true, true));
        }
        None
    }

    /// Snapshot of the line in progress (`complete = false`), if any byte belongs to it.
    pub fn partial(&self, end: u64) -> Option<Line> {
        if !self.started {
            return None;
        }
        let (text, elided) = self.current_text().render();
        Some(Line {
            text,
            rx_offset_start: self.start,
            rx_offset_end: end,
            source_seq_first: self.start_seq,
            generation: self.generation,
            complete: false,
            truncated: false,
            elided_chars: elided,
        })
    }

    /// End the current line at a gap / generation boundary. A pending lone CR completes it;
    /// otherwise it is emitted with `complete = false`.
    pub fn cut(&mut self, end: u64) -> Option<Line> {
        if !self.started {
            return None;
        }
        let complete = self.cr_pending;
        let end = if complete { self.cr_end } else { end };
        self.cr_pending = false;
        self.after_cr_seq = None;
        Some(self.finish(end, complete, false))
    }

    fn current_text(&self) -> &Seg {
        if self.keep_cr || !self.cur.is_empty() {
            &self.cur
        } else {
            &self.last_nonempty
        }
    }

    fn resolve_bare_cr(&mut self) -> Option<Line> {
        if !self.cr_pending {
            return None;
        }
        self.cr_pending = false;
        let next_seq = self.after_cr_seq.take();
        if self.keep_cr {
            let cr_end = self.cr_end;
            let gen_ = self.generation;
            let line = self.finish(cr_end, true, false);
            self.started = true;
            self.start = cr_end;
            self.start_seq = next_seq.unwrap_or(line.source_seq_first);
            self.generation = gen_;
            Some(line)
        } else {
            if !self.cur.is_empty() {
                std::mem::swap(&mut self.last_nonempty, &mut self.cur);
            }
            self.cur.clear();
            None
        }
    }

    fn finish(&mut self, end: u64, complete: bool, truncated: bool) -> Line {
        let (text, elided) = self.current_text().render();
        let line = Line {
            text,
            rx_offset_start: self.start,
            rx_offset_end: end,
            source_seq_first: self.start_seq,
            generation: self.generation,
            complete,
            truncated,
            elided_chars: elided,
        };
        self.cur.clear();
        self.last_nonempty.clear();
        self.started = false;
        line
    }
}

// ---------------------------------------------------------------------------------------------
// TextPipeline
// ---------------------------------------------------------------------------------------------

/// Counters of a [`TextPipeline`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextCounters {
    /// Escape sequences longer than [`MAX_ESCAPE_BYTES`].
    pub overlong_escapes: u64,
    /// Bytes shown as `\xNN` because they were not valid in the encoding.
    pub invalid_bytes: u64,
    /// Lines force-broken at the partial-line cap.
    pub truncated_lines: u64,
    /// Resets (explicit, gap, generation change or offset discontinuity).
    pub resets: u64,
}

/// Incremental bytes → [`Line`] pipeline with fixed-size state (plan §3.6, §7.5).
#[derive(Debug, Clone)]
pub struct TextPipeline {
    opts: TextOptions,
    ansi: AnsiFilter,
    utf8: Utf8Decoder,
    lines: LineAssembler,
    binary: BinaryDetector,
    next_offset: Option<u64>,
    generation: u32,
    invalid_bytes: u64,
    resets: u64,
}

impl TextPipeline {
    /// New pipeline (options are clamped to the module limits).
    pub fn new(opts: TextOptions) -> Self {
        let opts = opts.clamped();
        Self {
            opts,
            ansi: AnsiFilter::new(),
            utf8: Utf8Decoder::new(),
            lines: LineAssembler::new(opts.keep_cr, opts.line_chars, opts.max_line_bytes),
            binary: BinaryDetector::new(),
            next_offset: None,
            generation: 0,
            invalid_bytes: 0,
            resets: 0,
        }
    }

    /// Effective (clamped) options.
    pub fn options(&self) -> &TextOptions {
        &self.opts
    }

    /// Offset just past the last byte fed, if any.
    pub fn next_offset(&self) -> Option<u64> {
        self.next_offset
    }

    /// `likely_binary_or_wrong_baud` for the recent window.
    pub fn binary_hint(&self) -> bool {
        self.binary.likely_binary()
    }

    /// Counters so far.
    pub fn counters(&self) -> TextCounters {
        TextCounters {
            overlong_escapes: self.ansi.overlong_count(),
            invalid_bytes: self.invalid_bytes,
            truncated_lines: self.lines.truncated_lines(),
            resets: self.resets,
        }
    }

    /// The line in progress (`complete = false`), without consuming it.
    pub fn partial(&self) -> Option<Line> {
        self.lines.partial(self.next_offset.unwrap_or(0))
    }

    /// Resynchronise at a gap or generation boundary: held UTF-8 bytes are shown as `\xNN`,
    /// the line in progress is emitted (`complete = false` unless a lone CR ended it), and all
    /// decoder / escape / binary state is cleared.
    pub fn reset(&mut self, out: &mut Vec<Line>) {
        if let Some(end) = self.next_offset {
            self.flush_utf8(out);
            if let Some(l) = self.lines.cut(end) {
                out.push(l);
            }
        }
        self.ansi.reset();
        self.utf8 = Utf8Decoder::new();
        self.binary.reset();
        self.next_offset = None;
        self.resets += 1;
    }

    /// Feed raw bytes starting at `rx_offset` from chunk `source_seq` of `generation`.
    /// Completed lines are appended to `out`. A jump in `rx_offset` or a new `generation`
    /// resets the pipeline first (a match/line never spans a hole).
    pub fn feed(
        &mut self,
        rx_offset: u64,
        source_seq: u64,
        generation: u32,
        bytes: &[u8],
        out: &mut Vec<Line>,
    ) {
        if let Some(n) = self.next_offset
            && (n != rx_offset || generation != self.generation)
        {
            self.reset(out);
        }
        self.generation = generation;
        for (i, &b) in bytes.iter().enumerate() {
            let off = rx_offset + i as u64;
            self.lines.note_byte(off, source_seq, generation, b == CR);
            match self.ansi.classify(b) {
                ByteClass::Sequence => {
                    self.flush_utf8(out);
                    self.binary.record(false);
                    self.sequence_byte(b, out);
                }
                ByteClass::Text => self.text_byte(b, off, out),
            }
            if let Some(l) = self.lines.check_limit(off + 1) {
                out.push(l);
            }
            self.next_offset = Some(off + 1);
        }
    }

    fn push_unit(&mut self, u: Unit, out: &mut Vec<Line>) {
        if let Some(l) = self.lines.unit(u) {
            out.push(l);
        }
    }

    fn sequence_byte(&mut self, b: u8, out: &mut Vec<Line>) {
        let u = match self.opts.ansi {
            AnsiMode::Strip => return,
            AnsiMode::Keep if b.is_ascii() => Unit::Char(char::from(b)),
            AnsiMode::Escape if b == ESC => Unit::EscapeE,
            AnsiMode::Escape if (0x20..0x7f).contains(&b) => Unit::Char(char::from(b)),
            _ => Unit::Hex(b),
        };
        self.push_unit(u, out);
    }

    fn flush_utf8(&mut self, out: &mut Vec<Line>) {
        let mut held = [0u8; 4];
        let n = self.utf8.pending().len();
        held[..n].copy_from_slice(self.utf8.pending());
        self.utf8.flush(&mut |_| {});
        for &b in &held[..n] {
            self.invalid_bytes += 1;
            self.binary.record(true);
            self.push_unit(Unit::Hex(b), out);
        }
    }

    fn text_byte(&mut self, b: u8, off: u64, out: &mut Vec<Line>) {
        match self.opts.encoding {
            Encoding::Latin1 => self.decoded(Decoded::Char(char::from(b)), off, true, out),
            Encoding::Utf8 => {
                let mut units = [Decoded::Invalid(0); 5];
                let mut n = 0;
                self.utf8.push(b, &mut |d| {
                    if n < units.len() {
                        units[n] = d;
                        n += 1;
                    }
                });
                for &d in &units[..n] {
                    self.decoded(d, off, false, out);
                }
            }
        }
    }

    fn decoded(&mut self, d: Decoded, off: u64, latin1: bool, out: &mut Vec<Line>) {
        match d {
            Decoded::Invalid(b) => {
                self.invalid_bytes += 1;
                self.binary.record(true);
                self.push_unit(Unit::Hex(b), out);
            }
            Decoded::Char('\r') => {
                self.binary.record(false);
                self.lines.cr(off);
            }
            Decoded::Char('\n') => {
                self.binary.record(false);
                if let Some(l) = self.lines.lf(off) {
                    out.push(l);
                }
            }
            Decoded::Char('\t') => {
                self.binary.record(false);
                self.push_unit(Unit::Char('\t'), out);
            }
            Decoded::Char(c) if c.is_control() => {
                let n = if latin1 { 1 } else { c.len_utf8() };
                self.binary.record_n(n, true);
                let code = u32::from(c);
                let u = match u8::try_from(code) {
                    Ok(byte) if code < 0x80 || latin1 => Unit::Hex(byte),
                    _ => Unit::Unicode(c),
                };
                self.push_unit(u, out);
            }
            Decoded::Char(c) => {
                let n = if latin1 { 1 } else { c.len_utf8() };
                self.binary.record_n(n, false);
                self.push_unit(Unit::Char(c), out);
            }
        }
    }
}

/// Convenience: run `bytes` through a fresh pipeline and return all complete lines plus the
/// trailing partial line (if any). Intended for tests and one-shot rendering.
pub fn render_lines(opts: TextOptions, rx_offset: u64, bytes: &[u8]) -> Vec<Line> {
    let mut p = TextPipeline::new(opts);
    let mut out = Vec::new();
    p.feed(rx_offset, 0, 0, bytes, &mut out);
    out.extend(p.partial());
    out
}
