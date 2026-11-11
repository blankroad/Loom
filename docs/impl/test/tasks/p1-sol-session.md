# TEST P1 — explore-sol: `crates/loom-test` (세션 RPC 모듈 + `loom test` CLI + 스킬)

먼저 `p1-common.md`와 다음을 읽는다:
- 계약 `07-test-session.md` 전체
- `docs/plan/test/plan.md` §3.1(상태표·개입 RPC·집계·종료 코드)·§4(CLI 표, `loom-test` 스킬 초안 §4.1)·§5
- `docs/plan/_xteam/core-test.md` O2·O5–O8
- `docs/impl/requests/ui-test.md`의 `## test 답변`(UI가 이 payload로 테스트 화면을 만들었다)
- 템플릿 `crates/loom-echo`, `crates/loom-eth`(gateway·cli 구조)

배정 범위: 새 crate `crates/loom-test/`, `skills/loom-test/`. `loom-test-format`(네 crate)은 의존해서 쓰고, 고칠 일이 있으면 같이 고쳐도 된다. **`loom-test-predicate`는 explore-opus 작업 중이라 고치지도 의존하지도 않는다.** 검증은 아래 trait seam으로 둔다.

## 모듈 (`TestModule: DomainModule`, `Registry::for_core("test")`)
`test`는 CORE 예약 domain이다. `for_core`를 쓰는 경계는 CORE 승인 대기 중이다(`docs/impl/requests/test-core.md`). 승인되지 않으면 CORE가 이 모듈을 감싸 등록할 수 있게, 공개 API를 깔끔히 둔다.
- **세션 상태기계**(계약 7.1):
  - 상태: `PLANNED → WAITING_APPROVAL → RUNNING ⇄ PAUSED → STOPPED | DONE`. F1 기본이면 승인 단계는 즉시 통과한다.
  - 상한: 동시 세션 1(설정 ≤ 4), 단계 ≤ 500, 판정당 REF ≤ 64, 제어 큐 1k(드롭 금지 → 넘치면 backpressure 오류), 메모 4 KiB·세션당 1k, 엔진 RSS 예산 64 MiB는 문서로.
- **RPC**:
  - 세션: `test.start{tc, plan?}`(tc·plan 텍스트 또는 경로 — 파싱은 `loom-test-format`; plan check 실패면 exit 2/4), `test.status`, `test.end{abort?, run_finally?}`
  - 단계·판정: `test.step{id}`(이전 단계 자동 종료), `test.step.end`, `test.verdict{step, verdict, evidence[]}`, `test.evidence.add`, `test.check{step, evidence?, auto?}`
  - 사람 개입: `test.note`, `test.ask{question, options, timeout_ms}` / `test.answer`, `test.pause/resume/skip/retry`
  - 계획: `test.plan.show`, `test.plan.amend`(기존 승인 무효)
  - 보존 상태·종료 코드: plan §3.1 상태표와 계약 7.4 그대로.
- **이벤트**(2토큰, ui-test 답변 모양): `test.session_started/ended`, `test.step_started/ended`, `test.evidence_checked`, `test.verdict`(거부된 PASS = `rejected{code,message}`, `effective:null`), `test.note`, `test.ask`, `test.answer`. 64비트 값은 `U64` 문자열.
- **검증 seam**: `trait VerifierEngine { fn check(&self, step: &StepSpec, citations: &[Citation], ctx: &mut CheckCtx) -> CheckOutcome; fn decide(...) -> Decision }`. 기본 구현은 **`NotWired`**로, `E_NOT_IMPLEMENTED`를 낸다(가짜 PASS 금지). explore-opus의 predicate crate가 끝나면 리드가 연결한다.
  - 판정 RPC는 이 seam의 결과로만 PASS를 확정한다.
  - **agent client의 override 거부**, override는 viewer 자격만(CallCtx에서 role 확인).
- **reducer snapshot**: `{session_id, title, state, current_step, steps[{step_id, n, title, status}], recent_evidence[≤200]}`(ui-test 5번).
- **위험 요약**: `test.plan.show`·start 결과에 `loom-test-format`의 `check_plan`으로 계산한 risk_summary를 넣는다. `RiskProvider`는 Registry 명령표에서 만든다. CORE에서 표를 못 받으면 trait 주입.

## CLI (`DomainCli`, plan §4 표)
- 하위 명령: `loom test import`(M2 — 지금은 `E_NOT_IMPLEMENTED`), `plan check|show`, `start [--wait-approval]`, `step <id> [--end]`, `check [--auto]`, `verdict <step> pass|fail|inconclusive|blocked|skipped --evidence REF…`, `evidence add …`, `note`, `ask`, `status`, `wait`, `end [--abort]`, `report`(P2 — `E_NOT_IMPLEMENTED`), `freeze`(P2 — `E_NOT_IMPLEMENTED`).
- `plan check`는 **로컬 실행 가능**(파일 파싱 + 정적 검사, 데몬 불필요)하게 한다. 명령표 없이 돌 때는 "명령 존재 미검사" 경고를 단다.
- 사람용 출력과 `--json` 봉투. **종료 코드 0/11/8/6**(end), 4(증거 거절), 5, 3.

## 스킬 `skills/loom-test/SKILL.md` (60줄 이하)
- 바탕: plan §4.1 초안을 계약 07·v2.2(F1 자동: 일괄 승인 대기는 기본 없음)에 맞게 다듬는다.
- 반드시 넣을 것: "REF는 결과에서 복사만", "자기 송신은 증거 아님", "증거 없으면 pass 시도 금지 → inconclusive", "재부팅 뒤 대기 예시(UART `--from seq`, UDS `link == connected`)", "질문 조건".

## 시험 (testkit RegistryProbe·가짜 CallCtx/저널, `NotWired`와 시험용 가짜 VerifierEngine)
- 상태기계 전이 전부
- PAUSED 중 쓰기 거부(계약 7.1)
- 단계 자동 종료
- 판정 상한
- agent override 거부
- 증거 0개 PASS 거부
- 집계 → 종료 코드
- snapshot
- TC-1 fixture로 start → plan.show risk_summary
- CLI 파싱·렌더 golden

## 보고
`/Users/ctmctm/workspace/Loom/docs/impl/test/reports/p1-sol-session.md`(50줄 이하, 결정·질문·CORE 요청). 커밋하지 않는다.
