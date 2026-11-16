//! Opt-in bounded reconnect (operator-upgrade U2, batch B3).
//!
//! Contract: work/reports/operator-upgrade/uart/FUNCTIONS.md §B3.2. Off unless `uart.open{reconnect:{enabled:true}}`.
//! After the reader reports a disconnect, one supervisor thread polls every [`RECONNECT_POLL`] for at most the
//! configured window: the same target, the same recorded USB identity (when one was known), a new generation.
//! STOP, close or a replaced port end it; nothing that was running (agent writes, macros, REC) is resumed.

use std::fmt;
use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use loom_proto::worker::WorkerError;
use loom_worker_sdk::WorkerCtx;
use serde_json::Value;

use super::UartWorker;
use crate::transport::{self, OpenOpts, PortConfig, Transport};

/// Window when `window_ms` is absent.
pub const RECONNECT_WINDOW_DEFAULT: Duration = Duration::from_secs(10);
/// Smallest window.
pub const RECONNECT_WINDOW_MIN: Duration = Duration::from_secs(1);
/// Largest window.
pub const RECONNECT_WINDOW_MAX: Duration = Duration::from_secs(120);
/// Poll period while reconnecting.
pub const RECONNECT_POLL: Duration = Duration::from_millis(300);

/// Reconnect setting of one open port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectCfg {
    /// Reconnect after a disconnect.
    pub enabled: bool,
    /// How long to keep trying.
    pub window: Duration,
}

impl ReconnectCfg {
    /// Default: off.
    pub const OFF: ReconnectCfg = ReconnectCfg {
        enabled: false,
        window: RECONNECT_WINDOW_DEFAULT,
    };
}

/// USB identity of a port as the OS lists it (`uart.ports`). All `None` = not a listed USB device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortIdentity {
    /// USB vendor id.
    pub vid: Option<u16>,
    /// USB product id.
    pub pid: Option<u16>,
    /// USB serial number.
    pub serial: Option<String>,
}

impl PortIdentity {
    /// Any field known. See FUNCTIONS.md §B3.2.
    pub fn is_known(&self) -> bool {
        self.vid.is_some() || self.pid.is_some() || self.serial.is_some()
    }

    /// Every field known in `self` is equal in `found` (a field `self` knows but `found` lacks = mismatch).
    /// See FUNCTIONS.md §B3.2.
    pub fn matches(&self, found: &PortIdentity) -> bool {
        (self.vid.is_none() || self.vid == found.vid)
            && (self.pid.is_none() || self.pid == found.pid)
            && (self.serial.is_none() || self.serial == found.serial)
    }
}

/// How a disconnected port is re-identified and reopened. Production: [`SystemReopener`]; tests inject a fake
/// through [`UartWorker::set_reopener`].
pub trait Reopener: Send + Sync + fmt::Debug {
    /// Identity of the device now at `target`; `None` = not present / not listed.
    fn identity(&self, target: &str) -> Option<PortIdentity>;
    /// Open `target` again with the session's line settings.
    fn open(&self, target: &str, config: &PortConfig, opts: &OpenOpts) -> io::Result<Box<dyn Transport>>;
}

/// [`transport::list_ports`] + [`transport::open`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemReopener;

impl Reopener for SystemReopener {
    /// See FUNCTIONS.md §B3.2 `SystemReopener::identity`.
    fn identity(&self, target: &str) -> Option<PortIdentity> {
        let list = transport::list_ports().ok()?;
        if list.truncated {
            return None;
        }
        list.ports
            .into_iter()
            .find(|p| p.path == target)
            .map(|p| PortIdentity {
                vid: p.vid,
                pid: p.pid,
                serial: p.serial,
            })
    }

    fn open(&self, target: &str, config: &PortConfig, opts: &OpenOpts) -> io::Result<Box<dyn Transport>> {
        transport::open(target, config, opts)
    }
}

/// Live reconnect bookkeeping of a port (status / events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ReconnectState {
    /// The supervisor is polling now.
    pub(crate) active: bool,
    /// Reopen attempts made in the current episode.
    pub(crate) attempts: u32,
}

/// One reopen attempt's outcome ([`UartWorker::reopen_after_disconnect`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReopenStep {
    /// A new port is open with this generation.
    Reopened(u32),
    /// Not possible yet (device absent, open failed): keep polling. Text = last reason.
    NotYet(String),
    /// A device is at the target but its identity differs: give up.
    IdentityMismatch {
        expected: PortIdentity,
        found: PortIdentity,
    },
    /// The port was closed, replaced or is connected again: stop silently.
    Gone,
}

