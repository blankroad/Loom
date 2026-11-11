//! TEST plan services built from the real domain registries (`domains::builtin`).
//!
//! [`RegistryCommands`] is the `CommandTable`/`RiskProvider` the TEST module checks plans with:
//! * **lookup** — only commands a domain registered (no embedded method list);
//! * **arguments** — the registered roles, write class, `Bounds::max_wait` and params JSON Schema;
//! * **risk** — the domain's own `WriteHooks::classify` (the same hook the gateway write pipeline
//!   calls), blocks from that hook, and `safe_state` only from a registered `SafeStateSpec`.
//!
//! What this does **not** provide stays unavailable (see [`test_module`]): source fences, recording
//! health, STOP from CORE, plan approval and the `finally` dispatcher. Plan-time risk is a preview;
//! the gateway classifies every real write again when it is dispatched.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::{
    Classification, DomainError, Registry, RoleSet, SafeStateSpec, TargetRef, WriteHooks, WriteRequest,
};
use loom_proto::event::{Env, Origin, Risk as ProtoRisk};
use loom_proto::policy::{BlockClass, WriteClass};
use loom_test::{NotWiredHooks, PredicateEngine, SessionConfig, TestModule};
use loom_test_format::{CommandSpec, CommandTable, Risk, RiskLevel, RiskProvider};
use serde_json::{Map, Value};

/// Client id used when a plan action is classified (no connection exists at plan time).
pub const PLAN_CLIENT_ID: &str = "test.plan-check";
/// Plan-level runner deadline keys (`loom-test-format` treats them as one value).
const DEADLINE_KEYS: [&str; 2] = ["timeout", "timeout_ms"];
/// Same ceiling as `loom_test_format::MAX_TIMEOUT_MS`.
const MAX_DEADLINE_MS: u64 = 3_600_000;
/// Schema evaluation budget per argument check (nodes visited), guards `$ref` loops.
const MAX_SCHEMA_STEPS: usize = 100_000;
/// Schema/instance nesting ceiling for one argument check.
const MAX_SCHEMA_DEPTH: usize = 64;
/// Argument problems reported per action; the rest are counted.
const MAX_ARG_ERRORS: usize = 8;
/// Every environment is classified; the highest risk wins (CAN sends are R1 on sim, R2 on hardware).
const ENVS: [Env; 3] = [Env::Sim, Env::Bench, Env::Vehicle];

/// One registered command, copied out of its domain `Registry`.
struct Entry {
    domain: String,
    write_class: Option<WriteClass>,
    roles: RoleSet,
    params: Value,
    max_wait: Duration,
    hooks: Option<Arc<dyn WriteHooks>>,
    safe_states: Vec<SafeStateSpec>,
}

/// Command table + risk provider over the real domain registries.
pub struct RegistryCommands {
    entries: BTreeMap<String, Entry>,
}

