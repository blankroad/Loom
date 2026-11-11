//! B1 혼합 부하 soak. 측정 대상은 별도 프로세스이며 하네스의 RSS는 섞지 않는다.

mod metrics;
mod process;
mod rpc;
mod state;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_gateway::home::{DaemonInfo, Home};
use serde_json::{Value, json};

use metrics::Sample;
use rpc::Rpc;
use state::State;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum Profile {
    Mixed,
}

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// 부하 시간(분). 소수는 smoke용이며 30분 관문은 리드가 별도 실행한다.
    #[arg(long, default_value_t = 10.0)]
    minutes: f64,
    /// 현재 지원하는 부하 조합.
    #[arg(long,value_enum,default_value_t=Profile::Mixed)]
    profile: Profile,
    /// 시간열과 판정을 저장할 JSON 경로.
    #[arg(long)]
    out: Option<PathBuf>,
}

type Shared = Arc<Mutex<State>>;
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
fn pause_until(stop: &AtomicBool, end: Instant) {
    while !stop.load(Ordering::Acquire) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10).min(end.saturating_duration_since(Instant::now())));
    }
}

fn binary(name: &str, env: &str) -> Result<PathBuf, String> {
    let cargo_key = format!("CARGO_BIN_EXE_{name}");
    if let Some(path) = std::env::var_os(env).or_else(|| std::env::var_os(cargo_key)) {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| format!("{env} 바이너리 없음"));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let sibling = exe.with_file_name(&file);
    if sibling.is_file() {
        return Ok(sibling);
    }
    // cargo test의 deps 하위 실행도 같은 profile의 실제 bin을 찾는다.
    let target = exe.parent().and_then(Path::parent).map(|p| p.join(file));
    target
        .filter(|p| p.is_file())
        .ok_or_else(|| format!("{name} 없음: 같은 target profile에 빌드하거나 {env} 지정"))
}

struct DaemonChild {
    child: Child,
}
impl Drop for DaemonChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_daemon(bin: &Path, home: &Path, log: &Path) -> Result<(DaemonChild, DaemonInfo), String> {
    let err = File::create(log).map_err(|e| e.to_string())?;
    let child = Command::new(bin)
        .args(["--port", "0", "--env", "sim"])
        .env("LOOM_HOME", home)
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(err)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut guard = DaemonChild { child };
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if guard.child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("loomd 준비 전 종료: stderr 로그 확인".into());
        }
        if let Ok(info) = DaemonInfo::read(&Home::at(home))
            && info.pid == guard.child.id()
            && Rpc::connect(&info, None, "soak-ready").is_ok()
        {
            return Ok((guard, info));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("loomd 준비 10초 시간 초과".into())
}

fn writer(
    mut rpc: Rpc,
    state: Shared,
    gate: Arc<Mutex<()>>,
    stop: Arc<AtomicBool>,
    start: Instant,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        // 20 Hz에서도 느린 TCP 구독을 포화시키고 4 MiB 채널 링 밖 질의를 만든다.
        let fill = "x".repeat(16 * 1024);
        let mut id = 0usize;
        while !stop.load(Ordering::Acquire) {
            let next = start + Duration::from_secs_f64(id as f64 / 20.0);
            pause_until(&stop, next);
            if stop.load(Ordering::Acquire) {
                break;
            }
            let _permit = lock(&gate);
            let mut params = json!({"text":format!("soak-agent:{id}:{fill}")});
            if id.is_multiple_of(100) {
                params["_loom"] = json!({"idempotency_key":format!("soak-{id}")});
            }
            let result = rpc.call("echo.send", params.clone(), &mut |_| {});
            let ok = result.is_ok();
            {
                let mut s = lock(&state);
                s.sent(false, id, ok);
                if let Err(e) = &result {
                    s.error("쓰기", e);
                }
            }
            if ok && id.is_multiple_of(100) {
                let result = rpc.call("echo.send", params, &mut |_| {});
                let mut s = lock(&state);
                s.idem_replays += 1;
                if let Err(e) = result {
                    s.idem_failures += 1;
                    s.error("멱등 재요청", &e);
                }
            }
            id += 1;
            // 지연 뒤 무제한 catch-up 폭주를 만들지 않고 계획 슬롯을 건너뛴다.
            let due = (start.elapsed().as_secs_f64() * 20.0).floor() as usize;
            id = id.max(due);
        }
    })
}

