//! Worker instances per kind (contract 01 §1.1, answers P1 "PerChannel").
//!
//! * `Instances::Shared` → one instance (id = kind) started with the daemon.
//! * `Instances::PerChannel{max}` → one process per channel, started on first use of that channel
//!   (id = `<kind>.<channel>`; the id is also the worker's producer id and journal directory, so it
//!   is unique across domains). Calls without a channel (enumeration) go to any running instance,
//!   else to a temporary `<kind>._enum` instance.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_domain_api::{DomainError, Instances, WorkerSpec};
use loom_proto::error::ErrorCode;
use loom_proto::event::kinds;
use loom_proto::num::U64;
use loom_worker_sdk::{Clock, EmitSink};
use serde_json::json;

use crate::daemon::Config;
use crate::domains::InprocFn;
use crate::hub::{Hub, core_source, lock};
use crate::journal::Journal;
use crate::policy::WorkersMode;
use crate::worker_host::WorkerHost;

/// Longest wait for a fresh worker's Hello.
pub const READY_WAIT: Duration = Duration::from_secs(5);

/// What starting a worker needs (shared by all pools).
pub struct Launcher {
    pub(crate) cfg: Config,
    pub(crate) hub: Hub,
    pub(crate) journal: Arc<Journal>,
    pub(crate) inproc_sink: Arc<dyn EmitSink>,
    pub(crate) ipc_sink: Arc<dyn EmitSink>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) epoch: u64,
    pub(crate) mode: WorkersMode,
}

impl std::fmt::Debug for Launcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launcher")
            .field("epoch", &self.epoch)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

/// Socket directory for worker lanes: `$LOOM_HOME/run/w`, or a short temp path when that would
/// exceed the Unix socket path limit (~104 B).
fn sock_dir(cfg: &Config) -> PathBuf {
    let d = cfg.home.run_dir().join("w");
    if d.join("x-worker-instance-name.data.sock").as_os_str().len() < 100 {
        return d;
    }
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("loom-w-{}-{n}", std::process::id()))
}

/// Channel names usable in instance ids (and so in socket and journal paths).
pub fn valid_channel(ch: &str) -> bool {
    !ch.is_empty()
        && ch != "_enum"
        && ch.len() <= 32
        && ch
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

impl Launcher {
    /// Start one IPC worker under supervision. State changes become `core.worker_state`; a crashed
    /// worker's recovered journal tail becomes `record.gap{cause:"crash_tail"}`.
    fn ipc(
        &self,
        spec: WorkerSpec,
        instance: &str,
        program: Option<PathBuf>,
        tick: Option<Duration>,
    ) -> Arc<dyn WorkerHost> {
        let cfg = &self.cfg;
        let program = program
            .or_else(|| cfg.worker_program.clone())
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("loomd"));
        let mut hc = loom_worker_host::HostConfig::new(spec, instance, program);
        hc.sock_dir = sock_dir(cfg);
        hc.session_id = format!("s{}", self.epoch);
        hc.stream_epoch = self.epoch;
        hc.journal_dir = Some(self.journal.root.clone());
        hc.config = json!({
            "journal_dir": self.journal.root,
            "tick_ms": tick.map(|t| u64::try_from(t.as_millis()).unwrap_or(u64::MAX)),
        });
        hc.orphan = cfg.worker_orphan;
        hc.env
            .push(("LOOM_HOME".into(), cfg.home.root().display().to_string()));
        let hub2 = self.hub.clone();
        let cb: loom_worker_host::StateCallback = Arc::new(move |e: &loom_worker_host::StateEvent| {
            let mut recs = Vec::new();
            for mut g in e.detail["journal_recovery"]["record_gaps"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                if g["cause"].is_null() {
                    g["cause"] = json!("crash_tail");
                }
                recs.push(core_source(&hub2.clock, "record", kinds::RECORD_GAP, g));
            }
            let payload = json!({
                "instance": e.instance, "kind": e.kind, "state": e.state,
                "producer_epoch": U64(e.producer_epoch), "pid": e.pid, "detail": e.detail,
            });
            recs.push(core_source(
                &hub2.clock,
                "core",
                kinds::CORE_WORKER_STATE,
                payload,
            ));
            drop(hub2.publish(recs));
        });
        loom_worker_host::IpcHost::start(hc, self.ipc_sink.clone(), cb)
    }
}

