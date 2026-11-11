//! CLI piece: `loom eth …` (tshark-style). Converts arguments to RPC calls and renders results.
//! Links only the decoder (for `show --hex` text helpers), never capture/device crates (I1).

use std::io;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use loom_cli_kit::{
    DomainCli, MAX_BYTES, MAX_FOLLOW, MAX_ROWS, MAX_WAIT, RpcCall, UsageError, cursor_arg, render_common,
};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::Envelope;
use loom_proto::event::payload_bytes;
use serde_json::{Value, json};

use crate::DOMAIN;
use crate::model::QUERY_DEFAULT_ROWS;

/// Default `wait-for --timeout` when none is given.
pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Bytes shown by `show --hex` (the rest is summarized).
pub const HEX_DUMP_MAX: usize = 16 * 1024;
/// Tree depth printed by `show`.
pub const TREE_MAX_DEPTH: usize = 16;
/// Width of the info column.
const INFO_WIDTH: usize = 120;

/// The `eth` CLI fragment.
#[derive(Debug, Default)]
pub struct EthCli;

/// `5s`, `250ms`, `2m`, `1h`, or bare seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (n, unit) = s.split_at(split);
    let n: u64 = n
        .parse()
        .map_err(|_| format!("bad duration {s:?} (use 500ms, 5s, 2m, 1h)"))?;
    let d = match unit {
        "ms" => Duration::from_millis(n),
        "" | "s" => Duration::from_secs(n),
        "m" => Duration::from_secs(n.saturating_mul(60)),
        "h" => Duration::from_secs(n.saturating_mul(3600)),
        _ => return Err(format!("bad duration unit in {s:?} (ms, s, m, h)")),
    };
    Ok(d)
}

