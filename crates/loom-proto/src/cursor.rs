//! Cursors (contract v1 §4.5, D5·A7·CR-C3).
//!
//! Grammar (wire form is always the string):
//!
//! | form | example | variant |
//! |---|---|---|
//! | `@last` | `@last` | [`Cursor::Last`] — this client's stored cursor |
//! | `now` | `now` | [`Cursor::Now`] — hub high-water at request time |
//! | `seq:N` / `e<E>:seq:N` | `e3:seq:88124` | [`Cursor::Seq`] |
//! | `mark:<label>` | `mark:boot` | [`Cursor::Mark`] |
//! | `-<n><unit>` (unit `ms`,`s`,`m`,`h`) | `-5s` | [`Cursor::Relative`] |
//! | `HH:MM:SS[.mmm]` | `14:02:11.250` | [`Cursor::Clock`] |
//! | ISO 8601 date-time | `2026-11-10T14:02:11.250Z` | [`Cursor::Iso8601`] (shape checked only) |
//! | `<prefix>:<value>` | `off:1024`, `send:last` | [`Cursor::Domain`] (A7 extension) |
//!
//! Numbers follow the canonical rule of [`crate::num`] (no leading zeros). Labels and domain
//! values are 1–128 printable ASCII characters without whitespace; domain values additionally may
//! not contain `:` (so `e:seq:1` is rejected instead of becoming a domain cursor).

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ProtoError;
use crate::event::is_token;
use crate::num::parse_u64_canonical;

/// Unit of a relative cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    /// Milliseconds (`ms`).
    Millis,
    /// Seconds (`s`).
    Secs,
    /// Minutes (`m`).
    Mins,
    /// Hours (`h`).
    Hours,
}

impl TimeUnit {
    fn suffix(self) -> &'static str {
        match self {
            TimeUnit::Millis => "ms",
            TimeUnit::Secs => "s",
            TimeUnit::Mins => "m",
            TimeUnit::Hours => "h",
        }
    }
}

/// Time of day (host local time zone of the session, stated in results).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClockTime {
    /// Hour 0–23.
    pub hour: u8,
    /// Minute 0–59.
    pub minute: u8,
    /// Second 0–59.
    pub second: u8,
    /// Milliseconds 0–999 when given (always 3 digits on the wire).
    pub millis: Option<u16>,
}

/// A position in the event stream (see module docs for the grammar).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// `@last` — this client's cursor.
    Last,
    /// `now` — bound atomically to the hub high-water when the request is accepted.
    Now,
    /// `seq:N`, optionally bound to a stream epoch (`e3:seq:N`).
    Seq {
        /// Stream epoch, when explicit.
        epoch: Option<u64>,
        /// Stream seq.
        seq: u64,
    },
    /// `mark:<label>`.
    Mark(String),
    /// `-<amount><unit>` — that long before now.
    Relative {
        /// Amount (positive; the sign is implied).
        amount: u64,
        /// Unit.
        unit: TimeUnit,
    },
    /// `HH:MM:SS[.mmm]`.
    Clock(ClockTime),
    /// ISO 8601 date-time, shape-checked, kept verbatim.
    Iso8601(String),
    /// Domain extension `<prefix>:<value>` (e.g. UART `off:N`, `send:last`).
    Domain {
        /// Prefix token (not one of the reserved common prefixes).
        prefix: String,
        /// Value.
        value: String,
    },
}

/// Prefixes owned by the common grammar; domain extensions may not use them.
pub const RESERVED_CURSOR_PREFIXES: &[&str] = &["seq", "mark", "now", "last"];

impl Cursor {
    /// Duration of a relative cursor (saturating).
    pub fn relative_duration(&self) -> Option<Duration> {
        match self {
            Cursor::Relative { amount, unit } => Some(match unit {
                TimeUnit::Millis => Duration::from_millis(*amount),
                TimeUnit::Secs => Duration::from_secs(*amount),
                TimeUnit::Mins => Duration::from_secs(amount.saturating_mul(60)),
                TimeUnit::Hours => Duration::from_secs(amount.saturating_mul(3600)),
            }),
            _ => None,
        }
    }
}

fn bad(s: &str, why: &str) -> ProtoError {
    let shown: String = s.chars().take(64).collect();
    ProtoError::InvalidCursor(format!("{why}: {shown:?}"))
}

fn valid_label(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|c| c.is_ascii_graphic())
}

fn two_digits(s: &str, max: u8) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() != 2 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = (b[0] - b'0') * 10 + (b[1] - b'0');
    (v <= max).then_some(v)
}

fn parse_clock(s: &str) -> Option<ClockTime> {
    let (hms, ms) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let mut it = hms.split(':');
    let (h, m, sec) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    let millis = match ms {
        None => None,
        Some(f) if f.len() == 3 && f.bytes().all(|c| c.is_ascii_digit()) => f.parse::<u16>().ok(),
        Some(_) => return None,
    };
    Some(ClockTime {
        hour: two_digits(h, 23)?,
        minute: two_digits(m, 59)?,
        second: two_digits(sec, 59)?,
        millis,
    })
}

fn digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|c| c.is_ascii_digit())
}

