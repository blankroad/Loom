//! Channel core — the synchronous, pure state of one channel (one port) (UART plan §3.3, §3.6,
//! §4.4–4.6, §7.4; tasks T08 part + T09 read/expect engine).
//!
//! [`ChannelCore`] takes RX/TX chunks and loss ranges, keeps the ring, the expect matcher, the
//! prompt watcher and the alternate-screen detector, answers agent `read` / `expect` / `run`
//! and panel `lines`, and builds the event payloads to publish. No threads, no I/O, no clock
//! (times come in the chunks). The worker wraps it in a `Mutex` shared by the reader pump and
//! the RPC handlers, so **every method is bounded**: reads stop at [`MAX_READ_SCAN`] bytes,
//! expect backfill at [`EXPECT_BACKFILL_SCAN`], `lines` at [`LINES_SCAN_BYTES`], context and
//! `run` output at their own caps.
//!
//! Coordinates: RX offsets and worker `source_seq` only. Stream `seq` is the gateway's (it maps
//! `source_seq` → `seq`); u64 values are decimal strings in JSON.
//!
//! Matching rules (plan §4.5): patterns run on ANSI-stripped bytes with raw offsets; a match
//! never spans a gap or a generation boundary (the matcher window is reset there, and the ring
//! backfill resets at the same points). Waiters survive a new generation and match its bytes.

mod alt;
mod profile;
mod prompt;
mod query;
mod render;

use std::collections::VecDeque;

use loom_console::cursor::{self, CursorError, CursorStore, ReadLimits, ReadResult};
use loom_console::matcher::{Matcher, MatcherLimits, Pattern, ResetReason, WINDOW_BYTES};
use loom_console::ring::{Chunk, Entry, GapLayer, GapMark, Ring, RingLimits, RxItem};
use loom_console::text::{Line, TextOptions, TextPipeline};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Origin};
use serde_json::{Value, json};

pub use loom_console::cursor::{ReadFrom, ReadMode};
pub use loom_console::matcher::{MatchError, MatchHit};
pub use loom_console::text::{AnsiMode, Encoding};
pub use profile::{Eol, Profile};
pub use query::{
    ExpectFrom, LINES_DEFAULT_LIMIT, LINES_MAX_LIMIT, LinesQuery, PatternKind, PatternSpec, PosSpec,
    ReadQuery,
};

use alt::AltScreen;
use prompt::PromptWatch;
use render::{
    GapList, align_to_line, cut_str, data_payload, escape_bytes, gaps_in, line_json, num, render_range,
    window_ring,
};

/// RX bytes one `read` may decode (task p1-channel-core; plan §3.6 "유한 작업 … 초과 시
/// `E_SCAN_LIMIT`와 이어 읽을 위치"). Beyond it a page stops early with `scan_limit.resume_from`,
/// a summary decodes only the newest part and reports the rest as `skipped`.
///
/// 128 KiB, not the 4 MiB of the task text: decoding runs at ≈ 10 MB/s in a debug build
/// (≈ 80 MB/s release), so 4 MiB held the channel mutex ≈ 430 ms (debug) / ≈ 55 ms (release),
/// over the 50 ms per-call target. 128 KiB ≈ 1 500 typical lines — more than one page
/// (≤ 1 000 lines / 64 KiB of text) usually needs.
pub const MAX_READ_SCAN: u64 = 128 << 10;
/// RX bytes an expect may re-scan from the ring when it starts in the past (plan §4.5 byte /
/// work budget). Divided by the number of regex patterns of the waiter (each regex re-scans the
/// matcher window per step). Beyond it: `ChannelError::ScanLimit { resume_from }`.
pub const EXPECT_BACKFILL_SCAN: u64 = 256 << 10;
/// Floor of the per-waiter backfill budget (8 regex patterns → 64 KiB, not 32 KiB).
pub const EXPECT_BACKFILL_MIN: u64 = 64 << 10;
/// RX bytes one `uart.lines` call may decode (plan §3.13 bounded work; larger ranges go through
/// HTTP / `raw_ref`, A5). Same reasoning as [`MAX_READ_SCAN`].
pub const LINES_SCAN_BYTES: u64 = 128 << 10;
/// Row text bytes of one `uart.lines` result (plan §3.6 hard 64 KiB / RPC message 64 KiB).
pub const LINES_MAX_BYTES: usize = 64 << 10;
/// Context lines of an expect result / timeout (plan §4.5 `--context 15`).
pub const CONTEXT_LINES: usize = 15;
/// RX bytes decoded for that context.
pub const CONTEXT_SCAN_BYTES: u64 = 16 << 10;
/// RX bytes decoded for `run` output; older output is cut (`truncated`, `skipped`). Same
/// reasoning as [`MAX_READ_SCAN`].
pub const RUN_SCAN_BYTES: u64 = 128 << 10;
/// `run` output lines (plan §4.6 `--max-lines 100`); head 20 + tail, the middle is counted.
pub const RUN_MAX_LINES: usize = 100;
/// `run` output text bytes (plan §3.6 default 16 KiB).
pub const RUN_MAX_BYTES: usize = 16 << 10;
/// Head lines kept when `run` output is cut (plan §3.6 "머리 20줄").
pub const RUN_HEAD_LINES: usize = 20;
/// Finished / removed expect waiters remembered for `expect_result` / `expect_timeout` after
/// the matcher dropped them (bounded, oldest forgotten).
pub const FINISHED_WAITERS_MAX: usize = 16;
/// Display characters of one TX row in `uart.lines`.
pub const TX_ROW_CHARS: usize = 512;

/// Result of [`ChannelCore::on_rx`].
#[derive(Debug, Clone)]
pub struct Ingest {
    /// `uart.data` payload (dir rx) — `{bytes{encoding,data}, len, rx_offset}`.
    pub data_payload: Value,
    /// Expect waiters that fired on this chunk (each is removed from the matcher).
    pub hits: Vec<MatchHit>,
    /// The RX stream now ends with the profile prompt (InputArbiter `prompt_seen` hint).
    pub prompt_seen: bool,
    /// An alternate-screen set/reset (`?1049`, `?47`, `?1047`) was in this chunk: new state.
    pub alt_screen: Option<bool>,
}

