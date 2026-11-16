//! Operator upgrade UDS batch 3 — wiring over the real DoIP client + loopback sim entity
//! (Opus-authored specification). U2 `uds.security_unlock` (provider from `[uds]` config before any
//! byte, seed sub-row, R3 sendKey = the call's own row, zero seed, no retry, STOP) and U5
//! `uds.db.set` / `uds.db.clear` (decode with DB → ISO → raw, `db_hash` row stamp, state `db`,
//! refusals, replace drops the DB). Contracts: FUNCTIONS.md (B3-*).
//!
//! Platform: tests that run a provider PROCESS use the POSIX `sh` fixtures and are `cfg(unix)` (reason at
//! each gate). Every other test here is platform-independent: it configures a provider with an OS-native
//! absolute path that does not exist, so reaching the spawn would fail with E_CONFIG — the expected
//! different outcome proves the provider was never started.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CtxServices, DomainError, EventDraft, JournalQuery, JournalReader,
    Registry, ScanResult, WorkerHandle,
};
use loom_proto::error::ErrorCode;
use loom_proto::event::Risk;
use loom_proto::rpc::{Rights, Role};
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe};
use loom_uds::UdsModule;
use loom_uds::manager::UdsManager;
use loom_uds::module::ManagedUdsModule;
use loom_worker_sdk::{Clock, SystemClock};
use serde_json::{Value, json};

#[derive(Debug)]
struct Fake {
    clock: Arc<dyn Clock>,
    events: Mutex<Vec<EventDraft>>,
    /// The merged `loom.toml [uds]` section the daemon would hand the module.
    config: Mutex<Value>,
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
        self.events.lock().expect("lock").push(draft);
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
    fn config(&self) -> Value {
        self.config.lock().expect("lock").clone()
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
                config: Mutex::new(Value::Null),
            }),
            n: 0,
        }
    }

    fn set_config(&self, v: Value) {
        *self.fake.config.lock().expect("lock") = v;
    }

    async fn call(
        &mut self,
        role: Role,
        method: &str,
        params: Value,
    ) -> (Result<Value, DomainError>, String) {
        self.n += 1;
        let rid = format!("r{}", self.n);
        let (client, rights) = match role {
            Role::Viewer => ("conn-7", Some(Rights::Operate)),
            _ => ("claude-1", None),
        };
        let mut ctx = CallCtx::new(
            role,
            rights,
            client,
            Some(rid.clone()),
            Instant::now() + Duration::from_secs(60),
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
        (h.call(&mut ctx, params).await, rid)
    }

    fn events(&self) -> Vec<EventDraft> {
        self.fake.events.lock().expect("lock").clone()
    }

    fn state(&self, target: &str) -> Value {
        self.events()
            .into_iter()
            .rev()
            .find(|e| e.kind == "uds.state" && e.payload["target"] == target)
            .map(|e| e.payload)
            .unwrap_or(Value::Null)
    }

    /// Every row event (tx/pending/result) of a request id.
    fn row_events(&self, rid: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e.request_id.as_deref() == Some(rid) && e.kind != "uds.rx")
            .map(|e| e.payload)
            .collect()
    }

    /// Request bytes (hex) of every `uds.result` row on any target, in order.
    fn sent_results(&self) -> Vec<String> {
        self.events()
            .into_iter()
            .filter(|e| e.kind == "uds.result")
            .map(|e| e.payload["bytes"]["data"].as_str().unwrap_or("").to_owned())
            .collect()
    }

    fn sim_security(&self) -> Option<u8> {
        self.module
            .manager()
            .target("ecu1")
            .expect("t")
            .with_sim_ecu(|e| e.security_level())
            .expect("sim")
    }
}

fn code(r: &Result<Value, DomainError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

fn err(r: &Result<Value, DomainError>) -> &DomainError {
    r.as_ref()
        .err()
        .unwrap_or_else(|| panic!("expected an error, got {r:?}"))
}

async fn connected() -> Env {
    let mut env = Env::new();
    env.call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await
        .0
        .expect("connect");
    env
}

async fn extended(env: &mut Env) {
    env.call(
        Role::Agent,
        "uds.session",
        json!({"target": "ecu1", "session": "extended"}),
    )
    .await
    .0
    .expect("session extended");
}

/// OS-native absolute path that does not exist: a provider that is never expected to be started.
fn never_run() -> String {
    if cfg!(windows) {
        r"C:\loom-test\never-run-seedkey.exe".into()
    } else {
        "/nonexistent/loom-test/never-run-seedkey".into()
    }
}

fn providers(command: &str, levels: &[u8]) -> Value {
    json!({"key_providers": [{"name": "bench", "command": command, "levels": levels, "timeout_ms": 3000}]})
}

#[cfg(unix)]
fn script(name: &str) -> String {
    format!("{}/tests/fixtures/keyprovider/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn db() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/fixtures/diagdb/bench.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture"),
    )
    .expect("json")
}

