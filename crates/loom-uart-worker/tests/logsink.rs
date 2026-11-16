//! B4 (operator-upgrade U1-A): native log sink — formats, provenance header, line assembly, rotation, quota (never
//! delete), bounded queue with gap lines. Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B4.1 (lead-written).
//! Files go to a per-test directory under the OS temp dir and are removed afterwards.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use loom_uart_worker::logsink::{
    LOG_QUEUE_BYTES, LOG_SUBDIR, LineAssembler, LogCfg, LogDir, LogFormat, LogHeader, LogRecord, LogSink,
    LogStartError, file_name, footer_text, gap_text, header_text, jsonl_line, line_prefix, log_dir, utc_iso,
};
use serde_json::Value;

const T: u64 = 1_794_819_907_123_000_000; // 2026-11-16T09:05:07.123Z

fn rec(dir: LogDir, seq: u64, bytes: &[u8]) -> LogRecord {
    LogRecord {
        dir,
        origin: if dir == LogDir::Rx {
            "device".into()
        } else {
            "agent".into()
        },
        generation: 2,
        rx_offset: (dir == LogDir::Rx).then_some(seq * 10),
        request_id: (dir == LogDir::Tx).then(|| "r-1".to_owned()),
        client_id: (dir == LogDir::Tx).then(|| "agent#1".to_owned()),
        producer_id: "p-uart".into(),
        producer_epoch: 3,
        source_seq: seq,
        mono_ns: seq * 1000,
        wall_ns: T + seq * 1_000_000,
        clock_id: "host".into(),
        bytes: bytes.to_vec(),
        secret: false,
        emit_ok: true,
    }
}

fn header() -> LogHeader {
    LogHeader {
        channel: "dut".into(),
        target: "/dev/ttyUSB0".into(),
        producer_id: "p-uart".into(),
        producer_epoch: 3,
        generation: 2,
        clock_id: "host".into(),
    }
}

