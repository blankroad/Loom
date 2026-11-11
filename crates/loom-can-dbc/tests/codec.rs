//! Encode/decode against hand-computed bytes (no external tool; expected values by hand).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_dbc::{
    Database, DecodedMessage, EncodeBase, EncodeError, EncodeOptions, LoadOptions, Message, RawValue,
    SignalInput, SignalValue, Unavailable, ValueSource,
};
use loom_can_frame::{CanFrame, CanId};

fn load(name: &str) -> Database {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(path).unwrap();
    Database::load(&bytes, &LoadOptions::default()).unwrap().0
}

fn enc(m: &Message, values: &[(&str, SignalInput<'_>)]) -> Result<Vec<u8>, EncodeError> {
    m.encode(values, EncodeBase::Initial, &EncodeOptions::default())
        .map(|e| e.frame.data().to_vec())
}

fn phys(d: &DecodedMessage<'_>, name: &str) -> f64 {
    match &d.signals.iter().find(|s| s.signal.name() == name).unwrap().value {
        SignalValue::Value { physical, .. } => *physical,
        other => panic!("{name}: {other:?}"),
    }
}

fn names(d: &DecodedMessage<'_>) -> Vec<String> {
    d.signals.iter().map(|s| s.signal.name().to_string()).collect()
}

use SignalInput::{Label, Physical, Raw};

#[test]
fn intel_motorola_hand_vectors() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    // EngRPM 3000/0.25 = 12000 = 0x2EE0 (Intel, bytes 0..1 LE)      → E0 2E
    // EngTemp -10 signed 8 = 0xF6                                     → byte2 F6
    // Flag 1 → byte3 bit0; Gear "D" = 3 at bits 25..27 → 3<<1 = 06   → byte3 07
    // MotoA 0xABC Motorola MSB byte4 bit7, 12 bits → byte4 AB, byte5 C_
    // MotoB 5 Motorola MSB byte5 bit3, 4 bits → byte5 _5             → byte5 C5
    // Volt (12.34 + 1.5) / 0.01 = 1384 = 0x0568, Motorola MSB byte6 → 05 68
    let bytes = enc(
        m,
        &[
            ("EngRPM", Physical(3000.0)),
            ("EngTemp", Physical(-10.0)),
            ("Flag", Raw(1)),
            ("Gear", Label("D")),
            ("MotoA", Raw(0xABC)),
            ("MotoB", Raw(5)),
            ("Volt", Physical(12.34)),
        ],
    )
    .unwrap();
    assert_eq!(bytes, [0xE0, 0x2E, 0xF6, 0x07, 0xAB, 0xC5, 0x05, 0x68]);

    let frame = CanFrame::classic(m.id(), &bytes).unwrap();
    let d = db.decode(&frame).unwrap();
    assert_eq!(phys(&d, "EngRPM"), 3000.0);
    assert_eq!(phys(&d, "EngTemp"), -10.0);
    assert_eq!(phys(&d, "MotoA"), 2748.0);
    assert!((phys(&d, "Volt") - 12.34).abs() < 1e-9);
    let gear = d.signals.iter().find(|s| s.signal.name() == "Gear").unwrap();
    assert_eq!(
        gear.value,
        SignalValue::Value {
            raw: RawValue::Unsigned(3),
            physical: 3.0,
            label: Some("D")
        }
    );
    assert!(d.length_mismatch.is_none() && !d.fd_mismatch);
}

#[test]
fn negative_motorola_and_offset() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    // Volt -100: (-100 + 1.5) / 0.01 = -9850 → 16-bit 0xD986.
    let bytes = enc(m, &[("Volt", Physical(-100.0))]).unwrap();
    assert_eq!(&bytes[6..8], &[0xD9, 0x86]);
    let d = m.decode_payload(&bytes).unwrap();
    assert!((phys(&d, "Volt") + 100.0).abs() < 1e-9);
}

