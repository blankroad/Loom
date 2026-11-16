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

use crate::logsink::{
    LOG_QUOTA_DEFAULT_MIB, LOG_QUOTA_MAX_MIB, LOG_ROTATE_DEFAULT_MIB, LOG_ROTATE_MAX_MIB, LOG_ROTATE_MIN_MIB,
    LogCfg, LogFormat, LogStats,
};
use crate::tx::{PACING_CHAR_MAX, PACING_LINE_MAX, Pacing, SubmitError, TxDone, TxErrorCode};

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
    /// B4 (U1-A): native log started / stopped.
    pub const LOG: &str = "uart.log";
    /// B3 (U6): a break / DTR / RTS operation.
    pub const SIGNAL: &str = "uart.signal";
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

/// Keys allowed in a `pacing` object (`uart.open.pacing`).
pub(crate) const PACING_KEYS: [&str; 2] = ["char_delay_ms", "line_delay_ms"];

/// B2: `base` with the optional `char_delay_ms` (0–[`PACING_CHAR_MAX`]) and `line_delay_ms`
/// (0–[`PACING_LINE_MAX`]) fields of `v` applied; other keys of `v` are ignored. A present field
/// that is not a non-negative integer in range → `E_USAGE`. See FUNCTIONS.md §B2.2.
pub(crate) fn pacing_fields(v: &Value, base: Pacing) -> Result<Pacing, WorkerError> {
    let field = |key: &str,
                 max: std::time::Duration,
                 keep: std::time::Duration|
     -> Result<std::time::Duration, WorkerError> {
        match v.get(key) {
            None | Some(Value::Null) => Ok(keep),
            Some(val) => match val.as_u64() {
                Some(n) if std::time::Duration::from_millis(n) <= max => {
                    Ok(std::time::Duration::from_millis(n))
                }
                _ => Err(werr(
                    ErrorCode::Usage,
                    format!("bad {key}: {val} (0–{} ms)", max.as_millis()),
                )),
            },
        }
    };
    let char_delay = field("char_delay_ms", PACING_CHAR_MAX, base.char_delay)?;
    let line_delay = field("line_delay_ms", PACING_LINE_MAX, base.line_delay)?;
    Ok(Pacing {
        char_delay,
        line_delay,
    })
}

/// B2: the `pacing` option of `uart.open`. Absent / null → `Ok(None)`; not an object, or a key
/// outside [`PACING_KEYS`] → `E_USAGE`; else `Some(pacing_fields(v, Pacing::NONE)?)`. §B2.2.
pub(crate) fn parse_pacing(v: Option<&Value>) -> Result<Option<Pacing>, WorkerError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => {
            if let Some(k) = m.keys().find(|k| !PACING_KEYS.contains(&k.as_str())) {
                return Err(werr(ErrorCode::Usage, format!("unknown pacing field '{k}'")));
            }
            Ok(Some(pacing_fields(&Value::Object(m.clone()), Pacing::NONE)?))
        }
        Some(_) => Err(werr(ErrorCode::Usage, "pacing must be an object")),
    }
}

/// B2: `{"char_delay_ms": u64, "line_delay_ms": u64}` (whole milliseconds). §B2.2.
pub(crate) fn pacing_json(p: Pacing) -> Value {
    let ms = |d: std::time::Duration| u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
    json!({"char_delay_ms": ms(p.char_delay), "line_delay_ms": ms(p.line_delay)})
}

/// Keys allowed in a `log` object (`uart.open.log`, `uart.log.start`).
pub(crate) const LOG_KEYS: [&str; 3] = ["format", "rotate_mib", "quota_mib"];

/// B4: `{format?: "log"|"jsonl", rotate_mib?: 1–1024, quota_mib?: 1–8192}` → [`LogCfg`] (defaults: log, 64, 1024).
/// `allow_other_keys` = the whole `uart.log.start` args (channel etc. ignored); `false` = the `uart.open.log` object
/// (unknown key → `E_USAGE`). See FUNCTIONS.md §B4.2.
pub(crate) fn parse_log_cfg(v: &Value, allow_other_keys: bool) -> Result<LogCfg, WorkerError> {
    let Value::Object(m) = v else {
        return Err(werr(ErrorCode::Usage, "log must be an object"));
    };
    if !allow_other_keys && let Some(k) = m.keys().find(|k| !LOG_KEYS.contains(&k.as_str())) {
        return Err(werr(ErrorCode::Usage, format!("unknown log field '{k}'")));
    }
    let format = match m.get("format") {
        None | Some(Value::Null) => LogFormat::Log,
        Some(Value::String(s)) if s == "log" => LogFormat::Log,
        Some(Value::String(s)) if s == "jsonl" => LogFormat::Jsonl,
        Some(x) => return Err(werr(ErrorCode::Usage, format!("bad log format: {x} (log|jsonl)"))),
    };
    let mib = |key: &str, default: u64, min: u64, max: u64| -> Result<u64, WorkerError> {
        match m.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(x) => match x.as_u64() {
                Some(n) if (min..=max).contains(&n) => Ok(n),
                _ => Err(werr(ErrorCode::Usage, format!("bad {key}: {x} ({min}–{max})"))),
            },
        }
    };
    let rotate = mib(
        "rotate_mib",
        LOG_ROTATE_DEFAULT_MIB,
        LOG_ROTATE_MIN_MIB,
        LOG_ROTATE_MAX_MIB,
    )?;
    let quota = mib("quota_mib", LOG_QUOTA_DEFAULT_MIB, 1, LOG_QUOTA_MAX_MIB)?;
    Ok(LogCfg {
        format,
        rotate_bytes: rotate * 1_048_576,
        quota_bytes: quota * 1_048_576,
    })
}

/// B4: `{active, dir, file, format, rotate_mib, quota_mib, files, bytes_written, dir_bytes, dropped_records,
/// dropped_bytes, stopped}` (`cfg` None → format/rotate/quota null). See §B4.2.
pub(crate) fn log_json(st: &LogStats, cfg: Option<&LogCfg>) -> Value {
    let path = |p: &std::path::Path| -> Value {
        if p.as_os_str().is_empty() {
            Value::Null
        } else {
            json!(p.to_string_lossy())
        }
    };
    json!({
        "active": st.active,
        "dir": path(&st.dir),
        "file": st.file.as_deref().map(|f| f.to_string_lossy().into_owned()),
        "format": cfg.map(|c| if c.format == LogFormat::Jsonl { "jsonl" } else { "log" }),
        "rotate_mib": cfg.map(|c| c.rotate_bytes / 1_048_576),
        "quota_mib": cfg.map(|c| c.quota_bytes / 1_048_576),
        "files": st.files,
        "bytes_written": st.bytes_written,
        "dir_bytes": st.dir_bytes,
        "dropped_records": st.dropped_records,
        "dropped_bytes": st.dropped_bytes,
        "stopped": st.stopped,
    })
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
