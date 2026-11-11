//! Synthetic classic pcap -> ReplaySource(Max) -> bounded queue -> consumer.
//! No NIC, native capture, decoding, journal or UI. The virtual file owns only one
//! packet template: the benchmark does not allocate a whole 300 MB input fixture.
use loom_eth_capture::*;
use std::io::{self, Read, Seek, SeekFrom};
use std::time::{Duration, Instant};
/// Finite benchmark input ceiling; increasing runs requires an explicit source edit.
const MAX_BENCH_PACKETS: u64 = 200_000;
/// Template ceiling; reject larger frames before constructing the input.
const MAX_BENCH_FRAME_BYTES: usize = 1500;
#[derive(Debug)]
struct SyntheticPcap {
    header: [u8; 24],
    record: Vec<u8>,
    position: u64,
    total: u64,
}
impl SyntheticPcap {
    fn new(size: usize) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        if !(42..=MAX_BENCH_FRAME_BYTES).contains(&size) {
            return Err("frame size bound".into());
        }
        let b = etherparse::PacketBuilder::ethernet2([2, 0, 0, 0, 0, 1], [2, 0, 0, 0, 0, 2])
            .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 64)
            .udp(13400, 13400);
        let payload = vec![0x55; size - 42];
        let mut data = Vec::with_capacity(size);
        b.write(&mut data, &payload)?;
        let mut header = [0; 24];
        header[..4].copy_from_slice(&0xa1b23c4du32.to_le_bytes());
        header[4..6].copy_from_slice(&2u16.to_le_bytes());
        header[6..8].copy_from_slice(&4u16.to_le_bytes());
        header[16..20].copy_from_slice(&65535u32.to_le_bytes());
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        let mut record = Vec::with_capacity(16 + size);
        for n in [0, 0, size as u32, size as u32] {
            record.extend(n.to_le_bytes());
        }
        record.extend(data);
        let total = 24 + record.len() as u64 * MAX_BENCH_PACKETS;
        Ok(Self {
            header,
            record,
            position: 0,
            total,
        })
    }
}
impl Read for SyntheticPcap {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut copied = 0;
        while copied < out.len() && self.position < self.total {
            let part = if self.position < 24 {
                &self.header[self.position as usize..]
            } else {
                &self.record[((self.position - 24) % self.record.len() as u64) as usize..]
            };
            let n = part
                .len()
                .min(out.len() - copied)
                .min((self.total - self.position) as usize);
            out[copied..copied + n].copy_from_slice(&part[..n]);
            copied += n;
            self.position += n as u64;
        }
        Ok(copied)
    }
}
impl Seek for SyntheticPcap {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let p = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.total) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        if p < 0 || p > i128::from(self.total) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "virtual file seek bounds",
            ));
        }
        self.position = p as u64;
        Ok(self.position)
    }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!(
        "platform={} arch={} release=true input=synthetic_classic_pcap packets_per_size={MAX_BENCH_PACKETS} nic_capture=false",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for size in [64, 1500] {
        let source = ReplaySource::new(SyntheticPcap::new(size)?, ReplayOptions::default())?;
        let start = Instant::now();
        let run = CaptureLoop::start(
            source,
            CaptureConfig {
                queue: QueueConfig {
                    max_frames: 4096,
                    max_bytes: 8 * 1024 * 1024,
                },
                first_seq: 1,
            },
        )?;
        let q = run.queue();
        let mut consumed = 0u64;
        let mut last_seq = 0;
        loop {
            if let Some(frame) = q.pop_timeout(Duration::from_millis(50)) {
                if frame.source_seq <= last_seq {
                    return Err("sequence order".into());
                }
                last_seq = frame.source_seq;
                consumed += 1;
            } else if q.is_closed() {
                break;
            }
            if start.elapsed() > Duration::from_secs(120) {
                run.stop_handle().stop();
                return Err("benchmark exceeded 120 seconds".into());
            }
        }
        let summary = run.join()?;
        let elapsed = start.elapsed();
        let mut drops = 0;
        while let Some(g) = q.try_gap() {
            drops += g.counts.record;
        }
        if summary.frames_seen != MAX_BENCH_PACKETS
            || consumed != summary.frames_enqueued
            || consumed + drops != summary.frames_seen
        {
            return Err("benchmark accounting mismatch".into());
        }
        println!(
            "frame_bytes={size} input={} consumed={consumed} record_drops={drops} elapsed_ms={:.3} consumed_pps={:.0} ingress_pps={:.0} queue_bytes_cap={} queue_frames_cap=4096",
            summary.frames_seen,
            elapsed.as_secs_f64() * 1000.0,
            consumed as f64 / elapsed.as_secs_f64(),
            summary.frames_seen as f64 / elapsed.as_secs_f64(),
            8 * 1024 * 1024
        );
    }
    Ok(())
}
