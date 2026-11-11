//! Contract golden fixtures, embedded from `crates/loom-proto/tests/fixtures` at compile time
//! (single source; nothing is copied into this crate).

use loom_proto::event::PublishedRecord;
use serde_json::Value;

macro_rules! fixtures {
    ($($name:literal),+ $(,)?) => {
        /// All fixtures as `(name, json text)`.
        pub const ALL: &[(&str, &str)] = &[
            $( ($name, include_str!(concat!("../../loom-proto/tests/fixtures/", $name, ".json"))), )+
        ];
    };
}

fixtures!(
    "envelope_error",
    "envelope_ok",
    "envelope_truncated",
    "event_can_frame",
    "event_core_latch",
    "event_display_gap",
    "event_record_gap",
    "event_source_gap",
    "event_uart_rx",
    "event_uart_write_rows",
    "event_uds_rows",
    "hello",
    "hello_result",
    "stream_heartbeat",
    "stream_snapshot_part",
);

/// Raw JSON of a fixture.
pub fn json(name: &str) -> Option<Value> {
    ALL.iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, t)| serde_json::from_str(t).ok())
}

/// Published records of an `event_*` fixture (a single record or an array).
pub fn events(name: &str) -> Vec<PublishedRecord> {
    match json(name) {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Some(v) => serde_json::from_value(v).ok().into_iter().collect(),
        None => Vec::new(),
    }
}
