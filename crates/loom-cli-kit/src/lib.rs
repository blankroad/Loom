//! CLI kit for `loom` and domain CLI fragments (contract v1 §3, plan §4).
//!
//! A domain crate's `cli` piece implements [`DomainCli`]: a clap subcommand tree, a pure
//! `ArgMatches → RpcCall` conversion and a human renderer. The common parts — exit codes,
//! `NOTE:`/`WARNING:`/`hint:` lines, cursor arguments, client id, output limits — live here so
//! every domain prints the same way.
//!
//! P0 (`core-p0-d2`): pure helpers work; [`run_cli`] parses arguments and the domain conversion,
//! then answers `E_NOT_IMPLEMENTED` (exit 1) because the daemon client arrives in P1 (C15).

use std::fmt::Write as _;
use std::io;
use std::marker::PhantomData;
use std::process::ExitCode;
use std::time::Duration;

use loom_proto::cursor::Cursor;
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast, warnings};
use loom_proto::error::ErrorCode;
use serde_json::Value;

/// Hard output limit: rows (D5).
pub const MAX_ROWS: usize = 1000;
/// Hard output limit: bytes (D5).
pub const MAX_BYTES: usize = 64 * 1024;
/// Longest `--follow` stream (D5).
pub const MAX_FOLLOW: Duration = Duration::from_secs(600);
/// Longest single-result wait (`expect`, `wait-for`, approval wait — A6).
pub const MAX_WAIT: Duration = Duration::from_secs(3600);
/// Environment variable naming the client (D5).
pub const CLIENT_ENV: &str = "LOOM_CLIENT";

/// An RPC the CLI will send.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    /// Method (`uart.send`).
    pub method: String,
    /// Params object.
    pub params: Value,
    /// `--idempotency-key` for writes (never auto-retry; query by key instead).
    pub idempotency_key: Option<String>,
    /// How long the CLI may wait for the result (bounded by [`MAX_WAIT`]).
    pub wait: Option<Duration>,
}

impl RpcCall {
    /// Call with params and no options.
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        RpcCall {
            method: method.into(),
            params,
            idempotency_key: None,
            wait: None,
        }
    }
}

/// Usage problem found while converting arguments (exit 2 `E_USAGE`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct UsageError {
    /// What is wrong.
    pub message: String,
    /// How to fix it.
    pub hint: Option<String>,
}

impl UsageError {
    /// New usage error.
    pub fn new(message: impl Into<String>) -> Self {
        UsageError {
            message: message.into(),
            hint: None,
        }
    }

