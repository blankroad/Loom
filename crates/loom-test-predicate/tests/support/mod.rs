//! Record builders for tests.
#![allow(dead_code)]

use loom_domain_api::{Coverage, EvalBudget, PointerSpec, ValueType};
use loom_proto::event::{PublishedRecord, SeqRange};
use loom_test_predicate::{Check, Citation};
use serde_json::{Value, json};

/// A record builder (defaults: epoch 1, generation 1, producer `w1`, origin device).
#[derive(Clone)]
pub struct R(pub Value);

pub fn r(seq: u64, t_ms: u64, domain: &str, channel: &str, kind: &str, payload: Value) -> R {
    R(json!({
        "schema_version": 1, "session_id": "s", "domain": domain, "channel": channel, "kind": kind,
        "seq": seq.to_string(), "epoch": "1", "producer_id": "w1", "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": {"mono_ns": (t_ms * 1_000_000).to_string(), "wall_ns": (t_ms * 1_000_000).to_string(), "clock_id": "c", "source": "device"},
        "origin": "device", "generation": 1, "payload": payload,
    }))
}

impl R {
    pub fn origin(mut self, o: &str) -> R {
        self.0["origin"] = json!(o);
        self
    }
    pub fn req(mut self, id: &str) -> R {
        self.0["request_id"] = json!(id);
        self
    }
    pub fn generation(mut self, g: u32) -> R {
        self.0["generation"] = json!(g);
        self
    }
    pub fn epoch(mut self, e: u64) -> R {
        self.0["epoch"] = json!(e.to_string());
        self
    }
    pub fn producer(mut self, p: &str) -> R {
        self.0["producer_id"] = json!(p);
        self
    }
    pub fn no_channel(mut self) -> R {
        if let Some(o) = self.0.as_object_mut() {
            o.remove("channel");
        }
        self
    }
    pub fn build(self) -> PublishedRecord {
        serde_json::from_value(self.0).expect("valid record")
    }
}

pub fn recs(rs: Vec<R>) -> Vec<PublishedRecord> {
    rs.into_iter().map(R::build).collect()
}

pub fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: 1.into(),
            to: 1000.into(),
        },
        complete,
        causes: if complete {
            vec![]
        } else {
            vec!["scan_limit".into()]
        },
    }
}

pub fn specs() -> Vec<PointerSpec> {
    let p = |path: &str, ty: ValueType, unit: Option<&str>| PointerSpec {
        path: path.into(),
        unit: unit.map(str::to_owned),
        ty,
    };
    vec![
        p("/payload/value", ValueType::F64, None),
        p("/payload/signal", ValueType::String, None),
        p("/payload/text", ValueType::String, None),
        p("/payload/bytes", ValueType::Array, None),
        p("/payload/bytes/0", ValueType::U8, None),
        p("/payload/bytes/1", ValueType::U8, None),
        p("/payload/sid", ValueType::Hex, None),
        p(
            "/payload/state",
            ValueType::Enum(vec!["on".into(), "off".into()]),
            None,
        ),
        p("/payload/age_ms", ValueType::U32, Some("ms")),
        p("/payload/volt", ValueType::F64, Some("V")),
        p("/payload/big", ValueType::U64String, None),
        p("/payload/ok", ValueType::Bool, None),
        p("/payload/raw", ValueType::Object, None),
        p("/payload/signals/VehState.PowerMode", ValueType::U8, None),
    ]
}

/// Parses + type-checks a citation, panicking with the error.
pub fn cite(v: Value) -> Citation {
    let mut c = Citation::parse(&v, "steps[0].expect[0]").unwrap_or_else(|e| panic!("parse: {e}"));
    c.check(&specs(), "steps[0].expect[0]")
        .unwrap_or_else(|e| panic!("check: {e}"));
    c
}

pub fn eval(c: &Citation, rs: &[PublishedRecord], complete: bool) -> Check {
    c.evaluate(rs, &cov(complete), &mut EvalBudget::default())
}
