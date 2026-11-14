//! `loom` — the agent's (and user's) command line to the Loom daemon (contract 03, plan §4).
//!
//! Every command prints one result envelope (`--json`) or human text with the common
//! `NOTE:`/`WARNING:` lines, and exits with the contract exit code. Commands are bounded: no
//! endless streams (`events --follow` ≤ 600 s), at most 1000 rows / 64 KiB per result.

pub mod client;
pub mod render;
mod skills;

use std::io::{self, IsTerminal as _, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Arg, ArgAction, ArgMatches, Command};
use loom_cli_kit::{DynDomainCli, MAX_FOLLOW, MAX_WAIT, RpcCall, client_id, cursor_arg, render_common};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast, Warning, warnings};
use loom_proto::error::ErrorCode;
use serde_json::{Value, json};

use crate::client::{Conn, Connected, DaemonInfo, Start, body, connect, loom_home, rpc_error_body};

/// Default wait for one call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Commands that exist but arrive after M1.
const LATER: &[(&str, &str)] = &[
    (
        "confirm",
        "Confirm a write with a token (only when the user enabled confirmations)",
    ),
    (
        "approvals",
        "List / wait for approvals (only when the user enabled approvals)",
    ),
    ("run", "Run a Loom script"),
    ("test", "Integration test sessions"),
    ("record", "Recording sessions and export"),
    ("policy", "Show / diff / propose policy"),
];

/// Domain CLI fragments linked into `loom` (cli pieces only — invariant I1).
pub fn domains() -> Vec<Box<dyn DynDomainCli>> {
    vec![
        #[cfg(feature = "echo")]
        Box::new(loom_cli_kit::CliOf::<loom_echo::cli::EchoCli>::new()),
        #[cfg(feature = "uart")]
        Box::new(loom_cli_kit::CliOf::<loom_uart::cli::UartCli>::new()),
        #[cfg(feature = "can")]
        Box::new(loom_cli_kit::CliOf::<loom_can::cli::CanCli>::new()),
        #[cfg(feature = "uds")]
        Box::new(loom_cli_kit::CliOf::<loom_uds::cli::UdsCli>::new()),
        #[cfg(feature = "eth")]
        Box::new(loom_cli_kit::CliOf::<loom_eth::cli::EthCli>::new()),
        #[cfg(feature = "psu")]
        Box::new(loom_cli_kit::CliOf::<loom_psu::cli::PsuCli>::new()),
        #[cfg(feature = "test")]
        Box::new(loom_cli_kit::CliOf::<loom_test::cli::TestCli>::new()),
        #[cfg(feature = "logic")]
        Box::new(loom_cli_kit::CliOf::<loom_logic::cli::LogicCli>::new()),
    ]
}

fn filter_args(c: Command) -> Command {
    c.arg(
        Arg::new("domain")
            .long("domain")
            .action(ArgAction::Append)
            .help("Only this domain (repeatable)"),
    )
    .arg(
        Arg::new("channel")
            .long("channel")
            .action(ArgAction::Append)
            .help("Only this channel (repeatable)"),
    )
    .arg(
        Arg::new("kind")
            .long("kind")
            .action(ArgAction::Append)
            .help("Only this event kind (repeatable)"),
    )
    .arg(
        Arg::new("origin")
            .long("origin")
            .action(ArgAction::Append)
            .value_parser(["device", "user", "agent", "script"])
            .help("Only this origin (repeatable)"),
    )
}

