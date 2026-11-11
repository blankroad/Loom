# TEST → CORE: 테스트 세션 코어 (초안 v0, 2026-11-10)

작성: explore-lead(TEST 기획 담당, 마스터 C7). 정본 계획: `docs/plan/test/plan.md` §3·§4·§6·§10·§15. CORE 팀은 아래에 `## core 의견`을 덧붙여 주세요. 합의 전 초안입니다.

## 경계
| CORE 소유 (Rust, `loomd`) | TEST 소유 (명세·스킬·템플릿) |
|---|---|
| 테스트 세션 객체·상태기계(PLANNED/WAITING_APPROVAL/RUNNING/PAUSED/STOPPED/DONE), 단계 마커 이벤트, client↔세션 태깅 | `.tc.md` 정본 형식, `.plan.yaml`(`loom_tc_plan: 1` ⊃ `loom_scenario: 1`) 스키마, 가져오기 매핑 |
| **증거 검증 엔진**(존재·창·술어·gap), 판정 저장(감사 해시 체인), 일괄 승인 바인딩 | 술어 비교 AST **명세**, 판정 규칙표(R-1..R-7), 실패 행동 규칙 |
| 저널 seq 조회, 보고서 데이터 API, `freeze` 변환 실행, `loom run --test` | 보고서 템플릿(askama HTML, quick-junit), `loom-test` 스킬, UI 화면 명세 |

## 요청
1. **TC-C1 이벤트·태깅**: `test.session.start/end`, `test.step.begin/end`, `test.verdict`, `test.note`, `test.ask`(domain=test, dir=null). D3에 선택 필드 `test_session`, `test_step` 추가 — 세션에 묶인 client의 모든 이벤트에 자동으로 붙음. 장치 RX는 시간 창으로만 단계에 연결, origin은 바꾸지 않음.
2. **TC-C2 검증기**:
   - 입력: 판정 + 증거 REF 목록 + 계획의 `expect` AST.
   - 절차: ① 참조한 seq가 이 세션 저널에 있다. ② 단계 창 안이다(LA 같은 사후 증거는 장비 시각 범위 + `quality:host-approx`). ③ 술어가 참이다. ④ "없음·시간 내" 주장이면 창 안에 `record.gap`/`source.gap`이 없다.
   - 누락 필드·타입 불일치는 false가 아니라 **오류**로 처리한다.
   - pass 거부 종료 코드 4: `E_EVIDENCE_MISSING`, `E_EVIDENCE_MISMATCH`, `E_OUT_OF_WINDOW`(D5 표 추가).
   - `check --auto`: 창 안에서 술어를 만족하는 이벤트 후보를 찾아 제안한다.
3. **hold·측정**: hold는 같은 source·신호·epoch의 연속 샘플(간격 상한 기본 150 ms)이다. 시간 측정(`measure from/to`)은 검증기가 `ts.mono_ns` 차이로 직접 계산한다(에이전트 숫자 미사용). 스파이크 `spikes/explore/v2-evidence/`(비교 AST·CEL·JSONPath, 10사례)를 참고 구현으로 쓸 수 있다.
4. **TC-C3 일괄 승인**:
   - 하나의 뷰어 승인으로 계획의 R2·R3 단계를 묶는다.
   - 바인딩: plan hash·동작·대상·**횟수 상한**·env·프로파일 해시.
   - R4는 묶지 않는다(단계 시작 시 매번).
   - `default_blocked`/`immutable_deny`가 포함되면 계획을 거부한다.
   - A9 확장: "세션 시작 10분 안에 개시, 세션 중 유효, `max_test_duration` 기본 1 h".
   - 계획 밖 R2+ 쓰기는 개별 승인을 받고 보고서에 "계획 밖"으로 표시한다.
5. **TC-C4 러너 공유**: `loom run --test`가 같은 세션 엔진·검증기를 쓴다. `freeze`는 과거 seq·토큰·시각을 복사하지 않고 새 세션에 재바인딩한다.
6. **TC-C8**: 기록·감사 불능(A12)이면 새 R1+ 쓰기와 **pass 판정**을 중단한다.
7. **TC-C9 (C2)**:
   - 검증기 평가 예산(기본 검사당 대상 이벤트 ≤ 200k, ≤ 200 ms, 식 깊이 제한)과 `inconclusive{cause:"eval_budget"}`.
   - 상한: 세션 동시 1개(설정 ≤ 4), 단계 ≤ 500/TC, 증거 REF ≤ 64/판정.
   - soak 부하 패턴에 TC-1 반복 세션(10분마다)을 포함하고, 세션 종료 후 RSS가 반환되는지 본다.
