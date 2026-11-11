//! Shared helpers for worker-host tests.
#![allow(dead_code, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{Arch, Budgets, Instances, Platform, Reopen, WorkerSpec};
use loom_proto::event::{Origin, SourceRecord};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::worker::{Call, Lane, Write};
use loom_worker_host::{HostConfig, IpcHost, StateCallback, StateEvent, WorkerState};
use loom_worker_sdk::{EmitError, EmitSink};
use serde_json::{Value, json};

pub const TEST_WORKER: &str = env!("CARGO_BIN_EXE_loom-test-worker");

/// Short socket dir (Unix socket paths are limited to ~100 bytes).
pub fn sock_dir() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("lw").tempdir_in("/tmp").unwrap()
}

pub fn spec() -> WorkerSpec {
    WorkerSpec {
        kind: "test".into(),
        instances: Instances::Shared,
        budgets: Budgets {
            mem_soft: 0,
            mem_hard: 0,
            credit: 1024 * 1024,
        },
        reopen_after_crash: Reopen::None,
        arch: Arch::Native,
        platforms: vec![Platform::Macos, Platform::Linux, Platform::Windows],
    }
}

/// Fast timings for tests.
pub fn config(dir: &std::path::Path, instance: &str, extra: &[&str]) -> HostConfig {
    let mut c = HostConfig::new(spec(), instance, TEST_WORKER);
    c.args = vec![];
    c.extra_args = extra.iter().map(|s| (*s).to_string()).collect();
    c.sock_dir = dir.to_path_buf();
    c.health_interval = Duration::from_millis(100);
    c.suspect_after = Duration::from_millis(400);
    c.stall_after = Duration::from_millis(700);
    c.kill_after = Duration::from_millis(1200);
    c.backoff_base = Duration::from_millis(30);
    c.backoff_max = Duration::from_millis(200);
    c.orphan = Some(Duration::from_millis(300));
    c
}

/// Sink collecting everything.
#[derive(Debug, Default)]
pub struct CollectSink {
    pub recs: Mutex<Vec<(Lane, SourceRecord)>>,
}

impl EmitSink for CollectSink {
    fn try_push(&self, lane: Lane, record: SourceRecord) -> Result<(), EmitError> {
        self.recs.lock().unwrap().push((lane, record));
        Ok(())
    }
}

impl CollectSink {
    pub fn snapshot(&self) -> Vec<(Lane, SourceRecord)> {
        self.recs.lock().unwrap().clone()
    }
    pub fn kinds(&self, lane: Lane, kind: &str) -> Vec<SourceRecord> {
        self.snapshot()
            .into_iter()
            .filter(|(l, r)| *l == lane && r.kind == kind)
            .map(|(_, r)| r)
            .collect()
    }
}

/// State event log.
#[derive(Debug, Default, Clone)]
pub struct States(pub Arc<Mutex<Vec<StateEvent>>>);

impl States {
    pub fn callback(&self) -> StateCallback {
        let s = self.0.clone();
        Arc::new(move |e: &StateEvent| s.lock().unwrap().push(e.clone()))
    }
    pub fn seen(&self, st: WorkerState) -> usize {
        self.0.lock().unwrap().iter().filter(|e| e.state == st).count()
    }
    pub fn events(&self) -> Vec<StateEvent> {
        self.0.lock().unwrap().clone()
    }
}

pub struct Started {
    pub host: Arc<IpcHost>,
    pub sink: Arc<CollectSink>,
    pub states: States,
}

pub fn start(cfg: HostConfig) -> Started {
    let sink = Arc::new(CollectSink::default());
    let states = States::default();
    let host = IpcHost::start(cfg, sink.clone(), states.callback());
    Started { host, sink, states }
}

pub async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + timeout;
    while !f() {
        assert!(Instant::now() < end, "condition not met within {timeout:?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

pub async fn wait_state(host: &IpcHost, st: WorkerState, timeout: Duration) {
    wait_until(timeout, || host.worker_state() == st).await;
}

pub fn call(method: &str, args: Value) -> Call {
    Call {
        conn_id: None,
        call_id: U64(0),
        method: method.into(),
        args,
        origin: Origin::Agent,
        client_id: Some("test".into()),
        actor_id: None,
        request_id: None,
        deadline_mono_ns: U64(0),
        approval_ref: None,
    }
}

pub fn write(request_id: &str, op: Value) -> Write {
    Write {
        call_id: U64(0),
        request_id: request_id.into(),
        write_class: WriteClass::Request,
        op,
        origin: Origin::Agent,
        client_id: Some("test".into()),
        plan_digest: None,
        approval_ref: None,
        deadline_mono_ns: U64(0),
    }
}

pub fn data(v: &str) -> Value {
    json!({"data": v})
}

/// True while a process with `pid` exists (Unix `kill -0`).
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn kill9(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
}
