//! Loading: lookups, attributes, recovery from unparsable statements, limits, encodings.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_dbc::{
    Database, DecodeError, E2eReason, EncodeBase, EncodeError, EncodeOptions, InitialSource, LimitKind,
    Limits, LoadError, LoadOptions, SignalInput, TextEncoding, WarningKind, is_e2e_suspect,
};
use loom_can_frame::{CanFrame, CanId};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn load_with(bytes: &[u8], limits: Limits) -> Result<Database, LoadError> {
    Database::load(
        bytes,
        &LoadOptions {
            encoding: TextEncoding::Utf8,
            limits,
        },
    )
    .map(|(db, _)| db)
}

#[test]
fn lookups_and_attributes() {
    let bytes = fixture("basic.dbc");
    let (db, warnings) = Database::load(&bytes, &LoadOptions::default()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(db.messages().len(), 4);
    assert_eq!(db.nodes(), ["ECU", "GW"]);
    assert_eq!(db.sha256_hex().len(), 64);
    let (db2, _) = Database::load(&bytes, &LoadOptions::default()).unwrap();
    assert_eq!(db.sha256(), db2.sha256());

    let m = db.message_by_id(CanId::standard(0x100).unwrap()).unwrap();
    assert_eq!(m.name(), "EngineData");
    assert_eq!(m.size(), 8);
    assert!(!m.is_fd());
    assert_eq!(m.transmitter(), Some("ECU"));
    assert_eq!(m.cycle_time_ms(), Some(10));
    assert_eq!(m.comment(), Some("Engine status"));
    assert_eq!(m.line(), Some(14));
    assert_eq!(db.message_by_name("Wide").unwrap().cycle_time_ms(), None);

    let (_, rpm) = db.signal_by_path("EngineData.EngRPM").unwrap();
    assert_eq!(rpm.unit(), "rpm");
    assert_eq!(rpm.comment(), Some("Engine speed"));
    assert_eq!(rpm.range(), Some((0.0, 16000.0)));
    assert_eq!(rpm.receivers(), ["GW"]);
    assert_eq!(rpm.initial_source(), Some(InitialSource::Signal));
    assert_eq!(db.signals_named("Volt").len(), 1);
    assert!(db.signal_by_path("Volt").is_some());
    let (_, volt) = db.signal_by_path("Volt").unwrap();
    assert_eq!(volt.initial_raw_bits(), None);
    let (_, u64sig) = db.signal_by_path("U64").unwrap();
    assert_eq!(u64sig.range(), None); // [0|0] = no range
    let (_, gear) = db.signal_by_path("Gear").unwrap();
    assert_eq!(gear.value_table().len(), 4);
    assert_eq!(gear.raw_for_label("N"), Some(2));
    assert!(db.approx_heap_bytes() > 0);
}

#[test]
fn unknown_and_unparsable_statements_are_reported_by_line() {
    let (db, warnings) = Database::load(&fixture("broken.dbc"), &LoadOptions::default()).unwrap();
    let dropped: Vec<u32> = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::StatementDropped)
        .filter_map(|w| w.line)
        .collect();
    // Broken BO_ (line 10 is the bad SG_), SGTYPE_, SIG_TYPE_REF_, bad SG_MUL_VAL_, bad BA_.
    assert_eq!(dropped, [10, 19, 20, 21, 24], "{warnings:#?}");

    // Untouched message works both ways.
    let fine = db.message_by_name("Fine").unwrap();
    assert!(fine.encode_blocked().is_none() && fine.decode_blocked().is_none());

    // Unparsable start value: decode fine, encode fail closed.
    let good = db.message_by_name("Good").unwrap();
    assert!(good.decode_blocked().is_none());
    assert!(good.encode_blocked().unwrap().contains("line 24"));
    let err = good
        .encode(
            &[("G1", SignalInput::Raw(1))],
            EncodeBase::Initial,
            &EncodeOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(err, EncodeError::Blocked { .. }));

    // Unparsable mux ranges: both directions blocked.
    let mux = db.message_by_name("MuxBad").unwrap();
    assert!(mux.decode_blocked().is_some() && mux.encode_blocked().is_some());
    let f = CanFrame::classic(mux.id(), &[1; 8]).unwrap();
    assert!(matches!(db.decode(&f), Err(DecodeError::Blocked { .. })));

    // Unparsable BO_ block is kept as a blocked placeholder, so the id is explained.
    let broken = db.message_by_id(CanId::standard(101).unwrap()).unwrap();
    assert_eq!(broken.name(), "Broken");
    assert!(broken.signals().is_empty());
    assert!(broken.decode_blocked().unwrap().contains("line 9"));
}

#[test]
fn invalid_dbc_is_an_error() {
    let err = load_with(b"this is not a dbc\nat all\n", Limits::default()).unwrap_err();
    assert!(
        matches!(err, LoadError::InvalidDbc { line: Some(1), .. }),
        "{err:?}"
    );
}

