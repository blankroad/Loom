docs/plan/02-core-principle.md 준수.

# UI M1b 선행 보고 — ui-opus: 테스트 실행 화면 `shell.test` 최소판 + `psu.gauge`

작성: ui-opus, 2026-11-11. 기준 ui/main `e12a1da`, 작업 공간 `.worktrees/ui/ui` (커밋 안 함).
결과: **`pnpm check` 통과** — typecheck + lint + test **23개 파일 222개**(이번 추가 19개: 테스트 화면·스토어·REF 13, PSU 6). `pnpm build` 통과. 레이아웃 프리셋 **`test`**(`#/l/test`: shell.test + psu.gauge bench), 상단 바 **TEST 배지**(세션이 있을 때, 클릭 → test 레이아웃), 셸 하단 **간이 단계 띠**.
근거: CORE 계약 07(`core-p0-d5` 태그), `loom-proto` d5 `event.rs`·`evidence.rs`·`core_events.rs`(test.*는 전부 priority), `_xteam/ui-test.md`(+ ui·core 의견), `test/plan.md` §3·§5, `_xteam/explore-ui.md` §psu.gauge.

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/store/testSession.ts` (신규) | **TestSessionStore** — `test.*` 리듀서(세션 시작·상태·끝, 단계 시작·끝, 증거 검사, 판정, 메모, 질문·답). **판정은 서버 기록만 표시**: 마지막 기록(`verdict`)과 마지막 **수락** 판정(`accepted`)을 분리 → 거부된 PASS가 확정 판정을 덮지 않음, 이력 8개. 늦은 합류: snapshot backlog 이벤트 + 리듀서 상태 객체(`state.test` 또는 `items[{domain:'test', state}]`) 둘 다. 우선 경로와 구독 양쪽으로 오는 중복은 `epoch:seq`로 제거. C2: 단계 500·증거 64/단계·이력 8·메모 200·질문 16·seen 4,096 |
| `src/shell/services.ts` | 스토어 생성, 제어 경로 `test.*` → 스토어, 셸 구독 `{domains:['test']}` 하나(snapshot) 추가 |
| `src/shell/test/TestScreen.tsx` (신규) | **`shell.test`** — 상단(TC·세션·실행자·env·상태·결과·S n/N·[계획 보기][⏸/▶ `test.pause/resume`][■ 중단 `test.end{abort}` 확인 카드 1회]) + 배너(일시정지 = "장비 시간은 흐름", **REC FAILED — PASS 판정 불가**, **STOP — finally 자동 실행 안 함 + [복구 송신으로 →]**). 단계 표(아이콘 ✓✗?⊘⏭●▶·, 경과/한도, 증거 검증 상태 + "창 안 gap ⚠", 판정 문구). 선택 단계 상세: 기대, 증거 행 + [증거 열기](`ui.reveal`), **판정 이력(원판정·확정·수정 by·이전, 거부된 PASS의 코드·메시지)**, [판정 수정…](approve만, 판정 선택 + 사유 필수 → `test.verdict{override:true}`), [건너뛰기…](사유 필수), [재시도]. 실시간 증거(현재 단계 domain·channel 구독, 최근 2,000행, 넘침은 "화면 생략 N행", **사용자 이벤트 [판정 제외]**). **질문 도크**(왼쪽 아래 고정·청록 테두리 — 가운데 호박색 승인 카드와 다른 색·위치, 남은 시간, 선택지·직접 입력 → `test.answer`). 계획 검토 모달(`test.plan.show` 결과: 원문 ↔ 해석 단계·술어·등급 배지, 정책으로 승인이 켜졌을 때만 [계획 승인]). 검증 이벤트가 하나도 없으면 증거 칸 "**검증기 P2**"(정직 자리표시). `StepBand`, `TestBadge` 포함 |
| `src/shell/Layout.tsx`, `TopBar.tsx`, `ShellApp.tsx` | 프리셋 `test`(슬롯 `shell.test`는 패널이 아니라 셸 화면으로 렌더), TEST 배지, 하단 단계 띠 |
| `src/sdk/evidenceRef.ts` | **계약 07 REF**: `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]` 파싱(`epoch` 필드 추가), epoch 생략형 `@seq:`와 Loom 확장 `@req:` 유지. loom-proto 규칙대로 채널 문자 집합·길이 ≤ 512·포인터 `~0/~1`만·역범위 거부. `formatSeqRef` 추가 |
| `src/sdk/context.ts` | `ctx.stopLatched`; **셸 상태(STOP latch·record health·env·rights)가 바뀌면 패널 다시 그리기**(이전엔 데이터가 올 때만 다시 그려져 배지가 늦게 바뀔 수 있었음) |
| `src/panels/psu/gaugePanel.ts`, `psuModel.ts` (신규) | **`psu.gauge`** — 상단(출력 ●ON/○OFF·보호 상태·"STOP — 출력 유지 중"(latch + 출력 ON)·**[! 비상 OFF]**), 설정 V/I·모드·실측 V/I/P(+age), [출력 ON/OFF] **R3 배지**, [설정](V·A) **R2 배지**, 60 s uPlot(V/A 두 축, `spanGaps:false`, `source.gap{scpi_no_response}` 자리에 null 점 → 선 끊김, 손실 횟수 표시). 비상 OFF = `psu.out{channel, state:'off'}`, **watch 허용·확인 없음·STALLED에도 가능**(안전 방향), 패널 포커스에서 `!` 키. 같은 RPC의 두 액션(비상 OFF/출력 토글)을 manifest에 따로 선언해 자격 규칙을 분리. 2D 캔버스가 없으면(헤드리스) uPlot을 만들지 않고 "플롯 사용 불가" 표시 |
| `src/panels/test/fixtures.ts`, `src/panels/psu/fixtures.ts` (신규) | TC-1(세션 → S1 done → S2 pass(검증) → S3 **지어낸 seq PASS 거부(`E_OUT_OF_WINDOW`)** → 사용자 개입 메모 → 검증 → pass → S4 mismatch → fail → S5 수동 + 질문), 판정 수정 이벤트 / SimPsu 60 s + 30 s 지점 SCPI 무응답 gap |
| `src/panels/index.ts` | psu 등록 한 줄 |
| `src/shell/shell.css` | 테스트 화면·질문 도크·단계 띠·PSU |
| 시험 | `src/shell/test/testScreen.test.tsx`(13), `src/panels/psu/psu.test.ts`(6) |

## 2. 시험 (과제 4 항목 대응)

| 과제 요구 | 시험 | 결과 |
|---|---|---|
| 지어낸 seq PASS 거부 표시 | S3 상세에 "⛔ PASS 거부 — E_OUT_OF_WINDOW: seq:17990 is before step S3 begin", 확정 판정은 그 뒤의 수락된 pass, 표 아이콘 ✓ / 스토어: 거부가 `accepted`를 덮지 않음 | 통과 |
| 원·수정 판정 병기 | S4 override → "원판정 fail / 확정 inconclusive / 수정 by viewer:tab-1 (이전 fail by claude-1)" + 사유 | 통과 |
| watch 판정 수정 불가·approve 가능 | watch·operate에서 [판정 수정…] 비활성 / approve: 사유 없으면 제출 비활성, 사유 입력 후 `test.verdict{session, step:'S4', verdict:'pass', override:true, reason}` | 통과 |
| 늦은 합류 snapshot으로 같은 상태 | backlog로 받은 스토어와 라이브 스토어의 단계·판정·증거·현재 단계·개입 수가 같음(중복 수신 무시) / 리듀서 상태 객체로 복원 | 통과 |
| STOP 시 finally 자동 실행 안 함 + 복구 키 | latch → "finally는 자동 실행하지 않습니다" 배너, STOP 패널의 [복구 송신 ⏎] 존재, `test.*` RPC 0건 | 통과 |
| ask 대화상자 색·위치 | `ask-dock` 클래스(왼쪽 아래·청록), 승인 오버레이 밖, "남은 120s", [예] → `test.answer{session, ask_id:'q1', value:'예'}`, `test.answer` 이벤트 → 닫힘 | 통과 |
| REF 새 형식 파싱 | `uart:dut@e3:seq:10..12#/payload/state` → epoch 3·범위·포인터 / `@seq:`·`@req:` 유지 / 잘못된 포인터·역범위·512자 초과·대문자 도메인 거부 | 통과 |
| psu 비상 OFF watch·확인 없음 | watch에서 [! 비상 OFF] 활성 → 호출 정확히 `psu.out{channel:'bench', state:'off'}` 1건, [출력]·[설정] 비활성 / `!` 키 | 통과 |
| 기타 | REC FAILED 배너, 검증기 없을 때 "검증기 P2", [증거 열기] → reveal(epoch 포함), PSU 값·출력 상태, STOP 중 "출력 유지 중", SCPI gap null 점·횟수, 60 s 창 가지치기 | 통과 |

