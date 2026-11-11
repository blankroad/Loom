//! A fake domain registers every item; invalid names / limits are refused.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use loom_domain_api::*;
use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, Risk, SeqRange};
use loom_proto::policy::{PolicyAction, WriteClass};
use loom_proto::rpc::SubFilter;
use schemars::json_schema;
use serde_json::{Value, json};

#[derive(Debug)]
struct Echo;
impl Handler for Echo {
    fn call<'a>(&'a self, _ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move { Ok(params) })
    }
}

#[derive(Debug)]
struct Hooks;
impl WriteHooks for Hooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        Classification {
            risk: Risk::R1,
            reason: format!("{} write", req.method),
            block: None,
        }
    }
    fn presend_check(&self, _req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Last(Option<Value>);
impl Reducer for Last {
    fn kinds(&self) -> &[&'static str] {
        &["fake.data"]
    }
    fn apply(&mut self, ev: &PublishedRecord) {
        self.0 = Some(ev.payload.clone());
    }
    fn snapshot(&self, _filter: &SubFilter) -> Vec<Value> {
        self.0.iter().cloned().collect()
    }
}

#[derive(Debug)]
struct Verifier;
impl EvidenceVerifier for Verifier {
    fn predicates(&self) -> &[&'static str] {
        &["fake.contains"]
    }
    fn coverage_need(&self, _pred: &Predicate) -> CoverageNeed {
        CoverageNeed::Positive
    }
    fn verify(
        &self,
        _p: &Predicate,
        events: &[PublishedRecord],
        _c: &Coverage,
        b: &mut EvalBudget,
    ) -> Verification {
        match b.spend(events.len()) {
            Ok(()) => Verification::Mismatch {
                observed: json!(events.len()),
                reason: "fake".into(),
            },
            Err(_) => Verification::Inconclusive {
                reason: "eval_budget".into(),
            },
        }
    }
}

#[derive(Debug)]
struct Summ;
impl DisplaySummarizer for Summ {
    fn summarize(&self, dropped: &[PublishedRecord]) -> Value {
        json!({"n": dropped.len()})
    }
}

fn cmd(method: &str) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: Some(WriteClass::Request),
        params: json_schema!({"type": "object"}),
        result: json_schema!({"type": "object"}),
        roles: RoleSet::WRITE,
        handler: Arc::new(Echo),
        bounds: Bounds::DEFAULT,
        default_action: None,
    }
}

fn kind(k: &str) -> EventKindSpec {
    EventKindSpec {
        kind: k.into(),
        payload_schema: json_schema!({"type": "object"}),
        pointers: vec![PointerSpec {
            path: "/payload/state".into(),
            unit: None,
            ty: ValueType::String,
        }],
        version: 1,
    }
}

fn worker(kind: &str) -> WorkerSpec {
    WorkerSpec {
        kind: kind.into(),
        instances: Instances::PerChannel { max: 8 },
        budgets: Budgets {
            mem_soft: 48 << 20,
            mem_hard: 64 << 20,
            credit: 1 << 20,
        },
        reopen_after_crash: Reopen::None,
        arch: Arch::Native,
        platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
    }
}

