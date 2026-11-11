# 07. 테스트 세션 코어 (C7, E3, E4)

TC 형식·`loom-test` 스킬·술어 AST 명세·보고서 템플릿은 TEST 기획(`docs/plan/test/plan.md`). 여기서는 CORE가 집행하는 계약만. 합의 기록: `docs/plan/_xteam/core-test.md`.

## 7.1 세션
- 상태기계 `PLANNED → WAITING_APPROVAL → RUNNING ⇄ PAUSED → STOPPED | DONE`. F1 기본에서는 승인 단계가 즉시 통과한다(정책으로 승인을 켠 경우만 대기).
- 이벤트(2토큰): `test.session_started/ended`, `test.step_started/ended`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask`, `test.answer`. 세션에 묶인 client의 모든 이벤트에 허브가 `test_session`·`test_step`을 자동으로 붙인다.
- 장치 RX의 단계 귀속은 publish 순서가 아니라 단계 시작·끝의 **worker별 source fence**와 수집 시각으로.
- 상한: 동시 세션 1(설정 ≤ 4), 단계 ≤ 500/TC, 판정당 REF ≤ 64, 제어 큐 1k(드롭 금지), 메모 ≤ 4 KiB·세션당 ≤ 1k, 세션 엔진 ≤ 64 MiB, 동시 검사 ≤ 2.
- PAUSED 중 세션 client의 새 쓰기 = exit 6 `E_SESSION_PAUSED`. 개입 RPC `test.pause/resume/skip/retry/answer`, `test.plan.amend`, `test.plan.show`, `test.check`, `test.status`.
- M1 범위: `core.mark` + `test.step_started/ended` 마커만. 검증기·묶음 승인·보고 데이터는 P2.

## 7.2 증거 REF
- JSON DTO(`loom_proto::evidence`): `{ref:{stream_epoch, seq_range, source_refs[]}, kind, pointer?, predicate_id?, observed?, expected?, request_id?, complete}`. 도메인 대기·검증 명령은 결과 `data.evidence[]`에 넣는다. **에이전트는 REF를 복사만 한다**(seq·값을 지어내지 않음).
- 사람용 문자열: `<domain>:<channel>@e<epoch>:seq:<a>[..<b>][#<json-pointer>]`(epoch 생략형은 세션 바인딩으로 해석). 사후 생성물: `la:<dev>@capture:<n>#row:<a>..<b>`(파서는 LA 도메인 합류 시 P2에 추가 — `core-p0-d2`에는 없음).
- JSON Pointer는 발행 레코드 JSON 기준(`02-events.md` §2.2). 도메인은 kind별 고정 경로·단위·숫자 타입을 스키마로 등록한다.
- 대기 timeout 시 `evidence[]`에 timeout 이벤트 seq와 스캔한 창·완전성을 넣는다. timeout 자체는 "발생하지 않음"의 충분한 증거가 아니다.

## 7.3 검증 (환각 방지)
1. 판정은 등록된 plan의 `step.expect` id·hash에 결속된다. 다르면 `E_PLAN_MISMATCH`(exit 4). 계획 변경은 revision + 묶음 승인 무효.
2. 창(anchor 이벤트 기준, 재시도마다 attempt 창)·epoch·세션·단계 fence를 확인한다.
3. 술어 coverage 분류: `Positive`(exists, count ≥ N) / `Complete`(absent, exact, max, hold, order) / `Measure`(within_ms, from/to). Complete·Measure는 창이 `record.gap`·`source.gap`·보존 삭제·`--no-record`·스캔 한도와 겹치면 `inconclusive`.
4. 검증기는 인용 witness만 보지 않고 **filter로 range 전체를 재조회**해 술어를 평가한다. 검증 중 임시 pin, verified면 정식 pin.
5. 코어 공통 술어(exists·absent·count·order·within_ms·hold — hold = 같은 source·신호·generation 연속 샘플, 간격 ≤ 150 ms)는 코어가, 의미 술어(`uart.text_contains`, `can.signal_range`, `uds.dtc_absent` …)는 도메인 `EvidenceVerifier`가 원본으로 재평가한다.
6. 관측 증거 origins 기본 `["device"]`, 요청·응답은 `request_id`로 짝. 창 안에 같은 domain·channel·target에 대한 사용자 쓰기가 있으면 `inconclusive{cause:"user_intervention"}`. 장비 시각 증거(LA)는 capture→host 오차가 창 경계를 가로지르면 inconclusive.
7. 평가 예산: 검사당 이벤트 ≤ 200k, CPU ≤ 200 ms(협력 budget), 스캔 ≤ 64 MiB·2 s, AST 깊이·노드·정규식 입력 상한(정규식은 Rust `regex`). 초과 → `inconclusive{cause:"eval_budget"}`.
8. 결과 `{status: verified|mismatch|not_found|inconclusive|too_large, observed, digest}`. 부재 digest는 expect·창·필터·coverage·DB/정책 버전·gap 상태를 포함한다.

## 7.4 판정 규칙
- PASS = 인용 ≥ 1 + 전부 verified + 증거 종류(witness)가 expect에 맞음: 긍정 관측 / 완전 창의 부재 / 측정값 / 사용자 assertion(별도 타입, 자동 PASS로 위장 금지).
- **에이전트 자신의 송신 이벤트만으로는 PASS 불가**. 자극 전용 단계는 판정 `done`(쓰기 상태 ≥ confirmed)이고 pass로 세지 않는다.
- 증거 없는 PASS = exit 4 `E_EVIDENCE_MISSING`, 불일치 = `E_EVIDENCE_MISMATCH`(기록은 `pass_rejected`), 창 밖 = `E_OUT_OF_WINDOW`. 기록 불능(`record.health=failed`) 중 PASS 거절. 누락 필드·타입 오류는 false가 아니라 오류.
- 판정 수정(override)은 viewer approve rights만, 감사 `test.override`.
- 세션 결과 `passed | failed | inconclusive | incomplete`. `test end`·`run --test` 종료 코드: **0 passed / 11 `E_TEST_FAILED` / 8 inconclusive·incomplete·blocked / 6 STOP·`E_SESSION_PAUSED`**.

## 7.5 러너·freeze·보고
- `loom run --test`는 같은 세션 엔진·검증기를 쓴다. 단계는 CLI와 같은 RPC(origin=script, 숨은 경로 없음), 임의 eval 없음.
- `loom test freeze`는 과거 seq·토큰·시각을 복사하지 않고 새 세션에 재바인딩하는 `loom_scenario: 1`을 만든다. 계획 밖 쓰기가 있었던 세션은 `--allow-unplanned` 없이는 거부, 허용해도 `needs_review`.
- 보고: JUnit은 코어가 생성(매핑 규칙은 TEST 소유: inconclusive = skipped + 사유), HTML은 TEST 템플릿 + 코어 데이터, 스트리밍 생성. 기록 구간 링크 포함.
- 사용자 abort는 finally 실행 여부를 묻고, 전역 STOP은 finally를 자동 실행하지 않는다(A15).