/// The `loom` command tree.
pub fn command(domains: &[Box<dyn DynDomainCli>]) -> Command {
    let mut c = Command::new("loom")
        .about("Loom — agents and users on the same live device stream")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand_required(true)
        .disable_help_subcommand(true)
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Print the result envelope as JSON"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .global(true)
                .value_name("SECS")
                .value_parser(clap::value_parser!(u64).range(1..=3600))
                .help("Longest wait for the result (default 30 s; events --follow ≤ 600 s)"),
        )
        .arg(
            Arg::new("idempotency_key")
                .long("idempotency-key")
                .global(true)
                .value_name("KEY")
                .help("Write once per key; a repeat returns the first result (never re-sends)"),
        )
        .subcommand(
            Command::new("daemon")
                .about("Daemon lifecycle")
                .subcommand_required(true)
                .subcommand(Command::new("start").about("Start the daemon if it is not running"))
                .subcommand(Command::new("stop").about("Stop the daemon"))
                .subcommand(Command::new("status").about("Is the daemon running?"))
                .subcommand(
                    Command::new("logs").about("Last lines of run/loomd.log").arg(
                        Arg::new("tail")
                            .long("tail")
                            .value_parser(clap::value_parser!(u32).range(1..=1000))
                            .default_value("100"),
                    ),
                ),
        )
        .subcommand(Command::new("status").about("Daemon, workers, STOP latch, recording, viewers"))
        .subcommand(
            Command::new("stop-all")
                .about("STOP: latch all writes (only a user can release)")
                .arg(Arg::new("reason").long("reason")),
        )
        .subcommand(filter_args(
            Command::new("events")
                .about("Bounded read of recent events (live ring)")
                .arg(cursor_arg("since"))
                .arg(cursor_arg("until"))
                .arg(
                    Arg::new("max")
                        .long("max")
                        .value_parser(clap::value_parser!(u32).range(1..=1000))
                        .default_value("100"),
                )
                .arg(
                    Arg::new("peek")
                        .long("peek")
                        .action(ArgAction::SetTrue)
                        .help("Do not move your @last cursor"),
                )
                .arg(
                    Arg::new("follow")
                        .long("follow")
                        .action(ArgAction::SetTrue)
                        .help("Keep printing new events until --timeout (≤ 600 s)"),
                ),
        ))
        .subcommand(
            Command::new("mark")
                .about("Put a marker on the shared timeline")
                .arg(Arg::new("label").required(true))
                .arg(Arg::new("note").long("note")),
        )
        .subcommand(
            Command::new("ui")
                .about("Open the Loom UI for the user (TTY only), or focus a panel")
                .subcommand(
                    Command::new("open")
                        .about("Ask the UI to show a panel")
                        .arg(Arg::new("panel").required(true))
                        .arg(Arg::new("filter").long("filter"))
                        .arg(cursor_arg("at")),
                ),
        )
        .subcommand(Command::new("help").about("Commands (use --json for the canonical machine help)"))
        .subcommand(Command::new("version").about("Version"))
        .subcommand(Command::new("doctor").about("Check paths, permissions, daemon and port"))
        .subcommand(skills::command());
    for (name, about) in LATER {
        if domains.iter().any(|d| d.name() == *name) {
            continue;
        }
        c = c.subcommand(
            Command::new(*name).about(format!("{about} (not in M1)")).arg(
                Arg::new("args")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true),
            ),
        );
    }
    for d in domains {
        c = c.subcommand(d.command());
    }
    c
}

/// Per-invocation context.
#[derive(Debug, Clone)]
struct Ctx {
    json: bool,
    timeout: Duration,
    home: PathBuf,
    client: String,
    idem: Option<String>,
}

type Render = fn(&Value, &mut dyn Write) -> io::Result<()>;

/// What a command produced.
struct Output {
    env: Envelope<Value>,
    human: Human,
}

enum Human {
    Core(Render),
    Domain(usize, RpcCall),
    Lines(Vec<String>),
}

fn failure(cmd: &str, b: ErrorBody) -> Envelope<Value> {
    Envelope::failure(cmd, b, Meta::default(), OthersSinceLast::default())
}

fn not_implemented(cmd: &str) -> Envelope<Value> {
    failure(
        cmd,
        body(
            ErrorCode::NotImplemented,
            format!("`loom {cmd}` arrives after M1"),
            Some("nothing was sent to any device"),
        ),
    )
}

fn add_start_warnings(env: &mut Envelope<Value>, c: &Connected) {
    if c.autostarted {
        eprintln!(
            "loom: started the Loom daemon (pid {}, port {})",
            c.info.pid, c.info.port
        );
        env.warnings.push(Warning {
            code: warnings::DAEMON_AUTOSTARTED.into(),
            detail: None,
        });
    }
    if c.tied_to_job {
        env.warnings.push(Warning {
            code: warnings::DAEMON_TIED_TO_PARENT_JOB.into(),
            detail: None,
        });
    }
}

/// Wrap a plain core result into an envelope using `core.meta` (others_since_last on every result).
fn core_envelope(
    conn: &mut Conn,
    cmd: &str,
    r: Result<Value, ErrorBody>,
    timeout: Duration,
) -> Envelope<Value> {
    let m = conn
        .call("core.meta", json!({}), timeout)
        .ok()
        .and_then(Result::ok)
        .unwrap_or(Value::Null);
    let meta: Meta = serde_json::from_value(m["meta"].clone()).unwrap_or_default();
    let others: OthersSinceLast = serde_json::from_value(m["others_since_last"].clone()).unwrap_or_default();
    let mut env = match r {
        Ok(v) => Envelope::success(cmd, v, meta, others),
        Err(b) => Envelope::failure(cmd, b, meta, others),
    };
    env.warnings = serde_json::from_value(m["warnings"].clone()).unwrap_or_default();
    env
}

