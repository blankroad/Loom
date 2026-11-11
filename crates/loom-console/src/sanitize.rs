//! `sanitize` — render sanitizer: device bytes → bytes that are safe to hand to a
//! terminal emulator (UART plan §5.4, §5.5 ①, §5.15.2, §7.5).
//!
//! The event stream, journal and agent results keep the **original** bytes. This
//! filter runs only on the drawing side: the web panel (WASM build, right before
//! `term.write`) and the auxiliary TUI.
//!
//! Design (allowlist):
//! - Printable text, valid UTF-8 and BEL/BS/HT/LF/CR pass. Invalid UTF-8 becomes
//!   U+FFFD (so a wrong baud stays visible); C1 code points (U+0080..U+009F, 8-bit
//!   CSI/OSC/DCS) and every other C0 control are removed and counted.
//! - Escape sequences are collected by a small byte state machine (not `vte`: the
//!   collected bytes are copied **verbatim** when they pass, and the same rules are
//!   easy to port to TypeScript with the golden corpus in `tests/golden/sanitize`).
//! - Every terminal query whose reply would be typed back into the device (DA,
//!   DSR/CPR, DECRQM, DECRQSS, XTVERSION, window reports, kitty, colour queries) is
//!   removed — first layer of the plan §5.5 triple defence.
//! - Any one escape sequence / control string is bounded to [`MAX_SEQ_BYTES`]; past
//!   that the rest is discarded up to its terminator and `overlong` is counted.
//!   Control strings (OSC, DCS, SOS/PM/APC) are always dropped, so they are never
//!   buffered at all. State is fixed-size: memory does not grow with input.
//! - Splitting the same input at any point gives the same output and counters.
//!
//! No `std::time`, threads or files: this module builds for `wasm32` on its own.

mod csi;
mod utf8;

use std::collections::VecDeque;

use serde::Serialize;

use csi::Modes;
use utf8::{Step, Utf8};

/// Longest escape sequence / control string accepted (plan §7.5 parser cap,
/// §5.15.2 "escape 시퀀스 최대 4 KiB"). Longer ones are discarded and counted.
pub const MAX_SEQ_BYTES: usize = 4096;
/// Capacity of the recent-blocked ring returned by [`Sanitizer::take_recent`]
/// (plan §5.4 "클릭 시 최근 목록"). Oldest entries are overwritten.
pub const RECENT_CAP: usize = 64;
/// Bytes written by [`Sanitizer::on_gap`]: SGR reset (plan §3.11-4).
pub const GAP_RESET: &[u8] = b"\x1b[0m";
/// UTF-8 encoding of U+FFFD, written for each invalid byte sequence.
pub const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// Which renderer the output feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// xterm.js in the Loom UI panel: the status line is DOM, DECSTBM passes.
    Web,
    /// Auxiliary TUI: the last row is the status line, DECSTBM is clamped.
    Tui,
}

/// Sanitizer configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SanitizeConfig {
    /// Target renderer.
    pub profile: Profile,
    /// Pass mouse tracking modes (`?9`, `?1000`…`?1016`). M1: `false` (plan §5.4).
    pub allow_mouse: bool,
    /// Terminal rows for the TUI DECSTBM clamp (ignored by [`Profile::Web`]).
    pub rows: u16,
}

impl SanitizeConfig {
    /// Web panel defaults (M1): no mouse.
    pub const fn web() -> Self {
        Self {
            profile: Profile::Web,
            allow_mouse: false,
            rows: 0,
        }
    }

    /// TUI defaults for a terminal with `rows` rows (status line on the last row).
    pub const fn tui(rows: u16) -> Self {
        Self {
            profile: Profile::Tui,
            allow_mouse: false,
            rows,
        }
    }
}

