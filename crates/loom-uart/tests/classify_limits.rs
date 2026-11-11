//! Boundary-oriented deterministic checks of the fast, no-command long-line detector.

use loom_uart::classify::{MAX_CLASSIFY_BYTES, MAX_LINE_BYTES, classify_text_with};

fn check(data: &[u8]) {
    let prefix = &data[..data.len().min(MAX_CLASSIFY_BYTES)];
    let expected = data.len() > MAX_CLASSIFY_BYTES
        || prefix
            .split(|byte| matches!(byte, b'\r' | b'\n'))
            .any(|line| line.len() > MAX_LINE_BYTES);
    let verdict = classify_text_with("generic", &[], data);
    assert_eq!(
        verdict.reason.contains("truncated"),
        expected,
        "length={}",
        data.len()
    );
}

#[test]
fn newline_positions_on_both_sides_of_half_chunk_and_line_limits() {
    for length in [
        MAX_LINE_BYTES - 1,
        MAX_LINE_BYTES,
        MAX_LINE_BYTES + 1,
        MAX_CLASSIFY_BYTES,
        MAX_CLASSIFY_BYTES + 1,
    ] {
        for stride in [
            MAX_LINE_BYTES / 2 - 1,
            MAX_LINE_BYTES / 2,
            MAX_LINE_BYTES / 2 + 1,
            MAX_LINE_BYTES,
            MAX_LINE_BYTES + 1,
        ] {
            for offset in [0, 1, stride / 2, stride - 1] {
                let mut data = vec![b'x'; length];
                for index in (offset..length).step_by(stride) {
                    data[index] = if index % 2 == 0 { b'\r' } else { b'\n' };
                }
                check(&data);
            }
        }
    }
}

#[test]
fn dense_newlines_and_an_isolated_overlong_line_are_both_detected() {
    let mut data = vec![b'\n'; MAX_CLASSIFY_BYTES];
    for offset in [
        0,
        1,
        MAX_LINE_BYTES / 2 - 1,
        MAX_LINE_BYTES / 2,
        MAX_CLASSIFY_BYTES - MAX_LINE_BYTES - 1,
    ] {
        for length in [MAX_LINE_BYTES, MAX_LINE_BYTES + 1] {
            data.fill(b'\n');
            data[offset..offset + length].fill(b'x');
            check(&data);
        }
    }
}