// ================================================================== U2 risk / approval binding
#[test]
fn u2_unlock_is_r3_from_the_send_key_bytes_and_binds_the_provider() {
    let (reg, _) = RegistryProbe::assert_ok(&UdsModule);
    let rows: Vec<_> = reg
        .risk_table_ref()
        .expect("table")
        .rows
        .iter()
        .filter(|r| r.method == "uds.security_unlock")
        .collect();
    assert_eq!(rows.len(), 1, "typed risk row");
    assert_eq!(rows[0].risk, Risk::R3);
    let h = HookHarness::new(reg.hooks_ref().expect("hooks").clone());
    let req = |level: Value, provider: &str| {
        OwnedWriteRequest::new("uds.security_unlock", "ecu1")
            .args(json!({"target": "ecu1", "level": level, "provider": provider}))
    };
    let c = h.classify(&req(json!(1), "bench")).value;
    assert_eq!(c.risk, Risk::R3, "{c:?}");
    assert!(c.reason.contains("sendKey"), "{c:?}");
    for bad in [
        json!(2),
        json!(0),
        json!(0x7F),
        json!(0xFF),
        json!("01"),
        Value::Null,
    ] {
        assert_eq!(
            h.classify(&req(bad.clone(), "bench")).value.risk,
            Risk::R4,
            "unencodable level {bad}"
        );
    }
    let a = h.approval_binding(&req(json!(1), "bench")).value;
    let b = h.approval_binding(&req(json!(1), "other")).value;
    let l3 = h.approval_binding(&req(json!(3), "bench")).value;
    assert_eq!(a.digest_fields.get("provider"), Some(&json!("bench")));
    assert_ne!(
        a.digest_fields, b.digest_fields,
        "an approval for one provider never covers another"
    );
    assert_ne!(a.digest_fields, l3.digest_fields, "nor another level");
    assert_eq!(
        a.display["req_hex"], "2702",
        "the approved bytes are the sendKey sub-function"
    );
    assert_eq!(a.display["provider"], "bench");
    let plain = h
        .approval_binding(&OwnedWriteRequest::new("uds.request", "ecu1").bytes(&[0x27, 0x02, 0, 0, 0, 0]))
        .value;
    assert!(
        !plain.digest_fields.contains_key("provider"),
        "only uds.security_unlock binds a provider"
    );
}

