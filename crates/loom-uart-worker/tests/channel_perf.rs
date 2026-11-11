//! Every `ChannelCore` method on a full 16 MiB ring stays short (task p1-channel-core: one call
//! < 50 ms in a debug build; reads are cut by their scan budget). Each call is timed three
//! times on equivalent input and the fastest run is compared (filters scheduler noise from
//! parallel builds); all three are printed with `--nocapture`.

use std::time::{Duration, Instant};

use loom_uart_worker::bridge::{LostRange, RxChunk};
use loom_uart_worker::channel::{
    ChannelCore, ExpectFrom, LinesQuery, PatternSpec, Profile, ReadFrom, ReadMode, ReadQuery,
};
use loom_uart_worker::tx::{TxChunk, TxLane, TxOrigin};
use serde_json::Value;

/// Strict budget (plan target) — enforced only with `LOOM_PERF_STRICT=1` on a quiet machine.
const STRICT_LIMIT: Duration = Duration::from_millis(50);
/// Default ceiling: catches unbounded work (seconds) without failing on a loaded shared host.
const LOOSE_LIMIT: Duration = Duration::from_millis(500);

fn limit() -> Duration {
    if std::env::var_os("LOOM_PERF_STRICT").is_some() {
        STRICT_LIMIT
    } else {
        LOOSE_LIMIT
    }
}

fn filler(n: usize, tag: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(n + 128);
    let mut i = 0u64;
    while v.len() < n {
        v.extend_from_slice(
            format!(
                "[{:6}.{:06}] \x1b[0;32m  OK  \x1b[0m {tag}: line {i} of the boot log, some text\r\n",
                i / 1000,
                i % 1000
            )
            .as_bytes(),
        );
        i += 1;
    }
    v.truncate(n);
    v
}

struct Feed {
    core: ChannelCore,
    seq: u64,
    off: u64,
}

impl Feed {
    fn rx(&mut self, bytes: &[u8]) -> loom_uart_worker::channel::Ingest {
        self.seq += 1;
        let c = RxChunk {
            source_seq: self.seq,
            rx_offset: self.off,
            mono_ns: self.seq * 1000,
            wall_ns: 1_700_000_000_000_000_000 + self.seq * 1000,
            bytes: bytes.to_vec(),
        };
        self.off += bytes.len() as u64;
        self.core.on_rx(&c)
    }
}

/// Run `f` three times (fresh input each time via `i`), report and return the fastest.
fn measure<T>(label: &str, report: &mut Vec<(String, Duration)>, mut f: impl FnMut(u64) -> T) -> T {
    let mut best = Duration::MAX;
    let mut all = Vec::new();
    let mut last = None;
    for i in 0..3 {
        let t = Instant::now();
        let r = f(i);
        let d = t.elapsed();
        all.push(format!("{:.2}", d.as_secs_f64() * 1e3));
        best = best.min(d);
        last = Some(r);
    }
    eprintln!(
        "{label:<44} best {:>7.2} ms  (runs {})",
        best.as_secs_f64() * 1e3,
        all.join(", ")
    );
    report.push((label.to_owned(), best));
    match last {
        Some(r) => r,
        None => unreachable!("three runs"),
    }
}

fn num(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).expect("decimal string")
}

