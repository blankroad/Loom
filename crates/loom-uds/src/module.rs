//! `UdsModule`: what `loomd` registers for the `uds` domain.
//!
//! Handlers are wired to the connection manager ([`crate::manager`]): requests go through the
//! runner over DoIP (also for `connect --sim`) and publish `uds.*` events before replying.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::{
    Bounds, BoxFuture, CallCtx, CommandSpec, DOMAIN_API_VERSION, DomainError, DomainModule, EventKindSpec,
    Handler, HelpExample, HelpSpec, ModuleRuntime, PointerSpec, Reducer, RegisterError, Registry, RoleSet,
    SafeStateSpec, ValueType,
};
use loom_proto::event::PublishedRecord;
use loom_proto::policy::WriteClass;
use loom_proto::rpc::SubFilter;
use loom_uds_codec::nrc_name;
use schemars::{JsonSchema, Schema, schema_for};
use serde_json::{Value, json};

use crate::evidence::UdsEvidence;
use crate::handlers;
use crate::hooks::UdsHooks;
use crate::manager::UdsManager;
use crate::payload::{DryRunResult, RowPayload, RxPayload, StatePayload};
use crate::risk::{RULES, risk_table};
use crate::{DOMAIN, KIND_PENDING, KIND_RESULT, KIND_RX, KIND_STATE, KIND_TX};

/// Parameter schemas (schema only; the gateway validates params against them).
#[allow(dead_code)]
mod params {
    use schemars::JsonSchema;

    /// `uds.connect`.
    #[derive(JsonSchema)]
    pub struct Connect {
        /// Target alias (`ecu1`).
        pub name: String,
        /// Start the virtual ECU behind a loopback DoIP entity and connect to it: `true` or the
        /// profile name `bench`.
        pub sim: Option<serde_json::Value>,
        /// DoIP entity IP (real hardware).
        pub host: Option<String>,
        /// TCP port, default 13400.
        pub port: Option<u16>,
        /// Target logical address (number or hex string `0x1001`), default 0x1001 for sim.
        pub la: Option<serde_json::Value>,
        /// Tester logical address, default 0x0E80.
        pub tester_la: Option<serde_json::Value>,
        /// Routing activation type (numeric), default 0x00.
        pub activation: Option<u8>,
        /// Reconnect after link loss (generation + 1, no resend), default true.
        pub auto_reconnect: Option<bool>,
        pub timeout_ms: Option<u32>,
    }

    /// `uds.lease.takeover` / `uds.lease.release`.
    #[derive(JsonSchema)]
    pub struct Lease {
        pub target: Option<String>,
    }

    /// `uds.status`.
    #[derive(JsonSchema)]
    pub struct Status {
        pub target: Option<String>,
        /// Confirm the session with `22 F186`.
        pub probe: Option<bool>,
    }

    /// `uds.read_did`.
    #[derive(JsonSchema)]
    pub struct ReadDid {
        pub target: Option<String>,
        /// DIDs as hex strings (`F190`, `0xF190`), 1–64.
        pub dids: Vec<String>,
        pub timeout_ms: Option<u32>,
    }

    /// `uds.write_did`.
    #[derive(JsonSchema)]
    pub struct WriteDid {
        pub target: Option<String>,
        pub did: String,
        pub data_hex: String,
        pub timeout_ms: Option<u32>,
    }

    /// `uds.session`.
    #[derive(JsonSchema)]
    pub struct Session {
        pub target: Option<String>,
        /// `default | extended | programming | safety` or hex id.
        pub session: String,
    }

    /// `uds.dtc.list`.
    #[derive(JsonSchema)]
    pub struct DtcList {
        pub target: Option<String>,
        /// Status mask, default 0xFF.
        pub mask: Option<u8>,
    }

    /// `uds.request` (CLI `loom uds raw`, UI console line).
    #[derive(JsonSchema)]
    pub struct Request {
        pub target: Option<String>,
        /// Request text (`22 F1 90`, `read F190`, `write F1A0 02`, `session ext`, `tp`).
        pub text: Option<String>,
        /// Raw request bytes as hex.
        pub bytes_hex: Option<String>,
        pub functional: Option<bool>,
        /// Preview only: no row, event, audit or journal entry.
        pub dry_run: Option<bool>,
        pub timeout_ms: Option<u32>,
    }

