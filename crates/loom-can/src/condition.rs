//! Pure signal condition syntax shared by CLI and gateway.
use loom_proto::event::Origin;
use serde_json::{Value, json};

/// Maximum signal condition bytes.
pub const MAX_EXPR_BYTES: usize = 256;
/// Maximum bounded signal query window.
pub const SIG_MAX_WINDOW_MS: u32 = 60_000;
/// Maximum signal wait.
pub const WAIT_MAX_MS: u32 = 3_600_000;

/// Comparison of a condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
    Between,
}

impl Op {
    /// Wire / expression form.
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Between => "between",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Op> {
        Some(match s {
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "<" => Op::Lt,
            "<=" => Op::Le,
            "==" => Op::Eq,
            "!=" => Op::Ne,
            "between" => Op::Between,
            _ => return None,
        })
    }
}

/// A single signal condition.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    /// `Msg.Sig` or a bare signal name.
    pub signal: String,
    pub op: Op,
    /// Compared value (lower bound for `between`).
    pub value: f64,
    /// Upper bound for `between` (equals `value` otherwise).
    pub hi: f64,
    /// Tolerance that widens the condition toward "true" (≥ 0).
    pub tol: f64,
}

fn number(t: &str) -> Result<f64, String> {
    let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).map(|v| v as f64).map_err(|_| ()),
        None => t.parse::<f64>().map_err(|_| ()),
    };
    match v {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(format!("{t:?} is not a number")),
    }
}

/// Parses `EngRPM > 3000`, `EngineData.EngRPM>=3000`, `Volt between 11.5 14.5` (`and` optional).
pub fn parse_condition(expr: &str) -> Result<Condition, String> {
    const FORMS: &str = "use `<Signal> <op> <number>` (op: > >= < <= == !=) or `<Signal> between <a> <b>`";
    let s = expr.trim();
    if s.len() > MAX_EXPR_BYTES {
        return Err(format!("expression longer than {MAX_EXPR_BYTES} bytes"));
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(s.len());
    let signal = &s[..end];
    let valid_start = signal
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if !valid_start || signal.ends_with('.') || signal.matches('.').count() > 1 {
        return Err(format!("expected a signal name at the start; {FORMS}"));
    }
    let rest = s[end..].trim_start();
    let (op, args) = if let Some(r) = rest
        .strip_prefix("between")
        .filter(|r| r.starts_with(char::is_whitespace))
    {
        (Op::Between, r)
    } else {
        let op_len = rest
            .chars()
            .take_while(|c| matches!(c, '<' | '>' | '=' | '!'))
            .count();
        let op = Op::from_str(&rest[..op_len])
            .filter(|o| *o != Op::Between)
            .ok_or_else(|| format!("unknown or missing operator {:?}; {FORMS}", &rest[..op_len]))?;
        (op, &rest[op_len..])
    };
    let toks: Vec<&str> = args.split_whitespace().collect();
    let (value, hi) = match (op, toks.as_slice()) {
        (Op::Between, [a, b]) | (Op::Between, [a, "and", b]) => {
            let (a, b) = (number(a)?, number(b)?);
            if a > b {
                return Err(format!(
                    "between {a} {b}: the lower bound is larger than the upper"
                ));
            }
            (a, b)
        }
        (Op::Between, _) => return Err(format!("between needs two numbers; {FORMS}")),
        (_, [v]) => {
            let v = number(v)?;
            (v, v)
        }
        _ => return Err(format!("expected one number after {}; {FORMS}", op.as_str())),
    };
    Ok(Condition {
        signal: signal.to_owned(),
        op,
        value,
        hi,
        tol: 0.0,
    })
}

impl Condition {
    /// Whether a physical value satisfies the condition. NaN never does.
    pub fn holds(&self, v: f64) -> bool {
        if !v.is_finite() {
            return false;
        }
        let (x, t) = (self.value, self.tol);
        match self.op {
            Op::Gt => v > x - t,
            Op::Ge => v >= x - t,
            Op::Lt => v < x + t,
            Op::Le => v <= x + t,
            Op::Eq => (v - x).abs() <= t,
            Op::Ne => (v - x).abs() > t,
            Op::Between => v >= x - t && v <= self.hi + t,
        }
    }

    /// Expression text.
    pub fn text(&self) -> String {
        match self.op {
            Op::Between => format!("{} between {} {}", self.signal, self.value, self.hi),
            op => format!("{} {} {}", self.signal, op.as_str(), self.value),
        }
    }

    /// `can.signal_range` arguments, pinned to a DBC.
    pub fn args(&self, sha: &str, origin: Option<Origin>) -> Value {
        let mut a = match self.op {
            Op::Between => json!({"signal": self.signal, "between": [self.value, self.hi]}),
            op => json!({"signal": self.signal, "op": op.as_str(), "value": self.value}),
        };
        if self.tol > 0.0 {
            a["tol"] = json!(self.tol);
        }
        a["db_sha256"] = json!(sha);
        if let Some(o) = origin {
            a["origin"] = json!(o);
        }
        a
    }
}
