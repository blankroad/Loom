//! B7 (operator-upgrade U1-A/U2/U3/U6): CLI flags and verbs → RPC params. Contract: FUNCTIONS.md §B7 (lead-written).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_cli_kit::DomainCli;
use loom_uart::cli::UartCli;
use loom_uart::commands::help_json;
use serde_json::json;

fn call(args: &[&str]) -> loom_cli_kit::RpcCall {
    let m = UartCli::command()
        .try_get_matches_from(std::iter::once("uart").chain(args.iter().copied()))
        .expect("parse");
    UartCli::to_call(&m).expect("to_call")
}

fn parse_fails(args: &[&str]) -> bool {
    UartCli::command()
        .try_get_matches_from(std::iter::once("uart").chain(args.iter().copied()))
        .is_err()
}

fn usage_fails(args: &[&str]) -> bool {
    match UartCli::command().try_get_matches_from(std::iter::once("uart").chain(args.iter().copied())) {
        Err(_) => true,
        Ok(m) => UartCli::to_call(&m).is_err(),
    }
}

#[test]
fn open_options_for_pacing_reconnect_and_log() {
    let o = call(&[
        "open",
        "/dev/ttyUSB0",
        "--as",
        "dut",
        "--char-delay",
        "5",
        "--line-delay",
        "100",
        "--reconnect",
        "--reconnect-window",
        "30",
        "--log",
        "jsonl",
        "--log-rotate",
        "16",
        "--log-quota",
        "2048",
    ]);
    assert_eq!(o.method, "uart.open");
    assert_eq!(
        o.params["pacing"],
        json!({"char_delay_ms": 5, "line_delay_ms": 100})
    );
    assert_eq!(
        o.params["reconnect"],
        json!({"enabled": true, "window_ms": 30_000})
    );
    assert_eq!(
        o.params["log"],
        json!({"format": "jsonl", "rotate_mib": 16, "quota_mib": 2048})
    );
}

#[test]
fn open_defaults_send_nothing_new() {
    let o = call(&["open", "sim://bootshell", "--as", "dut"]);
    for k in ["pacing", "reconnect", "log"] {
        assert!(o.params.get(k).is_none(), "{k} must be absent unless asked");
    }
    let o = call(&[
        "open",
        "sim://bootshell",
        "--log",
        "--reconnect",
        "--line-delay",
        "20",
    ]);
    assert_eq!(o.params["log"], json!({"format": "log"}));
    assert_eq!(o.params["reconnect"], json!({"enabled": true}));
    assert_eq!(o.params["pacing"], json!({"line_delay_ms": 20}));
}

#[test]
fn open_option_bounds_and_dependencies() {
    assert!(parse_fails(&["open", "x", "--char-delay", "1001"]));
    assert!(parse_fails(&["open", "x", "--line-delay", "10001"]));
    assert!(
        parse_fails(&["open", "x", "--reconnect-window", "30"]),
        "window needs --reconnect"
    );
    assert!(parse_fails(&[
        "open",
        "x",
        "--reconnect",
        "--reconnect-window",
        "121"
    ]));
    assert!(parse_fails(&[
        "open",
        "x",
        "--reconnect",
        "--reconnect-window",
        "0"
    ]));
    assert!(parse_fails(&["open", "x", "--log", "csv"]));
    assert!(
        parse_fails(&["open", "x", "--log-rotate", "16"]),
        "rotate needs --log"
    );
    assert!(parse_fails(&["open", "x", "--log", "--log-quota", "8193"]));
}

#[test]
fn send_and_run_take_per_request_delays() {
    let s = call(&["send", "dut", "ls", "--char-delay", "10"]);
    assert_eq!(s.params["char_delay_ms"], 10);
    assert!(s.params.get("line_delay_ms").is_none());
    let r = call(&["run", "dut", "dmesg", "--line-delay", "0"]);
    assert_eq!(r.params["line_delay_ms"], 0);
    assert!(parse_fails(&["send", "dut", "ls", "--char-delay", "2000"]));
}

#[test]
fn pacing_verb() {
    let p = call(&["pacing", "dut", "--char", "3", "--line", "50"]);
    assert_eq!(p.method, "uart.pacing");
    assert_eq!(
        p.params,
        json!({"channel": "dut", "char_delay_ms": 3, "line_delay_ms": 50})
    );
    assert_eq!(
        call(&["pacing", "dut"]).params,
        json!({"channel": "dut"}),
        "no flags = query"
    );
    assert!(parse_fails(&["pacing", "dut", "--line", "10001"]));
}

#[test]
fn signal_verb() {
    let b = call(&["signal", "dut", "break"]);
    assert_eq!(b.method, "uart.signal");
    assert_eq!(b.params, json!({"channel": "dut", "kind": "break"}));
    assert_eq!(
        call(&["signal", "dut", "break", "--ms", "500"]).params["duration_ms"],
        500
    );
    assert_eq!(
        call(&["signal", "dut", "dtr", "--off", "--ms", "100"]).params,
        json!({"channel": "dut", "kind": "dtr", "value": false, "duration_ms": 100})
    );
    assert_eq!(call(&["signal", "dut", "rts", "--on"]).params["value"], true);
    assert!(
        usage_fails(&["signal", "dut", "dtr"]),
        "dtr/rts need --on or --off"
    );
    assert!(
        usage_fails(&["signal", "dut", "break", "--off"]),
        "break is a pulse"
    );
    assert!(parse_fails(&["signal", "dut", "rts", "--on", "--off"]));
    assert!(parse_fails(&["signal", "dut", "reset"]));
    assert!(parse_fails(&["signal", "dut", "break", "--ms", "5001"]));
    assert!(parse_fails(&["signal", "dut", "break", "--ms", "0"]));
}

#[test]
fn log_verb() {
    let s = call(&[
        "log", "dut", "start", "--format", "jsonl", "--rotate", "8", "--quota", "100",
    ]);
    assert_eq!(s.method, "uart.log.start");
    assert_eq!(
        s.params,
        json!({"channel": "dut", "format": "jsonl", "rotate_mib": 8, "quota_mib": 100})
    );
    assert_eq!(call(&["log", "dut", "start"]).params, json!({"channel": "dut"}));
    let t = call(&["log", "dut", "stop"]);
    assert_eq!(t.method, "uart.log.stop");
    assert_eq!(t.params, json!({"channel": "dut"}));
    assert!(parse_fails(&["log", "dut", "pause"]));
    assert!(parse_fails(&["log", "dut", "start", "--rotate", "0"]));
}

#[test]
fn help_mentions_the_new_operator_features() {
    let h = help_json();
    let text = h.to_string();
    for needle in [
        "loom uart pacing",
        "loom uart signal",
        "loom uart log",
        "--reconnect",
        "--log",
    ] {
        assert!(text.contains(needle), "help lacks {needle}");
    }
    assert!(
        h["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap_or("").contains("stream seq")),
        "help explains that native log files carry no stream seq"
    );
}
