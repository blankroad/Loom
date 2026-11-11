//! CLI fragment plus a real daemon-free local plan check; run_local is the CLI host seam.
use crate::*;
use clap::{Arg, ArgAction, ArgMatches, Command};
use loom_cli_kit::{DomainCli, RpcCall, UsageError, render_common};
use loom_proto::{
    envelope::{Envelope, ErrorBody, Meta, OthersSinceLast},
    error::ErrorCode,
};
use serde_json::{Value, json};
use std::{io, time::Duration};
#[derive(Debug, Default)]
pub struct TestCli;
fn positional(name: &'static str) -> Arg {
    Arg::new(name).required(true)
}
fn flag(name: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::SetTrue)
}
fn value(name: &'static str) -> Arg {
    Arg::new(name).long(name)
}
fn session(cmd: Command) -> Command {
    cmd.arg(positional("session"))
}
fn timeout() -> Arg {
    value("timeout").default_value("30s")
}
fn evidence() -> Arg {
    value("evidence").action(ArgAction::Append).num_args(1..)
}
impl DomainCli for TestCli {
    fn command() -> Command {
        Command::new("test")
            .about("TC sessions with evidence-backed verdicts")
            .subcommand_required(true)
            .subcommand(Command::new("import").arg(positional("path")))
            .subcommand(
                Command::new("plan")
                    .subcommand_required(true)
                    .subcommand(Command::new("check").arg(positional("plan")).arg(value("tc")))
                    .subcommand(Command::new("show").arg(positional("plan")).arg(flag("local")))
                    .subcommand(session(Command::new("amend")).arg(positional("plan"))),
            )
            .subcommand(
                Command::new("start")
                    .arg(positional("tc"))
                    .arg(value("plan"))
                    .arg(value("title"))
                    .arg(value("bench"))
                    .arg(value("env").value_parser(["sim", "bench", "vehicle"]))
                    .arg(
                        value("wait-approval")
                            .num_args(0..=1)
                            .default_missing_value("300s"),
                    ),
            )
            .subcommand(
                session(Command::new("step"))
                    .arg(positional("step"))
                    .arg(flag("end"))
                    .arg(flag("begin").conflicts_with("end")),
            )
            .subcommand(
                session(Command::new("check"))
                    .arg(positional("step"))
                    .arg(flag("auto"))
                    .arg(evidence()),
            )
            .subcommand(
                session(Command::new("verdict"))
                    .arg(positional("step"))
                    .arg(positional("verdict").value_parser([
                        "pass",
                        "fail",
                        "inconclusive",
                        "blocked",
                        "skipped",
                        "done",
                    ]))
                    .arg(evidence())
                    .arg(value("reason"))
                    .arg(flag("override")),
            )
            .subcommand(
                Command::new("evidence").subcommand_required(true).subcommand(
                    session(Command::new("add"))
                        .arg(positional("step"))
                        .arg(evidence())
                        .arg(value("range"))
                        .arg(value("filter"))
                        .arg(value("pred")),
                ),
            )
            .subcommand(
                session(Command::new("note"))
                    .arg(positional("text"))
                    .arg(value("step")),
            )
            .subcommand(
                session(Command::new("ask"))
                    .arg(value("question").required(true))
                    .arg(value("options").value_delimiter(','))
                    .arg(timeout())
                    .arg(value("step")),
            )
            .subcommand(
                session(Command::new("answer"))
                    .arg(positional("ask-id"))
                    .arg(positional("value")),
            )
            .subcommand(session(Command::new("status")))
            .subcommand(
                session(Command::new("wait"))
                    .arg(timeout())
                    .arg(
                        value("until")
                            .value_parser(["ended", "running", "answer"])
                            .default_value("ended"),
                    )
                    .arg(value("ask-id")),
            )
            .subcommand(session(Command::new("pause")))
            .subcommand(session(Command::new("resume")))
            .subcommand(session(Command::new("retry")).arg(positional("step")))
            .subcommand(
                session(Command::new("skip"))
                    .arg(positional("step"))
                    .arg(value("reason").required(true)),
            )
            .subcommand(
                session(Command::new("end"))
                    .arg(flag("abort"))
                    .arg(value("run-finally").value_parser(clap::value_parser!(bool))),
            )
            .subcommand(
                session(Command::new("report")).arg(
                    value("format")
                        .value_parser(["html", "junit", "json"])
                        .default_value("html"),
                ),
            )
            .subcommand(session(Command::new("freeze")).arg(value("output")))
    }
    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        let Some((name, s)) = m.subcommand() else {
            return Err(UsageError::new("test subcommand required"));
        };
        let mut p = json!({});
        let mut method = format!("test.{name}");
        let get = |s: &ArgMatches, k: &str| s.get_one::<String>(k).cloned();
        let put = |p: &mut Value, s: &ArgMatches, k: &str, dest: &str| {
            if let Some(v) = get(s, k) {
                p[dest] = json!(v);
            }
        };
        match name {
            "plan" => {
                let Some((sub, a)) = s.subcommand() else {
                    return Err(UsageError::new("plan check/show/amend required"));
                };
                method = format!("test.plan.{sub}");
                put(&mut p, a, "plan", "plan");
                if sub == "amend" {
                    put(&mut p, a, "session", "session");
                    p["plan"] = json!({"path":p["plan"]});
                } else if sub == "show"
                    && !a.get_flag("local")
                    && !get(a, "plan").is_some_and(|p| p.ends_with(".yaml") || p.ends_with(".yml"))
                {
                    p["session"] = p["plan"].take();
                    p.as_object_mut().expect("params object").remove("plan");
                } else {
                    if sub == "check" {
                        put(&mut p, a, "tc", "tc");
                    }
                    p["local"] = json!(true);
                }
            }
            "start" => {
                p["tc"] = json!({"path":get(s,"tc")});
                if let Some(path) = get(s, "plan") {
                    p["plan"] = json!({"path":path});
                }
                for key in ["title", "bench", "env"] {
                    put(&mut p, s, key, key);
                }
                if let Some(d) = get(s, "wait-approval") {
                    p["wait_approval_ms"] = json!(duration_ms(&d)?);
                }
            }
            "evidence" => {
                let Some(("add", a)) = s.subcommand() else {
                    return Err(UsageError::new("evidence add required"));
                };
                method = "test.evidence.add".into();
                for k in ["session", "step"] {
                    put(&mut p, a, k, k);
                }
                let refs = get_refs(a);
                if !refs.is_empty() {
                    p["evidence"] = json!(refs);
                } else {
                    for k in ["range", "filter", "pred"] {
                        if let Some(v) = get(a, k) {
                            p[k] = if k == "range" && !v.starts_with('{') {
                                json!(v)
                            } else {
                                serde_json::from_str(&v).map_err(|e| UsageError::new(e.to_string()))?
                            };
                        }
                    }
                }
            }
            "import" => put(&mut p, s, "path", "path"),
            _ => {
                put(&mut p, s, "session", "session");
                match name {
                    "step" => {
                        put(&mut p, s, "step", "id");
                        if s.get_flag("end") {
                            method = "test.step.end".into();
                        }
                    }
                    "check" | "verdict" => {
                        put(&mut p, s, "step", "step");
                        let refs = get_refs(s);
                        if !refs.is_empty() {
                            p["evidence"] = json!(refs);
                        }
                        if name == "check" {
                            p["auto"] = json!(s.get_flag("auto"));
                        } else {
                            put(&mut p, s, "verdict", "verdict");
                            put(&mut p, s, "reason", "reason");
                            p["override"] = json!(s.get_flag("override"));
                        }
                    }
                    "note" => {
                        put(&mut p, s, "text", "text");
                        put(&mut p, s, "step", "step");
                    }
                    "ask" => {
                        put(&mut p, s, "question", "question");
                        put(&mut p, s, "step", "step");
                        p["options"] = json!(
                            s.get_many::<String>("options")
                                .map(|x| x.cloned().collect::<Vec<_>>())
                                .unwrap_or_default()
                        );
                        p["timeout_ms"] = json!(duration_ms(get(s, "timeout").as_deref().unwrap_or("30s"))?);
                    }
                    "answer" => {
                        put(&mut p, s, "ask-id", "ask_id");
                        put(&mut p, s, "value", "value");
                    }
                    "wait" => {
                        p["timeout_ms"] = json!(duration_ms(get(s, "timeout").as_deref().unwrap_or("30s"))?);
                        put(&mut p, s, "until", "until");
                        put(&mut p, s, "ask-id", "ask_id");
                    }
                    "skip" => {
                        put(&mut p, s, "step", "step");
                        put(&mut p, s, "reason", "reason");
                    }
                    "retry" => put(&mut p, s, "step", "step"),
                    "end" => {
                        p["abort"] = json!(s.get_flag("abort"));
                        if let Some(v) = s.get_one::<bool>("run-finally") {
                            p["run_finally"] = json!(v);
                        }
                    }
                    "report" => put(&mut p, s, "format", "format"),
                    "freeze" => put(&mut p, s, "output", "output"),
                    _ => (),
                }
            }
        }
        let mut call = RpcCall::new(method, p);
        if let Some(ms) = call
            .params
            .get("timeout_ms")
            .or(call.params.get("wait_approval_ms"))
            .and_then(Value::as_u64)
        {
            call.wait = Some(Duration::from_millis(ms) + Duration::from_secs(2));
        }
        Ok(call)
    }
    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        let data = env
            .data
            .as_ref()
            .or_else(|| env.error.as_ref().map(|e| &e.detail));
        if let Some(d) = data {
            if let Some(result) = d.get("result").and_then(Value::as_str) {
                writeln!(
                    out,
                    "{} · {} · exit {}",
                    d["session_id"].as_str().unwrap_or("session"),
                    result,
                    d["exit_code"]
                )?;
            } else if call.method.starts_with("test.plan.") {
                writeln!(
                    out,
                    "plan {}",
                    d["plan_hash"]
                        .as_str()
                        .or(d["plan_sha256"].as_str())
                        .unwrap_or("unbound")
                )?;
                if let Some(rows) = d.get("warnings").and_then(Value::as_array) {
                    for w in rows {
                        writeln!(out, "WARNING: {}", w["message"].as_str().unwrap_or("unchecked"))?;
                    }
                }
                if let Some(rows) = d.get("errors").and_then(Value::as_array) {
                    for row in rows {
                        if let Ok(diagnostic) =
                            serde_json::from_value::<loom_test_format::Diagnostic>(row.clone())
                        {
                            writeln!(out, "ERROR: {diagnostic}")?;
                        }
                    }
                }
                if let Some(r) = d.get("risk_summary") {
                    writeln!(out, "risk {r}")?;
                }
            } else if let Some(state) = d.get("state").and_then(Value::as_str) {
                writeln!(
                    out,
                    "{} · {} · step {}",
                    d["session_id"].as_str().unwrap_or("session"),
                    state,
                    d["current_step"].as_str().unwrap_or("—")
                )?;
            } else {
                writeln!(out, "{}", preview(&d.to_string()))?;
            }
        }
        render_common(env, out)
    }
}
fn get_refs(s: &ArgMatches) -> Vec<String> {
    s.get_many::<String>("evidence")
        .map(|v| v.cloned().collect())
        .unwrap_or_default()
}
pub fn duration_ms(s: &str) -> Result<u64, UsageError> {
    let (n, f) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60_000)
    } else {
        (s, 1000)
    };
    n.parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(f))
        .filter(|n| *n > 0 && *n <= MAX_WAIT_MS)
        .ok_or_else(|| UsageError::new("duration must be positive and <=1h"))
}
/// CLI host calls this before daemon dispatch. Local plan check/show do not need loomd.
pub fn run_local(
    call: &RpcCall,
    commands: Option<&dyn loom_test_format::CommandTable>,
) -> Option<Envelope<Value>> {
    if !matches!(call.method.as_str(), "test.plan.check" | "test.plan.show")
        || call.params.get("local") != Some(&Value::Bool(true))
    {
        return None;
    }
    let result = (|| {
        let path = text(&call.params, "plan", 4096)?;
        let source = source::load(&json!({"path":path}), ".plan.yaml")?;
        let plan = if let Some(path) = call.params.get("tc").and_then(Value::as_str) {
            let tc = source::load(&json!({"path":path}), ".tc.md")?;
            let tc = loom_test_format::parse_tc(&tc).map_err(source::format_error)?;
            loom_test_format::parse_plan_with_tc(&source, &tc)
        } else {
            loom_test_format::parse_plan(&source)
        }
        .map_err(source::format_error)?;
        if let Some(commands) = commands {
            let report = loom_test_format::check_plan(&plan, commands);
            let value = serde_json::to_value(&report).map_err(|e| usage(e.to_string()))?;
            if !report.is_valid() {
                return Err(err(
                    if report.errors.iter().any(|e| e.code == "blocked_command") {
                        ErrorCode::Denied
                    } else {
                        ErrorCode::Usage
                    },
                    "plan check failed",
                )
                .with_detail(value));
            }
            Ok(value)
        } else {
            let report = loom_test_format::inspect_plan(&plan);
            let value = serde_json::to_value(&report).map_err(|e| usage(e.to_string()))?;
            if !report.errors.is_empty() {
                return Err(usage("structural plan check failed").with_detail(value));
            }
            Ok(value)
        }
    })();
    Some(match result {
        Ok(v) => Envelope::success(&call.method, v, Meta::default(), OthersSinceLast::default()),
        Err(e) => Envelope::failure(
            &call.method,
            ErrorBody {
                code: e.code,
                message: e.message,
                hint: None,
                retryable: false,
                request_id: None,
                detail: e.detail,
            },
            Meta::default(),
            OthersSinceLast::default(),
        ),
    })
}
