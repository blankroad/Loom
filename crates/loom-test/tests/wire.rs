//! `PredicateEngine` end to end through the session module: TC-1 S1–S4 with a fake journal and
//! fake domain verifiers (UART `text_contains`, UDS `dtc_absent`), fences from a test clock.
//! Checks re-read the journal; agent REFs are witness candidates only.

#![allow(clippy::unwrap_used)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::*;
use loom_proto::cursor::Cursor;
use loom_proto::error::ErrorCode;
use loom_proto::event::{PublishedRecord, SourceRef};
use loom_proto::num::U64;
use loom_proto::rpc::Role;
use loom_test::*;
use serde_json::{Value, json};

const TC1: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md");
const PLAN1: &str = include_str!("../../loom-test-format/tests/fixtures/TC-1.plan.yaml");

// --- fakes ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Journal {
    events: Mutex<Vec<PublishedRecord>>,
    incomplete: AtomicBool,
}

impl JournalReader for Journal {
    fn scan<'a>(
        &'a self,
        q: &'a JournalQuery,
        byte_limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            let from = match &q.from {
                Cursor::Seq { seq, .. } => *seq,
                _ => 0,
            };
            let until = match &q.until {
                Some(Cursor::Seq { seq, .. }) => *seq,
                _ => u64::MAX,
            };
            let all = self.events.lock().unwrap();
            let mut out = Vec::new();
            let mut resume_from = None;
            let mut scanned_bytes = 0u64;
            let mut scanned_through = 0;
            for e in all.iter().filter(|e| e.seq.get() >= from && e.seq.get() <= until) {
                let matching = (q.filter.kinds.is_empty() || q.filter.kinds.contains(&e.kind))
                    && (q.filter.domains.is_empty() || q.filter.domains.contains(&e.domain))
                    && (q.filter.channels.is_empty()
                        || e.channel.as_ref().is_some_and(|c| q.filter.channels.contains(c)))
                    && (q.filter.origins.is_empty() || q.filter.origins.contains(&e.origin))
                    && q.request_id
                        .as_ref()
                        .is_none_or(|id| e.request_id.as_ref() == Some(id));
                let size = serde_json::to_vec(e).expect("record JSON").len() as u64;
                // The pending row stays unread when either budget is exhausted.
                if scanned_bytes.saturating_add(size) > byte_limit
                    || (matching && out.len() >= q.max_events as usize)
                {
                    resume_from = Some(Cursor::Seq {
                        epoch: Some(1),
                        seq: e.seq.get(),
                    });
                    break;
                }
                scanned_bytes += size;
                scanned_through = e.seq.get();
                if matching {
                    out.push(e.clone());
                }
            }
            Ok(ScanResult {
                events: out,
                complete: resume_from.is_none() && !self.incomplete.load(Ordering::Acquire),
                gaps: vec![],
                resume_from,
                scanned_bytes,
                scanned_through,
            })
        })
    }
}

#[derive(Default)]
struct Services {
    journal: Journal,
    published: Mutex<Vec<EventDraft>>,
}

struct NoApprovals;
impl ApprovalHandle for NoApprovals {
    fn invalidate(&self, _: &str, _: &str) -> Result<(), DomainError> {
        Ok(())
    }
}
static NO_APPROVALS: NoApprovals = NoApprovals;

impl CtxServices for Services {
    fn worker(&self, _: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Err(DomainError::not_implemented("no device path in TEST"))
    }
    fn publish(&self, d: EventDraft) -> Result<(), DomainError> {
        self.published.lock().unwrap().push(d);
        Ok(())
    }
    fn journal(&self) -> &dyn JournalReader {
        &self.journal
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &NO_APPROVALS
    }
}

/// CORE-like fences: per producer, the newest committed `source_seq` of the fake journal (every
/// pushed record is committed at once), plus a test clock in the records' clock domain `c`.
#[derive(Default)]
struct Hooks {
    clock_ms: AtomicU64,
    journal: Mutex<Option<Arc<Services>>>,
    unflushed: Mutex<Vec<String>>,
}