/// Kind of a removed sequence or control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// Terminal query with an automatic reply (DA1/2/3, DSR/CPR, DECRQM, XTVERSION,
    /// DECREQTPARM, DECRQCRA, kitty `CSI ? u`, XTQMODKEYS …).
    Query,
    /// `CSI … t` window manipulation / report.
    WindowOp,
    /// OSC 0/1/2 title.
    Title,
    /// OSC 4/10/11/12/17/19 colour set or query.
    OscColor,
    /// OSC 8 hyperlink (link text itself still passes).
    Osc8,
    /// OSC 52 clipboard.
    Osc52,
    /// Any other OSC.
    OscOther,
    /// Mouse tracking mode (when `allow_mouse` is false).
    Mouse,
    /// Focus reporting `?1004`.
    Focus,
    /// Other set/reset mode not on the allowlist.
    Mode,
    /// DCS string (DECRQSS, XTGETTCAP, sixel, …).
    Dcs,
    /// SOS / PM / APC string.
    ApcPmSos,
    /// RIS `ESC c` (full reset).
    Ris,
    /// Other escape sequence not on the allowlist (or aborted).
    Esc,
    /// Other CSI not on the allowlist, malformed or aborted.
    Csi,
    /// C0 control outside BEL/BS/HT/LF/CR (ENQ, NUL, SO/SI, CAN, …) and DEL.
    C0,
    /// C1 control code point U+0080..U+009F.
    C1,
    /// Escape sequence or control string longer than [`MAX_SEQ_BYTES`].
    Overlong,
}

/// Per-kind block counters (monotonic since construction).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Counters {
    /// [`BlockKind::Query`].
    pub query: u64,
    /// [`BlockKind::WindowOp`].
    pub window_op: u64,
    /// [`BlockKind::Title`].
    pub title: u64,
    /// [`BlockKind::OscColor`].
    pub osc_color: u64,
    /// [`BlockKind::Osc8`].
    pub osc8: u64,
    /// [`BlockKind::Osc52`].
    pub osc52: u64,
    /// [`BlockKind::OscOther`].
    pub osc_other: u64,
    /// [`BlockKind::Mouse`].
    pub mouse: u64,
    /// [`BlockKind::Focus`].
    pub focus: u64,
    /// [`BlockKind::Mode`].
    pub mode: u64,
    /// [`BlockKind::Dcs`].
    pub dcs: u64,
    /// [`BlockKind::ApcPmSos`].
    pub apc_pm_sos: u64,
    /// [`BlockKind::Ris`].
    pub ris: u64,
    /// [`BlockKind::Esc`].
    pub esc: u64,
    /// [`BlockKind::Csi`].
    pub csi: u64,
    /// [`BlockKind::C0`].
    pub c0: u64,
    /// [`BlockKind::C1`].
    pub c1: u64,
    /// [`BlockKind::Overlong`].
    pub overlong: u64,
    /// Invalid UTF-8 sequences replaced by U+FFFD (not a block, shown for baud hints).
    pub utf8_replaced: u64,
}

impl Counters {
    /// Sum of all block counters (excluding `utf8_replaced`).
    pub fn total_blocked(&self) -> u64 {
        self.query
            + self.window_op
            + self.title
            + self.osc_color
            + self.osc8
            + self.osc52
            + self.osc_other
            + self.mouse
            + self.focus
            + self.mode
            + self.dcs
            + self.apc_pm_sos
            + self.ris
            + self.esc
            + self.csi
            + self.c0
            + self.c1
            + self.overlong
    }

    fn bump(&mut self, k: BlockKind) {
        let c = match k {
            BlockKind::Query => &mut self.query,
            BlockKind::WindowOp => &mut self.window_op,
            BlockKind::Title => &mut self.title,
            BlockKind::OscColor => &mut self.osc_color,
            BlockKind::Osc8 => &mut self.osc8,
            BlockKind::Osc52 => &mut self.osc52,
            BlockKind::OscOther => &mut self.osc_other,
            BlockKind::Mouse => &mut self.mouse,
            BlockKind::Focus => &mut self.focus,
            BlockKind::Mode => &mut self.mode,
            BlockKind::Dcs => &mut self.dcs,
            BlockKind::ApcPmSos => &mut self.apc_pm_sos,
            BlockKind::Ris => &mut self.ris,
            BlockKind::Esc => &mut self.esc,
            BlockKind::Csi => &mut self.csi,
            BlockKind::C0 => &mut self.c0,
            BlockKind::C1 => &mut self.c1,
            BlockKind::Overlong => &mut self.overlong,
        };
        *c = c.saturating_add(1);
    }
}

/// One removed item: kind and absolute byte offset (counted from the first byte ever
/// fed to this sanitizer) of the sequence start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Blocked {
    /// What was removed.
    pub kind: BlockKind,
    /// Offset of its first byte in the cumulative input.
    pub offset: u64,
}

