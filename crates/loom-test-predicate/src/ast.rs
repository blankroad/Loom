//! Comparison AST (TEST plan §6.3, decision T3): `{field: <JSON Pointer>, op, value}` with
//! `all[]`, `any[]`, `not`. Parsed from `serde_json::Value` (plan YAML is converted to JSON
//! first). Fields are read from the published record JSON (`02-events` §2.2).
//!
//! A missing field or a value of the wrong type is an **error**, never `false` (contract 07
//! §7.4); the caller turns it into `Inconclusive`.

use std::cmp::Ordering;
use std::fmt;

use loom_domain_api::{PointerSpec, ValueType};
use loom_proto::event::payload_bytes;
use loom_proto::evidence::validate_json_pointer;
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};

use crate::limits::*;
use crate::units::{Num, convert, json_num, parse_quantity};

/// Parse / type-check error with the location in the plan.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {msg} (expected {expected}, got {actual})")]
pub struct PredError {
    /// Location, e.g. `steps[2].expect[0].pred.where.all[1].value`.
    pub path: String,
    /// What was expected.
    pub expected: String,
    /// What was found (shortened).
    pub actual: String,
    /// Message.
    pub msg: String,
}

/// Shortened JSON text for error messages.
pub fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 80 {
        format!("{}…", s.chars().take(79).collect::<String>())
    } else {
        s
    }
}

pub(crate) fn perr(path: &str, expected: &str, actual: &Value, msg: impl Into<String>) -> PredError {
    PredError {
        path: path.to_owned(),
        expected: expected.to_owned(),
        actual: short(actual),
        msg: msg.into(),
    }
}

/// Evaluation error (missing field, wrong type …). Becomes `Inconclusive` with the seq.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// The field is not in the record.
    #[error("{field}: missing")]
    Missing {
        /// JSON Pointer.
        field: String,
    },
    /// The field has another type.
    #[error("{field}: expected {expected}, got {actual}")]
    Type {
        /// JSON Pointer.
        field: String,
        /// Expected type.
        expected: String,
        /// Actual value (shortened).
        actual: String,
    },
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `==`
    Eq,
    /// `!=` (= `not ==`; a missing field is still an error)
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `in` (list)
    In,
    /// `regex` (Rust `regex`, unanchored)
    Regex,
    /// `contains` (substring or array element)
    Contains,
    /// `between` `[lo, hi]` inclusive
    Between,
    /// `bytes_eq` (hex)
    BytesEq,
}

impl Op {
    /// Wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::In => "in",
            Op::Regex => "regex",
            Op::Contains => "contains",
            Op::Between => "between",
            Op::BytesEq => "bytes_eq",
        }
    }

    fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "==" | "eq" => Op::Eq,
            "!=" | "ne" => Op::Ne,
            "<" | "lt" => Op::Lt,
            "<=" | "le" => Op::Le,
            ">" | "gt" => Op::Gt,
            ">=" | "ge" => Op::Ge,
            "in" => Op::In,
            "regex" => Op::Regex,
            "contains" => Op::Contains,
            "between" => Op::Between,
            "bytes_eq" => Op::BytesEq,
            _ => return None,
        })
    }

    fn numeric(self) -> bool {
        matches!(self, Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::Between)
    }
}

/// Literal value.
#[derive(Debug, Clone)]
pub enum Lit {
    /// Number.
    Num(Num),
    /// Number with a unit (`"5 s"`); `raw` keeps the text for string fields.
    Quantity {
        /// Value.
        value: f64,
        /// Unit.
        unit: String,
        /// Original text.
        raw: String,
    },
    /// String.
    Str(String),
    /// Boolean.
    Bool(bool),
    /// `null`.
    Null,
    /// `in` list.
    List(Vec<Lit>),
    /// `between` bounds.
    Range(Box<Lit>, Box<Lit>),
    /// `bytes_eq` bytes.
    Bytes(Vec<u8>),
    /// Compiled regex.
    Regex(Regex),
}

/// One comparison.
#[derive(Debug, Clone)]
pub struct Cmp {
    /// JSON Pointer into the record.
    pub field: String,
    /// Operator.
    pub op: Op,
    /// Literal.
    pub value: Lit,
    /// Literal as written (for rendering and digests).
    pub raw: Value,
    /// Unit of the field, set by [`Expr::check`] from the domain's `PointerSpec`.
    pub field_unit: Option<String>,
}

