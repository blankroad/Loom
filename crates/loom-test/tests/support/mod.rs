use loom_domain_api::*;
use loom_proto::{
    cursor::Cursor,
    error::ErrorCode,
    event::{PublishedRecord, SourceRef},
    num::U64,
    rpc::{Rights, Role},
};
use loom_test::*;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Debug, Default)]
pub struct Services {
    pub published: Mutex<Vec<EventDraft>>,
    pub fail_kind: Mutex<Option<String>>,
    pub journal: Journal,
    pub approvals: Approvals,
}
#[derive(Debug, Default)]
pub struct Journal {
    pub scans: AtomicUsize,
}
impl JournalReader for Journal {
    fn scan<'a>(&'a self, _: &'a JournalQuery, _: u64) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            self.scans.fetch_add(1, Ordering::Relaxed);
            Ok(ScanResult {
                events: Vec::new(),
                complete: true,
                gaps: Vec::new(),
                resume_from: None,
                scanned_bytes: 0,
                // This deliberately empty journal has examined no stream row.
                scanned_through: 0,
            })
        })
    }
}
#[derive(Debug, Default)]
pub struct Approvals {
    pub invalidated: Mutex<Vec<String>>,
}
impl ApprovalHandle for Approvals {
    fn invalidate(&self, id: &str, _: &str) -> Result<(), DomainError> {
        self.invalidated.lock().expect("lock").push(id.into());
        Ok(())
    }
}
impl CtxServices for Services {
    fn worker(&self, _: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Err(DomainError::not_implemented("no device path in TEST"))
    }
    fn publish(&self, d: EventDraft) -> Result<(), DomainError> {
        let mut fail = self.fail_kind.lock().expect("lock");
        if fail.as_deref() == Some(d.kind.as_str()) {
            *fail = None;
            return Err(DomainError::new(
                ErrorCode::RecordingFailed,
                "test publication failure",
            ));
        }
        self.published.lock().expect("lock").push(d);
        Ok(())
    }
    fn journal(&self) -> &dyn JournalReader {
        &self.journal
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &self.approvals
    }
}
pub fn context(svc: &Arc<Services>, role: Role, rights: Option<Rights>, client: &str) -> CallCtx {
    CallCtx::new(
        role,
        rights,
        client,
        Some("req".into()),
        Instant::now() + Duration::from_secs(5),
        svc.clone(),
    )
}
#[derive(Debug, Default)]
pub struct Hooks {
    pub health: AtomicU8,
    pub stop: AtomicBool,
    pub approved: AtomicBool,
    pub finally_calls: AtomicUsize,
    pub fences: AtomicUsize,
    pub finally_wired: AtomicBool,
}
impl SessionHooks for Hooks {
    fn fence(&self, _: &str, _: &str, _: u32) -> Result<Option<Fence>, DomainError> {
        let seq = self.fences.fetch_add(1, Ordering::AcqRel) as u64 + 1;
        Ok(Some(Fence {
            stream_epoch: U64(1),
            source_refs: vec![SourceRef {
                producer_id: "device-worker".into(),
                producer_epoch: U64(1),
                source_seq: U64(seq),
            }],
            captured_mono_ns: U64(seq),
        }))
    }
    fn recording_health(&self) -> RecordingHealth {
        match self.health.load(Ordering::Acquire) {
            0 => RecordingHealth::Healthy,
            1 => RecordingHealth::Failed,
            _ => RecordingHealth::Unknown,
        }
    }
    fn stop_latched(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
    fn approval(&self, _: &str, _: U64, _: Option<&str>) -> Result<ApprovalStatus, DomainError> {
        if self.approved.load(Ordering::Acquire) {
            Ok(ApprovalStatus::Granted {
                approval_id: "real-test-grant".into(),
            })
        } else {
            Ok(ApprovalStatus::Pending)
        }
    }
    fn finally<'a>(
        &'a self,
        _: &'a mut CallCtx,
        _: &'a str,
        _: Vec<loom_test_format::Step>,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            if !self.finally_wired.load(Ordering::Acquire) {
                return Err(DomainError::not_implemented("finally dispatcher"));
            }
            self.finally_calls.fetch_add(1, Ordering::AcqRel);
            Ok(json!({"complete":true}))
        })
    }
}
#[derive(Debug, Default)]
pub struct Commands;
impl loom_test_format::RiskProvider for Commands {
    fn classify(&self, m: &str, a: &Value) -> loom_test_format::Risk {
        use loom_test_format::{Risk, RiskLevel};
        match m {
            "psu.out" if a["state"] == "off" => Risk::new(RiskLevel::R1, true),
            "psu.out" | "psu.cycle" => Risk::new(RiskLevel::R3, false),
            "uds.dtc.clear" => Risk::new(RiskLevel::R2, false),
            "uart.send" => Risk::new(RiskLevel::R1, false),
            _ => Risk::new(RiskLevel::R0, false),
        }
    }
}
impl loom_test_format::CommandTable for Commands {
    fn lookup(&self, m: &str) -> Option<loom_test_format::CommandSpec> {
        matches!(
            m,
            "psu.out"
                | "psu.cycle"
                | "uart.expect"
                | "uart.send"
                | "can.sig.wait-for"
                | "uds.dtc.list"
                | "uds.dtc.clear"
        )
        .then(|| loom_test_format::CommandSpec {
            waiting: m == "uart.expect" || m == "can.sig.wait-for",
        })
    }
}
/// Deliberately small test seam; it scans the provided journal and returns synthetic facts.
/// No production code depends on this or the in-progress predicate crate.
#[derive(Debug, Default)]
pub struct Verifier {
    pub mode: AtomicU8,
    pub delay_ms: AtomicUsize,
    pub calls: AtomicUsize,
}
impl VerifierEngine for Verifier {
    fn check(&self, step: &StepSpec, c: &[Citation], ctx: &mut CheckCtx) -> CheckOutcome {
        self.calls.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(Duration::from_millis(self.delay_ms.load(Ordering::Acquire) as u64));
        let query = JournalQuery {
            from: Cursor::Seq {
                epoch: Some(1),
                seq: 1,
            },
            until: None,
            filter: Default::default(),
            request_id: None,
            max_events: 1,
        };
        let _ = tokio::runtime::Handle::current().block_on(ctx.journal().scan(&query, 1024));
        let _ = ctx.budget.spend(c.len());
        let mode = self.mode.load(Ordering::Acquire);
        let mut checks = Vec::new();
        for (i, citation) in c.iter().enumerate() {
            let r = citation
                .reference
                .clone()
                .unwrap_or_else(|| "uart:dut@e1:seq:1".into());
            let own = r.contains("seq:99");
            checks.push(EvidenceCheck {
                evidence_id: format!("e{i}"),
                expectation: citation.expectation,
                reference: r.clone(),
                status: if mode == 1 {
                    EvidenceStatus::Uncertain
                } else if mode == 2 {
                    EvidenceStatus::Mismatch
                } else {
                    EvidenceStatus::Verified
                },
                observed: json!("checked"),
                cause: (mode == 1).then(|| "source.gap".into()),
                complete: mode != 1,
                gap_in_window: mode == 1,
                digest: Some(loom_test_format::sha256(r.as_bytes())),
                matched_refs: vec![r],
                witness: if own { Witness::OwnTxOnly } else { Witness::Device },
            });
        }
        CheckOutcome {
            checks,
            all_expectations_verified: mode == 0,
            stimulus_confirmed: mode == 0,
            error: None,
            verified_iterations: if mode == 4 {
                0
            } else {
                step.template.as_ref().map_or(1, |t| t.repeat_count)
            },
        }
    }
    fn decide(&self, _: &StepSpec, p: Verdict, o: &CheckOutcome, _: &mut CheckCtx) -> Decision {
        match self.mode.load(Ordering::Acquire) {
            1 if p == Verdict::Pass => {
                let mut d = Decision::accept(Verdict::Inconclusive);
                d.error = Some(DomainError::new(ErrorCode::Partial, "gap"));
                d
            }
            2 if p == Verdict::Pass => Decision::reject(ErrorCode::EvidenceMismatch, "mismatch"),
            3 if p != Verdict::Pass => Decision::accept(Verdict::Pass),
            _ => {
                let mut d = Decision::accept(p);
                d.unsupported = p == Verdict::Fail && o.checks.is_empty();
                d
            }
        }
    }
}
pub const TC: &str = "---\nid: TC\ntitle: Test\nenv: sim\n---\n## 단계\n| # | 동작 | 기대 결과 | 합격 기준 |\n|---|---|---|---|\n| 1 | Observe | login | bounded |\n| 2 | Observe again | login | bounded |\n";
pub const PLAN: &str = "loom_tc_plan: 1\nenv: sim\nsteps:\n - id: S1\n   tc_step: 1\n   uart.expect: {port: dut, timeout: 1s}\n   retry: {count: 1}\n   expect:\n    - filter: {domain: uart, channel: dut, kinds: [uart.data]}\n      pred: {op: exists}\n - id: S2\n   tc_step: 2\n   uart.expect: {port: dut, timeout: 1s}\n   expect:\n    - filter: {domain: uart, channel: dut, kinds: [uart.data]}\n      pred: {op: exists}\n";
pub struct Harness {
    pub module: TestModule,
    pub services: Arc<Services>,
    pub hooks: Arc<Hooks>,
    pub verifier: Arc<Verifier>,
}
impl Harness {
    pub fn new() -> Self {
        Self::config(SessionConfig::default(), true)
    }
    pub fn config(config: SessionConfig, wired: bool) -> Self {
        let services = Arc::new(Services::default());
        let hooks = Arc::new(Hooks::default());
        let verifier = Arc::new(Verifier::default());
        let engine: Arc<dyn VerifierEngine> = if wired {
            verifier.clone()
        } else {
            Arc::new(NotWired)
        };
        let module = TestModule::with_services(config, Some(Arc::new(Commands)), engine, hooks.clone())
            .expect("module");
        Self {
            module,
            services,
            hooks,
            verifier,
        }
    }
    pub fn ctx(&self) -> CallCtx {
        context(&self.services, Role::Agent, None, "agent")
    }
    pub fn viewer(&self, rights: Rights) -> CallCtx {
        context(&self.services, Role::Viewer, Some(rights), "viewer")
    }
    pub async fn call(&self, method: &str, p: Value) -> Result<Value, DomainError> {
        self.module.call(&mut self.ctx(), method, p).await
    }
    pub async fn start(&self, plan: &str) -> String {
        self.call("test.start", json!({"tc":{"text":TC},"plan":{"text":plan}}))
            .await
            .expect("start")["session_id"]
            .as_str()
            .expect("id")
            .into()
    }
    pub async fn begin(&self, id: &str, step: &str) {
        self.call("test.step", json!({"session":id,"id":step}))
            .await
            .expect("step");
    }
    pub async fn verdict(&self, id: &str, step: &str, v: &str) -> Result<Value, DomainError> {
        self.call("test.verdict",json!({"session":id,"step":step,"verdict":v,"evidence":["uart:dut@e1:seq:1"],"reason":"observed"})).await
    }
    pub fn events(&self) -> Vec<EventDraft> {
        self.services.published.lock().expect("lock").clone()
    }
}
pub fn published(seq: u64, d: &EventDraft) -> PublishedRecord {
    serde_json::from_value(json!({"schema_version":1,"session_id":"journal-session","domain":"test","kind":d.kind,"seq":seq.to_string(),"epoch":"1","producer_id":"gateway","producer_epoch":"1","source_seq":seq.to_string(),"origin":"device","ts":{"mono_ns":seq.to_string(),"wall_ns":seq.to_string(),"clock_id":"test","source":"synthetic"},"payload":d.payload})).expect("record")
}
