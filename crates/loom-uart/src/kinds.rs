//! UART event kinds and payload schemas — the data contract of the UI panels
//! (`docs/plan/_xteam/uart-ui.md` §2, answers in `docs/impl/requests/ui-uart.md`).
//!
//! Every kind except [`DATA`] is a control/state event and must travel on the priority
//! (never-drop) queue. `uart.data` may be dropped for slow viewers (`display.gap`).

use loom_domain_api::{EventKindSpec, PointerSpec, ValueType};
use schemars::{Schema, json_schema};

/// RX/TX bytes (`dir` rx|tx). Payload `{bytes{encoding,data}, len, rx_offset?}`.
pub const DATA: &str = "uart.data";
/// Write row of one request, updated in place by `request_id` (`payload.write_state`).
pub const WRITE: &str = "uart.write";
/// Channel/port state transitions (plan §3.4).
pub const STATE: &str = "uart.state";
/// Line settings in effect (readback, not the request).
pub const CONFIG: &str = "uart.config";
/// Input ownership (InputArbiter, plan §3.7).
pub const INPUT_OWNER: &str = "uart.input_owner";
/// Per-key acknowledgement for viewer input.
pub const INPUT_ACK: &str = "uart.input_ack";
/// Viewer input that looks like a terminal auto-reply (audit hint, plan §5.5).
pub const INPUT_SUSPECT: &str = "uart.input_suspect";
/// HOLD on/off.
pub const HOLD: &str = "uart.hold";
/// Agent lock on/off.
pub const LOCK: &str = "uart.lock";
/// What the agent is waiting for (deferred send, expect).
pub const AGENT_ACTIVITY: &str = "uart.agent_activity";
/// Bulk transfer progress (M2).
pub const TRANSFER: &str = "uart.transfer";
/// Line signal (break/DTR/RTS, M2).
pub const SIGNAL: &str = "uart.signal";
/// Named position.
pub const MARK: &str = "uart.mark";
/// Result of an expect / send --expect (evidence row).
pub const EXPECT_RESULT: &str = "uart.expect_result";

/// All kinds in registration order.
pub const ALL: [&str; 14] = [
    DATA,
    WRITE,
    STATE,
    CONFIG,
    INPUT_OWNER,
    INPUT_ACK,
    INPUT_SUSPECT,
    HOLD,
    LOCK,
    AGENT_ACTIVITY,
    TRANSFER,
    SIGNAL,
    MARK,
    EXPECT_RESULT,
];

/// Write states a UART write row can carry. Success ends at `sent` (contract 02 §2.3).
pub const WRITE_STATES: [&str; 5] = ["requested", "accepted", "sent", "failed", "outcome_unknown"];

fn bytes_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["encoding"],
        "properties": {
            "encoding": {"enum": ["base64", "hex", "raw_ref"]},
            "data": {"type": "string"},
            "ref": {"type": "string"}
        }
    })
}

fn ptr(path: &str, ty: ValueType) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: None,
        ty,
    }
}

fn spec(kind: &str, payload_schema: Schema, pointers: Vec<PointerSpec>) -> EventKindSpec {
    EventKindSpec {
        kind: kind.into(),
        payload_schema,
        pointers,
        version: 1,
    }
}

fn enum_of(v: &[&str]) -> ValueType {
    ValueType::Enum(v.iter().map(|s| (*s).to_owned()).collect())
}

