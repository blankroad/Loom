//! `matcher` — bounded expect matcher (UART plan §7.4, §4.5, §3.13 "expect" row).
//!
//! One [`Matcher`] per channel keeps a single shared window (64 KiB) of recent RX bytes; waiters
//! never copy it. Each waiter has up to 8 patterns (8 compiled patterns per channel in total)
//! and fires once, then is removed.
//!
//! - Literal patterns use [`memchr::memmem::Finder`]; each search re-checks only the last
//!   `needle_len − 1` already-scanned bytes, so a literal split across chunks is never missed.
//! - Regex patterns use Rust `regex::bytes` with `size_limit`/`dfa_size_limit` 256 KiB and
//!   `nest_limit` 64, and run one `find` over the waiter's part of the window per feed step
//!   (O(m × n), linear in the window). Matches longer than the window are not promised (§4.5).
//! - [`Pattern::Prompt`] is a profile prompt regex anchored at the end of the received data
//!   (`(?:p)\z`): it matches only while the prompt is the last thing the device printed.
//!
//! The matcher works on raw bytes. With `strip_ansi` (default) escape sequences are removed
//! first with the same [`AnsiFilter`] the agent text uses; hit offsets are always raw RX
//! offsets. A hit is reported at most once per waiter, earliest start first, ties by pattern
//! index.
//!
//! Window semantics: a waiter whose `from_rx_offset` is older than the window starts at the
//! window start (re-scanning the ring is P1). [`Matcher::reset`] (gap / new generation) empties
//! the window so bytes before and after a boundary never join into one match.
//!
//! Error mapping for P1: `PatternTooLong`, `Syntax`, `EmptyPattern`, `DuplicateWaiter` →
//! `E_USAGE` (exit 2); `CompileTooBig`, `TooManyPatterns`, `TooManyWaiters` → `E_SCAN_LIMIT`
//! (exit 8).

use std::collections::VecDeque;

use memchr::memmem::Finder;
use regex::bytes::{Regex, RegexBuilder};

use crate::text::{AnsiFilter, ByteClass};

/// Pattern source bytes (plan §7.4 "패턴 UTF-8 2 KiB").
pub const MAX_PATTERN_BYTES: usize = 2048;

/// Regex compiled-size limit (plan §7.4 `size_limit(256 KiB)`).
pub const REGEX_SIZE_LIMIT: usize = 256 << 10;

/// Regex lazy-DFA cache limit (plan §7.4 `dfa_size_limit(256 KiB)`).
pub const REGEX_DFA_SIZE_LIMIT: usize = 256 << 10;

/// Regex nesting limit (plan §7.4 `nest_limit(64)`).
pub const REGEX_NEST_LIMIT: u32 = 64;

/// Shared window per channel (plan §7.4, §3.13 expect 행 "공유 창 64 KiB").
pub const WINDOW_BYTES: usize = 64 << 10;

/// Compiled patterns per channel, over all waiters (plan §3.13 expect 행 "총 8개/채널").
pub const MAX_PATTERNS: usize = 8;

/// Concurrent waiters per channel (plan §3.13 expect 행 "대기 8개").
pub const MAX_WAITERS: usize = 8;

/// Bytes appended to the window between scans. Must be well below the window so no byte
/// leaves the window before every waiter has seen it.
pub const FEED_STEP_BYTES: usize = 16 << 10;

/// Matched text returned in a hit (lossy UTF-8, longer matches are cut with `…`).
pub const MAX_HIT_TEXT_BYTES: usize = 4096;

/// Matcher limits (plan §7.4). [`MatcherLimits::effective`] clamps to the constants above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatcherLimits {
    /// Pattern source bytes.
    pub pattern_bytes: usize,
    /// Regex `size_limit`.
    pub size_limit: usize,
    /// Regex `dfa_size_limit`.
    pub dfa_size_limit: usize,
    /// Regex `nest_limit`.
    pub nest_limit: u32,
    /// Window bytes.
    pub window: usize,
    /// Patterns per channel.
    pub max_patterns: usize,
    /// Waiters per channel.
    pub max_waiters: usize,
}