/// `N` or `N..M` or `N..` → (min, max).
pub fn parse_count(s: &str) -> Result<(u64, Option<u64>), String> {
    let bad = || format!("bad count {s:?} (use N, N..M or N..)");
    match s.split_once("..") {
        None => {
            // A bare `N` means "at least N" (wait until N packets).
            Ok((s.parse().map_err(|_| bad())?, None))
        }
        Some((a, b)) => {
            let a = if a.is_empty() {
                0
            } else {
                a.parse().map_err(|_| bad())?
            };
            let b = if b.is_empty() {
                None
            } else {
                Some(b.parse().map_err(|_| bad())?)
            };
            if b.is_some_and(|b| b < a) {
                return Err(bad());
            }
            Ok((a, b))
        }
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn ch_arg() -> Arg {
    Arg::new("ch")
        .long("ch")
        .value_name("CH")
        .help("capture channel (default: the only open one)")
}

fn filter_arg() -> Arg {
    Arg::new("filter")
        .short('Y')
        .long("filter")
        .value_name("DISPLAY_FILTER")
        .allow_hyphen_values(true)
        .help("Wireshark display-filter subset (`loom eth filter check` validates)")
}

fn dur_arg(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .value_name("DURATION")
        .value_parser(parse_duration)
        .help(help)
}

fn range(m: &ArgMatches) -> Value {
    let c = |k: &str| m.try_get_one::<Cursor>(k).ok().flatten().map(ToString::to_string);
    json!({"from": c("since"), "to": c("until")})
}

fn opt_str(m: &ArgMatches, k: &str) -> Option<String> {
    m.try_get_one::<String>(k).ok().flatten().cloned()
}

fn need(m: &ArgMatches, k: &str) -> Result<String, UsageError> {
    opt_str(m, k).ok_or_else(|| UsageError::new(format!("missing {k}")))
}

impl DomainCli for EthCli {
    fn command() -> Command {
        let rows = value_parser!(u32).range(1..=i64::try_from(MAX_ROWS).unwrap_or(1000));
        Command::new(DOMAIN)
            .about("Ethernet capture/decode (tshark-style): DoIP, UDS over DoIP, SOME/IP")
            .subcommand_required(true)
            .subcommand(Command::new("ifaces").about("List capture interfaces"))
            .subcommand(
                Command::new("capture")
                    .about("Start/stop/inspect capture")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("start")
                            .about("Start capture on an interface (or replay/sim)")
                            .arg(Arg::new("iface").required(true))
                            .arg(ch_arg())
                            .arg(
                                Arg::new("backend")
                                    .long("backend")
                                    .value_parser(["live", "replay", "sim"]),
                            )
                            .arg(
                                Arg::new("replay")
                                    .long("replay")
                                    .value_name("FILE")
                                    .help("pcap/pcapng to replay"),
                            )
                            .arg(
                                Arg::new("speed")
                                    .long("speed")
                                    .value_name("max|N")
                                    .requires("replay"),
                            )
                            .arg(
                                Arg::new("repeat")
                                    .long("repeat")
                                    .action(ArgAction::SetTrue)
                                    .requires("replay"),
                            )
                            .arg(
                                Arg::new("bpf")
                                    .long("bpf")
                                    .value_name("CAPTURE_FILTER")
                                    .help("libpcap capture filter (not -Y)"),
                            )
                            .arg(
                                Arg::new("snaplen")
                                    .long("snaplen")
                                    .value_parser(value_parser!(u32).range(64..=262_144)),
                            )
                            .arg(Arg::new("promisc").long("promisc").action(ArgAction::SetTrue))
                            .arg(
                                Arg::new("buffer")
                                    .long("buffer")
                                    .value_name("MiB")
                                    .value_parser(value_parser!(u32).range(1..=1024)),
                            ),
                    )
                    .subcommand(Command::new("stop").about("Stop capture").arg(ch_arg()))
                    .subcommand(Command::new("status").about("Capture state").arg(ch_arg())),
            )
            .subcommand(
                Command::new("trace")
                    .about("Packet list: the latest --max rows in [--since, --until]")
                    .arg(Arg::new("chan").value_name("CH"))
                    .arg(ch_arg())
                    .arg(filter_arg())
                    .arg(cursor_arg("since"))
                    .arg(cursor_arg("until"))
                    .arg(
                        Arg::new("max")
                            .long("max")
                            .value_parser(rows)
                            .default_value("100"),
                    )
                    .arg(
                        Arg::new("detail")
                            .long("detail")
                            .action(ArgAction::SetTrue)
                            .help("include compact layers"),
                    )
                    .arg(
                        Arg::new("follow")
                            .long("follow")
                            .action(ArgAction::SetTrue)
                            .help("wait for new packets (≤ 600 s)"),
                    )
                    .arg(dur_arg("timeout", "follow/wait limit")),
            )
            .subcommand(
                Command::new("last")
                    .about("Latest packets")
                    .arg(ch_arg())
                    .arg(filter_arg())
                    .arg(Arg::new("n").short('n').value_parser(rows).default_value("20")),
            )
            .subcommand(
                Command::new("show")
                    .about("Layer tree (+ hex) of one packet")
                    .arg(Arg::new("ref").required(true).value_name("REF"))
                    .arg(Arg::new("hex").long("hex").action(ArgAction::SetTrue)),
            )
            .subcommand(
                Command::new("wait-for")
                    .about("Wait for matching packets (exit 3 timeout/count, 8 losses)")
                    .arg(ch_arg())
                    .arg(filter_arg().required(true))
                    .arg(
                        Arg::new("count")
                            .long("count")
                            .value_name("N[..M]")
                            .value_parser(parse_count),
                    )
                    .arg(dur_arg("within", "window length from the anchor (or from now)"))
                    .arg(Arg::new("anchor").long("anchor").value_name("REF"))
                    .arg(dur_arg("timeout", "give up after (≤ 3600 s, default 30 s)"))
                    .arg(
                        Arg::new("include-own")
                            .long("include-own")
                            .action(ArgAction::SetTrue)
                            .help("count Loom's own sends too"),
                    ),
            )
            .subcommand(
                Command::new("stats")
                    .about("Statistics over a range")
                    .arg(ch_arg())
                    .arg(
                        Arg::new("by")
                            .long("by")
                            .value_parser(["proto", "conv", "endpoint", "io"])
                            .default_value("proto"),
                    )
                    .arg(filter_arg())
                    .arg(cursor_arg("since"))
                    .arg(cursor_arg("until")),
            )
            .subcommand(
                Command::new("follow")
                    .about("Payload of one TCP stream / UDP conversation")
                    .arg(Arg::new("proto").required(true).value_parser(["tcp", "udp"]))
                    .arg(Arg::new("stream").required(true).value_parser(value_parser!(u32)))
                    .arg(ch_arg())
                    .arg(cursor_arg("since"))
                    .arg(cursor_arg("until"))
                    .arg(Arg::new("max-bytes").long("max-bytes").value_parser(
                        value_parser!(u32).range(1..=i64::try_from(MAX_BYTES).unwrap_or(65536)),
                    )),
            )
            .subcommand(
                Command::new("send")
                    .about("Send a raw frame or a UDP/TCP payload (--dry-run previews only)")
                    .arg(
                        Arg::new("kind")
                            .required(true)
                            .value_parser(["raw", "udp", "tcp"]),
                    )
                    .arg(Arg::new("hex").required(true).value_name("HEX"))
                    .arg(Arg::new("to").long("to").value_name("IP:PORT"))
                    .arg(
                        Arg::new("from-port")
                            .long("from-port")
                            .value_parser(value_parser!(u16))
                            .help("udp only: source port"),
                    )
                    .arg(
                        Arg::new("keep-open")
                            .long("keep-open")
                            .action(ArgAction::SetTrue)
                            .help("tcp only: keep Loom's own connection open for the next send (≤ 4, idle 30 s)"),
                    )
                    .arg(ch_arg())
                    .arg(Arg::new("dry-run").long("dry-run").action(ArgAction::SetTrue))
                    .arg(Arg::new("idempotency_key").long("idempotency-key")),
            )
            .subcommand(
                Command::new("export")
                    .about("Export packets to a file")
                    .arg(Arg::new("format").required(true).value_parser(["pcapng"]))
                    .arg(
                        Arg::new("out")
                            .short('o')
                            .long("out")
                            .required(true)
                            .value_name("FILE"),
                    )
                    .arg(ch_arg())
                    .arg(filter_arg())
                    .arg(cursor_arg("since"))
                    .arg(cursor_arg("until")),
            )
            .subcommand(
                Command::new("filter")
                    .about("Display filter tools")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("check")
                            .about("Validate a display filter")
                            .arg(Arg::new("expr").required(true).allow_hyphen_values(true)),
                    ),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        match m.subcommand() {
            Some(("ifaces", _)) => Ok(RpcCall::new("eth.ifaces", json!({}))),
            Some(("capture", c)) => capture_call(c),
            Some(("trace", s)) => {
                let ch = opt_str(s, "chan").or_else(|| opt_str(s, "ch"));
                let follow = s.get_flag("follow");
                let timeout = s.try_get_one::<Duration>("timeout").ok().flatten().copied();
                if timeout.is_some() && !follow {
                    return Err(UsageError::new("--timeout needs --follow"));
                }
                let wait = follow.then(|| timeout.unwrap_or(MAX_FOLLOW).min(MAX_FOLLOW));
                let mut call = RpcCall::new(
                    "eth.query",
                    json!({"ch": ch, "range": range(s), "filter": opt_str(s, "filter"),
                           "max": s.get_one::<u32>("max").copied().unwrap_or(QUERY_DEFAULT_ROWS),
                           "detail": s.get_flag("detail"), "wait_ms": wait.map(ms)}),
                );
                call.wait = wait.map(|d| d + Duration::from_secs(2));
                Ok(call)
            }
            Some(("last", s)) => Ok(RpcCall::new(
                "eth.query",
                json!({"ch": opt_str(s, "ch"), "range": {"from": null, "to": null}, "filter": opt_str(s, "filter"),
                       "max": s.get_one::<u32>("n").copied().unwrap_or(20), "detail": false}),
            )),
            Some(("show", s)) => {
                let r = need(s, "ref")?;
                if !r.starts_with("eth:") || !r.contains("@seq:") && !r.contains("/seq:") {
                    return Err(UsageError::new(format!("bad REF {r:?}"))
                        .with_hint("REF looks like eth:dut@seq:1234"));
                }
                Ok(RpcCall::new(
                    "eth.show",
                    json!({"ref": r, "hex": s.get_flag("hex")}),
                ))
            }
            Some(("wait-for", s)) => {
                let (min, max) = s
                    .get_one::<(u64, Option<u64>)>("count")
                    .copied()
                    .unwrap_or((1, None));
                let timeout = s
                    .get_one::<Duration>("timeout")
                    .copied()
                    .unwrap_or(DEFAULT_WAIT_TIMEOUT);
                if timeout > MAX_WAIT {
                    return Err(UsageError::new("--timeout must be ≤ 3600s"));
                }
                let within = s.get_one::<Duration>("within").copied();
                if opt_str(s, "anchor").is_some() && within.is_none() {
                    return Err(UsageError::new("--anchor needs --within"));
                }
                let mut call = RpcCall::new(
                    "eth.wait_for",
                    json!({"ch": opt_str(s, "ch"), "filter": need(s, "filter")?, "count": {"min": min, "max": max},
                           "within_ms": within.map(ms), "anchor": opt_str(s, "anchor"), "timeout_ms": ms(timeout),
                           "include_own": s.get_flag("include-own")}),
                );
                call.wait = Some(timeout + Duration::from_secs(2));
                Ok(call)
            }
            Some(("stats", s)) => Ok(RpcCall::new(
                "eth.stats",
                json!({"ch": opt_str(s, "ch"), "by": opt_str(s, "by").unwrap_or_else(|| "proto".into()),
                       "filter": opt_str(s, "filter"), "range": range(s)}),
            )),
            Some(("follow", s)) => Ok(RpcCall::new(
                "eth.follow",
                json!({"ch": opt_str(s, "ch"), "proto": need(s, "proto")?, "stream": s.get_one::<u32>("stream").copied(),
                       "max_bytes": s.get_one::<u32>("max-bytes").copied(), "range": range(s)}),
            )),
            Some(("send", s)) => send_call(s),
            Some(("export", s)) => Ok(RpcCall::new(
                "eth.export",
                json!({"ch": opt_str(s, "ch"), "format": need(s, "format")?, "path": need(s, "out")?,
                       "filter": opt_str(s, "filter"), "range": range(s)}),
            )),
            Some(("filter", f)) => match f.subcommand() {
                Some(("check", s)) => Ok(RpcCall::new(
                    "eth.filter.check",
                    json!({"expr": need(s, "expr")?}),
                )),
                _ => Err(UsageError::new("expected `filter check EXPR`")),
            },
            _ => Err(UsageError::new("unknown eth command").with_hint("loom eth help --json")),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            render_data(call, d, out)?;
        }
        if let Some(d) = env.error.as_ref().map(|e| &e.detail).filter(|d| d.is_object())
            && call.method == "eth.wait_for"
        {
            render_wait(d, out)?;
        }
        render_common(env, out)
    }
}

fn capture_call(c: &ArgMatches) -> Result<RpcCall, UsageError> {
    match c.subcommand() {
        Some(("start", s)) => {
            let replay = opt_str(s, "replay");
            let backend = opt_str(s, "backend")
                .unwrap_or_else(|| if replay.is_some() { "replay" } else { "live" }.into());
            if backend == "replay" && replay.is_none() {
                return Err(UsageError::new("--backend replay needs --replay FILE"));
            }
            let speed = match opt_str(s, "speed").as_deref() {
                None | Some("max") => json!("max"),
                Some(n) => json!(
                    n.parse::<f64>()
                        .map_err(|_| UsageError::new("--speed is max or a number"))?
                ),
            };
            Ok(RpcCall::new(
                "eth.capture.start",
                json!({"iface": need(s, "iface")?, "ch": opt_str(s, "ch"), "backend": backend, "bpf": opt_str(s, "bpf"),
                       "snaplen": s.get_one::<u32>("snaplen").copied().unwrap_or(65535), "promisc": s.get_flag("promisc"),
                       "buffer_mib": s.get_one::<u32>("buffer").copied().unwrap_or(16),
                       "replay": replay.map(|p| json!({"path": p, "speed": speed, "repeat": s.get_flag("repeat")}))}),
            ))
        }
        Some(("stop", s)) => Ok(RpcCall::new("eth.capture.stop", json!({"ch": opt_str(s, "ch")}))),
        Some(("status", s)) => Ok(RpcCall::new(
            "eth.capture.status",
            json!({"ch": opt_str(s, "ch")}),
        )),
        _ => Err(UsageError::new("expected capture start|stop|status")),
    }
}

fn send_call(s: &ArgMatches) -> Result<RpcCall, UsageError> {
    let kind = need(s, "kind")?;
    let hex: String = need(s, "hex")?
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .collect();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UsageError::new("HEX must be an even number of hex digits"));
    }
    let to = opt_str(s, "to");
    if kind != "raw"
        && to
            .as_deref()
            .and_then(|t| t.parse::<std::net::SocketAddr>().ok())
            .is_none()
    {
        return Err(UsageError::new(format!("send {kind} needs --to IP:PORT")));
    }
    if kind == "raw" && to.is_some() {
        return Err(UsageError::new(
            "send raw takes the whole frame; --to is for udp/tcp",
        ));
    }
    if s.get_flag("keep-open") && kind != "tcp" {
        return Err(UsageError::new("--keep-open is for send tcp only"));
    }
    if s.get_one::<u16>("from-port").is_some() && kind != "udp" {
        return Err(UsageError::new("--from-port is for send udp only"));
    }
    let dry = s.get_flag("dry-run");
    let params = json!({"ch": opt_str(s, "ch"), "kind": kind, "hex": hex, "to": to,
                        "from_port": s.get_one::<u16>("from-port").copied()});
    // dry-run: the server decodes and classifies only (schema v1: no row, no audit).
    let mut params = params;
    if dry {
        params["dry_run"] = json!(true);
    }
    if s.get_flag("keep-open") {
        params["keep_open"] = json!(true);
    }
    let mut call = RpcCall::new("eth.send", params);
    call.idempotency_key = opt_str(s, "idempotency_key");
    Ok(call)
}