/// Errors of the channel queries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelError {
    /// Bad arguments (bad cursor token, unresolvable position, bad client name …).
    #[error("usage: {0}")]
    Usage(String),
    /// Bounded work exhausted; continue from `resume_from` (exit 8).
    #[error("scan limit reached; resume from rx offset {resume_from}")]
    ScanLimit {
        /// RX offset to continue from.
        resume_from: u64,
    },
    /// The start position is no longer in the ring (exit 8).
    #[error("cursor gap: start is older than the ring; earliest retained rx offset {earliest}")]
    CursorGap {
        /// Oldest RX offset still in the ring.
        earliest: u64,
    },
    /// Pattern admission / compile error.
    #[error("pattern: {0}")]
    Pattern(MatchError),
}

impl ChannelError {
    /// Wire error code (P0 mapping: syntax-like → `E_USAGE`, budget-like → `E_SCAN_LIMIT`).
    pub fn code(&self) -> ErrorCode {
        match self {
            ChannelError::Usage(_) => ErrorCode::Usage,
            ChannelError::ScanLimit { .. } => ErrorCode::ScanLimit,
            ChannelError::CursorGap { .. } => ErrorCode::CursorGap,
            ChannelError::Pattern(e) => match e {
                MatchError::PatternTooLong { .. }
                | MatchError::Syntax { .. }
                | MatchError::EmptyPattern
                | MatchError::DuplicateWaiter => ErrorCode::Usage,
                MatchError::CompileTooBig { .. }
                | MatchError::TooManyPatterns
                | MatchError::TooManyWaiters => ErrorCode::ScanLimit,
            },
        }
    }

    /// Machine-readable detail (`resume_from` / `earliest` as decimal strings).
    pub fn detail(&self) -> Value {
        match self {
            ChannelError::Usage(m) => json!({"message": m}),
            ChannelError::ScanLimit { resume_from } => json!({"resume_from": num(*resume_from)}),
            ChannelError::CursorGap { earliest } => json!({"earliest": num(*earliest)}),
            ChannelError::Pattern(e) => json!({"pattern_error": format!("{e:?}")}),
        }
    }
}

fn cursor_err(e: CursorError) -> ChannelError {
    match e {
        CursorError::BadToken => ChannelError::Usage("bad cursor token".into()),
        CursorError::RevisionConflict { expected, current } => ChannelError::Usage(format!(
            "cursor revision conflict (expected {expected}, current {current}): another process \
             with the same client name moved this cursor"
        )),
        other => ChannelError::Usage(format!("{other}")),
    }
}

/// What the core remembers about an expect waiter.
#[derive(Debug, Clone)]
struct WaiterInfo {
    id: u64,
    client: Option<String>,
    from: u64,
    specs: Vec<PatternSpec>,
    /// Longest literal when every pattern is a literal (resume rule of plan §4.5).
    literal_max: Option<usize>,
}

enum Backfill {
    Clean,
    Hit(MatchHit),
    Limit(u64),
}

/// One channel's ring, cursors, matcher and query engine.
#[derive(Debug)]
pub struct ChannelCore {
    channel: String,
    profile: Profile,
    generation: u32,
    ring: Ring,
    cursors: CursorStore,
    matcher: Matcher,
    prompt: Option<PromptWatch>,
    prompt_error: Option<String>,
    alt: AltScreen,
    waiters: Vec<WaiterInfo>,
    finished: VecDeque<WaiterInfo>,
    producer: Option<(String, u64)>,
    /// RX offset space end, including ranges lost before the ring.
    rx_end: u64,
    rx_bytes: u64,
    rx_chunks: u64,
    tx_bytes: u64,
    tx_chunks: u64,
    lost_events: u64,
    lost_chunks: u64,
    lost_bytes: u64,
    ring_rejects: u64,
    last_mono_ns: u64,
    wall_minus_mono: Option<i128>,
}

impl ChannelCore {
    /// New channel with the default ring (16 MiB / 32 768 entries / 4 MiB meta) and matcher
    /// limits (64 KiB window, 8 waiters, 8 patterns).
    pub fn new(channel: &str, profile: Profile, generation: u32) -> Self {
        let (prompt, prompt_error) = match profile.prompt.as_deref().map(PromptWatch::new) {
            None => (None, None),
            Some(Ok(w)) => (Some(w), None),
            Some(Err(e)) => (None, Some(format!("{e:?}"))),
        };
        Self {
            channel: channel.to_owned(),
            profile,
            generation,
            ring: Ring::new(RingLimits::default()),
            cursors: CursorStore::new(),
            matcher: Matcher::new(MatcherLimits::default(), true),
            prompt,
            prompt_error,
            alt: AltScreen::new(),
            waiters: Vec::new(),
            finished: VecDeque::new(),
            producer: None,
            rx_end: 0,
            rx_bytes: 0,
            rx_chunks: 0,
            tx_bytes: 0,
            tx_chunks: 0,
            lost_events: 0,
            lost_chunks: 0,
            lost_bytes: 0,
            ring_rejects: 0,
            last_mono_ns: 0,
            wall_minus_mono: None,
        }
    }

    /// Producer identity put into gap payloads' `producer_range` (contract 02 §2.4). Without
    /// it `producer_range` carries only `from`/`to` and the worker must add the identity.
    pub fn set_producer(&mut self, producer_id: &str, producer_epoch: u64) {
        self.producer = Some((producer_id.to_owned(), producer_epoch));
    }

    /// Channel name.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Current port generation.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Channel profile.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// RX bytes so far = end of the RX offset space (lost ranges included).
    pub fn rx_end(&self) -> u64 {
        self.rx_end.max(self.ring.rx_end())
    }

    // -----------------------------------------------------------------------------------------
    // Ingest
    // -----------------------------------------------------------------------------------------

    /// One RX chunk: ring, expect matcher, prompt watcher, alternate-screen detector.
    pub fn on_rx(&mut self, c: &crate::bridge::RxChunk) -> Ingest {
        let data_payload = data_payload(&c.bytes, Some(c.rx_offset));
        if c.bytes.is_empty() {
            return Ingest {
                data_payload,
                hits: Vec::new(),
                prompt_seen: false,
                alt_screen: None,
            };
        }
        let pushed = self.ring.push(Entry::Data(Chunk {
            source_seq: c.source_seq,
            dir: Dir::Rx,
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            generation: self.generation,
            mono_ns: c.mono_ns,
            wall_ns: c.wall_ns,
            rx_offset: Some(c.rx_offset),
            bytes: c.bytes.clone(),
        }));
        if pushed.is_err() {
            self.ring_rejects += 1;
        }
        let len = c.bytes.len() as u64;
        self.rx_bytes += len;
        self.rx_chunks += 1;
        self.rx_end = self.rx_end.max(c.rx_offset + len);
        self.last_mono_ns = c.mono_ns;
        self.wall_minus_mono = Some(i128::from(c.wall_ns) - i128::from(c.mono_ns));

        let hits = self.matcher.feed(c.rx_offset, &c.bytes);
        for h in &hits {
            self.finish_waiter(h.id, Some(h.end_off));
        }
        let prompt_seen = self
            .prompt
            .as_mut()
            .is_some_and(|p| p.feed(c.rx_offset, &c.bytes));
        let alt_screen = self.alt.feed(&c.bytes);
        Ingest {
            data_payload,
            hits,
            prompt_seen,
            alt_screen,
        }
    }