/// Control string type (always dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrKind {
    Osc,
    Dcs,
    ApcPmSos,
}

/// Parser state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Text.
    Ground,
    /// After ESC; `buf` holds ESC + intermediates.
    Esc,
    /// Inside `ESC [`; `buf` holds the sequence so far.
    Csi,
    /// Overlong ESC/CSI: drop until its final byte.
    Discard { csi: bool },
    /// Control string; nothing is buffered.
    Str {
        kind: StrKind,
        len: usize,
        overlong: bool,
        after_esc: bool,
        /// OSC number being parsed (`None` once it is known not to be numeric).
        num: Option<u32>,
        num_done: bool,
        digits: u8,
    },
}

/// Streaming render sanitizer (fixed-size state).
#[derive(Debug, Clone)]
pub struct Sanitizer {
    cfg: SanitizeConfig,
    state: State,
    /// Collected ESC/CSI bytes, never longer than [`MAX_SEQ_BYTES`].
    buf: Vec<u8>,
    utf8: Utf8,
    modes: Modes,
    counters: Counters,
    recent: VecDeque<Blocked>,
    /// Absolute offset of the next input byte.
    offset: u64,
    /// Absolute offset where the current sequence started.
    seq_start: u64,
}

impl Sanitizer {
    /// New sanitizer in the ground state.
    pub fn new(cfg: SanitizeConfig) -> Self {
        Self {
            cfg,
            state: State::Ground,
            buf: Vec::with_capacity(MAX_SEQ_BYTES),
            utf8: Utf8::default(),
            modes: Modes::default(),
            counters: Counters::default(),
            recent: VecDeque::with_capacity(RECENT_CAP),
            offset: 0,
            seq_start: 0,
        }
    }

    /// Configuration in use.
    pub fn config(&self) -> &SanitizeConfig {
        &self.cfg
    }

    /// Change the TUI row count (terminal resize). Affects later DECSTBM only.
    pub fn set_rows(&mut self, rows: u16) {
        self.cfg.rows = rows;
    }

