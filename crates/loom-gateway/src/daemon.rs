//! The real `loomd` (P1): shared daemon state, core RPCs, domain dispatch, the request-class write
//! pipeline v0 (contract 04 §4.2, F1 = automatic), STOP, markers and activity.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_domain_api::{
    ApprovalHandle, BoxFuture, CallCtx, CommandSpec, CtxServices, DomainError, EventDraft, Interlock,
    JournalQuery, JournalReader, RoleSet, SafeStateSpec, ScanResult, TargetRef, WaitResult, WaitSpec,
    WorkerHandle, WriteHooks, WriteRequest,
};
use loom_proto::bytes::Bytes;
use loom_proto::core_events::{
    ActivityPayload, AgentCursorPayload, LatchPayload, SafeStateItem, SafeStateKind, SafeStateList,
    SafeStateOutcome, SafeStateRun, SafeStateRunItem, SafeStateRunResult,
};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::{
    Envelope, ErrorBody, Meta, OthersSinceLast, RecordHealth, Viewers, Visibility, Warning, warnings,
};
use loom_proto::error::ErrorCode;
use loom_proto::event::{
    Dir, Env, Origin, PublishedRecord, Risk, SourceRecord, WriteState, kinds, payload_write_state,
};
use loom_proto::num::U64;
use loom_proto::policy::WriteClass;
use loom_proto::rpc::{METHOD_NOT_FOUND, Rights, Role, RpcError, SubFilter};
use loom_proto::worker::{Call, Input, InputLeaseEnd, InputLeaseEndCause, Latch, Write};
use loom_worker_sdk::{Clock, EmitSink, WriteOutcome};
use serde_json::{Value, json};

use crate::audit::Audit;
use crate::auth::Auth;
use crate::domains::{self, Loaded};
use crate::home::{DaemonInfo, Home, InstanceLock, random_hex};
use crate::hub::{Hub, HubBuilder, core_source, lock};
use crate::journal::{Journal, JournalMapping, worse};
use crate::policy::{Action, PolicySet, Reload, WorkersMode};
use crate::pool::{Launcher, WorkerPool, wait_ready};
use crate::rpcutil::{app_error, app_error_detail, params, rpc_error};
use crate::state::{Idempotency, Reserve, load_latch, persist_latch};
use crate::worker_host::{HubSink, PendingWrites, WorkerHost};

mod core_rpc;
pub mod fence;

/// Default port (contract: fixed default, OS-assigned if taken).
pub const DEFAULT_PORT: u16 = 47123;
/// Protocol spoken.
pub const PROTO: &str = "loom/1.0";
/// Worker latch ack budget.
pub const LATCH_ACK: Duration = Duration::from_millis(200);
/// Notable-event log length (others_since_last).
const NOTABLE_CAP: usize = 65_536;
/// Client cursors kept (LRU) and their TTL.
const CURSOR_CAP: usize = 4096;
const CURSOR_TTL: Duration = Duration::from_secs(24 * 3600);

/// Daemon configuration.
#[derive(Clone)]
pub struct Config {
    /// State root.
    pub home: Home,
    /// Port: `None` = [`DEFAULT_PORT`] (OS-assigned if taken), `Some(0)` = OS-assigned.
    pub port: Option<u16>,
    /// Static UI directory.
    pub ui_dir: Option<PathBuf>,
    /// Rights of bootstrap cookies (`--rights`, default operate+approve = approve).
    pub bootstrap_rights: Rights,
    /// Environment badge from `--env` (`None` = `[env] env` of the policy files, else bench).
    pub env: Option<Env>,
    /// Project policy file (`--config`); default `$LOOM_HOME/loom.toml`.
    pub config_path: Option<PathBuf>,
    /// Clock stamping in-process worker events (default: the gateway clock; benches inject theirs).
    pub clock: Option<Arc<dyn Clock>>,
    /// Journal scan budget per query (bytes).
    pub scan_bytes: u64,
    /// Retention interval (default 10 min).
    pub retain_interval: Duration,
    /// Worker placement (`None` = `[workers] mode`, else IPC).
    pub workers: Option<WorkersMode>,
    /// Worker executable for `loomd worker <kind>` (default: this executable).
    pub worker_program: Option<PathBuf>,
    /// Orphan time passed to IPC workers (`None` = worker default 30 s).
    pub worker_orphan: Option<Duration>,
    /// Test hook: inject journal I/O faults.
    #[doc(hidden)]
    pub journal_fault: Option<loom_journal::FaultHook>,
    /// Extra allowed browser origins (dev servers).
    pub allow_origins: Vec<String>,
    /// publish-before-reply deadline.
    pub publish_deadline: Duration,
    /// Print the bootstrap URL (stdout is a TTY).
    pub print_url: bool,
}

impl Config {
    /// Defaults for `home`.
    pub fn new(home: Home) -> Self {
        Config {
            home,
            port: None,
            ui_dir: None,
            bootstrap_rights: Rights::Approve,
            env: None,
            config_path: None,
            clock: None,
            scan_bytes: loom_journal::reader::DEFAULT_SCAN_BYTES,
            retain_interval: Duration::from_secs(600),
            journal_fault: None,
            workers: None,
            worker_program: None,
            worker_orphan: None,
            allow_origins: Vec::new(),
            publish_deadline: Duration::from_secs(2),
            print_url: false,
        }
    }

    /// Policy files in merge order: project (`--config` or `$LOOM_HOME/loom.toml`), then user
    /// (`$LOOM_HOME/config/policy.toml`).
    pub fn policy_files(&self) -> Vec<PathBuf> {
        vec![
            self.config_path.clone().unwrap_or_else(|| self.home.config()),
            self.home.root().join("config").join("policy.toml"),
        ]
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("home", &self.home)
            .field("port", &self.port)
            .field("ui_dir", &self.ui_dir)
            .field("env", &self.env)
            .field("config_path", &self.config_path)
            .field("scan_bytes", &self.scan_bytes)
            .finish_non_exhaustive()
    }
}

/// Who is calling (decided by connection credentials, never by params).
#[derive(Debug, Clone)]
pub struct Caller {
    /// Role.
    pub role: Role,
    /// Viewer rights.
    pub rights: Option<Rights>,
    /// Client id.
    pub client_id: String,
    /// Connection id.
    pub conn_id: u64,
}

impl Caller {
    /// Origin of everything this caller does.
    pub fn origin(&self) -> Origin {
        self.role.origin()
    }
    fn viewer_at_least(&self, need: Rights) -> bool {
        self.role == Role::Viewer && self.rights.is_some_and(|r| r >= need)
    }
    fn allowed(&self, roles: &RoleSet) -> bool {
        match self.role {
            Role::Agent => roles.agent,
            Role::Script => roles.script,
            Role::Viewer => roles.viewer.is_some_and(|need| self.viewer_at_least(need)),
        }
    }
}

struct DomainEntry {
    commands: HashMap<String, CommandSpec>,
    hooks: Option<Arc<dyn WriteHooks>>,
    safe_states: Vec<SafeStateSpec>,
    emergency_offs: Vec<loom_domain_api::EmergencyOffSpec>,
    input_owner: loom_domain_api::Owner,
    interlocks: Vec<(String, Interlock)>,
    write_kind: String,
    /// Declared success terminal per write method (`Registry::success_terminal`; default `confirmed`).
    terminals: HashMap<String, WriteState>,
    dry_run_previews: HashMap<String, RoleSet>,
    help: Option<loom_domain_api::HelpSpec>,
    pools: Vec<Arc<WorkerPool>>,
}

impl DomainEntry {
    fn interlock_active(&self) -> Option<&str> {
        self.interlocks.iter().find(|(_, f)| f()).map(|(n, _)| n.as_str())
    }
}

#[derive(Debug)]
struct NotableItem {
    seq: u64,
    origin: Origin,
    client_id: Option<String>,
    kind: String,
    key: String,
}

#[derive(Debug, Default)]
struct Notable {
    hw: u64,
    items: VecDeque<NotableItem>,
}

/// Counted by `others_since_last` (contract 03 §3.2): user actions, other clients' and scripts'
/// writes (one per request: the `requested` row), interactive input, STOP and policy changes.
/// Device RX is never counted; the caller's own events are filtered out at read time.
fn is_notable(r: &PublishedRecord) -> bool {
    if r.kind == kinds::CORE_LATCH || r.kind == kinds::CORE_POLICY_CHANGED {
        return true;
    }
    if r.origin == Origin::Device {
        return false;
    }
    let st = payload_write_state(&r.payload).ok().flatten();
    st == Some(WriteState::Requested)
        || r.kind.ends_with(".input")
        || (r.origin == Origin::User && r.kind == kinds::CORE_MARK)
}

#[derive(Debug, Default)]
struct InputBatch {
    text: Vec<u8>,
    first: Option<Instant>,
    domain: String,
    channel: Option<String>,
    client_id: String,
}

/// Shared daemon state.
pub struct Daemon {
    /// Configuration.
    pub cfg: Config,
    /// Event hub.
    pub hub: Hub,
    /// Credentials.
    pub auth: Auth,
    /// Audit log.
    pub audit: Audit,
    domains: BTreeMap<String, DomainEntry>,
    pending: Arc<PendingWrites>,
    latch: AtomicBool,
    latch_seq: AtomicU64,
    latch_lock: tokio::sync::Mutex<()>,
    record_failed_published: AtomicBool,
    idem: Idempotency,
    policy: Mutex<PolicySet>,
    env: Mutex<Env>,
    /// Recording (session journal, mapping log, retention).
    pub journal: Arc<Journal>,
    /// `(journal health, retention health, last published overall health)`.
    rec: Mutex<(RecordHealth, RecordHealth, RecordHealth)>,
    notable: Arc<Mutex<Notable>>,
    cursors: Mutex<HashMap<String, (u64, Instant)>>,
    activity: Mutex<BTreeMap<(String, String), u64>>,
    presence: Mutex<HashMap<u64, bool>>,
    metrics: Mutex<Value>,
    inputs: Mutex<HashMap<(u64, String), InputBatch>>,
    /// Input leases: `(conn, worker instance)` → `(lease id, worker)`.
    leases: Mutex<LeaseMap>,
    event_cursors: Mutex<HashMap<String, u64>>,
    shutdown: tokio::sync::Notify,
    closing: AtomicBool,
    focus_last: Mutex<HashMap<String, Instant>>,
    /// Hub high-water, updated on every publish (`CallCtx::wait`).
    hw_rx: tokio::sync::watch::Receiver<u64>,
    next_id: AtomicU64,
    viewer_conns: AtomicUsize,
    port: AtomicU16,
    started: Instant,
    /// Per-producer commit coverage for CORE source fences (hub observer).
    commits: Arc<Mutex<fence::Commits>>,
    /// TEST session module and its CORE runtime hooks (fences, STOP, write gate, finally).
    #[cfg(feature = "test")]
    test: Option<crate::test_services::TestWiring>,
}

impl std::fmt::Debug for Daemon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Daemon")
            .field("cfg", &self.cfg)
            .field("domains", &self.domains.keys().collect::<Vec<_>>())
            .field("latch", &self.latch)
            .finish_non_exhaustive()
    }
}

/// A started daemon.
#[derive(Debug)]
pub struct Running {
    /// Shared state.
    pub daemon: Arc<Daemon>,
    /// Bound address.
    pub addr: SocketAddr,
    /// Discovery info written to `daemon.json`.
    pub info: DaemonInfo,
    /// Server task.
    pub server: tokio::task::JoinHandle<()>,
    /// Held instance lock.
    pub lock: InstanceLock,
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1)
}

/// Cursor/self key of a client id: the part before `#` (`LOOM_CLIENT` name without the pid), so
/// successive CLI processes of one agent share their `others_since_last` cursor (contract 03).
pub fn client_key(id: &str) -> &str {
    id.split_once('#').map_or(id, |(k, _)| k)
}