    /// RX bytes dropped before the ring (reader bridge overflow): a `record` gap mark in the
    /// ring, matcher / prompt / escape state reset. Returns the gap payload (contract 02 §2.4:
    /// `{producer_range, count, cause, bytes, detail}`; no stream seq is invented).
    pub fn on_lost(&mut self, l: &crate::bridge::LostRange) -> Value {
        let pushed = self.ring.push(Entry::Gap(GapMark {
            layer: GapLayer::Record,
            cause: cut_str(l.cause, loom_console::ring::MAX_META_FIELD_BYTES).to_owned(),
            rx_offset_from: Some(l.rx_from),
            bytes: Some(l.bytes),
            source_seq_range: Some((l.seq_from, l.seq_to)),
            generation: self.generation,
            mono_ns: self.last_mono_ns,
        }));
        if pushed.is_err() {
            self.ring_rejects += 1;
        }
        self.reset_matching(ResetReason::Gap);
        self.lost_events += 1;
        self.lost_chunks += l.chunks;
        self.lost_bytes += l.bytes;
        self.rx_end = self.rx_end.max(l.rx_to);

        let mut range = json!({"from": num(l.seq_from), "to": num(l.seq_to)});
        if let (Some((id, epoch)), Value::Object(m)) = (&self.producer, &mut range) {
            m.insert("producer_id".into(), json!(id));
            m.insert("producer_epoch".into(), num(*epoch));
        }
        json!({
            "producer_range": range,
            "count": l.chunks.to_string(),
            "cause": l.cause,
            "bytes": num(l.bytes),
            "detail": {
                "channel": self.channel,
                "generation": self.generation,
                "rx_from": num(l.rx_from),
                "rx_to": num(l.rx_to),
                "chunks": l.chunks,
            },
        })
    }

    /// One TX chunk written to the device: into the ring (never into RX text), returns the
    /// `uart.data` payload (dir tx, no `rx_offset`).
    pub fn on_tx(&mut self, t: &crate::tx::TxChunk) -> Value {
        let payload = data_payload(&t.bytes, None);
        if t.bytes.is_empty() {
            return payload;
        }
        let wall_ns = self
            .wall_minus_mono
            .and_then(|d| u64::try_from(i128::from(t.mono_ns) + d).ok())
            .unwrap_or(0);
        let client = cut_str(&t.client_id, loom_console::ring::MAX_META_FIELD_BYTES);
        let pushed = self.ring.push(Entry::Data(Chunk {
            source_seq: t.source_seq,
            dir: Dir::Tx,
            origin: match t.origin {
                crate::tx::TxOrigin::User => Origin::User,
                crate::tx::TxOrigin::Agent => Origin::Agent,
            },
            client_id: (!client.is_empty()).then(|| client.to_owned()),
            request_id: Some(t.request_id.to_string()),
            generation: self.generation,
            mono_ns: t.mono_ns,
            wall_ns,
            rx_offset: None,
            bytes: t.bytes.clone(),
        }));
        if pushed.is_err() {
            self.ring_rejects += 1;
        }
        self.tx_bytes += t.bytes.len() as u64;
        self.tx_chunks += 1;
        payload
    }

    /// A new port session: matcher, prompt and escape state reset; the ring is kept (chunks
    /// carry their generation). Pending waiters stay and match the new generation's bytes.
    pub fn new_generation(&mut self, generation: u32) {
        self.generation = generation;
        self.reset_matching(ResetReason::Generation);
    }

    fn reset_matching(&mut self, why: ResetReason) {
        self.matcher.reset(why);
        if let Some(p) = &mut self.prompt {
            p.reset(why);
        }
        self.alt.reset_parser();
    }

    // -----------------------------------------------------------------------------------------
    // read
    // -----------------------------------------------------------------------------------------

