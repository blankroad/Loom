//! `loom logic` CLI fragment. Pure argument conversion; no engine/device access.

use std::{io, time::Duration};

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use loom_cli_kit::{DomainCli, RpcCall, UsageError, cursor_arg, render_common};
use loom_proto::{cursor::Cursor, envelope::Envelope};
use serde_json::{Value, json};

use crate::model::StartConfig;

/// Logic-analyzer CLI fragment, registered through `CliOf<LogicCli>`.
#[derive(Debug, Default)]
pub struct LogicCli;

// Integer quantities only: no float rounding, saturating overflow or shell parsing.
fn scaled(s: &str, units: &[(&str, u64)]) -> Result<u64, String> {
    let at = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, unit) = s.split_at(at);
    let n = digits
        .parse::<u64>()
        .map_err(|_| format!("invalid quantity {s:?}"))?;
    let factor = units
        .iter()
        .find(|(u, _)| *u == unit)
        .map(|(_, n)| *n)
        .ok_or_else(|| format!("invalid unit in {s:?}"))?;
    n.checked_mul(factor)
        .ok_or_else(|| format!("quantity {s:?} overflows"))
}

/// Parse `1000000`, `250k`, `1M` (Hz).
pub fn parse_rate(s: &str) -> Result<u64, String> {
    scaled(s, &[("", 1), ("k", 1_000), ("K", 1_000), ("M", 1_000_000)])
}

/// Parse integer milliseconds/seconds/minutes/hours; bare numbers are seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    scaled(
        s,
        &[
            ("ms", 1),
            ("", 1_000),
            ("s", 1_000),
            ("m", 60_000),
            ("h", 3_600_000),
        ],
    )
    .map(Duration::from_millis)
}

fn dur(id: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_name("DURATION")
        .value_parser(parse_duration)
}

fn capture() -> Arg {
    Arg::new("capture").long("capture").value_name("ID")
}

