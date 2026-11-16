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
    assert_eq!(c.wait, Some(Duration::from_secs(22)));
    let c = ok(&["trace", "--follow", "--timeout", "2h"]);
    assert_eq!(
        c.wait,
        Some(Duration::from_secs(602)),
        "follow is capped at 600 s"
    );

    let c = ok(&["last", "-n", "5"]);
    assert_eq!(
        (c.method.as_str(), c.params["max"].clone()),
        ("eth.query", json!(5))
    );

    let c = ok(&["show", "eth:dut@seq:12", "--hex"]);
    // B3b: absent --decode-as is sent as null like other absent options
    assert_eq!(
        c.params,
        json!({"ref": "eth:dut@seq:12", "hex": true, "decode_as": null})
    );

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
    assert_eq!(c.wait, Some(Duration::from_secs(12)));

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

/// operator-upgrade B3a (ETH-01, FUNCTIONS.md F3.8): the human trailer states completeness from the
/// daemon's B1 honesty fields; an older daemon (no `partial_reasons`) renders exactly as before.
#[test]
fn trace_and_export_render_completeness_from_partial_reasons() {
    let call = ok(&["trace", "dut"]);
    let row = json!({"ref": "eth:dut@seq:5", "seq": "5", "ts": {"wall_ns": "3723000456000"}, "src": "a", "dst": "b",
                     "proto": "UDP", "len": 60, "info": "x", "attributed_origin": null});
    let base = json!({"rows": [row], "truncated": false, "matched": 1, "gaps": []});
    let with = |extra: Value| {
        let mut d = base.clone();
        for (k, v) in extra.as_object().expect("object") {
            d[k] = v.clone();
        }
        render(&call, d)
    };
    let last = |s: String| s.lines().last().expect("trailer").to_owned();
    assert_eq!(
        last(with(json!({"partial_reasons": [], "complete": true}))),
        "(1 rows · complete)"
    );
    assert_eq!(
        last(with(
            json!({"partial_reasons": ["default_lookback"], "complete": false, "prev_cursor": "seq:4"})
        )),
        "(1 rows · older: --until seq:4 · partial: default_lookback)"
    );
    assert_eq!(
        last(with(
            json!({"partial_reasons": ["scan_budget", "loss"], "complete": false, "resume_cursor": "e1:seq:77"})
        )),
        "(1 rows · partial: scan_budget,loss · resume: --since e1:seq:77)"
    );
    // complete:true but a reason present (inconsistent daemon) is still partial
    assert_eq!(
        last(with(json!({"partial_reasons": ["row_cap"], "complete": true}))),
        "(1 rows · partial: row_cap)"
    );
    // older daemon: unchanged
    assert_eq!(last(render(&call, base.clone())), "(1 rows)");

    let call = ok(&["export", "pcapng", "-o", "/tmp/x.pcapng"]);
    let e = |extra: Value| {
        let mut d = json!({"path": "/tmp/x.pcapng", "packets": 3, "bytes": 180, "truncated": false});
        for (k, v) in extra.as_object().expect("object") {
            d[k] = v.clone();
        }
        render(&call, d)
    };
    assert_eq!(
        e(json!({"partial_reasons": [], "complete": true})),
        "wrote 3 packets (180 B) to /tmp/x.pcapng · complete\n"
    );
    assert_eq!(
        e(json!({"partial_reasons": ["output_cap"], "complete": false, "truncated": true})),
        "wrote 3 packets (180 B) to /tmp/x.pcapng · truncated · partial: output_cap\n"
    );
    assert_eq!(e(json!({})), "wrote 3 packets (180 B) to /tmp/x.pcapng\n");
}

/// operator-upgrade B3b (ETH-06, FUNCTIONS.md F5.13): `--decode-as tcp|udp:PORT=doip|someip`, repeatable,
/// on trace / last / show / stats / export; absent → `decode_as: null`.
#[test]
fn decode_as_flag_maps_to_the_request_array() {
    let want = json!([{"transport": "udp", "port": 64681, "proto": "doip"}, {"transport": "tcp", "port": 30509, "proto": "someip"}]);
    for args in [
        vec!["trace", "dut"],
        vec!["last"],
        vec!["show", "eth:dut@seq:3"],
        vec!["stats"],
        vec!["export", "pcapng", "-o", "/tmp/x.pcapng"],
    ] {
        let mut a = args.clone();
        a.extend(["--decode-as", "udp:64681=doip", "--decode-as", "tcp:30509=someip"]);
        assert_eq!(ok(&a).params["decode_as"], want, "{args:?}");
        assert_eq!(ok(&args).params["decode_as"], Value::Null, "{args:?}");
    }
    for bad in [
        "udp:0=doip",
        "udp:65536=doip",
        "sctp:1=doip",
        "udp:1=uds",
        "udp:x=doip",
        "udp-1=doip",
        "",
    ] {
        let e = to_call(&["trace", "--decode-as", bad]).expect_err(bad);
        assert!(e.contains("tcp|udp:PORT=doip|someip"), "{bad}: {e}");
    }
    assert_eq!(
        loom_eth::cli::parse_decode_as("udp:1=someip"),
        Ok(json!({"transport": "udp", "port": 1, "proto": "someip"}))
    );
}

/// operator-upgrade B5: every command line the loom-eth skill documents for the upgrade parses.
#[test]
fn skill_examples_parse() {
    let c = ok(&[
        "capture",
        "start",
        "host.pcapng",
        "--replay",
        "/path/on/daemon/host.pcapng",
        "--ch",
        "rp1",
        "--speed",
        "max",
        "--repeat",
    ]);
    assert_eq!(c.params["backend"], "replay");
    assert_eq!(
        c.params["replay"],
        json!({"path": "/path/on/daemon/host.pcapng", "speed": "max", "repeat": true})
    );
    let c = ok(&[
        "trace",
        "dut",
        "-Y",
        "doip",
        "--since",
        "seq:1",
        "--decode-as",
        "udp:64681=doip",
    ]);
    assert_eq!(c.params["range"]["from"], "seq:1");
    assert_eq!(
        c.params["decode_as"],
        json!([{"transport": "udp", "port": 64681, "proto": "doip"}])
    );
    assert_eq!(
        ok(&["trace", "dut", "--since", "e3:seq:1370"]).params["range"]["from"],
        "e3:seq:1370"
    );
    let c = ok(&["follow", "udp", "3"]);
    assert_eq!(
        (c.params["proto"].clone(), c.params["stream"].clone()),
        (json!("udp"), json!(3))
    );
    assert_eq!(ok(&["capture", "status"]).method, "eth.capture.status");
}