    /// `uart.read` result data (plan §3.6, §4.4). Summary / Page move the client's cursor, Peek
    /// does not. At most [`MAX_READ_SCAN`] RX bytes are decoded:
    /// - Page / Peek over the budget stop at it: `more_available`, `scan_limit.resume_from`.
    /// - Summary over the budget decodes only the newest part (from a line start), reports
    ///   the older part as `skipped{from_rx,to_rx,bytes}` + `scan_limit.resume_from` (its start,
    ///   still readable with `--page`), and moves the cursor to the end (plan §3.6 "커서는 끝까지
    ///   전진").
    ///
    /// Shape: `{lines:[{text, rx_offset_start, rx_offset_end, source_seq, generation, complete,
    /// truncated?, elided_chars?}], truncated, omitted?, more_available, next_cursor, revision,
    /// cursor_gap?, binary_hint, gaps?, range{from_rx,to_rx}, scan_limit?, skipped?,
    /// evicted_client?}`.
    pub fn read(&mut self, client: &str, q: &ReadQuery) -> Result<Value, ChannelError> {
        CursorStore::check_name(client).map_err(cursor_err)?;
        let retained = self.ring.retained_from_rx_offset();
        let ring_end = self.ring.rx_end();

        // 1. Resolve the start (same rules as `cursor::read`).
        let mut inexact_gap = false;
        let (from, requested) = match &q.from {
            ReadFrom::Last => (
                ReadFrom::Last,
                self.cursors.get(client).map_or(retained, |p| p.rx_offset),
            ),
            ReadFrom::RxOffset(o) => (ReadFrom::RxOffset(*o), *o),
            ReadFrom::SourceSeq(s) => {
                if *s < self.ring.retained_from_source_seq() {
                    inexact_gap = true;
                    (ReadFrom::RxOffset(retained), retained)
                } else {
                    let o = self.ring.rx_offset_for_source_seq(*s);
                    (ReadFrom::RxOffset(o), o)
                }
            }
            ReadFrom::Token(t) => {
                let p = self.cursors.decode_token(t).map_err(cursor_err)?;
                (ReadFrom::Token(t.clone()), p.rx_offset)
            }
        };
        let start = requested.max(retained).min(ring_end);
        let end = q.until.map_or(ring_end, |u| u.min(ring_end)).max(start);

        let limits = ReadLimits {
            max_lines: q.max_lines.unwrap_or(cursor::DEFAULT_MAX_LINES),
            max_bytes: q.max_bytes.unwrap_or(cursor::DEFAULT_MAX_BYTES),
            ..ReadLimits::default()
        };
        let text = TextOptions {
            encoding: q.encoding,
            ansi: q.ansi,
            keep_cr: q.keep_cr,
            ..TextOptions::default()
        };

        // 2. Pick the decoded range.
        let mut skipped = None;
        let mut budget_cut = false;
        let (res, scanned_end) = if end == ring_end && end - start <= MAX_READ_SCAN {
            let r = cursor::read(
                &self.ring,
                &mut self.cursors,
                client,
                from,
                q.mode,
                &limits,
                &text,
            );
            (r.map_err(cursor_err)?, end)
        } else if q.mode == ReadMode::Summary && end == ring_end {
            let tail = align_to_line(&self.ring, end - MAX_READ_SCAN, end);
            let w = window_ring(&self.ring, tail, end);
            let r = cursor::read(&w, &mut self.cursors, client, from, q.mode, &limits, &text);
            skipped = Some((start, tail));
            (r.map_err(cursor_err)?, end)
        } else {
            let wend = end.min(start.saturating_add(MAX_READ_SCAN));
            let w = window_ring(&self.ring, start, wend);
            let r = cursor::read(&w, &mut self.cursors, client, from, q.mode, &limits, &text)
                .map_err(cursor_err)?;
            // Only a read that ran into the window end was cut by the budget (a page that
            // filled its line / byte caps first was not).
            budget_cut = wend < end && !r.more_available;
            (r, wend)
        };
        let ReadResult {
            mut lines,
            truncated,
            omitted,
            more_available,
            next_cursor,
            cursor_gap,
            binary_hint,
            from_rx_offset,
            next_rx_offset,
            revision,
            evicted_client,
        } = res;

        // A trailing "partial" line cut by the window end is not the device's line in progress.
        if budget_cut
            && lines
                .last()
                .is_some_and(|l| !l.complete && l.rx_offset_end >= scanned_end)
        {
            lines.pop();
        }
        let more_available = more_available || budget_cut;

        // 3. Range actually covered.
        let range_from = match skipped {
            Some((s, _)) => s,
            None => from_rx_offset,
        };
        let range_to = if more_available && q.mode != ReadMode::Summary {
            lines
                .last()
                .map_or(next_rx_offset, |l| l.rx_offset_end.max(next_rx_offset))
        } else {
            scanned_end
        };

        let mut out = json!({
            "lines": lines.iter().map(line_json).collect::<Vec<_>>(),
            "truncated": truncated || budget_cut || skipped.is_some(),
            "more_available": more_available,
            "next_cursor": next_cursor,
            "revision": revision,
            "binary_hint": binary_hint,
            "range": {"from_rx": num(range_from), "to_rx": num(range_to)},
        });
        let Value::Object(m) = &mut out else {
            return Ok(out);
        };
        if let Some(o) = omitted {
            m.insert(
                "omitted".into(),
                json!({
                    "from_source_seq": num(o.from_seq),
                    "to_source_seq": num(o.to_seq),
                    "lines": o.lines,
                    "bytes": num(o.bytes),
                    "from_rx": num(o.from_rx_offset),
                    "to_rx": num(o.to_rx_offset),
                    "at_line": o.at_line,
                }),
            );
        }
        // A real cursor gap is only one below the ring's retained start.
        let real_gap = if inexact_gap {
            Some(
                json!({"lost_from": null, "lost_to": num(retained), "exact": false, "earliest": num(retained)}),
            )
        } else {
            cursor_gap.filter(|_| requested < retained).map(|g| {
                json!({
                    "lost_from": num(g.lost_from),
                    "lost_to": num(retained),
                    "exact": g.exact,
                    "earliest": num(retained),
                })
            })
        };
        if let Some(g) = real_gap {
            m.insert("cursor_gap".into(), g);
        }
        if let Some((s, t)) = skipped {
            m.insert(
                "skipped".into(),
                json!({"from_rx": num(s), "to_rx": num(t), "bytes": num(t - s), "reason": "scan_limit"}),
            );
            m.insert(
                "scan_limit".into(),
                json!({"code": ErrorCode::ScanLimit.as_str(), "budget_bytes": MAX_READ_SCAN, "resume_from": num(s)}),
            );
        } else if budget_cut {
            m.insert(
                "scan_limit".into(),
                json!({"code": ErrorCode::ScanLimit.as_str(), "budget_bytes": MAX_READ_SCAN, "resume_from": num(next_rx_offset)}),
            );
        }
        if let Some(c) = evicted_client {
            m.insert("evicted_client".into(), json!(c));
        }
        gaps_in(&self.ring, range_from, range_to).put(&mut out);
        Ok(out)
    }

    /// Move `client`'s cursor forward to `rx_offset` (never backwards). For callers that know
    /// the client of an `ExpectFrom::Now` expect (`send --expect`, plan §4.5 "성공 시 커서 = 매치
    /// 끝"). `ExpectFrom::Cursor` expects do this themselves.
    pub fn commit_cursor(&mut self, client: &str, rx_offset: u64) -> Result<(), ChannelError> {
        CursorStore::check_name(client).map_err(cursor_err)?;
        self.advance_cursor(client, rx_offset);
        Ok(())
    }

    fn advance_cursor(&mut self, client: &str, off: u64) {
        let off = off.min(self.ring.rx_end());
        if self.cursors.get(client).is_none_or(|p| off > p.rx_offset) {
            // `None` = unconditional; only fails on a bad name, checked by the callers.
            let _ = self.cursors.commit(client, None, off, self.generation);
        }
    }

    // -----------------------------------------------------------------------------------------
    // lines
    // -----------------------------------------------------------------------------------------

