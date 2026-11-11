//! Tables, names and parsers: error→exit table (cross-checked against an independent copy),
//! method/kind naming, cursors, evidence REFs, policy defaults (F1), schema export.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use loom_proto::cursor::{ClockTime, Cursor, TimeUnit};
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, Risk, kinds, validate_event_kind};
use loom_proto::evidence::{RefString, validate_json_pointer};
use loom_proto::policy::{PolicyAction, RiskDefaults};
use loom_proto::rpc::{
    RESERVED_DOMAINS, Request, Response, Rights, Role, is_reserved_domain, validate_method,
};
use loom_proto::schema::{DRAFT_2020_12, export_all};
use loom_proto::worker::{KNOWN_MSG_TYPES, Lane, WorkerMsg};
use serde_json::{Value, json};

/// Independent copy of contract §4.4 / E2 / E3 — must match `ErrorCode::exit_code`.
const EXIT_TABLE: &[(&str, u8)] = &[
    ("E_INTERNAL", 1),
    ("E_NOT_IMPLEMENTED", 1),
    ("E_USAGE", 2),
    ("E_AMBIGUOUS_CHANNEL", 2),
    ("E_CONFIG", 2),
    ("E_DB", 2),
    ("E_UNKNOWN_METHOD", 2),
    ("E_SCHEMA", 2),
    ("E_TIMEOUT", 3),
    ("E_NO_MATCH", 3),
    ("E_UNEXPECTED_MATCH", 3),
    ("E_COUNT_OUT_OF_RANGE", 3),
    ("E_DENIED", 4),
    ("E_UNSUPPORTED", 4),
    ("E_RATE_LIMIT", 4),
    ("E_LIMIT", 4),
    ("E_BUSY", 4),
    ("E_RECORDING_FAILED", 4),
    ("E_EVIDENCE_MISSING", 4),
    ("E_EVIDENCE_MISMATCH", 4),
    ("E_OUT_OF_WINDOW", 4),
    ("E_PLAN_MISMATCH", 4),
    ("E_CONFIRM_REQUIRED", 5),
    ("E_APPROVAL_REQUIRED", 5),
    ("E_NO_APPROVER", 5),
    ("E_APPROVAL_DENIED", 5),
    ("E_APPROVAL_EXPIRED", 5),
    ("E_USER_ACTIVE", 6),
    ("E_LOCKED", 6),
    ("E_LOCK_BROKEN", 6),
    ("E_HELD_BY_USER", 6),
    ("E_INTERRUPTED_BY_USER", 6),
    ("E_QUARANTINE", 6),
    ("E_STOPPED", 6),
    ("E_LEASE", 6),
    ("E_SESSION_ACTIVE", 6),
    ("E_SESSION_PAUSED", 6),
    ("E_NOT_OPEN", 7),
    ("E_DISCONNECTED", 7),
    ("E_CONNECTION_CHANGED", 7),
    ("E_BUS_OFF", 7),
    ("E_NO_RESPONSE", 7),
    ("E_PORT_BUSY", 7),
    ("E_PERMISSION", 7),
    ("E_NOT_FOUND", 7),
    ("E_WORKER_DOWN", 7),
    ("E_WORKER_BUSY", 7),
    ("E_CURSOR_GAP", 8),
    ("E_SCAN_LIMIT", 8),
    ("E_PARTIAL", 8),
    ("E_DAEMON", 9),
    ("E_PROTO_MISMATCH", 9),
    ("E_NRC", 10),
    ("E_TEST_FAILED", 11),
];

#[test]
fn error_codes_match_contract_table() {
    assert_eq!(ErrorCode::ALL.len(), EXIT_TABLE.len());
    for (wire, exit) in EXIT_TABLE {
        let code = ErrorCode::from_wire(wire).unwrap_or_else(|| panic!("missing {wire}"));
        assert_eq!(code.exit_code(), *exit, "{wire}");
        assert_eq!(code.as_str(), *wire);
        let j = serde_json::to_value(code).unwrap();
        assert_eq!(j, json!(wire));
        assert_eq!(serde_json::from_value::<ErrorCode>(j).unwrap(), code);
    }
    let names: BTreeSet<_> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), ErrorCode::ALL.len(), "duplicate wire names");
}

#[test]
fn envelope_exit_codes() {
    let body = |code| ErrorBody {
        code,
        message: "m".into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: Value::Null,
    };
    let ok = Envelope::success("x.y", json!(1), Meta::default(), OthersSinceLast::default());
    assert_eq!(ok.exit_code(), 0);
    for (code, exit) in [
        (ErrorCode::TestFailed, 11),
        (ErrorCode::NoMatch, 3),
        (ErrorCode::Stopped, 6),
    ] {
        let e: Envelope<Value> =
            Envelope::failure("x.y", body(code), Meta::default(), OthersSinceLast::default());
        assert_eq!(e.exit_code(), exit);
    }
}