impl Default for MatcherLimits {
    fn default() -> Self {
        Self {
            pattern_bytes: MAX_PATTERN_BYTES,
            size_limit: REGEX_SIZE_LIMIT,
            dfa_size_limit: REGEX_DFA_SIZE_LIMIT,
            nest_limit: REGEX_NEST_LIMIT,
            window: WINDOW_BYTES,
            max_patterns: MAX_PATTERNS,
            max_waiters: MAX_WAITERS,
        }
    }
}

impl MatcherLimits {
    /// Limits actually applied (never above the plan values; window ≥ 2 × feed step).
    pub fn effective(&self) -> MatcherLimits {
        MatcherLimits {
            pattern_bytes: self.pattern_bytes.min(MAX_PATTERN_BYTES),
            size_limit: self.size_limit.min(REGEX_SIZE_LIMIT),
            dfa_size_limit: self.dfa_size_limit.min(REGEX_DFA_SIZE_LIMIT),
            nest_limit: self.nest_limit.min(REGEX_NEST_LIMIT),
            window: self.window.clamp(1024, WINDOW_BYTES),
            max_patterns: self.max_patterns.min(MAX_PATTERNS),
            max_waiters: self.max_waiters.min(MAX_WAITERS),
        }
    }

    fn step(&self) -> usize {
        (self.window / 4).clamp(1, FEED_STEP_BYTES)
    }
}

/// An expect pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// Exact bytes (default; no regex engine).
    Literal(Vec<u8>),
    /// Rust `regex` syntax (`--regex`).
    Regex(String),
    /// Profile prompt regex, anchored at the end of the received data.
    Prompt(String),
}

/// Waiter id (assigned by the caller, unique per matcher).
pub type WaiterId = u64;

/// A fired waiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchHit {
    /// Waiter.
    pub id: WaiterId,
    /// Index of the matching pattern in the waiter's list.
    pub pattern_idx: usize,
    /// Raw RX offset of the first matched byte.
    pub start_off: u64,
    /// Raw RX offset just past the last matched byte.
    pub end_off: u64,
    /// Matched bytes as lossy UTF-8 (≤ [`MAX_HIT_TEXT_BYTES`]).
    pub text: String,
}

/// Why the window is reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    /// Data loss (any gap layer).
    Gap,
    /// New port generation.
    Generation,
}

/// Admission / compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchError {
    /// Pattern longer than `pattern_bytes`.
    PatternTooLong {
        /// Pattern index.
        idx: usize,
        /// Its length.
        len: usize,
    },
    /// Regex exceeds `size_limit` (or nesting) once compiled.
    CompileTooBig {
        /// Pattern index.
        idx: usize,
    },
    /// Regex syntax error / unsupported syntax (backreference, look-around).
    Syntax {
        /// Pattern index.
        idx: usize,
        /// Message from the regex crate.
        msg: String,
    },
    /// Empty pattern or empty pattern list.
    EmptyPattern,
    /// Channel pattern budget exhausted.
    TooManyPatterns,
    /// Channel waiter budget exhausted.
    TooManyWaiters,
    /// A waiter with this id already exists.
    DuplicateWaiter,
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MatchError {}