    /// `uart.lines` (panel trace history): assembled RX lines (same boundaries as `read`) and
    /// TX rows, no cursor change. `from` set → forward from it; `from` unset → the last
    /// `limit` rows before `until`. At most [`LINES_SCAN_BYTES`] RX bytes and
    /// [`LINES_MAX_BYTES`] of row text.
    ///
    /// Shape: `{rows:[{dir:"rx", text, rx_offset_start, rx_offset_end, source_seq, generation,
    /// complete, …} | {dir:"tx", text, rx_offset, len, source_seq, generation, origin,
    /// client_id?, request_id?}], range{from_rx,to_rx}, more_before, more_after, next_from?,
    /// cursor_gap?, gaps?, scan_limit?}`.
    pub fn lines(&self, q: &LinesQuery) -> Result<Value, ChannelError> {
        let limit = usize::try_from(q.limit.clamp(1, LINES_MAX_LIMIT)).unwrap_or(2000);
        let retained = self.ring.retained_from_rx_offset();
        let ring_end = self.ring.rx_end();
        let until = q.until.map_or(ring_end, |u| u.min(ring_end));
        let backward = q.from.is_none();
        let (start, scan_end) = match q.from {
            Some(f) => {
                let s = f.max(retained).min(until);
                (s, until.min(s.saturating_add(LINES_SCAN_BYTES)))
            }
            None => {
                let s = until.saturating_sub(LINES_SCAN_BYTES).max(retained);
                (align_to_line(&self.ring, s, until), until)
            }
        };
        let want_rx = q.dir != Some(Dir::Tx);
        let want_tx = q.dir != Some(Dir::Rx);
        let opts = TextOptions {
            ansi: q.ansi,
            keep_cr: q.keep_cr,
            ..TextOptions::default()
        };

        let mut rows = RowCollector::new(limit, backward);
        let mut gaps = GapList::default();
        let mut pipe = TextPipeline::new(opts);
        let mut batch: Vec<Line> = Vec::new();
        // RX position while walking (TX rows are placed at the RX end of their time).
        let mut pos = retained;
        for e in self.ring.entries() {
            if rows.full() {
                break;
            }
            match e {
                Entry::Data(c) if c.dir == Dir::Rx => {
                    let Some(off) = c.rx_offset else { continue };
                    let cend = off + c.bytes.len() as u64;
                    pos = cend;
                    if cend <= start {
                        continue;
                    }
                    if off >= scan_end {
                        break;
                    }
                    if !want_rx {
                        continue;
                    }
                    let from = start.max(off);
                    let to = scan_end.min(cend);
                    let (a, b) = (
                        usize::try_from(from - off).unwrap_or(0),
                        usize::try_from(to - off).unwrap_or(c.bytes.len()),
                    );
                    for (i, piece) in c.bytes[a..b].chunks(render::FEED_STEP).enumerate() {
                        pipe.feed(
                            from + (i * render::FEED_STEP) as u64,
                            c.source_seq,
                            c.generation,
                            piece,
                            &mut batch,
                        );
                        for l in batch.drain(..) {
                            rows.push(Row::Rx(l));
                        }
                    }
                }
                Entry::Data(c) => {
                    if pos < start {
                        continue;
                    }
                    // TX after the newest RX byte belongs to the range when it ends at the ring end.
                    if pos > scan_end || (pos == scan_end && scan_end < ring_end) {
                        break;
                    }
                    if want_tx {
                        rows.push(Row::Tx {
                            text: escape_bytes(&c.bytes, TX_ROW_CHARS),
                            pos,
                            chunk: c,
                        });
                    }
                }
                Entry::Gap(g) => {
                    let at = g.rx_offset_from.unwrap_or(pos);
                    if at < start {
                        continue;
                    }
                    if at >= scan_end {
                        break;
                    }
                    gaps.push(g);
                    pipe.reset(&mut batch);
                    for l in batch.drain(..) {
                        rows.push(Row::Rx(l));
                    }
                }
            }
        }
        if want_rx
            && !rows.full()
            && let Some(p) = pipe.partial()
        {
            rows.push(Row::Rx(p));
        }

        let RowCollector {
            rows,
            dropped_front,
            stopped,
            last_end,
            ..
        } = rows;
        let more_before = backward && (dropped_front || start > retained);
        let more_after = !backward && (stopped || scan_end < until);
        let range_to = if stopped {
            last_end.unwrap_or(start)
        } else {
            scan_end
        };
        let mut out = json!({
            "rows": rows.iter().map(Row::to_json).collect::<Vec<_>>(),
            "range": {"from_rx": num(start), "to_rx": num(range_to)},
            "more_before": more_before,
            "more_after": more_after,
        });
        if let Value::Object(m) = &mut out {
            if more_after {
                m.insert("next_from".into(), num(range_to));
            }
            if let Some(f) = q.from.filter(|f| *f < retained) {
                m.insert(
                    "cursor_gap".into(),
                    json!({"lost_from": num(f), "lost_to": num(retained), "exact": true, "earliest": num(retained)}),
                );
            }
            if (!backward && scan_end < until && !stopped) || (backward && start > retained && !dropped_front)
            {
                m.insert(
                    "scan_limit".into(),
                    json!({"code": ErrorCode::ScanLimit.as_str(), "budget_bytes": LINES_SCAN_BYTES,
                           "resume_from": num(if backward { start } else { scan_end })}),
                );
            }
        }
        gaps.put(&mut out);
        Ok(out)
    }

    // -----------------------------------------------------------------------------------------
    // expect
    // -----------------------------------------------------------------------------------------

