//! Explicit, repeatable unoptimized hook timing; run separately to avoid parallel test noise.

use loom_uart::classify::{MAX_CLASSIFY_BYTES, MAX_LINE_BYTES, PROFILES, classify_text_with, warm};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

#[test]
#[ignore = "timing probe: run with --ignored --nocapture --test-threads=1"]
fn warmed_32k_input_p99() {
    warm();
    let mut regular = vec![b'x'; MAX_CLASSIFY_BYTES];
    for offset in (MAX_LINE_BYTES - 1..MAX_CLASSIFY_BYTES).step_by(MAX_LINE_BYTES) {
        regular[offset] = b'\n';
    }
    let mut late_risk = regular.clone();
    let risk = b";sf erase 0 100";
    let offset = MAX_CLASSIFY_BYTES - risk.len() - 1;
    late_risk[offset..offset + risk.len()].copy_from_slice(risk);
    let mut many_lines = vec![b'\n'; MAX_CLASSIFY_BYTES];
    for offset in (0..MAX_CLASSIFY_BYTES).step_by(2) {
        many_lines[offset] = b'x';
    }
    let mut edited = regular.clone();
    let mut edited_late_risk = late_risk.clone();
    for offset in (20..MAX_CLASSIFY_BYTES).step_by(64) {
        edited[offset] = 0x08;
        edited_late_risk[offset] = 0x08;
    }
    let long_line = vec![b'x'; MAX_CLASSIFY_BYTES];
    let mut all_passed = true;
    for profile in PROFILES {
        for (name, input) in [
            ("regular", &regular),
            ("late_risk", &late_risk),
            ("many_lines", &many_lines),
            ("edited", &edited),
            ("edited_late_risk", &edited_late_risk),
            ("long_line", &long_line),
        ] {
            for _ in 0..16 {
                black_box(classify_text_with(profile, &[], black_box(input)));
            }
            let mut samples = Vec::with_capacity(200);
            for _ in 0..200 {
                let start = Instant::now();
                black_box(classify_text_with(profile, &[], black_box(input)));
                samples.push(start.elapsed());
            }
            samples.sort_unstable();
            let p50 = samples[99];
            let p99 = samples[197];
            let max = samples[199];
            println!(
                "profile={profile} input={name} bytes={} n=200 p50_us={:.3} p99_us={:.3} max_us={:.3} debug_assertions={}",
                input.len(),
                p50.as_secs_f64() * 1e6,
                p99.as_secs_f64() * 1e6,
                max.as_secs_f64() * 1e6,
                cfg!(debug_assertions)
            );
            all_passed &= p99 < Duration::from_millis(1);
        }
    }
    assert!(
        all_passed,
        "one or more 32 KiB p99 measurements exceeded the 1 ms hook budget"
    );
}
