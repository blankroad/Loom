use crossbeam_channel::{TrySendError, bounded};
use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

// Fixed storage estimate, not a wire/D3 serialization layout.
#[repr(C)]
#[derive(Clone, Copy)]
struct Frame {
    seq: u64,
    stamp_ns: u64,
    metadata: [u8; 48],
    data: [u8; 64],
}

fn percentile(v: &[f64], p: f64) -> f64 {
    v[((v.len() as f64 * p).ceil() as usize)
        .saturating_sub(1)
        .min(v.len() - 1)]
}
fn stats(v: &mut [f64]) -> String {
    v.sort_by(f64::total_cmp);
    format!(
        "{{\"p50_ms\":{:.6},\"p99_ms\":{:.6},\"max_ms\":{:.6}}}",
        percentile(v, 0.5),
        percentile(v, 0.99),
        v[v.len() - 1]
    )
}
fn timing(load: bool, n: usize) -> String {
    let stop = Arc::new(AtomicBool::new(false));
    let competitor = if load {
        let flag = stop.clone();
        Some(thread::spawn(move || {
            let mut x = 1u64;
            while !flag.load(Ordering::Relaxed) {
                for _ in 0..10000 {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                    black_box(x);
                }
            }
        }))
    } else {
        None
    };
    let r = thread::Builder::new().name("can-periodic-probe".into()).spawn(move || {
        let period = Duration::from_millis(10);
        let start = Instant::now();
        let mut deadline = start + period;
        let mut previous = start;
        let mut phase = Vec::with_capacity(n);
        let mut interval = Vec::with_capacity(n);
        let mut skipped = 0;
        for _ in 0..n {
            let now = Instant::now();
            if now < deadline { thread::sleep(deadline-now); }
            let sent_hook = Instant::now();
            phase.push(sent_hook.saturating_duration_since(deadline).as_secs_f64()*1000.0);
            interval.push((sent_hook.duration_since(previous).as_secs_f64()*1000.0-10.0).abs());
            previous = sent_hook;
            deadline += period;
            while deadline <= sent_hook { skipped += 1; deadline += period; }
        }
        format!("{{\"cpu_competitors\":{},\"n\":{},\"elapsed_s\":{:.3},\"skipped\":{},\"deadline_lateness\":{},\"absolute_interval_error\":{}}}",
            usize::from(load), n, start.elapsed().as_secs_f64(), skipped, stats(&mut phase), stats(&mut interval))
    }).unwrap().join().unwrap();
    stop.store(true, Ordering::Relaxed);
    if let Some(c) = competitor {
        c.join().unwrap();
    }
    r
}
fn channel_run(n: u64) -> String {
    let (tx, rx) = bounded::<Frame>(8192);
    let start = Instant::now();
    let consumer = thread::spawn(move || {
        let mut seen = 0;
        let mut latency = Vec::new();
        while let Ok(f) = rx.recv() {
            assert_eq!(f.seq, seen);
            seen += 1;
            if f.seq % 1000 == 0 {
                latency.push((start.elapsed().as_nanos() as u64 - f.stamp_ns) as f64 / 1e6);
            }
            black_box(f.data);
        }
        assert_eq!(seen, n);
        (seen, latency)
    });
    // Blocking synthetic producer: throughput benchmark only, not the proposed driver RX policy.
    for seq in 0..n {
        tx.send(Frame {
            seq,
            stamp_ns: start.elapsed().as_nanos() as u64,
            metadata: [0; 48],
            data: [0; 64],
        })
        .unwrap();
    }
    drop(tx);
    let (seen, mut latency) = consumer.join().unwrap();
    let seconds = start.elapsed().as_secs_f64();
    format!(
        "{{\"frames\":{},\"seconds\":{:.6},\"frames_per_s\":{:.0},\"loss\":0,\"reorder\":0,\"sampled_queue_latency\":{}}}",
        seen,
        seconds,
        seen as f64 / seconds,
        stats(&mut latency)
    )
}
fn main() {
    assert_eq!(std::mem::size_of::<Frame>(), 128);
    let (tx, rx) = bounded::<u64>(8);
    for i in 0..8 {
        tx.try_send(i).unwrap();
    }
    assert!(matches!(tx.try_send(8), Err(TrySendError::Full(8))));
    for i in 0..8 {
        assert_eq!(rx.recv().unwrap(), i);
    }
    let idle = timing(false, 1000);
    let loaded = timing(true, 1000);
    let runs: Vec<_> = (0..5).map(|_| channel_run(1_000_000)).collect();
    println!(
        "{{\"platform\":\"{}-{}\",\"thread_priority\":\"default\",\"timer\":\"std::thread::sleep absolute deadline\",\"frame_bytes\":128,\"channel_capacity\":8192,\"saturation_full_detected\":true,\"timing\":[{},{}],\"channel\":[{}]}}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        idle,
        loaded,
        runs.join(",")
    );
}