/// Start the daemon: lock, home, hub, domains, workers, audit, HTTP/WS server, `daemon.json`.
pub async fn start(cfg: Config) -> std::io::Result<Running> {
    cfg.home.ensure()?;
    let instance_lock = InstanceLock::acquire(&cfg.home)?;
    let (latched, latch_why) = load_latch(&cfg.home);
    let policy = PolicySet::load(cfg.policy_files())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("policy: {e}")))?;
    let env = cfg.env.or(policy.current.env).unwrap_or(Env::Bench);

    // Epoch = start time: unique across restarts without extra state; also names the session dir.
    let epoch = unix_secs();
    let (journal, crash_gaps) = Journal::open(&cfg.home, epoch, cfg.journal_fault.clone())?;
    let notable = Arc::new(Mutex::new(Notable::default()));
    let n2 = notable.clone();
    let commits = Arc::new(Mutex::new(fence::Commits::default()));
    let c2 = commits.clone();
    let (hw_tx, hw_rx) = tokio::sync::watch::channel(0u64);
    let j2 = journal.clone();
    let mut hb = HubBuilder::new(epoch)
        .mapping(Box::new(JournalMapping(journal.clone())))
        .pre_publish(Box::new(move |s: &mut SourceRecord| {
            j2.stamp_and_append_gateway(s)
        }))
        .observer(Box::new(move |r: &PublishedRecord| {
            lock(&c2).observe(r);
            let mut n = lock(&n2);
            n.hw = r.seq.get();
            hw_tx.send_replace(n.hw);
            if is_notable(r) {
                if n.items.len() == NOTABLE_CAP {
                    n.items.pop_front();
                }
                n.items.push_back(NotableItem {
                    seq: r.seq.get(),
                    origin: r.origin,
                    client_id: r.client_id.clone(),
                    kind: r.kind.clone(),
                    key: match &r.channel {
                        Some(c) => format!("{}:{c}", r.domain),
                        None => r.domain.clone(),
                    },
                });
            }
        }));

    // Domains need the hub for their workers' sink, and the hub needs their reducers: build the
    // hub with a sink placeholder by registering domains against a deferred hub handle.
    let pending = Arc::new(PendingWrites::default());
    // Placement: `Config.workers` > `[workers] mode` > in-process when a clock is injected (it only
    // reaches in-process workers) > IPC.
    let mode = cfg
        .workers
        .or(policy.current.workers)
        .unwrap_or(if cfg.clock.is_some() {
            WorkersMode::Inproc
        } else {
            WorkersMode::Ipc
        });
    let loaded: Vec<Loaded> = domains::builtin();
    type Extra = (Option<String>, Option<Duration>);
    let mut decls: Vec<(String, Vec<domains::WorkerDecl>, Vec<Extra>)> = Vec::new();
    let mut entries = BTreeMap::new();
    #[cfg(feature = "test")]
    let mut test_wiring = None;
    for mut l in loaded {
        #[cfg(feature = "test")]
        if let Some(t) = l.test.take() {
            test_wiring = Some(t);
        }
        let name = l.registry.domain().to_owned();
        for r in l.registry.take_reducers() {
            hb = hb.reducer(&name, r);
        }
        let write_kind = l
            .registry
            .event_kinds()
            .iter()
            .find(|k| k.pointers.iter().any(|p| p.path == "/payload/write_state"))
            .map_or_else(|| format!("{name}.write"), |k| k.kind.clone());
        entries.insert(
            name.clone(),
            DomainEntry {
                commands: l
                    .registry
                    .commands()
                    .iter()
                    .map(|c| (c.method.clone(), c.clone()))
                    .collect(),
                dry_run_previews: l
                    .registry
                    .commands()
                    .iter()
                    .filter_map(|c| {
                        l.registry
                            .dry_run_preview_roles(&c.method)
                            .map(|roles| (c.method.clone(), roles))
                    })
                    .collect(),
                hooks: l.registry.hooks_ref().cloned(),
                safe_states: l.registry.safe_states().to_vec(),
                emergency_offs: l.registry.emergency_offs().to_vec(),
                input_owner: l.registry.input_events_owner(),
                interlocks: l
                    .registry
                    .interlocks()
                    .map(|(n, f)| (n.to_owned(), f.clone()))
                    .collect(),
                write_kind,
                terminals: l
                    .registry
                    .commands()
                    .iter()
                    .filter(|c| c.write_class.is_some())
                    .map(|c| {
                        let t = l
                            .registry
                            .success_terminal_of(&c.method)
                            .unwrap_or(WriteState::Confirmed);
                        (c.method.clone(), t)
                    })
                    .collect(),
                help: l.registry.help_ref().cloned(),
                pools: Vec::new(),
            },
        );
        let extra: Vec<Extra> = l
            .workers
            .iter()
            .map(|d| {
                let k = &d.spec.kind;
                (
                    l.registry.worker_program_of(k).map(str::to_owned),
                    l.registry.worker_tick_of(k),
                )
            })
            .collect();
        decls.push((name, std::mem::take(&mut l.workers), extra));
    }
    let hub = hb.start()?;
    // In-process workers stamp `ts` with the gateway clock (one clock domain with gateway events and
    // source fences) unless a test injects its own clock.
    let clock: Arc<dyn Clock> = cfg
        .clock
        .clone()
        .unwrap_or_else(|| Arc::new(fence::HubClockSource(hub.clock.clone())));
    // Workers start once the hub exists. In-process: the gateway journals their events. IPC: the
    // worker journals itself into this session (`WorkerHost::journals_itself`).
    let inproc_sink: Arc<dyn EmitSink> = HubSink::new(hub.clone(), pending.clone(), Some(journal.clone()));
    let ipc_sink: Arc<dyn EmitSink> = HubSink::new(hub.clone(), pending.clone(), None);
    let launcher = Arc::new(Launcher {
        cfg: cfg.clone(),
        hub: hub.clone(),
        journal: journal.clone(),
        inproc_sink,
        ipc_sink,
        clock: clock.clone(),
        epoch,
        mode,
    });
    for (name, ds, extra) in decls {
        let pools: Vec<Arc<WorkerPool>> = ds
            .into_iter()
            .zip(extra)
            .map(|(d, (prog, tick))| {
                // `Registry::worker_program` names a file next to loomd; the domains table may
                // give a full path. A missing file falls back to `loomd worker <kind>`.
                let program = d.program.clone().or_else(|| {
                    let name = prog?;
                    let exe = if cfg!(windows) {
                        format!("{name}.exe")
                    } else {
                        name.clone()
                    };
                    let p = std::env::current_exe().ok()?.with_file_name(exe);
                    if p.is_file() {
                        Some(p)
                    } else {
                        tracing::warn!("worker program {name} not found next to loomd; using `loomd worker`");
                        None
                    }
                });
                let pool = WorkerPool::new(&name, d.spec, d.inproc, program, tick, launcher.clone());
                pool.start_shared();
                pool
            })
            .collect();
        if let Some(e) = entries.get_mut(&name) {
            e.pools = pools;
        }
    }
    // Do not announce the daemon (daemon.json) before its workers answered Hello: the first write
    // of an autostarted CLI must not race worker start-up. Bounded by the hello timeout.
    let ready_by = Instant::now() + Duration::from_secs(5);
    for w in entries
        .values()
        .flat_map(|e| e.pools.iter().flat_map(|p| p.all()))
    {
        wait_ready(&w, ready_by).await;
    }

    let token = random_hex(32);
    let daemon = Arc::new(Daemon {
        audit: Audit::start(cfg.home.audit_dir()),
        auth: Auth::new(token.clone()),
        hub: hub.clone(),
        domains: entries,
        pending,
        latch: AtomicBool::new(latched),
        latch_seq: AtomicU64::new(0),
        latch_lock: tokio::sync::Mutex::new(()),
        record_failed_published: AtomicBool::new(false),
        idem: Idempotency::default(),
        policy: Mutex::new(policy),
        env: Mutex::new(env),
        journal,
        rec: Mutex::new((RecordHealth::Ok, RecordHealth::Ok, RecordHealth::Ok)),
        notable,
        cursors: Mutex::new(HashMap::new()),
        activity: Mutex::new(BTreeMap::new()),
        presence: Mutex::new(HashMap::new()),
        metrics: Mutex::new(Value::Null),
        inputs: Mutex::new(HashMap::new()),
        leases: Mutex::new(HashMap::new()),
        event_cursors: Mutex::new(HashMap::new()),
        shutdown: tokio::sync::Notify::new(),
        closing: AtomicBool::new(false),
        focus_last: Mutex::new(HashMap::new()),
        hw_rx,
        next_id: AtomicU64::new(1),
        viewer_conns: AtomicUsize::new(0),
        port: AtomicU16::new(0),
        started: Instant::now(),
        commits,
        #[cfg(feature = "test")]
        test: test_wiring,
        cfg,
    });
    #[cfg(feature = "test")]
    if let Some(t) = &daemon.test {
        t.runtime.bind(Arc::downgrade(&daemon));
    }

    // Restart with on/unknown latch → latched everywhere, and say so.
    if latched {
        for domain in daemon.domains.values() {
            if let Some(hooks) = &domain.hooks {
                hooks.on_latch(true);
            }
        }
        for w in daemon.workers() {
            let _ = tokio::time::timeout(
                LATCH_ACK,
                w.latch(Latch {
                    on: true,
                    scope: "all".into(),
                    reason: Some(format!("restart: latch file {latch_why}")),
                    latch_seq: U64(0),
                }),
            )
            .await;
        }
        let p = LatchPayload {
            on: true,
            by: format!("loomd (restart, latch file {latch_why})"),
            origin: Origin::Device,
            acked: Vec::new(),
            missing: Vec::new(),
            jobs_stopped: Vec::new(),
            approvals_cancelled: Vec::new(),
            uncancellable: Vec::new(),
            outputs_held: Vec::new(),
            persisted: latch_why == "on",
        };
        drop(daemon.hub.publish(vec![core_source(
            &daemon.hub.clock,
            "core",
            kinds::CORE_LATCH,
            serde_json::to_value(p).unwrap_or(Value::Null),
        )]));
    }
    if daemon.audit.failed() {
        daemon.report_record_failed("audit log unavailable at start");
    }
    daemon.publish_gaps(
        crash_gaps
            .iter()
            .map(loom_journal::GapReport::to_gap_data)
            .collect(),
    );
    daemon.write_manifest();
    spawn_recording(daemon.clone());
    for w in daemon.workers() {
        let mut s = core_source(&daemon.hub.clock, "core", kinds::CORE_WORKER_STATE, w.state());
        s.payload["instance"] = json!(wname(w.as_ref()));
        drop(daemon.hub.publish(vec![s]));
    }
    spawn_activity(daemon.clone());

    let (addr, server) = crate::server::serve(daemon.clone()).await?;
    let info = DaemonInfo {
        port: addr.port(),
        pid: std::process::id(),
        token,
        version: env!("CARGO_PKG_VERSION").into(),
        proto: PROTO.into(),
    };
    info.write(&daemon.cfg.home)?;
    if daemon.cfg.print_url {
        let code = daemon.auth.issue_code(daemon.cfg.bootstrap_rights);
        println!("loomd: http://localhost:{}/ui/#boot={code}", addr.port());
    }
    Ok(Running {
        daemon,
        addr,
        info,
        server,
        lock: instance_lock,
    })
}

fn spawn_recording(d: Arc<Daemon>) {
    let weak = Arc::downgrade(&d);
    let every = d.cfg.retain_interval;
    drop(d);
    tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(1));
        let mut last_retain: Option<Instant> = None;
        loop {
            t.tick().await;
            let Some(d) = weak.upgrade() else { break };
            let retain = last_retain.is_none_or(|l| l.elapsed() >= every);
            if retain {
                last_retain = Some(Instant::now());
            }
            // retention walks the disk: keep it off the async workers
            let _ = tokio::task::spawn_blocking(move || d.record_tick(retain)).await;
        }
    });
}

fn spawn_activity(d: Arc<Daemon>) {
    let weak = Arc::downgrade(&d);
    drop(d);
    tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(1));
        loop {
            t.tick().await;
            let Some(d) = weak.upgrade() else { break };
            let batch = std::mem::take(&mut *lock(&d.activity));
            if batch.is_empty() {
                continue;
            }
            let recs = batch
                .into_iter()
                .map(|((client, method), n)| {
                    let p = ActivityPayload {
                        action: None,
                        domain: None,
                        channel: None,
                        client_id: client.clone(),
                        method,
                        summary: format!("{n} call(s) in the last second"),
                        range: None,
                        waiting: None,
                    };
                    let mut s = core_source(
                        &d.hub.clock,
                        "core",
                        kinds::CORE_ACTIVITY,
                        serde_json::to_value(p).unwrap_or(Value::Null),
                    );
                    s.origin = Origin::Agent;
                    s.client_id = Some(client);
                    s
                })
                .collect();
            drop(d.hub.publish(recs));
        }
    });
}

impl Daemon {
    /// Every running worker instance.
    fn workers(&self) -> Vec<Arc<dyn WorkerHost>> {
        self.domains
            .values()
            .flat_map(|d| d.pools.iter().flat_map(|p| p.all()))
            .collect()
    }

    /// The instance of `pool` serving `channel` (started on first use; a fresh instance inherits
    /// the STOP latch).
    pub(crate) async fn instance(
        &self,
        pool: &WorkerPool,
        channel: Option<&str>,
    ) -> Result<Arc<dyn WorkerHost>, DomainError> {
        let (w, fresh) = pool.get(channel).await?;
        if fresh && self.latched() {
            let l = Latch {
                on: true,
                scope: "all".into(),
                reason: Some("STOP latched".into()),
                latch_seq: U64(0),
            };
            let _ = tokio::time::timeout(LATCH_ACK, w.latch(l)).await;
        }
        Ok(w)
    }

