//! Property checks for arbitrary bytes, bounded prefixes, and raw unfinished-line replay.
//!
//! Requires the lead-owned manifest to register `proptest` as a dev-dependency.

use std::sync::OnceLock;

use loom_proto::event::Risk;
use loom_uart::classify::{BASE_RULES, MAX_CLASSIFY_BYTES, MAX_LINE_BYTES, PROFILES, classify_text_with};
use proptest::prelude::*;
use regex::bytes::{Regex, RegexBuilder};

fn reference_rules() -> &'static Vec<(&'static str, Risk, Regex)> {
    static RULES: OnceLock<Vec<(&'static str, Risk, Regex)>> = OnceLock::new();
    RULES.get_or_init(|| {
        BASE_RULES
            .iter()
            .filter(|rule| {
                rule.id
                    .split_once('.')
                    .is_some_and(|(profile, _)| PROFILES.contains(&profile))
            })
            .map(|rule| {
                (
                    rule.id,
                    rule.risk,
                    RegexBuilder::new(rule.pattern)
                        .unicode(false)
                        .case_insensitive(true)
                        .build()
                        .expect("fixed oracle rules must compile"),
                )
            })
            .collect()
    })
}

// An independent, deliberately simple oracle for single lines without edit characters.
fn reference_risk(profile: &str, line: &[u8]) -> Risk {
    let generic = profile == "generic" || !PROFILES.contains(&profile);
    reference_rules()
        .iter()
        .filter(|(id, _, _)| {
            generic
                || id.starts_with("generic.")
                || id.split_once('.').is_some_and(|(owner, _)| owner == profile)
        })
        .filter(|(_, _, expression)| expression.is_match(line))
        .map(|(_, risk, _)| *risk)
        .max()
        .unwrap_or(Risk::R1)
}

fn profile(index: usize) -> &'static str {
    PROFILES.get(index).copied().unwrap_or("unknown-profile")
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(256) })]

    #[test]
    fn arbitrary_bytes_never_panic_or_change_input(
        state in proptest::collection::vec(any::<u8>(), 0..MAX_LINE_BYTES + 64),
        payload in proptest::collection::vec(any::<u8>(), 0..MAX_CLASSIFY_BYTES + 64),
        index in 0usize..7,
    ) {
        let original_state = state.clone();
        let original_payload = payload.clone();
        let verdict = classify_text_with(profile(index), &state, &payload);
        prop_assert!(verdict.risk >= Risk::R1 && verdict.risk <= Risk::R4);
        prop_assert!(verdict.reason.len() < 256);
        prop_assert_eq!(state, original_state);
        prop_assert_eq!(payload, original_payload);
    }

    #[test]
    fn each_fragment_replays_to_the_same_unfinished_line_verdict(
        data in proptest::collection::vec(any::<u8>(), 0..MAX_LINE_BYTES + 64),
        cuts in proptest::collection::vec(any::<usize>(), 1..16),
        index in 0usize..7,
    ) {
        // One raw line may contain BS/DEL/^U and Latin-1 bytes, but no line terminator.
        let line: Vec<u8> = data.into_iter().map(|byte| if matches!(byte, b'\r' | b'\n') { b'_' } else { byte }).collect();
        let mut cuts: Vec<_> = cuts.into_iter().map(|cut| cut % (line.len() + 1)).collect();
        cuts.push(line.len());
        cuts.sort_unstable();
        let mut start = 0;
        for end in cuts {
            let fragmented = classify_text_with(profile(index), &line[..start], &line[start..end]);
            let contiguous = classify_text_with(profile(index), &[], &line[..end]);
            prop_assert_eq!(fragmented, contiguous);
            start = end;
        }
    }

    #[test]
    fn byte_prefilter_agrees_with_individual_rule_oracle(
        data in proptest::collection::vec(any::<u8>(), 0..1024),
        index in 0usize..7,
    ) {
        let line: Vec<u8> = data.into_iter().map(|byte| if matches!(byte, b'\r' | b'\n' | 0x08 | 0x15 | 0x7f) { b'_' } else { byte }).collect();
        prop_assert_eq!(classify_text_with(profile(index), &[], &line).risk, reference_risk(profile(index), &line));
    }

    #[test]
    fn truncated_reason_matches_the_raw_prefix_limits(
        data in proptest::collection::vec(prop_oneof![Just(b'x'), Just(b'\r'), Just(b'\n')], 0..MAX_CLASSIFY_BYTES + 64),
        split in any::<usize>(),
    ) {
        let cut = split % (data.len() + 1);
        let prefix = &data[..data.len().min(MAX_CLASSIFY_BYTES)];
        let expected = data.len() > MAX_CLASSIFY_BYTES || prefix.split(|byte| matches!(byte, b'\r' | b'\n')).any(|line| line.len() > MAX_LINE_BYTES);
        let verdict = classify_text_with("generic", &data[..cut], &data[cut..]);
        prop_assert_eq!(verdict.reason.contains("truncated"), expected);
    }
}