#[test]
fn method_names() {
    for m in [
        "uart.send",
        "can.isotp.open",
        "core.approval.respond",
        "core.events.query",
        "test.step.begin",
        "a1.b_2",
    ] {
        validate_method(m).unwrap_or_else(|e| panic!("{m}: {e}"));
    }
    for m in [
        "uart",
        "",
        ".send",
        "uart.",
        "uart..send",
        "Uart.send",
        "uart.Send",
        "1uart.send",
        "uart.send-x",
        "uart.send.x.y",
        "_uart.send",
        "uart.sénd",
        "uart. send",
    ] {
        assert!(validate_method(m).is_err(), "accepted {m:?}");
    }
    assert_eq!(RESERVED_DOMAINS.len(), 8);
    assert!(is_reserved_domain("core") && is_reserved_domain("adapter") && !is_reserved_domain("uart"));

    let r = Request::new(loom_proto::rpc::Id::Number(1), "uart.send", json!({}));
    r.validate().unwrap();
    let mut bad = r.clone();
    bad.jsonrpc = "1.0".into();
    assert!(bad.validate().is_err());
    assert!(Response::ok(None, json!(1)).validate().is_ok());
    let mut both = Response::ok(None, json!(1));
    both.error = Some(loom_proto::rpc::RpcError {
        code: -32603,
        message: "x".into(),
        data: None,
    });
    assert!(both.validate().is_err());
}

#[test]
fn event_kind_names() {
    for k in kinds::ALL {
        validate_event_kind(k).unwrap();
    }
    let set: BTreeSet<_> = kinds::ALL.iter().collect();
    assert_eq!(set.len(), kinds::ALL.len());
    for k in ["can.isotp_rx_start", "uart.data"] {
        validate_event_kind(k).unwrap();
    }
    for k in [
        "core",
        "core.a.b",
        "Core.x",
        "core.",
        ".x",
        "core.x-y",
        "test.session.start",
    ] {
        assert!(validate_event_kind(k).is_err(), "accepted {k}");
    }
}

#[test]
fn cursor_roundtrip() {
    let cases = [
        ("@last", Cursor::Last),
        ("now", Cursor::Now),
        ("seq:0", Cursor::Seq { epoch: None, seq: 0 }),
        (
            "seq:88124",
            Cursor::Seq {
                epoch: None,
                seq: 88124,
            },
        ),
        (
            "e3:seq:5",
            Cursor::Seq {
                epoch: Some(3),
                seq: 5,
            },
        ),
        ("mark:boot", Cursor::Mark("boot".into())),
        (
            "-5s",
            Cursor::Relative {
                amount: 5,
                unit: TimeUnit::Secs,
            },
        ),
        (
            "-250ms",
            Cursor::Relative {
                amount: 250,
                unit: TimeUnit::Millis,
            },
        ),
        (
            "-2m",
            Cursor::Relative {
                amount: 2,
                unit: TimeUnit::Mins,
            },
        ),
        (
            "-1h",
            Cursor::Relative {
                amount: 1,
                unit: TimeUnit::Hours,
            },
        ),
        (
            "14:02:11",
            Cursor::Clock(ClockTime {
                hour: 14,
                minute: 2,
                second: 11,
                millis: None,
            }),
        ),
        (
            "23:59:59.250",
            Cursor::Clock(ClockTime {
                hour: 23,
                minute: 59,
                second: 59,
                millis: Some(250),
            }),
        ),
        (
            "2026-11-10T14:02:11Z",
            Cursor::Iso8601("2026-11-10T14:02:11Z".into()),
        ),
        (
            "2026-11-10T14:02:11.250+09:00",
            Cursor::Iso8601("2026-11-10T14:02:11.250+09:00".into()),
        ),
        (
            "2026-11-10T14:02:11",
            Cursor::Iso8601("2026-11-10T14:02:11".into()),
        ),
        (
            "off:1024",
            Cursor::Domain {
                prefix: "off".into(),
                value: "1024".into(),
            },
        ),
        (
            "send:last",
            Cursor::Domain {
                prefix: "send".into(),
                value: "last".into(),
            },
        ),
    ];
    for (s, c) in cases {
        let parsed: Cursor = s.parse().unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(parsed, c, "{s}");
        assert_eq!(parsed.to_string(), s);
        let j = serde_json::to_value(&parsed).unwrap();
        assert_eq!(j, json!(s));
        assert_eq!(serde_json::from_value::<Cursor>(j).unwrap(), c);
    }
    assert_eq!(
        "-2m".parse::<Cursor>().unwrap().relative_duration(),
        Some(std::time::Duration::from_secs(120))
    );
}