// ================================================================== U2 config before any byte
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u2_provider_config_is_checked_before_any_byte() {
    let mut env = connected().await;
    extended(&mut env).await;
    let before = env.sent_results().len();
    let unlock = json!({"target": "ecu1", "level": 1, "provider": "bench"});
    let (r, rid) = env.call(Role::Agent, "uds.security_unlock", unlock.clone()).await;
    assert_eq!(code(&r), Some(ErrorCode::Config), "{r:?}");
    assert_eq!(
        err(&r).message,
        "key provider \"bench\" is not configured in loom.toml [[uds.key_providers]]"
    );
    assert!(
        env.row_events(&rid).is_empty() && env.row_events(&format!("{rid}.seed")).is_empty(),
        "no row, no byte"
    );
    env.set_config(providers(&never_run(), &[3]));
    let (r, _) = env.call(Role::Agent, "uds.security_unlock", unlock.clone()).await;
    assert_eq!(
        err(&r).message,
        "key provider does not allow level 0x01 (allowed: 03)"
    );
    env.set_config(json!({"key_providers": [{"name": "bench", "command": "seedkey", "levels": [1]}]}));
    let (r, _) = env.call(Role::Agent, "uds.security_unlock", unlock.clone()).await;
    assert_eq!(
        code(&r),
        Some(ErrorCode::Config),
        "relative command refused: {r:?}"
    );
    env.set_config(json!({"key_providers": [{"name": "bench", "command": never_run(), "levels": [1], "shell": "/bin/sh"}]}));
    let (r, _) = env.call(Role::Agent, "uds.security_unlock", unlock).await;
    assert_eq!(
        code(&r),
        Some(ErrorCode::Config),
        "unknown config keys refused: {r:?}"
    );
    assert_eq!(
        env.sent_results().len(),
        before,
        "nothing was sent for any of these"
    );
    for (p, what) in [
        (
            json!({"target": "ecu1", "level": 2, "provider": "bench"}),
            "even level",
        ),
        (
            json!({"target": "ecu1", "level": 0x7F, "provider": "bench"}),
            "level above 0x7D",
        ),
        (json!({"target": "ecu1", "level": 1}), "provider missing"),
        (json!({"target": "ecu1", "provider": "bench"}), "level missing"),
    ] {
        assert_eq!(
            code(&env.call(Role::Agent, "uds.security_unlock", p).await.0),
            Some(ErrorCode::Usage),
            "{what}"
        );
    }
    env.set_config(providers(&never_run(), &[1]));
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.security_unlock",
            json!({"target": "nope", "level": 1, "provider": "bench"}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u2_negative_seed_is_returned_and_the_provider_never_starts() {
    let mut env = connected().await; // default session: the sim answers 7F 27 7F
    env.set_config(providers(&never_run(), &[1]));
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.security_unlock",
            json!({"target": "ecu1", "level": 1, "provider": "bench"}),
        )
        .await;
    assert_eq!(
        code(&r),
        Some(ErrorCode::Nrc),
        "the seed's own error, not a provider spawn error: {r:?}"
    );
    assert!(err(&r).message.starts_with("NRC 0x7F"), "{}", err(&r).message);
    let seed = env.row_events(&format!("{rid}.seed"));
    assert!(
        seed.iter()
            .any(|p| p["write_state"] == "failed" && p["bytes"]["data"] == "2701" && p["risk"] == "R1"),
        "{seed:?}"
    );
    assert!(
        seed.iter().all(|p| p["generation"] == 1),
        "seed sub-row stamped like any row"
    );
    assert!(env.row_events(&rid).is_empty(), "no sendKey row");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u2_stop_latched_sends_nothing_and_starts_no_provider() {
    let mut env = connected().await;
    extended(&mut env).await;
    env.set_config(providers(&never_run(), &[1]));
    env.module.manager().stop_latch();
    let before = env.sent_results().iter().filter(|b| b.starts_with("27")).count();
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.security_unlock",
            json!({"target": "ecu1", "level": 1, "provider": "bench"}),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Stopped), "{r:?}");
    assert_eq!(
        env.sent_results()
            .iter()
            .filter(|b| b.starts_with("27") && !b.is_empty())
            .count(),
        before + 1,
        "only the refused seed row"
    );
    assert!(
        env.row_events(&format!("{rid}.seed"))
            .iter()
            .any(|p| p["write_state"] == "failed")
    );
    assert!(env.row_events(&rid).is_empty(), "no sendKey row");
    assert_eq!(env.sim_security(), None);
}

// ================================================================== U2 provider process (POSIX fixtures)
// Platform gate: the providers below are POSIX `sh` scripts (tests/fixtures/keyprovider/*.sh); Windows
// has no `sh` to exec them. Windows process coverage: b3_units `b3_k6_windows_system_executables`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u2_unlock_seed_then_provider_then_send_key() {
    let mut env = connected().await;
    extended(&mut env).await;
    env.set_config(providers(&script("xor.sh"), &[1]));
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.security_unlock",
            json!({"target": "ecu1", "level": 1, "provider": "bench"}),
        )
        .await;
    let d = r.expect("unlock");
    assert_eq!(
        (
            d["unlocked"].clone(),
            d["already_unlocked"].clone(),
            d["level"].clone()
        ),
        (json!(true), json!(false), json!(1))
    );
    assert_eq!(d["seed_request_id"], format!("{rid}.seed"));
    assert_eq!(
        d["request_id"], rid,
        "the approved R3 sendKey is the call's own request"
    );
    assert_eq!(d["row"]["risk"], "R3");
    assert_eq!(d["target"], "ecu1");
    let seed = env.row_events(&format!("{rid}.seed"));
    assert!(
        seed.iter()
            .any(|p| p["write_state"] == "confirmed" && p["bytes"]["data"] == "2701" && p["risk"] == "R1"),
        "{seed:?}"
    );
    let key = env.row_events(&rid);
    let fin = key
        .iter()
        .rev()
        .find(|p| p["write_state"] == "confirmed")
        .unwrap_or_else(|| panic!("key result: {key:?}"));
    let kb = fin["bytes"]["data"].as_str().expect("hex");
    assert!(
        kb.starts_with("2702") && kb.len() == 12,
        "27 02 + 4-byte key: {kb}"
    );
    assert_eq!(fin["resp_bytes"]["data"], "6702");
    assert_eq!(env.sim_security(), Some(1));
    assert_eq!(env.state("ecu1")["security"]["level"], 1);

    // already unlocked: the sim answers an all-zero seed → no provider run, no key
    let n = env.sent_results().len();
    let (r, rid2) = env
        .call(
            Role::Agent,
            "uds.security_unlock",
            json!({"target": "ecu1", "level": 1, "provider": "bench"}),
        )
        .await;
    let d = r.expect("already");
    assert_eq!(
        (d["unlocked"].clone(), d["already_unlocked"].clone()),
        (json!(true), json!(true))
    );
    assert!(d.get("row").is_none_or(Value::is_null), "no key row: {d}");
    assert_eq!(env.sent_results().len(), n + 1, "only the seed");
    assert!(env.row_events(&rid2).is_empty());
}

