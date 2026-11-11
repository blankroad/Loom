//! WS 수신까지 측정한다. 브라우저 렌더·JSON-RPC·저널은 포함하지 않는다.
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocketUpgrade},
    },
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
#[derive(Serialize, Deserialize)]
struct Event {
    seq: u64,
    mono_ns: u64,
    data: Vec<u8>,
}
#[derive(Clone)]
struct Hub {
    clients: Arc<Mutex<Vec<mpsc::Sender<Message>>>>,
    drops: Arc<AtomicU64>,
    timeouts: Arc<AtomicU64>,
}
fn now() -> u64 {
    unsafe {
        let mut t = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t);
        t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
    }
}
fn usage() -> (f64, i64) {
    unsafe {
        let mut u = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        (
            u.ru_utime.tv_sec as f64
                + u.ru_utime.tv_usec as f64 / 1e6
                + u.ru_stime.tv_sec as f64
                + u.ru_stime.tv_usec as f64 / 1e6,
            u.ru_maxrss,
        )
    }
}
async fn ws(State(h): State<Hub>, w: WebSocketUpgrade) -> Response {
    w.on_upgrade(move |mut sock| async move {
        let (tx, mut rx) = mpsc::channel::<Message>(8);
        h.clients.lock().unwrap().push(tx);
        while let Some(m) = rx.recv().await {
            if !matches!(
                tokio::time::timeout(Duration::from_millis(100), sock.send(m)).await,
                Ok(Ok(()))
            ) {
                h.timeouts.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    })
}
fn pct(v: &mut [u64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.;
    }
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p) as usize] as f64 / 1e6
}

