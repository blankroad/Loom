# 04. 안전 · 정책 · 감사

> **F1(사용자 결정, 최우선)**: "모두 자동. 그게 벤치니까 설정이나 위험한 부분은 테스트하는 사람이 인지하고 잘 설정해야지."
> 기본 정책은 **R0–R4 모두 자동 실행**이다(sim·bench·vehicle 동일, B4). 확인 토큰·뷰어 승인·존재 확인은 **기본 꺼짐**이며 `loom.toml`로 켠다.

## 4.1 위험 등급 (D6) — 분류는 유지, 기본 동작은 자동
| 등급 | 의미 | 예 |
|---|---|---|
| R0 | 관찰(읽기) | trace, read-did |
| R1 | 가역·저위험 쓰기 | UART 일반 입력, sim 송신, UDS 세션 진입 |
| R2 | 상태 변경 | DID 쓰기, DTC 삭제, 리셋, bench 단발 송신 |
| R3 | 위험 | 보안 접근, 액추에이터·IO 제어, 주기 송신·IG·replay, PSU 출력 ON |
| R4 | 비가역·고위험 | 플래시·메모리 지우기, ECU 위조 송신, OTP |
- 서비스·명령별 등급 표는 도메인이 `WriteHooks::classify`로 정한다. 등급은 **항상** 쓰기 이벤트(`risk`)·UI·감사에 표시된다(F1 유지 항목).
- env(`sim|bench|vehicle`)는 등급·동작을 바꾸지 않는다. 모든 뷰어와 CLI 결과에 env 배지(`meta.env`), `vehicle`은 상시 표시.

## 4.2 정책 동작 (F1, E4, E9) — 타입 `loom_proto::policy`
| 동작 | 의미 | 기본 |
|---|---|---|
| `auto` | 바로 실행 | **R0–R4 전부** |
| `confirm` | 첫 호출 exit 5 + 토큰, `loom confirm <tok>` 재제출 또는 뷰어(operate) 확인 | 꺼짐 |
| `approve` | 뷰어(approve rights) 승인, scope once/session, 사용 기한 ≤ 10분 | 꺼짐 |
| `deny` | 하드 차단(승인으로 우회 불가, 설정 변경으로만) | 목록 비어 있음 |
- 설정 단위: 도메인·동작·대상(`loom.toml` 도메인 섹션). 예: `[uds.policy] "0x31:*" = "approve"`, `[can.ch0] tx = "deny"`.
- **존재 확인(WebAuthn·대역 외 코드)**: 기본 꺼짐. 켜면 R4에만 기본 적용(E9), 정책으로 R3에도. 등록 게이트(TTY 1회 코드 또는 기존 자격증명)는 사용 전 1회 — approve rights가 있어도 존재 확인을 켠 R4는 **등록 게이트를 따로** 통과한 자격증명이 있어야 한다(UI Q4).
- **`immutable_deny`는 Loom이 구현하지 않은 서비스로만**(예: UDS 0x2A·0x86). 장치 잠금·mass erase·옵션 바이트 등은 기본 허용 — 막으려면 사용자가 `deny`로 설정.
- 승인을 켠 경우 규칙(E4, A9): 승인 객체는 digest(요청 바이트·대상·binding)·stream_epoch·generation·정책/프로파일 hash·기한·횟수에 결속, 1회용. 사용권은 presend 전 **원자 예약**, 제출 시 확정 소비. 테스트·run 묶음 승인은 시작 ≤ 10분, 이후 세션/run 종료·`max_test_duration`(기본 1 h, ≤ 8 h)·STOP·정책/epoch 변경 중 먼저 오는 때까지, 매 송신 `within_pattern` 재결속. R4는 묶음 제외. **CLI approve 동사는 없다**(에이전트 자기 승인 차단). Playwright 등 자동화 브라우저는 watch만.

## 4.3 쓰기 파이프라인 (D10, A1, A2) — 모든 도메인·모든 origin 공통
요청형: `requested` 발행 → 전역 차단(STOP·기록 불능) → `classify` 훅 → 정책(차단 → 레이트 → 한계 → 동작) → (설정 시) 확인·승인 → 사용권 원자 예약 → 감사 intent → `accepted` → `presend_check` 훅 → backend(worker) → `sent|failed|outcome_unknown` → publish-before-reply → 응답.
- 훅 3개(`classify`, `approval_binding`, `presend_check`)와 `within_pattern`은 **동기·논블로킹·≤ 1 ms·I/O 금지**.
- 잡형(주기 송신·IG·replay·ISO-TP 채널): 코어는 start/update/renew/stop만 게이트·감사, 단위 송신은 worker가 plan 범위 안에서 하고 단위마다 latch·예약 ID·레이트·하드 차단을 재확인. renew는 `max_job_duration`(기본 1 h) 안에서.
- 대화형 입력(UI 키 입력): 입력권(lease) 획득 시 1회 판정, 이후 키 청크는 저지연 경로, 감사는 `input.batch` 묶음.
- 판단은 Rust gateway에서만. worker·어댑터는 집행만.

