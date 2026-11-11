docs/plan/02-core-principle.md 준수

# P0 Day 2 — core-opus: compile-only trait crate 3개

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`. 전제: `loom-proto`(Day 1) 완료·리드 커밋 후. 근거: `docs/contract/08-domain-integration.md`, `05-worker-protocol.md`, `04-safety.md` §4.3, `docs/plan/core/plan.md` §3.9·§4.1·§10.3.

## 목표
도메인 팀이 `core-p0-d2` 태그 직후 **자기 crate를 이 trait에 맞춰 compile** 할 수 있게 한다. 구현은 거의 없다(compile-only). 동작은 P1. 단, 시그니처는 P1 구현이 바꾸지 않아도 되게 신중히.

## 공통
- 각 crate `crates/<name>/Cargo.toml`은 `loom-proto`의 형식을 그대로 따른다(`version.workspace = true` 등, `[lints] workspace = true`). 의존은 `[workspace.dependencies]`만. **clap은 리드가 workspace에 `clap = { version = "4.6.7", features = ["derive"] }`로 추가해 둔다** — 그 밖에 필요한 것은 보고.
- async: 외부 crate(async-trait) 없이 `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>`로 dyn-호환 trait를 만든다(`loom-domain-api`에 정의, worker-sdk도 재사용하거나 같은 별칭).
- tokio 의존 금지(이 3 crate는 런타임 중립). 동기 훅은 진짜 동기.
- 미구현 함수 본문은 `Err(...E_NOT_IMPLEMENTED...)` 또는 `unimplemented!` 대신 **오류 반환**. `todo!`는 lint로 금지.

## 1. `crates/loom-domain-api`
- `DomainModule`(NAME, API_VERSION, `register(&self, &mut Registry) -> Result<(), RegisterError>`).
- `Registry`(구체 struct, 등록 내용을 담기만 함 — 필드 비공개 + 조회용 `pub fn commands()` 등 읽기 접근자. 이름 검증은 `loom_proto::rpc::validate_method`·`event::validate_event_kind`로 **실제로** 수행 — 이건 구현한다, 순수 함수라서).
  - `command(CommandSpec)`, `event_kind(EventKindSpec{kind, payload_schema: schemars::Schema, pointers: Vec<PointerSpec{path, unit, ty}>, version})`, `risk_table(RiskTable)`, `hooks(Arc<dyn WriteHooks>)`, `worker(WorkerSpec{kind, instances: Instances::{PerChannel{max}, Shared}, budgets: Budgets{mem_soft, mem_hard, ...}, reopen_after_crash: Reopen::{None, RxOnly}, arch, platforms})`, `runtime(ModuleRuntime::{Shared, Dedicated{threads}})`, `reducer(Box<dyn Reducer>)`, `evidence_verifier(Arc<dyn EvidenceVerifier>)`, `display_summarizer`, `capability`, `safe_state(SafeStateSpec)`, `emergency_off(EmergencyOffSpec)`, `interlock(name, Arc<dyn Fn() -> bool + Send + Sync>)`, `panel_manifest(serde_json::Value)`, `policy_section(schemars::Schema)`, `help(HelpSpec)`.
- `CommandSpec{method, write_class: Option<WriteClass>, params: Schema, result: Schema, roles, handler: Arc<dyn Handler>, bounds: Bounds{max_wait, default_rows, max_rows}}` — F1이므로 **R2 경로 기본값 필드는 두지 않는다**(정책은 설정). 대신 `default_action: Option<PolicyAction>` 정도는 도메인이 더 엄격한 기본을 제안할 수 있게 허용(기본 None = Auto).
- `WriteHooks`(plan §3.9 시그니처, `within_pattern` 기본 false) + `WriteRequest<'a>`, `Classification`, `Binding`, `PresendError`, `Grant`, `PatternBinding`.
- `EvidenceVerifier{predicates, coverage_need, verify(..., budget: &mut EvalBudget)}`, `CoverageNeed{Positive, Complete, Measure}`, `Verification`, `Coverage`, `EvalBudget`(협력 예산: `fn spend(&mut self, events: usize) -> Result<(), BudgetExceeded>`).
- `Reducer{kinds, apply(&mut self, &PublishedRecord), snapshot(&self, &SubFilter) -> Vec<Value>}`, `DisplaySummarizer`.
- `Handler{fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>>}`.
- `CallCtx`(struct, 내부는 trait 객체 핸들들 — P0는 시그니처만): `origin()`, `client_id()`, `request_id()`, `deadline()`, `worker(kind) -> &dyn WorkerHandle`, `publish(EventDraft)`, `journal() -> &dyn JournalReader`, `approvals() -> &dyn ApprovalHandle`. `JournalReader::scan(&self, query: &JournalQuery, byte_limit: u64) -> BoxFuture<Result<ScanResult, DomainError>>`.
- `DomainError{code: loom_proto::error::ErrorCode, message, detail}`.
- 시험: 가짜 도메인 하나가 `register`로 모든 항목을 등록 → 잘못된 메서드 이름·이벤트 이름은 `RegisterError`.