#[derive(Debug)]
enum Compiled {
    Literal(Finder<'static>),
    Regex(Regex),
}

#[derive(Debug)]
struct Waiter {
    id: WaiterId,
    pats: Vec<Compiled>,
    /// Absolute window index where this waiter's search starts.
    from_idx: u64,
    /// Raw offset this waiter asked for, until it is mapped into the window.
    from_raw: Option<u64>,
    /// Absolute window index up to which literals were scanned.
    scanned: u64,
}

/// A run of window bytes whose raw offsets are contiguous.
#[derive(Debug, Clone, Copy)]
struct Seg {
    idx: u64,
    raw: u64,
}

/// Per-channel bounded expect matcher.
#[derive(Debug)]
pub struct Matcher {
    limits: MatcherLimits,
    strip_ansi: bool,
    ansi: AnsiFilter,
    window: Vec<u8>,
    /// Absolute window index of `window[0]`.
    base: u64,
    /// Raw offset mapping of the window (sorted by `idx`).
    segs: VecDeque<Seg>,
    /// Raw offset just past the last byte fed.
    next_raw: Option<u64>,
    /// Raw offset just past the last byte appended to the window.
    last_kept_raw: Option<u64>,
    waiters: Vec<Waiter>,
    patterns: usize,
}

impl Matcher {
    /// New matcher (`strip_ansi = true` is the default the agent text uses).
    pub fn new(limits: MatcherLimits, strip_ansi: bool) -> Self {
        let limits = limits.effective();
        Self {
            limits,
            strip_ansi,
            ansi: AnsiFilter::new(),
            window: Vec::new(),
            base: 0,
            segs: VecDeque::new(),
            next_raw: None,
            last_kept_raw: None,
            waiters: Vec::new(),
            patterns: 0,
        }
    }

    /// Limits in force.
    pub fn limits(&self) -> MatcherLimits {
        self.limits
    }

    /// Waiters registered.
    pub fn waiters(&self) -> usize {
        self.waiters.len()
    }

    /// Compiled patterns held over all waiters.
    pub fn patterns(&self) -> usize {
        self.patterns
    }

    /// Bytes currently in the window.
    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    /// Raw offset of the oldest byte in the window, if any.
    pub fn window_start_offset(&self) -> Option<u64> {
        self.segs.front().map(|s| s.raw)
    }

    /// Raw-offset mapping runs held (≤ window bytes).
    pub fn segment_count(&self) -> usize {
        self.segs.len()
    }

    fn compile(&self, idx: usize, p: Pattern) -> Result<Compiled, MatchError> {
        let len = match &p {
            Pattern::Literal(b) => b.len(),
            Pattern::Regex(s) | Pattern::Prompt(s) => s.len(),
        };
        if len == 0 {
            return Err(MatchError::EmptyPattern);
        }
        if len > self.limits.pattern_bytes {
            return Err(MatchError::PatternTooLong { idx, len });
        }
        let src = match p {
            Pattern::Literal(b) => return Ok(Compiled::Literal(Finder::new(&b).into_owned())),
            Pattern::Regex(s) => s,
            Pattern::Prompt(s) => format!("(?:{s})\\z"),
        };
        RegexBuilder::new(&src)
            .size_limit(self.limits.size_limit)
            .dfa_size_limit(self.limits.dfa_size_limit)
            .nest_limit(self.limits.nest_limit)
            .build()
            .map(Compiled::Regex)
            .map_err(|e| match e {
                regex::Error::CompiledTooBig(_) => MatchError::CompileTooBig { idx },
                regex::Error::Syntax(msg) => MatchError::Syntax { idx, msg },
                other => MatchError::Syntax {
                    idx,
                    msg: other.to_string(),
                },
            })
    }

    /// Register a one-shot waiter. Patterns are compiled here; on error nothing is added.
    /// Call [`Matcher::poll`] (or the next [`Matcher::feed`]) to scan bytes already in the
    /// window.
    pub fn add(
        &mut self,
        id: WaiterId,
        patterns: Vec<Pattern>,
        from_rx_offset: u64,
    ) -> Result<(), MatchError> {
        if patterns.is_empty() {
            return Err(MatchError::EmptyPattern);
        }
        if self.waiters.iter().any(|w| w.id == id) {
            return Err(MatchError::DuplicateWaiter);
        }
        if self.waiters.len() >= self.limits.max_waiters {
            return Err(MatchError::TooManyWaiters);
        }
        if self.patterns + patterns.len() > self.limits.max_patterns {
            return Err(MatchError::TooManyPatterns);
        }
        let pats = patterns
            .into_iter()
            .enumerate()
            .map(|(i, p)| self.compile(i, p))
            .collect::<Result<Vec<_>, _>>()?;
        self.patterns += pats.len();
        let mut w = Waiter {
            id,
            pats,
            from_idx: self.base,
            from_raw: Some(from_rx_offset),
            scanned: self.base,
        };
        self.map_waiter_start(&mut w);
        self.waiters.push(w);
        Ok(())
    }