fn transport(cmd: &str, e: &str) -> Envelope<Value> {
    failure(
        cmd,
        body(
            ErrorCode::Daemon,
            format!("daemon connection failed: {e}"),
            Some("check `loom daemon status`"),
        ),
    )
}

fn core(ctx: &Ctx, cmd: &str, method: &str, params: Value, start: Start) -> Envelope<Value> {
    let mut c = match connect(&ctx.home, &ctx.client, ctx.timeout, start) {
        Ok(c) => c,
        Err(b) => return failure(cmd, b),
    };
    let mut env = match c.conn.call(method, params, ctx.timeout) {
        Ok(r) => core_envelope(&mut c.conn, cmd, r.map_err(|e| rpc_error_body(&e)), ctx.timeout),
        Err(e) => transport(cmd, &e),
    };
    add_start_warnings(&mut env, &c);
    c.conn.close();
    env
}

fn strings(m: &ArgMatches, id: &str) -> Vec<String> {
    m.get_many::<String>(id)
        .map(|v| v.cloned().collect())
        .unwrap_or_default()
}

fn events_params(m: &ArgMatches, since: Option<String>) -> Value {
    json!({
        "since": since,
        "until": m.get_one::<Cursor>("until").map(ToString::to_string),
        "filter": {"domains": strings(m, "domain"), "channels": strings(m, "channel"), "kinds": strings(m, "kind"), "origins": strings(m, "origin")},
        "max": m.get_one::<u32>("max").copied().unwrap_or(100),
        "peek": m.get_flag("peek"),
    })
}

fn seq_field(v: &Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_u64())
}

/// Inclusive start (`seq:N` reads seq ≥ N) of the next `--follow` poll, or `None` to repeat the
/// same cursor. A capped page continues at the daemon's `next_cursor`. Otherwise the daemon
/// scanned through `stream_seq_hw`, so continue after it even when nothing matched: re-sending a
/// moving cursor such as `now` would skip everything published between two polls.
fn follow_resume(data: &Value) -> Option<String> {
    let to = seq_field(&data["range"]["to"]);
    let through = if data["truncated"].as_bool() == Some(true) {
        if let Some(next) = data["next_cursor"].as_str() {
            return Some(next.to_owned());
        }
        to? // capped without a cursor: never jump to the high-water past unread rows
    } else {
        to.max(seq_field(&data["stream_seq_hw"]))?
    };
    let epoch = seq_field(&data["stream_epoch"]).unwrap_or(0);
    Some(format!("e{epoch}:seq:{}", through + 1))
}

/// `events --follow`: poll the ring until the timeout (≤ 600 s), printing as events arrive.
fn follow(ctx: &Ctx, m: &ArgMatches, out: &mut dyn Write) -> u8 {
    let limit = ctx.timeout.min(MAX_FOLLOW);
    let mut c = match connect(&ctx.home, &ctx.client, DEFAULT_TIMEOUT, Start::Auto) {
        Ok(c) => c,
        Err(b) => return emit_env(ctx, &failure("events", b), &Human::Core(render::events), &[], out),
    };
    let end = Instant::now() + limit;
    let mut since = m
        .get_one::<Cursor>("since")
        .map(ToString::to_string)
        .or_else(|| Some("now".into()));
    let mut first = true;
    loop {
        let r = match c.conn.call(
            "core.events.query",
            events_params(m, since.clone()),
            DEFAULT_TIMEOUT,
        ) {
            Ok(r) => r.map_err(|e| rpc_error_body(&e)),
            Err(e) => {
                return emit_env(
                    ctx,
                    &transport("events", &e),
                    &Human::Core(render::events),
                    &[],
                    out,
                );
            }
        };
        let data = match r {
            Ok(d) => d,
            Err(b) => {
                let env = core_envelope(&mut c.conn, "events", Err(b), DEFAULT_TIMEOUT);
                return emit_env(ctx, &env, &Human::Core(render::events), &[], out);
            }
        };
        let n = data["count"].as_u64().unwrap_or(0);
        let more = data["truncated"].as_bool() == Some(true);
        if let Some(next) = follow_resume(&data) {
            since = Some(next);
        }
        if n > 0 || first {
            let mut env = core_envelope(&mut c.conn, "events", Ok(data), DEFAULT_TIMEOUT);
            if first {
                add_start_warnings(&mut env, &c);
            }
            if ctx.json {
                let _ = writeln!(out, "{}", serde_json::to_string(&env).unwrap_or_default());
            } else {
                let _ = render::events(env.data.as_ref().unwrap_or(&Value::Null), out);
                let _ = render_common(&env, &mut io::stderr());
            }
            let _ = out.flush();
            first = false;
        }
        if Instant::now() >= end {
            break;
        }
        if !more {
            // A capped page continues at once; otherwise wait for new events.
            std::thread::sleep(Duration::from_millis(200).min(end.saturating_duration_since(Instant::now())));
        }
    }
    c.conn.close();
    0
}

