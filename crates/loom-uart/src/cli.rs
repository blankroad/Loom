//! CLI piece: `loom uart …` (plan §4.2–4.6). Linked into the single `loom` binary; it only
//! builds RPC calls and renders results — it never opens a device (invariant I1).

use std::io;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use loom_cli_kit::{DomainCli, RpcCall, UsageError, render_common};
use loom_proto::envelope::Envelope;
use serde_json::{Map, Value, json};

/// The `uart` CLI fragment.
#[derive(Debug, Default)]
pub struct UartCli;

/// Longest single-result wait (contract 03 §3.1).
const MAX_WAIT_S: u64 = 3600;
/// RPC delivery margin beyond the device observation deadline (including zero-time polls).
const REPLY_MARGIN_S: u64 = 2;

fn ch() -> Arg {
    Arg::new("channel")
        .required(true)
        .help("channel alias (e.g. dut)")
}

fn timeout(default: &'static str) -> Arg {
    Arg::new("timeout")
        .long("timeout")
        .value_parser(value_parser!(u64).range(0..=MAX_WAIT_S))
        .default_value(default)
        .help("seconds")
}

fn text_input_args(cmd: Command) -> Command {
    cmd.arg(Arg::new("text").help("text to send (Enter added per --eol)"))
        .arg(
            Arg::new("hex")
                .long("hex")
                .help("raw bytes as hex, e.g. \"1b 5b 41\"")
                .conflicts_with("text"),
        )
        .arg(
            Arg::new("ctrl")
                .long("ctrl")
                .value_parser(["c", "d", "z", "]", "\\"])
                .conflicts_with_all(["text", "hex"]),
        )
        .arg(
            Arg::new("secret_env")
                .long("secret-env")
                .value_name("VAR")
                .conflicts_with_all(["text", "hex", "ctrl"]),
        )
        .arg(
            Arg::new("eol")
                .long("eol")
                .value_parser(["cr", "lf", "crlf", "none"]),
        )
        .arg(
            Arg::new("escapes")
                .long("escapes")
                .action(ArgAction::SetTrue)
                .help("interpret \\r \\n \\t \\xNN \\\\"),
        )
}

