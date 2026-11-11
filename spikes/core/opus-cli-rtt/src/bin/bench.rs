// 프로세스 생성 → 종료까지 벽시계 (에이전트가 실제로 기다리는 시간)
use std::{process::{Command, Stdio}, time::Instant};
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { v.sort_by(|a,b| a.partial_cmp(b).unwrap()); v[((v.len() as f64 - 1.0) * p).round() as usize] }
fn main() {
    let dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let pf = std::env::temp_dir().join(format!("opus-cli-rtt-{}.port", std::process::id()));
    let mut srv = Command::new(dir.join("srv")).arg(&pf).spawn().unwrap();
    while !pf.exists() { std::thread::sleep(std::time::Duration::from_millis(5)); }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let n = 300;
    let run = |args: &[&str]| -> Vec<f64> {
        let mut v = vec![];
        for i in 0..(n + 20) {
            let t = Instant::now();
            let ok = Command::new(dir.join("cli")).args(args).stdout(Stdio::null()).status().unwrap().success();
            assert!(ok);
            if i >= 20 { v.push(t.elapsed().as_secs_f64() * 1e3); }
        }
        v
    };
    let pfs = pf.to_str().unwrap();
    let mut full = run(&["--port-file", pfs, "--json", "dut", "reboot\\n"]);
    let mut help = { let mut v = vec![]; for i in 0..(n+20) { let t = Instant::now(); Command::new(dir.join("cli")).arg("--help").stdout(Stdio::null()).status().unwrap(); if i>=20 { v.push(t.elapsed().as_secs_f64()*1e3);} } v };
    let size = std::fs::metadata(dir.join("cli")).unwrap().len();
    println!("{{\"n\":{n},\"cli_bytes\":{size},");
    println!(" \"spawn_help_only_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"p99\":{:.2},\"max\":{:.2}}},", pct(&mut help,0.5), pct(&mut help,0.95), pct(&mut help,0.99), pct(&mut help,1.0));
    println!(" \"spawn_connect_hello_rpc_exit_ms\":{{\"p50\":{:.2},\"p95\":{:.2},\"p99\":{:.2},\"max\":{:.2}}}}}", pct(&mut full,0.5), pct(&mut full,0.95), pct(&mut full,0.99), pct(&mut full,1.0));
    let _ = srv.kill(); let _ = std::fs::remove_file(&pf);
}