impl std::fmt::Debug for RegistryCommands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryCommands")
            .field("commands", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RegistryCommands {
    /// Copy every command of `regs`. TEST's own registry is skipped: a plan cannot drive `test.*`.
    pub fn from_registries(regs: &[&Registry]) -> Self {
        let mut entries = BTreeMap::new();
        for r in regs.iter().filter(|r| r.domain() != loom_test::DOMAIN) {
            let safe: Vec<SafeStateSpec> = r.safe_states().to_vec();
            for c in r.commands() {
                entries.insert(
                    c.method.clone(),
                    Entry {
                        domain: r.domain().to_owned(),
                        write_class: c.write_class,
                        roles: c.roles,
                        params: c.params.as_value().clone(),
                        max_wait: c.bounds.max_wait,
                        hooks: r.hooks_ref().cloned(),
                        safe_states: safe.iter().filter(|s| s.method == c.method).cloned().collect(),
                    },
                );
            }
        }
        Self { entries }
    }

    /// Number of registered commands available to plans.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no domain registered a command.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Params a dispatcher sends for a plan action: the plan args without the plan-level deadline
    /// (`timeout`/`timeout_ms`), plus `timeout_ms` when the command's schema declares it. A future
    /// step/`finally` dispatcher must use this same function so the check matches the dispatch.
    pub fn rpc_params(&self, method: &str, args: &Value) -> Result<Value, String> {
        let entry = self
            .entries
            .get(method)
            .ok_or_else(|| format!("command {method} is not registered"))?;
        rpc_params(entry, args)
    }

    /// Domain classification of a plan action (max over environments), for display/tests.
    pub fn classification(&self, method: &str, args: &Value) -> Option<(Risk, String)> {
        let entry = self.entries.get(method)?;
        Some(classify_entry(method, entry, args))
    }
}

impl RiskProvider for RegistryCommands {
    fn classify(&self, method: &str, args: &Value) -> Risk {
        match self.entries.get(method) {
            Some(entry) => classify_entry(method, entry, args).0,
            // Unknown commands never look harmless (check_plan reports them before classifying).
            None => Risk {
                level: RiskLevel::R4,
                safe_state: false,
                default_blocked: false,
                immutable_deny: true,
            },
        }
    }
}

impl CommandTable for RegistryCommands {
    fn lookup(&self, method: &str) -> Option<CommandSpec> {
        let entry = self.entries.get(method)?;
        // The plan deadline becomes the RPC's required `timeout_ms`, so the plan must give one.
        Some(CommandSpec {
            waiting: required(&entry.params).any(|k| k == "timeout_ms"),
        })
    }

    fn validate_args(&self, method: &str, args: &Value) -> Result<(), String> {
        let entry = self
            .entries
            .get(method)
            .ok_or_else(|| format!("command {method} is not registered"))?;
        if !entry.roles.script {
            return Err(format!(
                "{method} may not be called by a test script (registered roles exclude script)"
            ));
        }
        match entry.write_class {
            None | Some(WriteClass::Request) => (),
            Some(class) => {
                return Err(format!(
                    "{method} is a {class:?} write; the gateway does not dispatch job/interactive writes yet"
                ));
            }
        }
        if let Some(ms) = deadline_ms(args)?
            && u128::from(ms) > entry.max_wait.as_millis()
        {
            return Err(format!(
                "timeout {ms} ms exceeds the registered max_wait {} ms of {method}",
                entry.max_wait.as_millis()
            ));
        }
        let params = rpc_params(entry, args)?;
        let mut v = Validator {
            root: &entry.params,
            steps: 0,
            errors: Vec::new(),
            total: 0,
        };
        v.check(&entry.params, &params, "", 0);
        if v.total == 0 {
            return Ok(());
        }
        let mut msg = format!("{method} arguments do not match the registered schema: ");
        msg.push_str(&v.errors.join("; "));
        if v.total > v.errors.len() {
            msg.push_str(&format!("; … {} more", v.total - v.errors.len()));
        }
        Err(msg)
    }
}

/// TEST module wired to the real command/risk table and domain evidence verifiers. Session hooks
/// stay [`NotWiredHooks`]: no source fence, recording health `Unknown`, approval/finally
/// `E_NOT_IMPLEMENTED` — so no automatic PASS can be confirmed until CORE wires them.
pub fn test_module(regs: &[&Registry]) -> Result<TestModule, DomainError> {
    TestModule::with_services(
        SessionConfig::default(),
        Some(Arc::new(RegistryCommands::from_registries(regs))),
        Arc::new(PredicateEngine::from_registries(regs)),
        Arc::new(NotWiredHooks),
    )
}

fn required(schema: &Value) -> impl Iterator<Item = &str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn declares(schema: &Value, key: &str) -> bool {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|p| p.contains_key(key))
}

/// Plan deadline in ms, mirroring `loom-test-format` (`timeout_ms` integer, `timeout` integer ms or
/// `<n>ms|s|m`). Malformed values are left to the format check, which reports them with a location.
fn deadline_ms(args: &Value) -> Result<Option<u64>, String> {
    let parse = |key: &str| -> Option<u64> {
        let v = args.get(key)?;
        let ms = match v {
            Value::Number(_) => v.as_u64(),
            Value::String(s) if key == "timeout" => {
                let (n, mul) = if let Some(n) = s.strip_suffix("ms") {
                    (n, 1)
                } else if let Some(n) = s.strip_suffix('s') {
                    (n, 1000)
                } else if let Some(n) = s.strip_suffix('m') {
                    (n, 60_000)
                } else {
                    return None;
                };
                n.parse::<u64>().ok()?.checked_mul(mul)
            }
            _ => None,
        }?;
        (1..=MAX_DEADLINE_MS).contains(&ms).then_some(ms)
    };
    let (a, b) = (parse("timeout_ms"), parse("timeout"));
    if a.is_some() && b.is_some() && a != b {
        return Err("conflicting timeout and timeout_ms".into());
    }
    Ok(a.or(b))
}