impl DomainCli for UartCli {
    fn command() -> Command {
        Command::new("uart")
            .about("Serial consoles — the user sees the same bytes live in the Loom UI")
            .subcommand_required(true)
            .subcommand(
                Command::new("ports")
                    .about("List serial ports")
                    .arg(Arg::new("all").long("all").action(ArgAction::SetTrue)),
            )
            .subcommand(
                Command::new("open")
                    .about("Open a port or sim://<scenario> as a channel")
                    .arg(Arg::new("target").required(true))
                    .arg(Arg::new("alias").long("as"))
                    .arg(Arg::new("baud").long("baud").value_parser(value_parser!(u32)))
                    .arg(
                        Arg::new("data")
                            .long("data")
                            .value_parser(value_parser!(u8).range(5..=8)),
                    )
                    .arg(
                        Arg::new("parity")
                            .long("parity")
                            .value_parser(["N", "E", "O", "M", "S"]),
                    )
                    .arg(Arg::new("stop").long("stop").value_parser(["1", "1.5", "2"]))
                    .arg(
                        Arg::new("flow")
                            .long("flow")
                            .value_parser(["none", "rtscts", "xonxoff"]),
                    )
                    .arg(Arg::new("profile").long("profile"))
                    .arg(
                        Arg::new("env")
                            .long("env")
                            .value_parser(["sim", "bench", "vehicle"]),
                    )
                    .arg(
                        Arg::new("char_delay")
                            .long("char-delay")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=1000))
                            .help("pace input: wait MS after every byte"),
                    )
                    .arg(
                        Arg::new("line_delay")
                            .long("line-delay")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=10_000))
                            .help("pace input: wait MS after every line"),
                    )
                    .arg(
                        Arg::new("reconnect")
                            .long("reconnect")
                            .action(ArgAction::SetTrue)
                            .help("reopen the same device after an unplug (bounded window)"),
                    )
                    .arg(
                        Arg::new("reconnect_window")
                            .long("reconnect-window")
                            .value_name("S")
                            .value_parser(value_parser!(u64).range(1..=120))
                            .requires("reconnect"),
                    )
                    .arg(
                        Arg::new("log")
                            .long("log")
                            .value_name("FORMAT")
                            .num_args(0..=1)
                            .default_missing_value("log")
                            .value_parser(["log", "jsonl"])
                            .help("keep a native log file (default format: log)"),
                    )
                    .arg(
                        Arg::new("log_rotate")
                            .long("log-rotate")
                            .value_name("MIB")
                            .value_parser(value_parser!(u64).range(1..=1024))
                            .requires("log"),
                    )
                    .arg(
                        Arg::new("log_quota")
                            .long("log-quota")
                            .value_name("MIB")
                            .value_parser(value_parser!(u64).range(1..=8192))
                            .requires("log"),
                    ),
            )
            .subcommand(Command::new("close").about("Close a channel").arg(ch()))
            .subcommand(
                Command::new("status")
                    .about("Channel state")
                    .arg(Arg::new("channel"))
                    .arg(Arg::new("request").long("request")),
            )
            .subcommand(
                text_input_args(
                    Command::new("send")
                        .about("Send bytes (waits while the user is typing)")
                        .arg(ch()),
                )
                .arg(
                    Arg::new("expect")
                        .long("expect")
                        .action(ArgAction::Append)
                        .help("literal to wait for after sending"),
                )
                .arg(Arg::new("regex").long("regex").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("expect_prompt")
                        .long("expect-prompt")
                        .action(ArgAction::SetTrue),
                )
                .arg(timeout("10"))
                .arg(
                    Arg::new("defer_timeout")
                        .long("defer-timeout")
                        .value_parser(value_parser!(u64).range(0..=600))
                        .default_value("10"),
                )
                .arg(Arg::new("dry_run").long("dry-run").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("char_delay")
                        .long("char-delay")
                        .value_name("MS")
                        .value_parser(value_parser!(u64).range(0..=1000)),
                )
                .arg(
                    Arg::new("line_delay")
                        .long("line-delay")
                        .value_name("MS")
                        .value_parser(value_parser!(u64).range(0..=10_000)),
                ),
            )
            .subcommand(
                Command::new("read")
                    .about("Bounded read from your cursor")
                    .arg(ch())
                    .arg(
                        Arg::new("tail")
                            .long("tail")
                            .value_parser(value_parser!(u32).range(1..=1000)),
                    )
                    .arg(Arg::new("since").long("since"))
                    .arg(Arg::new("until").long("until"))
                    .arg(Arg::new("page").long("page").action(ArgAction::SetTrue))
                    .arg(
                        Arg::new("peek")
                            .long("peek")
                            .action(ArgAction::SetTrue)
                            .conflicts_with("page"),
                    )
                    .arg(Arg::new("cursor_token").long("cursor-token"))
                    .arg(
                        Arg::new("max_lines")
                            .long("max-lines")
                            .value_parser(value_parser!(u32).range(1..=1000)),
                    )
                    .arg(
                        Arg::new("max_bytes")
                            .long("max-bytes")
                            .value_parser(value_parser!(u32).range(1..=65536)),
                    )
                    .arg(
                        Arg::new("ansi")
                            .long("ansi")
                            .value_parser(["strip", "keep", "escape"]),
                    ),
            )
            .subcommand(
                Command::new("expect")
                    .about("Wait for text (exit 3 on timeout)")
                    .arg(ch())
                    .arg(Arg::new("patterns").num_args(0..=8).action(ArgAction::Append))
                    .arg(Arg::new("prompt").long("prompt").action(ArgAction::SetTrue))
                    .arg(Arg::new("regex").long("regex").action(ArgAction::SetTrue))
                    .arg(Arg::new("from").long("from"))
                    .arg(timeout("30")),
            )
            .subcommand(
                Command::new("run")
                    .about("Run one command line; output up to the next prompt")
                    .arg(ch())
                    .arg(Arg::new("command").required(true))
                    .arg(
                        Arg::new("prompt")
                            .long("prompt")
                            .help("prompt regex (default: channel profile)"),
                    )
                    .arg(timeout("30"))
                    .arg(
                        Arg::new("char_delay")
                            .long("char-delay")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=1000)),
                    )
                    .arg(
                        Arg::new("line_delay")
                            .long("line-delay")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=10_000)),
                    ),
            )
            .subcommand(
                Command::new("pacing")
                    .about("Show or change the input pacing of a channel")
                    .arg(ch())
                    .arg(
                        Arg::new("char")
                            .long("char")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=1000)),
                    )
                    .arg(
                        Arg::new("line")
                            .long("line")
                            .value_name("MS")
                            .value_parser(value_parser!(u64).range(0..=10_000)),
                    ),
            )
            .subcommand(
                Command::new("signal")
                    .about("Break pulse, or set / pulse DTR or RTS")
                    .arg(ch())
                    .arg(
                        Arg::new("kind")
                            .required(true)
                            .value_parser(["break", "dtr", "rts"]),
                    )
                    .arg(
                        Arg::new("on")
                            .long("on")
                            .action(ArgAction::SetTrue)
                            .conflicts_with("off"),
                    )
                    .arg(Arg::new("off").long("off").action(ArgAction::SetTrue))
                    .arg(
                        Arg::new("ms")
                            .long("ms")
                            .value_name("N")
                            .value_parser(value_parser!(u64).range(1..=5000))
                            .help("break length, or DTR/RTS pulse length"),
                    ),
            )
            .subcommand(
                Command::new("log")
                    .about("Start or stop the native log file of a channel")
                    .arg(ch())
                    .arg(Arg::new("action").required(true).value_parser(["start", "stop"]))
                    .arg(Arg::new("format").long("format").value_parser(["log", "jsonl"]))
                    .arg(
                        Arg::new("rotate")
                            .long("rotate")
                            .value_name("MIB")
                            .value_parser(value_parser!(u64).range(1..=1024)),
                    )
                    .arg(
                        Arg::new("quota")
                            .long("quota")
                            .value_name("MIB")
                            .value_parser(value_parser!(u64).range(1..=8192)),
                    ),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        let s = |a: &ArgMatches, k: &str| a.get_one::<String>(k).cloned();
        let (verb, a) = m
            .subcommand()
            .ok_or_else(|| UsageError::new("missing verb").with_hint("loom uart help --json"))?;
        let mut p = Map::new();
        let mut put = |k: &str, v: Value| {
            if !v.is_null() {
                p.insert(k.into(), v);
            }
        };
        let mut wait = None;
        let method = match verb {
            "ports" => {
                put("all", json!(a.get_flag("all")));
                "uart.ports"
            }
            "open" => {
                put("target", json!(s(a, "target")));
                put("alias", json!(s(a, "alias")));
                put("profile", json!(s(a, "profile")));
                put("env", json!(s(a, "env")));
                let mut cfg = Map::new();
                if let Some(b) = a.get_one::<u32>("baud") {
                    cfg.insert("baud".into(), json!(b));
                }
                if let Some(d) = a.get_one::<u8>("data") {
                    cfg.insert("data_bits".into(), json!(d));
                }
                for (k, key) in [("parity", "parity"), ("stop", "stop_bits"), ("flow", "flow")] {
                    if let Some(v) = s(a, k) {
                        cfg.insert(key.into(), json!(v));
                    }
                }
                if !cfg.is_empty() {
                    put("config", Value::Object(cfg));
                }
                let mut pacing = Map::new();
                if let Some(v) = a.get_one::<u64>("char_delay") {
                    pacing.insert("char_delay_ms".into(), json!(v));
                }
                if let Some(v) = a.get_one::<u64>("line_delay") {
                    pacing.insert("line_delay_ms".into(), json!(v));
                }
                if !pacing.is_empty() {
                    put("pacing", Value::Object(pacing));
                }
                if a.get_flag("reconnect") {
                    let mut rc = Map::new();
                    rc.insert("enabled".into(), json!(true));
                    if let Some(sec) = a.get_one::<u64>("reconnect_window") {
                        rc.insert("window_ms".into(), json!(sec * 1000));
                    }
                    put("reconnect", Value::Object(rc));
                }
                if let Some(format) = s(a, "log") {
                    let mut lg = Map::new();
                    lg.insert("format".into(), json!(format));
                    if let Some(v) = a.get_one::<u64>("log_rotate") {
                        lg.insert("rotate_mib".into(), json!(v));
                    }
                    if let Some(v) = a.get_one::<u64>("log_quota") {
                        lg.insert("quota_mib".into(), json!(v));
                    }
                    put("log", Value::Object(lg));
                }
                "uart.open"
            }
            "close" => {
                put("channel", json!(s(a, "channel")));
                "uart.close"
            }
            "status" => {
                put("channel", json!(s(a, "channel")));
                put("request_id", json!(s(a, "request")));
                "uart.status"
            }
            "send" => {
                put("channel", json!(s(a, "channel")));
                let (bytes, default_eol) = send_bytes(a)?;
                put("data", json!(hex::encode(&bytes)));
                put("eol", json!(s(a, "eol").unwrap_or_else(|| default_eol.into())));
                if a.get_one::<String>("secret_env").is_some() {
                    put("secret", json!(true));
                }
                let pats: Vec<String> = a
                    .get_many::<String>("expect")
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect();
                let regex = a.get_flag("regex");
                if !pats.is_empty() {
                    put("expect", patterns_json(&pats, regex));
                } else if a.get_flag("expect_prompt") {
                    put("expect", json!([{"kind": "prompt"}]));
                }
                let t = a.get_one::<u64>("timeout").copied().unwrap_or(10);
                let d = a.get_one::<u64>("defer_timeout").copied().unwrap_or(10);
                put("timeout_ms", json!(t * 1000));
                put("defer_timeout_ms", json!(d * 1000));
                put("dry_run", json!(a.get_flag("dry_run")));
                put("char_delay_ms", json!(a.get_one::<u64>("char_delay")));
                put("line_delay_ms", json!(a.get_one::<u64>("line_delay")));
                wait = Some(Duration::from_secs(t + d + REPLY_MARGIN_S));
                "uart.send"
            }
            "read" => {
                put("channel", json!(s(a, "channel")));
                let mode = if a.get_flag("page") {
                    "page"
                } else if a.get_flag("peek") || a.get_one::<u32>("tail").is_some() || s(a, "since").is_some()
                {
                    "peek"
                } else {
                    "summary"
                };
                put("mode", json!(mode));
                put("tail", json!(a.get_one::<u32>("tail")));
                put("from", json!(s(a, "since")));
                put("until", json!(s(a, "until")));
                put("cursor_token", json!(s(a, "cursor_token")));
                put("max_lines", json!(a.get_one::<u32>("max_lines")));
                put("max_bytes", json!(a.get_one::<u32>("max_bytes")));
                put("ansi", json!(s(a, "ansi")));
                "uart.read"
            }
            "expect" => {
                put("channel", json!(s(a, "channel")));
                let pats: Vec<String> = a
                    .get_many::<String>("patterns")
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect();
                if a.get_flag("prompt") {
                    put("patterns", json!([{"kind": "prompt"}]));
                } else if pats.is_empty() {
                    return Err(UsageError::new("give at least one pattern or --prompt"));
                } else {
                    put("patterns", patterns_json(&pats, a.get_flag("regex")));
                }
                put("from", json!(s(a, "from")));
                let t = a.get_one::<u64>("timeout").copied().unwrap_or(30);
                put("timeout_ms", json!(t * 1000));
                wait = Some(Duration::from_secs(t + REPLY_MARGIN_S));
                "uart.expect"
            }
            "run" => {
                put("channel", json!(s(a, "channel")));
                let cmd = s(a, "command").unwrap_or_default();
                put("data", json!(hex::encode(cmd.as_bytes())));
                put("eol", json!("cr"));
                put("prompt", json!(s(a, "prompt")));
                put("char_delay_ms", json!(a.get_one::<u64>("char_delay")));
                put("line_delay_ms", json!(a.get_one::<u64>("line_delay")));
                let t = a.get_one::<u64>("timeout").copied().unwrap_or(30);
                put("timeout_ms", json!(t * 1000));
                wait = Some(Duration::from_secs(t + REPLY_MARGIN_S));
                "uart.run"
            }
            "pacing" => {
                put("channel", json!(s(a, "channel")));
                put("char_delay_ms", json!(a.get_one::<u64>("char")));
                put("line_delay_ms", json!(a.get_one::<u64>("line")));
                "uart.pacing"
            }
            "signal" => {
                put("channel", json!(s(a, "channel")));
                let kind = s(a, "kind").unwrap_or_default();
                let (on, off) = (a.get_flag("on"), a.get_flag("off"));
                if kind == "break" && off {
                    return Err(UsageError::new("break is a pulse"));
                }
                if kind != "break" && !on && !off {
                    return Err(UsageError::new("dtr/rts need --on or --off"));
                }
                put("kind", json!(kind));
                if on || off {
                    put("value", json!(on));
                }
                put("duration_ms", json!(a.get_one::<u64>("ms")));
                wait = Some(Duration::from_secs(10));
                "uart.signal"
            }
            "log" => {
                put("channel", json!(s(a, "channel")));
                if s(a, "action").as_deref() == Some("stop") {
                    "uart.log.stop"
                } else {
                    put("format", json!(s(a, "format")));
                    put("rotate_mib", json!(a.get_one::<u64>("rotate")));
                    put("quota_mib", json!(a.get_one::<u64>("quota")));
                    "uart.log.start"
                }
            }
            other => {
                return Err(
                    UsageError::new(format!("unknown verb {other}")).with_hint("loom uart help --json")
                );
            }
        };
        let mut call = RpcCall::new(method, Value::Object(p));
        call.wait = wait;
        Ok(call)
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            match call.method.as_str() {
                "uart.send" | "uart.run" => {
                    writeln!(
                        out,
                        "sent {} ({} B) state={}{}",
                        d["sent_escaped"]
                            .as_str()
                            .map(|s| format!("\"{s}\""))
                            .unwrap_or_default(),
                        d["written_bytes"],
                        d["state"].as_str().unwrap_or("?"),
                        d["deferred_ms"]
                            .as_u64()
                            .filter(|ms| *ms > 0)
                            .map(|ms| format!(" (waited {ms} ms for user)"))
                            .unwrap_or_default()
                    )?;
                    if let Some(text) = d["output"].as_str().or(d["expect_result"]["before"].as_str()) {
                        writeln!(out, "{text}")?;
                    }
                }
                "uart.read" | "uart.expect" => {
                    for l in d["lines"].as_array().into_iter().flatten() {
                        writeln!(out, "{}", l["text"].as_str().or(l.as_str()).unwrap_or_default())?;
                    }
                    if let Some(m) = d["match"].as_str() {
                        writeln!(out, "matched {m:?} (pattern {})", d["matched_index"])?;
                    }
                }
                _ => writeln!(out, "{}", serde_json::to_string_pretty(d).unwrap_or_default())?,
            }
        }
        render_common(env, out)
    }
}

