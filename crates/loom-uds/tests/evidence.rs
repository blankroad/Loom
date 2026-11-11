//! `uds.dtc_absent` evidence verifier on records produced by the real module over the sim ECU
//! (sim default DTCs: 923400 status 0x09, C10000 status 0x2F).
//!
//! Drafts become records the way the gateway publishes them: producer `gateway.uds`, caller
//! origin, except `dir: rx` drafts which are `device` (Services::publish).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, Coverage, CoverageNeed, CtxServices, DomainError, EvalBudget,
    EventDraft, JournalQuery, JournalReader, Predicate, Registry, ScanResult, Verification, WorkerHandle,
};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Dir, PublishedRecord, SeqRange};
use loom_proto::num::U64;
use loom_proto::rpc::Role;
use loom_testkit::RegistryProbe;
use loom_uds::UdsModule;
use loom_uds::evidence::PRED_DTC_ABSENT;
use loom_uds::manager::UdsManager;
use serde_json::{Value, json};

#[derive(Debug)]
struct Fake {
    events: Mutex<Vec<EventDraft>>,
}

#[derive(Debug)]
struct NoJournal;
impl JournalReader for NoJournal {
    fn scan<'a>(&'a self, _q: &'a JournalQuery, _l: u64) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async { Ok(ScanResult::default()) })
    }
}

#[derive(Debug)]
struct NoApprovals;
impl ApprovalHandle for NoApprovals {
    fn invalidate(&self, _id: &str, _reason: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

static JOURNAL: NoJournal = NoJournal;
static APPROVALS: NoApprovals = NoApprovals;

impl CtxServices for Fake {
    fn worker(&self, _kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        Err(DomainError::new(ErrorCode::NotFound, "uds has no worker"))
    }
    fn publish(&self, draft: EventDraft) -> Result<(), DomainError> {
        self.events.lock().expect("lock").push(draft);
        Ok(())
    }
    fn now_mono_ns(&self) -> u64 {
        0
    }
    fn journal(&self) -> &dyn JournalReader {
        &JOURNAL
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &APPROVALS
    }
}

struct Env {
    reg: Registry,
    fake: Arc<Fake>,
    n: u64,
}

impl Env {
    fn new() -> Self {
        let module = UdsModule::with_manager(Arc::new(UdsManager::new()));
        let (reg, _) = RegistryProbe::assert_ok(&module);
        Env {
            reg,
            fake: Arc::new(Fake {
                events: Mutex::new(Vec::new()),
            }),
            n: 0,
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let mut ctx = CallCtx::new(
            Role::Agent,
            None,
            "claude-1",
            Some(format!("r{}", self.n)),
            Instant::now() + Duration::from_secs(30),
            self.fake.clone(),
        );
        let h = self
            .reg
            .commands()
            .iter()
            .find(|c| c.method == method)
            .unwrap_or_else(|| panic!("{method} registered"))
            .handler
            .clone();
        h.call(&mut ctx, params)
            .await
            .unwrap_or_else(|e| panic!("{method}: {e:?}"))
    }

    /// Everything published so far, as stream records (seq from 1, epoch 3).
    fn take(&self) -> Vec<PublishedRecord> {
        std::mem::take(&mut *self.fake.events.lock().expect("lock"))
            .into_iter()
            .map(record)
            .collect()
    }
}

static SEQ: Mutex<u64> = Mutex::new(0);

fn record(d: EventDraft) -> PublishedRecord {
    let seq = {
        let mut s = SEQ.lock().expect("seq");
        *s += 1;
        *s
    };
    let origin = if d.dir == Some(Dir::Rx) { "device" } else { "agent" };
    serde_json::from_value(json!({
        "schema_version": 1, "session_id": "s", "domain": "uds", "channel": d.channel,
        "kind": d.kind, "seq": seq.to_string(), "epoch": "3",
        "producer_id": "gateway.uds", "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": {"mono_ns": (seq * 1000).to_string(), "wall_ns": "0", "clock_id": "c", "source": "gateway"},
        "dir": d.dir, "origin": origin, "client_id": "claude-1", "request_id": d.request_id,
        "generation": d.generation, "payload": d.payload,
    }))
    .expect("record")
}

fn gap(seq: u64) -> PublishedRecord {
    serde_json::from_value(json!({
        "schema_version": 1, "session_id": "s", "domain": "record", "kind": "record.gap",
        "seq": seq.to_string(), "epoch": "3", "producer_id": "gateway", "producer_epoch": "1",
        "source_seq": "1", "ts": {"mono_ns": "1", "wall_ns": "0", "clock_id": "c", "source": "gateway"},
        "origin": "device", "payload": {},
    }))
    .expect("gap")
}

fn pred(args: Value) -> Predicate {
    Predicate {
        name: PRED_DTC_ABSENT.into(),
        args,
        expect_id: Some("S4#0".into()),
    }
}

/// TC-1 S4 as the TEST wire passes it.
fn tc1() -> Predicate {
    pred(json!({"mask": 9, "ch": "ecu1",
                "filter": {"domain": "uds", "kinds": ["uds.rx"], "channel": "ecu1", "origins": ["device"]}}))
}

fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: U64(0),
            to: U64(0),
        },
        complete,
        causes: if complete {
            vec![]
        } else {
            vec!["gap_in_window".into()]
        },
    }
}