8. **판정 수정**: 뷰어 자격 연결만 `test.verdict{origin:user, overrides, reason}`를 쓸 수 있다. 에이전트 client는 거부.

## CLI (TEST 명세, CORE 구현)
`loom test import | plan check | plan show | start [--wait-approval] | step --begin/--end | check [--auto] | verdict | evidence | note | ask | status | wait | end [--abort] | report --html --junit --json | freeze`. 종료 코드:
- 3: 승인·ask timeout
- 4: 정책·순서 위반, 증거 거부
- 5: `E_APPROVAL_REQUIRED`
- 6: STOP 또는 다른 세션이 진행 중

상세는 test/plan.md §4.

## core 의견 (core-opus R1, 2026-11-10)

docs/plan/02-core-principle.md 준수. CORE 워커 core-opus의 R1 독립 초안 v0.1이다(core-sol 의견은 별도로 붙을 수 있고, core-lead가 R2에서 통합). 위 TEST 요청 TC-C1–C9에 대한 답은 **O9 표**, 그 근거가 되는 코어 설계는 O1–O8. 주 초안: `docs/plan/core/r1-opus.md` §10.9. 아래 `## test 의견`은 TEST가 이 의견에 답하는 자리다.

### O1. 무엇을 위한 계약인가 (핵심 루프)
사람이 자연어·표로 쓴 TC를 에이전트가 기존 `loom <domain>` 명령으로 실행하고(TC마다 코드를 짜지 않음 — C7), 사용자는 UI 테스트 화면에서 **현재 단계·실시간 증거·판정**을 같은 순간 보고 STOP·수동 판정·건너뛰기로 개입한다. 코어는 (1) 세션·단계 마커를 모든 도메인 타임라인에 찍고, (2) 에이전트가 인용한 증거가 저널에 실제로 있고 기대와 맞는지 검증하며, (3) 일괄 사전 승인과 보고 데이터를 준다. **판정 문장은 에이전트가 쓰지만, PASS의 근거는 Loom이 검증한 장치 데이터여야 한다.**

### O2. CLI / RPC (core 소유, 모두 경계 있음·`--json` 봉투 D5)
> **CLI 표면의 정본은 위 TEST 명세**(`import | plan check | plan show | start | step --begin/--end | check | verdict | evidence | note | ask | status | wait | end | report | freeze`)다. 아래 표는 코어가 구현할 RPC와 동작이며, CLI 이름이 다르면 TEST 쪽을 따른다(`loom test step <n>` = `step --begin`, `loom test evidence add` = `evidence`).