fn maintenance(
    mut rpc: Rpc,
    archive_epoch: u64,
    state: Shared,
    stop: Arc<AtomicBool>,
    start: Instant,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut i = 0u64;
        while !stop.load(Ordering::Acquire) {
            let old = i.is_multiple_of(5);
            let params = if old {
                let epoch = if i.is_multiple_of(10) {
                    archive_epoch
                } else {
                    rpc.epoch
                };
                json!({"since":format!("e{epoch}:seq:1"),"filter":{"kinds":["echo.data"]},"max":10,"peek":true})
            } else {
                json!({"max":10,"peek":true})
            };
            let result = rpc.call("core.events.query", params, &mut |_| {});
            {
                let mut s = lock(&state);
                s.query_runs += 1;
                s.old_queries += u64::from(old);
                match result {
                    Ok(v) => {
                        s.journal_queries +=
                            u64::from(v["source"].as_str().is_some_and(|v| v.contains("journal")));
                    }
                    Err(e) => {
                        s.query_failures += 1;
                        s.error("질의", &e);
                    }
                }
            }
            if i.is_multiple_of(10) {
                let result = rpc.call(
                    "core.mark",
                    json!({"label":"soak","note":format!("tick {i}")}),
                    &mut |_| {},
                );
                let mut s = lock(&state);
                if let Err(e) = result {
                    s.error("마커", &e);
                } else {
                    s.marks += 1;
                }
            }
            i += 1;
            pause_until(&stop, start + Duration::from_secs(i));
        }
    })
}

fn reconnect(old: Rpc, info: &DaemonInfo, cookie: &str, view: usize, state: &Shared) -> Result<Rpc, String> {
    let after = {
        let s = lock(state);
        (old.epoch, s.views[view].last_seq)
    };
    drop(old);
    let mut rpc = Rpc::connect(
        info,
        Some(cookie),
        if view == 0 { "soak-normal" } else { "soak-slow" },
    )?;
    {
        let mut s = lock(state);
        let v = &mut s.views[view];
        v.connects += 1;
        v.reconnects += 1;
        v.heartbeat_previous += v.heartbeat_drops;
        v.heartbeat_drops = 0;
    }
    rpc.subscribe(Some(after), &mut |v| lock(state).observe(view, v))?;
    Ok(rpc)
}

fn viewer(
    mut rpc: Rpc,
    credentials: (DaemonInfo, String),
    view: usize,
    state: Shared,
    gate: Arc<Mutex<()>>,
    stop: Arc<AtomicBool>,
    start: Instant,
) -> JoinHandle<()> {
    let (info, cookie) = credentials;
    std::thread::spawn(move || {
        let mut reconnect_at = start + Duration::from_secs(300);
        let mut stop_at = start + Duration::from_secs(60);
        let mut drain_end = None;
        loop {
            let draining = stop.load(Ordering::Acquire);
            if draining {
                let end = drain_end.get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
                if Instant::now() >= *end {
                    break;
                }
            }
            if !draining && Instant::now() >= reconnect_at {
                let next = reconnect(rpc, &info, &cookie, view, &state);
                match next {
                    Ok(r) => rpc = r,
                    Err(e) => {
                        lock(&state).error("viewer 재연결", &e);
                        break;
                    }
                }
                reconnect_at += Duration::from_secs(300);
            }
            if view == 0 && !draining && Instant::now() >= stop_at {
                let _permit = lock(&gate);
                let stop_result = rpc.call(
                    "core.stop",
                    json!({"reason":"임시 soak STOP 시험"}),
                    &mut |v| lock(&state).observe(view, v),
                );
                let release_result = rpc.call("core.stop.release", json!({}), &mut |v| {
                    lock(&state).observe(view, v)
                });
                let mut s = lock(&state);
                match (stop_result, release_result) {
                    (Ok(_), Ok(_)) => s.views[view].stop_pairs += 1,
                    (a, b) => s.error("STOP/release", &format!("{a:?} / {b:?}")),
                }
                stop_at += Duration::from_secs(60);
            }
            // 2초 정지/2초 읽기. 읽는 구간도 초당 4프레임으로 느린 소비자를 유지한다.
            if view == 1 && !draining && start.elapsed().as_secs() % 4 < 2 {
                pause_until(&stop, Instant::now() + Duration::from_millis(50));
                continue;
            }
            match rpc.read() {
                Ok(Some(v)) => lock(&state).observe(view, &v),
                Ok(None) => {}
                Err(e) => {
                    lock(&state).error(
                        if view == 0 {
                            "정상 viewer"
                        } else {
                            "느린 viewer"
                        },
                        &e,
                    );
                    if draining {
                        break;
                    }
                    // 느린 구독이 닫혀도 부하를 끝내지 않고 같은 커서로 다시 구독한다.
                    pause_until(&stop, Instant::now() + Duration::from_millis(500));
                    match reconnect(rpc, &info, &cookie, view, &state) {
                        Ok(next) => rpc = next,
                        Err(e) => {
                            lock(&state).error("viewer 복구 실패", &e);
                            break;
                        }
                    }
                }
            }
            if view == 1 && !draining {
                pause_until(&stop, Instant::now() + Duration::from_millis(250));
            }
        }
    })
}