#[derive(Debug)]
struct Fake;
impl DomainModule for Fake {
    const NAME: &'static str = "fake";
    const API_VERSION: u32 = DOMAIN_API_VERSION;
    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let flashing = Arc::new(AtomicBool::new(false));
        let f = flashing.clone();
        reg.command(cmd("fake.send"))?
            .command(CommandSpec {
                write_class: None,
                roles: RoleSet::READ,
                default_action: None,
                ..cmd("fake.read")
            })?
            .command(CommandSpec {
                default_action: Some(PolicyAction::Approve),
                ..cmd("fake.erase.all")
            })?
            .event_kind(kind("fake.data"))?
            .event_kind(kind("fake.write"))?
            .risk_table(RiskTable {
                rows: vec![RiskRow {
                    method: "fake.send".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "send".into(),
                }],
                default_blocked: vec![],
                immutable_deny: vec!["fake.unimplemented".into()],
            })?
            .hooks(Arc::new(Hooks))?
            .worker(worker("fake"))?
            .runtime(ModuleRuntime::Dedicated { threads: 1 })?
            .reducer(Box::new(Last::default()))?
            .evidence_verifier(Arc::new(Verifier))?
            .display_summarizer(Arc::new(Summ))?
            .capability("loopback")?
            .safe_state(SafeStateSpec {
                name: "fake.idle".into(),
                method: "fake.send".into(),
                args: json!({"data": ""}),
                description: "send nothing".into(),
            })?
            .emergency_off(EmergencyOffSpec {
                name: "fake.off".into(),
                method: "fake.send".into(),
                targets: vec!["dut".into()],
                description: "off".into(),
            })?
            .interlock("fake.flashing", Arc::new(move || f.load(Ordering::Relaxed)))?
            .panel_manifest(json!({"id": "fake.terminal", "type": "console"}))?
            .policy_section(json_schema!({"type": "object"}))?
            .help(HelpSpec {
                summary: "fake domain".into(),
                examples: vec![HelpExample {
                    cmd: "loom fake send x".into(),
                    explain: "send".into(),
                }],
                notes: vec![],
            })?;
        flashing.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn fake_domain_registers_everything() {
    let mut reg = Registry::new(Fake::NAME).unwrap();
    Fake.register(&mut reg).unwrap();
    assert_eq!(reg.domain(), "fake");
    assert_eq!(reg.commands().len(), 3);
    assert_eq!(reg.event_kinds().len(), 2);
    assert!(reg.risk_table_ref().is_some() && reg.hooks_ref().is_some());
    assert_eq!(reg.workers()[0].instances, Instances::PerChannel { max: 8 });
    assert_eq!(reg.runtime_choice(), ModuleRuntime::Dedicated { threads: 1 });
    assert_eq!(reg.reducers().len(), 1);
    assert_eq!(reg.evidence_verifiers().len(), 1);
    assert!(reg.display_summarizer_ref().is_some());
    assert_eq!(reg.capabilities().collect::<Vec<_>>(), ["loopback"]);
    assert_eq!(reg.safe_states().len(), 1);
    assert_eq!(reg.emergency_offs().len(), 1);
    let (name, active) = reg.interlocks().next().unwrap();
    assert_eq!(name, "fake.flashing");
    assert!(active());
    assert_eq!(reg.panel_manifests().len(), 1);
    assert!(reg.policy_section_ref().is_some() && reg.help_ref().is_some());
    assert_eq!(reg.take_reducers().len(), 1);
    assert!(reg.reducers().is_empty());
    assert!(format!("{reg:?}").contains("fake.send"));
}

#[test]
fn bad_names_and_limits_are_refused() {
    assert!(matches!(
        Registry::new("core"),
        Err(RegisterError::InvalidDomain(_))
    ));
    assert!(matches!(
        Registry::new("Fake"),
        Err(RegisterError::InvalidDomain(_))
    ));
    assert!(Registry::for_core("core").is_ok());

    let mut reg = Registry::new("fake").unwrap();
    for m in ["fake", "fake.Send", "fake.a.b.c", "fake..x"] {
        assert!(
            matches!(reg.command(cmd(m)), Err(RegisterError::InvalidName { .. })),
            "{m}"
        );
    }
    assert!(matches!(
        reg.command(cmd("other.send")),
        Err(RegisterError::WrongDomain { .. })
    ));
    reg.command(cmd("fake.send")).unwrap();
    assert!(matches!(
        reg.command(cmd("fake.send")),
        Err(RegisterError::Duplicate { .. })
    ));

    let too_long = CommandSpec {
        bounds: Bounds {
            max_wait: Duration::from_secs(3601),
            default_rows: 1,
            max_rows: 1,
        },
        ..cmd("fake.wait")
    };
    assert!(matches!(
        reg.command(too_long),
        Err(RegisterError::Invalid { .. })
    ));
    let rows = CommandSpec {
        bounds: Bounds {
            max_rows: 1001,
            ..Bounds::DEFAULT
        },
        ..cmd("fake.rows")
    };
    assert!(reg.command(rows).is_err());
    let nobody = CommandSpec {
        roles: RoleSet {
            agent: false,
            script: false,
            viewer: None,
        },
        ..cmd("fake.x")
    };
    assert!(reg.command(nobody).is_err());

    for k in ["fake", "fake.a.b", "Fake.data", "other.data"] {
        assert!(reg.event_kind(kind(k)).is_err(), "{k}");
    }
    let mut bad_ptr = kind("fake.ok");
    bad_ptr.pointers[0].path = "/ts/mono_ns".into();
    assert!(reg.event_kind(bad_ptr).is_err());
    let mut bad_ptr2 = kind("fake.ok");
    bad_ptr2.pointers[0].path = "/payload/a~2".into();
    assert!(reg.event_kind(bad_ptr2).is_err());

    let mut w = worker("fake");
    w.budgets.mem_soft = w.budgets.mem_hard + 1;
    assert!(reg.worker(w).is_err());
    assert!(
        reg.worker(WorkerSpec {
            instances: Instances::PerChannel { max: 0 },
            ..worker("fake")
        })
        .is_err()
    );
    assert!(reg.worker(worker("Bad-kind")).is_err());
    assert!(reg.runtime(ModuleRuntime::Dedicated { threads: 0 }).is_err());
    assert!(reg.interlock("other.flashing", Arc::new(|| false)).is_err());
    assert!(reg.capability("has space").is_err());
    assert!(reg.panel_manifest(json!([1])).is_err());
    reg.hooks(Arc::new(Hooks)).unwrap();
    assert!(reg.hooks(Arc::new(Hooks)).is_err());
}

#[test]
fn errors_and_budget() {
    assert_eq!(DomainError::not_implemented("x").code, ErrorCode::NotImplemented);
    assert_eq!(PresendError::Quarantine("u".into()).code().exit_code(), 6);
    assert_eq!(PresendError::EpochChanged("e".into()).code().exit_code(), 6);
    let mut b = EvalBudget::new(10, Duration::from_secs(5));
    b.spend(6).unwrap();
    assert_eq!(b.events_left(), 4);
    assert_eq!(b.spend(5), Err(BudgetExceeded::Events));
    let v = Verifier.verify(
        &Predicate {
            name: "fake.contains".into(),
            args: json!({}),
            expect_id: None,
        },
        &[],
        &Coverage {
            window: SeqRange {
                from: 0.into(),
                to: 0.into(),
            },
            complete: true,
            causes: vec![],
        },
        &mut EvalBudget::default(),
    );
    assert!(matches!(v, Verification::Mismatch { .. }));
}
