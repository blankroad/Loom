//! Unit-level behaviour of the display text pipeline (plan §3.6, §7.5).

use loom_console::text::{
    AnsiMode, BinaryDetector, Decoded, Encoding, Line, TextOptions, TextPipeline, Utf8Decoder, render_lines,
};

fn opts() -> TextOptions {
    TextOptions::default()
}

fn texts(lines: &[Line]) -> Vec<&str> {
    lines.iter().map(|l| l.text.as_str()).collect()
}

/// Feed `parts` as consecutive chunks (seq = index + 1) and return complete lines + partial.
fn feed_parts(o: TextOptions, parts: &[&[u8]]) -> (Vec<Line>, Option<Line>) {
    let mut p = TextPipeline::new(o);
    let mut out = Vec::new();
    let mut off = 0u64;
    for (i, part) in parts.iter().enumerate() {
        p.feed(off, i as u64 + 1, 0, part, &mut out);
        off += part.len() as u64;
    }
    (out, p.partial())
}

#[test]
fn terminators_lf_crlf_and_offsets() {
    let (lines, partial) = feed_parts(opts(), &[b"a\nbb\r\nc"]);
    assert_eq!(texts(&lines), ["a", "bb"]);
    assert_eq!((lines[0].rx_offset_start, lines[0].rx_offset_end), (0, 2));
    assert_eq!((lines[1].rx_offset_start, lines[1].rx_offset_end), (2, 6));
    assert!(lines.iter().all(|l| l.complete && !l.truncated));
    let p = partial.expect("partial line");
    assert_eq!(
        (p.text.as_str(), p.complete, p.rx_offset_start, p.rx_offset_end),
        ("c", false, 6, 7)
    );
}

#[test]
fn crlf_split_across_chunks() {
    let (lines, partial) = feed_parts(opts(), &[b"abc\r", b"\ndef\n"]);
    assert_eq!(texts(&lines), ["abc", "def"]);
    assert_eq!(lines[0].rx_offset_end, 5);
    assert_eq!(lines[1].source_seq_first, 2);
    assert!(partial.is_none());
}

#[test]
fn cr_overwrite_keeps_last_state() {
    let (lines, _) = feed_parts(opts(), &[b"10%\r20%\r", b"100%\r\n"]);
    assert_eq!(texts(&lines), ["100%"]);
    assert_eq!((lines[0].rx_offset_start, lines[0].rx_offset_end), (0, 14));
    // "\r\r\n" is one terminator, not an overwrite with an empty segment.
    let (lines, _) = feed_parts(opts(), &[b"x\r\r\ny\n"]);
    assert_eq!(texts(&lines), ["x", "y"]);
    // A progress bar redrawn with "\r ESC[K" collapses too.
    let (lines, _) = feed_parts(opts(), &[b"1/3\r\x1b[K2/3\r\x1b[K3/3\n"]);
    assert_eq!(texts(&lines), ["3/3"]);
    // Segment-wise, not column-wise: "abc\rX" shows "X".
    let (lines, _) = feed_parts(opts(), &[b"abc\rX\n"]);
    assert_eq!(texts(&lines), ["X"]);
}

#[test]
fn keep_cr_makes_lone_cr_a_terminator() {
    let o = TextOptions {
        keep_cr: true,
        ..opts()
    };
    let (lines, partial) = feed_parts(o, &[b"10%\r20%\r", b"100%\r\nnext"]);
    assert_eq!(texts(&lines), ["10%", "20%", "100%"]);
    assert_eq!((lines[1].rx_offset_start, lines[1].rx_offset_end), (4, 8));
    assert_eq!(lines[2].source_seq_first, 2);
    assert_eq!(partial.expect("partial").text, "next");
}

#[test]
fn utf8_split_at_every_boundary() {
    let s = "é€😀 ok\n".as_bytes();
    for cut in 0..s.len() {
        let (lines, _) = feed_parts(opts(), &[&s[..cut], &s[cut..]]);
        assert_eq!(texts(&lines), ["é€😀 ok"], "cut at {cut}");
    }
    // One byte at a time.
    let parts: Vec<&[u8]> = s.chunks(1).collect();
    let (lines, _) = feed_parts(opts(), &parts);
    assert_eq!(texts(&lines), ["é€😀 ok"]);
}

#[test]
fn invalid_utf8_is_hex_escaped() {
    let lines = render_lines(opts(), 0, b"a\xffb\xc3(\xed\xa0\x80\xf0\x9f\n");
    assert_eq!(texts(&lines), [r"a\xffb\xc3(\xed\xa0\x80\xf0\x9f"]);

    let mut d = Utf8Decoder::new();
    let mut got = Vec::new();
    d.decode(b"\xe2\x82", &mut |x| got.push(x));
    assert!(got.is_empty());
    assert_eq!(d.pending(), b"\xe2\x82");
    d.flush(&mut |x| got.push(x));
    assert_eq!(got, [Decoded::Invalid(0xe2), Decoded::Invalid(0x82)]);
}

#[test]
fn truncated_utf8_before_reset_is_shown_as_hex() {
    let mut p = TextPipeline::new(opts());
    let mut out = Vec::new();
    p.feed(0, 1, 0, b"ab\xe2\x82", &mut out);
    p.reset(&mut out);
    assert_eq!(texts(&out), [r"ab\xe2\x82"]);
    assert!(!out[0].complete);
    // After the reset the decoder starts clean: a lone continuation byte is invalid.
    out.clear();
    p.feed(10, 2, 0, b"\xacz\n", &mut out);
    assert_eq!(texts(&out), [r"\xacz"]);
}

