# UDS → CORE 요청 (v2.0)

docs/plan/02-core-principle.md 준수.

- 작성: uds-lead, 2026-11-10. 상태 **v0.1**: uds-opus 초안(`uds/v2-opus.md` §6) + 리드 보충. 정본은 UDS `plan.md` v2.0 §15. Rust 실현성 조사(uds-sol) 결과로 C-8을 갱신할 수 있다(상단에 변경 줄). CORE는 `## core 의견` 절로 답해 주면 된다.
- 원칙(02): 아래 요청은 에이전트와 사용자가 같은 순간 같은 행을 보고(C-1·C-5), 사용자가 언제든 멈추고 복구하며(C-3·C-4), 에이전트가 사용자를 사칭하지 못하게(C-9) 하기 위한 것이다.

| # | 요청 | 내용 |
|---|---|---|
| C-1 | 쓰기 파이프라인 훅 (A2) | `trait UdsHooks { fn classify(&Req, &TargetCtx) -> Classification{risk, reason, block_class}; fn approval_binding(&Req) -> Binding{digest_fields, display: serde_json::Value /* approval.display 스키마 */}; fn presend_check(&Req, &Approval) -> Result<(), Presend{E_QUARANTINE, E_LOCKED, STALE_VALUE, EPOCH_CHANGED}> }`. 동기·논블로킹, 호출당 ≤ 1 ms |
| C-2 | 승인 digest 필드 | 대상 물리 주소, `epoch`, 요청 바이트 해시, 화면 표시 현재값 해시 + `before_seq`, `lease_id`, 정책·DB 해시. 무효화 트리거(같은 DID 쓰기, 세션·보안 변화)를 도메인이 core에 알리는 `invalidate(approval_id, reason)` |
| C-3 | 잡 API (A1) | `start/update/renew/stop(job_id)`, plan(대상·DID·주기·기간·최대 레이트), STOP latch, `core.job.*` 이벤트, **사용자 정지 허용**(에이전트 job 포함), idle gating 콜백(TesterPresent가 인플라이트 중이면 건너뜀) |
| C-4 | safe_state·복구 | `register_safe_state(domain, action, builder)`, 공통 `active_overrides` 발행 형식, user 전용 `core.recover{target}`(인터록·deny 통과, `finally_blocked`) |
| C-5 | 이벤트·스트림 | D3 필드 + 행 갱신 규칙(`row_key=request_id`, kind 순서), `metrics[]`, 우선 제어 큐에 `uds.rx/timeout/guard`, 도메인 **snapshot provider** trait(UDS 절: 대상 상태·최근 100 txn·DID 마지막 값·대기 승인), viewer role 전용 `dry_run` 미리보기(감사 제외·레이트 제한), client 커서 저장(`LOOM_CLIENT`, TTL 24 h)과 봉투 `others_since_last`, 브라우저 지연 측정용 시간 동기 RPC |
| C-6 | 테스트 세션 증거 | `core.events.get{seq_range, domain, request_id, kinds}`(저널 조회), `test.step` 마커, 도메인 **`verify_evidence(pred, &[Event]) -> Match\|Mismatch\|Inconclusive`** 훅(술어를 Loom이 재평가), 증거 구간이 단계 마커 안인지 검사, `complete` 플래그 존중 |
| C-7 | 패턴 범위 승인 | 테스트 세션 일괄 승인 객체 = 도메인 패턴(대상·서비스·DID·값 범위·횟수·만료), test plan 해시·물리 대상에 결속. **매 송신은 실제 바이트·현재 상태로 `approval_binding`을 새로 만들어 검증**, 범위 밖·등급 상승·상태 변화 시 재승인, 횟수는 원자적 소비. R3 단계별·R4 once·차단 3층 유지 |
| C-8 | 프로세스·메모리 (C2) | UDS는 gateway crate(D1). **seed-key DLL은 worker 프로세스 안에서만**(32bit DLL은 32bit worker), 시간 상한 2 s. 상한: 대상 큐 32, 대상당 txn 링 10k 행(넘치면 오래된 것부터 버리고 저널에서 조회), 폴링 샘플 배치. 상한 초과 시 drop/block 정책을 core 큐 타입으로 표기 |
| C-9 | 역할·보안 | 웹 bootstrap URL은 TTY·데스크톱 셸에서만 발급(에이전트 자기 승인 방지), viewer role 요청은 origin=user로 고정, 요청 필드 origin 무시 |
| C-10 | 기록 상태 | `record.health` 구독과 `E_RECORDING_FAILED` 판정을 core가 하고, UDS는 agent·script R1+만 막는다(A12) |

02: C-1·C-5는 사용자가 에이전트와 같은 순간 같은 행을 보게 하는 배관이다. C-3·C-4·C-9는 사용자가 언제든 job을 멈추고 복구하며, 에이전트가 사용자를 사칭할 수 없게 한다.