impl SessionHooks for Hooks {
    fn fence(&self, _: &str, _: &str, _: u32) -> Result<Option<Fence>, DomainError> {
        let t = self.clock_ms.load(Ordering::Acquire);
        let mut refs: Vec<SourceRef> = Vec::new();
        if let Some(svc) = self.journal.lock().unwrap().as_ref() {
            for r in svc.journal.events.lock().unwrap().iter() {
                match refs.iter_mut().find(|x| x.producer_id == r.producer_id) {
                    Some(x) if x.producer_epoch == r.producer_epoch => {
                        x.source_seq = U64(x.source_seq.get().max(r.source_seq.get()));
                    }
                    Some(x) if x.producer_epoch.get() < r.producer_epoch.get() => *x = r.source_ref(),
                    Some(_) => {}
                    None => refs.push(r.source_ref()),
                }
            }
        }
        Ok(Some(Fence {
            stream_epoch: U64(1),
            source_refs: refs,
            captured_mono_ns: U64(t * 1_000_000),
            clock_id: "c".into(),
            unflushed_domains: self.unflushed.lock().unwrap().clone(),
        }))
    }
    fn recording_health(&self) -> RecordingHealth {
        RecordingHealth::Healthy
    }
    fn finally<'a>(
        &'a self,
        _: &'a mut CallCtx,
        _: &'a str,
        _: Vec<loom_test_format::Step>,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async { Ok(json!({"complete": true})) })
    }
}

struct Commands;
impl loom_test_format::RiskProvider for Commands {
    fn classify(&self, m: &str, a: &Value) -> loom_test_format::Risk {
        use loom_test_format::{Risk, RiskLevel};
        match m {
            "psu.out" if a["state"] == "off" => Risk::new(RiskLevel::R1, true),
            "psu.out" => Risk::new(RiskLevel::R3, false),
            "uds.dtc.clear" => Risk::new(RiskLevel::R2, false),
            _ => Risk::new(RiskLevel::R0, false),
        }
    }
}
impl loom_test_format::CommandTable for Commands {
    fn lookup(&self, m: &str) -> Option<loom_test_format::CommandSpec> {
        matches!(
            m,
            "psu.out" | "uart.expect" | "can.sig.wait-for" | "uds.dtc.list" | "uds.dtc.clear"
        )
        .then(|| loom_test_format::CommandSpec {
            waiting: m == "uart.expect" || m == "can.sig.wait-for",
        })
    }
}

fn eref(r: &PublishedRecord) -> EventRef {
    EventRef {
        epoch: r.epoch,
        seq: r.seq,
        source: r.source_ref(),
    }
}

/// `uart.text_contains{pattern}` over `uart.data`.
struct UartFake;
impl EvidenceVerifier for UartFake {
    fn predicates(&self) -> &[&'static str] {
        &["uart.text_contains"]
    }
    fn coverage_need(&self, _: &Predicate) -> CoverageNeed {
        CoverageNeed::Positive
    }
    fn verify(
        &self,
        p: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        _: &mut EvalBudget,
    ) -> Verification {
        let pat = p.args["pattern"].as_str().unwrap_or("\u{0}");
        let hits: Vec<EventRef> = events
            .iter()
            .filter(|e| e.kind == "uart.data" && e.payload["text"].as_str().is_some_and(|t| t.contains(pat)))
            .map(eref)
            .collect();
        if !hits.is_empty() {
            Verification::Match {
                observed: json!({"matches": hits.len()}),
                matched: hits,
            }
        } else if !cov.complete {
            Verification::Inconclusive {
                reason: "coverage".into(),
            }
        } else {
            Verification::Mismatch {
                observed: json!({}),
                reason: "not_found".into(),
            }
        }
    }
}