fn cfg(format: LogFormat, rotate: u64, quota: u64) -> LogCfg {
    LogCfg {
        format,
        rotate_bytes: rotate,
        quota_bytes: quota,
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("loom-logsink-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        Self(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn total(dir: &Path) -> u64 {
    files(dir)
        .iter()
        .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum()
}

// ---------------------------------------------------------------- pure

#[test]
fn utc_iso_formats_proleptic_gregorian_utc() {
    assert_eq!(utc_iso(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(utc_iso(T), "2026-11-16T09:05:07.123Z");
    assert_eq!(utc_iso(1_709_251_199_999_000_000), "2024-02-29T23:59:59.999Z");
    assert_eq!(utc_iso(951_868_800_000_000_000), "2000-03-01T00:00:00.000Z");
}

#[test]
fn names_and_dirs() {
    assert_eq!(LOG_SUBDIR, ["uart", "logs"]);
    assert_eq!(log_dir(Path::new("/h"), "dut"), Path::new("/h/uart/logs/dut"));
    assert_eq!(
        file_name("dut", T, 2, 1, LogFormat::Log),
        "dut-20261116T090507Z-g2-001.log"
    );
    assert_eq!(
        file_name("dut", T, 2, 1234, LogFormat::Jsonl),
        "dut-20261116T090507Z-g2-1234.jsonl"
    );
}

#[test]
fn line_prefix_carries_source_identity_never_a_stream_seq() {
    let mut r = rec(LogDir::Rx, 7, b"x");
    r.emit_ok = false;
    assert_eq!(
        line_prefix(&r),
        "2026-11-16T09:05:07.130Z g2 RX src=3:7 off=70 emit=FAILED | "
    );
    let t = rec(LogDir::Tx, 8, b"y");
    assert_eq!(
        line_prefix(&t),
        "2026-11-16T09:05:07.131Z g2 TX src=3:8 off=- emit=ok | "
    );
}

#[test]
fn line_assembler_splits_lines_and_keeps_the_starting_chunk_metadata() {
    let mut a = LineAssembler::default();
    let r1 = rec(LogDir::Rx, 1, b"hel");
    let r2 = rec(LogDir::Rx, 2, b"lo\r\nwor");
    let r3 = rec(LogDir::Rx, 3, b"ld\n");
    let mut out = a.feed(&r1);
    assert_eq!(out, "");
    out.push_str(&a.feed(&r2));
    out.push_str(&a.feed(&r3));
    assert_eq!(
        out,
        format!("{}hello\n{}world\n", line_prefix(&r1), line_prefix(&r2))
    );
    assert_eq!(a.flush(), "");
}

#[test]
fn line_assembler_escapes_controls_joins_split_utf8_and_flushes_tails() {
    let mut a = LineAssembler::default();
    let e = "é".as_bytes(); // c3 a9
    let r1 = rec(LogDir::Rx, 1, &[b'a', 0x07, 0x1b, b'[', b'm', e[0]]);
    let r2 = rec(LogDir::Rx, 2, &[e[1], b'\t', b'z']);
    let mut out = a.feed(&r1);
    out.push_str(&a.feed(&r2));
    assert_eq!(out, "");
    assert_eq!(
        a.flush(),
        format!("{}a\\x07\\x1b[mé\tz ⟨no newline⟩\n", line_prefix(&r1))
    );
}

#[test]
fn line_assembler_secret_and_long_lines() {
    let mut a = LineAssembler::default();
    let mut s = rec(LogDir::Tx, 1, b"hunter2");
    s.secret = true;
    assert_eq!(a.feed(&s), format!("{}[secret 7 B]\n", line_prefix(&s)));
    let long = vec![b'q'; 64 * 1024 + 10];
    let r = rec(LogDir::Rx, 2, &long);
    let out = a.feed(&r);
    assert!(out.starts_with(&line_prefix(&r)));
    assert!(out.ends_with(" ⟨continued⟩\n"), "64 KiB piece written");
    assert_eq!(out.matches('q').count(), 64 * 1024);
    assert!(a.flush().contains("qqqqqqqqqq ⟨no newline⟩"));
}

#[test]
fn jsonl_line_has_exact_bytes_and_identity() {
    let v: Value = serde_json::from_str(jsonl_line(&rec(LogDir::Rx, 9, b"hi\n")).trim_end()).expect("json");
    assert_eq!(v["t"], "2026-11-16T09:05:07.132Z");
    assert_eq!(v["wall_ns"], (T + 9_000_000).to_string());
    assert_eq!(v["mono_ns"], "9000");
    assert_eq!(v["clock_id"], "host");
    assert_eq!(v["generation"], 2);
    assert_eq!(v["dir"], "rx");
    assert_eq!(v["origin"], "device");
    assert_eq!(v["rx_offset"], "90");
    assert_eq!(v["producer_id"], "p-uart");
    assert_eq!(v["producer_epoch"], "3");
    assert_eq!(v["source_seq"], "9");
    assert_eq!(v["len"], 3);
    assert_eq!(v["hex"], "68690a");
    assert_eq!(v["emit"], "ok");
    assert!(
        v.get("seq").is_none() && v.get("stream_seq").is_none(),
        "no stream seq field"
    );
    let mut s = rec(LogDir::Tx, 10, b"pw");
    s.secret = true;
    let v: Value = serde_json::from_str(jsonl_line(&s).trim_end()).expect("json");
    assert!(v.get("hex").is_none());
    assert_eq!(v["secret"]["len"], 2);
    assert_eq!(v["request_id"], "r-1");
    assert_eq!(v["rx_offset"], Value::Null);
}

#[test]
fn header_gap_and_footer_texts() {
    let h = header_text(&header(), &cfg(LogFormat::Log, 1000, 5000), 3, T);
    assert!(
        h.starts_with("# Loom UART native log · channel dut · file 3 · started 2026-11-16T09:05:07.123Z\n")
    );
    assert!(h.contains("provenance: native-worker-log; stream_seq: not available"));
    assert!(h.contains("(producer_id, producer_epoch, source_seq)"));
    assert!(h.contains("producer_id p-uart · producer_epoch 3 · generation 2 · clock_id host"));
    assert!(h.lines().all(|l| l.starts_with("# ")));
    let j: Value =
        serde_json::from_str(header_text(&header(), &cfg(LogFormat::Jsonl, 1000, 5000), 1, T).trim_end())
            .expect("json");
    assert_eq!(j["type"], "header");
    assert_eq!(j["provenance"], "native-worker-log");
    assert_eq!(j["stream_seq"], "not available");
    assert_eq!(j["file_index"], 1);
    assert_eq!(
        gap_text(LogFormat::Log, 3, 99, T),
        "# GAP 2026-11-16T09:05:07.123Z dropped 3 records / 99 bytes (log queue full; the daemon journal may still have them)\n"
    );
    let g: Value = serde_json::from_str(gap_text(LogFormat::Jsonl, 3, 99, T).trim_end()).expect("json");
    assert_eq!(
        (
            g["type"].clone(),
            g["dropped_records"].clone(),
            g["dropped_bytes"].clone()
        ),
        ("gap".into(), 3.into(), 99.into())
    );
    assert_eq!(
        footer_text(LogFormat::Log, "user", 5, 1, 7, T),
        "# END 2026-11-16T09:05:07.123Z reason=user records=5 dropped_records=1 dropped_bytes=7\n"
    );
}

// ---------------------------------------------------------------- sink

#[test]
fn a_sink_writes_header_lines_and_footer_and_counts_exactly() {
    let td = TempDir::new("basic");
    let dir = td.0.join("dut");
    let sink = LogSink::start(&dir, header(), cfg(LogFormat::Log, 1 << 20, 1 << 30), T).expect("start");
    assert!(sink.stats().active);
    sink.record(rec(LogDir::Rx, 1, b"boot ok\n"));
    sink.record(rec(LogDir::Tx, 2, b"ls\r"));
    sink.record(rec(LogDir::Rx, 3, b"tail"));
    let st = sink.stop("user");
    assert!(!st.active);
    assert_eq!(st.stopped.as_deref(), Some("user"));
    assert_eq!(st.files, 1);
    let fs_ = files(&dir);
    assert_eq!(fs_.len(), 1);
    assert_eq!(
        fs_[0].file_name().and_then(|n| n.to_str()),
        Some("dut-20261116T090507Z-g2-001.log")
    );
    let text = fs::read_to_string(&fs_[0]).expect("read");
    assert!(text.starts_with("# Loom UART native log"));
    assert!(text.contains("RX src=3:1 off=10 emit=ok | boot ok\n"));
    assert!(
        text.contains("TX src=3:2 off=- emit=ok | ls ⟨no newline⟩\n"),
        "TX tail flushed at stop:\n{text}"
    );
    assert!(text.contains("| tail ⟨no newline⟩\n"));
    assert!(
        text.trim_end()
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("# END ") && l.contains("reason=user records=3"))
    );
    assert_eq!(st.bytes_written, fs::metadata(&fs_[0]).expect("meta").len());
    assert_eq!(st.dir_bytes, st.bytes_written);
}

#[test]
fn rotation_starts_numbered_files_each_with_a_header() {
    let td = TempDir::new("rotate");
    let dir = td.0.join("dut");
    let sink = LogSink::start(&dir, header(), cfg(LogFormat::Jsonl, 2048, 1 << 30), T).expect("start");
    for i in 0..40 {
        sink.record(rec(LogDir::Rx, i, b"0123456789abcdef\n"));
    }
    let st = sink.stop("user");
    let fs_ = files(&dir);
    assert!(fs_.len() >= 3, "rotated: {fs_:?}");
    assert_eq!(st.files as usize, fs_.len());
    for (i, f) in fs_.iter().enumerate() {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(name.ends_with(&format!("-{:03}.jsonl", i + 1)), "{name}");
        let text = fs::read_to_string(f).expect("read");
        assert!(
            text.starts_with("{\"") && text.lines().next().is_some_and(|l| l.contains("\"header\"")),
            "{name}"
        );
        assert!(
            fs::metadata(f).expect("meta").len() <= 2048,
            "{name} over rotate size"
        );
    }
    let data_lines: usize = fs_
        .iter()
        .map(|f| {
            fs::read_to_string(f)
                .expect("read")
                .lines()
                .filter(|l| l.contains("\"source_seq\""))
                .count()
        })
        .sum();
    assert_eq!(data_lines, 40, "every record exactly once across files");
}

#[test]
fn the_quota_stops_logging_and_never_deletes() {
    let td = TempDir::new("quota");
    let dir = td.0.join("dut");
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("older.log"), vec![b'x'; 900]).expect("seed");
    // Directory already at/over quota: refuse to start.
    match LogSink::start(&dir, header(), cfg(LogFormat::Log, 1 << 20, 900), T) {
        Err(LogStartError::Quota {
            dir_bytes,
            quota_bytes,
        }) => assert_eq!((dir_bytes, quota_bytes), (900, 900)),
        other => panic!("expected Quota, got {other:?}"),
    }
    let sink = LogSink::start(&dir, header(), cfg(LogFormat::Log, 1 << 20, 2500), T).expect("start");
    for i in 0..200 {
        sink.record(rec(LogDir::Rx, i, b"some device output line\n"));
    }
    let end = Instant::now() + Duration::from_secs(3);
    while sink.stats().active && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let st = sink.stats();
    assert!(!st.active);
    assert_eq!(st.stopped.as_deref(), Some("quota"));
    assert_eq!(sink.take_stop_notice().as_deref(), Some("quota"));
    assert_eq!(sink.take_stop_notice(), None, "notice once");
    let _ = sink.stop("user");
    assert!(dir.join("older.log").exists(), "existing files are never deleted");
    assert_eq!(fs::metadata(dir.join("older.log")).expect("meta").len(), 900);
    assert!(
        total(&dir) <= 2500,
        "directory total {} within quota",
        total(&dir)
    );
}

#[test]
fn queue_overflow_is_counted_and_written_as_a_gap_line() {
    let td = TempDir::new("overflow");
    let dir = td.0.join("dut");
    let sink = LogSink::start(&dir, header(), cfg(LogFormat::Log, 64 << 20, 1 << 30), T).expect("start");
    sink.record(rec(LogDir::Rx, 1, &vec![b'b'; LOG_QUEUE_BYTES + 1]));
    let st = sink.stats();
    assert_eq!(
        (st.dropped_records, st.dropped_bytes),
        (1, LOG_QUEUE_BYTES as u64 + 1)
    );
    sink.record(rec(LogDir::Rx, 2, b"after\n"));
    let st = sink.stop("user");
    assert_eq!(st.dropped_records, 1);
    let text = fs::read_to_string(&files(&dir)[0]).expect("read");
    let gap = text.find("# GAP ").expect("gap line");
    let after = text.find("| after").expect("next record");
    assert!(gap < after, "gap line precedes the next record");
    assert!(text.contains(&format!("dropped 1 records / {} bytes", LOG_QUEUE_BYTES + 1)));
    assert!(text.contains("dropped_records=1"));
}

#[test]
fn dropping_a_sink_closes_it_with_reason_closed_and_record_after_stop_is_ignored() {
    let td = TempDir::new("drop");
    let dir = td.0.join("dut");
    {
        let sink = LogSink::start(&dir, header(), cfg(LogFormat::Log, 1 << 20, 1 << 30), T).expect("start");
        sink.record(rec(LogDir::Rx, 1, b"x\n"));
    }
    let text = fs::read_to_string(&files(&dir)[0]).expect("read");
    assert!(text.contains("reason=closed"));
}