// Platform gate: POSIX `sh` provider fixtures (see above).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u2_provider_failure_or_wrong_key_is_reported_and_never_retried() {
    let mut env = connected().await;
    extended(&mut env).await;
    let unlock = json!({"target": "ecu1", "level": 1, "provider": "bench"});

    env.set_config(providers(&script("fail.sh"), &[1]));
    let (r, rid) = env.call(Role::Agent, "uds.security_unlock", unlock.clone()).await;
    assert_eq!(code(&r), Some(ErrorCode::Config));
    assert_eq!(err(&r).message, "key provider exited with code 3: no license");
    assert_eq!(err(&r).detail["seed_request_id"], format!("{rid}.seed"));
    assert!(
        env.row_events(&rid).is_empty(),
        "no sendKey after a provider failure"
    );

    env.set_config(providers(&script("junk.sh"), &[1]));
    let (r, rid) = env.call(Role::Agent, "uds.security_unlock", unlock.clone()).await;
    assert_eq!(code(&r), Some(ErrorCode::Config));
    assert!(
        err(&r).message.starts_with("key provider output is not a key"),
        "{}",
        err(&r).message
    );
    assert!(
        !err(&r).message.contains("key=1234"),
        "provider stdout is never echoed"
    );
    assert!(env.row_events(&rid).is_empty());

    env.set_config(providers(&script("wrong.sh"), &[1]));
    let seeds_before = env.sent_results().iter().filter(|b| b.as_str() == "2701").count();
    let (r, rid) = env.call(Role::Agent, "uds.security_unlock", unlock).await;
    assert_eq!(code(&r), Some(ErrorCode::Nrc));
    assert!(
        err(&r).message.starts_with("NRC 0x35"),
        "invalidKey returned as is: {}",
        err(&r).message
    );
    let keys: Vec<_> = env
        .sent_results()
        .into_iter()
        .filter(|b| b.starts_with("2702"))
        .collect();
    assert_eq!(
        keys,
        vec!["270200000000".to_string()],
        "exactly one key attempt in the whole test"
    );
    assert_eq!(
        env.sent_results().iter().filter(|b| b.as_str() == "2701").count(),
        seeds_before + 1,
        "no second seed"
    );
    assert!(
        env.row_events(&rid)
            .iter()
            .any(|p| p["write_state"] == "failed" && p["nrc"] == 0x35)
    );
    assert_eq!(env.sim_security(), None);
}