#[test]
fn limits() {
    let bytes = fixture("basic.dbc");
    let tight = |f: fn(&mut Limits)| {
        let mut l = Limits::default();
        f(&mut l);
        load_with(&bytes, l).unwrap_err()
    };
    let what = |e: LoadError| match e {
        LoadError::LimitExceeded { what, .. } => what,
        other => panic!("{other:?}"),
    };
    assert_eq!(what(tight(|l| l.max_file_bytes = 100)), LimitKind::FileBytes);
    assert_eq!(what(tight(|l| l.max_messages = 3)), LimitKind::Messages);
    assert_eq!(what(tight(|l| l.max_signals = 9)), LimitKind::Signals);
    assert_eq!(what(tight(|l| l.max_string_bytes = 10)), LimitKind::StringBytes);
    assert_eq!(what(tight(|l| l.max_ir_bytes = 1000)), LimitKind::IrBytes);
    assert_eq!(what(tight(|l| l.max_statements = 5)), LimitKind::Statements);

    // Default 16 MiB file limit.
    let mut big = vec![b' '; 16 * 1024 * 1024 + 1];
    big[..12].copy_from_slice(b"VERSION \"\"\n ");
    assert!(matches!(
        load_with(&big, Limits::default()),
        Err(LoadError::LimitExceeded {
            what: LimitKind::FileBytes,
            limit: 16_777_216,
            ..
        })
    ));

    // Nested mux: ExtMux S1 is 2 hops deep (S1 → M2 → M1).
    let l = Limits {
        max_mux_depth: 1,
        ..Limits::default()
    };
    assert_eq!(
        what(load_with(&fixture("ext_mux.dbc"), l).unwrap_err()),
        LimitKind::MuxDepth
    );

    let l = Limits {
        max_dropped_statements: 2,
        ..Limits::default()
    };
    assert_eq!(
        what(load_with(&fixture("broken.dbc"), l).unwrap_err()),
        LimitKind::DroppedStatements
    );
}

#[test]
fn warnings_are_bounded() {
    let mut text = String::from("VERSION \"\"\nBU_: E\n");
    for i in 0..50 {
        text.push_str(&format!("SGTYPE_ T{i} : 8@1+ (1,0) [0|255] \"\" ;\n"));
    }
    text.push_str("BO_ 1 M: 8 E\n SG_ S : 0|8@1+ (1,0) [0|0] \"\" E\n");
    let l = Limits {
        max_warnings: 10,
        ..Limits::default()
    };
    let (db, w) = Database::load(
        text.as_bytes(),
        &LoadOptions {
            limits: l,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(db.messages().len(), 1);
    assert_eq!(w.len(), 11);
    assert_eq!(w[10].kind, WarningKind::WarningsTruncated);
    assert!(w[10].detail.starts_with("40 "));
}

#[test]
fn encodings() {
    let bytes = b"VERSION \"\"\nBU_: E\nBO_ 1 M: 1 E\n SG_ T : 0|8@1+ (1,0) [0|0] \"\xB0C\" E\n";
    assert!(matches!(
        Database::load(bytes, &LoadOptions::default()),
        Err(LoadError::Encoding { .. })
    ));
    let opts = LoadOptions {
        encoding: TextEncoding::Windows1252,
        ..Default::default()
    };
    let (db, w) = Database::load(bytes, &opts).unwrap();
    assert!(w.is_empty() && !db.encoding_guessed());
    assert_eq!(db.signal_by_path("T").unwrap().1.unit(), "°C");
    let opts = LoadOptions {
        encoding: TextEncoding::Auto,
        ..Default::default()
    };
    let (db, w) = Database::load(bytes, &opts).unwrap();
    assert!(db.encoding_guessed());
    assert_eq!(db.encoding(), TextEncoding::Windows1252);
    assert_eq!(w[0].kind, WarningKind::EncodingGuessed);
}

#[test]
fn duplicates_and_pseudo_messages() {
    let text = "VERSION \"\"\nBU_: E\n\
BO_ 1 A: 8 E\n SG_ S : 0|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 1 B: 8 E\n SG_ S : 0|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 2 A: 8 E\n SG_ T : 0|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 3 Dup: 8 E\n SG_ X : 0|8@1+ (1,0) [0|0] \"\" E\n SG_ X : 8|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 4 Overlap: 8 E\n SG_ P : 0|8@1+ (1,0) [0|0] \"\" E\n SG_ Q : 4|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 5000 TooBig: 8 E\n SG_ Y : 0|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 3221225472 VECTOR__INDEPENDENT_SIG_MSG: 0 Vector__XXX\n SG_ Lost : 0|8@1+ (1,0) [0|0] \"\" Vector__XXX\n";
    let (db, w) = Database::load(text.as_bytes(), &LoadOptions::default()).unwrap();
    let kinds: Vec<_> = w.iter().map(|w| w.kind).collect();
    assert!(kinds.contains(&WarningKind::DuplicateMessageId));
    assert!(kinds.contains(&WarningKind::DuplicateMessageName));
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == WarningKind::MessageSkipped)
            .count(),
        2
    );

    let id1 = CanId::standard(1).unwrap();
    assert!(db.is_ambiguous_id(id1) && db.message_by_id(id1).is_none());
    let f = CanFrame::classic(id1, &[0; 8]).unwrap();
    assert_eq!(db.decode(&f).unwrap_err(), DecodeError::AmbiguousId);
    assert!(db.message_by_name("A").is_none());
    assert!(db.message_by_id(CanId::standard(2).unwrap()).is_some());
    // Duplicate signal name: layout unclear → both directions blocked.
    let dup = db.message_by_name("Dup").unwrap();
    assert!(dup.decode_blocked().is_some());
    // Overlapping always-present signals: decode fine, encode blocked.
    let ov = db.message_by_name("Overlap").unwrap();
    assert!(ov.decode_blocked().is_none());
    assert!(ov.encode_blocked().unwrap().contains("overlap"));
    // Standard id above 0x7FF and the Vector pseudo message are left out.
    assert!(db.message_by_name("TooBig").is_none());
    assert!(db.message_by_name("VECTOR__INDEPENDENT_SIG_MSG").is_none());
    let unknown = CanFrame::classic(CanId::standard(9).unwrap(), &[]).unwrap();
    assert_eq!(db.decode(&unknown).unwrap_err(), DecodeError::UnknownId);
}