fn patterns_json(pats: &[String], regex: bool) -> Value {
    Value::Array(
        pats.iter()
            .map(|p| json!({"kind": if regex { "regex" } else { "literal" }, "pattern": p}))
            .collect(),
    )
}

/// Bytes to send and the default line ending (`cr` for text, `none` for hex/ctrl/secret).
fn send_bytes(a: &ArgMatches) -> Result<(Vec<u8>, &'static str), UsageError> {
    if let Some(h) = a.get_one::<String>("hex") {
        let clean: String = h.chars().filter(|c| !c.is_whitespace()).collect();
        let b = hex::decode(clean).map_err(|e| UsageError::new(format!("bad --hex: {e}")))?;
        return Ok((b, "none"));
    }
    if let Some(c) = a.get_one::<String>("ctrl") {
        let b = match c.as_str() {
            "c" => 0x03,
            "d" => 0x04,
            "z" => 0x1a,
            "]" => 0x1d,
            _ => 0x1c,
        };
        return Ok((vec![b], "none"));
    }
    if let Some(var) = a.get_one::<String>("secret_env") {
        let v = std::env::var(var).map_err(|_| {
            UsageError::new(format!("E_SECRET_MISSING: environment variable {var} is not set"))
        })?;
        return Ok((v.into_bytes(), "cr"));
    }
    let text = a
        .get_one::<String>("text")
        .ok_or_else(|| UsageError::new("give TEXT, --hex, --ctrl or --secret-env"))?;
    if a.get_flag("escapes") {
        return Ok((unescape(text)?, "cr"));
    }
    Ok((text.clone().into_bytes(), "cr"))
}

/// `\r \n \t \xNN \\` (plan §4.3 `--escapes`).
pub fn unescape(s: &str) -> Result<Vec<u8>, UsageError> {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let next = *b
            .get(i + 1)
            .ok_or_else(|| UsageError::new("trailing backslash"))?;
        match next {
            b'r' => out.push(b'\r'),
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'\\' => out.push(b'\\'),
            b'x' => {
                let h = s
                    .get(i + 2..i + 4)
                    .ok_or_else(|| UsageError::new("\\x needs two hex digits"))?;
                out.push(u8::from_str_radix(h, 16).map_err(|_| UsageError::new("bad \\xNN"))?);
                i += 2;
            }
            other => return Err(UsageError::new(format!("unknown escape \\{}", other as char))),
        }
        i += 2;
    }
    Ok(out)
}
