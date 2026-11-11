//! CLI piece (K3): argument → RpcCall conversion and human rendering, no daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use loom_can::cli::CanCli;
use loom_can::{
    RPC_DB_INFO, RPC_DB_LOAD, RPC_EXPECT, RPC_LAST, RPC_SEND, RPC_SIG_GET, RPC_SIG_WAIT_FOR, RPC_STATUS,
    RPC_TRACE_QUERY,
};
use loom_cli_kit::{CliOf, DomainCli, DynDomainCli, RpcCall, run_cli_with};
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast};
use loom_proto::error::ErrorCode;
use serde_json::{Value, json};

fn call(args: &[&str]) -> Result<RpcCall, String> {
    let argv: Vec<&str> = std::iter::once("can").chain(args.iter().copied()).collect();
    let m = CanCli::command()
        .try_get_matches_from(argv)
        .map_err(|e| format!("clap: {e}"))?;
    CanCli::to_call(&m).map_err(|e| format!("usage: {e}"))
}

#[test]
fn send_conversion() {
    let c = call(&["send", "300#01"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        (RPC_SEND, json!({"frame": "300#01"}))
    );
    assert_eq!(c.wait, None);

    let c = call(&[
        "send", "300#01", "-c", "ch1", "--count", "3", "--gap", "10", "--expect", "0x301", "--within", "50",
    ])
    .unwrap();
    assert_eq!(
        c.params,
        json!({"channel": "ch1", "frame": "300#01", "count": 3, "gap_ms": 10,
               "expect": {"id": 0x301, "ext": false, "within_ms": 50}})
    );
    assert_eq!(c.wait, Some(Duration::from_millis(2_050)));
    let c = call(&["send", "18DAF110#01", "--expect", "0x18DA10F1", "--dry-run"]).unwrap();
    assert_eq!(
        c.params["expect"],
        json!({"id": 0x18DA10F1_u32, "ext": true, "within_ms": 100})
    );
    assert_eq!(c.params["dry_run"], json!(true));
    let c = call(&["send", "300#01", "--ext"]).unwrap();
    assert_eq!(c.params["ext"], json!(true));

    for bad in [
        &["send", "300#0"][..],                    // odd hex
        &["send", "300#01", "--within", "10"][..], // --within without --expect
        &["send", "1FFFFFFFF#01"][..],             // id too large
    ] {
        assert!(call(bad).unwrap_err().starts_with("usage"), "{bad:?}");
    }
    for bad in [
        &["send", "300#01", "--count", "0"][..],
        &["send", "300#01", "--count", "101"][..],
        &["send", "300#01", "--gap", "10001"][..],
        &["send", "300#01", "--expect", "zz"][..],
        &["send", "300#01", "--within", "60001", "--expect", "1"][..],
    ] {
        assert!(call(bad).unwrap_err().starts_with("clap"), "{bad:?}");
    }
}

#[test]
fn trace_last_expect_status_conversion() {
    let c = call(&[
        "trace",
        "--since",
        "-5s",
        "--until",
        "seq:900",
        "--ids",
        "0x300,0x7e0-0x7ef,768",
        "--origin",
        "agent,device",
        "--dir",
        "rx",
        "--max",
        "50",
        "--peek",
    ])
    .unwrap();
    assert_eq!(c.method, RPC_TRACE_QUERY);
    assert_eq!(
        c.params,
        json!({"since": "-5s", "until": "seq:900", "ids": ["0x300", "0x7E0-0x7EF", "0x300"],
               "origins": ["agent", "device"], "dir": "rx", "max": 50, "peek": true})
    );
    assert_eq!(call(&["trace"]).unwrap().params, json!({}));
    for bad in [
        &["trace", "--max", "1001"][..],
        &["trace", "--max", "0"][..],
        &["trace", "--origin", "robot"][..],
        &["trace", "--ids", "0x900-0x100"][..],
        &["trace", "--dir", "up"][..],
        &["trace", "--since", "yesterday"][..],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }

    let c = call(&["last", "--id", "0x310", "--window", "5000"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params),
        (RPC_LAST, json!({"id": "0x310", "window_ms": 5000}))
    );

    let c = call(&[
        "expect", "--id", "0x301", "--data", "01", "--mask", "FF", "--origin", "device", "--after", "seq:12",
        "--within", "200",
    ])
    .unwrap();
    assert_eq!(c.method, RPC_EXPECT);
    assert_eq!(
        c.params,
        json!({"id": 0x301, "data": "01", "mask": "ff", "origin": "device", "after": "seq:12", "within_ms": 200})
    );
    assert_eq!(c.wait, Some(Duration::from_millis(2_200)));
    assert!(
        call(&["expect", "--id", "0x301"]).is_err(),
        "--within is required"
    );
    assert!(
        call(&["expect", "--id", "1", "--mask", "FF", "--within", "9"])
            .unwrap_err()
            .starts_with("usage")
    );
    assert!(
        call(&[
            "expect", "--id", "1", "--data", "01", "--mask", "FFFF", "--within", "9"
        ])
        .unwrap_err()
        .starts_with("usage")
    );

    let c = call(&["status", "--channel", "ch0"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params),
        (RPC_STATUS, json!({"channel": "ch0"}))
    );
}

#[test]
fn run_cli_exit_codes() {
    let cli = CliOf::<CanCli>::new();
    let domains: [&dyn DynDomainCli; 1] = [&cli];
    let run = |args: &[&str]| {
        let a: Vec<String> = std::iter::once("loom")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = run_cli_with(&a, &domains, &mut o, &mut e);
        (code, String::from_utf8(o).unwrap())
    };
    assert_eq!(run(&["can", "trace", "--max", "5000"]).0, 2);
    assert_eq!(run(&["can", "send", "300#0"]).0, 2, "usage error from to_call");
    let (code, out) = run(&["can", "send", "300#01", "--json"]);
    assert_eq!(code, 1, "client not wired yet in cli-kit (E_NOT_IMPLEMENTED)");
    assert!(out.contains("can.send"));
}

// ---- human rendering goldens ------------------------------------------------------------------

fn frame_payload(id: u32, data: &str) -> Value {
    json!({"id": id, "ext": false, "fd": false, "brs": false, "esi": false, "rtr": false,
           "dlc": data.len() / 2, "len": data.len() / 2, "bytes": {"encoding": "hex", "data": data}})
}

fn rec(seq: u64, ms: u64, kind: &str, origin: &str, dir: &str, payload: Value, rid: Option<&str>) -> Value {
    let mut v = json!({"schema_version": 1, "session_id": "s", "domain": "can", "channel": "ch0",
        "kind": kind, "seq": seq.to_string(), "epoch": "1", "producer_id": "can_sim-1",
        "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": {"mono_ns": (ms * 1_000_000).to_string(), "wall_ns": "0", "clock_id": "c", "source": "synthetic"},
        "dir": dir, "origin": origin, "payload": payload});
    if let Some(r) = rid {
        v["request_id"] = json!(r);
    }
    v
}

fn render(method: &str, data: Value) -> String {
    let env = Envelope::success(method, data, Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    CanCli::render_human(&RpcCall::new(method, json!({})), &env, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn trace_render_golden() {
    let tx_row = |rid: &str, state: &str, id: u32, data: &str| json!({"write_state": state, "frame": frame_payload(id, data), "count": 1, "sent": 1, "rid": rid});
    let rows = json!([
        rec(
            1,
            812_004,
            "can.frame",
            "agent",
            "tx",
            frame_payload(0x300, "01"),
            Some("r1")
        ),
        rec(
            2,
            812_004,
            "can.tx",
            "agent",
            "tx",
            tx_row("r1", "sent", 0x300, "01"),
            Some("r1")
        ),
        rec(
            3,
            812_009,
            "can.frame",
            "device",
            "rx",
            frame_payload(0x301, "01"),
            None
        ),
        rec(
            4,
            812_050,
            "can.frame",
            "device",
            "rx",
            frame_payload(0x310, "0100000000000000"),
            None
        ),
        rec(
            5,
            818_731,
            "can.tx",
            "user",
            "tx",
            tx_row("r2", "requested", 0x300, "00"),
            Some("r2")
        ),
        rec(
            6,
            818_732,
            "can.tx",
            "user",
            "tx",
            tx_row("r2", "failed", 0x300, "00"),
            Some("r2")
        ),
    ]);
    let out = render(
        RPC_TRACE_QUERY,
        json!({"rows": rows, "next_cursor": "e1:seq:7", "truncated": true, "complete": true}),
    );
    let expected = "   812.004  tx AGT 0x300       1  01  sent
   812.009  rx DEV 0x301       1  01
   812.050  rx DEV 0x310       8  01 00 00 00 00 00 00 00
   818.732  tx USR 0x300       1  00  failed
-- 4 rows · next e1:seq:7 · truncated (narrow --ids/--since or continue with the cursor)
";
    assert_eq!(out, expected, "\n{out}");
}

#[test]
fn send_expect_last_render_golden() {
    let out = render(
        RPC_SEND,
        json!({"dry_run": true, "channel": "ch0", "frame": "300#01", "bytes": "01", "count": 1,
               "risk": "R1", "reason": "sim channel", "policy": "auto"}),
    );
    assert_eq!(
        out,
        "DRY-RUN ch0 300#01 ×1  bytes 01  risk=R1 (sim channel)  policy=auto  (nothing sent)\n"
    );

    let resp = rec(
        5013,
        812_009,
        "can.frame",
        "device",
        "rx",
        frame_payload(0x301, "01"),
        None,
    );
    let out = render(
        RPC_SEND,
        json!({"channel": "ch0", "frame": "300#01", "request_id": "r1", "risk": "R1", "reason": "sim channel",
               "state": "sent", "write_state": "effective_verified",
               "expect": {"matched": true, "frame": resp, "refs": ["can:ch0@e1:seq:5013"]}}),
    );
    assert_eq!(
        out,
        "effective_verified ch0 300#01  request r1  risk=R1 (sim channel)
expect: DEV 0x301 [1] 01  seq:5013
evidence: can:ch0@e1:seq:5013
"
    );

    let out = render(
        RPC_SEND,
        json!({"channel": "ch0", "frame": "300#01", "request_id": "r2", "risk": "R1", "reason": "sim channel",
               "write_state": "effect_unverified", "expect": {"matched": false, "within_ms": 100, "refs": []}}),
    );
    assert!(
        out.contains("expect: no response within 100 ms → effect_unverified"),
        "{out}"
    );

    let out = render(
        RPC_EXPECT,
        json!({"matched": true, "frame": resp, "refs": ["can:ch0@e1:seq:5013"]}),
    );
    assert_eq!(
        out,
        "MATCH DEV 0x301 [1] 01  seq:5013\nevidence: can:ch0@e1:seq:5013\n"
    );

    let last = rec(
        21,
        812_050,
        "can.frame",
        "device",
        "rx",
        frame_payload(0x310, "01"),
        None,
    );
    let out = render(
        RPC_LAST,
        json!({"window_ms": 10000, "rows": [{"id": 0x310, "ext": false, "count": 12, "last": last}],
               "truncated": false, "complete": true}),
    );
    assert_eq!(
        out,
        "0x310       1  01                       ×12     DEV  seq:21\n-- 1 ids in the last 10000 ms\n"
    );
}

#[test]
fn timeout_render_cites_the_scanned_window() {
    let body = ErrorBody {
        code: ErrorCode::Timeout,
        message: "no matching frame 0x301 on ch0 within 100 ms".into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: json!({"refs": ["can:ch0@e1:seq:10..14"], "evidence": []}),
    };
    let env: Envelope<Value> =
        Envelope::failure(RPC_EXPECT, body, Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    CanCli::render_human(&RpcCall::new(RPC_EXPECT, json!({})), &env, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        out,
        "scanned: can:ch0@e1:seq:10..14\nerror[E_TIMEOUT]: no matching frame 0x301 on ch0 within 100 ms\n"
    );
    assert_eq!(env.exit_code(), 3);
}

// ---- signals (M2a) ----------------------------------------------------------------------------

#[test]
fn db_and_sig_conversion() {
    let c = call(&["db", "load", "body.dbc", "-c", "ch1", "--encoding", "windows1252"]).unwrap();
    assert_eq!(c.method, RPC_DB_LOAD);
    let path = c.params["path"].as_str().unwrap();
    assert!(
        std::path::Path::new(path).is_absolute(),
        "relative paths become absolute: {path}"
    );
    assert!(path.ends_with("body.dbc"));
    assert_eq!(
        (c.params["channel"].clone(), c.params["encoding"].clone()),
        (json!("ch1"), json!("windows1252"))
    );
    let c = call(&["db", "load", "/x/y.dbc"]).unwrap();
    assert_eq!(c.params, json!({"path": "/x/y.dbc"}));
    assert_eq!(call(&["db", "info"]).unwrap().method, RPC_DB_INFO);
    assert!(call(&["db"]).is_err());
    assert!(call(&["db", "load", "a.dbc", "--encoding", "ebcdic"]).is_err());

    let c = call(&[
        "sig",
        "get",
        "ESP_01.VehicleSpeed",
        "--window",
        "500",
        "--origin",
        "device",
    ])
    .unwrap();
    assert_eq!(c.method, RPC_SIG_GET);
    assert_eq!(
        c.params,
        json!({"signal": "ESP_01.VehicleSpeed", "window_ms": 500, "origin": "device"})
    );
    assert!(call(&["sig", "get", "X", "--window", "60001"]).is_err());

    let c = call(&[
        "sig",
        "wait-for",
        "EngRPM>3000",
        "--timeout",
        "10",
        "--after",
        "seq:7",
    ])
    .unwrap();
    assert_eq!(c.method, RPC_SIG_WAIT_FOR);
    assert_eq!(
        c.params,
        json!({"expr": "EngRPM > 3000", "after": "seq:7", "within_ms": 10_000})
    );
    assert_eq!(c.wait, Some(Duration::from_millis(12_000)));
    for (t, ms) in [
        ("1.5", 1_500),
        ("500ms", 500),
        ("2m", 120_000),
        ("30s", 30_000),
        ("3600", 3_600_000),
    ] {
        let c = call(&["sig", "wait-for", "A between 1 2", "--timeout", t]).unwrap();
        assert_eq!(c.params["within_ms"], json!(ms), "{t}");
    }
    let c = call(&["sig", "wait-for", "A == 1", "--timeout", "1", "--any-origin"]).unwrap();
    assert_eq!(c.params["any_origin"], json!(true));
    for bad in [
        &["sig", "wait-for", "A > 1", "--timeout", "0"][..],
        &["sig", "wait-for", "A > 1", "--timeout", "3601"][..],
        &["sig", "wait-for", "A > 1", "--timeout", "soon"][..],
        &["sig", "wait-for", "A > 1"][..],
        &[
            "sig",
            "wait-for",
            "A > 1",
            "--timeout",
            "1",
            "--origin",
            "agent",
            "--any-origin",
        ][..],
    ] {
        assert!(call(bad).unwrap_err().starts_with("clap"), "{bad:?}");
    }
    assert!(
        call(&["sig", "wait-for", "A = 1", "--timeout", "1"])
            .unwrap_err()
            .starts_with("usage"),
        "the expression is checked before sending"
    );

    let c = call(&["trace", "--decode"]).unwrap();
    assert_eq!(c.params, json!({"decode": true}));
    let c = call(&["last", "--decode", "--id", "0x100"]).unwrap();
    assert_eq!(c.params, json!({"decode": true, "id": "0x100"}));
}

#[test]
fn signal_render_golden() {
    let out = render(
        RPC_SIG_GET,
        json!({"name": "VehicleSpeed", "value": 37.25, "unit": "km/h", "raw": 9536, "label": null,
               "message": "ESP_01", "id": "0x0FD", "age_ms": 18, "complete": true,
               "refs": ["can:ch0@e1:seq:42#/signals/VehicleSpeed"]}),
    );
    assert_eq!(
        out,
        "VehicleSpeed = 37.25 km/h  (ESP_01 0x0FD, age 18 ms)\nevidence: can:ch0@e1:seq:42#/signals/VehicleSpeed\n"
    );
    let out = render(
        RPC_SIG_GET,
        json!({"name": "Gear", "value": 2.0, "unit": "", "label": "N", "message": "EngineData", "id": "0x100",
               "age_ms": null, "complete": false, "refs": []}),
    );
    assert_eq!(
        out,
        "Gear = 2 \"N\"  (EngineData 0x100)\nwarning: the window was not read completely; a newer value may exist\n"
    );

    let out = render(
        RPC_SIG_WAIT_FOR,
        json!({"name": "EngRPM", "value": 3500.0, "unit": "rpm", "message": "EngineData", "id": "0x100",
               "seq": "5", "previous": {"value": 2000.0, "unit": "rpm"},
               "refs": ["can:ch0@e1:seq:5#/signals/EngRPM"]}),
    );
    assert_eq!(
        out,
        "MATCH EngRPM = 3500 rpm  (EngineData 0x100, seq:5, was 2000 rpm)\nevidence: can:ch0@e1:seq:5#/signals/EngRPM\n"
    );

    let body = ErrorBody {
        code: ErrorCode::Timeout,
        message: "EngineData.EngRPM > 3000 did not become true on ch0 within 120 ms".into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: json!({"refs": ["can:ch0@e1:seq:1..2#/signals/EngRPM"],
                       "last": {"name": "EngRPM", "value": 1200.0, "unit": "rpm", "seq": "2"}}),
    };
    let env: Envelope<Value> = Envelope::failure(
        RPC_SIG_WAIT_FOR,
        body,
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    CanCli::render_human(&RpcCall::new(RPC_SIG_WAIT_FOR, json!({})), &env, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "scanned: can:ch0@e1:seq:1..2#/signals/EngRPM\nlast: EngRPM = 1200 rpm  seq:2\n\
         error[E_TIMEOUT]: EngineData.EngRPM > 3000 did not become true on ch0 within 120 ms\n"
    );
}

#[test]
fn db_and_decoded_trace_render_golden() {
    let out = render(
        RPC_DB_LOAD,
        json!({"channel": "ch0", "loaded": true, "path": "/dbc/body.dbc",
               "db_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
               "messages": 4, "signals": 10, "encoding": "utf8", "encoding_guessed": true,
               "warnings": [{"line": 9, "kind": "StatementDropped", "detail": "SG_ B1"}], "warnings_total": 3,
               "blocked": [{"message": "Broken", "id": "0x065", "decode": "overlapping signals", "encode": null}],
               "blocked_total": 1}),
    );
    assert_eq!(
        out,
        "ch0: /dbc/body.dbc  sha256 0123456789ab…  4 messages, 10 signals  (encoding guessed: utf8)
  warning: line 9: StatementDropped: SG_ B1
  … 2 more warnings
  blocked: Broken (0x065) decode: overlapping signals
"
    );
    assert_eq!(
        render(
            RPC_DB_INFO,
            json!({"channels": [{"channel": "ch1", "loaded": false}]})
        ),
        "ch1: no DBC loaded\n"
    );
    assert_eq!(
        render(RPC_DB_INFO, json!({"channels": []})),
        "no DBC loaded (loom can db load <file.dbc>)\n"
    );

    let mut r = rec(
        3,
        812_009,
        "can.frame",
        "device",
        "rx",
        frame_payload(0x100, "0032F60400000000"),
        None,
    );
    r["decoded"] = json!({"message": "EngineData", "signals": [
        {"name": "EngRPM", "value": 3200.0, "unit": "rpm", "raw": 12800},
        {"name": "EngTemp", "value": -10.0, "unit": "degC", "raw": -10},
        {"name": "Gear", "value": 2.0, "unit": "", "raw": 2, "label": "N"},
        {"name": "Volt", "unavailable": "short_frame", "needed": 8, "actual": 7},
        {"name": "Ratio", "value": 0.30000000000000004, "unit": "", "raw": 3}]});
    let mut bad = rec(
        4,
        812_010,
        "can.frame",
        "device",
        "rx",
        frame_payload(0x200, "01"),
        None,
    );
    bad["decoded"] = json!({"message": null, "error": "several DBC messages share this id"});
    let out = render(
        RPC_TRACE_QUERY,
        json!({"rows": [r, bad], "truncated": false, "complete": true}),
    );
    assert_eq!(
        out,
        "   812.009  rx DEV 0x100       8  00 32 F6 04 00 00 00 00
            EngineData: EngRPM=3200 rpm, EngTemp=-10 degC, Gear=2 \"N\", Volt=n/a, Ratio=0.3
   812.010  rx DEV 0x200       1  01
            ?: (not decoded: several DBC messages share this id)
-- 2 rows
"
    );
}

// ---- ISO-TP (debug) ---------------------------------------------------------------------------

#[test]
fn isotp_conversion() {
    let c = call(&["isotp", "open", "--tx", "0x7E0", "--rx", "0x7E8"]).unwrap();
    assert_eq!(c.method, "can.isotp.open");
    assert_eq!(c.params, json!({"tx_id": 0x7E0, "rx_id": 0x7E8}));
    let c = call(&[
        "isotp",
        "open",
        "--tx",
        "0x18DA10F1",
        "--rx",
        "0x18DAF110",
        "-c",
        "ch1",
        "--fd",
        "--brs",
        "--tx-dl",
        "32",
        "--padding",
        "none",
        "--bs",
        "0",
        "--stmin",
        "5",
        "--n-bs",
        "500",
        "--n-cr",
        "800",
        "--owner",
        "me",
    ])
    .unwrap();
    assert_eq!(
        c.params,
        json!({"channel": "ch1", "tx_id": 0x18DA10F1_u32, "rx_id": 0x18DAF110_u32, "fd": true, "brs": true,
               "tx_dl": 32, "padding": null, "block_size": 0, "st_min": 5, "n_bs_ms": 500, "n_cr_ms": 800,
               "owner": "me"})
    );
    let c = call(&[
        "isotp",
        "open",
        "--tx",
        "0x7E8",
        "--rx",
        "0x7E0",
        "--server",
        "--model",
        "vin_demo",
        "--padding",
        "55",
    ])
    .unwrap();
    assert_eq!(
        (
            c.params["role"].clone(),
            c.params["model"].clone(),
            c.params["padding"].clone()
        ),
        (json!("server"), json!("vin_demo"), json!(0x55))
    );
    assert!(
        call(&["isotp", "open", "--tx", "1", "--rx", "2", "--model", "echo"]).is_err(),
        "--model needs --server"
    );
    assert!(call(&["isotp", "open", "--tx", "1"]).is_err());
    assert!(
        call(&["isotp", "open", "--tx", "1", "--rx", "2", "--padding", "zz"])
            .unwrap_err()
            .starts_with("usage")
    );

    let c = call(&["isotp", "send", "tp1", "22 F1 90"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        ("can.isotp.send", json!({"endpoint": "tp1", "pdu": "22f190"}))
    );
    assert!(
        call(&["isotp", "send", "tp1", "2"])
            .unwrap_err()
            .starts_with("usage")
    );
    assert_eq!(
        call(&["isotp", "abort", "tp1"]).unwrap().params,
        json!({"endpoint": "tp1"})
    );
    assert_eq!(
        call(&["isotp", "close", "tp1"]).unwrap().method,
        "can.isotp.close"
    );
    assert_eq!(
        call(&["isotp", "list", "-c", "ch0"]).unwrap().params,
        json!({"channel": "ch0"})
    );
}

#[test]
fn isotp_render_golden() {
    assert_eq!(
        render(
            "can.isotp.open",
            json!({"endpoint": "tp1", "channel": "ch0", "tx_id": "0x7E0", "rx_id": "0x7E8", "role": "client",
                   "epoch": 3})
        ),
        "tp1 open on ch0: tx 0x7E0 → rx 0x7E8 (client, epoch 3)\n"
    );
    assert_eq!(
        render(
            "can.isotp.send",
            json!({"accepted": true, "endpoint": "tp1", "transfer": "0.1", "len": 3, "request_id": "r7"})
        ),
        "accepted tp1 transfer 0.1 (3 bytes)  request r7\n"
    );
    assert_eq!(
        render(
            "can.isotp.abort",
            json!({"endpoint": "tp1", "aborted_tx": true, "aborted_rx": false, "residual_tx_possible": 2,
                   "cancelled_frames": 1})
        ),
        "aborted tp1: tx=true rx=false residual_tx_possible=2 cancelled_frames=1\n"
    );
    let list = json!({"endpoints": [{"endpoint": "tp1", "channel": "ch0", "role": "client", "tx_id": "0x7E0",
        "rx_id": "0x7E8", "state": "open", "owner": "uds:s1",
        "counters": {"tx_pdu": 2, "rx_pdu": 2, "tx_err": 0, "rx_err": 1}}]});
    assert_eq!(
        render("can.isotp.list", list),
        "tp1    ch0   client 0x7E0 → 0x7E8  open    tx_pdu=2 rx_pdu=2 tx_err=0 rx_err=1  owner uds:s1\n\
         -- 1 ISO-TP endpoints\n"
    );
}

// ---- periodic / IG ----------------------------------------------------------------------------

#[test]
fn periodic_and_sig_set_conversion() {
    let c = call(&[
        "periodic",
        "start",
        "300#01",
        "--period",
        "100",
        "--offset",
        "5",
        "--count",
        "10",
        "--lease",
        "30",
        "--max-duration",
        "2h",
        "--owner",
        "me",
        "-c",
        "ch1",
    ])
    .unwrap();
    assert_eq!(c.method, "can.periodic.start");
    assert_eq!(
        c.params,
        json!({"channel": "ch1", "frame": "300#01", "period_ms": 100, "offset_ms": 5, "count": 10,
               "lease_ms": 30_000, "max_duration_ms": 7_200_000, "owner": "me"})
    );
    let c = call(&[
        "periodic",
        "start",
        "--msg",
        "EngineData",
        "--set",
        "EngRPM=3000",
        "--set",
        "Gear=D",
        "--force-range",
    ])
    .unwrap();
    assert_eq!(
        c.params,
        json!({"msg": "EngineData", "signals": {"EngRPM": 3000.0, "Gear": "D"}, "force_range": true})
    );
    for bad in [
        &["periodic", "start", "300#01"][..],         // raw needs --period
        &["periodic", "start", "--period", "10"][..], // neither
        &["periodic", "start", "300#01", "--period", "10", "--set", "A=1"][..], // --set needs --msg
        &["periodic", "start", "--msg", "M", "--set", "A=1", "--set", "A=2"][..],
        &["periodic", "start", "300#0", "--period", "10"][..],
    ] {
        assert!(call(bad).unwrap_err().starts_with("usage"), "{bad:?}");
    }
    for bad in [
        &["periodic", "start", "300#01", "--msg", "M"][..],
        &["periodic", "start", "300#01", "--period", "60001"][..],
        &["periodic", "start", "300#01", "--period", "10", "--count", "0"][..],
        &["periodic", "start", "300#01", "--period", "10", "--lease", "25h"][..],
        &["periodic", "start", "--msg", "M", "--set", "A"][..],
    ] {
        assert!(call(bad).unwrap_err().starts_with("clap"), "{bad:?}");
    }

    let c = call(&["periodic", "update", "j1", "--rev", "3", "--data", "0102"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        (
            "can.periodic.update",
            json!({"job": "j1", "expected_rev": 3, "patch": {"data": "0102"}})
        )
    );
    let c = call(&[
        "periodic", "update", "j1", "--rev", "3", "--mask", "FF00", "--bytes", "0900",
    ])
    .unwrap();
    assert_eq!(c.params["patch"], json!({"mask": "ff00", "bytes": "0900"}));
    assert!(
        call(&["periodic", "update", "j1", "--rev", "3"])
            .unwrap_err()
            .starts_with("usage")
    );
    assert!(
        call(&["periodic", "update", "j1", "--data", "01"]).is_err(),
        "--rev is required"
    );

    assert_eq!(
        call(&["periodic", "stop", "j1"]).unwrap().params,
        json!({"job": "j1"})
    );
    assert_eq!(
        call(&["periodic", "stop", "--mine"]).unwrap().params,
        json!({"all_mine": true})
    );
    assert!(call(&["periodic", "stop"]).unwrap_err().starts_with("usage"));
    assert!(call(&["periodic", "stop", "j1", "--mine"]).is_err());
    assert_eq!(
        call(&["periodic", "renew", "j1"]).unwrap().method,
        "can.periodic.renew"
    );
    assert_eq!(
        call(&["periodic", "takeover", "j1"]).unwrap().params,
        json!({"job": "j1"})
    );
    assert_eq!(call(&["periodic", "list"]).unwrap().method, "can.periodic.list");

    let c = call(&["sig", "set", "EngineData.EngRPM=2500.5", "--job", "j1"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        (
            "can.sig.set",
            json!({"signal": "EngineData.EngRPM", "value": 2500.5, "job": "j1"})
        )
    );
    assert_eq!(
        call(&["sig", "set", "Gear=D"]).unwrap().params["value"],
        json!("D")
    );
    assert!(call(&["sig", "set", "Gear"]).is_err());
    assert!(call(&["sig", "set", "=3"]).is_err());
}

#[test]
fn periodic_render_golden() {
    let mut start = RpcCall::new("can.periodic.start", json!({}));
    let env = Envelope::success(
        "can.periodic.start",
        json!({"job": "j1", "revision": 0, "channel": "ch0", "frame": "100#E02EF60600000546", "period_ms": 10,
               "period_from_dbc": true, "risk": "R1", "reason": "sim channel"}),
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    CanCli::render_human(&start, &env, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "started j1 on ch0: 100#E02EF60600000546 every 10 ms (DBC cycle time) (rev 0)  risk=R1 (sim channel)\n"
    );
    start.method = "can.periodic.takeover".into();
    start.params = json!({"job": "j1"});
    let env = Envelope::success(
        "can.periodic.takeover",
        json!({"job": "j7", "revision": 0}),
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    CanCli::render_human(&start, &env, &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "took over j1 → job j7 (rev 0)\n");

    assert_eq!(
        render("can.periodic.update", json!({"job": "j1", "revision": 4})),
        "patched j1 → rev 4\n"
    );
    assert_eq!(render("can.periodic.stop", json!({"job": "j1"})), "stopped j1\n");
    assert_eq!(
        render("can.periodic.stop", json!({"stopped": ["j1", {"job": "j2"}]})),
        "stopped 2 jobs: j1, j2\n"
    );
    assert_eq!(
        render("can.periodic.renew", json!({"job": "j1", "lease_left_ms": 30000})),
        "renewed j1 (lease 30000 ms left)\n"
    );
    let list = json!({"jobs": [{"job": "j1", "channel": "ch0", "id": 256, "ext": false, "period_ms": 10,
        "revision": 4, "origin": "agent", "sent": 100, "missed": 0, "lateness_p99_us": 125, "state": "running",
        "owner": "me"}]});
    assert_eq!(
        render("can.periodic.list", list),
        "j1     ch0   0x100          10 ms  rev 4   agent  sent=100 missed=0 p99=125us  running  owner me\n\
         -- 1 periodic jobs\n"
    );

    let set = json!({"name": "Gear", "value": "D", "unit": "", "job": "j1", "revision": 4,
                     "changed": [{"byte": 3, "old": "04", "new": "06"}], "risk": "R1"});
    assert_eq!(
        render("can.sig.set", set),
        "Gear = \"D\" → j1 rev 4  (bytes 3:04→06)  risk=R1\n"
    );
    let set = json!({"name": "EngRPM", "value": 3000.0, "unit": "rpm", "job": "j1", "revision": 5,
                     "changed": [], "risk": "R1", "warning": "base bytes came from the last sent frame"});
    assert_eq!(
        render("can.sig.set", set),
        "EngRPM = 3000 rpm → j1 rev 5  (no byte changed)  risk=R1\nwarning: base bytes came from the last sent frame\n"
    );
}

#[test]
fn isotp_cancel_recv_barrier_cli() {
    let c = call(&["isotp", "cancel", "tp1", "0.3"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        ("can.isotp.cancel", json!({"endpoint": "tp1", "transfer": "0.3"}))
    );
    let c = call(&[
        "isotp",
        "recv",
        "tp1",
        "--after",
        "4",
        "--timeout",
        "500",
        "--max",
        "8",
    ])
    .unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        (
            "can.isotp.recv",
            json!({"endpoint": "tp1", "after_pdu_seq": "4", "timeout_ms": 500, "max_pdus": 8})
        )
    );
    assert_eq!(c.wait, Some(Duration::from_millis(2_500)));
    assert_eq!(
        call(&["isotp", "recv", "tp1"]).unwrap().params,
        json!({"endpoint": "tp1"})
    );
    let c = call(&[
        "isotp",
        "barrier",
        "tp1",
        "--epoch",
        "3",
        "--deadline-ns",
        "123456789012345678",
    ])
    .unwrap();
    assert_eq!(
        c.params,
        json!({"endpoint": "tp1", "epoch": "3", "deadline_mono_ns": "123456789012345678"})
    );
    for bad in [
        &["isotp", "cancel", "tp1"][..],
        &["isotp", "recv", "tp1", "--max", "17"][..],
        &["isotp", "recv", "tp1", "--timeout", "60001"][..],
        &["isotp", "barrier", "tp1", "--epoch", "3"][..],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }

    assert_eq!(
        render(
            "can.isotp.cancel",
            json!({"endpoint": "tp1", "transfer": "0.3", "state": "cancelled", "residual_tx_possible": 2})
        ),
        "cancelled tp1 transfer 0.3 (residual_tx_possible 2)\n"
    );
    let recv = json!({"endpoint": "tp1", "pdus": [{"pdu_seq": "5", "pdu": {"encoding": "hex", "data": "62f190"},
        "len": 3, "rx_id": "0x7E8"}], "next_pdu_seq": "5", "dropped": "2", "cursor_gap": true});
    assert_eq!(
        render("can.isotp.recv", recv),
        "#5 tp1 62f190 (3 B, rx 0x7E8)\n-- 1 PDUs · next 5 · dropped 2 · CURSOR GAP (older PDUs were dropped)\n"
    );
    assert_eq!(
        render(
            "can.isotp.barrier",
            json!({"endpoint": "tp1", "epoch": "3", "complete": false, "covered_ingest_seq": "41",
                   "transport_event_seq": "17"})
        ),
        "barrier tp1 epoch 3: NOT complete (uncertain)  covered_ingest_seq=41 transport_event_seq=17\n"
    );
}