## 4.4 STOP · 비상 OFF · 복구 (A3, A15)
- STOP(UI 버튼·`Ctrl-] x`·`loom stop-all`·`POST /api/v1/stop`) = **agent·script의 새 쓰기와 모든 job을 latch**. 사용자 입력은 잠그지 않는다. 출력을 끄지 않는다(PSU·릴레이 상태 유지 → "STOP — 출력 유지 중" 배지).
- 순서: 메모리 latch → 모든 worker `Latch`(ack, worker당 p99 ≤ 10 ms 목표) → 영속(`latch.json`). 영속 실패는 STOP을 무효로 만들지 않는다(`persisted:false` 배너). 재기동 시 latch가 on이거나 불명이면 **latched로 시작**. 해제는 viewer(operate)만. STOP 자체는 watch를 포함한 모든 viewer와 agent가 할 수 있다.
- **비상 OFF**: 도메인이 Registry에 선언한 항목만, 확인 없음, 인터록은 경고만. watch viewer도 실행 가능.
- **복구 송신**(`core.safe_state.run{ids[]}`, viewer operate): 도메인 `safe_state` 동작을 origin=user로 묶음 실행, latch 예외는 safe_state 항목만, 인터록·deny는 통과해야 하고 막히면 `finally_blocked`.
- 사용자 STOP 후에는 run/test의 `finally`를 자동 실행하지 않는다. 에이전트 cancel·on_error·정상 종료는 실행.
- 크래시 후 자동 재개 금지(job·주기 송신·ISO-TP 송신).

## 4.5 인터록 (F1 유지)
- 도메인이 등록한 인터록(예: `uds.flashing` 중 PSU 전원 사이클 금지)은 기본 켜짐, 설정으로 해제 가능. 복구 송신·finally도 인터록을 지난다.

## 4.6 감사 (D6, E6) — 끌 수 없음
- `~/.loom/audit/audit-YYYY-MM.jsonl`, 해시 체인 `h = SHA-256(prev ‖ JCS(record without h))`. 원본 quota와 별도 계정, 무기한.
- 모든 R1+ 쓰기의 intent와 결과, 확인·승인, latch, 정책 변경, worker 크래시, 입력 묶음, 판정 수정을 기록한다. R2 이상 intent는 송신 전 sync, R1·결과는 그룹 ≤ 100 ms.
- 대화형 입력 묶음 `input.batch{lease_id, channel, origin:"user", actor_id, source_range, start_mono_ns, end_mono_ns, byte_count, lines:[{n,len,sha256}], redacted, line_complete, record_health}` — 줄 종결 또는 1 s/4 KiB. 원문은 저널에만.
- 해시 체인은 변조 **탐지**용이다(같은 OS 사용자에 대한 방지 아님).

## 4.7 기록 불능 (A12, E6)
- 원본 손실 발생(저널 큐 초과·쓰기 오류·디스크 가득) 또는 감사 쓰기 실패 → `record.health=failed` → **agent·script의 새 R1+ 쓰기 거부(exit 4 `E_RECORDING_FAILED`)**, R0와 사용자 입력은 허용(감사는 데몬 stderr 폴백), 라이브 화면은 계속. backlog만 큰 상태는 `degraded`(허용 + 경고). 감사 디스크 부족도 같다.
- 테스트 세션은 `failed` 중 PASS 판정을 거절한다.

## 4.8 정책 파일 (D6, A8)
- 프로젝트 `loom.toml` + 사용자 `~/.loom/config/policy.toml`. 병합은 더 엄격한 쪽(deny 합집합, 한계 최솟값). 변경은 감시해 송신 전 effective hash를 검사한다. 파싱 실패 = 직전 유효 정책 유지 + 배너.
- F1에 따라 기본이 이미 가장 느슨하므로 "완화는 뷰어 승인 후 적용"은 승인을 켠 경우에만 의미가 있다. 모든 정책 변경은 감사에 남는다.

## 4.9 위협 모델 (D6)
목표는 에이전트의 실수를 사용자가 **보고 멈출 수 있게** 하는 것과 사용자의 통제권이다. 같은 OS 사용자로 악의적으로 우회하는 에이전트를 막는 보안 경계는 아니다. 장치 출력 속 문장은 데이터이지 지시가 아니다(스킬 규칙).
