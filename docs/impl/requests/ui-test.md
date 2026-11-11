# UI → TEST(EXPLORE)·CORE 요청 — test.* payload (shell.test 구현 중, 2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `shell.test` 최소판을 fixture 위에서 먼저 만들었다(ui/main 612417d, 가정은 `ui/src/store/testSession.ts` 머리 주석). loom-proto d5에는 test.* DTO가 없다 — TEST와 CORE가 확정해 주세요. 이름이 다르면 UI가 맞춘다.

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

10. 제어 구독 snapshot에 test 리듀서 상태를 넣어 주면(test.*는 이미 priority) 셸의 test 전용 구독을 없앨 수 있다(연결당 구독 ≤ 8 절약).

## test 답변 (explore-lead — TEST 기획, 2026-11-11)
DTO 정본은 CORE(`loom-proto`)다. TEST는 의미를 확정한다(`docs/plan/test/plan.md` §3.1, `_xteam/core-test.md` O1–O9). UI 가정은 대부분 그대로 수용한다.
1. **payload**: 가정 모양을 수용하고 다음을 더한다.
   - `session_started`에 `tc_sha256`, `bench`, `steps[]` 항목 `{step_id, n, title, expect_text, domains[{domain, channel}]}`
   - `evidence_checked.status ∈ verified|mismatch|not_found|uncertain|error`, 그리고 `digest`, `matched_refs[]`
   - `test.verdict.effective ∈ pass|fail|inconclusive|blocked|skipped|done`(`done` = 자극 전용 단계)
2. **거부된 PASS**: 별도 kind를 두지 않는다. `test.verdict{proposed:"pass", effective:null, rejected{code, message}}`(code = `E_EVIDENCE_MISSING|E_EVIDENCE_MISMATCH|E_OUT_OF_WINDOW|E_RECORDING_FAILED`).
3. **override**: `test.verdict{session, step, verdict, override:true, reason}`가 맞다. **viewer 자격 연결만** 가능하고 agent client는 거부한다. 감사 이벤트는 CORE가 별도로 남긴다.
4. **단계 → 증거 스트림**: 둘 다 쓴다. 단계 정의에 `domains[]`가 온다(계획 동사에서 도출). 허브 구독 필터 `filter.test_step`은 CORE에 요청한다.
5. **snapshot**: 가정 모양 + 최근 증거 200행을 포함한다(`_xteam/ui-test.md`와 같음).
6. **`test.plan.show`**: `{plan_hash, steps[{step_id, tc_step, source_text, command, predicate_text, risk}], risk_summary{R0..R4: count}, approval?{approval_id}}`. `risk_summary`는 Loom이 계산한 값이다(에이전트가 쓰지 않음).
7. **user abort**: 승인 카드가 아니라 **test 전용 확인 카드**로 처리한다. 카드에서 finally 실행 여부를 고르고 `test.end{session, abort:true, run_finally: bool}`를 보낸다. F1이라 승인 흐름은 없다. 전역 STOP은 finally 없이 복구 송신 키를 띄운다(A15).
8. `test.ask`·`test.answer` 상수 추가는 CORE에 동의한다.
9. 결과 문자열 표시 방식에 동의한다. 결과 집합은 `passed|failed|inconclusive|incomplete`이고 종료 코드는 0/11/8/6이다.
10. 제어 구독 snapshot에 test 리듀서 상태를 넣는 것에 찬성한다(CORE 판단).
