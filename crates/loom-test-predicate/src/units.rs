//! Numbers and units. JSON numbers and canonical `U64` decimal strings (contract §6.1) are both
//! numbers; integers compare exactly, everything else as `f64`. Quantities like `"5 s"`,
//! `"150 ms"`, `"13.5 V"` are converted to the unit the domain registered for the field
//! (`PointerSpec.unit`); different dimensions are an error.

use std::cmp::Ordering;

use serde_json::Value;

/// A number read from JSON.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Num {
    /// Integer (exact).
    Int(i128),
    /// Floating point (finite).
    Float(f64),
}

impl Num {
    /// As `f64`.
    pub fn f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }

    /// Total order for finite numbers (integers exactly).
    pub fn compare(self, other: Num) -> Ordering {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => a.cmp(&b),
            (a, b) => a.f64().partial_cmp(&b.f64()).unwrap_or(Ordering::Equal),
        }
    }

    /// Scale by a unit factor (exact when it stays integral).
    pub fn scaled(self, factor: f64) -> Num {
        if factor == 1.0 {
            return self;
        }
        let f = self.f64() * factor;
        if f.fract() == 0.0 && f.abs() < 9.0e15 {
            Num::Int(f as i128)
        } else {
            Num::Float(f)
        }
    }
}

/// Canonical unsigned decimal (no sign, no leading zeros except "0"), as `U64` strings are.
fn canonical_u64(s: &str) -> Option<u64> {
    if s.is_empty()
        || s.len() > 20
        || !s.bytes().all(|b| b.is_ascii_digit())
        || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok()
}

/// Reads a JSON number or a `U64` string. `None` for anything else (incl. NaN-like values).
pub fn json_num(v: &Value) -> Option<Num> {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Num::Int(i128::from(i)))
            } else if let Some(u) = n.as_u64() {
                Some(Num::Int(i128::from(u)))
            } else {
                n.as_f64().filter(|f| f.is_finite()).map(Num::Float)
            }
        }
        Value::String(s) => canonical_u64(s).map(|u| Num::Int(i128::from(u))),
        _ => None,
    }
}

/// Dimension of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dim {
    /// Seconds.
    Time,
    /// Volts.
    Voltage,
    /// Amperes.
    Current,
    /// Watts.
    Power,
    /// Hertz (and `1/s`).
    Frequency,
    /// Bytes.
    Bytes,
    /// Bytes per second.
    ByteRate,
    /// Dimensionless (e.g. `%`).
    Other,
}

/// `(dimension, factor to the base unit)` of a unit string.
pub fn unit(u: &str) -> Option<(Dim, f64)> {
    let u = u.trim();
    let fixed = match u {
        "ns" => Some((Dim::Time, 1e-9)),
        "us" | "µs" => Some((Dim::Time, 1e-6)),
        "ms" => Some((Dim::Time, 1e-3)),
        "s" => Some((Dim::Time, 1.0)),
        "min" => Some((Dim::Time, 60.0)),
        "h" => Some((Dim::Time, 3600.0)),
        "1/s" => Some((Dim::Frequency, 1.0)),
        "B" => Some((Dim::Bytes, 1.0)),
        "KiB" => Some((Dim::Bytes, 1024.0)),
        "MiB" => Some((Dim::Bytes, 1024.0 * 1024.0)),
        "B/s" => Some((Dim::ByteRate, 1.0)),
        "%" => Some((Dim::Other, 1.0)),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    // SI prefix + V/A/W/Hz.
    let bases: [(&str, Dim); 4] = [
        ("Hz", Dim::Frequency),
        ("V", Dim::Voltage),
        ("A", Dim::Current),
        ("W", Dim::Power),
    ];
    for (b, d) in bases {
        if let Some(p) = u.strip_suffix(b) {
            let f = match p {
                "" => 1.0,
                "u" | "µ" => 1e-6,
                "m" => 1e-3,
                "k" => 1e3,
                "M" => 1e6,
                _ => return None,
            };
            return Some((d, f));
        }
    }
    None
}

/// `"5 s"` / `"150ms"` / `"13.5 V"` → (number, unit). `None` if not a quantity string.
pub fn parse_quantity(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    let end =
        s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))?;
    let (n, u) = s.split_at(end);
    let n: f64 = n.trim().parse().ok().filter(|x: &f64| x.is_finite())?;
    let u = u.trim();
    unit(u)?;
    Some((n, u.to_owned()))
}

/// Converts `value` in unit `from` to unit `to`. `Err` when the dimensions differ.
pub fn convert(value: f64, from: &str, to: &str) -> Result<Num, String> {
    let (df, ff) = unit(from).ok_or_else(|| format!("unknown unit {from:?}"))?;
    let (dt, ft) = unit(to).ok_or_else(|| format!("unknown unit {to:?}"))?;
    if df != dt {
        return Err(format!("unit {from} is not comparable with {to}"));
    }
    Ok(Num::Float(value).scaled(ff / ft))
}

/// Milliseconds from a JSON number (ms) or a time quantity string (`"5 s"`).
pub fn duration_ms(v: &Value) -> Result<u64, String> {
    let ms = match v {
        Value::String(s) => {
            let (n, u) =
                parse_quantity(s).ok_or_else(|| format!("expected a duration like \"5 s\", got {s:?}"))?;
            convert(n, &u, "ms")?.f64()
        }
        other => json_num(other)
            .ok_or_else(|| format!("expected milliseconds or \"5 s\", got {other}"))?
            .f64(),
    };
    if !(0.0..=crate::limits::MAX_DURATION_MS as f64).contains(&ms) {
        return Err(format!(
            "duration {ms} ms out of range 0..={}",
            crate::limits::MAX_DURATION_MS
        ));
    }
    Ok(ms.round() as u64)
}
