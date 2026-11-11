//! `loom-bench` — latency harness (plan §9.4, task C16).
//!
//! * **core** gate: a real `loomd` (in this process, temp `LOOM_HOME`) gets `echo.send` calls; a
//!   WebSocket viewer subscribed to `echo.data` records when each event arrives. Latency =
//!   viewer receive time − the event's `ts.mono_ns` stamped by the (in-process) echo worker at
//!   host ingest. Both read the **same** `Instant` base (the bench injects its clock), so no
//!   cross-process clock alignment is involved. Profiles: `idle` (one at a time), `burst`
//!   (fixed rate, default 1000 ev/s for 10 s).
//! * **cli** round trip: `loom echo send` as a separate process N times against that daemon.
//!
//! Limits: in-process echo only — worker IPC and cross-process timing come with the IPC host;
//! the viewer is a Rust client (no browser render time).

use std::io::Write as _;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use loom_gateway::daemon::{Caller, Config, Daemon, Running, start};
use loom_gateway::home::Home;
use loom_proto::rpc::{Rights, Role};
use loom_worker_sdk::Clock;
use serde_json::{Value, json};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::{Message, WebSocket};

mod soak;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Scenario {
    Idle,
    Burst,
    Cli,
    All,
}

/// Loom latency harness.
#[derive(Debug, Parser)]
#[command(name = "loom-bench")]
struct Args {
    #[command(subcommand)]
    command: Option<BenchCommand>,
    /// What to run.
    #[arg(long, value_enum, default_value_t = Scenario::All)]
    scenario: Scenario,
    /// idle: number of sends.
    #[arg(long, default_value_t = 200)]
    n: usize,
    /// burst: events per second.
    #[arg(long, default_value_t = 1000)]
    rate: u32,
    /// burst: duration in seconds (≤ 300).
    #[arg(long, default_value_t = 10)]
    secs: u64,
    /// cli: process runs.
    #[arg(long, default_value_t = 30)]
    cli_runs: usize,
    /// Result directory.
    #[arg(long, default_value = "bench/results")]
    out_dir: PathBuf,
}

#[derive(Debug, clap::Subcommand)]
enum BenchCommand {
    /// 별도 loomd 프로세스에 혼합 부하를 걸고 자원 추세를 측정한다.
    Soak(soak::Args),
}

/// Clock shared by the in-process echo worker and the viewer thread.
#[derive(Debug)]
struct BenchClock {
    base: Instant,
}