/// `uds.dtc_absent{mask}`: a complete DTC list with no entries; needs complete coverage.
struct UdsFake;
impl EvidenceVerifier for UdsFake {
    fn predicates(&self) -> &[&'static str] {
        &["uds.dtc_absent"]
    }
    fn coverage_need(&self, _: &Predicate) -> CoverageNeed {
        CoverageNeed::Complete
    }
    fn verify(
        &self,
        _: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        _: &mut EvalBudget,
    ) -> Verification {
        if !cov.complete {
            return Verification::Inconclusive {
                reason: format!("coverage: {}", cov.causes.join("; ")),
            };
        }
        match events
            .iter()
            .find(|e| e.kind == "uds.rx" && e.payload["complete"] == true)
        {
            Some(r) if r.payload["dtcs"].as_array().is_some_and(Vec::is_empty) => Verification::Match {
                matched: vec![],
                observed: json!({"dtcs": 0}),
            },
            Some(r) => Verification::Mismatch {
                observed: json!({"dtcs": r.payload["dtcs"]}),
                reason: "dtc_present".into(),
            },
            None => Verification::Inconclusive {
                reason: "no complete DTC response".into(),
            },
        }
    }
}

fn specs() -> Vec<PointerSpec> {
    let p = |path: &str, ty: ValueType| PointerSpec {
        path: path.into(),
        unit: None,
        ty,
    };
    vec![
        p("/payload/state_out", ValueType::String),
        p("/payload/name", ValueType::String),
        p("/payload/value", ValueType::F64),
        p("/payload/text", ValueType::String),
    ]
}

// --- harness -------------------------------------------------------------------------------------

struct H {
    module: TestModule,
    svc: Arc<Services>,
    hooks: Arc<Hooks>,
}

impl H {
    fn new() -> H {
        let svc = Arc::new(Services::default());
        let hooks = Arc::new(Hooks::default());
        *hooks.journal.lock().unwrap() = Some(svc.clone());
        let engine = PredicateEngine::new(vec![Arc::new(UartFake), Arc::new(UdsFake)], specs());
        let module = TestModule::with_services(
            SessionConfig::default(),
            Some(Arc::new(Commands)),
            Arc::new(engine),
            hooks.clone(),
        )
        .expect("module");
        H { module, svc, hooks }
    }
    fn ctx(&self, role: Role) -> CallCtx {
        CallCtx::new(
            role,
            None,
            "agent",
            Some("rq".into()),
            Instant::now() + Duration::from_secs(10),
            self.svc.clone(),
        )
    }
    async fn call(&self, m: &str, p: Value) -> Result<Value, DomainError> {
        self.module.call(&mut self.ctx(Role::Agent), m, p).await
    }
    fn at(&self, ms: u64) {
        self.hooks.clock_ms.store(ms, Ordering::Release);
    }
    fn push(&self, r: Value) {
        let mut j = self.svc.journal.events.lock().unwrap();
        let seq = j.len() as u64 + 1;
        let mut r = r;
        r["seq"] = json!(seq.to_string());
        r["source_seq"] = json!(seq.to_string());
        j.push(serde_json::from_value(r).expect("record"));
    }
    async fn start(&self) -> String {
        self.call(
            "test.start",
            json!({"tc": {"text": TC1}, "plan": {"text": PLAN1}}),
        )
        .await
        .expect("start")["session_id"]
            .as_str()
            .expect("id")
            .into()
    }
    async fn step(&self, id: &str, step: &str, at_ms: u64) {
        self.at(at_ms);
        self.call("test.step", json!({"session": id, "id": step}))
            .await
            .expect("step");
    }
    async fn verdict(&self, id: &str, step: &str, v: &str, refs: &[&str]) -> Result<Value, DomainError> {
        self.call(
            "test.verdict",
            json!({"session": id, "step": step, "verdict": v, "evidence": refs, "reason": "observed"}),
        )
        .await
    }
    fn checks(&self) -> Vec<Value> {
        self.svc
            .published
            .lock()
            .unwrap()
            .iter()
            .filter(|d| d.kind == "test.evidence_checked")
            .map(|d| d.payload.clone())
            .collect()
    }
}

fn rec(t_ms: u64, domain: &str, channel: &str, kind: &str, origin: &str, payload: Value) -> Value {
    json!({"schema_version": 1, "session_id": "s", "domain": domain, "channel": channel, "kind": kind,
           "seq": "0", "epoch": "1", "producer_id": format!("{domain}-w"), "producer_epoch": "1", "source_seq": "0",
           "ts": {"mono_ns": (t_ms * 1_000_000).to_string(), "wall_ns": (t_ms * 1_000_000).to_string(), "clock_id": "c", "source": "device"},
           "origin": origin, "payload": payload})
}

