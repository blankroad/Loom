//! 계획 근거용 macOS/Unix 두 프로세스 소켓 측정. 제품 코드가 아니다.
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize, Clone)]
struct Event {
    seq: u64,
    mono_ns: u64,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
}
fn now() -> u64 {
    unsafe {
        let mut t = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
        t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
    }
}
fn cpu() -> f64 {
    unsafe {
        let mut u = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        u.ru_utime.tv_sec as f64
            + u.ru_utime.tv_usec as f64 / 1e6
            + u.ru_stime.tv_sec as f64
            + u.ru_stime.tv_usec as f64 / 1e6
    }
}
fn enc(c: &str, e: &[Event]) -> Vec<u8> {
    match c {
        "postcard" => postcard::to_allocvec(e).unwrap(),
        "bincode2" => bincode::serde::encode_to_vec(e, bincode::config::standard()).unwrap(),
        _ => rmp_serde::to_vec(e).unwrap(),
    }
}
fn dec(c: &str, b: &[u8]) -> Vec<Event> {
    match c {
        "postcard" => postcard::from_bytes(b).unwrap(),
        "bincode2" => {
            bincode::serde::decode_from_slice(b, bincode::config::standard())
                .unwrap()
                .0
        }
        _ => rmp_serde::from_slice(b).unwrap(),
    }
}
fn send(s: &mut UnixStream, b: &[u8]) {
    assert!(b.len() <= 65536);
    s.write_all(&(b.len() as u32).to_le_bytes()).unwrap();
    s.write_all(b).unwrap();
}
fn recv(s: &mut UnixStream) -> Vec<u8> {
    let mut n = [0; 4];
    s.read_exact(&mut n).unwrap();
    let n = u32::from_le_bytes(n) as usize;
    assert!(n <= 65536);
    let mut b = vec![0; n];
    s.read_exact(&mut b).unwrap();
    b
}
fn pct(v: &mut [u64], p: f64) -> f64 {
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p) as usize] as f64 / 1000.0
}
fn worker(path: &str, c: &str) {
    let mut s = UnixStream::connect(path).unwrap();
    for _ in 0..11000 {
        let b = recv(&mut s);
        let e = dec(c, &b);
        send(&mut s, &enc(c, &e));
    }
    let start = Instant::now();
    let cpu0 = cpu();
    for batch in 0..1500 {
        let target = Duration::from_micros(batch * 2000);
        if target > start.elapsed() {
            std::thread::sleep(target - start.elapsed());
        }
        let e: Vec<_> = (0..200)
            .map(|i| Event {
                seq: batch * 200 + i,
                mono_ns: now(),
                data: (0..64).map(|j| ((batch + i + j) % 251) as u8).collect(),
            })
            .collect();
        send(&mut s, &enc(c, &e));
    }
    send(
        &mut s,
        serde_json::to_string(
            &serde_json::json!({"cpu_s":cpu()-cpu0,"wall_s":start.elapsed().as_secs_f64()}),
        )
        .unwrap()
        .as_bytes(),
    );
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("worker") {
        worker(&a[2], &a[3]);
        return;
    }
    let mut rows = vec![];
    for codec in ["postcard", "bincode2", "msgpack"] {
        let path = std::env::current_dir()
            .unwrap()
            .join(format!("ipc-{}.sock", std::process::id()));
        let l = UnixListener::bind(&path).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["worker", path.to_str().unwrap(), codec])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let (mut s, _) = l.accept().unwrap();
        let e = vec![Event {
            seq: 1,
            mono_ns: now(),
            data: vec![42; 64],
        }];
        let b = enc(codec, &e);
        let mut rtt = vec![];
        for i in 0..11000 {
            let t = Instant::now();
            send(&mut s, &b);
            let got = dec(codec, &recv(&mut s));
            assert_eq!(got[0].seq, 1);
            if i >= 1000 {
                rtt.push(t.elapsed().as_nanos() as u64);
            }
        }
        let t = Instant::now();
        let cpu0 = cpu();
        let mut ages = Vec::with_capacity(300000);
        let mut bytes = 0;
        for batch in 0..1500 {
            let b = recv(&mut s);
            bytes += b.len() + 4;
            let es = dec(codec, &b);
            assert_eq!(es.len(), 200);
            let n = now();
            for (i, e) in es.iter().enumerate() {
                assert_eq!(e.seq, batch * 200 + i as u64);
                ages.push(n.saturating_sub(e.mono_ns));
            }
        }
        let wall = t.elapsed().as_secs_f64();
        let parent_cpu = cpu() - cpu0;
        let child_stats: serde_json::Value = serde_json::from_slice(&recv(&mut s)).unwrap();
        assert!(child.wait().unwrap().success());
        std::fs::remove_file(path).unwrap();
        rows.push(serde_json::json!({"codec":codec,"rtt_n":10000,"rtt_p50_us":pct(&mut rtt,0.5),"rtt_p99_us":pct(&mut rtt,0.99),"stream_n":ages.len(),"target_events_s":100000,"achieved_events_s":ages.len() as f64/wall,"stream_p50_us":pct(&mut ages,0.5),"stream_p99_us":pct(&mut ages,0.99),"gateway_cpu_s":parent_cpu,"worker":child_stats,"stream_wire_bytes":bytes,"single_request_bytes":b.len()+4}));
    }
    println!("{}", serde_json::to_string_pretty(&rows).unwrap());
}
