//! CLI piece: `loom echo send <text>` / `loom echo read --tail N [--since CURSOR]`.

use std::io;

use loom_cli_kit::{DomainCli, RpcCall, UsageError, cursor_arg, render_common};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::Envelope;
use serde_json::{Value, json};

/// The `echo` CLI fragment.
#[derive(Debug, Default)]
pub struct EchoCli;

impl DomainCli for EchoCli {
    fn command() -> clap::Command {
        clap::Command::new("echo")
            .about("Simulated loopback device (sample domain)")
            .subcommand_required(true)
            .subcommand(
                clap::Command::new("send")
                    .about("Send text")
                    .arg(clap::Arg::new("text").required(true)),
            )
            .subcommand(
                clap::Command::new("read")
                    .about("Read recent lines (bounded)")
                    .arg(
                        clap::Arg::new("tail")
                            .long("tail")
                            .value_parser(clap::value_parser!(u32).range(1..=1000))
                            .default_value("100"),
                    )
                    .arg(cursor_arg("since")),
            )
    }

    fn to_call(m: &clap::ArgMatches) -> Result<RpcCall, UsageError> {
        match m.subcommand() {
            Some(("send", s)) => {
                let text: &String = s.get_one("text").ok_or_else(|| UsageError::new("missing text"))?;
                Ok(RpcCall::new("echo.send", json!({"text": text})))
            }
            Some(("read", s)) => {
                let tail = s.get_one::<u32>("tail").copied().unwrap_or(100);
                let since = s.get_one::<Cursor>("since").map(ToString::to_string);
                Ok(RpcCall::new("echo.read", json!({"tail": tail, "since": since})))
            }
            _ => Err(UsageError::new("expected `send` or `read`").with_hint("loom echo help --json")),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            match call.method.as_str() {
                "echo.send" => writeln!(
                    out,
                    "sent {} B · state={}",
                    d["written_bytes"],
                    d["state"].as_str().unwrap_or("sent")
                )?,
                _ => {
                    for l in d["lines"].as_array().into_iter().flatten() {
                        writeln!(out, "{}", l.as_str().unwrap_or_default())?;
                    }
                }
            }
        }
        render_common(env, out)
    }
}