    /// `uds.trace`.
    #[derive(JsonSchema)]
    pub struct Trace {
        pub target: Option<String>,
        pub since: Option<String>,
        pub tail: Option<u32>,
    }

    /// `uds.abort`.
    #[derive(JsonSchema)]
    pub struct Abort {
        pub target: Option<String>,
        /// Viewer: `all` (default) or `agent`. Agents and scripts abort only their own origin.
        pub scope: Option<String>,
    }

    /// `uds.help`.
    #[derive(JsonSchema)]
    pub struct Help {}
}

/// The UDS domain module, as `loomd` registers it (`&loom_uds::UdsModule`).
///
/// Each registration creates its own [`UdsManager`] (targets, runners, lease) shared by the
/// handlers and the write hooks of that registry; the hooks' `on_latch` carries CORE STOP to it.
/// Use [`UdsModule::with_manager`] to keep a handle on the manager (tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct UdsModule;

impl UdsModule {
    /// Registration that uses `manager` instead of a fresh one.
    pub fn with_manager(manager: Arc<UdsManager>) -> ManagedUdsModule {
        ManagedUdsModule { manager }
    }
}

/// [`UdsModule`] bound to a caller-owned [`UdsManager`].
#[derive(Debug, Clone)]
pub struct ManagedUdsModule {
    manager: Arc<UdsManager>,
}

impl ManagedUdsModule {
    pub fn manager(&self) -> &Arc<UdsManager> {
        &self.manager
    }
}

/// `uds.help`: help, risk rules and NRC names from the same tables the hooks use.
#[derive(Debug)]
struct HelpHandler;
impl Handler for HelpHandler {
    fn call<'a>(
        &'a self,
        _ctx: &'a mut CallCtx,
        _params: Value,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { Ok(help_json()) })
    }
}

/// The `uds.help` result (also the source for `loom uds help --json`).
pub fn help_json() -> Value {
    let help = help_spec();
    let rules: Vec<Value> = RULES
        .iter()
        .map(|r| {
            json!({
                "sid": format!("{:02X}", r.sid),
                "selector": r.selector.describe(),
                "risk": r.risk,
                "reason": r.reason,
                "method": r.method,
                "block": r.block,
            })
        })
        .collect();
    let nrc: Vec<Value> = (0..=u8::MAX)
        .filter_map(|c| nrc_name(c).map(|n| json!({"code": format!("{c:02X}"), "name": n})))
        .collect();
    json!({
        "domain": DOMAIN,
        "summary": help.summary,
        "examples": help.examples,
        "notes": help.notes,
        "risk_rules": rules,
        "immutable_deny": risk_table().immutable_deny,
        "nrc": nrc,
    })
}

fn bounds(max_wait_s: u64) -> Bounds {
    Bounds {
        max_wait: Duration::from_secs(max_wait_s),
        default_rows: 40,
        max_rows: 1000,
    }
}

fn command(
    method: &str,
    params: Schema,
    write: bool,
    roles: RoleSet,
    max_wait_s: u64,
    handler: Arc<dyn Handler>,
) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: write.then_some(WriteClass::Request),
        params,
        result: schemars::json_schema!({"type": "object"}),
        roles,
        handler,
        bounds: bounds(max_wait_s),
        default_action: None, // F1: automatic; the user's loom.toml decides
    }
}

fn ptr(path: &str, ty: ValueType) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: None,
        ty,
    }
}

fn ptr_ms(path: &str, ty: ValueType) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: Some("ms".into()),
        ty,
    }
}

fn kind<T: JsonSchema>(name: &str, pointers: Vec<PointerSpec>) -> EventKindSpec {
    EventKindSpec {
        kind: name.into(),
        payload_schema: schema_for!(T),
        pointers,
        version: 1,
    }
}

fn write_states() -> ValueType {
    ValueType::Enum(
        [
            "requested",
            "accepted",
            "sent",
            "confirmed",
            "effective_verified",
            "effect_unverified",
            "failed",
            "outcome_unknown",
        ]
        .map(String::from)
        .to_vec(),
    )
}

fn confidences() -> ValueType {
    ValueType::Enum(["confirmed", "assumed", "stale"].map(String::from).to_vec())
}

