//! `uart.signal` (operator-upgrade U6, batch B3): break pulse, DTR / RTS set or pulse.
//!
//! Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B3.1. Line controls run on the TX actor
//! thread at a chunk boundary ([`crate::tx::TxHandle::control`]); a break or pulse is always released,
//! even when STOP / HOLD arrive while it is held.

use std::sync::Arc;
use std::time::Duration;

use loom_proto::worker::{WorkerError, Write};
use loom_worker_sdk::{WorkerCtx, WriteOutcome};
use serde_json::Value;

use super::port::Port;
#[allow(unused_imports)] // B3 bodies use it
use super::send::Rows;

/// Break length when `duration_ms` is absent.
pub const BREAK_DEFAULT_MS: u64 = 250;
/// Longest break / pulse (`duration_ms` 1–5000, `commands.rs` schema).
pub const SIGNAL_MAX_MS: u64 = 5000;
/// Wait for one line-control operation on the TX actor.
pub const SIGNAL_CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

/// Which line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalKind {
    /// Break condition (TX held low).
    Break,
    /// Data Terminal Ready.
    Dtr,
    /// Request To Send.
    Rts,
}

/// One parsed `uart.signal` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SignalOp {
    pub(crate) kind: SignalKind,
    /// Level to set (break: always `true`).
    pub(crate) value: bool,
    /// Break length, or pulse length for DTR/RTS (then the line returns to `!value`); `None` = set and keep.
    pub(crate) duration: Option<Duration>,
}

/// Last levels this worker set on the open port (`None` = never set by Loom; the OS default applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LineState {
    pub(crate) dtr: Option<bool>,
    pub(crate) rts: Option<bool>,
    /// Break is asserted now (or its release failed — then it may still be asserted).
    pub(crate) break_on: bool,
}

/// `{channel, kind, value?, duration_ms?}` → [`SignalOp`]. See FUNCTIONS.md §B3.1 `parse_signal`.
pub(crate) fn parse_signal(op: &Value) -> Result<SignalOp, WorkerError> {
    use loom_proto::error::ErrorCode;

    use super::util::{arg_str, werr};

    let kind = match arg_str(op, "kind") {
        Some("break") => SignalKind::Break,
        Some("dtr") => SignalKind::Dtr,
        Some("rts") => SignalKind::Rts,
        _ => return Err(werr(ErrorCode::Usage, "bad signal kind (break|dtr|rts)")),
    };
    let duration = match op.get("duration_ms") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) if (1..=SIGNAL_MAX_MS).contains(&n) => Some(Duration::from_millis(n)),
            _ => {
                return Err(werr(
                    ErrorCode::Usage,
                    format!("bad duration_ms: {v} (1–{SIGNAL_MAX_MS})"),
                ));
            }
        },
    };
    match kind {
        SignalKind::Break => match op.get("value") {
            None | Some(Value::Null) | Some(Value::Bool(true)) => Ok(SignalOp {
                kind,
                value: true,
                duration: Some(duration.unwrap_or(Duration::from_millis(BREAK_DEFAULT_MS))),
            }),
            _ => Err(werr(
                ErrorCode::Usage,
                "break is a pulse (value must be true or absent)",
            )),
        },
        SignalKind::Dtr | SignalKind::Rts => match op.get("value") {
            Some(Value::Bool(value)) => Ok(SignalOp {
                kind,
                value: *value,
                duration,
            }),
            _ => Err(werr(ErrorCode::Usage, "value must be a boolean")),
        },
    }
}

/// `{"dtr": bool|null, "rts": bool|null, "break": bool}`. See §B3.1.
pub(crate) fn line_json(l: LineState) -> Value {
    serde_json::json!({"dtr": l.dtr, "rts": l.rts, "break": l.break_on})
}