#[test]
fn ansi_strip_keep_escape() {
    let input: &[&[u8]] = &[b"\x1b", b"[31mred\x1b[0", b"m \x1b]0;title\x07x\x1bP+q\x1b\\y\n"];
    let (lines, _) = feed_parts(opts(), input);
    assert_eq!(texts(&lines), ["red xy"]);

    let esc = TextOptions {
        ansi: AnsiMode::Escape,
        ..opts()
    };
    let (lines, _) = feed_parts(esc, input);
    assert_eq!(texts(&lines), [r"\e[31mred\e[0m \e]0;title\x07x\eP+q\e\y"]);

    let keep = TextOptions {
        ansi: AnsiMode::Keep,
        ..opts()
    };
    let (lines, _) = feed_parts(keep, input);
    assert_eq!(
        texts(&lines),
        ["\u{1b}[31mred\u{1b}[0m \u{1b}]0;title\u{7}x\u{1b}P+q\u{1b}\\y"]
    );
}

#[test]
fn overlong_escape_is_dropped_and_counted() {
    let mut input = b"\x1b]".to_vec();
    input.extend(std::iter::repeat_n(b'a', 5000));
    input.extend_from_slice(b"\x07ok\n");
    let mut p = TextPipeline::new(opts());
    let mut out = Vec::new();
    p.feed(0, 1, 0, &input, &mut out);
    assert_eq!(texts(&out), ["ok"]);
    assert_eq!(p.counters().overlong_escapes, 1);

    // Unterminated overlong OSC: agent text resynchronises at the next LF.
    let mut input = b"\x1b]".to_vec();
    input.extend(std::iter::repeat_n(b'a', 5000));
    input.extend_from_slice(b"\nvisible\n");
    let lines = render_lines(opts(), 0, &input);
    assert_eq!(lines.last().map(|l| l.text.as_str()), Some("visible"));
}

#[test]
fn control_chars_are_escaped() {
    let lines = render_lines(opts(), 0, b"\x07bell\tx\x00\x7f\xc2\x85\n");
    assert_eq!(texts(&lines), [r"\x07bell	x\x00\x7f\u{85}"]);
}

#[test]
fn latin1_maps_bytes_directly() {
    let o = TextOptions {
        encoding: Encoding::Latin1,
        ..opts()
    };
    let lines = render_lines(o, 0, b"caf\xe9 \x85 \xff\n");
    assert_eq!(texts(&lines), [r"café \x85 ÿ"]);
}

#[test]
fn display_line_cap_512_chars() {
    let mut input = vec![b'x'; 600];
    input.push(b'\n');
    let lines = render_lines(opts(), 0, &input);
    let l = &lines[0];
    assert_eq!(l.elided_chars, 88);
    assert_eq!(l.text, format!("{}…[+88 chars]", "x".repeat(512)));
    assert_eq!(l.rx_offset_end, 601);
}

#[test]
fn partial_line_cap_64k_breaks_and_marks_truncated() {
    let mut input = vec![b'a'; 70_000];
    input.push(b'\n');
    let mut p = TextPipeline::new(opts());
    let mut out = Vec::new();
    for c in input.chunks(4096) {
        let off = p.next_offset().unwrap_or(0);
        p.feed(off, 1, 0, c, &mut out);
    }
    assert_eq!(out.len(), 2);
    assert!(out[0].truncated && out[0].complete);
    assert_eq!((out[0].rx_offset_start, out[0].rx_offset_end), (0, 65_536));
    assert_eq!(out[0].elided_chars, 65_536 - 512);
    assert!(!out[1].truncated);
    assert_eq!((out[1].rx_offset_start, out[1].rx_offset_end), (65_536, 70_001));
    assert_eq!(p.counters().truncated_lines, 1);
}

#[test]
fn generation_change_and_offset_jump_reset() {
    let mut p = TextPipeline::new(opts());
    let mut out = Vec::new();
    p.feed(0, 1, 0, b"abc", &mut out);
    p.feed(3, 2, 1, b"def\n", &mut out);
    assert_eq!(texts(&out), ["abc", "def"]);
    assert_eq!((out[0].complete, out[0].generation), (false, 0));
    assert_eq!((out[1].complete, out[1].generation), (true, 1));

    out.clear();
    p.feed(7, 3, 1, b"gh", &mut out);
    p.feed(100, 4, 1, b"ij\n", &mut out); // hole 9..100
    assert_eq!(texts(&out), ["gh", "ij"]);
    assert_eq!(out[1].rx_offset_start, 100);
    assert_eq!(p.counters().resets, 2);
}

#[test]
fn source_seq_first_is_the_chunk_of_the_first_byte() {
    let (lines, _) = feed_parts(opts(), &[b"ab\nc", b"d\n"]);
    assert_eq!((lines[1].text.as_str(), lines[1].source_seq_first), ("cd", 1));
    let (lines, _) = feed_parts(opts(), &[b"ab\n", b"cd\n"]);
    assert_eq!(lines[1].source_seq_first, 2);
}

#[test]
fn binary_detector_flags_garbage_not_text() {
    let mut p = TextPipeline::new(opts());
    let mut out = Vec::new();
    let text = b"U-Boot 2024.01 \x1b[1mDRAM\x1b[0m: 512 MiB\r\n".repeat(50);
    p.feed(0, 1, 0, &text, &mut out);
    assert!(!p.binary_hint());

    // Wrong-baud style noise: mostly high / control bytes.
    let noise: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8 | 0x80)
        .collect();
    let mut p = TextPipeline::new(opts());
    p.feed(0, 1, 0, &noise, &mut out);
    assert!(p.binary_hint());

    let mut d = BinaryDetector::new();
    d.record_n(63, true);
    assert!(!d.likely_binary(), "below minimum sample");
    d.record(true);
    assert!(d.likely_binary());
    d.record_n(4096, false);
    assert_eq!(d.counts(), (4096, 0));
}
