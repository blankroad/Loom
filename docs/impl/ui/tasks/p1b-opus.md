docs/plan/02-core-principle.md 준수.

# UI M1b 선행 — ui-opus: 테스트 실행 화면 `shell.test` 최소판 + `psu.gauge`

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md`·`p0-proto-v1-update.md` 그대로. 기준 커밋 ui/main `e12a1da`.

## 근거
- 내용 명세: `docs/plan/_xteam/ui-test.md`(+ `## ui 의견`), `docs/plan/test/plan.md` §5(와이어프레임)·§3(판정)
- CORE 계약 07(테스트 세션) — `core-p0-d5` 태그에서 읽기: `git -C /Users/ctmctm/workspace/Loom show core-p0-d5:docs/contract/07-test-session.md`. 요점:
  - 세션 상태 `PLANNED → WAITING_APPROVAL → RUNNING ⇄ PAUSED → STOPPED | DONE`(F1: 승인 단계 즉시 통과)
  - 이벤트 `test.session_started/ended`, `test.step_started/ended`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask`, `test.answer`. 세션 client 이벤트에는 허브가 `test_session`·`test_step`을 붙인다
  - 검증 결과 `status: verified | mismatch | not_found | inconclusive | too_large`, 판정 `pass | fail | inconclusive | blocked | skipped | done`(자극 전용) + 세션 결과 `passed | failed | inconclusive | incomplete`
  - **증거 REF 사람용 문자열 `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]`**(epoch 생략형 허용) — `sdk/evidenceRef.ts`를 이 형식에 맞추고 기존 `@seq:`·`@req:` 형식도 계속 받게
  - 판정 수정(override)은 **approve rights만**. 개입 RPC `test.pause/resume/skip/retry/answer`, `test.plan.show`, `test.check`, `test.status`
  - 종료 코드: 0 passed / 11 failed / 8 inconclusive·incomplete / 6 STOP·PAUSED
  - **M1 코어 범위는 `core.mark` + 단계 마커뿐**, 검증기·묶음 승인·보고 데이터는 P2 → 화면은 이벤트가 오면 그리고, 없으면 "검증기 P2" 자리표시로 정직하게
- EXPLORE 명세 `docs/plan/_xteam/explore-ui.md` §psu.gauge

## 만들 것
1. **`shell.test`**(셸 소유 화면, 레이아웃 프리셋 `test` + 상단 바의 테스트 세션 배지에서 열기):
   - 상단: TC 이름·세션·실행자·env·상태·결과·[⏸/▶](`test.pause/resume`)·[■ 중단](`test.end{abort}`, 확인 1회)·`REC FAILED — PASS 판정 불가` 배너
   - 단계 표(RowTable 재사용 가능하면): 아이콘 ✓✗?⊘⏭▶·(done), 경과/timeout, 증거 칸(검증 status + "창 안 gap" 칸), 판정(원판정 `proposed` / 확정 `effective` / 수정 `overrides` 병기, by)
   - 현재 단계의 **실시간 증거**: 단계 필터(같은 test_session·test_step 또는 단계가 쓰는 domain·channel) 이벤트 최근 2,000행, 사용자 이벤트에 "판정 제외" 표식, 넘치면 display.gap
   - 판정 근거 칸: 에이전트 인용 REF → [증거 열기]가 `ctx.ui.reveal(ref)`로 해당 패널·seq 이동
   - 사람 개입: [판정 수정…](approve만, 사유 필수 → `test.verdict` override RPC — 이름은 계약에서 확인, 없으면 가정하고 보고), [건너뛰기](사유 필수), [재시도], 질문 대화상자(`test.ask` → `test.answer`, **승인 카드와 다른 색·위치**, 남은 시간), 증거 후보 확인(`test.check`)
   - **간이 단계 띠**: 셸 하단 타임라인 한 줄 — 단계 시작/끝 마커, 현재 단계 강조. (통합 타임라인은 M3)
   - 계획 검토 모달: F1 기본은 승인 단계가 없으니 **정보 표시 모달**(원문 문장 ↔ 해석 단계, 술어 사람 문장, 등급 배지). 정책으로 승인을 켰을 때만 [계획 승인] 버튼
2. **`psu.gauge`**(spec 패널 또는 코드 패널): 설정 V/I·실측 V/I/P(60 s 시계열 — uPlot, gap에서 선 끊기)·출력 상태·보호 상태, `!` **비상 OFF**(확인 없음, watch 가능, `psu.out{off}`), 출력 토글(등급 R3 배지, F1 기본 자동), 값 편집(R2 배지). STOP 상태 패널의 "출력 유지 장치"와 같은 데이터원. kind는 explore-ui 명세(`psu.state`, `metrics[]`, `source.gap{scpi_no_response}`).
3. fixture: `src/panels/test/fixtures.ts`(TC-1 흐름: 세션 시작 → 단계 1–5 → 증거 확인 → 판정, 거부된 PASS 1건, user 개입 1건, ask 1건) / `src/panels/psu/fixtures.ts`.
4. 시험: 지어낸 seq PASS 거부 표시, 원·수정 판정 병기, watch로 판정 수정 불가·approve 가능, 늦은 합류 snapshot으로 같은 상태, STOP 시 finally 자동 실행 안 함 + 복구 키, ask 대화상자 색·위치, REF 새 형식 파싱, psu 비상 OFF watch 허용·확인 없음.

## 파일 범위
`ui/src/panels/test/**`, `ui/src/panels/psu/**`, `ui/src/sdk/**`, `ui/src/shell/**`, `ui/src/store/**`, `ui/src/panels/index.ts`. **수정 금지**: `ui/src/panels/can/**`, `ui/src/net/**`, `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`(CAN 서브에이전트 작업 중), 리드 소유 파일.

## 보고
`docs/impl/ui/reports/p1b-opus.md` — 파일, 시험 수, TEST·EXPLORE·CORE에 물을 것, mock 시나리오 요구. 끝나면 "완료: <경로>"만.
