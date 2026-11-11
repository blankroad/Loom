//! `loom` binary against a real `loomd` (autostarted into a temp `LOOM_HOME`). P1 C1 tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use loom_proto::envelope::Envelope;
use serde_json::Value;

/// `loomd` next to `loom`; built on demand when only this package was built.
fn loomd() -> PathBuf {
    static P: OnceLock<PathBuf> = OnceLock::new();
    P.get_or_init(|| {
        let loom = PathBuf::from(env!("CARGO_BIN_EXE_loom"));
        let p = loom.with_file_name(if cfg!(windows) { "loomd.exe" } else { "loomd" });
        let release = loom.parent().is_some_and(|d| d.ends_with("release"));
        let ok = Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "loom-gateway", "--bin", "loomd"])
            .args(release.then_some("--release"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok && p.is_file(), "loomd not built at {}", p.display());
        p
    })
    .clone()
}

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        Home {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn path(&self) -> &Path {
        self.dir.path()
    }
    fn loom(&self, args: &[&str], env: &[(&str, &str)]) -> (u8, String, String) {
        let mut c = Command::new(env!("CARGO_BIN_EXE_loom"));
        c.args(args)
            .env("LOOM_HOME", self.path())
            .env("LOOM_DAEMON_BIN", loomd())
            .env("LOOM_CLIENT", "cli-test")
            .env_remove("LOOM_NO_AUTOSTART");
        for (k, v) in env {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        let code = u8::try_from(o.status.code().unwrap_or(255)).unwrap_or(255);
        (
            code,
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    }
    fn json(&self, args: &[&str]) -> (u8, Value) {
        let mut a = vec!["--json"];
        a.extend_from_slice(args);
        let (code, out, err) = self.loom(&a, &[]);
        let v = serde_json::from_str(out.lines().last().unwrap_or(""))
            .unwrap_or_else(|e| panic!("{e}: {out} / {err}"));
        (code, v)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = self.loom(&["daemon", "stop"], &[]);
    }
}

fn has_warning(v: &Value, code: &str) -> bool {
    v["warnings"]
        .as_array()
        .is_some_and(|w| w.iter().any(|x| x["code"] == code))
}

/// Three CLIs at once on a cold home → exactly one daemon, started once.
#[test]
fn autostart_concurrent_one_daemon() {
    let h = Home::new();
    let hp = h.path().to_path_buf();
    let runs: Vec<_> = (0..3)
        .map(|_| {
            let hp = hp.clone();
            std::thread::spawn(move || {
                let o = Command::new(env!("CARGO_BIN_EXE_loom"))
                    .args(["--json", "status"])
                    .env("LOOM_HOME", &hp)
                    .env("LOOM_DAEMON_BIN", loomd())
                    .env_remove("LOOM_NO_AUTOSTART")
                    .output()
                    .unwrap();
                (
                    o.status.code(),
                    serde_json::from_slice::<Value>(&o.stdout).unwrap(),
                )
            })
        })
        .collect();
    let res: Vec<_> = runs.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(
        res.iter().all(|(c, v)| *c == Some(0) && v["ok"] == true),
        "{res:?}"
    );
    let pids: Vec<&Value> = res.iter().map(|(_, v)| &v["data"]["pid"]).collect();
    assert!(pids.windows(2).all(|w| w[0] == w[1]), "one daemon: {pids:?}");
    let started = res
        .iter()
        .filter(|(_, v)| has_warning(v, "daemon_autostarted"))
        .count();
    assert_eq!(started, 1, "exactly one call started it");
    let (_, out, _) = h.loom(&["daemon", "logs", "--tail", "5"], &[]);
    assert!(out.contains("listening"), "{out}");
}

/// `echo send` → exit 0 + envelope; after STOP → exit 6 `E_STOPPED`; no autostart → exit 9.
#[test]
fn echo_send_stop_and_exit_codes() {
    let h = Home::new();
    let (code, v) = h.json(&["echo", "send", "hello"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["ok"], true);
    assert_eq!(v["cmd"], "echo.send");
    assert_eq!(v["data"]["state"], "sent");
    assert!(has_warning(&v, "no_viewer_attached"));
    // envelope schema round trip
    let env: Envelope<Value> = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(&env).unwrap(), v);

    let (code, out, _) = h.loom(&["echo", "send", "human"], &[]);
    assert_eq!(code, 0);
    assert!(out.contains("state=sent"), "{out}");
    let (code, v) = h.json(&["echo", "read", "--tail", "5"]);
    assert_eq!(code, 0);
    assert_eq!(v["data"]["lines"], serde_json::json!(["hello", "human"]));

    let (code, v) = h.json(&["stop-all", "--reason", "test"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["on"], true);
    let (code, v) = h.json(&["echo", "send", "blocked"]);
    assert_eq!(code, 6, "{v}");
    assert_eq!(v["error"]["code"], "E_STOPPED");
    let (code, _, err) = h.loom(&["echo", "send", "blocked"], &[]);
    assert_eq!(code, 6);
    assert!(err.contains("E_STOPPED"), "{err}");

    let cold = Home::new();
    let (code, out, _) = cold.loom(&["--json", "status"], &[("LOOM_NO_AUTOSTART", "1")]);
    assert_eq!(code, 9);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["error"]["code"], "E_DAEMON");
    assert!(!cold.path().join("run/daemon.json").exists(), "nothing started");
    let (code, _, _) = cold.loom(&["confirm", "abc"], &[]);
    assert_eq!(code, 1, "placeholder = E_NOT_IMPLEMENTED");
    let (code, _, _) = cold.loom(&["events", "--max", "5000"], &[]);
    assert_eq!(code, 2, "usage");
}

/// Poll a (peeking) read until it returns `n` events (≤ 3 s).
fn wait_count(h: &Home, args: &[&str], n: u64) {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        if h.json(args).1["data"]["count"].as_u64() == Some(n) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `events` is bounded (`--max`), cursors work, `@last` moves.
#[test]
fn events_bounded_and_cursors() {
    let h = Home::new();
    for i in 0..5 {
        assert_eq!(h.json(&["echo", "send", &format!("m{i}")]).0, 0);
    }
    // IPC worker: rx comes on the data lane, after the reply (only write rows precede it).
    wait_count(&h, &["events", "--peek", "--max", "10", "--kind", "echo.data"], 5);
    let (code, v) = h.json(&["events", "--max", "3", "--kind", "echo.data"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["count"], 3);
    let texts: Vec<&str> = v["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["payload"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["m2", "m3", "m4"], "newest three");

    let (_, v) = h.json(&["events", "--since", "seq:1", "--max", "2"]);
    assert_eq!(v["data"]["count"], 2);
    assert_eq!(v["data"]["truncated"], true);
    let next = v["data"]["next_cursor"].as_str().unwrap().to_owned();
    let (_, v2) = h.json(&["events", "--since", &next, "--max", "2"]);
    assert_eq!(v2["data"]["events"][0]["seq"], "3");

    let (_, v) = h.json(&["mark", "here", "--note", "n"]);
    assert!(v["data"]["seq"].is_string());
    let (_, v) = h.json(&["events", "--since", "mark:here"]);
    assert_eq!(v["data"]["events"][0]["kind"], "core.mark");

    // Earlier reads already moved @last to the high-water: only new events come back.
    let (_, z) = h.json(&["events", "--since", "@last", "--kind", "echo.data"]);
    assert_eq!(z["data"]["count"], 0, "{z}");
    assert_eq!(h.json(&["echo", "send", "new"]).0, 0);
    wait_count(
        &h,
        &["events", "--since", "@last", "--kind", "echo.data", "--peek"],
        1,
    );
    let (_, p) = h.json(&["events", "--since", "@last", "--kind", "echo.data", "--peek"]);
    assert_eq!(p["data"]["count"], 1, "{p}");
    let (_, a) = h.json(&["events", "--since", "@last", "--kind", "echo.data"]);
    assert_eq!(a["data"]["count"], 1, "peek did not move @last: {a}");
    let (_, b) = h.json(&["events", "--since", "@last", "--kind", "echo.data"]);
    assert_eq!(b["data"]["count"], 0, "@last advanced: {b}");
    let (code, v) = h.json(&["events", "--since", "e1:seq:1"]);
    assert_eq!(code, 8, "other epoch = E_CURSOR_GAP: {v}");

    let t0 = Instant::now();
    let (code, out, _) = h.loom(&["events", "--follow", "--timeout", "1"], &[]);
    assert_eq!(code, 0);
    assert!(t0.elapsed() < Duration::from_secs(5));
    let _ = out;
}

/// Help: `loom help --json` and `loom echo help --json` come from the daemon registry (C8).
#[test]
fn help_and_version() {
    let h = Home::new();
    let (code, v) = h.json(&["help"]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["core"].as_array().unwrap().len() >= 5);
    assert!(
        v["data"]["domains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "echo")
    );
    let (code, out, _) = h.loom(&["echo", "help", "--json"], &[]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let methods: Vec<&str> = v["data"]["domains"][0]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["method"].as_str().unwrap())
        .collect();
    assert!(
        methods.contains(&"echo.send") && methods.contains(&"echo.input"),
        "{methods:?}"
    );
    let (code, out, _) = h.loom(&["version"], &[]);
    assert_eq!(code, 0);
    assert!(out.starts_with("loom "));
    let (code, out, _) = h.loom(&["ui"], &[]);
    assert_eq!(code, 0);
    assert!(
        out.contains("Not a terminal") && !out.contains("bootstrap="),
        "no code to agents: {out}"
    );
    let (code, v) = h.json(&["ui", "open", "terminal"]);
    assert_eq!(code, 0, "{v}");
    let (code, v) = h.json(&["doctor"]);
    assert_eq!(code, 0, "{v}");
}

/// CLI fixed cost (not a pass/fail gate): spawn → connect → hello → RPC → exit, 50 runs.
#[test]
fn fixed_cost_measurement() {
    let h = Home::new();
    assert_eq!(h.json(&["status"]).0, 0); // warm: daemon running
    let run = |args: &[&str]| {
        let mut v: Vec<f64> = (0..50)
            .map(|_| {
                let t = Instant::now();
                let (code, _, _) = h.loom(args, &[]);
                assert_eq!(code, 0);
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        v.sort_by(f64::total_cmp);
        (v[24], v[47])
    };
    let local = run(&["version"]);
    let status = run(&["--json", "status"]);
    let send = run(&["--json", "echo", "send", "x"]);
    eprintln!(
        "CLI_COST version(no daemon) p50={:.2}ms p95={:.2}ms",
        local.0, local.1
    );
    eprintln!(
        "CLI_COST status(hello+2 RPC) p50={:.2}ms p95={:.2}ms",
        status.0, status.1
    );
    eprintln!(
        "CLI_COST echo.send(hello+write) p50={:.2}ms p95={:.2}ms",
        send.0, send.1
    );
}

#[cfg(unix)]
fn alive(pid: u64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `loom daemon stop` releases the worker within 1 s (no 30 s orphan hold), the same home restarts
/// at once and sends; a user STOP survives the stop/restart, a plain stop does not latch.
#[cfg(unix)]
#[test]
fn daemon_stop_releases_worker_and_restarts() {
    let h = Home::new();
    assert_eq!(h.json(&["echo", "send", "first"]).0, 0);
    let (_, st) = h.json(&["status"]);
    let pid = st["data"]["workers"][0]["pid"]
        .as_u64()
        .unwrap_or_else(|| panic!("{st}"));
    let (code, v) = h.json(&["daemon", "stop"]);
    assert_eq!(code, 0, "{v}");
    let t = Instant::now();
    while alive(pid) && t.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!alive(pid), "worker gone within 1 s of `daemon stop`");
    let (code, v) = h.json(&["echo", "send", "again"]);
    assert_eq!(code, 0, "immediate restart works: {v}");
    let (_, st) = h.json(&["status"]);
    assert_eq!(st["data"]["latch"], false, "a plain stop does not latch");

    assert_eq!(h.json(&["stop-all", "--reason", "user"]).0, 0);
    assert_eq!(h.json(&["daemon", "stop"]).0, 0);
    let (_, st) = h.json(&["status"]);
    assert_eq!(st["data"]["latch"], true, "user STOP survives the restart");
}

/// A domain registered in both `loomd` and `loom` (uart) runs its sim from the CLI through its own
/// IPC worker instance (`uart.dut`). can/eth/psu are checked over RPC in loom-gateway `tests/r1.rs`
/// (their CLI pieces are opt-in until their crates split out the worker code — I1).
#[test]
fn domains_smoke_via_cli() {
    let h = Home::new();
    let ok = |args: &[&str]| {
        let (code, v) = h.json(args);
        assert_eq!(code, 0, "{args:?}: {v}");
        v
    };
    ok(&["uart", "open", "sim://bootshell", "--as", "dut"]);
    ok(&["uart", "send", "dut", "help"]);
    let end = Instant::now() + Duration::from_secs(5);
    let mut r = Value::Null;
    while Instant::now() < end {
        r = h.json(&["uart", "read", "dut"]).1;
        if r["data"]["lines"].as_array().is_some_and(|l| l.len() > 1) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(r["data"]["lines"].as_array().unwrap().len() > 1, "{r}");
    let (_, st) = h.json(&["status"]);
    let inst: Vec<&str> = st["data"]["workers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["instance"].as_str())
        .collect();
    assert!(inst.contains(&"uart.dut"), "{inst:?}");
}