fn encode(codec: &str, e: &[Event]) -> Message {
    match codec {
        "json" => Message::Text(serde_json::to_string(e).unwrap().into()),
        "binary" => Message::Binary(postcard::to_allocvec(e).unwrap().into()),
        "column" => {
            let mut b = b"LMC1".to_vec();
            b.extend_from_slice(&(e.len() as u32).to_le_bytes());
            b.extend_from_slice(&e[0].seq.to_le_bytes());
            for x in e {
                b.extend_from_slice(&x.mono_ns.to_le_bytes());
            }
            for x in e {
                assert_eq!(x.data.len(), 64);
                b.extend_from_slice(&x.data);
            }
            Message::Binary(b.into())
        }
        _ => panic!("알 수 없는 codec"),
    }
}
fn decode(codec: &str, b: &[u8]) -> Vec<Event> {
    match codec {
        "json" => serde_json::from_slice(b).unwrap(),
        "binary" => postcard::from_bytes(b).unwrap(),
        "column" => {
            assert_eq!(&b[..4], b"LMC1");
            let n = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
            let first = u64::from_le_bytes(b[8..16].try_into().unwrap());
            assert_eq!(b.len(), 16 + n * 72);
            (0..n)
                .map(|i| Event {
                    seq: first + i as u64,
                    mono_ns: u64::from_le_bytes(b[16 + i * 8..24 + i * 8].try_into().unwrap()),
                    data: b[16 + n * 8 + i * 64..16 + n * 8 + (i + 1) * 64].to_vec(),
                })
                .collect()
        }
        _ => panic!("알 수 없는 codec"),
    }
}
async fn server(codec: &str, n: usize) {
    let h = Hub {
        clients: Default::default(),
        drops: Default::default(),
        timeouts: Default::default(),
    };
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    println!("{}", l.local_addr().unwrap());
    std::io::stdout().flush().unwrap();
    let app = Router::new().route("/ws", get(ws)).with_state(h.clone());
    let handle = tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    tokio::task::spawn_blocking(|| {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).unwrap();
    })
    .await
    .unwrap();
    while h.clients.lock().unwrap().len() < n {
        tokio::task::yield_now().await;
    }
    let cpu0 = usage().0;
    let start = Instant::now();
    let mut bytes = 0;
    let mut max_frame = 0;
    for b in 0..1500u64 {
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            start + Duration::from_micros(b * 2000),
        ))
        .await;
        let e: Vec<_> = (0..200)
            .map(|i| Event {
                seq: b * 200 + i,
                mono_ns: now(),
                data: (0..64).map(|j| ((b + i + j) % 251) as u8).collect(),
            })
            .collect();
        let msg = encode(codec, &e);
        let len = match &msg {
            Message::Text(s) => s.len(),
            Message::Binary(b) => b.len(),
            _ => 0,
        };
        assert!(len <= 65536);
        bytes += len;
        max_frame = max_frame.max(len);
        for tx in h.clients.lock().unwrap().iter() {
            match tx.try_send(msg.clone()) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    h.drops.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {}
            }
        }
    }
    let wall = start.elapsed().as_secs_f64();
    let cpu_s = usage().0 - cpu0;
    for tx in h.clients.lock().unwrap().drain(..) {
        let _ = tx.try_send(Message::Close(None));
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (_, rss) = usage();
    println!(
        "{}",
        serde_json::json!({"sent_events":300000,"send_events_s":300000.0/wall,"wall_s":wall,"server_cpu_s":cpu_s,"server_rss_peak_bytes_macos":rss,"wire_payload_bytes_per_client":bytes,"max_frame_bytes":max_frame,"queue_dropped_batches":h.drops.load(Ordering::Relaxed),"send_timeouts":h.timeouts.load(Ordering::Relaxed)})
    );
    handle.abort();
}
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let a: Vec<_> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("server") {
        server(&a[2], a[3].parse().unwrap()).await;
        return;
    }
    let codec = a.get(1).cloned().unwrap_or("json".into());
    let n: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let slow = a.get(3).map(String::as_str) == Some("slow");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["server", &codec, &n.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut addr = String::new();
    stdout.read_line(&mut addr).unwrap();
    let mut joins = vec![];
    for i in 0..n {
        let (mut s, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", addr.trim()))
            .await
            .unwrap();
        let codec = codec.clone();
        let is_slow = slow && i == n - 1;
        joins.push(tokio::spawn(async move {
            let mut age=vec![];let mut count=0;let mut last=None;let mut missing=0;let mut dup=0;
            let deadline=tokio::time::Instant::now()+Duration::from_secs(5);
            loop {
                if is_slow {tokio::time::sleep(Duration::from_millis(250)).await;}
                if tokio::time::Instant::now() >= deadline {break;}
                match tokio::time::timeout_at(deadline,s.next()).await {
                    Ok(Some(Ok(m)))=>{if m.is_close(){break;}let e=decode(&codec,&m.into_data());let t=now();count+=e.len();
                        for x in e { if let Some(prev)=last { if x.seq<=prev{dup+=1;}else{missing+=x.seq-prev-1;} }last=Some(x.seq);age.push(t.saturating_sub(x.mono_ns)); }
                    },_=>break,
                }
            }
            serde_json::json!({"received_events":count,"slow":is_slow,"missing_inside_received_range":missing,"duplicates":dup,"p50_ms":pct(&mut age,0.5),"p99_ms":pct(&mut age,0.99)})
        }));
    }
    child.stdin.as_mut().unwrap().write_all(b"start\n").unwrap();
    let mut clients = vec![];
    for j in joins {
        clients.push(j.await.unwrap());
    }
    let mut stats = String::new();
    stdout.read_line(&mut stats).unwrap();
    assert!(child.wait().unwrap().success());
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"codec":codec,"clients":clients,"server":serde_json::from_str::<serde_json::Value>(&stats).unwrap(),"note":"별도 서버 프로세스 RSS/CPU. Rust 클라이언트 수신 지연이며 브라우저 렌더·정본 저널·JSON-RPC 봉투는 제외. column은 고정 64 B 합성 열 형식."})).unwrap());
}
