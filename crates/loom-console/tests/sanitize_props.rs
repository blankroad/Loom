//! Sanitizer counters, state, chunk independence and adversarial bounds
//! (UART plan §5.4, §5.5 ①, §7.5; task p0-opus A).

use loom_console::sanitize::{
    BlockKind, Blocked, Counters, GAP_RESET, MAX_SEQ_BYTES, RECENT_CAP, SanitizeConfig, Sanitizer,
};
use proptest::prelude::*;

fn sanitize(cfg: SanitizeConfig, input: &[u8]) -> (Vec<u8>, Sanitizer) {
    let mut s = Sanitizer::new(cfg);
    let mut out = Vec::new();
    s.feed(input, &mut out);
    (out, s)
}

#[test]
fn every_query_is_counted() {
    let input = b"\x1b[c\x1b[>c\x1b[=c\x1b[>q\x1b[5n\x1b[6n\x1b[?6n\x1b[?2004$p\x1b[4$p\x1b[?u\x1b[0x";
    let (out, s) = sanitize(SanitizeConfig::web(), input);
    assert!(out.is_empty(), "{}", out.escape_ascii());
    assert_eq!(s.counters().query, 11);
    assert_eq!(s.counters().total_blocked(), 11);
}

#[test]
fn strings_are_classified() {
    let input = b"\x1b]0;t\x07\x1b]2;t\x1b\\\x1b]11;?\x07\x1b]8;;u\x1b\\L\x1b]8;;\x1b\\\x1b]52;c;x\x07\x1b]777;x\x07\x1bP$qm\x1b\\\x1b_a\x1b\\\x1b]\x07";
    let (out, s) = sanitize(SanitizeConfig::web(), input);
    assert_eq!(out, b"L");
    let c = s.counters();
    assert_eq!(
        (
            c.title,
            c.osc_color,
            c.osc8,
            c.osc52,
            c.osc_other,
            c.dcs,
            c.apc_pm_sos
        ),
        (2, 1, 2, 1, 2, 1, 1)
    );
}

#[test]
fn mixed_decset_counts_each_removed_mode() {
    let (out, s) = sanitize(SanitizeConfig::web(), b"\x1b[?25;1000;1004;9999;2004h");
    assert_eq!(out, b"\x1b[?25;2004h");
    assert_eq!(
        (s.counters().mouse, s.counters().focus, s.counters().mode),
        (1, 1, 1)
    );
    assert!(s.bracketed_paste());
}

#[test]
fn mouse_passes_when_allowed() {
    let cfg = SanitizeConfig {
        allow_mouse: true,
        ..SanitizeConfig::web()
    };
    let (out, s) = sanitize(cfg, b"\x1b[?1000;1006h\x1b[?1004h");
    assert_eq!(out, b"\x1b[?1000;1006h");
    assert_eq!(s.counters().focus, 1);
}

#[test]
fn alt_screen_and_bracketed_paste_are_mirrored() {
    let mut s = Sanitizer::new(SanitizeConfig::web());
    let mut out = Vec::new();
    assert!(!s.alt_screen());
    s.feed(b"\x1b[?1049h", &mut out);
    assert!(s.alt_screen());
    s.feed(b"\x1b[?47l", &mut out);
    assert!(!s.alt_screen());
    s.feed(b"\x1b[?1047h\x1b[?2004h", &mut out);
    assert!(s.alt_screen() && s.bracketed_paste());
    s.feed(b"\x1b[?2004l", &mut out);
    assert!(!s.bracketed_paste());
}

#[test]
fn recent_list_has_kind_and_absolute_offset() {
    let mut s = Sanitizer::new(SanitizeConfig::web());
    let mut out = Vec::new();
    s.feed(b"ab", &mut out);
    s.feed(b"c\x1b[6", &mut out);
    s.feed(b"n\x05", &mut out);
    assert_eq!(
        s.take_recent(),
        vec![
            Blocked {
                kind: BlockKind::Query,
                offset: 3
            },
            Blocked {
                kind: BlockKind::C0,
                offset: 7
            },
        ]
    );
    assert!(s.take_recent().is_empty());
}

#[test]
fn recent_ring_is_bounded() {
    let input: Vec<u8> = std::iter::repeat_n(0x05u8, RECENT_CAP * 3).collect();
    let (_, mut s) = sanitize(SanitizeConfig::web(), &input);
    let recent = s.take_recent();
    assert_eq!(recent.len(), RECENT_CAP);
    assert_eq!(recent[0].offset, (RECENT_CAP * 2) as u64, "oldest dropped first");
    assert_eq!(s.counters().c0, (RECENT_CAP * 3) as u64);
}

#[test]
fn on_gap_drops_partial_state_and_resets_sgr() {
    let mut s = Sanitizer::new(SanitizeConfig::web());
    let mut out = Vec::new();
    s.feed(b"\x1b[1;3", &mut out);
    s.feed(b"\xed\x95", &mut out);
    assert!(s.buffered_bytes() > 0);
    s.on_gap(&mut out);
    assert_eq!(out, GAP_RESET);
    assert_eq!(s.buffered_bytes(), 0);
    // `1m` after the gap is plain text, not the tail of the old sequence.
    s.feed(b"1mX", &mut out);
    assert_eq!(&out[GAP_RESET.len()..], b"1mX");
}

