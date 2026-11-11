//! Evaluation over `Decoded::field_values`. Multi-value fields: a test is true if **any** value
//! satisfies it. `a != b` is evaluated as `not (a == b)`.

use std::borrow::Cow;
use std::cmp::Ordering;

use loom_eth_decode::{Decoded, FieldType, FieldValue};

use crate::EvalCtx;
use crate::compile::{Item, Lit, Node, Sel, Slice};
use crate::parser::Op;

pub(crate) fn eval(n: &Node, d: &Decoded, cx: &EvalCtx) -> bool {
    match n {
        Node::Or(v) => v.iter().any(|x| eval(x, d, cx)),
        Node::And(v) => v.iter().all(|x| eval(x, d, cx)),
        Node::Not(x) => !eval(x, d, cx),
        Node::Exists(s) => values(s, d, cx).next().is_some(),
        Node::Cmp(s, Op::Ne, lit) => !values(s, d, cx).any(|v| cmp(&v, lit) == Some(Ordering::Equal)),
        Node::Cmp(s, op, lit) => values(s, d, cx).any(|v| {
            let c = cmp(&v, lit);
            match op {
                Op::Eq => c == Some(Ordering::Equal),
                Op::Lt => c == Some(Ordering::Less),
                Op::Le => matches!(c, Some(Ordering::Less | Ordering::Equal)),
                Op::Gt => c == Some(Ordering::Greater),
                Op::Ge => matches!(c, Some(Ordering::Greater | Ordering::Equal)),
                Op::Ne => false,
            }
        }),
        Node::Contains(s, needle) => values(s, d, cx).any(|v| match as_bytes(&v) {
            Some(h) => needle.is_empty() || h.windows(needle.len()).any(|w| w == needle.as_slice()),
            None => false,
        }),
        Node::MatchStr(s, re) => values(s, d, cx).any(|v| match &v {
            Val::F(FieldValue::Str(t)) => re.is_match(t),
            _ => false,
        }),
        Node::MatchBytes(s, re) => values(s, d, cx).any(|v| as_bytes(&v).is_some_and(|b| re.is_match(&b))),
        Node::In(s, items) => values(s, d, cx).any(|v| {
            items.iter().any(|it| match it {
                Item::One(l) => cmp(&v, l) == Some(Ordering::Equal),
                Item::Range(lo, hi) => {
                    matches!(cmp(&v, lo), Some(Ordering::Greater | Ordering::Equal))
                        && matches!(cmp(&v, hi), Some(Ordering::Less | Ordering::Equal))
                }
            })
        }),
    }
}

/// A value under test: a decoded field value, a slice of one, or a computed time.
#[derive(Debug)]
enum Val {
    F(FieldValue),
    Bytes(Vec<u8>),
    Time(i128),
}

fn values<'a>(s: &'a Sel, d: &'a Decoded, cx: &'a EvalCtx) -> Box<dyn Iterator<Item = Val> + 'a> {
    if s.def.ty == FieldType::TimeRel {
        let ts = i128::from(d.meta.ts_ns);
        let v = match s.def.name {
            "frame.time_delta" => ts - i128::from(cx.prev_ts_ns.unwrap_or(d.meta.ts_ns)),
            _ => ts - i128::from(cx.time_ref_ns),
        };
        return Box::new(std::iter::once(Val::Time(v)));
    }
    let slice = s.slice;
    Box::new(d.field_values(s.def.name).filter_map(move |fv| match slice {
        None => Some(Val::F(fv)),
        Some(sl) => {
            let b = bytes_of(&fv)?;
            apply_slice(&b, sl).map(Val::Bytes)
        }
    }))
}

fn bytes_of(v: &FieldValue) -> Option<Cow<'_, [u8]>> {
    Some(match v {
        FieldValue::Bytes(b) => Cow::Borrowed(b.as_slice()),
        FieldValue::Str(s) => Cow::Borrowed(s.as_bytes()),
        FieldValue::Mac(m) => Cow::Borrowed(m.as_slice()),
        FieldValue::Ipv4(a) => Cow::Owned(a.octets().to_vec()),
        FieldValue::Ipv6(a) => Cow::Owned(a.octets().to_vec()),
        FieldValue::Uint(_) | FieldValue::Bool(_) => return None,
    })
}

fn as_bytes(v: &Val) -> Option<Cow<'_, [u8]>> {
    match v {
        Val::F(f) => bytes_of(f),
        Val::Bytes(b) => Some(Cow::Borrowed(b.as_slice())),
        Val::Time(_) => None,
    }
}

fn apply_slice(b: &[u8], s: Slice) -> Option<Vec<u8>> {
    let len = i64::try_from(b.len()).ok()?;
    let start = if s.start < 0 { len + s.start } else { s.start };
    if start < 0 || start > len {
        return None;
    }
    let start = usize::try_from(start).ok()?;
    let end = match s.len {
        None => b.len(),
        Some(n) => start.checked_add(usize::try_from(n).ok()?)?,
    };
    b.get(start..end).map(<[u8]>::to_vec)
}

/// Ordering of a value against a literal; None = not comparable (type mismatch).
fn cmp(v: &Val, lit: &Lit) -> Option<Ordering> {
    match (v, lit) {
        (Val::Time(t), Lit::Time(l)) => Some(t.cmp(l)),
        (Val::Bytes(b), Lit::Bytes(l)) => Some(b.as_slice().cmp(l.as_slice())),
        (Val::F(f), _) => match (f, lit) {
            (FieldValue::Uint(a), Lit::Uint(b)) => Some(a.cmp(b)),
            (FieldValue::Bool(a), Lit::Bool(b)) => Some(a.cmp(b)),
            (FieldValue::Bytes(a), Lit::Bytes(b)) => Some(a.as_slice().cmp(b.as_slice())),
            (FieldValue::Str(a), Lit::Str(b)) => Some(a.as_str().cmp(b.as_str())),
            (FieldValue::Mac(a), Lit::Mac(b)) => Some(a.cmp(b)),
            (FieldValue::Ipv4(a), Lit::V4(n)) => {
                if n.prefix_len() < 32 {
                    // CIDR: equal when inside the network, else order by address.
                    if n.contains(a) {
                        Some(Ordering::Equal)
                    } else {
                        Some(a.cmp(&n.network()))
                    }
                } else {
                    Some(a.cmp(&n.addr()))
                }
            }
            (FieldValue::Ipv6(a), Lit::V6(n)) => {
                if n.prefix_len() < 128 {
                    if n.contains(a) {
                        Some(Ordering::Equal)
                    } else {
                        Some(a.cmp(&n.network()))
                    }
                } else {
                    Some(a.cmp(&n.addr()))
                }
            }
            _ => None,
        },
        _ => None,
    }
}
