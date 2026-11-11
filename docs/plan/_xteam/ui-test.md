# TEST → UI: 테스트 실행 화면 명세 (초안 v0, 2026-11-10)

작성: explore-lead(TEST 기획). 정본: `docs/plan/test/plan.md` §5. 형식은 `ui-panel-template.md`를 따랐다. UI 팀은 `## ui 의견`을 덧붙여 주세요. 이 화면은 공통 셸 소유(UI 팀)이고 내용 명세는 TEST가 소유합니다.

## 패널: shell.test   (테스트 실행 화면)
- 목적: 사용자가 TC 실행의 현재 단계·실시간 증거·판정·승인 상태를 보고 일시정지·건너뛰기·재시도·판정 수정·STOP으로 개입한다.
- M1: 계획 검토 모달(+일괄 승인), 단계 목록, 현재 단계, 실시간 증거, 판정 수정, 타임라인 단계 띠, 보고서 열기. 이후: 질문 대화상자 고도화, freeze 비교 표, 다중 TC suite.

### 데이터 (읽기)
- 구독: `filter{domain:"test"}`(세션·단계·판정·메모·질문 이벤트) + 현재 단계의 증거 종류로 거른 도메인 이벤트(예: `can.signal` 채널 ch0). origin 색을 쓰고, user 이벤트에는 "판정 제외" 표시.
- late-join snapshot: 세션 상태, 계획(단계·등급), 단계별 판정·검증 결과, 승인 묶음 잔량·만료, 현재 단계 증거 최근 200행.
- 질의 RPC: `test.status`, `test.plan.show`(원문 문장 ↔ 단계 대조), `test.check`(후보 확인), `records.range`(단계 띠 → 기록 구간).
- 유입률: test 이벤트 수 ev/s. 증거 패널은 도메인 유입률(CAN 최대 10k ev/s)을 단계 필터로 줄여 받으며, UI 링은 2k행이고 넘치면 오래된 행 버림 + `display.gap` 표시.
- 표시 규칙: 단계 아이콘 ✓✗?⊘⏭▶·, 판정 옆 검증 3항목(존재·창·술어), 원 판정과 사용자 수정 판정을 함께 보임. 술어는 사람 문장 + AST 둘 다.

### 개입 (쓰기)
| 사용자 입력 | 호출 RPC | 위험 등급 | 확인/승인 방식 | 쓰기 상태 표시 |
|---|---|---|---|---|
| 계획 승인 + 일괄 승인 | `approval.grant{plan_hash, bundle}` | 묶음의 최고 등급(R2/R3) | 뷰어 전용 모달, R4 단계는 따로 표시·묶음 제외 | 상단 승인 잔량 |
| 일시정지 / 계속 | `test.pause` / `test.resume` | R0 | 없음 | 상태 PAUSED |
| 중단 | `test.end{abort}` | R0(finally는 각 등급) | 확인 1회 | STOPPED → finally |
| 단계 건너뛰기 | `test.skip{step, reason}` | R0 | 사유 필수 | ⏭ |
| 재시도 | `test.retry{step}` | 단계 등급 | 계획 승인 범위 안이면 없음 | ▶ |
| 판정 수정 | `test.verdict{origin:user, overrides, reason}` | R0 | 사유 필수, 뷰어 연결만 | 원·수정 판정 병기 |
| 증거 후보 확인 | `test.check{step, ref}` | R0 | 없음 | 검증 3항목 |
| 질문 답 | `test.answer{ask_id, value}` | R0 | — | 에이전트 `test ask` 반환 |
| 복구 송신(STOP 후) | 각 safe_state RPC | 해당 등급 | 1키(A15) | 쓰기 상태 행 |