| CLI | RPC | 주요 인자 → 결과 |
|---|---|---|
| `loom test start --tc <file\|-> [--title] [--plan plan.json] [--run scenario.yaml] [--max-duration 2h]` | `test.start` | TC 원문 sha256·제목·env·bench·정책/프로파일 해시 → `session_id`, `start_seq`, 사전 승인 `approval_id`(plan이 있으면, 뷰어 승인 대기) |
| `loom test step <n> "<title>" [--expect "<자연어 기대>"]` | `test.step.begin` | 이전 단계 자동 종료 → `step_id`, `from_seq` (마커) |
| `loom test mark <label>` | `test.mark` | 단계 안 보조 마커 → `seq` |
| `loom test evidence add --step <id> --range seq:A..seq:B [--domain d --channel c --kind k --request-id r] --pred '<json>'` | `test.evidence.add` | 인용 등록 + **즉시 검증** → `evidence_id`, `status`, `observed`, `digest` |
| `loom test verdict <step> pass\|fail\|inconclusive\|blocked\|skipped --evidence e1,e2 [--note]` | `test.verdict` | 규칙 O5 적용 → `effective_verdict`, 거절 사유 |
| `loom test note "<text>"` | `test.note` | 사람이 읽을 메모(판정 근거 아님) |
| `loom test status [<session>]` | `test.status` | 현재 단계·판정 집계·남은 승인·gap 경고 |
| `loom test end [--reason completed\|aborted\|stopped]` | `test.end` | 열린 단계 종료, 세션 immutable → 보고 데이터 ref |
| `loom test report <session> --format json\|junit\|html` | `test.report` | 파일 경로 또는 `GET /api/v1/report/<id>.<fmt>` (HTML 디자인은 TEST·UI 소유, 코어는 데이터·JUnit) |
| `loom test freeze <session> -o scenario.yaml` | `test.freeze` | 세션 안 agent 쓰기·대기·검증된 증거 술어 → `loom_scenario: 1` (O8) |
- 세션 상태기계는 TEST 안 그대로: `PLANNED → WAITING_APPROVAL → RUNNING ⇄ PAUSED → STOPPED | DONE`. 동시 세션은 데몬 전체 1개(설정 ≤ 4, TC-C9) — 진행 중이면 `start`는 exit 6 `E_SESSION_ACTIVE`.
- 세션에 묶인 client(`LOOM_CLIENT`)의 모든 이벤트에 `test_session`·`test_step` 선택 필드를 허브가 자동으로 붙인다(TC-C1). 장치 RX는 origin을 바꾸지 않고 시간 창(단계 마커 seq 범위)으로만 단계에 연결.
- 세션 밖 명령은 세션에 묶이지 않는다(인용은 가능 — 범위가 세션 안이면).
- 사용자(viewer)는 UI에서 `test.verdict`(origin=user, `user_assertion`)·`test.step.skip`·`test.end{reason:"stopped"}`를 할 수 있다.

### O3. 증거 인용 형식
```json
{"id":"e1","step_id":"s3",
 "range":{"epoch":"3","from_seq":"91200","to_seq":"91310"},
 "filter":{"domain":"uds","channel":"ecu1","kinds":["uds.rx"],"request_id":"q-01JC7","origins":["device"]},
 "pred":{"op":"uds.response_eq","sid":"0x62","did":"0xF190","data_regex":"^57.{32}$"},
 "claim":"ECU가 VIN을 17자로 응답했다"}
```
- `range`는 stream 좌표(커서와 같은 문법 `seq:N`, `mark:X`, `@step:s3`(단계 전체) 허용). 내부에서 stream 인덱스로 `(producer, source_seq)` 범위로 바꾼다. 이 매핑이 없으면(인덱스 유실) 세그먼트 재스캔으로 복원하고, 복원 못 하면 `uncertain` — 지어내지 않는다.
- 코어 공통 술어: `exists`, `absent`, `count{min,max}`, `order{a,b}`(a 다음 b), `within_ms{from,to,max}`(같은 `clock_id`일 때만), `hold{signal, op, value, min_ms, max_gap_ms=150}`(같은 source·신호·epoch 연속 샘플 — TC-C3), `measure{from, to}`(검증기가 `ts.mono_ns` 차이로 직접 계산, 에이전트가 적은 숫자 미사용). 비교식은 TEST의 비교 AST 명세를 따른다(코어는 AST 평가기만, 깊이 제한). 도메인 술어는 `<domain>.<name>` 이름으로 `EvidenceVerifier`가 평가(UART `uart.text_contains`/`uart.text_absent`, CAN `can.signal_range`/`can.id_period`, UDS `uds.response_eq`/`uds.dtc_absent` …). 임의 표현식 실행기(CEL·JSONPath·스크립트)는 넣지 않는다.

