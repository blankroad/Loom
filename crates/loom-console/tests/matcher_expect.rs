//! Bounded expect matcher (plan §7.4, §4.5, §3.13 expect 행).

use std::time::Instant;

use loom_console::matcher::{
    MAX_PATTERN_BYTES, MatchError, MatchHit, Matcher, MatcherLimits, Pattern, ResetReason, WINDOW_BYTES,
};
use proptest::prelude::*;

fn lit(s: &str) -> Pattern {
    Pattern::Literal(s.as_bytes().to_vec())
}

fn re(s: &str) -> Pattern {
    Pattern::Regex(s.to_owned())
}

fn m() -> Matcher {
    Matcher::new(MatcherLimits::default(), true)
}

fn feed_bytewise(m: &mut Matcher, start: u64, bytes: &[u8]) -> Vec<MatchHit> {
    let mut hits = Vec::new();
    for (i, b) in bytes.iter().enumerate() {
        hits.extend(m.feed(start + i as u64, std::slice::from_ref(b)));
    }
    hits
}

#[test]
fn literal_across_boundaries_one_byte_at_a_time() {
    let mut m = m();
    m.add(1, vec![lit("login:")], 0).expect("add");
    let hits = feed_bytewise(&mut m, 0, b"booting...\r\ndut login: ");
    assert_eq!(hits.len(), 1);
    let h = &hits[0];
    assert_eq!((h.id, h.pattern_idx, h.start_off, h.end_off), (1, 0, 16, 22));
    assert_eq!(h.text, "login:");
    assert_eq!(m.waiters(), 0);
    assert_eq!(m.patterns(), 0);
}

#[test]
fn regex_across_boundaries() {
    let mut m = m();
    m.add(7, vec![re(r"Linux version \d+\.\d+")], 0).expect("add");
    let hits = feed_bytewise(&mut m, 100, b"[    0.000000] Linux version 6.1.0 (gcc)");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "Linux version 6.1");
    assert_eq!(hits[0].start_off, 115);
}

#[test]
fn earliest_start_wins_then_pattern_index() {
    let mut m = m();
    m.add(1, vec![lit("panic"), lit("Kernel panic"), lit("login:")], 0)
        .expect("add");
    let hits = m.feed(0, b"Kernel panic - not syncing\n");
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].pattern_idx, hits[0].start_off), (1, 0));
}

#[test]
fn ansi_is_stripped_but_offsets_stay_raw() {
    let mut m = m();
    m.add(1, vec![lit("login:")], 0).expect("add");
    let input = b"lo\x1b[1mgi\x1b[0mn: ";
    let hits = feed_bytewise(&mut m, 0, input);
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].start_off, hits[0].end_off), (0, 14));
    assert_eq!(hits[0].text, "login:");

    let mut raw = Matcher::new(MatcherLimits::default(), false);
    raw.add(1, vec![lit("login:")], 0).expect("add");
    assert!(raw.feed(0, input).is_empty());
}

#[test]
fn prompt_is_anchored_at_end_of_data() {
    let mut m = m();
    m.add(1, vec![Pattern::Prompt(r"[#$] ".into())], 0).expect("add");
    assert!(m.feed(0, b"echo $ x\r\n").is_empty());
    let hits = m.feed(10, b"root@dut:~# ");
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].start_off, hits[0].end_off), (20, 22));
}