#[test]
fn e2e_suspicion() {
    let text = "VERSION \"\"\nBU_: E\n\
BO_ 1 Brake: 8 E\n SG_ Brake_CRC : 0|8@1+ (1,0) [0|0] \"\" E\n SG_ Brake_AliveCtr : 8|4@1+ (1,0) [0|0] \"\" E\n\
BO_ 2 Plain: 8 E\n SG_ Speed : 0|8@1+ (1,0) [0|0] \"\" E\n\
BO_ 3 Attr: 8 E\n SG_ V : 0|8@1+ (1,0) [0|0] \"\" E\n\
BA_DEF_ BO_ \"E2E_DataID\" INT 0 65535;\n\
BA_ \"E2E_DataID\" BO_ 3 77;\n";
    let (db, _) = Database::load(text.as_bytes(), &LoadOptions::default()).unwrap();
    let r = is_e2e_suspect(db.message_by_name("Brake").unwrap()).unwrap();
    assert!(r.contains(&E2eReason::SignalName {
        signal: "Brake_CRC".into(),
        pattern: "crc"
    }));
    assert!(r.contains(&E2eReason::SignalName {
        signal: "Brake_AliveCtr".into(),
        pattern: "alive"
    }));
    assert!(is_e2e_suspect(db.message_by_name("Plain").unwrap()).is_none());
    assert_eq!(
        is_e2e_suspect(db.message_by_name("Attr").unwrap()).unwrap(),
        [E2eReason::Attribute {
            name: "E2E_DataID".into()
        }]
    );
}

/// Size check near the plan limits: 50k messages, 400k short signals (~16 MiB of text;
/// 500k realistic signal lines do not fit the 16 MiB file limit). Run with
/// `cargo test --release -- --ignored large_file` (takes a few seconds).
#[test]
#[ignore]
fn large_file_at_limits() {
    let mut text = String::from("VERSION \"\"\nBU_: E\n");
    for m in 0..50_000u32 {
        text.push_str(&format!("BO_ {} M{m}: 64 E\n", 0x8000_0000u32 + m));
        for s in 0..8 {
            text.push_str(&format!(" SG_ s{s} : {}|16@1+ (1,0) [0|0] \"\" E\n", s * 16));
        }
    }
    // Finding (reported to lead): 400k signals compile to ~140 MB, so the default 32 MiB
    // IR target rejects this file. Load with a raised IR limit to measure time and size.
    assert!(matches!(
        Database::load(text.as_bytes(), &LoadOptions::default()),
        Err(LoadError::LimitExceeded {
            what: LimitKind::IrBytes,
            ..
        })
    ));
    let opts = LoadOptions {
        limits: Limits {
            max_ir_bytes: 512 * 1024 * 1024,
            ..Limits::default()
        },
        ..LoadOptions::default()
    };
    let t = std::time::Instant::now();
    let (db, w) = Database::load(text.as_bytes(), &opts).unwrap();
    eprintln!(
        "file {} bytes, load {:?}, IR estimate {} bytes, warnings {}",
        text.len(),
        t.elapsed(),
        db.approx_heap_bytes(),
        w.len()
    );
    assert_eq!(db.messages().len(), 50_000);
    assert!(text.len() <= Limits::default().max_file_bytes);
}
