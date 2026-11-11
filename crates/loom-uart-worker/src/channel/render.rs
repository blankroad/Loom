//! JSON shapes and bounded ring walks shared by the [`super::ChannelCore`] queries.
//!
//! u64 offsets / sequence numbers are decimal strings in JSON (same rule as `uart.data`
//! `rx_offset`, contract `crates/loom-uart/src/kinds.rs`).

use base64::Engine as _;
use loom_console::ring::{Chunk, Entry, GapLayer, GapMark, Ring, RxItem};
use loom_console::text::{Line, TextOptions, TextPipeline};
use loom_proto::event::{Dir, Origin};
use serde_json::{Value, json};

/// Gaps listed in one result; more are counted in `gaps_omitted` (§3.13 bounded output).
pub(crate) const MAX_GAPS_REPORTED: usize = 32;
/// How far a window start is moved forward to the next line start, at most (= the longest
/// partial line, `text::MAX_PARTIAL_LINE_BYTES`).
pub(crate) const LINE_ALIGN_SCAN: u64 = 64 * 1024;
/// Bytes fed to a text pipeline per step (`cursor::FEED_STEP_BYTES`, §3.13 "청크 ≤16 KiB").
pub(crate) const FEED_STEP: usize = 16 * 1024;

/// u64 as a JSON decimal string.
pub(crate) fn num(n: u64) -> Value {
    Value::String(n.to_string())
}

/// `uart.data` payload: `{bytes{encoding,data}, len, rx_offset?}`.
pub(crate) fn data_payload(bytes: &[u8], rx_offset: Option<u64>) -> Value {
    let mut v = json!({
        "bytes": {"encoding": "base64", "data": base64::engine::general_purpose::STANDARD.encode(bytes)},
        "len": bytes.len(),
    });
    if let (Some(o), Value::Object(m)) = (rx_offset, &mut v) {
        m.insert("rx_offset".into(), num(o));
    }
    v
}

/// One assembled RX line.
pub(crate) fn line_json(l: &Line) -> Value {
    let mut v = json!({
        "text": l.text,
        "rx_offset_start": num(l.rx_offset_start),
        "rx_offset_end": num(l.rx_offset_end),
        "source_seq": num(l.source_seq_first),
        "generation": l.generation,
        "complete": l.complete,
    });
    if let Value::Object(m) = &mut v {
        if l.truncated {
            m.insert("truncated".into(), json!(true));
        }
        if l.elided_chars > 0 {
            m.insert("elided_chars".into(), json!(l.elided_chars));
        }
    }
    v
}

fn layer_str(l: GapLayer) -> &'static str {
    match l {
        GapLayer::Display => "display",
        GapLayer::Record => "record",
        GapLayer::Source => "source",
    }
}

/// A gap mark inside a result range.
pub(crate) fn gap_json(g: &GapMark) -> Value {
    json!({
        "layer": layer_str(g.layer),
        "cause": g.cause,
        "rx_offset": g.rx_offset_from.map(num),
        "bytes": g.bytes.map(num),
        "source_seq_from": g.source_seq_range.map(|r| num(r.0)),
        "source_seq_to": g.source_seq_range.map(|r| num(r.1)),
        "generation": g.generation,
    })
}

/// Bounded gap list of a result.
#[derive(Debug, Default)]
pub(crate) struct GapList {
    items: Vec<Value>,
    omitted: u64,
}

impl GapList {
    pub(crate) fn push(&mut self, g: &GapMark) {
        if self.items.len() < MAX_GAPS_REPORTED {
            self.items.push(gap_json(g));
        } else {
            self.omitted += 1;
        }
    }

    /// Add `gaps` (and `gaps_omitted`) to `obj` when there are any.
    pub(crate) fn put(self, obj: &mut Value) {
        if let Value::Object(m) = obj {
            if !self.items.is_empty() {
                m.insert("gaps".into(), Value::Array(self.items));
            }
            if self.omitted > 0 {
                m.insert("gaps_omitted".into(), json!(self.omitted));
            }
        }
    }
}

