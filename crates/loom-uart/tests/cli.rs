//! CLI fragment: argument → RPC call mapping.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_cli_kit::DomainCli;
use loom_uart::cli::{UartCli, unescape};
use serde_json::json;

fn call(args: &[&str]) -> loom_cli_kit::RpcCall {
    let m = UartCli::command()
        .try_get_matches_from(std::iter::once("uart").chain(args.iter().copied()))
        .expect("parse");
    UartCli::to_call(&m).expect("to_call")
}

#[test]
fn send_text_defaults_to_cr_and_hex() {
    let c = call(&["send", "dut", "ls", "--expect", "# "]);
    assert_eq!(c.method, "uart.send");
    assert_eq!(c.params["data"], json!(hex::encode("ls")));
    assert_eq!(c.params["eol"], "cr");
    assert_eq!(c.params["expect"][0]["kind"], "literal");
    assert_eq!(c.params["timeout_ms"], 10_000);
}

#[test]
fn ctrl_and_hex_have_no_eol() {
    assert_eq!(call(&["send", "dut", "--ctrl", "c"]).params["data"], "03");
    let h = call(&["send", "dut", "--hex", "1b 5b 41"]);
    assert_eq!(h.params["data"], "1b5b41");
    assert_eq!(h.params["eol"], "none");
}

#[test]
fn open_read_expect_run() {
    let o = call(&["open", "sim://bootshell", "--as", "dut", "--baud", "115200"]);
    assert_eq!(o.params["config"]["baud"], 115200);
    assert_eq!(call(&["read", "dut"]).params["mode"], "summary");
    assert_eq!(call(&["read", "dut", "--tail", "20"]).params["mode"], "peek");
    let e = call(&["expect", "dut", "login:", "Kernel panic", "--timeout", "5"]);
    assert_eq!(e.params["patterns"].as_array().unwrap().len(), 2);
    assert_eq!(e.wait, Some(std::time::Duration::from_secs(5)));
    assert_eq!(call(&["run", "dut", "uname -a"]).method, "uart.run");
}

#[test]
fn escapes() {
    assert_eq!(unescape(r"a\r\n\x1b\\").unwrap(), b"a\r\n\x1b\\");
    assert!(unescape(r"\q").is_err());
}
