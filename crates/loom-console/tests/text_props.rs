//! Chunk-split invariance of the text pipeline (plan §7.5 proptest 대상).

use loom_console::text::{AnsiMode, Encoding, Line, TextCounters, TextOptions, TextPipeline};
use proptest::prelude::*;

/// Alphabet biased towards the interesting cases: UTF-8 lead/continuation bytes, escapes,
/// CR/LF, BEL/ST terminators and invalid bytes.
fn byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        4 => prop::sample::select(b"abc xyz019;[]?m\\".to_vec()),
        2 => Just(b'\r'),
        2 => Just(b'\n'),
        2 => Just(0x1b),
        1 => Just(0x07),
        1 => prop::sample::select(vec![0xc3u8, 0xa9, 0xe2, 0x82, 0xac, 0xf0, 0x9f, 0x98, 0x80, 0xed, 0xff, 0x00, 0x85]),
        1 => any::<u8>(),
    ]
}

fn options() -> impl Strategy<Value = TextOptions> {
    (
        prop::sample::select(vec![AnsiMode::Strip, AnsiMode::Keep, AnsiMode::Escape]),
        prop::sample::select(vec![Encoding::Utf8, Encoding::Latin1]),
        any::<bool>(),
        prop::sample::select(vec![16u32, 512]),
        prop::sample::select(vec![16usize, 64, 65_536]),
    )
        .prop_map(
            |(ansi, encoding, keep_cr, line_chars, max_line_bytes)| TextOptions {
                encoding,
                ansi,
                keep_cr,
                line_chars,
                max_line_bytes,
            },
        )
}

fn run(o: TextOptions, input: &[u8], cuts: &[usize]) -> (Vec<Line>, Option<Line>, TextCounters) {
    let mut p = TextPipeline::new(o);
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut cuts: Vec<usize> = cuts.iter().map(|c| c % (input.len() + 1)).collect();
    cuts.sort_unstable();
    cuts.push(input.len());
    for c in cuts {
        if c < pos {
            continue;
        }
        // One seq for all chunks: seq attribution legitimately depends on the split.
        p.feed(pos as u64, 7, 0, &input[pos..c], &mut out);
        pos = c;
    }
    (out, p.partial(), p.counters())
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(512) })]

    #[test]
    fn split_invariance(o in options(), input in prop::collection::vec(byte(), 0..400),
                        cuts in prop::collection::vec(any::<usize>(), 0..12)) {
        let whole = run(o, &input, &[]);
        let split = run(o, &input, &cuts);
        prop_assert_eq!(&whole.0, &split.0);
        prop_assert_eq!(&whole.1, &split.1);
        prop_assert_eq!(whole.2, split.2);
    }

    #[test]
    fn lines_tile_the_input(o in options(), input in prop::collection::vec(byte(), 0..400)) {
        let (lines, partial, _) = run(o, &input, &[]);
        let mut expect_start = 0u64;
        for l in lines.iter().chain(partial.iter()) {
            prop_assert_eq!(l.rx_offset_start, expect_start);
            prop_assert!(l.rx_offset_end > l.rx_offset_start);
            prop_assert!(l.rx_offset_end - l.rx_offset_start <= o.max_line_bytes as u64);
            let shown = l.text.chars().count() as u64;
            let cap = u64::from(o.line_chars);
            if l.elided_chars == 0 {
                prop_assert!(shown <= cap);
            } else {
                let suffix = format!("…[+{} chars]", l.elided_chars);
                prop_assert!(l.text.ends_with(&suffix));
            }
            expect_start = l.rx_offset_end;
        }
    }
}
