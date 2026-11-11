//! Built-in replay scenarios and fixture-directory replay.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use loom_proto::core_events::LatchPayload;
use loom_proto::event::{
    Dir, GapCount, GapData, Origin, ProducerRange, PublishedRecord, Risk, SeqRange, SourceRecord, kinds,
};
use loom_proto::num::U64;
use serde_json::{Value, json};

use super::hub::ReplayHub as Hub;

/// Which generators run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Scenario {
    /// UART lines, CAN frames (~300 fps), UDS request/response rows (incl. outcome_unknown).
    Normal,
    /// CAN 10k fps synthetic load (subscription caps apply → honest `display.gap`).
    Highload,
    /// display/record/source gaps (incl. `count:"unknown"`) every few seconds, plus normal.
    Gaps,
    /// 1,000 events published before anyone connects (snapshot backlog), plus normal.
    Late,
    /// Periodic priority bursts that overflow the control queue → `core.closed{slow_consumer}`.
    Saturate,
    /// normal + gaps + late.
    All,
}

fn every(speed: f64, ms: u64) -> Duration {
    Duration::from_secs_f64((ms as f64 / 1000.0) / speed.max(0.01))
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn uart_line(hub: &Hub, n: u64) -> SourceRecord {
    let line = format!("[{:>5}.{:03}] boot: line {n}\r\n", n / 10, (n % 10) * 100);
    let mut s = hub.source(
        "uart",
        Some("dut"),
        "uart.data",
        Some(Dir::Rx),
        Origin::Device,
        json!({"bytes": {"encoding": "base64", "data": b64(line.as_bytes())}, "bytes_len": line.len()}),
    );
    s.generation = Some(1);
    s
}

fn can_frame(hub: &Hub, n: u64) -> SourceRecord {
    let id = [0x123u32, 0x1A0, 0x0FD, 0x7E8][(n % 4) as usize];
    let data: Vec<u8> = (0..8).map(|i| (n as u8).wrapping_add(i)).collect();
    let mut s = hub.source(
        "can",
        Some("ch0"),
        "can.frame",
        Some(Dir::Rx),
        Origin::Device,
        json!({"id": id, "ext": false, "fd": false, "dlc": 8, "bytes": {"encoding": "hex", "data": hex_str(&data)}}),
    );
    s.metrics = vec![loom_proto::event::Metric {
        name: "VehSpeed".into(),
        value: (n % 1200) as f64 / 10.0,
        unit: Some("km/h".into()),
    }];
    s
}

fn hex_str(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// UDS request/response rows; every 5th request times out (`outcome_unknown`).
fn uds_txn(hub: &Hub, n: u64) -> Vec<SourceRecord> {
    let rid = format!("replay-uds-{n}");
    let row = |kind: &str, dir: Option<Dir>, origin: Origin, payload: Value| {
        let mut s = hub.source("uds", Some("ecu1"), kind, dir, origin, payload);
        s.request_id = Some(rid.clone());
        s.risk = Some(Risk::R0);
        s.generation = Some(1);
        s
    };
    let mut out = vec![
        row(
            "uds.tx",
            None,
            Origin::Agent,
            json!({"write_state": "requested", "sid": 0x22, "req_hex": "22f190"}),
        ),
        row(
            "uds.tx",
            None,
            Origin::Agent,
            json!({"write_state": "accepted", "sid": 0x22, "req_hex": "22f190"}),
        ),
        row(
            "uds.tx",
            Some(Dir::Tx),
            Origin::Agent,
            json!({"write_state": "sent", "sid": 0x22, "req_hex": "22f190"}),
        ),
    ];
    if n % 5 == 4 {
        out.push(row(
            "uds.timeout",
            None,
            Origin::Agent,
            json!({"write_state": "outcome_unknown", "may_have_executed": false}),
        ));
    } else {
        out.push(row(
            "uds.rx",
            Some(Dir::Rx),
            Origin::Device,
            json!({"write_state": "confirmed", "sid": 0x62, "positive": true, "resp_hex": "62f19057304c30303030", "p2_ms": 2.1}),
        ));
    }
    out
}

fn gap(hub: &Hub, kind: &str, domain: &str, cause: &str, count: GapCount, h: u64) -> SourceRecord {
    let g = GapData {
        seq_range: (kind == kinds::DISPLAY_GAP).then(|| SeqRange {
            from: U64(h.saturating_sub(600)),
            to: U64(h),
        }),
        producer_range: (kind != kinds::DISPLAY_GAP).then(|| ProducerRange {
            producer_id: "replay".into(),
            producer_epoch: U64(1),
            from: U64(h.saturating_sub(10)),
            to: U64(h),
        }),
        count,
        cause: cause.into(),
        bytes: None,
        detail: Value::Null,
    };
    hub.source(
        domain,
        Some("ch0"),
        kind,
        None,
        Origin::Device,
        serde_json::to_value(g).unwrap_or(Value::Null),
    )
}

/// Latch event payload.
pub fn latch_source(hub: &Hub, on: bool, by: &str) -> SourceRecord {
    let p = LatchPayload {
        on,
        by: by.into(),
        origin: Origin::User,
        acked: vec!["replay".into()],
        missing: vec![],
        jobs_stopped: if on {
            vec!["replay-periodic-0x123".into()]
        } else {
            vec![]
        },
        approvals_cancelled: vec![],
        uncancellable: vec![],
        outputs_held: vec![],
        persisted: true,
    };
    let mut s = hub.source(
        "core",
        None,
        kinds::CORE_LATCH,
        None,
        Origin::User,
        serde_json::to_value(p).unwrap_or(Value::Null),
    );
    s.client_id = Some(by.into());
    s
}

/// Publish `n` events immediately (late-join backlog).
pub fn prefill(hub: &Hub, n: u64) {
    let srcs = (0..n)
        .map(|i| {
            if i % 2 == 0 {
                uart_line(hub, i)
            } else {
                can_frame(hub, i)
            }
        })
        .collect();
    hub.publish(srcs);
}

fn spawn_normal(hub: Arc<Hub>, speed: f64) {
    let h = hub.clone();
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every(speed, 200));
        for n in 0.. {
            t.tick().await;
            h.publish(vec![uart_line(&h, n)]);
        }
    });
    let h = hub.clone();
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every(speed, 10));
        let mut n = 0u64;
        loop {
            t.tick().await;
            h.publish((0..3).map(|i| can_frame(&h, n + i)).collect());
            n += 3;
        }
    });
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every(speed, 2000));
        for n in 0.. {
            t.tick().await;
            hub.publish(uds_txn(&hub, n));
        }
    });
}