    /// With a hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A domain's CLI fragment (`loom <domain> …`). The `loom` binary links only these fragments,
/// never device crates (invariant I1).
pub trait DomainCli {
    /// Subcommand tree; its name is the domain (`uart`).
    fn command() -> clap::Command;
    /// Pure conversion of parsed arguments to an RPC (unit-testable without a daemon).
    fn to_call(matches: &clap::ArgMatches) -> Result<RpcCall, UsageError>;
    /// Human rendering of a result. Common `NOTE:`/`WARNING:` lines are added by the kit.
    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()>;
}

/// Object-safe view of a [`DomainCli`] (use [`CliOf`]).
pub trait DynDomainCli {
    /// Domain name (= subcommand name).
    fn name(&self) -> String;
    /// See [`DomainCli::command`].
    fn command(&self) -> clap::Command;
    /// See [`DomainCli::to_call`].
    fn to_call(&self, matches: &clap::ArgMatches) -> Result<RpcCall, UsageError>;
    /// See [`DomainCli::render_human`].
    fn render_human(&self, call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()>;
}

/// Adapter from a [`DomainCli`] type to [`DynDomainCli`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CliOf<T>(PhantomData<fn() -> T>);

impl<T: DomainCli> CliOf<T> {
    /// New adapter.
    pub const fn new() -> Self {
        CliOf(PhantomData)
    }
}

impl<T: DomainCli> DynDomainCli for CliOf<T> {
    fn name(&self) -> String {
        T::command().get_name().to_owned()
    }
    fn command(&self) -> clap::Command {
        T::command()
    }
    fn to_call(&self, matches: &clap::ArgMatches) -> Result<RpcCall, UsageError> {
        T::to_call(matches)
    }
    fn render_human(&self, call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        T::render_human(call, env, out)
    }
}

/// Exit code for an envelope: 0 on success, otherwise the error's group (D5·E2·E3).
pub fn exit_code_for<T>(env: &Envelope<T>) -> u8 {
    env.exit_code()
}

/// Parse a `--since`/`--until`/`--from` argument (clap `value_parser`-friendly).
pub fn parse_cursor_arg(s: &str) -> Result<Cursor, String> {
    s.parse::<Cursor>().map_err(|e| e.to_string())
}

/// Standard cursor option `--<name> <cursor>`: parsed to [`Cursor`] and accepting leading `-`
/// (so `--since -5s` works without `=`). Use for `--since`, `--until`, `--from`.
pub fn cursor_arg(name: &'static str) -> clap::Arg {
    clap::Arg::new(name)
        .long(name)
        .value_name("CURSOR")
        .allow_hyphen_values(true)
        .value_parser(parse_cursor_arg)
        .help("@last | now | seq:N | e<E>:seq:N | mark:X | -5s | HH:MM:SS[.mmm] | ISO8601")
}

fn valid_client_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

/// Client id `name#pid` from explicit inputs (pure; see [`client_id`]).
/// `name` = `LOOM_CLIENT` if valid, else `anon-<user>`.
pub fn client_id_from(loom_client: Option<&str>, user: Option<&str>, pid: u32) -> String {
    let name = match loom_client {
        Some(c) if valid_client_name(c) => c.to_owned(),
        _ => {
            let user: String = user
                .unwrap_or("user")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .take(32)
                .collect();
            format!("anon-{}", if user.is_empty() { "user" } else { &user })
        }
    };
    format!("{name}#{pid}")
}

/// Client id of this process: `<LOOM_CLIENT or anon-<user>>#<pid>`. The daemon keys the
/// `others_since_last` and `@last` cursors on the part before `#`, so successive `loom` processes of
/// one agent share them (D5). Set `LOOM_CLIENT` to keep two agents of one user apart.
pub fn client_id() -> String {
    let lc = std::env::var(CLIENT_ENV).ok();
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).ok();
    client_id_from(lc.as_deref(), user.as_deref(), std::process::id())
}

fn warning_text(code: &str) -> String {
    match code {
        warnings::NO_VIEWER_ATTACHED => {
            "no viewer attached — the user cannot see this. Ask them to run `loom ui`.".into()
        }
        warnings::NO_VISIBLE_VIEWER => "the Loom UI is open but not visible to the user.".into(),
        warnings::DAEMON_AUTOSTARTED => "started the Loom daemon for this call.".into(),
        warnings::RECORD_DEGRADED => "recording is falling behind (no data lost yet).".into(),
        warnings::ENV_VEHICLE => "running against a real vehicle (env=vehicle).".into(),
        warnings::DAEMON_TIED_TO_PARENT_JOB => {
            "the daemon is tied to this agent's job and may stop with it; ask the user to run `loom daemon start`.".into()
        }
        other => other.to_owned(),
    }
}

fn others_line(o: &OthersSinceLast) -> Option<String> {
    if o.count.get() == 0 {
        return None;
    }
    let origins: Vec<String> = o
        .origins
        .iter()
        .map(|g| {
            serde_json::to_value(g)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect();
    let mut s = format!(
        "NOTE: {} event{} by {} since your last call",
        o.count,
        if o.count.get() == 1 { "" } else { "s" },
        if origins.is_empty() {
            "others".to_owned()
        } else {
            origins.join(", ")
        }
    );
    if !o.by_channel.is_empty() {
        let parts: Vec<String> = o.by_channel.iter().map(|(k, v)| format!("{k} {v}")).collect();
        let _ = write!(s, " ({})", parts.join(", "));
    }
    if o.kinds.contains_key("core.latch") {
        s.push_str(" — STOP state changed");
    }
    Some(s)
}

/// Common human lines for any envelope: `error[…]`/`hint:`, `NOTE:` (user/other activity),
/// `WARNING:` (warnings). Domains print their data first, then call this.
pub fn render_common<T>(env: &Envelope<T>, out: &mut dyn io::Write) -> io::Result<()> {
    if let Some(e) = &env.error {
        writeln!(out, "error[{}]: {}", e.code, e.message)?;
        if let Some(h) = &e.hint {
            writeln!(out, "hint: {h}")?;
        }
    }
    if let Some(n) = others_line(&env.others_since_last) {
        writeln!(out, "{n}")?;
    }
    for w in &env.warnings {
        writeln!(out, "WARNING: {}", warning_text(&w.code))?;
    }
    Ok(())
}

/// Root `loom` command with the domain subcommands and the global `--json` flag.
pub fn root_command(domains: &[&dyn DynDomainCli]) -> clap::Command {
    let mut cmd = clap::Command::new("loom")
        .about("Loom — agents and users on the same live device stream")
        .subcommand_required(true)
        .arg(
            clap::Arg::new("json")
                .long("json")
                .global(true)
                .action(clap::ArgAction::SetTrue)
                .help("Print the result envelope as JSON"),
        );
    for d in domains {
        cmd = cmd.subcommand(d.command());
    }
    cmd
}

fn usage_envelope(cmd: &str, err: &UsageError) -> Envelope<Value> {
    let body = ErrorBody {
        code: ErrorCode::Usage,
        message: err.message.clone(),
        hint: err.hint.clone(),
        retryable: false,
        request_id: None,
        detail: Value::Null,
    };
    Envelope::failure(cmd, body, Meta::default(), OthersSinceLast::default())
}

fn not_implemented_envelope(cmd: &str) -> Envelope<Value> {
    let body = ErrorBody {
        code: ErrorCode::NotImplemented,
        message: "the daemon client is not implemented yet (P1 C15)".into(),
        hint: Some("P0 builds only check arguments; nothing was sent to any device".into()),
        retryable: false,
        request_id: None,
        detail: Value::Null,
    };
    Envelope::failure(cmd, body, Meta::default(), OthersSinceLast::default())
}

fn emit(env: &Envelope<Value>, json: bool, out: &mut dyn io::Write) -> io::Result<()> {
    if json {
        let s = serde_json::to_string(env).map_err(io::Error::other)?;
        writeln!(out, "{s}")
    } else {
        render_common(env, out)
    }
}

/// Testable body of [`run_cli`]: parse `args` (including `argv[0]`), convert with the domain,
/// print to `out`/`err`, return the exit code.
pub fn run_cli_with(
    args: &[String],
    domains: &[&dyn DynDomainCli],
    out: &mut dyn io::Write,
    err: &mut dyn io::Write,
) -> u8 {
    let matches = match root_command(domains).try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => {
            let code = if e.use_stderr() {
                ErrorCode::Usage.exit_code()
            } else {
                0
            };
            let _ = if e.use_stderr() {
                write!(err, "{}", e.render())
            } else {
                write!(out, "{}", e.render())
            };
            return code;
        }
    };
    let json = matches.get_flag("json");
    let Some((name, sub)) = matches.subcommand() else {
        return ErrorCode::Usage.exit_code();
    };
    let Some(domain) = domains.iter().find(|d| d.name() == name) else {
        return ErrorCode::Usage.exit_code();
    };
    let env = match domain.to_call(sub) {
        Ok(call) => not_implemented_envelope(&call.method),
        Err(u) => usage_envelope(name, &u),
    };
    // JSON goes to stdout (machine-readable); human error lines go to stderr.
    let _ = if json {
        emit(&env, true, out)
    } else {
        emit(&env, false, err)
    };
    exit_code_for(&env)
}

/// `loom` main entry. **P0:** argument checking only, then `E_NOT_IMPLEMENTED` (exit 1).
pub fn run_cli(domains: &[&dyn DynDomainCli]) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let code = run_cli_with(&args, domains, &mut io::stdout(), &mut io::stderr());
    ExitCode::from(code)
}
