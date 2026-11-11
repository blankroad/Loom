//! Builders → (simulated ECU bytes) → parsers.

use loom_uds_codec::request::{self, RoutineOp};
use loom_uds_codec::response::{
    parse_dtc_by_status_response, parse_read_did_response, parse_seed_response, parse_session_response,
};
use loom_uds_codec::{
    CodecError, DidCodec, DidDef, DidInput, DidLookup, DidValue, Endian, EnumEntry, ResponseKind,
    StandardDids, classify_response, nrc_name,
};

#[test]
fn session_timing() -> Result<(), CodecError> {
    let req = request::session_control(0x03, false)?;
    let resp = [0x50, 0x03, 0x00, 0x32, 0x01, 0xF4];
    assert_eq!(classify_response(&req, &resp), ResponseKind::Positive);
    let parsed = parse_session_response(&resp)?;
    let timing = parsed.timing.expect("6-byte response carries timing");
    assert_eq!(
        (parsed.session, timing.p2_ms, timing.p2_star_ms),
        (0x03, 50, 5000)
    );
    assert!(parse_session_response(&[0x50, 0x03])?.timing.is_none());
    assert!(parse_session_response(&[0x50, 0x03, 0x00]).is_err());
    Ok(())
}

struct Lens;
impl DidLookup for Lens {
    fn did_def(&self, did: u16) -> Option<DidDef> {
        match did {
            0xF1A0 => Some(DidDef {
                id: did,
                name: "sim_mode".into(),
                length: Some(1),
                codec: DidCodec::Hex,
                writable: true,
            }),
            other => StandardDids.did_def(other),
        }
    }
}

#[test]
fn read_did_multi() -> Result<(), CodecError> {
    let req = request::read_dids(&[0xF1A0, 0xF190])?;
    let mut resp = vec![0x62, 0xF1, 0xA0, 0x02, 0xF1, 0x90];
    resp.extend_from_slice(b"LOOMSIM0000000001");
    let recs = parse_read_did_response(&req, &resp, &Lens)?;
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].data, [0x02]);
    let vin = StandardDids
        .did_def(0xF190)
        .expect("VIN is standard")
        .decode(&recs[1].data)?;
    assert_eq!(
        vin,
        DidValue::Text {
            text: "LOOMSIM0000000001".into()
        }
    );
    Ok(())
}

#[test]
fn read_did_unknown_length_only_alone() -> Result<(), CodecError> {
    let single = request::read_dids(&[0x1234])?;
    assert_eq!(
        parse_read_did_response(&single, &[0x62, 0x12, 0x34, 9, 9, 9], &Lens)?[0].data,
        [9, 9, 9]
    );
    let multi = request::read_dids(&[0x1234, 0xF1A0])?;
    let err = parse_read_did_response(&multi, &[0x62, 0x12, 0x34, 9, 0xF1, 0xA0, 1], &Lens);
    assert_eq!(err, Err(CodecError::UnknownDidLength { did: 0x1234 }));
    let wrong = parse_read_did_response(&single, &[0x62, 0x12, 0x35, 9], &Lens);
    assert_eq!(
        wrong,
        Err(CodecError::DidEcho {
            expected: 0x1234,
            got: 0x1235
        })
    );
    Ok(())
}