fn daemon_cmd(ctx: &Ctx, sub: &str, m: &ArgMatches) -> Output {
    let cmd = format!("daemon {sub}");
    let core_h = |env| Output {
        env,
        human: Human::Core(render::generic),
    };
    match sub {
        "start" => match connect(&ctx.home, &ctx.client, ctx.timeout, Start::Always) {
            Ok(c) => {
                let mut env = core_envelope(
                    &mut { c.conn },
                    &cmd,
                    Ok(
                        json!({"running": true, "started": c.autostarted, "port": c.info.port, "pid": c.info.pid, "version": c.info.version}),
                    ),
                    ctx.timeout,
                );
                if c.autostarted {
                    env.warnings.push(Warning {
                        code: warnings::DAEMON_AUTOSTARTED.into(),
                        detail: None,
                    });
                }
                core_h(env)
            }
            Err(b) => core_h(failure(&cmd, b)),
        },
        "stop" => match connect(&ctx.home, &ctx.client, ctx.timeout, Start::Never) {
            Ok(mut c) => {
                let r = c.conn.call("core.shutdown", json!({}), ctx.timeout);
                let addr = c.info.addr();
                c.conn.close();
                let end = Instant::now() + Duration::from_secs(5);
                while Instant::now() < end
                    && std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                let env = match r {
                    Ok(Ok(_)) => Envelope::success(
                        &cmd,
                        json!({"running": false, "stopped_pid": c.info.pid}),
                        Meta::default(),
                        OthersSinceLast::default(),
                    ),
                    Ok(Err(e)) => failure(&cmd, rpc_error_body(&e)),
                    // The daemon may close before answering: that is the expected outcome.
                    Err(_) => Envelope::success(
                        &cmd,
                        json!({"running": false, "stopped_pid": c.info.pid}),
                        Meta::default(),
                        OthersSinceLast::default(),
                    ),
                };
                core_h(env)
            }
            Err(_) => core_h(Envelope::success(
                &cmd,
                json!({"running": false}),
                Meta::default(),
                OthersSinceLast::default(),
            )),
        },
        "status" => Output {
            env: core(ctx, &cmd, "core.status", json!({}), Start::Never),
            human: Human::Core(render::status),
        },
        _ => {
            let n = m.get_one::<u32>("tail").copied().unwrap_or(100) as usize;
            let path = ctx.home.join("run/loomd.log");
            let env = match std::fs::read_to_string(&path) {
                Ok(t) => {
                    let lines: Vec<&str> = t.lines().collect();
                    let tail: Vec<String> = lines[lines.len().saturating_sub(n)..]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect();
                    return Output {
                        env: Envelope::success(
                            &cmd,
                            json!({"path": path, "lines": tail}),
                            Meta::default(),
                            OthersSinceLast::default(),
                        ),
                        human: Human::Lines(tail),
                    };
                }
                Err(e) => failure(
                    &cmd,
                    body(
                        ErrorCode::NotFound,
                        format!("{}: {e}", path.display()),
                        Some("the log exists after the CLI started the daemon"),
                    ),
                ),
            };
            core_h(env)
        }
    }
}

