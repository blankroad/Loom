//! Operator upgrade UDS batch 2 — handlers over the real DoIP client + loopback sim entity
//! (Opus-authored specification). U8 row provenance, U6 disconnect / forget / connect-replace,
//! U1 TesterPresent job (default off, skip while busy, stop on link/STOP/lease/disconnect).
//! Same fake CtxServices pattern as tests/e2e.rs. Contracts: FUNCTIONS.md (B2-*).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft, JournalQuery, JournalReader,
    Registry, ScanResult, WorkerHandle,
};
use loom_proto::error::ErrorCode;
use loom_proto::rpc::{Rights, Role};
use loom_testkit::RegistryProbe;
use loom_uds::UdsModule;
use loom_uds::manager::UdsManager;
use loom_uds::module::ManagedUdsModule;
use loom_uds_sim::{FaultAction, FaultMatch, FaultRule};
use loom_worker_sdk::{Clock, SystemClock};
use serde_json::{Value, json};

const MAX_RECORDED: usize = 100_000;

#[derive(Debug)]
struct Fake {
    clock: Arc<dyn Clock>,
    events: Mutex<Vec<EventDraft>>,
}

#[derive(Debug)]
struct NoJournal;
impl JournalReader for NoJournal {
    fn scan<'a>(
        &'a self,
        _q: &'a JournalQuery,
        _limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async {
            Ok(ScanResult {
                events: Vec::new(),
                complete: true,
                gaps: Vec::new(),
                resume_from: None,
                scanned_through: 0,
                scanned_bytes: 0,
            })
        })
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
        let mut ev = self.events.lock().expect("lock");
        if ev.len() < MAX_RECORDED {
            ev.push(draft);
        }
        Ok(())
    }
    fn now_mono_ns(&self) -> u64 {
        self.clock.mono_ns()
    }
    fn journal(&self) -> &dyn JournalReader {
        &JOURNAL
    }
    fn approvals(&self) -> &dyn ApprovalHandle {
        &APPROVALS
    }
}

struct Env {
    module: ManagedUdsModule,
    reg: Registry,
    fake: Arc<Fake>,
    n: u64,
}

impl Env {
    fn new() -> Self {
        let module = UdsModule::with_manager(Arc::new(UdsManager::new()));
        let (reg, _) = RegistryProbe::assert_ok(&module);
        Env {
            module,
            reg,
            fake: Arc::new(Fake {
                clock: Arc::new(SystemClock::new()),
                events: Mutex::new(Vec::new()),
            }),
            n: 0,
        }
    }

    fn ctx(&mut self, role: Role) -> (CallCtx, String) {
        self.n += 1;
        let rid = format!("r{}", self.n);
        let (client, rights) = match role {
            Role::Viewer => ("conn-7", Some(Rights::Operate)),
            _ => ("claude-1", None),
        };
        let ctx = CallCtx::new(
            role,
            rights,
            client,
            Some(rid.clone()),
            Instant::now() + Duration::from_secs(60),
            self.fake.clone(),
        );
        (ctx, rid)
    }

    fn handler(&self, method: &str) -> Arc<dyn loom_domain_api::Handler> {
        self.reg
            .commands()
            .iter()
            .find(|c| c.method == method)
            .unwrap_or_else(|| panic!("{method} registered"))
            .handler
            .clone()
    }

    async fn call(
        &mut self,
        role: Role,
        method: &str,
        params: Value,
    ) -> (Result<Value, DomainError>, String) {
        let (mut ctx, rid) = self.ctx(role);
        let h = self.handler(method);
        (h.call(&mut ctx, params).await, rid)
    }

    fn spawn(
        &mut self,
        role: Role,
        method: &str,
        params: Value,
    ) -> (String, tokio::task::JoinHandle<Result<Value, DomainError>>) {
        let (mut ctx, rid) = self.ctx(role);
        let h = self.handler(method);
        (rid, tokio::spawn(async move { h.call(&mut ctx, params).await }))
    }

    fn events(&self) -> Vec<EventDraft> {
        self.fake.events.lock().expect("lock").clone()
    }

