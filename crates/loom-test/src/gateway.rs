//! Registration requires a CORE-owned Registry::for_core("test"). This crate does not create it.
use crate::*;
use loom_domain_api::*;
use loom_proto::{event::Risk, policy::WriteClass};
use schemars::json_schema;
use std::{sync::Arc, time::Duration};
pub const EVENT_KINDS: &[&str] = &[
    "test.session_started",
    "test.session_state",
    "test.session_ended",
    "test.step_started",
    "test.step_ended",
    "test.evidence_checked",
    "test.verdict",
    "test.note",
    "test.ask",
    "test.answer",
];
pub const METHODS: &[&str] = &[
    "test.start",
    "test.status",
    "test.end",
    "test.step",
    "test.step.begin",
    "test.step.end",
    "test.verdict",
    "test.evidence.add",
    "test.check",
    "test.note",
    "test.ask",
    "test.answer",
    "test.pause",
    "test.resume",
    "test.skip",
    "test.retry",
    "test.plan.show",
    "test.plan.amend",
    "test.wait",
    "test.import",
    "test.report",
    "test.freeze",
];
#[derive(Debug)]
struct HandlerOf {
    module: TestModule,
    method: &'static str,
}
impl Handler for HandlerOf {
    fn call<'a>(
        &'a self,
        ctx: &'a mut CallCtx,
        params: serde_json::Value,
    ) -> BoxFuture<'a, Result<serde_json::Value, DomainError>> {
        Box::pin(async move { self.module.call(ctx, self.method, params).await })
    }
}
#[derive(Debug)]
struct TestHooks {
    module: TestModule,
}
impl WriteHooks for TestHooks {
    fn classify(&self, _: &WriteRequest<'_>) -> Classification {
        Classification {
            risk: Risk::R0,
            reason: "TEST session metadata; device actions retain their domain policy".into(),
            block: None,
        }
    }
    fn presend_check(&self, req: &WriteRequest<'_>, _: Option<&Grant>) -> Result<(), PresendError> {
        self.module
            .check_client_write(
                req.client_id,
                match req.origin {
                    loom_proto::event::Origin::User => loom_proto::rpc::Role::Viewer,
                    loom_proto::event::Origin::Script => loom_proto::rpc::Role::Script,
                    _ => loom_proto::rpc::Role::Agent,
                },
            )
            .map_err(|e| PresendError::Locked(e.to_string()))
    }
}
impl TestModule {
    /// CORE grants the reserved namespace by supplying this registry; no domain-name workaround.
    pub fn register_into(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        if reg.domain() != DOMAIN {
            return Err(RegisterError::WrongDomain {
                name: "test.start".into(),
                domain: reg.domain().into(),
            });
        }
        for method in METHODS {
            let roles = if *method == "test.answer" {
                RoleSet::VIEWER_OPERATE
            } else if matches!(
                *method,
                "test.status" | "test.plan.show" | "test.check" | "test.wait" | "test.report"
            ) {
                RoleSet::READ
            } else {
                RoleSet::WRITE
            };
            reg.command(CommandSpec {
                method: (*method).into(),
                write_class: None::<WriteClass>,
                params: json_schema!({"type":"object"}),
                result: json_schema!({"type":"object"}),
                roles,
                handler: Arc::new(HandlerOf {
                    module: self.clone(),
                    method,
                }),
                bounds: Bounds {
                    max_wait: Duration::from_secs(3600),
                    default_rows: 100,
                    max_rows: 1000,
                },
                default_action: None,
            })?;
        }
        for kind in EVENT_KINDS {
            reg.event_kind(EventKindSpec {
                kind: (*kind).into(),
                payload_schema: json_schema!({"type":"object","required":["session_id"]}),
                pointers: vec![
                    PointerSpec {
                        path: "/payload/session_id".into(),
                        ty: ValueType::String,
                        unit: None,
                    },
                    PointerSpec {
                        path: "/payload/revision".into(),
                        ty: ValueType::U64String,
                        unit: None,
                    },
                ],
                version: 1,
            })?;
        }
        reg.hooks(Arc::new(TestHooks { module: self.clone() }))?
            .risk_table(RiskTable {
                rows: METHODS
                    .iter()
                    .map(|m| RiskRow {
                        method: (*m).into(),
                        selector: None,
                        risk: Risk::R0,
                        reason: "session control, not a device write".into(),
                    })
                    .collect(),
                ..Default::default()
            })?
            .reducer(Box::new(TestReducer::default()))?
            .capability("sessions")?
            .capability("verifier:injectable")?
            .help(help())?;
        Ok(())
    }
}
pub fn help() -> HelpSpec {
    HelpSpec{summary:"Evidence-backed TEST sessions on the same domain RPCs and journal as the UI. F1 auto by default; no verifier is pretended.".into(),examples:vec![HelpExample{cmd:"loom test plan check TC-1.plan.yaml".into(),explain:"local structural check; registry classification must be connected before start".into()},HelpExample{cmd:"loom test start TC-1.tc.md --plan TC-1.plan.yaml".into(),explain:"register the TC, plan digest and real risk summary".into()},HelpExample{cmd:"loom test verdict ts-1 S2 pass --evidence uart:dut@seq:12".into(),explain:"copy refs from results; the verifier checks the whole window".into()}],notes:vec!["Agent override is forbidden; viewer approve rights only.".into(),"PASS requires checked evidence. NotWired returns E_NOT_IMPLEMENTED; use inconclusive when verification is unavailable.".into(),"Abort needs an explicit run_finally choice; global STOP never runs cleanup.".into(),"import/report/freeze are explicit P2 E_NOT_IMPLEMENTED.".into()]}
}
