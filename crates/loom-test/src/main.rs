//! Daemon-free local checker; CORE's `loom` host uses the same DomainCli/run_local API.
use loom_cli_kit::DomainCli;
use loom_proto::{
    envelope::{Envelope, ErrorBody, Meta, OthersSinceLast},
    error::ErrorCode,
};
use loom_test::cli::{TestCli, run_local};
use serde_json::Value;
fn main() -> std::process::ExitCode {
    let command = TestCli::command().name("loom-test").arg(
        clap::Arg::new("json")
            .long("json")
            .global(true)
            .action(clap::ArgAction::SetTrue),
    );
    let matches = match command.try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return std::process::ExitCode::from(code);
        }
    };
    let call = match TestCli::to_call(&matches) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let envelope = run_local(&call, None).unwrap_or_else(|| {
        Envelope::<Value>::failure(
            &call.method,
            ErrorBody {
                code: ErrorCode::NotImplemented,
                message: "daemon dispatch belongs to CORE; this executable provides local plan check/show"
                    .into(),
                hint: None,
                retryable: false,
                request_id: None,
                detail: Value::Null,
            },
            Meta::default(),
            OthersSinceLast::default(),
        )
    });
    let code = envelope.exit_code();
    if matches.get_flag("json") {
        match serde_json::to_string(&envelope) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{e}");
                return std::process::ExitCode::from(1);
            }
        }
    } else if let Err(e) = TestCli::render_human(&call, &envelope, &mut std::io::stdout()) {
        eprintln!("{e}");
        return std::process::ExitCode::from(1);
    }
    std::process::ExitCode::from(code)
}
