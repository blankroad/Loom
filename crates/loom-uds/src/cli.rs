//! CLI piece: `loom uds connect|status|read-did|write-did|session|dtc list|raw|trace|abort`.
//!
//! Pure `ArgMatches → RpcCall` conversion and human rendering; no device, DoIP or worker code
//! (invariant I1). Params follow the `uds.*` schemas in [`crate::module`] and what the handlers
//! read; shape checks here only catch typos early — risk, dry-run, STOP, lease and every ECU
//! error stay in the gateway. `--json`, `--timeout`, `--idempotency-key`, exit codes and the
//! `NOTE:`/`WARNING:` lines come from `loom` and `loom-cli-kit`.

use std::io;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use loom_cli_kit::{DomainCli, MAX_WAIT, RpcCall, UsageError, render_common};
use loom_proto::envelope::Envelope;
use serde_json::{Map, Value, json};

use crate::{DOMAIN, text};

/// Most DIDs in one `read-did` (`uds.read_did` schema: 1–64).
pub const MAX_DIDS: usize = 64;
/// Most rows `trace` asks for (`handlers::MAX_TRACE_ROWS`).
pub const MAX_TRACE_TAIL: u32 = 1000;
/// Longest per-request timeout (`--timeout-ms`).
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// Extra CLI wait over the request timeout (queueing, 0x78 pendings, read-back).
const WAIT_MARGIN: Duration = Duration::from_secs(5);

/// The `uds` CLI fragment.
#[derive(Debug, Default)]
pub struct UdsCli;

fn strip_0x(t: &str) -> &str {
    t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t)
}

/// DID as `F190` / `0xF190` (1–4 hex digits): the gateway rule ([`text::parse_did`]).
pub fn parse_did(s: &str) -> Result<String, String> {
    text::parse_did(s)
        .map(|_| s.to_owned())
        .ok_or_else(|| format!("bad DID {s:?} (1–4 hex digits, e.g. F190)"))
}

/// 16-bit address as hex (`0x1001`, `1001`) → number (the gateway reads strings as hex too).
pub fn parse_addr(s: &str) -> Result<u16, String> {
    text::parse_did(s).ok_or_else(|| format!("bad address {s:?} (hex, e.g. 0x1001)"))
}

/// One byte as hex (`09`, `0xFF`) → number.
pub fn parse_hex_u8(s: &str) -> Result<u8, String> {
    let b = strip_0x(s);
    if b.is_empty() || b.len() > 2 {
        return Err(format!("bad byte {s:?} (hex 00–FF)"));
    }
    u8::from_str_radix(b, 16).map_err(|_| format!("bad byte {s:?} (hex 00–FF)"))
}

/// Shape check with the gateway's own pure encoder ([`text::encode_typed`]): the CLI refuses
/// exactly what `loomd` would refuse as `E_USAGE`, and never decides risk.
fn check(method: &str, p: &Map<String, Value>) -> Result<(), UsageError> {
    if p.values()
        .any(|v| v.as_str().is_some_and(|s| s.len() > text::MAX_TEXT_LEN))
    {
        return Err(UsageError::new(format!(
            "argument longer than {} characters",
            text::MAX_TEXT_LEN
        )));
    }
    text::encode_typed(method, &Value::Object(p.clone()))
        .map(|_| ())
        .map_err(UsageError::new)
}

fn target() -> Arg {
    Arg::new("target")
        .short('t')
        .long("target")
        .value_name("TARGET")
        .help("target name (default: the only connected target)")
}

fn timeout_ms() -> Arg {
    Arg::new("timeout_ms")
        .long("timeout-ms")
        .value_name("MS")
        .value_parser(value_parser!(u64).range(1..=MAX_TIMEOUT_MS))
        .help("request timeout in ms (ECU P2/0x78 handling stays in loomd)")
}

/// Params with `target` (when given) and the absent options left out.
fn params(m: &ArgMatches) -> Map<String, Value> {
    let mut p = Map::new();
    if let Ok(Some(t)) = m.try_get_one::<String>("target") {
        p.insert("target".into(), json!(t));
    }
    if let Ok(Some(t)) = m.try_get_one::<u64>("timeout_ms") {
        p.insert("timeout_ms".into(), json!(t));
    }
    p
}

/// RPC with a CLI wait covering `reads` requests of `--timeout-ms` each.
fn call(method: &str, m: &ArgMatches, p: Map<String, Value>, reads: u32) -> RpcCall {
    let t = m.try_get_one::<u64>("timeout_ms").ok().flatten().copied();
    let mut c = RpcCall::new(method, Value::Object(p));
    c.wait = t.map(|t| (Duration::from_millis(t) * reads + WAIT_MARGIN).min(MAX_WAIT));
    c
}

