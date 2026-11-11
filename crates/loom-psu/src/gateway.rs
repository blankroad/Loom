//! Gateway piece: what `loomd` loads at start (compile-time registration, C3).

pub mod common;
pub mod evidence;
pub mod handlers;
pub mod hooks;

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::*;
use loom_proto::event::Risk;
use loom_proto::policy::WriteClass;
use schemars::{Schema, json_schema};
use serde_json::{Value, json};

use self::common::ChannelBook;
use self::evidence::{PsuEvidence, PsuReducer, PsuSummarizer};
use self::handlers::*;
use self::hooks::{InterlockView, PsuHooks, no_interlocks};
use crate::model::*;
use crate::{DOMAIN, KIND_MEAS, KIND_STATE, KIND_WRITE, MAX_CHANNELS, WORKER_KIND};

/// Default command wait.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(30);
/// Worker memory (plan §0.4 C2: PSU 16 MiB).
pub const WORKER_MEM_SOFT: u64 = 16 << 20;
/// Worker hard memory.
pub const WORKER_MEM_HARD: u64 = 32 << 20;
/// Worker data-lane credit.
pub const WORKER_CREDIT: u64 = 1 << 20;
/// Rows of list results.
pub const MAX_ROWS: u32 = 1000;

/// The domain module. `interlocks` reads other domains' interlocks (default: none active).
#[derive(Clone)]
pub struct Psu {
    interlocks: InterlockView,
}

impl Default for Psu {
    fn default() -> Self {
        Psu {
            interlocks: no_interlocks(),
        }
    }
}

impl std::fmt::Debug for Psu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Psu").finish_non_exhaustive()
    }
}

impl Psu {
    /// Module reading interlocks through `view` (tests; CORE wiring later).
    pub fn with_interlocks(view: InterlockView) -> Psu {
        Psu { interlocks: view }
    }
}

fn spec(
    method: &str,
    write: bool,
    handler: Arc<dyn Handler>,
    params: Value,
    max_wait: Duration,
) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: write.then_some(WriteClass::Request),
        params: Schema::try_from(params).unwrap_or_else(|_| json_schema!({"type": "object"})),
        result: json_schema!({"type": "object"}),
        roles: if write { RoleSet::WRITE } else { RoleSet::READ },
        handler,
        bounds: Bounds {
            max_wait,
            default_rows: 100,
            max_rows: MAX_ROWS,
        },
        default_action: None, // F1: automatic; the user decides in loom.toml
    }
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn ptr(path: &str, ty: ValueType, unit: Option<&str>) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: unit.map(str::to_owned),
        ty,
    }
}

fn en(v: &[&str]) -> ValueType {
    ValueType::Enum(v.iter().map(|s| (*s).to_owned()).collect())
}

fn event_kinds() -> Vec<EventKindSpec> {
    let kind = |k: &str, required: &[&str], pointers: Vec<PointerSpec>| EventKindSpec {
        kind: k.into(),
        payload_schema: json_schema!({"type": "object", "required": required}),
        pointers,
        version: 1,
    };
    vec![
        kind(
            KIND_STATE,
            &["ch", "set_v", "set_i", "out", "prot", "mode"],
            vec![
                ptr("/payload/set_v", ValueType::F64, Some("V")),
                ptr("/payload/set_i", ValueType::F64, Some("A")),
                ptr("/payload/out", ValueType::Bool, None),
                ptr("/payload/prot", en(PROT_VALUES), None),
                ptr("/payload/mode", en(MODE_VALUES), None),
                ptr("/payload/vmax", ValueType::F64, Some("V")),
                ptr("/payload/imax", ValueType::F64, Some("A")),
                ptr("/payload/cycling", ValueType::Bool, None),
            ],
        ),
        kind(
            KIND_MEAS,
            &["ch", "age_ms"],
            // The worker always emits metrics in this order: voltage, current, power.
            vec![
                ptr("/payload/age_ms", ValueType::U32, Some("ms")),
                ptr("/metrics/0/value", ValueType::F64, Some("V")),
                ptr("/metrics/1/value", ValueType::F64, Some("A")),
                ptr("/metrics/2/value", ValueType::F64, Some("W")),
            ],
        ),
        kind(
            KIND_WRITE,
            &["ch", "write_state"],
            vec![
                ptr(
                    "/payload/write_state",
                    en(&[
                        "sent",
                        "confirmed",
                        "effective_verified",
                        "effect_unverified",
                        "failed",
                        "outcome_unknown",
                    ]),
                    None,
                ),
                ptr("/payload/action", en(&["set", "out", "cycle"]), None),
                ptr("/payload/state", en(&["on", "off"]), None),
            ],
        ),
    ]
}

