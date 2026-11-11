docs/plan/02-core-principle.md 준수

# UDS P1 — runner 보강 (리드 하위 작업자, uds-sol 대체)

공통 규칙: `docs/impl/uds/tasks/p0-common.md`(worktree, 질문 금지, 커밋 금지, 빌드, C2, 보고). 고칠 crate는 `crates/loom-uds-runner`만(필요하면 `loom-uds-doip`도 — 그 경우 보고서에 명시). **`crates/loom-uds`는 uds-opus가 지금 고치는 중(P1-b, runner를 사용)** — 건드리지 말고, runner 공개 API는 **추가만** 한다(기존 함수·생성자·필드 의미 유지, 새 필드는 기본값이 있는 builder 메서드로). 기존 시험 33개가 그대로 통과해야 한다.

배경: 02 원칙의 "사용자 개입"이 UDS에서 동작하려면 사용자 요청과 중단이 에이전트 대기열보다 먼저 가야 한다(plan §3.5, E2E-1 t3·t5: 사용자가 콘솔에서 쓰기, `!` abort, 셸 STOP).

## 할 일
1. **우선순위**: `Priority{Abort > User > Agent(=Script)}`. 대상별 대기열을 우선순위별로(각 상한, C2 상수). 실행 중인 요청은 끊지 않고(ECU 쪽 상태 보존), 다음에 꺼낼 때 높은 우선순위 먼저. `TxnRequest::with_priority(..)`, 기본 Agent.
2. **abort**: `RunnerHandle::abort(scope)` — scope = 대기 중 전부 / 특정 origin / 특정 request id. 대기 중 요청은 보내지 않고 `failed` + `E_INTERRUPTED_BY_USER`(사용자 abort) 또는 `E_STOPPED`(STOP latch용 변형). 실행 중 요청은 응답 대기를 멈추고: 읽기 = `failed`, 쓰기 = `outcome_unknown`(이미 보냈으므로), 그리고 그 응답은 늦은 응답으로 quarantine(D19 규칙 그대로). abort 자체는 ≤ 10 ms 안에 반환(측정 시험).
3. **`BoxFuture`**: 로컬 정의를 `loom_domain_api::BoxFuture`로 바꾼다(같은 모양의 type alias라 호환 — `loom-domain-api` 의존 추가는 안정 API라 허용).
4. **T_TCP 비활성 타이머**(ISO 13400 T_TCP_General_Inactivity 기본 5 min): 클라이언트는 tester 쪽이라 주기 진단이 없으면 alive 응답만으로 연결이 유지되는지 확인하고, 필요하면 엔티티 측 타이머를 `DoipEntity`에 설정 가능하게(sim 결함 주입용).
5. **읽기 1회 재시도 정책**(plan §3.4에 있으면 그대로, 없으면 하지 말고 보고서에 근거): 쓰기는 절대 재전송 없음.

## 시험
- 정지 시계 + FakeTransport: Agent 3개 대기 중 User 요청 → User가 다음에 실행, Abort가 대기열 비움, 실행 중 쓰기 abort → outcome_unknown + 늦은 응답 quarantine, 실행 중 읽기 abort → failed, 우선순위별 상한 넘침 → `E_BUSY`.
- DoIP 엔티티 루프백 1개: 0x78을 길게 보내는 중 abort.

## 보고
`/Users/ctmctm/workspace/Loom/docs/impl/uds/reports/p1-runner-sub.md`(40줄 이하): 추가 API 목록(uds-opus가 쓸 것), 상한 표, 시험 결과, 결정·질문. 끝나면 멈춘다.
