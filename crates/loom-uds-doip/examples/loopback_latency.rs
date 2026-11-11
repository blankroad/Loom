//! DoIP loopback round-trip latency: client → entity → ACK + response.
//!
//! `cargo run --release -p loom-uds-doip --example loopback_latency`
//! 20 warm-up + 1000 measured requests (`22 F190`), tokio current-thread runtime, 127.0.0.1.
//! Measures submit → response received by the client reader (ACK included). No IPC, UI or TLS.

use std::time::Duration;

use loom_uds_doip::{ClientConfig, DoipClient, DoipEntity, EntityConfig};
use tokio::time::Instant;

const WARMUP: usize = 20;
const SAMPLES: usize = 1000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let entity = DoipEntity::start(
        EntityConfig::loopback(0x1000, vec![0x0E80]),
        |_t, _n, _req: &[u8]| {
            let mut r = vec![0x62, 0xF1, 0x90];
            r.extend_from_slice(b"LOOMSIM0000000001");
            vec![(Duration::ZERO, r)]
        },
    )
    .await?;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), 0x0E00)).await?;
    let mut target = client.open_target(0x0E80)?;
    let mut times = Vec::with_capacity(SAMPLES);
    for i in 0..WARMUP + SAMPLES {
        let start = Instant::now();
        let until = start + Duration::from_secs(1);
        target.send(&[0x22, 0xF1, 0x90], until).await?;
        let rx = target.recv_until(until).await?;
        if i >= WARMUP {
            times.push(rx.at.duration_since(start).as_secs_f64() * 1000.0);
        }
    }
    times.sort_by(f64::total_cmp);
    let pick = |q: f64| {
        times
            .get(((times.len() as f64 * q) as usize).min(times.len() - 1))
            .copied()
    };
    println!(
        "{{\"samples\":{},\"warmup\":{},\"platform\":\"{}-{}\",\"rtt_ms\":{{\"p50\":{:.4},\"p99\":{:.4},\"max\":{:.4}}}}}",
        times.len(),
        WARMUP,
        std::env::consts::OS,
        std::env::consts::ARCH,
        pick(0.50).unwrap_or(f64::NAN),
        pick(0.99).unwrap_or(f64::NAN),
        times.last().copied().unwrap_or(f64::NAN),
    );
    Ok(())
}