    /// Register expect waiter `id` (≤ 8 patterns; ≤ 8 waiters / 8 patterns per channel).
    ///
    /// `from` in the past is first re-scanned from the ring (at most [`EXPECT_BACKFILL_SCAN`]
    /// bytes; over it → [`ChannelError::ScanLimit`] and nothing stays registered). A start older
    /// than the ring → [`ChannelError::CursorGap`]. A match found by that scan is returned at
    /// once (the waiter is not kept); otherwise the waiter stays in the live matcher and fires
    /// from [`ChannelCore::on_rx`]. `Cursor(client)` expects move that cursor to the match end.
    pub fn expect_add(
        &mut self,
        id: u64,
        patterns: &[PatternSpec],
        from: ExpectFrom,
    ) -> Result<Vec<MatchHit>, ChannelError> {
        let prompt = self.profile.prompt.clone();
        let pats = patterns
            .iter()
            .map(|p| p.to_pattern(prompt.as_deref()))
            .collect::<Result<Vec<Pattern>, _>>()?;
        let retained = self.ring.retained_from_rx_offset();
        let (start, client) = match from {
            ExpectFrom::Now => (self.rx_end(), None),
            ExpectFrom::RxOffset(o) => (o, None),
            ExpectFrom::Cursor(c) => {
                CursorStore::check_name(&c).map_err(cursor_err)?;
                (self.cursors.get(&c).map_or(retained, |p| p.rx_offset), Some(c))
            }
        };
        if start < retained {
            return Err(ChannelError::CursorGap { earliest: retained });
        }
        // Admission and compile errors first (the live matcher owns the per-channel budget).
        self.matcher
            .add(id, pats.clone(), start)
            .map_err(ChannelError::Pattern)?;
        let specs: Vec<PatternSpec> = patterns
            .iter()
            .zip(&pats)
            .map(|(s, p)| PatternSpec {
                kind: s.kind,
                pattern: match p {
                    Pattern::Literal(_) => s.pattern.clone(),
                    Pattern::Regex(r) | Pattern::Prompt(r) => r.clone(),
                },
            })
            .collect();
        let literal_max = pats
            .iter()
            .map(|p| match p {
                Pattern::Literal(b) => Some(b.len()),
                _ => None,
            })
            .collect::<Option<Vec<usize>>>()
            .and_then(|v| v.into_iter().max());
        self.waiters.push(WaiterInfo {
            id,
            client,
            from: start,
            specs,
            literal_max,
        });

        match self.backfill(id, pats, start, literal_max) {
            Ok(Backfill::Clean) => Ok(Vec::new()),
            Ok(Backfill::Hit(h)) => {
                self.matcher.remove(id);
                self.finish_waiter(id, Some(h.end_off));
                Ok(vec![h])
            }
            Ok(Backfill::Limit(resume_from)) => {
                self.matcher.remove(id);
                self.waiters.retain(|w| w.id != id);
                Err(ChannelError::ScanLimit { resume_from })
            }
            Err(e) => {
                self.matcher.remove(id);
                self.waiters.retain(|w| w.id != id);
                Err(e)
            }
        }
    }

    /// Re-scan `[from, ring end)` with a private matcher (resets at gaps and generation changes,
    /// like the live one), bounded by [`EXPECT_BACKFILL_SCAN`].
    fn backfill(
        &self,
        id: u64,
        pats: Vec<Pattern>,
        from: u64,
        literal_max: Option<usize>,
    ) -> Result<Backfill, ChannelError> {
        let end = self.ring.rx_end();
        if from >= end {
            return Ok(Backfill::Clean);
        }
        let regexes = pats.iter().filter(|p| !matches!(p, Pattern::Literal(_))).count() as u64;
        let budget = (EXPECT_BACKFILL_SCAN / regexes.max(1)).max(EXPECT_BACKFILL_MIN);
        let mut m = Matcher::new(MatcherLimits::default(), true);
        m.add(id, pats, from).map_err(ChannelError::Pattern)?;
        let mut scanned = 0u64;
        let mut generation = None;
        for item in self.ring.rx_items_from(from) {
            match item {
                RxItem::Gap(_) => m.reset(ResetReason::Gap),
                RxItem::Data(s) => {
                    let Some(off) = s.rx_offset() else { continue };
                    let c = s.chunk;
                    if generation.is_some_and(|g| g != c.generation) {
                        m.reset(ResetReason::Generation);
                    }
                    generation = Some(c.generation);
                    let room = usize::try_from(budget - scanned).unwrap_or(usize::MAX);
                    let take = room.min(s.bytes().len());
                    if let Some(h) = m.feed(off, &s.bytes()[..take]).into_iter().next() {
                        return Ok(Backfill::Hit(h));
                    }
                    scanned += take as u64;
                    let scan_end = off + take as u64;
                    if scanned >= budget && scan_end < end {
                        // Regex overlap: half of what was scanned (≤ the window), so a retry
                        // from `resume_from` always makes progress.
                        let overlap = (scanned / 2).min(WINDOW_BYTES as u64);
                        return Ok(Backfill::Limit(resume_point(
                            literal_max,
                            from,
                            scan_end,
                            overlap,
                        )));
                    }
                }
            }
        }
        Ok(Backfill::Clean)
    }

    /// Forget waiter `id` (timeout / cancel / already delivered). Its info stays available to
    /// [`ChannelCore::expect_timeout`] / [`ChannelCore::expect_result`] for a while.
    pub fn expect_remove(&mut self, id: u64) {
        self.matcher.remove(id);
        self.finish_waiter(id, None);
    }

    fn finish_waiter(&mut self, id: u64, hit_end: Option<u64>) {
        let Some(i) = self.waiters.iter().position(|w| w.id == id) else {
            return;
        };
        let w = self.waiters.remove(i);
        if let (Some(end), Some(c)) = (hit_end, &w.client) {
            let c = c.clone();
            self.advance_cursor(&c, end);
        }
        if self.finished.len() >= FINISHED_WAITERS_MAX {
            self.finished.pop_front();
        }
        self.finished.push_back(w);
    }

    fn waiter(&self, id: u64) -> Option<&WaiterInfo> {
        self.waiters
            .iter()
            .find(|w| w.id == id)
            .or_else(|| self.finished.iter().rev().find(|w| w.id == id))
    }

    /// Generation of the RX byte at `off` (current generation if not in the ring).
    fn generation_at(&self, off: u64) -> u32 {
        self.ring
            .iter_from_rx_offset(off)
            .next()
            .map_or(self.generation, |s| s.chunk.generation)
    }

    /// Last `n` lines ending at `end`, decoded from at most [`CONTEXT_SCAN_BYTES`] and never
    /// before `floor`.
    fn context(&self, floor: u64, end: u64, n: usize) -> (Vec<Line>, GapList) {
        let retained = self.ring.retained_from_rx_offset();
        let lo = floor.max(retained).max(end.saturating_sub(CONTEXT_SCAN_BYTES));
        let lo = if lo > floor.max(retained) {
            align_to_line(&self.ring, lo, end)
        } else {
            lo
        };
        let mut lines = Vec::new();
        let mut gaps = GapList::default();
        render_range(
            &self.ring,
            lo,
            end,
            TextOptions::default(),
            true,
            &mut gaps,
            &mut lines,
        );
        let skip = lines.len().saturating_sub(n);
        (lines.split_off(skip), gaps)
    }