/// Instances of one worker kind.
pub struct WorkerPool {
    /// Domain.
    pub domain: String,
    /// Worker kind.
    pub kind: String,
    spec: WorkerSpec,
    inproc: InprocFn,
    program: Option<PathBuf>,
    tick: Option<Duration>,
    launcher: Arc<Launcher>,
    instances: Mutex<BTreeMap<String, Arc<dyn WorkerHost>>>,
    spawn: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerPool")
            .field("domain", &self.domain)
            .field("kind", &self.kind)
            .field("instances", &lock(&self.instances).keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl WorkerPool {
    /// New pool.
    pub fn new(
        domain: &str,
        spec: WorkerSpec,
        inproc: InprocFn,
        program: Option<PathBuf>,
        tick: Option<Duration>,
        launcher: Arc<Launcher>,
    ) -> Arc<WorkerPool> {
        Arc::new(WorkerPool {
            domain: domain.to_owned(),
            kind: spec.kind.clone(),
            spec,
            inproc,
            program,
            tick,
            launcher,
            instances: Mutex::new(BTreeMap::new()),
            spawn: tokio::sync::Mutex::new(()),
        })
    }

    /// One process per channel.
    pub fn per_channel(&self) -> Option<u16> {
        match self.spec.instances {
            Instances::PerChannel { max } => Some(max),
            Instances::Shared => None,
        }
    }

    /// All running instances.
    pub fn all(&self) -> Vec<Arc<dyn WorkerHost>> {
        lock(&self.instances).values().cloned().collect()
    }

    fn launch(&self, instance: &str) -> Arc<dyn WorkerHost> {
        let l = &self.launcher;
        match l.mode {
            WorkersMode::Inproc => (self.inproc)(l.inproc_sink.clone(), l.clock.clone(), instance),
            WorkersMode::Ipc => l.ipc(self.spec.clone(), instance, self.program.clone(), self.tick),
        }
    }

    /// Start the shared instance (no-op for PerChannel kinds).
    pub fn start_shared(&self) {
        if self.per_channel().is_none() {
            let w = self.launch(&self.kind);
            lock(&self.instances).insert(self.kind.clone(), w);
        }
    }

    fn key(&self, channel: Option<&str>) -> Result<Option<String>, DomainError> {
        if self.per_channel().is_none() {
            return Ok(Some(self.kind.clone()));
        }
        match channel {
            Some(ch) if valid_channel(ch) => Ok(Some(format!("{}.{ch}", self.kind))),
            Some(ch) => Err(DomainError::new(
                ErrorCode::Usage,
                format!("channel {ch:?}: use 1–32 of [A-Za-z0-9_-]"),
            )),
            None => Ok(None),
        }
    }

    /// The instance serving `channel` if it runs (no start).
    pub fn existing(&self, channel: Option<&str>) -> Option<Arc<dyn WorkerHost>> {
        let key = self.key(channel).ok()??;
        lock(&self.instances).get(&key).cloned()
    }

    /// The instance serving `channel`, starting it when needed. `true` = started now.
    pub async fn get(&self, channel: Option<&str>) -> Result<(Arc<dyn WorkerHost>, bool), DomainError> {
        let key = match self.key(channel)? {
            Some(k) => k,
            None => {
                // enumeration etc.: any running instance, else a temporary `_enum` one
                if let Some(w) = lock(&self.instances).values().next().cloned() {
                    return Ok((w, false));
                }
                format!("{}._enum", self.kind)
            }
        };
        if let Some(w) = lock(&self.instances).get(&key).cloned() {
            return Ok((w, false));
        }
        let _g = self.spawn.lock().await;
        if let Some(w) = lock(&self.instances).get(&key).cloned() {
            return Ok((w, false));
        }
        if let Some(max) = self.per_channel() {
            let n = lock(&self.instances)
                .keys()
                .filter(|k| !k.ends_with("._enum"))
                .count();
            if n >= usize::from(max) && !key.ends_with("._enum") {
                return Err(DomainError::new(
                    ErrorCode::Limit,
                    format!("{}: at most {max} channels have a worker", self.kind),
                )
                .with_detail(json!({"limit": max})));
            }
        }
        let w = self.launch(&key);
        lock(&self.instances).insert(key, w.clone());
        wait_ready(&w, Instant::now() + READY_WAIT).await;
        Ok((w, true))
    }
}

/// Wait until a worker left `starting` (or `by` passed).
pub async fn wait_ready(w: &Arc<dyn WorkerHost>, by: Instant) {
    while w.state()["state"] == "starting" && Instant::now() < by {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