    /// Filter `input` and append the safe bytes to `out`.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            let off = self.offset;
            self.offset += 1;
            self.step(b, off, out);
        }
    }

    /// Gap or generation boundary: drop any partial sequence / UTF-8 and write an
    /// SGR reset so colours from before the gap do not leak (plan §3.11-4).
    /// Mirrored terminal modes (alternate screen, bracketed paste) are kept: they
    /// describe what was already sent to the terminal.
    pub fn on_gap(&mut self, out: &mut Vec<u8>) {
        self.state = State::Ground;
        self.buf.clear();
        self.utf8.reset();
        out.extend_from_slice(GAP_RESET);
    }

    /// Block counters since construction.
    pub fn counters(&self) -> &Counters {
        &self.counters
    }

    /// Recently removed items (≤ [`RECENT_CAP`], oldest first); clears the list.
    pub fn take_recent(&mut self) -> Vec<Blocked> {
        self.recent.drain(..).collect()
    }

    /// Alternate screen (`?47`, `?1047`, `?1049`) currently on in the terminal —
    /// InputArbiter's no-auto-return condition (plan §3.7-3).
    pub fn alt_screen(&self) -> bool {
        self.modes.alt_screen
    }

    /// Bracketed paste (`?2004`) currently on — panel paste confirmation (plan §5.3).
    pub fn bracketed_paste(&self) -> bool {
        self.modes.bracketed_paste
    }

    /// Bytes held between `feed` calls (partial sequence + partial UTF-8).
    /// Always ≤ [`MAX_SEQ_BYTES`] + 3.
    pub fn buffered_bytes(&self) -> usize {
        self.buf.len() + self.utf8.held()
    }

    fn block(&mut self, kind: BlockKind, offset: u64) {
        self.counters.bump(kind);
        if self.recent.len() == RECENT_CAP {
            self.recent.pop_front();
        }
        self.recent.push_back(Blocked { kind, offset });
    }

    fn step(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        match self.state {
            State::Ground => self.ground(b, off, out),
            State::Esc => self.esc(b, off, out),
            State::Csi => self.csi(b, off, out),
            State::Discard { csi } => self.discard(b, off, csi, out),
            State::Str { .. } => self.string(b, off, out),
        }
    }

    /// Start a new escape sequence at `off`.
    fn start_esc(&mut self, off: u64) {
        self.buf.clear();
        self.buf.push(0x1B);
        self.seq_start = off;
        self.state = State::Esc;
    }

    /// C0 control executed inside or outside a sequence (not ESC/CAN/SUB).
    fn c0(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        match b {
            0x07 | 0x08 | 0x09 | 0x0A | 0x0D => out.push(b),
            _ => self.block(BlockKind::C0, off),
        }
    }

    fn ground(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        if self.utf8.pending() || b >= 0x80 {
            match self.utf8.push(b) {
                Step::NeedMore => {}
                Step::Char { cp, bytes, len } => {
                    if (0x80..=0x9F).contains(&cp) {
                        let start = off + 1 - u64::from(len);
                        self.block(BlockKind::C1, start);
                    } else {
                        out.extend_from_slice(&bytes[..usize::from(len)]);
                    }
                }
                Step::Invalid { reprocess } => {
                    out.extend_from_slice(REPLACEMENT);
                    self.counters.utf8_replaced = self.counters.utf8_replaced.saturating_add(1);
                    if reprocess {
                        // Decoder is empty now; `b` is handled exactly once more.
                        self.ground(b, off, out);
                    }
                }
            }
            return;
        }
        match b {
            0x1B => self.start_esc(off),
            0x7F => self.block(BlockKind::C0, off),
            0x00..=0x1F => self.c0(b, off, out),
            _ => out.push(b),
        }
    }

    fn esc(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        match b {
            0x1B => self.start_esc(off),
            0x18 | 0x1A => {
                self.state = State::Ground;
                self.block(BlockKind::C0, off);
            }
            0x00..=0x1F => self.c0(b, off, out),
            0x7F => {}
            0x20..=0x2F => {
                if self.buf.len() >= MAX_SEQ_BYTES {
                    self.block(BlockKind::Overlong, self.seq_start);
                    self.state = State::Discard { csi: false };
                } else {
                    self.buf.push(b);
                }
            }
            b'[' if self.buf.len() == 1 => {
                self.buf.push(b);
                self.state = State::Csi;
            }
            b']' | b'P' | b'X' | b'^' | b'_' if self.buf.len() == 1 => {
                let kind = match b {
                    b']' => StrKind::Osc,
                    b'P' => StrKind::Dcs,
                    _ => StrKind::ApcPmSos,
                };
                self.buf.clear();
                self.state = State::Str {
                    kind,
                    len: 2,
                    overlong: false,
                    after_esc: false,
                    num: Some(0),
                    num_done: false,
                    digits: 0,
                };
            }
            0x30..=0x7E => {
                self.state = State::Ground;
                let inter = &self.buf[1..];
                let pass = match (inter, b) {
                    // DECSC/DECRC, IND, NEL, RI, keypad modes.
                    (b"", b'7' | b'8' | b'D' | b'E' | b'M' | b'=' | b'>') => true,
                    // G0/G1 charset: ASCII or DEC line drawing (menus, ncurses boxes).
                    (b"(" | b")", b'0' | b'B') => true,
                    _ => false,
                };
                if pass {
                    out.extend_from_slice(&self.buf);
                    out.push(b);
                } else if inter.is_empty() && b == b'c' {
                    self.block(BlockKind::Ris, self.seq_start);
                } else {
                    self.block(BlockKind::Esc, self.seq_start);
                }
                self.buf.clear();
            }
            _ => {
                // ≥ 0x80: abort, handle the byte as text.
                self.buf.clear();
                self.state = State::Ground;
                self.block(BlockKind::Esc, self.seq_start);
                self.ground(b, off, out);
            }
        }
    }

    fn csi(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        match b {
            0x1B => {
                self.block(BlockKind::Csi, self.seq_start);
                self.start_esc(off);
            }
            0x18 | 0x1A => {
                self.buf.clear();
                self.state = State::Ground;
                self.block(BlockKind::C0, off);
            }
            0x00..=0x1F => self.c0(b, off, out),
            0x7F => {}
            0x20..=0x7E => {
                if self.buf.len() >= MAX_SEQ_BYTES {
                    self.buf.clear();
                    self.block(BlockKind::Overlong, self.seq_start);
                    if (0x40..=0x7E).contains(&b) {
                        self.state = State::Ground;
                    } else {
                        self.state = State::Discard { csi: true };
                    }
                    return;
                }
                self.buf.push(b);
                if (0x40..=0x7E).contains(&b) {
                    self.state = State::Ground;
                    let start = self.seq_start;
                    let mut blocked: [Option<BlockKind>; 16] = [None; 16];
                    let mut n_blocked = 0usize;
                    let mut extra = 0u64;
                    {
                        let mut on_block = |k: BlockKind| {
                            if n_blocked < blocked.len() {
                                blocked[n_blocked] = Some(k);
                                n_blocked += 1;
                            } else {
                                extra += 1;
                            }
                        };
                        csi::filter(&self.buf, &self.cfg, &mut self.modes, out, &mut on_block);
                    }
                    for k in blocked.iter().flatten() {
                        self.block(*k, start);
                    }
                    for _ in 0..extra {
                        self.block(BlockKind::Mode, start);
                    }
                    self.buf.clear();
                }
            }
            _ => {
                self.buf.clear();
                self.state = State::Ground;
                self.block(BlockKind::Csi, self.seq_start);
                self.ground(b, off, out);
            }
        }
    }

    fn discard(&mut self, b: u8, off: u64, csi: bool, out: &mut Vec<u8>) {
        match b {
            0x1B => self.start_esc(off),
            0x18 | 0x1A => {
                self.state = State::Ground;
                self.block(BlockKind::C0, off);
            }
            0x00..=0x1F => self.c0(b, off, out),
            0x7F => {}
            0x40..=0x7E if csi => self.state = State::Ground,
            0x30..=0x7E if !csi => self.state = State::Ground,
            0x20..=0x7E => {}
            _ => {
                self.state = State::Ground;
                self.ground(b, off, out);
            }
        }
    }

    fn string(&mut self, b: u8, off: u64, out: &mut Vec<u8>) {
        let State::Str {
            kind,
            mut len,
            mut overlong,
            after_esc,
            mut num,
            mut num_done,
            mut digits,
        } = self.state
        else {
            return;
        };
        if after_esc {
            // `off - 1` was the ESC: ST if `\`, otherwise the string ended there and
            // a new escape sequence started at the ESC.
            self.finish_string(kind, num, digits);
            if b == b'\\' {
                self.state = State::Ground;
            } else {
                self.start_esc(off - 1);
                self.esc(b, off, out);
            }
            return;
        }
        match b {
            0x1B => {
                self.state = State::Str {
                    kind,
                    len,
                    overlong,
                    after_esc: true,
                    num,
                    num_done,
                    digits,
                };
                return;
            }
            0x07 if kind == StrKind::Osc => {
                self.finish_string(kind, num, digits);
                self.state = State::Ground;
                return;
            }
            0x18 | 0x1A => {
                self.finish_string(kind, num, digits);
                self.state = State::Ground;
                self.block(BlockKind::C0, off);
                return;
            }
            _ => {}
        }
        len = len.saturating_add(1);
        if len > MAX_SEQ_BYTES && !overlong {
            overlong = true;
            self.block(BlockKind::Overlong, self.seq_start);
        }
        if kind == StrKind::Osc && !num_done {
            if b.is_ascii_digit() && digits < 6 {
                digits += 1;
                num = num.map(|n| n * 10 + u32::from(b - b'0'));
            } else {
                if b != b';' || digits == 0 {
                    num = None;
                }
                num_done = true;
            }
        }
        self.state = State::Str {
            kind,
            len,
            overlong,
            after_esc: false,
            num,
            num_done,
            digits,
        };
    }

    /// Count a finished (terminated or aborted) control string.
    fn finish_string(&mut self, kind: StrKind, num: Option<u32>, digits: u8) {
        let k = match kind {
            StrKind::Dcs => BlockKind::Dcs,
            StrKind::ApcPmSos => BlockKind::ApcPmSos,
            StrKind::Osc => {
                // `ESC ] 0 BEL` ends before `;`: the number is still complete.
                let n = if digits == 0 { None } else { num };
                match n {
                    Some(0..=2) => BlockKind::Title,
                    Some(4 | 10 | 11 | 12 | 17 | 19) => BlockKind::OscColor,
                    Some(8) => BlockKind::Osc8,
                    Some(52) => BlockKind::Osc52,
                    _ => BlockKind::OscOther,
                }
            }
        };
        self.block(k, self.seq_start);
    }
}
