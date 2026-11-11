use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanFormat {
    TcPlan,
    Scenario,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub method: String,
    pub args: Value,
    pub location: Location,
    /// sleep/wait are bounded runner delays, not device RPCs; args are normalized milliseconds.
    pub delay_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub domain: String,
    pub channel: Option<String>,
    pub kinds: Vec<String>,
    pub origins: Vec<String>,
    pub request_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub step: String,
    pub kind: Option<String>,
    pub where_clause: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expectation {
    pub filter: Filter,
    pub pred: Value,
    pub anchor: Option<Anchor>,
    pub within_ms: Option<u64>,
    pub location: Location,
    /// Old UDS field/expected maps require translation by its adapter before execution.
    pub legacy: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureAction {
    Continue,
    Goto(String),
    StopTc,
    StopSession,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct When {
    pub step: String,
    pub verdict: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manual {
    pub prompt: String,
    pub timeout_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub tc_step: Option<u32>,
    pub actions: Vec<Action>,
    pub expect: Vec<Expectation>,
    pub on_fail: FailureAction,
    pub when: Option<When>,
    pub retry_count: u32,
    pub repeat_count: u32,
    pub manual: Option<Manual>,
    pub measure: Option<Value>,
    pub save: Option<String>,
    pub location: Location,
}
/// Parsed plans are immutable through this API: changing source requires reparsing/re-hashing.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    format: PlanFormat,
    canonical: Value,
    steps: Vec<Step>,
    finally: Vec<Step>,
    warnings: Vec<Diagnostic>,
    source_sha256: String,
    plan_sha256: String,
    tc_source_sha256: Option<String>,
    tc_hash_verified: bool,
}
impl Plan {
    pub fn format(&self) -> PlanFormat {
        self.format
    }
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }
    pub fn finally(&self) -> &[Step] {
        &self.finally
    }
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }
    pub fn tc_source_sha256(&self) -> Option<&str> {
        self.tc_source_sha256.as_deref()
    }
    pub fn tc_hash_verified(&self) -> bool {
        self.tc_hash_verified
    }
    pub fn canonical_value(&self) -> &Value {
        &self.canonical
    }
    pub fn requires_legacy_adapter(&self) -> bool {
        self.steps
            .iter()
            .chain(&self.finally)
            .flat_map(|s| &s.expect)
            .any(|e| e.legacy)
    }
}
pub fn parse_plan_bytes(bytes: &[u8]) -> Result<Plan, FormatError> {
    parse_plan(input(bytes)?)
}
pub fn parse_plan(source: &str) -> Result<Plan, FormatError> {
    parse(source, None)
}
pub fn parse_plan_with_tc(source: &str, tc: &TcDoc) -> Result<Plan, FormatError> {
    parse(source, Some(tc))
}
struct Context {
    location: Location,
    path: String,
}
impl Context {
    fn fail(&self, code: &str, message: impl Into<String>) -> FormatError {
        Diagnostic {
            code: code.into(),
            message: message.into(),
            path: self.path.clone(),
            location: self.location,
        }
    }
    fn child(&self, key: &str) -> Self {
        Self {
            location: self.location,
            path: format!("{}/{}", self.path, key),
        }
    }
    fn object<'a>(&self, v: &'a Value) -> Result<&'a Map<String, Value>, FormatError> {
        v.as_object()
            .ok_or_else(|| self.fail("type", "expected an object"))
    }
    fn string(&self, v: &Value) -> Result<String, FormatError> {
        let s = v
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= MAX_SCALAR_BYTES)
            .ok_or_else(|| self.fail("type", "expected a nonempty bounded string"))?;
        Ok(s.into())
    }
    fn id(&self, v: &Value) -> Result<String, FormatError> {
        let s = self.string(v)?;
        if s.len() > MAX_ID_BYTES || s.chars().any(char::is_whitespace) {
            return Err(self.fail(
                "identifier",
                "identifier exceeds 128 bytes or contains whitespace",
            ));
        }
        Ok(s)
    }
    fn strings(&self, v: &Value) -> Result<Vec<String>, FormatError> {
        let a = v
            .as_array()
            .filter(|a| a.len() <= MAX_EXPECTATIONS)
            .ok_or_else(|| self.fail("type", "expected an array of at most 64 strings"))?;
        a.iter().map(|v| self.string(v)).collect()
    }
    fn field<'a>(&self, m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, FormatError> {
        m.get(key)
            .ok_or_else(|| self.child(key).fail("missing", format!("missing {key}")))
    }
    fn keys(&self, m: &Map<String, Value>, allowed: &[&str]) -> Result<(), FormatError> {
        for key in m.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(self
                    .child(key)
                    .fail("unknown_field", format!("unknown field {key}")));
            }
        }
        Ok(())
    }
}
fn parse(source: &str, tc: Option<&TcDoc>) -> Result<Plan, FormatError> {
    input(source.as_bytes())?;
    let mut value = yaml::parse(source, source, 0)?;
    let root = Context {
        location: Location::at(source, 0),
        path: String::new(),
    };
    let m = root.object(&value)?;
    let format = match (m.get("loom_tc_plan"), m.get("loom_scenario")) {
        (Some(v), None) if v.as_u64() == Some(1) => PlanFormat::TcPlan,
        (None, Some(v)) if v.as_u64() == Some(1) => PlanFormat::Scenario,
        _ => {
            return Err(root.fail(
                "version",
                "exactly one of loom_tc_plan: 1 or loom_scenario: 1 is required",
            ));
        }
    };
    if let Some(env) = m.get("env")
        && !matches!(env.as_str(), Some("sim" | "bench" | "vehicle"))
    {
        return Err(root.child("env").fail("env", "env must be sim, bench or vehicle"));
    }
    for key in ["name", "db", "kind"] {
        if let Some(v) = m.get(key) {
            root.child(key).string(v)?;
        }
    }
    if let Some(v) = m.get("vars") {
        root.child("vars").object(v)?;
    }
    let legacy_continue = if let Some(v) = m.get("on_error") {
        match v.as_str() {
            Some("stop") => false,
            Some("continue") => true,
            _ => {
                return Err(root
                    .child("on_error")
                    .fail("on_error", "scenario on_error must be stop or continue"));
            }
        }
    } else {
        false
    };
    let mut warnings = Vec::new();
    if m.contains_key("risk_summary") {
        warnings.push(root.child("risk_summary").fail(
            "ignored_risk_summary",
            "agent risk_summary ignored; only RiskProvider classification is trusted",
        ));
    }
    let steps_value = root.field(m, "steps")?;
    let items = steps_value
        .as_array()
        .ok_or_else(|| root.child("steps").fail("type", "steps must be an array"))?;
    let empty = Vec::new();
    let finally = match m.get("finally") {
        None => &empty,
        Some(v) => v
            .as_array()
            .ok_or_else(|| root.child("finally").fail("type", "finally must be an array"))?,
    };
    if items.len() + finally.len() > MAX_STEPS {
        let in_main = items.len() > MAX_STEPS;
        let i = if in_main {
            MAX_STEPS
        } else {
            MAX_STEPS - items.len()
        };
        let key = if in_main { "steps" } else { "finally" };
        let offsets = step_offsets(source, key, if in_main { items.len() } else { finally.len() });
        return Err(error(
            source,
            offsets.get(i).copied().unwrap_or(0),
            &format!("/{key}/{i}"),
            "step_limit",
            "steps + finally exceed 500",
        ));
    }
    let target = m
        .get("target")
        .map(|v| root.child("target").string(v))
        .transpose()?;
    let mut parse_steps = |key: &str, items: &[Value]| -> Result<Vec<Step>, FormatError> {
        let offsets = step_offsets(source, key, items.len());
        items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let c = Context {
                    location: Location::at(source, offsets[i]),
                    path: format!("/{key}/{i}"),
                };
                let mut step = parse_step(
                    v,
                    &c,
                    format,
                    key == "finally",
                    i,
                    target.as_deref(),
                    &mut warnings,
                )?;
                if legacy_continue && v.get("on_fail").is_none() {
                    step.on_fail = FailureAction::Continue;
                }
                Ok(step)
            })
            .collect()
    };
    let steps = parse_steps("steps", items)?;
    let final_steps = parse_steps("finally", finally)?;
    let supplied = m
        .get("plan_hash")
        .map(|v| root.child("plan_hash").string(v))
        .transpose()?;
    let mut tc_hash = None;
    if let Some(v) = m.get("tc") {
        let reference = root.child("tc").string(v)?;
        if let Some((_, hash)) = reference.rsplit_once("@sha256:") {
            if !is_hash(hash) {
                return Err(root
                    .child("tc")
                    .fail("tc_hash", "TC SHA-256 must be 64 lowercase hex digits"));
            }
            tc_hash = Some(hash.to_owned());
        }
    }
    if let Some(tc) = tc {
        if tc_hash.as_ref().is_some_and(|hash| hash != &tc.source_sha256) {
            return Err(root.child("tc").fail(
                "tc_hash",
                "TC source hash does not match the supplied TC document",
            ));
        }
        if let (Some(id), Some(reference)) = (&tc.meta.id, m.get("tc").and_then(Value::as_str))
            && reference.rsplit_once("@sha256:").map_or(reference, |(id, _)| id) != id
        {
            return Err(root
                .child("tc")
                .fail("tc_id", "TC id does not match the supplied TC document"));
        }
        tc_hash = Some(tc.source_sha256.clone());
    }
    let obj = value
        .as_object_mut()
        .ok_or_else(|| root.fail("type", "plan must be an object"))?;
    obj.remove("risk_summary");
    obj.remove("plan_hash");
    if let Some(tc) = tc {
        let id = tc.meta.id.as_deref().unwrap_or("tc");
        obj.insert("tc".into(), json!(format!("{id}@sha256:{}", tc.source_sha256)));
    }
    let plan_sha256 = canonical_sha256(&value)?;
    if let Some(supplied) = supplied
        && supplied.strip_prefix("sha256:").unwrap_or(&supplied) != plan_sha256
    {
        return Err(root.child("plan_hash").fail(
            "plan_hash",
            "supplied plan_hash does not match canonical plan content",
        ));
    }
    Ok(Plan {
        format,
        canonical: value,
        steps,
        finally: final_steps,
        warnings,
        source_sha256: sha256(source.as_bytes()),
        plan_sha256,
        tc_source_sha256: tc_hash,
        tc_hash_verified: tc.is_some(),
    })
}
fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
/// Semantic errors use the containing step's original line + JSON pointer. Flow-style lists
/// point at their containing line; parser syntax errors retain serde_norway's exact mark.
fn step_offsets(source: &str, key: &str, count: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(count);
    let mut section = None;
    let mut indent = None;
    let mut offset = 0;
    let root_keys = [key.to_owned(), format!("\"{key}\""), format!("'{key}'")];
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let n = line.len() - trimmed.len();
        let root_key = root_keys.iter().any(|k| {
            trimmed
                .strip_prefix(k)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
        });
        if root_key && n == 0 {
            section = Some(offset);
            indent = None;
        } else if section.is_some() && !trimmed.trim().is_empty() && !trimmed.starts_with('#') {
            if trimmed
                .strip_prefix('-')
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
                && indent.is_none_or(|i| n == i)
            {
                indent = Some(n);
                if result.len() < count {
                    result.push(offset + n);
                }
            } else if n == 0 && !trimmed.starts_with('-') {
                break;
            }
        }
        offset += line.len();
    }
    result.resize(count, section.unwrap_or(0));
    result
}
fn parse_step(
    v: &Value,
    c: &Context,
    format: PlanFormat,
    is_final: bool,
    index: usize,
    target: Option<&str>,
    warnings: &mut Vec<Diagnostic>,
) -> Result<Step, FormatError> {
    let m = c.object(v)?;
    let id = match m.get("id") {
        Some(v) => c.child("id").id(v)?,
        None if format == PlanFormat::Scenario || is_final => {
            format!("{}{}", if is_final { "F" } else { "S" }, index + 1)
        }
        None => return Err(c.child("id").fail("missing", "plan step id is required")),
    };
    let known = [
        "id", "tc_step", "do", "expect", "anchor", "on_fail", "when", "retry", "manual", "repeat", "measure",
        "save",
    ];
    let mut actions = Vec::new();
    let mut direct = Vec::new();
    for (k, v) in m {
        if known.contains(&k.as_str()) {
            continue;
        }
        if k.contains('.') || matches!(k.as_str(), "sleep" | "wait") {
            direct.push((k, v));
        } else {
            return Err(c
                .child(k)
                .fail("unknown_field", format!("unknown step field {k}")));
        }
    }
    if let Some(do_value) = m.get("do") {
        if !direct.is_empty() {
            return Err(c.fail("actions", "use one direct command OR do, not both"));
        }
        let a = do_value
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= MAX_ACTIONS)
            .ok_or_else(|| {
                c.child("do")
                    .fail("actions", "do must contain 1..=64 command objects")
            })?;
        for (i, v) in a.iter().enumerate() {
            let cc = c.child(&format!("do/{i}"));
            let map = cc.object(v)?;
            if map.len() != 1 {
                return Err(cc.fail("actions", "each do entry must have exactly one command"));
            }
            if let Some((method, args)) = map.iter().next() {
                actions.push(parse_action(method, args, &cc, target)?);
            }
        }
    } else {
        if direct.len() > 1 {
            return Err(c.fail("actions", "multiple commands need a do array"));
        }
        for (method, args) in direct {
            actions.push(parse_action(method, args, c, target)?);
        }
    }
    let manual = m
        .get("manual")
        .map(|v| {
            let cc = c.child("manual");
            let o = cc.object(v)?;
            cc.keys(o, &["prompt", "timeout", "timeout_ms"])?;
            let prompt = cc.child("prompt").string(cc.field(o, "prompt")?)?;
            if prompt.len() > MAX_CELL_BYTES {
                return Err(cc.fail("manual_limit", "manual prompt exceeds 4 KiB"));
            }
            Ok(Manual {
                prompt,
                timeout_ms: timeout(o, &cc)?,
            })
        })
        .transpose()?;
    let measure = m
        .get("measure")
        .map(|v| {
            c.child("measure").object(v)?;
            Ok(v.clone())
        })
        .transpose()?;
    if actions.is_empty() && manual.is_none() && measure.is_none() {
        return Err(c.fail("actions", "step needs a command, manual prompt or measure"));
    }
    let tc_step = m
        .get("tc_step")
        .map(|v| {
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| c.child("tc_step").fail("type", "tc_step must be a positive u32"))
        })
        .transpose()?;
    let retry_count = count(m.get("retry"), "retry", MAX_RETRY, 0, c)?;
    let repeat_count = count(m.get("repeat"), "repeat", MAX_REPEAT, 1, c)?;
    let on_fail = match m.get("on_fail") {
        None => FailureAction::StopTc,
        Some(v) => failure(v, &c.child("on_fail"))?,
    };
    let when = m
        .get("when")
        .map(|v| {
            let cc = c.child("when");
            let o = cc.object(v)?;
            cc.keys(o, &["step", "verdict"])?;
            let step = cc.child("step").id(cc.field(o, "step")?)?;
            let verdict = cc.child("verdict").string(cc.field(o, "verdict")?)?;
            if !matches!(
                verdict.as_str(),
                "pass" | "fail" | "inconclusive" | "blocked" | "done" | "skipped"
            ) {
                return Err(cc.fail("verdict", "unknown when verdict"));
            }
            Ok(When { step, verdict })
        })
        .transpose()?;
    let default_anchor = m
        .get("anchor")
        .map(|v| anchor(v, &c.child("anchor")))
        .transpose()?;
    if default_anchor.is_some()
        && m.get("expect")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        return Err(c
            .child("anchor")
            .fail("unused_anchor", "step anchor needs an expectation"));
    }
    if default_anchor.is_some() && !m.contains_key("expect") {
        return Err(c
            .child("anchor")
            .fail("unused_anchor", "step anchor needs an expectation"));
    }
    let mut expect = Vec::new();
    if let Some(v) = m.get("expect") {
        if let Some(items) = v.as_array() {
            if items.len() > MAX_EXPECTATIONS {
                return Err(c.child("expect").fail("expect_limit", "at most 64 expectations"));
            }
            for (i, v) in items.iter().enumerate() {
                let cc = c.child(&format!("expect/{i}"));
                let mut e = expectation(v, &cc)?;
                if e.anchor.is_none() {
                    e.anchor = default_anchor.clone();
                }
                expect.push(e);
            }
        } else if format == PlanFormat::Scenario && v.is_object() && !actions.is_empty() {
            if !actions[0].method.starts_with("uds.") {
                return Err(c
                    .child("expect")
                    .fail("legacy_expect", "legacy expectation maps are UDS-only"));
            }
            let domain = "uds".to_owned();
            expect.push(Expectation {
                filter: Filter {
                    domain,
                    channel: target.map(str::to_owned),
                    kinds: vec!["uds.rx".into()],
                    origins: vec!["device".into()],
                    request_id: None,
                },
                pred: v.clone(),
                anchor: default_anchor,
                within_ms: None,
                location: c.location,
                legacy: true,
            });
            if warnings.len() < MAX_DIAGNOSTICS {
                warnings.push(c.child("expect").fail("legacy_expect","legacy UDS expect map preserved; adapter translation required before predicate evaluation"));
            }
        } else {
            return Err(c
                .child("expect")
                .fail("type", "expect must be an array of citation templates"));
        }
    }
    let save = m.get("save").map(|v| c.child("save").id(v)).transpose()?;
    Ok(Step {
        id,
        tc_step,
        actions,
        expect,
        on_fail,
        when,
        retry_count,
        repeat_count,
        manual,
        measure,
        save,
        location: c.location,
    })
}
fn parse_action(
    method: &str,
    args: &Value,
    c: &Context,
    target: Option<&str>,
) -> Result<Action, FormatError> {
    if method.len() > MAX_ID_BYTES
        || !method.split('.').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return Err(c.child(method).fail("method", "invalid command name"));
    }
    let (args, delay_ms) = if matches!(method, "sleep" | "wait") {
        let ms = if let Some(m) = args.as_object() {
            c.child(method).keys(m, &["duration_ms"])?;
            milliseconds(c.field(m, "duration_ms")?, &c.child(method))?
        } else {
            duration(args, &c.child(method))?
        };
        (json!({"duration_ms":ms}), Some(ms))
    } else {
        if !method.contains('.') {
            return Err(c.child(method).fail("method", "command must have a domain"));
        }
        let mut o = c.child(method).object(args)?.clone();
        if method.starts_with("uds.")
            && let Some(target) = target
        {
            o.entry("target").or_insert(json!(target));
        }
        (Value::Object(o), None)
    };
    Ok(Action {
        method: method.into(),
        args,
        delay_ms,
        location: c.location,
    })
}
fn count(v: Option<&Value>, name: &str, max: u32, default: u32, c: &Context) -> Result<u32, FormatError> {
    let Some(v) = v else { return Ok(default) };
    let cc = c.child(name);
    let m = cc.object(v)?;
    cc.keys(m, &["count"])?;
    let n = cc
        .field(m, "count")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n <= max && (name == "retry" || *n > 0))
        .ok_or_else(|| {
            cc.fail(
                "count_limit",
                format!("{name} count exceeds its bound or has the wrong type"),
            )
        })?;
    Ok(n)
}
fn failure(v: &Value, c: &Context) -> Result<FailureAction, FormatError> {
    let name = if let Some(s) = v.as_str() {
        s
    } else {
        let m = c.object(v)?;
        if m.len() != 1 {
            return Err(c.fail("on_fail", "on_fail needs exactly one action"));
        }
        let (k, v) = m
            .iter()
            .next()
            .ok_or_else(|| c.fail("on_fail", "empty on_fail"))?;
        if k == "goto" {
            return Ok(FailureAction::Goto(c.child(k).id(v)?));
        }
        if !v.is_null() && v.as_bool() != Some(true) {
            return Err(c.fail("on_fail", "on_fail action must be null or true"));
        }
        k
    };
    match name {
        "continue" => Ok(FailureAction::Continue),
        "stop_tc" => Ok(FailureAction::StopTc),
        "stop_session" => Ok(FailureAction::StopSession),
        _ => Err(c.fail("on_fail", "unknown failure action")),
    }
}
fn expectation(v: &Value, c: &Context) -> Result<Expectation, FormatError> {
    let m = c.object(v)?;
    c.keys(m, &["filter", "pred", "anchor", "within_ms"])?;
    let fc = c.child("filter");
    let f = fc.object(c.field(m, "filter")?)?;
    fc.keys(f, &["domain", "channel", "kinds", "origins", "request_id"])?;
    let domain = fc.child("domain").id(fc.field(f, "domain")?)?;
    let kinds = fc.child("kinds").strings(fc.field(f, "kinds")?)?;
    if kinds.is_empty() {
        return Err(fc.fail("filter", "kinds must not be empty"));
    }
    let channel = f
        .get("channel")
        .map(|v| fc.child("channel").string(v))
        .transpose()?;
    let request_id = f
        .get("request_id")
        .map(|v| fc.child("request_id").string(v))
        .transpose()?;
    let origins = match f.get("origins") {
        None => vec!["device".into()],
        Some(v) => {
            let a = fc.child("origins").strings(v)?;
            if a.is_empty()
                || a.iter()
                    .any(|o| !matches!(o.as_str(), "device" | "agent" | "user" | "script"))
            {
                return Err(fc.fail("origins", "origins must contain known origin names"));
            }
            a
        }
    };
    let pred = c.field(m, "pred")?;
    c.child("pred").object(pred)?;
    let anchor = m
        .get("anchor")
        .map(|v| anchor(v, &c.child("anchor")))
        .transpose()?;
    let within_ms = m
        .get("within_ms")
        .map(|v| milliseconds(v, &c.child("within_ms")))
        .transpose()?;
    Ok(Expectation {
        filter: Filter {
            domain,
            channel,
            kinds,
            origins,
            request_id,
        },
        pred: pred.clone(),
        anchor,
        within_ms,
        location: c.location,
        legacy: false,
    })
}
fn anchor(v: &Value, c: &Context) -> Result<Anchor, FormatError> {
    let m = c.object(v)?;
    c.keys(m, &["step", "kind", "where"])?;
    let step = c.child("step").id(c.field(m, "step")?)?;
    let kind = m.get("kind").map(|v| c.child("kind").string(v)).transpose()?;
    let where_clause = m
        .get("where")
        .map(|v| {
            c.child("where").object(v)?;
            Ok(v.clone())
        })
        .transpose()?;
    Ok(Anchor {
        step,
        kind,
        where_clause,
    })
}
fn timeout(m: &Map<String, Value>, c: &Context) -> Result<Option<u64>, FormatError> {
    let a = m
        .get("timeout_ms")
        .map(|v| milliseconds(v, &c.child("timeout_ms")))
        .transpose()?;
    let b = m
        .get("timeout")
        .map(|v| duration(v, &c.child("timeout")))
        .transpose()?;
    if a.is_some() && b.is_some() && a != b {
        return Err(c.fail("timeout", "conflicting timeout and timeout_ms"));
    }
    Ok(a.or(b))
}
fn milliseconds(v: &Value, c: &Context) -> Result<u64, FormatError> {
    v.as_u64()
        .filter(|n| *n > 0 && *n <= MAX_TIMEOUT_MS)
        .ok_or_else(|| c.fail("duration", "milliseconds must be an integer in 1..=3600000"))
}
fn duration(v: &Value, c: &Context) -> Result<u64, FormatError> {
    if v.is_number() {
        return milliseconds(v, c);
    }
    let s = v.as_str().ok_or_else(|| {
        c.fail(
            "duration",
            "duration must be integer milliseconds or a duration string",
        )
    })?;
    let (n, mul) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60000)
    } else {
        return Err(c.fail("duration", "duration suffix must be ms, s or m"));
    };
    n.parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mul))
        .filter(|n| *n > 0 && *n <= MAX_TIMEOUT_MS)
        .ok_or_else(|| c.fail("duration", "duration outside bounds"))
}
pub(crate) fn action_timeout(a: &Action) -> Result<Option<u64>, FormatError> {
    let c = Context {
        location: a.location,
        path: format!("/{}", a.method),
    };
    let mut e = timeout(c.object(&a.args)?, &c);
    if let Err(err) = &mut e {
        err.location = a.location;
    }
    e
}