/// Gap marks with `from <= rx position < to` (half-open: a gap at a read's end is reported by
/// the next read, never twice).
pub(crate) fn gaps_in(ring: &Ring, from: u64, to: u64) -> GapList {
    let mut out = GapList::default();
    if to <= from {
        return out;
    }
    for item in ring.rx_items_from(from) {
        match item {
            RxItem::Data(s) => {
                if s.rx_offset().is_some_and(|o| o >= to) {
                    break;
                }
            }
            RxItem::Gap(g) => {
                let at = g.rx_offset_from.unwrap_or(from);
                if at >= to {
                    break;
                }
                if at >= from {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Escaped preview of TX bytes: printable UTF-8 as is, `\r` `\n` `\t` `\\`, other bytes `\xNN`;
/// at most `max_chars` output characters (then `…[+N B]`).
pub(crate) fn escape_bytes(bytes: &[u8], max_chars: usize) -> String {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if chars >= max_chars {
            out.push_str(&format!("…[+{} B]", bytes.len() - i));
            break;
        }
        let b = bytes[i];
        let (piece, used): (String, usize) = match b {
            b'\r' => ("\\r".into(), 1),
            b'\n' => ("\\n".into(), 1),
            b'\t' => ("\\t".into(), 1),
            b'\\' => ("\\\\".into(), 1),
            0x20..=0x7e => (char::from(b).to_string(), 1),
            _ => {
                let end = (i + 4).min(bytes.len());
                match utf8_char_at(&bytes[i..end]) {
                    Some((c, n)) if !c.is_control() => (c.to_string(), n),
                    _ => (format!("\\x{b:02x}"), 1),
                }
            }
        };
        chars += piece.chars().count();
        out.push_str(&piece);
        i += used;
    }
    out
}

fn utf8_char_at(b: &[u8]) -> Option<(char, usize)> {
    (2..=b.len()).find_map(|n| {
        std::str::from_utf8(&b[..n])
            .ok()
            .and_then(|s| s.chars().next().map(|c| (c, n)))
    })
}

/// First line start at or after `start` (the byte after a `\n`), looking at most
/// [`LINE_ALIGN_SCAN`] bytes ahead and never past `end`. `start` itself when it already is a
/// line start or no `\n` is found.
pub(crate) fn align_to_line(ring: &Ring, start: u64, end: u64) -> u64 {
    if start == 0 || start <= ring.retained_from_rx_offset() {
        return start;
    }
    // Already at a line start?
    if let Some(prev) = ring.iter_from_rx_offset(start - 1).next()
        && prev.rx_offset() == Some(start - 1)
        && prev.bytes().first() == Some(&b'\n')
    {
        return start;
    }
    let limit = end.min(start.saturating_add(LINE_ALIGN_SCAN));
    for s in ring.iter_from_rx_offset(start) {
        let Some(off) = s.rx_offset() else { continue };
        if off >= limit {
            break;
        }
        let n = usize::try_from(limit - off)
            .unwrap_or(usize::MAX)
            .min(s.bytes().len());
        if let Some(p) = s.bytes()[..n].iter().position(|&b| b == b'\n') {
            let at = off + p as u64 + 1;
            return if at < end { at } else { start };
        }
    }
    start
}

/// A copy of the RX part of `ring` limited to `[start, end)` (gap marks kept), so the
/// loom-console readers can run on a bounded range (§3.6 "유한 작업"). Copies at most
/// `end - start` payload bytes.
pub(crate) fn window_ring(ring: &Ring, start: u64, end: u64) -> Ring {
    let mut w = Ring::new_at(ring.limits(), start);
    for item in ring.rx_items_from(start) {
        match item {
            RxItem::Data(s) => {
                let Some(off) = s.rx_offset() else { continue };
                if off >= end {
                    break;
                }
                let bytes = s.bytes();
                let take = usize::try_from(end - off).unwrap_or(usize::MAX).min(bytes.len());
                if take == 0 {
                    continue;
                }
                let c = s.chunk;
                // A fresh ring with the same limits accepts everything the source ring held.
                let _ = w.push(Entry::Data(Chunk {
                    source_seq: c.source_seq,
                    dir: Dir::Rx,
                    origin: c.origin,
                    client_id: None,
                    request_id: None,
                    generation: c.generation,
                    mono_ns: c.mono_ns,
                    wall_ns: c.wall_ns,
                    rx_offset: Some(off),
                    bytes: bytes[..take].to_vec(),
                }));
            }
            RxItem::Gap(g) => {
                if g.rx_offset_from.unwrap_or(w.rx_end()) >= end {
                    break;
                }
                let _ = w.push(Entry::Gap(g.clone()));
            }
        }
    }
    w
}

/// Assemble RX lines of `[start, end)` (gap marks reset the decoder and are collected).
/// `with_partial` appends the unterminated last line. Work is bounded by `end - start`.
pub(crate) fn render_range(
    ring: &Ring,
    start: u64,
    end: u64,
    opts: TextOptions,
    with_partial: bool,
    gaps: &mut GapList,
    out: &mut Vec<Line>,
) -> bool {
    let mut pipe = TextPipeline::new(opts);
    for item in ring.rx_items_from(start) {
        match item {
            RxItem::Gap(g) => {
                if g.rx_offset_from.unwrap_or(start) >= end {
                    break;
                }
                gaps.push(g);
                pipe.reset(out);
            }
            RxItem::Data(s) => {
                let Some(off) = s.rx_offset() else { continue };
                if off >= end {
                    break;
                }
                let n = usize::try_from(end - off)
                    .unwrap_or(usize::MAX)
                    .min(s.bytes().len());
                let c = s.chunk;
                for (i, piece) in s.bytes()[..n].chunks(FEED_STEP).enumerate() {
                    pipe.feed(
                        off + (i * FEED_STEP) as u64,
                        c.source_seq,
                        c.generation,
                        piece,
                        out,
                    );
                }
            }
        }
    }
    if with_partial && let Some(p) = pipe.partial() {
        out.push(p);
    }
    pipe.binary_hint()
}

/// `origin` wire name.
pub(crate) fn origin_str(o: Origin) -> &'static str {
    match o {
        Origin::Agent => "agent",
        Origin::User => "user",
        Origin::Device => "device",
        Origin::Script => "script",
    }
}

/// Cut `s` to at most `max` bytes on a char boundary.
pub(crate) fn cut_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut n = max;
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_preview() {
        assert_eq!(escape_bytes(b"ls -l\r", 64), "ls -l\\r");
        assert_eq!(escape_bytes("é\x01".as_bytes(), 64), "é\\x01");
        assert_eq!(escape_bytes(b"abcdef", 3), "abc…[+3 B]");
        assert_eq!(cut_str("aé", 2), "a");
    }
}
