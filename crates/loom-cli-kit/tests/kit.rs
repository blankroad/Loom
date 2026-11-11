//! Exit codes, NOTE/WARNING rendering (string snapshots), cursor args, client id, run_cli_with.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io;

use loom_cli_kit::*;
use loom_proto::cursor::Cursor;
use loom_proto::envelope::{Envelope, ErrorBody, Meta, OthersSinceLast, Warning, warnings};
use loom_proto::error::ErrorCode;
use loom_proto::event::Origin;
use loom_proto::num::U64;
use serde_json::{Value, json};

fn body(code: ErrorCode, msg: &str, hint: Option<&str>) -> ErrorBody {
    ErrorBody {
        code,
        message: msg.into(),
        hint: hint.map(Into::into),
        retryable: false,
        request_id: None,
        detail: Value::Null,
    }
}

#[test]
fn exit_codes() {
    let ok = Envelope::success(
        "uart.send",
        json!({}),
        Meta::default(),
        OthersSinceLast::default(),
    );
    assert_eq!(exit_code_for(&ok), 0);
    for (code, exit) in [
        (ErrorCode::Usage, 2),
        (ErrorCode::NoMatch, 3),
        (ErrorCode::Denied, 4),
        (ErrorCode::ApprovalRequired, 5),
        (ErrorCode::Stopped, 6),
        (ErrorCode::NoResponse, 7),
        (ErrorCode::ScanLimit, 8),
        (ErrorCode::Daemon, 9),
        (ErrorCode::Nrc, 10),
        (ErrorCode::TestFailed, 11),
        (ErrorCode::NotImplemented, 1),
    ] {
        let e: Envelope<Value> = Envelope::failure(
            "x.y",
            body(code, "m", None),
            Meta::default(),
            OthersSinceLast::default(),
        );
        assert_eq!(exit_code_for(&e), exit, "{code}");
    }
}

#[test]
fn notes_snapshot() {
    let mut others = OthersSinceLast {
        count: U64(3),
        origins: vec![Origin::User, Origin::Script],
        ..Default::default()
    };
    others.kinds.insert("core.latch".into(), U64(1));
    others.kinds.insert("can.frame".into(), U64(2));
    others.by_channel.insert("can:ch0".into(), U64(2));
    others.by_channel.insert("core".into(), U64(1));
    let mut env: Envelope<Value> = Envelope::failure(
        "can.send",
        body(
            ErrorCode::Stopped,
            "all agent writes are latched by STOP",
            Some("ask the user before continuing"),
        ),
        Meta::default(),
        others,
    );
    env.warnings = vec![
        Warning {
            code: warnings::NO_VIEWER_ATTACHED.into(),
            detail: None,
        },
        Warning {
            code: "future_warning".into(),
            detail: None,
        },
    ];
    let mut out = Vec::new();
    render_common(&env, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "error[E_STOPPED]: all agent writes are latched by STOP\n\
         hint: ask the user before continuing\n\
         NOTE: 3 events by user, script since your last call (can:ch0 2, core 1) — STOP state changed\n\
         WARNING: no viewer attached — the user cannot see this. Ask them to run `loom ui`.\n\
         WARNING: future_warning\n"
    );

    // Nothing to say on a quiet success.
    let ok = Envelope::success("x.y", json!(1), Meta::default(), OthersSinceLast::default());
    let mut out = Vec::new();
    render_common(&ok, &mut out).unwrap();
    assert!(out.is_empty());

    let one = OthersSinceLast {
        count: U64(1),
        origins: vec![Origin::User],
        ..Default::default()
    };
    let env = Envelope::success("x.y", json!(1), Meta::default(), one);
    let mut out = Vec::new();
    render_common(&env, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "NOTE: 1 event by user since your last call\n"
    );
}

#[test]
fn cursor_args() {
    assert_eq!(parse_cursor_arg("@last").unwrap(), Cursor::Last);
    assert_eq!(
        parse_cursor_arg("e3:seq:5").unwrap(),
        Cursor::Seq {
            epoch: Some(3),
            seq: 5
        }
    );
    assert!(parse_cursor_arg("seq:-1").is_err());
    assert!(parse_cursor_arg("").is_err());
}

#[test]
fn client_ids() {
    assert_eq!(
        client_id_from(Some("claude-7f3a"), Some("ctm"), 4121),
        "claude-7f3a#4121"
    );
    assert_eq!(client_id_from(None, Some("ctm"), 1), "anon-ctm#1");
    assert_eq!(client_id_from(Some("bad name!"), Some("c t/m"), 2), "anon-ctm#2");
    assert_eq!(client_id_from(Some(""), None, 3), "anon-user#3");
    assert!(client_id().contains('#'));
}

struct Fake;
impl DomainCli for Fake {
    fn command() -> clap::Command {
        clap::Command::new("fake").subcommand(
            clap::Command::new("send")
                .arg(clap::Arg::new("data").required(true))
                .arg(cursor_arg("since")),
        )
    }
    fn to_call(m: &clap::ArgMatches) -> Result<RpcCall, UsageError> {
        let (_, s) = m.subcommand().ok_or_else(|| UsageError::new("missing verb"))?;
        let data: &String = s.get_one("data").ok_or_else(|| UsageError::new("missing data"))?;
        if data == "bad" {
            return Err(UsageError::new("data may not be 'bad'").with_hint("use another word"));
        }
        let since = s.get_one::<Cursor>("since").map(ToString::to_string);
        Ok(RpcCall::new("fake.send", json!({"data": data, "since": since})))
    }
    fn render_human(_c: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        writeln!(out, "ok={}", env.ok)
    }
}

fn run(args: &[&str]) -> (u8, String, String) {
    let fake = CliOf::<Fake>::new();
    let domains: [&dyn DynDomainCli; 1] = [&fake];
    let args: Vec<String> = std::iter::once("loom")
        .chain(args.iter().copied())
        .map(String::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_cli_with(&args, &domains, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn run_cli_p0_behaviour() {
    let (code, out, _) = run(&["fake", "send", "hello", "--since", "-5s", "--json"]);
    assert_eq!(code, 1);
    let env: Envelope<Value> = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(env.cmd, "fake.send");
    assert_eq!(env.error.unwrap().code, ErrorCode::NotImplemented);

    let (code, _, err) = run(&["fake", "send", "hello"]);
    assert_eq!(code, 1);
    assert!(err.starts_with("error[E_NOT_IMPLEMENTED]"), "{err}");

    let (code, out, _) = run(&["fake", "send", "bad", "--json"]);
    assert_eq!(code, 2);
    assert!(out.contains("\"E_USAGE\"") && out.contains("use another word"));

    let (code, _, err) = run(&["fake", "send", "x", "--since", "seq:01"]);
    assert_eq!(code, 2);
    assert!(err.contains("since"), "{err}");

    let (code, _, _) = run(&["nosuch"]);
    assert_eq!(code, 2);
    assert_eq!(CliOf::<Fake>::new().name(), "fake");
}