## 3. 정직성 메모
- UI는 판정하지 않는다. 확정 판정은 서버의 수락 기록, 거부된 시도는 별도 행(코드·메시지)으로 남는다.
- M1 CORE는 단계 마커만 낸다(계약 07 §7.1) → 증거 칸은 "검증기 P2"라고 쓰고, 검증 결과가 없는 상태를 통과처럼 보이게 하지 않는다.
- 일시정지는 "스케줄링만 멈춤, 장비 시간은 흐름", STOP은 "finally 자동 실행 안 함"을 명시한다(A15).
- 사용자 이벤트는 실시간 증거에서 [판정 제외]로 표시한다(검증기 규칙 6과 같은 의미).

## 4. 물을 것

### TEST·CORE (test.* payload — loom-proto d5에 DTO 없음, 가정은 `testSession.ts` 머리 주석)
1. **이벤트 payload 모양 확정**: `test.session_started{session_id, tc_id, title, actor, env, plan_digest, steps[]}`, `test.step_started{step_id, n, title, expect_text, timeout_ms, domains[{domain, channel}]}`, `test.evidence_checked{step_id, evidence_id, ref, status, observed, cause, complete, gap_in_window}`, `test.verdict{step_id, proposed, effective, by, reason, evidence_refs[], override, previous{effective, by}, rejected{code, message}}`, `test.note{step_id, text, kind}`, `test.ask{ask_id, question, options[], timeout_ms, step_id}`, `test.answer{ask_id, value}`, `test.session_ended{result, reason}`.
2. **거부된 PASS 기록**: 계약은 "`pass_rejected`로 기록"이라고 한다 — 이벤트가 `test.verdict{rejected}`인지 별도 kind인지. UI는 `rejected{code, message}`로 가정.
3. **override RPC**: `test.verdict{session, step, verdict, override:true, reason}`로 가정. 이름·인자 확정(감사 이벤트 `test.override`는 별도라고 했음).
4. **단계 → 증거 스트림**: 실시간 증거 창이 단계의 `domains[{domain, channel}]`에 의존한다. 단계 정의에 이 정보가 오지 않으면 "도메인 정보 없음"으로 표시된다. 또는 허브가 붙이는 `test_step` 태그로 거르는 구독 필터(`filter.test_step`)를 제공할지.
5. **늦은 합류 리듀서 상태**: snapshot `items[{domain:'test', state}]`의 `state` 모양(`session_id, title, state, current_step, steps[{step_id, n, title, status}]` 가정)과 최근 증거 200행 포함 여부.
6. **`test.plan.show` 결과**: `{plan_hash, steps[{source_text, interpreted|command, predicate_text, risk}], approval?{approval_id}}` 가정.
7. **`test.end{abort:true}`**: 계약 7.5 "사용자 abort는 finally 실행 여부를 묻는다" — 묻는 경로가 승인 카드(`core.approval_requested`)인지 test 전용 ask인지. UI는 지금 확인 카드 1회 후 `test.end{session, abort:true}`만 보냄.
8. 질문 `test.ask`·`test.answer`가 loom-proto `kinds::ALL`에 없다(계약 문장에는 있음) — 상수 추가 요청.
9. 세션 결과 `incomplete`·종료 코드(0/11/8/6) 표시는 화면에 결과 문자열로만 넣었다. 보고서 링크(HTML/JUnit)는 P2 보고 데이터가 생기면 추가.