fn bounded_duration(
    s: &ArgMatches,
    id: &str,
    default: Duration,
    max: Duration,
) -> Result<Duration, UsageError> {
    let d = s.get_one::<Duration>(id).copied().unwrap_or(default);
    if d.is_zero() || d > max {
        return Err(UsageError::new(format!(
            "--{id} must be 1ms..={}s",
            max.as_secs()
        )));
    }
    Ok(d)
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn put_string(p: &mut Value, s: &ArgMatches, arg: &str, wire: &str) {
    if let Some(v) = s.get_one::<String>(arg) {
        p[wire] = json!(v);
    }
}

fn put_cursor(p: &mut Value, s: &ArgMatches) {
    if let Some(c) = s.get_one::<Cursor>("since") {
        p["since"] = json!(c.to_string());
    }
}

impl DomainCli for LogicCli {
    fn command() -> Command {
        Command::new("logic")
            .about(
                "Logic-analyzer raw waveform and real sigrok decoder logging (physical hardware unverified)",
            )
            .subcommand_required(true)
            .subcommand(
                Command::new("status")
                    .about("Engine availability, implemented sources and current capture; no USB scan"),
            )
            .subcommand(
                Command::new("start")
                    .about("Start one bounded capture; source is explicit, with no fallback")
                    .arg(Arg::new("source").long("source").required(true).value_parser([
                        "synthetic",
                        "sigrok-demo",
                        "sigrok-device",
                    ]))
                    .arg(
                        Arg::new("driver")
                            .long("driver")
                            .help("sigrok-device: fx2lafw only (8-bit candidate, unverified)"),
                    )
                    .arg(
                        Arg::new("conn")
                            .long("conn")
                            .help("sigrok-device USB bus.address; never scans automatically for status"),
                    )
                    .arg(
                        Arg::new("rate")
                            .long("rate")
                            .value_parser(parse_rate)
                            .help("Hz, 250k or 1M; pattern demo uses 200k"),
                    )
                    .arg(
                        Arg::new("channels")
                            .long("channels")
                            .value_delimiter(',')
                            .help("Display bits D0..D7; acquisition always uses all eight"),
                    )
                    .arg(
                        Arg::new("decode")
                            .long("decode")
                            .action(ArgAction::Append)
                            .help("One uart:rx=D0:baudrate=115200 or spi:clk=D0:mosi=D1:cs=D2"),
                    )
                    .arg(dur("duration").help("100ms..1h; default 60s"))
                    .arg(
                        Arg::new("max-samples")
                            .long("max-samples")
                            .value_parser(value_parser!(u64)),
                    )
                    .arg(
                        Arg::new("pattern")
                            .long("pattern")
                            .help("sigrok-demo only (e.g. graycode), fixed 200kHz"),
                    )
                    .arg(
                        Arg::new("text")
                            .long("text")
                            .help("Synthetic UTF-8 bytes, ≤256 B; {n} expands to a frame counter"),
                    )
                    .arg(dur("frame-period").help("Synthetic frame period, 1ms..10s"))
                    .arg(
                        Arg::new("clock-hz")
                            .long("clock-hz")
                            .value_parser(value_parser!(u64).range(1..=5_000_000))
                            .help("Synthetic SPI clock in Hz (also ≤ sample rate / 2)"),
                    ),
            )
            .subcommand(
                Command::new("stop")
                    .about("Stop capture and reap its engine processes")
                    .arg(Arg::new("id").value_name("ID")),
            )
            .subcommand(
                Command::new("read")
                    .about("Read flattened journal rows, ≤1000 rows / 64KiB; raw records: loom events")
                    .arg(capture())
                    .arg(
                        Arg::new("kind")
                            .long("kind")
                            .value_parser(["annotation", "edges", "state", "all"])
                            .default_value("annotation"),
                    )
                    .arg(
                        Arg::new("tail")
                            .long("tail")
                            .value_parser(value_parser!(u32).range(1..=1000))
                            .default_value("100"),
                    )
                    .arg(cursor_arg("since"))
                    .arg(Arg::new("follow").long("follow").action(ArgAction::SetTrue))
                    .arg(dur("wait").requires("follow").help(
                        "One follow long-poll wait, ≤30s (default 10s); resume with result next cursor",
                    )),
            )
            .subcommand(
                Command::new("wait")
                    .about("Wait for decoded text/ASCII evidence in the same journal (≤600s)")
                    .arg(capture())
                    .arg(cursor_arg("since"))
                    .arg(
                        Arg::new("text")
                            .long("text")
                            .help("Annotation text substring, e.g. 4C"),
                    )
                    .arg(
                        Arg::new("ascii")
                            .long("ascii")
                            .help("ASCII sequence reconstructed from decoded byte annotations"),
                    )
                    .group(
                        ArgGroup::new("match")
                            .args(["text", "ascii"])
                            .required(true)
                            .multiple(false),
                    )
                    .arg(
                        Arg::new("decoder")
                            .long("decoder")
                            .help("Decoder selector (e.g. uart or uart-1)"),
                    )
                    .arg(
                        dur("timeout")
                            .default_value("5s")
                            .help("Required result deadline, 1ms..600s"),
                    ),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        match m.subcommand() {
            Some(("status", _)) => Ok(RpcCall::new("logic.status", json!({}))),
            Some(("start", s)) => {
                let mut p = json!({});
                for (arg, wire) in [
                    ("source", "source"),
                    ("driver", "driver"),
                    ("conn", "conn"),
                    ("pattern", "pattern"),
                    ("text", "text"),
                ] {
                    put_string(&mut p, s, arg, wire);
                }
                for (arg, wire) in [
                    ("rate", "samplerate"),
                    ("max-samples", "max_samples"),
                    ("clock-hz", "clock_hz"),
                ] {
                    if let Some(n) = s.get_one::<u64>(arg) {
                        p[wire] = json!(n);
                    }
                }
                for (arg, wire) in [("duration", "duration_ms"), ("frame-period", "frame_period_ms")] {
                    if let Some(d) = s.get_one::<Duration>(arg) {
                        p[wire] = json!(ms(*d));
                    }
                }
                if let Some(cs) = s.get_many::<String>("channels") {
                    p["channels"] = json!(cs.collect::<Vec<_>>());
                }
                if let Some(ds) = s.get_many::<String>("decode") {
                    p["decoders"] = json!(ds.collect::<Vec<_>>());
                }
                // Share the exact backend wire validation and canonicalization.
                let cfg = StartConfig::from_params(&p).map_err(|e| UsageError::new(e.to_string()))?;
                Ok(RpcCall::new("logic.start", cfg.to_params()))
            }
            Some(("stop", s)) => {
                let mut p = json!({});
                put_string(&mut p, s, "id", "capture_id");
                Ok(RpcCall::new("logic.stop", p))
            }
            Some(("read", s)) => {
                let follow = s.get_flag("follow");
                let d = bounded_duration(s, "wait", Duration::from_secs(10), Duration::from_secs(30))?;
                let mut p = json!({"kind":s.get_one::<String>("kind"), "tail":s.get_one::<u32>("tail"), "follow":follow,
                                   "wait_ms":if follow {ms(d)} else {0}});
                put_string(&mut p, s, "capture", "capture_id");
                put_cursor(&mut p, s);
                let mut c = RpcCall::new("logic.read", p);
                if follow {
                    c.wait = Some(d + Duration::from_secs(2));
                }
                Ok(c)
            }
            Some(("wait", s)) => {
                let d = bounded_duration(s, "timeout", Duration::from_secs(5), Duration::from_secs(600))?;
                let mut p = json!({"timeout_ms":ms(d)});
                for (arg, wire) in [
                    ("capture", "capture_id"),
                    ("decoder", "decoder"),
                    ("text", "text"),
                    ("ascii", "ascii"),
                ] {
                    put_string(&mut p, s, arg, wire);
                }
                for k in ["text", "ascii"] {
                    if let Some(t) = p[k].as_str()
                        && (t.is_empty() || t.len() > 256 || (k == "ascii" && !t.is_ascii()))
                    {
                        return Err(UsageError::new(format!(
                            "--{k} must be 1..256 {}bytes",
                            if k == "ascii" { "ASCII " } else { "" }
                        )));
                    }
                }
                put_cursor(&mut p, s);
                let mut c = RpcCall::new("logic.wait", p);
                c.wait = Some(d + Duration::from_secs(2));
                Ok(c)
            }
            _ => Err(UsageError::new("expected status, start, stop, read or wait")
                .with_hint("loom logic help --json")),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            if call.method == "logic.read" {
                for row in d["rows"].as_array().into_iter().flatten() {
                    writeln!(out, "{row}")?;
                }
                for field in ["complete", "rows_left_out"] {
                    if let Some(value) = d.get(field) {
                        writeln!(out, "{field}: {value}")?;
                    }
                }
                if let Some(next) = d.get("next").filter(|v| !v.is_null()) {
                    writeln!(out, "next: {next}")?;
                }
            } else {
                writeln!(
                    out,
                    "{}",
                    serde_json::to_string_pretty(d).map_err(io::Error::other)?
                )?;
            }
        }
        render_common(env, out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn flattened_read_rows_and_completeness_are_visible() {
        use loom_proto::envelope::{Meta, OthersSinceLast};
        let c = RpcCall::new("logic.read", json!({}));
        let env = Envelope::success(
            "logic.read",
            json!({
                "rows": [{"type":"annotation", "seq":"42", "capture_id":"lc-one", "decoder":"uart-1", "class":null, "text":"4C"}],
                "complete":false, "rows_left_out":7, "next":"e3:seq:43"
            }),
            Meta::default(),
            OthersSinceLast::default(),
        );
        let mut out = Vec::new();
        LogicCli::render_human(&c, &env, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"text\":\"4C\"") && text.contains("\"decoder\":\"uart-1\""));
        assert!(text.contains("\"class\":null"));
        assert!(text.contains("complete: false") && text.contains("rows_left_out: 7"));
        assert!(text.contains("e3:seq:43"));
    }
    fn call(args: &[&str]) -> Result<RpcCall, String> {
        let m = LogicCli::command()
            .try_get_matches_from(std::iter::once("logic").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
        LogicCli::to_call(&m).map_err(|e| e.to_string())
    }
    #[test]
    fn explicit_source_and_model_validation() {
        assert!(call(&["start"]).is_err());
        assert!(call(&["start", "--source", "sigrok-device", "--driver", "saleae-logic16"]).is_err());
        assert!(call(&["start", "--source", "sigrok-device", "--driver", "fx2lafw:conn=x"]).is_err());
        assert!(call(&["start", "--source", "synthetic"]).is_err());
        assert!(call(&["start", "--source", "synthetic", "--decode", "uart:rx=D8"]).is_err());
    }
    #[test]
    fn synthetic_wire_has_no_legacy_keys() {
        let c = call(&[
            "start",
            "--source",
            "synthetic",
            "--rate",
            "1M",
            "--decode",
            "uart:rx=D5:baudrate=100000",
            "--channels",
            "D5,D7",
            "--duration",
            "2s",
            "--text",
            "LOOM",
        ])
        .unwrap();
        assert_eq!(c.method, "logic.start");
        assert_eq!(c.params["samplerate"], 1_000_000);
        assert_eq!(c.params["duration_ms"], 2000);
        assert_eq!(c.params["channels"], json!(["D5", "D7"]));
        assert!(c.params.get("protocol").is_none() && c.params.get("baudrate").is_none());
        assert!(StartConfig::from_params(&c.params).is_ok());
        let demo = call(&["start", "--source", "sigrok-demo", "--pattern", "graycode"]).unwrap();
        assert_eq!(demo.params["samplerate"], 200_000);
    }
    #[test]
    fn spi_and_capture_limits_use_backend_wire() {
        let c = call(&[
            "start",
            "--source",
            "synthetic",
            "--decode",
            "spi:clk=D0:mosi=D1:cs=D2:cpol=1:cpha=1",
            "--frame-period",
            "100ms",
            "--clock-hz",
            "100000",
            "--max-samples",
            "4096",
        ])
        .unwrap();
        assert_eq!(c.params["frame_period_ms"], 100);
        assert_eq!(c.params["max_samples"], 4096);
        assert!(StartConfig::from_params(&c.params).is_ok());
        assert!(call(&["start", "--source", "sigrok-demo", "--duration", "99ms"]).is_err());
        assert!(call(&["start", "--source", "sigrok-demo", "--rate", "24M"]).is_err());
        assert!(call(&["start", "--source", "sigrok-demo", "--max-samples", "0"]).is_err());
        assert!(
            call(&[
                "start",
                "--source",
                "synthetic",
                "--decode",
                "uart:rx=D0",
                "--decode",
                "spi:clk=D1:mosi=D2"
            ])
            .is_err()
        );
        assert_eq!(
            call(&["stop", "lc-one"]).unwrap().params,
            json!({"capture_id":"lc-one"})
        );
        assert_eq!(call(&["status"]).unwrap().method, "logic.status");
    }
    #[test]
    fn read_bounds_and_cursor_roundtrip() {
        let c = call(&[
            "read",
            "--since",
            "e7:seq:10",
            "--tail",
            "1000",
            "--follow",
            "--wait",
            "30s",
        ])
        .unwrap();
        assert_eq!(c.params["since"], "e7:seq:10");
        assert_eq!(c.params["wait_ms"], 30_000);
        assert_eq!(c.wait, Some(Duration::from_secs(32)));
        assert!(call(&["read", "--tail", "1001"]).is_err());
        assert!(call(&["read", "--follow", "--wait", "31s"]).is_err());
        assert!(call(&["read", "--wait", "1s"]).is_err());
        assert!(call(&["read", "--since", "bad-cursor"]).is_err());
    }
    #[test]
    fn wait_is_finite_and_match_is_exclusive() {
        assert!(call(&["wait"]).is_err());
        assert!(call(&["wait", "--text", "4C", "--ascii", "L"]).is_err());
        assert!(call(&["wait", "--text", "4C", "--timeout", "601s"]).is_err());
        assert!(call(&["wait", "--ascii", "한"]).is_err());
        let c = call(&[
            "wait",
            "--ascii",
            "LOOM",
            "--decoder",
            "uart-1",
            "--timeout",
            "600s",
        ])
        .unwrap();
        assert_eq!(c.params["timeout_ms"], 600_000);
        assert_eq!(c.params["decoder"], "uart-1");
        assert_eq!(c.wait, Some(Duration::from_secs(602)));
        assert!(parse_duration("18446744073709551615h").is_err());
        assert!(parse_rate("999999999999999999M").is_err());
    }
}