fn ui_cmd(ctx: &Ctx, m: &ArgMatches) -> Output {
    if let Some(("open", o)) = m.subcommand() {
        let p = json!({
            "panel": o.get_one::<String>("panel"),
            "filter": o.get_one::<String>("filter"),
            "at": o.get_one::<Cursor>("at").map(ToString::to_string),
        });
        return Output {
            env: core(ctx, "ui open", "core.ui.focus", p, Start::Auto),
            human: Human::Core(render::generic),
        };
    }
    let mut c = match connect(&ctx.home, &ctx.client, ctx.timeout, Start::Auto) {
        Ok(c) => c,
        Err(b) => {
            return Output {
                env: failure("ui", b),
                human: Human::Core(render::generic),
            };
        }
    };
    // The one-time code goes only to a human terminal (contract 01 §1.3).
    let tty = io::stdout().is_terminal() && io::stdin().is_terminal();
    let (data, lines) = if tty {
        match client::http_get(&c.info, "/api/v1/bootstrap/new") {
            Ok((200, b)) => {
                let url = serde_json::from_str::<Value>(&b)
                    .ok()
                    .and_then(|v| v["url"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                let opened = open_browser(&url);
                (
                    json!({"opened": opened}),
                    vec![
                        format!("Loom UI: {url}"),
                        if opened {
                            "opened in your browser".into()
                        } else {
                            "open this URL in your browser".into()
                        },
                    ],
                )
            }
            other => (
                json!({"opened": false, "error": format!("{other:?}")}),
                vec!["could not get a bootstrap code".into()],
            ),
        }
    } else {
        let msg = "Not a terminal: ask the user to run `loom ui` in their own terminal (the login code is never printed to agents).";
        (
            json!({"opened": false, "tty": false, "hint": msg}),
            vec![msg.to_owned()],
        )
    };
    let mut env = core_envelope(&mut c.conn, "ui", Ok(data), ctx.timeout);
    add_start_warnings(&mut env, &c);
    c.conn.close();
    Output {
        env,
        human: Human::Lines(lines),
    }
}

fn open_browser(url: &str) -> bool {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    cmd.arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

fn doctor(ctx: &Ctx) -> Output {
    let mut checks = Vec::new();
    let mut fail = false;
    let mut check = |name: &str, ok: bool, detail: String, critical: bool| {
        fail |= critical && !ok;
        checks.push(json!({"check": name, "ok": ok, "detail": detail}));
    };
    check("home", ctx.home.is_dir(), ctx.home.display().to_string(), false);
    let bin = client::daemon_bin();
    let bin_ok = bin.is_file() || bin.components().count() == 1;
    check("loomd binary", bin_ok, bin.display().to_string(), true);
    match DaemonInfo::read(&ctx.home) {
        Ok(info) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = std::fs::metadata(ctx.home.join("run/daemon.json"))
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0);
                check(
                    "daemon.json permissions",
                    mode == 0o600,
                    format!("{mode:o}"),
                    true,
                );
            }
            let port_ok =
                std::net::TcpStream::connect_timeout(&info.addr(), Duration::from_millis(300)).is_ok();
            check(
                "daemon port",
                port_ok,
                format!("127.0.0.1:{} (pid {})", info.port, info.pid),
                false,
            );
            if port_ok {
                let hello = Conn::open(&info, &ctx.client, ctx.timeout);
                check(
                    "daemon hello",
                    hello.is_ok(),
                    hello
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| format!("v{}", info.version)),
                    true,
                );
                if let Ok(c) = hello {
                    c.close();
                }
            }
        }
        Err(e) => check(
            "daemon.json",
            false,
            format!("{e} (daemon not running — it starts on first use)"),
            false,
        ),
    }
    let data = json!({"checks": checks});
    let env = if fail {
        let mut b = body(
            ErrorCode::Config,
            "doctor found problems",
            Some("see data.checks"),
        );
        b.detail = data.clone();
        let mut e = failure("doctor", b);
        e.data = Some(data);
        e
    } else {
        Envelope::success("doctor", data, Meta::default(), OthersSinceLast::default())
    };
    Output {
        env,
        human: Human::Core(|d, out| {
            for c in d["checks"].as_array().into_iter().flatten() {
                writeln!(
                    out,
                    "{} {:<26} {}",
                    if c["ok"] == true { "ok  " } else { "FAIL" },
                    c["check"].as_str().unwrap_or(""),
                    c["detail"].as_str().unwrap_or("")
                )?;
            }
            Ok(())
        }),
    }
}

