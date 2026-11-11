//! pty E2E: fake devices on real pty pairs, opened through the same `SerialTransport` path as a
//! real port (UART plan §9.1 M1 "direct access" proof).
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::{Session, bootshell_script};
use loom_uart_worker::bridge::{BRIDGE_MAX_BYTES, BridgeItem, PopError, bridge};
use loom_uart_worker::clock::{SeqSource, SystemClock};
use loom_uart_worker::reader::Reader;
use loom_uart_worker::sim::{
    Fnv64, PtyDevice, PtyHandle, Scenario, SimSpec, binary_frames, flood_bytes, flood_digest,
};
use loom_uart_worker::transport::{self, OpenOpts, PortConfig, SerialTransport, Transport, TransportKind};
use std::sync::Arc;

const FLOOD_TOTAL: usize = 4 * 1024 * 1024;

fn sim_pty_opts() -> OpenOpts {
    OpenOpts {
        sim_pty: true,
        ..OpenOpts::default()
    }
}

fn open_pty(pty: &PtyHandle) -> Box<dyn Transport> {
    let t = transport::open(&pty.slave_path_str(), &PortConfig::default(), &sim_pty_opts())
        .expect("open pty slave via SerialTransport");
    assert_eq!(t.kind(), TransportKind::Pty);
    t
}

#[test]
fn pty_bootshell_login_uname_ping_ctrl_c() {
    let pty = PtyDevice::spawn_spec(&SimSpec::new(Scenario::Bootshell, 1)).expect("spawn pty");
    let mut s = Session::new(open_pty(&pty));
    bootshell_script(&mut s);
}

#[test]
fn pty_binary_all_bytes_both_directions() {
    let mut spec = SimSpec::new(Scenario::Binary, 9);
    spec.frames = 4;
    let pty = PtyDevice::spawn_spec(&spec).expect("spawn pty");
    let mut s = Session::new(open_pty(&pty));
    // device → host: 4 × 0x00..=0xFF, unchanged by the line discipline (CR/LF, ^C, NUL).
    assert_eq!(s.read_exact(1024), binary_frames(4));
    // host → device → host loopback.
    let all: Vec<u8> = (0..=255u8).collect();
    let d = s.send(&all);
    assert_eq!(d.written_bytes, 256);
    assert_eq!(s.read_exact(256), all);
}

#[test]
fn pty_second_open_is_refused_while_exclusive() {
    let pty = PtyDevice::spawn_spec(&SimSpec::new(Scenario::Bootshell, 2)).expect("spawn pty");
    let path = pty.slave_path_str();
    let first = SerialTransport::open(&path, &PortConfig::default(), &sim_pty_opts()).expect("first open");
    let second = SerialTransport::open(&path, &PortConfig::default(), &sim_pty_opts());
    match &second {
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::ResourceBusy, "{e}"),
        Ok(t) => panic!("second exclusive open must fail, got {t:?}"),
    }
    drop(first);
    SerialTransport::open(&path, &PortConfig::default(), &sim_pty_opts()).expect("reopen after close");
}

#[test]
fn pty_open_rejects_unsupported_settings() {
    let pty = PtyDevice::spawn_spec(&SimSpec::new(Scenario::Binary, 0)).expect("spawn pty");
    let cfg = PortConfig {
        parity: transport::Parity::Space,
        ..PortConfig::default()
    };
    let e = SerialTransport::open(&pty.slave_path_str(), &cfg, &sim_pty_opts())
        .expect_err("space parity must be refused");
    assert_eq!(e.kind(), std::io::ErrorKind::Unsupported);
}

fn flood_pty() -> (PtyHandle, Box<dyn Transport>) {
    let spec = SimSpec::parse(&format!("sim://flood?rate=0&bytes={FLOOD_TOTAL}&seed=5")).expect("spec");
    let pty = PtyDevice::spawn_spec(&spec).expect("spawn pty");
    let t = open_pty(&pty);
    (pty, t)
}