/// Domain help (C8: `loom uds help --json` is generated from this plus the commands).
pub fn help_spec() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "ECU diagnostics (UDS over DoIP / CAN-TP). Every request goes through loomd and is shown \
                  live in the Loom UI UDS console; the user sees the same rows and can intervene."
            .into(),
        examples: vec![
            ex("loom uds connect --sim bench --name ecu1", "start a virtual DoIP ECU and connect"),
            ex("loom uds read-did F190 -t ecu1", "read the VIN (R0)"),
            ex("loom uds write-did F1A0 --hex 01 -t ecu1", "write a DID (R2, automatic by default)"),
            ex("loom uds session extended -t ecu1", "enter the extended session (R1)"),
            ex("loom uds dtc list --mask 09 -t ecu1", "DTCs with testFailed|confirmed (R0)"),
            ex("loom uds raw '22 F1 90' -t ecu1 --dry-run", "preview bytes, service and risk without sending"),
        ],
        notes: vec![
            "Talk to ECUs only through `loom uds …`; never open UDS/DoIP libraries directly.".into(),
            "Risk class is computed from the bytes sent (R0–R4) and always shown; a DB cannot lower it.".into(),
            "0x2A ReadDataByPeriodicIdentifier and 0x86 ResponseOnEvent are not implemented (immutable_deny).".into(),
            "Never resend a write whose result is outcome_unknown; report it.".into(),
            "If others_since_last.count > 0, read the trace from from_cursor before the next write.".into(),
        ],
    }
}

/// Snapshot provider: last `uds.state` per target, at most [`MAX_SNAPSHOT_TARGETS`].
pub const MAX_SNAPSHOT_TARGETS: usize = 64;

/// When full, the target updated longest ago is evicted.
#[derive(Debug, Default)]
pub struct StateReducer {
    targets: BTreeMap<String, (u64, Value)>,
    counter: u64,
}

impl Reducer for StateReducer {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_STATE]
    }

    fn apply(&mut self, ev: &PublishedRecord) {
        if ev.kind != KIND_STATE {
            return;
        }
        let Some(target) = ev.payload.get("target").and_then(Value::as_str) else {
            return;
        };
        self.counter += 1;
        if !self.targets.contains_key(target)
            && self.targets.len() >= MAX_SNAPSHOT_TARGETS
            && let Some(oldest) = self
                .targets
                .iter()
                .min_by_key(|(_, (n, _))| *n)
                .map(|(k, _)| k.clone())
        {
            self.targets.remove(&oldest);
        }
        self.targets
            .insert(target.to_owned(), (self.counter, ev.payload.clone()));
    }

    fn snapshot(&self, filter: &SubFilter) -> Vec<Value> {
        self.targets
            .iter()
            .filter(|(t, _)| filter.channels.is_empty() || filter.channels.iter().any(|c| c == *t))
            .map(|(_, (_, v))| json!({"kind": KIND_STATE, "payload": v}))
            .collect()
    }
}

impl DomainModule for UdsModule {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        register_with(reg, &Arc::new(UdsManager::new()))
    }
}

impl DomainModule for ManagedUdsModule {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        register_with(reg, &self.manager)
    }
}