    fn next(&self, prefix: &str) -> String {
        format!("{prefix}{}", self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn set_port(&self, p: u16) {
        self.port.store(p, Ordering::Release);
    }

    /// Graceful stop (contract 05 §5.5): refuse new requests → `Latch{on, reason:"shutdown"}` to
    /// every worker (acks ≤ 200 ms) → release workers (close lanes, wait ≤ 2 s, else kill) → close
    /// the journal. The shutdown latch is **not** a user STOP: `latch.json` is left as it was, so a
    /// user STOP survives the restart and a running bench does not come back latched.
    pub async fn shutdown_gracefully(&self) -> Value {
        if self.closing.swap(true, Ordering::AcqRel) {
            return json!({"already": true});
        }
        let ws = self.workers();
        let latches = ws.iter().map(|w| {
            let l = Latch {
                on: true,
                scope: "all".into(),
                reason: Some("shutdown".into()),
                latch_seq: U64(0),
            };
            async move {
                (
                    wname(w.as_ref()),
                    tokio::time::timeout(LATCH_ACK, w.latch(l)).await,
                )
            }
        });
        let (mut acked, mut missing) = (Vec::new(), Vec::new());
        for (kind, r) in futures_util::future::join_all(latches).await {
            match r {
                Ok(Ok(())) => acked.push(kind),
                _ => missing.push(kind),
            }
        }
        let t0 = Instant::now();
        futures_util::future::join_all(ws.iter().map(|w| w.shutdown())).await;
        let released_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
        let j = self.journal.clone();
        let _ = tokio::task::spawn_blocking(move || j.close()).await;
        tracing::info!(
            ?acked,
            ?missing,
            released_ms,
            "graceful shutdown: workers released"
        );
        json!({"acked": acked, "missing": missing, "released_ms": released_ms})
    }

    /// Shutting down: new requests are refused.
    pub fn closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    /// Resolves when `core.shutdown` was called.
    pub async fn wait_shutdown(&self) {
        self.shutdown.notified().await;
    }

    /// Bound port.
    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Acquire)
    }

    /// CORE source fence: gateway clock, then a hub barrier, then the committed watermark of every
    /// producer (see [`fence`]). Fails closed when the hub cannot confirm coverage.
    pub async fn source_fence(&self) -> Result<fence::SourceFence, DomainError> {
        let captured_mono_ns = self.hub.clock.mono_ns();
        if !self.hub.barrier(fence::FENCE_BARRIER).await {
            return Err(DomainError::new(
                ErrorCode::Partial,
                "hub did not confirm commit coverage in time; no source fence",
            ));
        }
        let source_refs = lock(&self.commits).snapshot().ok_or_else(|| {
            DomainError::new(
                ErrorCode::Partial,
                format!("more than {} producers; no source fence", fence::MAX_PRODUCERS),
            )
        })?;
        let unflushed_domains = self
            .domains
            .iter()
            .filter(|(_, d)| d.pools.iter().flat_map(|p| p.all()).any(|w| w.journals_itself()))
            .map(|(n, _)| n.clone())
            .collect();
        Ok(fence::SourceFence {
            stream_epoch: self.hub.epoch,
            captured_mono_ns,
            clock_id: self.hub.clock.clock_id.clone(),
            source_refs,
            unflushed_domains,
        })
    }

    /// Call context for a gateway-side domain action (TEST runtime: STOP events).
    #[cfg(feature = "test")]
    pub(crate) fn domain_ctx(self: &Arc<Self>, c: &Caller, domain: &str, method: &str) -> CallCtx {
        let svc = self.services(c, domain, None, None, method);
        CallCtx::new(
            c.role,
            c.rights,
            c.client_id.clone(),
            None,
            Instant::now() + LATCH_ACK,
            svc,
        )
    }

    /// STOP latched.
    pub fn latched(&self) -> bool {
        self.latch.load(Ordering::Acquire)
    }

    /// Recording health = worst of audit, journal writers and retention.
    pub fn record_health(&self) -> RecordHealth {
        let audit = if self.audit.failed() {
            RecordHealth::Failed
        } else {
            RecordHealth::Ok
        };
        let journal = if self.journal.failed() {
            RecordHealth::Failed
        } else {
            RecordHealth::Ok
        };
        let r = lock(&self.rec);
        worse(worse(audit, journal), worse(r.0, r.1))
    }

    /// Hub high-water seen by the daemon.
    pub fn hw(&self) -> u64 {
        lock(&self.notable).hw
    }

    fn report_record_failed(&self, why: &str) {
        self.record_failed_published.store(true, Ordering::Release);
        self.refresh_record(why);
    }

    /// F1: automation has its granted rights when no confirmation/approval rules are configured.
    /// Send-time policy, STOP and interlocks remain authoritative for every request.
    pub(crate) fn hello_capabilities(&self) -> Vec<String> {
        let mut capabilities = vec!["core/1".to_owned()];
        if !lock(&self.policy).current.actions.iter().any(|(_, action)| {
            matches!(
                action,
                crate::policy::Action::Confirm | crate::policy::Action::Approve
            )
        }) {
            capabilities.push("policy/auto".to_owned());
        }
        capabilities
    }

    /// Effective environment badge.
    pub fn env(&self) -> Env {
        *lock(&self.env)
    }

    /// Recording lost data: agent/script R1+ writes are refused (contract 04 §4.7).
    pub fn record_failed(&self) -> bool {
        self.record_health() == RecordHealth::Failed
    }

    /// Publish `record.health` when the overall health changed.
    pub fn refresh_record(&self, why: &str) {
        let now = self.record_health();
        let (journal, retention) = {
            let mut r = lock(&self.rec);
            if r.2 == now {
                return;
            }
            r.2 = now;
            (r.0, r.1)
        };
        let audit = if self.audit.failed() { "failed" } else { "ok" };
        let s = core_source(
            &self.hub.clock,
            "record",
            kinds::RECORD_HEALTH,
            json!({"health": now, "detail": why, "causes": {"audit": audit, "journal": journal, "retention": retention}}),
        );
        drop(self.hub.publish(vec![s]));
    }

    /// Publish `record.gap` events (journal losses, crash tails, lost mapping).
    pub fn publish_gaps(&self, gaps: Vec<loom_proto::event::GapData>) {
        if gaps.is_empty() {
            return;
        }
        let recs = gaps
            .into_iter()
            .map(|g| {
                core_source(
                    &self.hub.clock,
                    "record",
                    kinds::RECORD_GAP,
                    serde_json::to_value(g).unwrap_or(Value::Null),
                )
            })
            .collect();
        drop(self.hub.publish(recs));
    }

