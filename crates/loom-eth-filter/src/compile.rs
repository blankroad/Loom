//! Type checking: resolves fields against the decode registry, parses literals for the field type,
//! checks operator/type combinations, and builds the compiled [`Node`] tree.

use std::net::{Ipv4Addr, Ipv6Addr};

use ipnet::{Ipv4Net, Ipv6Net};
use loom_eth_decode::{FieldDef, FieldType, field_def};
use regex::RegexBuilder;
use regex::bytes::RegexBuilder as BytesRegexBuilder;

use crate::parser::{Ast, FieldRef, ItemAst, Op, Rel, Value};
use crate::{FilterError, MAX_REGEX_LEN, REGEX_SIZE_LIMIT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lit {
    Uint(u64),
    Bool(bool),
    Bytes(Vec<u8>),
    Str(String),
    V4(Ipv4Net),
    V6(Ipv6Net),
    Mac([u8; 6]),
    /// Seconds as nanoseconds.
    Time(i128),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    One(Lit),
    Range(Lit, Lit),
}

/// `[start:len]`; `start` < 0 counts from the end; `len` None = to the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slice {
    pub start: i64,
    pub len: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Sel {
    pub def: &'static FieldDef,
    pub slice: Option<Slice>,
}

impl Sel {
    /// Effective value type after slicing (slices and protocols compare as bytes).
    pub(crate) fn ty(&self) -> FieldType {
        if self.slice.is_some() || self.def.ty == FieldType::Protocol {
            FieldType::Bytes
        } else {
            self.def.ty
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Or(Vec<Node>),
    And(Vec<Node>),
    Not(Box<Node>),
    Exists(Sel),
    Cmp(Sel, Op, Lit),
    Contains(Sel, Vec<u8>),
    MatchStr(Sel, regex::Regex),
    MatchBytes(Sel, regex::bytes::Regex),
    In(Sel, Vec<Item>),
}

fn err(pos: usize, expected: &str, msg: impl Into<String>) -> FilterError {
    FilterError {
        pos,
        expected: expected.into(),
        msg: msg.into(),
    }
}

pub(crate) fn compile(ast: Ast, used: &mut Vec<&'static str>) -> Result<Node, FilterError> {
    Ok(match ast {
        Ast::Or(v) => Node::Or(
            v.into_iter()
                .map(|a| compile(a, used))
                .collect::<Result<_, _>>()?,
        ),
        Ast::And(v) => Node::And(
            v.into_iter()
                .map(|a| compile(a, used))
                .collect::<Result<_, _>>()?,
        ),
        Ast::Not(a) => Node::Not(Box::new(compile(*a, used)?)),
        Ast::Test(f, rel) => test(f, rel, used)?,
    })
}

fn sel(f: &FieldRef, used: &mut Vec<&'static str>) -> Result<Sel, FilterError> {
    let Some(def) = field_def(&f.name) else {
        return Err(err(
            f.pos,
            "a known field name",
            format!("unknown field `{}`", f.name),
        ));
    };
    if !used.contains(&def.name) {
        used.push(def.name);
    }
    let slice = match &f.slice {
        None => None,
        Some((spec, pos)) => {
            if matches!(
                def.ty,
                FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64
            ) || matches!(def.ty, FieldType::Bool | FieldType::TimeRel)
            {
                return Err(err(
                    *pos,
                    "a byte-valued field",
                    format!("`{}` cannot be sliced", def.name),
                ));
            }
            Some(parse_slice(spec, *pos)?)
        }
    };
    Ok(Sel { def, slice })
}

fn parse_slice(spec: &str, pos: usize) -> Result<Slice, FilterError> {
    let bad = || {
        err(
            pos + 1,
            "a slice like 0:3, :2, 4:, 2-4 or 5",
            format!("bad slice `{spec}`"),
        )
    };
    let int = |s: &str| -> Result<i64, FilterError> { s.parse::<i64>().map_err(|_| bad()) };
    if let Some((a, b)) = spec.split_once(':') {
        let start = if a.is_empty() { 0 } else { int(a)? };
        let len = if b.is_empty() {
            None
        } else {
            Some(u64::try_from(int(b)?).map_err(|_| bad())?)
        };
        return Ok(Slice { start, len });
    }
    // `a-b` (inclusive range); a leading `-` alone means a negative offset.
    if let Some((a, b)) = spec
        .get(1..)
        .and_then(|r| r.split_once('-'))
        .map(|(x, y)| (&spec[..x.len() + 1], y))
    {
        let (s, e) = (int(a)?, int(b)?);
        if e < s || s < 0 {
            return Err(bad());
        }
        return Ok(Slice {
            start: s,
            len: Some(u64::try_from(e - s + 1).map_err(|_| bad())?),
        });
    }
    Ok(Slice {
        start: int(spec)?,
        len: Some(1),
    })
}

fn test(f: FieldRef, rel: Option<Rel>, used: &mut Vec<&'static str>) -> Result<Node, FilterError> {
    let s = sel(&f, used)?;
    let ty = s.ty();
    let Some(rel) = rel else {
        return Ok(Node::Exists(s));
    };
    match rel {
        Rel::Cmp(op, v, opos) => {
            if matches!(op, Op::Lt | Op::Le | Op::Gt | Op::Ge) && ty == FieldType::Bool {
                return Err(err(
                    opos,
                    "== or !=",
                    format!("`{}` is boolean; ordering is not defined", s.def.name),
                ));
            }
            let lit = literal(ty, &v)?;
            Ok(Node::Cmp(s, op, lit))
        }
        Rel::Contains(v, opos) => {
            let needle = match ty {
                FieldType::Bytes => match literal(FieldType::Bytes, &v)? {
                    Lit::Bytes(b) => b,
                    _ => return Err(err(v.pos, "bytes or a string", "bad contains value")),
                },
                FieldType::Str => v.bytes.clone(),
                _ => {
                    return Err(err(
                        opos,
                        "a string or byte field",
                        format!("`contains` not defined for `{}`", s.def.name),
                    ));
                }
            };
            Ok(Node::Contains(s, needle))
        }
        Rel::Matches(v, opos) => {
            if !v.quoted {
                return Err(err(
                    v.pos,
                    "a quoted regular expression",
                    "`matches` needs a quoted string",
                ));
            }
            if v.raw.len() > MAX_REGEX_LEN {
                return Err(err(
                    v.pos,
                    "a shorter pattern",
                    format!("pattern longer than {MAX_REGEX_LEN} bytes"),
                ));
            }
            match ty {
                FieldType::Str => {
                    let re = RegexBuilder::new(&v.raw)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(REGEX_SIZE_LIMIT)
                        .build()
                        .map_err(|e| err(v.pos, "a valid regular expression", e.to_string()))?;
                    Ok(Node::MatchStr(s, re))
                }
                FieldType::Bytes => {
                    let re = BytesRegexBuilder::new(&v.raw)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(REGEX_SIZE_LIMIT)
                        .unicode(false)
                        .build()
                        .map_err(|e| err(v.pos, "a valid regular expression", e.to_string()))?;
                    Ok(Node::MatchBytes(s, re))
                }
                _ => Err(err(
                    opos,
                    "a string or byte field",
                    format!("`matches` not defined for `{}`", s.def.name),
                )),
            }
        }
        Rel::In(items, opos) => {
            let orderable = !matches!(ty, FieldType::Bool);
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(match it {
                    ItemAst::One(v) => Item::One(literal(ty, &v)?),
                    ItemAst::Range(a, b) => {
                        if !orderable {
                            return Err(err(opos, "single values", "ranges are not defined for booleans"));
                        }
                        let (lo, hi) = (literal(ty, &a)?, literal(ty, &b)?);
                        Item::Range(lo, hi)
                    }
                });
            }
            Ok(Node::In(s, out))
        }
    }
}

/// Parses a literal for a field type. An unquoted word that names a field (and does not parse
/// as a literal) is reported as an unsupported field-to-field comparison.
fn literal(ty: FieldType, v: &Value) -> Result<Lit, FilterError> {
    if let (true, Some(p)) = (v.quoted, v.bad_escape) {
        return Err(err(
            p,
            "\\\" \\\\ \\n \\t \\r \\0 \\xHH",
            "unknown escape in string",
        ));
    }
    let r = parse_lit(ty, v);
    if r.is_err() && !v.quoted && field_def(&v.text).is_some() {
        return Err(err(v.pos, "a literal", "comparing two fields is not supported"));
    }
    r
}

fn parse_lit(ty: FieldType, v: &Value) -> Result<Lit, FilterError> {
    let t = v.text.as_str();
    let bad = |what: &str| err(v.pos, what, format!("`{t}` is not {what}"));
    match ty {
        FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => {
            if v.quoted {
                return Err(bad("an integer"));
            }
            let n = parse_uint(t).ok_or_else(|| bad("an integer"))?;
            let max = ty.uint_max().unwrap_or(u64::MAX);
            if n > max {
                return Err(err(
                    v.pos,
                    "a smaller integer",
                    format!("{n} is larger than {max}"),
                ));
            }
            Ok(Lit::Uint(n))
        }
        FieldType::Bool => match t.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Lit::Bool(true)),
            "false" | "0" => Ok(Lit::Bool(false)),
            _ => Err(bad("a boolean (true, false, 1, 0)")),
        },
        FieldType::Bytes | FieldType::Protocol => {
            if v.quoted {
                return Ok(Lit::Bytes(v.bytes.clone()));
            }
            parse_bytes(t)
                .map(Lit::Bytes)
                .ok_or_else(|| bad("a byte string (aa:bb:cc)"))
        }
        FieldType::Str => Ok(Lit::Str(t.to_string())),
        FieldType::Ipv4 => {
            let net = if t.contains('/') {
                t.parse::<Ipv4Net>().ok()
            } else {
                t.parse::<Ipv4Addr>().ok().map(Ipv4Net::from)
            };
            net.map(Lit::V4).ok_or_else(|| bad("an IPv4 address or CIDR"))
        }
        FieldType::Ipv6 => {
            let net = if t.contains('/') {
                t.parse::<Ipv6Net>().ok()
            } else {
                t.parse::<Ipv6Addr>().ok().map(Ipv6Net::from)
            };
            net.map(Lit::V6).ok_or_else(|| bad("an IPv6 address or CIDR"))
        }
        FieldType::Mac => parse_bytes(t)
            .and_then(|b| <[u8; 6]>::try_from(b.as_slice()).ok())
            .map(Lit::Mac)
            .ok_or_else(|| bad("a MAC address (aa:bb:cc:dd:ee:ff)")),
        FieldType::TimeRel => parse_secs(t)
            .map(Lit::Time)
            .ok_or_else(|| bad("seconds (e.g. 1.5)")),
    }
}