/// Comparison AST.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Comparison.
    Cmp(Cmp),
    /// All must hold (empty = true).
    All(Vec<Expr>),
    /// One must hold (empty = false).
    Any(Vec<Expr>),
    /// Negation.
    Not(Box<Expr>),
}

struct Parser {
    nodes: usize,
}

fn lit_scalar(v: &Value, path: &str) -> Result<Lit, PredError> {
    match v {
        Value::Null => Ok(Lit::Null),
        Value::Bool(b) => Ok(Lit::Bool(*b)),
        Value::Number(_) => json_num(v)
            .map(Lit::Num)
            .ok_or_else(|| perr(path, "finite number", v, "bad number")),
        Value::String(s) => {
            if s.len() > MAX_STRING_LEN {
                return Err(perr(
                    path,
                    &format!("string ≤ {MAX_STRING_LEN} B"),
                    v,
                    "string too long",
                ));
            }
            // A canonical U64 decimal string (contract §6.1) is a number.
            if let Some(n) = json_num(v) {
                return Ok(Lit::Num(n));
            }
            Ok(match parse_quantity(s) {
                Some((value, unit)) => Lit::Quantity {
                    value,
                    unit,
                    raw: s.clone(),
                },
                None => Lit::Str(s.clone()),
            })
        }
        _ => Err(perr(
            path,
            "scalar (number, string, bool, null)",
            v,
            "not a scalar",
        )),
    }
}

fn lit_number(v: &Value, path: &str) -> Result<Lit, PredError> {
    match lit_scalar(v, path)? {
        l @ (Lit::Num(_) | Lit::Quantity { .. }) => Ok(l),
        _ => Err(perr(path, "number or quantity like \"5 s\"", v, "not a number")),
    }
}

/// Hex text (spaces, `:` and `-` allowed) → bytes.
pub fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = s.bytes().filter(|b| !matches!(b, b' ' | b':' | b'-')).collect();
    if !clean.len().is_multiple_of(2) || clean.len() / 2 > MAX_BYTES_LEN {
        return None;
    }
    clean
        .chunks(2)
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|t| u8::from_str_radix(t, 16).ok())
        })
        .collect()
}