fn risk_table() -> RiskTable {
    let row = |m: &str, sel: Option<&str>, risk: Risk, reason: &str| RiskRow {
        method: m.into(),
        selector: sel.map(str::to_owned),
        risk,
        reason: reason.into(),
    };
    RiskTable {
        rows: vec![
            row("psu.connect", None, Risk::R0, "opens the supply; audited"),
            row(
                "psu.set",
                None,
                Risk::R2,
                "within vmax/imax; over the profile limit is refused (exit 4), never clamped",
            ),
            row("psu.out", Some("on"), Risk::R3, "output ON (master D6)"),
            row(
                "psu.out",
                Some("off"),
                Risk::R3,
                "output OFF; interlocks refuse agent/script, warn the user",
            ),
            row(
                "psu.cycle",
                None,
                Risk::R3,
                "power cycle; interlocks as output OFF",
            ),
        ],
        ..RiskTable::default()
    }
}

/// UI panel manifest (`psu.gauge`, same shape as the UI panel; filters are declarative JSON).
pub fn panel_manifest() -> Value {
    json!({
        "id": "psu.gauge", "domain": DOMAIN, "title": "벤치 전원 (PSU)", "version": 1, "milestone": "M1b",
        "topics": [{"key": "psu", "filter": {"domains": [DOMAIN], "channelParam": "channel", "defaultChannel": "bench"},
                    "encoding": "json", "snapshot": "required", "mirrorRows": 1000}],
        "queries": ["psu.status", "psu.meas"],
        "actions": [
            {"id": "emergencyOff", "rpc": "psu.out", "class": "request", "right": "watch", "confirm": "none",
             "safetyDirection": "reduce", "label": "비상 OFF"},
            {"id": "output", "rpc": "psu.out", "class": "request", "right": "operate", "confirm": "none", "label": "출력 토글"},
            {"id": "set", "rpc": "psu.set", "class": "request", "right": "operate", "confirm": "none", "label": "설정"}
        ],
        "memory": {"maxBytes": 2 << 20},
        "render": {"maxHz": 10, "budgetMs": 4, "lagTargetMs": 100},
        "shellSlots": ["channel_bar"]
    })
}

/// Domain help (`loom psu help --json` is generated from it).
pub fn help() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "Bench power supply (M1: SimPsu). Set voltage/current, switch the output, measure at 5 Hz, wait \
                  for current/voltage conditions held over time; the user watches the gauge and can hit emergency OFF."
            .into(),
        examples: vec![
            ex("loom psu connect bench", "connect the simulated supply as `bench`"),
            ex("loom psu set bench --volt 13.5 --curr 2", "set (R2); over vmax/imax is refused, never clamped"),
            ex("loom psu out bench on", "output ON (R3)"),
            ex("loom psu wait-for bench --curr-lt 0.005 --hold 5s --timeout 60s", "sleep current held 5 s"),
            ex("loom psu watch bench --for 10s", "min/max/mean over 10 s"),
            ex("loom psu cycle bench --off 2s", "power cycle (R3)"),
        ],
        notes: vec![
            format!("Profile limits (default {DEFAULT_VMAX} V / {DEFAULT_IMAX} A) are refused with exit 4, never clamped; only the user raises them in loom.toml."),
            "Wait with wait-for, never sleep. Exit 3 timeout, 6 the user switched the output off (emergency OFF) — stop and ask.".into(),
            "Emergency OFF and STOP belong to the user: never switch the output back on after one without asking.".into(),
            format!("Interlocks {} refuse agent `out off`/`cycle` (exit 6 E_LOCKED).", RESPECTED_INTERLOCKS.join(", ")),
            "Read-back is the supply's average; inrush needs a scope. Unanswered polls are gaps, never interpolated.".into(),
        ],
    }
}

