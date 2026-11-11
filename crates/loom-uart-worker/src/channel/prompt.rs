//! Prompt watcher: "is the profile prompt the last thing the device printed?" after each RX
//! chunk (UART plan §3.7 rule 3, InputArbiter `prompt_seen`).
//!
//! Uses its own small [`Matcher`] (so it never takes one of the 8 expect slots) with one
//! [`Pattern::Prompt`] waiter. The waiter is re-armed only after a hit, so the regex is compiled
//! once per prompt seen, not per chunk.

use loom_console::matcher::{MatchError, Matcher, MatcherLimits, Pattern, ResetReason};
use loom_console::text::{AnsiFilter, ByteClass};

/// Window of the prompt matcher (§3.13: bounded; prompts are short — the matcher's minimum).
pub(crate) const PROMPT_WINDOW_BYTES: usize = 1024;

const PROMPT_WAITER: u64 = 0;

#[derive(Debug)]
pub(crate) struct PromptWatch {
    m: Matcher,
    pattern: String,
    armed: bool,
    next_from: u64,
}

impl PromptWatch {
    /// `Err` if the profile prompt does not compile under the matcher limits.
    pub(crate) fn new(pattern: &str) -> Result<Self, MatchError> {
        let limits = MatcherLimits {
            window: PROMPT_WINDOW_BYTES,
            ..MatcherLimits::default()
        };
        let mut w = Self {
            m: Matcher::new(limits, true),
            pattern: pattern.to_owned(),
            armed: false,
            next_from: 0,
        };
        w.arm(0)?;
        Ok(w)
    }

    fn arm(&mut self, from: u64) -> Result<(), MatchError> {
        self.m.remove(PROMPT_WAITER);
        self.m
            .add(PROMPT_WAITER, vec![Pattern::Prompt(self.pattern.clone())], from)?;
        self.armed = true;
        Ok(())
    }

    pub(crate) fn reset(&mut self, reason: ResetReason) {
        self.m.reset(reason);
    }

    /// Feed one RX chunk; `true` when the chunk ends with the prompt (only escape sequences may
    /// follow it).
    pub(crate) fn feed(&mut self, rx_offset: u64, bytes: &[u8]) -> bool {
        if !self.armed && self.arm(self.next_from).is_err() {
            return false;
        }
        let hits = self.m.feed(rx_offset, bytes);
        let Some(h) = hits.last() else {
            return false;
        };
        self.armed = false;
        self.next_from = h.end_off;
        if tail_is_escapes(rx_offset, bytes, h.end_off) {
            return true;
        }
        // The hit was in the middle of the chunk: look for a prompt at the very end.
        if self.arm(h.end_off).is_err() {
            return false;
        }
        let again = self.m.poll();
        match again.last() {
            Some(h2) => {
                self.armed = false;
                self.next_from = h2.end_off;
                tail_is_escapes(rx_offset, bytes, h2.end_off)
            }
            None => false,
        }
    }
}

/// `true` when every byte of `bytes` at or after raw offset `end` is part of an escape sequence.
fn tail_is_escapes(rx_offset: u64, bytes: &[u8], end: u64) -> bool {
    let skip = usize::try_from(end.saturating_sub(rx_offset)).unwrap_or(usize::MAX);
    let Some(tail) = bytes.get(skip..) else {
        return true;
    };
    let mut f = AnsiFilter::new();
    tail.iter().all(|&b| f.classify(b) == ByteClass::Sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_at_end_only() {
        let mut w = PromptWatch::new(r"[#$] $").expect("valid prompt");
        assert!(!w.feed(0, b"booting\r\n"));
        assert!(w.feed(9, b"root@sim:~# "));
        assert!(!w.feed(21, b"ls\r\n"));
        assert!(!w.feed(25, b"a.txt # b\r\n"));
        // Prompt split across chunks, then a trailing escape sequence.
        assert!(!w.feed(36, b"root@sim:~"));
        assert!(w.feed(46, b"# \x1b[?2004h"));
    }

    #[test]
    fn prompt_mid_chunk_then_at_end() {
        let mut w = PromptWatch::new(r"=> $").expect("valid prompt");
        let mut chunk = b"=> ".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 3000));
        chunk.extend(b"\r\n=> ");
        assert!(w.feed(0, &chunk));
        let mut chunk2 = b"=> ".to_vec();
        chunk2.extend(std::iter::repeat_n(b'y', 3000));
        assert!(!w.feed(chunk.len() as u64, &chunk2));
    }
}