fn domain_call(
    ctx: &Ctx,
    idx: usize,
    domains: &[Box<dyn DynDomainCli>],
    name: &str,
    sub: &ArgMatches,
) -> Output {
    let d = &domains[idx];
    let mut call = match d.to_call(sub) {
        Ok(c) => c,
        Err(u) => {
            let mut b = body(ErrorCode::Usage, u.message.clone(), u.hint.as_deref());
            b.hint = u.hint;
            return Output {
                env: failure(name, b),
                human: Human::Core(render::generic),
            };
        }
    };
    if let Some(k) = ctx.idem.clone().or_else(|| call.idempotency_key.clone()) {
        call.idempotency_key = Some(k.clone());
        if let Some(o) = call.params.as_object_mut() {
            o.insert("_loom".into(), json!({"idempotency_key": k}));
        }
    }
    #[cfg(feature = "test")]
    if let Some(env) = loom_test::cli::run_local(&call, None) {
        return Output {
            env,
            human: Human::Domain(idx, call),
        };
    }
    let wait = call.wait.unwrap_or(ctx.timeout).min(MAX_WAIT);
    let mut c = match connect(&ctx.home, &ctx.client, ctx.timeout, Start::Auto) {
        Ok(c) => c,
        Err(b) => {
            return Output {
                env: failure(&call.method, b),
                human: Human::Domain(idx, call),
            };
        }
    };
    let mut env = match c.conn.call(&call.method, call.params.clone(), wait) {
        Ok(Ok(v)) => serde_json::from_value::<Envelope<Value>>(v).unwrap_or_else(|e| {
            failure(
                &call.method,
                body(
                    ErrorCode::Internal,
                    format!("bad envelope from daemon: {e}"),
                    None,
                ),
            )
        }),
        Ok(Err(e)) => core_envelope(&mut c.conn, &call.method, Err(rpc_error_body(&e)), ctx.timeout),
        Err(e) => {
            // A write whose answer did not arrive is not known to have failed: never retry blindly.
            let mut b = body(
                ErrorCode::Daemon,
                format!("no result: {e}"),
                Some("do not resend; check `loom events --since @last`"),
            );
            b.detail = json!({"write_state": "outcome_unknown"});
            failure(&call.method, b)
        }
    };
    add_start_warnings(&mut env, &c);
    c.conn.close();
    Output {
        env,
        human: Human::Domain(idx, call),
    }
}

fn emit_env(
    ctx: &Ctx,
    env: &Envelope<Value>,
    human: &Human,
    domains: &[Box<dyn DynDomainCli>],
    out: &mut dyn Write,
) -> u8 {
    if ctx.json {
        let _ = writeln!(out, "{}", serde_json::to_string(env).unwrap_or_default());
        return env.exit_code();
    }
    let mut err = io::stderr();
    let target: &mut dyn Write = if env.ok { out } else { &mut err };
    match human {
        Human::Domain(i, call) => {
            let _ = domains[*i].render_human(call, env, target);
        }
        Human::Core(r) => {
            if let Some(d) = &env.data {
                let _ = r(d, target);
            }
            let _ = render_common(env, target);
        }
        Human::Lines(lines) => {
            for l in lines {
                let _ = writeln!(target, "{l}");
            }
            let _ = render_common(env, target);
        }
    }
    env.exit_code()
}

/// `<domain> help [--json]` (C8): intercepted before clap, which would print its own help.
fn domain_help(args: &[String], domains: &[Box<dyn DynDomainCli>]) -> Option<(usize, bool)> {
    let pos: Vec<&String> = args.iter().skip(1).filter(|a| !a.starts_with('-')).collect();
    let (d, h) = (pos.first()?, pos.get(1)?);
    let idx = domains.iter().position(|x| &x.name() == *d)?;
    (*h == "help").then(|| (idx, args.iter().any(|a| a == "--json")))
}

fn context(m: &ArgMatches, home: PathBuf) -> Ctx {
    // Only the root's seconds-valued timeout configures transport. Domain deadlines are
    // interpreted by to_call() and carried in RpcCall.wait; 1ms/zero-poll commands must not
    // shorten connect/hello and mistake a healthy daemon for a missing one.
    let timeout = m
        .try_get_one::<u64>("timeout")
        .ok()
        .flatten()
        .filter(|s| **s > 0)
        .map_or(DEFAULT_TIMEOUT, |s| Duration::from_secs(*s))
        .min(MAX_WAIT);
    Ctx {
        json: m.get_flag("json"),
        timeout,
        home,
        client: client_id(),
        idem: m.get_one::<String>("idempotency_key").cloned(),
    }
}

