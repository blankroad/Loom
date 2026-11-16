//! Periodic TesterPresent keep-alive (U1): parameters, pure scheduling decisions and the job's
//! cancel handle. The job task itself lives in [`crate::manager`] (it needs the target's gate and
//! runner). Default off; started only by `uds.tester_present {on:true, interval_ms}`; never
//! restarted automatically (not after reconnect, not after STOP release).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use loom_uds_runner::{TxnResult, TxnStatus};
use serde_json::Value;
use tokio::sync::Notify;

/// Allowed `interval_ms` (inclusive). No default: the caller chooses (ECU S3 differs per ECU).
pub const TP_MIN_INTERVAL_MS: u64 = 500;
pub const TP_MAX_INTERVAL_MS: u64 = 30_000;
/// Consecutive failed exchanges after which the job stops itself.
pub const TP_MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Upper bound of one exchange (also bounded by the interval).
pub const TP_EXCHANGE_TIMEOUT: Duration = Duration::from_secs(2);
/// Re-check delay while the target is busy with other traffic.
pub const TP_BUSY_RECHECK: Duration = Duration::from_millis(50);

/// Settings of one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TpConfig {
    pub interval: Duration,
    /// `3E 80` (suppress positive response) when true, `3E 00` otherwise.
    pub suppress: bool,
}

impl TpConfig {
    /// B2-T1 (Sonnet body): the request PDU (`[0x3E, 0x80]` or `[0x3E, 0x00]`).
    pub fn pdu(&self) -> Vec<u8> {
        vec![0x3E, if self.suppress { 0x80 } else { 0x00 }]
    }
}

/// Parsed `uds.tester_present` params.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpRequest {
    On(TpConfig),
    Off,
}

/// B2-T2 (Sonnet body): parse `uds.tester_present` params (`on`, `interval_ms`, `suppress`).
/// Error text is the `E_USAGE` message.
pub fn parse_tp_params(p: &Value) -> Result<TpRequest, String> {
    let Some(on) = p.get("on").and_then(Value::as_bool) else {
        return Err("tester_present needs on: true|false".into());
    };
    if !on {
        return Ok(TpRequest::Off);
    }
    let interval_ms = match p.get("interval_ms") {
        None | Some(Value::Null) => {
            return Err("tester_present on needs interval_ms (500–30000 ms, below the ECU's S3)".into());
        }
        Some(v) => match v.as_u64() {
            Some(ms) if (TP_MIN_INTERVAL_MS..=TP_MAX_INTERVAL_MS).contains(&ms) => ms,
            _ => return Err(format!("interval_ms must be an integer 500–30000 ms, got {v}")),
        },
    };
    let suppress = match p.get("suppress") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("suppress must be true|false".into()),
    };
    Ok(TpRequest::On(TpConfig {
        interval: Duration::from_millis(interval_ms),
        suppress,
    }))
}

/// What the job does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpTick {
    /// Send one TesterPresent now.
    Send,
    /// Due, but other traffic holds the target: re-check after [`TP_BUSY_RECHECK`].
    Busy,
    /// Not due yet (recent activity): sleep this long, then decide again.
    Wait(Duration),
}

/// B2-T3 (Sonnet body): pure scheduling decision.
pub fn tp_decide(
    now: Duration,
    last_activity: Option<Duration>,
    interval: Duration,
    target_busy: bool,
) -> TpTick {
    let due = last_activity.map_or(now, |t| t.saturating_add(interval));
    if now < due {
        TpTick::Wait(due - now)
    } else if target_busy {
        TpTick::Busy
    } else {
        TpTick::Send
    }
}

/// B2-T4 (Sonnet body): did one exchange keep the session alive? `Err` carries the failure text
/// (counted toward [`TP_MAX_CONSECUTIVE_FAILURES`]).
pub fn tp_exchange_outcome(res: &TxnResult) -> Result<(), String> {
    match &res.status {
        TxnStatus::Positive | TxnStatus::OkSuppressed => Ok(()),
        TxnStatus::Negative { nrc } => Err(match loom_uds_codec::nrc_name(*nrc) {
            Some(name) => format!("NRC 0x{nrc:02X} {name}"),
            None => format!("NRC 0x{nrc:02X}"),
        }),
        TxnStatus::Timeout { .. } => Err("timeout".into()),
        TxnStatus::LinkLost => Err("link lost".into()),
        TxnStatus::GenerationChanged => Err("generation changed".into()),
        TxnStatus::TransportNack { code } => Err(format!("transport NACK 0x{code:02X}")),
        TxnStatus::Rejected(r) => Err(format!("not sent: {r:?}")),
        TxnStatus::Aborted { kind } => Err(format!("aborted ({kind:?})")),
    }
}

/// Cancel handle shared by the job task and whoever stops it (sync stop, async observe).
#[derive(Debug, Default)]
pub struct TpCancel {
    flag: AtomicBool,
    notify: Notify,
}

impl TpCancel {
    /// B2-T5 (Sonnet body): set the flag and wake the task. Idempotent, never blocks.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// B2-T5 (Sonnet body).
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// B2-T5 (Sonnet body): resolves once cancelled (immediately if already cancelled).
    pub async fn cancelled(&self) {
        loop {
            let n = self.notify.notified();
            tokio::pin!(n);
            n.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            n.await;
        }
    }
}
