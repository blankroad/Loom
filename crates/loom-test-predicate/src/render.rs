//! Human sentences (Korean) for the plan approval screen, generated from the AST — never
//! written by the agent. Example: `/payload/signals/VehState.PowerMode == 2` →
//! "신호 VehState.PowerMode가 2".

use serde_json::Value;

use crate::ast::{Expr, Op};

/// `이`/`가` (or another particle pair) after `word`: Hangul by final consonant, digits by
/// their Korean reading, Latin letters by a small sound table.
pub fn josa(word: &str, with_final: &'static str, without_final: &'static str) -> &'static str {
    let Some(c) = word.trim_end_matches(['"', ')', ']', ' ']).chars().last() else {
        return without_final;
    };
    let has_final = match c {
        '가'..='힣' => !(c as u32 - 0xAC00).is_multiple_of(28),
        '0' | '1' | '3' | '6' | '7' | '8' => true,
        '2' | '4' | '5' | '9' => false,
        'l' | 'm' | 'n' | 'L' | 'M' | 'N' | 'r' | 'R' | 'k' | 'K' | 'p' | 'P' | 't' | 'T' | 'b' | 'B'
        | 'g' | 'G' => true,
        _ => false,
    };
    if has_final { with_final } else { without_final }
}

/// Readable name of a JSON Pointer.
pub fn label(ptr: &str) -> String {
    let unescape = |s: &str| s.replace("~1", "/").replace("~0", "~");
    let parts: Vec<String> = ptr.split('/').skip(1).map(unescape).collect();
    match parts.as_slice() {
        [p, s, name, ..] if p == "payload" && s == "signals" => format!("신호 {name}"),
        [m, i, v] if m == "metrics" && v == "value" => format!("측정값 #{i}"),
        [p, rest @ ..] if p == "payload" && !rest.is_empty() => rest.join("."),
        [t, k] if t == "ts" => format!("시각 {k}"),
        _ => parts.join("."),
    }
}

fn val(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(val).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// One sentence for an AST.
pub fn expr(e: &Expr) -> String {
    match e {
        Expr::Cmp(c) => {
            let l = label(&c.field);
            let ga = josa(&l, "이", "가");
            let v = val(&c.raw);
            match c.op {
                Op::Eq => format!("{l}{ga} {v}"),
                Op::Ne => format!("{l}{ga} {v}{} 아님", josa(&v, "이", "가")),
                Op::Lt => format!("{l}{ga} {v} 미만"),
                Op::Le => format!("{l}{ga} {v} 이하"),
                Op::Gt => format!("{l}{ga} {v} 초과"),
                Op::Ge => format!("{l}{ga} {v} 이상"),
                Op::In => format!("{l}{ga} {{{v}}} 중 하나"),
                Op::Regex => format!("{l}{ga} 정규식 /{v}/와 일치"),
                Op::Contains => format!("{l}에 \"{v}\" 포함"),
                Op::Between => {
                    let (a, b) = match c.raw.as_array().map(Vec::as_slice) {
                        Some([a, b]) => (val(a), val(b)),
                        _ => (v.clone(), String::new()),
                    };
                    format!("{l}{ga} {a} 이상 {b} 이하")
                }
                Op::BytesEq => format!("{l} 바이트가 {v}"),
            }
        }
        Expr::All(xs) if xs.is_empty() => "항상 참".into(),
        Expr::Any(xs) if xs.is_empty() => "항상 거짓".into(),
        Expr::All(xs) => xs.iter().map(expr).collect::<Vec<_>>().join(" 그리고 "),
        Expr::Any(xs) => format!("({})", xs.iter().map(expr).collect::<Vec<_>>().join(" 또는 ")),
        Expr::Not(x) => format!("[{}]이(가) 아님", expr(x)),
    }
}

/// `"5000"` ms → `"5 s"` style.
pub fn ms(ms: u64) -> String {
    if ms >= 1000 && ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{ms} ms")
    }
}