fn tagged(mut r: Value, step: &str) -> Value {
    r["test_step"] = json!(step);
    r
}

/// S1 (power on, done) and S2 (login), shared by most tests. Returns the session id.
async fn through_s2(h: &H) -> String {
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    )); // seq 1
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("S1 done");
    h.step(&id, "S2", 1_200).await;
    h.push(rec(
        5_000,
        "uart",
        "dut",
        "uart.data",
        "device",
        json!({"text": "bench login: "}),
    )); // seq 2
    h.push(tagged(
        rec(
            5_001,
            "uart",
            "dut",
            "uart.expect_result",
            "agent",
            json!({"matched": true}),
        ),
        "S2",
    )); // seq 3
    h.verdict(&id, "S2", "pass", &["uart:dut@e1:seq:2"])
        .await
        .expect("S2 pass");
    id
}

async fn s3_pass(h: &H, id: &str) {
    h.step(id, "S3", 5_100).await;
    h.push(rec(
        6_000,
        "can",
        "ch0",
        "can.signal",
        "device",
        json!({"name": "VehState.PowerMode", "value": 2}),
    )); // seq 4
    h.verdict(id, "S3", "pass", &["can:ch0@e1:seq:4"])
        .await
        .expect("S3 pass");
}

// --- tests ---------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn tc1_s1_to_s4_passes_and_exits_0() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": []}),
    )); // seq 5
    h.verdict(&id, "S4", "pass", &["uds:bcm@e1:seq:5"])
        .await
        .expect("S4 pass");
    let end = h.call("test.end", json!({"session": id})).await.expect("passed");
    assert_eq!(
        (end["result"].clone(), end["exit_code"].clone()),
        (json!("passed"), json!(0))
    );
    // Every check re-read the journal, carried a digest and says it is not pinned (no CORE pin yet).
    let checks = h.checks();
    assert!(checks.len() >= 4);
    for c in &checks {
        assert!(c["digest"].as_str().is_some_and(|d| d.len() == 64), "{c}");
        assert_eq!(c["observed"]["pinned"], false, "{c}");
    }
    let s3 = checks.iter().find(|c| c["step_id"] == "S3").expect("S3 check");
    assert_eq!(
        (s3["status"].clone(), s3["witness"].clone()),
        (json!("verified"), json!("device"))
    );
    assert_eq!(s3["matched_refs"][0], "can:ch0@e1:seq:4");
    let s4 = checks.iter().find(|c| c["step_id"] == "S4").expect("S4 check");
    assert_eq!(s4["witness"], "complete_absence");
}

