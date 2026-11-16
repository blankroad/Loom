//! `loom logic archive …` / `loom logic export …` argument conversion (B2b) — Opus spec tests.
//! Pure: clap parse → RpcCall; the shared `archive::{ReadReq, ExportReq}` validators decide.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use loom_cli_kit::{DomainCli, RpcCall};
use loom_logic::archive::{ExportFormat, ExportReq, ReadKind, ReadReq};
use loom_logic::cli::LogicCli;
use serde_json::json;

fn call(args: &[&str]) -> Result<RpcCall, String> {
    let m = LogicCli::command()
        .try_get_matches_from(std::iter::once("logic").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    LogicCli::to_call(&m).map_err(|e| e.to_string())
}

#[test]
fn archive_list_and_read_map_to_the_shared_request() {
    let c = call(&["archive", "list"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        ("logic.archive.list", json!({}))
    );
    let c = call(&[
        "archive", "read", "lc-1a-1", "--kind", "edges", "--from", "5", "--to", "9", "--mask", "0x03",
        "--limit", "10",
    ])
    .unwrap();
    assert_eq!(c.method, "logic.archive.read");
    assert_eq!(
        ReadReq::from_params(&c.params).unwrap(),
        ReadReq {
            capture_id: "lc-1a-1".into(),
            kind: ReadKind::Edges,
            from_sample: Some(5),
            to_sample: Some(9),
            limit: Some(10),
            mask: Some(3),
            after_row: None
        }
    );
    let c = call(&[
        "archive",
        "read",
        "lc-1a-1",
        "--kind",
        "annotations",
        "--after-row",
        "41",
        "--mask",
        "255",
    ]);
    assert!(c.is_err(), "mask is edges-only");
    let c = call(&[
        "archive",
        "read",
        "lc-1a-1",
        "--kind",
        "annotations",
        "--after-row",
        "41",
    ])
    .unwrap();
    assert_eq!(ReadReq::from_params(&c.params).unwrap().after_row, Some(41));
    let c = call(&["archive", "read", "lc-1a-1", "--kind", "edges", "--mask", "17"]).unwrap();
    assert_eq!(c.params["mask"], 17);
    for bad in [
        vec!["archive", "read", "lc-1a-1", "--kind", "edges", "--mask", "256"],
        vec!["archive", "read", "lc-1a-1", "--kind", "edges", "--mask", "0x100"],
        vec!["archive", "read", "lc-1a-1", "--kind", "edges", "--mask", "zz"],
        vec![
            "archive", "read", "lc-1a-1", "--kind", "edges", "--from", "9", "--to", "9",
        ],
        vec!["archive", "read", "lc-1a-1", "--kind", "edges", "--limit", "2049"],
        vec!["archive", "read", "../etc", "--kind", "edges"],
        vec!["archive", "read", "lc-1a-1"],
        vec!["archive"],
    ] {
        assert!(call(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn export_defaults_to_csv_and_waits_for_the_file() {
    let c = call(&["export", "lc-1a-1", "--kind", "annotations"]).unwrap();
    assert_eq!(c.method, "logic.export");
    assert_eq!(
        ExportReq::from_params(&c.params).unwrap(),
        ExportReq {
            capture_id: "lc-1a-1".into(),
            kind: ReadKind::Annotations,
            format: ExportFormat::Csv,
            from_sample: None,
            to_sample: None
        }
    );
    assert_eq!(c.wait, Some(Duration::from_secs(62)));
    let c = call(&[
        "export",
        "lc-1a-1",
        "--kind",
        "annotations",
        "--format",
        "txt",
        "--from",
        "100",
        "--to",
        "200",
    ])
    .unwrap();
    assert_eq!(
        ExportReq::from_params(&c.params).unwrap().format,
        ExportFormat::Txt
    );
    assert_eq!(
        (c.params["from_sample"].as_u64(), c.params["to_sample"].as_u64()),
        (Some(100), Some(200))
    );
    assert!(call(&["export", "lc-1a-1", "--kind", "edges", "--format", "txt"]).is_err());
    assert!(call(&["export", "lc-1a-1"]).is_err());
}

#[test]
fn glitch_and_duplex_reach_the_start_wire() {
    let c = call(&[
        "start",
        "--source",
        "synthetic",
        "--decode",
        "uart:rx=D0:tx=D1:baudrate=100000",
        "--glitch-every",
        "5",
    ])
    .unwrap();
    assert_eq!(c.params["glitch_every"], 5);
    assert_eq!(
        c.params["decoders"],
        json!(["uart:rx=D0:tx=D1:baudrate=100000:data_bits=8:parity=none:stop_bits=1:bit_order=lsb-first"])
    );
    assert!(
        call(&[
            "start",
            "--source",
            "synthetic",
            "--decode",
            "spi:clk=D0:mosi=D1",
            "--glitch-every",
            "5"
        ])
        .is_err()
    );
    assert!(
        call(&[
            "start",
            "--source",
            "synthetic",
            "--decode",
            "uart:rx=D0",
            "--glitch-every",
            "0"
        ])
        .is_err()
    );
}

#[test]
fn trigger_flags_build_the_trigger_object() {
    let c = call(&[
        "start",
        "--source",
        "synthetic",
        "--decode",
        "uart:rx=D0",
        "--duration",
        "5s",
        "--trigger",
        "D3:rising",
        "--pre",
        "10ms",
        "--post",
        "500ms",
    ])
    .unwrap();
    assert_eq!(
        c.params["trigger"],
        json!({"channel": "D3", "edge": "rising", "pre_ms": 10, "post_ms": 500})
    );
    let c = call(&[
        "start",
        "--source",
        "sigrok-demo",
        "--trigger",
        "D0:either",
        "--post",
        "1s",
    ])
    .unwrap();
    assert_eq!(c.params["trigger"]["pre_ms"], 0);
    for bad in [
        vec!["start", "--source", "sigrok-demo", "--trigger", "D3:rising"],
        vec![
            "start",
            "--source",
            "sigrok-demo",
            "--trigger",
            "D3",
            "--post",
            "1s",
        ],
        vec![
            "start",
            "--source",
            "sigrok-demo",
            "--trigger",
            "D9:rising",
            "--post",
            "1s",
        ],
        vec![
            "start",
            "--source",
            "sigrok-demo",
            "--trigger",
            "D3:high",
            "--post",
            "1s",
        ],
        vec!["start", "--source", "sigrok-demo", "--post", "1s"],
    ] {
        assert!(call(&bad).is_err(), "{bad:?}");
    }
}