#[test]
fn start_values_fill_unset_signals() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    // EngRPM start raw 3200 = 0x0C80 → 80 0C; EngTemp -10 → F6; Gear 2 → 2<<1 = 04;
    // Volt 0 → (0 + 1.5) / 0.01 = 150 = 0x0096.
    let e = m
        .encode(
            &[("Volt", Physical(0.0))],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        )
        .unwrap();
    assert_eq!(e.frame.data(), &[0x80, 0x0C, 0xF6, 0x04, 0x00, 0x00, 0x00, 0x96]);
    let src = |i: u32| e.sources.iter().find(|(s, _)| *s == i).unwrap().1;
    assert_eq!(src(0), ValueSource::Initial);
    assert_eq!(src(6), ValueSource::Given);
    let rpm = m.signal_by_name("EngRPM").unwrap().1;
    assert_eq!(rpm.initial_physical(), Some(800.0));
}

#[test]
fn rounding_is_half_away_from_zero() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    let raw = |rpm: f64| {
        let b = enc(m, &[("EngRPM", Physical(rpm)), ("Volt", Physical(0.0))]).unwrap();
        u16::from_le_bytes([b[0], b[1]])
    };
    assert_eq!(raw(3000.1), 12000); // 12000.4
    assert_eq!(raw(3000.125), 12001); // 12000.5 → away from zero
    // Signed: -0.005 V → (-0.005 + 1.5) / 0.01 = 149.5 → 150
    let b = enc(m, &[("Volt", Physical(-0.005))]).unwrap();
    assert_eq!(u16::from_be_bytes([b[6], b[7]]), 150);
}