#[tokio::test(flavor = "multi_thread")]
async fn invented_seq_is_refused_even_when_real_evidence_exists() {
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    ));
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("done");
    h.step(&id, "S2", 1_200).await;
    // No login line at all: the invented REF cannot make a PASS.
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:77"])
        .await
        .expect_err("no evidence");
    assert_eq!(e.code, ErrorCode::EvidenceMismatch);
    // A real line exists but the agent still cites a seq that is not in the journal.
    h.push(rec(
        5_000,
        "uart",
        "dut",
        "uart.data",
        "device",
        json!({"text": "login:"}),
    ));
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:78"])
        .await
        .expect_err("invented ref");
    assert_eq!(e.code, ErrorCode::EvidenceMismatch);
    assert!(e.message.contains("ref_not_in_journal"), "{}", e.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn evidence_outside_the_within_window_is_out_of_window() {
    let h = H::new();
    let id = through_s2(&h).await;
    h.step(&id, "S3", 5_100).await;
    // PowerMode = 2 only 7 s after the login anchor (window 5 s).
    h.push(rec(
        12_100,
        "can",
        "ch0",
        "can.signal",
        "device",
        json!({"name": "VehState.PowerMode", "value": 2}),
    )); // seq 4
    let e = h
        .verdict(&id, "S3", "pass", &["can:ch0@e1:seq:4"])
        .await
        .expect_err("late");
    assert_eq!(e.code, ErrorCode::OutOfWindow);
    assert_eq!(e.code.exit_code(), 4);
    // The anchor was S2's event, read separately (outside S3's attempt window).
    let c = h
        .checks()
        .into_iter()
        .rev()
        .find(|c| c["step_id"] == "S3")
        .expect("check");
    assert_eq!(c["cause"], "out_of_window");
}

#[tokio::test(flavor = "multi_thread")]
async fn gap_in_window_is_inconclusive_and_exits_8() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_400,
        "source",
        "bcm",
        "source.gap",
        "device",
        json!({"cause": "overflow", "count": "unknown"}),
    ));
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": []}),
    ));
    let e = h
        .verdict(&id, "S4", "pass", &["uds:bcm@e1:seq:6"])
        .await
        .expect_err("gap");
    assert_eq!(e.code, ErrorCode::EvidenceMissing);
    let c = h
        .checks()
        .into_iter()
        .rev()
        .find(|c| c["step_id"] == "S4")
        .expect("check");
    assert_eq!(
        (c["status"].clone(), c["gap_in_window"].clone()),
        (json!("uncertain"), json!(true))
    );
    h.verdict(&id, "S4", "inconclusive", &[])
        .await
        .expect("inconclusive accepted");
    let e = h
        .call("test.end", json!({"session": id}))
        .await
        .expect_err("not passed");
    assert_eq!(e.code.exit_code(), 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn own_transmission_only_is_refused() {
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    ));
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("done");
    h.step(&id, "S2", 1_200).await;
    // "login:" only appears in what the agent itself sent (origin agent).
    h.push(rec(
        5_000,
        "uart",
        "dut",
        "uart.data",
        "agent",
        json!({"text": "echo login:"}),
    )); // seq 2
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:2"])
        .await
        .expect_err("own tx");
    assert_eq!(e.code, ErrorCode::EvidenceMissing);
    let c = h
        .checks()
        .into_iter()
        .rev()
        .find(|c| c["step_id"] == "S2")
        .expect("check");
    assert_eq!(c["witness"], "own_tx_only");
}