fn rpc_params(entry: &Entry, args: &Value) -> Result<Value, String> {
    let Value::Object(map) = args else {
        return Err("action arguments must be an object".into());
    };
    let deadline = deadline_ms(args)?;
    let mut out: Map<String, Value> = map
        .iter()
        .filter(|(k, _)| !DEADLINE_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(ms) = deadline
        && declares(&entry.params, "timeout_ms")
    {
        out.insert("timeout_ms".into(), Value::from(ms));
    }
    Ok(Value::Object(out))
}

/// Same channel keys as the gateway write pipeline (`daemon::call_channel`).
fn call_channel(args: &Value) -> Option<String> {
    ["channel", "ch", "alias"]
        .iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
}

fn level(r: ProtoRisk) -> RiskLevel {
    match r {
        ProtoRisk::R0 => RiskLevel::R0,
        ProtoRisk::R1 => RiskLevel::R1,
        ProtoRisk::R2 => RiskLevel::R2,
        ProtoRisk::R3 => RiskLevel::R3,
        ProtoRisk::R4 => RiskLevel::R4,
    }
}

/// A registered safe-state action: same method and every arg of the spec present and equal.
fn is_safe_state(entry: &Entry, params: &Value) -> bool {
    entry.safe_states.iter().any(|s| match &s.args {
        Value::Object(want) => want.iter().all(|(k, v)| params.get(k) == Some(v)),
        other => other == params,
    })
}

fn classify_entry(method: &str, entry: &Entry, args: &Value) -> (Risk, String) {
    let params = match rpc_params(entry, args) {
        Ok(p) => p,
        Err(why) => {
            return (
                Risk {
                    level: RiskLevel::R4,
                    safe_state: false,
                    default_blocked: false,
                    immutable_deny: true,
                },
                format!("unclassifiable arguments: {why}"),
            );
        }
    };
    let safe_state = is_safe_state(entry, &params);
    // Registry contract: `write_class: None` is a read (R0). Reads are not safe-state actions.
    let Some(write_class) = entry.write_class else {
        return (Risk::new(RiskLevel::R0, false), "read".into());
    };
    // The gateway's own fallback for a domain without hooks (`daemon::write_request`).
    let Some(hooks) = &entry.hooks else {
        return (
            Risk::new(RiskLevel::R1, safe_state),
            "no domain hooks; gateway default R1".into(),
        );
    };
    let target = TargetRef {
        channel: call_channel(&params),
        target: params.get("target").and_then(Value::as_str).map(str::to_owned),
        generation: None,
    };
    let mut risk = Risk::new(RiskLevel::R0, safe_state);
    let mut reasons: Vec<String> = Vec::new();
    for env in ENVS {
        let req = WriteRequest {
            domain: &entry.domain,
            method,
            target: &target,
            bytes: None,
            args: &params,
            origin: Origin::Script,
            client_id: PLAN_CLIENT_ID,
            env,
            write_class,
            job: None,
        };
        let Classification {
            risk: r,
            reason,
            block,
        } = hooks.classify(&req);
        risk.level = risk.level.max(level(r));
        match block {
            Some(BlockClass::DefaultBlocked) => risk.default_blocked = true,
            Some(BlockClass::HardDeny | BlockClass::ImmutableDeny) => risk.immutable_deny = true,
            None => (),
        }
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    (risk, reasons.join(" | "))
}

/// Bounded JSON Schema (2020-12 subset) check of the keywords the domains register. An assertion
/// keyword it cannot evaluate fails the check instead of being skipped.
struct Validator<'a> {
    root: &'a Value,
    steps: usize,
    errors: Vec<String>,
    total: usize,
}

const ANNOTATIONS: [&str; 10] = [
    "$schema",
    "$id",
    "$comment",
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
];

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_ok(want: &str, v: &Value) -> bool {
    let have = type_name(v);
    want == have
        || (want == "number" && have == "integer")
        || (want == "integer" && v.as_f64().is_some_and(|f| f.fract() == 0.0) && v.is_number())
}

fn int_range(format: &str) -> Option<(i128, i128)> {
    Some(match format {
        "uint8" => (0, i128::from(u8::MAX)),
        "uint16" => (0, i128::from(u16::MAX)),
        "uint32" => (0, i128::from(u32::MAX)),
        "uint64" | "uint" => (0, i128::from(u64::MAX)),
        "int8" => (i128::from(i8::MIN), i128::from(i8::MAX)),
        "int16" => (i128::from(i16::MIN), i128::from(i16::MAX)),
        "int32" => (i128::from(i32::MIN), i128::from(i32::MAX)),
        "int64" | "int" => (i128::from(i64::MIN), i128::from(i64::MAX)),
        _ => return None,
    })
}

fn as_i128(v: &Value) -> Option<i128> {
    v.as_i64().map(i128::from).or_else(|| v.as_u64().map(i128::from))
}

impl<'a> Validator<'a> {
    fn fail(&mut self, path: &str, msg: impl Into<String>) {
        self.total += 1;
        if self.errors.len() < MAX_ARG_ERRORS {
            let at = if path.is_empty() { "/" } else { path };
            self.errors.push(format!("{at}: {}", msg.into()));
        }
    }

    /// Run `schema` against `v` in isolation (for anyOf/oneOf/not); returns `true` when valid.
    fn probe(&mut self, schema: &'a Value, v: &Value, path: &str, depth: usize) -> bool {
        let (errors, total) = (std::mem::take(&mut self.errors), self.total);
        self.total = 0;
        self.check(schema, v, path, depth);
        let ok = self.total == 0;
        self.errors = errors;
        self.total = total;
        ok
    }

    fn resolve(&self, r: &str) -> Option<&'a Value> {
        let pointer = r.strip_prefix('#')?;
        self.root.pointer(pointer)
    }

    fn check(&mut self, schema: &'a Value, v: &Value, path: &str, depth: usize) {
        self.steps += 1;
        if self.steps > MAX_SCHEMA_STEPS || depth > MAX_SCHEMA_DEPTH {
            self.fail(path, "argument schema evaluation budget exceeded");
            return;
        }
        let map = match schema {
            Value::Bool(true) => return,
            Value::Bool(false) => return self.fail(path, "no value is allowed here"),
            Value::Object(m) => m,
            _ => return self.fail(path, "registered schema is not an object"),
        };
        for (k, kv) in map {
            match k.as_str() {
                k if ANNOTATIONS.contains(&k) => (),
                "$defs" | "definitions" => (),
                "$ref" => match kv.as_str().and_then(|r| self.resolve(r)) {
                    Some(target) => self.check(target, v, path, depth + 1),
                    None => self.fail(path, format!("unsupported schema reference {kv}")),
                },
                "type" => {
                    let wants: Vec<&str> = match kv {
                        Value::String(s) => vec![s.as_str()],
                        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                        _ => Vec::new(),
                    };
                    if !wants.iter().any(|w| type_ok(w, v)) {
                        self.fail(
                            path,
                            format!("expected {}, got {}", wants.join("|"), type_name(v)),
                        );
                    }
                }
                "enum" => {
                    if !kv.as_array().is_some_and(|a| a.contains(v)) {
                        self.fail(path, format!("must be one of {kv}"));
                    }
                }
                "const" => {
                    if kv != v {
                        self.fail(path, format!("must be {kv}"));
                    }
                }
                "format" => {
                    if let Some(f) = kv.as_str()
                        && v.is_number()
                        && let Some((lo, hi)) = int_range(f)
                        && !as_i128(v).is_some_and(|x| (lo..=hi).contains(&x))
                    {
                        self.fail(path, format!("outside {f} range"));
                    }
                    // Other formats are annotations in 2020-12.
                }
                "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                    if let (Some(x), Some(lim)) = (v.as_f64(), kv.as_f64()) {
                        let ok = match k.as_str() {
                            "minimum" => x >= lim,
                            "maximum" => x <= lim,
                            "exclusiveMinimum" => x > lim,
                            _ => x < lim,
                        };
                        if !ok {
                            self.fail(path, format!("{k} {lim}"));
                        }
                    }
                }
                "minLength" | "maxLength" => {
                    if let (Some(s), Some(lim)) = (v.as_str(), kv.as_u64()) {
                        let n = s.chars().count() as u64;
                        if (k == "minLength" && n < lim) || (k == "maxLength" && n > lim) {
                            self.fail(path, format!("{k} {lim}"));
                        }
                    }
                }
                "pattern" => {
                    if v.is_string() {
                        // No regex engine in the gateway: a constrained string is not claimed valid.
                        self.fail(
                            path,
                            format!("pattern {kv} cannot be checked by the TEST plan check"),
                        );
                    }
                }
                "minItems" | "maxItems" => {
                    if let (Some(a), Some(lim)) = (v.as_array(), kv.as_u64()) {
                        let n = a.len() as u64;
                        if (k == "minItems" && n < lim) || (k == "maxItems" && n > lim) {
                            self.fail(path, format!("{k} {lim}"));
                        }
                    }
                }
                "items" => {
                    if let Some(a) = v.as_array() {
                        for (i, x) in a.iter().enumerate() {
                            self.check(kv, x, &format!("{path}/{i}"), depth + 1);
                        }
                    }
                }
                "minProperties" | "maxProperties" => {
                    if let (Some(o), Some(lim)) = (v.as_object(), kv.as_u64()) {
                        let n = o.len() as u64;
                        if (k == "minProperties" && n < lim) || (k == "maxProperties" && n > lim) {
                            self.fail(path, format!("{k} {lim}"));
                        }
                    }
                }
                "required" => {
                    if let Some(o) = v.as_object() {
                        for r in kv.as_array().into_iter().flatten().filter_map(Value::as_str) {
                            if !o.contains_key(r) {
                                self.fail(path, format!("missing required {r}"));
                            }
                        }
                    }
                }
                "properties" => {
                    if let (Some(o), Some(props)) = (v.as_object(), kv.as_object()) {
                        for (name, sub) in props {
                            if let Some(x) = o.get(name) {
                                self.check(sub, x, &format!("{path}/{name}"), depth + 1);
                            }
                        }
                    }
                }
                "additionalProperties" => {
                    if let Some(o) = v.as_object() {
                        let props = map.get("properties").and_then(Value::as_object);
                        for (name, x) in o {
                            if props.is_some_and(|p| p.contains_key(name)) {
                                continue;
                            }
                            match kv {
                                Value::Bool(false) => {
                                    self.fail(&format!("{path}/{name}"), "unknown argument")
                                }
                                _ => self.check(kv, x, &format!("{path}/{name}"), depth + 1),
                            }
                        }
                    }
                }
                "allOf" | "anyOf" | "oneOf" => {
                    let subs: &'a [Value] = kv.as_array().map_or(&[], Vec::as_slice);
                    if k == "allOf" {
                        for s in subs {
                            self.check(s, v, path, depth + 1);
                        }
                        continue;
                    }
                    let passed = subs.iter().filter(|s| self.probe(s, v, path, depth + 1)).count();
                    if (k == "anyOf" && passed == 0) || (k == "oneOf" && passed != 1) {
                        self.fail(
                            path,
                            format!("{k}: {passed} of {} alternatives match", subs.len()),
                        );
                    }
                }
                "not" => {
                    if self.probe(kv, v, path, depth + 1) {
                        self.fail(path, "matches a forbidden schema");
                    }
                }
                other => self.fail(path, format!("schema keyword {other} cannot be checked")),
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use loom_test_format::{check_plan, parse_plan, parse_plan_with_tc, parse_tc};
    use serde_json::json;

    fn table() -> RegistryCommands {
        let loaded = crate::domains::builtin();
        let regs: Vec<&Registry> = loaded.iter().map(|l| &l.registry).collect();
        RegistryCommands::from_registries(&regs)
    }

    fn errors(r: &loom_test_format::Report) -> Vec<(String, String, String)> {
        r.errors
            .iter()
            .map(|d| (d.code.clone(), d.path.clone(), d.message.clone()))
            .collect()
    }

    #[test]
    fn lookup_uses_only_registered_commands() {
        let t = table();
        assert!(!t.is_empty());
        assert!(t.lookup("psu.out").is_some());
        assert!(t.lookup("can.sig.wait_for").is_some());
        // Names that only exist in the TC-1 fixture/fakes are not invented here.
        assert!(t.lookup("can.sig.wait-for").is_none());
        assert!(t.lookup("uds.dtc.clear").is_none());
        // TEST cannot drive itself from a plan.
        assert!(t.lookup("test.start").is_none());
        assert!(t.lookup("core.stop").is_none());
        // Waiting = the registered schema requires the deadline.
        assert!(t.lookup("uart.expect").is_some_and(|s| s.waiting));
        assert!(t.lookup("psu.wait_for").is_some_and(|s| s.waiting));
        assert!(t.lookup("psu.out").is_some_and(|s| !s.waiting));
        // Unknown commands never classify as harmless.
        let r = t.classify("nope.write", &json!({}));
        assert_eq!(r.level, RiskLevel::R4);
        assert!(r.immutable_deny);
    }

    #[test]
    fn arguments_follow_registered_schema_roles_and_bounds() {
        let t = table();
        t.validate_args(
            "uart.expect",
            &json!({"channel":"dut","patterns":["login:"],"timeout":"30s"}),
        )
        .unwrap();
        assert_eq!(
            t.rpc_params(
                "uart.expect",
                &json!({"channel":"dut","patterns":["login:"],"timeout":"30s"})
            )
            .unwrap(),
            json!({"channel":"dut","patterns":["login:"],"timeout_ms":30000})
        );
        // TC-1 fixture shape is not the registered shape.
        let e = t
            .validate_args(
                "uart.expect",
                &json!({"port":"dut","pattern":"login:","timeout":"30s"}),
            )
            .unwrap_err();
        assert!(
            e.contains("missing required channel") && e.contains("missing required patterns"),
            "{e}"
        );
        // Registered bounds: maximum / format / additionalProperties:false.
        assert!(
            t.validate_args("uds.dtc.list", &json!({"target":"bcm","mask":9}))
                .is_ok()
        );
        let e = t
            .validate_args("uds.dtc.list", &json!({"target":"bcm","mask":256}))
            .unwrap_err();
        assert!(e.contains("/mask"), "{e}");
        let e = t
            .validate_args(
                "can.sig.wait_for",
                &json!({"expr":"A == 1","within_ms":5000,"cond":"x"}),
            )
            .unwrap_err();
        assert!(e.contains("/cond: unknown argument"), "{e}");
        t.validate_args(
            "can.sig.wait_for",
            &json!({"channel":"ch0","expr":"VehState.PowerMode == 2","within_ms":5000,"timeout":"5s"}),
        )
        .unwrap();
        // Enum, $ref through anyOf.
        assert!(t.validate_args("psu.out", &json!({"state":"maybe"})).is_err());
        assert!(
            t.validate_args(
                "can.send",
                &json!({"channel":"ch0","id":256,"expect":{"id":1,"within_ms":10}})
            )
            .is_ok()
        );
        let e = t
            .validate_args("can.send", &json!({"channel":"ch0","id":256,"expect":{"id":1}}))
            .unwrap_err();
        assert!(e.contains("anyOf"), "{e}");
        // Deadline beyond the registered max_wait (psu.out: 30 s).
        let e = t
            .validate_args("psu.out", &json!({"state":"on","timeout":"31s"}))
            .unwrap_err();
        assert!(e.contains("max_wait"), "{e}");
        // Roles: viewer-only commands are not script actions.
        assert!(
            t.validate_args("uart.input.takeover", &json!({}))
                .unwrap_err()
                .contains("roles")
        );
        // Job writes are refused by the gateway today, so a plan cannot claim them.
        assert!(
            t.validate_args("can.periodic.start", &json!({"channel":"ch0"}))
                .unwrap_err()
                .contains("job/interactive")
        );
        // A pattern-constrained string is not claimed valid without a regex engine.
        assert!(
            t.validate_args("uart.open", &json!({"target":"sim://x","alias":"dut"}))
                .unwrap_err()
                .contains("pattern")
        );
        assert!(t.validate_args("uart.open", &json!({"target":"sim://x"})).is_ok());
    }

    #[test]
    fn risk_comes_from_domain_hooks() {
        let t = table();
        // PSU hooks: output on/off are R3; OFF matches the registered safe state.
        let on = t.classify("psu.out", &json!({"state":"on"}));
        assert_eq!(on.level, RiskLevel::R3);
        assert!(!on.safe_state);
        let off = t.classify("psu.out", &json!({"ch":"bench","state":"off"}));
        assert_eq!(off.level, RiskLevel::R3);
        assert!(off.safe_state);
        // UDS hooks classify the encoded PDU: 0x19 read DTC = R0.
        assert_eq!(
            t.classify("uds.dtc.list", &json!({"target":"bcm","mask":9}))
                .level,
            RiskLevel::R0
        );
        // Reads are R0 by the registry contract, not by a fallback.
        assert_eq!(
            t.classify(
                "uart.expect",
                &json!({"channel":"dut","patterns":["x"],"timeout_ms":10})
            ),
            Risk::new(RiskLevel::R0, false)
        );
        // CAN send: R1 on sim, R2 on hardware → the plan sees the higher class.
        assert_eq!(
            t.classify("can.send", &json!({"channel":"ch0","frame":"123#00"}))
                .level,
            RiskLevel::R2
        );
        // UDS immutable deny (0x86) comes back as a block from the domain hook.
        let deny = t.classify("uds.request", &json!({"target":"bcm","bytes_hex":"8601"}));
        assert!(deny.immutable_deny || deny.default_blocked, "{deny:?}");
        let (_, why) = t.classification("psu.out", &json!({"state":"off"})).unwrap();
        assert!(why.contains("output OFF"), "{why}");
    }

    #[test]
    fn tc1_fixture_reports_real_registry_mismatches() {
        let tc = parse_tc(include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md")).unwrap();
        let plan = parse_plan_with_tc(
            include_str!("../../loom-test-format/tests/fixtures/TC-1.plan.yaml"),
            &tc,
        )
        .unwrap();
        let report = check_plan(&plan, &table());
        let errs = errors(&report);
        let has = |code: &str, needle: &str| {
            errs.iter()
                .any(|(c, p, m)| c == code && (m.contains(needle) || p.contains(needle)))
        };
        assert!(has("unknown_command", "can.sig.wait-for"), "{errs:?}");
        assert!(has("unknown_command", "uds.dtc.clear"), "{errs:?}");
        assert!(has("arguments", "uart.expect"), "{errs:?}");
        assert!(has("unsafe_finally", "psu.out"), "{errs:?}");
        assert!(!report.is_valid());
    }

    #[test]
    fn registered_plan_checks_with_domain_risks() {
        let plan = parse_plan(
            r#"loom_tc_plan: 1
env: sim
steps:
  - id: S1
    do: [{psu.out: {ch: bench, state: "off"}}, {sleep: 2s}, {psu.out: {ch: bench, state: "on"}}]
  - id: S2
    uart.expect: {channel: dut, patterns: ["login:"], timeout: 30s}
  - id: S3
    can.sig.wait_for: {channel: ch0, expr: "VehState.PowerMode == 2", within_ms: 5000, timeout: 6s}
  - id: S4
    uds.dtc.list: {target: bcm, mask: 0x09}
"#,
        )
        .unwrap();
        let report = check_plan(&plan, &table());
        assert!(report.is_valid(), "{:?}", errors(&report));
        assert_eq!(report.maximum_risk, RiskLevel::R3);
        let psu: Vec<_> = report.risks.iter().filter(|r| r.method == "psu.out").collect();
        assert_eq!(psu.len(), 2);
        assert!(psu.iter().all(|r| r.risk.level == RiskLevel::R3));
        // A waiting command without a plan deadline is rejected by the format check.
        let missing = parse_plan(
            "loom_tc_plan: 1\nsteps:\n  - id: S1\n    uart.expect: {channel: dut, patterns: [\"x\"]}\n",
        )
        .unwrap();
        let errs = errors(&check_plan(&missing, &table()));
        assert!(errs.iter().any(|(c, ..)| c == "missing_timeout"), "{errs:?}");
    }

    #[test]
    fn module_keeps_session_hooks_unwired() {
        let loaded = crate::domains::builtin();
        let regs: Vec<&Registry> = loaded.iter().map(|l| &l.registry).collect();
        let m = test_module(&regs).unwrap();
        // Registers as the TEST domain; hooks are the explicit NotWired seam.
        let r = crate::domains::register(&m).unwrap();
        assert_eq!(r.domain(), loom_test::DOMAIN);
        let hooks = NotWiredHooks;
        use loom_test::SessionHooks;
        assert_eq!(hooks.recording_health(), loom_test::RecordingHealth::Unknown);
        assert!(hooks.fence("s", "S1", 1).unwrap().is_none());
        assert!(hooks.approval("s", loom_proto::num::U64(1), None).is_err());
    }
}