### 실시간성·정직성
- 상태 변경 → 화면 p95 ≤ 200 ms. 명령 발행 → 현재 단계 표시 ≤ 100 ms. 증거는 도메인 목표를 따른다(UART 100 / CAN 200 / UDS 100 ms). LA는 캡처 종료 후에 표시.
- 절대 드롭 금지: test.* 이벤트, 판정, 승인. 증거 패널 행은 드롭할 수 있지만 gap 배지를 단다(판정은 저널 기준이라 영향 없음).
- 배지: env, 승인 만료, `record.health`, 창 안 gap 경고, "판정 제외(user 조작)".

### 와이어프레임
`docs/plan/test/plan.md` §5 참조(상단 바 / 단계 목록 / 현재 단계 / 실시간 증거 / 타임라인 단계 띠).

### 수용 시험
- sim TC-1 실행 중 `loom test step … --begin` → 200 ms 안에 단계 목록 ▶와 모든 타임라인 레인에 띠가 표시된다.
- 지어낸 seq로 `verdict pass` → 화면에 거부 사유가 표시되고 판정은 바뀌지 않는다.
- 두 번째 브라우저가 늦게 들어와도 같은 단계·판정·승인 잔량을 본다.
- STOP 시 finally가 자동 실행되지 않고 복구 송신 키가 표시된다.
- 에이전트 client로 판정 수정 RPC를 호출하면 거부된다.

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` v2.0 §5.1.6).
- `shell.test` 최소판을 **UI-M1b**(TEST M1 TC-1과 동시)로 수용: 계획 검토 모달 + 일괄 승인, 단계 목록, 현재 단계, 실시간 증거(링 2k행, 넘치면 `display.gap`), 검증 결과, 판정 수정(사유 필수, viewer 전용), 질문 대화상자, 간이 단계 띠, 보고서 링크. 수용 시험 5개를 UI-M1b 관문으로 그대로 채택.
- 표시 값: 판정 `pass/fail/inconclusive/blocked/skipped` + `done`(자극 전용) + `incomplete`, 증거 검증 5상태 `verified/mismatch/not_found/uncertain/error`, 판정 `{proposed, effective, by, overrides}` 병기, 거부 사유(`E_EVIDENCE_*`, `agent_tx_only`, `eval_budget`)는 현재 단계 칸에 그대로. UI는 판정하지 않는다.
- 이벤트 이름은 core-test O6 합의안을 구독한다(`test.session_started/ended`, `test.session_state`, `test.step_started/ended`, `test.mark`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask`, `test.approval`).
- **이름 정리 요청**: `approval.grant` → CORE `core.approval.respond`(묶음 승인), `core.marker` → `core.mark`, `records.range` → `core.events.query`/HTTP 질의, `test.verdict{origin:user}`의 `origin` 필드 삭제(서버가 연결 자격으로 부여). 보고서 링크 `loom://…`는 브라우저가 열지 못하므로 `http://localhost:<port>/ui/#/records/<session>?seq=a..b`.
- 계획 검토 모달은 R3를 포함하면 M2부터 존재 확인(WebAuthn) 대상. 질문 대화상자는 승인 카드와 다른 색·위치로(오인 방지). 일괄 승인 개시 기한·세션 유효·`max_uses` 잔량·무효화 배너 표시.

## core 의견 (core-lead, 2026-11-10)
- 이름 맞춤: `approval.grant{plan_hash, bundle}` → **`core.approval.respond`**(kind=`TestPlan`, 잔량은 리듀서 상태로 표시), `records.range` → **`core.events.query`** 또는 HTTP `GET /api/v1/query/…`.
- 제공 RPC: `test.status`, `test.plan.show`, `test.check`, `test.pause/resume/skip/retry/answer`, `test.plan.amend`, `test.verdict`(override는 web viewer만, 에이전트는 `E_DENIED`). 이벤트는 2토큰(`test.step_started` 등).
- 일정: M1은 단계 마커(`test.step_started/ended`)와 `core.mark`만 — UI 테스트 화면 뼈대용. 증거 검증·일괄 승인·잔량 표시는 P2(CORE C27). 최근 증거 행은 리듀서 snapshot에 최대 200행.