impl Clock for BenchClock {
    fn mono_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    fn wall_ns(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(0))
            .unwrap_or(0)
    }
    fn clock_id(&self) -> &str {
        "loom-bench"
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn stats(mut v: Vec<f64>, sent: usize) -> Value {
    v.sort_by(f64::total_cmp);
    json!({
        "samples": v.len(),
        "sent": sent,
        "missing": sent.saturating_sub(v.len()),
        "p50_ms": pct(&v, 0.50),
        "p95_ms": pct(&v, 0.95),
        "p99_ms": pct(&v, 0.99),
        "max_ms": v.last().copied().unwrap_or(f64::NAN),
    })
}

/// Viewer on its own thread; returns `text → latency ms` as events arrive.
struct Viewer {
    lat: Arc<Mutex<Vec<(String, f64)>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    h: std::thread::JoinHandle<()>,
}

fn viewer(run: &Running, clock: Arc<BenchClock>) -> Viewer {
    let port = run.addr.port();
    let code = run.daemon.auth.issue_code(Rights::Watch);
    let session = run.daemon.auth.redeem(&code).map(|(s, _)| s).unwrap_or_default();
    let mut req = format!("ws://127.0.0.1:{port}/rpc")
        .into_client_request()
        .unwrap_or_else(|_| unreachable!());
    if let (Ok(c), Ok(o)) = (
        format!("loom_session={session}").parse(),
        format!("http://localhost:{port}").parse(),
    ) {
        req.headers_mut().insert("cookie", c);
        req.headers_mut().insert("origin", o);
    }
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|e| panic!("connect: {e}"));
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    let (mut ws, _): (WebSocket<TcpStream>, _) =
        tungstenite::client::client(req, stream).unwrap_or_else(|e| panic!("ws: {e}"));
    let send = |ws: &mut WebSocket<TcpStream>, id: u32, m: &str, p: Value| {
        let _ = ws.send(Message::Text(
            json!({"jsonrpc": "2.0", "id": id, "method": m, "params": p})
                .to_string()
                .into(),
        ));
    };
    send(
        &mut ws,
        1,
        "core.hello",
        json!({"proto": "loom/1.0", "client_id": "bench-viewer", "want_role": "viewer", "client": {"kind": "bench", "version": "0", "tty": false}}),
    );
    send(
        &mut ws,
        2,
        "core.subscribe",
        json!({"filter": {"kinds": ["echo.data"]}, "snapshot": true, "max_batch_ms": 1}),
    );
    // wait for snapshot_end so nothing is missed
    loop {
        match ws.read() {
            Ok(Message::Text(t)) if t.as_str().contains("core.snapshot_end") => break,
            Ok(_) => {}
            Err(e) => panic!("viewer: {e}"),
        }
    }
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(50)))
        .ok();
    let lat = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (l2, s2) = (lat.clone(), stop.clone());
    let h = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Acquire) {
            let Ok(Message::Text(t)) = ws.read() else { continue };
            let now = clock.mono_ns();
            let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else {
                continue;
            };
            if v["method"] != "core.events" {
                continue;
            }
            let mut g = l2.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for e in v["params"]["events"].as_array().into_iter().flatten() {
                let ts: u64 = e["ts"]["mono_ns"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(now);
                let text = e["payload"]["text"].as_str().unwrap_or_default().to_owned();
                g.push((text, now.saturating_sub(ts) as f64 / 1e6));
            }
        }
    });
    Viewer { lat, stop, h }
}