    /// Remove a waiter (timeout / cancel).
    pub fn remove(&mut self, id: WaiterId) {
        if let Some(i) = self.waiters.iter().position(|w| w.id == id) {
            let w = self.waiters.remove(i);
            self.patterns -= w.pats.len();
        }
    }

    /// Empty the window (gap / generation boundary). Waiters stay and only see later bytes.
    pub fn reset(&mut self, _reason: ResetReason) {
        self.base += self.window.len() as u64;
        self.window.clear();
        self.segs.clear();
        self.ansi.reset();
        self.next_raw = None;
        self.last_kept_raw = None;
        let base = self.base;
        for w in &mut self.waiters {
            w.from_idx = w.from_idx.max(base);
            w.scanned = w.scanned.max(base);
        }
    }

    /// Scan waiters against the current window without new bytes.
    pub fn poll(&mut self) -> Vec<MatchHit> {
        let mut hits = Vec::new();
        self.scan(&mut hits);
        hits
    }

    /// Feed raw RX bytes starting at `rx_offset`. A jump in `rx_offset` is treated as a gap.
    /// Returns the waiters that fired (each is removed).
    pub fn feed(&mut self, rx_offset: u64, bytes: &[u8]) -> Vec<MatchHit> {
        let mut hits = Vec::new();
        let (mut off, mut bytes) = (rx_offset, bytes);
        if let Some(n) = self.next_raw {
            if off < n {
                // Overlap with bytes already seen: skip them.
                let skip = usize::try_from(n - off).unwrap_or(usize::MAX).min(bytes.len());
                bytes = &bytes[skip..];
                off += skip as u64;
            } else if off > n {
                self.reset(ResetReason::Gap);
            }
        }
        if bytes.is_empty() {
            self.scan(&mut hits);
            return hits;
        }
        let step = self.limits.step();
        for piece in bytes.chunks(step) {
            self.append(off, piece);
            off += piece.len() as u64;
            self.scan(&mut hits);
        }
        hits
    }

    fn append(&mut self, off: u64, piece: &[u8]) {
        for (i, &b) in piece.iter().enumerate() {
            let raw = off + i as u64;
            if self.strip_ansi && self.ansi.classify(b) == ByteClass::Sequence {
                continue;
            }
            let idx = self.base + self.window.len() as u64;
            if self.last_kept_raw != Some(raw) {
                self.segs.push_back(Seg { idx, raw });
            }
            self.window.push(b);
            self.last_kept_raw = Some(raw + 1);
        }
        self.next_raw = Some(off + piece.len() as u64);
        // Map waiters that asked for an offset not yet seen.
        let mut ws = std::mem::take(&mut self.waiters);
        for w in &mut ws {
            self.map_waiter_start(w);
        }
        self.waiters = ws;
        self.trim();
    }

    fn trim(&mut self) {
        let excess = self.window.len().saturating_sub(self.limits.window);
        if excess == 0 {
            return;
        }
        self.window.drain(..excess);
        self.base += excess as u64;
        let base = self.base;
        // Drop runs that lie entirely before the new base; re-anchor the first one.
        while self.segs.len() > 1 && self.segs.get(1).is_some_and(|s| s.idx <= base) {
            self.segs.pop_front();
        }
        if let Some(first) = self.segs.front_mut()
            && first.idx < base
        {
            first.raw += base - first.idx;
            first.idx = base;
        }
        for w in &mut self.waiters {
            w.from_idx = w.from_idx.max(base);
            w.scanned = w.scanned.max(base);
        }
    }