/// Shape check: `YYYY-MM-DDTHH:MM:SS[.f{1,9}][Z|±HH:MM]`.
fn is_iso8601(s: &str) -> bool {
    let Some((date, time)) = s.split_once('T') else {
        return false;
    };
    let d: Vec<&str> = date.split('-').collect();
    if d.len() != 3 || !digits(d[0], 4) || two_digits(d[1], 12).is_none() || two_digits(d[2], 31).is_none() {
        return false;
    }
    if matches!(two_digits(d[1], 12), Some(0)) || matches!(two_digits(d[2], 31), Some(0)) {
        return false;
    }
    let (clock, zone) = if let Some(c) = time.strip_suffix('Z') {
        (c, None)
    } else if let Some(i) = time.rfind(['+', '-']) {
        (&time[..i], Some(&time[i + 1..]))
    } else {
        (time, None)
    };
    if let Some(z) = zone {
        let Some((zh, zm)) = z.split_once(':') else {
            return false;
        };
        if two_digits(zh, 23).is_none() || two_digits(zm, 59).is_none() {
            return false;
        }
    }
    let (hms, frac) = match clock.split_once('.') {
        Some((a, f)) => (a, Some(f)),
        None => (clock, None),
    };
    if let Some(f) = frac
        && (f.is_empty() || f.len() > 9 || !f.bytes().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    parse_clock(hms).is_some()
}

impl FromStr for Cursor {
    type Err = ProtoError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 256 {
            return Err(bad(s, "length"));
        }
        match s {
            "@last" => return Ok(Cursor::Last),
            "now" => return Ok(Cursor::Now),
            _ => {}
        }
        if let Some(rest) = s.strip_prefix('-') {
            let split = rest
                .find(|c: char| !c.is_ascii_digit())
                .ok_or_else(|| bad(s, "missing unit"))?;
            let (num, unit) = rest.split_at(split);
            let amount = parse_u64_canonical(num).map_err(|_| bad(s, "bad relative amount"))?;
            let unit = match unit {
                "ms" => TimeUnit::Millis,
                "s" => TimeUnit::Secs,
                "m" => TimeUnit::Mins,
                "h" => TimeUnit::Hours,
                _ => return Err(bad(s, "unit must be ms|s|m|h")),
            };
            return Ok(Cursor::Relative { amount, unit });
        }
        if s.contains('T') && s.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return if is_iso8601(s) {
                Ok(Cursor::Iso8601(s.to_owned()))
            } else {
                Err(bad(s, "bad ISO 8601"))
            };
        }
        if s.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return parse_clock(s)
                .map(Cursor::Clock)
                .ok_or_else(|| bad(s, "bad HH:MM:SS[.mmm]"));
        }
        let (prefix, value) = s.split_once(':').ok_or_else(|| bad(s, "unknown cursor"))?;
        if let Some(e) = prefix.strip_prefix('e')
            && e.bytes().next().is_some_and(|c| c.is_ascii_digit())
        {
            let epoch = parse_u64_canonical(e).map_err(|_| bad(s, "bad epoch"))?;
            let seq = value
                .strip_prefix("seq:")
                .ok_or_else(|| bad(s, "expected e<epoch>:seq:<n>"))?;
            let seq = parse_u64_canonical(seq).map_err(|_| bad(s, "bad seq"))?;
            return Ok(Cursor::Seq {
                epoch: Some(epoch),
                seq,
            });
        }
        match prefix {
            "seq" => {
                let seq = parse_u64_canonical(value).map_err(|_| bad(s, "bad seq"))?;
                Ok(Cursor::Seq { epoch: None, seq })
            }
            "mark" if valid_label(value) => Ok(Cursor::Mark(value.to_owned())),
            "mark" => Err(bad(s, "bad mark label")),
            p if is_token(p)
                && !RESERVED_CURSOR_PREFIXES.contains(&p)
                && valid_label(value)
                && !value.contains(':') =>
            {
                Ok(Cursor::Domain {
                    prefix: p.to_owned(),
                    value: value.to_owned(),
                })
            }
            _ => Err(bad(s, "unknown cursor prefix")),
        }
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cursor::Last => f.write_str("@last"),
            Cursor::Now => f.write_str("now"),
            Cursor::Seq { epoch: None, seq } => write!(f, "seq:{seq}"),
            Cursor::Seq { epoch: Some(e), seq } => write!(f, "e{e}:seq:{seq}"),
            Cursor::Mark(l) => write!(f, "mark:{l}"),
            Cursor::Relative { amount, unit } => write!(f, "-{amount}{}", unit.suffix()),
            Cursor::Clock(c) => {
                write!(f, "{:02}:{:02}:{:02}", c.hour, c.minute, c.second)?;
                match c.millis {
                    Some(ms) => write!(f, ".{ms:03}"),
                    None => Ok(()),
                }
            }
            Cursor::Iso8601(s) => f.write_str(s),
            Cursor::Domain { prefix, value } => write!(f, "{prefix}:{value}"),
        }
    }
}

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cursor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <Cow<'de, str>>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Cursor {
    fn schema_name() -> Cow<'static, str> {
        "Cursor".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 256,
            "description": "Cursor: @last | now | seq:N | e<E>:seq:N | mark:X | -5s | HH:MM:SS[.mmm] | ISO8601 | <prefix>:<value>"
        })
    }
}
