//! Shared test helpers.
#![allow(dead_code, clippy::unwrap_used)]

use std::time::{Duration, Instant};

use loom_journal::{JournalConfig, JournalHandle};
use loom_proto::event::{Dir, Origin, PayloadBytes, SourceRecord, Ts, TsSource};
use loom_proto::num::U64;
use serde_json::json;

pub const PRODUCER: &str = "uart-dut";
pub const EPOCH: u64 = 3;

/// A realistic UART rx record; `mono_ns = seq * 1 ms`.
pub fn record(seq: u64) -> SourceRecord {
    SourceRecord {
        schema_version: 1,
        session_id: "sess-1".into(),
        domain: "uart".into(),
        channel: Some("dut".into()),
        kind: "uart.rx".into(),
        producer_id: PRODUCER.into(),
        producer_epoch: U64(EPOCH),
        source_seq: U64(seq),
        ts: Ts {
            mono_ns: U64(seq * 1_000_000),
            wall_ns: U64(1_700_000_000_000_000_000 + seq),
            clock_id: "boot-1".into(),
            source: TsSource::HostReadComplete,
            quality: None,
        },
        dir: Some(Dir::Rx),
        origin: Origin::Device,
        client_id: None,
        actor_id: None,
        external_actor: None,
        original_origin: None,
        request_id: None,
        job_id: None,
        generation: Some(1),
        purpose: None,
        risk: None,
        test_session: None,
        test_step: None,
        env: None,
        payload: json!({
            "bytes": PayloadBytes::base64(format!("line {seq} ok\r\n").as_bytes()),
            "line": format!("line {seq} ok"),
        }),
        metrics: vec![],
        quality: vec![],
    }
}

/// Fast-timing config for tests.
pub fn config(root: &std::path::Path) -> JournalConfig {
    let mut c = JournalConfig::new(root, PRODUCER, EPOCH);
    c.clock_id = "boot-1".into();
    c.write_interval = Duration::from_millis(2);
    c.sync_interval = Duration::from_millis(20);
    c.retry_interval = Duration::from_millis(50);
    c
}

/// Poll until `f` is true (panics after `timeout`).
pub fn wait_for(timeout: Duration, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + timeout;
    while !f() {
        assert!(Instant::now() < end, "condition not met within {timeout:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Append `range` and return the handle (panics on error).
pub fn append_all(h: &JournalHandle, range: std::ops::Range<u64>) {
    for s in range {
        h.try_append(&record(s)).unwrap();
    }
}