/// The `uart.signal` request write: rows, STOP/HOLD/connected checks, control ops, `uart.signal` event.
/// See FUNCTIONS.md §B3.1 `signal_write`.
pub(crate) async fn signal_write(port: Arc<Port>, cx: &WorkerCtx, w: Write) -> WriteOutcome {
    use loom_proto::error::ErrorCode;
    use loom_proto::event::{Origin, TsSource, WriteState};
    use serde_json::json;

    use super::util::{draft, kind, werr};
    use crate::tx::ControlOp;

    let client = w.client_id.clone().unwrap_or_else(|| "agent".into());
    let rows = Rows {
        port: &port,
        origin: w.origin,
        client: client.clone(),
        request_id: w.request_id.clone(),
    };
    let sop = match parse_signal(&w.op) {
        Ok(o) => o,
        Err(e) => {
            rows.emit("requested", json!({}));
            return rows.fail(WriteState::Failed, e, json!({}));
        }
    };
    let name = match sop.kind {
        SignalKind::Break => "break",
        SignalKind::Dtr => "dtr",
        SignalKind::Rts => "rts",
    };
    let duration_ms = sop
        .duration
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let mut requested = json!({"signal": name, "value": sop.value});
    if let Some(ms) = duration_ms {
        requested["duration_ms"] = json!(ms);
    }
    rows.emit("requested", requested);

    // Refusals: no line is touched and no `uart.signal` event is emitted.
    let refusal = if cx.latch().is_on() && w.origin != Origin::User {
        Some(werr(ErrorCode::Stopped, "STOP latched"))
    } else if port.arb().hold_on() && w.origin != Origin::User {
        Some(werr(ErrorCode::HeldByUser, "HOLD is on"))
    } else if !port.connected.load(std::sync::atomic::Ordering::Acquire) {
        Some(werr(ErrorCode::Disconnected, "port disconnected"))
    } else {
        None
    };
    if let Some(e) = refusal {
        return rows.fail(WriteState::Failed, e, json!({}));
    }
    rows.emit("accepted", json!({}));

    let mk = |level: bool| match sop.kind {
        SignalKind::Break => ControlOp::Break(level),
        SignalKind::Dtr => ControlOp::Dtr(level),
        SignalKind::Rts => ControlOp::Rts(level),
    };
    let record = |level: bool| {
        let mut l = port.lines.lock().unwrap_or_else(|p| p.into_inner());
        match sop.kind {
            SignalKind::Break => l.break_on = level,
            SignalKind::Dtr => l.dtr = Some(level),
            SignalKind::Rts => l.rts = Some(level),
        }
    };
    let run = |op: ControlOp| {
        let tx = port.tx.clone();
        async move {
            match tokio::task::spawn_blocking(move || tx.control(op, SIGNAL_CONTROL_TIMEOUT)).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(werr(io_code(e.kind()), format!("line control failed: {e}"))),
                Err(e) => Err(werr(ErrorCode::Internal, format!("signal task failed: {e}"))),
            }
        }
    };

    let first = run(mk(sop.value)).await;
    if first.is_ok() {
        record(sop.value);
    }
    // A pulse (and every break) is always released, even when the assert failed or STOP/HOLD arrived.
    let release = match sop.duration {
        Some(d) => {
            if first.is_ok() {
                tokio::time::sleep(d).await;
            }
            let r = run(mk(!sop.value)).await;
            if r.is_ok() {
                record(!sop.value);
            }
            Some(r)
        }
        None => None,
    };

    let (state, result, error) = match (first, release) {
        (Err(e), _) => (WriteState::Failed, "failed", Some(e)),
        (Ok(()), Some(Err(e))) => (
            WriteState::OutcomeUnknown,
            "release_failed",
            Some(werr(e.code, "release failed — the line may still be held")),
        ),
        _ => (WriteState::Sent, "ok", None),
    };

    let lines = *port.lines.lock().unwrap_or_else(|p| p.into_inner());
    let mut payload = json!({"kind": name, "value": sop.value, "result": result});
    let mut detail = json!({
        "request_id": w.request_id,
        "kind": name,
        "value": sop.value,
        "lines": line_json(lines),
        "generation": port.info.generation,
    });
    if let Some(ms) = duration_ms {
        payload["duration_ms"] = json!(ms);
        detail["duration_ms"] = json!(ms);
    }
    if let Some(e) = &error {
        payload["error"] = json!({"code": e.code.as_str(), "message": e.message});
    }
    let mut ev = draft(
        kind::SIGNAL,
        port.ts_now(TsSource::HostReadComplete),
        w.origin,
        payload,
    );
    ev.client_id = Some(client);
    ev.request_id = Some(w.request_id.clone());
    port.emit(ev);

    match error {
        None => {
            rows.emit("sent", json!({"final": true}));
            WriteOutcome {
                state,
                detail,
                error: None,
            }
        }
        Some(e) => {
            let mut out = rows.fail(state, e, json!({}));
            out.detail = detail;
            out
        }
    }
}

/// Line-control io error → worker error code (§B3.1).
fn io_code(kind: std::io::ErrorKind) -> loom_proto::error::ErrorCode {
    use loom_proto::error::ErrorCode;
    match kind {
        std::io::ErrorKind::Unsupported => ErrorCode::Unsupported,
        std::io::ErrorKind::TimedOut => ErrorCode::Timeout,
        std::io::ErrorKind::WouldBlock => ErrorCode::RateLimit,
        std::io::ErrorKind::BrokenPipe => ErrorCode::NotOpen,
        _ => ErrorCode::Disconnected,
    }
}