    fn write_manifest(&self) {
        let p = lock(&self.policy).current.clone();
        self.journal.write_manifest(&json!({
            "env": self.env(),
            "policy_hash": p.hash,
            "policy_files": lock(&self.policy).files,
            "domains": self.domains.iter().map(|(n, d)| json!({"name": n, "workers": d.pools.iter().flat_map(|p| p.all()).map(|w| w.state()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "pid": std::process::id(),
        }));
    }

    /// One recording tick: journal health + gaps; retention when `retain` is set.
    pub fn record_tick(&self, retain: bool) {
        let (h, gaps) = self.journal.poll();
        self.publish_gaps(gaps);
        lock(&self.rec).0 = h;
        if retain {
            self.publish_gaps(self.journal.recover_earlier());
            let pol = lock(&self.policy).current.retention();
            let r = self.journal.retain(&pol);
            let level = match r.level {
                loom_journal::QuotaLevel::Ok => RecordHealth::Ok,
                loom_journal::QuotaLevel::Warn | loom_journal::QuotaLevel::Prune => RecordHealth::Degraded,
                loom_journal::QuotaLevel::Full => RecordHealth::Failed,
            };
            if r.deleted > 0 {
                tracing::info!(deleted = r.deleted, bytes = r.bytes, "retention");
            }
            lock(&self.rec).1 = level;
        }
        self.refresh_record("recording state changed");
    }

    /// Re-read policy files if they changed (before every write). Publishes `core.policy_changed`
    /// (ok or failed — a broken file keeps the previous policy) and audits the change.
    pub async fn policy_check(&self) {
        let (r, cur) = {
            let mut p = lock(&self.policy);
            let r = p.check();
            (r, p.current.clone())
        };
        let payload = match &r {
            Reload::Same => return,
            Reload::Changed => {
                let env = self.cfg.env.or(cur.env).unwrap_or(Env::Bench);
                let old = std::mem::replace(&mut *lock(&self.env), env);
                if old != env {
                    let s = core_source(
                        &self.hub.clock,
                        "core",
                        kinds::CORE_ENV,
                        json!({"env": env, "previous": old}),
                    );
                    drop(self.hub.publish(vec![s]));
                }
                json!({"ok": true, "hash": cur.hash, "deny": cur.deny, "actions": cur.actions, "warnings": cur.warnings, "env": env})
            }
            Reload::Failed(e) => json!({"ok": false, "error": e, "kept_hash": cur.hash}),
        };
        let s = core_source(
            &self.hub.clock,
            "core",
            kinds::CORE_POLICY_CHANGED,
            payload.clone(),
        );
        drop(self.hub.publish(vec![s]));
        self.audit.append_nowait("policy.changed", payload).await;
        if matches!(r, Reload::Changed) {
            self.write_manifest();
        }
    }

    /// Policy block for `method` as `(code, message, detail)`.
    fn policy_block(&self, method: &str) -> Option<(String, String, Value)> {
        let b = lock(&self.policy).current.blocked(method)?;
        let msg = match b.action {
            Action::Deny => format!("denied by policy ({})", b.pattern),
            a => format!(
                "policy asks for {a:?} on {} — confirmations/approvals arrive in M2, so this is enforced as deny (ask the user)",
                b.pattern
            )
            .to_lowercase(),
        };
        Some((
            b.pattern.clone(),
            msg,
            json!({"pattern": b.pattern, "action": b.action}),
        ))
    }

    /// A viewer connection said hello.
    pub fn viewer_joined(&self, conn: u64) {
        self.viewer_conns.fetch_add(1, Ordering::AcqRel);
        lock(&self.presence).insert(conn, true);
    }

    /// A connection closed.
    pub fn conn_closed(&self, conn: u64, viewer: bool) {
        if viewer {
            self.viewer_conns.fetch_sub(1, Ordering::AcqRel);
            lock(&self.presence).remove(&conn);
        }
        // Leases of a closed viewer end at the worker (uart 2: it may stop echoing that viewer).
        let ended: Vec<(String, Arc<dyn WorkerHost>)> = {
            let mut m = lock(&self.leases);
            let keys: Vec<_> = m.keys().filter(|(c, _)| *c == conn).cloned().collect();
            keys.into_iter().filter_map(|k| m.remove(&k)).collect()
        };
        for (lease_id, w) in ended {
            let end = InputLeaseEnd {
                lease_id,
                conn_id: Some(format!("c{conn}")),
                cause: InputLeaseEndCause::ConnClosed,
            };
            tokio::spawn(async move {
                let _ = w.input_lease_end(end).await;
            });
        }
        let pending: Vec<_> = {
            let mut m = lock(&self.inputs);
            let keys: Vec<_> = m.keys().filter(|(c, _)| *c == conn).cloned().collect();
            keys.into_iter().filter_map(|k| m.remove(&k)).collect()
        };
        for b in pending {
            self.flush_input(b);
        }
    }

    /// `meta.viewers` / visibility.
    pub fn viewers(&self) -> Viewers {
        let p = lock(&self.presence);
        Viewers {
            subscribed: u16::try_from(p.len()).unwrap_or(u16::MAX),
            visible: u16::try_from(p.values().filter(|v| **v).count()).unwrap_or(u16::MAX),
        }
    }

    fn meta(&self) -> (Meta, Vec<Warning>) {
        let viewers = self.viewers();
        let visibility = if viewers.subscribed == 0 {
            Visibility::NoViewer
        } else if viewers.visible == 0 {
            Visibility::NotVisible
        } else {
            Visibility::Visible
        };
        let mut w = Vec::new();
        match visibility {
            Visibility::NoViewer => w.push(Warning {
                code: warnings::NO_VIEWER_ATTACHED.into(),
                detail: None,
            }),
            Visibility::NotVisible => w.push(Warning {
                code: warnings::NO_VISIBLE_VIEWER.into(),
                detail: None,
            }),
            Visibility::Visible => {}
        }
        if self.env() == Env::Vehicle {
            w.push(Warning {
                code: warnings::ENV_VEHICLE.into(),
                detail: None,
            });
        }
        let meta = Meta {
            record_health: self.record_health(),
            env: Some(self.env()),
            latch: self.latched(),
            viewers,
            stream_epoch: U64(self.hub.epoch),
            visibility: Some(visibility),
            ..Meta::default()
        };
        (meta, w)
    }

    /// `others_since_last` for `client` (client-global cursor), advancing the cursor to now.
    /// Publishes `core.agent_cursor` when an agent is told about others' activity.
    pub fn others_since_last(&self, c: &Caller) -> OthersSinceLast {
        let me = client_key(&c.client_id).to_owned();
        let n = lock(&self.notable);
        let hw = n.hw;
        let prev = {
            let mut cur = lock(&self.cursors);
            if cur.len() >= CURSOR_CAP && !cur.contains_key(&me) {
                cur.retain(|_, (_, at)| at.elapsed() < CURSOR_TTL);
                if cur.len() >= CURSOR_CAP
                    && let Some(oldest) = cur.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone())
                {
                    cur.remove(&oldest);
                }
            }
            cur.insert(me.clone(), (hw, Instant::now())).map(|(s, _)| s)
        };
        let Some(from) = prev else {
            return OthersSinceLast::default();
        };
        let mut o = OthersSinceLast {
            from_cursor: Some(Cursor::Seq {
                epoch: Some(self.hub.epoch),
                seq: from,
            }),
            ..OthersSinceLast::default()
        };
        let mut origins = BTreeSet::new();
        let mut count = 0u64;
        for it in n.items.iter().rev().take_while(|i| i.seq > from) {
            if it.seq > hw || it.client_id.as_deref().map(client_key) == Some(me.as_str()) {
                continue;
            }
            count += 1;
            origins.insert(it.origin);
            let k = o.kinds.entry(it.kind.clone()).or_insert(U64(0));
            *k = U64(k.get() + 1);
            let b = o.by_channel.entry(it.key.clone()).or_insert(U64(0));
            *b = U64(b.get() + 1);
        }
        drop(n);
        o.count = U64(count);
        o.origins = origins.into_iter().collect();
        if count > 0 && c.role == Role::Agent {
            let p = AgentCursorPayload {
                client_id: c.client_id.clone(),
                from_seq: U64(from),
                to_seq: U64(hw),
                others_count: U64(count),
            };
            let mut s = core_source(
                &self.hub.clock,
                "core",
                kinds::CORE_AGENT_CURSOR,
                serde_json::to_value(p).unwrap_or(Value::Null),
            );
            s.origin = Origin::Agent;
            s.client_id = Some(c.client_id.clone());
            drop(self.hub.publish(vec![s]));
        }
        o
    }

    fn envelope(
        &self,
        c: &Caller,
        cmd: &str,
        r: Result<Value, ErrorBody>,
        data_on_err: Option<Value>,
    ) -> Value {
        let (mut meta, w) = self.meta();
        let others = self.others_since_last(c);
        let mut env = match r {
            Ok(mut v) => {
                self.promote_meta(c, &mut v, &mut meta);
                Envelope::success(cmd, v, meta, others)
            }
            Err(e) => {
                let mut env = Envelope::failure(cmd, e, meta, others);
                env.data = data_on_err;
                env
            }
        };
        env.warnings = w;
        serde_json::to_value(env).unwrap_or(Value::Null)
    }

    /// Domain results may carry `_meta{next_cursor, truncated, range}`: it becomes the envelope
    /// `meta` and the client's `@last` moves to `next_cursor` (else the end of `range`).
    fn promote_meta(&self, c: &Caller, v: &mut Value, meta: &mut Meta) {
        let Some(m) = v.as_object_mut().and_then(|o| o.remove("_meta")) else {
            return;
        };
        if let Some(t) = m.get("truncated").and_then(Value::as_bool) {
            meta.truncated = t;
        }
        if let Some(r) = m
            .get("range")
            .and_then(|r| serde_json::from_value::<loom_proto::event::SeqRange>(r.clone()).ok())
        {
            meta.range = Some(r);
        }
        let next = m
            .get("next_cursor")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Cursor>().ok());
        let last = match &next {
            Some(Cursor::Seq { seq, .. }) => Some(seq.saturating_sub(1)),
            _ => meta.range.as_ref().map(|r| r.to.get()),
        };
        meta.next_cursor = next;
        if let Some(seq) = last {
            lock(&self.event_cursors).insert(client_key(&c.client_id).to_owned(), seq);
        }
    }

    /// Bounded wait (`CallCtx::wait`): woken by every hub publish, never polls.
    pub async fn wait(&self, spec: &WaitSpec) -> Result<WaitResult, DomainError> {
        let mut after = match &spec.after {
            Cursor::Seq { epoch: Some(e), .. } if *e != self.hub.epoch => {
                return Err(DomainError::new(
                    ErrorCode::CursorGap,
                    "cursor is from another daemon run",
                ));
            }
            Cursor::Seq { seq, .. } => *seq,
            _ => self.hw(),
        };
        let max = usize::try_from(spec.max_events.clamp(1, 1000)).unwrap_or(1000);
        let mut rx = self.hw_rx.clone();
        let mut events = Vec::new();
        loop {
            let hw = *rx.borrow_and_update();
            if hw > after {
                let r = self
                    .hub
                    .range(crate::hub::RangeQuery {
                        filter: spec.filter.clone(),
                        after: Some(after),
                        until: Some(hw),
                        max: max - events.len(),
                        newest: false,
                        ..Default::default()
                    })
                    .await
                    .map_err(|_| DomainError::new(ErrorCode::Daemon, "hub stopped"))?;
                after = if r.more {
                    r.events.last().map_or(hw, |p| p.rec.seq.get())
                } else {
                    hw
                };
                events.extend(r.events.iter().map(|p| p.rec.clone()));
            }
            let done = events.len() >= max;
            let left = spec.deadline.saturating_duration_since(Instant::now());
            if done || left.is_zero() {
                return Ok(WaitResult {
                    events,
                    cursor: Cursor::Seq {
                        epoch: Some(self.hub.epoch),
                        seq: after,
                    },
                    timed_out: !done,
                    scanned_through: after,
                });
            }
            tokio::select! {
                r = rx.changed() => if r.is_err() { tokio::time::sleep(left).await },
                () = tokio::time::sleep(left) => {}
            }
        }
    }

    /// Domain config: merged `loom.toml [<domain>]` (project over user).
    pub fn domain_config(&self, domain: &str) -> Value {
        lock(&self.policy)
            .current
            .sections
            .get(domain)
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn record_activity(&self, c: &Caller, method: &str) {
        if c.role == Role::Agent {
            *lock(&self.activity)
                .entry((c.client_id.clone(), method.to_owned()))
                .or_insert(0) += 1;
        }
    }

    /// Dispatch one RPC (everything except `core.hello`/`core.subscribe`/`core.unsubscribe`, which
    /// the connection owns).
    pub async fn call(
        self: &Arc<Self>,
        c: &Caller,
        method: &str,
        p: Option<Value>,
    ) -> Result<Value, RpcError> {
        self.record_activity(c, method);
        if self.closing() && method != "core.status" {
            return Err(app_error(ErrorCode::Daemon, "the daemon is shutting down"));
        }
        match method {
            "core.time" => {
                let clock = &self.hub.clock;
                let recv = clock.mono_ns();
                Ok(
                    json!({"recv_mono_ns": U64(recv), "send_mono_ns": U64(clock.mono_ns()), "wall_ns": U64(clock.wall_ns()), "clock_id": clock.clock_id}),
                )
            }
            "core.status" => Ok(self.status().await),
            "core.viewer.presence" => {
                let v: loom_proto::stream::ViewerPresence = params(p)?;
                if c.role == Role::Viewer {
                    lock(&self.presence).insert(c.conn_id, v.visible);
                }
                Ok(json!({}))
            }
            "core.metrics.report" => {
                let v: loom_proto::stream::MetricsReport = params(p)?;
                *lock(&self.metrics) = serde_json::to_value(&v).unwrap_or(Value::Null);
                Ok(json!({}))
            }
            // STOP is safe-direction: every viewer (watch included), agents and scripts (UI Q3).
            "core.stop" => {
                let v: Value = params(p)?;
                Ok(self.stop(c, true, v["reason"].as_str()).await)
            }
            "core.events.query" => self.events_query(c, p).await,
            "core.channels.list" => {
                let st = self
                    .hub
                    .status()
                    .await
                    .map_err(|_| app_error(ErrorCode::Daemon, "hub stopped"))?;
                Ok(json!({"channels": st.channels}))
            }
            "core.worker.list" => {
                Ok(json!({"workers": self.workers().iter().map(|w| w.state()).collect::<Vec<_>>()}))
            }
            "core.worker.restart" => {
                if !c.viewer_at_least(Rights::Operate) {
                    return Err(app_error(
                        ErrorCode::Denied,
                        "restarting a worker needs a viewer with operate rights",
                    ));
                }
                let v: Value = params(p)?;
                let want = v["instance"].as_str().or_else(|| v["kind"].as_str());
                let mut done = Vec::new();
                for w in self.workers() {
                    let name = wname(w.as_ref());
                    if want.is_none_or(|k| k == w.kind() || k == name) {
                        w.restart();
                        done.push(name);
                    }
                }
                if done.is_empty() {
                    return Err(app_error(ErrorCode::NotFound, "no such worker"));
                }
                self.audit
                    .append_nowait("worker.restart", json!({"by": c.client_id, "workers": done}))
                    .await;
                Ok(json!({"restarted": done}))
            }
            "core.help" => self.help(p),
            "core.ui.focus" => self.ui_focus(c, p).await,
            "core.meta" => Ok(self.meta_rpc(c)),
            "core.shutdown" => self.shutdown_rpc(c),
            "core.stop.release" => {
                if !c.viewer_at_least(Rights::Operate) {
                    return Err(app_error(
                        ErrorCode::Denied,
                        "only a viewer with operate rights can release STOP",
                    ));
                }
                Ok(self.stop(c, false, None).await)
            }
            "core.safe_state.list" => Ok(serde_json::to_value(SafeStateList {
                items: self.safe_state_list(),
            })
            .unwrap_or(Value::Null)),
            "core.safe_state.run" => {
                let run: SafeStateRun = params(p)?;
                // Emergency OFF is safe-direction like STOP: any viewer (watch included). Recovery
                // sends need operate (contract 04 §4.4).
                let all_off =
                    !run.ids.is_empty() && run.ids.iter().all(|id| self.emergency_off(id).is_some());
                let ok = if all_off {
                    c.role == Role::Viewer
                } else {
                    c.viewer_at_least(Rights::Operate)
                };
                if !ok {
                    return Err(app_error(
                        ErrorCode::Denied,
                        "recovery send needs a viewer with operate rights (emergency OFF: any viewer)",
                    ));
                }
                Ok(self.safe_state_run(c, run).await)
            }
            "core.mark" => {
                let v: Value = params(p)?;
                let label = v["label"]
                    .as_str()
                    .ok_or_else(|| rpc_error(loom_proto::rpc::INVALID_PARAMS, "label is required"))?;
                Ok(self
                    .marker(
                        c,
                        kinds::CORE_MARK,
                        "core",
                        json!({"label": label, "note": v.get("note")}),
                    )
                    .await)
            }
            // Legacy step markers; with the TEST module loaded these go to its session engine
            // (fences, ordering, gates) instead of publishing a free-standing marker.
            "test.step.begin" | "test.step.end" if !self.domains.contains_key("test") => {
                let v: Value = params(p)?;
                let step = v["step"]
                    .as_str()
                    .or_else(|| v["name"].as_str())
                    .ok_or_else(|| rpc_error(loom_proto::rpc::INVALID_PARAMS, "step is required"))?;
                let kind = if method == "test.step.begin" {
                    kinds::TEST_STEP_STARTED
                } else {
                    kinds::TEST_STEP_ENDED
                };
                let mut payload = json!({"step": step});
                for k in ["note", "verdict", "test_session"] {
                    if let Some(x) = v.get(k) {
                        payload[k] = x.clone();
                    }
                }
                Ok(self.marker(c, kind, "test", payload).await)
            }
            m => {
                let Some((domain, _)) = m.split_once('.') else {
                    return Err(rpc_error(METHOD_NOT_FOUND, format!("method not found: {m}")));
                };
                let Some(d) = self.domains.get(domain) else {
                    return Err(rpc_error(METHOD_NOT_FOUND, format!("method not found: {m}")));
                };
                if m == format!("{domain}.input") && !d.commands.contains_key(m) {
                    return self.input(c, domain, p).await;
                }
                let Some(spec) = d.commands.get(m).cloned() else {
                    return Err(rpc_error(METHOD_NOT_FOUND, format!("method not found: {m}")));
                };
                let args = p.unwrap_or_else(|| json!({}));
                let preview_roles = (args.get("dry_run").and_then(Value::as_bool) == Some(true))
                    .then(|| d.dry_run_previews.get(m))
                    .flatten();
                if !c.allowed(preview_roles.unwrap_or(&spec.roles)) {
                    return Ok(self.envelope(
                        c,
                        m,
                        Err(err_body(
                            ErrorCode::Denied,
                            "this role may not call this command",
                            None,
                        )),
                        None,
                    ));
                }
                if preview_roles.is_some() {
                    return Ok(self.read(c, domain, &spec, args).await);
                }
                Ok(match spec.write_class {
                    None => self.read(c, domain, &spec, args).await,
                    Some(WriteClass::Request) => self.write_request(c, domain, &spec, args, false).await,
                    Some(_) => self.envelope(
                        c,
                        m,
                        Err(err_body(
                            ErrorCode::NotImplemented,
                            "job/interactive write classes arrive in M2",
                            None,
                        )),
                        None,
                    ),
                })
            }
        }
    }

    async fn status(&self) -> Value {
        let hub = self.hub.status().await.ok();
        let policy = {
            let p = lock(&self.policy);
            json!({"mode": "auto (F1)", "deny": p.current.deny, "actions": p.current.actions, "hash": p.current.hash, "warnings": p.current.warnings, "files": p.files})
        };
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "proto": PROTO,
            "pid": std::process::id(),
            "port": self.port(),
            "uptime_ms": u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "stream_epoch": U64(self.hub.epoch),
            "stream_seq_hw": U64(hub.as_ref().map_or(0, |h| h.hw)),
            "latch": self.latched(),
            "record_health": self.record_health(),
            "env": self.env(),
            "viewers": self.viewers(),
            "connections": hub.as_ref().map_or(0, |h| h.conns),
            "subscriptions": hub.as_ref().map_or(0, |h| h.subs),
            "domains": self.domains.iter().map(|(n, d)| json!({"name": n, "commands": d.commands.keys().collect::<BTreeSet<_>>()})).collect::<Vec<_>>(),
            "workers": self.workers().iter().map(|w| w.state()).collect::<Vec<_>>(),
            "policy": policy,
            "record": {"session": self.journal.root, "health": self.record_health()},
            "ui_metrics": *lock(&self.metrics),
        })
    }

    fn source(
        &self,
        c: &Caller,
        domain: &str,
        channel: Option<&str>,
        kind: &str,
        payload: Value,
    ) -> SourceRecord {
        let mut s = core_source(&self.hub.clock, domain, kind, payload);
        s.channel = channel.map(Into::into);
        s.origin = c.origin();
        s.client_id = Some(c.client_id.clone());
        s.env = Some(self.env());
        s
    }

    async fn marker(&self, c: &Caller, kind: &str, domain: &str, payload: Value) -> Value {
        let s = self.source(c, domain, None, kind, payload);
        let rec = self.hub.publish(vec![s]).await.unwrap_or_default();
        let seq = rec.first().map(|p| p.rec.seq);
        json!({"seq": seq, "stream_epoch": U64(self.hub.epoch)})
    }

    // ------------------------------------------------------------------ STOP

    /// STOP (`on`) / release. Order: memory latch → worker latches (ack ≤ 200 ms) → persist →
    /// `core.latch` (with `persisted`) → hub watermark.
    pub async fn stop(&self, c: &Caller, on: bool, reason: Option<&str>) -> Value {
        let _g = self.latch_lock.lock().await;
        self.latch.store(on, Ordering::Release);
        for domain in self.domains.values() {
            if let Some(hooks) = &domain.hooks {
                hooks.on_latch(on);
            }
        }
        // TEST sessions: STOP ends running attempts (no fence, no cleanup) and marks them STOPPED;
        // their gates already read the memory latch above. Release does not resume them.
        #[cfg(feature = "test")]
        if let Some(t) = &self.test {
            t.runtime.on_latch(c, on, &t.module);
        }
        let seq = self.latch_seq.fetch_add(1, Ordering::AcqRel) + 1;
        let (mut acked, mut missing) = (Vec::new(), Vec::new());
        let ws = self.workers();
        let acks = ws.iter().map(|w| {
            let l = Latch {
                on,
                scope: "all".into(),
                reason: Some(match reason {
                    Some(r) => format!("by {}: {r}", c.client_id),
                    None => format!("by {}", c.client_id),
                }),
                latch_seq: U64(seq),
            };
            async move {
                (
                    wname(w.as_ref()),
                    tokio::time::timeout(LATCH_ACK, w.latch(l)).await,
                )
            }
        });
        for (kind, r) in futures_util::future::join_all(acks).await {
            match r {
                Ok(Ok(())) => acked.push(kind),
                _ => missing.push(kind),
            }
        }
        let home = self.cfg.home.clone();
        let (by, origin) = (c.client_id.clone(), c.origin());
        let persisted = tokio::task::spawn_blocking(move || persist_latch(&home, on, &by, origin))
            .await
            .unwrap_or(false);
        let p = LatchPayload {
            on,
            by: c.client_id.clone(),
            origin: c.origin(),
            acked,
            missing,
            jobs_stopped: Vec::new(),
            approvals_cancelled: Vec::new(),
            uncancellable: Vec::new(),
            outputs_held: Vec::new(),
            persisted,
        };
        let payload = serde_json::to_value(&p).unwrap_or(Value::Null);
        let s = self.source(c, "core", None, kinds::CORE_LATCH, payload.clone());
        let rec = self.hub.publish(vec![s]).await.unwrap_or_default();
        self.audit
            .append_nowait(
                if on { "stop" } else { "stop.release" },
                json!({"by": c.client_id, "origin": c.origin(), "persisted": persisted, "reason": reason}),
            )
            .await;
        let mut out = payload;
        out["seq"] = json!(rec.first().map(|r| r.rec.seq));
        out
    }

    fn safe_state_list(&self) -> Vec<SafeStateItem> {
        self.domains
            .iter()
            .flat_map(|(dn, d)| {
                let blocked = d.interlock_active().is_some();
                let recover = d.safe_states.iter().map(move |s| SafeStateItem {
                    kind: SafeStateKind::Recover,
                    id: s.name.clone(),
                    domain: dn.clone(),
                    channel: s.args.get("channel").and_then(Value::as_str).map(str::to_owned),
                    method: s.method.clone(),
                    description: s.description.clone(),
                    blocked,
                });
                // Emergency OFF: interlocks only warn (contract 04 §4.4), so never `blocked`.
                let off = d.emergency_offs.iter().map(move |s| SafeStateItem {
                    kind: SafeStateKind::EmergencyOff,
                    id: s.name.clone(),
                    domain: dn.clone(),
                    channel: s.targets.first().cloned(),
                    method: s.method.clone(),
                    description: s.description.clone(),
                    blocked: false,
                });
                recover.chain(off)
            })
            .collect()
    }

    fn emergency_off(&self, id: &str) -> Option<(String, loom_domain_api::EmergencyOffSpec)> {
        self.domains.iter().find_map(|(dn, d)| {
            d.emergency_offs
                .iter()
                .find(|e| e.name == id)
                .map(|e| (dn.clone(), e.clone()))
        })
    }

    async fn safe_state_run(self: &Arc<Self>, c: &Caller, run: SafeStateRun) -> Value {
        let mut items = Vec::new();
        for id in run.ids {
            // Emergency OFF: the spec's method with its fixed params `{targets}`; no
            // confirmation, interlocks only warn (they are reported, not enforced).
            if let Some((dn, e)) = self.emergency_off(&id) {
                let d = self.domains.get(&dn);
                let warn = d.and_then(|d| d.interlock_active().map(str::to_owned));
                let Some(spec) = d.and_then(|d| d.commands.get(&e.method)).cloned() else {
                    items.push(SafeStateRunItem {
                        id,
                        outcome: SafeStateOutcome::Failed,
                        finally_blocked: false,
                        reason: Some(format!("{} not registered", e.method)),
                        request_id: None,
                    });
                    continue;
                };
                let env = self
                    .write_request(c, &dn, &spec, json!({"targets": e.targets}), true)
                    .await;
                let ok = env["ok"].as_bool() == Some(true);
                items.push(SafeStateRunItem {
                    id,
                    outcome: if ok {
                        SafeStateOutcome::Sent
                    } else {
                        SafeStateOutcome::Failed
                    },
                    finally_blocked: false,
                    reason: env["error"]["message"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| warn.map(|il| format!("warning: interlock {il} active"))),
                    request_id: env["data"]["request_id"]
                        .as_str()
                        .or_else(|| env["error"]["request_id"].as_str())
                        .map(str::to_owned),
                });
                continue;
            }
            let found = self.domains.iter().find_map(|(dn, d)| {
                d.safe_states
                    .iter()
                    .find(|s| s.name == id)
                    .map(|s| (dn.clone(), d, s.clone()))
            });
            let Some((dn, d, s)) = found else {
                items.push(SafeStateRunItem {
                    id,
                    outcome: SafeStateOutcome::Skipped,
                    finally_blocked: false,
                    reason: Some("unknown safe-state id".into()),
                    request_id: None,
                });
                continue;
            };
            if let Some(il) = d.interlock_active() {
                items.push(SafeStateRunItem {
                    id,
                    outcome: SafeStateOutcome::Skipped,
                    finally_blocked: true,
                    reason: Some(format!("interlock {il} active")),
                    request_id: None,
                });
                continue;
            }
            if let Some((_, msg, _)) = self.policy_block(&s.method) {
                items.push(SafeStateRunItem {
                    id,
                    outcome: SafeStateOutcome::Skipped,
                    finally_blocked: true,
                    reason: Some(msg),
                    request_id: None,
                });
                continue;
            }
            let Some(spec) = d.commands.get(&s.method).cloned() else {
                items.push(SafeStateRunItem {
                    id,
                    outcome: SafeStateOutcome::Failed,
                    finally_blocked: false,
                    reason: Some(format!("{} not registered", s.method)),
                    request_id: None,
                });
                continue;
            };
            // Recovery send: origin = user, latch exception for safe-state items only.
            let env = self.write_request(c, &dn, &spec, s.args.clone(), true).await;
            let ok = env["ok"].as_bool() == Some(true);
            items.push(SafeStateRunItem {
                id,
                outcome: if ok {
                    SafeStateOutcome::Sent
                } else {
                    SafeStateOutcome::Failed
                },
                finally_blocked: false,
                reason: env["error"]["message"].as_str().map(str::to_owned),
                request_id: env["data"]["request_id"]
                    .as_str()
                    .or_else(|| env["error"]["request_id"].as_str())
                    .map(str::to_owned),
            });
        }
        serde_json::to_value(SafeStateRunResult { items }).unwrap_or(Value::Null)
    }

    // ------------------------------------------------------------------ reads

    fn services(
        self: &Arc<Self>,
        c: &Caller,
        domain: &str,
        request_id: Option<String>,
        channel: Option<String>,
        method: &str,
    ) -> Arc<Services> {
        let pools = self
            .domains
            .get(domain)
            .map(|d| d.pools.clone())
            .unwrap_or_default();
        let outcome = Arc::new(Mutex::new(None));
        Arc::new(Services {
            d: self.clone(),
            domain: domain.to_owned(),
            caller: c.clone(),
            channel: channel.clone(),
            method: method.to_owned(),
            handles: pools
                .into_iter()
                .map(|pool| DomainWorker {
                    d: self.clone(),
                    pool,
                    channel: channel.clone(),
                    caller: c.clone(),
                    request_id: request_id.clone(),
                    outcome: outcome.clone(),
                    clock: self.hub.clock.clone(),
                })
                .collect(),
            outcome,
        })
    }

    async fn read(self: &Arc<Self>, c: &Caller, domain: &str, spec: &CommandSpec, args: Value) -> Value {
        let svc = self.services(c, domain, None, call_channel(&args), &spec.method);
        let mut ctx = CallCtx::new(
            c.role,
            c.rights,
            c.client_id.clone(),
            None,
            Instant::now() + spec.bounds.max_wait,
            svc,
        );
        let r = match tokio::time::timeout(spec.bounds.max_wait, spec.handler.call(&mut ctx, args)).await {
            Ok(r) => r,
            Err(_) => Err(DomainError::new(
                ErrorCode::Timeout,
                "command exceeded its max wait",
            )),
        };
        let r = r.map_err(|e| err_body(e.code, &e.message, Some(e.detail)));
        self.envelope(c, &spec.method, r, None)
    }

    // ------------------------------------------------------------------ write pipeline v0

    /// Request-class write (contract 04 §4.2). `recovery` = safe-state run (latch exception).
    async fn write_request(
        self: &Arc<Self>,
        c: &Caller,
        domain: &str,
        spec: &CommandSpec,
        mut args: Value,
        recovery: bool,
    ) -> Value {
        let key = args
            .as_object_mut()
            .and_then(|o| o.remove("_loom"))
            .and_then(|l| l["idempotency_key"].as_str().map(str::to_owned));
        if let Some(k) = &key {
            match self.idem.reserve(&c.client_id, k) {
                Reserve::Done(v) => return v,
                Reserve::Pending(mut rx) => {
                    let wait = rx.wait_for(Option::is_some);
                    return match tokio::time::timeout(spec.bounds.max_wait, wait).await {
                        Ok(Ok(v)) => v.clone().unwrap_or(Value::Null),
                        _ => self.envelope(
                            c,
                            &spec.method,
                            Err(err_body(
                                ErrorCode::Timeout,
                                "same idempotency key still in progress",
                                None,
                            )),
                            None,
                        ),
                    };
                }
                Reserve::New => {}
            }
        }
        let env = self.write_inner(c, domain, spec, args, recovery).await;
        if let Some(k) = &key {
            self.idem.complete(&c.client_id, k, env.clone());
        }
        env
    }

    async fn write_inner(
        self: &Arc<Self>,
        c: &Caller,
        domain: &str,
        spec: &CommandSpec,
        args: Value,
        recovery: bool,
    ) -> Value {
        let Some(d) = self.domains.get(domain) else {
            return Value::Null;
        };
        let rid = self.next("w");
        let channel = call_channel(&args);
        let row = |state: &str, risk: Option<Risk>, extra: Value| {
            let mut payload = json!({"write_state": state, "method": spec.method});
            if let (Some(o), Value::Object(e)) = (payload.as_object_mut(), extra) {
                o.extend(e);
            }
            let mut s = self.source(c, domain, channel.as_deref(), &d.write_kind, payload);
            s.request_id = Some(rid.clone());
            s.risk = risk;
            s.purpose = Some(if recovery {
                "recovery".into()
            } else {
                spec.method.clone()
            });
            s
        };
        drop(self.hub.publish(vec![row("requested", None, json!({}))]));

        // Global block: STOP (except recovery and user), recording failure (agent/script).
        let gated = matches!(c.origin(), Origin::Agent | Origin::Script);
        let fail = |code: ErrorCode, msg: String, risk: Option<Risk>, detail: Value| {
            let rid = rid.clone();
            let row = &row;
            async move {
                let mut s = row(
                    "failed",
                    risk,
                    json!({"final": true, "error": {"code": code, "message": msg}}),
                );
                s.payload["detail"] = detail.clone();
                drop(self.hub.publish(vec![s]));
                let published = self.hub.barrier(self.cfg.publish_deadline).await;
                self.audit.append_nowait("write.result", json!({"request_id": rid, "method": spec.method, "origin": c.origin(), "client_id": c.client_id, "state": "failed", "code": code})).await;
                let mut body = err_body(code, &msg, Some(detail));
                body.request_id = Some(rid.clone());
                if !published {
                    body.detail["published"] = json!(false);
                }
                self.envelope(c, &spec.method, Err(body), None)
            }
        };
        if self.latched() && c.origin() != Origin::User && !recovery {
            return fail(
                ErrorCode::Stopped,
                "STOP is latched — ask the user (only a viewer can release it)".into(),
                None,
                Value::Null,
            )
            .await;
        }
        // Session-bound agent/script clients: a paused/unauthorized/ending TEST session refuses
        // every domain write of its client (contract 07; viewers keep direct control).
        #[cfg(feature = "test")]
        if gated
            && !recovery
            && let Some(t) = &self.test
            && let Err(e) = t.module.check_client_write(&c.client_id, c.role)
        {
            return fail(
                e.code,
                format!("TEST session gate: {}", e.message),
                None,
                json!({"test_session_gate": true}),
            )
            .await;
        }
        self.policy_check().await;
        if gated && self.record_failed() {
            self.refresh_record("write refused: recording failed");
            return fail(
                ErrorCode::RecordingFailed,
                "recording failed (audit or journal cannot write) — agent/script writes are refused".into(),
                None,
                Value::Null,
            )
            .await;
        }

        // classify (timed)
        let target = TargetRef {
            channel: channel.clone(),
            target: args.get("target").and_then(Value::as_str).map(str::to_owned),
            generation: None,
        };
        let req = WriteRequest {
            domain,
            method: &spec.method,
            target: &target,
            bytes: None,
            args: &args,
            origin: c.origin(),
            client_id: &c.client_id,
            env: self.env(),
            write_class: WriteClass::Request,
            job: None,
        };
        let t0 = Instant::now();
        let class = d.hooks.as_ref().map(|h| h.classify(&req));
        let took = t0.elapsed();
        if took > Duration::from_millis(1) {
            tracing::warn!(method = %spec.method, ?took, "classify hook exceeded 1 ms");
        }
        let risk = class.as_ref().map_or(Risk::R1, |k| k.risk);
        if let Some(b) = class.as_ref().and_then(|k| k.block) {
            return fail(
                ErrorCode::Denied,
                format!(
                    "blocked by domain: {}",
                    class.as_ref().map_or("", |k| k.reason.as_str())
                ),
                Some(risk),
                json!({"block": b}),
            )
            .await;
        }
        // policy (F1: automatic unless the user's files say otherwise)
        if let Some((pat, msg, detail)) = self.policy_block(&spec.method) {
            let mut s = self.source(
                c,
                "core",
                channel.as_deref(),
                kinds::CORE_POLICY_DENIED,
                json!({"method": spec.method, "pattern": pat, "request_id": rid}),
            );
            s.request_id = Some(rid.clone());
            drop(self.hub.publish(vec![s]));
            return fail(ErrorCode::Denied, msg, Some(risk), detail).await;
        }
        // audit intent (R2+ synced before sending)
        let intent = json!({"request_id": rid, "method": spec.method, "domain": domain, "channel": channel, "origin": c.origin(), "client_id": c.client_id, "risk": risk, "args": args});
        if let Err(e) = self.audit.append("write.intent", intent, risk >= Risk::R2).await {
            self.report_record_failed(&e.0);
            if gated {
                return fail(
                    ErrorCode::RecordingFailed,
                    format!("recording failed: {}", e.0),
                    Some(risk),
                    Value::Null,
                )
                .await;
            }
        }
        drop(self.hub.publish(vec![row(
            "accepted",
            Some(risk),
            json!({"reason": class.as_ref().map(|k| k.reason.clone())}),
        )]));
        // a quarantined worker takes no writes (contract 05 §5.5): E_WORKER_DOWN{quarantined}
        let target = d
            .pools
            .iter()
            .filter_map(|p| p.existing(channel.as_deref()))
            .find(|w| w.state()["state"] == "quarantined");
        if let Some(w) = target {
            let st = w.state();
            return fail(
                ErrorCode::WorkerDown,
                format!(
                    "worker {} is quarantined after repeated crashes — a viewer can restart it",
                    w.kind()
                ),
                Some(risk),
                json!({"quarantined": true, "worker": st}),
            )
            .await;
        }
        // presend
        if let Some(h) = &d.hooks
            && let Err(e) = h.presend_check(&req, None)
        {
            return fail(e.code(), e.to_string(), Some(risk), Value::Null).await;
        }
        // backend
        let terminal = d
            .terminals
            .get(&spec.method)
            .copied()
            .unwrap_or(WriteState::Confirmed);
        self.pending.begin(&rid, terminal);
        let svc = self.services(c, domain, Some(rid.clone()), channel.clone(), &spec.method);
        let mut ctx = CallCtx::new(
            c.role,
            c.rights,
            c.client_id.clone(),
            Some(rid.clone()),
            Instant::now() + spec.bounds.max_wait,
            svc.clone(),
        );
        let r = match tokio::time::timeout(spec.bounds.max_wait, spec.handler.call(&mut ctx, args)).await {
            Ok(r) => r,
            Err(_) => Err(
                DomainError::new(ErrorCode::Timeout, "write exceeded its max wait")
                    .with_detail(json!({"write_state": "outcome_unknown"})),
            ),
        };
        let outcome = lock(&svc.outcome).take();
        let final_seen = self.pending.end(&rid);
        let state = match (&r, &outcome) {
            (_, Some(o)) => o.state,
            (Ok(_), None) => terminal,
            (Err(e), None) => payload_write_state(&e.detail)
                .ok()
                .flatten()
                .unwrap_or(WriteState::Failed),
        };
        if !final_seen {
            let extra = match &r {
                Ok(_) => json!({"final": true}),
                Err(e) => json!({"final": true, "error": {"code": e.code, "message": e.message}}),
            };
            drop(self.hub.publish(vec![row(
                    &serde_json::to_value(state)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "failed".into()),
                    Some(risk),
                    extra,
                )]));
        }
        // publish-before-reply
        let published = self.hub.barrier(self.cfg.publish_deadline).await;
        self.audit
            .append_nowait("write.result", json!({"request_id": rid, "method": spec.method, "origin": c.origin(), "client_id": c.client_id, "state": state, "published": published}))
            .await;
        match r {
            Ok(mut data) => {
                if let Some(o) = data.as_object_mut() {
                    o.insert("request_id".into(), json!(rid));
                    o.entry("state").or_insert(json!(state));
                }
                if published {
                    self.envelope(c, &spec.method, Ok(data), None)
                } else {
                    // The device acted; only the screen guarantee is missing. Not `failed`, no retry.
                    let mut b = err_body(
                        ErrorCode::Partial,
                        "result not yet visible to viewers (hub stalled) — do not retry",
                        Some(json!({"published": false, "write_state": state})),
                    );
                    b.request_id = Some(rid.clone());
                    self.envelope(c, &spec.method, Err(b), Some(data))
                }
            }
            Err(e) => {
                let mut b = err_body(e.code, &e.message, Some(e.detail));
                b.request_id = Some(rid.clone());
                if !published {
                    b.detail["published"] = json!(false);
                }
                self.envelope(c, &spec.method, Err(b), None)
            }
        }
    }

