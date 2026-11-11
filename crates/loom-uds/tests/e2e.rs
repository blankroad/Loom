//! Handlers wired to runner + DoIP + virtual ECU, driven without a gateway.
//!
//! A fake `CtxServices` records every published event with its monotonic time; the test calls the
//! registered handlers directly and asserts the rows, the event order and publish-before-reply.

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

/// Recorded events are capped (C2) — the test never comes near it.
const MAX_RECORDED: usize = 100_000;

#[derive(Debug, Clone)]
struct Rec {
    at_mono_ns: u64,
    draft: EventDraft,
}

#[derive(Debug)]
struct Fake {
    clock: Arc<dyn Clock>,
    events: Mutex<Vec<Rec>>,
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
            ev.push(Rec {
                at_mono_ns: self.clock.mono_ns(),
                draft,
            });
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
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let module = UdsModule::with_manager(Arc::new(UdsManager::new()));
        let (reg, _) = RegistryProbe::assert_ok(&module);
        Env {
            module,
            reg,
            fake: Arc::new(Fake {
                clock,
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
            Instant::now() + Duration::from_secs(30),
            self.fake.clone(),
        );
        (ctx, rid)
    }

    /// Call a registered command; returns (result, rid, mono time when the handler returned).
    async fn call(
        &mut self,
        role: Role,
        method: &str,
        params: Value,
    ) -> (Result<Value, DomainError>, String, u64) {
        let (mut ctx, rid) = self.ctx(role);
        let handler = self
            .reg
            .commands()
            .iter()
            .find(|c| c.method == method)
            .unwrap_or_else(|| panic!("{method} registered"))
            .handler
            .clone();
        let r = handler.call(&mut ctx, params).await;
        (r, rid, self.fake.clock.mono_ns())
    }

    fn events(&self) -> Vec<Rec> {
        self.fake.events.lock().expect("lock").clone()
    }

    /// (kind, write_state) of the events of one request id, in publish order.
    fn rows(&self, rid: &str) -> Vec<(String, Option<String>)> {
        self.events()
            .into_iter()
            .filter(|r| r.draft.request_id.as_deref() == Some(rid))
            .map(|r| {
                (
                    r.draft.kind.clone(),
                    r.draft.payload["write_state"].as_str().map(str::to_owned),
                )
            })
            .collect()
    }

    fn last_result(&self, rid: &str) -> Rec {
        self.fake
            .events
            .lock()
            .expect("lock")
            .iter()
            .rev()
            .find(|r| r.draft.request_id.as_deref() == Some(rid) && r.draft.kind == "uds.result")
            .cloned()
            .unwrap_or_else(|| panic!("no uds.result for {rid}"))
    }
}

fn kinds(rows: &[(String, Option<String>)]) -> Vec<String> {
    rows.iter()
        .map(|(k, s)| format!("{k}:{}", s.as_deref().unwrap_or("-")))
        .collect()
}

fn err_code(r: &Result<Value, DomainError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_and_user_over_sim_doip() {
    let mut env = Env::new();
    let agent = Role::Agent;

    // connect --sim
    let (r, _, _) = env
        .call(agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    let data = r.expect("connect");
    assert_eq!(data["state"]["generation"], 1);
    assert_eq!(data["state"]["backend"], "sim");
    let target = env.module.manager().target("ecu1").expect("target");

    // read F190: row sequence, decoded VIN, publish-before-reply. The call carries a request id
    // like the gateway write path, which publishes `requested`/`accepted` itself.
    let (r, rid, returned_at) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F190"]}))
        .await;
    let data = r.expect("read F190");
    assert_eq!(data["row"]["decoded"][0]["value"], "LOOMSIM0000000001");
    assert_eq!(
        kinds(&env.rows(&rid)),
        ["uds.tx:sent", "uds.rx:-", "uds.result:confirmed"]
    );
    assert!(
        env.last_result(&rid).at_mono_ns <= returned_at,
        "uds.result published before the reply"
    );

    // session extended (state event), then write F1A0 + read-back → effective_verified
    let (r, _, _) = env
        .call(
            agent,
            "uds.session",
            json!({"target": "ecu1", "session": "extended"}),
        )
        .await;
    r.expect("session");
    assert_eq!(target.state().session.id, 3);
    let (r, rid, _) = env
        .call(
            agent,
            "uds.write_did",
            json!({"target": "ecu1", "did": "F1A0", "data_hex": "02"}),
        )
        .await;
    let data = r.expect("write F1A0");
    assert_eq!(data["row"]["write_state"], "effective_verified");
    assert_eq!(target.sim_did(0xF1A0), Some(vec![0x02]));
    assert_eq!(
        kinds(&env.rows(&format!("{rid}.verify"))),
        [
            "uds.tx:requested",
            "uds.tx:accepted",
            "uds.tx:sent",
            "uds.rx:-",
            "uds.result:confirmed"
        ],
        "read-back is a visible row the domain opens itself"
    );
    assert_eq!(
        env.last_result(&rid).draft.payload["write_state"],
        "effective_verified"
    );

    // NRC
    let (r, rid, _) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F1A1"]}))
        .await;
    assert_eq!(err_code(&r), Some(ErrorCode::Nrc));
    let p = env.last_result(&rid).draft.payload;
    assert_eq!(
        (
            p["write_state"].as_str(),
            p["nrc"].as_u64(),
            p["nrc_name"].as_str()
        ),
        (Some("failed"), Some(0x31), Some("requestOutOfRange"))
    );

    // 0x78 ×2 then positive
    target
        .with_sim_ecu(|e| {
            e.add_fault(FaultRule::times(
                FaultMatch::sid_id(0x22, 0xF190),
                FaultAction::PendingThen {
                    count: 2,
                    every: Duration::from_millis(20),
                },
                1,
            ))
        })
        .expect("sim")
        .expect("fault");
    let (r, rid, _) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F190"]}))
        .await;
    r.expect("read after 0x78");
    let k = kinds(&env.rows(&rid));
    assert_eq!(
        k.iter().filter(|s| s.starts_with("uds.pending")).count(),
        2,
        "{k:?}"
    );
    let sent = k.iter().position(|s| s == "uds.tx:sent").expect("sent");
    let first_rx = k.iter().position(|s| s == "uds.rx:-").expect("rx");
    let result = k
        .iter()
        .position(|s| s.starts_with("uds.result"))
        .expect("result");
    assert!(sent < first_rx && first_rx < result, "{k:?}");
    assert_eq!(env.last_result(&rid).draft.payload["pending_count"], 2);

    // timeout (response 300 ms late, P2 50 ms) → failed + timeout; the late answer shows up on
    // uds.rx with late:true and the timed-out request id while the next request runs.
    target
        .with_sim_ecu(|e| {
            e.add_fault(FaultRule::times(
                FaultMatch::sid_id(0x22, 0xF18C),
                FaultAction::Late {
                    after: Duration::from_millis(300),
                },
                1,
            ))
        })
        .expect("sim")
        .expect("fault");
    let (r, late_rid, _) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F18C"]}))
        .await;
    assert_eq!(err_code(&r), Some(ErrorCode::NoResponse));
    let p = env.last_result(&late_rid).draft.payload;
    assert_eq!(
        (p["write_state"].as_str(), p["timeout"].as_bool()),
        (Some("failed"), Some(true))
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let (r, _, _) = env
        .call(agent, "uds.request", json!({"target": "ecu1", "text": "tp"}))
        .await;
    r.expect("tester present after the late response");
    let late = env
        .events()
        .into_iter()
        .find(|e| e.draft.kind == "uds.rx" && e.draft.payload["late"] == true)
        .expect("late rx event");
    assert_eq!(
        late.draft.request_id.as_deref(),
        Some(late_rid.as_str()),
        "late response attributed to its request"
    );

    // user writes from the console → lease user → agent write refused → release → agent ok
    let (r, _, _) = env
        .call(
            Role::Viewer,
            "uds.request",
            json!({"target": "ecu1", "text": "write F1A0 03"}),
        )
        .await;
    r.expect("user write");
    assert_eq!(target.state().lease.map(|l| l.client_id), Some("conn-7".into()));
    let (r, _, _) = env
        .call(
            agent,
            "uds.write_did",
            json!({"target": "ecu1", "did": "F1A0", "data_hex": "04"}),
        )
        .await;
    assert_eq!(err_code(&r), Some(ErrorCode::HeldByUser));
    let detail = r.as_ref().err().map(|e| e.detail.clone()).unwrap_or_default();
    assert_eq!(
        detail["others_since_last"]["count"], 1,
        "the refused agent call already reports the user write"
    );
    assert_eq!(
        target.sim_did(0xF1A0),
        Some(vec![0x03]),
        "refused write was not sent"
    );
    let (r, _, _) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F1A0"]}))
        .await;
    let data = r.expect("agent read still allowed (R0)");
    assert_eq!(
        data["others_since_last"]["count"], 0,
        "cursor advanced by the previous call"
    );
    assert_eq!(
        data["row"]["decoded"][0]["value"], "03",
        "agent reads the value the user wrote"
    );
    let (r, _, _) = env
        .call(Role::Viewer, "uds.lease.release", json!({"target": "ecu1"}))
        .await;
    r.expect("release");
    let (r, _, _) = env
        .call(
            agent,
            "uds.write_did",
            json!({"target": "ecu1", "did": "F1A0", "data_hex": "04"}),
        )
        .await;
    r.expect("agent write after release");

    // link loss → reconnect with generation + 1 (no resend), then requests work again
    assert!(target.sim_close_connections());
    let deadline = Instant::now() + Duration::from_secs(5);
    while target.state().generation < 2 || target.state().link.state != loom_uds::payload::LinkState::Up {
        assert!(
            Instant::now() < deadline,
            "reconnect within 5 s: {:?}",
            target.state().link
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(target.state().link.cause.as_deref(), Some("reconnected"));
    let (r, rid, _) = env
        .call(agent, "uds.read_did", json!({"target": "ecu1", "dids": ["F190"]}))
        .await;
    r.expect("read after reconnect");
    assert_eq!(env.last_result(&rid).draft.generation, Some(2));
    let states: Vec<u64> = env
        .events()
        .iter()
        .filter(|e| e.draft.kind == "uds.state")
        .filter_map(|e| e.draft.payload["generation"].as_u64())
        .collect();
    assert!(
        states.contains(&2),
        "state event for generation 2 published: {states:?}"
    );

    // blocked service never reaches the ECU
    let (r, rid, _) = env
        .call(
            agent,
            "uds.request",
            json!({"target": "ecu1", "bytes_hex": "2A0101"}),
        )
        .await;
    assert_eq!(err_code(&r), Some(ErrorCode::Unsupported));
    assert_eq!(kinds(&env.rows(&rid)), ["uds.result:failed"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presend_sees_user_lease() {
    let mut env = Env::new();
    let (r, _, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    r.expect("connect");
    let (r, _, _) = env
        .call(Role::Viewer, "uds.lease.takeover", json!({"target": "ecu1"}))
        .await;
    r.expect("takeover");
    let hooks = loom_testkit::HookHarness::new(env.reg.hooks_ref().expect("hooks").clone());
    let mut w = loom_testkit::OwnedWriteRequest::new("uds.write_did", "ecu1")
        .args(json!({"target": "ecu1", "did": "F1A0", "data_hex": "02"}));
    w.target.generation = Some(1);
    let t = hooks.presend_check(&w, None);
    assert!(
        matches!(t.value, Err(loom_domain_api::PresendError::Locked(_))),
        "{:?}",
        t.value
    );
    assert!(!t.slow, "presend ≤ 1 ms");
    w.target.generation = Some(9);
    assert!(matches!(
        hooks.presend_check(&w, None).value,
        Err(loom_domain_api::PresendError::EpochChanged(_))
    ));
    let read = loom_testkit::OwnedWriteRequest::new("uds.read_did", "ecu1")
        .args(json!({"target": "ecu1", "dids": ["F190"]}));
    assert!(
        hooks.presend_check(&read, None).value.is_ok(),
        "reads are not blocked by the user lease"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn response_to_result_publish_latency() {
    let mut env = Env::new();
    let (r, _, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    r.expect("connect");
    let mut lat_us: Vec<u64> = Vec::with_capacity(1000);
    let started = Instant::now();
    for _ in 0..1000 {
        let (r, rid, _) = env
            .call(
                Role::Agent,
                "uds.read_did",
                json!({"target": "ecu1", "dids": ["F190"]}),
            )
            .await;
        r.expect("read");
        let rec = env.last_result(&rid);
        let final_rx: u64 = rec.draft.payload["t_final_rx_mono_ns"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .expect("t_final_rx_mono_ns");
        lat_us.push(rec.at_mono_ns.saturating_sub(final_rx) / 1000);
    }
    let total = started.elapsed();
    lat_us.sort_unstable();
    let pct = |p: usize| lat_us[(lat_us.len() * p / 100).min(lat_us.len() - 1)];
    println!(
        "uds response→uds.result publish over loopback DoIP sim (n=1000, {total:?}): p50 {} µs, p95 {} µs, p99 {} µs, max {} µs",
        pct(50),
        pct(95),
        pct(99),
        lat_us[lat_us.len() - 1]
    );
    assert!(pct(95) < 20_000, "p95 response→publish under 20 ms");
}

impl Env {
    /// Start a call concurrently; returns its request id and a join handle.
    fn spawn(
        &mut self,
        role: Role,
        method: &str,
        params: Value,
    ) -> (String, tokio::task::JoinHandle<Result<Value, DomainError>>) {
        let (mut ctx, rid) = self.ctx(role);
        let handler = self
            .reg
            .commands()
            .iter()
            .find(|c| c.method == method)
            .unwrap_or_else(|| panic!("{method} registered"))
            .handler
            .clone();
        (
            rid,
            tokio::spawn(async move { handler.call(&mut ctx, params).await }),
        )
    }

    /// Publish order of `uds.result` events (request ids).
    fn result_order(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter(|e| e.draft.kind == "uds.result")
            .filter_map(|e| e.draft.request_id)
            .collect()
    }
}

/// Make every `22 F190` slow: `n` × 0x78 every 100 ms before the answer.
fn slow_f190(env: &Env, n: u8) {
    let t = env.module.manager().target("ecu1").expect("target");
    t.with_sim_ecu(|e| {
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

async fn connected() -> Env {
    let mut env = Env::new();
    let (r, _, _) = env
        .call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await;
    r.expect("connect");
    env
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_request_jumps_waiting_agent_requests() {
    let mut env = connected().await;
    slow_f190(&env, 3);
    let read = json!({"target": "ecu1", "dids": ["F190"]});
    let (a0, h0) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(50)).await; // a0 is in flight
    let (a1, h1) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    let (a2, h2) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(30)).await;
    let (u, hu) = env.spawn(
        Role::Viewer,
        "uds.request",
        json!({"target": "ecu1", "text": "read F18C"}),
    );
    for h in [h0, h1, h2, hu] {
        h.await.expect("task").expect("request ok");
    }
    let order: Vec<String> = env
        .result_order()
        .into_iter()
        .filter(|r| [&a0, &a1, &a2, &u].contains(&r))
        .collect();
    assert_eq!(
        order.first(),
        Some(&a0),
        "in-flight request is never pre-empted: {order:?}"
    );
    assert_eq!(
        order.get(1),
        Some(&u),
        "user request runs before waiting agent requests: {order:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_key_stops_in_flight_and_waiting() {
    let mut env = connected().await;
    slow_f190(&env, 20); // ~2 s of 0x78
    let read = json!({"target": "ecu1", "dids": ["F190"], "timeout_ms": 10_000});
    let (a0, h0) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (a1, h1) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(30)).await;
    let t = Instant::now();
    let (r, _, _) = env
        .call(Role::Viewer, "uds.abort", json!({"target": "ecu1"}))
        .await;
    let returned = t.elapsed();
    let data = r.expect("abort");
    assert!(
        returned < Duration::from_millis(10),
        "abort returns fast: {returned:?}"
    );
    assert_eq!(data["in_flight_aborted"], true);
    assert_eq!(data["cancelled_waiting"], 1);
    for (rid, h) in [(a0, h0), (a1, h1)] {
        let r = h.await.expect("task");
        assert_eq!(err_code(&r), Some(ErrorCode::InterruptedByUser), "{rid}");
        assert_eq!(
            env.last_result(&rid).draft.payload["write_state"],
            "failed",
            "{rid}: read ends failed"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_latch_stops_agents_not_users() {
    let mut env = connected().await;
    slow_f190(&env, 3);
    let read = json!({"target": "ecu1", "dids": ["F190"]});
    let (_a0, h0) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_a1, h1) = env.spawn(Role::Agent, "uds.read_did", read.clone());
    tokio::time::sleep(Duration::from_millis(30)).await;
    let summary = env.module.manager().stop_latch();
    assert_eq!(summary["targets"][0]["cancelled_waiting"], 1);
    for h in [h0, h1] {
        assert_eq!(err_code(&h.await.expect("task")), Some(ErrorCode::Stopped));
    }
    // The aborted in-flight 0x22 read is quarantined until its late answer arrives; a
    // non-overlapping user request waits for that and then runs.
    let (r, _, _) = env
        .call(
            Role::Viewer,
            "uds.request",
            json!({"target": "ecu1", "text": "tp", "timeout_ms": 8000}),
        )
        .await;
    r.expect("user request still runs during STOP");
}
