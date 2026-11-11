//! Property tests: random layouts and values round-trip through encode → decode,
//! and encode touches only the signal's own bits.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_dbc::{Database, EncodeBase, EncodeOptions, LoadOptions, RawValue, SignalInput, SignalValue};
use proptest::prelude::*;

/// (motorola, signed, size, start bit, raw seed) for an 8-byte message.
fn layout() -> impl Strategy<Value = (bool, bool, u64, u64, u64)> {
    (
        any::<bool>(),
        any::<bool>(),
        1u64..=64,
        any::<u64>(),
        any::<u64>(),
    )
        .prop_map(|(moto, signed, size, s, raw)| {
            // Pick a start bit so the signal fits 8 bytes.
            let start = if moto {
                // MSB position: big-endian index b must satisfy b + size <= 64.
                let b = s % (64 - size + 1);
                (b / 8) * 8 + 7 - b % 8
            } else {
                s % (64 - size + 1)
            };
            (moto, signed, size, start, raw)
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn round_trip((moto, signed, size, start, seed) in layout()) {
        let text = format!(
            "VERSION \"\"\nBU_: E\nBO_ 1 M: 8 E\n SG_ S : {start}|{size}@{}{} (1,0) [0|0] \"\" E\n",
            if moto { 0 } else { 1 },
            if signed { "-" } else { "+" },
        );
        let (db, w) = Database::load(text.as_bytes(), &LoadOptions::default()).unwrap();
        prop_assert!(w.is_empty());
        let m = db.message_by_name("M").unwrap();
        let mask = if size == 64 { u64::MAX } else { (1u64 << size) - 1 };
        let bits = seed & mask;
        let raw: i128 = if signed && size < 64 && bits >> (size - 1) == 1 {
            bits as i128 - (1i128 << size)
        } else if signed && size == 64 {
            bits as i64 as i128
        } else {
            bits as i128
        };
        let e = m
            .encode(&[("S", SignalInput::Raw(raw))], EncodeBase::Initial, &EncodeOptions::default())
            .unwrap();
        // Only the signal's own bits may be set.
        let ones: u32 = e.frame.data().iter().map(|b| b.count_ones()).sum();
        prop_assert_eq!(ones, bits.count_ones());
        let d = m.decode_payload(e.frame.data()).unwrap();
        let got = match d.signals[0].value {
            SignalValue::Value { raw: RawValue::Unsigned(v), .. } => v as i128,
            SignalValue::Value { raw: RawValue::Signed(v), .. } => v as i128,
            ref other => panic!("{other:?}"),
        };
        prop_assert_eq!(got, raw);
        // Re-encoding on top of a full-ones base changes only the signal bits.
        let e2 = m
            .encode(&[("S", SignalInput::Raw(raw))], EncodeBase::Payload(&[0xFF; 8]), &EncodeOptions::default())
            .unwrap();
        let zeros: u32 = e2.frame.data().iter().map(|b| b.count_zeros()).sum();
        prop_assert_eq!(zeros, size as u32 - bits.count_ones());
    }

    #[test]
    fn physical_round_trip(rpm in 0.0f64..16000.0) {
        let bytes = std::fs::read(format!("{}/tests/fixtures/basic.dbc", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let (db, _) = Database::load(&bytes, &LoadOptions::default()).unwrap();
        let m = db.message_by_name("EngineData").unwrap();
        let e = m.encode(
            &[("EngRPM", SignalInput::Physical(rpm)), ("Volt", SignalInput::Physical(0.0))],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        ).unwrap();
        let d = m.decode_payload(e.frame.data()).unwrap();
        let SignalValue::Value { physical, .. } = d.signals[0].value else { panic!() };
        // Quantisation error is at most half a step (factor 0.25).
        prop_assert!((physical - rpm).abs() <= 0.125 + 1e-9);
    }
}