impl DomainCli for UdsCli {
    fn command() -> Command {
        Command::new(DOMAIN)
            .about("ECU diagnostics over UDS (DoIP; `--sim bench` = virtual ECU behind loopback DoIP)")
            .subcommand_required(true)
            .subcommand(
                Command::new("connect")
                    .about("Connect a target: --sim bench, or --host [--port] [--la] (only addresses the user gave)")
                    .arg(
                        Arg::new("name")
                            .long("name")
                            .value_name("NAME")
                            .required(true)
                            .help("target alias, e.g. ecu1"),
                    )
                    .arg(
                        Arg::new("sim")
                            .long("sim")
                            .value_name("PROFILE")
                            .num_args(0..=1)
                            .default_missing_value("bench")
                            .value_parser(["bench"])
                            .help("virtual ECU behind a loopback DoIP entity (profile: bench)"),
                    )
                    .arg(
                        Arg::new("host")
                            .long("host")
                            .value_name("IP")
                            .help("DoIP entity address (real ECU)"),
                    )
                    .group(ArgGroup::new("backend").args(["sim", "host"]).required(true))
                    .arg(
                        Arg::new("port")
                            .long("port")
                            .value_parser(value_parser!(u16).range(1..))
                            .help("TCP port (default 13400)"),
                    )
                    .arg(
                        Arg::new("la")
                            .long("la")
                            .value_name("HEX")
                            .value_parser(parse_addr)
                            .help("ECU logical address (default 0x1001)"),
                    )
                    .arg(
                        Arg::new("tester_la")
                            .long("tester-la")
                            .value_name("HEX")
                            .value_parser(parse_addr)
                            .help("tester logical address (default 0x0E80)"),
                    )
                    .arg(
                        Arg::new("activation")
                            .long("activation")
                            .value_name("HEX")
                            .value_parser(parse_hex_u8)
                            .help("routing activation type (default 00)"),
                    )
                    .arg(
                        Arg::new("no_reconnect")
                            .long("no-reconnect")
                            .action(ArgAction::SetTrue)
                            .help("do not reconnect after link loss"),
                    )
                    .arg(timeout_ms().help("connect timeout in ms (default 3000)")),
            )
            .subcommand(
                Command::new("status")
                    .about("Targets: link, session (+confidence), security, lease")
                    .arg(target()),
            )
            .subcommand(
                Command::new("read-did")
                    .about("ReadDataByIdentifier 0x22 (R0)")
                    .arg(
                        Arg::new("dids")
                            .value_name("DID")
                            .required(true)
                            .num_args(1..=MAX_DIDS)
                            .value_parser(parse_did)
                            .help("DIDs as hex, e.g. F190 F18C"),
                    )
                    .arg(target())
                    .arg(timeout_ms()),
            )
            .subcommand(
                Command::new("write-did")
                    .about("WriteDataByIdentifier 0x2E, then read back (R2; ISO identification DIDs R3)")
                    .arg(
                        Arg::new("did")
                            .value_name("DID")
                            .required(true)
                            .value_parser(parse_did),
                    )
                    .arg(
                        Arg::new("hex")
                            .long("hex")
                            .value_name("HEX")
                            .required(true)
                            .help("data bytes as hex, e.g. 02 or '01 02'"),
                    )
                    .arg(target())
                    .arg(timeout_ms()),
            )
            .subcommand(
                Command::new("session")
                    .about("DiagnosticSessionControl 0x10 (R1; programming R3)")
                    .arg(
                        Arg::new("session")
                            .value_name("SESSION")
                            .required(true)
                            .help("default | extended | programming | safety | hex id"),
                    )
                    .arg(target()),
            )
            .subcommand(
                Command::new("dtc")
                    .about("Diagnostic trouble codes")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("list")
                            .about("ReadDTCInformation 19 02 by status mask (R0)")
                            .arg(
                                Arg::new("mask")
                                    .long("mask")
                                    .value_name("HEX")
                                    .value_parser(parse_hex_u8)
                                    .help("status mask as hex (default FF)"),
                            )
                            .arg(target()),
                    ),
            )
            .subcommand(
                Command::new("raw")
                    .about("Send request bytes; class comes from the bytes (--dry-run previews only)")
                    .arg(
                        Arg::new("bytes")
                            .value_name("HEX")
                            .required(true)
                            .num_args(1..)
                            .help("request bytes as hex, e.g. '22 F1 90' or 22F190"),
                    )
                    .arg(
                        Arg::new("dry_run")
                            .long("dry-run")
                            .action(ArgAction::SetTrue)
                            .help("show bytes, service and risk; nothing is sent or recorded"),
                    )
                    .arg(target())
                    .arg(timeout_ms()),
            )
            .subcommand(
                Command::new("trace")
                    .about("Last UDS events of a target from the journal (bounded)")
                    .arg(target())
                    .arg(
                        Arg::new("tail")
                            .long("tail")
                            .value_name("N")
                            .value_parser(value_parser!(u32).range(1..=i64::from(MAX_TRACE_TAIL)))
                            .help("rows (default 40, ≤ 1000)"),
                    ),
            )
            .subcommand(
                Command::new("abort")
                    .about("Abort queued and in-flight requests of a target")
                    .arg(target())
                    .arg(
                        Arg::new("scope")
                            .long("scope")
                            .value_parser(["all", "agent"])
                            .help("viewers: all (default) or agent; agents abort only their own"),
                    ),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        let help = || UsageError::new("unknown uds command").with_hint("loom uds help --json");
        let (sub, s) = m.subcommand().ok_or_else(help)?;
        let mut p = params(s);
        match sub {
            "connect" => {
                let name = s
                    .get_one::<String>("name")
                    .ok_or_else(|| UsageError::new("connect needs --name"))?;
                p.insert("name".into(), json!(name));
                if let Some(sim) = s.get_one::<String>("sim") {
                    p.insert("sim".into(), json!(sim));
                }
                if let Some(h) = s.get_one::<String>("host") {
                    p.insert("host".into(), json!(h));
                }
                if let Some(v) = s.get_one::<u16>("port") {
                    p.insert("port".into(), json!(v));
                }
                for k in ["la", "tester_la"] {
                    if let Some(v) = s.get_one::<u16>(k) {
                        p.insert(k.into(), json!(v));
                    }
                }
                if let Some(v) = s.get_one::<u8>("activation") {
                    p.insert("activation".into(), json!(v));
                }
                if s.get_flag("no_reconnect") {
                    p.insert("auto_reconnect".into(), json!(false));
                }
                Ok(call("uds.connect", s, p, 1))
            }
            "status" => Ok(call("uds.status", s, p, 0)),
            "read-did" => {
                let dids: Vec<&String> = s.get_many::<String>("dids").into_iter().flatten().collect();
                p.insert("dids".into(), json!(dids));
                check("uds.read_did", &p)?;
                Ok(call("uds.read_did", s, p, 1))
            }
            "write-did" => {
                p.insert("did".into(), json!(s.get_one::<String>("did")));
                p.insert("data_hex".into(), json!(s.get_one::<String>("hex")));
                check("uds.write_did", &p).map_err(|e| UsageError::new(format!("--hex: {}", e.message)))?;
                // Write + read-back verify: two requests.
                Ok(call("uds.write_did", s, p, 2))
            }
            "session" => {
                p.insert("session".into(), json!(s.get_one::<String>("session")));
                check("uds.session", &p)
                    .map_err(|e| e.with_hint("default | extended | programming | safety | hex id"))?;
                Ok(call("uds.session", s, p, 1))
            }
            "dtc" => match s.subcommand() {
                Some(("list", l)) => {
                    let mut p = params(l);
                    if let Some(v) = l.get_one::<u8>("mask") {
                        p.insert("mask".into(), json!(v));
                    }
                    Ok(call("uds.dtc.list", l, p, 1))
                }
                _ => Err(UsageError::new("expected `dtc list`")),
            },
            "raw" => {
                let bytes = s
                    .get_many::<String>("bytes")
                    .into_iter()
                    .flatten()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(" ");
                p.insert("bytes_hex".into(), json!(bytes));
                check("uds.request", &p)
                    .map_err(|e| e.with_hint("hex bytes only, e.g. loom uds raw '22 F1 90'"))?;
                if s.get_flag("dry_run") {
                    p.insert("dry_run".into(), json!(true));
                }
                Ok(call("uds.request", s, p, 1))
            }
            "trace" => {
                if let Some(n) = s.get_one::<u32>("tail") {
                    p.insert("tail".into(), json!(n));
                }
                Ok(call("uds.trace", s, p, 0))
            }
            "abort" => {
                if let Some(v) = s.get_one::<String>("scope") {
                    p.insert("scope".into(), json!(v));
                }
                Ok(call("uds.abort", s, p, 0))
            }
            _ => Err(help()),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            render_data(call, d, out)?;
        } else if let Some(e) = &env.error
            && e.detail.get("row").is_some()
        {
            // A refused / negative / timed-out request still carries its row.
            render_row(&e.detail, out)?;
        }
        render_common(env, out)
    }
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => "-".into(),
        x => x.to_string(),
    }
}