    // ------------------------------------------------------------------ interactive input

    /// `<domain>.input` (viewer operate): straight to the worker (no request queue), lease taken
    /// once per connection+domain, audited in batches (line / 1 s / 4 KiB).
    async fn input(self: &Arc<Self>, c: &Caller, domain: &str, p: Option<Value>) -> Result<Value, RpcError> {
        if !c.viewer_at_least(Rights::Operate) {
            return Err(app_error(
                ErrorCode::Denied,
                "interactive input needs a viewer with operate rights",
            ));
        }
        let v: Value = params(p)?;
        let bytes: Vec<u8> = if let Some(t) = v["text"].as_str() {
            t.as_bytes().to_vec()
        } else if let Some(h) = v["hex"].as_str().or_else(|| v["data_hex"].as_str()) {
            loom_proto::bytes::decode_hex(h)
                .map_err(|e| rpc_error(loom_proto::rpc::INVALID_PARAMS, e.to_string()))?
        } else {
            return Err(rpc_error(
                loom_proto::rpc::INVALID_PARAMS,
                "text or hex is required",
            ));
        };
        let input_seq = match v.get("input_seq") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(U64(loom_proto::num::parse_u64_canonical(s)
                .map_err(|e| rpc_error(loom_proto::rpc::INVALID_PARAMS, e.to_string()))?)),
            Some(n) => Some(U64(n.as_u64().ok_or_else(|| {
                rpc_error(
                    loom_proto::rpc::INVALID_PARAMS,
                    "input_seq must be a uint64 decimal string or integer",
                )
            })?)),
        };
        let channel = call_channel(&v);
        let Some(pool) = self.domains.get(domain).and_then(|d| d.pools.first()).cloned() else {
            return Err(app_error(ErrorCode::WorkerDown, "domain has no worker"));
        };
        let w = self
            .instance(&pool, channel.as_deref())
            .await
            .map_err(|e| app_error_detail(e.code, &e.message, e.detail))?;
        let lease = lock(&self.leases)
            .entry((c.conn_id, wname(w.as_ref())))
            .or_insert_with(|| (format!("lease-{}-{}", c.conn_id, wname(w.as_ref())), w.clone()))
            .0
            .clone();
        let n = bytes.len();
        let r = w
            .input(Input {
                conn_id: Some(format!("c{}", c.conn_id)),
                channel: channel.clone(),
                input_seq,
                call_id: None,
                lease_id: lease,
                bytes: Bytes(bytes.clone()),
                // Ctrl-C (0x03) jumps the queue (uart 8): the worker's priority mailbox.
                priority: v["priority"].as_bool().unwrap_or(false) || bytes.contains(&0x03),
            })
            .await;
        if let Err(e) = r {
            return Err(app_error_detail(e.code, &e.message, e.detail));
        }
        // `Registry::input_events(Owner::Domain)`: the worker publishes its own input events.
        let core_owned = self
            .domains
            .get(domain)
            .is_none_or(|d| d.input_owner == loom_domain_api::Owner::Core);
        let rec = if core_owned {
            let mut s = self.source(
                c,
                domain,
                channel.as_deref(),
                &format!("{domain}.input"),
                json!({"write_state": "sent", "final": true, "len": n}),
            );
            s.dir = Some(Dir::Tx);
            s.purpose = Some("user_input".into());
            self.hub.publish(vec![s]).await.unwrap_or_default()
        } else {
            Vec::new()
        };
        self.batch_input(c, domain, channel, &bytes);
        Ok(json!({"sent": n, "seq": rec.first().map(|r| r.rec.seq)}))
    }

    fn batch_input(self: &Arc<Self>, c: &Caller, domain: &str, channel: Option<String>, bytes: &[u8]) {
        let key = (c.conn_id, domain.to_owned());
        let flush_now = {
            let mut m = lock(&self.inputs);
            let b = m.entry(key.clone()).or_insert_with(|| InputBatch {
                domain: domain.to_owned(),
                channel,
                client_id: c.client_id.clone(),
                ..InputBatch::default()
            });
            let first = b.first.is_none();
            if first {
                b.first = Some(Instant::now());
                let d = Arc::downgrade(self);
                let k = key.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    if let Some(d) = d.upgrade() {
                        let b = lock(&d.inputs).remove(&k);
                        if let Some(b) = b {
                            d.flush_input(b);
                        }
                    }
                });
            }
            b.text.extend_from_slice(bytes);
            if bytes.contains(&b'\n') || bytes.contains(&b'\r') || b.text.len() >= 4096 {
                m.remove(&key)
            } else {
                None
            }
        };
        if let Some(b) = flush_now {
            self.flush_input(b);
        }
    }

    fn flush_input(&self, b: InputBatch) {
        if b.text.is_empty() {
            return;
        }
        let audit = self.audit.clone();
        let rec = json!({"domain": b.domain, "channel": b.channel, "client_id": b.client_id, "origin": "user", "len": b.text.len(), "hex": hex::encode(&b.text)});
        tokio::spawn(async move { audit.append_nowait("input.batch", rec).await });
    }
}

