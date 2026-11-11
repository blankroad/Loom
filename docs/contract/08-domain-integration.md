# 08. 도메인 통합 · crate 경계 · P0 태그

## 8.1 도메인 등록 (C3, E1 — D10의 Python entry point 대체)
- 도메인은 Rust crate가 `DomainModule`을 구현하고, `loomd`가 cargo feature로 켜진 도메인을 컴파일 시 등록한다. **동적 dylib 로딩 없음.**
- 도메인 crate는 세 조각을 낸다: `gateway`(Registry 등록·훅·리듀서·질의), `worker`(선택 — `loomd worker <kind>`가 호출), `cli`(clap 조각 + 사람용 렌더러 — `loom`이 링크).
- Registry 항목(trait 정본 `loom-domain-api`): `command(CommandSpec)`, `event_kind(EventKindSpec — payload 스키마·고정 JSON Pointer·단위)`, `risk_table`, `hooks(WriteHooks)`, `worker(WorkerSpec{instances: PerChannel{max}|Shared, budgets, reopen_after_crash, arch})`, `runtime(Shared|Dedicated)`, `reducer`(snapshot provider), `evidence_verifier`, `display_summarizer`, `capability`, `safe_state`, `emergency_off`, `interlock`, `panel_manifest`(UI 형식), `policy_section`, `help`(`loom <domain> help --json` 정본, C8).
- 외부 언어 어댑터(예: CANoe COM이 Rust로 안 될 때 — E11)는 같은 worker 프로토콜 + Hello에 manifest(명령 스키마·risk table·이벤트 kind). 정책 판정은 gateway의 범용 `adapter` 모듈이 manifest 정적 표로 한다.

## 8.2 훅 (A2)
| 훅 | 시점 | 규칙 |
|---|---|---|
| `classify(req) → {risk, reason, block?}` | 정책 전 | 실제 보낼 바이트·대상 기준. env 보정 금지 |
| `approval_binding(req) → {digest_fields, display}` | 승인을 켠 경우 digest에 결속 | 물리 주소·generation·요청 바이트 hash·현재값 hash + before_seq·lease·DB hash |
| `presend_check(req, grant) → Ok / Quarantine·Locked·StaleValue·EpochChanged` | backend 직전 | 실패 시 사용권 해제·승인 무효, exit 6 |
| `within_pattern(req, pattern) → bool` | 묶음 승인 사용 시 | 기본 false |
- 모두 **동기·논블로킹·호출당 ≤ 1 ms·I/O 금지**. 코어가 시간을 재서 1 ms 초과는 경고, 10 ms 초과는 도메인 버그로 기록.
- 도메인은 승인 무효화를 `CallCtx::approvals().invalidate(id, reason)`로 알린다.
- 링크 자동 재연결(E1): 정책 키 `[<domain>.<channel>] auto_reconnect = true|false`, 재연결 시 `<domain>.state` 이벤트 `generation+1` + `payload.link{state, cause:"reconnected"}`, 미완료 쓰기 `outcome_unknown`, 재전송 없음.

## 8.3 crate 경계 (impl playbook §1)
| 경로 | 소유 |
|---|---|
| 루트 `Cargo.toml`, `crates/loom-proto`, `crates/loom-core*`, `crates/loom-domain-api`, `crates/loom-worker-sdk`, `crates/loom-cli-kit`, `crates/loom-testkit`, `crates/loom-schema`, `crates/loom-journal`, `crates/loom-echo`, `docs/contract/` | CORE |
| `crates/loom-<comp>*` | 각 도메인 팀 |
| `ui/` | UI 팀 |
| `skills/loom/` | CORE, `skills/loom-<comp>/` 도메인, `skills/loom-test/` TEST |
- **안정 API 표면**(도메인이 의존해도 되는 crate): `loom-proto`, `loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`, `loom-testkit`. 나머지 CORE crate에 직접 의존하지 않는다(CI 검사).
- 도메인 장치 열기 API는 `pub(crate)`. `loom` CLI 바이너리는 장치 crate를 링크하지 않는다(불변식 I1).
- workspace 의존성 버전은 루트 `[workspace.dependencies]`에서 CORE가 관리한다. 추가가 필요하면 `docs/impl/requests/<team>-core.md`.
- 품질: edition 2024, Rust stable 1.93, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`(시뮬레이션 백엔드로 실HW 없이 통과). `unsafe`는 workspace lint로 deny — FFI crate(`*-sys`, 벤더 바인딩)는 crate 단위로 `#![allow(unsafe_code)]` + 안전 주석.

## 8.4 P0 태그와 가져오는 방법
| 태그 | 내용 | 도메인이 할 수 있는 것 |
|---|---|---|
| `core-p0-d2` | workspace 배치, `loom-proto`(이벤트·봉투·오류·커서·RPC·worker 메시지·증거 DTO·정책 타입), 계약 v1 문서, compile-only trait crate(`loom-domain-api`, `loom-worker-sdk`, `loom-cli-kit`) | 자기 crate를 workspace member로 옮기고 타입·trait에 맞춰 compile |
| `core-p0-d5` | `loom-testkit`(FakeWorker·RegistryProbe·HookHarness·fixture assert), `loom-echo` 견본, JSON Schema/TS 생성, `loomd --fixture-replay`(WS 서버 모드) | 단위 시험, UI는 브라우저로 붙어 패널 개발 |
- 태그가 `main`에 병합되면 각 팀은 자기 브랜치에서 `git merge main`. crate는 `crates/<이름>/`에 두고 루트 workspace의 `members = ["crates/*"]`에 자동 포함된다. 의존은 `loom-proto = { workspace = true }`.
- 태그 사이 계약 변경은 `CHANGELOG.md`에 기록하고, 파괴적 변경(필드 삭제·의미 변경·enum 값 제거)은 도메인 리드 리뷰 후에만.
- stub은 가짜 장치 성공을 만들지 않는다. 미구현은 `E_NOT_IMPLEMENTED`(exit 1).