    /// `uart.expect` success data, also the `uart.expect_result` payload: `{matched:true,
    /// pattern_index, matched_index, pattern, kind, text, match, rx_offset_start,
    /// rx_offset_end, channel, generation, started_rx_offset, range{from_rx,to_rx}, before,
    /// before_lines, gaps?}`. `before` = the last ≤ 15 lines up to the match end.
    pub fn expect_result(&self, hit: &MatchHit, started_rx: u64) -> Value {
        let info = self.waiter(hit.id);
        let spec = info.and_then(|w| w.specs.get(hit.pattern_idx));
        let from = info.map_or(started_rx, |w| w.from.min(started_rx));
        let (before, gaps) = self.context(from.min(hit.start_off), hit.end_off, CONTEXT_LINES);
        let mut out = json!({
            "matched": true,
            "pattern_index": hit.pattern_idx,
            "matched_index": hit.pattern_idx,
            "pattern": spec.map(|s| s.pattern.clone()),
            "kind": spec.map(|s| s.kind),
            "text": hit.text,
            "match": hit.text,
            "rx_offset_start": num(hit.start_off),
            "rx_offset_end": num(hit.end_off),
            "channel": self.channel,
            "generation": self.generation_at(hit.start_off),
            "started_rx_offset": num(started_rx),
            "range": {"from_rx": num(from.min(hit.start_off)), "to_rx": num(hit.end_off)},
            "before": join_lines(&before),
            "before_lines": before.len(),
        });
        gaps.put(&mut out);
        out
    }

    /// Timeout data (also the `uart.expect_result` payload): `{matched:false, cause:"timeout",
    /// channel, generation, resume_from, started_rx_offset, scanned_to, patterns, context,
    /// context_lines, gaps?}`. Cursor unchanged. `resume_from` (plan §4.5): literals → scan
    /// end − (longest − 1); regex / prompt → start of the matcher window.
    pub fn expect_timeout(&self, id: u64, started_rx: u64) -> Value {
        let info = self.waiter(id);
        let end = self.rx_end();
        let from = info.map_or(started_rx, |w| w.from);
        let overlap = end - self.matcher.window_start_offset().unwrap_or(end).min(end);
        let resume = resume_point(info.and_then(|w| w.literal_max), from, end, overlap);
        let (ctx, gaps) = self.context(started_rx.min(from), self.ring.rx_end(), CONTEXT_LINES);
        let mut out = json!({
            "matched": false,
            "cause": "timeout",
            "channel": self.channel,
            "generation": self.generation,
            "resume_from": num(resume),
            "started_rx_offset": num(started_rx),
            "scanned_to": num(end),
            "patterns": info.map(|w| w.specs.clone()),
            "context": join_lines(&ctx),
            "context_lines": ctx.len(),
        });
        gaps.put(&mut out);
        out
    }

    /// `uart.run` output (plan §4.6): RX text from `started_rx` up to the prompt match, ANSI
    /// stripped, the command echo (first line) and the prompt removed. At most
    /// [`RUN_SCAN_BYTES`] decoded and [`RUN_MAX_LINES`] / [`RUN_MAX_BYTES`] returned (head 20 +
    /// tail, the middle counted in `omitted_lines`). Also carries the `uart.expect_result`
    /// fields (`matched`, `pattern_index`, `text`, offsets).
    pub fn run_output(&self, sent: &[u8], hit: &MatchHit, started_rx: u64) -> Value {
        let retained = self.ring.retained_from_rx_offset();
        let from = started_rx.max(retained);
        let to = hit.start_off.max(from);
        let head_cut = to - from > RUN_SCAN_BYTES;
        let lo = if head_cut {
            align_to_line(&self.ring, to - RUN_SCAN_BYTES, to)
        } else {
            from
        };
        let mut lines = Vec::new();
        let mut gaps = GapList::default();
        let binary_hint = render_range(
            &self.ring,
            lo,
            to,
            TextOptions::default(),
            true,
            &mut gaps,
            &mut lines,
        );

        // The prompt's own line start (e.g. `root@sim:~` before the matched `# `).
        let mut prompt_prefix = None;
        if lines.last().is_some_and(|l| !l.complete) {
            prompt_prefix = lines.pop().map(|l| l.text);
        }
        let cmd = String::from_utf8_lossy(sent);
        let cmd = cmd.trim_end_matches(['\r', '\n']);
        let mut echo_stripped = false;
        if !head_cut
            && !cmd.is_empty()
            && lines
                .first()
                .is_some_and(|l| l.text.trim_end() == cmd || l.text.trim_end().ends_with(cmd))
        {
            lines.remove(0);
            echo_stripped = true;
        }

        let total = lines.len();
        let (kept, omitted_lines) = cap_lines(lines);
        let mut out = json!({
            "matched": true,
            "pattern_index": hit.pattern_idx,
            "text": hit.text,
            "prompt": format!("{}{}", prompt_prefix.as_deref().unwrap_or(""), hit.text),
            "rx_offset_start": num(hit.start_off),
            "rx_offset_end": num(hit.end_off),
            "output": join_lines(&kept),
            "lines": total,
            "echo_stripped": echo_stripped,
            "prompt_stripped": true,
            "truncated": head_cut || omitted_lines > 0,
            "channel": self.channel,
            "generation": self.generation_at(hit.start_off),
            "started_rx_offset": num(started_rx),
            "range": {"from_rx": num(lo), "to_rx": num(to)},
            "binary_hint": binary_hint,
        });
        if let Value::Object(m) = &mut out {
            if omitted_lines > 0 {
                m.insert("omitted_lines".into(), json!(omitted_lines));
            }
            if head_cut {
                m.insert(
                    "skipped".into(),
                    json!({"from_rx": num(from), "to_rx": num(lo), "bytes": num(lo - from)}),
                );
            }
        }
        gaps.put(&mut out);
        out
    }

    // -----------------------------------------------------------------------------------------
    // status
    // -----------------------------------------------------------------------------------------

