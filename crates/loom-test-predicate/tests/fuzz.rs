//! 100 000 random predicate JSON values (deterministic seed, no fuzz crate): parse, type check,
//! render and evaluate never panic; errors carry a path inside the input. Plus the hash
//! convention matches `loom-test-format` byte for byte.

mod support;

use loom_domain_api::EvalBudget;
use loom_test_predicate::{Citation, Expr};
use serde_json::{Value, json};
use support::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() % xs.len() as u64) as usize]
    }
}

const FIELDS: &[&str] = &[
    "/payload/value",
    "/payload/text",
    "/payload/state",
    "/payload/age_ms",
    "/payload/bytes",
    "/origin",
    "/kind",
    "/payload/nope",
    "bad",
    "/payload/~2",
];
const OPS: &[&str] = &[
    "==", "!=", "<", "<=", ">", ">=", "in", "regex", "contains", "between", "bytes_eq", "??",
];

fn scalar(r: &mut Rng) -> Value {
    match r.next() % 10 {
        0 => json!(null),
        1 => json!(r.next().is_multiple_of(2)),
        2 => json!((r.next() % 1000) as f64 / 7.0),
        3 => json!(r.next()),
        4 => json!(r.pick(&[
            "5 s",
            "150 ms",
            "13.5 V",
            "3 parsecs",
            "on",
            "login:",
            "(a",
            "[",
            "^x+$",
            "0a0b",
            "zz"
        ])),
        5 => json!(r.next().to_string()),
        6 => json!([r.next() % 300, "x"]),
        7 => json!({"k": 1}),
        8 => json!(-(r.next() as f64)),
        _ => json!(""),
    }
}

fn expr(r: &mut Rng, depth: u32) -> Value {
    match r.next() % if depth > 6 { 3 } else { 8 } {
        0..=2 => {
            let mut m = serde_json::Map::new();
            if !r.next().is_multiple_of(10) {
                m.insert("field".into(), json!(r.pick(FIELDS)));
            }
            if !r.next().is_multiple_of(10) {
                m.insert("op".into(), json!(r.pick(OPS)));
            }
            if !r.next().is_multiple_of(10) {
                let v = if r.next().is_multiple_of(3) {
                    json!([scalar(r), scalar(r)])
                } else {
                    scalar(r)
                };
                m.insert("value".into(), v);
            }
            if r.next().is_multiple_of(20) {
                m.insert("junk".into(), json!(1));
            }
            Value::Object(m)
        }
        3 | 4 => json!({"all": (0..r.next() % 4).map(|_| expr(r, depth + 1)).collect::<Vec<_>>()}),
        5 => json!({"any": (0..r.next() % 4).map(|_| expr(r, depth + 1)).collect::<Vec<_>>()}),
        6 => json!({"not": expr(r, depth + 1)}),
        _ => scalar(r),
    }
}

fn citation(r: &mut Rng) -> Value {
    let name = *r.pick(&[
        "exists",
        "absent",
        "count",
        "order",
        "within_ms",
        "hold",
        "measure",
        "nope",
    ]);
    let args = json!({"where": expr(r, 0), "min": scalar(r), "max": scalar(r), "a": expr(r, 0), "b": expr(r, 0),
                      "anchor": {"mono_ns": scalar(r)}, "min_ms": scalar(r), "from": expr(r, 0), "to": expr(r, 0)});
    let mut args = args;
    if let Some(o) = args.as_object_mut() {
        let keep: Vec<String> = o.keys().filter(|_| r.next().is_multiple_of(2)).cloned().collect();
        o.retain(|k, _| keep.contains(k));
    }
    json!({"filter": {"domain": r.pick(&["uart", "can"]), "origins": r.pick(&[json!(["device"]), json!(["agent", "device"]), json!("x")])},
           "pred": {name: args}})
}

#[test]
fn random_predicates_never_panic() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let events = recs(vec![
        r_(
            1,
            0,
            json!({"value": 2.5, "text": "login:", "state": "on", "age_ms": 10, "bytes": [1, 2]}),
        ),
        r_(2, 100, json!({"value": "bad"})),
        r_(3, 200, json!({})),
    ]);
    let (mut parsed, mut checked) = (0u32, 0u32);
    for i in 0u32..100_000 {
        let v = if i.is_multiple_of(2) {
            expr(&mut r, 0)
        } else {
            citation(&mut r)
        };
        if i.is_multiple_of(2) {
            match Expr::parse(&v, "p") {
                Ok(mut e) => {
                    parsed += 1;
                    let _ = loom_test_predicate::render::expr(&e);
                    if e.check(&specs(), "p").is_ok() {
                        checked += 1;
                    }
                    for rec in &events {
                        let _ = e.eval(&serde_json::to_value(rec).unwrap_or(Value::Null));
                    }
                }
                Err(err) => assert!(err.path.starts_with('p'), "{err}"),
            }
        } else if let Ok(mut c) = Citation::parse(&v, "steps[0].expect[0]") {
            let _ = c.render();
            let _ = c.check(&specs(), "steps[0].expect[0]");
            let _ = c.evaluate(&events, &cov(i % 4 == 1), &mut EvalBudget::default());
        }
    }
    assert!(
        parsed > 1000 && checked > 100,
        "generator too weak: parsed {parsed}, checked {checked}"
    );
}

fn r_(seq: u64, t: u64, payload: Value) -> R {
    r(seq, t, "uart", "dut", "uart.line", payload)
}

#[test]
fn hash_convention_matches_loom_test_format() {
    let samples = [
        json!({"b": 1, "a": [3, {"z": true, "y": null}], "ü": "é", "n": 1.5}),
        json!([{"field": "/payload/x", "op": "==", "value": "5 s"}]),
        json!({"kind": "match", "matched": [{"epoch": "1", "seq": "18446744073709551615"}]}),
    ];
    for v in samples {
        let mine = loom_test_predicate::canonical::canonical_json(&v);
        let theirs = loom_test_format::canonical_json(&v).expect("format canonical");
        assert_eq!(mine, theirs, "{v}");
        assert_eq!(
            loom_test_predicate::canonical::canonical_sha256(&v),
            loom_test_format::canonical_sha256(&v).expect("format sha")
        );
    }
}
