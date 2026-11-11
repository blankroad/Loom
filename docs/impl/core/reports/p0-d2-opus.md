# P0 D2 보고 — core-opus: compile-only trait crate 3개

작업 위치: `.worktrees/core`(브랜치 `core/main`, 기준 커밋 `3ab73b6`). **커밋하지 않음.** 새 파일: `crates/loom-domain-api/`, `crates/loom-worker-sdk/`, `crates/loom-cli-kit/`(+ `Cargo.lock` 갱신). 루트 `Cargo.toml`·`loom-proto`는 수정하지 않음.

## 만든 것
- **`loom-domain-api`**: `BoxFuture`, `DomainError`(`not_implemented`), `DomainModule`, **`Registry`(실제 이름·한도 검증)** + 스펙 타입(`CommandSpec{… roles: RoleSet, bounds: Bounds, default_action: Option<PolicyAction>}`, `EventKindSpec{payload_schema, pointers: Vec<PointerSpec{path, unit, ty: ValueType}>, version}`, `RiskTable`, `WorkerSpec{instances: PerChannel{max}|Shared, budgets, reopen_after_crash: None|RxOnly, arch, platforms}`, `ModuleRuntime`, `SafeStateSpec`, `EmergencyOffSpec`, `HelpSpec`), `WriteHooks`(동기 4개, `within_pattern` 기본 false) + `WriteRequest/Classification/Binding/Grant/PatternBinding/PresendError`, `EvidenceVerifier`/`CoverageNeed`/`Verification`/`Coverage`/**`EvalBudget`(협력 예산, 구현)**, `Reducer`, `DisplaySummarizer`, `Handler`, `CallCtx` + `CtxServices`/`WorkerHandle`/`JournalReader`/`ApprovalHandle`/`EventDraft`/`JournalQuery`/`ScanResult`.
- **`loom-worker-sdk`**: `Worker` trait(BoxFuture), `WorkerCtx`, `JobCmd`, `WriteOutcome`, **`Emitter`(동기·non-blocking, 구현)** + `EmitSink`·`MemorySink`, **`LatchFlag`·`RateGuard`(구현)**, `Clock`·`SystemClock`·`SimClock`, **프레임 코덱 `encode_frame`/`decode_frame`(구현)**, `run_worker`(P0: 오류 출력 후 exit 1).
- **`loom-cli-kit`**: `DomainCli` + `DynDomainCli` + `CliOf<T>`, `RpcCall`, `UsageError`, `exit_code_for`, 상한 상수(1,000행·64 KiB·600 s·3600 s), `render_common`(`error[…]`/`hint:`/`NOTE:`/`WARNING:`), `parse_cursor_arg`, **`cursor_arg(name)`**, `client_id`/`client_id_from`, `root_command`, `run_cli`/`run_cli_with`(P0: 인자 검사 후 `E_NOT_IMPLEMENTED` exit 1, 사용법 오류는 `E_USAGE` exit 2).

## 시험
워크스페이스 전체 **45개 통과**(loom-proto 31 + domain-api 3 + worker-sdk 6 + cli-kit 5). `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS=-D warnings cargo doc --workspace` 통과.
- domain-api: 가짜 도메인이 모든 항목 등록 / 잘못된 메서드·이벤트·포인터·도메인 귀속·중복·한도(3600 s, 1000행, budgets)·예약 domain 거절 / `EvalBudget`·`PresendError` 코드.
- worker-sdk: LatchFlag 공유, RateGuard(버스트·충전·시간 역행), 프레임 왕복·연속 스트림·잘린 입력 전 구간 Incomplete·64 KiB 초과 거절·헤더/본문 타입 불일치·쓰레기 본문, Emitter(lane·seq·Full 시 seq 구멍·source.gap).
- cli-kit: 종료 코드 1–11, NOTE/WARNING 문자열 스냅숏, 커서 인자, client id, `run_cli_with`(JSON·사람용·사용법 오류·`--since -5s`).

## 계약 문서·지시서와 다른 점 (결정한 세부)
1. `Registry`의 모든 등록 메서드는 `Result<&mut Self, RegisterError>`(`?` 체이닝, P1에서 검증이 늘어도 시그니처 불변). 등록 메서드와 이름이 겹치는 조회자는 `_ref` 접미사(`risk_table_ref()`, `hooks_ref()` …), `runtime_choice()`.
2. `Registry::new`는 예약 domain 8개를 거절, CORE 모듈용 `Registry::for_core`를 둠. 이벤트 kind·검증 술어·interlock·safe_state 이름은 `<자기 domain>.<name>`이어야 함. 포인터는 `/payload/`·`/metrics/` 아래만.
3. `CallCtx::worker(kind)`는 `Result<&dyn WorkerHandle, DomainError>`(없는 worker를 표현해야 해서). `WorkerHandle`은 `call`·`write` 두 메서드.
4. `EventDraft`가 **두 개**다: gateway용(`loom-domain-api`, origin·시각은 코어가 채움)과 worker용(`loom-worker-sdk`, ts·origin 포함). 공통화가 필요하면 P1에 `loom-proto`로 옮기자.
5. worker-sdk는 `BoxFuture`를 자체 정의(같은 모양). worker-sdk → domain-api 의존을 피함.
6. `Emitter`는 `Full`이어도 source_seq를 소비한다 — 손실이 항상 seq 구멍으로 드러남. `MemorySink`는 `try_lock` 실패도 `Full`로 처리(block 없음).
7. **`SystemClock.mono_ns`는 P0에서 프로세스 기준**(`Instant` 경과값, `clock_id = proc-<pid>`). 호스트 전역 monotonic(QPC/`CLOCK_MONOTONIC`/`mach_absolute_time`)은 OS API가 필요 → 아래 요청.
8. `PresendError`의 `StaleValue`·`EpochChanged`는 `E_LOCKED`, `Quarantine`은 `E_QUARANTINE`(모두 exit 6, 계약 8.2와 일치).
9. cli-kit: `--json` 봉투는 stdout, 사람용 오류 줄은 stderr. `NOTE:` 형식 = `NOTE: <n> event(s) by <origins> since your last call (<channel> <n>, …)`(+ `— STOP state changed`). 경고 코드별 문구 고정. `client_id` P0 = `LOOM_CLIENT` 또는 `anon-<user>`, 뒤에 `#pid`.
10. 지시서에 없던 `cursor_arg(name)` 추가: clap이 `--since -5s`의 `-5s`를 플래그로 해석하는 문제를 `allow_hyphen_values`로 막는다. 도메인은 커서 옵션에 이것을 쓰면 된다.

## 리드에게 필요한 것
- **루트 `[workspace.dependencies]`에 `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit` path 항목 추가**(계약 8.4처럼 도메인이 `{ workspace = true }`로 쓰도록). 지금은 `loom-proto`만 있음.
- P1(C12/C16)에서 호스트 전역 monotonic 시계를 위해 `libc`(Unix `clock_gettime`)·`windows-sys`(QPC) 계열 의존이 필요할 것(지금 추가는 불필요).

## 도메인 팀 사용 예
```rust
// Cargo.toml: loom-domain-api.workspace = true  (리드가 workspace에 추가한 뒤)
impl DomainModule for Uart { const NAME: &'static str = "uart"; const API_VERSION: u32 = DOMAIN_API_VERSION;
  fn register(&self, r: &mut Registry) -> Result<(), RegisterError> {
    r.command(CommandSpec { method: "uart.send".into(), write_class: Some(WriteClass::Request), roles: RoleSet::WRITE,
        bounds: Bounds::DEFAULT, default_action: None, params: schema, result: schema2, handler: Arc::new(Send) })?
     .event_kind(EventKindSpec { kind: "uart.data".into(), payload_schema: s, pointers: vec![], version: 1 })?
     .hooks(Arc::new(UartHooks))?
     .worker(WorkerSpec { kind: "uart".into(), instances: Instances::PerChannel { max: 8 }, reopen_after_crash: Reopen::None, .. })?;
    Ok(()) } }
// worker 스레드: emitter.rx("uart.data", clock.ts(TsSource::HostReadComplete), json!({...})).ok(); // 실패는 카운트만
```