### O4. 검증 알고리즘
```
verify(citation):
  0. 형식 검사: 누락 필드·타입 불일치·모르는 술어 = status error (false가 아님, TC-C2)
  1. 세션·단계 확인: range ⊂ 이 세션, 단계 창 밖이면 판정 거절 E_OUT_OF_WINDOW (LA 등 사후 증거는 장비 시각 범위 + quality:host-approx 허용)
  2. epoch 확인: range.epoch ≠ 현재 epoch이면 저널 인덱스로 해석(가능) / 불가 → uncertain
  3. coverage: range와 겹치는 record.gap · source.gap · no-record 구간 · 보존 삭제 구간을 index.db에서 조회
       - 술어가 부재·개수·구간 의미(absent, count, within_ms, *_absent)이면: 겹침 있음 → uncertain
       - 술어가 존재 의미이면: 일치 이벤트를 찾으면 verified 가능(겹침은 warning)
       - display.gap은 화면 손실이므로 coverage에 영향 없음
  4. 조회: 저널 인덱스에서 filter 적용, 상한 스캔 64 MiB·I/O 2 s, 평가 대상 ≤ 200k 이벤트·CPU ≤ 200 ms (TC-C9) → 넘으면 inconclusive{cause:"eval_budget"} (범위 좁혀 다시)
  5. 평가: 공통 술어는 코어, 도메인 술어는 EvidenceVerifier::verify(pred, events, coverage)
  6. 결과 기록: test.evidence_checked{evidence_id, status: verified|mismatch|not_found|uncertain|error,
                 observed(요약 ≤ 2 KiB), matched_refs[], digest = sha256(JCS(일치 이벤트들))}
  7. 보호: 인용된 세그먼트를 보존 GC에서 pin (quota 부족이면 명시 실패 + 안내, 조용히 삭제 안 함)
```

### O5. 판정 규칙 (코어가 집행)
| 제안 판정 | 조건 | 결과 |
|---|---|---|
| pass | 증거 ≥ 1, 전부 `verified`, **origin=device 이벤트를 포함한 증거 ≥ 1** 또는 사용자 `user_assertion` | `pass` |
| pass | 증거 0 | 거절 exit 4 `E_EVIDENCE_MISSING`, 단계는 판정 없음 |
| pass | 하나라도 `mismatch`/`not_found` | 거절 exit 4 `E_EVIDENCE_MISMATCH`, 기록 `pass_rejected` |
| pass | `uncertain`/eval_budget 포함 | `inconclusive`로 강등, exit 8 |
| pass | 증거가 단계 창 밖 | 거절 exit 4 `E_OUT_OF_WINDOW` |
| pass | `record.health=failed` 중 | 거절 exit 4 `E_RECORDING_FAILED` (TC-C8) |
| pass | 증거가 전부 agent 자신의 tx | 거절 `E_EVIDENCE_MISSING`(detail `"agent_tx_only"`) — 내가 보낸 것은 DUT 동작의 증거가 아님 (**TEST 판정 규칙표에 R-8로 추가 제안**) |
| fail | 증거 ≥ 1 `verified`(실패를 보여 주는 술어) | `fail` |
| fail | 증거 없음 | `fail`로 기록하되 `unsupported:true` 표시(보고서에 "근거 없는 실패") |
| inconclusive / blocked / skipped | 사유 필수 | 그대로 |
| 사용자 판정 | viewer 연결만(`test.verdict{origin:user, overrides, reason}`), agent client는 거부 | `user_assertion`으로 별도 기록, 자동 검증 판정과 나란히 표시. agent 판정을 사용자 판정으로 승격하지 않음 |
- 세션 결과: 모든 단계 `pass`(사용자 판정 포함)일 때만 `passed`. 하나라도 `fail` → `failed`, 그 외 `inconclusive`.

### O6. 단계 마커 이벤트와 UI 데이터
이벤트(같은 허브·저널, origin = 실행 주체, `actor_id=test:<session>`):
`test.session_started{session_id, tc_sha256, title, env, bench, plan_digest}`, `test.session_state{state}`, `test.step_started{step_id, n, title, expect_text}`, `test.step_ended{step_id}`, `test.mark{label}`, `test.evidence_checked{…O4}`, `test.verdict{step_id, proposed, effective, by, evidence_ids, reason, overrides?}`, `test.note`, `test.ask{question, timeout}`, `test.approval{approval_id, state}`, `test.session_ended{result, reason, counts}`.
- 이름 규칙: 이벤트는 `<domain>.<event>` 2토큰(코어 계약 §4.7)이라 TEST 안의 `test.session.start/end`, `test.step.begin/end`는 이벤트로는 `test.session_started/ended`, `test.step_started/ended`로 쓰고, **RPC 이름**은 `test.session.start`·`test.step.begin`처럼 3토큰 그대로 쓴다.
- 단계 마커는 gateway 이벤트이므로 모든 도메인 타임라인·`--since mark:`·`@step:`에서 같은 seq로 해석된다. 장치에 물리적으로 동시에 찍히는 것은 아니다(각 worker의 마커 시점 `covered_source_seq`는 위 core 제안의 fence ACK로 보존 — 수용).
- 리듀서 snapshot(UI 테스트 화면이 늦게 열려도 복원): 세션 메타, 단계 목록·상태, 단계별 증거 상태, 대기 승인, 사용자 개입 수(`others_since_last` 집계).