#[tokio::test(flavor = "multi_thread")]
async fn scan_limit_is_inconclusive() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": []}),
    )); // seq 5
    h.svc.journal.incomplete.store(true, Ordering::Release);
    let e = h
        .verdict(&id, "S4", "pass", &["uds:bcm@e1:seq:5"])
        .await
        .expect_err("scan incomplete");
    assert_eq!(e.code, ErrorCode::EvidenceMissing);
    let c = h
        .checks()
        .into_iter()
        .rev()
        .find(|c| c["step_id"] == "S4")
        .expect("check");
    assert_eq!(c["status"], "uncertain");
    assert!(
        c["cause"]
            .as_str()
            .is_some_and(|s| s.contains("journal_incomplete")),
        "{c}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dtc_present_fails_recovery_fails_and_exits_11() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": ["C1234"]}),
    )); // seq 5
    // PASS is refused by the re-read; FAIL is supported by the mismatch.
    let e = h
        .verdict(&id, "S4", "pass", &["uds:bcm@e1:seq:5"])
        .await
        .expect_err("dtc present");
    assert_eq!(e.code, ErrorCode::EvidenceMismatch);
    let v = h
        .verdict(&id, "S4", "fail", &["uds:bcm@e1:seq:5"])
        .await
        .expect("fail");
    assert_eq!(v["verdict"]["unsupported"], false);
    // on_fail: goto S5 (clear + relist) — the DTC is still there.
    h.step(&id, "S5", 8_000).await;
    h.push(rec(
        8_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": ["C1234"]}),
    )); // seq 6
    h.verdict(&id, "S5", "fail", &["uds:bcm@e1:seq:6"])
        .await
        .expect("S5 fail");
    let e = h
        .call("test.end", json!({"session": id}))
        .await
        .expect_err("failed");
    assert_eq!(e.code, ErrorCode::TestFailed);
    assert_eq!(e.code.exit_code(), 11);
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_branch_that_passes_replaces_the_failure() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": ["C1234"]}),
    )); // seq 5
    h.verdict(&id, "S4", "fail", &["uds:bcm@e1:seq:5"])
        .await
        .expect("fail");
    h.step(&id, "S5", 8_000).await;
    h.push(rec(
        8_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": []}),
    )); // seq 6
    h.verdict(&id, "S5", "pass", &["uds:bcm@e1:seq:6"])
        .await
        .expect("S5 pass");
    let end = h
        .call("test.end", json!({"session": id}))
        .await
        .expect("recovered");
    assert_eq!(end["exit_code"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_fences_the_window_is_unknown() {
    // CORE R1 owns real fences: the default module never confirms a PASS.
    let svc = Arc::new(Services::default());
    let module = TestModule::with_services(
        SessionConfig::default(),
        Some(Arc::new(Commands)),
        Arc::new(PredicateEngine::new(vec![Arc::new(UartFake)], specs())),
        Arc::new(NotWiredHooks),
    )
    .expect("module");
    let mut ctx = CallCtx::new(
        Role::Agent,
        None,
        "agent",
        None,
        Instant::now() + Duration::from_secs(5),
        svc.clone(),
    );
    let id = module
        .call(
            &mut ctx,
            "test.start",
            json!({"tc": {"text": TC1}, "plan": {"text": PLAN1}}),
        )
        .await
        .expect("start")["session_id"]
        .as_str()
        .expect("id")
        .to_owned();
    module
        .call(&mut ctx, "test.step", json!({"session": id, "id": "S1"}))
        .await
        .expect("S1");
    module
        .call(&mut ctx, "test.step", json!({"session": id, "id": "S2"}))
        .await
        .expect("S2");
    let e = module
        .call(
            &mut ctx,
            "test.verdict",
            json!({"session": id, "step": "S2", "verdict": "pass", "evidence": ["uart:dut@e1:seq:1"]}),
        )
        .await
        .expect_err("no fence");
    assert!(
        matches!(e.code, ErrorCode::NotImplemented | ErrorCode::EvidenceMissing),
        "{e:?}"
    );
}

// --- source-fence counterexamples ----------------------------------------------------------------

fn last_check(h: &H, step: &str) -> Value {
    h.checks()
        .into_iter()
        .rev()
        .find(|c| c["step_id"] == step)
        .expect("check")
}

#[tokio::test(flavor = "multi_thread")]
async fn record_committed_before_the_start_fence_is_out_of_window() {
    // A login line emitted while S1 was running (old attempt/step) cannot pass S2, even though
    // its timestamp lies inside S2's within_ms window: membership is decided by source fences.
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    )); // seq 1
    h.push(rec(
        1_150,
        "uart",
        "dut",
        "uart.data",
        "device",
        json!({"text": "login:"}),
    )); // seq 2, before S2
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("done");
    h.step(&id, "S2", 1_200).await;
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:2"])
        .await
        .expect_err("pre-start line");
    assert_eq!(e.code, ErrorCode::OutOfWindow, "{e:?}");
    assert_eq!(last_check(&h, "S2")["cause"], "out_of_window");
}