#[test]
fn every_method_is_bounded_on_a_full_ring() {
    let mut f = Feed {
        core: ChannelCore::new("dut", Profile::named("linux-shell").expect("profile"), 1),
        seq: 0,
        off: 0,
    };
    let data = filler(17 << 20, "kernel");
    let t = Instant::now();
    for piece in data.chunks(4096) {
        f.rx(piece);
    }
    eprintln!(
        "fill 17 MiB in 4 KiB chunks: {:.0} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let st = f.core.status();
    assert_eq!(st["ring"]["payload_bytes"], 16 << 20, "ring is full");
    let retained = num(&st["retained_from"]);
    assert!(retained > 0, "oldest bytes were evicted");

    let mut m = Vec::new();

    // Worst live matcher: 8 regex waiters starting at the window start re-scan 64 KiB each.
    let live_from = f.off - 60_000;
    for i in 0..8 {
        f.core
            .expect_add(
                100 + i,
                &[PatternSpec::regex(&format!("never{i}[0-9]+x"))],
                ExpectFrom::RxOffset(live_from),
            )
            .expect("register");
    }
    let chunk = filler(16 << 10, "more");
    measure("on_rx 16 KiB, 8 regex waiters (full window)", &mut m, |_| {
        f.rx(&chunk)
    });
    for i in 0..8 {
        f.core.expect_remove(100 + i);
    }
    measure("on_rx 16 KiB, no waiters", &mut m, |_| f.rx(&chunk));
    measure("on_tx 1 KiB", &mut m, |i| {
        f.seq += 1;
        f.core.on_tx(&TxChunk {
            source_seq: f.seq,
            lane: TxLane::Agent,
            origin: TxOrigin::Agent,
            client_id: "agent".into(),
            request_id: i,
            mono_ns: f.seq * 1000,
            bytes: vec![b'x'; 1024],
        })
    });

    let r = measure("read summary over 16 MiB (budget cut)", &mut m, |i| {
        let q = ReadQuery {
            from: ReadFrom::Last,
            ..ReadQuery::default()
        };
        f.core.read(&format!("sum{i}"), &q).expect("read")
    });
    assert!(
        r["skipped"].is_object() && r["scan_limit"]["resume_from"].is_string(),
        "{}",
        r["skipped"]
    );
    measure("read page, hard caps (1000 lines / 64 KiB)", &mut m, |i| {
        let q = ReadQuery {
            from: ReadFrom::RxOffset(retained + i * 100_000),
            mode: ReadMode::Page,
            max_lines: Some(1000),
            max_bytes: Some(65536),
            ..ReadQuery::default()
        };
        f.core.read("page", &q).expect("read")
    });
    measure("read peek from the oldest byte", &mut m, |_| {
        let q = ReadQuery {
            from: ReadFrom::RxOffset(retained),
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        };
        f.core.read("peek", &q).expect("read")
    });
    measure("lines: last 2000 rows", &mut m, |_| {
        f.core
            .lines(&LinesQuery {
                limit: 2000,
                ..LinesQuery::default()
            })
            .expect("lines")
    });
    measure("lines: 2000 rows forward from the oldest", &mut m, |_| {
        f.core
            .lines(&LinesQuery {
                from: Some(retained),
                limit: 2000,
                ..LinesQuery::default()
            })
            .expect("lines")
    });

    // Backfill budgets: one regex, eight regexes, one literal — all end in ScanLimit.
    let retained = num(&f.core.status()["retained_from"]);
    let r = measure("expect_add backfill, 1 regex", &mut m, |i| {
        f.core.expect_add(
            1 + i,
            &[PatternSpec::regex(r"zz[0-9]+qq")],
            ExpectFrom::RxOffset(retained),
        )
    });
    assert!(
        matches!(r, Err(loom_uart_worker::channel::ChannelError::ScanLimit { .. })),
        "{r:?}"
    );
    let eight: Vec<PatternSpec> = (0..8)
        .map(|i| PatternSpec::regex(&format!("zz{i}[0-9]+qq")))
        .collect();
    let _ = measure("expect_add backfill, 8 regexes", &mut m, |i| {
        f.core.expect_add(10 + i, &eight, ExpectFrom::RxOffset(retained))
    });
    let _ = measure("expect_add backfill, literal", &mut m, |i| {
        f.core.expect_add(
            20 + i,
            &[PatternSpec::literal("zzqq")],
            ExpectFrom::RxOffset(retained),
        )
    });
    let started = f.off - 200_000;
    let hit = measure("expect_add backfill hit", &mut m, |i| {
        f.core
            .expect_add(
                30 + i,
                &[PatternSpec::literal("of the boot")],
                ExpectFrom::RxOffset(started),
            )
            .expect("hit")
    })
    .into_iter()
    .next()
    .expect("one hit");
    measure("expect_result", &mut m, |_| f.core.expect_result(&hit, started));
    let far = f.off - 1_000_000;
    let late = loom_uart_worker::channel::MatchHit {
        id: 30,
        pattern_idx: 0,
        start_off: f.off - 10,
        end_off: f.off,
        text: "x".into(),
    };
    measure("run_output over 1 MB of output (budget cut)", &mut m, |_| {
        f.core.run_output(b"dmesg\r", &late, far)
    });
    f.core
        .expect_add(50, &[PatternSpec::regex("nomatch[0-9]")], ExpectFrom::Now)
        .expect("add");
    measure("expect_timeout", &mut m, |_| f.core.expect_timeout(50, far));
    measure("status", &mut m, |_| f.core.status());
    measure("on_lost", &mut m, |_| {
        f.seq += 2;
        let l = LostRange {
            rx_from: f.off,
            rx_to: f.off + 10,
            bytes: 10,
            seq_from: f.seq - 1,
            seq_to: f.seq - 1,
            chunks: 1,
            cause: "reader_bridge_overflow",
        };
        f.off += 10;
        f.core.on_lost(&l)
    });

    let (label, worst) = m.iter().max_by_key(|(_, d)| *d).cloned().expect("measured");
    eprintln!("worst: {label} {:.2} ms", worst.as_secs_f64() * 1e3);
    for (label, d) in &m {
        assert!(
            *d < limit(),
            "{label} took {:.2} ms (limit {:?})",
            d.as_secs_f64() * 1e3,
            limit()
        );
    }
}