#[test]
fn cursor_rejects() {
    for s in [
        "",
        "@first",
        "last",
        "seq:-1",
        "seq:01",
        "seq:",
        "seq:x",
        "mark:",
        "mark:a b",
        "-5",
        "-5d",
        "-05s",
        "-s",
        "24:00:00",
        "14:60:00",
        "14:02:61",
        "14:02:11.25",
        "14:02",
        "2026-13-10T00:00:00Z",
        "2026-11-00T00:00:00Z",
        "2026-11-10T14:02:11+9",
        "2026-11-10T25:00:00Z",
        "e3:mark:x",
        "e:seq:1",
        "e03:seq:1",
        "last:foo",
        "Off:1",
        "off:",
        "@last2",
    ] {
        assert!(s.parse::<Cursor>().is_err(), "accepted {s:?}");
    }
    assert!(serde_json::from_value::<Cursor>(json!(5)).is_err());
}

#[test]
fn evidence_refs() {
    for (s, epoch, from, to, ptr) in [
        ("uart:dut@e3:seq:88120..88409", Some(3), 88120, 88409, None),
        ("uds:ecu1@seq:812..815", None, 812, 815, None),
        ("can:ch0@e1:seq:5#/payload/id", Some(1), 5, 5, Some("/payload/id")),
        ("uart:dut@seq:7", None, 7, 7, None),
        (
            "eth:eth0.lan-1@e2:seq:1..2#/payload/a~1b~0c",
            Some(2),
            1,
            2,
            Some("/payload/a~1b~0c"),
        ),
    ] {
        let r: RefString = s.parse().unwrap_or_else(|e| panic!("{s}: {e}"));
        assert_eq!(
            (r.epoch, r.from, r.to, r.pointer.as_deref()),
            (epoch, from, to, ptr),
            "{s}"
        );
        assert_eq!(r.to_string(), s);
    }
    for s in [
        "uart:dut",
        "uart@seq:1",
        "Uart:dut@seq:1",
        "uart:dut@seq:5..3",
        "uart:dut@e:seq:1",
        "uart:dut@e3:1",
        "uart:dut@seq:1#payload",
        "uart:dut@seq:1#/a~2",
        "uart:d u t@seq:1",
        "uart:dut@seq:01",
        "uart:@seq:1",
        "uart:dut@seq:1..",
        "uart:dut@e3:seq:1..2..3",
    ] {
        assert!(s.parse::<RefString>().is_err(), "accepted {s:?}");
    }
    validate_json_pointer("").unwrap();
    validate_json_pointer("/").unwrap();
    assert!(validate_json_pointer("a/b").is_err());
    assert!(validate_json_pointer("/a~").is_err());
}

#[test]
fn policy_defaults_are_auto_f1() {
    let d = RiskDefaults::default();
    for r in [Risk::R0, Risk::R1, Risk::R2, Risk::R3, Risk::R4] {
        assert_eq!(d.for_risk(r), PolicyAction::Auto, "{r:?}");
    }
    assert_eq!(serde_json::from_value::<RiskDefaults>(json!({})).unwrap(), d);
    assert_eq!(
        serde_json::from_value::<RiskDefaults>(json!({"r4": "approve"}))
            .unwrap()
            .r4,
        PolicyAction::Approve
    );
}

#[test]
fn roles_and_rights() {
    assert_eq!(Role::Agent.origin(), Origin::Agent);
    assert_eq!(Role::Viewer.origin(), Origin::User);
    assert_eq!(Role::Script.origin(), Origin::Script);
    assert!(Rights::Watch < Rights::Operate && Rights::Operate < Rights::Approve);
}

#[test]
fn worker_msg_types_unique_and_lanes() {
    let set: BTreeSet<_> = KNOWN_MSG_TYPES.iter().collect();
    assert_eq!(set.len(), KNOWN_MSG_TYPES.len());
    let credit = WorkerMsg::Credit(loom_proto::worker::Credit { bytes: 1.into() });
    assert_eq!(credit.lane(), Lane::Data);
    assert!(KNOWN_MSG_TYPES.contains(&credit.msg_type()));
}

#[test]
fn schemas_export_draft_2020_12() {
    let all = export_all();
    assert!(all.len() >= 10);
    for (name, s) in &all {
        assert_eq!(s.as_value().get("$schema"), Some(&json!(DRAFT_2020_12)), "{name}");
    }
    let rec = serde_json::to_string(all.get("published_record").unwrap().as_value()).unwrap();
    assert!(rec.contains("^(0|[1-9][0-9]{0,19})$"), "U64 pattern missing");
    let err = all.get("error_code").unwrap().as_value().to_string();
    assert!(err.contains("E_TEST_FAILED") && err.contains("E_NO_MATCH"));
}