fn hex_of(v: &Value) -> String {
    s(&v["data"])
}

fn render_state(st: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let lease = &st["lease"];
    writeln!(
        out,
        "{}  {} {}  link {}{}  session {} ({})  security {}  gen {}{}",
        s(&st["target"]),
        s(&st["backend"]),
        s(&st["addr"]),
        s(&st["link"]["state"]),
        st["link"]["cause"]
            .as_str()
            .map(|c| format!(" ({c})"))
            .unwrap_or_default(),
        s(&st["session"]["name"]),
        s(&st["session"]["confidence"]),
        st["security"]["level"]
            .as_u64()
            .map_or_else(|| "locked".to_owned(), |l| format!("level {l:02X}")),
        s(&st["generation"]),
        if lease.is_object() {
            format!("  lease {} {}", s(&lease["owner_origin"]), s(&lease["actor_id"]))
        } else {
            String::new()
        }
    )
}

fn render_row(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let r = &d["row"];
    let mut line = format!(
        "{}  {}  {}  {} · {}",
        s(&d["request_id"]),
        s(&r["write_state"]),
        s(&r["summary"]),
        s(&r["risk"]),
        s(&r["reason"])
    );
    if r["resp_bytes"].is_object() {
        line.push_str(&format!("  → {}", hex_of(&r["resp_bytes"])));
    }
    if let Some(n) = r["nrc"].as_u64() {
        line.push_str(&format!("  NRC 0x{n:02X} {}", s(&r["nrc_name"])));
    }
    if r["timeout"] == true {
        line.push_str("  TIMEOUT");
    }
    if r["ok_suppressed"] == true {
        line.push_str("  (positive response suppressed)");
    }
    if r["block"].is_object() {
        line.push_str(&format!("  BLOCKED {}", s(&r["block"]["reason"])));
    }
    if let Some(ms) = r["rtt_ms"].as_f64() {
        line.push_str(&format!("  {ms:.1} ms"));
    }
    writeln!(out, "{line}")?;
    for x in r["decoded"].as_array().into_iter().flatten() {
        writeln!(
            out,
            "  {} {}  {}{}",
            s(&x["did"]),
            s(&x["name"]),
            s(&x["value"]),
            x["unit"].as_str().map(|u| format!(" {u}")).unwrap_or_default()
        )?;
    }
    if let Some(dtcs) = r["dtcs"].as_array() {
        writeln!(out, "  {} DTC(s)", dtcs.len())?;
        for x in dtcs {
            let st = x["status"].as_u64().unwrap_or(0);
            writeln!(out, "  {}  status {st:02X}", s(&x["dtc"]))?;
        }
    }
    if r["write_state"] == "outcome_unknown" {
        writeln!(
            out,
            "WARNING: outcome unknown — do not resend; read the value back"
        )?;
    }
    Ok(())
}

