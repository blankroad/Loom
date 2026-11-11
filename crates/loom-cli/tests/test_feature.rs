//! TEST local commands must not fall through the M1 placeholder or autostart loomd.
#![cfg(feature = "test")]
#![allow(clippy::unwrap_used)]

#[test]
fn plan_check_runs_locally_without_daemon() {
    let home = tempfile::tempdir().unwrap();
    let plan = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../loom-test-format/tests/fixtures/TC-1.plan.yaml"
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["--json", "test", "plan", "check", plan])
        .env("LOOM_HOME", home.path())
        .env("LOOM_DAEMON_BIN", home.path().join("no-daemon"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], true);
    assert!(!home.path().join("run/daemon.json").exists());
}