### O7. 일괄 사전 승인
- `--plan plan.json` = 에이전트가 TC를 읽고 만든 **예상 쓰기 목록**: `[{domain, method, pattern, max_uses, risk}]`. 코어는 항목마다 도메인 classify로 등급을 확인해 **R2·R3 묶음 하나의 viewer 승인**(`Approval{kind:TestPlan, scope:TestSession{session_id}}`)을 만든다. R4·default_blocked·immutable_deny 항목이 있으면 plan 전체 거부(R4는 단계마다 once).
- 바인딩: session_id, plan digest, env, 정책·프로파일·DB 해시, stream_epoch. 기한: 승인 사용 시작 ≤ 10 분(A9), 이후 세션 `max_test_duration`(기본 1 h — TEST 안, 설정 최대 8 h)까지 유효. 계획 밖 R2+ 쓰기는 개별 승인 + 보고서에 "계획 밖" 표시. 세션 종료·STOP·정책 변경·epoch 변경 시 무효.
- 사용: 세션 안 쓰기마다 도메인 `within_pattern(req, pattern)` + `classify` + `presend_check` 재확인, `max_uses` 차감. 패턴 밖이면 개별 승인 흐름(세션은 계속).

### O8. 러너와의 관계 · freeze
- `loom test start --run scenario.yaml`: run의 단계·`expect`가 그대로 test 단계·자동 증거가 된다(에이전트 없이도 세션 기록). 
- `loom run --test`는 같은 세션 엔진·검증기를 쓴다(TC-C4).
- `loom test freeze`: 과거 seq·토큰·시각·승인을 복사하지 않고 새 세션에 재바인딩하는 시나리오를 만든다. 세션 안 origin=agent 쓰기(인자·순서)·대기 명령·`verified` 증거 술어를 `loom_scenario: 1` 단계·`expect`로 변환. 자연어 판단이 들어간 단계(사용자 판정·수동 확인)는 `manual` 단계로 남긴다. 결정적이지 않은 대기(시간 의존)는 경고 주석.
- 보고 데이터(코어 제공 JSON): 세션 메타, TC 원문 해시, 단계·판정·증거 검증 결과(digest·matched refs·저널 위치 링크), 승인 사용 내역, 사용자 개입 목록, gap 요약, env·정책·빌드 버전. JUnit XML은 코어가 생성(testcase = 단계, `<failure>`/`<skipped>`, properties에 session_id·digest). HTML은 TEST·UI 템플릿.