impl Parser {
    fn node(&mut self, path: &str, v: &Value) -> Result<(), PredError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(perr(
                path,
                &format!("≤ {MAX_NODES} nodes"),
                v,
                "predicate too large",
            ));
        }
        Ok(())
    }

    fn expr(&mut self, v: &Value, path: &str, depth: usize) -> Result<Expr, PredError> {
        if depth > MAX_DEPTH {
            return Err(perr(
                path,
                &format!("depth ≤ {MAX_DEPTH}"),
                v,
                "predicate nested too deep",
            ));
        }
        self.node(path, v)?;
        let Some(o) = v.as_object() else {
            return Err(perr(
                path,
                "object {field, op, value} | {all} | {any} | {not}",
                v,
                "not an object",
            ));
        };
        let logic = |k: &str| o.get(k);
        if let Some(items) = logic("all").or_else(|| logic("any")) {
            let key = if o.contains_key("all") { "all" } else { "any" };
            if o.len() != 1 {
                return Err(perr(
                    path,
                    &format!("only `{key}`"),
                    v,
                    "extra keys next to a logic node",
                ));
            }
            let arr = items
                .as_array()
                .ok_or_else(|| perr(&format!("{path}.{key}"), "array", items, "not an array"))?;
            let mut out = Vec::with_capacity(arr.len());
            for (i, e) in arr.iter().enumerate() {
                out.push(self.expr(e, &format!("{path}.{key}[{i}]"), depth + 1)?);
            }
            return Ok(if key == "all" {
                Expr::All(out)
            } else {
                Expr::Any(out)
            });
        }
        if let Some(inner) = logic("not") {
            if o.len() != 1 {
                return Err(perr(path, "only `not`", v, "extra keys next to `not`"));
            }
            return Ok(Expr::Not(Box::new(self.expr(
                inner,
                &format!("{path}.not"),
                depth + 1,
            )?)));
        }
        for k in o.keys() {
            if !matches!(k.as_str(), "field" | "op" | "value") {
                return Err(perr(
                    &format!("{path}.{k}"),
                    "field | op | value | all | any | not",
                    &json!(k),
                    "unknown key",
                ));
            }
        }
        let fv = o
            .get("field")
            .ok_or_else(|| perr(path, "`field`", v, "missing field"))?;
        let field = fv.as_str().ok_or_else(|| {
            perr(
                &format!("{path}.field"),
                "JSON Pointer string",
                fv,
                "not a string",
            )
        })?;
        if field.len() > MAX_POINTER_LEN || field.is_empty() || validate_json_pointer(field).is_err() {
            return Err(perr(
                &format!("{path}.field"),
                "JSON Pointer like /payload/x",
                fv,
                "bad pointer",
            ));
        }
        let ov = o.get("op").ok_or_else(|| perr(path, "`op`", v, "missing op"))?;
        let op = ov.as_str().and_then(Op::parse).ok_or_else(|| {
            perr(
                &format!("{path}.op"),
                "== != < <= > >= in regex contains between bytes_eq",
                ov,
                "unknown operator",
            )
        })?;
        let raw = o
            .get("value")
            .cloned()
            .ok_or_else(|| perr(path, "`value`", v, "missing value"))?;
        let vp = format!("{path}.value");
        let value =
            match op {
                Op::Eq | Op::Ne => lit_scalar(&raw, &vp)?,
                Op::Lt | Op::Le | Op::Gt | Op::Ge => lit_number(&raw, &vp)?,
                Op::Contains => match lit_scalar(&raw, &vp)? {
                    Lit::Quantity { raw: s, .. } => Lit::Str(s),
                    l => l,
                },
                Op::In => {
                    let arr = raw
                        .as_array()
                        .ok_or_else(|| perr(&vp, "array", &raw, "`in` needs a list"))?;
                    if arr.len() > MAX_IN_ITEMS {
                        return Err(perr(
                            &vp,
                            &format!("≤ {MAX_IN_ITEMS} items"),
                            &raw,
                            "list too long",
                        ));
                    }
                    let mut items = Vec::with_capacity(arr.len());
                    for (i, x) in arr.iter().enumerate() {
                        items.push(lit_scalar(x, &format!("{vp}[{i}]"))?);
                    }
                    Lit::List(items)
                }
                Op::Between => {
                    let arr = raw
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .ok_or_else(|| perr(&vp, "[lo, hi]", &raw, "`between` needs two bounds"))?;
                    let lo = lit_number(&arr[0], &format!("{vp}[0]"))?;
                    let hi = lit_number(&arr[1], &format!("{vp}[1]"))?;
                    Lit::Range(Box::new(lo), Box::new(hi))
                }
                Op::Regex => {
                    let pat = raw
                        .as_str()
                        .ok_or_else(|| perr(&vp, "regex string", &raw, "not a string"))?;
                    if pat.len() > MAX_REGEX_LEN {
                        return Err(perr(
                            &vp,
                            &format!("regex ≤ {MAX_REGEX_LEN} B"),
                            &raw,
                            "regex too long",
                        ));
                    }
                    let re = RegexBuilder::new(pat)
                        .size_limit(REGEX_SIZE_LIMIT)
                        .dfa_size_limit(REGEX_SIZE_LIMIT)
                        .build()
                        .map_err(|e| perr(&vp, "valid Rust regex", &raw, e.to_string()))?;
                    Lit::Regex(re)
                }
                Op::BytesEq => {
                    let s = raw
                        .as_str()
                        .ok_or_else(|| perr(&vp, "hex string", &raw, "not a string"))?;
                    Lit::Bytes(hex_bytes(s).ok_or_else(|| {
                        perr(&vp, &format!("even hex ≤ {MAX_BYTES_LEN} B"), &raw, "bad hex")
                    })?)
                }
            };
        Ok(Expr::Cmp(Cmp {
            field: field.to_owned(),
            op,
            value,
            raw,
            field_unit: None,
        }))
    }
}