#[tokio::test(flavor = "multi_thread")]
async fn late_response_after_the_end_fence_is_out_of_window() {
    let h = H::new();
    let id = through_s2(&h).await;
    h.step(&id, "S3", 5_100).await;
    h.call("test.step.end", json!({"session": id, "step": "S3"}))
        .await
        .expect("S3 ended");
    // PowerMode arrives after S3's end fence, still inside the 5 s anchor window by time.
    h.push(rec(
        6_000,
        "can",
        "ch0",
        "can.signal",
        "device",
        json!({"name": "VehState.PowerMode", "value": 2}),
    )); // seq 4
    let e = h
        .verdict(&id, "S3", "pass", &["can:ch0@e1:seq:4"])
        .await
        .expect_err("late");
    assert_eq!(e.code, ErrorCode::OutOfWindow, "{e:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn old_stream_epoch_ref_is_refused() {
    let h = H::new();
    let id = through_s2(&h).await;
    h.step(&id, "S3", 5_100).await;
    h.push(rec(
        6_000,
        "can",
        "ch0",
        "can.signal",
        "device",
        json!({"name": "VehState.PowerMode", "value": 2}),
    )); // seq 4
    let e = h
        .verdict(&id, "S3", "pass", &["can:ch0@e0:seq:4"])
        .await
        .expect_err("old epoch");
    assert_eq!(e.code, ErrorCode::EvidenceMismatch, "{e:?}");
    assert!(e.message.contains("ref_not_in_journal"), "{}", e.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn done_with_a_write_committed_before_the_attempt_is_refused() {
    let h = H::new();
    let id = h.start().await;
    // A confirmed PSU write that happened before S1 started (e.g. an earlier run/attempt).
    h.push(tagged(
        rec(
            900,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    )); // seq 1
    h.step(&id, "S1", 1_000).await;
    let e = h
        .verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect_err("old write");
    assert_eq!(e.code, ErrorCode::OutOfWindow, "{e:?}");
    assert_eq!(last_check(&h, "S1")["cause"], "out_of_window");
    // The same kind of write inside the attempt confirms the stimulus.
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    )); // seq 2
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:2"])
        .await
        .expect("in-attempt write");
}

#[tokio::test(flavor = "multi_thread")]
async fn unflushed_domain_cannot_pass() {
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    ));
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("done");
    *h.hooks.unflushed.lock().unwrap() = vec!["uart".into()];
    h.step(&id, "S2", 1_200).await;
    h.push(rec(
        5_000,
        "uart",
        "dut",
        "uart.data",
        "device",
        json!({"text": "login:"}),
    )); // seq 2
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:2"])
        .await
        .expect_err("unflushed");
    assert_ne!(e.code, ErrorCode::Usage);
    let c = last_check(&h, "S2");
    assert_eq!(c["status"], "uncertain", "{c}");
    assert!(
        c["cause"].as_str().unwrap().starts_with("fence_unflushed(uart)"),
        "{c}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn evidence_in_another_clock_domain_is_uncertain() {
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.push(tagged(
        rec(
            1_100,
            "psu",
            "bench",
            "psu.write",
            "agent",
            json!({"write_state": "confirmed", "state_out": "on"}),
        ),
        "S1",
    ));
    h.verdict(&id, "S1", "done", &["psu:bench@e1:seq:1"])
        .await
        .expect("done");
    h.step(&id, "S2", 1_200).await;
    let mut r = rec(
        5_000,
        "uart",
        "dut",
        "uart.data",
        "device",
        json!({"text": "login:"}),
    );
    r["ts"]["clock_id"] = json!("proc-other");
    h.push(r); // seq 2
    let e = h
        .verdict(&id, "S2", "pass", &["uart:dut@e1:seq:2"])
        .await
        .expect_err("clock mismatch");
    assert_ne!(e.code, ErrorCode::Usage);
    let c = last_check(&h, "S2");
    assert_eq!(c["status"], "uncertain", "{c}");
    assert!(
        c["cause"].as_str().unwrap().starts_with("clock_domain_mismatch"),
        "{c}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn display_gap_in_window_blocks_complete_absence() {
    let h = H::new();
    let id = through_s2(&h).await;
    s3_pass(&h, &id).await;
    h.step(&id, "S4", 7_000).await;
    h.push(rec(
        7_400,
        "display",
        "",
        "display.gap",
        "device",
        json!({"cause": "hub_overload"}),
    ));
    h.push(rec(
        7_500,
        "uds",
        "bcm",
        "uds.rx",
        "device",
        json!({"complete": true, "dtcs": []}),
    ));
    let e = h
        .verdict(&id, "S4", "pass", &["uds:bcm@e1:seq:6"])
        .await
        .expect_err("display gap");
    assert_eq!(e.code, ErrorCode::EvidenceMissing, "{e:?}");
    assert_eq!(last_check(&h, "S4")["gap_in_window"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn transition_with_an_older_fence_is_refused() {
    // A transition whose (pre-lock) fence is older than the running attempt's start lost a race.
    let h = H::new();
    let id = h.start().await;
    h.step(&id, "S1", 1_000).await;
    h.at(500);
    let e = h
        .call("test.step", json!({"session": id, "id": "S2"}))
        .await
        .expect_err("stale fence");
    assert_eq!(e.code, ErrorCode::Locked, "{e:?}");
    // State unchanged: S1 still running, S2 can begin with a current fence.
    h.step(&id, "S2", 1_200).await;
}
