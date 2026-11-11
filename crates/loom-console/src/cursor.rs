//! `cursor` — per-client read cursors and bounded `read` results (UART plan §3.3, §3.6).
//!
//! A cursor is keyed by `client_name` and stores an RX offset (plus the generation at that
//! offset and a `revision` bumped on every move). [`read`] turns a range of the [`Ring`] into
//! display [`Line`]s under hard output limits:
//!
//! - [`ReadMode::Summary`] — if the range does not fit, the first lines (≤ 20) and as many of
//!   the last lines as fit are returned with an [`Omitted`] marker in between; the cursor moves
//!   to the end of the range.
//! - [`ReadMode::Page`] — lines up to the limits; the cursor moves past the returned lines only.
//! - [`ReadMode::Peek`] — like `Page` but the cursor does not move.
//!
//! The trailing line still in progress (no terminator yet, e.g. a `login: ` prompt) is shown
//! with `complete = false` but never consumed: the cursor stops at its first byte, so the next
//! read shows it again, completed.
//!
//! Stream `seq` is unknown here: lines carry only `source_seq`; mapping to stream seq is the
//! gateway's job (P1).

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::ring::{Ring, RxItem};
use crate::text::{Line, MAX_LINE_CHARS, MIN_LINE_CHARS, TextOptions, TextPipeline};

/// Cursors per channel; the least recently used one is evicted (and reported) beyond this
/// (task p0-console-core §C "클라이언트 수 상한 256", plan §3.3).
pub const MAX_CLIENTS: usize = 256;

/// Longest `client_name` accepted (same rule as ring metadata fields).
pub const MAX_CLIENT_NAME_BYTES: usize = crate::ring::MAX_META_FIELD_BYTES;

/// Default `--max-lines` (plan §3.6, A7).
pub const DEFAULT_MAX_LINES: u32 = 100;

/// Default `--max-bytes` (plan §3.6, A7).
pub const DEFAULT_MAX_BYTES: u32 = 16 * 1024;

/// Hard line cap of one read (plan §3.6 "하드 1 000행").
pub const HARD_MAX_LINES: u32 = 1000;

/// Hard byte cap of one read (plan §3.6 "하드 64 KiB").
pub const HARD_MAX_BYTES: u32 = 64 * 1024;

/// Smallest byte cap a caller may ask for: one full-width line (512 chars × 4 B + suffix)
/// must always fit so a read can make progress.
pub const MIN_MAX_BYTES: u32 = 4 * 1024;

/// Head lines kept by [`ReadMode::Summary`] (plan §3.6 "머리 20줄").
pub const SUMMARY_HEAD_LINES: u32 = 20;

/// Bytes fed to the text pipeline per step, so per-step line batches stay bounded even if a
/// chunk is large (plan §3.13 "청크 ≤16 KiB").
pub const FEED_STEP_BYTES: usize = 16 * 1024;

/// Bytes a line costs against `max_bytes`: its text plus one separator byte.
pub fn line_cost(line: &Line) -> u32 {
    u32::try_from(line.text.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

/// Read mode (plan §3.6, §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    /// Head + tail with an omission marker; cursor to the end (`--new`, default).
    Summary,
    /// Up to the limits; cursor past the returned lines (`--page`).
    Page,
    /// Up to the limits; cursor unchanged (`--peek`).
    Peek,
}

/// Where a read starts. (Named `ReadFrom`, not `From`, so it does not shadow `std::convert::From`.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadFrom {
    /// This client's cursor (`@last`); a new client starts at the oldest retained byte.
    Last,
    /// RX offset (`off:N`).
    RxOffset(u64),
    /// First RX byte at or after this `source_seq`.
    SourceSeq(u64),
    /// A `next_cursor` token from an earlier read (`--cursor-token`).
    Token(String),
}

/// Output limits of one read (plan §3.6). Values are clamped by [`ReadLimits::effective`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadLimits {
    /// Requested line cap (default 100).
    pub max_lines: u32,
    /// Requested byte cap (default 16 KiB).
    pub max_bytes: u32,
    /// Hard line cap (default and maximum 1000).
    pub hard_lines: u32,
    /// Hard byte cap (default and maximum 64 KiB).
    pub hard_bytes: u32,
    /// Display characters per line (default and maximum 512).
    pub line_chars: u32,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_bytes: DEFAULT_MAX_BYTES,
            hard_lines: HARD_MAX_LINES,
            hard_bytes: HARD_MAX_BYTES,
            line_chars: MAX_LINE_CHARS,
        }
    }
}

