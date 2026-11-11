//! Domain registration (contract v1 §8.1). [`Registry`] only stores what a domain declares and
//! validates names and limits; the gateway consumes it from P1.
//!
//! Every registration method returns `Result<&mut Self, RegisterError>` so calls chain with `?`
//! and later releases can add validation without changing signatures.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use loom_proto::event::{PublishedRecord, Risk, WriteState, validate_event_kind};
use loom_proto::evidence::validate_json_pointer;
use loom_proto::policy::{PolicyAction, WriteClass};
use loom_proto::rpc::{Rights, SubFilter, is_reserved_domain, validate_method};
use schemars::Schema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ctx::Handler;
use crate::hooks::WriteHooks;
use crate::is_token;
use crate::verify::EvidenceVerifier;

/// Registration failure (the domain is disabled and the reason shown in `loom status`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// Bad domain name or a reserved CORE domain.
    #[error("invalid domain {0:?}")]
    InvalidDomain(String),
    /// Name violates the naming rule.
    #[error("invalid name {name:?}: {reason}")]
    InvalidName {
        /// Offending name.
        name: String,
        /// Why.
        reason: String,
    },
    /// Name does not start with this domain.
    #[error("{name:?} does not belong to domain {domain:?}")]
    WrongDomain {
        /// Offending name.
        name: String,
        /// Registering domain.
        domain: String,
    },
    /// Registered twice.
    #[error("duplicate {what}: {name:?}")]
    Duplicate {
        /// Item type.
        what: &'static str,
        /// Name.
        name: String,
    },
    /// A value is outside the contract limits.
    #[error("invalid {what}: {reason}")]
    Invalid {
        /// Item type.
        what: &'static str,
        /// Why.
        reason: String,
    },
}

/// Who may call a command. Origins come from connection roles (D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSet {
    /// Agent connections (CLI).
    pub agent: bool,
    /// Runner / test actors.
    pub script: bool,
    /// Minimum viewer rights; `None` = viewers may not call it.
    pub viewer: Option<Rights>,
}

impl RoleSet {
    /// Reads: everyone, viewers with watch.
    pub const READ: RoleSet = RoleSet {
        agent: true,
        script: true,
        viewer: Some(Rights::Watch),
    };
    /// Writes: agent, script, viewers with operate.
    pub const WRITE: RoleSet = RoleSet {
        agent: true,
        script: true,
        viewer: Some(Rights::Operate),
    };
    /// Viewer-only operate (`uart.input`, recovery, take-control).
    pub const VIEWER_OPERATE: RoleSet = RoleSet {
        agent: false,
        script: false,
        viewer: Some(Rights::Operate),
    };
    /// Viewer-only approve (approvals, overrides).
    pub const VIEWER_APPROVE: RoleSet = RoleSet {
        agent: false,
        script: false,
        viewer: Some(Rights::Approve),
    };
}

/// Output and wait bounds of a command (D5·A6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    /// Longest the command may wait (≤ 3600 s; `--follow` streams ≤ 600 s).
    pub max_wait: Duration,
    /// Default rows of output.
    pub default_rows: u32,
    /// Hard row limit (≤ 1000).
    pub max_rows: u32,
}

impl Bounds {
    /// Contract ceiling for single-result waits.
    pub const MAX_WAIT: Duration = Duration::from_secs(3600);
    /// Contract ceiling for rows.
    pub const MAX_ROWS: u32 = 1000;
    /// Typical read command: 30 s, 100 rows, 1000 max.
    pub const DEFAULT: Bounds = Bounds {
        max_wait: Duration::from_secs(30),
        default_rows: 100,
        max_rows: 1000,
    };
}