/// `(conn, worker instance)` → `(lease id, worker)`.
type LeaseMap = HashMap<(u64, String), (String, Arc<dyn WorkerHost>)>;

/// Instance id of a worker (`state().instance`, else its kind).
fn wname(w: &dyn WorkerHost) -> String {
    w.state()["instance"]
        .as_str()
        .map_or_else(|| w.kind().to_owned(), str::to_owned)
}

/// The channel a call is about: `params.channel`, `ch`, or `alias` (the channel an `open`
/// creates — uart `open --alias dut`). PerChannel workers are routed by it.
fn call_channel(args: &Value) -> Option<String> {
    ["channel", "ch", "alias"]
        .iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
}

fn err_body(code: ErrorCode, msg: &str, detail: Option<Value>) -> ErrorBody {
    ErrorBody {
        code,
        message: msg.into(),
        hint: None,
        retryable: false,
        request_id: None,
        detail: detail.unwrap_or(Value::Null),
    }
}

/// `CtxServices` for one call.
struct Services {
    d: Arc<Daemon>,
    domain: String,
    caller: Caller,
    /// `params.channel` (or `ch`) of the call.
    channel: Option<String>,
    /// RPC method being served.
    method: String,
    handles: Vec<DomainWorker>,
    outcome: Arc<Mutex<Option<WriteOutcome>>>,
}