impl Viewer {
    fn take(&self, prefix: &str) -> Vec<f64> {
        let g = self.lat.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.iter()
            .filter(|(t, _)| t.starts_with(prefix))
            .map(|(_, l)| *l)
            .collect()
    }
    fn wait_for(&self, prefix: &str, n: usize, max: Duration) {
        let end = Instant::now() + max;
        while Instant::now() < end && self.take(prefix).len() < n {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn agent() -> Caller {
    Caller {
        role: Role::Agent,
        rights: None,
        client_id: "bench#1".into(),
        conn_id: 0,
    }
}

async fn send(d: &Arc<Daemon>, text: String) -> bool {
    d.call(&agent(), "echo.send", Some(json!({"text": text})))
        .await
        .is_ok_and(|v| v["ok"] == true)
}

fn idle(rt: &tokio::runtime::Runtime, d: &Arc<Daemon>, v: &Viewer, n: usize) -> Value {
    let t0 = Instant::now();
    let mut ok = 0;
    for i in 0..n {
        if rt.block_on(send(d, format!("idle-{i}"))) {
            ok += 1;
        }
        std::thread::sleep(Duration::from_millis(5)); // quiet gap: the adaptive batcher sends at once
    }
    v.wait_for("idle-", ok, Duration::from_secs(3));
    let mut s = stats(v.take("idle-"), n);
    s["profile"] = json!("idle");
    s["write_errors"] = json!(n - ok);
    s["wall_s"] = json!(t0.elapsed().as_secs_f64());
    s
}

fn burst(rt: &tokio::runtime::Runtime, d: &Arc<Daemon>, v: &Viewer, rate: u32, secs: u64) -> Value {
    let total = usize::try_from(u64::from(rate) * secs).unwrap_or(usize::MAX);
    let t0 = Instant::now();
    let errors = rt.block_on(async {
        let period = Duration::from_secs_f64(1.0 / f64::from(rate.max(1)));
        let mut tick = tokio::time::interval(period);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
        let mut tasks = Vec::with_capacity(total);
        for i in 0..total {
            tick.tick().await;
            let d = d.clone();
            tasks.push(tokio::spawn(async move { send(&d, format!("burst-{i}")).await }));
        }
        let mut err = 0;
        for t in tasks {
            if !t.await.unwrap_or(false) {
                err += 1;
            }
        }
        err
    });
    let send_s = t0.elapsed().as_secs_f64();
    v.wait_for("burst-", total - errors, Duration::from_secs(5));
    let mut s = stats(v.take("burst-"), total);
    s["profile"] = json!("burst");
    s["rate"] = json!(rate);
    s["secs"] = json!(secs);
    s["achieved_rate"] = json!(total as f64 / send_s);
    s["write_errors"] = json!(errors);
    s
}

fn loom_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOOM_BIN") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let p = exe.with_file_name(if cfg!(windows) { "loom.exe" } else { "loom" });
    p.is_file().then_some(p)
}

fn cli(home: &std::path::Path, runs: usize) -> Value {
    let Some(bin) = loom_bin() else {
        return json!({"skipped": "loom binary not found next to loom-bench (cargo build -p loom-cli) or LOOM_BIN"});
    };
    let mut v = Vec::with_capacity(runs);
    let mut failures = 0;
    // one warm-up run (page cache, first connection) is not counted
    let _ = Command::new(&bin)
        .args(["--json", "version"])
        .env("LOOM_HOME", home)
        .output();
    for i in 0..runs {
        let t = Instant::now();
        let st = Command::new(&bin)
            .args(["--json", "echo", "send", &format!("cli-{i}")])
            .env("LOOM_HOME", home)
            .env("LOOM_NO_AUTOSTART", "1")
            .env("LOOM_CLIENT", "bench-cli")
            .output();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match st {
            Ok(o) if o.status.success() => v.push(ms),
            _ => failures += 1,
        }
    }
    let mut s = stats(v, runs);
    s["profile"] = json!("cli echo send");
    s["failures"] = json!(failures);
    s["binary"] = json!(bin);
    s
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    if let Some(BenchCommand::Soak(args)) = args.command {
        return soak::run(args);
    }
    let secs = args.secs.min(300);
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("loom-bench: runtime: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let home = match tempfile::tempdir() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("loom-bench: temp dir: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let clock = Arc::new(BenchClock { base: Instant::now() });
    let mut cfg = Config::new(Home::at(home.path()));
    cfg.port = Some(0);
    cfg.clock = Some(clock.clone());
    let run = match rt.block_on(start(cfg)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("loom-bench: loomd: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let d = run.daemon.clone();
    let v = viewer(&run, clock);
    let mut results = serde_json::Map::new();
    let sc = args.scenario;
    if matches!(sc, Scenario::Idle | Scenario::All) {
        results.insert("core_idle".into(), idle(&rt, &d, &v, args.n));
    }
    if matches!(sc, Scenario::Burst | Scenario::All) {
        results.insert("core_burst".into(), burst(&rt, &d, &v, args.rate, secs));
    }
    if matches!(sc, Scenario::Cli | Scenario::All) {
        results.insert("cli_roundtrip".into(), cli(home.path(), args.cli_runs));
    }
    v.stop.store(true, std::sync::atomic::Ordering::Release);
    let _ = v.h.join();
    d.journal.close();
    run.server.abort();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let scenario = format!("{sc:?}").to_lowercase();
    let out = json!({
        "scenario": scenario,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "date": loom_gateway::journal::date_of(now),
        "unix_time": now,
        "method": "in-process loomd + echo worker; latency = viewer WS receive − echo ts.mono_ns (shared Instant base)",
        "results": results,
    });
    let text = serde_json::to_string_pretty(&out).unwrap_or_default();
    println!("{text}");
    let file = args.out_dir.join(format!(
        "{scenario}-{}-{}.json",
        std::env::consts::OS,
        loom_gateway::journal::date_of(now)
    ));
    match std::fs::create_dir_all(&args.out_dir)
        .and_then(|()| std::fs::File::create(&file))
        .and_then(|mut f| f.write_all(text.as_bytes()))
    {
        Ok(()) => eprintln!("loom-bench: wrote {}", file.display()),
        Err(e) => eprintln!("loom-bench: cannot write {}: {e}", file.display()),
    }
    std::process::ExitCode::SUCCESS
}