/// Fields every published record has (`02-events`), always known to the type checker.
pub fn builtin_fields() -> Vec<PointerSpec> {
    let p = |path: &str, ty: ValueType, unit: Option<&str>| PointerSpec {
        path: path.into(),
        unit: unit.map(str::to_owned),
        ty,
    };
    let en = |v: &[&str]| ValueType::Enum(v.iter().map(|s| (*s).to_owned()).collect());
    vec![
        p("/domain", ValueType::String, None),
        p("/channel", ValueType::String, None),
        p("/kind", ValueType::String, None),
        p("/origin", en(&["agent", "user", "device", "script"]), None),
        p("/dir", en(&["rx", "tx"]), None),
        p("/request_id", ValueType::String, None),
        p("/client_id", ValueType::String, None),
        p("/producer_id", ValueType::String, None),
        p("/seq", ValueType::U64String, None),
        p("/epoch", ValueType::U64String, None),
        p("/source_seq", ValueType::U64String, None),
        p("/generation", ValueType::U32, None),
        p("/ts/mono_ns", ValueType::U64String, Some("ns")),
        p("/ts/wall_ns", ValueType::U64String, Some("ns")),
        p("/purpose", ValueType::String, None),
        p("/payload/write_state", ValueType::String, None),
    ]
}

fn is_numeric(t: &ValueType) -> bool {
    matches!(
        t,
        ValueType::U8
            | ValueType::U16
            | ValueType::U32
            | ValueType::U64String
            | ValueType::I64
            | ValueType::F64
    )
}

fn ty_name(t: &ValueType) -> String {
    match t {
        ValueType::Enum(v) => format!("enum {}", v.join("|")),
        other => format!("{other:?}"),
    }
}

impl Expr {
    /// Parses an AST. `path` is the location of `v` in the plan (used in errors).
    pub fn parse(v: &Value, path: &str) -> Result<Expr, PredError> {
        Parser { nodes: 0 }.expr(v, path, 0)
    }

    /// Type-checks against the fields the domain registered (+ [`builtin_fields`]): the field
    /// must exist, the literal must fit its type, units must be comparable. Records the field
    /// unit for evaluation. `path` as in [`Expr::parse`].
    pub fn check(&mut self, specs: &[PointerSpec], path: &str) -> Result<(), PredError> {
        let builtin = builtin_fields();
        self.check_in(specs, &builtin, path)
    }

    fn check_in(
        &mut self,
        specs: &[PointerSpec],
        builtin: &[PointerSpec],
        path: &str,
    ) -> Result<(), PredError> {
        match self {
            Expr::All(xs) => {
                for (i, x) in xs.iter_mut().enumerate() {
                    x.check_in(specs, builtin, &format!("{path}.all[{i}]"))?;
                }
                Ok(())
            }
            Expr::Any(xs) => {
                for (i, x) in xs.iter_mut().enumerate() {
                    x.check_in(specs, builtin, &format!("{path}.any[{i}]"))?;
                }
                Ok(())
            }
            Expr::Not(x) => x.check_in(specs, builtin, &format!("{path}.not")),
            Expr::Cmp(c) => {
                let spec = specs
                    .iter()
                    .chain(builtin)
                    .find(|s| s.path == c.field)
                    .ok_or_else(|| {
                        perr(
                            &format!("{path}.field"),
                            "a field the domain registered",
                            &json!(c.field),
                            "unknown field",
                        )
                    })?;
                c.field_unit = spec.unit.clone();
                check_cmp(c, spec, &format!("{path}.value"))
            }
        }
    }