/// One RPC command.
#[derive(Clone)]
pub struct CommandSpec {
    /// Method `<domain>.<verb>` / `<domain>.<noun>.<verb>`.
    pub method: String,
    /// Write class; `None` = read (R0).
    pub write_class: Option<WriteClass>,
    /// Params schema.
    pub params: Schema,
    /// Result `data` schema.
    pub result: Schema,
    /// Who may call it.
    pub roles: RoleSet,
    /// Handler.
    pub handler: Arc<dyn Handler>,
    /// Bounds.
    pub bounds: Bounds,
    /// Stricter default the domain suggests (F1: `None` = automatic; the user's policy wins).
    pub default_action: Option<PolicyAction>,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec")
            .field("method", &self.method)
            .field("write_class", &self.write_class)
            .field("roles", &self.roles)
            .field("bounds", &self.bounds)
            .field("default_action", &self.default_action)
            .finish_non_exhaustive()
    }
}

/// Type of a value at a fixed JSON Pointer (evidence predicates read these).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    /// JSON boolean.
    Bool,
    /// Unsigned 8-bit integer (JSON number).
    U8,
    /// Unsigned 16-bit integer.
    U16,
    /// Unsigned 32-bit integer.
    U32,
    /// Unsigned 64-bit integer as canonical decimal string.
    U64String,
    /// Signed 64-bit integer.
    I64,
    /// Floating point number.
    F64,
    /// String.
    String,
    /// Hex string.
    Hex,
    /// One of the listed strings.
    Enum(Vec<String>),
    /// Object.
    Object,
    /// Array.
    Array,
}

/// Fixed JSON Pointer of an event kind (path relative to the published record JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerSpec {
    /// RFC 6901 path, e.g. `/payload/state`.
    pub path: String,
    /// Unit, if numeric.
    pub unit: Option<String>,
    /// Value type.
    pub ty: ValueType,
}

/// One event kind of the domain.
#[derive(Debug, Clone, PartialEq)]
pub struct EventKindSpec {
    /// Kind `<domain>.<event>`.
    pub kind: String,
    /// Schema of `payload`.
    pub payload_schema: Schema,
    /// Fixed pointers usable in evidence.
    pub pointers: Vec<PointerSpec>,
    /// Payload schema version.
    pub version: u32,
}

/// One risk table row (domain-owned mapping, D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskRow {
    /// Method.
    pub method: String,
    /// Domain selector (UDS SID, UART rule …), `None` = whole method.
    pub selector: Option<String>,
    /// Class.
    pub risk: Risk,
    /// Reason.
    pub reason: String,
}

/// Risk table. Under F1 `default_blocked` is empty by default and `immutable_deny` lists only
/// services Loom does not implement.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RiskTable {
    /// Rows.
    pub rows: Vec<RiskRow>,
    /// Blocked until unlocked (empty by default).
    pub default_blocked: Vec<String>,
    /// Unimplemented services (never enabled).
    pub immutable_deny: Vec<String>,
}

/// How many worker processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instances {
    /// One process per channel (UART: max 8).
    PerChannel {
        /// Maximum processes.
        max: u16,
    },
    /// One process for all channels.
    Shared,
}

/// Memory/credit budgets of one worker process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budgets {
    /// Soft memory (bytes): pressure → shrink screen mirrors.
    pub mem_soft: u64,
    /// Hard memory (bytes; Windows = commit limit).
    pub mem_hard: u64,
    /// Data-lane credit window (bytes).
    pub credit: u64,
}

/// Channel reopen after a worker crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reopen {
    /// Never (UART — DTR reset risk).
    None,
    /// Only `autostart` channels, identity re-verified, RX only (CAN).
    RxOnly,
}

/// Worker binary architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    /// Same as `loomd`.
    Native,
    /// 32-bit x86 helper (32-bit vendor DLLs, seed-key).
    I686,
}

/// Host OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Windows x64.
    Windows,
    /// Linux x64.
    Linux,
    /// macOS (development, simulation).
    Macos,
}

/// A worker process type of the domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    /// Worker kind (`loomd worker <kind>`).
    pub kind: String,
    /// Instance model.
    pub instances: Instances,
    /// Budgets per process.
    pub budgets: Budgets,
    /// Reopen rule.
    pub reopen_after_crash: Reopen,
    /// Architecture.
    pub arch: Arch,
    /// Supported platforms.
    pub platforms: Vec<Platform>,
}