fn spawn_highload(hub: Arc<Hub>, speed: f64) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every(speed, 10));
        let mut n = 0u64;
        loop {
            t.tick().await;
            hub.publish((0..100).map(|i| can_frame(&hub, n + i)).collect());
            n += 100;
        }
    });
}

fn spawn_gaps(hub: Arc<Hub>, speed: f64) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(every(speed, 3000));
        loop {
            t.tick().await;
            let h = hub.hw().await;
            hub.publish(vec![
                gap(
                    &hub,
                    kinds::DISPLAY_GAP,
                    "can", // domain·channel = the affected stream (contract 02, UI C-4)
                    "slow_client",
                    GapCount::Exact(U64(601)),
                    h,
                ),
                gap(
                    &hub,
                    kinds::RECORD_GAP,
                    "can", // domain·channel = the affected stream (contract 02, UI C-4)
                    "crash_tail",
                    GapCount::Unknown,
                    h,
                ),
                gap(
                    &hub,
                    kinds::SOURCE_GAP,
                    "can", // domain·channel = the affected stream (contract 02, UI C-4)
                    "driver_overrun",
                    GapCount::Unknown,
                    h,
                ),
            ]);
        }
    });
}

fn spawn_saturate(hub: Arc<Hub>, speed: f64) {
    tokio::spawn(async move {
        tokio::time::sleep(every(speed, 1000)).await;
        let mut t = tokio::time::interval(every(speed, 5000));
        loop {
            t.tick().await;
            // More priority events than the control queue holds, in one hub turn.
            let n = crate::hub::PRIO_CAP_EVENTS as u64 + 44;
            hub.publish(
                (0..n)
                    .flat_map(|i| uds_txn(&hub, 10_000 + i).into_iter().take(1))
                    .collect(),
            );
        }
    });
}

/// Start generators for a built-in scenario (call inside a tokio runtime).
pub fn start_builtin(hub: Arc<Hub>, scenario: Scenario, speed: f64) {
    match scenario {
        Scenario::Normal => spawn_normal(hub, speed),
        Scenario::Highload => spawn_highload(hub, speed),
        Scenario::Gaps => {
            spawn_normal(hub.clone(), speed);
            spawn_gaps(hub, speed);
        }
        Scenario::Late => {
            prefill(&hub, 1000);
            spawn_normal(hub, speed);
        }
        Scenario::Saturate => {
            spawn_normal(hub.clone(), speed);
            spawn_saturate(hub, speed);
        }
        Scenario::All => {
            prefill(&hub, 1000);
            spawn_normal(hub.clone(), speed);
            spawn_gaps(hub, speed);
        }
    }
}

/// Load `event_*.json` fixtures (single record or array) from `dir`.
pub fn load_dir(dir: &Path) -> std::io::Result<Vec<SourceRecord>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("event_"))
        })
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f)?;
        let v: Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        let items = match v {
            Value::Array(a) => a,
            other => vec![other],
        };
        for i in items {
            let r: PublishedRecord = serde_json::from_value(i).map_err(std::io::Error::other)?;
            out.push(r.into_source());
        }
    }
    Ok(out)
}

/// Replay fixture records in a loop (new seqs, fresh timestamps), one record per 200 ms / speed.
pub fn start_dir(hub: Arc<Hub>, records: Vec<SourceRecord>, speed: f64) {
    tokio::spawn(async move {
        if records.is_empty() {
            return;
        }
        let mut t = tokio::time::interval(every(speed, 200));
        for r in records.iter().cycle() {
            t.tick().await;
            hub.publish(vec![hub.restamp(r.clone())]);
        }
    });
}
