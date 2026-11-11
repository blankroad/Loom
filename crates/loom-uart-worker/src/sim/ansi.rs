//! `ansi` — terminal escape corpus (UART plan §9.2, §5.4–5.5).
//!
//! Emits colors, cursor moves, a `\r` progress bar, alternate screen enter/leave, OSC 52 / OSC 0 /
//! OSC 8, mouse (`?1000h`) and focus (`?1004h`) reporting, and then **every** query sequence in
//! [`QUERY_SEQUENCES`] (one per line, labelled), followed by [`ANSI_DONE_MARKER`]. Anything the
//! host sends back (e.g. a terminal auto-reply that leaked through) is reported as
//! `rx: <escaped>\r\n`, so E2E tests can prove that no reply was injected.

use std::time::Duration;

use super::{Device, Signal, escape_bytes};

/// A terminal query sequence a sanitizer must not answer (shared with UI E2E / sanitize tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuerySeq {
    /// Short name (DA1, CPR, ...).
    pub name: &'static str,
    /// Exact bytes.
    pub bytes: &'static [u8],
}

/// All terminal queries the `ansi` scenario emits, in order.
pub const QUERY_SEQUENCES: &[QuerySeq] = &[
    QuerySeq {
        name: "DA1",
        bytes: b"\x1b[c",
    },
    QuerySeq {
        name: "DA2",
        bytes: b"\x1b[>c",
    },
    QuerySeq {
        name: "DA3",
        bytes: b"\x1b[=c",
    },
    QuerySeq {
        name: "XTVERSION",
        bytes: b"\x1b[>0q",
    },
    QuerySeq {
        name: "DSR",
        bytes: b"\x1b[5n",
    },
    QuerySeq {
        name: "CPR",
        bytes: b"\x1b[6n",
    },
    QuerySeq {
        name: "DECRQM",
        bytes: b"\x1b[?2004$p",
    },
    QuerySeq {
        name: "DECRQSS",
        bytes: b"\x1bP$qm\x1b\\",
    },
    QuerySeq {
        name: "XTWINOPS_18T",
        bytes: b"\x1b[18t",
    },
    QuerySeq {
        name: "KITTY_KBD_QUERY",
        bytes: b"\x1b[?u",
    },
    QuerySeq {
        name: "OSC10_QUERY",
        bytes: b"\x1b]10;?\x07",
    },
    QuerySeq {
        name: "OSC11_QUERY",
        bytes: b"\x1b]11;?\x1b\\",
    },
];

/// Line printed after the last query.
pub const ANSI_DONE_MARKER: &str = "ANSI-SIM DONE";

/// Most bytes of a single host input echoed back in an `rx:` report.
const RX_REPORT_MAX: usize = 256;

/// The `ansi` fake device.
#[derive(Debug, Default)]
pub struct Ansi;

impl Ansi {
    /// Creates the device.
    pub fn new() -> Self {
        Self
    }
}

/// The full corpus emitted on open.
pub fn ansi_corpus() -> Vec<u8> {
    let mut o: Vec<u8> = Vec::new();
    o.extend_from_slice(b"ansi: colors \x1b[31mred\x1b[0m \x1b[1;32mbold green\x1b[0m ");
    o.extend_from_slice(b"\x1b[38;5;208m256-color\x1b[0m \x1b[38;2;10;20;30mtruecolor\x1b[0m\r\n");
    o.extend_from_slice(b"ansi: cursor \x1b[2;5H\x1b[K\x1b[3A\x1b[2B\x1b[10C\x1b[4D\x1b[s\x1b[u\r\n");
    for pct in (0..=100).step_by(25) {
        o.extend_from_slice(format!("ansi: progress {pct:3}%\r").as_bytes());
    }
    o.extend_from_slice(b"\n");
    o.extend_from_slice(b"\x1b[?1049h\x1b[2J\x1b[Halt screen\x1b[?1049l");
    o.extend_from_slice(b"ansi: left alternate screen\r\n");
    o.extend_from_slice(b"\x1b]0;loom-sim title\x07");
    o.extend_from_slice(b"\x1b]52;c;bG9vbS1jbGlwYm9hcmQ=\x07");
    o.extend_from_slice(b"ansi: \x1b]8;;https://example.invalid/loom\x1b\\link\x1b]8;;\x1b\\\r\n");
    o.extend_from_slice(b"\x1b[?1000h\x1b[?1004h");
    o.extend_from_slice("ansi: utf-8 ✓ °C 한글\r\n".as_bytes());
    for q in QUERY_SEQUENCES {
        o.extend_from_slice(format!("query {}: ", q.name).as_bytes());
        o.extend_from_slice(q.bytes);
        o.extend_from_slice(b"\r\n");
    }
    o.extend_from_slice(ANSI_DONE_MARKER.as_bytes());
    o.extend_from_slice(b"\r\n");
    o
}

impl Device for Ansi {
    fn on_open(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&ansi_corpus());
    }

    fn on_bytes(&mut self, b: &[u8], _now: Duration, out: &mut Vec<u8>) {
        out.extend_from_slice(b"rx: ");
        escape_bytes(&b[..b.len().min(RX_REPORT_MAX)], out);
        out.extend_from_slice(b"\r\n");
    }

    fn tick(&mut self, _now: Duration, _out: &mut Vec<u8>) -> Option<Duration> {
        None
    }

    fn on_signal(&mut self, _s: Signal, _out: &mut Vec<u8>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_contains_every_query_and_marker() {
        let c = ansi_corpus();
        for q in QUERY_SEQUENCES {
            assert!(
                c.windows(q.bytes.len()).any(|w| w == q.bytes),
                "{} missing",
                q.name
            );
        }
        for must in [
            &b"\x1b[?1049h"[..],
            b"\x1b]52;",
            b"\x1b]0;",
            b"\x1b]8;;",
            b"\x1b[?1000h",
            b"\x1b[?1004h",
        ] {
            assert!(c.windows(must.len()).any(|w| w == must));
        }
        assert!(c.ends_with(format!("{ANSI_DONE_MARKER}\r\n").as_bytes()));
    }

    #[test]
    fn host_bytes_are_reported_escaped() {
        let mut d = Ansi::new();
        let mut out = Vec::new();
        d.on_bytes(b"\x1b[?1;2c", Duration::ZERO, &mut out);
        assert_eq!(out, b"rx: \\x1b[?1;2c\r\n");
    }
}