// ================================================================== U5 diagnostic DB
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u5_db_set_decodes_rows_and_stamps_the_hash() {
    let mut env = connected().await;
    let (r, _) = env
        .call(Role::Agent, "uds.db.set", json!({"target": "ecu1", "db": db()}))
        .await;
    let d = r.expect("db set");
    let hash = d["db"]["hash"].as_str().expect("hash").to_owned();
    assert_eq!(hash.len(), 16);
    assert_eq!(
        (
            d["db"]["ecu"].clone(),
            d["db"]["dids"].clone(),
            d["replaced"].clone()
        ),
        (json!("bench"), json!(3), json!(false))
    );
    assert_eq!(d["warnings"].as_array().map(Vec::len), Some(2), "{d}");
    assert_eq!(d["state"]["db"]["hash"], hash.as_str());
    assert_eq!(env.state("ecu1")["db"], d["db"], "published before reply");

    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.read_did",
            json!({"target": "ecu1", "dids": ["F1A0", "F190"]}),
        )
        .await;
    let row = &r.expect("read")["row"];
    assert_eq!(
        row["decoded"],
        json!([
            {"did": "F1A0", "name": "Supply voltage", "value": 0.5, "unit": "V"},
            {"did": "F190", "name": "VIN", "value": "LOOMSIM0000000001", "unit": null},
        ])
    );
    assert!(
        env.row_events(&rid).iter().all(|p| p["db_hash"] == hash.as_str()),
        "every event of the row carries the DB identity"
    );

    let (r, _) = env
        .call(
            Role::Agent,
            "uds.db.set",
            json!({"target": "ecu1", "db": {"loom_diag": 1, "dids": []}}),
        )
        .await;
    let d2 = r.expect("replace db");
    assert_eq!(d2["replaced"], true);
    assert_ne!(d2["db"]["hash"], hash.as_str());
    assert_eq!(d2["warnings"], json!([]));

    let (r, _) = env
        .call(Role::Agent, "uds.db.clear", json!({"target": "ecu1"}))
        .await;
    assert_eq!(r.expect("clear")["cleared"], true);
    assert!(
        env.state("ecu1").get("db").is_none_or(Value::is_null),
        "state without db"
    );
    let (r, rid) = env
        .call(
            Role::Agent,
            "uds.read_did",
            json!({"target": "ecu1", "dids": ["F1A0"]}),
        )
        .await;
    assert_eq!(
        r.expect("read")["row"]["decoded"],
        json!([{"did": "F1A0", "name": null, "value": "01", "unit": null}])
    );
    assert!(
        env.row_events(&rid).iter().all(|p| p.get("db_hash").is_none()),
        "no DB, no stamp"
    );
    let (r, _) = env
        .call(Role::Agent, "uds.db.clear", json!({"target": "ecu1"}))
        .await;
    assert_eq!(r.expect("clear again")["cleared"], false, "idempotent");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn u5_db_refusals_lease_and_replace() {
    let mut env = connected().await;
    let set = |db: Value| json!({"target": "ecu1", "db": db});
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.db.set",
            set(json!({"loom_diag": 2, "dids": []})),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Usage));
    assert!(
        err(&r).message.starts_with("invalid loom_diag DB: "),
        "{}",
        err(&r).message
    );
    let (r, _) = env
        .call(
            Role::Agent,
            "uds.db.set",
            set(json!({"loom_diag": 1, "ecu": "x".repeat(50_000), "dids": []})),
        )
        .await;
    assert_eq!(code(&r), Some(ErrorCode::Usage));
    assert!(err(&r).message.contains("49152"), "{}", err(&r).message);
    assert_eq!(
        code(&env.call(Role::Agent, "uds.db.set", json!({"db": db()})).await.0),
        Some(ErrorCode::Usage),
        "target required"
    );
    assert_eq!(
        code(
            &env.call(Role::Agent, "uds.db.set", json!({"target": "ecu1"}))
                .await
                .0
        ),
        Some(ErrorCode::Usage),
        "db required"
    );
    assert_eq!(
        code(&env.call(Role::Agent, "uds.db.clear", json!({})).await.0),
        Some(ErrorCode::Usage),
        "target required"
    );
    assert_eq!(
        code(
            &env.call(Role::Agent, "uds.db.set", json!({"target": "nope", "db": db()}))
                .await
                .0
        ),
        Some(ErrorCode::NotOpen)
    );
    assert!(
        env.state("ecu1").get("db").is_none_or(Value::is_null),
        "nothing attached by a refusal"
    );

    // the user's lease: an agent cannot change what the user's rows decode to; the user can
    env.call(Role::Viewer, "uds.lease.takeover", json!({"target": "ecu1"}))
        .await
        .0
        .expect("takeover");
    assert_eq!(
        code(&env.call(Role::Agent, "uds.db.set", set(db())).await.0),
        Some(ErrorCode::HeldByUser)
    );
    env.call(Role::Viewer, "uds.db.set", set(db()))
        .await
        .0
        .expect("user attaches");
    assert_eq!(
        code(
            &env.call(Role::Agent, "uds.db.clear", json!({"target": "ecu1"}))
                .await
                .0
        ),
        Some(ErrorCode::HeldByUser)
    );
    env.call(Role::Viewer, "uds.lease.release", json!({"target": "ecu1"}))
        .await
        .0
        .expect("release");

    // closed target: set refused; a replace via connect starts without a DB (new link = new decision)
    env.call(Role::Agent, "uds.disconnect", json!({"target": "ecu1"}))
        .await
        .0
        .expect("disconnect");
    let (r, _) = env.call(Role::Agent, "uds.db.set", set(db())).await;
    assert_eq!(code(&r), Some(ErrorCode::NotOpen), "{r:?}");
    env.call(Role::Agent, "uds.connect", json!({"name": "ecu1", "sim": true}))
        .await
        .0
        .expect("replace");
    let st = env.state("ecu1");
    assert_eq!(st["generation"], 2);
    assert!(
        st.get("db").is_none_or(Value::is_null),
        "replace drops the DB: {st}"
    );
}
