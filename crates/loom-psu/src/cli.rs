//! CLI piece: `loom psu connect|status|meas|watch|wait-for|set|out|cycle|sim` (plan §5.1 d).

use std::io;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use loom_cli_kit::{DomainCli, MAX_FOLLOW, MAX_WAIT, RpcCall, UsageError, cursor_arg, render_common};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::Envelope;
use serde_json::{Value, json};

use crate::DOMAIN;
use crate::model::MAX_CYCLE_OFF_MS;

/// The `psu` CLI fragment.
#[derive(Debug, Default)]
pub struct PsuCli;

/// `5s`, `250ms`, `2m`, `1h`, or bare seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (n, unit) = s.split_at(split);
    let n: u64 = n
        .parse()
        .map_err(|_| format!("bad duration {s:?} (use 500ms, 5s, 2m, 1h)"))?;
    match unit {
        "ms" => Ok(Duration::from_millis(n)),
        "" | "s" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_secs(n.saturating_mul(60))),
        "h" => Ok(Duration::from_secs(n.saturating_mul(3600))),
        _ => Err(format!("bad duration unit in {s:?} (ms, s, m, h)")),
    }
}

/// `A,B` → `[A, B]` with A < B.
pub fn parse_range(s: &str) -> Result<(f64, f64), String> {
    let (a, b) = s
        .split_once(',')
        .ok_or_else(|| format!("bad range {s:?} (use LO,HI)"))?;
    let a: f64 = a.trim().parse().map_err(|_| format!("bad number {a:?}"))?;
    let b: f64 = b.trim().parse().map_err(|_| format!("bad number {b:?}"))?;
    if a < b && a.is_finite() && b.is_finite() {
        Ok((a, b))
    } else {
        Err(format!("range {s:?} needs LO < HI"))
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn name() -> Arg {
    Arg::new("name")
        .required(true)
        .value_name("NAME")
        .help("supply name (channel), e.g. bench")
}

fn dur(id: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_name("DURATION")
        .value_parser(parse_duration)
        .help(help)
}

fn num(id: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_name("NUM")
        .value_parser(value_parser!(f64))
        .allow_hyphen_values(true)
        .help(help)
}

fn dry() -> Arg {
    Arg::new("dry-run")
        .long("dry-run")
        .action(ArgAction::SetTrue)
        .help("show class and reason only; nothing is sent")
}

fn get_name(m: &ArgMatches) -> Result<String, UsageError> {
    m.get_one::<String>("name")
        .cloned()
        .ok_or_else(|| UsageError::new("missing supply name"))
}

impl DomainCli for PsuCli {
    fn command() -> Command {
        Command::new(DOMAIN)
            .about("Bench power supply (M1: SimPsu)")
            .subcommand_required(true)
            .subcommand(
                Command::new("connect")
                    .about("Connect a supply (M1: simulated)")
                    .arg(name())
                    .arg(
                        Arg::new("visa")
                            .long("visa")
                            .value_name("RESOURCE")
                            .help("real supply (not in M1)"),
                    )
                    .arg(num("vmax", "tighten the voltage limit (cannot raise it)"))
                    .arg(num("imax", "tighten the current limit (cannot raise it)"))
                    .arg(
                        Arg::new("seed")
                            .long("seed")
                            .value_parser(value_parser!(u64))
                            .help("sim noise seed"),
                    ),
            )
            .subcommand(
                Command::new("status")
                    .about("Settings, output, protection, poll rate")
                    .arg(name()),
            )
            .subcommand(Command::new("meas").about("Measure now").arg(name()))
            .subcommand(
                Command::new("watch")
                    .about("Summary of the next DURATION (≤ 600 s)")
                    .arg(name())
                    .arg(dur("for", "how long").required(true))
                    .arg(
                        Arg::new("raw")
                            .long("raw")
                            .action(ArgAction::SetTrue)
                            .help("list samples"),
                    )
                    .arg(
                        Arg::new("max")
                            .long("max")
                            .value_parser(value_parser!(u32).range(1..=1000))
                            .requires("raw"),
                    )
                    .arg(cursor_arg("since")),
            )
            .subcommand(
                Command::new("wait-for")
                    .about("Wait until a current/voltage condition holds (exit 3 timeout, 6 user OFF)")
                    .arg(name())
                    .arg(num("curr-lt", "current below A"))
                    .arg(num("curr-gt", "current above A"))
                    .arg(
                        Arg::new("curr-between")
                            .long("curr-between")
                            .value_name("LO,HI")
                            .value_parser(parse_range),
                    )
                    .arg(num("volt-lt", "voltage below V"))
                    .arg(num("volt-gt", "voltage above V"))
                    .arg(
                        Arg::new("volt-between")
                            .long("volt-between")
                            .value_name("LO,HI")
                            .value_parser(parse_range),
                    )
                    .group(
                        ArgGroup::new("cond")
                            .args([
                                "curr-lt",
                                "curr-gt",
                                "curr-between",
                                "volt-lt",
                                "volt-gt",
                                "volt-between",
                            ])
                            .required(true),
                    )
                    .arg(dur("hold", "condition must hold this long (default 0)"))
                    .arg(dur(
                        "max-gap",
                        "largest sample interval inside a hold (default 600ms)",
                    ))
                    .arg(dur("timeout", "give up after (≤ 3600 s)").required(true))
                    .arg(cursor_arg("since")),
            )
            .subcommand(
                Command::new("set")
                    .about("Set voltage/current (R2; over the profile limit → exit 4)")
                    .arg(name())
                    .arg(num("volt", "volts"))
                    .arg(num("curr", "current limit, amperes"))
                    .group(
                        ArgGroup::new("value")
                            .args(["volt", "curr"])
                            .required(true)
                            .multiple(true),
                    )
                    .arg(dry()),
            )
            .subcommand(
                Command::new("out")
                    .about("Output on/off (R3)")
                    .arg(name())
                    .arg(Arg::new("state").required(true).value_parser(["on", "off"]))
                    .arg(dry()),
            )
            .subcommand(
                Command::new("cycle")
                    .about("Power cycle: off, wait, on (R3)")
                    .arg(name())
                    .arg(dur("off", "off time (≤ 60 s)").required(true))
                    .arg(dry()),
            )
            .subcommand(
                Command::new("sim")
                    .about("Simulator controls")
                    .subcommand_required(true)
                    .subcommand(
                        Command::new("inject")
                            .about("Inject answer delay / unanswered queries")
                            .arg(name())
                            .arg(
                                Arg::new("delay-ms")
                                    .long("delay-ms")
                                    .value_parser(value_parser!(u64).range(0..=10_000)),
                            )
                            .arg(
                                Arg::new("no-response")
                                    .long("no-response")
                                    .value_name("N")
                                    .value_parser(value_parser!(u32).range(0..=1000)),
                            )
                            .arg(num("load-ohm", "load resistance"))
                            .arg(Arg::new("clear").long("clear").action(ArgAction::SetTrue)),
                    ),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        match m.subcommand() {
            Some(("connect", s)) => {
                let backend = if s.contains_id("visa") && s.get_one::<String>("visa").is_some() {
                    "visa"
                } else {
                    "sim"
                };
                Ok(RpcCall::new(
                    "psu.connect",
                    json!({"ch": get_name(s)?, "backend": backend, "visa": s.get_one::<String>("visa"),
                           "vmax": s.get_one::<f64>("vmax"), "imax": s.get_one::<f64>("imax"), "seed": s.get_one::<u64>("seed")}),
                ))
            }
            Some(("status", s)) => Ok(RpcCall::new("psu.status", json!({"ch": get_name(s)?}))),
            Some(("meas", s)) => Ok(RpcCall::new("psu.meas", json!({"ch": get_name(s)?}))),
            Some(("watch", s)) => {
                let d = s.get_one::<Duration>("for").copied().unwrap_or_default();
                if d > MAX_FOLLOW {
                    return Err(UsageError::new("--for must be ≤ 600s"));
                }
                let mut c = RpcCall::new(
                    "psu.watch",
                    json!({"ch": get_name(s)?, "for_ms": ms(d), "raw": s.get_flag("raw"), "max": s.get_one::<u32>("max"),
                           "since": s.get_one::<Cursor>("since").map(ToString::to_string)}),
                );
                c.wait = Some(d + Duration::from_secs(10));
                Ok(c)
            }
            Some(("wait-for", s)) => {
                let timeout = s.get_one::<Duration>("timeout").copied().unwrap_or_default();
                if timeout > MAX_WAIT {
                    return Err(UsageError::new("--timeout must be ≤ 3600s"));
                }
                let mut p = json!({"ch": get_name(s)?, "timeout_ms": ms(timeout),
                                   "hold_ms": s.get_one::<Duration>("hold").map_or(0, |d| ms(*d)),
                                   "since": s.get_one::<Cursor>("since").map(ToString::to_string)});
                if let Some(g) = s.get_one::<Duration>("max-gap") {
                    p["max_gap_ms"] = json!(ms(*g));
                }
                for k in ["curr-lt", "curr-gt", "volt-lt", "volt-gt"] {
                    if let Some(v) = s.get_one::<f64>(k) {
                        p[k.replace('-', "_")] = json!(v);
                    }
                }
                for k in ["curr-between", "volt-between"] {
                    if let Some((a, b)) = s.get_one::<(f64, f64)>(k) {
                        p[k.replace('-', "_")] = json!([a, b]);
                    }
                }
                let mut c = RpcCall::new("psu.wait_for", p);
                c.wait = Some(timeout);
                Ok(c)
            }
            Some(("set", s)) => Ok(RpcCall::new(
                "psu.set",
                json!({"ch": get_name(s)?, "volt": s.get_one::<f64>("volt"), "curr": s.get_one::<f64>("curr"),
                       "dry_run": s.get_flag("dry-run")}),
            )),
            Some(("out", s)) => Ok(RpcCall::new(
                "psu.out",
                json!({"ch": get_name(s)?, "state": s.get_one::<String>("state"), "dry_run": s.get_flag("dry-run")}),
            )),
            Some(("cycle", s)) => {
                let off = s.get_one::<Duration>("off").copied().unwrap_or_default();
                if off.is_zero() || ms(off) > MAX_CYCLE_OFF_MS {
                    return Err(UsageError::new("--off must be between 1ms and 60s"));
                }
                Ok(RpcCall::new(
                    "psu.cycle",
                    json!({"ch": get_name(s)?, "off_ms": ms(off), "dry_run": s.get_flag("dry-run")}),
                ))
            }
            Some(("sim", sim)) => match sim.subcommand() {
                Some(("inject", s)) => {
                    let mut p = json!({"ch": get_name(s)?, "clear": s.get_flag("clear")});
                    if let Some(d) = s.get_one::<u64>("delay-ms") {
                        p["delay_ms"] = json!(d);
                    }
                    if let Some(n) = s.get_one::<u32>("no-response") {
                        p["no_response"] = json!(n);
                    }
                    if let Some(r) = s.get_one::<f64>("load-ohm") {
                        p["load"] = json!({"r_ohm": r});
                    }
                    Ok(RpcCall::new("psu.sim.inject", p))
                }
                _ => Err(UsageError::new("expected `sim inject`")),
            },
            _ => Err(UsageError::new("unknown psu command").with_hint("loom psu help --json")),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            render_data(call, d, out)?;
        }
        if let Some(e) = &env.error
            && call.method == "psu.wait_for"
            && e.detail.is_object()
        {
            let d = &e.detail;
            writeln!(
                out,
                "best hold {} ms of {} ms · samples {} · losses {} · last {}",
                s(&d["best_ms"]),
                s(&d["hold_ms"]),
                s(&d["samples"]),
                s(&d["losses"]),
                f3(&d["last"])
            )?;
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

fn f(v: &Value, digits: usize) -> String {
    v.as_f64().map_or_else(|| "-".into(), |x| format!("{x:.digits$}"))
}

fn f3(v: &Value) -> String {
    f(v, 3)
}

fn onoff(v: &Value) -> &'static str {
    if v == &Value::Bool(true) { "ON" } else { "OFF" }
}

fn render_data(call: &RpcCall, d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    if d["dry_run"] == true {
        return writeln!(
            out,
            "dry run · {} · {}{} · nothing sent",
            s(&d["risk"]),
            s(&d["reason"]),
            if d["blocked"] == true {
                " · WOULD BE REFUSED"
            } else {
                ""
            }
        );
    }
    match call.method.as_str() {
        "psu.connect" | "psu.status" => {
            writeln!(
                out,
                "{}  set {} V / {} A  output {}  mode {}  prot {}  limits {} V / {} A",
                s(&d["ch"]),
                f(&d["set_v"], 2),
                f(&d["set_i"], 2),
                onoff(&d["out"]),
                s(&d["mode"]),
                s(&d["prot"]),
                f(&d["vmax"], 1),
                f(&d["imax"], 1)
            )?;
            if d.get("poll_hz_req").is_some() {
                writeln!(
                    out,
                    "poll {} Hz (requested {} Hz) · samples {} · timeouts {} · {}",
                    f(&d["poll_hz_actual"], 1),
                    s(&d["poll_hz_req"]),
                    s(&d["samples"]),
                    s(&d["timeouts"]),
                    s(&d["idn"])
                )?;
            }
            Ok(())
        }
        "psu.meas" => writeln!(
            out,
            "{}  {} V  {} A  {} W  {}  age {} ms",
            s(&d["ch"]),
            f(&d["v"], 3),
            f(&d["i"], 3),
            f(&d["p"], 2),
            s(&d["mode"]),
            s(&d["age_ms"])
        ),
        "psu.watch" => {
            let (v, i) = (&d["voltage"], &d["current"]);
            writeln!(
                out,
                "{}  voltage min {}  max {}  mean {} V",
                s(&d["ch"]),
                f3(&v["min"]),
                f3(&v["max"]),
                f3(&v["mean"])
            )?;
            let lost = d["timeouts"].as_u64().unwrap_or(0);
            writeln!(
                out,
                "current  min {}  max {}  mean {} A   samples {}/{}{}  rate {} Hz",
                f3(&i["min"]),
                f3(&i["max"]),
                f3(&i["mean"]),
                s(&d["samples"]),
                s(&d["expected"]),
                if lost > 0 {
                    format!(" ({lost} timeout)")
                } else {
                    String::new()
                },
                f(&d["rate_hz"], 1)
            )?;
            for r in d["rows"].as_array().into_iter().flatten() {
                writeln!(
                    out,
                    "  seq {:>8}  {} V  {} A",
                    s(&r["seq"]),
                    f3(&r["v"]),
                    f3(&r["i"])
                )?;
            }
            let users = d["user_actions"].as_array().map_or(0, Vec::len);
            if users > 0 {
                writeln!(
                    out,
                    "NOTE: {users} user action(s) in this window — check before acting"
                )?;
            }
            writeln!(
                out,
                "evidence {} · next: --since {}",
                s(&d["ref"]),
                s(&d["next_cursor"])
            )
        }
        "psu.wait_for" => writeln!(
            out,
            "matched {} held {} ms ({} samples) · evidence {}",
            s(&d["condition"]),
            s(&d["held_ms"]),
            s(&d["samples"]),
            s(&d["ref"])
        ),
        "psu.set" | "psu.out" | "psu.cycle" => {
            if let Some(rs) = d["results"].as_array() {
                for r in rs {
                    writeln!(
                        out,
                        "{}  {}",
                        s(&r["ch"]),
                        if r["ok"] == true {
                            "off".to_owned()
                        } else {
                            s(&r["error"])
                        }
                    )?;
                }
                return Ok(());
            }
            let rb = &d["readback"];
            let mut parts = Vec::new();
            if rb.get("set_v").is_some() {
                parts.push(format!("set {} V / {} A", f(&rb["set_v"], 2), f(&rb["set_i"], 2)));
            }
            if rb.get("out").is_some() {
                parts.push(format!("output {}", onoff(&rb["out"])));
            }
            if rb.get("prot").is_some_and(|p| p != "ok") {
                parts.push(format!("PROTECTION {}", s(&rb["prot"])));
            }
            if d.get("off_ms").is_some() {
                parts.push(format!("off for {} ms, then on", s(&d["off_ms"])));
            }
            writeln!(
                out,
                "{} · {}",
                call.method,
                if parts.is_empty() {
                    "sent".to_owned()
                } else {
                    parts.join(" · ")
                }
            )
        }
        _ => writeln!(out, "{}", serde_json::to_string_pretty(d).unwrap_or_default()),
    }
}