#[test]
fn dtc_and_seed() -> Result<(), CodecError> {
    let req = request::read_dtc_by_status_mask(0x09);
    let resp = [0x59, 0x02, 0xFF, 0x92, 0x34, 0x00, 0x09, 0xC1, 0x00, 0x00, 0x2F];
    assert_eq!(classify_response(&req, &resp), ResponseKind::Positive);
    let rep = parse_dtc_by_status_response(&resp)?;
    assert_eq!(rep.dtcs.len(), 2);
    assert_eq!((rep.dtcs[0].dtc, rep.dtcs[0].status), (0x92_3400, 0x09));
    assert!(parse_dtc_by_status_response(&[0x59, 0x02, 0xFF, 1, 2]).is_err());

    let seed = parse_seed_response(&[0x67, 0x01, 0xDE, 0xAD, 0xBE, 0xEF])?;
    assert_eq!(
        (seed.seed_sub, seed.seed.as_slice()),
        (0x01, &[0xDE, 0xAD, 0xBE, 0xEF][..])
    );
    assert!(parse_seed_response(&[0x67, 0x01, 0, 0])?.already_unlocked());
    let key = request::security_send_key(seed.seed_sub, &[1, 2, 3, 4])?;
    assert_eq!(classify_response(&key, &[0x67, 0x02]), ResponseKind::Positive);
    let rc = request::routine_control(RoutineOp::RequestResults, 0x0203, &[], false)?;
    assert_eq!(
        classify_response(&rc, &[0x71, 0x03, 0x02, 0x03, 0x00]),
        ResponseKind::Positive
    );
    Ok(())
}

#[test]
fn did_codecs_roundtrip() -> Result<(), CodecError> {
    let speed = DidDef {
        id: 0xF40D,
        name: "VehicleSpeed".into(),
        length: Some(2),
        codec: DidCodec::Uint {
            endian: Endian::Big,
            scale: 0.5,
            offset: -10.0,
            unit: Some("km/h".into()),
        },
        writable: true,
    };
    let bytes = speed.encode(&DidInput::Number(40.0))?;
    assert_eq!(bytes, [0x00, 0x64]);
    match speed.decode(&bytes)? {
        DidValue::Number { raw, value, .. } => assert_eq!((raw, value), (100, 40.0)),
        other => panic!("unexpected {other:?}"),
    }
    assert!(speed.encode(&DidInput::Number(1e9)).is_err());
    assert!(speed.decode(&[1]).is_err(), "length mismatch is an error");

    let le = DidDef {
        codec: DidCodec::Uint {
            endian: Endian::Little,
            scale: 1.0,
            offset: 0.0,
            unit: None,
        },
        ..speed
    };
    assert_eq!(le.encode(&DidInput::Number(258.0))?, [0x02, 0x01]);

    let region = DidDef {
        id: 0x0101,
        name: "region".into(),
        length: Some(1),
        codec: DidCodec::Enum {
            map: vec![
                EnumEntry {
                    value: 2,
                    name: "EU".into(),
                },
                EnumEntry {
                    value: 5,
                    name: "KR".into(),
                },
            ],
        },
        writable: true,
    };
    assert_eq!(region.encode(&DidInput::EnumName("EU".into()))?, [2]);
    assert_eq!(
        region.decode(&[5])?,
        DidValue::Enum {
            raw: 5,
            name: Some("KR".into())
        }
    );
    assert!(region.encode(&DidInput::EnumName("US".into())).is_err());

    let date = DidDef {
        id: 0xF199,
        name: "date".into(),
        length: Some(3),
        codec: DidCodec::Bcd,
        writable: true,
    };
    assert_eq!(date.encode(&DidInput::Text("261111".into()))?, [0x26, 0x11, 0x11]);
    assert_eq!(
        date.decode(&[0x26, 0x11, 0x11])?,
        DidValue::Bcd {
            digits: "261111".into()
        }
    );
    assert!(date.decode(&[0x2A, 0, 0]).is_err());

    let vin = StandardDids.did_def(0xF190).expect("standard");
    assert!(vin.encode(&DidInput::Text("SHORT".into())).is_err());
    assert_eq!(
        vin.decode(b"WVWZZZ1KZAW00000\0")?,
        DidValue::Text {
            text: "WVWZZZ1KZAW00000".into()
        }
    );
    assert!(
        vin.decode(b"WVWZZZ1KZAW00000\x01").is_err(),
        "non-printable byte is an error"
    );
    Ok(())
}

#[test]
fn nrc_table_source() {
    assert_eq!(nrc_name(0x7F), Some("serviceNotSupportedInActiveSession"));
}