impl ReadLimits {
    /// Limits actually applied: hard values never exceed the module constants and requested
    /// values never exceed the hard ones.
    pub fn effective(&self) -> ReadLimits {
        let hard_lines = self.hard_lines.clamp(1, HARD_MAX_LINES);
        let hard_bytes = self.hard_bytes.clamp(MIN_MAX_BYTES, HARD_MAX_BYTES);
        ReadLimits {
            max_lines: self.max_lines.clamp(1, hard_lines),
            max_bytes: self.max_bytes.clamp(MIN_MAX_BYTES, hard_bytes),
            hard_lines,
            hard_bytes,
            line_chars: self.line_chars.clamp(MIN_LINE_CHARS, MAX_LINE_CHARS),
        }
    }
}

/// Lines left out in the middle of a [`ReadMode::Summary`] result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Omitted {
    /// `source_seq` of the first omitted line.
    pub from_seq: u64,
    /// `source_seq` of the last omitted line.
    pub to_seq: u64,
    /// Omitted line count.
    pub lines: u64,
    /// Omitted raw bytes.
    pub bytes: u64,
    /// RX offset of the first omitted byte.
    pub from_rx_offset: u64,
    /// RX offset just past the last omitted byte.
    pub to_rx_offset: u64,
    /// Index in [`ReadResult::lines`] where the omission sits (= head line count).
    pub at_line: u32,
}

/// The requested start was already evicted from the ring (display only here; the record-file
/// lookup is P1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorGap {
    /// First RX offset no longer in the ring (exact only if `exact`).
    pub lost_from: u64,
    /// RX offset where reading resumed (oldest retained byte).
    pub lost_to: u64,
    /// `false` when the start was given as a `source_seq` whose RX offset is unknown.
    pub exact: bool,
}

/// Result of [`read`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadResult {
    /// Lines in order (head, then tail for a summary).
    pub lines: Vec<Line>,
    /// Some output was left out (summary omission or page limit).
    pub truncated: bool,
    /// Summary omission.
    pub omitted: Option<Omitted>,
    /// More complete lines are available after the returned ones (page / peek).
    pub more_available: bool,
    /// Opaque token for a stateless continuation (`--cursor-token`).
    pub next_cursor: String,
    /// The start had been evicted from the ring.
    pub cursor_gap: Option<CursorGap>,
    /// `likely_binary_or_wrong_baud`.
    pub binary_hint: bool,
    /// RX offset the read started at (after gap adjustment).
    pub from_rx_offset: u64,
    /// RX offset the next read continues at (the stored cursor for summary/page).
    pub next_rx_offset: u64,
    /// Cursor revision after the read.
    pub revision: u64,
    /// Another client's cursor evicted (LRU) to make room for this one.
    pub evicted_client: Option<String>,
}

/// Cursor / read error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorError {
    /// Empty `client_name`.
    EmptyClientName,
    /// `client_name` longer than [`MAX_CLIENT_NAME_BYTES`].
    ClientNameTooLong(usize),
    /// Token malformed, corrupted or not issued by this store.
    BadToken,
    /// The cursor was moved by someone else since the revision the caller started from.
    RevisionConflict {
        /// Revision the caller started from.
        expected: u64,
        /// Revision now stored.
        current: u64,
    },
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CursorError {}

/// Stored cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CursorPos {
    /// RX offset of the next unread byte.
    pub rx_offset: u64,
    /// Generation at that offset (informational).
    pub generation: u32,
    /// Bumped on every move; 0 = never stored.
    pub revision: u64,
}

/// Result of [`CursorStore::commit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitOutcome {
    /// Cursor after the commit.
    pub pos: CursorPos,
    /// Least recently used client evicted to make room.
    pub evicted: Option<String>,
}

#[derive(Debug, Clone)]
struct CursorSlot {
    pos: CursorPos,
    last_used: u64,
}

const TOKEN_PREFIX: &str = "c1-";
const TOKEN_HEX_LEN: usize = 16 + 8 + 16 + 16;

/// Cursors of one channel, at most [`MAX_CLIENTS`] (LRU eviction).
#[derive(Debug, Clone)]
pub struct CursorStore {
    map: HashMap<String, CursorSlot>,
    tick: u64,
    key: (u64, u64),
}

