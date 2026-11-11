//! Shared model: limits, value sets, conditions and parameter helpers used by all three pieces.

use serde_json::Value;

/// Default profile limit: voltage (V). Over it a set is refused (exit 4), never clamped.
pub const DEFAULT_VMAX: f64 = 16.0;
/// Default profile limit: current (A).
pub const DEFAULT_IMAX: f64 = 5.0;
/// Default poll rate (Hz).
pub const POLL_HZ: u32 = 5;
/// Poll period (ns) at [`POLL_HZ`].
pub const POLL_PERIOD_NS: u64 = 1_000_000_000 / POLL_HZ as u64;
/// Longest `psu.cycle` off time.
pub const MAX_CYCLE_OFF_MS: u64 = 60_000;
/// Longest `psu.watch --for`.
pub const MAX_WATCH_MS: u64 = 600_000;
/// Longest `psu.wait_for` (contract single-result wait).
pub const MAX_WAIT_MS: u64 = 3_600_000;
/// Longest hold of a wait/evidence condition.
pub const MAX_HOLD_MS: u64 = 3_600_000;
/// Longest injected response delay.
pub const MAX_INJECT_DELAY_MS: u64 = 10_000;
/// Most queued no-response injections.
pub const MAX_INJECT_NO_RESPONSE: u32 = 1000;

/// Protection state values (`psu.state.prot`).
pub const PROT_VALUES: &[&str] = &["ok", "ovp", "ocp", "otp", "unknown"];
/// Mode values (`psu.state.mode`).
pub const MODE_VALUES: &[&str] = &["cv", "cc", "off", "unknown"];
/// Interlocks of other domains that block agent `out off` / `cycle` (A2, §5.1 i).
pub const RESPECTED_INTERLOCKS: &[&str] = &["uds.flashing", "debug.flashing"];

/// Profile limits of one supply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Volts.
    pub vmax: f64,
    /// Amperes.
    pub imax: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            vmax: DEFAULT_VMAX,
            imax: DEFAULT_IMAX,
        }
    }
}

impl Limits {
    /// Limits from a `psu.state`/connect payload; only tightening below the default is accepted
    /// (raising a limit is a `loom.toml` change by the user, not an agent argument).
    pub fn from_value(v: &Value) -> Limits {
        let d = Limits::default();
        let pick = |k: &str, def: f64| {
            v.get(k)
                .and_then(Value::as_f64)
                .filter(|x| x.is_finite() && *x > 0.0)
                .map_or(def, |x| x.min(def))
        };
        Limits {
            vmax: pick("vmax", d.vmax),
            imax: pick("imax", d.imax),
        }
    }

    /// Why `volt`/`curr` is over the limit, if it is.
    pub fn violation(&self, volt: Option<f64>, curr: Option<f64>) -> Option<String> {
        match (volt, curr) {
            (Some(v), _) if v > self.vmax => Some(format!("{v} V over profile limit vmax {} V", self.vmax)),
            (_, Some(i)) if i > self.imax => Some(format!("{i} A over profile limit imax {} A", self.imax)),
            _ => None,
        }
    }
}

/// `volt` (or the UI's `v`).
pub fn param_volt(p: &Value) -> Option<f64> {
    p.get("volt").or_else(|| p.get("v")).and_then(Value::as_f64)
}

/// `curr` (or the UI's `i`).
pub fn param_curr(p: &Value) -> Option<f64> {
    p.get("curr").or_else(|| p.get("i")).and_then(Value::as_f64)
}

/// `ch` (or the UI's `channel`).
pub fn param_ch(p: &Value) -> Option<&str> {
    p.get("ch")
        .or_else(|| p.get("channel"))
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
}

/// Measured quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// Volts.
    Voltage,
    /// Amperes.
    Current,
}

impl Quantity {
    /// Metric name.
    pub fn metric(self) -> &'static str {
        match self {
            Quantity::Voltage => "voltage",
            Quantity::Current => "current",
        }
    }
}

/// A wait/evidence condition on one quantity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    /// What is compared.
    pub q: Quantity,
    /// Lower bound (exclusive), if any.
    pub gt: Option<f64>,
    /// Upper bound (exclusive), if any.
    pub lt: Option<f64>,
}

impl Condition {
    /// Holds for `x`.
    pub fn holds(&self, x: f64) -> bool {
        self.gt.is_none_or(|g| x > g) && self.lt.is_none_or(|l| x < l)
    }

    /// `curr_lt: 0.005` style text.
    pub fn describe(&self) -> String {
        let n = match self.q {
            Quantity::Voltage => "V",
            Quantity::Current => "I",
        };
        match (self.gt, self.lt) {
            (Some(g), Some(l)) => format!("{g} < {n} < {l}"),
            (Some(g), None) => format!("{n} > {g}"),
            (None, Some(l)) => format!("{n} < {l}"),
            (None, None) => format!("{n} any"),
        }
    }

    /// Exactly one of `curr_lt|curr_gt|curr_between|volt_lt|volt_gt|volt_between` (`between` = `[lo, hi]`).
    pub fn from_params(p: &Value) -> Result<Condition, String> {
        let keys = [
            "curr_lt",
            "curr_gt",
            "curr_between",
            "volt_lt",
            "volt_gt",
            "volt_between",
        ];
        let present: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|k| p.get(*k).is_some_and(|v| !v.is_null()))
            .collect();
        let [key] = present.as_slice() else {
            return Err(format!("give exactly one of {}", keys.join(", ")));
        };
        let v = &p[*key];
        let q = if key.starts_with("curr") {
            Quantity::Current
        } else {
            Quantity::Voltage
        };
        let num = |x: &Value| {
            x.as_f64()
                .filter(|f| f.is_finite())
                .ok_or_else(|| format!("{key} needs a number"))
        };
        let c = if key.ends_with("_lt") {
            Condition {
                q,
                gt: None,
                lt: Some(num(v)?),
            }
        } else if key.ends_with("_gt") {
            Condition {
                q,
                gt: Some(num(v)?),
                lt: None,
            }
        } else {
            let a = v
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| format!("{key} needs [lo, hi]"))?;
            let (lo, hi) = (num(&a[0])?, num(&a[1])?);
            if lo >= hi {
                return Err(format!("{key}: lo must be < hi"));
            }
            Condition {
                q,
                gt: Some(lo),
                lt: Some(hi),
            }
        };
        Ok(c)
    }
}

/// Metric value of a record payload's `metrics` list.
pub fn metric(metrics: &[loom_proto::event::Metric], name: &str) -> Option<f64> {
    metrics.iter().find(|m| m.name == name).map(|m| m.value)
}