impl DomainModule for Psu {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let book = ChannelBook::default();
        let hooks = PsuHooks {
            book: book.clone(),
            interlocks: self.interlocks.clone(),
        };
        let ch = json!({"type": "string"});
        let dry = json!({"type": "boolean"});
        let cond = json!({"curr_lt": {"type": "number"}, "curr_gt": {"type": "number"}, "curr_between": {"type": "array"},
                          "volt_lt": {"type": "number"}, "volt_gt": {"type": "number"}, "volt_between": {"type": "array"}});
        let mut wait_props = cond;
        if let Some(o) = wait_props.as_object_mut() {
            o.insert("ch".into(), ch.clone());
            o.insert("hold_ms".into(), json!({"type": "integer"}));
            o.insert("max_gap_ms".into(), json!({"type": "integer"}));
            o.insert("timeout_ms".into(), json!({"type": "integer"}));
            o.insert("since".into(), json!({"type": "string"}));
        }
        let commands = vec![
            spec(
                "psu.connect",
                true,
                Arc::new(Connect),
                obj(
                    json!({"ch": ch, "backend": {"enum": ["sim", "visa"]}, "vmax": {"type": "number"},
                           "imax": {"type": "number"}, "seed": {"type": "integer"}, "load": {"type": "object"}}),
                    &[],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.status",
                false,
                Arc::new(Forward("psu.status", book.clone())),
                obj(json!({"ch": ch}), &[]),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.meas",
                false,
                Arc::new(Forward("psu.meas", book.clone())),
                obj(json!({"ch": ch}), &[]),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.watch",
                false,
                Arc::new(Watch(book.clone())),
                obj(
                    json!({"ch": ch, "for_ms": {"type": "integer"}, "raw": {"type": "boolean"},
                           "max": {"type": "integer"}, "since": {"type": "string"}}),
                    &["for_ms"],
                ),
                Duration::from_millis(MAX_WATCH_MS) + DEFAULT_WAIT,
            ),
            spec(
                "psu.wait_for",
                false,
                Arc::new(WaitFor(book.clone())),
                obj(wait_props, &["timeout_ms"]),
                Duration::from_millis(MAX_WAIT_MS),
            ),
            spec(
                "psu.set",
                true,
                Arc::new(Set(hooks.clone())),
                obj(
                    json!({"ch": ch, "volt": {"type": "number"}, "curr": {"type": "number"}, "dry_run": dry}),
                    &[],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.out",
                true,
                Arc::new(Out(hooks.clone())),
                obj(
                    json!({"ch": ch, "state": {"enum": ["on", "off"]}, "dry_run": dry}),
                    &["state"],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.cycle",
                true,
                Arc::new(Cycle(hooks.clone())),
                obj(
                    json!({"ch": ch, "off_ms": {"type": "integer"}, "dry_run": dry}),
                    &["off_ms"],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "psu.sim.inject",
                false,
                Arc::new(Forward("psu.sim.inject", book.clone())),
                obj(
                    json!({"ch": ch, "delay_ms": {"type": "integer"}, "no_response": {"type": "integer"},
                           "clear": {"type": "boolean"}, "load": {"type": "object"}, "ocp": {"type": "boolean"}}),
                    &[],
                ),
                DEFAULT_WAIT,
            ),
        ];
        for c in commands {
            reg.command(c)?;
        }
        for k in event_kinds() {
            reg.event_kind(k)?;
        }
        let cycling = book.clone();
        reg.risk_table(risk_table())?
            .hooks(Arc::new(hooks))?
            .worker(WorkerSpec {
                kind: WORKER_KIND.into(),
                instances: Instances::PerChannel { max: MAX_CHANNELS },
                budgets: Budgets {
                    mem_soft: WORKER_MEM_SOFT,
                    mem_hard: WORKER_MEM_HARD,
                    credit: WORKER_CREDIT,
                },
                reopen_after_crash: Reopen::None,
                arch: Arch::Native,
                platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
            })?
            .reducer(Box::new(PsuReducer::new(book)))?
            .evidence_verifier(Arc::new(PsuEvidence))?
            .display_summarizer(Arc::new(PsuSummarizer))?
            // Same server action as the panel `!` and `psu.out{state:"off"}` (A3: no confirmation).
            .emergency_off(EmergencyOffSpec {
                name: "psu.emergency_off".into(),
                method: "psu.out".into(),
                targets: vec!["*".into()],
                description: "Switch every supply output OFF (no confirmation; interlocks only warn)".into(),
            })?
            .safe_state(SafeStateSpec {
                name: "psu.output_off".into(),
                method: "psu.out".into(),
                args: json!({"state": "off"}),
                description: "PSU output OFF".into(),
            })?
            .interlock("psu.cycling", Arc::new(move || cycling.any_cycling()))?
            .capability("sim")?
            .capability("measure:voltage,current,power")?
            .panel_manifest(panel_manifest())?
            .help(help())?;
        Ok(())
    }
}