// --- human rendering --------------------------------------------------------------------------

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|x| x.parse().ok()))
}

/// `HH:MM:SS.ffffff` (UTC) from wall-clock ns.
pub fn clock(wall_ns: u64) -> String {
    let secs = wall_ns / 1_000_000_000;
    let us = (wall_ns % 1_000_000_000) / 1000;
    format!(
        "{:02}:{:02}:{:02}.{us:06}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

fn cut(t: &str, w: usize) -> String {
    if t.chars().count() <= w {
        t.to_owned()
    } else {
        let mut o: String = t.chars().take(w.saturating_sub(1)).collect();
        o.push('…');
        o
    }
}

fn render_data(call: &RpcCall, d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    match call.method.as_str() {
        "eth.query" => render_rows(d, out),
        "eth.show" => render_show(
            d,
            call.params.get("hex").and_then(Value::as_bool).unwrap_or(false),
            out,
        ),
        "eth.wait_for" => render_wait(d, out),
        "eth.stats" => {
            writeln!(
                out,
                "{:<48} {:>10} {:>12}",
                d["by"].as_str().unwrap_or("key"),
                "packets",
                "bytes"
            )?;
            for r in d["rows"].as_array().into_iter().flatten() {
                writeln!(
                    out,
                    "{:<48} {:>10} {:>12}",
                    cut(&s(&r["key"]), 48),
                    s(&r["packets"]),
                    s(&r["bytes"])
                )?;
            }
            writeln!(
                out,
                "({} packets · {} bytes · {} keys{})",
                s(&d["total_packets"]),
                s(&d["total_bytes"]),
                s(&d["keys"]),
                gaps_note(d)
            )
        }
        "eth.follow" => {
            for seg in d["segments"].as_array().into_iter().flatten() {
                let arrow = if seg["dir"] == "c2s" { ">>" } else { "<<" };
                let hex = s(&seg["data"]);
                let bytes = hex_to_bytes(&hex);
                writeln!(
                    out,
                    "{arrow} {} B @{} seq {}",
                    bytes.len(),
                    s(&seg["off"]),
                    s(&seg["seq"])
                )?;
                hexdump(&bytes, out)?;
            }
            writeln!(
                out,
                "({} bytes{}{})",
                s(&d["bytes"]),
                if d["truncated"] == true {
                    " · truncated"
                } else {
                    ""
                },
                gaps_note(d)
            )
        }
        "eth.send" if d.get("risk").is_some() && d.get("proto").is_some() => writeln!(
            out,
            "{} {} B  {}  [{}: {}]{}",
            s(&d["proto"]),
            s(&d["len"]),
            s(&d["info"]),
            s(&d["risk"]),
            s(&d["reason"]),
            if d["dry_run"] == true {
                "  (dry run, nothing sent)"
            } else {
                ""
            }
        ),
        "eth.export" => writeln!(
            out,
            "wrote {} packets ({} B) to {}{}",
            s(&d["packets"]),
            s(&d["bytes"]),
            s(&d["path"]),
            if d["truncated"] == true {
                " · truncated"
            } else {
                ""
            }
        ),
        "eth.filter.check" => {
            if d["ok"] == true {
                let f: Vec<String> = d["fields_used"].as_array().into_iter().flatten().map(s).collect();
                writeln!(out, "ok · fields: {}", f.join(", "))
            } else {
                let expr = call.params["expr"].as_str().unwrap_or("");
                let pos = num(&d["error"]["pos"]).unwrap_or(0) as usize;
                writeln!(out, "{expr}")?;
                writeln!(
                    out,
                    "{}^ {} (expected {})",
                    " ".repeat(pos.min(expr.len())),
                    s(&d["error"]["msg"]),
                    s(&d["error"]["expected"])
                )
            }
        }
        "eth.ifaces" => {
            for i in d["ifaces"].as_array().into_iter().flatten() {
                let flag = |k: &str, yes: &str, no: &str| {
                    if i[k] == true {
                        yes.to_owned()
                    } else {
                        no.to_owned()
                    }
                };
                writeln!(
                    out,
                    "{:<16} {:<4} {:<8} {:<10} {}",
                    s(&i["name"]),
                    flag("up", "up", "down"),
                    flag("loopback", "loopback", ""),
                    flag("can_capture", "capture", "no-capture"),
                    s(&i["reason"])
                )?;
            }
            Ok(())
        }
        "eth.capture.status" if d.get("channels").is_some() => {
            for c in d["channels"].as_array().into_iter().flatten() {
                writeln!(
                    out,
                    "{:<8} {:<18} {:<7} {}",
                    s(&c["ch"]),
                    s(&c["state"]),
                    s(&c["backend"]),
                    s(&c["iface"])
                )?;
            }
            Ok(())
        }
        _ => writeln!(out, "{}", serde_json::to_string_pretty(d).unwrap_or_default()),
    }
}

fn gaps_note(d: &Value) -> String {
    let n = d["gaps"].as_array().map_or(0, Vec::len) + num(&d["gaps_omitted"]).unwrap_or(0) as usize;
    if n == 0 {
        String::new()
    } else {
        format!(" · gaps {n}")
    }
}

/// tshark-like packet list with the standard trailer.
fn render_rows(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let rows = d["rows"].as_array().cloned().unwrap_or_default();
    writeln!(
        out,
        "{:>8} {:<15} {:<22} {:<22} {:<10} {:>5} INFO ORG",
        "SEQ", "TIME", "SOURCE", "DESTINATION", "PROTO", "LEN"
    )?;
    for r in &rows {
        let org = match r["attributed_origin"].as_str() {
            Some("agent") => "AGT",
            Some("user") => "USR",
            Some("script") => "SCR",
            _ => "",
        };
        let line = format!(
            "{:>8} {:<15} {:<22} {:<22} {:<10} {:>5} {} {org}",
            s(&r["seq"]),
            num(&r["ts"]["wall_ns"]).map(clock).unwrap_or_default(),
            cut(&s(&r["src"]), 22),
            cut(&s(&r["dst"]), 22),
            cut(&s(&r["proto"]), 10),
            s(&r["len"]),
            cut(&s(&r["info"]), INFO_WIDTH),
        );
        writeln!(out, "{}", line.trim_end())?;
    }
    let gaps = d["gaps"].as_array().cloned().unwrap_or_default();
    let src_gap = gaps
        .iter()
        .filter(|g| g["kind"] == "source" || g["kind"] == "record" || g["kind"] == "scan")
        .count();
    let unpublished: u64 = gaps
        .iter()
        .filter(|g| g["kind"] == "unpublished")
        .filter_map(|g| num(&g["count"]))
        .sum();
    let mut parts = vec![format!("{} rows", rows.len())];
    if src_gap > 0 {
        parts.push(format!("source.gap {src_gap}"));
    }
    if unpublished > 0 {
        parts.push(format!(
            "display {}/{}",
            rows.len(),
            rows.len() as u64 + unpublished
        ));
    }
    if d["truncated"] == true {
        parts.push(format!("matched {}", s(&d["matched"])));
    }
    if let Some(p) = d["prev_cursor"].as_str() {
        parts.push(format!("older: --until {p}"));
    }
    if let Some(n) = d["next_cursor"].as_str() {
        parts.push(format!("next: --since {n}"));
    }
    writeln!(out, "({})", parts.join(" · "))
}

fn render_tree(nodes: &Value, depth: usize, out: &mut dyn io::Write) -> io::Result<()> {
    if depth > TREE_MAX_DEPTH {
        return Ok(());
    }
    for n in nodes.as_array().into_iter().flatten() {
        writeln!(out, "{}{}", "    ".repeat(depth), s(&n["label"]))?;
        render_tree(&n["children"], depth + 1, out)?;
    }
    Ok(())
}

fn render_show(d: &Value, hex: bool, out: &mut dyn io::Write) -> io::Result<()> {
    writeln!(
        out,
        "{} · {} B on wire, {} B captured{}",
        s(&d["ref"]),
        s(&d["len"]),
        s(&d["caplen"]),
        if d["truncated"] == true {
            " (stored bytes truncated)"
        } else {
            ""
        }
    )?;
    render_tree(&d["tree"], 0, out)?;
    for w in d["warnings"].as_array().into_iter().flatten() {
        writeln!(out, "[{}] {}: {}", s(&w["code"]), s(&w["layer"]), s(&w["detail"]))?;
    }
    if hex {
        match payload_bytes(&json!({"bytes": d["bytes"]})) {
            Ok(Some(b)) => {
                hexdump(&b[..b.len().min(HEX_DUMP_MAX)], out)?;
                if b.len() > HEX_DUMP_MAX {
                    writeln!(out, "… {} more bytes", b.len() - HEX_DUMP_MAX)?;
                }
            }
            _ => writeln!(out, "(bytes not inline)")?,
        }
    }
    Ok(())
}

fn render_wait(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let ev = &d["evidence"][0];
    writeln!(
        out,
        "{} · {} matching packets · window {}",
        if d["matched"] == true {
            "matched"
        } else {
            "not matched"
        },
        s(&d["count"]),
        s(&ev["ref"]),
    )?;
    for r in ev["matched_refs"].as_array().into_iter().flatten() {
        writeln!(out, "  {}", s(r))?;
    }
    let g = d["gaps"].as_array().map_or(0, Vec::len);
    if g > 0 {
        writeln!(
            out,
            "  losses in window: {g} (absence/count claims are inconclusive)"
        )?;
    }
    Ok(())
}

fn hex_to_bytes(h: &str) -> Vec<u8> {
    let h: Vec<u8> = h.bytes().filter(u8::is_ascii_hexdigit).collect();
    h.chunks_exact(2)
        .filter_map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|t| u8::from_str_radix(t, 16).ok())
        })
        .collect()
}

/// `0000  45 00 …  E.…` lines.
pub fn hexdump(b: &[u8], out: &mut dyn io::Write) -> io::Result<()> {
    for (i, line) in b.chunks(16).enumerate() {
        let hex: Vec<String> = line.iter().map(|x| format!("{x:02x}")).collect();
        let asc: String = line
            .iter()
            .map(|&x| {
                if x.is_ascii_graphic() || x == b' ' {
                    x as char
                } else {
                    '.'
                }
            })
            .collect();
        writeln!(out, "{:04x}  {:<47}  {asc}", i * 16, hex.join(" "))?;
    }
    Ok(())
}
