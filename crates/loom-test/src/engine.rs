use crate::*;
use loom_domain_api::{CallCtx, DomainError, DomainModule, EventDraft, RegisterError, Registry};
use loom_proto::{
    error::ErrorCode,
    event::Env,
    num::U64,
    rpc::{Rights, Role},
};
use loom_test_format::{self as format, FailureAction, Plan, TcDoc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub max_sessions: usize,
    pub approval: ApprovalMode,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1,
            approval: ApprovalMode::Auto,
        }
    }
}
#[derive(Debug)]
struct Charge {
    used: Arc<AtomicUsize>,
    bytes: usize,
}
impl Charge {
    fn reserve(used: Arc<AtomicUsize>, bytes: usize) -> Result<Self, DomainError> {
        used.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            n.checked_add(bytes).filter(|n| *n <= MAX_ENGINE_BYTES)
        })
        .map_err(|_| {
            err(
                ErrorCode::Locked,
                "TEST retained/in-flight byte budget full; retry later",
            )
        })?;
        Ok(Self { used, bytes })
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Debug)]
struct StepState {
    definition: StepDefinition,
    template: Option<format::Step>,
    status: StepStatus,
    window: AttemptWindow,
    accepted: Option<VerdictRecord>,
    latest: Option<VerdictRecord>,
    history: VecDeque<VerdictRecord>,
    evidence: Vec<Citation>,
}
impl StepState {
    fn snapshot(&self) -> Value {
        let latest = serde_json::to_value(&self.latest).unwrap_or(Value::Null);
        let accepted = serde_json::to_value(&self.accepted).unwrap_or(Value::Null);
        json!({"step_id":self.definition.step_id,"n":self.definition.n,"title":self.definition.title,"status":self.status,"attempt":self.window.attempt,"verdict":snapshot::verdict_row(&latest),"accepted":snapshot::verdict_row(&accepted),"history_count":self.history.len()})
    }
}
#[derive(Debug)]
struct OpenAsk {
    question: Question,
    deadline: Instant,
}
#[derive(Debug)]
struct Session {
    id: String,
    owner: String,
    owner_role: Role,
    tc: Arc<TcDoc>,
    plan: Option<Arc<Plan>>,
    report: Option<format::Report>,
    title: String,
    env: Option<Env>,
    bench: Option<String>,
    state: SessionState,
    revision: U64,
    steps: Vec<StepState>,
    current: Option<usize>,
    notes: Vec<Value>,
    asks: BTreeMap<String, OpenAsk>,
    answers: VecDeque<Value>,
    ask_count: u32,
    recent: VecDeque<Value>,
    approval_id: Option<String>,
    result: Option<SessionResult>,
    closing: bool,
    finally_done: bool,
    finally_unknown: bool,
    _base: Charge,
    extra: Vec<Charge>,
}
impl Session {
    fn snapshot(&self) -> Value {
        json!({"session_id":self.id,"title":self.title,"tc_id":self.tc.meta.id,"tc_sha256":self.tc.source_sha256,"state":self.state,"current_step":self.current.map(|i|self.steps[i].definition.step_id.clone()),"steps":self.steps.iter().map(StepState::snapshot).collect::<Vec<_>>(),"recent_evidence":self.recent,"notes_count":self.notes.len(),"notes":self.notes.iter().rev().take(20).map(|n|json!({"step_id":n["step_id"],"text":n["text"].as_str().map(preview),"kind":n["kind"]})).collect::<Vec<_>>(),"asks":self.asks.values().map(|a|&a.question).collect::<Vec<_>>(),"answers":self.answers.iter().rev().take(MAX_OPEN_ASKS).collect::<Vec<_>>(),"revision":self.revision,"plan_hash":self.plan.as_ref().map(|p|p.plan_sha256()),"approval":self.approval_id.as_ref().map(|id|json!({"approval_id":id})),"env":self.env,"bench":self.bench,"result":self.result,"ending":self.closing,"finally_done":self.finally_done,"finally_outcome_unknown":self.finally_unknown})
    }
    fn spec(&self, i: usize) -> StepSpec {
        let s = &self.steps[i];
        StepSpec {
            definition: s.definition.clone(),
            template: s.template.clone(),
            plan_hash: self.plan.as_ref().map(|p| p.plan_sha256().into()),
            revision: self.revision,
            window: s.window.clone(),
        }
    }
    fn step_index(&self, id: &str) -> Result<usize, DomainError> {
        self.steps
            .iter()
            .position(|s| s.definition.step_id == id)
            .ok_or_else(|| {
                err(
                    ErrorCode::NotFound,
                    "step does not exist in the registered plan/TC",
                )
            })
    }
    fn risk_summary(&self) -> Value {
        self.report.as_ref().map(risks).unwrap_or(Value::Null)
    }
    fn plan_view(&self) -> Value {
        let mut steps = Vec::new();
        for s in &self.steps {
            let risk = self.report.as_ref().map(|r| {
                r.risks
                    .iter()
                    .filter(|r| r.step == s.definition.step_id)
                    .map(|r| format!("{:?}", r.risk.level))
                    .collect::<Vec<_>>()
            });
            steps.push(json!({"step_id":s.definition.step_id,"tc_step":s.template.as_ref().and_then(|t|t.tc_step),"source_text":s.definition.source_text,"command":s.template.as_ref().map(|t|t.actions.iter().map(|a|json!({"method":a.method,"args":a.args})).collect::<Vec<_>>()),"predicate_text":s.definition.expect_text,"risk":risk}));
        }
        json!({"session_id":self.id,"plan_hash":self.plan.as_ref().map(|p|p.plan_sha256()),"tc_sha256":self.tc.source_sha256,"revision":self.revision,"steps":steps,"risk_summary":self.risk_summary(),"approval":self.approval_id.as_ref().map(|id|json!({"approval_id":id})),"classification_wired":self.report.is_some()})
    }
}
#[derive(Debug, Default)]
struct State {
    sessions: BTreeMap<String, Session>,
    completed: VecDeque<String>,
    next_id: u64,
}
struct Shared {
    state: Mutex<State>,
    config: SessionConfig,
    commands: Option<Arc<dyn format::CommandTable + Send + Sync>>,
    verifier: Arc<dyn VerifierEngine>,
    hooks: Arc<dyn SessionHooks>,
    control: Arc<Semaphore>,
    checks: Arc<Semaphore>,
    preparations: Arc<Semaphore>,
    waiters: Arc<Semaphore>,
    used: Arc<AtomicUsize>,
    notify: Notify,
    stopped: AtomicBool,
    origin: Instant,
}
impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestShared")
            .field("config", &self.config)
            .field("used_bytes", &self.used.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone)]