/// Where a gateway module's async tasks run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleRuntime {
    /// Shared gateway runtime.
    #[default]
    Shared,
    /// Dedicated runtime thread(s) (UDS timer jitter gate, uds C-13).
    Dedicated {
        /// Thread count (≥ 1).
        threads: u8,
    },
}

/// A safe-state action offered as "recovery send" after STOP (A15). Interlocks still apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafeStateSpec {
    /// Action name `<domain>.<name>`.
    pub name: String,
    /// Method invoked (origin = user).
    pub method: String,
    /// Arguments.
    pub args: Value,
    /// Text shown on the button.
    pub description: String,
}

/// Emergency OFF for output-holding devices (A3): no confirmation, interlocks only warn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmergencyOffSpec {
    /// Name `<domain>.<name>`.
    pub name: String,
    /// Method invoked.
    pub method: String,
    /// Targets (channels / outputs).
    pub targets: Vec<String>,
    /// Text shown.
    pub description: String,
}

/// Help example.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelpExample {
    /// Command line.
    pub cmd: String,
    /// What it does.
    pub explain: String,
}

/// Domain help (`loom <domain> help --json` is generated from commands + this; C8).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HelpSpec {
    /// One-paragraph summary.
    pub summary: String,
    /// Examples.
    pub examples: Vec<HelpExample>,
    /// Rules the agent must follow.
    pub notes: Vec<String>,
}

/// Snapshot provider: folds events into state shown to late-joining viewers.
pub trait Reducer: Send {
    /// Event kinds this reducer consumes.
    fn kinds(&self) -> &[&'static str];
    /// Apply one event (hub thread; must be quick and bounded).
    fn apply(&mut self, ev: &PublishedRecord);
    /// Snapshot parts for a subscriber filter.
    fn snapshot(&self, filter: &SubFilter) -> Vec<Value>;
}

/// Summarizes screen drops for `display.gap.detail` (e.g. top CAN ids).
pub trait DisplaySummarizer: Send + Sync {
    /// Summary of dropped events.
    fn summarize(&self, dropped: &[PublishedRecord]) -> Value;
}

/// Interlock predicate: `true` while the interlock is active (e.g. `uds.flashing`).
pub type Interlock = Arc<dyn Fn() -> bool + Send + Sync>;

/// Everything a domain registered.
pub struct Registry {
    domain: String,
    commands: Vec<CommandSpec>,
    event_kinds: Vec<EventKindSpec>,
    risk_table: Option<RiskTable>,
    hooks: Option<Arc<dyn WriteHooks>>,
    workers: Vec<WorkerSpec>,
    runtime: ModuleRuntime,
    reducers: Vec<Box<dyn Reducer>>,
    verifiers: Vec<Arc<dyn EvidenceVerifier>>,
    summarizer: Option<Arc<dyn DisplaySummarizer>>,
    capabilities: BTreeSet<String>,
    safe_states: Vec<SafeStateSpec>,
    emergency_offs: Vec<EmergencyOffSpec>,
    interlocks: Vec<(String, Interlock)>,
    panels: Vec<Value>,
    policy_section: Option<Schema>,
    help: Option<HelpSpec>,
    success_terminals: BTreeMap<String, WriteState>,
    worker_programs: BTreeMap<String, String>,
    worker_ticks: BTreeMap<String, Duration>,
    input_events: Owner,
}

/// Who publishes `<domain>.input` events for interactive input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// The core publishes `<domain>.input` (default).
    #[default]
    Core,
    /// The domain (its worker) publishes its own input events; the core only audits
    /// (`input.batch`).
    Domain,
}