fn render_data(call: &RpcCall, d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    if d["dry_run"] == true {
        return writeln!(
            out,
            "dry run · {} · {} · {} · {}{} · nothing sent",
            s(&d["target"]),
            hex_of(&d["bytes"]),
            s(&d["summary"]),
            s(&d["risk"]),
            match &d["block"] {
                Value::Null => format!(" · {}", s(&d["reason"])),
                b => format!(" · WOULD BE REFUSED ({}: {})", s(b), s(&d["reason"])),
            }
        );
    }
    match call.method.as_str() {
        "uds.connect" => {
            if d["existing"] == true {
                writeln!(out, "already connected:")?;
            }
            render_state(&d["state"], out)
        }
        "uds.status" => {
            let ts = d["targets"].as_array().map(Vec::as_slice).unwrap_or_default();
            if ts.is_empty() {
                return writeln!(out, "no targets (loom uds connect --sim bench --name ecu1)");
            }
            for t in ts {
                render_state(t, out)?;
            }
            Ok(())
        }
        "uds.trace" => {
            for e in d["events"].as_array().into_iter().flatten() {
                let p = &e["payload"];
                writeln!(
                    out,
                    "{:>8}  {:<11} {:<5} {}  {}{}",
                    s(&e["seq"]),
                    s(&e["kind"]),
                    s(&e["origin"]),
                    s(&e["request_id"]),
                    p.get("write_state").map(s).unwrap_or_default(),
                    p.get("summary")
                        .map(|x| format!("  {}", s(x)))
                        .or_else(|| p.get("bytes").map(|b| format!("  {}", hex_of(b))))
                        .unwrap_or_default()
                )?;
            }
            if d["complete"] == false {
                writeln!(out, "NOTE: scan limit reached; older events not shown")?;
            }
            Ok(())
        }
        "uds.abort" => writeln!(
            out,
            "{}  scope {}  cancelled {} waiting{}",
            s(&d["target"]),
            s(&d["scope"]),
            s(&d["cancelled_waiting"]),
            if d["in_flight_aborted"] == true {
                " · in-flight request aborted (outcome unknown)"
            } else {
                ""
            }
        ),
        _ if d.get("row").is_some() => render_row(d, out),
        _ => writeln!(out, "{}", serde_json::to_string_pretty(d).unwrap_or_default()),
    }
}