fn parse_uint(t: &str) -> Option<u64> {
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16).ok();
    }
    if let Some(b) = t.strip_prefix("0b") {
        return u64::from_str_radix(b, 2).ok();
    }
    if !t.bytes().all(|c| c.is_ascii_digit()) || t.is_empty() {
        return None;
    }
    t.parse::<u64>().ok()
}

/// `aa:bb:cc`, `aa-bb-cc`, `aa.bb.cc` (1–2 hex digits per group) or contiguous even-length hex.
pub(crate) fn parse_bytes(t: &str) -> Option<Vec<u8>> {
    if t.is_empty() {
        return None;
    }
    let sep = t.chars().find(|c| matches!(c, ':' | '-' | '.'));
    match sep {
        Some(s) => t
            .split(s)
            .map(|g| {
                if (1..=2).contains(&g.len()) {
                    u8::from_str_radix(g, 16).ok()
                } else {
                    None
                }
            })
            .collect(),
        None => {
            if !t.len().is_multiple_of(2) {
                return None;
            }
            (0..t.len())
                .step_by(2)
                .map(|i| t.get(i..i + 2).and_then(|g| u8::from_str_radix(g, 16).ok()))
                .collect()
        }
    }
}

/// Decimal seconds → nanoseconds (`1`, `1.5`, `-0.25`, up to 9 fractional digits).
fn parse_secs(t: &str) -> Option<i128> {
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if !int.bytes().all(|c| c.is_ascii_digit()) || !frac.bytes().all(|c| c.is_ascii_digit()) || frac.len() > 9
    {
        return None;
    }
    let i: i128 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut f: i128 = if frac.is_empty() { 0 } else { frac.parse().ok()? };
    for _ in frac.len()..9 {
        f *= 10;
    }
    let ns = i.checked_mul(1_000_000_000)?.checked_add(f)?;
    Some(if neg { -ns } else { ns })
}