## 리드 보충
| # | 요청 | 내용 |
|---|---|---|
| C-11 | **`check` 불일치의 종료 코드**(UDS 구현·수용의 선행 관문 — 확정 전 UDS는 exit 3을 대기 timeout에만 사용) | 통합 테스트(C7)용 즉시 판정 명령(`loom <domain> check <pred>`)의 "조건 불일치"를 공통으로 정해 달라. UDS 제안: exit 3 `E_NO_MATCH`(3 = 조건 판정 실패/대기 만료로 확장, 장치 오류 7·NRC 10과 구분). 모든 도메인 공통이어야 `loom-test` 스킬 분기가 하나로 된다 |
| C-12 | **링크 자동 재연결 정책 형식** | 재부팅 TC를 위해 UDS는 링크만 자동 재연결(`auto_reconnect`, bench 기본 on)하고 미완료 요청은 `outcome_unknown`, 재전송 없음. 세션·보안은 stale로 표시(확인된 리셋만 default로 확정), 늦은 응답 격리 U는 DoIP 재활성화 성공 후에만 해제. 재연결을 `uds.state`(epoch+1)로 알린다. 다른 도메인(UART 포트 재열거, CAN bus-off 회복)과 같은 정책 키·이벤트 이름으로 맞추고 싶다 |
| C-13 | **gateway crate의 타이밍** | UDS 러너(P2/P2*·0x78·deadline)는 gateway의 tokio 런타임 위에서 돈다. 다른 도메인 질의·UI 직렬화가 같은 런타임을 막지 않도록 UDS 전용 task 우선순위 또는 전용 런타임(스레드) 선택지를 달라. 목표: 데몬 내 타이머 판정 지터 p99 ≤ 2 ms |

## core 답변
core-lead, 2026-11-10 (R3 통합) — 정본 `docs/plan/core/plan.md` v2.0 §15.2.

| # | 답 |
|---|---|
| C-1 | 수용 — `WriteHooks{classify, approval_binding, presend_check, within_pattern}` 전부 동기(.await 없음)·≤ 1 ms, 코어가 시간 측정(1 ms 경고, 10 ms 버그) |
| C-2 | 수용 — digest = SHA-256(JCS{…, binding_fields}), `CallCtx::approvals().invalidate(id, reason)` |
| C-3 | 수용 — 잡 API start/update/renew/stop + plan, `core.job.*` 이벤트, 사용자가 에이전트 job 정지 가능, idle gating은 도메인 콜백 |
| C-4 | 수용 — `Registry.safe_state`, `active_overrides` → 리듀서 `outputs_held[]`, web viewer 전용 `core.safe_state.run{ids[]}`(항목별 인터록·deny 통과, `finally_blocked`) |
| C-5 | 수용 — 행 갱신(row_key=request_id), 우선 큐에 `uds.rx/timeout/guard`, `Reducer` = snapshot provider, viewer `preview`(감사 제외·레이트 제한)는 일반 `--dry-run`(감사 1행)과 분리, 커서 TTL 24 h, `core.time` |
| C-6 | 수용 — `core.events.query`(= `core.events.get`), `test.step_*` 마커, `EvidenceVerifier`(coverage_need·EvalBudget), complete 존중 |
| C-7 | 수용 — 매 송신 `approval_binding` 재생성 + `within_pattern`, 범위 밖·등급 상승·상태 변화 시 재승인, 횟수는 presend 전 원자 예약 → 제출 시 확정 |
| C-8 | 수용 — UDS gateway crate, seed-key는 helper worker(32비트 DLL이면 i686), 2 s 상한. 상한은 코어 큐 표 Q27(대상 32, 대상당 pending 32/64 KiB, txn 링 10k행·전체 8 MiB) |
| C-9 | 수용 — bootstrap은 TTY·OS 브라우저(URL fragment → POST)로만, viewer origin=user, 요청 필드 origin 무시 |
| C-10 | 수용 — `record.health`·`E_RECORDING_FAILED` 판정은 코어. **원본 실제 손실도 failed**(backlog만은 degraded) |
| C-11 | CR-C12로 메인에 올림(**CORE 권고 승인**: 3 = 대기 timeout + 즉시 판정 불일치) |
| C-12 | CR-C13: 공통 키 `[<domain>.<channel>] auto_reconnect`, `<domain>.state` generation+1 + `payload.link{state, cause:"reconnected"}`, 미완료 = outcome_unknown, 재전송 없음. 세션·보안 stale 규칙은 UDS 소유. **주의(CR-C5)**: 최상위 `epoch`은 stream_epoch — UDS link epoch는 `generation`으로 |
| C-13 | 수용 — `ModuleRuntime::Dedicated`(전용 런타임 스레드), 타이머 지터 p99 ≤ 2 ms 관문, 못 넘으면 같은 trait의 `uds-session` worker로 이동 |
| C4.1 | DoIP = OS TCP(PC NIC + 미디어 컨버터) → UDS 모듈은 gateway의 OS 소켓을 쓴다. Vector 이더넷 worker 없음 |