pub struct TestModule {
    shared: Arc<Shared>,
}
impl Default for TestModule {
    fn default() -> Self {
        Self::new()
    }
}
impl TestModule {
    pub fn new() -> Self {
        Self::with_services(
            SessionConfig::default(),
            None,
            Arc::new(crate::wire::PredicateEngine::default()),
            Arc::new(NotWiredHooks),
        )
        .expect("valid built-in session configuration")
    }
    pub fn with_services(
        config: SessionConfig,
        commands: Option<Arc<dyn format::CommandTable + Send + Sync>>,
        verifier: Arc<dyn VerifierEngine>,
        hooks: Arc<dyn SessionHooks>,
    ) -> Result<Self, DomainError> {
        if !(1..=MAX_ACTIVE_SESSIONS).contains(&config.max_sessions) {
            return Err(usage("session count must be 1..=4"));
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                config,
                commands,
                verifier,
                hooks,
                control: Arc::new(Semaphore::new(MAX_CONTROL_REQUESTS)),
                checks: Arc::new(Semaphore::new(MAX_CHECKS)),
                preparations: Arc::new(Semaphore::new(1)),
                waiters: Arc::new(Semaphore::new(MAX_WAITERS)),
                used: Arc::new(AtomicUsize::new(0)),
                notify: Notify::new(),
                stopped: AtomicBool::new(false),
                origin: Instant::now(),
            }),
        })
    }
    fn now(&self) -> U64 {
        U64(u64::try_from(self.shared.origin.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }
    fn stopped(&self) -> bool {
        self.shared.stopped.load(Ordering::Acquire) || self.shared.hooks.stop_latched()
    }
    /// CORE calls this on every domain write, not just test.*. Viewers retain direct device control.
    pub fn check_client_write(&self, client: &str, role: Role) -> Result<(), DomainError> {
        if role == Role::Viewer {
            return Ok(());
        }
        if self.stopped() {
            return Err(err(ErrorCode::Stopped, "global STOP"));
        }
        for s in lock(&self.shared.state)
            .sessions
            .values()
            .filter(|s| s.owner == client && s.state.active())
        {
            match s.state {
                SessionState::Paused => {
                    return Err(err(ErrorCode::SessionPaused, "session client is paused"));
                }
                SessionState::WaitingApproval | SessionState::Planned => {
                    return Err(err(ErrorCode::ApprovalRequired, "session plan is not authorized"));
                }
                _ => (),
            };
            if s.closing {
                return Err(err(ErrorCode::Locked, "session is ending"));
            }
        }
        Ok(())
    }
    /// CORE latch integration. Releasing STOP does not resume stopped sessions or run finally.
    pub fn set_stop(&self, ctx: &CallCtx, on: bool) -> Result<(), DomainError> {
        self.shared.stopped.store(on, Ordering::Release);
        if on {
            let mut state = lock(&self.shared.state);
            for s in state.sessions.values_mut().filter(|s| s.state.active()) {
                self.step_end(s, ctx)?;
                self.emit(
                    ctx,
                    s,
                    "test.session_state",
                    json!({"state":"STOPPED","reason":"stopped"}),
                )?;
                s.state = SessionState::Stopped;
            }
        }
        self.shared.notify.notify_waiters();
        Ok(())
    }
    pub fn snapshot(&self, session: &str) -> Result<Value, DomainError> {
        lock(&self.shared.state)
            .sessions
            .get(session)
            .map(Session::snapshot)
            .ok_or_else(|| err(ErrorCode::NotFound, "session snapshot not retained"))
    }
    pub fn bindings(&self) -> Vec<ClientBinding> {
        lock(&self.shared.state)
            .sessions
            .values()
            .filter(|s| s.state.active())
            .map(|s| ClientBinding {
                client_id: s.owner.clone(),
                role: s.owner_role,
                session_id: s.id.clone(),
            })
            .collect()
    }
    pub fn accounted_bytes(&self) -> usize {
        self.shared.used.load(Ordering::Acquire)
    }
    fn authorize(&self, s: &Session, ctx: &CallCtx, viewer_only: bool) -> Result<(), DomainError> {
        if ctx.role() == Role::Viewer {
            return if ctx.rights().is_some_and(|r| r >= Rights::Operate) {
                Ok(())
            } else {
                Err(err(ErrorCode::Denied, "viewer needs operate rights"))
            };
        }
        if viewer_only || s.owner != ctx.client_id() {
            return Err(err(
                ErrorCode::Denied,
                "session control belongs to its client or an operating viewer",
            ));
        }
        Ok(())
    }
    fn writable(&self, s: &Session, ctx: &CallCtx) -> Result<(), DomainError> {
        self.authorize(s, ctx, false)?;
        if self.stopped() {
            return Err(err(ErrorCode::Stopped, "global STOP"));
        }
        if s.state == SessionState::Paused && ctx.role() != Role::Viewer {
            return Err(err(ErrorCode::SessionPaused, "session is paused"));
        }
        if s.state != SessionState::Running {
            return Err(err(ErrorCode::Locked, "session is not running"));
        }
        if s.closing {
            return Err(err(ErrorCode::Locked, "session is ending"));
        }
        if s.finally_done || s.finally_unknown {
            return Err(err(
                ErrorCode::Locked,
                "cleanup has been dispatched; finish or explicitly amend the session",
            ));
        }
        Ok(())
    }
    fn emit(&self, ctx: &CallCtx, s: &Session, kind: &str, mut payload: Value) -> Result<(), DomainError> {
        let o = payload
            .as_object_mut()
            .ok_or_else(|| usage("event payload must be an object"))?;
        o.insert("session_id".into(), json!(s.id));
        o.entry("revision").or_insert(json!(s.revision));
        json_len(&payload, MAX_EVENT_BYTES)?;
        let mut draft = EventDraft::new(kind, payload);
        draft.env = s.env;
        draft.purpose = Some("test_session".into());
        ctx.publish(draft).map_err(|e| {
            err(
                ErrorCode::RecordingFailed,
                format!("TEST event was not published: {e}"),
            )
        })?;
        Ok(())
    }
    fn get_id(state: &State, p: &Value) -> Result<String, DomainError> {
        if let Some(id) =
            optional_text(p, "session", MAX_ID_BYTES)?.or(optional_text(p, "session_id", MAX_ID_BYTES)?)
        {
            return Ok(id.into());
        }
        let active = state
            .sessions
            .values()
            .filter(|s| s.state.active())
            .collect::<Vec<_>>();
        match active.as_slice() {
            [s] => Ok(s.id.clone()),
            [] => Err(err(ErrorCode::NotFound, "no active session; specify session id")),
            _ => Err(usage("multiple sessions; specify session id")),
        }
    }
    fn step_end(&self, s: &mut Session, ctx: &CallCtx) -> Result<(), DomainError> {
        if let Some(i) = s.current {
            let end =
                self.shared
                    .hooks
                    .fence(&s.id, &s.steps[i].definition.step_id, s.steps[i].window.attempt)?;
            check_fence(&end)?;
            self.emit(ctx,s,"test.step_ended",json!({"step_id":s.steps[i].definition.step_id,"attempt":s.steps[i].window.attempt,"fence_available":end.is_some()}))?;
            s.steps[i].status = StepStatus::Ended;
            s.steps[i].window.end = end;
            s.steps[i].window.host_ended_ns = Some(self.now());
            s.current = None;
        }
        Ok(())
    }
    fn session_state(
        &self,
        s: &mut Session,
        ctx: &CallCtx,
        state: SessionState,
        reason: &str,
    ) -> Result<(), DomainError> {
        self.emit(
            ctx,
            s,
            "test.session_state",
            json!({"state":state,"reason":reason}),
        )?;
        s.state = state;
        Ok(())
    }
    pub async fn call(&self, ctx: &mut CallCtx, method: &str, params: Value) -> Result<Value, DomainError> {
        if ctx.client_id().is_empty() || ctx.client_id().len() > MAX_ID_BYTES {
            return Err(err(ErrorCode::Denied, "invalid connection client id"));
        }
        if !params.is_object() {
            return Err(usage("params must be an object"));
        }
        validate_json(&params)?;
        let n = json_len(&params, MAX_REQUEST_BYTES)?;
        let ctl = self.shared.control.clone().try_acquire_owned().map_err(|_| {
            err(
                ErrorCode::Locked,
                "TEST control queue full; no request was dropped",
            )
        })?;
        let memory = Charge::reserve(self.shared.used.clone(), n)?;
        if Instant::now() >= ctx.deadline() {
            return Err(err(ErrorCode::Timeout, "RPC deadline expired"));
        }
        if method == "test.wait" {
            drop(ctl);
            drop(memory);
            return self.wait(ctx, &params).await;
        }
        let result = match method {
            "test.start" => self.start(ctx, &params).await,
            "test.plan.amend" => self.amend(ctx, &params).await,
            "test.check" | "test.evidence.add" | "test.verdict" => self.verify(ctx, method, &params).await,
            "test.end" => self.end(ctx, &params).await,
            "test.import" | "test.report" | "test.freeze" => {
                Err(DomainError::not_implemented("TEST P2 import/report/freeze"))
            }
            _ => self.control(ctx, method, &params),
        };
        drop(ctl);
        drop(memory);
        self.shared.notify.notify_waiters();
        if method == "test.start"
            && result
                .as_ref()
                .err()
                .is_some_and(|e| e.code == ErrorCode::ApprovalRequired)
            && params.get("wait_approval_ms").is_some()
        {
            let mut p = params.clone();
            if let Some(id) = result
                .as_ref()
                .err()
                .and_then(|e| e.detail.get("session_id"))
                .and_then(Value::as_str)
            {
                p["session"] = json!(id);
                p["until"] = json!("running");
                p["timeout_ms"] = json!(u64_arg(&params, "wait_approval_ms", None)?);
                return self.wait(ctx, &p).await;
            }
        }
        result
    }
}
impl DomainModule for TestModule {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = loom_domain_api::DOMAIN_API_VERSION;
    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        self.register_into(reg)
    }
}
fn check_fence(f: &Option<Fence>) -> Result<(), DomainError> {
    if f.as_ref().is_some_and(|f| f.source_refs.len() > MAX_CITATIONS) {
        return Err(err(ErrorCode::Partial, "CORE fence exceeds worker/source bound"));
    }
    Ok(())
}
fn risks(r: &format::Report) -> Value {
    let mut out = serde_json::Map::new();
    for level in [
        format::RiskLevel::R0,
        format::RiskLevel::R1,
        format::RiskLevel::R2,
        format::RiskLevel::R3,
        format::RiskLevel::R4,
    ] {
        let count = r
            .risks
            .iter()
            .filter(|x| x.risk.level == level)
            .map(|x| x.max_uses)
            .sum();
        out.insert(format!("{level:?}"), json!(U64(count)));
    }
    Value::Object(out)
}
fn definitions(tc: &TcDoc, plan: Option<&Plan>) -> Vec<StepState> {
    let build = |definition, template| StepState {
        definition,
        template,
        status: StepStatus::Pending,
        window: AttemptWindow {
            attempt: 0,
            host_started_ns: U64(0),
            host_ended_ns: None,
            start: None,
            end: None,
        },
        accepted: None,
        latest: None,
        history: VecDeque::new(),
        evidence: Vec::new(),
    };
    if let Some(plan) = plan {
        plan.steps()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let original = s.tc_step.and_then(|n| tc.steps.iter().find(|x| x.n == n));
                let source_text = original.map(|s| s.action.clone()).unwrap_or_else(|| s.id.clone());
                let title = preview(&source_text);
                let expect_text = preview(&serde_json::to_string(&s.expect).unwrap_or_default());
                let mut domains = Vec::new();
                for a in &s.actions {
                    if let Some((domain, _)) = a.method.split_once('.') {
                        let channel = ["ch", "channel", "port", "target", "name", "device"]
                            .iter()
                            .find_map(|k| a.args.get(k).and_then(Value::as_str))
                            .map(str::to_owned);
                        let d = StepDomain {
                            domain: domain.into(),
                            channel,
                        };
                        if !domains.contains(&d) {
                            domains.push(d);
                        }
                    }
                }
                for e in &s.expect {
                    let d = StepDomain {
                        domain: e.filter.domain.clone(),
                        channel: e.filter.channel.clone(),
                    };
                    if !domains.contains(&d) {
                        domains.push(d);
                    }
                }
                let timeout_ms = s
                    .actions
                    .iter()
                    .filter_map(|a| a.args.get("timeout_ms").and_then(Value::as_u64))
                    .max()
                    .map(U64);
                build(
                    StepDefinition {
                        step_id: s.id.clone(),
                        n: i as u32 + 1,
                        title,
                        expect_text,
                        source_text: preview(&source_text),
                        domains,
                        timeout_ms,
                    },
                    Some(s.clone()),
                )
            })
            .collect()
    } else {
        tc.steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                build(
                    StepDefinition {
                        step_id: format!("S{}", i + 1),
                        n: s.n,
                        title: preview(&s.action),
                        expect_text: preview(&s.expected),
                        source_text: preview(&s.action),
                        domains: Vec::new(),
                        timeout_ms: None,
                    },
                    None,
                )
            })
            .collect()
    }
}
struct Prepared {
    tc: Arc<TcDoc>,
    plan: Option<Arc<Plan>>,
    report: Option<format::Report>,
    charge: Charge,
}
impl TestModule {
    async fn prepare(&self, tc: Value, plan: Option<Value>) -> Result<Prepared, DomainError> {
        let permit = self
            .shared
            .preparations
            .clone()
            .try_acquire_owned()
            .map_err(|_| err(ErrorCode::Locked, "another TC/plan parse is in progress"))?;
        let charge = Charge::reserve(self.shared.used.clone(), CAP_SESSION_BASE_BYTES)?;
        let commands = self.shared.commands.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let text = source::load(&tc, ".tc.md")?;
            let tc = Arc::new(format::parse_tc(&text).map_err(source::format_error)?);
            let (plan, report) = if let Some(value) = plan {
                let text = source::load(&value, ".plan.yaml")?;
                let plan = format::parse_plan_with_tc(&text, &tc).map_err(source::format_error)?;
                let commands = commands.ok_or_else(|| {
                    DomainError::not_implemented("actual Registry command/risk table must be injected")
                })?;
                let report = format::check_plan(&plan, commands.as_ref());
                if !report.is_valid() {
                    let blocked = report.errors.iter().any(|e| e.code == "blocked_command");
                    return Err(err(
                        if blocked {
                            ErrorCode::Denied
                        } else {
                            ErrorCode::Usage
                        },
                        "plan check failed",
                    )
                    .with_detail(serde_json::to_value(report).unwrap_or(Value::Null)));
                }
                if report.requires_legacy_adapter {
                    return Err(DomainError::not_implemented(
                        "legacy scenario expectations need the UDS adapter",
                    ));
                }
                (Some(Arc::new(plan)), Some(report))
            } else {
                (None, None)
            };
            Ok(Prepared {
                tc,
                plan,
                report,
                charge,
            })
        })
        .await
        .map_err(|_| err(ErrorCode::Internal, "TC/plan parser task failed"))?
    }
    async fn start(&self, ctx: &CallCtx, p: &Value) -> Result<Value, DomainError> {
        optional_text(p, "env", MAX_ID_BYTES)?;
        if ctx.role() == Role::Viewer && ctx.rights().is_none_or(|r| r < Rights::Operate) {
            return Err(err(ErrorCode::Denied, "viewer requires operate"));
        }
        if self.stopped() {
            return Err(err(ErrorCode::Stopped, "global STOP"));
        }
        {
            let mut state = lock(&self.shared.state);
            // STOP can make sessions terminal without an explicit test.end call.
            let terminal = state
                .sessions
                .iter()
                .filter(|(_, s)| !s.state.active())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in terminal {
                if !state.completed.contains(&id) {
                    state.completed.push_back(id);
                }
            }
            while self.accounted_bytes().saturating_add(CAP_SESSION_BASE_BYTES) > MAX_ENGINE_BYTES {
                if let Some(id) = state.completed.pop_front() {
                    state.sessions.remove(&id);
                } else {
                    break;
                }
            }
            if state.sessions.values().filter(|s| s.state.active()).count() >= self.shared.config.max_sessions
            {
                return Err(err(ErrorCode::SessionActive, "session limit already active"));
            }
            if state
                .sessions
                .values()
                .any(|s| s.state.active() && s.owner == ctx.client_id())
            {
                return Err(err(ErrorCode::SessionActive, "client already owns a session"));
            }
        }
        let tc = p.get("tc").ok_or_else(|| usage("tc source is required"))?.clone();
        let plan = p.get("plan").filter(|v| !v.is_null()).cloned();
        let prepared = self.prepare(tc, plan).await?;
        let title = optional_text(p, "title", MAX_TEXT_BYTES)?
            .map(str::to_owned)
            .or(prepared.tc.meta.title.clone())
            .unwrap_or_else(|| "Untitled TC".into());
        let bench = optional_text(p, "bench", MAX_ID_BYTES)?.map(str::to_owned);
        let mut env = None;
        for value in [
            p.get("env").and_then(Value::as_str),
            prepared
                .plan
                .as_ref()
                .and_then(|p| p.canonical_value().get("env"))
                .and_then(Value::as_str),
            prepared.tc.meta.env.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let parsed = match value {
                "sim" => Env::Sim,
                "bench" => Env::Bench,
                "vehicle" => Env::Vehicle,
                _ => return Err(usage("invalid environment")),
            };
            if env.is_some_and(|old| old != parsed) {
                return Err(usage("TC/plan/call environments differ"));
            }
            env = Some(parsed);
        }
        let mut state = lock(&self.shared.state);
        if Instant::now() >= ctx.deadline() {
            return Err(err(ErrorCode::Timeout, "start deadline expired during parsing"));
        }
        if state.sessions.values().filter(|s| s.state.active()).count() >= self.shared.config.max_sessions
            || state
                .sessions
                .values()
                .any(|s| s.state.active() && s.owner == ctx.client_id())
        {
            return Err(err(
                ErrorCode::SessionActive,
                "session became active while parsing",
            ));
        }
        while state.completed.len() >= MAX_RETAINED_SESSIONS {
            if let Some(id) = state.completed.pop_front() {
                state.sessions.remove(&id);
            }
        }
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or_else(|| err(ErrorCode::Config, "session id counter exhausted"))?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| err(ErrorCode::Internal, "clock before epoch"))?
            .as_nanos();
        let id = format!("ts-{stamp:x}-{}-{}", std::process::id(), state.next_id);
        let mut s = Session {
            id: id.clone(),
            owner: ctx.client_id().into(),
            owner_role: ctx.role(),
            steps: definitions(&prepared.tc, prepared.plan.as_deref()),
            tc: prepared.tc,
            plan: prepared.plan,
            report: prepared.report,
            title,
            env,
            bench,
            state: SessionState::Planned,
            revision: U64(1),
            current: None,
            notes: Vec::new(),
            asks: BTreeMap::new(),
            answers: VecDeque::new(),
            ask_count: 0,
            recent: VecDeque::new(),
            approval_id: None,
            result: None,
            closing: false,
            finally_done: false,
            finally_unknown: false,
            _base: prepared.charge,
            extra: Vec::new(),
        };
        self.emit(ctx,&s,"test.session_started",json!({"session_id":id,"state":"PLANNED","title":s.title,"tc_id":s.tc.meta.id,"tc_sha256":s.tc.source_sha256,"actor":ctx.client_id(),"env":s.env,"bench":s.bench,"plan_digest":s.plan.as_ref().map(|p|p.plan_sha256()),"steps":s.steps.iter().map(|s|&s.definition).collect::<Vec<_>>(),"risk_summary":s.risk_summary()}))?;
        // Preserve an already-published start even if a later transition publication fails.
        let transition = self.session_state(&mut s, ctx, SessionState::WaitingApproval, "plan_policy_gate");
        state.sessions.insert(id.clone(), s);
        transition?;
        let s = state
            .sessions
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::Internal, "new session missing"))?;
        if self.shared.config.approval == ApprovalMode::Auto {
            self.session_state(s, ctx, SessionState::Running, "F1_auto")?;
            Ok(
                json!({"session_id":id,"state":s.state,"tc_sha256":s.tc.source_sha256,"plan_hash":s.plan.as_ref().map(|p|p.plan_sha256()),"risk_summary":s.risk_summary(),"approval":null}),
            )
        } else {
            Err(err(
                ErrorCode::ApprovalRequired,
                "session waits for policy-enabled CORE approval",
            )
            .with_detail(json!({"session_id":id,"state":s.state,"risk_summary":s.risk_summary()})))
        }
    }
}
impl TestModule {
    fn expire_asks(&self, s: &mut Session, ctx: &CallCtx) -> Result<(), DomainError> {
        let ids = s
            .asks
            .iter()
            .filter(|(_, a)| Instant::now() >= a.deadline)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            self.emit(
                ctx,
                s,
                "test.answer",
                json!({"ask_id":id,"value":null,"expired":true}),
            )?;
            s.asks.remove(&id);
            s.answers
                .push_back(json!({"ask_id":id,"expired":true,"value":null}));
            if s.answers.len() > MAX_RECENT_EVIDENCE {
                s.answers.pop_front();
            }
        }
        Ok(())
    }
    fn keep(&self, s: &mut Session, value: &Value) -> Result<(), DomainError> {
        if s.extra.len() >= MAX_SESSION_MUTATIONS {
            return Err(err(ErrorCode::Locked, "session mutation retention quota reached"));
        }
        let size = json_len(value, MAX_EVENT_BYTES)?
            .saturating_mul(2)
            .saturating_add(256);
        s.extra.push(Charge::reserve(self.shared.used.clone(), size)?);
        Ok(())
    }
    fn publish_kept(
        &self,
        ctx: &CallCtx,
        s: &mut Session,
        kind: &str,
        value: Value,
    ) -> Result<(), DomainError> {
        if s.extra.len() >= MAX_SESSION_MUTATIONS {
            return Err(err(ErrorCode::Locked, "session mutation quota reached"));
        }
        let bytes = json_len(&value, MAX_EVENT_BYTES)?
            .saturating_mul(2)
            .saturating_add(256);
        let charge = Charge::reserve(self.shared.used.clone(), bytes)?;
        self.emit(ctx, s, kind, value)?;
        s.extra.push(charge);
        Ok(())
    }
    fn record(
        &self,
        s: &mut Session,
        ctx: &CallCtx,
        i: usize,
        record: VerdictRecord,
    ) -> Result<(), DomainError> {
        let mut payload = serde_json::to_value(&record).map_err(|e| usage(e.to_string()))?;
        payload["step_id"] = json!(s.steps[i].definition.step_id);
        self.publish_kept(ctx, s, "test.verdict", payload)?;
        let step = &mut s.steps[i];
        if record.rejected.is_none() {
            step.accepted = Some(record.clone());
        }
        step.latest = Some(record.clone());
        step.history.push_back(record);
        if step.history.len() > MAX_VERDICT_HISTORY {
            step.history.pop_front();
        }
        Ok(())
    }
    fn begin(&self, s: &mut Session, ctx: &CallCtx, i: usize, retry: bool) -> Result<Value, DomainError> {
        self.writable(s, ctx)?;
        if retry {
            let allowed = s.steps[i].template.as_ref().map_or(0, |t| t.retry_count);
            if s.steps[i].status != StepStatus::Ended || s.steps[i].window.attempt > allowed {
                return Err(err(
                    ErrorCode::Denied,
                    "retry is not planned or attempt budget exhausted",
                ));
            }
        } else {
            if s.steps[i].status != StepStatus::Pending {
                return Err(err(ErrorCode::Locked, "step already started; use planned retry"));
            }
            if let Some(when) = s.steps[i].template.as_ref().and_then(|t| t.when.as_ref()) {
                let dependency = s.step_index(&when.step)?;
                let actual = s.steps[dependency]
                    .accepted
                    .as_ref()
                    .and_then(|v| v.effective)
                    .map(|v| serde_json::to_value(v).unwrap_or(Value::Null));
                if actual.as_ref().and_then(Value::as_str) != Some(when.verdict.as_str()) {
                    return Err(err(ErrorCode::NoMatch, "step when condition is not satisfied"));
                }
            }
            let jump=s.current.is_some_and(|j|s.steps[j].accepted.as_ref().and_then(|v|v.effective)==Some(Verdict::Fail)&&s.steps[j].template.as_ref().is_some_and(|t|matches!(&t.on_fail,FailureAction::Goto(id) if id==&s.steps[i].definition.step_id)));
            if !jump {
                for j in 0..i {
                    if Some(j) != s.current
                        && s.steps[j].status == StepStatus::Pending
                        && !condition_false(s, j)
                    {
                        return Err(err(ErrorCode::Denied, "earlier required step has not started"));
                    }
                }
            }
        }
        let attempt = s.steps[i]
            .window
            .attempt
            .checked_add(1)
            .ok_or_else(|| err(ErrorCode::Config, "attempt counter exhausted"))?;
        let fence = self
            .shared
            .hooks
            .fence(&s.id, &s.steps[i].definition.step_id, attempt)?;
        check_fence(&fence)?;
        self.step_end(s, ctx)?;
        let def = &s.steps[i].definition;
        let mut payload = serde_json::to_value(def).map_err(|e| usage(e.to_string()))?;
        payload["attempt"] = json!(attempt);
        payload["retry"] = json!(retry);
        payload["fence_available"] = json!(fence.is_some());
        self.emit(ctx, s, "test.step_started", payload)?;
        let step = &mut s.steps[i];
        step.status = StepStatus::Running;
        step.window = AttemptWindow {
            attempt,
            host_started_ns: self.now(),
            host_ended_ns: None,
            start: fence,
            end: None,
        };
        step.accepted = None;
        step.latest = None;
        step.evidence.clear();
        s.current = Some(i);
        Ok(
            json!({"session_id":s.id,"step_id":step.definition.step_id,"attempt":attempt,"source_fence_available":step.window.start.is_some()}),
        )
    }
    fn control(&self, ctx: &CallCtx, method: &str, p: &Value) -> Result<Value, DomainError> {
        if method == "test.status" && p.get("session").is_none() && p.get("session_id").is_none() {
            let mut state = lock(&self.shared.state);
            let ids = state.sessions.keys().cloned().collect::<Vec<_>>();
            for id in &ids {
                if let Some(s) = state.sessions.get_mut(id) {
                    self.expire_asks(s, ctx)?;
                }
            }
            let sessions = state.sessions.values().map(|s|json!({"session_id":s.id,"title":preview(&s.title),"state":s.state,"result":s.result,"current_step":s.current.map(|i|&s.steps[i].definition.step_id)})).collect::<Vec<_>>();
            return Ok(json!({"sessions":sessions,"accounted_bytes":U64(self.accounted_bytes() as u64)}));
        }
        let mut state = lock(&self.shared.state);
        let id = Self::get_id(&state, p)?;
        let s = state
            .sessions
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
        self.expire_asks(s, ctx)?;
        match method {
            "test.status" => Ok(s.snapshot()),
            "test.plan.show" => Ok(s.plan_view()),
            "test.step" | "test.step.begin" => {
                let step = text(p, "id", MAX_ID_BYTES).or_else(|_| text(p, "step", MAX_ID_BYTES))?;
                self.writable(s, ctx)?;
                let existing = s.steps.iter().position(|x| x.definition.step_id == step);
                let appended = existing.is_none() && s.plan.is_none();
                let i = if let Some(i) = existing {
                    i
                } else if appended {
                    if s.steps.len() >= format::MAX_STEPS {
                        return Err(usage("at most 500 step markers"));
                    }
                    let title = optional_text(p, "title", MAX_TEXT_BYTES)?.unwrap_or(step);
                    s.steps.push(StepState {
                        definition: StepDefinition {
                            step_id: step.into(),
                            n: s.steps.len() as u32 + 1,
                            title: preview(title),
                            expect_text: String::new(),
                            source_text: preview(title),
                            domains: Vec::new(),
                            timeout_ms: None,
                        },
                        template: None,
                        status: StepStatus::Pending,
                        window: AttemptWindow {
                            attempt: 0,
                            host_started_ns: U64(0),
                            host_ended_ns: None,
                            start: None,
                            end: None,
                        },
                        accepted: None,
                        latest: None,
                        history: VecDeque::new(),
                        evidence: Vec::new(),
                    });
                    s.steps.len() - 1
                } else {
                    return Err(err(ErrorCode::NotFound, "step is not registered in the plan"));
                };
                let result = self.begin(s, ctx, i, false);
                if result.is_err() && appended {
                    s.steps.pop();
                }
                result
            }
            "test.step.end" => {
                self.writable(s, ctx)?;
                if let Some(id) =
                    optional_text(p, "id", MAX_ID_BYTES)?.or(optional_text(p, "step", MAX_ID_BYTES)?)
                    && s.current.map(|i| s.steps[i].definition.step_id.as_str()) != Some(id)
                {
                    return Err(usage("step is not the current running step"));
                }
                self.step_end(s, ctx)?;
                Ok(s.snapshot())
            }
            "test.pause" => {
                self.authorize(s, ctx, false)?;
                if s.state != SessionState::Running {
                    return Err(err(ErrorCode::Locked, "only RUNNING can pause"));
                }
                self.session_state(s, ctx, SessionState::Paused, "user_pause")?;
                Ok(s.snapshot())
            }
            "test.resume" => {
                self.authorize(s, ctx, false)?;
                if s.closing || s.finally_done || s.finally_unknown {
                    return Err(err(
                        ErrorCode::Locked,
                        "cleanup has been dispatched; finish the session",
                    ));
                }
                if self.stopped() {
                    return Err(err(ErrorCode::Stopped, "global STOP"));
                }
                if s.state == SessionState::WaitingApproval {
                    match self.shared.hooks.approval(
                        &s.id,
                        s.revision,
                        s.plan.as_ref().map(|p| p.plan_sha256()),
                    )? {
                        ApprovalStatus::Pending => {
                            return Err(err(ErrorCode::ApprovalRequired, "approval pending"));
                        }
                        ApprovalStatus::Granted { approval_id } => {
                            if approval_id.is_empty() || approval_id.len() > MAX_ID_BYTES {
                                return Err(err(ErrorCode::Internal, "invalid approval id from CORE"));
                            }
                            self.emit(
                                ctx,
                                s,
                                "test.session_state",
                                json!({"state":"RUNNING","reason":"CORE_approval","approval_id":approval_id}),
                            )?;
                            s.approval_id = Some(approval_id);
                            s.state = SessionState::Running;
                        }
                    }
                } else if s.state == SessionState::Paused {
                    self.session_state(s, ctx, SessionState::Running, "user_resume")?;
                } else {
                    return Err(err(ErrorCode::Locked, "only PAUSED/WAITING_APPROVAL can resume"));
                }
                Ok(s.snapshot())
            }
            "test.note" => {
                self.authorize(s, ctx, false)?;
                if !s.state.active() {
                    return Err(err(ErrorCode::Locked, "ended session is immutable"));
                }
                if s.notes.len() >= MAX_NOTES {
                    return Err(err(ErrorCode::Locked, "session note limit reached"));
                }
                let value = text(p, "text", MAX_TEXT_BYTES)?;
                let step = optional_text(p, "step", MAX_ID_BYTES)?;
                if let Some(step) = step {
                    s.step_index(step)?;
                }
                let payload = json!({"step_id":step,"text":value,"kind":optional_text(p,"kind",MAX_ID_BYTES)?.unwrap_or("note")});
                self.publish_kept(ctx, s, "test.note", payload.clone())?;
                s.notes.push(payload);
                Ok(json!({"session_id":id,"notes":s.notes.len()}))
            }
            "test.ask" => {
                self.authorize(s, ctx, false)?;
                if !s.state.active() {
                    return Err(err(ErrorCode::Locked, "ended session is immutable"));
                }
                if s.asks.len() >= MAX_OPEN_ASKS || s.ask_count as usize >= MAX_NOTES {
                    return Err(err(ErrorCode::Locked, "question limit reached"));
                }
                let question = text(p, "question", MAX_TEXT_BYTES)?.to_owned();
                let timeout = u64_arg(p, "timeout_ms", None)?;
                let mut options = Vec::new();
                if let Some(value) = p.get("options") {
                    let items = value
                        .as_array()
                        .filter(|a| a.len() <= MAX_ASK_OPTIONS)
                        .ok_or_else(|| usage("at most 16 question options"))?;
                    for item in items {
                        let t = item
                            .as_str()
                            .filter(|s| !s.is_empty() && s.len() <= 256)
                            .ok_or_else(|| usage("options must be nonempty strings <=256 bytes"))?;
                        options.push(t.into());
                    }
                }
                let step_id = optional_text(p, "step", MAX_ID_BYTES)?
                    .map(str::to_owned)
                    .or_else(|| s.current.map(|i| s.steps[i].definition.step_id.clone()));
                if let Some(id) = &step_id {
                    s.step_index(id)?;
                }
                let count = s
                    .ask_count
                    .checked_add(1)
                    .ok_or_else(|| usage("question counter exhausted"))?;
                let ask_id = format!("ask-{}-{count}", s.revision);
                let q = Question {
                    ask_id: ask_id.clone(),
                    question,
                    options,
                    timeout_ms: U64(timeout),
                    step_id,
                    revision: s.revision,
                };
                let payload = serde_json::to_value(&q).map_err(|e| usage(e.to_string()))?;
                self.publish_kept(ctx, s, "test.ask", payload)?;
                s.asks.insert(
                    ask_id.clone(),
                    OpenAsk {
                        question: q.clone(),
                        deadline: Instant::now() + Duration::from_millis(timeout),
                    },
                );
                s.ask_count = count;
                Ok(json!({"session_id":id,"ask_id":ask_id,"status":"waiting","question":q}))
            }
            "test.answer" => {
                self.authorize(s, ctx, true)?;
                let ask_id = text(p, "ask_id", MAX_ID_BYTES)?;
                let value = text(p, "value", MAX_TEXT_BYTES)?;
                let ask = s.asks.get(ask_id).ok_or_else(|| {
                    if s.answers
                        .iter()
                        .any(|v| v["ask_id"] == ask_id && v["expired"] == true)
                    {
                        err(ErrorCode::Timeout, "question expired")
                    } else {
                        err(ErrorCode::NotFound, "question is not open")
                    }
                })?;
                if !ask.question.options.is_empty() && !ask.question.options.iter().any(|s| s == value) {
                    return Err(usage("answer must match one of the question options"));
                }
                let payload = json!({"ask_id":ask_id,"value":value,"step_id":ask.question.step_id});
                self.publish_kept(ctx, s, "test.answer", payload.clone())?;
                s.asks.remove(ask_id);
                s.answers.push_back(payload);
                if s.answers.len() > MAX_RECENT_EVIDENCE {
                    s.answers.pop_front();
                }
                Ok(s.snapshot())
            }
            "test.skip" => {
                self.authorize(s, ctx, false)?;
                if !s.state.active() {
                    return Err(err(ErrorCode::Locked, "ended session is immutable"));
                }
                let step = text(p, "step", MAX_ID_BYTES)?;
                let i = s.step_index(step)?;
                let reason = text(p, "reason", MAX_TEXT_BYTES)?.to_owned();
                if ctx.role() != Role::Viewer && !condition_false(s, i) {
                    return Err(err(
                        ErrorCode::Denied,
                        "agent skip requires an unsatisfied planned when branch",
                    ));
                }
                if s.current == Some(i) {
                    self.step_end(s, ctx)?;
                }
                let row = VerdictRecord {
                    proposed: Verdict::Skipped,
                    effective: Some(Verdict::Skipped),
                    by: ctx.client_id().into(),
                    reason: Some(reason),
                    evidence_refs: Vec::new(),
                    overridden: false,
                    previous: s.steps[i]
                        .accepted
                        .as_ref()
                        .map(|r| json!({"effective":r.effective,"by":r.by})),
                    rejected: None,
                    unsupported: false,
                    user_assertion: ctx.role() == Role::Viewer,
                    attempt: s.steps[i].window.attempt,
                    revision: s.revision,
                };
                self.record(s, ctx, i, row)?;
                s.steps[i].status = StepStatus::Ended;
                Ok(s.snapshot())
            }
            "test.retry" => {
                let id = text(p, "step", MAX_ID_BYTES)?;
                let i = s.step_index(id)?;
                self.begin(s, ctx, i, true)
            }
            _ => Err(err(ErrorCode::UnknownMethod, "unknown test method")),
        }
    }
    async fn wait(&self, ctx: &CallCtx, p: &Value) -> Result<Value, DomainError> {
        let _permit = self
            .shared
            .waiters
            .clone()
            .try_acquire_owned()
            .map_err(|_| err(ErrorCode::Locked, "TEST waiter limit reached"))?;
        let timeout = u64_arg(p, "timeout_ms", Some(30_000))?;
        let deadline = ctx
            .deadline()
            .min(Instant::now() + Duration::from_millis(timeout));
        let until = optional_text(p, "until", 32)?.unwrap_or("ended");
        if !matches!(until, "ended" | "running" | "answer") {
            return Err(usage("until must be ended, running or answer"));
        }
        let id = {
            let state = lock(&self.shared.state);
            Self::get_id(&state, p)?
        };
        loop {
            let notified = self.shared.notify.notified();
            let snapshot = {
                let mut state = lock(&self.shared.state);
                let s = state
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
                self.expire_asks(s, ctx)?;
                if until == "running"
                    && s.state == SessionState::WaitingApproval
                    && let ApprovalStatus::Granted { approval_id } = self.shared.hooks.approval(
                        &id,
                        s.revision,
                        s.plan.as_ref().map(|p| p.plan_sha256()),
                    )?
                {
                    if approval_id.is_empty() || approval_id.len() > MAX_ID_BYTES {
                        return Err(err(ErrorCode::Internal, "invalid CORE approval id"));
                    }
                    self.emit(
                        ctx,
                        s,
                        "test.session_state",
                        json!({"state":"RUNNING","reason":"CORE_approval","approval_id":approval_id}),
                    )?;
                    s.approval_id = Some(approval_id);
                    s.state = SessionState::Running;
                }
                let done = match until {
                    "ended" => s.result.is_some() || s.state == SessionState::Stopped,
                    "running" => s.state == SessionState::Running,
                    "answer" => {
                        let ask = text(p, "ask_id", MAX_ID_BYTES)?;
                        s.answers.iter().any(|v| v["ask_id"] == ask)
                    }
                    _ => false,
                };
                if done { Some(s.snapshot()) } else { None }
            };
            if let Some(v) = snapshot {
                let code = if v["state"] == "STOPPED" {
                    Some(ErrorCode::Stopped)
                } else if v["result"] == "failed" {
                    Some(ErrorCode::TestFailed)
                } else if v["result"] == "incomplete" || v["result"] == "inconclusive" {
                    Some(ErrorCode::Partial)
                } else {
                    None
                };
                if let Some(code) = code {
                    return Err(err(code, "TEST wait observed terminal state").with_detail(v));
                }
                if until == "answer"
                    && let Some(ask) = p.get("ask_id").and_then(Value::as_str)
                    && v["answers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|a| a["ask_id"] == ask && a["expired"] == true)
                {
                    return Err(err(ErrorCode::Timeout, "question expired").with_detail(v));
                }
                return Ok(v);
            }
            if self.stopped() {
                return Err(err(ErrorCode::Stopped, "global STOP"));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(err(ErrorCode::Timeout, "TEST wait deadline reached")
                    .with_detail(json!({"session_id":id})));
            }
            let delay = Duration::from_millis(20).min(deadline.saturating_duration_since(now));
            tokio::select! {_=notified=>(),_=tokio::time::sleep(delay)=>()};
        }
    }
}
fn condition_false(s: &Session, i: usize) -> bool {
    let Some(when) = s.steps[i].template.as_ref().and_then(|t| t.when.as_ref()) else {
        return false;
    };
    let Ok(j) = s.step_index(&when.step) else {
        return false;
    };
    let Some(actual) = s.steps[j].accepted.as_ref().and_then(|v| v.effective) else {
        return false;
    };
    serde_json::to_value(actual)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .is_some_and(|v| v != when.verdict)
}
impl TestModule {
    async fn verify(&self, ctx: &CallCtx, method: &str, p: &Value) -> Result<Value, DomainError> {
        let proposed = if method == "test.verdict" {
            Some(
                serde_json::from_value::<Verdict>(
                    p.get("verdict")
                        .cloned()
                        .ok_or_else(|| usage("verdict required"))?,
                )
                .map_err(|_| usage("unknown verdict"))?,
            )
        } else {
            None
        };
        let overridden = bool_arg(p, "override", false)?;
        let auto = bool_arg(p, "auto", false)?;
        let reason = optional_text(p, "reason", MAX_TEXT_BYTES)?.map(str::to_owned);
        let (id, i, spec, citations, user_assertion) = {
            let mut state = lock(&self.shared.state);
            let id = Self::get_id(&state, p)?;
            let s = state
                .sessions
                .get_mut(&id)
                .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
            self.authorize(s, ctx, false)?;
            if !s.state.active() || s.closing {
                return Err(err(ErrorCode::Locked, "session is immutable or ending"));
            }
            let name = text(p, "step", MAX_ID_BYTES)?;
            let i = s.step_index(name)?;
            let spec = s.spec(i);
            if overridden
                && !(ctx.role() == Role::Viewer && ctx.rights().is_some_and(|r| r >= Rights::Approve))
            {
                return Err(err(
                    ErrorCode::Denied,
                    "override requires a viewer with approve rights; role comes from CallCtx",
                ));
            }
            if overridden && reason.is_none() {
                return Err(usage("override requires reason"));
            }
            let user_assertion = ctx.role() == Role::Viewer
                && (overridden || spec.template.as_ref().is_some_and(|t| t.manual.is_some()));
            if user_assertion && reason.is_none() {
                return Err(usage("user assertion requires a reason"));
            }
            if proposed
                .is_some_and(|v| matches!(v, Verdict::Inconclusive | Verdict::Blocked | Verdict::Skipped))
                && reason.is_none()
            {
                return Err(usage("inconclusive/blocked/skipped requires reason"));
            }
            if proposed == Some(Verdict::Skipped) && ctx.role() != Role::Viewer && !condition_false(s, i) {
                return Err(err(
                    ErrorCode::Denied,
                    "agent skip is only valid for an unsatisfied planned branch",
                ));
            }
            if proposed.is_some() && ctx.role() != Role::Viewer && s.state == SessionState::Paused {
                return Err(err(ErrorCode::SessionPaused, "session is paused"));
            }
            if s.steps[i].window.attempt == 0 && !user_assertion && proposed != Some(Verdict::Skipped) {
                return Err(err(ErrorCode::OutOfWindow, "step has no attempt window"));
            }
            let citations = if let Some(value) = p.get("evidence") {
                parse_citations(value, &spec)?
            } else if method == "test.evidence.add" {
                if p.get("ref").is_some() || p.get("range").is_some() {
                    parse_citations(&json!([p]), &spec)?
                } else {
                    return Err(usage("evidence.add requires evidence/ref/range"));
                }
            } else {
                s.steps[i].evidence.clone()
            };
            let prior = s.steps[i]
                .accepted
                .as_ref()
                .map(|r| json!({"effective":r.effective,"by":r.by}));
            if proposed == Some(Verdict::Pass) && citations.is_empty() && !user_assertion {
                let row = rejected_record(
                    ctx,
                    proposed.expect("checked"),
                    reason.clone(),
                    Vec::new(),
                    (overridden, prior.clone()),
                    Rejection {
                        code: ErrorCode::EvidenceMissing,
                        message: "PASS needs copied evidence references".into(),
                    },
                    &spec,
                );
                self.record(s, ctx, i, row)?;
                return Err(err(
                    ErrorCode::EvidenceMissing,
                    "PASS needs at least one citation",
                ));
            }
            if let Some(proposed) = proposed
                && self.shared.hooks.recording_health() == RecordingHealth::Failed
                && matches!(proposed, Verdict::Pass | Verdict::Done)
            {
                let row = rejected_record(
                    ctx,
                    proposed,
                    reason.clone(),
                    references(&citations),
                    (overridden, prior.clone()),
                    Rejection {
                        code: ErrorCode::RecordingFailed,
                        message: "record health is failed".into(),
                    },
                    &spec,
                );
                self.record(s, ctx, i, row)?;
                return Err(err(ErrorCode::RecordingFailed, "record health is failed"));
            }
            (id, i, spec, citations, user_assertion)
        };
        let permit = self
            .shared
            .checks
            .clone()
            .try_acquire_owned()
            .map_err(|_| err(ErrorCode::Locked, "two verification calls already running"))?;
        let verifier = self.shared.verifier.clone();
        let call = ctx.clone();
        let now_ns = self.now();
        let thread_spec = spec.clone();
        let thread_citations = citations.clone();
        let thread_id = id.clone();
        let (mut outcome, mut decision) = tokio::task::spawn_blocking(move || {
            let _permit: OwnedSemaphorePermit = permit;
            let mut check_ctx = CheckCtx {
                call,
                session_id: thread_id,
                step_id: thread_spec.definition.step_id.clone(),
                revision: thread_spec.revision,
                plan_hash: thread_spec.plan_hash.clone(),
                window: thread_spec.window.clone(),
                now_ns,
                budget: loom_domain_api::EvalBudget::default(),
                auto,
                user_assertion,
            };
            let mut outcome = if thread_citations.is_empty()
                && !auto
                && proposed.is_some_and(|p| !matches!(p, Verdict::Pass | Verdict::Done))
            {
                CheckOutcome {
                    checks: Vec::new(),
                    all_expectations_verified: false,
                    stimulus_confirmed: false,
                    verified_iterations: 0,
                    error: None,
                }
            } else {
                verifier.check(&thread_spec, &thread_citations, &mut check_ctx)
            };
            let mut decision = proposed.map(|p| verifier.decide(&thread_spec, p, &outcome, &mut check_ctx));
            if check_ctx.budget.spend(0).is_err() {
                outcome.error = Some(err(ErrorCode::Partial, "verification eval_budget exceeded"));
                if proposed == Some(Verdict::Pass) {
                    let mut d = Decision::accept(Verdict::Inconclusive);
                    d.error = Some(err(ErrorCode::Partial, "verification eval_budget exceeded"));
                    decision = Some(d);
                }
            }
            (outcome, decision)
        })
        .await
        .map_err(|_| err(ErrorCode::Internal, "verifier task panicked; nothing confirmed"))?;
        if Instant::now() >= ctx.deadline() {
            return Err(err(ErrorCode::Timeout, "verification RPC deadline expired"));
        }
        validate_outcome(&outcome, auto)?;
        if let Some(d) = &mut decision {
            if d.effective
                .is_some_and(|v| matches!(v, Verdict::Pass | Verdict::Done))
                && d.effective != proposed
            {
                *d = Decision::reject(
                    ErrorCode::Internal,
                    "verifier cannot upgrade a different proposal to PASS/done",
                );
            }
            if d.rejected.is_some() {
                d.effective = None;
            }
            if d.effective.is_none() && d.rejected.is_none() {
                *d = Decision::reject(ErrorCode::Internal, "verifier decision has no result");
            }
        }
        if let Some(proof) = decision
            .as_ref()
            .and_then(|d| d.effective)
            .filter(|v| matches!(v, Verdict::Pass | Verdict::Done))
        {
            let guard = if self.stopped() {
                Some((ErrorCode::Stopped, "global STOP"))
            } else if self.shared.hooks.recording_health() == RecordingHealth::Failed {
                Some((ErrorCode::RecordingFailed, "recording failed during verification"))
            } else if self.shared.hooks.recording_health() != RecordingHealth::Healthy {
                Some((ErrorCode::NotImplemented, "record health adapter is not wired"))
            } else if !user_assertion {
                if spec.window.start.is_none() {
                    Some((
                        ErrorCode::NotImplemented,
                        "CORE source fence adapter is not wired",
                    ))
                } else if spec.plan_hash.is_none() {
                    Some((
                        ErrorCode::PlanMismatch,
                        "automatic verdict needs a registered plan",
                    ))
                } else if let Some(e) = &outcome.error {
                    Some((e.code, "verifier returned an error"))
                } else if spec
                    .template
                    .as_ref()
                    .is_some_and(|t| outcome.verified_iterations < t.repeat_count)
                {
                    Some((
                        ErrorCode::EvidenceMismatch,
                        "not all repeat iterations were verified",
                    ))
                } else if proof == Verdict::Pass && spec.template.as_ref().is_none_or(|t| t.expect.is_empty())
                {
                    Some((
                        ErrorCode::EvidenceMissing,
                        "stimulus-only steps use confirmed done, not pass",
                    ))
                } else if proof == Verdict::Pass
                    && (!outcome.all_expectations_verified || !valid_pass(&spec, &outcome, ctx))
                {
                    Some((
                        ErrorCode::EvidenceMismatch,
                        "expectations/witness roles were not all verified",
                    ))
                } else if proof == Verdict::Done
                    && (!outcome.stimulus_confirmed
                        || citations.is_empty()
                        || spec.template.as_ref().is_none_or(|t| !t.expect.is_empty()))
                {
                    Some((
                        ErrorCode::EvidenceMissing,
                        "done needs a confirmed write reference and no observation expect",
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((code, message)) = guard {
                decision = Some(Decision::reject(code, message));
            }
        }
        if outcome.checks.is_empty()
            && let Some(e) = &outcome.error
        {
            outcome.checks.push(EvidenceCheck {
                evidence_id: "unwired".into(),
                reference: citations
                    .first()
                    .and_then(|c| c.reference.clone())
                    .unwrap_or_default(),
                status: EvidenceStatus::Error,
                observed: Value::Null,
                cause: Some(e.message.clone()),
                complete: false,
                gap_in_window: false,
                digest: None,
                matched_refs: Vec::new(),
                witness: Witness::Unknown,
                expectation: 0,
            });
        }
        let mut state = lock(&self.shared.state);
        let s = state
            .sessions
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::NotFound, "session no longer retained"))?;
        if s.revision != spec.revision
            || s.steps.get(i).is_none_or(|s| s.window != spec.window)
            || !s.state.active()
            || s.closing
        {
            return Err(err(
                ErrorCode::PlanMismatch,
                "plan/attempt window changed while verification ran",
            ));
        }
        if proposed.is_some() && s.state == SessionState::Paused && ctx.role() != Role::Viewer {
            return Err(err(
                ErrorCode::SessionPaused,
                "session paused while verification ran",
            ));
        }
        for check in &outcome.checks {
            let mut payload = serde_json::to_value(check).map_err(|e| usage(e.to_string()))?;
            payload["step_id"] = json!(spec.definition.step_id);
            payload["attempt"] = json!(spec.window.attempt);
            self.publish_kept(ctx, s, "test.evidence_checked", payload.clone())?;
            s.recent.push_back(snapshot::evidence_row(&payload));
            if s.recent.len() > MAX_RECENT_EVIDENCE {
                s.recent.pop_front();
            }
        }
        if method == "test.evidence.add" {
            let mut combined = s.steps[i].evidence.clone();
            for c in &citations {
                if !combined.contains(c) {
                    combined.push(c.clone());
                }
            }
            if combined.len() > MAX_CITATIONS {
                return Err(usage("step evidence limit is 64"));
            }
            let bytes = serde_json::to_value(&combined).map_err(|e| usage(e.to_string()))?;
            self.keep(s, &bytes)?;
            s.steps[i].evidence = combined;
        }
        // Check the current accepted verdict at commit, including competing in-flight proposals.
        // Omitting override:true must not turn an agent into an authorized override actor.
        if s.steps[i].accepted.is_some()
            && !overridden
            && decision.as_ref().is_some_and(|d| d.effective.is_some())
        {
            decision = Some(Decision::reject(
                ErrorCode::Denied,
                "revising an accepted verdict requires an explicit viewer approve override",
            ));
        }
        let mut data = json!({"session_id":id,"step_id":spec.definition.step_id,"checks":outcome.checks,"auto":auto,"revision":s.revision,"attempt":spec.window.attempt,"during_pause":s.state==SessionState::Paused});
        if let Some(proposed) = proposed {
            let d = decision.ok_or_else(|| err(ErrorCode::Internal, "verifier returned no decision"))?;
            let record = VerdictRecord {
                proposed,
                effective: d.effective,
                by: ctx.client_id().into(),
                reason,
                evidence_refs: references(&citations),
                overridden,
                previous: s.steps[i]
                    .accepted
                    .as_ref()
                    .map(|r| json!({"effective":r.effective,"by":r.by})),
                rejected: d.rejected,
                unsupported: d.unsupported,
                user_assertion,
                attempt: spec.window.attempt,
                revision: spec.revision,
            };
            self.record(s, ctx, i, record.clone())?;
            data["verdict"] = serde_json::to_value(&record).map_err(|e| usage(e.to_string()))?;
            if let Some(e) = d.error {
                return Err(e.with_detail(data));
            }
            if record.effective == Some(Verdict::Inconclusive) && proposed == Verdict::Pass {
                return Err(err(ErrorCode::Partial, "PASS downgraded to inconclusive").with_detail(data));
            }
        } else if let Some(e) = outcome.error {
            return Err(e.with_detail(data));
        }
        Ok(data)
    }
}
fn rejected_record(
    ctx: &CallCtx,
    proposed: Verdict,
    reason: Option<String>,
    refs: Vec<String>,
    previous: (bool, Option<Value>),
    rejection: Rejection,
    spec: &StepSpec,
) -> VerdictRecord {
    VerdictRecord {
        proposed,
        effective: None,
        by: ctx.client_id().into(),
        reason,
        evidence_refs: refs,
        overridden: previous.0,
        previous: previous.1,
        rejected: Some(rejection),
        unsupported: false,
        user_assertion: false,
        attempt: spec.window.attempt,
        revision: spec.revision,
    }
}
fn references(c: &[Citation]) -> Vec<String> {
    c.iter().filter_map(|c| c.reference.clone()).collect()
}
fn validate_range(v: &Value) -> Result<(), DomainError> {
    if let Some(s) = v.as_str() {
        if let Some(seq) = s.strip_prefix("seq:") {
            let (from, to) = seq.split_once("..").unwrap_or((seq, seq));
            let from = loom_proto::num::parse_u64_canonical(from).map_err(|_| usage("invalid seq cursor"))?;
            let to = loom_proto::num::parse_u64_canonical(to).map_err(|_| usage("invalid seq cursor"))?;
            if from > to {
                return Err(usage("reversed seq cursor"));
            }
        } else if let Some(id) = s.strip_prefix("@step:").or_else(|| s.strip_prefix("mark:")) {
            if id.is_empty()
                || id.len() > MAX_ID_BYTES
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            {
                return Err(usage("invalid step/mark cursor"));
            }
        } else {
            return Err(usage("expected seq:, mark: or @step: cursor"));
        }
    } else {
        let obj = v
            .as_object()
            .filter(|o| o.len() == 3)
            .ok_or_else(|| usage("range needs epoch, from_seq and to_seq"))?;
        let num = |key: &str| -> Result<u64, DomainError> {
            let s = obj
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| usage(format!("range.{key} must be U64 string")))?;
            loom_proto::num::parse_u64_canonical(s).map_err(|_| usage("invalid range U64"))
        };
        num("epoch")?;
        if num("from_seq")? > num("to_seq")? {
            return Err(usage("reversed evidence range"));
        }
    }
    Ok(())
}
fn parse_citations(value: &Value, spec: &StepSpec) -> Result<Vec<Citation>, DomainError> {
    let a = value
        .as_array()
        .filter(|a| a.len() <= MAX_CITATIONS)
        .ok_or_else(|| usage("evidence must contain at most 64 citations"))?;
    let mut out = Vec::new();
    for v in a {
        validate_json(v)?;
        json_len(v, MAX_CITATION_BYTES)?;
        let reference = if let Some(s) = v.as_str() {
            s.parse::<loom_proto::evidence::RefString>()
                .map_err(|e| usage(e.to_string()))?;
            Some(s.into())
        } else if let Some(s) = v.get("ref").and_then(Value::as_str) {
            s.parse::<loom_proto::evidence::RefString>()
                .map_err(|e| usage(e.to_string()))?;
            Some(s.into())
        } else if v.get("ref").is_some() {
            let e: loom_proto::evidence::Evidence =
                serde_json::from_value(v.clone()).map_err(|e| usage(e.to_string()))?;
            if e.r#ref.seq_range.from > e.r#ref.seq_range.to || e.r#ref.source_refs.len() > MAX_CITATIONS {
                return Err(usage("invalid evidence range/source list"));
            }
            None
        } else if v.get("range").is_some() {
            validate_range(&v["range"])?;
            None
        } else {
            return Err(usage("citation needs a result ref or an explicit range"));
        };
        let expectation = match v.get("expectation") {
            None => 0,
            Some(v) => v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| usage("expectation index must be u32"))?,
        };
        if let Some(template) = &spec.template
            && !template.expect.is_empty()
        {
            let e = template
                .expect
                .get(expectation as usize)
                .ok_or_else(|| err(ErrorCode::PlanMismatch, "citation expectation is not registered"))?;
            if let Some(pred) = v.get("pred")
                && pred != &e.pred
            {
                return Err(err(
                    ErrorCode::PlanMismatch,
                    "citation predicate differs from the registered expect",
                ));
            }
        }
        if let Some(hash) = v.get("plan_hash") {
            let hash = hash.as_str().ok_or_else(|| usage("plan_hash must be a string"))?;
            if spec.plan_hash.as_deref() != Some(hash) {
                return Err(err(ErrorCode::PlanMismatch, "citation plan hash differs"));
            }
        }
        if let Some(filter) = v.get("filter") {
            let mut filter = filter.clone();
            let object = filter
                .as_object_mut()
                .ok_or_else(|| usage("filter must be an object"))?;
            if object.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "domain" | "channel" | "kinds" | "origins" | "request_id"
                )
            }) {
                return Err(usage("unknown citation filter field"));
            }
            object.entry("origins").or_insert(json!(["device"]));
            let filter: format::Filter = serde_json::from_value(filter).map_err(|e| usage(e.to_string()))?;
            if let Some(expected) = spec
                .template
                .as_ref()
                .and_then(|t| t.expect.get(expectation as usize))
                && filter != expected.filter
            {
                return Err(err(
                    ErrorCode::PlanMismatch,
                    "citation filter differs from registered expectation",
                ));
            }
        }
        out.push(Citation {
            input: v.clone(),
            reference,
            expectation,
        });
    }
    Ok(out)
}
fn validate_outcome(o: &CheckOutcome, auto: bool) -> Result<(), DomainError> {
    if o.checks.len() > if auto { 20 } else { MAX_CITATIONS } {
        return Err(err(
            ErrorCode::Partial,
            "verifier result exceeds evidence/candidate limit",
        ));
    }
    for c in &o.checks {
        if c.evidence_id.is_empty()
            || c.evidence_id.len() > MAX_ID_BYTES
            || c.reference.len() > 512
            || c.cause.as_ref().is_some_and(|s| s.len() > MAX_TEXT_BYTES)
            || c.matched_refs.len() > MAX_CITATIONS
        {
            return Err(err(
                ErrorCode::Partial,
                "verifier exceeded bounded result contract",
            ));
        }
        json_len(&c.observed, MAX_OBSERVED_BYTES)?;
        for r in c
            .matched_refs
            .iter()
            .chain(std::iter::once(&c.reference))
            .filter(|r| !r.is_empty())
        {
            r.parse::<loom_proto::evidence::RefString>()
                .map_err(|e| err(ErrorCode::Partial, e.to_string()))?;
        }
        if let Some(d) = &c.digest
            && !(d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(err(ErrorCode::Partial, "invalid verifier digest"));
        }
    }
    Ok(())
}
fn valid_pass(spec: &StepSpec, o: &CheckOutcome, ctx: &CallCtx) -> bool {
    let Some(t) = &spec.template else {
        return false;
    };
    if o.checks.is_empty()
        || o.checks
            .iter()
            .any(|c| c.status != EvidenceStatus::Verified || c.digest.is_none())
    {
        return false;
    }
    if !(0..t.expect.len()).all(|i| o.checks.iter().any(|c| c.expectation as usize == i)) {
        return false;
    }
    o.checks.iter().all(|c| match c.witness {
        Witness::Device => !c.matched_refs.is_empty(),
        Witness::CompleteAbsence | Witness::Measurement => c.complete && !c.gap_in_window,
        Witness::UserAssertion => ctx.role() == Role::Viewer && t.manual.is_some(),
        Witness::OwnTxOnly | Witness::Unknown => false,
    })
}
impl TestModule {
    async fn amend(&self, ctx: &CallCtx, p: &Value) -> Result<Value, DomainError> {
        let (id, tc, revision) = {
            let state = lock(&self.shared.state);
            let id = Self::get_id(&state, p)?;
            let s = state
                .sessions
                .get(&id)
                .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
            self.authorize(s, ctx, false)?;
            if !s.state.active() || s.closing {
                return Err(err(ErrorCode::Locked, "ended/ending session is immutable"));
            }
            (id, s.tc.clone(), s.revision)
        };
        let source = p
            .get("plan")
            .ok_or_else(|| usage("plan source is required"))?
            .clone();
        let parse_permit = self
            .shared
            .preparations
            .clone()
            .try_acquire_owned()
            .map_err(|_| err(ErrorCode::Locked, "another plan parse is in progress"))?;
        let memory = Charge::reserve(self.shared.used.clone(), CAP_SESSION_BASE_BYTES)?;
        let commands = self
            .shared
            .commands
            .clone()
            .ok_or_else(|| DomainError::not_implemented("actual command/risk table"))?;
        let parse_tc = tc.clone();
        let (plan, report, _memory) = tokio::task::spawn_blocking(move || {
            let _parse_permit = parse_permit;
            let memory = memory;
            let text = source::load(&source, ".plan.yaml")?;
            let plan = format::parse_plan_with_tc(&text, &parse_tc).map_err(source::format_error)?;
            let report = format::check_plan(&plan, commands.as_ref());
            if !report.is_valid() {
                return Err(err(
                    if report.errors.iter().any(|e| e.code == "blocked_command") {
                        ErrorCode::Denied
                    } else {
                        ErrorCode::Usage
                    },
                    "amended plan check failed",
                )
                .with_detail(serde_json::to_value(report).unwrap_or(Value::Null)));
            }
            if report.requires_legacy_adapter {
                return Err(DomainError::not_implemented("legacy UDS expectation adapter"));
            }
            Ok((plan, report, memory))
        })
        .await
        .map_err(|_| err(ErrorCode::Internal, "plan parser task failed"))??;
        let mut state = lock(&self.shared.state);
        let s = state
            .sessions
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
        if s.revision != revision || !s.state.active() || s.closing {
            return Err(err(ErrorCode::PlanMismatch, "session changed during amend"));
        }
        if let Some(approval) = &s.approval_id {
            ctx.approvals()
                .invalidate(approval, "test plan amendment invalidates all prior grants")?;
        }
        let next = s
            .revision
            .get()
            .checked_add(1)
            .ok_or_else(|| err(ErrorCode::Config, "revision exhausted"))?;
        let was_paused = s.state == SessionState::Paused;
        self.step_end(s, ctx)?;
        let pending = s.asks.keys().cloned().collect::<Vec<_>>();
        for ask in pending {
            self.emit(
                ctx,
                s,
                "test.answer",
                json!({"ask_id":ask,"value":null,"cancelled":true,"reason":"plan_amended"}),
            )?;
            s.asks.remove(&ask);
        }
        let definitions = definitions(&tc, Some(&plan));
        let summary = risks(&report);
        self.emit(ctx,s,"test.session_state",json!({"state":"PLANNED","reason":"plan_amended","plan_changed":true,"revision":U64(next),"plan_hash":plan.plan_sha256(),"steps":definitions.iter().map(|s|&s.definition).collect::<Vec<_>>(),"risk_summary":summary}))?;
        s.steps = definitions;
        s.plan = Some(Arc::new(plan));
        s.report = Some(report);
        s.revision = U64(next);
        s.state = SessionState::Planned;
        s.approval_id = None;
        s.recent.clear();
        s.asks.clear();
        s.finally_done = false;
        s.finally_unknown = false;
        self.session_state(s, ctx, SessionState::WaitingApproval, "new_plan_policy_gate")?;
        if self.shared.config.approval == ApprovalMode::Auto {
            self.session_state(
                s,
                ctx,
                if was_paused {
                    SessionState::Paused
                } else {
                    SessionState::Running
                },
                "F1_auto_new_revision",
            )?;
            Ok(s.plan_view())
        } else {
            Err(err(
                ErrorCode::ApprovalRequired,
                "new plan invalidated previous approval",
            )
            .with_detail(s.plan_view()))
        }
    }
    async fn end(&self, ctx: &mut CallCtx, p: &Value) -> Result<Value, DomainError> {
        let abort = bool_arg(p, "abort", false)?;
        if p.get("run_finally").is_some() {
            bool_arg(p, "run_finally", false)?;
        }
        let (id, revision, run_finally, actions, stopped) = {
            let mut state = lock(&self.shared.state);
            let id = Self::get_id(&state, p)?;
            let s = state
                .sessions
                .get_mut(&id)
                .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
            self.authorize(s, ctx, false)?;
            if let Some(result) = s.result {
                return end_result(s, result, s.state == SessionState::Stopped);
            }
            if s.closing {
                return Err(err(ErrorCode::Locked, "session end already in progress"));
            }
            let stopped = self.stopped() || s.state == SessionState::Stopped;
            let actions = s.plan.as_ref().map(|p| p.finally().to_vec()).unwrap_or_default();
            let run_finally = if stopped || s.finally_done {
                false
            } else if abort {
                if ctx.role() == Role::Viewer {
                    if !p.get("run_finally").is_some_and(Value::is_boolean) {
                        return Err(usage("viewer abort requires explicit run_finally choice"));
                    }
                    bool_arg(p, "run_finally", false)?
                } else {
                    if p.get("run_finally") == Some(&Value::Bool(false)) && !actions.is_empty() {
                        return Err(err(
                            ErrorCode::Denied,
                            "only a viewer can choose to skip planned finally",
                        ));
                    }
                    !actions.is_empty()
                }
            } else {
                if p.get("run_finally") == Some(&Value::Bool(false)) && !actions.is_empty() {
                    return Err(usage(
                        "normal end must run planned finally; use explicit abort choice",
                    ));
                }
                !actions.is_empty()
            };
            if run_finally && s.finally_unknown {
                return Err(err(
                    ErrorCode::Partial,
                    "prior finally outcome is unknown; inspect its journal, do not replay automatically",
                ));
            }
            self.step_end(s, ctx)?;
            if run_finally {
                self.session_state(s, ctx, SessionState::Paused, "finally_dispatch_pending")?;
                s.finally_unknown = true;
            }
            s.closing = true;
            (id, s.revision, run_finally, actions, stopped)
        };
        let mut guard = EndGuard {
            module: self.clone(),
            id: id.clone(),
            revision,
            armed: true,
        };
        let finally = if run_finally {
            match self.shared.hooks.finally(ctx, &id, actions).await {
                Ok(value) => Some(value),
                Err(error) => {
                    if error.code == ErrorCode::NotImplemented
                        && let Some(s) = lock(&self.shared.state).sessions.get_mut(&id)
                    {
                        s.finally_unknown = false;
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        if let Some(v) = &finally {
            json_len(v, MAX_EVENT_BYTES)?;
            if v.get("complete") != Some(&Value::Bool(true)) {
                return Err(err(
                    ErrorCode::Partial,
                    "finally dispatcher did not report complete; session remains paused",
                ));
            }
        }
        let mut state = lock(&self.shared.state);
        let s = state
            .sessions
            .get_mut(&id)
            .ok_or_else(|| err(ErrorCode::NotFound, "session not retained"))?;
        if s.revision != revision {
            return Err(err(ErrorCode::PlanMismatch, "plan changed during end"));
        }
        if run_finally {
            s.finally_done = true;
            s.finally_unknown = false;
        }
        let stopped = stopped || self.stopped() || s.state == SessionState::Stopped;
        let result = if stopped || abort {
            SessionResult::Incomplete
        } else {
            aggregate(s)
        };
        let reason = if stopped {
            "stopped"
        } else if abort {
            "aborted"
        } else {
            "completed"
        };
        let halted = stopped || abort;
        self.emit(ctx,s,"test.session_ended",json!({"result":result,"reason":reason,"exit_code":if halted{6}else{result.exit_code()},"finally":finally,"finally_skipped":!run_finally&&!s.finally_done,"state":if halted{SessionState::Stopped}else{SessionState::Done}}))?;
        s.state = if halted {
            SessionState::Stopped
        } else {
            SessionState::Done
        };
        s.result = Some(result);
        s.current = None;
        s.asks.clear();
        s.closing = false;
        guard.armed = false;
        let output = end_result(s, result, halted);
        if !state.completed.contains(&id) {
            state.completed.push_back(id);
        }
        self.shared.notify.notify_waiters();
        output
    }
}
struct EndGuard {
    module: TestModule,
    id: String,
    revision: U64,
    armed: bool,
}
impl Drop for EndGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Some(s) = lock(&self.module.shared.state).sessions.get_mut(&self.id)
                && s.revision == self.revision
            {
                s.closing = false;
            }
            self.module.shared.notify.notify_waiters();
        }
    }
}
/// Session result (R-6, R-9) through `loom_test_predicate::verdict::aggregate`. The plan path is
/// walked as the engine runs it: `when`-false steps are not required, `on_fail: goto` jumps to its
/// recovery step (the failure is replaced only when every recovery step passed), steps jumped over
/// are not required, a step attempted off the path counts as inconclusive, a cycle is inconclusive.
fn aggregate(s: &Session) -> SessionResult {
    use loom_test_predicate::verdict::{self as v, Final, StepState as Agg, StepVerdict};
    if s.steps.is_empty() {
        return SessionResult::Incomplete;
    }
    let mut on_path = std::collections::BTreeSet::new();
    let mut visited = std::collections::BTreeSet::new();
    let mut i = 0;
    while i < s.steps.len() {
        if !visited.insert(i) {
            return SessionResult::Inconclusive;
        }
        if condition_false(s, i) {
            i += 1;
            continue;
        }
        on_path.insert(i);
        let step = &s.steps[i];
        if step.accepted.as_ref().and_then(|r| r.effective) == Some(Verdict::Fail)
            && let Some(FailureAction::Goto(target)) = step.template.as_ref().map(|t| &t.on_fail)
            && let Ok(j) = s.step_index(target)
        {
            i = j;
            continue;
        }
        i += 1;
    }
    let states: Vec<Agg> = s
        .steps
        .iter()
        .enumerate()
        .filter_map(|(i, step)| {
            let off_path_attempt = !on_path.contains(&i) && step.window.attempt > 0 && !condition_false(s, i);
            if !on_path.contains(&i) && !off_path_attempt {
                return None;
            }
            let verdict = if off_path_attempt {
                Some(StepVerdict::Inconclusive("attempted off the plan path".into()))
            } else {
                step.accepted.as_ref().and_then(|r| r.effective).map(|e| match e {
                    Verdict::Pass => StepVerdict::Pass,
                    Verdict::Done => StepVerdict::Done,
                    Verdict::Fail => StepVerdict::Fail {
                        unsupported: step.accepted.as_ref().is_some_and(|r| r.unsupported),
                    },
                    Verdict::Inconclusive => StepVerdict::Inconclusive("verdict".into()),
                    Verdict::Blocked => StepVerdict::Blocked("verdict".into()),
                    // Plan-branch skips are `when`-false and never reach here: a recorded skip is the user's.
                    Verdict::Skipped => StepVerdict::Skipped { by_user: true },
                })
            };
            let recovery = match step.template.as_ref().map(|t| &t.on_fail) {
                Some(FailureAction::Goto(t)) => vec![t.clone()],
                _ => Vec::new(),
            };
            Some(Agg {
                id: step.definition.step_id.clone(),
                required: true,
                verdict: verdict.map(Final::Auto),
                recovery,
            })
        })
        .collect();
    match v::aggregate(&states, false).map(|r| r.outcome) {
        Ok(v::Outcome::Passed) => SessionResult::Passed,
        Ok(v::Outcome::Failed) => SessionResult::Failed,
        Ok(v::Outcome::Incomplete) => SessionResult::Incomplete,
        Ok(v::Outcome::Inconclusive) | Err(_) => SessionResult::Inconclusive,
    }
}

fn end_result(s: &Session, result: SessionResult, stopped: bool) -> Result<Value, DomainError> {
    let data = json!({"session_id":s.id,"state":s.state,"result":result,"exit_code":if stopped{6}else{result.exit_code()},"report_available":false});
    let code = if stopped {
        Some(ErrorCode::Stopped)
    } else {
        match result {
            SessionResult::Passed => None,
            SessionResult::Failed => Some(ErrorCode::TestFailed),
            _ => Some(ErrorCode::Partial),
        }
    };
    if let Some(code) = code {
        Err(err(code, "TEST session ended; see result").with_detail(data))
    } else {
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_domain_api::{
        ApprovalHandle, BoxFuture, CtxServices, JournalQuery, JournalReader, ScanResult, WorkerHandle,
    };
    #[derive(Debug)]
    struct Services;
    impl JournalReader for Services {
        fn scan<'a>(&'a self, _: &'a JournalQuery, _: u64) -> BoxFuture<'a, Result<ScanResult, DomainError>> {
            Box::pin(async { Err(DomainError::not_implemented("queue test has no journal")) })
        }
    }
    impl ApprovalHandle for Services {
        fn invalidate(&self, _: &str, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }
    impl CtxServices for Services {
        fn worker(&self, _: &str) -> Result<&dyn WorkerHandle, DomainError> {
            Err(DomainError::not_implemented("queue test has no worker"))
        }
        fn publish(&self, _: EventDraft) -> Result<(), DomainError> {
            Ok(())
        }
        fn journal(&self) -> &dyn JournalReader {
            self
        }
        fn approvals(&self) -> &dyn ApprovalHandle {
            self
        }
    }
    #[tokio::test]
    async fn full_control_queue_returns_backpressure_and_recovers_without_a_drop() {
        let module = TestModule::new();
        let all = module
            .shared
            .control
            .clone()
            .try_acquire_many_owned(MAX_CONTROL_REQUESTS as u32)
            .expect("all slots");
        let mut ctx = CallCtx::new(
            Role::Agent,
            None,
            "agent",
            None,
            Instant::now() + Duration::from_secs(1),
            Arc::new(Services),
        );
        assert_eq!(
            module
                .call(&mut ctx, "test.status", json!({}))
                .await
                .expect_err("full queue")
                .code,
            ErrorCode::Locked
        );
        assert_eq!(module.accounted_bytes(), 0);
        drop(all);
        assert!(module.call(&mut ctx, "test.status", json!({})).await.is_ok());
        assert_eq!(module.accounted_bytes(), 0);
    }
}