## 2. `crates/loom-worker-sdk`
- `Worker` trait(KIND, `on_call`, `on_write`, `on_job`, `on_input`, `on_latch`) — BoxFuture 기반. `WorkerCtx{emitter(channel), latch() -> &LatchFlag, clock() -> &dyn Clock}`.
- **`Emitter`는 동기·non-blocking**: `rx(kind, ts, payload) -> Result<SourceSeq, EmitError>`, `priority(EventDraft) -> Result<…>`, `source_gap(cause, GapCount)`. `EmitError::{Full, Closed}` — 도메인 hot path는 실패를 받고 카운트만 한다(block 금지).
- `LatchFlag`(AtomicBool 래퍼 — 이건 구현), `RateGuard`(토큰 버킷 — 구현, 순수), `Clock` trait + `SystemClock`, `SimClock`.
- `run_worker<W: Worker>(w: W) -> !` 시그니처는 두되, P0 본문은 "아직 구현되지 않음" 오류 출력 후 종료 코드 1(`std::process::exit(1)`). 실제 IPC는 P1(C12).
- 프레이밍 코덱: `loom_proto::worker`의 헤더 함수를 감싸는 `encode_frame(msg: &WorkerMessage) -> Result<Vec<u8>, _>`/`decode_frame(&[u8]) -> Result<(WorkerMessage, usize), _>`(rmp-serde named) — **구현**(순수 함수, 시험 포함: 왕복, 64 KiB 초과 거절, 잘린 입력). rmp-serde는 이 crate의 일반 의존으로(workspace에 이미 있음).
- 시험: LatchFlag·RateGuard·프레임 코덱.

## 3. `crates/loom-cli-kit`
- `DomainCli` trait: `fn command() -> clap::Command`, `fn to_call(&clap::ArgMatches) -> Result<RpcCall, UsageError>`, `fn render_human(&RpcCall, &Envelope<Value>, &mut dyn io::Write) -> io::Result<()>`. dyn 용 `DynDomainCli` 래퍼.
- 순수 함수 구현(P0에 동작해야 하는 것): `exit_code_for(&Envelope<_>) -> u8`(ok면 0, 아니면 `error.code.exit_code()`), 공통 출력 상한 상수(1,000행/64 KiB), 사람용 `NOTE:`/`WARNING:`/`hint:` 줄 렌더러(`others_since_last`·warnings에서), `parse_cursor_arg`(proto Cursor 위임), `client_id()`(env `LOOM_CLIENT` 또는 부모 세션 기반 안정 id — P0는 env만, 없으면 `"anon-<uid>"` 류).
- `run_cli(domains: &[&dyn DynDomainCli]) -> std::process::ExitCode` 시그니처, P0 본문은 `E_NOT_IMPLEMENTED` 봉투를 출력하고 exit 1 (`--json`이면 JSON).
- 시험: exit 코드 매핑, NOTE 렌더링 스냅샷(문자열 비교), 커서 인자.

## 품질·보고
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 전체 workspace 통과. 커밋하지 않는다. 보고는 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p0-d2-opus.md`(만든 것·시험 수·시그니처 결정 중 계약 문서와 다른 점·도메인 팀이 알아야 할 사용 예 10줄 이내). 화면에는 경로와 3줄 요약만.