### EXPLORE (psu)
1. `psu.state` payload `{set_v, set_i, out, prot, mode}`와 측정값을 record `metrics[{name: voltage|current|power}]`로 주는지(별도 `psu.meas` kind인지).
2. 비상 OFF RPC: `psu.out{channel, state:'off'}`로 가정. 같은 동작을 `core.safe_state.list`의 `emergency_off` 항목으로도 노출할지(STOP 패널의 [비상 OFF]는 그쪽을 씀) — 두 경로가 같은 서버 동작이어야 한다.
3. 출력 ON = R3, 값 설정 = R2 배지는 명세대로 정적 표시. 서버 classify 등급을 이벤트·결과로 주면 그 값을 쓰겠다.
4. 보호 상태 값 집합(`ok|ovp|ocp|otp|…`)과 CV/CC 모드.

## 5. mock 시나리오 요구 (리드 배정)
1. **TC-1 세션**: `test.*` 이벤트를 우선 경로(`core.events{sub:"control"}`)로 — `src/panels/test/fixtures.ts`의 순서 그대로 재생 가능. 거부된 PASS 1건, 사용자 개입 1건, ask 1건 포함.
2. **개입 RPC 에코**: `test.pause/resume`(→ `test.session_state`), `test.skip`/`test.retry`, `test.answer`(→ `test.answer` 이벤트), `test.verdict{override}`(viewer approve만 허용, 아니면 `E_DENIED`), `test.end{abort}`(→ `test.session_ended{result:'incomplete', reason:'aborted'}`).
3. **늦은 합류**: 시나리오 `late`에서 test 구독 snapshot에 backlog 또는 리듀서 상태.
4. **SimPsu** `bench`: `psu.state` 1 Hz + metrics, `psu.out`/`psu.set` 에코(→ `psu.state` 갱신), 30 s 지점 `source.gap{cause:'scpi_no_response'}`. STOP 시 `core.latch.outputs_held[{domain:'psu', channel:'bench'}]` + `core.safe_state.list`에 `emergency_off` 항목.

## 6. 리드 요청
- 셸 구독이 2개가 됐다(`core.activity`, `test`). 연결당 8 상한 안이지만, CORE가 `test.*`를 제어 구독에 이미 넣으므로 snapshot만 필요하다 — "제어 구독의 snapshot에 test 리듀서 상태 포함"을 CORE에 요청하면 셸 test 구독을 없앨 수 있다.
- (반복) vitest 공통 setup으로 `matchMedia` 스텁(지금 uart·psu 시험 파일이 각자 스텁 — uPlot은 모듈 로드 시점에 필요해서 `vi.hoisted` 사용).

## 7. 남은 것
보고서 뷰(P2 보고 데이터), 통합 타임라인(M3 — 지금은 셸 하단 간이 단계 띠), 일괄 승인 잔량·개시 기한·무효화 배너(P2), 증거 후보 확인 [이 증거로 확인](`test.check`)은 버튼 경로만 선언했고 후보 목록은 검증기(P2) 뒤에 붙인다, 계획 밖 쓰기 표식.