    /// Last `uds.state` payload published for `target`.
    fn state(&self, target: &str) -> Value {
        self.events()
            .into_iter()
            .rev()
            .find(|e| e.kind == "uds.state" && e.payload["target"] == target)
            .map(|e| e.payload)
            .unwrap_or(Value::Null)
    }

    /// Final row payloads of daemon keep-alive exchanges.
    fn tp_rows(&self) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e.kind == "uds.result" && e.payload["purpose"] == "tester_present")
            .map(|e| e.payload)
            .collect()
    }

    /// Every row event (tx/pending/result) of a request id.
    fn row_events(&self, rid: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e.request_id.as_deref() == Some(rid) && e.kind != "uds.rx")
            .map(|e| e.payload)
            .collect()
    }

    async fn wait_for(&self, what: &str, timeout: Duration, mut f: impl FnMut(&Env) -> bool) {
        let end = Instant::now() + timeout;
        while !f(self) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

fn code(r: &Result<Value, DomainError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

fn ns(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("mono ns string, got {v}"))
}

async fn connected() -> Env {
    let mut env = Env::new();
    let (r, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    r.expect("connect");
    env
}

fn slow_f190(env: &Env, n: u8) {
    env.module
        .manager()
        .target("ecu1")
        .expect("target")
        .with_sim_ecu(|e| {
            e.add_fault(FaultRule::always(
                FaultMatch::sid_id(0x22, 0xF190),
                FaultAction::PendingThen {
                    count: n,
                    every: Duration::from_millis(100),
                },
            ))
        })
        .expect("sim")
        .expect("fault");
}

async fn reconnect_once(env: &Env) {
    assert!(
        env.module
            .manager()
            .target("ecu1")
            .expect("t")
            .sim_close_connections()
    );
    env.wait_for("generation 2 up", Duration::from_secs(5), |e| {
        let s = e.state("ecu1");
        s["generation"] == 2 && s["link"]["state"] == "up"
    })
    .await;
}

// ================================================================== U8 provenance
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u8_rows_carry_the_route_of_their_generation() {
    let mut env = connected().await;
    let read = json!({"target": "ecu1", "dids": ["F190"]});
    let (r, rid1) = env.call(Role::Agent, "uds.read_did", read.clone()).await;
    r.expect("read gen 1");
    let evs = env.row_events(&rid1);
    assert!(!evs.is_empty());
    for p in &evs {
        assert_eq!(
            (p["generation"].clone(), p["backend"].clone(), p["addr"].clone()),
            (json!(1), json!("sim"), json!("DoIP 0x1001")),
            "every row event of {rid1}: {p}"
        );
    }
    reconnect_once(&env).await;
    let (r, rid2) = env.call(Role::Agent, "uds.read_did", read).await;
    r.expect("read gen 2");
    assert!(
        env.row_events(&rid2).iter().all(|p| p["generation"] == 2),
        "new generation stamped"
    );
    assert!(
        env.row_events(&rid1).iter().all(|p| p["generation"] == 1),
        "old rows never rewritten"
    );
    // 0x2E needs a non-default session on the sim (loom-uds-sim ecu.rs write_did; same precondition as tests/e2e.rs).
    let (r, rid_s) = env
        .call(
            Role::Agent,
            "uds.session",
            json!({"target": "ecu1", "session": "extended"}),
        )
        .await;
    r.expect("session extended");
    assert!(
        env.row_events(&rid_s).iter().all(|p| p["generation"] == 2),
        "session row stamped"
    );
    let (r, rid3) = env
        .call(
            Role::Agent,
            "uds.write_did",
            json!({"target": "ecu1", "did": "F1A0", "data_hex": "01"}),
        )
        .await;
    r.expect("write");
    let verify = env.row_events(&format!("{rid3}.verify"));
    assert!(
        !verify.is_empty() && verify.iter().all(|p| p["generation"] == 2),
        "sub-request rows stamped too"
    );
}

// ================================================================== U6 disconnect / forget / replace
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u6_disconnect_closes_without_reconnect_and_refuses_requests() {
    let mut env = connected().await;
    let (r, _) = env
        .call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await;
    let d = r.expect("disconnect");
    assert_eq!(d["state"]["link"]["state"], "closed");
    assert_eq!(d["state"]["link"]["cause"], "disconnected by agent");
    assert_eq!(
        env.state("ecu1")["link"]["state"],
        "closed",
        "published before reply"
    );
    tokio::time::sleep(Duration::from_millis(700)).await; // > 2 reconnect intervals
    assert_eq!(
        env.state("ecu1")["link"]["state"],
        "closed",
        "no auto-reconnect after an explicit disconnect"
    );
    assert_eq!(env.state("ecu1")["generation"], 1);
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.read_did",
            json!({"target": "ecu1", "dids": ["F190"]}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen));
    let (r, _) = env
        .call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await;
    assert_eq!(r.expect("idempotent")["already_closed"], true);
    let (r, _) = env.call(Role::Agent, "uds.disconnect", json!({})).await;
    assert_eq!(code(&r), Some(ErrorCode::Usage), "target is required");
    let (r, _) = env
        .call(Role::Agent, "uds.disconnect", json!({"target": "nope"}))
        .await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u6_disconnect_busy_needs_abort() {
    let mut env = connected().await;
    slow_f190(&env, 20); // ~2 s of 0x78
    let (rid, h) = env.spawn(
        Role::Agent,
        "uds.read_did",
        json!({"target": "ecu1", "dids": ["F190"], "timeout_ms": 10_000}),
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (r, _) = env
        .call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Busy), "nothing changes without abort");
    assert_eq!(env.state("ecu1")["link"]["state"], "up");
    let t = Instant::now();
    let (r, _) = env
        .call(
            Role::Viewer,
            "uds.disconnect",
            json!({"target": "ecu1", "abort": true}),
        )
        .await;
    let d = r.expect("disconnect with abort");
    assert!(t.elapsed() < Duration::from_secs(5), "bounded: {:?}", t.elapsed());
    assert_eq!(d["aborted"]["in_flight"], true);
    assert_eq!(d["state"]["link"]["cause"], "disconnected by user");
    assert_eq!(
        code(&h.await.expect("task")),
        Some(ErrorCode::InterruptedByUser),
        "{rid} aborted, not resent"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u6_connect_replaces_a_closed_target_and_continues_generations() {
    let mut env = connected().await;
    let (r, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    let d = r.expect("existing");
    assert_eq!(
        (d["existing"].clone(), d["reconnecting"].clone()),
        (json!(true), json!(false)),
        "an up target is never replaced"
    );
    env.call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await
        .0
        .expect("disconnect");
    let (r, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    let d = r.expect("replace");
    assert_eq!(d["existing"], false);
    assert_eq!(d["replaced"], true);
    assert_eq!(d["previous_generation"], 1);
    assert_eq!(d["state"]["generation"], 2, "generation numbers are never reused");
    assert_eq!(d["state"]["link"]["state"], "up");
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.read_did",
            json!({"target": "ecu1", "dids": ["F190"]}),
        )
        .await;
    r.expect("read on the new link");
    assert!(env.row_events(&rid).iter().all(|p| p["generation"] == 2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u6_forget_only_closed_targets() {
    let mut env = connected().await;
    let (r, _) = env
        .call(Role::Agent, "uds.forget", json!({"target": "ecu1"}))
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Busy), "disconnect first");
    env.call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await
        .0
        .expect("disconnect");
    let (r, _) = env
        .call(Role::Agent, "uds.forget", json!({"target": "ecu1"}))
        .await;
    let d = r.expect("forget");
    assert_eq!(d["forgotten"], true);
    assert_eq!(d["last_generation"], 1);
    assert_eq!(env.state("ecu1")["forgotten"], true, "final state published");
    let (r, _) = env.call(Role::Agent, "uds.status", json!({})).await;
    assert_eq!(r.expect("status")["targets"], json!([]));
    let (r, _) = env
        .call(Role::Agent, "uds.forget", json!({"target": "ecu1"}))
        .await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen));
    // the name is free: a fresh connect starts over at generation 1
    let (r, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    assert_eq!(r.expect("connect")["state"]["generation"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u6_agent_cannot_disconnect_or_forget_a_user_held_target() {
    let mut env = connected().await;
    env.call(Role::Viewer, "uds.lease.takeover", json!({"target": "ecu1"}))
        .await
        .0
        .expect("takeover");
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.disconnect",
            json!({"target": "ecu1", "abort": true}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::HeldByUser));
    assert_eq!(env.state("ecu1")["link"]["state"], "up");
    let (r, _) = env
        .call(Role::Agent, "uds.forget", json!({"target": "ecu1"}))
        .await;
    assert_eq!(code(&r), Some(ErrorCode::HeldByUser));
    let (r, _) = env
        .call(Role::Viewer, "uds.disconnect", json!({"target": "ecu1"}))
        .await;
    assert_eq!(
        r.expect("user may")["state"]["lease"],
        Value::Null,
        "closing frees the lease"
    );
}

// ================================================================== U1 TesterPresent
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u1_tp_default_off_params_and_periodic_rows() {
    let mut env = connected().await;
    assert_eq!(env.state("ecu1")["tester_present"], Value::Null, "default off");
    for bad in [
        json!({"target": "ecu1", "on": true}),
        json!({"target": "ecu1", "on": true, "interval_ms": 100}),
    ] {
        let (r, _) = env.call(Role::Agent, "uds.tester_present", bad.clone()).await;
        assert_eq!(code(&r), Some(ErrorCode::Usage), "{bad}");
    }
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.tester_present",
            json!({"target": "ecu1", "on": true, "interval_ms": 500}),
        )
        .await;
    let d = r.expect("tp on");
    assert_eq!(d["state"]["tester_present"]["active"], true);
    assert_eq!(d["state"]["tester_present"]["interval_ms"], 500);
    assert_eq!(d["state"]["tester_present"]["suppress"], true);
    assert_eq!(d["state"]["tester_present"]["owner"]["owner_origin"], "agent");
    env.wait_for("2 keep-alive rows", Duration::from_secs(3), |e| {
        e.tp_rows().len() >= 2
    })
    .await;
    for p in env.tp_rows() {
        assert_eq!(p["bytes"]["data"], "3e80");
        assert_eq!(p["risk"], "R0");
        assert_eq!(p["ok_suppressed"], true);
        assert_eq!(p["generation"], 1);
    }
    let ids: Vec<String> = env
        .events()
        .into_iter()
        .filter(|e| e.kind == "uds.result" && e.payload["purpose"] == "tester_present")
        .filter_map(|e| e.request_id)
        .collect();
    assert_eq!(ids.first().map(String::as_str), Some("tp-ecu1-1-1"));
    assert!(
        env.events()
            .iter()
            .filter(|e| e.request_id.as_deref().is_some_and(|r| r.starts_with("tp-")))
            .all(|e| e.kind == "uds.result"),
        "one uds.result per exchange; no tx/pending/rx events for keep-alive"
    );
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.tester_present",
            json!({"target": "ecu1", "on": false}),
        )
        .await;
    let d = r.expect("tp off");
    assert_eq!(d["was_active"], true);
    assert_eq!(d["state"]["tester_present"], Value::Null);
    assert_eq!(d["state"]["tp_stop_cause"], "stopped by agent");
    let n = env.tp_rows().len();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        env.tp_rows().len() <= n + 1,
        "at most the exchange in flight finishes after off"
    );
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.tester_present",
            json!({"target": "ecu1", "on": false}),
        )
        .await;
    assert_eq!(r.expect("off again")["was_active"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u1_tp_never_overlaps_a_real_request() {
    let mut env = connected().await;
    slow_f190(&env, 10); // ~1 s in flight
    env.call(
        Role::Agent,
        "uds.tester_present",
        json!({"target": "ecu1", "on": true, "interval_ms": 500}),
    )
    .await
    .0
    .expect("tp on");
    tokio::time::sleep(Duration::from_millis(80)).await;
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.read_did",
            json!({"target": "ecu1", "dids": ["F190"], "timeout_ms": 5000}),
        )
        .await;
    r.expect("slow read");
    let fin = env.row_events(&rid).into_iter().last().expect("result");
    let (sent, done) = (ns(&fin["t_send_mono_ns"]), ns(&fin["t_final_rx_mono_ns"]));
    tokio::time::sleep(Duration::from_millis(700)).await;
    for p in env.tp_rows() {
        let t = ns(&p["t_send_mono_ns"]);
        assert!(
            !(sent..=done).contains(&t),
            "keep-alive sent at {t} inside the read [{sent}, {done}]"
        );
    }
    assert!(!env.tp_rows().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u1_tp_stops_on_link_loss_and_never_restarts() {
    let mut env = connected().await;
    env.call(
        Role::Agent,
        "uds.tester_present",
        json!({"target": "ecu1", "on": true, "interval_ms": 500}),
    )
    .await
    .0
    .expect("tp on");
    env.wait_for("first keep-alive", Duration::from_secs(2), |e| {
        !e.tp_rows().is_empty()
    })
    .await;
    reconnect_once(&env).await;
    let s = env.state("ecu1");
    assert_eq!(s["tester_present"], Value::Null);
    assert!(
        s["tp_stop_cause"]
            .as_str()
            .is_some_and(|c| c.starts_with("link down")),
        "{s}"
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        env.tp_rows().iter().all(|p| p["generation"] == 1),
        "nothing sent on generation 2"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u1_tp_stops_on_stop_latch_and_release_resumes_nothing() {
    let mut env = connected().await;
    env.call(
        Role::Viewer,
        "uds.tester_present",
        json!({"target": "ecu1", "on": true, "interval_ms": 500}),
    )
    .await
    .0
    .expect("user tp on");
    env.wait_for("first keep-alive", Duration::from_secs(2), |e| {
        !e.tp_rows().is_empty()
    })
    .await;
    env.module.manager().stop_latch();
    assert_eq!(
        env.state("ecu1")["tester_present"],
        Value::Null,
        "STOP stops every keep-alive job, also a user's"
    );
    assert_eq!(env.state("ecu1")["tp_stop_cause"], "STOP");
    let (r, _) = env
        .call(
            Role::Viewer,
            "uds.tester_present",
            json!({"target": "ecu1", "on": true, "interval_ms": 500}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Stopped));
    let n = env.tp_rows().len();
    env.module.manager().release_latch();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(env.tp_rows().len() <= n + 1, "release resumes nothing");
    assert_eq!(env.state("ecu1")["tester_present"], Value::Null);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u1_tp_stops_when_someone_else_takes_the_lease_or_disconnects() {
    let mut env = connected().await;
    env.call(
        Role::Agent,
        "uds.tester_present",
        json!({"target": "ecu1", "on": true, "interval_ms": 500}),
    )
    .await
    .0
    .expect("agent tp on");
    env.call(Role::Viewer, "uds.lease.takeover", json!({"target": "ecu1"}))
        .await
        .0
        .expect("takeover");
    assert_eq!(env.state("ecu1")["tester_present"], Value::Null);
    assert_eq!(env.state("ecu1")["tp_stop_cause"], "lease taken by conn-7");
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.tester_present",
            json!({"target": "ecu1", "on": true, "interval_ms": 500}),
        )
        .await;
    assert_eq!(
        code(&r),
        Some(ErrorCode::HeldByUser),
        "agent cannot start while the user holds the lease"
    );
    env.call(
        Role::Viewer,
        "uds.tester_present",
        json!({"target": "ecu1", "on": true, "interval_ms": 500}),
    )
    .await
    .0
    .expect("user tp on");
    env.call(Role::Viewer, "uds.disconnect", json!({"target": "ecu1"}))
        .await
        .0
        .expect("disconnect");
    let s = env.state("ecu1");
    assert_eq!(s["tester_present"], Value::Null);
    assert_eq!(s["tp_stop_cause"], "target closed");
    let (r, _) = env
        .call(
            Role::Viewer,
            "uds.tester_present",
            json!({"target": "ecu1", "on": true, "interval_ms": 500}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen));
}