struct DomainWorker {
    d: Arc<Daemon>,
    pool: Arc<WorkerPool>,
    channel: Option<String>,
    caller: Caller,
    request_id: Option<String>,
    outcome: Arc<Mutex<Option<WriteOutcome>>>,
    clock: Arc<crate::hub::HubClock>,
}

impl DomainWorker {
    async fn host(&self) -> Result<Arc<dyn WorkerHost>, DomainError> {
        self.d.instance(&self.pool, self.channel.as_deref()).await
    }

    fn deadline_ns(&self, deadline: Instant) -> U64 {
        let left =
            u64::try_from(deadline.saturating_duration_since(Instant::now()).as_nanos()).unwrap_or(u64::MAX);
        U64(self.clock.mono_ns().saturating_add(left))
    }
}

impl WorkerHandle for DomainWorker {
    fn call<'a>(
        &'a self,
        method: &'a str,
        args: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let call = Call {
                // the viewer connection, so the worker can tie the call to that viewer's lease
                conn_id: (self.caller.role == Role::Viewer).then(|| format!("c{}", self.caller.conn_id)),
                call_id: U64(0),
                method: method.into(),
                args,
                origin: self.caller.origin(),
                client_id: Some(self.caller.client_id.clone()),
                actor_id: None,
                request_id: self.request_id.clone(),
                deadline_mono_ns: self.deadline_ns(deadline),
                approval_ref: None,
            };
            self.host()
                .await?
                .call(call)
                .await
                .map_err(|e| DomainError::new(e.code, e.message).with_detail(e.detail))
        })
    }

    fn write<'a>(
        &'a self,
        request_id: &'a str,
        op: Value,
        deadline: Instant,
    ) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let w = Write {
                call_id: U64(0),
                request_id: request_id.into(),
                write_class: WriteClass::Request,
                op,
                origin: self.caller.origin(),
                client_id: Some(self.caller.client_id.clone()),
                plan_digest: None,
                approval_ref: None,
                deadline_mono_ns: self.deadline_ns(deadline),
            };
            let o = match self.host().await {
                Ok(h) => h.write(w).await,
                Err(e) => WriteOutcome {
                    state: WriteState::Failed,
                    detail: json!({"sent": false}),
                    error: Some(loom_proto::worker::WorkerError {
                        code: e.code,
                        message: e.message,
                        detail: e.detail,
                    }),
                },
            };
            *lock(&self.outcome) = Some(o.clone());
            let st = serde_json::to_value(o.state).unwrap_or(Value::Null);
            match o.state {
                WriteState::Failed | WriteState::OutcomeUnknown => {
                    let e = o.error.unwrap_or(loom_proto::worker::WorkerError {
                        code: ErrorCode::Internal,
                        message: "write failed".into(),
                        detail: Value::Null,
                    });
                    let mut detail = if e.detail.is_object() { e.detail } else { json!({}) };
                    detail["write_state"] = st;
                    Err(DomainError::new(e.code, e.message).with_detail(detail))
                }
                _ => {
                    let mut v = if o.detail.is_object() { o.detail } else { json!({}) };
                    v["state"] = st;
                    Ok(v)
                }
            }
        })
    }
}

impl CtxServices for Services {
    fn worker(&self, kind: &str) -> Result<&dyn WorkerHandle, DomainError> {
        self.handles
            .iter()
            .find(|h| h.pool.kind == kind)
            .map(|h| h as &dyn WorkerHandle)
            .ok_or_else(|| DomainError::new(ErrorCode::WorkerDown, format!("worker {kind} not registered")))
    }

    fn wait<'a>(&'a self, spec: &'a WaitSpec) -> BoxFuture<'a, Result<WaitResult, DomainError>> {
        Box::pin(async move { self.d.wait(spec).await })
    }

    fn now_mono_ns(&self) -> u64 {
        self.d.hub.clock.mono_ns()
    }

    fn run_blocking(&self, f: Box<dyn FnOnce() + Send>) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            let _ = tokio::task::spawn_blocking(f).await;
        })
    }

    fn config(&self) -> Value {
        self.d.domain_config(&self.domain)
    }

    fn activity(
        &self,
        action: &str,
        range: Option<loom_proto::event::SeqRange>,
        predicate: Option<Value>,
    ) -> Result<(), DomainError> {
        let p = ActivityPayload {
            action: Some(action.to_owned()),
            domain: Some(self.domain.clone()),
            channel: self.channel.clone(),
            client_id: self.caller.client_id.clone(),
            method: self.method.clone(),
            summary: action.to_owned(),
            range,
            waiting: predicate,
        };
        let mut s = self.d.source(
            &self.caller,
            "core",
            None,
            kinds::CORE_ACTIVITY,
            serde_json::to_value(p).unwrap_or(Value::Null),
        );
        s.actor_id = None;
        drop(self.d.hub.publish(vec![s]));
        Ok(())
    }

    fn worker_for(&self, kind: &str, channel: &str) -> Result<Box<dyn WorkerHandle + '_>, DomainError> {
        let h = self.handles.iter().find(|h| h.pool.kind == kind).ok_or_else(|| {
            DomainError::new(ErrorCode::WorkerDown, format!("worker {kind} not registered"))
        })?;
        Ok(Box::new(DomainWorker {
            d: h.d.clone(),
            pool: h.pool.clone(),
            channel: Some(channel.to_owned()),
            caller: h.caller.clone(),
            request_id: h.request_id.clone(),
            outcome: h.outcome.clone(),
            clock: h.clock.clone(),
        }))
    }

    fn publish(&self, draft: EventDraft) -> Result<(), DomainError> {
        if !draft.kind.starts_with(&format!("{}.", self.domain)) {
            return Err(DomainError::new(
                ErrorCode::Usage,
                format!("{} is not a {} event", draft.kind, self.domain),
            ));
        }
        let mut s = self.d.source(
            &self.caller,
            &self.domain,
            draft.channel.as_deref(),
            &draft.kind,
            draft.payload,
        );
        s.producer_id = format!("gateway.{}", self.domain);
        s.dir = draft.dir;
        // Gateway-side transports (e.g. DoIP) receive bytes on behalf of a caller. The
        // request stays linked, but the received bytes came from the device, not that caller.
        if s.dir == Some(loom_proto::event::Dir::Rx) {
            s.origin = Origin::Device;
        }
        s.request_id = draft.request_id;
        s.job_id = draft.job_id;
        s.generation = draft.generation;
        s.purpose = draft.purpose;
        s.risk = draft.risk;
        if draft.env.is_some() {
            s.env = draft.env;
        }
        s.metrics = draft.metrics;
        s.quality = draft.quality;
        self.d.pending.mark(&mut s);
        drop(self.d.hub.publish(vec![s]));
        Ok(())
    }

    fn journal(&self) -> &dyn JournalReader {
        self
    }

    fn approvals(&self) -> &dyn ApprovalHandle {
        self
    }
}

