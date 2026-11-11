//! Small helpers shared by the worker modules: error mapping, timestamps, argument parsing.

use std::io;
use std::sync::Arc;

use loom_console::arbiter::ArbiterEvent;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, Env, Origin, Ts, TsSource};
use loom_proto::num::U64;
use loom_proto::worker::WorkerError;
use loom_worker_sdk::{EventDraft, WorkerCtx};
use serde_json::{Value, json};

use crate::tx::{SubmitError, TxDone, TxErrorCode};

/// Event kinds (canonical list: `crates/loom-uart/src/kinds.rs`; the worker does not depend on
/// the gateway crate, so the names are repeated here and checked by the worker tests).
pub(crate) mod kind {
    pub const DATA: &str = "uart.data";
    pub const WRITE: &str = "uart.write";
    pub const STATE: &str = "uart.state";
    pub const CONFIG: &str = "uart.config";
    pub const INPUT_ACK: &str = "uart.input_ack";
    pub const AGENT_ACTIVITY: &str = "uart.agent_activity";
    pub const EXPECT_RESULT: &str = "uart.expect_result";
}

/// `WorkerError` with an empty detail.
pub(crate) fn werr(code: ErrorCode, message: impl Into<String>) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail: Value::Null,
    }
}

/// `WorkerError` with a detail object.
pub(crate) fn werr_detail(code: ErrorCode, message: impl Into<String>, detail: Value) -> WorkerError {
    WorkerError {
        code,
        message: message.into(),
        detail,
    }
}

/// Port open failure → error code (plan §3.4 ERROR codes).
pub(crate) fn open_error(e: &io::Error) -> WorkerError {
    let code = match e.kind() {
        io::ErrorKind::NotFound => ErrorCode::NotFound,
        io::ErrorKind::PermissionDenied => ErrorCode::Permission,
        io::ErrorKind::ResourceBusy | io::ErrorKind::AddrInUse | io::ErrorKind::WouldBlock => {
            ErrorCode::PortBusy
        }
        io::ErrorKind::Unsupported => ErrorCode::Unsupported,
        io::ErrorKind::InvalidInput => ErrorCode::Usage,
        _ => ErrorCode::NotOpen,
    };
    werr(code, format!("open failed: {e}"))
}

/// TX actor error placeholder → error code.
pub(crate) fn tx_error_code(e: &TxErrorCode) -> ErrorCode {
    match e {
        TxErrorCode::Timeout => ErrorCode::Timeout,
        TxErrorCode::InterruptedByUser => ErrorCode::InterruptedByUser,
        TxErrorCode::Io(_) => ErrorCode::Disconnected,
        TxErrorCode::Shutdown => ErrorCode::NotOpen,
    }
}

/// Error code of a finished TX request: a cancellation cause wins over the generic placeholder
/// (`stopped` → `E_STOPPED`, `held_by_user` → `E_HELD_BY_USER`, `closed` → `E_NOT_OPEN`,
/// `interrupted_by_user` → `E_INTERRUPTED_BY_USER`).
pub(crate) fn done_error_code(done: &TxDone) -> ErrorCode {
    match done.cause {
        Some("stopped") => ErrorCode::Stopped,
        Some("held_by_user") => ErrorCode::HeldByUser,
        Some("closed") => ErrorCode::NotOpen,
        Some("interrupted_by_user") => ErrorCode::InterruptedByUser,
        _ => done.error.as_ref().map_or(ErrorCode::Internal, tx_error_code),
    }
}

/// TX submit refusal → error code.
pub(crate) fn submit_error(e: &SubmitError) -> WorkerError {
    let code = match e {
        SubmitError::QueueFull { .. } => ErrorCode::RateLimit,
        SubmitError::TooLarge { .. } | SubmitError::Empty => ErrorCode::Usage,
        SubmitError::Stopped => ErrorCode::NotOpen,
    };
    werr(code, e.to_string())
}