impl Default for CursorStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorStore {
    /// Empty store with a per-process random token key.
    pub fn new() -> Self {
        let rs = RandomState::new();
        Self::with_token_key(rs.hash_one(0x6c6f_6f6d_u64), rs.hash_one(0x6375_7273_u64))
    }

    /// Empty store with a fixed token key (tests, or a key persisted with `cursors.json`).
    pub fn with_token_key(k0: u64, k1: u64) -> Self {
        Self {
            map: HashMap::new(),
            tick: 0,
            key: (k0, k1),
        }
    }

    /// Number of stored cursors.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when no cursor is stored.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Cursor of `client`, if stored (does not touch LRU order).
    pub fn get(&self, client: &str) -> Option<CursorPos> {
        self.map.get(client).map(|s| s.pos)
    }

    /// All cursors (for persisting `cursors.json`).
    pub fn iter(&self) -> impl Iterator<Item = (&str, CursorPos)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.pos))
    }

    /// Forget `client`'s cursor.
    pub fn remove(&mut self, client: &str) -> Option<CursorPos> {
        self.map.remove(client).map(|s| s.pos)
    }

    /// Validate a client name.
    pub fn check_name(client: &str) -> Result<(), CursorError> {
        if client.is_empty() {
            return Err(CursorError::EmptyClientName);
        }
        if client.len() > MAX_CLIENT_NAME_BYTES {
            return Err(CursorError::ClientNameTooLong(client.len()));
        }
        Ok(())
    }

    /// Move `client`'s cursor to `rx_offset`.
    ///
    /// `expected` is the revision the caller based its read on (`None` = unconditional). If the
    /// stored revision differs, the commit is refused with [`CursorError::RevisionConflict`] —
    /// unless the cursor already sits at `rx_offset` (idempotent replay of a lost `--page`
    /// response). Moving to the current position does not bump the revision.
    pub fn commit(
        &mut self,
        client: &str,
        expected: Option<u64>,
        rx_offset: u64,
        generation: u32,
    ) -> Result<CommitOutcome, CursorError> {
        Self::check_name(client)?;
        self.tick += 1;
        let tick = self.tick;
        if let Some(slot) = self.map.get_mut(client) {
            let cur = slot.pos;
            if cur.rx_offset == rx_offset {
                slot.last_used = tick;
                return Ok(CommitOutcome {
                    pos: cur,
                    evicted: None,
                });
            }
            if let Some(exp) = expected
                && exp != cur.revision
            {
                return Err(CursorError::RevisionConflict {
                    expected: exp,
                    current: cur.revision,
                });
            }
            slot.pos = CursorPos {
                rx_offset,
                generation,
                revision: cur.revision + 1,
            };
            slot.last_used = tick;
            return Ok(CommitOutcome {
                pos: slot.pos,
                evicted: None,
            });
        }
        if let Some(exp) = expected
            && exp != 0
        {
            return Err(CursorError::RevisionConflict {
                expected: exp,
                current: 0,
            });
        }
        let mut evicted = None;
        if self.map.len() >= MAX_CLIENTS
            && let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, s)| s.last_used)
                .map(|(k, _)| k.clone())
        {
            self.map.remove(&oldest);
            evicted = Some(oldest);
        }
        let pos = CursorPos {
            rx_offset,
            generation,
            revision: 1,
        };
        self.map
            .insert(client.to_owned(), CursorSlot { pos, last_used: tick });
        Ok(CommitOutcome { pos, evicted })
    }

    fn tag(&self, rx_offset: u64, generation: u32, revision: u64) -> u64 {
        let mut h = DefaultHasher::new();
        self.key.0.hash(&mut h);
        rx_offset.hash(&mut h);
        generation.hash(&mut h);
        revision.hash(&mut h);
        self.key.1.hash(&mut h);
        h.finish()
    }

    /// Encode an opaque `next_cursor` token (RX offset + generation + revision + integrity tag
    /// keyed per store; not a cryptographic MAC).
    pub fn encode_token(&self, pos: CursorPos) -> String {
        format!(
            "{TOKEN_PREFIX}{:016x}{:08x}{:016x}{:016x}",
            pos.rx_offset,
            pos.generation,
            pos.revision,
            self.tag(pos.rx_offset, pos.generation, pos.revision)
        )
    }

    /// Decode a token from [`CursorStore::encode_token`]; malformed or altered → `BadToken`.
    pub fn decode_token(&self, token: &str) -> Result<CursorPos, CursorError> {
        let hex = token.strip_prefix(TOKEN_PREFIX).ok_or(CursorError::BadToken)?;
        if hex.len() != TOKEN_HEX_LEN || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CursorError::BadToken);
        }
        let field = |a: usize, b: usize| {
            hex.get(a..b)
                .and_then(|s| u64::from_str_radix(s, 16).ok())
                .ok_or(CursorError::BadToken)
        };
        let rx_offset = field(0, 16)?;
        let generation = u32::try_from(field(16, 24)?).map_err(|_| CursorError::BadToken)?;
        let revision = field(24, 40)?;
        let tag = field(40, 56)?;
        if tag != self.tag(rx_offset, generation, revision) {
            return Err(CursorError::BadToken);
        }
        Ok(CursorPos {
            rx_offset,
            generation,
            revision,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Line collector
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Collected {
    lines: Vec<Line>,
    omitted: Option<Omitted>,
    more: bool,
    /// End offset and generation of the last consumed line.
    consumed_end: Option<(u64, u32)>,
}

#[derive(Debug)]
struct Collector {
    summary: bool,
    max_lines: u32,
    max_bytes: u32,
    head_cap: u32,
    head: Vec<Line>,
    head_bytes: u32,
    head_closed: bool,
    tail: VecDeque<Line>,
    tail_bytes: u32,
    omitted: Option<Omitted>,
    more: bool,
    stop: bool,
    consumed_end: Option<(u64, u32)>,
}

impl Collector {
    fn new(mode: ReadMode, l: &ReadLimits) -> Self {
        let head_cap = SUMMARY_HEAD_LINES.min(l.max_lines / 2);
        Self {
            summary: mode == ReadMode::Summary,
            max_lines: l.max_lines,
            max_bytes: l.max_bytes,
            head_cap,
            head: Vec::new(),
            head_bytes: 0,
            head_closed: false,
            tail: VecDeque::new(),
            tail_bytes: 0,
            omitted: None,
            more: false,
            stop: false,
            consumed_end: None,
        }
    }

    fn total_lines(&self) -> u32 {
        u32::try_from(self.head.len() + self.tail.len()).unwrap_or(u32::MAX)
    }

    /// Offer a line. `consume` = the cursor may move past it.
    fn offer(&mut self, line: Line, consume: bool) {
        if self.stop {
            if consume {
                self.more = true;
            }
            return;
        }
        let cost = line_cost(&line);
        if !self.summary {
            let fits = self.total_lines() < self.max_lines
                && (self.head.is_empty() || self.head_bytes.saturating_add(cost) <= self.max_bytes);
            if fits {
                if consume {
                    self.consumed_end = Some((line.rx_offset_end, line.generation));
                }
                self.head_bytes = self.head_bytes.saturating_add(cost);
                self.head.push(line);
            } else {
                self.stop = true;
                self.more = true;
            }
            return;
        }
        // Summary: every complete line is consumed, shown or not.
        if consume {
            self.consumed_end = Some((line.rx_offset_end, line.generation));
        }
        if !self.head_closed {
            let fits = u32::try_from(self.head.len()).unwrap_or(u32::MAX) < self.head_cap
                && self.head_bytes.saturating_add(cost) <= self.max_bytes / 2;
            if fits {
                self.head_bytes = self.head_bytes.saturating_add(cost);
                self.head.push(line);
                return;
            }
            self.head_closed = true;
        }
        self.tail_bytes = self.tail_bytes.saturating_add(cost);
        self.tail.push_back(line);
        let line_room = self.max_lines - u32::try_from(self.head.len()).unwrap_or(0);
        let byte_room = self.max_bytes - self.head_bytes;
        while u32::try_from(self.tail.len()).unwrap_or(u32::MAX) > line_room
            || (self.tail_bytes > byte_room && self.tail.len() > 1)
        {
            let Some(old) = self.tail.pop_front() else {
                break;
            };
            self.tail_bytes -= line_cost(&old);
            self.omit(&old);
        }
    }

    fn omit(&mut self, l: &Line) {
        let at_line = u32::try_from(self.head.len()).unwrap_or(u32::MAX);
        let raw = l.rx_offset_end - l.rx_offset_start;
        match &mut self.omitted {
            Some(o) => {
                o.to_seq = l.source_seq_first;
                o.lines += 1;
                o.bytes += raw;
                o.to_rx_offset = l.rx_offset_end;
            }
            None => {
                self.omitted = Some(Omitted {
                    from_seq: l.source_seq_first,
                    to_seq: l.source_seq_first,
                    lines: 1,
                    bytes: raw,
                    from_rx_offset: l.rx_offset_start,
                    to_rx_offset: l.rx_offset_end,
                    at_line,
                });
            }
        }
    }

    fn finish(self) -> Collected {
        let mut lines = self.head;
        lines.extend(self.tail);
        Collected {
            lines,
            omitted: self.omitted,
            more: self.more,
            consumed_end: self.consumed_end,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// read
// ---------------------------------------------------------------------------------------------

/// Read lines for `client` from `ring` (plan §3.6 read rules; see module docs).
///
/// `text.line_chars` is overridden by `limits.line_chars`. The cursor is committed with the
/// revision the read started from, so a concurrent move of the same `client` by another
/// request yields [`CursorError::RevisionConflict`] and nothing is moved.
pub fn read(
    ring: &Ring,
    store: &mut CursorStore,
    client: &str,
    from: ReadFrom,
    mode: ReadMode,
    limits: &ReadLimits,
    text: &TextOptions,
) -> Result<ReadResult, CursorError> {
    CursorStore::check_name(client)?;
    let limits = limits.effective();
    let current = store.get(client);
    let retained = ring.retained_from_rx_offset();

    // 1. Resolve the start position and the revision we base the commit on.
    let (requested, expected, exact) = match &from {
        ReadFrom::Last => match current {
            Some(p) => (p.rx_offset, Some(p.revision), true),
            None => (retained, None, true),
        },
        ReadFrom::RxOffset(o) => (*o, current.map(|p| p.revision), true),
        ReadFrom::SourceSeq(s) => {
            if *s < ring.retained_from_source_seq() {
                (0, current.map(|p| p.revision), false)
            } else {
                (
                    ring.rx_offset_for_source_seq(*s),
                    current.map(|p| p.revision),
                    true,
                )
            }
        }
        ReadFrom::Token(t) => {
            let p = store.decode_token(t)?;
            (p.rx_offset, Some(p.revision), true)
        }
    };

    // 2. Out of the ring → cursor_gap, read the remaining range.
    let mut cursor_gap = None;
    let mut start = requested;
    if start < retained || !exact {
        cursor_gap = Some(CursorGap {
            lost_from: start,
            lost_to: retained,
            exact,
        });
        start = retained;
    }
    let start = start.min(ring.rx_end());

    // 3. Decode and collect.
    let mut opts = *text;
    opts.line_chars = limits.line_chars;
    let mut pipe = TextPipeline::new(opts);
    let mut col = Collector::new(mode, &limits);
    let mut batch = Vec::new();
    'scan: for item in ring.rx_items_from(start) {
        match item {
            RxItem::Gap(_) => pipe.reset(&mut batch),
            RxItem::Data(slice) => {
                let base = slice.rx_offset().unwrap_or(start);
                let c = slice.chunk;
                for (i, piece) in slice.bytes().chunks(FEED_STEP_BYTES).enumerate() {
                    let off = base + (i * FEED_STEP_BYTES) as u64;
                    pipe.feed(off, c.source_seq, c.generation, piece, &mut batch);
                    for l in batch.drain(..) {
                        col.offer(l, true);
                    }
                    if col.stop && col.more {
                        break 'scan;
                    }
                }
            }
        }
        for l in batch.drain(..) {
            col.offer(l, true);
        }
    }
    if !(col.stop && col.more)
        && let Some(p) = pipe.partial()
    {
        col.offer(p, false);
    }
    let binary_hint = pipe.binary_hint();
    let Collected {
        lines,
        omitted,
        more: more_available,
        consumed_end: consumed,
    } = col.finish();

    // 4. Move the cursor.
    let (next_rx_offset, next_gen) = consumed.unwrap_or((start, ring.generation()));
    let mut evicted_client = None;
    let revision = match mode {
        ReadMode::Summary | ReadMode::Page => {
            let out = store.commit(client, expected, next_rx_offset, next_gen)?;
            evicted_client = out.evicted;
            out.pos.revision
        }
        ReadMode::Peek => current.map_or(0, |p| p.revision),
    };
    let next_cursor = store.encode_token(CursorPos {
        rx_offset: next_rx_offset,
        generation: next_gen,
        revision,
    });

    Ok(ReadResult {
        truncated: omitted.is_some() || more_available,
        lines,
        omitted,
        more_available,
        next_cursor,
        cursor_gap,
        binary_hint,
        from_rx_offset: start,
        next_rx_offset,
        revision,
        evicted_client,
    })
}