fn cli_loop(
    bin: PathBuf,
    home: PathBuf,
    scratch: PathBuf,
    state: Shared,
    gate: Arc<Mutex<()>>,
    stop: Arc<AtomicBool>,
    start: Instant,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut id = 0usize;
        while !stop.load(Ordering::Acquire) {
            pause_until(&stop, start + Duration::from_secs((id as u64 + 1) * 10));
            if stop.load(Ordering::Acquire) {
                break;
            }
            let _permit = lock(&gate);
            let result = process::output(
                Command::new(&bin)
                    .args(["--json", "echo", "send", &format!("soak-cli:{id}")])
                    .env("LOOM_HOME", &home)
                    .env("LOOM_NO_AUTOSTART", "1")
                    .env("LOOM_CLIENT", "soak-cli"),
                &scratch,
            );
            let mut s = lock(&state);
            s.sent(true, id, result.is_ok());
            if let Err(e) = result {
                s.error("CLI", &e);
            }
            id += 1;
        }
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}

fn seed_archive(bin: &Path, home: &Path, log: &Path) -> Result<u64, String> {
    // 현재 링에 절대 들어오지 않는 이전 세션을 준비해 저널 질의 경로를 보장한다.
    let (mut child, info) = start_daemon(bin, home, log)?;
    let mut rpc = Rpc::connect(&info, None, "soak-seed")?;
    let epoch = rpc.epoch;
    rpc.call("echo.send", json!({"text":"soak-seed"}), &mut |_| {})?;
    rpc.call("core.shutdown", json!({}), &mut |_| {})?;
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("이전 세션 기록 데몬이 비정상 종료됨".into());
            }
            break;
        }
        if Instant::now() >= end {
            return Err("이전 세션 기록 종료 5초 시간 초과".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // M1 daemon epoch는 wall second에서 만들어질 수 있으므로 같은 초 재시작을 피한다.
    let boundary = now();
    while now() == boundary {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(epoch)
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn execute(args: Args) -> Result<Value, String> {
    if cfg!(windows) {
        return Err("Windows RSS/핸들 soak은 P2에서 지원한다".into());
    }
    if !args.minutes.is_finite() || args.minutes <= 0.0 || args.minutes > 1440.0 {
        return Err("minutes는 0 초과 1440 이하의 유한한 수여야 한다".into());
    }
    let seconds = args.minutes * 60.0;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root 없음")?;
    let runs = root.join("target/loom-bench-soak");
    std::fs::create_dir_all(&runs).map_err(|e| e.to_string())?;
    let temp = tempfile::tempdir_in(&runs).map_err(|e| e.to_string())?;
    let daemon_bin = binary("loomd", "LOOMD_BIN")?;
    let cli_bin = binary("loom", "LOOM_BIN")?;
    let unix_time = now();
    let out = args.out.unwrap_or_else(|| {
        root.join(format!(
            "bench/results/soak-{}-{unix_time}.json",
            std::env::consts::OS
        ))
    });
    let log = out.with_extension("daemon.log");
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let home = temp.path().join("home");
    let archive_epoch = seed_archive(&daemon_bin, &home, &out.with_extension("seed-daemon.log"))?;
    let (mut child, info) = start_daemon(&daemon_bin, &home, &log)?;
    let normal_cookie = rpc::cookie(&info, true)?;
    let slow_cookie = rpc::cookie(&info, false)?;
    let state = Arc::new(Mutex::new(State::new(seconds)));
    let gate = Arc::new(Mutex::new(()));
    let stop = Arc::new(AtomicBool::new(false));
    let mut normal = Rpc::connect(&info, Some(&normal_cookie), "soak-normal")?;
    normal.subscribe(None, &mut |v| lock(&state).observe(0, v))?;
    let mut slow = Rpc::connect(&info, Some(&slow_cookie), "soak-slow")?;
    slow.subscribe(None, &mut |v| lock(&state).observe(1, v))?;
    let writer_rpc = Rpc::connect(&info, None, "soak-agent")?;
    let mut query_rpc = Rpc::connect(&info, None, "soak-query")?;
    let initial_status = query_rpc.call("core.status", json!({}), &mut |_| {})?;
    lock(&state).views.iter_mut().for_each(|v| v.connects = 1);
    let start = Instant::now();
    let viewer_stop = Arc::new(AtomicBool::new(false));
    let viewers = vec![
        viewer(
            normal,
            (info.clone(), normal_cookie),
            0,
            state.clone(),
            gate.clone(),
            viewer_stop.clone(),
            start,
        ),
        viewer(
            slow,
            (info.clone(), slow_cookie),
            1,
            state.clone(),
            gate.clone(),
            viewer_stop.clone(),
            start,
        ),
    ];
    let producers = vec![
        writer(writer_rpc, state.clone(), gate.clone(), stop.clone(), start),
        maintenance(query_rpc, archive_epoch, state.clone(), stop.clone(), start),
        cli_loop(
            cli_bin.clone(),
            home.clone(),
            temp.path().to_path_buf(),
            state.clone(),
            gate,
            stop.clone(),
            start,
        ),
    ];
    let mut sampler = process::Sampler::default();
    let mut samples: Vec<Sample> = Vec::new();
    let mut discovery_complete = true;
    let mut tick = 0u64;
    let mut daemon_exited = false;
    while start.elapsed().as_secs_f64() < seconds {
        if child.child.try_wait().map_err(|e| e.to_string())?.is_some() {
            daemon_exited = true;
            break;
        }
        let at = start.elapsed().as_secs_f64();
        samples.extend(sampler.sample(info.pid, at, temp.path()));
        discovery_complete &= sampler.discovery_complete;
        if tick.is_multiple_of(60) {
            let partial = json!({"complete":false,"elapsed_s":at,"sampler_errors":sampler.errors,"workload":lock(&state).json(),"series":samples.iter().map(Sample::json).collect::<Vec<_>>()});
            if let Err(e) = write_json(&out.with_extension("partial.json"), &partial) {
                lock(&state).error("체크포인트", &e);
            }
            let s = lock(&state);
            eprintln!(
                "soak: {:.0}/{seconds:.0}초, 송신 {}, 실패 {}, RSS 표본 {}",
                at,
                s.writes,
                s.write_failures,
                samples.iter().filter(|s| s.rss.is_some()).count()
            );
        }
        tick += 1;
        // 측정이 늦었으면 밀린 1초 tick을 폭주시키지 않는다.
        tick = tick.max(start.elapsed().as_secs());
        pause_until(
            &stop,
            (start + Duration::from_secs(tick)).min(start + Duration::from_secs_f64(seconds)),
        );
    }
    let measured = start.elapsed().as_secs_f64();
    stop.store(true, Ordering::Release);
    for thread in producers {
        if thread.join().is_err() {
            lock(&state).error("하네스 스레드", "panic");
        }
    }
    viewer_stop.store(true, Ordering::Release);
    for thread in viewers {
        if thread.join().is_err() {
            lock(&state).error("viewer 스레드", "panic");
        }
    }
    if let Ok(mut rpc) = Rpc::connect(&info, None, "soak-shutdown") {
        let _ = rpc.call("core.shutdown", json!({}), &mut |_| {});
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && child.child.try_wait().map_err(|e| e.to_string())?.is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut analysis = metrics::analyze(&samples, measured, args.minutes);
    if !discovery_complete {
        analysis["p1_rss_gate"]["verdict"] = json!("판정 불가");
        analysis["p1_rss_gate"]["reason"] =
            json!("프로세스 탐색 실패 또는 32개 추적 상한 초과: worker 측정 완전성 보장 불가");
    }
    if daemon_exited {
        analysis["p1_rss_gate"]["verdict"] = json!("판정 불가");
        analysis["p1_rss_gate"]["reason"] = json!("데몬이 부하 종료 전에 종료됨");
    }
    let workload = lock(&state).json();
    let normal = &workload["viewers"][0];
    let delivery_ok = normal["missing_matches_gap_count"] == true
        && normal["unknown_gap_events"] == 0
        && normal["record_or_source_gaps"] == 0;
    let value = json!({"schema_version":1,"complete":!daemon_exited,"scenario":"soak","profile":"mixed","requested_minutes":args.minutes,
        "elapsed_s":measured,"unix_time":unix_time,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "build_profile":if cfg!(debug_assertions){"debug"}else{"release"},"daemon_binary":daemon_bin,"cli_binary":cli_bin,
        "daemon_pid":info.pid,"daemon_version":info.version,"daemon_initial_status":initial_status,"archive_seed_epoch":archive_epoch,
        "daemon_stderr":log,"daemon_exited_early":daemon_exited,"process_discovery_complete":discovery_complete,
        "sampling":{"rss":"ps -o rss= -p PID (KiB→MiB)","threads":if cfg!(target_os="macos"){"ps -M -p PID -o pid= 행 수"}else{"ps -o nlwp= -p PID"},
            "fd":if cfg!(target_os="macos"){"lsof -nP -a -p PID -F f: 숫자 FD만, 60초 간격"}else{"/proc/PID/fd: 1초 간격"},"interval_s":1,"errors":sampler.errors,
            "expected_ticks":seconds.ceil() as u64,"observed_ticks":tick,"series_limit":((seconds.ceil() as usize+2)*32)},
        "load":{"agent_hz":20,"echo_payload_bytes":16*1024,"idempotency_every":100,"slow_pause_s":2,"slow_read_phase_s":2,"slow_read_frames_per_s":4,
            "reconnect_s":300,"stop_release_s":60,"cli_interval_s":10,"query_interval_s":1,"old_query_interval_s":5,"archive_query_interval_s":10,"mark_interval_s":10},
        "workload":workload,"assessment":{"normal_delivery_consistent":delivery_ok,"slow_gap_observed":lock(&state).views[1].gap_events>0},
        "analysis":analysis,"series":samples.iter().map(Sample::json).collect::<Vec<_>>(),
        "limitations":["P1은 30분 RSS 증가 관문이며 10분 실행은 추세만 제공한다","워밍업 제외 후 10분 미만이면 처음/마지막 5분 창이 겹친다","macOS FD는 60초 간격이며 미측정 시각은 null이다","Windows 자원 측정은 P2","브라우저 렌더와 실제 장비는 측정하지 않는다"]});
    write_json(&out, &value)?;
    let _ = std::fs::remove_file(out.with_extension("partial.json"));
    eprintln!("soak: 결과 {}", out.display());
    Ok(value)
}

pub(crate) fn run(args: Args) -> ExitCode {
    match execute(args) {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value["analysis"]).unwrap_or_default()
            );
            if value["daemon_exited_early"] == true {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("loom-bench soak: {e}");
            ExitCode::FAILURE
        }
    }
}