    /// Absolute window index → raw offset.
    fn raw_of(&self, idx: u64) -> u64 {
        let i = self.segs.partition_point(|s| s.idx <= idx);
        match i.checked_sub(1).and_then(|j| self.segs.get(j)) {
            Some(s) => s.raw + (idx - s.idx),
            None => self.next_raw.unwrap_or(0),
        }
    }

    /// First absolute window index whose raw offset is ≥ `raw`, or `None` if `raw` is past
    /// the bytes fed so far.
    fn idx_of_raw(&self, raw: u64) -> Option<u64> {
        let end = self.base + self.window.len() as u64;
        if self.next_raw.is_none_or(|n| raw >= n) {
            return if self.next_raw == Some(raw) {
                Some(end)
            } else {
                None
            };
        }
        let i = self.segs.partition_point(|s| s.raw <= raw);
        let Some(j) = i.checked_sub(1) else {
            return Some(self.base);
        };
        let s = self.segs[j];
        let run_end = self.segs.get(j + 1).map_or(end, |n| n.idx);
        let idx = s.idx + (raw - s.raw);
        Some(if idx < run_end { idx } else { run_end })
    }

    fn map_waiter_start(&self, w: &mut Waiter) {
        if let Some(raw) = w.from_raw
            && let Some(idx) = self.idx_of_raw(raw)
        {
            w.from_idx = idx.max(self.base);
            w.scanned = w.from_idx;
            w.from_raw = None;
        }
    }

    fn scan(&mut self, hits: &mut Vec<MatchHit>) {
        let end = self.base + self.window.len() as u64;
        let mut fired = Vec::new();
        for (wi, w) in self.waiters.iter_mut().enumerate() {
            if w.from_raw.is_some() {
                continue;
            }
            let from = usize::try_from(w.from_idx - self.base).unwrap_or(usize::MAX);
            let Some(hay) = self.window.get(from..) else {
                continue;
            };
            let mut best: Option<(usize, usize, usize)> = None;
            for (pi, p) in w.pats.iter().enumerate() {
                let found = match p {
                    Compiled::Literal(f) => {
                        let overlap = f.needle().len().saturating_sub(1) as u64;
                        let s = w.scanned.saturating_sub(overlap).max(w.from_idx);
                        let rel = usize::try_from(s - w.from_idx).unwrap_or(0);
                        hay.get(rel..)
                            .and_then(|h| f.find(h))
                            .map(|i| (rel + i, rel + i + f.needle().len()))
                    }
                    Compiled::Regex(r) => r.find(hay).map(|m| (m.start(), m.end())),
                };
                if let Some((s, e)) = found
                    && best.is_none_or(|(bs, _, _)| s < bs)
                {
                    best = Some((s, e, pi));
                }
            }
            w.scanned = end;
            if let Some((s, e, pi)) = best {
                fired.push((wi, from + s, from + e, pi));
            }
        }
        for &(wi, s, e, pi) in fired.iter().rev() {
            let w = self.waiters.remove(wi);
            self.patterns -= w.pats.len();
            let bytes = &self.window[s..e];
            let mut text =
                String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_HIT_TEXT_BYTES)]).into_owned();
            if bytes.len() > MAX_HIT_TEXT_BYTES {
                text.push('…');
            }
            let si = self.base + s as u64;
            let ei = self.base + e as u64;
            let start_off = self.raw_of(si);
            let end_off = if e > s { self.raw_of(ei - 1) + 1 } else { start_off };
            hits.push(MatchHit {
                id: w.id,
                pattern_idx: pi,
                start_off,
                end_off,
                text,
            });
        }
        // `fired` was removed in reverse index order; report in waiter registration order.
        let n = fired.len();
        let len = hits.len();
        hits[len - n..].reverse();
    }
}
