//! Invalid-input corpus (plan T15): every case must return `Err`, never panic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_proto::bytes::{Base64Bytes, Bytes, MAX_INLINE_BYTES};
use loom_proto::error::{ErrorCode, ProtoError};
use loom_proto::event::{
    GapCount, Origin, PayloadBytes, PublishedRecord, payload_bytes, payload_write_state,
};
use loom_proto::num::U64;
use loom_proto::worker::{FrameHeader, HEADER_LEN, MAX_PAYLOAD, split_frame};
use serde_json::json;

#[test]
fn bad_u64_json() {
    for s in [
        "\"-1\"",
        "\"1.5\"",
        "\" 1\"",
        "\"1 \"",
        "\"+1\"",
        "\"01\"",
        "\"00\"",
        "\"\"",
        "\"abc\"",
        "\"18446744073709551616\"",
        "\"99999999999999999999\"",
        "\"123456789012345678901\"",
        "\"１２\"",
        "5",
        "-5",
        "1.0",
        "null",
        "true",
    ] {
        assert!(serde_json::from_str::<U64>(s).is_err(), "accepted {s}");
    }
    assert_eq!(serde_json::from_str::<U64>("\"0\"").unwrap(), U64(0));
    assert_eq!(
        serde_json::from_str::<U64>("\"18446744073709551615\"").unwrap(),
        U64::MAX
    );
}

#[test]
fn bad_u64_msgpack() {
    // A string where an integer is expected, and a negative integer.
    let s = rmp_serde::to_vec(&"5").unwrap();
    assert!(rmp_serde::from_slice::<U64>(&s).is_err());
    let n = rmp_serde::to_vec(&-1i64).unwrap();
    assert!(rmp_serde::from_slice::<U64>(&n).is_err());
}

#[test]
fn bad_hex_bytes() {
    let too_big = format!("\"{}\"", "ab".repeat(MAX_INLINE_BYTES + 1));
    for s in [
        "\"abc\"",
        "\"zz\"",
        "\"0g\"",
        "\"éé\"",
        "\" ab\"",
        "\"ab \"",
        "12",
        "null",
        too_big.as_str(),
    ] {
        assert!(
            serde_json::from_str::<Bytes>(s).is_err(),
            "accepted {}",
            &s[..s.len().min(20)]
        );
    }
}

#[test]
fn bad_base64_bytes() {
    let too_big = format!("\"{}\"", "AAAA".repeat(MAX_INLINE_BYTES / 3 + 2));
    for s in [
        "\"!!!!\"",
        "\"abc\"",
        "\"aGk\"",
        "\"aG=k\"",
        "\"ü===\"",
        too_big.as_str(),
    ] {
        assert!(
            serde_json::from_str::<Base64Bytes>(s).is_err(),
            "accepted {}",
            &s[..s.len().min(20)]
        );
    }
}

#[test]
fn oversized_msgpack_bin() {
    let big = serde_bytes::ByteBuf::from(vec![0u8; MAX_INLINE_BYTES + 1]);
    let m = rmp_serde::to_vec(&big).unwrap();
    assert!(rmp_serde::from_slice::<Bytes>(&m).is_err());
    assert!(rmp_serde::from_slice::<Base64Bytes>(&m).is_err());
    // A string is not a bin in binary form.
    let s = rmp_serde::to_vec(&"0102").unwrap();
    assert!(rmp_serde::from_slice::<Bytes>(&s).is_err());
}

#[test]
fn truncated_msgpack_never_panics() {
    let rec: PublishedRecord = serde_json::from_str(include_str!("fixtures/event_can_frame.json")).unwrap();
    let full = rmp_serde::to_vec_named(&rec).unwrap();
    for cut in 0..full.len() {
        assert!(
            rmp_serde::from_slice::<PublishedRecord>(&full[..cut]).is_err(),
            "prefix {cut} accepted"
        );
    }
    assert_eq!(rmp_serde::from_slice::<PublishedRecord>(&full).unwrap(), rec);
}

#[test]
fn unknown_enum_values() {
    assert!(serde_json::from_str::<Origin>("\"robot\"").is_err());
    assert!(serde_json::from_str::<ErrorCode>("\"E_NOPE\"").is_err());
    assert!(serde_json::from_str::<ErrorCode>("\"STOPPED\"").is_err());
    let mut v: serde_json::Value = serde_json::from_str(include_str!("fixtures/event_uart_rx.json")).unwrap();
    v["origin"] = json!("robot");
    assert!(serde_json::from_value::<PublishedRecord>(v).is_err());
}

#[test]
fn gap_count_rejects() {
    for s in ["\"-1\"", "\"01\"", "\"Unknown\"", "\"\"", "7"] {
        assert!(serde_json::from_str::<GapCount>(s).is_err(), "accepted {s}");
    }
}

#[test]
fn payload_helpers_reject() {
    assert!(payload_write_state(&json!({"write_state": "teleported"})).is_err());
    assert!(payload_write_state(&json!({"write_state": 3})).is_err());
    assert!(payload_bytes(&json!({"bytes": {"encoding": "hex", "data": "abc"}})).is_err());
    assert!(payload_bytes(&json!({"bytes": {"encoding": "hex"}})).is_err());
    assert!(payload_bytes(&json!({"bytes": {"encoding": "zip", "data": "00"}})).is_err());
    let raw = json!({"bytes": {"encoding": "raw_ref", "ref": {"id": "a1", "sha256": "00", "length": "9", "format": "raw"}}});
    assert_eq!(payload_bytes(&raw), Err(ProtoError::NotInline));
    assert_eq!(payload_bytes(&json!({})), Ok(None));
    assert_eq!(PayloadBytes::hex(b"\x01").decode().unwrap(), vec![1]);
}

#[test]
fn frame_header_rejects() {
    let good = FrameHeader::new(5, 17).unwrap().encode();
    for cut in 0..HEADER_LEN {
        assert!(
            matches!(
                FrameHeader::decode(&good[..cut]),
                Err(ProtoError::Incomplete { .. })
            ),
            "cut {cut}"
        );
    }
    assert_eq!(
        FrameHeader::decode(&good).unwrap(),
        FrameHeader {
            len: 5,
            msg_type: 17,
            flags: 0
        }
    );

    let mut over = good;
    over[..4].copy_from_slice(&((MAX_PAYLOAD as u32) + 1).to_le_bytes());
    assert!(matches!(
        FrameHeader::decode(&over),
        Err(ProtoError::InvalidFrame(_))
    ));
    let mut huge = good;
    huge[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(FrameHeader::decode(&huge).is_err());
    let mut ty = good;
    ty[4] = 99;
    assert!(FrameHeader::decode(&ty).is_err());
    let mut fl = good;
    fl[5] = 1;
    assert!(FrameHeader::decode(&fl).is_err());
    let mut rs = good;
    rs[7] = 1;
    assert!(FrameHeader::decode(&rs).is_err());

    assert!(FrameHeader::new(MAX_PAYLOAD + 1, 17).is_err());
    assert!(FrameHeader::new(1, 0).is_err());

    // Partial payload → Incomplete; full frame splits off the rest.
    let mut buf = good.to_vec();
    buf.extend_from_slice(b"abc");
    assert_eq!(split_frame(&buf), Err(ProtoError::Incomplete { need: 2 }));
    buf.extend_from_slice(b"deXYZ");
    let (h, payload, rest) = split_frame(&buf).unwrap();
    assert_eq!((h.len, payload, rest), (5, &b"abcde"[..], &b"XYZ"[..]));
}