    /// Evaluates against one record (as JSON).
    pub fn eval(&self, rec: &Value) -> Result<bool, EvalError> {
        match self {
            Expr::All(xs) => {
                for x in xs {
                    if !x.eval(rec)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Expr::Any(xs) => {
                for x in xs {
                    if x.eval(rec)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Not(x) => Ok(!x.eval(rec)?),
            Expr::Cmp(c) => c.eval(rec),
        }
    }

    /// JSON Pointers the expression reads.
    pub fn fields(&self) -> Vec<&str> {
        match self {
            Expr::Cmp(c) => vec![c.field.as_str()],
            Expr::All(xs) | Expr::Any(xs) => xs.iter().flat_map(Expr::fields).collect(),
            Expr::Not(x) => x.fields(),
        }
    }

    /// Canonical JSON form (for digests and plan hashes).
    pub fn to_json(&self) -> Value {
        match self {
            Expr::Cmp(c) => json!({"field": c.field, "op": c.op.as_str(), "value": c.raw}),
            Expr::All(xs) => json!({"all": xs.iter().map(Expr::to_json).collect::<Vec<_>>()}),
            Expr::Any(xs) => json!({"any": xs.iter().map(Expr::to_json).collect::<Vec<_>>()}),
            Expr::Not(x) => json!({"not": x.to_json()}),
        }
    }
}

fn lit_fits(l: &Lit, spec: &PointerSpec, path: &str, raw: &Value) -> Result<(), PredError> {
    let t = &spec.ty;
    let bad = |exp: &str| {
        Err(perr(
            path,
            exp,
            raw,
            format!("does not fit field {} ({})", spec.path, ty_name(t)),
        ))
    };
    match l {
        Lit::Num(_) if is_numeric(t) => Ok(()),
        Lit::Quantity { unit, .. } if is_numeric(t) => match &spec.unit {
            Some(fu) => convert(1.0, unit, fu)
                .map(|_| ())
                .map_err(|e| perr(path, &format!("a value in {fu}"), raw, e)),
            None => bad("a plain number (the field has no unit)"),
        },
        Lit::Str(_) | Lit::Quantity { .. } if matches!(t, ValueType::String | ValueType::Hex) => Ok(()),
        Lit::Str(s) => match t {
            ValueType::Enum(vals) if vals.contains(s) => Ok(()),
            ValueType::Enum(vals) => bad(&format!("one of {}", vals.join("|"))),
            _ => bad(&ty_name(t)),
        },
        Lit::Bool(_) if matches!(t, ValueType::Bool) => Ok(()),
        Lit::Null => Ok(()),
        _ if matches!(t, ValueType::Object | ValueType::Array) => Ok(()),
        _ => bad(&ty_name(t)),
    }
}

fn check_cmp(c: &mut Cmp, spec: &PointerSpec, path: &str) -> Result<(), PredError> {
    // A quantity- or number-looking string compared with a text field is just text.
    if matches!(spec.ty, ValueType::String | ValueType::Hex | ValueType::Enum(_)) {
        match (&c.value, &c.raw) {
            (Lit::Quantity { raw, .. }, _) => c.value = Lit::Str(raw.clone()),
            (Lit::Num(_), Value::String(s)) => c.value = Lit::Str(s.clone()),
            _ => {}
        }
    }
    let raw = c.raw.clone();
    match (&c.value, c.op) {
        (_, op) if op.numeric() && !is_numeric(&spec.ty) => Err(perr(
            path,
            "a numeric field",
            &raw,
            format!(
                "`{}` needs a number field, {} is {}",
                op.as_str(),
                spec.path,
                ty_name(&spec.ty)
            ),
        )),
        (Lit::Range(a, b), _) => {
            lit_fits(a, spec, &format!("{path}[0]"), &raw)?;
            lit_fits(b, spec, &format!("{path}[1]"), &raw)
        }
        (Lit::List(items), _) => {
            for (i, it) in items.iter().enumerate() {
                lit_fits(it, spec, &format!("{path}[{i}]"), &raw)?;
            }
            Ok(())
        }
        (Lit::Regex(_), _) => match spec.ty {
            ValueType::String | ValueType::Hex | ValueType::Enum(_) => Ok(()),
            _ => Err(perr(
                path,
                "a text field",
                &raw,
                format!("regex on {} ({})", spec.path, ty_name(&spec.ty)),
            )),
        },
        (Lit::Bytes(_), _) => match spec.ty {
            ValueType::Hex | ValueType::Array | ValueType::Object | ValueType::String => Ok(()),
            _ => Err(perr(
                path,
                "a bytes field (hex, array, payload bytes)",
                &raw,
                format!("bytes_eq on {}", spec.path),
            )),
        },
        (l, Op::Contains) => match spec.ty {
            ValueType::String | ValueType::Hex | ValueType::Enum(_) if matches!(l, Lit::Str(_)) => Ok(()),
            ValueType::Array => Ok(()),
            _ => Err(perr(
                path,
                "text or array field",
                &raw,
                format!("contains on {} ({})", spec.path, ty_name(&spec.ty)),
            )),
        },
        (l, _) => lit_fits(l, spec, path, &raw),
    }
}

fn type_err(field: &str, expected: &str, v: &Value) -> EvalError {
    EvalError::Type {
        field: field.to_owned(),
        expected: expected.to_owned(),
        actual: short(v),
    }
}

impl Cmp {
    fn num_lit(&self, l: &Lit) -> Result<Num, EvalError> {
        match l {
            Lit::Num(n) => Ok(*n),
            Lit::Quantity { value, unit, .. } => match &self.field_unit {
                Some(fu) => convert(*value, unit, fu).map_err(|e| type_err(&self.field, &e, &json!(unit))),
                None => Err(type_err(
                    &self.field,
                    "a field with a registered unit (run check first)",
                    &json!(unit),
                )),
            },
            other => Err(type_err(
                &self.field,
                "number literal",
                &json!(format!("{other:?}")),
            )),
        }
    }

    fn eq(&self, l: &Lit, v: &Value) -> Result<bool, EvalError> {
        let f = &self.field;
        match l {
            Lit::Num(_) | Lit::Quantity { .. } => {
                let x = json_num(v).ok_or_else(|| type_err(f, "number", v))?;
                Ok(x.compare(self.num_lit(l)?) == Ordering::Equal)
            }
            Lit::Str(s) => v.as_str().map(|x| x == s).ok_or_else(|| type_err(f, "string", v)),
            Lit::Bool(b) => v.as_bool().map(|x| x == *b).ok_or_else(|| type_err(f, "bool", v)),
            Lit::Null => Ok(v.is_null()),
            _ => Err(type_err(f, "scalar literal", v)),
        }
    }

    fn eval(&self, rec: &Value) -> Result<bool, EvalError> {
        let f = &self.field;
        let v = rec
            .pointer(f)
            .ok_or_else(|| EvalError::Missing { field: f.clone() })?;
        match (self.op, &self.value) {
            (Op::Eq, l) => self.eq(l, v),
            (Op::Ne, l) => Ok(!self.eq(l, v)?),
            (Op::In, Lit::List(items)) => {
                for it in items {
                    if self.eq(it, v)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            (Op::Lt | Op::Le | Op::Gt | Op::Ge, l) => {
                let x = json_num(v).ok_or_else(|| type_err(f, "number", v))?;
                let o = x.compare(self.num_lit(l)?);
                Ok(match self.op {
                    Op::Lt => o == Ordering::Less,
                    Op::Le => o != Ordering::Greater,
                    Op::Gt => o == Ordering::Greater,
                    _ => o != Ordering::Less,
                })
            }
            (Op::Between, Lit::Range(a, b)) => {
                let x = json_num(v).ok_or_else(|| type_err(f, "number", v))?;
                Ok(x.compare(self.num_lit(a)?) != Ordering::Less
                    && x.compare(self.num_lit(b)?) != Ordering::Greater)
            }
            (Op::Regex, Lit::Regex(re)) => v
                .as_str()
                .map(|s| re.is_match(s))
                .ok_or_else(|| type_err(f, "string", v)),
            (Op::Contains, l) => match v {
                Value::String(s) => match l {
                    Lit::Str(n) => Ok(s.contains(n.as_str())),
                    _ => Err(type_err(f, "text literal for a text field", v)),
                },
                Value::Array(items) => {
                    for it in items {
                        if self.eq(l, it).unwrap_or(false) {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => Err(type_err(f, "string or array", v)),
            },
            (Op::BytesEq, Lit::Bytes(want)) => {
                let got = match v {
                    Value::String(s) => hex_bytes(s).ok_or_else(|| type_err(f, "hex string", v))?,
                    Value::Array(a) => a
                        .iter()
                        .map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
                        .collect::<Option<Vec<u8>>>()
                        .ok_or_else(|| type_err(f, "array of bytes", v))?,
                    Value::Object(_) => payload_bytes(&json!({"bytes": v}))
                        .ok()
                        .flatten()
                        .ok_or_else(|| type_err(f, "inline payload bytes", v))?,
                    _ => return Err(type_err(f, "bytes", v)),
                };
                Ok(&got == want)
            }
            _ => Err(type_err(f, "operator/literal pair", v)),
        }
    }
}

impl fmt::Display for Expr {
    /// Compact text form (`/payload/x == 2 && …`), for logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Cmp(c) => write!(f, "{} {} {}", c.field, c.op.as_str(), c.raw),
            Expr::All(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " && ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, ")")
            }
            Expr::Any(xs) => {
                write!(f, "(")?;
                for (i, x) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, " || ")?;
                    }
                    write!(f, "{x}")?;
                }
                write!(f, ")")
            }
            Expr::Not(x) => write!(f, "!{x}"),
        }
    }
}