#[test]
fn sixty_four_bit_signals() {
    let db = load("basic.dbc");
    let w = db.message_by_name("Wide").unwrap();
    assert_eq!(enc(w, &[("U64", Raw(u64::MAX as i128))]).unwrap(), [0xFF; 8]);
    let d = w.decode_payload(&[0xFF; 8]).unwrap();
    assert_eq!(
        d.signals[0].value,
        SignalValue::Value {
            raw: RawValue::Unsigned(u64::MAX),
            physical: u64::MAX as f64,
            label: None
        }
    );
    let wm = db.message_by_name("WideM").unwrap();
    // Motorola 64-bit from byte0 bit7: plain big-endian.
    assert_eq!(
        enc(wm, &[("S64", Raw(-2))]).unwrap(),
        [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE]
    );
    assert_eq!(
        enc(wm, &[("S64", Raw(i64::MIN as i128))]).unwrap(),
        [0x80, 0, 0, 0, 0, 0, 0, 0]
    );
    let d = wm.decode_payload(&[0x80, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    assert!(matches!(
        d.signals[0].value,
        SignalValue::Value {
            raw: RawValue::Signed(i64::MIN),
            ..
        }
    ));
    assert!(matches!(
        enc(w, &[("U64", Raw(-1))]),
        Err(EncodeError::RawOverflow { bits: 64, .. })
    ));
}

#[test]
fn extended_id_message() {
    let db = load("basic.dbc");
    let id = CanId::extended(0x18FE_F100).unwrap();
    let m = db.message_by_id(id).unwrap();
    assert_eq!(m.name(), "J1939");
    // 100 km/h / (1/256) = 25600 = 0x6400 at bit 8 Intel → byte1 00, byte2 64.
    let e = m.encode(
        &[("Speed", Physical(100.0))],
        EncodeBase::Initial,
        &EncodeOptions::default(),
    );
    // No GenSigStartValue default in basic.dbc, but the only signal is given.
    let e = e.unwrap();
    assert_eq!(e.frame.id(), id);
    assert_eq!(e.frame.data(), &[0x00, 0x00, 0x64, 0, 0, 0, 0, 0]);
}

#[test]
fn floats() {
    let db = load("float.dbc");
    let m = db.message_by_name("F32Msg").unwrap();
    // Temp32 1.5 = 0x3FC00000 Intel → 00 00 C0 3F.
    // Scaled32 physical 7 → (7 - 1) / 2 = 3.0 = 0x40400000 Motorola bytes 4..7 → 40 40 00 00.
    let bytes = enc(m, &[("Temp32", Physical(1.5)), ("Scaled32", Physical(7.0))]).unwrap();
    assert_eq!(bytes, [0x00, 0x00, 0xC0, 0x3F, 0x40, 0x40, 0x00, 0x00]);
    let d = m.decode_payload(&bytes).unwrap();
    assert_eq!(phys(&d, "Temp32"), 1.5);
    assert_eq!(phys(&d, "Scaled32"), 7.0);
    assert!(matches!(
        enc(m, &[("Temp32", Physical(1e40)), ("Scaled32", Physical(0.0))]),
        Err(EncodeError::NotRepresentable { .. })
    ));

    let m = db.message_by_name("F64Msg").unwrap();
    // -2.5 = 0xC004000000000000 LE.
    let bytes = enc(m, &[("Val64", Physical(-2.5))]).unwrap();
    assert_eq!(bytes, [0, 0, 0, 0, 0, 0, 0x04, 0xC0]);
    assert_eq!(phys(&m.decode_payload(&bytes).unwrap(), "Val64"), -2.5);
}

#[test]
fn simple_mux() {
    let db = load("mux.dbc");
    let m = db.message_by_name("MuxMsg").unwrap();
    assert!(m.is_multiplexed());
    // B, C need Mode = 1 (set automatically).
    let e = m
        .encode(
            &[("B", Raw(1)), ("C", Raw(2))],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        )
        .unwrap();
    assert_eq!(e.frame.data(), &[0x01, 0x01, 0x02, 0, 0, 0, 0, 0]);
    assert!(e.sources.contains(&(0, ValueSource::AutoMultiplexor)));
    // A needs Mode = 0. 0x1234 Intel at bit 8.
    assert_eq!(
        enc(m, &[("A", Raw(0x1234))]).unwrap(),
        [0x00, 0x34, 0x12, 0, 0, 0, 0, 0]
    );

    let d = m.decode_payload(&[0x01, 0x07, 0x08, 0, 0, 0, 0, 0x55]).unwrap();
    assert_eq!(names(&d), ["Mode", "B", "C", "Common"]);
    assert_eq!(phys(&d, "Common"), 85.0);
    let d = m.decode_payload(&[0x00, 0x34, 0x12, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(names(&d), ["Mode", "A", "Common"]);
    assert_eq!(phys(&d, "A"), 4660.0);
}

#[test]
fn mux_errors() {
    let db = load("mux.dbc");
    let m = db.message_by_name("MuxMsg").unwrap();
    assert!(matches!(
        enc(m, &[("Mode", Raw(0)), ("B", Raw(1))]),
        Err(EncodeError::InactiveMuxSignal {
            multiplexor_value: Some(0),
            ..
        })
    ));
    assert!(matches!(
        enc(m, &[("A", Raw(1)), ("B", Raw(1))]),
        Err(EncodeError::ConflictingMultiplexor { .. }) | Err(EncodeError::InactiveMuxSignal { .. })
    ));
}

#[test]
fn extended_mux_nested_and_ranges() {
    let db = load("ext_mux.dbc");
    let m = db.message_by_name("ExtMux").unwrap();
    // S1 allows M2 in 0..2: the multiplexor cannot be guessed.
    assert!(matches!(
        enc(m, &[("S1", Raw(5))]),
        Err(EncodeError::AmbiguousMultiplexor { .. })
    ));
    // M2 = 2 given, M1 = 1 automatic (M2 is active only when M1 == 1): byte0 = 1 | 2<<4.
    assert_eq!(
        enc(m, &[("S1", Raw(5)), ("M2", Raw(2))]).unwrap(),
        [0x21, 0x05, 0, 0, 0, 0, 0, 0]
    );
    // S2 → M2 = 3 → M1 = 1, both automatic: byte0 = 0x31, S2 0xBEEF LE.
    assert_eq!(
        enc(m, &[("S2", Raw(0xBEEF))]).unwrap(),
        [0x31, 0xEF, 0xBE, 0, 0, 0, 0, 0]
    );
    // S3 → M1 = 0; M2 inactive.
    assert_eq!(enc(m, &[("S3", Raw(9))]).unwrap(), [0x00, 0x09, 0, 0, 0, 0, 0, 0]);

    let d = m.decode_payload(&[0x21, 0x05, 0, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(names(&d), ["M1", "M2", "S1"]);
    let d = m.decode_payload(&[0x00, 0x09, 0, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(names(&d), ["M1", "S3"]);
    // M1 = 2: no branch for M2 → only M1.
    let d = m.decode_payload(&[0x02, 0x09, 0, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(names(&d), ["M1"]);
    // M2 given but M1 = 0 makes it inactive.
    assert!(matches!(
        enc(m, &[("M1", Raw(0)), ("S1", Raw(1)), ("M2", Raw(0))]),
        Err(EncodeError::InactiveMuxSignal { .. })
    ));
}

#[test]
fn can_fd_messages() {
    let db = load("fd.dbc");
    let big = db.message_by_name("FdBig").unwrap();
    assert!(big.is_fd());
    assert_eq!(big.id(), CanId::extended(0x18DA_F110).unwrap());
    let e = big
        .encode(
            &[("First", Raw(1)), ("Mid", Raw(0x1234)), ("Last", Raw(0xFF))],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        )
        .unwrap();
    assert!(e.frame.is_fd() && !e.padded);
    let data = e.frame.data();
    assert_eq!(data.len(), 64);
    // Mid: Motorola MSB at bit 263 = byte 32 bit 7 → bytes 32, 33.
    assert_eq!((data[0], data[32], data[33], data[63]), (1, 0x12, 0x34, 0xFF));
    assert_eq!(data.iter().filter(|&&b| b != 0).count(), 4);

    let small = db.message_by_name("FdSmall").unwrap();
    let e = small
        .encode(&[("X", Raw(7))], EncodeBase::Initial, &EncodeOptions::default())
        .unwrap();
    assert!(e.padded);
    assert_eq!(e.frame.len(), 20);
    assert_eq!(e.frame.data()[17], 7);
    let d = small.decode(&e.frame).unwrap();
    assert!(d.length_mismatch.is_none());

    // VFrameFormat FD with 8 bytes is still an FD frame.
    let c = db.message_by_name("ClassicLenFd").unwrap();
    assert!(c.is_fd());
    // Default VFrameFormat "StandardCAN" → Classic.
    let p = db.message_by_name("Plain").unwrap();
    assert!(!p.is_fd());
    let classic = CanFrame::classic(c.id(), &[1; 8]).unwrap();
    assert!(c.decode(&classic).unwrap().fd_mismatch);
}

#[test]
fn base_payload() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    let base = [0xE0, 0x2E, 0xF6, 0x07, 0xAB, 0xC5, 0x05, 0x68];
    let e = m
        .encode(
            &[("EngTemp", Physical(20.0))],
            EncodeBase::Payload(&base),
            &EncodeOptions::default(),
        )
        .unwrap();
    assert_eq!(e.frame.data(), &[0xE0, 0x2E, 0x14, 0x07, 0xAB, 0xC5, 0x05, 0x68]);
    assert!(e.sources.contains(&(0, ValueSource::Base)));
    assert!(matches!(
        m.encode(&[], EncodeBase::Payload(&base[..7]), &EncodeOptions::default()),
        Err(EncodeError::BaseLengthMismatch {
            expected: 8,
            actual: 7
        })
    ));

    // Switching the mux branch clears the old branch and keeps common signals.
    let db = load("mux.dbc");
    let m = db.message_by_name("MuxMsg").unwrap();
    let base = [0x01, 0x07, 0x08, 0, 0, 0, 0, 0x55];
    let e = m
        .encode(
            &[("A", Raw(0x1234))],
            EncodeBase::Payload(&base),
            &EncodeOptions::default(),
        )
        .unwrap();
    assert_eq!(e.frame.data(), &[0x00, 0x34, 0x12, 0, 0, 0, 0, 0x55]);
}

#[test]
fn range_and_input_errors() {
    let db = load("basic.dbc");
    let m = db.message_by_name("EngineData").unwrap();
    let with_volt = |extra: (&'static str, SignalInput<'static>)| {
        m.encode(
            &[("Volt", Physical(0.0)), extra],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        )
    };
    assert!(matches!(
        with_volt(("EngRPM", Physical(16000.5))),
        Err(EncodeError::OutOfRange { .. })
    ));
    // Range edge with tolerance.
    assert!(with_volt(("EngRPM", Physical(16000.0))).is_ok());
    assert!(matches!(
        with_volt(("EngTemp", Raw(-41))),
        Err(EncodeError::OutOfRange { .. })
    ));
    let force = EncodeOptions {
        force_range: true,
        ..EncodeOptions::default()
    };
    assert!(
        m.encode(
            &[("Volt", Physical(0.0)), ("EngRPM", Physical(16383.75))],
            EncodeBase::Initial,
            &force
        )
        .is_ok()
    );
    // Forcing the range never lets a value overflow the bit width.
    assert!(matches!(
        m.encode(
            &[("Volt", Physical(0.0)), ("EngRPM", Physical(16384.0))],
            EncodeBase::Initial,
            &force
        ),
        Err(EncodeError::RawOverflow {
            raw: 65536,
            bits: 16,
            ..
        })
    ));
    assert!(matches!(
        with_volt(("Gear", Label("X"))),
        Err(EncodeError::UnknownLabel { .. })
    ));
    assert!(matches!(
        with_volt(("Nope", Raw(1))),
        Err(EncodeError::UnknownSignal { .. })
    ));
    assert!(matches!(
        with_volt(("Volt", Raw(1))),
        Err(EncodeError::DuplicateSignal { .. })
    ));
    assert!(matches!(
        with_volt(("EngRPM", Physical(f64::NAN))),
        Err(EncodeError::NotRepresentable { .. })
    ));
    // Volt has no start value.
    assert!(matches!(
        enc(m, &[]),
        Err(EncodeError::MissingSignal { name }) if name == "Volt"
    ));
}

#[test]
fn short_frame_decode_never_panics() {
    let db = load("mux.dbc");
    let m = db.message_by_name("MuxMsg").unwrap();
    let d = m.decode_payload(&[0x01, 0x07]).unwrap();
    assert_eq!(names(&d), ["Mode", "B", "C", "Common"]);
    assert_eq!(phys(&d, "B"), 7.0);
    let c = d.signals.iter().find(|s| s.signal.name() == "C").unwrap();
    assert_eq!(
        c.value,
        SignalValue::Unavailable(Unavailable::ShortFrame { needed: 3, actual: 2 })
    );
    let lm = d.length_mismatch.unwrap();
    assert_eq!((lm.expected, lm.actual), (8, 2));
    // Multiplexor itself missing: branch unknown, only the multiplexor is reported.
    let d = m.decode_payload(&[]).unwrap();
    assert_eq!(names(&d), ["Mode", "Common"]);
    for len in 0..=8 {
        let db = load("basic.dbc");
        let e = db.message_by_name("EngineData").unwrap();
        assert!(e.decode_payload(&vec![0xA5; len]).is_ok());
    }
    let remote = CanFrame::remote(m.id(), 8).unwrap();
    assert_eq!(
        m.decode(&remote).unwrap_err(),
        loom_can_dbc::DecodeError::RemoteFrame
    );
}