fn register_with(reg: &mut Registry, m: &Arc<UdsManager>) -> Result<(), RegisterError> {
    use ValueType as T;
    use params::*;
    let w = RoleSet::WRITE;
    let r = RoleSet::READ;
    let h = |f: fn(Arc<UdsManager>) -> Arc<dyn Handler>| f(m.clone());
    reg.command(command(
        "uds.connect",
        schema_for!(Connect),
        false,
        w,
        30,
        h(handlers::connect),
    ))?
    .command(command(
        "uds.status",
        schema_for!(Status),
        false,
        r,
        10,
        h(handlers::status),
    ))?
    .command(command(
        "uds.read_did",
        schema_for!(ReadDid),
        true,
        w,
        30,
        h(handlers::read_did),
    ))?
    .command(command(
        "uds.write_did",
        schema_for!(WriteDid),
        true,
        w,
        30,
        h(handlers::write_did),
    ))?
    .command(command(
        "uds.session",
        schema_for!(Session),
        true,
        w,
        30,
        h(handlers::session),
    ))?
    .command(command(
        "uds.dtc.list",
        schema_for!(DtcList),
        true,
        w,
        30,
        h(handlers::dtc_list),
    ))?
    .command(command(
        "uds.request",
        schema_for!(Request),
        true,
        w,
        30,
        h(handlers::request),
    ))?
    .command(command(
        "uds.trace",
        schema_for!(Trace),
        false,
        r,
        10,
        h(handlers::trace),
    ))?
    .command(command(
        "uds.abort",
        schema_for!(Abort),
        false,
        w,
        10,
        h(handlers::abort),
    ))?
    .command(command(
        "uds.lease.takeover",
        schema_for!(Lease),
        false,
        RoleSet::VIEWER_OPERATE,
        5,
        h(handlers::lease_takeover),
    ))?
    .command(command(
        "uds.lease.release",
        schema_for!(Lease),
        false,
        RoleSet::VIEWER_OPERATE,
        5,
        h(handlers::lease_release),
    ))?
    .command(command(
        "uds.help",
        schema_for!(Help),
        false,
        r,
        5,
        Arc::new(HelpHandler),
    ))?
    .dry_run_preview("uds.request", RoleSet::READ)?
    .event_kind(kind::<RowPayload>(KIND_TX, row_pointers()))?
    .event_kind(kind::<RowPayload>(KIND_PENDING, row_pointers()))?
    .event_kind(kind::<RowPayload>(KIND_RESULT, row_pointers()))?
    .event_kind(kind::<RxPayload>(
        KIND_RX,
        vec![
            ptr("/payload/bytes/data", T::Hex),
            ptr(
                "/payload/kind",
                T::Enum(
                    ["positive", "negative", "pending", "foreign", "malformed"]
                        .map(String::from)
                        .to_vec(),
                ),
            ),
            ptr("/payload/nrc", T::U8),
            ptr("/payload/late", T::Bool),
            ptr("/payload/t_rx_mono_ns", T::U64String),
        ],
    ))?
    .event_kind(kind::<StatePayload>(
        KIND_STATE,
        vec![
            ptr("/payload/generation", T::U32),
            ptr(
                "/payload/link/state",
                T::Enum(["up", "down", "connecting"].map(String::from).to_vec()),
            ),
            ptr("/payload/session/id", T::U8),
            ptr("/payload/session/confidence", confidences()),
            ptr("/payload/security/level", T::U8),
            ptr_ms("/payload/p2_ms", T::U32),
            ptr_ms("/payload/p2_star_ms", T::U32),
        ],
    ))?
    .risk_table(risk_table())?
    .hooks(Arc::new(UdsHooks::with_manager(m.clone())))?
    .policy_section(schema_for!(crate::policy::PolicySection))?
    .runtime(ModuleRuntime::Dedicated { threads: 1 })?
    .reducer(Box::new(StateReducer::default()))?
    .evidence_verifier(Arc::new(UdsEvidence))?
    .safe_state(SafeStateSpec {
        name: "uds.session_default".into(),
        method: "uds.session".into(),
        args: json!({"session": "default", "recovery": true}),
        description: "Return the ECU to the default session (10 01)".into(),
    })?
    .capability("doip")?
    .capability("sim")?
    .capability("dry_run")?
    .help(help_spec())?;
    Ok(())
}

/// JSON Schema of the dry-run result (for the UI request answer).
pub fn dry_run_schema() -> Schema {
    schema_for!(DryRunResult)
}

/// Fixed pointers shared by the row kinds (`uds.tx`, `uds.pending`, `uds.result`).
fn row_pointers() -> Vec<PointerSpec> {
    use ValueType as T;
    vec![
        ptr("/payload/write_state", write_states()),
        ptr("/payload/bytes/data", T::Hex),
        ptr("/payload/resp_bytes/data", T::Hex),
        ptr("/payload/sid", T::U8),
        ptr("/payload/nrc", T::U8),
        ptr("/payload/timeout", T::Bool),
        ptr("/payload/ok_suppressed", T::Bool),
        ptr("/payload/pending_count", T::U32),
        ptr_ms("/payload/rtt_ms", T::F64),
        ptr("/payload/decoded", T::Array),
        ptr("/payload/dtcs", T::Array),
    ]
}
