use crate::{plan::action_timeout, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
/// At most one risk entry per action in the bounded input; never silently omit a branch.
pub const MAX_RISK_ENTRIES: usize = MAX_STEPS * MAX_ACTIONS;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    R0,
    R1,
    R2,
    R3,
    R4,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Risk {
    pub level: RiskLevel,
    pub safe_state: bool,
    pub default_blocked: bool,
    pub immutable_deny: bool,
}
impl Risk {
    pub const fn new(level: RiskLevel, safe_state: bool) -> Self {
        Self {
            level,
            safe_state,
            default_blocked: false,
            immutable_deny: false,
        }
    }
}
pub trait RiskProvider {
    fn classify(&self, method: &str, args: &Value) -> Risk;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub waiting: bool,
}
/// Implemented by the caller's real command registry + domain classifiers; no embedded fake RPCs.
pub trait CommandTable: RiskProvider {
    fn lookup(&self, method: &str) -> Option<CommandSpec>;
    fn validate_args(&self, _method: &str, _args: &Value) -> Result<(), String> {
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskUse {
    pub step: String,
    pub method: String,
    pub args: Value,
    pub risk: Risk,
    pub max_uses: u64,
    pub finally: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub risks: Vec<RiskUse>,
    pub maximum_risk: RiskLevel,
    pub plan_sha256: String,
    pub tc_source_sha256: Option<String>,
    pub tc_hash_verified: bool,
    /// Static validity is not evidence verification or approval. Legacy UDS expectations need an adapter.
    pub requires_legacy_adapter: bool,
}
impl Report {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}
fn issue(step: &Step, path: &str, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        path: path.into(),
        location: step.location,
    }
}
fn push(out: &mut Vec<Diagnostic>, mut d: Diagnostic) {
    if out.len() < MAX_DIAGNOSTICS {
        if d.message.len() > MAX_CELL_BYTES {
            let mut n = MAX_CELL_BYTES - 3;
            while !d.message.is_char_boundary(n) {
                n -= 1;
            }
            d.message.truncate(n);
            d.message.push_str("...");
        }
        out.push(d);
    }
}
pub fn check_plan(plan: &Plan, commands: &dyn CommandTable) -> Report {
    check_impl(plan, Some(commands))
}
/// Structural checks without a registry. No risk classification or command-existence claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralReport {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub plan_sha256: String,
    pub command_existence_checked: bool,
}
pub fn inspect_plan(plan: &Plan) -> StructuralReport {
    let r = check_impl(plan, None);
    let mut warnings = r.warnings;
    warnings.push(Diagnostic{code:"registry_unavailable".into(),message:"명령 존재 미검사: command existence, domain argument schemas, required timeouts and risk/finally classifications were not checked".into(),path:"/".into(),location:Location{line:1,column:1,offset:0}});
    StructuralReport {
        errors: r.errors,
        warnings,
        plan_sha256: r.plan_sha256,
        command_existence_checked: false,
    }
}
fn check_impl(plan: &Plan, commands: Option<&dyn CommandTable>) -> Report {
    let mut report = Report {
        errors: Vec::new(),
        warnings: plan.warnings().to_vec(),
        risks: Vec::new(),
        maximum_risk: RiskLevel::R0,
        plan_sha256: plan.plan_sha256().into(),
        tc_source_sha256: plan.tc_source_sha256().map(str::to_owned),
        tc_hash_verified: plan.tc_hash_verified(),
        requires_legacy_adapter: plan.requires_legacy_adapter(),
    };
    if !plan.tc_hash_verified() {
        report.warnings.push(Diagnostic {
            code: "tc_unverified".into(),
            message: "TC source bytes were not supplied; claimed TC hash is not verified".into(),
            path: "/tc".into(),
            location: Location {
                line: 1,
                column: 1,
                offset: 0,
            },
        });
    }
    let all = plan.steps().iter().chain(plan.finally()).collect::<Vec<_>>();
    let main_len = plan.steps().len();
    let mut ids = BTreeMap::new();
    for (i, s) in all.iter().enumerate() {
        if ids.insert(s.id.as_str(), i).is_some() {
            push(
                &mut report.errors,
                issue(s, "/id", "duplicate_id", format!("duplicate step id {}", s.id)),
            );
        }
    }
    let mut graph = vec![Vec::new(); all.len()];
    for (i, s) in all.iter().enumerate() {
        let is_final = i >= main_len;
        let prefix = format!(
            "/{}/{}",
            if is_final { "finally" } else { "steps" },
            if is_final { i - main_len } else { i }
        );
        if (i + 1 < main_len) || (is_final && i + 1 < all.len()) {
            graph[i].push(i + 1);
        }
        if let FailureAction::Goto(target) = &s.on_fail {
            match ids.get(target.as_str()).copied() {
                Some(j) if !is_final && j < main_len => graph[i].push(j),
                _ => push(
                    &mut report.errors,
                    issue(
                        s,
                        &format!("{prefix}/on_fail/goto"),
                        "goto_target",
                        format!("goto target {target} is missing or crosses finally"),
                    ),
                ),
            }
        }
        if let Some(when) = &s.when {
            match ids.get(when.step.as_str()).copied() {
                Some(j) if j < i => graph[j].push(i),
                Some(_) => push(
                    &mut report.errors,
                    issue(
                        s,
                        &format!("{prefix}/when"),
                        "when_cycle",
                        "when must depend on an earlier completed step",
                    ),
                ),
                None => push(
                    &mut report.errors,
                    issue(
                        s,
                        &format!("{prefix}/when"),
                        "when_target",
                        format!("when target {} does not exist", when.step),
                    ),
                ),
            }
        }
        for (j, e) in s.expect.iter().enumerate() {
            if let Some(anchor) = &e.anchor {
                match ids.get(anchor.step.as_str()).copied() {
                    Some(n) if n <= i => (),
                    _ => push(
                        &mut report.errors,
                        issue(
                            s,
                            &format!("{prefix}/expect/{j}/anchor"),
                            "anchor_target",
                            "anchor must refer to this or an earlier existing step",
                        ),
                    ),
                }
            }
        }
        if let Some(manual) = &s.manual
            && manual.timeout_ms.is_none()
        {
            push(
                &mut report.errors,
                issue(
                    s,
                    &format!("{prefix}/manual"),
                    "missing_timeout",
                    "manual wait needs manual.timeout or manual.timeout_ms",
                ),
            );
        }
        if is_final && (s.manual.is_some() || s.measure.is_some()) {
            push(
                &mut report.errors,
                issue(
                    s,
                    &prefix,
                    "unsafe_finally",
                    "finally only permits classified safe-state commands and bounded delays",
                ),
            );
        }
        let max_uses = u64::from(s.repeat_count) * (u64::from(s.retry_count) + 1);
        for (j, a) in s.actions.iter().enumerate() {
            let path = format!("{prefix}/actions/{j}");
            if commands.is_none() && a.delay_ms.is_none() {
                if let Err(mut e) = action_timeout(a) {
                    e.path = path.clone();
                    push(&mut report.errors, e);
                }
                continue;
            }
            let risk = if a.delay_ms.is_some() {
                Risk::new(RiskLevel::R0, true)
            } else {
                let Some(commands) = commands else {
                    continue;
                };
                let Some(spec) = commands.lookup(&a.method) else {
                    push(
                        &mut report.errors,
                        issue(
                            s,
                            &path,
                            "unknown_command",
                            format!("command {} is not registered", a.method),
                        ),
                    );
                    continue;
                };
                if let Err(message) = commands.validate_args(&a.method, &a.args) {
                    push(&mut report.errors, issue(s, &path, "arguments", message));
                }
                match action_timeout(a) {
                    Err(mut e) => {
                        e.path = path.clone();
                        push(&mut report.errors, e);
                    }
                    Ok(None) if spec.waiting => push(
                        &mut report.errors,
                        issue(
                            s,
                            &path,
                            "missing_timeout",
                            format!("waiting command {} requires timeout or timeout_ms", a.method),
                        ),
                    ),
                    _ => (),
                }
                commands.classify(&a.method, &a.args)
            };
            if risk.default_blocked || risk.immutable_deny {
                push(
                    &mut report.errors,
                    issue(
                        s,
                        &path,
                        "blocked_command",
                        format!("{} is default_blocked/immutable_deny", a.method),
                    ),
                );
            }
            if is_final && (risk.level > RiskLevel::R1 || !risk.safe_state) {
                push(
                    &mut report.errors,
                    issue(
                        s,
                        &path,
                        "unsafe_finally",
                        format!("finally {} requires R0–R1 AND safe_state", a.method),
                    ),
                );
            }
            report.maximum_risk = report.maximum_risk.max(risk.level);
            if report.risks.len() < MAX_RISK_ENTRIES {
                report.risks.push(RiskUse {
                    step: s.id.clone(),
                    method: a.method.clone(),
                    args: a.args.clone(),
                    risk,
                    max_uses,
                    finally: is_final,
                });
            } else {
                push(
                    &mut report.errors,
                    issue(s, &path, "risk_limit", "risk summary entry limit"),
                );
            }
        }
    }
    // Kahn's algorithm includes normal success flow and goto failure edges. A backward
    // goto is a loop even if its target has no explicit goto of its own.
    let mut incoming = vec![0usize; all.len()];
    for edges in &graph {
        for j in edges {
            incoming[*j] += 1;
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(i) = ready.pop_front() {
        visited += 1;
        for j in &graph[i] {
            incoming[*j] -= 1;
            if incoming[*j] == 0 {
                ready.push_back(*j);
            }
        }
    }
    if visited != all.len()
        && let Some((i, _)) = incoming.iter().enumerate().find(|(_, n)| **n > 0)
    {
        push(
            &mut report.errors,
            issue(
                all[i],
                "/steps",
                "cycle",
                "control-flow cycle; use bounded retry/repeat instead of backward goto",
            ),
        );
    }
    report
}