/// Run `loom` with `args` (including `argv[0]`); returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write) -> u8 {
    let domains = domains();
    let Some(home) = loom_home() else {
        let _ = writeln!(io::stderr(), "loom: neither LOOM_HOME nor HOME is set");
        return ErrorCode::Config.exit_code();
    };
    if let Some((idx, json)) = domain_help(args, &domains) {
        if !json {
            let _ = writeln!(out, "{}", domains[idx].command().render_long_help());
            return 0;
        }
        let ctx = Ctx {
            json: true,
            timeout: DEFAULT_TIMEOUT,
            home,
            client: client_id(),
            idem: None,
        };
        let name = domains[idx].name();
        let env = core(
            &ctx,
            &format!("{name} help"),
            "core.help",
            json!({"domain": name}),
            Start::Auto,
        );
        return emit_env(&ctx, &env, &Human::Core(render::help), &domains, out);
    }
    let m = match command(&domains).try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => {
            let code = if e.use_stderr() {
                ErrorCode::Usage.exit_code()
            } else {
                0
            };
            let _ = if e.use_stderr() {
                write!(io::stderr(), "{}", e.render())
            } else {
                write!(out, "{}", e.render())
            };
            return code;
        }
    };
    let ctx = context(&m, home);
    let Some((name, sub)) = m.subcommand() else {
        return ErrorCode::Usage.exit_code();
    };
    let o = match name {
        "daemon" => {
            let Some((s, sm)) = sub.subcommand() else {
                return ErrorCode::Usage.exit_code();
            };
            daemon_cmd(&ctx, s, sm)
        }
        "status" => Output {
            env: core(&ctx, "status", "core.status", json!({}), Start::Auto),
            human: Human::Core(render::status),
        },
        "stop-all" => Output {
            env: core(
                &ctx,
                "stop-all",
                "core.stop",
                json!({"reason": sub.get_one::<String>("reason")}),
                Start::Auto,
            ),
            human: Human::Core(render::latch),
        },
        "events" if sub.get_flag("follow") => return follow(&ctx, sub, out),
        "events" => {
            let p = events_params(sub, sub.get_one::<Cursor>("since").map(ToString::to_string));
            Output {
                env: core(&ctx, "events", "core.events.query", p, Start::Auto),
                human: Human::Core(render::events),
            }
        }
        "mark" => {
            let p = json!({"label": sub.get_one::<String>("label"), "note": sub.get_one::<String>("note")});
            Output {
                env: core(&ctx, "mark", "core.mark", p, Start::Auto),
                human: Human::Core(|d, o| writeln!(o, "marked at seq {}", d["seq"].as_str().unwrap_or("?"))),
            }
        }
        "ui" => ui_cmd(&ctx, sub),
        "help" => {
            if !ctx.json {
                let _ = writeln!(out, "{}", command(&domains).render_long_help());
                return 0;
            }
            Output {
                env: core(&ctx, "help", "core.help", json!({}), Start::Auto),
                human: Human::Core(render::help),
            }
        }
        "version" => {
            let daemon = DaemonInfo::read(&ctx.home)
                .ok()
                .map(|i| json!({"version": i.version, "port": i.port, "pid": i.pid}));
            let data =
                json!({"version": env!("CARGO_PKG_VERSION"), "proto": client::PROTO, "daemon": daemon});
            Output {
                env: Envelope::success("version", data, Meta::default(), OthersSinceLast::default()),
                human: Human::Lines(vec![format!(
                    "loom {} ({})",
                    env!("CARGO_PKG_VERSION"),
                    client::PROTO
                )]),
            }
        }
        "doctor" => doctor(&ctx),
        "skills" => Output {
            env: match sub.subcommand() {
                Some(("sync", m)) => match skills::sync(m) {
                    Ok(data) => {
                        Envelope::success("skills sync", data, Meta::default(), OthersSinceLast::default())
                    }
                    Err(e) => failure("skills sync", body(ErrorCode::Usage, e, None)),
                },
                _ => not_implemented("skills"),
            },
            human: Human::Core(render::generic),
        },
        n if LATER.iter().any(|(l, _)| *l == n) && !domains.iter().any(|d| d.name() == n) => Output {
            env: not_implemented(n),
            human: Human::Core(render::generic),
        },
        n => match domains.iter().position(|d| d.name() == n) {
            Some(i) => domain_call(&ctx, i, &domains, n, sub),
            None => return ErrorCode::Usage.exit_code(),
        },
    };
    emit_env(&ctx, &o.env, &o.human, &domains, out)
}