#[test]
fn pty_flood_4mib_hash_matches() {
    let (_pty, t) = flood_pty();
    let (btx, rx) = bridge();
    let _reader = Reader::spawn(
        t.rx_handle().expect("rx"),
        btx,
        Arc::new(SeqSource::default()),
        Arc::new(SystemClock::new()),
        0,
    )
    .expect("reader");
    let started = Instant::now();
    let mut hash = Fnv64::new();
    let mut total = 0usize;
    let mut items = Vec::new();
    while total < FLOOD_TOTAL {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "flood too slow: {total} bytes"
        );
        items.clear();
        match rx.pop_batch(&mut items, 64, Duration::from_millis(200)) {
            Ok(_) => {}
            Err(PopError::Timeout) => continue,
            Err(PopError::Closed) => panic!("bridge closed"),
        }
        for item in items.drain(..) {
            match item {
                BridgeItem::Data(c) => {
                    assert_eq!(c.rx_offset, total as u64);
                    hash.update(&c.bytes);
                    total += c.bytes.len();
                }
                other => panic!("unexpected {other:?} at {total}"),
            }
        }
    }
    assert_eq!(total, FLOOD_TOTAL);
    assert_eq!(hash.finish(), flood_digest(0, FLOOD_TOTAL));
    let secs = started.elapsed().as_secs_f64();
    eprintln!(
        "pty flood: {} MiB in {:.3} s = {:.1} MiB/s, wakeups={}",
        FLOOD_TOTAL / (1024 * 1024),
        secs,
        FLOOD_TOTAL as f64 / 1_048_576.0 / secs,
        rx.stats().wakeups
    );
    drop(t);
}

#[test]
fn pty_flood_bridge_overflow_reports_exact_lost_ranges() {
    let (_pty, t) = flood_pty();
    let (btx, rx) = bridge();
    let _reader = Reader::spawn(
        t.rx_handle().expect("rx"),
        btx,
        Arc::new(SeqSource::default()),
        Arc::new(SystemClock::new()),
        0,
    )
    .expect("reader");
    // Consumer deliberately stalled until the reader has read the whole stream.
    let started = Instant::now();
    while rx.stats().pushed_bytes < FLOOD_TOTAL as u64 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "reader too slow: {:?}",
            rx.stats()
        );
        std::thread::sleep(Duration::from_millis(10));
        assert!(rx.queued_cost() <= BRIDGE_MAX_BYTES);
    }
    let stats = rx.stats();
    assert!(stats.dropped_bytes > 0, "4 MiB must overflow a 1 MiB bridge");

    let reference = flood_bytes(0, FLOOD_TOTAL);
    let mut next = 0u64;
    let mut lost_bytes = 0u64;
    let mut lost_items = 0;
    loop {
        match rx.pop(Duration::from_millis(50)) {
            Ok(BridgeItem::Lost(l)) => {
                assert_eq!(l.rx_from, next, "lost range must start where data stopped");
                assert_eq!(l.rx_to - l.rx_from, l.bytes);
                assert_eq!(l.cause, "reader_bridge_overflow");
                assert!(l.seq_from <= l.seq_to);
                next = l.rx_to;
                lost_bytes += l.bytes;
                lost_items += 1;
            }
            Ok(BridgeItem::Data(c)) => {
                assert_eq!(c.rx_offset, next, "data must be contiguous after loss");
                let at = c.rx_offset as usize;
                assert_eq!(&reference[at..at + c.bytes.len()], &c.bytes[..]);
                next += c.bytes.len() as u64;
            }
            Ok(other) => panic!("unexpected {other:?}"),
            Err(_) => break,
        }
    }
    assert_eq!(next, FLOOD_TOTAL as u64, "data + lost must tile the stream");
    assert_eq!(lost_bytes, stats.dropped_bytes);
    assert_eq!(lost_items, 1, "one stalled period ⇒ one merged lost range");
    drop(t);
}