/// Timestamp from a chunk's own stamps.
pub(crate) fn ts_at(mono_ns: u64, wall_ns: u64, clock_id: &str, source: TsSource) -> Ts {
    Ts {
        mono_ns: U64(mono_ns),
        wall_ns: U64(wall_ns),
        clock_id: clock_id.to_owned(),
        source,
        quality: None,
    }
}

/// A priority event draft with the common fields filled.
pub(crate) fn draft(kind: &str, ts: Ts, origin: Origin, payload: Value) -> EventDraft {
    EventDraft {
        kind: kind.to_owned(),
        ts,
        dir: None,
        origin,
        client_id: None,
        request_id: None,
        job_id: None,
        generation: None,
        purpose: None,
        risk: None,
        env: None,
        payload,
        metrics: Vec::new(),
        quality: Vec::new(),
    }
}

/// Arbiter event → priority draft. State events have no actor: origin `device` stands for
/// "the channel" (the payload says who owns the input).
pub(crate) fn arbiter_draft(ev: &ArbiterEvent, ts: Ts, generation: u32) -> EventDraft {
    let mut d = draft(ev.kind(), ts, Origin::Device, ev.payload());
    d.generation = Some(generation);
    d
}

/// `uart.data` TX draft.
pub(crate) fn tx_data_draft(
    payload: Value,
    ts: Ts,
    origin: Origin,
    client_id: &str,
    request_id: Option<String>,
    generation: u32,
) -> EventDraft {
    let mut d = draft(kind::DATA, ts, origin, payload);
    d.dir = Some(Dir::Tx);
    d.client_id = Some(client_id.to_owned());
    d.request_id = request_id;
    d.generation = Some(generation);
    d
}

/// Bytes as printable escapes (`ls\r` → `ls\r`), at most `max_chars` characters.
pub(crate) fn escape_preview(bytes: &[u8], max_chars: usize) -> String {
    let mut out = String::new();
    for c in String::from_utf8_lossy(bytes).chars() {
        let piece = match c {
            '\r' => "\\r".to_owned(),
            '\n' => "\\n".to_owned(),
            '\t' => "\\t".to_owned(),
            '\\' => "\\\\".to_owned(),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => format!("\\x{:02x}", c as u32),
            c => c.to_string(),
        };
        if out.chars().count() + piece.chars().count() > max_chars {
            out.push('…');
            break;
        }
        out.push_str(&piece);
    }
    out
}

/// String argument.
pub(crate) fn arg_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// u64 argument.
pub(crate) fn arg_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

/// Environment from an argument (`sim|bench|vehicle`).
pub(crate) fn parse_env(s: &str) -> Option<Env> {
    match s {
        "sim" => Some(Env::Sim),
        "bench" => Some(Env::Bench),
        "vehicle" => Some(Env::Vehicle),
        _ => None,
    }
}

/// Environment as its wire name.
pub(crate) fn env_name(e: Env) -> &'static str {
    match e {
        Env::Sim => "sim",
        Env::Bench => "bench",
        Env::Vehicle => "vehicle",
    }
}

/// Write-state row payload.
pub(crate) fn write_row(state: &str, extra: Value) -> Value {
    let mut p = json!({ "write_state": state });
    if let (Value::Object(m), Value::Object(e)) = (&mut p, extra) {
        m.extend(e);
    }
    p
}

/// Adapter so the reader / TX actor stamp chunks with the SDK clock (one clock domain).
#[derive(Debug, Clone)]
pub(crate) struct CtxClock(pub(crate) WorkerCtx);

impl crate::clock::Clock for CtxClock {
    fn mono_ns(&self) -> u64 {
        self.0.clock().mono_ns()
    }
    fn wall_ns(&self) -> u64 {
        self.0.clock().wall_ns()
    }
}

/// Shared handle to the adapter.
pub(crate) fn ctx_clock(cx: &WorkerCtx) -> Arc<dyn crate::clock::Clock> {
    Arc::new(CtxClock(cx.clone()))
}