/// Path helper for tests and tools.
pub fn daemon_json(home: &Path) -> PathBuf {
    home.join("run/daemon.json")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod argument_tests {
    use super::*;
    #[test]
    fn domain_deadlines_do_not_shorten_transport_or_panic_on_arg_types() {
        let ds = domains();
        let mut cases = vec![(vec!["loom", "events", "--timeout", "2"], 2000)];
        #[cfg(feature = "eth")]
        cases.push((vec!["loom", "eth", "trace", "--timeout", "25ms"], 30000));
        #[cfg(feature = "test")]
        cases.push((vec!["loom", "test", "wait", "s", "--timeout", "3s"], 30000));
        #[cfg(feature = "logic")]
        cases.push((
            vec!["loom", "logic", "wait", "--text", "4C", "--timeout", "3s"],
            30000,
        ));
        for (args, millis) in cases {
            let m = command(&ds).try_get_matches_from(args.clone()).unwrap();
            assert_eq!(
                context(&m, PathBuf::new()).timeout.as_millis(),
                millis,
                "{args:?}"
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod follow_tests {
    use super::follow_resume;
    use serde_json::{Value, json};

    /// Model of `core.events.query` cursor semantics (gateway core_rpc.rs): `now` = after the
    /// current high-water, `seq:N` inclusive (after N-1), oldest `max` first, `truncated` +
    /// `next_cursor` when capped, `stream_seq_hw` is the high-water the query scanned.
    struct Ring {
        hw: u64,
        events: Vec<(u64, &'static str)>,
    }
    impl Ring {
        fn publish(&mut self, kind: &'static str) {
            self.hw += 1;
            self.events.push((self.hw, kind));
        }
        fn query(&self, since: &str, kind: &str, max: usize) -> Value {
            let after = if since == "now" {
                self.hw
            } else {
                since.rsplit(':').next().unwrap().parse::<u64>().unwrap() - 1
            };
            let all: Vec<u64> = self
                .events
                .iter()
                .filter(|(s, k)| *s > after && *k == kind)
                .map(|(s, _)| *s)
                .collect();
            let page = &all[..all.len().min(max)];
            let truncated = all.len() > max;
            let s = |n: u64| n.to_string();
            json!({
                "events": page.iter().map(|n| json!({"seq": s(*n)})).collect::<Vec<_>>(),
                "count": page.len(),
                "truncated": truncated,
                "next_cursor": truncated.then(|| format!("e7:seq:{}", page.last().unwrap() + 1)),
                "range": page.first().zip(page.last()).map(|(a, b)| json!({"from": s(*a), "to": s(*b)})),
                "stream_seq_hw": s(self.hw),
                "stream_epoch": "7",
            })
        }
    }
    fn seqs(v: &Value) -> Vec<u64> {
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_str().unwrap().parse().unwrap())
            .collect()
    }

    /// Before the fix an empty first page kept `now`, so every later event was skipped.
    #[test]
    fn empty_now_follow_sees_later_events_once_across_capped_pages() {
        let mut ring = Ring { hw: 6, events: (1..=6).map(|s| (s, "uds.tx")).collect() };
        let mut since = "now".to_owned();
        let mut seen = Vec::new();
        let poll = |ring: &Ring, since: &mut String, seen: &mut Vec<u64>| {
            let page = ring.query(since, "uds.tx", 2);
            seen.extend(seqs(&page));
            if let Some(n) = follow_resume(&page) {
                *since = n;
            }
        };
        poll(&ring, &mut since, &mut seen);
        assert!(seen.is_empty());
        assert_eq!(since, "e7:seq:7", "empty page resumes after the scanned high-water");
        // Filtered-out traffic moves the high-water but must not hide matching events.
        ring.publish("uds.state");
        poll(&ring, &mut since, &mut seen);
        assert_eq!(since, "e7:seq:8");
        for _ in 0..5 {
            ring.publish("uds.tx");
            ring.publish("uds.rx");
        }
        for _ in 0..6 {
            poll(&ring, &mut since, &mut seen);
        }
        assert_eq!(seen, vec![8, 10, 12, 14, 16], "all, in order, no duplicates");
        assert_eq!(since, "e7:seq:18");
    }

    #[test]
    fn capped_page_continues_at_next_cursor_not_high_water() {
        let r = json!({"count": 2, "truncated": true, "next_cursor": "e7:seq:4",
            "range": {"from": "2", "to": "3"}, "stream_seq_hw": "50", "stream_epoch": "7"});
        assert_eq!(follow_resume(&r).unwrap(), "e7:seq:4");
        let r = json!({"truncated": true, "next_cursor": null, "range": {"from": "2", "to": "3"},
            "stream_seq_hw": "50", "stream_epoch": "7"});
        assert_eq!(follow_resume(&r).unwrap(), "e7:seq:4");
        // Earlier-epoch journal read: no high-water, continue after the last row; empty → keep.
        let r = json!({"truncated": false, "range": {"from": "1", "to": "9"},
            "stream_seq_hw": null, "stream_epoch": "3"});
        assert_eq!(follow_resume(&r).unwrap(), "e3:seq:10");
        let r = json!({"truncated": false, "range": null, "stream_seq_hw": null, "stream_epoch": "3"});
        assert_eq!(follow_resume(&r), None);
    }
}
