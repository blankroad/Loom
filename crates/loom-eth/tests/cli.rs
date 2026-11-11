//! CLI piece: argument → RPC mapping and human rendering (tshark-style columns).

use std::time::Duration;

use loom_cli_kit::{CliOf, DomainCli, DynDomainCli, RpcCall, run_cli_with};
use loom_eth::cli::{EthCli, clock, parse_count, parse_duration};
use loom_proto::envelope::{Envelope, Meta, OthersSinceLast};
use serde_json::{Value, json};

fn to_call(args: &[&str]) -> Result<RpcCall, String> {
    let m = EthCli::command()
        .try_get_matches_from(std::iter::once("eth").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    EthCli::to_call(&m).map_err(|e| e.message)
}

fn ok(args: &[&str]) -> RpcCall {
    to_call(args).unwrap_or_else(|e| panic!("{args:?}: {e}"))
}

fn render(call: &RpcCall, data: Value) -> String {
    let env = Envelope::success(
        call.method.clone(),
        data,
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    EthCli::render_human(call, &env, &mut out).expect("render");
    String::from_utf8(out).expect("utf8")
}

#[test]
fn durations_and_counts() {
    assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
    assert_eq!(parse_duration("5s"), Ok(Duration::from_secs(5)));
    assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
    assert_eq!(parse_duration("7"), Ok(Duration::from_secs(7)));
    assert!(parse_duration("5d").is_err());
    assert_eq!(parse_count("3"), Ok((3, None)));
    assert_eq!(parse_count("2..4"), Ok((2, Some(4))));
    assert_eq!(parse_count("0..0"), Ok((0, Some(0))));
    assert!(parse_count("4..2").is_err());
    assert_eq!(clock(3_723_000_456_000), "01:02:03.000456");
}

#[test]
fn commands_map_to_rpcs() {
    let c = ok(&[
        "capture",
        "start",
        "en5",
        "--ch",
        "dut",
        "--bpf",
        "tcp port 13400",
        "--promisc",
    ]);
    assert_eq!(c.method, "eth.capture.start");
    assert_eq!(c.params["backend"], "live");
    assert_eq!(c.params["bpf"], "tcp port 13400");
    let c = ok(&[
        "capture", "start", "sim0", "--replay", "a.pcapng", "--speed", "2", "--repeat",
    ]);
    assert_eq!(c.params["backend"], "replay");
    assert_eq!(
        c.params["replay"],
        json!({"path": "a.pcapng", "speed": 2.0, "repeat": true})
    );

    let c = ok(&[
        "trace",
        "dut",
        "-Y",
        "uds.sid == 0x22",
        "--since",
        "-10s",
        "--max",
        "50",
    ]);
    assert_eq!(c.method, "eth.query");
    assert_eq!(c.params["ch"], "dut");
    assert_eq!(c.params["filter"], "uds.sid == 0x22");
    assert_eq!(c.params["range"]["from"], "-10s");
    assert_eq!(c.params["max"], 50);
    assert!(c.wait.is_none());
    let c = ok(&["trace", "--follow", "--timeout", "20s", "--since", "seq:10"]);
    assert_eq!(c.params["wait_ms"], 20_000);
    assert_eq!(c.wait, Some(Duration::from_secs(20)));
    let c = ok(&["trace", "--follow", "--timeout", "2h"]);
    assert_eq!(
        c.wait,
        Some(Duration::from_secs(600)),
        "follow is capped at 600 s"
    );

    let c = ok(&["last", "-n", "5"]);
    assert_eq!(
        (c.method.as_str(), c.params["max"].clone()),
        ("eth.query", json!(5))
    );

    let c = ok(&["show", "eth:dut@seq:12", "--hex"]);
    assert_eq!(c.params, json!({"ref": "eth:dut@seq:12", "hex": true}));

    let c = ok(&[
        "wait-for",
        "-Y",
        "doip.type == 0x0004",
        "--count",
        "3",
        "--within",
        "5s",
        "--timeout",
        "10s",
    ]);
    assert_eq!(c.method, "eth.wait_for");
    assert_eq!(c.params["count"], json!({"min": 3, "max": null}));
    assert_eq!(c.params["within_ms"], 5000);
    assert_eq!(c.params["timeout_ms"], 10_000);
    assert_eq!(c.wait, Some(Duration::from_secs(10)));

    let c = ok(&["stats", "--by", "conv"]);
    assert_eq!(c.params["by"], "conv");
    let c = ok(&["follow", "tcp", "3", "--max-bytes", "1024"]);
    assert_eq!(
        (c.params["stream"].clone(), c.params["max_bytes"].clone()),
        (json!(3), json!(1024))
    );

    let c = ok(&["send", "udp", "02fd 0004 0000 0000", "--to", "192.168.0.10:13400"]);
    assert_eq!(c.method, "eth.send");
    assert_eq!(c.params["hex"], "02fd000400000000");
    let c = ok(&["send", "raw", "ffffffffffff", "--dry-run"]);
    assert_eq!(
        (c.method.as_str(), c.params["dry_run"].clone()),
        ("eth.send", json!(true))
    );

    let c = ok(&["export", "pcapng", "-o", "/tmp/x.pcapng", "-Y", "doip"]);
    assert_eq!(
        (c.params["path"].clone(), c.params["format"].clone()),
        (json!("/tmp/x.pcapng"), json!("pcapng"))
    );
    let c = ok(&["filter", "check", "tcp.port == 13400"]);
    assert_eq!(c.params["expr"], "tcp.port == 13400");
}

#[test]
fn usage_errors() {
    for args in [
        &["send", "udp", "0102"][..],
        &["send", "raw", "010"],
        &["send", "raw", "0102", "--to", "1.2.3.4:5"],
        &["show", "dut:12"],
        &["trace", "--timeout", "5s"],
        &["wait-for", "-Y", "udp", "--anchor", "eth:dut@seq:1"],
        &["wait-for", "-Y", "udp", "--timeout", "2h"],
        &["wait-for", "--timeout", "1s"],
        &["capture", "start", "en5", "--backend", "replay"],
        &["trace", "--max", "5000"],
    ] {
        assert!(to_call(args).is_err(), "{args:?} should be a usage error");
    }
}

#[test]
fn run_cli_checks_arguments_first() {
    let cli = CliOf::<EthCli>::new();
    let domains: [&dyn DynDomainCli; 1] = [&cli];
    let run = |args: &[&str]| {
        let a: Vec<String> = ["loom", "eth"]
            .iter()
            .chain(args)
            .map(|s| (*s).to_owned())
            .collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        run_cli_with(&a, &domains, &mut o, &mut e)
    };
    assert_eq!(run(&["send", "udp", "01"]), 2, "usage error → exit 2");
    assert_eq!(
        run(&["filter", "check", "udp"]),
        1,
        "P0 kit: not implemented after argument checks"
    );
}

#[test]
fn trace_renders_tshark_columns_and_trailer() {
    let call = ok(&["trace", "dut", "--since", "seq:1"]);
    let row = |seq: &str, own: Value| {
        json!({"ref": format!("eth:dut@seq:{seq}"), "seq": seq, "ts": {"wall_ns": "3723000456000"},
               "src": "192.168.0.2", "dst": "192.168.0.10", "proto": "UDS", "len": 64,
               "info": "Diagnostic message 0x0E00 → 0x1001: ReadDataByIdentifier F190", "attributed_origin": own})
    };
    let data = json!({
        "rows": [row("1", Value::Null), row("3", json!("agent"))],
        "truncated": false, "matched": 2, "next_cursor": "seq:4",
        "gaps": [{"kind": "source", "count": 3}, {"kind": "unpublished", "count": 8}],
    });
    let golden = concat!(
        "     SEQ TIME            SOURCE                 DESTINATION            PROTO        LEN INFO ORG\n",
        "       1 01:02:03.000456 192.168.0.2            192.168.0.10           UDS           64 Diagnostic message 0x0E00 → 0x1001: ReadDataByIdentifier F190\n",
        "       3 01:02:03.000456 192.168.0.2            192.168.0.10           UDS           64 Diagnostic message 0x0E00 → 0x1001: ReadDataByIdentifier F190 AGT\n",
        "(2 rows · source.gap 1 · display 2/10 · next: --since seq:4)\n",
    );
    assert_eq!(render(&call, data), golden);
}

#[test]
fn show_wait_and_check_render() {
    let call = ok(&["show", "eth:dut@seq:3", "--hex"]);
    let data = json!({
        "ref": "eth:dut@seq:3", "len": 4, "caplen": 4, "truncated": false,
        "bytes": {"encoding": "base64", "data": "AEFCQw=="},
        "tree": [{"name": "doip", "label": "DoIP", "children": [{"name": "doip.type", "label": "Type: 0x8001", "children": []}]}],
    });
    assert_eq!(
        render(&call, data),
        "eth:dut@seq:3 · 4 B on wire, 4 B captured\nDoIP\n    Type: 0x8001\n0000  00 41 42 43                                      .ABC\n"
    );

    let call = ok(&["wait-for", "-Y", "uds", "--timeout", "1s"]);
    let data = json!({"matched": true, "count": 1, "gaps": [],
                      "evidence": [{"ref": "eth:dut@seq:11..12", "matched_refs": ["eth:dut@seq:11"]}]});
    assert_eq!(
        render(&call, data),
        "matched · 1 matching packets · window eth:dut@seq:11..12\n  eth:dut@seq:11\n"
    );

    let call = ok(&["filter", "check", "tcp.port =="]);
    let data = json!({"ok": false, "error": {"pos": 11, "expected": "value", "msg": "missing value"}});
    assert_eq!(
        render(&call, data),
        "tcp.port ==\n           ^ missing value (expected value)\n"
    );
}

#[test]
fn keep_open_is_tcp_only_and_from_port_udp_only() {
    let c = ok(&[
        "send",
        "tcp",
        "02fd000500000007",
        "--to",
        "192.168.0.10:13400",
        "--keep-open",
    ]);
    assert_eq!(
        (c.method.as_str(), c.params["keep_open"].clone()),
        ("eth.send", json!(true))
    );
    let c = ok(&[
        "send",
        "tcp",
        "00",
        "--to",
        "192.168.0.10:13400",
        "--keep-open",
        "--dry-run",
    ]);
    assert_eq!(
        (c.params["keep_open"].clone(), c.params["dry_run"].clone()),
        (json!(true), json!(true))
    );
    let c = ok(&[
        "send",
        "udp",
        "00",
        "--to",
        "192.168.0.10:13400",
        "--from-port",
        "13400",
    ]);
    assert_eq!(c.params["from_port"], 13400);
    assert!(
        ok(&["send", "tcp", "00", "--to", "192.168.0.10:13400"])
            .params
            .get("keep_open")
            .is_none()
    );
    for bad in [
        &["send", "udp", "00", "--to", "192.168.0.10:13400", "--keep-open"][..],
        &["send", "raw", "00", "--keep-open"],
        &[
            "send",
            "tcp",
            "00",
            "--to",
            "192.168.0.10:13400",
            "--from-port",
            "5000",
        ],
        &["send", "raw", "00", "--from-port", "5000"],
    ] {
        assert!(to_call(bad).is_err(), "{bad:?}");
    }
}