#[test]
fn limits_each() {
    let mut m = m();
    let long = "a".repeat(MAX_PATTERN_BYTES + 1);
    assert_eq!(
        m.add(1, vec![lit("ok"), Pattern::Regex(long.clone())], 0),
        Err(MatchError::PatternTooLong {
            idx: 1,
            len: MAX_PATTERN_BYTES + 1
        })
    );
    assert!(matches!(
        m.add(1, vec![Pattern::Literal(long.into_bytes())], 0),
        Err(MatchError::PatternTooLong { .. })
    ));
    assert!(matches!(
        m.add(1, vec![re(r"(\w{100}){100}")], 0),
        Err(MatchError::CompileTooBig { idx: 0 })
    ));
    assert!(matches!(
        m.add(1, vec![re(r"(a)\1")], 0),
        Err(MatchError::Syntax { .. })
    ));
    assert!(matches!(
        m.add(1, vec![re(r"(?=a)")], 0),
        Err(MatchError::Syntax { .. })
    ));
    let deep = format!("{}a{}", "(".repeat(70), ")".repeat(70));
    assert!(matches!(
        m.add(1, vec![re(&deep)], 0),
        Err(MatchError::Syntax { .. })
    ));
    assert_eq!(m.add(1, vec![], 0), Err(MatchError::EmptyPattern));
    assert_eq!(m.add(1, vec![lit("")], 0), Err(MatchError::EmptyPattern));
    assert_eq!(m.waiters(), 0, "failed adds leave nothing behind");

    // 8 patterns per channel in total.
    m.add(1, vec![lit("a"), lit("b"), lit("c"), lit("d"), lit("e")], 0)
        .expect("5");
    assert_eq!(
        m.add(2, vec![lit("f"), lit("g"), lit("h"), lit("i")], 0),
        Err(MatchError::TooManyPatterns)
    );
    m.add(2, vec![lit("f"), lit("g"), lit("h")], 0).expect("8");
    assert_eq!(m.add(3, vec![lit("z")], 0), Err(MatchError::TooManyPatterns));
    assert_eq!(m.add(2, vec![lit("z")], 0), Err(MatchError::DuplicateWaiter));
    m.remove(1);
    assert_eq!(m.patterns(), 3);

    // 8 waiters per channel.
    let mut m = self::m();
    for id in 0..8 {
        m.add(id, vec![lit("x")], 0).expect("waiter");
    }
    assert_eq!(m.add(8, vec![lit("x")], 0), Err(MatchError::TooManyWaiters));

    // Configured limits never exceed the plan values.
    let e = MatcherLimits {
        pattern_bytes: 1 << 20,
        size_limit: 1 << 30,
        dfa_size_limit: 1 << 30,
        nest_limit: 1000,
        window: 1 << 30,
        max_patterns: 100,
        max_waiters: 100,
    }
    .effective();
    assert_eq!(e, MatcherLimits::default());
}

#[test]
fn eight_waiters_fire_together() {
    let mut m = m();
    let words = ["alpha", "beta", "gamma", "delta", "eps", "zeta", "eta", "theta"];
    for (i, w) in words.iter().enumerate() {
        m.add(i as u64, vec![lit(w)], 0).expect("add");
    }
    let text = words.join(" ");
    let hits = feed_bytewise(&mut m, 0, text.as_bytes());
    // "eta" also occurs inside "beta"/"zeta"/"theta" — it fires at the first one ("beta").
    let mut ids: Vec<u64> = hits.iter().map(|h| h.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, (0..8).collect::<Vec<_>>());
    let eta = hits.iter().find(|h| h.id == 6).expect("eta");
    assert_eq!(eta.start_off, 7);
    assert_eq!(m.waiters(), 0);
}

#[test]
fn reset_never_joins_bytes_across_a_gap() {
    for reason in [ResetReason::Gap, ResetReason::Generation] {
        let mut m = m();
        m.add(1, vec![lit("login:")], 0).expect("add");
        m.add(2, vec![re("lo.in:")], 0).expect("add");
        assert!(m.feed(0, b"logi").is_empty());
        m.reset(reason);
        assert!(m.feed(4, b"n: ").is_empty());
        assert_eq!(m.waiters(), 2, "waiters survive the reset");
        let hits = m.feed(7, b"login:");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| h.start_off == 7));
    }
    // An offset jump is an implicit gap.
    let mut m = m();
    m.add(1, vec![lit("abcd")], 0).expect("add");
    assert!(m.feed(0, b"ab").is_empty());
    assert!(m.feed(50, b"cd").is_empty());
}