/// M1 journal = the hub ring (durable journal queries attach with `loom-journal`).
impl JournalReader for Services {
    fn scan<'a>(
        &'a self,
        q: &'a JournalQuery,
        byte_limit: u64,
    ) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
        Box::pin(async move {
            let mut filter: SubFilter = q.filter.clone();
            if filter.domains.is_empty() {
                filter.domains = vec![self.domain.clone()];
            }
            let max = usize::try_from(q.max_events).unwrap_or(1000).min(1000);
            // A seq cursor older than the ring (or from an earlier run), or at/below an event the
            // ring evicted from a channel this filter selects, reads the recording.
            if let Cursor::Seq { epoch, seq } = &q.from {
                let e = epoch.unwrap_or(self.d.hub.epoch);
                let ring = self
                    .d
                    .hub
                    .range(crate::hub::RangeQuery {
                        filter: filter.clone(),
                        max: 0,
                        ..Default::default()
                    })
                    .await
                    .map_err(|_| DomainError::new(ErrorCode::Daemon, "hub stopped"))?;
                if e != self.d.hub.epoch
                    || ring.oldest.is_none_or(|o| *seq < o)
                    || ring.evicted_through.is_some_and(|ev| *seq <= ev)
                {
                    let to = match &q.until {
                        Some(Cursor::Seq { seq, .. }) => *seq,
                        _ => u64::MAX,
                    };
                    let jr = self
                        .d
                        .journal_read_bounded(e, *seq, to, &filter, max, byte_limit)
                        .await
                        .map_err(|err| {
                            let b = crate::rpcutil::error_body(&err);
                            DomainError::new(b.code, b.message).with_detail(b.detail)
                        })?;
                    let mut events = jr.events;
                    if let Some(rid) = &q.request_id {
                        events.retain(|e| e.request_id.as_ref() == Some(rid));
                    }
                    let gaps: Vec<loom_proto::event::GapData> = jr
                        .unreadable
                        .iter()
                        .map(|(a, b)| ring_gap(*a, *b, "producer_not_recorded"))
                        .collect();
                    return Ok(ScanResult {
                        events,
                        complete: jr.next_seq.is_none() && gaps.is_empty(),
                        gaps,
                        resume_from: jr.next_seq.map(|n| Cursor::Seq {
                            epoch: Some(e),
                            seq: n,
                        }),
                        scanned_through: jr.scanned_through,
                        scanned_bytes: jr.scanned_bytes,
                    });
                }
            }
            let hw = self.d.hw();
            let mut range = crate::hub::RangeQuery {
                filter,
                max: 1001,
                ..Default::default()
            };
            match &q.from {
                Cursor::Seq { seq, .. } => range.after = Some(seq.saturating_sub(1)),
                Cursor::Now => range.after = Some(hw),
                Cursor::Last => {
                    range.after = lock(&self.d.event_cursors)
                        .get(client_key(&self.caller.client_id))
                        .copied()
                }
                Cursor::Mark(label) => {
                    range.after = Some(
                        self.d
                            .mark_seq(label)
                            .await
                            .map_err(|_| DomainError::new(ErrorCode::CursorGap, "mark not in ring"))?
                            .saturating_sub(1),
                    )
                }
                cur if cur.relative_duration().is_some() => {
                    range.since_at = Instant::now().checked_sub(cur.relative_duration().unwrap_or_default());
                }
                _ => return Err(DomainError::not_implemented("journal cursor")),
            }
            match &q.until {
                Some(Cursor::Seq { epoch, seq }) => {
                    if epoch.is_some_and(|e| e != self.d.hub.epoch) {
                        return Err(DomainError::new(
                            ErrorCode::CursorGap,
                            "until epoch differs from live stream",
                        ));
                    }
                    range.until = Some(*seq);
                }
                Some(Cursor::Now | Cursor::Last) | None => range.until = Some(hw),
                Some(Cursor::Mark(label)) => {
                    range.until = Some(
                        self.d
                            .mark_seq(label)
                            .await
                            .map_err(|_| DomainError::new(ErrorCode::CursorGap, "mark not in ring"))?,
                    )
                }
                Some(cur) if cur.relative_duration().is_some() => {
                    range.until_at = Instant::now().checked_sub(cur.relative_duration().unwrap_or_default());
                }
                _ => return Err(DomainError::not_implemented("journal until cursor")),
            }
            let upper = range.until.unwrap_or(hw).min(hw);
            let mut scanned_through = range.after.unwrap_or(0);
            let (after, since_at) = (range.after, range.since_at);
            let rows = self
                .d
                .hub
                .range(range)
                .await
                .map_err(|_| DomainError::new(ErrorCode::Daemon, "hub stopped"))?;
            let mut out = Vec::new();
            let mut bytes = 0u64;
            let mut resume_from = None;
            for e in &rows.events {
                let size = e.json.len() as u64;
                if bytes.saturating_add(size) > byte_limit || out.len() >= max {
                    resume_from = Some(Cursor::Seq {
                        epoch: Some(self.d.hub.epoch),
                        seq: e.rec.seq.get(),
                    });
                    break;
                }
                bytes += size;
                scanned_through = e.rec.seq.get();
                if q.request_id
                    .as_ref()
                    .is_none_or(|rid| e.rec.request_id.as_ref() == Some(rid))
                {
                    out.push(e.rec.clone());
                }
            }
            if resume_from.is_none() && rows.more {
                resume_from = Some(Cursor::Seq {
                    epoch: Some(self.d.hub.epoch),
                    seq: scanned_through.saturating_add(1),
                });
            }
            if resume_from.is_none() {
                scanned_through = upper;
            }
            // The live ring evicted events of a selected channel inside the requested range: the
            // read is missing them. Say so (gap + incomplete) instead of claiming a full window.
            let lost = match (rows.evicted_through, rows.evicted_at) {
                (Some(ev), Some(at)) => {
                    after.is_some_and(|a| ev > a)
                        || since_at.is_some_and(|t| at >= t)
                        || (after.is_none() && since_at.is_none())
                }
                _ => false,
            };
            let gaps = if lost {
                let from = after.map_or(0, |a| a.saturating_add(1));
                vec![ring_gap(
                    from,
                    rows.evicted_through.unwrap_or(from),
                    "ring_evicted",
                )]
            } else {
                Vec::new()
            };
            Ok(ScanResult {
                events: out,
                complete: resume_from.is_none() && gaps.is_empty(),
                gaps,
                resume_from,
                scanned_bytes: bytes,
                scanned_through,
            })
        })
    }
}

/// A scan loss over stream seqs `from..=to` (count unknown).
fn ring_gap(from: u64, to: u64, cause: &str) -> loom_proto::event::GapData {
    loom_proto::event::GapData {
        seq_range: Some(loom_proto::event::SeqRange {
            from: U64(from),
            to: U64(to.max(from)),
        }),
        producer_range: None,
        count: loom_proto::event::GapCount::Unknown,
        cause: cause.into(),
        bytes: None,
        detail: Value::Null,
    }
}

/// F1: approvals are off by default; nothing to invalidate.
impl ApprovalHandle for Services {
    fn invalidate(&self, _approval_id: &str, _reason: &str) -> Result<(), DomainError> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod r1_scan_tests {
    use super::*;

    fn scan_caller() -> Caller {
        Caller {
            role: Role::Agent,
            rights: None,
            client_id: "scan-evict".into(),
            conn_id: 0,
        }
    }

    /// Publish `n` ~8 KiB gateway records on `echo/<channel>`; returns the first stream seq.
    async fn flood(d: &Daemon, channel: &str, n: usize) -> u64 {
        let mut first = None;
        for i in 0..n {
            let mut s = core_source(
                &d.hub.clock,
                "echo",
                "echo.evict",
                json!({"n": i, "pad": "x".repeat(8192)}),
            );
            s.channel = Some(channel.into());
            let p = d.hub.publish(vec![s]).await.unwrap();
            first.get_or_insert(p[0].rec.seq.get());
        }
        assert!(d.hub.barrier(Duration::from_secs(5)).await);
        first.unwrap()
    }

    fn evict_query(from: Cursor, channel: &str) -> JournalQuery {
        JournalQuery {
            from,
            until: None,
            filter: SubFilter {
                domains: vec!["echo".into()],
                channels: vec![channel.into()],
                kinds: vec!["echo.evict".into()],
                ..Default::default()
            },
            request_id: None,
            max_events: 10_000,
        }
    }

    /// Per-channel ring eviction (4 MiB) inside a requested range is never reported as a
    /// complete live read: seq cursors read the recording, time cursors report `ring_evicted`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn live_scan_reports_ring_eviction() {
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.workers = Some(WorkersMode::Inproc);
        let running = start(cfg).await.unwrap();
        let d = &running.daemon;
        let small = flood(d, "small", 1).await;
        let first = flood(d, "big", 700).await; // ~5.6 MiB > RING_CHANNEL_BYTES
        let ring = d
            .hub
            .range(crate::hub::RangeQuery {
                filter: evict_query(Cursor::Now, "big").filter,
                max: 0,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            ring.evicted_through.is_some_and(|e| e >= first),
            "{:?}",
            ring.evicted_through
        );
        d.journal.flush(Duration::from_secs(5));
        let svc = d.services(&scan_caller(), "echo", None, None, "echo.scan");

        // Seq cursor at the evicted start: served by the durable journal, complete.
        let mut q = evict_query(
            Cursor::Seq {
                epoch: Some(d.hub.epoch),
                seq: first,
            },
            "big",
        );
        q.until = Some(Cursor::Seq {
            epoch: Some(d.hub.epoch),
            seq: first,
        });
        let r = svc.scan(&q, 64 << 20).await.unwrap();
        assert_eq!(r.events.len(), 1, "{r:?}");
        assert_eq!(r.events[0].seq.get(), first);
        assert!(r.complete && r.gaps.is_empty());

        // Time cursor over the evicted range: explicit gap, not complete.
        let rel = Cursor::Relative {
            amount: 60_000,
            unit: loom_proto::cursor::TimeUnit::Millis,
        };
        let r = svc
            .scan(&evict_query(rel.clone(), "big"), 64 << 20)
            .await
            .unwrap();
        assert!(!r.complete, "evicted window must not be complete");
        assert_eq!(r.gaps.len(), 1, "{:?}", r.gaps);
        assert_eq!(r.gaps[0].cause, "ring_evicted");

        // An unrelated channel's ring was not evicted: its live read stays complete.
        let r = svc.scan(&evict_query(rel, "small"), 64 << 20).await.unwrap();
        assert!(r.complete && r.gaps.is_empty(), "{r:?}");
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.events[0].seq.get(), small);
    }

    /// A mapped range whose producer journal is gone is reported, never skipped silently: both
    /// in the journal read and in `Services::scan` (durable path for an evicted range).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_producer_journal_is_unreadable_not_empty() {
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.workers = Some(WorkersMode::Inproc);
        let running = start(cfg).await.unwrap();
        let d = &running.daemon;
        let first = flood(d, "gone", 700).await; // evicted from the live ring
        d.journal.flush(Duration::from_secs(5));
        let filter = evict_query(Cursor::Now, "gone").filter;
        let rec = d
            .journal_read_bounded(d.hub.epoch, first, first, &filter, 10, 1 << 20)
            .await
            .unwrap();
        assert_eq!(rec.events.len(), 1);
        assert!(rec.unreadable.is_empty());
        let epoch = rec.events[0].producer_epoch.get();
        let dir = loom_journal::producer_dir(&d.journal.root, "gateway", epoch);
        std::fs::remove_dir_all(&dir).unwrap();

        let rec = d
            .journal_read_bounded(d.hub.epoch, first, first, &filter, 10, 1 << 20)
            .await
            .unwrap();
        assert!(rec.events.is_empty());
        assert_eq!(rec.unreadable, vec![(first, first)]);

        let svc = d.services(&scan_caller(), "echo", None, None, "echo.scan");
        let at = Cursor::Seq {
            epoch: Some(d.hub.epoch),
            seq: first,
        };
        let mut q = evict_query(at.clone(), "gone");
        q.until = Some(at);
        let r = svc.scan(&q, 64 << 20).await.unwrap();
        assert!(r.events.is_empty());
        assert!(!r.complete, "unreadable producer range must not be complete");
        assert_eq!(r.gaps.len(), 1, "{:?}", r.gaps);
        assert_eq!(r.gaps[0].cause, "producer_not_recorded");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn journal_scan_honors_inclusive_range_and_byte_budget() {
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::new(Home::at(home.path()));
        cfg.port = Some(0);
        cfg.workers = Some(WorkersMode::Inproc);
        let running = start(cfg).await.unwrap();
        let d = &running.daemon;
        let caller = Caller {
            role: Role::Agent,
            rights: None,
            client_id: "scan-r1".into(),
            conn_id: 0,
        };
        for n in 0..3 {
            let mut source = core_source(&d.hub.clock, "echo", "echo.scan_r1", json!({"n": n}));
            source.request_id = Some("scan-r1".into());
            drop(d.hub.publish(vec![source]));
        }
        let rows = d
            .hub
            .range(crate::hub::RangeQuery {
                filter: SubFilter {
                    kinds: vec!["echo.scan_r1".into()],
                    ..Default::default()
                },
                max: 3,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(rows.events.len(), 3);
        let from = rows.events[1].rec.seq.get();
        let services = d.services(&caller, "echo", None, None, "echo.scan");
        let q = JournalQuery {
            from: Cursor::Seq {
                epoch: Some(d.hub.epoch),
                seq: from,
            },
            until: Some(Cursor::Seq {
                epoch: Some(d.hub.epoch),
                seq: from,
            }),
            filter: SubFilter {
                kinds: vec!["echo.scan_r1".into()],
                ..Default::default()
            },
            request_id: Some("scan-r1".into()),
            max_events: 10,
        };
        let result = services.scan(&q, 100_000).await.unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].seq.get(), from);
        assert_eq!(result.scanned_through, from);
        assert!(result.complete);
        let limited = services.scan(&q, 0).await.unwrap();
        assert!(limited.events.is_empty());
        assert!(!limited.complete);
        assert_eq!(limited.scanned_bytes, 0);
        assert_eq!(limited.resume_from, Some(q.from.clone()));
        d.journal.flush(Duration::from_secs(2));
        let recorded = d
            .journal_read_bounded(d.hub.epoch, from, from, &q.filter, 10, 100_000)
            .await
            .unwrap();
        assert_eq!(recorded.events.len(), 1);
        assert_eq!(recorded.scanned_through, from);
        let mut meta = Meta::default();
        let mut payload =
            json!({"value": 1, "_meta": {"next_cursor": format!("seq:{}", from + 1), "truncated": true}});
        d.promote_meta(&caller, &mut payload, &mut meta);
        assert!(payload.get("_meta").is_none());
        assert!(meta.truncated);
        assert_eq!(lock(&d.event_cursors).get("scan-r1"), Some(&from));
        d.shutdown_gracefully().await;
        running.server.abort();
    }
}