fn verify(reg: &Registry, p: &Predicate, w: &[PublishedRecord], complete: bool) -> Verification {
    let v = reg
        .evidence_verifiers()
        .iter()
        .find(|v| v.predicates().contains(&PRED_DTC_ABSENT))
        .expect("uds verifier registered")
        .clone();
    v.verify(p, w, &cov(complete), &mut EvalBudget::default())
}

fn reason(v: &Verification) -> String {
    match v {
        Verification::Inconclusive { reason } | Verification::Mismatch { reason, .. } => reason.clone(),
        Verification::Match { .. } => String::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dtc_absent_on_real_sim_records() {
    let mut env = Env::new();
    let v = env.reg.evidence_verifiers();
    assert!(v.iter().any(|v| v.predicates() == [PRED_DTC_ABSENT]));
    assert_eq!(
        v[0].coverage_need(&tc1()),
        CoverageNeed::Complete,
        "absence needs the whole window"
    );
    env.call("uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    let setup = env.take();

    // S4: list with mask 0x09 → 923400 (0x09) and C10000 (0x2F) are reported → Mismatch.
    env.call("uds.dtc.list", json!({"target": "ecu1", "mask": 9}))
        .await;
    let dirty = env.take();
    let r = verify(&env.reg, &tc1(), &dirty, true);
    let Verification::Mismatch { observed, .. } = &r else {
        panic!("dirty list: {r:?}")
    };
    assert_eq!(observed["dtcs"][0]["dtc"], "923400");
    assert_eq!(observed["dtcs"].as_array().map(Vec::len), Some(2));
    // A listed DTC is final even over an incomplete window.
    assert!(matches!(
        verify(&env.reg, &tc1(), &dirty, false),
        Verification::Mismatch { .. }
    ));

    // S5: clear all, then list → Match with the device response as witness.
    env.call("uds.request", json!({"target": "ecu1", "bytes_hex": "14FFFFFF"}))
        .await;
    env.call("uds.dtc.list", json!({"target": "ecu1", "mask": 9}))
        .await;
    let clean = env.take();
    let r = verify(&env.reg, &tc1(), &clean, true);
    let Verification::Match { matched, observed } = &r else {
        panic!("clean list: {r:?}")
    };
    assert_eq!(matched.len(), 1);
    let w = clean
        .iter()
        .find(|e| e.seq == matched[0].seq)
        .expect("witness in window");
    assert_eq!((w.kind.as_str(), w.dir), ("uds.rx", Some(Dir::Rx)));
    assert_eq!(serde_json::to_value(w.origin).expect("origin"), "device");
    assert_eq!(observed["responses"], 1);

    // The list before the clear is superseded within one attempt window.
    let mut both = dirty.clone();
    both.extend(clean.iter().cloned());
    assert!(
        matches!(verify(&env.reg, &tc1(), &both, true), Verification::Match { .. }),
        "clear then list decides"
    );

    // Single-key form, hex-string mask, no origins (default device).
    let nested = pred(
        json!({"uds.dtc_absent": {"mask": "0x09"}, "filter": {"domain": "uds", "kinds": [], "channel": "ecu1"}}),
    );
    assert!(matches!(
        verify(&env.reg, &nested, &clean, true),
        Verification::Match { .. }
    ));

    // Incomplete coverage / a gap record never proves absence.
    let r = verify(&env.reg, &tc1(), &clean, false);
    assert!(matches!(r, Verification::Inconclusive { .. }), "{r:?}");
    let mut gapped = clean.clone();
    gapped.push(gap(1_000_000));
    let r = verify(&env.reg, &tc1(), &gapped, true);
    assert!(reason(&r).contains("record.gap"), "{r:?}");

    // Empty window / window with only the connect state: no response → never a pass.
    assert!(matches!(
        verify(&env.reg, &tc1(), &[], true),
        Verification::Inconclusive { .. }
    ));
    let r = verify(&env.reg, &tc1(), &setup, true);
    assert!(reason(&r).contains("no usable 59 02 response"), "{r:?}");

    // A response without its request row (forged / cut window) is not counted.
    let rx_only: Vec<PublishedRecord> = clean.iter().filter(|e| e.kind == "uds.rx").cloned().collect();
    let r = verify(&env.reg, &tc1(), &rx_only, true);
    assert!(reason(&r).contains("no request row"), "{r:?}");

    // Responses of another origin (old caller-origin publishing) are not device evidence.
    let agent_rx: Vec<PublishedRecord> = clean
        .iter()
        .cloned()
        .map(|mut e| {
            e.origin = loom_proto::event::Origin::Agent;
            e
        })
        .collect();
    let r = verify(&env.reg, &tc1(), &agent_rx, true);
    assert!(reason(&r).contains("another origin"), "{r:?}");

    // Late responses are recorded only, never evidence.
    let late: Vec<PublishedRecord> = clean
        .iter()
        .cloned()
        .map(|mut e| {
            if e.kind == "uds.rx" {
                e.payload["late"] = json!(true);
            }
            e
        })
        .collect();
    assert!(matches!(
        verify(&env.reg, &tc1(), &late, true),
        Verification::Inconclusive { .. }
    ));

    // Request mask must contain the judged bits: list 0x01 does not prove 0x08 absent.
    env.call("uds.dtc.list", json!({"target": "ecu1", "mask": 1}))
        .await;
    let narrow = env.take();
    let r = verify(&env.reg, &tc1(), &narrow, true);
    assert!(reason(&r).contains("does not include 0x09"), "{r:?}");

    // Bytes the row and the rx disagree on are not counted.
    let mut tampered = clean.clone();
    for e in &mut tampered {
        if e.kind == "uds.result" && e.payload["sid"] == 0x19 {
            e.payload["resp_bytes"] = json!({"encoding": "hex", "data": "5902ff923400 09".replace(' ', "")});
        }
    }
    let r = verify(&env.reg, &tc1(), &tampered, true);
    assert!(reason(&r).contains("differ"), "{r:?}");

    // Malformed predicates are errors, not "false".
    for bad in [
        json!({"filter": {"domain": "uds"}}),
        json!({"mask": 0}),
        json!({"mask": 9, "extra": 1}),
        json!({"mask": 9, "filter": {"domain": "uds", "kinds": ["uds.result"]}}),
        json!({"mask": 9, "ch": "a", "filter": {"channel": "b"}}),
    ] {
        let r = verify(&env.reg, &pred(bad.clone()), &clean, true);
        assert!(reason(&r).starts_with("error:"), "{bad} → {r:?}");
    }
}

/// Many large positive `uds.rx` records: the clear / response passes decode each one, so they
/// must check the time budget themselves (the event charge happens only in the first pass).
#[test]
fn later_passes_respect_the_time_budget() {
    let big = format!("62{}", "00".repeat(16_000));
    let w: Vec<PublishedRecord> = (0..300)
        .map(|k| {
            let mut d = EventDraft::new(
                "uds.rx",
                json!({"target": "ecu1", "bytes": {"encoding": "hex", "data": big}, "kind": "positive",
                       "late": false, "t_rx_mono_ns": "1"}),
            );
            d.channel = Some("ecu1".into());
            d.dir = Some(Dir::Rx);
            d.request_id = Some(format!("q{k}"));
            d.generation = Some(1);
            record(d)
        })
        .collect();
    let reg = RegistryProbe::assert_ok(&UdsModule).0;
    let v = reg.evidence_verifiers()[0].clone();
    // Already expired.
    let mut zero = EvalBudget::new(EvalBudget::DEFAULT_EVENTS, Duration::ZERO);
    std::thread::sleep(Duration::from_millis(2));
    let r = v.verify(&tc1(), &w, &cov(true), &mut zero);
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
    // 1 ms: ~5 MB of hex decoding in the later passes runs it out.
    let mut short = EvalBudget::new(EvalBudget::DEFAULT_EVENTS, Duration::from_millis(1));
    let r = v.verify(&tc1(), &w, &cov(true), &mut short);
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
    // Event budget smaller than the window.
    let mut few = EvalBudget::new(10, Duration::from_secs(30));
    let r = v.verify(&tc1(), &w, &cov(true), &mut few);
    assert!(reason(&r).starts_with("eval_budget"), "{r:?}");
}