/// Event kind specs registered by the domain.
pub fn specs() -> Vec<EventKindSpec> {
    let bytes = bytes_schema();
    vec![
        spec(
            DATA,
            json_schema!({
                "type": "object",
                "required": ["bytes", "len"],
                "properties": {
                    "bytes": bytes,
                    "len": {"type": "integer", "minimum": 0},
                    "rx_offset": {"type": "string", "description": "u64 decimal; RX only: offset of the first byte"}
                }
            }),
            vec![
                ptr("/payload/bytes/data", ValueType::String),
                ptr("/payload/len", ValueType::U32),
                ptr("/payload/rx_offset", ValueType::U64String),
            ],
        ),
        spec(
            WRITE,
            json_schema!({
                "type": "object",
                "required": ["write_state"],
                "properties": {
                    "write_state": {"enum": WRITE_STATES},
                    "final": {"type": "boolean"},
                    "bytes": bytes_schema(),
                    "secret": {"type": "object", "properties": {"len": {"type": "integer"}, "sha256": {"type": "string"}}},
                    "text": {"type": "string", "maxLength": 1024},
                    "written_bytes": {"type": "integer", "minimum": 0},
                    "remaining_bytes": {"type": "integer", "minimum": 0},
                    "deferred_ms": {"type": "integer", "minimum": 0},
                    "error": {"type": "object", "properties": {"code": {"type": "string"}, "message": {"type": "string"}}}
                }
            }),
            vec![
                ptr("/payload/write_state", enum_of(&WRITE_STATES)),
                ptr("/payload/written_bytes", ValueType::U32),
            ],
        ),
        spec(
            STATE,
            json_schema!({
                "type": "object",
                "required": ["state"],
                "properties": {
                    "state": {"enum": ["closed", "opening", "open", "released", "disconnected", "matching", "reopening", "ambiguous", "stopped", "closing", "error"]},
                    "path": {"type": "string"},
                    "cause": {"type": "string"},
                    "error": {"type": "string"}
                }
            }),
            vec![ptr(
                "/payload/state",
                enum_of(&[
                    "closed",
                    "opening",
                    "open",
                    "released",
                    "disconnected",
                    "matching",
                    "reopening",
                    "ambiguous",
                    "stopped",
                    "closing",
                    "error",
                ]),
            )],
        ),
        spec(
            CONFIG,
            json_schema!({
                "type": "object",
                "properties": {
                    "baud": {"type": "integer"},
                    "data_bits": {"type": "integer"},
                    "parity": {"enum": ["none", "odd", "even", "mark", "space"]},
                    "stop_bits": {"enum": ["1", "1.5", "2"]},
                    "flow": {"enum": ["none", "software", "hardware"]},
                    "profile": {"type": "string"},
                    "env": {"enum": ["sim", "bench", "vehicle"]}
                }
            }),
            vec![ptr("/payload/baud", ValueType::U32)],
        ),
        spec(
            INPUT_OWNER,
            json_schema!({
                "type": "object",
                "required": ["owner", "dirty", "hold"],
                "properties": {
                    "owner": {"enum": ["none", "user", "agent"]},
                    "viewer": {"type": "string", "description": "conn_id of the writer viewer"},
                    "viewer_label": {"type": "string"},
                    "client": {"type": "string", "description": "agent client id when owner=agent"},
                    "dirty": {"type": "boolean"},
                    "hold": {"type": "boolean"}
                }
            }),
            vec![ptr("/payload/owner", enum_of(&["none", "user", "agent"]))],
        ),
        spec(
            INPUT_ACK,
            json_schema!({
                "type": "object",
                "required": ["input_seq", "state"],
                "properties": {
                    "input_seq": {"type": "string", "description": "u64 decimal, echoed from uart.input"},
                    "state": {"enum": ["sent", "held", "discarded"]},
                    "reason": {"enum": ["other_writer", "agent_lock", "queue_full", "takeover_timeout", "viewer_left", "not_open"]}
                }
            }),
            vec![ptr("/payload/state", enum_of(&["sent", "held", "discarded"]))],
        ),
        spec(
            INPUT_SUSPECT,
            json_schema!({
                "type": "object",
                "properties": {"pattern": {"type": "string"}, "query_seq": {"type": "string"}}
            }),
            vec![ptr("/payload/pattern", ValueType::String)],
        ),
        spec(
            HOLD,
            json_schema!({"type": "object", "required": ["on"], "properties": {"on": {"type": "boolean"}}}),
            vec![ptr("/payload/on", ValueType::Bool)],
        ),
        spec(
            LOCK,
            json_schema!({
                "type": "object",
                "required": ["on"],
                "properties": {
                    "on": {"type": "boolean"},
                    "reason": {"type": "string"},
                    "ttl_ms": {"type": "integer", "minimum": 0},
                    "actor": {"type": "string"}
                }
            }),
            vec![ptr("/payload/on", ValueType::Bool)],
        ),
        spec(
            AGENT_ACTIVITY,
            json_schema!({
                "type": "object",
                "required": ["kind"],
                "properties": {
                    "kind": {"enum": ["deferred_send", "expect", "idle"]},
                    "text": {"type": "string", "maxLength": 320, "description": "escaped preview, at most 80 chars"},
                    "elapsed_ms": {"type": "integer"},
                    "timeout_ms": {"type": "integer"},
                    "request_id": {"type": "string"},
                    "client": {"type": "string"}
                }
            }),
            vec![ptr(
                "/payload/kind",
                enum_of(&["deferred_send", "expect", "idle"]),
            )],
        ),
        spec(
            TRANSFER,
            json_schema!({
                "type": "object",
                "properties": {"done_bytes": {"type": "integer"}, "total_bytes": {"type": "integer"}, "state": {"type": "string"}}
            }),
            vec![ptr("/payload/done_bytes", ValueType::U32)],
        ),
        spec(
            SIGNAL,
            json_schema!({
                "type": "object",
                "required": ["kind"],
                "properties": {"kind": {"enum": ["break", "dtr", "rts"]}, "value": {"type": "boolean"}, "duration_ms": {"type": "integer"}}
            }),
            vec![ptr("/payload/kind", enum_of(&["break", "dtr", "rts"]))],
        ),
        spec(
            MARK,
            json_schema!({"type": "object", "required": ["label"], "properties": {"label": {"type": "string"}, "rx_offset": {"type": "string"}}}),
            vec![ptr("/payload/label", ValueType::String)],
        ),
        spec(
            EXPECT_RESULT,
            json_schema!({
                "type": "object",
                "required": ["matched"],
                "properties": {
                    "matched": {"type": "boolean"},
                    "pattern_index": {"type": "integer"},
                    "text": {"type": "string"},
                    "rx_offset_start": {"type": "string"},
                    "rx_offset_end": {"type": "string"},
                    "cause": {"type": "string"}
                }
            }),
            vec![
                ptr("/payload/matched", ValueType::Bool),
                ptr("/payload/text", ValueType::String),
            ],
        ),
    ]
}
