//! Write hooks (plan §5.1 i, F1: classes are shown and audited, nothing is blocked by default).
//! Synchronous, no I/O.
//!
//! | operation | class | rule |
//! |---|---|---|
//! | connect | R0 | opens the supply, audited |
//! | set (inside the profile limits) | R2 | over `vmax`/`imax` → blocked (exit 4), never clamped |
//! | out on | R3 | |
//! | out off / cycle | R3 | interlock (`uds.flashing`, `debug.flashing`) refuses agent/script; the user (emergency OFF) only gets a warning (A3) |

use std::sync::Arc;

use loom_domain_api::{Classification, Grant, PresendError, WriteHooks, WriteRequest};
use loom_proto::event::{Origin, Risk};
use loom_proto::policy::BlockClass;
use serde_json::Value;

use super::common::ChannelBook;
use crate::model::{Limits, RESPECTED_INTERLOCKS, param_curr, param_volt};

/// Reads whether an interlock of another domain is active. CORE has no cross-domain interlock
/// query for hooks yet (CORE ask); the default sees none active.
pub type InterlockView = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// View with no active interlocks.
pub fn no_interlocks() -> InterlockView {
    Arc::new(|_| false)
}

/// Write hooks of the psu domain.
#[derive(Clone)]
pub struct PsuHooks {
    pub(crate) book: ChannelBook,
    pub(crate) interlocks: InterlockView,
}

impl std::fmt::Debug for PsuHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PsuHooks").finish_non_exhaustive()
    }
}

impl PsuHooks {
    fn limits(&self, req: &WriteRequest<'_>) -> Limits {
        req.target
            .channel
            .as_deref()
            .and_then(|c| self.book.get(c))
            .map(|i| i.limits())
            .unwrap_or_default()
    }

    /// Active interlocks that block switching the output off.
    pub fn active_interlocks(&self) -> Vec<&'static str> {
        RESPECTED_INTERLOCKS
            .iter()
            .copied()
            .filter(|n| (self.interlocks)(n))
            .collect()
    }
}

/// Is this request switching an output off (out off / cycle)?
pub fn is_power_cut(method: &str, args: &Value) -> bool {
    method == "psu.cycle" || (method == "psu.out" && args.get("state").and_then(Value::as_str) != Some("on"))
}

impl WriteHooks for PsuHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        let (risk, mut reason, block) = match req.method {
            "psu.set" => match self
                .limits(req)
                .violation(param_volt(req.args), param_curr(req.args))
            {
                // Not a policy choice: the profile limit cannot be confirmed away (plan §5.1 i).
                Some(why) => (Risk::R2, why, Some(BlockClass::HardDeny)),
                None => (
                    Risk::R2,
                    "set voltage/current within the profile limits".to_owned(),
                    None,
                ),
            },
            "psu.out" if req.args.get("state").and_then(Value::as_str) == Some("on") => {
                (Risk::R3, "output ON".to_owned(), None)
            }
            "psu.out" => (Risk::R3, "output OFF".to_owned(), None),
            "psu.cycle" => (Risk::R3, "power cycle".to_owned(), None),
            m => (Risk::R0, format!("{m}: open the supply (audited)"), None),
        };
        if is_power_cut(req.method, req.args) {
            let active = self.active_interlocks();
            if !active.is_empty() {
                let who = if req.origin == Origin::User {
                    "WARNING (user action proceeds)"
                } else {
                    "blocks agent/script"
                };
                reason.push_str(&format!("; interlock {} active — {who}", active.join(", ")));
            }
        }
        Classification { risk, reason, block }
    }

    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        if is_power_cut(req.method, req.args) && req.origin != Origin::User {
            let active = self.active_interlocks();
            if !active.is_empty() {
                return Err(PresendError::Locked(format!(
                    "interlock {} active: output stays on",
                    active.join(", ")
                )));
            }
        }
        if let (Some(ch), Some(want)) = (req.target.channel.as_deref(), req.target.generation)
            && let Some(have) = self.book.get(ch).and_then(|i| i.generation)
            && have != want
            && req.origin != Origin::User
        {
            return Err(PresendError::EpochChanged(format!(
                "supply {ch} reconnected (generation {have}, request made against {want})"
            )));
        }
        Ok(())
    }
}