/// The `uart.open` `reconnect` option. Absent / null → `Ok(None)`. See FUNCTIONS.md §B3.2 `parse_reconnect`.
pub(crate) fn parse_reconnect(v: Option<&Value>) -> Result<Option<ReconnectCfg>, WorkerError> {
    let usage = |m: String| super::werr(loom_proto::error::ErrorCode::Usage, m);
    let v = match v {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(m)) => m,
        Some(_) => return Err(usage("reconnect must be an object".into())),
    };
    if let Some(k) = v.keys().find(|k| *k != "enabled" && *k != "window_ms") {
        return Err(usage(format!("unknown reconnect field '{k}'")));
    }
    let enabled = match v.get("enabled") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(usage("reconnect.enabled must be a boolean".into())),
    };
    let window = match v.get("window_ms") {
        None => RECONNECT_WINDOW_DEFAULT,
        Some(x) => match x.as_u64().map(Duration::from_millis) {
            Some(d) if (RECONNECT_WINDOW_MIN..=RECONNECT_WINDOW_MAX).contains(&d) => d,
            _ => {
                return Err(usage(format!(
                    "bad reconnect.window_ms: {x} ({}–{} ms)",
                    RECONNECT_WINDOW_MIN.as_millis(),
                    RECONNECT_WINDOW_MAX.as_millis()
                )));
            }
        },
    };
    Ok(Some(ReconnectCfg { enabled, window }))
}

/// `{"enabled", "window_ms", "active", "attempts"}`. See §B3.2.
pub(crate) fn reconnect_json(c: ReconnectCfg, st: ReconnectState) -> Value {
    serde_json::json!({
        "enabled": c.enabled,
        "window_ms": u64::try_from(c.window.as_millis()).unwrap_or(u64::MAX),
        "active": st.active,
        "attempts": st.attempts,
    })
}

/// `{"vid", "pid", "serial"}` (nulls kept). See §B3.2.
pub(crate) fn identity_json(i: &PortIdentity) -> Value {
    serde_json::json!({"vid": i.vid, "pid": i.pid, "serial": i.serial})
}

/// Supervisor thread body for one port generation: waits for that port's disconnect signal (`rx`; a closed
/// channel = the port was shut down → return), then runs one bounded reconnect episode. See FUNCTIONS.md §B3.2
/// `supervise`.
pub(crate) fn supervise(w: UartWorker, cx: WorkerCtx, rx: Receiver<()>, generation: u32, cfg: ReconnectCfg) {
    use loom_proto::event::{Origin, TsSource};
    use serde_json::json;

    use super::util::{draft, kind};

    if rx.recv().is_err() {
        return;
    }
    let Some(port) = w.port() else { return };
    if port.info.generation != generation {
        return;
    }
    let started = std::time::Instant::now();
    let state_now = |p: &super::port::Port| *super::lock(&p.reconnect);
    *super::lock(&port.reconnect) = ReconnectState {
        active: true,
        attempts: 0,
    };
    port.emit(draft(
        kind::STATE,
        port.ts_now(TsSource::HostReadComplete),
        Origin::Device,
        json!({"state": "reconnecting", "path": port.info.target, "reconnect": reconnect_json(cfg, state_now(&port))}),
    ));
    let mut attempt = 0u32;
    let (reason, mismatch) = loop {
        if cx.latch().is_on() {
            break ("stopped", None);
        }
        if started.elapsed() >= cfg.window {
            break ("gave_up", None);
        }
        std::thread::sleep(RECONNECT_POLL);
        attempt += 1;
        super::lock(&port.reconnect).attempts = attempt;
        let after_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match w.reopen_after_disconnect(&cx, generation, attempt, after_ms) {
            ReopenStep::Reopened(_) | ReopenStep::Gone => return,
            ReopenStep::NotYet(_) => {}
            ReopenStep::IdentityMismatch { expected, found } => {
                break ("identity_mismatch", Some((expected, found)));
            }
        }
    };
    let Some(cur) = w.port() else { return };
    if cur.info.generation != generation {
        return;
    }
    *super::lock(&cur.reconnect) = ReconnectState {
        active: false,
        attempts: attempt,
    };
    let mut payload = json!({
        "state": "disconnected",
        "path": cur.info.target,
        "reason": reason,
        "reconnect": reconnect_json(cfg, state_now(&cur)),
    });
    if let Some((expected, found)) = mismatch {
        payload["expected"] = identity_json(&expected);
        payload["found"] = identity_json(&found);
    }
    cur.emit(draft(
        kind::STATE,
        cur.ts_now(TsSource::HostReadComplete),
        Origin::Device,
        payload,
    ));
}
