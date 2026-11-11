//! JSON Schema export (contract v1 §7, C3): Rust types → JSON Schema (draft 2020-12) →
//! TS / Python. `loom-schema` (P0 D3–5) writes these to `schema/v1/`.

use std::collections::BTreeMap;

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema};
use serde_json::Value;

use crate::core_events::{
    ActivityPayload, AgentCursorPayload, ApprovalRequestedPayload, ApprovalResolvedPayload, ApprovalRespond,
    LatchPayload, SafeStateItem, SafeStateList, SafeStateRun, SafeStateRunResult,
};
use crate::cursor::Cursor;
use crate::envelope::Envelope;
use crate::error::ErrorCode;
use crate::event::{GapData, PayloadBytes, PublishedRecord, SourceRecord};
use crate::evidence::Evidence;
use crate::rpc::{EventsBatch, Hello, HelloResult, Request, Response, Subscribe, SubscribeResult};
use crate::stream::{
    Closed, Heartbeat, MetricsReport, SnapshotEnd, SnapshotPart, TimeResult, ViewerPresence,
};
use crate::worker::WorkerMsg;

/// `$schema` URI of every exported schema.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Root schema for `T`, pinned to draft 2020-12.
pub fn schema_of<T: JsonSchema>() -> Schema {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
}

/// All contract schemas by file stem (`published_record` → `schema/v1/published_record.schema.json`).
pub fn export_all() -> BTreeMap<&'static str, Schema> {
    let mut m = BTreeMap::new();
    m.insert("published_record", schema_of::<PublishedRecord>());
    m.insert("source_record", schema_of::<SourceRecord>());
    m.insert("payload_bytes", schema_of::<PayloadBytes>());
    m.insert("gap_data", schema_of::<GapData>());
    m.insert("envelope", schema_of::<Envelope<Value>>());
    m.insert("error_code", schema_of::<ErrorCode>());
    m.insert("cursor", schema_of::<Cursor>());
    m.insert("rpc_request", schema_of::<Request>());
    m.insert("rpc_response", schema_of::<Response>());
    m.insert("hello", schema_of::<Hello>());
    m.insert("hello_result", schema_of::<HelloResult>());
    m.insert("subscribe", schema_of::<Subscribe>());
    m.insert("subscribe_result", schema_of::<SubscribeResult>());
    m.insert("events_batch", schema_of::<EventsBatch>());
    m.insert("evidence", schema_of::<Evidence>());
    m.insert("worker_msg", schema_of::<WorkerMsg>());
    m.insert("snapshot_part", schema_of::<SnapshotPart>());
    m.insert("snapshot_end", schema_of::<SnapshotEnd>());
    m.insert("heartbeat", schema_of::<Heartbeat>());
    m.insert("closed", schema_of::<Closed>());
    m.insert("time_result", schema_of::<TimeResult>());
    m.insert("viewer_presence", schema_of::<ViewerPresence>());
    m.insert("metrics_report", schema_of::<MetricsReport>());
    m.insert("latch_payload", schema_of::<LatchPayload>());
    m.insert("agent_cursor_payload", schema_of::<AgentCursorPayload>());
    m.insert("activity_payload", schema_of::<ActivityPayload>());
    m.insert(
        "approval_requested_payload",
        schema_of::<ApprovalRequestedPayload>(),
    );
    m.insert(
        "approval_resolved_payload",
        schema_of::<ApprovalResolvedPayload>(),
    );
    m.insert("approval_respond", schema_of::<ApprovalRespond>());
    m.insert("safe_state_item", schema_of::<SafeStateItem>());
    m.insert("safe_state_list", schema_of::<SafeStateList>());
    m.insert("safe_state_run", schema_of::<SafeStateRun>());
    m.insert("safe_state_run_result", schema_of::<SafeStateRunResult>());
    m
}