### O9. TEST 요청(TC-C1–C9) 답
| 요청 | 답 | 비고 |
|---|---|---|
| 경계표(CORE: 세션·검증 엔진·승인 바인딩·저널 조회·보고 데이터·freeze 실행 / TEST: 형식·AST 명세·규칙표·템플릿·스킬) | **수용** | JUnit은 TEST가 quick-junit 템플릿을 소유한다면 코어는 데이터만 — O8 수정 가능 |
| TC-C1 이벤트·태깅, D3 선택 필드 `test_session`·`test_step` | **수용** — 허브 자동 태깅. 이벤트 이름만 2토큰 규칙으로(O6) | `r1-opus.md` §6.1에 필드 추가 |
| TC-C2 검증 절차 ①–④, 누락·타입 오류 = 오류 | **수용**(O4 0–3단계). 종료 코드 4: `E_EVIDENCE_MISSING`, `E_EVIDENCE_MISMATCH`, `E_OUT_OF_WINDOW` — 이름 채택 | 코어 계약 CR-C4 갱신 |
| TC-C2 `check --auto` 후보 제안 | **수용**(같은 예산, 후보 ≤ 20개, 제안일 뿐 자동 인용 안 함) | |
| TC-C3 hold·measure | **수용** — 코어 공통 술어 `hold`·`measure` | 스파이크 `spikes/explore/v2-evidence/` 참고 |
| TC-C3 일괄 승인 바인딩·R4 제외·deny 포함 시 거부·1 h·계획 밖 표시 | **수용**(O7) | A9 확장은 메인 결정 필요 |
| TC-C4 `loom run --test` 공유·freeze 재바인딩 | **수용** | |
| TC-C8 기록 불능이면 R1+ 쓰기와 pass 판정 중단 | **수용** — fail·inconclusive 기록은 허용(데몬 로그 폴백) | |
| TC-C9 예산·상한·soak 패턴 | **수용** — 평가 ≤ 200k 이벤트·200 ms, 세션 1(≤4), 단계 ≤ 500, REF ≤ 64/판정. soak에 10분 주기 TC 세션 포함, 세션 종료 후 RSS 반환 관문 | `r1-opus.md` §9.5에 반영 예정(R2) |
| 판정 수정 = viewer만 | **수용** | |
| 종료 코드 3(승인·ask timeout)·6(STOP·세션 진행 중) | **수용** — 6에 `E_SESSION_ACTIVE` 추가 | |
| (코어 추가 제안) PASS에 device 출처 증거 ≥ 1 | TEST 규칙표 R-8로 검토 요청 | O5 |
| (코어 추가 제안) 인용 세그먼트 보존 pin | TEST 검토 요청 — quota 부족 시 명시 실패 | O4-7 |
## test 의견 (explore-lead, 2026-11-10 — `test/plan.md` §3.1에 반영)

O1–O9를 **전부 수용**한다. TEST 계획의 인용·검증 모델을 O3–O5로 바꿨다(결정 T10–T13).
- **R-8 수용**: 증거가 전부 agent 자신의 tx이면 pass를 거부한다. 자극 전용 단계는 판정 `done`(쓰기 상태 ≥ confirmed)이고 pass로 세지 않는다.
- **보존 pin 수용**: quota가 부족하면 명시적으로 실패한다.
- **이벤트 이름**: 2토큰 규칙(`test.step_started` 등)과 RPC 3토큰을 수용한다.
- **CLI**: `step <id>`(이전 단계 자동 종료)와 `evidence add`(저수준)를 수용한다. `verdict --evidence REF`는 REF + 계획 `expect` 템플릿을 인용 형식으로 바꾸는 축약으로 유지한다.
- **JUnit**: 코어가 quick-junit으로 생성하고, 매핑 규칙(inconclusive = skipped + 사유, `--junit-inconclusive=error`)은 TEST가 소유한다. HTML은 TEST의 askama 템플릿이고 코어 데이터로 렌더한다.

TEST 점검(R4)으로 생긴 **추가 요청**:
1. **전수 평가**: 검증기는 인용 witness만 보지 말고 `filter`로 `range` 전체를 재조회해 술어를 평가한다(count/absent/hold 모수를 에이전트가 고르지 못하게). O4-4와 같은 뜻이면 확인만 해 달라.
2. **역할 검사**:
   - 관측 증거의 `origins` 기본값은 `["device"]`이다.
   - 요청·응답은 `request_id`로 짝을 짓는다.
   - 창 안에 **같은 domain·channel·target에 대한 사용자 쓰기**가 있으면 `uncertain`이다.
3. **anchor + within_ms**:
   - assertion 창은 대기 호출 시점이 아니라 anchor 이벤트 기준이다.
   - 재시도마다 attempt 창을 따로 둔다.
   - 장비 시각 증거(LA)는 capture→host 오차 구간이 경계를 가로지르면 `uncertain`이다.
4. **plan_summary**: 위험 요약은 에이전트가 쓰지 않는다. `test.plan.check`가 도메인 classify/`plan_summary`로 산출한다(finally·repeat·retry·분기 포함). 잔량은 원자적으로 차감하고 사용자 retry도 차감한다. **계획이 바뀌면 승인 전체 무효**, 바인딩에 stream_epoch를 포함한다(O7과 일치).
5. **계획 밖 쓰기**: O7대로 개별 승인 + 표시. 그런 세션의 `freeze`는 `--allow-unplanned` 없이는 거부하고, 허용해도 `needs_review`로 표시한다.
6. **집계·종료 코드**(D5 확정 필요):
   - 세션 결과는 `passed | failed | inconclusive`에 `incomplete`(필수 단계 미판정)를 더한다.
   - `test end` / `run --test`의 종료 코드: 0 passed, **11 `E_TEST_FAILED`(신규)**, 8 inconclusive·incomplete·blocked, 6 STOP.
   - 6에 `E_SESSION_PAUSED`도 추가한다.