/// Shortest worker tick.
pub const MIN_WORKER_TICK: Duration = Duration::from_millis(10);
/// Longest worker tick.
pub const MAX_WORKER_TICK: Duration = Duration::from_secs(60);

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("domain", &self.domain)
            .field("commands", &self.commands)
            .field(
                "event_kinds",
                &self.event_kinds.iter().map(|k| &k.kind).collect::<Vec<_>>(),
            )
            .field("workers", &self.workers)
            .field("runtime", &self.runtime)
            .field("reducers", &self.reducers.len())
            .field("verifiers", &self.verifiers.len())
            .field("capabilities", &self.capabilities)
            .field(
                "interlocks",
                &self.interlocks.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

fn invalid_name(name: &str, reason: impl Into<String>) -> RegisterError {
    RegisterError::InvalidName {
        name: name.to_owned(),
        reason: reason.into(),
    }
}

impl Registry {
    /// Registry for a domain crate. Reserved CORE domains are refused.
    pub fn new(domain: &str) -> Result<Self, RegisterError> {
        if !is_token(domain) || is_reserved_domain(domain) {
            return Err(RegisterError::InvalidDomain(domain.to_owned()));
        }
        Ok(Self::unchecked(domain))
    }

    /// Registry for CORE's own modules (may use reserved domains).
    pub fn for_core(domain: &str) -> Result<Self, RegisterError> {
        if !is_token(domain) {
            return Err(RegisterError::InvalidDomain(domain.to_owned()));
        }
        Ok(Self::unchecked(domain))
    }

    fn unchecked(domain: &str) -> Self {
        Registry {
            domain: domain.to_owned(),
            commands: Vec::new(),
            event_kinds: Vec::new(),
            risk_table: None,
            hooks: None,
            workers: Vec::new(),
            runtime: ModuleRuntime::Shared,
            reducers: Vec::new(),
            verifiers: Vec::new(),
            summarizer: None,
            capabilities: BTreeSet::new(),
            safe_states: Vec::new(),
            emergency_offs: Vec::new(),
            interlocks: Vec::new(),
            panels: Vec::new(),
            policy_section: None,
            help: None,
            success_terminals: BTreeMap::new(),
            worker_programs: BTreeMap::new(),
            worker_ticks: BTreeMap::new(),
            input_events: Owner::Core,
        }
    }

    /// `name` must be `<this domain>.<…>` with valid tokens.
    fn check_owned(&self, name: &str) -> Result<(), RegisterError> {
        match name.split_once('.') {
            Some((d, _)) if d == self.domain => Ok(()),
            _ => Err(RegisterError::WrongDomain {
                name: name.to_owned(),
                domain: self.domain.clone(),
            }),
        }
    }

    fn check_two_token(&self, name: &str) -> Result<(), RegisterError> {
        validate_event_kind(name).map_err(|e| invalid_name(name, e.to_string()))?;
        self.check_owned(name)
    }

    /// Register an RPC command.
    pub fn command(&mut self, spec: CommandSpec) -> Result<&mut Self, RegisterError> {
        validate_method(&spec.method).map_err(|e| invalid_name(&spec.method, e.to_string()))?;
        self.check_owned(&spec.method)?;
        if spec.bounds.max_wait > Bounds::MAX_WAIT
            || spec.bounds.max_rows > Bounds::MAX_ROWS
            || spec.bounds.default_rows > spec.bounds.max_rows
        {
            return Err(RegisterError::Invalid {
                what: "bounds",
                reason: format!(
                    "{}: max_wait ≤ 3600 s, default_rows ≤ max_rows ≤ 1000",
                    spec.method
                ),
            });
        }
        if spec.roles
            == (RoleSet {
                agent: false,
                script: false,
                viewer: None,
            })
        {
            return Err(RegisterError::Invalid {
                what: "roles",
                reason: format!("{}: nobody may call it", spec.method),
            });
        }
        if self.commands.iter().any(|c| c.method == spec.method) {
            return Err(RegisterError::Duplicate {
                what: "command",
                name: spec.method,
            });
        }
        self.commands.push(spec);
        Ok(self)
    }

    /// Register an event kind with its payload schema and fixed pointers.
    pub fn event_kind(&mut self, spec: EventKindSpec) -> Result<&mut Self, RegisterError> {
        self.check_two_token(&spec.kind)?;
        for p in &spec.pointers {
            validate_json_pointer(&p.path).map_err(|e| invalid_name(&p.path, e.to_string()))?;
            if !(p.path.starts_with("/payload/") || p.path.starts_with("/metrics/")) {
                return Err(invalid_name(
                    &p.path,
                    "pointer must be under /payload/ or /metrics/",
                ));
            }
        }
        if self.event_kinds.iter().any(|k| k.kind == spec.kind) {
            return Err(RegisterError::Duplicate {
                what: "event kind",
                name: spec.kind,
            });
        }
        self.event_kinds.push(spec);
        Ok(self)
    }

    /// Set the risk table (once).
    pub fn risk_table(&mut self, table: RiskTable) -> Result<&mut Self, RegisterError> {
        if self.risk_table.is_some() {
            return Err(RegisterError::Duplicate {
                what: "risk table",
                name: self.domain.clone(),
            });
        }
        for row in &table.rows {
            self.check_owned(&row.method)?;
        }
        self.risk_table = Some(table);
        Ok(self)
    }

    /// Set the write hooks (once).
    pub fn hooks(&mut self, hooks: Arc<dyn WriteHooks>) -> Result<&mut Self, RegisterError> {
        if self.hooks.is_some() {
            return Err(RegisterError::Duplicate {
                what: "hooks",
                name: self.domain.clone(),
            });
        }
        self.hooks = Some(hooks);
        Ok(self)
    }

    /// Declare a worker process type.
    pub fn worker(&mut self, spec: WorkerSpec) -> Result<&mut Self, RegisterError> {
        if !is_token(&spec.kind) {
            return Err(invalid_name(&spec.kind, "worker kind must be a token"));
        }
        let b = spec.budgets;
        if b.mem_soft == 0 || b.mem_soft > b.mem_hard || b.credit == 0 {
            return Err(RegisterError::Invalid {
                what: "budgets",
                reason: format!("{}: 0 < soft ≤ hard, credit > 0", spec.kind),
            });
        }
        if matches!(spec.instances, Instances::PerChannel { max: 0 }) || spec.platforms.is_empty() {
            return Err(RegisterError::Invalid {
                what: "worker",
                reason: format!("{}: instances ≥ 1 and ≥ 1 platform", spec.kind),
            });
        }
        if self.workers.iter().any(|w| w.kind == spec.kind) {
            return Err(RegisterError::Duplicate {
                what: "worker",
                name: spec.kind,
            });
        }
        self.workers.push(spec);
        Ok(self)
    }

    /// Choose where the module's tasks run.
    pub fn runtime(&mut self, rt: ModuleRuntime) -> Result<&mut Self, RegisterError> {
        if rt == (ModuleRuntime::Dedicated { threads: 0 }) {
            return Err(RegisterError::Invalid {
                what: "runtime",
                reason: "threads ≥ 1".into(),
            });
        }
        self.runtime = rt;
        Ok(self)
    }

    /// Add a reducer (snapshot provider).
    pub fn reducer(&mut self, r: Box<dyn Reducer>) -> Result<&mut Self, RegisterError> {
        for k in r.kinds() {
            validate_event_kind(k).map_err(|e| invalid_name(k, e.to_string()))?;
        }
        self.reducers.push(r);
        Ok(self)
    }

    /// Add an evidence verifier.
    pub fn evidence_verifier(&mut self, v: Arc<dyn EvidenceVerifier>) -> Result<&mut Self, RegisterError> {
        for p in v.predicates() {
            self.check_two_token(p)?;
        }
        self.verifiers.push(v);
        Ok(self)
    }

    /// Set the display-gap summarizer (once).
    pub fn display_summarizer(&mut self, s: Arc<dyn DisplaySummarizer>) -> Result<&mut Self, RegisterError> {
        if self.summarizer.is_some() {
            return Err(RegisterError::Duplicate {
                what: "display summarizer",
                name: self.domain.clone(),
            });
        }
        self.summarizer = Some(s);
        Ok(self)
    }

    /// Declare a capability (`break`, `fd`, `doip` …).
    pub fn capability(&mut self, name: &str) -> Result<&mut Self, RegisterError> {
        if name.is_empty() || name.len() > 64 || !name.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid_name(name, "capability must be 1–64 printable ASCII"));
        }
        self.capabilities.insert(name.to_owned());
        Ok(self)
    }

    /// Declare the success terminal write state of a write command (contract 02 §2.3, UI Q11).
    ///
    /// The core marks the request's row in that state with `payload.final = true`. Without a
    /// declaration the core uses `confirmed`. Allowed: `sent`, `confirmed`, `effective_verified`.
    pub fn success_terminal(&mut self, method: &str, state: WriteState) -> Result<&mut Self, RegisterError> {
        self.check_owned(method)?;
        if !matches!(
            state,
            WriteState::Sent | WriteState::Confirmed | WriteState::EffectiveVerified
        ) {
            return Err(RegisterError::Invalid {
                what: "success_terminal",
                reason: format!("{method}: success terminal must be sent, confirmed or effective_verified"),
            });
        }
        self.success_terminals.insert(method.to_owned(), state);
        Ok(self)
    }

    /// The worker `kind` is a separate executable `name` (found next to `loomd`, e.g.
    /// `loom-eth-worker` for least-privilege capture). Default: `loomd worker <kind>`.
    pub fn worker_program(&mut self, kind: &str, name: &str) -> Result<&mut Self, RegisterError> {
        let ok = !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
            && !name.starts_with('.');
        if !ok {
            return Err(RegisterError::Invalid {
                what: "worker_program",
                reason: format!("{name:?}: a plain file name ([A-Za-z0-9._-], no path)"),
            });
        }
        self.worker_programs.insert(kind.to_owned(), name.to_owned());
        Ok(self)
    }

    /// Ask the SDK to call the worker's tick every `period` (10 ms – 60 s; psu polling …).
    pub fn worker_tick(&mut self, kind: &str, period: Duration) -> Result<&mut Self, RegisterError> {
        if !(MIN_WORKER_TICK..=MAX_WORKER_TICK).contains(&period) {
            return Err(RegisterError::Invalid {
                what: "worker_tick",
                reason: format!("{kind}: period must be 10 ms – 60 s"),
            });
        }
        self.worker_ticks.insert(kind.to_owned(), period);
        Ok(self)
    }

    /// Who publishes `<domain>.input` events (default [`Owner::Core`]).
    pub fn input_events(&mut self, owner: Owner) -> Result<&mut Self, RegisterError> {
        self.input_events = owner;
        Ok(self)
    }

    /// Add a safe-state recovery action.
    pub fn safe_state(&mut self, s: SafeStateSpec) -> Result<&mut Self, RegisterError> {
        self.check_two_token(&s.name)?;
        validate_method(&s.method).map_err(|e| invalid_name(&s.method, e.to_string()))?;
        self.check_owned(&s.method)?;
        self.safe_states.push(s);
        Ok(self)
    }

    /// Add an emergency-off action.
    pub fn emergency_off(&mut self, s: EmergencyOffSpec) -> Result<&mut Self, RegisterError> {
        self.check_two_token(&s.name)?;
        validate_method(&s.method).map_err(|e| invalid_name(&s.method, e.to_string()))?;
        self.check_owned(&s.method)?;
        self.emergency_offs.push(s);
        Ok(self)
    }

    /// Add an interlock (`<domain>.<name>`).
    pub fn interlock(&mut self, name: &str, active: Interlock) -> Result<&mut Self, RegisterError> {
        self.check_two_token(name)?;
        if self.interlocks.iter().any(|(n, _)| n == name) {
            return Err(RegisterError::Duplicate {
                what: "interlock",
                name: name.to_owned(),
            });
        }
        self.interlocks.push((name.to_owned(), active));
        Ok(self)
    }

    /// Add a UI panel manifest (UI team format, passed through).
    pub fn panel_manifest(&mut self, manifest: Value) -> Result<&mut Self, RegisterError> {
        if !manifest.is_object() {
            return Err(RegisterError::Invalid {
                what: "panel manifest",
                reason: "must be an object".into(),
            });
        }
        self.panels.push(manifest);
        Ok(self)
    }

    /// Schema of the domain's `loom.toml` section (once).
    pub fn policy_section(&mut self, schema: Schema) -> Result<&mut Self, RegisterError> {
        if self.policy_section.is_some() {
            return Err(RegisterError::Duplicate {
                what: "policy section",
                name: self.domain.clone(),
            });
        }
        self.policy_section = Some(schema);
        Ok(self)
    }

    /// Domain help (once).
    pub fn help(&mut self, help: HelpSpec) -> Result<&mut Self, RegisterError> {
        if self.help.is_some() {
            return Err(RegisterError::Duplicate {
                what: "help",
                name: self.domain.clone(),
            });
        }
        self.help = Some(help);
        Ok(self)
    }

    /// Domain name.
    pub fn domain(&self) -> &str {
        &self.domain
    }
    /// Declared success terminal of a write command (`None` = core default `confirmed`).
    pub fn success_terminal_of(&self, method: &str) -> Option<WriteState> {
        self.success_terminals.get(method).copied()
    }

    /// Separate executable of worker `kind`, if declared.
    pub fn worker_program_of(&self, kind: &str) -> Option<&str> {
        self.worker_programs.get(kind).map(String::as_str)
    }

    /// Tick period of worker `kind`, if declared.
    pub fn worker_tick_of(&self, kind: &str) -> Option<Duration> {
        self.worker_ticks.get(kind).copied()
    }

    /// Who publishes `<domain>.input`.
    pub fn input_events_owner(&self) -> Owner {
        self.input_events
    }

    /// Registered commands.
    pub fn commands(&self) -> &[CommandSpec] {
        &self.commands
    }
    /// Event kinds.
    pub fn event_kinds(&self) -> &[EventKindSpec] {
        &self.event_kinds
    }
    /// Risk table.
    pub fn risk_table_ref(&self) -> Option<&RiskTable> {
        self.risk_table.as_ref()
    }
    /// Write hooks.
    pub fn hooks_ref(&self) -> Option<&Arc<dyn WriteHooks>> {
        self.hooks.as_ref()
    }
    /// Workers.
    pub fn workers(&self) -> &[WorkerSpec] {
        &self.workers
    }
    /// Runtime choice.
    pub fn runtime_choice(&self) -> ModuleRuntime {
        self.runtime
    }
    /// Reducers (the hub takes ownership with [`Registry::take_reducers`]).
    pub fn reducers(&self) -> &[Box<dyn Reducer>] {
        &self.reducers
    }
    /// Move reducers out (hub thread owns them).
    pub fn take_reducers(&mut self) -> Vec<Box<dyn Reducer>> {
        std::mem::take(&mut self.reducers)
    }
    /// Evidence verifiers.
    pub fn evidence_verifiers(&self) -> &[Arc<dyn EvidenceVerifier>] {
        &self.verifiers
    }
    /// Display summarizer.
    pub fn display_summarizer_ref(&self) -> Option<&Arc<dyn DisplaySummarizer>> {
        self.summarizer.as_ref()
    }
    /// Capabilities.
    pub fn capabilities(&self) -> impl Iterator<Item = &str> {
        self.capabilities.iter().map(String::as_str)
    }
    /// Safe-state actions.
    pub fn safe_states(&self) -> &[SafeStateSpec] {
        &self.safe_states
    }
    /// Emergency-off actions.
    pub fn emergency_offs(&self) -> &[EmergencyOffSpec] {
        &self.emergency_offs
    }
    /// Interlocks.
    pub fn interlocks(&self) -> impl Iterator<Item = (&str, &Interlock)> {
        self.interlocks.iter().map(|(n, f)| (n.as_str(), f))
    }
    /// Panel manifests.
    pub fn panel_manifests(&self) -> &[Value] {
        &self.panels
    }
    /// Policy section schema.
    pub fn policy_section_ref(&self) -> Option<&Schema> {
        self.policy_section.as_ref()
    }
    /// Help.
    pub fn help_ref(&self) -> Option<&HelpSpec> {
        self.help.as_ref()
    }
}