    /// Ring statistics, retained range, RX/TX counters, losses, pending expects, binary hint.
    pub fn status(&self) -> Value {
        let st = self.ring.stats();
        let mut pipe = TextPipeline::new(TextOptions::default());
        let mut sink = Vec::new();
        for s in self
            .ring
            .last_n_bytes(loom_console::text::BINARY_WINDOW_BYTES, false)
        {
            if let Some(off) = s.rx_offset() {
                pipe.feed(off, s.chunk.source_seq, s.chunk.generation, s.bytes(), &mut sink);
                sink.clear();
            }
        }
        json!({
            "channel": self.channel,
            "generation": self.generation,
            "profile": {
                "name": self.profile.name,
                "prompt": self.profile.prompt,
                "line_kill_ctrl_u": self.profile.line_kill_ctrl_u,
                "eol": self.profile.eol.as_str(),
                "prompt_error": self.prompt_error,
            },
            "rx_end": num(self.rx_end()),
            "retained_from": num(st.retained_from_rx_offset),
            "retained_from_source_seq": num(st.retained_from_source_seq),
            "last_source_seq": st.last_source_seq.map(num),
            "oldest_mono_ns": st.oldest_mono_ns.map(num),
            "rx": {"bytes": num(self.rx_bytes), "chunks": num(self.rx_chunks)},
            "tx": {"bytes": num(self.tx_bytes), "chunks": num(self.tx_chunks)},
            "lost": {"events": num(self.lost_events), "chunks": num(self.lost_chunks), "bytes": num(self.lost_bytes)},
            "ring": {
                "entries": st.entries,
                "payload_bytes": st.payload_bytes,
                "meta_bytes": st.meta_bytes,
                "slot_bytes": st.slot_capacity.saturating_mul(st.slot_size),
                "evicted_entries": num(st.evicted_entries),
                "evicted_bytes": num(st.evicted_bytes),
                "max_payload": st.limits.max_payload,
                "max_entries": st.limits.max_entries,
                "max_meta": st.limits.max_meta,
                "rejected": num(self.ring_rejects),
            },
            "expect": {
                "pending": self.waiters.len(),
                "patterns": self.matcher.patterns(),
                "max_waiters": self.matcher.limits().max_waiters,
                "max_patterns": self.matcher.limits().max_patterns,
                "window_bytes": self.matcher.window_len(),
            },
            "cursors": self.cursors.len(),
            "binary_hint": pipe.binary_hint(),
            "alt_screen": self.alt.on(),
        })
    }
}

/// `resume_from` rule of plan §4.5: literal → `scan_end − (longest − 1)` (lossless overlap),
/// regex / prompt → `scan_end − regex_overlap` (the matcher window); never before `from`.
fn resume_point(literal_max: Option<usize>, from: u64, scan_end: u64, regex_overlap: u64) -> u64 {
    let r = match literal_max {
        Some(n) => scan_end.saturating_sub(n.saturating_sub(1) as u64),
        None => scan_end.saturating_sub(regex_overlap),
    };
    r.max(from)
}

fn join_lines(lines: &[Line]) -> String {
    let mut s = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&l.text);
    }
    s
}

/// Keep head [`RUN_HEAD_LINES`] + as many tail lines as fit [`RUN_MAX_LINES`] /
/// [`RUN_MAX_BYTES`]; returns the kept lines and the omitted count.
fn cap_lines(lines: Vec<Line>) -> (Vec<Line>, usize) {
    let cost = |l: &Line| l.text.len() + 1;
    let total_bytes: usize = lines.iter().map(cost).sum();
    if lines.len() <= RUN_MAX_LINES && total_bytes <= RUN_MAX_BYTES {
        return (lines, 0);
    }
    let mut head = Vec::new();
    let mut bytes = 0usize;
    let mut it = lines.into_iter();
    for l in it.by_ref() {
        head.push(l);
        bytes += head.last().map_or(0, cost);
        if head.len() >= RUN_HEAD_LINES || bytes >= RUN_MAX_BYTES / 2 {
            break;
        }
    }
    let rest: Vec<Line> = it.collect();
    let mut tail = VecDeque::new();
    let mut tail_bytes = 0usize;
    for l in rest.iter().rev() {
        let c = cost(l);
        if head.len() + tail.len() >= RUN_MAX_LINES || bytes + tail_bytes + c > RUN_MAX_BYTES {
            break;
        }
        tail_bytes += c;
        tail.push_front(l.clone());
    }
    let omitted = rest.len() - tail.len();
    head.extend(tail);
    (head, omitted)
}

/// One `uart.lines` row before JSON (only kept rows are serialised).
#[derive(Debug)]
enum Row<'a> {
    Rx(Line),
    Tx {
        text: String,
        pos: u64,
        chunk: &'a Chunk,
    },
}

impl Row<'_> {
    fn cost(&self) -> usize {
        match self {
            Row::Rx(l) => l.text.len() + 1,
            Row::Tx { text, .. } => text.len() + 1,
        }
    }

    fn end(&self) -> u64 {
        match self {
            Row::Rx(l) => l.rx_offset_end,
            Row::Tx { pos, .. } => *pos,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Row::Rx(l) => {
                let mut v = line_json(l);
                if let Value::Object(m) = &mut v {
                    m.insert("dir".into(), json!("rx"));
                }
                v
            }
            Row::Tx { text, pos, chunk: c } => json!({
                "dir": "tx",
                "text": text,
                "rx_offset": num(*pos),
                "len": c.bytes.len(),
                "source_seq": num(c.source_seq),
                "generation": c.generation,
                "origin": render::origin_str(c.origin),
                "client_id": c.client_id,
                "request_id": c.request_id,
            }),
        }
    }
}

/// Rows of one `uart.lines` result (forward: stop at the cap; backward: keep the newest).
#[derive(Debug)]
struct RowCollector<'a> {
    rows: VecDeque<Row<'a>>,
    bytes: usize,
    limit: usize,
    backward: bool,
    dropped_front: bool,
    stopped: bool,
    last_end: Option<u64>,
}

impl<'a> RowCollector<'a> {
    fn new(limit: usize, backward: bool) -> Self {
        Self {
            rows: VecDeque::new(),
            bytes: 0,
            limit,
            backward,
            dropped_front: false,
            stopped: false,
            last_end: None,
        }
    }

    fn full(&self) -> bool {
        self.stopped
    }

    fn push(&mut self, row: Row<'a>) {
        if self.stopped {
            return;
        }
        let cost = row.cost();
        if !self.backward && (self.rows.len() >= self.limit || self.bytes + cost > LINES_MAX_BYTES) {
            self.stopped = true;
            return;
        }
        self.last_end = Some(row.end());
        self.rows.push_back(row);
        self.bytes += cost;
        while self.backward && (self.rows.len() > self.limit || self.bytes > LINES_MAX_BYTES) {
            if let Some(r) = self.rows.pop_front() {
                self.bytes -= r.cost();
            }
            self.dropped_front = true;
        }
    }
}