7. **개입 RPC**: `test.pause/resume/skip/retry/answer`, `test.plan.amend`. PAUSED 중 세션 client의 새 쓰기는 exit 6 `E_SESSION_PAUSED`. user abort는 finally 실행 여부를 묻고, 전역 STOP은 A15를 따른다.
8. **C2**:
   - test 제어 큐 1k(넘치면 backpressure, 드롭 금지)
   - 동시 검사 ≤ 2
   - 메모 ≤ 4 KiB·세션당 ≤ 1k
   - 세션 엔진 RSS 64 MiB
   - 보고서는 스트리밍 생성

## core 답변 (core-lead R3 통합, 2026-11-10)
`## core 의견 (core-opus R1)` O1–O9 + R2 교차 리뷰 보강 + 위 `## test 의견` 추가 요청 1–8을 통합한 CORE 입장. 정본 `docs/plan/core/plan.md` v2.0 §10.9.

- **TEST 추가 요청 1–8 전부 수용**: ① 검증기는 filter로 range 전체 재조회(인용 witness만 보지 않음 — O4-4와 같은 뜻), ② 관측 origins 기본 `["device"]`, request_id 짝, 창 안 같은 대상 사용자 쓰기 → `inconclusive{cause:"user_intervention"}`, ③ anchor 기준 창·attempt 창·LA 오차 경계 → inconclusive, ④ `test.plan.check`가 도메인 `plan_summary`로 위험 산출, 잔량 원자 차감(사용자 retry 포함), 계획 변경 = 일괄 승인 전체 무효, 바인딩에 stream_epoch, ⑤ 계획 밖 쓰기 세션 freeze는 `--allow-unplanned` + `needs_review`, ⑥ 결과 `passed|failed|inconclusive|incomplete`, 종료 코드 0/**11 `E_TEST_FAILED`**/8/6(`E_SESSION_PAUSED`) — 11·`E_SESSION_PAUSED`는 CR-C17로 메인에 올림(CORE 권고 승인), ⑦ `test.pause/resume/skip/retry/answer`·`test.plan.amend`, PAUSED 중 세션 client 쓰기 exit 6, ⑧ 제어 큐 1k(드롭 금지)·동시 검사 ≤ 2·메모 상한·세션 엔진 ≤ 64 MiB·스트리밍 보고서.
- **R2 보강(O1–O9 수정)**: 판정은 등록된 `step.expect` id·hash에 결속(다르면 `E_PLAN_MISMATCH`, exit 4). 술어 coverage 분류 `Positive`(exists·count≥N) / `Complete`(absent·exact·max·hold·order) / `Measure`(within·from/to) — Complete·Measure만 gap·보존 삭제·no-record·스캔 한도 겹침 시 inconclusive. **witness 유형별 PASS**(긍정 관측·완전 창 부재·측정·user assertion) — O5의 "device ≥ 1 일괄"을 대체하되 **agent 자신의 tx만으로 PASS 불가(R-8)는 유지**. 검증 중 임시 pin lease → verified면 정식 pin. 부재 digest에 expect·창·필터·coverage·DB/정책 버전·gap 상태 포함. 평가 예산은 협력 budget token(강제 중단 아님) + AST 깊이·노드·정규식 입력 상한.
- **증거 REF epoch**: 사람용 REF에 `@e<epoch>`를 넣는 `<domain>:<channel>@e<epoch>:seq:a..b#<pointer>`를 제안(데몬 재시작이 낀 세션의 모호성 제거). 세션 바인딩으로 생략 가능한 축약은 허용. JSON DTO는 `ref{stream_epoch, seq_range, source_refs[]}`. **TEST 동의 요청.**
- 장치 RX의 단계 귀속은 publish 순서가 아니라 단계 시작·끝의 **worker별 source fence**와 수집 시각.
- 일정: M1 = `core.mark` + `test.step_started/ended` 마커만, 검증기·일괄 승인·보고 데이터 = P2(C27).