#[test]
fn tui_clamps_scroll_region_and_follows_resize() {
    let mut s = Sanitizer::new(SanitizeConfig::tui(24));
    let mut out = Vec::new();
    s.feed(b"\x1b[2;24r", &mut out);
    s.set_rows(40);
    s.feed(b"\x1b[r", &mut out);
    s.feed(b"\x1b[30;20r", &mut out); // top ≥ bottom → dropped
    assert_eq!(out, b"\x1b[2;23r\x1b[1;39r");
    assert_eq!(s.counters().csi, 1);
}

#[test]
fn web_passes_scroll_region_verbatim() {
    let (out, _) = sanitize(SanitizeConfig::web(), b"\x1b[2;24r\x1b[r");
    assert_eq!(out, b"\x1b[2;24r\x1b[r");
}

#[test]
fn unterminated_10_mib_osc_does_not_buffer() {
    let mut s = Sanitizer::new(SanitizeConfig::web());
    let mut out = Vec::new();
    s.feed(b"\x1b]52;c;", &mut out);
    let block = vec![b'A'; 1 << 20];
    for _ in 0..10 {
        s.feed(&block, &mut out);
        assert!(s.buffered_bytes() <= MAX_SEQ_BYTES + 3);
    }
    assert!(out.is_empty());
    assert_eq!(s.counters().overlong, 1);
    s.feed(b"\x07after", &mut out);
    assert_eq!(out, b"after");
    assert_eq!(s.counters().osc52, 1);
}

/// xorshift64* — deterministic bytes without a rand dependency.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

/// Random bytes biased towards escape-sequence structure.
fn structured_noise(seed: u64, len: usize) -> Vec<u8> {
    const ALPHABET: &[u8] = b"\x1b\x1b\x1b[[]]P_^X?;;0123456789hlmcnqtpu$ >=\x07\x18\\\x05\r\nAz";
    noise(seed, len)
        .into_iter()
        .enumerate()
        .map(|(i, b)| {
            if i % 3 == 0 {
                b
            } else {
                ALPHABET[usize::from(b) % ALPHABET.len()]
            }
        })
        .collect()
}

#[test]
fn adversarial_one_mib_is_bounded() {
    for (seed, input) in [
        (1u64, noise(0x9E37_79B9, 1 << 20)),
        (2, structured_noise(0xC0FF_EE11, 1 << 20)),
    ] {
        for cfg in [SanitizeConfig::web(), SanitizeConfig::tui(24)] {
            let mut s = Sanitizer::new(cfg);
            let mut out = Vec::new();
            for part in input.chunks(1500) {
                s.feed(part, &mut out);
                assert!(s.buffered_bytes() <= MAX_SEQ_BYTES + 3);
            }
            s.on_gap(&mut out);
            // Each invalid UTF-8 byte may become U+FFFD (3 bytes); a TUI DECSTBM
            // `ESC [ r` (3 bytes) may become `ESC [ 1 ; 23 r` (8 bytes).
            let bound = 3 * input.len() + GAP_RESET.len();
            assert!(out.len() <= bound, "seed {seed}: {} > {bound}", out.len());
            // Nothing that starts a query reply-trigger survives.
            assert!(
                !out.windows(3).any(|w| w == b"\x1b[c" || w == b"\x1b]5"),
                "seed {seed}"
            );
        }
    }
}

fn run_split(input: &[u8], cuts: &[usize]) -> (Vec<u8>, Counters, Vec<Blocked>, bool, bool) {
    let mut s = Sanitizer::new(SanitizeConfig::web());
    let mut out = Vec::new();
    let mut prev = 0;
    let mut points: Vec<usize> = cuts.iter().map(|c| c % (input.len() + 1)).collect();
    points.sort_unstable();
    for p in points {
        s.feed(&input[prev..p], &mut out);
        prev = p;
    }
    s.feed(&input[prev..], &mut out);
    let recent = s.take_recent();
    (
        out,
        s.counters().clone(),
        recent,
        s.alt_screen(),
        s.bracketed_paste(),
    )
}

fn byte_strategy() -> impl Strategy<Value = u8> {
    prop_oneof![
        3 => Just(0x1Bu8),
        6 => prop::sample::select(b"[]P_^X?;0123456789hlmcnqtupr$ >=\\(B".to_vec()),
        1 => prop::sample::select(vec![0x07u8, 0x18, 0x1A, 0x05, 0x0D, 0x0A, 0x7F]),
        2 => 0x80u8..=0xFF,
        3 => 0x20u8..=0x7E,
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Same bytes split anywhere → same output, counters, recent list and modes.
    #[test]
    fn chunk_independence(
        input in prop::collection::vec(byte_strategy(), 0..1500),
        cuts in prop::collection::vec(any::<usize>(), 0..12),
    ) {
        let whole = run_split(&input, &[]);
        let split = run_split(&input, &cuts);
        prop_assert_eq!(whole, split);
    }

    /// No passed CSI asks the terminal for a reply.
    #[test]
    fn no_query_survives(input in prop::collection::vec(byte_strategy(), 0..1500)) {
        let (out, ..) = run_split(&input, &[]);
        let mut s = Sanitizer::new(SanitizeConfig::web());
        let mut again = Vec::new();
        s.feed(&out, &mut again);
        // Output is a fixed point: sanitizing it again blocks nothing.
        prop_assert_eq!(s.counters().total_blocked(), 0, "{}", out.escape_ascii());
        prop_assert_eq!(again, out);
    }
}
