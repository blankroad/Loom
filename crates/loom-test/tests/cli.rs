use loom_cli_kit::{CliOf, DomainCli, root_command};
use loom_proto::{
    envelope::{Envelope, Meta, OthersSinceLast},
    error::ErrorCode,
};
use loom_test::cli::{TestCli, run_local};
use serde_json::{Value, json};
fn parsed_call(args: &[&str]) -> loom_cli_kit::RpcCall {
    let cli = CliOf::<TestCli>::new();
    let matches = root_command(&[&cli]).try_get_matches_from(args).expect("parse");
    TestCli::to_call(matches.subcommand().expect("test").1).expect("call")
}
#[test]
fn cli_commands_and_control_flags_translate_without_a_daemon() {
    for (args, method) in [
        (
            vec![
                "loom",
                "test",
                "start",
                "tc.md",
                "--plan",
                "p.yaml",
                "--wait-approval",
                "3s",
            ],
            "test.start",
        ),
        (vec!["loom", "test", "step", "s", "S1"], "test.step"),
        (vec!["loom", "test", "step", "s", "S1", "--end"], "test.step.end"),
        (vec!["loom", "test", "check", "s", "S1", "--auto"], "test.check"),
        (
            vec![
                "loom",
                "test",
                "verdict",
                "s",
                "S1",
                "pass",
                "--evidence",
                "uart:dut@seq:1",
                "--reason",
                "copied",
            ],
            "test.verdict",
        ),
        (
            vec![
                "loom",
                "test",
                "evidence",
                "add",
                "s",
                "S1",
                "--range",
                "seq:1..seq:2",
                "--filter",
                "{\"domain\":\"uart\",\"kinds\":[\"uart.data\"]}",
                "--pred",
                "{\"op\":\"exists\"}",
            ],
            "test.evidence.add",
        ),
        (vec!["loom", "test", "note", "s", "note"], "test.note"),
        (
            vec![
                "loom",
                "test",
                "ask",
                "s",
                "--question",
                "check?",
                "--options",
                "yes,no",
                "--timeout",
                "5s",
            ],
            "test.ask",
        ),
        (vec!["loom", "test", "answer", "s", "ask-1", "yes"], "test.answer"),
        (
            vec![
                "loom",
                "test",
                "wait",
                "s",
                "--until",
                "answer",
                "--ask-id",
                "a",
                "--timeout",
                "20ms",
            ],
            "test.wait",
        ),
        (
            vec!["loom", "test", "end", "s", "--abort", "--run-finally", "false"],
            "test.end",
        ),
        (vec!["loom", "test", "pause", "s"], "test.pause"),
        (vec!["loom", "test", "resume", "s"], "test.resume"),
        (vec!["loom", "test", "retry", "s", "S1"], "test.retry"),
        (
            vec!["loom", "test", "plan", "amend", "s", "p.yaml"],
            "test.plan.amend",
        ),
        (vec!["loom", "test", "status", "s"], "test.status"),
        (
            vec!["loom", "test", "skip", "s", "S1", "--reason", "user"],
            "test.skip",
        ),
        (vec!["loom", "test", "import", "a.xlsx"], "test.import"),
        (vec!["loom", "test", "report", "s"], "test.report"),
        (vec!["loom", "test", "freeze", "s"], "test.freeze"),
    ] {
        assert_eq!(parsed_call(&args).method, method);
    }
    let start = parsed_call(&[
        "loom",
        "test",
        "start",
        "tc.md",
        "--plan",
        "p.yaml",
        "--wait-approval",
        "3s",
    ]);
    assert_eq!(start.params["tc"], json!({"path":"tc.md"}));
    assert_eq!(start.params["wait_approval_ms"], 3000);
    assert_eq!(start.wait, Some(std::time::Duration::from_secs(5)));
    let check = parsed_call(&["loom", "test", "check", "s", "S1"]);
    assert!(
        check.params.get("evidence").is_none(),
        "omitted refs must retain stored candidates"
    );
}
#[test]
fn local_plan_check_is_real_and_marks_missing_registry_without_fake_risks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../loom-test-format/tests/fixtures/TC-1.plan.yaml");
    let args = parsed_call(&["loom", "test", "plan", "check", path.to_str().expect("UTF-8")]);
    let envelope = run_local(&args, None).expect("handled locally");
    assert!(envelope.ok);
    let data = envelope.data.expect("data");
    assert_eq!(data["command_existence_checked"], false);
    assert!(data.get("risk_summary").is_none());
    assert!(data["warnings"].as_array().expect("warnings").iter().any(|w| {
        w["message"]
            .as_str()
            .is_some_and(|s| s.contains("명령 존재 미검사"))
    }));
    let broken = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../loom-test-format/tests/fixtures/bad-cycle.plan.yaml");
    let args = parsed_call(&["loom", "test", "plan", "check", broken.to_str().expect("path")]);
    let result = run_local(&args, None).expect("local");
    assert_eq!(result.error.expect("cycle").code, ErrorCode::Usage);
}
#[test]
fn renderer_golden_and_json_envelope_keep_session_exit_codes() {
    let call = parsed_call(&["loom", "test", "end", "s"]);
    let e = Envelope::success(
        "test.end",
        json!({"session_id":"s","result":"passed","exit_code":0}),
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    TestCli::render_human(&call, &e, &mut out).expect("render");
    assert_eq!(String::from_utf8(out).expect("UTF-8"), "s · passed · exit 0\n");
    assert_eq!(e.exit_code(), 0);
    let call = parsed_call(&["loom", "test", "status", "s"]);
    let e = Envelope::success(
        "test.status",
        json!({"session_id":"s","state":"PAUSED","current_step":"S2"}),
        Meta::default(),
        OthersSinceLast::default(),
    );
    let mut out = Vec::new();
    TestCli::render_human(&call, &e, &mut out).expect("render");
    assert_eq!(String::from_utf8(out).expect("UTF-8"), "s · PAUSED · step S2\n");
    let bytes = serde_json::to_vec(&e).expect("envelope");
    let roundtrip: Envelope<Value> = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(roundtrip, e);
}
#[test]
fn standalone_checker_executes_without_core_client() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../loom-test-format/tests/fixtures/TC-1.plan.yaml");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom-test"))
        .args(["plan", "check", path.to_str().expect("path"), "--json"])
        .output()
        .expect("checker process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let e: Envelope<Value> = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(e.data.expect("data")["command_existence_checked"], false);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom-test"))
        .args(["report", "s", "--json"])
        .output()
        .expect("process");
    assert_eq!(output.status.code(), Some(1));
    let e: Envelope<Value> = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(e.error.expect("error").code, ErrorCode::NotImplemented);
}
#[test]
fn invalid_cli_waits_and_wrong_syntax_are_usage_errors() {
    assert!(loom_test::cli::duration_ms("0s").is_err());
    assert!(loom_test::cli::duration_ms("2h").is_err());
    assert!(loom_test::cli::duration_ms("3601s").is_err());
    let cli = CliOf::<TestCli>::new();
    for args in [
        vec!["loom", "test", "step", "s", "S1", "--begin", "--end"],
        vec!["loom", "test", "verdict", "s", "S1", "invented"],
        vec!["loom", "test", "ask", "s"],
    ] {
        assert!(root_command(&[&cli]).try_get_matches_from(args).is_err());
    }
}

#[test]
fn plan_show_targets_registered_sessions_and_explicit_local_files() {
    let call = parsed_call(&["loom", "test", "plan", "show", "ts-123"]);
    assert_eq!(call.params, json!({"session":"ts-123"}));
    assert!(run_local(&call, None).is_none());
    let call = parsed_call(&["loom", "test", "plan", "show", "arbitrary-extension", "--local"]);
    assert_eq!(call.params["local"], true);
    let call = parsed_call(&[
        "loom",
        "test",
        "evidence",
        "add",
        "s",
        "S1",
        "--range",
        "{\"epoch\":\"1\",\"from_seq\":\"2\",\"to_seq\":\"3\"}",
    ]);
    assert_eq!(call.params["range"]["epoch"], "1");
}