#[test]
fn from_offset_before_window_starts_at_window_and_future_offset_waits() {
    let mut m = m();
    let filler = vec![b'.'; WINDOW_BYTES + 10_000];
    m.feed(0, b"OLD-MARK");
    m.feed(8, &filler);
    assert!(m.window_len() <= WINDOW_BYTES);
    // OLD-MARK left the window: a waiter from offset 0 cannot see it.
    m.add(1, vec![lit("OLD-MARK")], 0).expect("add");
    assert!(m.poll().is_empty());
    // A waiter from a future offset ignores bytes before it.
    let end = 8 + filler.len() as u64;
    m.add(2, vec![lit("X")], end + 3).expect("add");
    let hits = m.feed(end, b"XXXXX");
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].id, hits[0].start_off), (2, end + 3));
    // A waiter added after the bytes arrived finds them with poll().
    m.add(3, vec![lit("XXX")], end).expect("add");
    let hits = m.poll();
    assert_eq!((hits[0].id, hits[0].start_off), (3, end));
}

#[test]
fn window_and_offset_map_stay_bounded() {
    let mut m = m();
    m.add(1, vec![lit("never-there")], 0).expect("add");
    let colored = b"\x1b[32mok\x1b[0m ".repeat(100_000); // ~1.2 MB, many stripped runs
    for (i, c) in colored.chunks(4096).enumerate() {
        m.feed((i * 4096) as u64, c);
        assert!(m.window_len() <= WINDOW_BYTES);
        assert!(m.segment_count() <= m.window_len().max(1));
    }
    assert_eq!(m.waiters(), 1);
}

#[test]
fn pathological_regex_on_64k_window_is_linear() {
    let mut m = m();
    m.add(1, vec![re("(a|aa)*b")], 0).expect("add");
    let input = vec![b'a'; WINDOW_BYTES];
    let t = Instant::now();
    // Fed in 4 KiB chunks: 16 feeds, each one `find` over the whole waiter window.
    for (i, c) in input.chunks(4096).enumerate() {
        assert!(m.feed((i * 4096) as u64, c).is_empty());
    }
    let feeds = t.elapsed();
    let t = Instant::now();
    assert!(m.poll().is_empty()); // one single find over the full 64 KiB window
    let single = t.elapsed();
    eprintln!("PATHOLOGICAL (a|aa)*b: 16 feeds of 4 KiB = {feeds:?}, single 64 KiB find = {single:?}");
    assert!(feeds.as_secs_f64() < 10.0 && single.as_secs_f64() < 2.0);
}

fn literal_hits(input: &[u8], cuts: &[usize], needle: &str) -> Vec<(u64, u64)> {
    let mut m = Matcher::new(MatcherLimits::default(), true);
    m.add(1, vec![lit(needle)], 0).expect("add");
    m.add(2, vec![re(&regex_escape(needle))], 0).expect("add");
    let mut cuts: Vec<usize> = cuts.iter().map(|c| c % (input.len() + 1)).collect();
    cuts.sort_unstable();
    cuts.push(input.len());
    let mut pos = 0;
    let mut out = Vec::new();
    for c in cuts {
        if c < pos {
            continue;
        }
        for h in m.feed(pos as u64, &input[pos..c]) {
            out.push((h.id, h.start_off));
        }
        pos = c;
    }
    out.sort_unstable();
    out
}

fn regex_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() {
                vec![c]
            } else {
                vec!['\\', c]
            }
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(256) })]

    #[test]
    fn split_invariant_hits(prefix in prop::collection::vec(prop::sample::select(b"ab: \x1b[m\r\n".to_vec()), 0..200),
                            cuts in prop::collection::vec(any::<usize>(), 0..10)) {
        let mut input = prefix;
        // CAN ends any escape sequence the random prefix left open (a bare ESC would
        // legitimately swallow the next byte under strip_ansi).
        input.extend_from_slice(b"\x18ab:ba");
        let whole = literal_hits(&input, &[], "ab:ba");
        let split = literal_hits(&input, &cuts, "ab:ba");
        prop_assert_eq!(whole.len(), 2);
        prop_assert_eq!(whole, split);
    }
}
