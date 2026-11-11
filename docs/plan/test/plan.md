# TEST 기획 — TC·자연어 기반 통합 테스트 (v2.0)

작성: explore-lead(TEST 기획 담당, 마스터 C7), 2026-11-10. 기준: `02-core-principle.md`(최우선) → 마스터 §2.3 C1–C8(특히 C7·C8) → B → A → D, `test/brief.md`.
입력: `r1-opus.md`(구조·UX·스킬·예시 TC 초안), `r1-sol.md`(사실 확인·Rust 스파이크 — `spikes/explore/v2-evidence/`, `spikes/explore/v2-saleae-rs/`).
성격: **계획 문서. 구현 코드 없음.** 불확실한 사실은 "미확인". `core/plan.md`, `ui/plan.md`는 작성 중이라 경계는 `_xteam/core-test.md`, `_xteam/ui-test.md`로 제안했다(합의 전). 결정 기록은 §16.

## 핵심 루프 — 테스트 실행에서의 4요소

| 요소 | TC 실행에서 무엇인가 | 구현 (명령 · 코어 · UI) |
|---|---|---|
| ① 직접 접근 | 에이전트가 TC 해석 계획대로 **도메인 명령을 직접** 부른다. 테스트 전용 장치 경로는 없다 | `loom uart/can/uds/dlt/la/psu …`. `loomd`가 장치를 소유한다. 테스트 세션은 같은 client에 **태그만** 붙인다 |
| ② 읽기 | 기대 결과를 확인할 **증거**를 경계 있는 대기·조회로 얻는다 | `uart expect`, `can sig wait-for`, `uds dtc list`, `dlt wait-for`, `la wait`. 결과 `--json`의 `data.evidence[]`에 seq 핸들이 들어 있다 |
| ③ 쓰기 | 단계의 자극(송신·전원·DTC 삭제 등). 세션 시작 때 **일괄 사전 승인** 범위 안에서 실행한다 | 같은 쓰기 파이프라인(D10/A1)을 탄다. `test.step.begin/end` 마커를 붙인다 |
| ④ 사용자 실시간 UI + 개입 | 사용자는 **테스트 실행 화면**에서 현재 단계·실시간 증거·판정·승인 상태를 보고 일시정지·건너뛰기·재시도·판정 수정·STOP을 한다 | UI 앱 테스트 화면(§5). 모든 도메인 타임라인에 단계 띠가 그려진다. 사용자 조작은 origin=user로 기록된다 |

**판정 원칙**: PASS는 Loom 검증기가 확인한 증거가 있을 때만 성립한다. 에이전트의 말은 판정 근거가 아니다.

---

## 1. 범위 / 비범위

**범위**(이 계획이 정하는 것)
- TC 형식: 정본 형식, 자연어·반구조 표 입력, 엑셀·문서 가져오기.
- **해석 계획**(plan) 형식과 사람 확인 흐름.
- 테스트 세션 CLI(`loom test …`): 일괄 사전 승인, 단계 마커.
- 증거 참조 형식, 술어 DSL, 판정 규칙(환각 방지), 실패·개입 규칙.
- 보고서(HTML, JUnit XML, JSON)와 회귀 고정(`freeze` → `loom run`).
- `loom-test` 스킬, UI 테스트 실행 화면 명세(→ `_xteam/ui-test.md`), 예시 TC, 마일스톤.

**CORE와의 경계**(`_xteam/core-test.md`에서 합의)

| CORE 소유(Rust, `loomd`) | TEST 소유(명세·스킬·템플릿) |
|---|---|
| 테스트 세션 객체·상태기계, 단계 마커 이벤트, client↔세션 태깅 | TC 정본 형식, plan 스키마(러너 D12와 공유), 가져오기 매핑 |
| 증거 검증 엔진(존재·시간 창·술어 평가), 판정 저장, 일괄 승인 바인딩 | 술어 DSL **명세**, 판정 규칙표, 실패 행동 규칙 |
| 저널 조회(seq 범위), 보고서 데이터 API, `freeze` 변환기 실행 | 보고서 템플릿(HTML/JUnit), `loom-test` 스킬, UI 화면 명세 |

**비범위**
- 자연어를 해석하는 LLM 자체(그 역할은 에이전트가 한다).
- TC 관리 시스템(버전·배정·이력 DB).
- 성능 부하 테스트 프레임워크.
- 비전(카메라) 판정. 시각 확인은 `manual` 단계로 사람이 판정한다.
- 원격 다중 벤치 오케스트레이션.

## 2. 사용 시나리오 (예시 TC 3개 — 자연어 → 계획 → 명령 → 증거 → 판정)

### TC-1 콜드 부팅 스모크 (UART + CAN + UDS + PSU sim) — **M1 E2E 수용 시나리오(시뮬레이션)**
자연어 원문:
> 전원을 껐다 켠 뒤 30초 안에 콘솔에 `login:`이 떠야 한다. 부팅 후 5초 안에 CAN에서 PowerMode가 2(RUN)가 되어야 한다. BCM DTC(mask 09)가 없어야 하고, 있으면 지운 뒤 다시 읽어 없어야 한다.

해석 계획(에이전트가 작성 → 사용자가 UI에서 확인, §6.2 형식):
```yaml
loom_tc_plan: 1
tc: TC-1@sha256:9c1e…            # 원문 해시
env: sim
steps:
  - id: S1                       # 전원 사이클 (자극 전용 → 판정 done)
    tc_step: 1
    do: [{psu.out: {name: bench, state: "off"}}, {sleep: 2s}, {psu.out: {name: bench, state: "on"}}]
  - id: S2                       # 부팅 프롬프트: 전원 ON 후 30 s 안
    tc_step: 2
    uart.expect: {port: dut, pattern: "login:", timeout: 30s}
    expect:
      - filter: {domain: uart, channel: dut, kinds: [uart.data], origins: [device]}
        pred: {op: uart.text_contains, pattern: "login:"}
        anchor: {step: S1, kind: psu.write, where: {field: /payload/state_out, op: "==", value: "on"}}
        within_ms: 30000
  - id: S3                       # PowerMode: 부팅(S2 매치) 후 5 s 안
    tc_step: 3
    can.sig.wait-for: {channel: ch0, cond: "VehState.PowerMode == 2", timeout: 5s}
    expect:
      - filter: {domain: can, channel: ch0, kinds: [can.signal], origins: [device]}
        pred: {op: exists, where: {all: [{field: /payload/name, op: "==", value: VehState.PowerMode},
                                          {field: /payload/value, op: "==", value: 2}]}}
        anchor: {step: S2, kind: uart.expect_result}
        within_ms: 5000
  - id: S4                       # BCM DTC(mask 09) 없음
    tc_step: 4
    uds.dtc.list: {target: bcm, mask: 0x09}
    expect:
      - filter: {domain: uds, channel: bcm, kinds: [uds.rx], origins: [device]}
        pred: {op: uds.dtc_absent, mask: 0x09}     # 19 02 쌍·complete·마스크·단계 시작 이후
    on_fail: {goto: S5}
  - id: S5                       # 있으면 삭제 후 재확인 (R2)
    tc_step: 4
    when: {step: S4, verdict: fail}
    do: [{uds.dtc.clear: {target: bcm}}, {uds.dtc.list: {target: bcm, mask: 0x09}}]
    expect:
      - filter: {domain: uds, channel: bcm, kinds: [uds.rx], origins: [device]}
        pred: {op: uds.dtc_absent, mask: 0x09}
finally: [{psu.out: {name: bench, state: "off"}}]   # safe_state
# 위험 요약은 쓰지 않는다 — `loom test plan check`가 classify로 산출(§3.1):
#   R3 psu.out on ×1, psu.out off ×2(S1·finally), R2 uds.dtc.clear ×1
```

명령과 결과(발췌, `--json` 봉투 D5):
```
$ loom test start tc/TC-1.tc.md --plan tc/TC-1.plan.yaml
{"schema_version":1,"ok":false,"cmd":"test.start","error":{"code":"E_APPROVAL_REQUIRED","message":"plan approval pending in UI","hint":"ask user to review plan TC-1 in Test screen","retryable":true,"request_id":"r88"}}   # exit 5
  ... 사용자가 UI에서 계획 확인 + 일괄 승인(R2×1, R3×1) ...
$ loom test start tc/TC-1.tc.md --plan tc/TC-1.plan.yaml --wait-approval 300s
{"ok":true,"cmd":"test.start","data":{"session":"ts-0042","plan_hash":"sha256:41ab…","approval":"ap-17","steps":5},"others_since_last":{"count":0}}
$ loom test step ts-0042 S2 --begin
$ loom uart expect dut 'login:' --timeout 30 --json
{"ok":true,"cmd":"uart.expect","data":{"match":"login:","t_ms":11840,"evidence":[{"ref":"uart:dut@seq:18231","kind":"uart.match","text":"buildroot login:"}]}}
$ loom test verdict ts-0042 S2 pass --evidence uart:dut@seq:18231
{"ok":true,"cmd":"test.verdict","data":{"step":"S2","verdict":"pass","verified":{"exists":true,"in_window":true,"predicate":true,"detail":"11.84 s after psu.out.on seq:18010 ≤ 30 s"}}}
```
- S4에서 가상 ECU가 DTC 1개(`B1234-00`)를 반환한다. 판정은 fail(증거 `uds:bcm@seq:18402`, `uds.rx` `/payload/decoded/dtc_count=1`)이고 S5로 간다.
- `dtc.clear`는 일괄 승인 범위 안이다. 재조회 결과 `dtc_count=0`이므로 S5는 pass다.
- **사용자 개입 시연**: S3 대기 중 사용자가 UI에서 **일시정지**를 누르고 CAN IG 패널에서 프레임 1개를 직접 보낸 뒤 계속을 누른다. 에이전트의 다음 결과에 `others_since_last{count:1, origins:[user]}`가 붙는다. 스킬 규칙에 따라 에이전트는 S3 판정에 사용자 프레임이 섞였는지 확인하고 판정 메모에 적는다. 사용자 프레임 seq는 증거에서 제외된다.
- TC 판정: S4 fail은 계획상 분기(`on_fail: goto`)이고 복구 경로 S5가 pass이므로 TC 결과는 **passed**다(§3.1 집계, R-6). 보고서에는 S4 fail 사실이 그대로 남는다. S1은 자극 전용이라 `done`이다.
- **사용자 STOP 변형**: S3 대기 중 전역 STOP → 에이전트 exit 6, finally 미실행, UI에 복구 송신 키(`psu.out off`). TC는 inconclusive(exit 6).

### TC-2 DLT 레벨 변경 + LA로 UART 물리 확인 (DLT + LA(Saleae) + UART)
- 원문: "CAM 컨텍스트를 debug로 올리고 콘솔에 `cam stat`을 보내면, DLT에 debug 로그가 2초 안에 나와야 하고, UART TX 선에 실제 `cam stat\r`이 115200으로 나가야 한다."
- 계획:
  - S1 `dlt.level`(R1) → expect `dlt.ctrl` 상태 `effective_verified`.
  - S2 `la.capture`(trigger uart_tx:fall, 2 s, async 디코드) + `uart.send`.
  - S3 `la.wait` → expect `la.decode` 행의 바이트열 = `63 61 6d 20 73 74 61 74 0d`, 그리고 `uart.tx` seq와 정렬 매치.
  - S4 `dlt.wait-for --level debug --ctx CAM --timeout 2s` → expect `dlt.log` 1건 이상.
- 증거 예: `dlt:ecu1@seq:9120`(ctrl ack), `la:sal@cap:0f3a…#row:0..8`(디코드 행), `uart:dut@seq:18501`(tx), `dlt:ecu1@seq:9177`(debug 로그).
- 판정 주의: LA는 캡처 후 표시라 증거가 캡처 종료 뒤에 생긴다. 시간 창은 §3.1-6에 따라 capture→host 매핑과 오차 구간으로 검사한다. DLT의 "2초 안"은 `uart.send` 쓰기를 anchor로 둔 assertion 창이므로, 에이전트가 LA를 기다린 뒤 `dlt.wait-for`를 불러도 저널 기준으로 판정된다(§3.1-3). 에이전트는 DLT 대기를 자극 **전에** 걸어 두는 편이 안전하다(스킬 예시).

### TC-3 전원 사이클 부팅 시간 (PSU sim + UART + DLT)
- 원문: "전원 ON부터 `boot complete` DLT 로그까지 8초 이하. 5회 반복해 최대값으로 판정."
- 계획: `repeat: {count: 5}` × [`psu.cycle`(R3 — 일괄 승인 count=5), `dlt.wait-for 'boot complete' --timeout 20s`] → `measure: {from: psu.out.on, to: dlt.match, agg: max}` → expect `max_ms <= 8000`.
- 증거: 회차마다 두 seq 쌍. 측정값은 검증기가 seq의 `ts.mono_ns` 차이로 **직접 계산**한다(에이전트가 적은 숫자는 쓰지 않는다).
- 판정: 4회 7.1–7.6 s, 1회 `source.gap{cause:mcnt_discontinuity}`가 창 안에 있으면 그 회차는 inconclusive이고 TC 결과도 **inconclusive**다. 손실 구간에서 "없음"이나 시각을 주장하지 않는다.

**사용자 관점 시나리오**: (a) 엑셀 TC 시트를 가져와 계획을 검토·수정 후 승인. (b) 실행 중 일시정지 → 직접 조작 → 계속. (c) 에이전트 판정을 UI에서 수정(사유 필수) → 보고서에 원판정과 수정 판정이 함께 남음.

## 3. 아키텍처

```
 사람 ── TC(.tc.md / 엑셀) ──▶ 에이전트(loom-test 스킬) ──▶ 해석 계획(.plan.yaml)
                                       │  loom test plan check (정적 검증: 명령 존재·등급·timeout·술어 파싱)
                                       ▼
 UI 테스트 화면 ◀─ core.subscribe ─ loomd ─┬─ test 세션 엔진 (CORE) ── 판정 저장소
   계획 검토/승인, 진행, 증거, 판정, 개입    │    ├─ 증거 검증기: 저널 seq 조회 → 창 검사 → 술어 평가
                                          │    └─ 일괄 승인 바인딩(plan hash·횟수·대상)
                                          ├─ 쓰기 파이프라인(policy→approval→audit→worker)
                                          └─ 도메인 worker(uart/can/uds/dlt/la …) ── 장치
 에이전트 ── loom <domain> … (LOOM_CLIENT=claude-1, 세션 태그 자동) ─▶ 같은 RPC
 결정적 재실행: loom run frozen.scenario.yaml --test  ─▶ 같은 세션 엔진·같은 검증기
```
- **세션 태깅**: `test start`가 세션을 `client_id`에 묶는다. 이후 그 client의 모든 이벤트에 `test_session`·`test_step`이 붙는다(D3 확장 요청). 장치 RX는 시간 창으로만 단계에 연결된다. 출처는 바꾸지 않는다.
- **단계 마커**: `test.step.begin/end`(dir=null, domain=test)를 발행하면 UI 타임라인의 **모든 레인**에 띠가 그려진다. 러너도 같은 이벤트를 쓴다.
- **하나의 엔진**: 테스트 세션 = 기록·판정 계층이다. 실행자는 에이전트(대화형) 또는 러너(`loom run`, 결정적) 둘 중 하나다. 검증기·보고서·UI는 같다.
- **증거 검증기 절차**: §3.1(R4 개정, CORE O3–O5 수용)이 정본이다. 요지: 인용 = 범위 + 필터 + 술어이고, 검증기는 **필터된 범위 전체**를 저널에서 다시 읽어 평가한다(에이전트가 고른 샘플만 보지 않음).
- **누락 필드·타입 불일치는 "거짓"이 아니라 오류**다(결정 T3). 오류는 `inconclusive` 후보로 돌리고 메시지에 `seq=43 path=/payload/value expected finite number`처럼 seq·경로·기대·실제를 넣는다. 스파이크에서 JSONPath는 누락 필드를 조용히 false로 처리했다 → 필드 선택은 JSON Pointer + 명시 타입 검사로 한다(`r1-sol.md` A3).
- **hold(값 유지) 의미**: 같은 source·신호·epoch의 **연속 관측 샘플** 기준. 시각이 단조 증가하고 샘플 간격이 상한(기본 150 ms, 신호별 설정) 이하일 때만 누적한다. gap·epoch 변경에서 누적이 끊기고, 그 구간에 걸친 hold는 `inconclusive`다. 샘플 사이의 물리적 연속을 증명하는 것은 아니며 보고서에 "관측 기준"을 명시한다(스파이크는 CEL·JSONPath 두 엔진 + 공통 Rust 시간 연산자로 10사례 통과. **자체 비교 AST 파서·타입 검사·Pointer 평가기는 아직 미구현 — M0 관문**).
- **평가 예산(C2)**: 식 길이·깊이, 검사 대상 이벤트 수(기본 ≤ 200k/검사), 결과 후보 수, 실행 시간(기본 ≤ 200 ms/검사)에 상한을 둔다. 초과 시 `inconclusive{cause:"eval_budget"}`. 동기 평가를 async timeout만으로 끊을 수 있다고 가정하지 않고 평가기 안에서 예산을 센다.

### 3.1 증거 검증 모델 (R4 개정 — `r4-sol-check.md` P1·P2, CORE `core-test.md` O3–O5 수용, 결정 T10–T13)

**인용(citation)** = `{range, filter, pred, anchor?, claim}` (CORE O3 형식)
- `range`: stream 좌표 `{epoch, from_seq, to_seq}`. 커서 문법 `seq:N`·`mark:X`·`@step:<id>`(단계 전체) 허용. **REF 문자열**(`<domain>:<channel>@[e<epoch>/]seq:<a>..<b>`, epoch 생략 = 현재)은 도메인 결과 `data.evidence[]`가 주는 이 좌표의 축약형이다. 에이전트는 REF를 복사만 한다(T4). 예외: `<domain>:<channel>@req:<request_id>`(channel `*` 허용)는 UI 링크 칩용 **이동(reveal) 전용 REF**이며 판정 증거로 인정하지 않는다. LA 캡처 REF는 `la:<dev>@cap:<capture_uuid>#row:<a>..<b>`이고 artifact sha256에 묶인다(재사용되는 앱 캡처 번호 금지).
- `filter`: `{domain, channel, kinds[], request_id?, origins[]}`. **관측 증거의 `origins` 기본값 = `["device"]`.**
- `pred`: 공통 술어(`exists`, `absent`, `count`, `order`, `within_ms`, `hold`, `measure`) 또는 도메인 술어(`uart.text_contains|text_absent|text_count|text_sequence`, `uds.response_eq|dtc_absent`, `can.signal_range|id_period`, `la.decode_eq`, `dlt.log_match` …). 비교식은 TEST 비교 AST. 임의 실행기(CEL·스크립트)는 없음.
- `anchor`: 시간 기준점 이벤트(이전 단계 것이어도 됨) + `within_ms`. 예: "부팅 후 5 s" = anchor `uart.expect_result(login:)`, within 5000.

**검증 절차** (CORE O4를 TEST 규칙으로 고정)
1. **전수 평가**(P1-1): 검증기는 인용한 증거만 보지 않는다. `filter`로 `range` **전체**를 저널에서 다시 읽고 `pred`를 평가한다. `count/min/max/absent/hold`의 모수는 에이전트가 고를 수 없다. 에이전트가 낸 REF는 witness 후보일 뿐이다.
2. **완결성**: 범위와 겹치는 `record.gap`·`source.gap`·no-record·보존 삭제·epoch 변경을 조회한다.
   - 부재·개수·구간·hold 술어는 겹침이 있으면 `uncertain` → inconclusive다.
   - 존재 술어는 일치 이벤트가 있으면 verified다(겹침은 경고).
3. **창 규칙**(P1-4): 단계 판정의 **assertion 창** = `[anchor 또는 단계 시도 시작, + within_ms 또는 단계 끝]`이다.
   - 에이전트가 대기 명령을 언제 불렀는지와 무관하게 저널 기준으로 본다. 예: TC-2의 "자극 후 2 s"는 LA 캡처가 끝난 뒤 검사해도 자극 시각 기준으로 판정된다.
   - anchor는 이전 단계 이벤트여도 되지만, 증거 자체는 현재 단계 **시도(attempt)** 창 안이어야 한다. 재시도마다 새 attempt 창이 생긴다.
4. **역할 검사**(P1-2):
   - 관측 증거는 `origins`·`dir`·`kind`·`source`가 계획의 기대와 맞아야 한다.
   - 요청·응답은 `request_id`로 짝을 지어야 한다(다른 요청의 응답 불가).
   - **에이전트 자신의 tx만으로는 pass 불가**(R-8, CORE 제안 수용).
   - 창 안에 사용자 쓰기(origin=user)가 **같은 domain·channel·target**에 있으면 원인을 구분할 수 없다고 보고 `uncertain` → inconclusive다. 사용자 판정으로만 해소된다.
5. **도메인 부재 증명**(P1-3): UDS DTC 부재는 `uds.dtc_absent`만 쓴다. 이 술어는 다음을 모두 요구한다(UDS 의견 3).
   - 같은 `request_id`의 `19 02` 요청·긍정 응답 쌍
   - 마스크가 판정 비트를 포함
   - `/payload/decoded/complete == true`
   - 단계 시작 이후의 응답
   
   평면 `dtc_count == 0` 비교는 쓰지 않는다. UART 부재·개수도 `complete:false`면 inconclusive다.
6. **시각 변환**(P1-5): LA 등 장비 시각 증거는 `capture→host` 매핑과 **오차 구간**을 증거에 함께 기록한다.
   - 시간 술어의 경계가 오차 구간 안에 들면 inconclusive다.
   - 정렬 앵커(UART tx 디코드 매치)가 있으면 오차를 좁힌다.
   - 캡처 시작 단계와 사후 검증 단계는 같은 단계로 묶거나 anchor로 연결한다.
7. **예산**(C2): 검사당 스캔 ≤ 64 MiB·I/O ≤ 2 s, 평가 이벤트 ≤ 200k·CPU ≤ 200 ms, 동시 검사 ≤ 2. 넘으면 `inconclusive{cause:"eval_budget"}`.
8. **결과**: `test.evidence_checked{status: verified|mismatch|not_found|uncertain|error, observed(≤ 2 KiB), matched_refs, digest}`. 인용된 세그먼트는 보존 GC에서 pin한다. quota 부족이면 명시적으로 실패하고 조용히 지우지 않는다.

**판정 집계**(P1-8, CORE O5 + TEST R-1..R-8)
- 단계 `pass` = 인용 ≥ 1, 전부 verified, origin=device 증거 ≥ 1(또는 사용자 판정).
- 자극 전용 단계(expect 없음)의 판정은 `done`이다. 근거는 쓰기 결과 REF의 상태가 `confirmed` 이상인지다. pass 근거로 집계하지 않는다.
- 근거 없는 `fail`은 `unsupported:true`로 기록한다.
- 에이전트는 `skipped`를 `when` 분기 미충족에만 쓸 수 있다.
- 사용자가 필수 단계를 건너뛰면 TC는 **최대 inconclusive**다.
- TC `passed` = 모든 필수 단계 pass 또는 done.
  - 미판정·미실행 필수 단계가 있으면 `incomplete` → inconclusive.
  - `on_fail: goto` 대체는 복구 경로의 필수 단계가 모두 pass일 때만 성립하고, 원래 fail은 보고서에 남는다.

**최종 종료 코드**(P2-8 — CORE·메인에 D5 확정 요청): `loom test end`와 `loom run --test`
- 0 = passed
- **11 `E_TEST_FAILED`**(신규 제안) = failed
- **8** `E_TEST_INCONCLUSIVE` = inconclusive·incomplete·blocked(부분 결과 의미)
- 6 = 사용자 STOP

JSON 판정·JUnit 표현·프로세스 종료 코드는 서로 독립이다. CI 게이트는 종료 코드로 한다.

**세션 상태와 개입 RPC**(P2-7)
| 상태 | 들어가는 입력 | 그 상태에서 |
|---|---|---|
| WAITING_APPROVAL | `test.start` | 쓰기 없음. 계획·일괄 승인 대기(exit 5, `--wait-approval`) |
| RUNNING | 승인 | 정상 |
| PAUSED | 사용자 `test.pause` | 세션 client의 **새 쓰기는 exit 6 `E_SESSION_PAUSED`**. 읽기·대기는 허용하되 진행 중이던 대기 결과에 `during_pause` 표시. 사용자 조작은 `operator_action`으로 자동 기록 |
| RUNNING | 사용자 `test.resume` | 다음 결과의 `others_since_last`에 pause 구간 요약 |
| (단계) | 사용자 `test.skip{reason}` / `test.retry`(잔량·epoch·인터록 재검사) / `test.answer` | — |
| STOPPED | 사용자 `test.end{abort}` | 대화상자에서 finally 실행 여부를 사용자가 고른다 |
| STOPPED | 전역 STOP | finally 자동 실행 안 함, 복구 송신 1키(A15) |
| DONE | `test.end` | finally → 판정 집계 → 세션 immutable |

**일괄 승인**(P1-6, P1-7)
- 위험 요약은 에이전트가 쓰지 않는다. `loom test plan check`가 계획의 모든 동작을 대상으로 **Loom 도메인 classify·`plan_summary`**(UDS `uds.plan_summary{intents[]}` 등)를 불러 산출한다.
  - 대상 동작: `finally`, `repeat`, `retry`, 분기 양쪽 포함.
  - 결과: `{domain, method, pattern(대상·서비스·값 범위), risk, max_uses}`.
- 바인딩: plan digest·env·정책/프로파일/DB 해시·**stream epoch**.
- 쓰기마다 잔량을 원자적으로 차감한다. 사용자 재시도도 같은 잔량에서 차감한다.
- **계획이 바뀌면 기존 승인은 무효**이고 새 digest로 전체를 다시 승인한다(부분 이월은 M3 이후 CORE 계약으로 검토).
- 계획 밖 R2+ 쓰기(P2-6):
  - 스킬은 하지 않는다. 필요하면 `loom test plan amend` → UI 재승인.
  - 코어가 받으면 개별 승인 + "계획 밖" 표시.
  - 그런 세션의 `freeze`는 `--allow-unplanned` 없이는 거부한다. 허용해도 결과 시나리오는 `needs_review`로 표시한다.

**계획 스키마 하나**(P2-3)
- 단계 = `{id, tc_step, <domain>.<verb>: {args} | do: [{<domain>.<verb>: {args}}…], expect: [citation 템플릿], anchor?, on_fail, when, retry, manual}`.
- citation 템플릿 = `{filter, pred, anchor?, within_ms?}`. 실행 때 범위가 채워진다.
- `loom_scenario: 1`의 단계는 이 스키마의 부분집합이다. TC-1 예제는 이 형식으로 고쳤고 M0 fixture로 검증한다.

**C2 상한 보강**(P2-5)
- test 제어 큐 1k: 넘치면 **backpressure**(에이전트 명령이 exit 6 `E_LOCKED` busy)이고 드롭하지 않는다.
- 메모 ≤ 4 KiB·세션당 ≤ 1k.
- 세션 엔진 RSS 예산 64 MiB.
- 보고서는 **스트리밍 생성**(증거 표를 저널에서 순차 렌더)한다.

## 4. CLI / API 명세 초안 (+ `loom-test` 스킬)

D5 봉투·종료 코드·커서를 따른다. 모든 결과에 `others_since_last`가 붙는다.

| 명령 | 역할 | 비고 / 종료 코드 |
|---|---|---|
| `loom test import <xlsx\|csv> --map map.yaml [--sheet S] -o tc/` | 표 → `.tc.md` 여러 개 | 결정적 변환. 매핑 없는 열은 `notes`로 |
| `loom test plan check <plan.yaml>` | 정적 검증: 명령·인자 스키마, 등급 요약, timeout 필수, 술어 파싱, 증거 종류 존재 | 2 스키마 오류, 4 `default_blocked`/`immutable_deny` 포함 |
| `loom test plan show <plan.yaml>` | 사람용 요약(단계·등급·승인 묶음) | |
| `loom test start <tc.md> --plan <plan.yaml> [--wait-approval 300s]` | 계획 검토 요청 + 일괄 승인 대기 → 세션 시작 | 5 `E_APPROVAL_REQUIRED`, 3 승인 대기 timeout, 6 `E_SESSION_ACTIVE`(다른 세션 진행 중) |
| `loom test step <sess> <id> [--end]` | 단계 마커. 다음 단계 시작 시 이전 단계 자동 종료(CORE O2) | 4 순서 위반 |
| `loom test check <sess> <id> [--evidence REF…] [--auto]` | 검증만(판정 저장 안 함). `--auto`는 창 안에서 술어를 만족하는 이벤트 후보 ≤ 20개를 제안(자동 인용 안 함) | 0 + 결과 표, 8 uncertain |
| `loom test evidence add --step <id> --range … --filter … --pred …` | 저수준 인용 등록 + 즉시 검증(CORE O2). `verdict --evidence REF`는 REF + 계획 `expect` 템플릿을 이 형식으로 바꾸는 축약 | |
| `loom test verdict <sess> <id> pass\|fail\|inconclusive\|blocked\|skipped --evidence REF… [--note T]` | 판정 기록 | **4 `E_EVIDENCE_MISSING`/`E_EVIDENCE_MISMATCH`/`E_OUT_OF_WINDOW`**(pass 거부) |
| `loom test evidence <sess> <id> --add REF [--label L]` | 보조 증거(첨부, 판정 근거 아님) | |
| `loom test note <sess> [<id>] TEXT` | 메모(에이전트 관찰, 사용자 개입 설명) | |
| `loom test ask <sess> --question T [--options a,b] --timeout 600s` | 사람에게 질문 → UI 대화상자, 답은 origin=user | 3 timeout |
| `loom test status <sess>` / `wait <sess> --timeout` | 진행 상태 / 사용자 개입·일시정지 해제 대기 | 6 `E_INTERRUPTED_BY_USER`(STOP) |
| `loom test plan amend <sess> <plan.yaml>` | 실행 중 계획 변경 요청 → 기존 승인 무효 → UI 재승인 | 5 |
| `loom test pause\|resume\|skip\|retry\|answer` | 개입 RPC(사용자는 UI, 에이전트는 retry만 계획 범위 안) — 상태표 §3.1 | 6 `E_SESSION_PAUSED` |
| `loom test end <sess> [--abort]` | 종료 → finally(A15) → TC 판정 집계 | **0 passed / 11 `E_TEST_FAILED` / 8 inconclusive·incomplete·blocked / 6 STOP**(§3.1) |
| `loom test report <sess> --html --junit --json -o out/` | 보고서 | 8 기록 gap 포함 시 부분 표시 |
| `loom test freeze <sess> -o regress/X.scenario.yaml` | 결정적 시나리오로 고정(§6.5) | 비결정 단계 목록을 경고로 |

판정 거부 예:
```
$ loom test verdict ts-0042 S3 pass --evidence can:ch0@seq:17990
{"ok":false,"cmd":"test.verdict","error":{"code":"E_OUT_OF_WINDOW","message":"seq:17990 at t=-1.2s is before step S3 begin","hint":"cite evidence produced inside S3, or run 'loom test check ts-0042 S3 --auto'","retryable":true}}   # exit 4
```

**증거 참조 문법**(REF, 결정 T4): `<domain>:<channel>@[e<epoch>/]seq:<a>[..<b>][#<pointer>]`(epoch 생략 = 현재). 사후 생성물은 `la:<dev>@cap:<capture_uuid>#row:<a>..<b>`(artifact sha256에 묶임). 인용 전체 형식은 §3.1. `<pointer>`는 JSON Pointer(RFC 6901, 예: `#/payload/value`)다. 도메인은 결과 `data.evidence[]`에 이 REF를 그대로 넣어 준다(§15 요청) — 에이전트는 seq를 지어내지 않고 **복사**한다.

### 4.1 `loom-test` 스킬 초안 (`skills/loom-test/SKILL.md`, 본문 60줄 이하)
```markdown
---
name: loom-test
description: Run mixed-module integration tests (UART/CAN/UDS/DLT/LA/PSU) from natural-language or table TCs with evidence-backed verdicts. Use when the user gives a TC, test sheet, or asks to "test/verify" device behavior.
---
# loom-test
먼저 `loom` 스킬(데몬·종료 코드·승인·others_since_last)과 해당 도메인 스킬을 따른다. 명령 세부는 `loom <domain> help --json`이 정본.

## 1. 해석 (실행 전)
- TC를 `.tc.md`로 정리(없으면 만든다). 원문 문장은 고치지 않고 `source:`에 보존.
- 각 단계를 `loom` 명령 + timeout + `expect` 술어로 옮긴 `.plan.yaml`을 쓴다. 모호하면 추측하지 말고 `loom test ask`로 묻는다.
- 기대 결과가 기계로 확인 불가(화면·소리·사람 눈)면 `manual` 단계로 둔다.
- `loom test plan check` 통과 후 `loom test start`. 사용자가 UI에서 계획·일괄 승인을 할 때까지 기다린다(직접 승인 시도 금지).

## 2. 실행
- 단계마다 `loom test step <s> <id> --begin` → 명령 → `--end`. 계획에 없는 쓰기는 하지 않는다(필요하면 `loom test plan amend` → UI 재승인, 기존 승인은 무효).
- 대기는 항상 경계 있는 명령(`expect`, `wait-for`, `la wait`)과 계획의 timeout. `sleep`으로 결과를 기다리지 않는다. 판정 창은 anchor 기준이라, 자극 뒤 로그를 볼 대기는 가능하면 자극 **전에** 건다.
- 재부팅이 끼면: UART는 `expect … --from seq:<리셋 응답 seq> --within T`(generation 변경, 자동 재전송 없음), UDS는 `wait-for "link == connected"` 다음 읽기.
- 결과마다 `others_since_last`를 확인. user 조작이 있으면 `loom test note`로 기록하고 그 이벤트를 증거에서 뺀다.

## 3. 판정 — 증거 인용 의무
- 판정은 `loom test verdict … --evidence <REF>`. REF는 명령 결과 `data.evidence[]`에서 **복사**한다. seq·값·시각을 직접 만들지 않는다. 자기 송신은 기준점일 뿐 증거가 아니다.
- 검증기는 범위 전체를 다시 읽는다. 유리한 행만 인용해도 결과는 바뀌지 않는다.
- 증거가 없거나 검증이 거부되면 pass를 다시 시도하지 말고 `inconclusive` 또는 `fail`(위반 증거가 있을 때만).
- 창 안에 gap(`record.gap`/`source.gap`)이 있으면 "없음·시간 내" 주장은 `inconclusive`.
- 수치(시간·전류)는 검증기 계산값을 인용한다. 자신이 계산한 숫자를 판정 근거로 쓰지 않는다.

## 4. 실패·중단
- 계획의 `on_fail`을 따른다(기본: 그 TC 중단 → finally). 같은 단계 재시도는 계획에 `retry`가 있을 때 1회만.
- STOP(exit 6 `E_INTERRUPTED_BY_USER`)이면 즉시 멈추고 복구는 사용자에게 맡긴다(finally 자동 실행 안 됨).
- 장치 오류(exit 7)·`outcome_unknown` 쓰기는 다시 보내지 않는다. 상태를 읽어 확인하고 보고한다.

## 5. 사람에게 묻는 조건 (loom test ask)
TC 해석이 두 가지 이상 / R4 단계 / 계획 밖 쓰기가 필요 / fail이 장치 손상 가능성 / 같은 단계 2회 inconclusive /
사용자 개입이 판정에 영향 / 정책 거부(exit 4)로 계획 진행 불가. (env=vehicle은 계획 승인 화면의 배지로 충분 — 단계마다 묻지 않음, B4)

## 6. 보고
`loom test end` → `loom test report --html --junit`. 요약에는 판정·증거 REF·재현 정보만. 증거 없는 서술 금지.

## 금지
승인 우회(설정 파일 수정, 다른 client 사칭), 판정 override(사용자 전용), 증거 seq 조작, 계획 밖 R2+ 쓰기, 실패를 pass로 재분류.
```

## 5. 사용자 UI — 테스트 실행 화면 (→ `_xteam/ui-test.md`)

```
┌ Loom ─ env:sim ─ ● LIVE ─ STOP[Ctrl-] x] ───────────────────────────────────────── 승인: ap-17 R2 1/1 · R3 1/1 · 만료 08:12 ┐
│ TEST ts-0042  TC-1 콜드 부팅 스모크   ▶ RUNNING  S3/5   경과 00:41   실행자 agent:claude-1   [⏸ 일시정지] [⏹ 중단]        │
├──────────────────────┬────────────────────────────────────────────────────────┬────────────────────────────────────────┤
│ 단계                 │ S3 PowerMode == 2 (≤5 s)                   대기 2.1/5 s │ 증거 (실시간)                          │
│ ✓ S1 전원 사이클  R3 │ 명령  loom can sig wait-for "VehState.PowerMode == 2"   │ 15:02:11.402 can ch0 0x3A0 PM=1  dev   │
│ ✓ S2 login: 11.8s    │ 기대  can.signal VehState.PowerMode == 2                │ 15:02:12.010 can ch0 0x3A0 PM=1  dev   │
│ ▶ S3 PowerMode       │ 판정  — (대기 중)                                        │ 15:02:12.388 can ch0 0x3A0 PM=2  dev ◀ │
│ · S4 DTC 읽기        │ 검증  존재 – · 창 – · 술어 –                            │   술어 일치 후보  [이 증거로 확인]     │
│ · S5 DTC 삭제     R2 │                                                        │ ⚠ user tx 0x123 15:02:12.1 (판정 제외) │
│ finally: psu off     │ [⏭ 건너뛰기] [↻ 재시도] [✎ 판정 수정] [? 질문 2]        │ gap: 없음   record: OK                 │
├──────────────────────┴────────────────────────────────────────────────────────┴────────────────────────────────────────┤
│ 타임라인  psu ▇▇▇▇on──────  uart ··login:·  can ▁▁▁▁▂▂▃  uds ──     |S1|  S2  | S3▶                                       │
└───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

| 요소 | 내용 | 지연 목표 | 사용자 입력 |
|---|---|---|---|
| 상단 바 | 세션·TC·상태(PLANNED/WAITING_APPROVAL/RUNNING/PAUSED/STOPPED/DONE)·단계 n/N·실행자, env 배지, 승인 묶음 잔량·만료 | 상태 변경 → 화면 ≤ 200 ms | 일시정지/계속, 중단(=end --abort), 전역 STOP |
| 계획 검토 모달 | 원문 ↔ 해석 단계 대조(문장 하이라이트), 단계별 등급, 승인 묶음 요약, plan hash | — | **계획 승인 + 일괄 승인**(뷰어 전용, R4 단계는 따로 표시·제외), 단계 편집 요청(메모) |
| 단계 목록 | 상태 아이콘(✓✗?⊘⏭▶·), 판정, 등급 표시 | ≤ 200 ms | 단계 클릭 → 그 단계 띠로 타임라인 이동 |
| 현재 단계 | 명령, 기대 술어(사람 문장 + DSL), 대기 진행, 판정, 검증 3항목 결과 | 명령 발행 → ≤ 100 ms | 건너뛰기(사유), 재시도(계획에 retry 있을 때 또는 사용자 권한), **판정 수정**(사유 필수, origin=user), 질문 답하기 |
| 실시간 증거 | 이 단계 증거 종류로 필터된 도메인 이벤트(출처 색). 술어 일치 후보 강조. 사용자 이벤트는 "판정 제외" 표시 | 장치 → 화면은 도메인 목표(UART 100 / CAN 200 / UDS 100 ms), LA는 캡처 종료 후 | 후보 클릭 → 그 이벤트로 검증 요청(origin=user 판정 근거로 기록) |
| 질문 대화상자 | `loom test ask` 질문·선택지·남은 시간 | ≤ 200 ms | 답 선택/입력 |
| 복구 송신 키 | STOP 뒤 safe_state 동작 목록(A15) | — | 1키 실행(user) |
| 타임라인 | 모든 레인 위 단계 띠, gap 표시 | 도메인 목표 | 줌·커서, 띠 클릭 → 기록 구간 열기 |

## 6. 데이터 / 기록 포맷

### 6.1 TC 정본 형식 — Markdown + YAML 머리말 (`*.tc.md`)
사람이 쓰기 쉽고, 메일·위키·git diff가 되며, 표가 없어도 된다(자연어 본문만으로 유효).
```markdown
---
loom_tc: 1
id: TC-1
title: 콜드 부팅 스모크
env: sim            # sim | bench | vehicle
equipment: [psu:bench, uart:dut, can:ch0, uds:bcm]
preconditions: ["DUT 전원 OFF", "BCM DTC 임의 상태"]
tags: [smoke, boot]
---
## 설명
전원을 껐다 켠 뒤 30초 안에 …(자유 서술)

## 단계
| # | 동작 | 기대 결과 | 합격 기준 |
|---|---|---|---|
| 1 | 전원 OFF 2초 후 ON | — | — |
| 2 | 콘솔 관찰 | `login:` 표시 | 전원 ON 후 ≤ 30 s |
| 3 | CAN 관찰 | PowerMode = 2 | 부팅 후 ≤ 5 s |
```
- 머리말 키는 모두 선택이다. 없으면 에이전트가 계획에서 채우고 사용자가 확인한다.
- 표 열 이름은 한국어/영어 별칭을 허용한다(`Step/Action/Expected/Pass criteria`).

### 6.2 해석 계획 `*.plan.yaml` (`loom_tc_plan: 1`)
- `loom_scenario: 1`(D12) 단계 형식(`- <domain>.<verb>: {args}`)을 그대로 쓰는 **상위 집합**이다. 추가 키는 `id`, `tc_step`(원문 행 번호), `expect`, `on_fail`, `when`, `retry`, `measure`, `manual`이다.
- 원문 해시(`tc`)와 계획 해시(`plan_hash`)를 둔다. 승인은 plan hash에 묶인다(D6). 계획을 바꾸면 기존 승인은 무효이고 새 digest로 전체를 다시 승인한다(§3.1).

### 6.3 술어 DSL — **구조화 비교(YAML) + JSON Pointer 필드 선택** 채택
| 후보 | 장점 | 단점 | 판단 |
|---|---|---|---|
| **구조화 비교** `{field, op, value}` + `all/any/not`, `within_ms_of`, `count` | 정적 검증·사람용 문장 자동 생성·freeze 직렬화가 쉬움. 튜링 완전 아님. UDS `expect` 스타일과 같은 계열 | 표현력 제한 | **채택(M1)** |
| CEL | 표현력, 표준, 안전한 평가. 문법 오류에 행·열·caret | Rust `cel` 0.15.0(2026-10-06, MIT, pre-1.0·breaking 변경 있음 — 구 이름 `cel-interpreter` 0.10.0의 후속). 스파이크에서 같은 10사례 통과, 단 이벤트당 변환 비용 큼(한 쌍 80.7 µs vs 13.5 µs, 소형 단일 측정) | M3 이후 `expr:` 탈출구 후보 |
| JSONPath | 선택은 강력(`serde_json_path` 0.7.2, RFC 9535) | 시간·hold 술어 없음, **누락 필드를 오류 없이 false로 처리**(스파이크 확인) | 필드 선택은 JSON Pointer(serde_json `Value::pointer`) + 타입 검사로 대체 |
| jq 계열(`jaq` 3.1.2) | 변환·다중 결과 | 임의 재귀 시 시간·메모리 무제한 가능(공급자 명시) | 판정에 안 씀. 오프라인 변환 보조만 |
- 연산자: `== != < <= > >= in regex contains between`, 바이트열 `bytes_eq`(hex), 시간 `within_ms_of{step, event, max}`, 집계 `count/min/max` over evidence set.
- 정규식 엔진은 Rust `regex`(백트래킹 없음, ReDoS 안전) — 같은 문법을 UART `expect`와 공유한다(`_xteam/test-domains.md`로 UART 팀에 요청).
- 판정 정본 = **작은 비교 AST + Rust 시간/coverage 연산자**(결정 T3). 사람용 문장은 AST에서 자동 생성해 계획 승인 화면에 보인다.

### 6.4 판정 규칙
| 규칙 | 내용 |
|---|---|
| R-1 | `pass` = 계획 `expect`의 모든 항이 검증기에서 참(존재·창·술어). 없으면 거부 |
| R-2 | `fail` = 위반을 보이는 증거(술어 거짓인 이벤트) 또는 **대기 timeout 이벤트 + 창 전체 기록 건강**(gap 없음) |
| R-3 | `inconclusive` = 증거 부족, gap, 사용자 조작 혼입으로 구분 불가, 검증기 평가 불가 |
| R-4 | `blocked` = 전제 미충족·장치 열기 실패·정책 거부로 실행 불가 / `skipped` = 계획 분기(`when`) 또는 사용자 건너뛰기(사유 기록) |
| R-5 | 사람 판정 수정: `test.verdict{origin:user, overrides:<이전>, reason}`. 에이전트는 수정 불가. 보고서에 둘 다 표시 |
| R-6 | TC 판정 = 필수 단계 중 최악(`fail > blocked > inconclusive > pass`). 계획상 분기(`on_fail: goto`)로 처리된 fail은 분기 결과로 대체하되 보고서에 원 fail을 남김 |
| R-7 | `manual` 단계는 사용자만 판정(pass/fail). 에이전트는 질문만 |
| R-8 | 증거가 전부 에이전트 자신의 tx이면 pass 거부(`agent_tx_only`). 자극 전용 단계는 `done`(쓰기 상태 ≥ confirmed) |
| R-9 | 집계·종료 코드는 §3.1(필수 단계 미완료 = incomplete → inconclusive, 사용자 필수 skip = 최대 inconclusive) |

### 6.5 회귀 고정 `freeze`
- 출력: `loom_scenario: 1` + `kind: test` + 원 세션·계획 해시.
- **고정되는 것**: 실행한 명령과 해석된 인자(별칭→실 ID), 단계 순서·분기, `expect` 술어, timeout, 일괄 승인 요구 등급 요약, finally.
- **고정 안 되는 것**:
  - 자연어 판단(`manual` 단계로 남아 재실행 때 사람이 판정).
  - 에이전트가 계획 밖에서 한 탐색 조회(제외하고 목록으로 경고).
  - 실측 타이밍(재실행은 같은 timeout 상한으로 기다리고 시각은 새로 측정).
  - 장치가 준 값(`save:` 변수로 재획득).
  - 사용자 개입(재현하지 않음, 원본 보고서에만).
- **재바인딩**: 과거 seq·승인 토큰·시각은 복사하지 않는다. 새 세션의 단계 구간으로 다시 바인딩하고, 승인·감사·STOP 규칙은 새 실행에 그대로 적용한다. 이것은 **명령·판정 절차의 고정**이지 DUT 응답의 비트 단위 재현이 아니다(선례: Robot Framework 키워드 고정, Playwright codegen — `r1-sol.md` A4).
- 실행: `loom run regress/X.scenario.yaml --test` → 같은 세션 엔진·검증기·보고서. 에이전트 없이 CI·야간 회귀에 쓴다.

### 6.6 보고서
- **JSON(정본)**: 세션, TC/계획 해시, 단계별 판정·증거 REF·검증 결과·사용자 수정·메모, 재현 정보.
- **HTML**: `loomd`가 `/reports/<sess>`로 제공(UI에서 열람)하고 정적 내보내기도 한다. 증거 REF는 `loom://records/<session>?seq=a..b` 링크로 UI 기록 탐색기의 그 구간을 연다.
- **JUnit XML**(결정 T5, CORE가 quick-junit으로 생성하고 매핑 규칙은 TEST 소유): 방언이 여럿(Ant legacy·Jenkins·Surefire XSD)이므로 공통 최소 요소만 쓴다 — suite/case name·classname·time(초), counts, `failure`/`error`/`skipped`, 짧은 system-out. testsuite = TC, testcase = 단계. 기대 불일치 = `failure`, 실행·인프라 오류(blocked) = `error`, **inconclusive·사용자 중단 = `skipped` + `message="inconclusive: <사유>"`**(기본). CI가 inconclusive를 녹색으로 넘기면 안 되는 곳은 `--junit-inconclusive=error`. 원래 판정은 HTML·JSON 정본에 보존. GitLab은 properties 등 일부 요소를 무시하므로 증거 REF는 HTML 링크가 정본이다. 세 CI 실제 업로드는 미확인(M3).
- **재현 정보**: Loom 버전·커밋, OS, env, 프로파일(`loom.toml`·policy) 해시, 장치 신원(포트 VID/PID, CAN 인터페이스 모델·드라이버 버전, UDS F189/F195 등 계획에 `collect_versions` 단계가 있을 때의 ECU SW 버전, Saleae 모델·Logic 2 버전), 시뮬레이터 seed.

## 7. 라이브러리 선택과 근거 (Rust, C1 — `r1-sol.md` 확인, crates.io 2026-11-10 기준)
| 용도 | 선택 | 버전 · 릴리스 · 라이선스 | 근거·주의 |
|---|---|---|---|
| 엑셀·ODS 가져오기 | `calamine` | 0.38.0 · 2026-11-02 · MIT | 순수 Rust, Office 불필요. 읽기 전용(재계산 안 함), 입력 크기·행 수 상한(C2) |
| CSV | `csv` | 1.4.0 · 2025-10-17 · Unlicense/MIT | 구분자·BOM·인코딩 명시 |
| Gherkin(선택 입력) | `gherkin` 파서만 | 0.16.0 · 2026-04-23 · MIT/Apache-2.0 | `.feature` → `.tc.md` 변환. `cucumber` 실행 프레임워크는 **쓰지 않음**(TC마다 Rust step 코드를 짜게 되어 C7 목적과 어긋남) |
| 술어 평가 | 자체 비교 AST + `regex` + serde_json `pointer` | — | 스파이크 10사례 통과, 오류 메시지 직접 통제. CEL(`cel` 0.15.0)은 M3 탈출구 후보 |
| JUnit XML | `quick-junit` | 0.8.0 · 2026-09-10 · Apache-2.0/MIT | nextest 사용, XML 불가 문자 제거 |
| HTML 보고서 | `askama`(고정 템플릿) | 0.16.1 · 2026-09-04 · MIT/Apache-2.0 | 빌드 시 타입 검사·escape. 사용자 템플릿 요구가 생기면 `minijinja` 3.1.0(Apache-2.0) |
| Markdown 표 | `pulldown-cmark`(GFM tables) | 미확인 | M0에서 확인 |
| YAML | 미정 | 미확인 | `serde_yaml` 유지보수 중단 여부 확인 후 M0에서 결정(CORE와 같은 crate 사용) |
| 외부 TC 도구 | Polarion·codebeamer(XLSX round-trip·ReqIF), TestRail(XML/CSV/Excel), Xray(CSV·Cucumber feature) | 상용 | **M2는 XLSX/CSV 매핑만**. ReqIF·ASAM ATX(`.atxml`)는 샘플이 오면 검토. ASAM XIL은 벤치 API 표준이지 TC 내보내기 형식이 아님 |
Python 사용 없음(C1). 스파이크 보조 스크립트만 Python이고 판정 코드는 Rust였다.

## 8. HW / OS 지원
TEST 기능은 소프트웨어다. `loomd`가 도는 곳이면 동작한다: 실HW Windows x64·Linux x64(B1), 개발·CI macOS(sim). 보고서 HTML은 브라우저만 있으면 된다. 장치 지원 범위는 각 도메인 계획을 따른다. TC의 `equipment`가 이 호스트에 없으면 `blocked`다.

## 9. 시뮬레이션 & 테스트 전략
- **M1 E2E**: TC-1을 `env=sim`에서 실행한다. 장치는 SimPsu, UART pty `bootshell`, CAN SimNode(PowerMode 신호), UDS 가상 DoIP ECU(DTC 1개 주입)다. 실행자는 (a) 스크립트화한 가짜 에이전트(CI용 결정적 명령열), (b) 실제 Claude Code 수동 시연 둘 다로 한다.
- **검증기 단위 시험**: 표 기반으로 존재·창·술어·gap 조합 40+ 사례를 둔다. "없는 seq", "창 밖", "술어 거짓", "gap 중 absence 주장", "user 이벤트 인용" 거부를 반드시 넣는다.
- **환각 방지 회귀**: 가짜 에이전트가 지어낸 seq·값으로 pass를 시도하면 100% 거부되는지 본다.
- **freeze 왕복**: 세션 → freeze → `loom run --test` 결과가 같은 판정이어야 한다(manual 제외).
- **가져오기**: 샘플 xlsx 3종(열 순서·병합 셀·한국어 열 이름) → `.tc.md` 골든 비교.
- 3-OS CI, 모두 sim.
- **C2 soak 기여**: 24 h soak(CAN 고부하 + UART flood + UDS 폴링)에 **TC-1 반복 실행 세션**을 함께 돌린다(10분마다 1회, 144 세션). 관문: 세션·판정 저장소와 검증기의 RSS 증가 없음(세션 종료 후 메모리 반환), 검증 1회 p95 ≤ 200 ms 유지, 보고서 생성이 저널 쓰기를 막지 않음.
- **테스트 쪽 큐·버퍼 상한(C2)**: 세션 동시 1개(기본, 설정 시 ≤ 4), 단계 수 ≤ 500/TC, 증거 REF ≤ 64/판정, 실시간 증거 패널 구독은 단계 필터로 제한(UI 쪽 링 2k 행, 넘치면 오래된 행 버림 + `display.gap`), 보고서 HTML ≤ 20 MB(넘으면 증거 표를 링크로만).

## 10. 안전 / 가드레일
- **일괄 사전 승인**: `test start` 시 계획의 R2·R3 단계를 하나의 뷰어 승인으로 묶는다. 바인딩은 plan hash·단계별 동작·대상·**횟수 상한**(예: `psu.out on ×5`)·env·프로파일 해시다. 사용 기한은 A9 확장이 필요하다: "세션 시작 10분 내 개시, 세션 중 유효, `max_test_duration` 기본 1 h"(§15 요청).
  - **R4는 묶지 않는다** — 그 단계 시작 때 매번 뷰어 승인(once).
  - `default_blocked`/`immutable_deny` 단계가 있으면 계획을 거부한다.
- **승인 범위 밖 쓰기**: 스킬은 하지 않는다(`plan amend` → 재승인). 코어가 받으면 개별 승인 + 보고서 "계획 밖 쓰기" 표시, 그런 세션의 freeze는 `--allow-unplanned` + `needs_review`(§3.1).
- **env=vehicle**(B4): 같은 등급·경로를 쓰고 추가 차단·추가 질문은 없다. 계획 검토 모달·테스트 화면·보고서에 `env=vehicle` 배지를 상시 표시한다.
- **실패 시 행동** — `on_fail`(단계 기본값은 `stop_tc`):
  - `continue` / `goto <id>` / `stop_tc`(TC 중단 → finally) / `stop_session`.
  - 사용자 STOP(A3)은 모든 쓰기·잡을 latch한다. **finally를 자동 실행하지 않고** "복구 송신" 1키를 띄운다(A15).
  - 복구 송신은 safe_state 동작만 가능하고 인터록·deny를 통과해야 한다.
- **판정 무결성**:
  - 판정 수정은 뷰어 자격 연결만 할 수 있다(origin=user).
  - 판정·증거·수정은 감사 로그(해시 체인)에 남는다.
  - 기록·감사 불능(A12)이면 새 R1+ 쓰기와 **pass 판정**이 중단된다(증거 저장이 안 되므로).
- **사용자 개입 반영**: 일시정지 중 user 조작은 `test.note{origin:user, kind:operator_action}`으로 자동 기록된다. 그 시간 창의 판정에는 경고 배지가 붙는다.

## 11. 마일스톤

| M | 내용 | 수용 기준 |
|---|---|---|
| **M0** 계약 | `_xteam/core-test.md`(세션·마커·검증기·판정 API), `.tc.md`/`.plan.yaml`/술어 DSL 명세 v0, 증거 REF 문법, 도메인 `data.evidence[]` 요청 합의 | CORE·UI·3팀이 서명. 스키마 fixture 5개(TC-1~3 포함) 파싱 green |
| **M1** 걷는 뼈대 (UI 팀 M1 셸·CORE 세션 엔진 의존) | sim 위 **TC-1을 웹 UI 테스트 화면으로 실행**: 계획 검토·일괄 승인 → 단계 진행·실시간 증거 → 사용자 일시정지+직접 송신 → 에이전트가 `others_since_last`로 인지·메모 → 증거 검증 pass/fail → 사용자 판정 수정 1회 → HTML·JUnit 보고서 | ① 지어낸 seq로 pass 시도 거부 ② 단계 띠가 모든 레인에 표시 ③ 명령 → 화면 ≤ 200 ms(render ack) ④ 두 번째 브라우저 늦은 합류 시 같은 상태 ⑤ STOP 시 finally 미실행 + 복구 키 ⑥ 3-OS CI(가짜 에이전트) green |
| **M2** 실무 입력 | 엑셀 가져오기, `test ask`, `freeze` → `loom run --test`, TC-2(LA·DLT)·TC-3(repeat·measure) | freeze 왕복 동일 판정, xlsx 골든 3종, LA 사후 증거 창 검사 |
| **M3** 실HW·확장 | Windows/Linux 실HW에서 TC-1 변형(PCAN·실 UART), JUnit CI 연동, CEL 탈출구 검토, 다중 TC 묶음(suite) | 실HW 체크리스트 두 OS, 24 h 회귀 야간 실행 |

## 12. 작업 분해 (병렬 단위 · 담당 · 의존)
| ID | 작업 | 담당 | 의존 |
|---|---|---|---|
| W1 | `.tc.md`·`.plan.yaml` 스키마 명세 + fixture | Opus | — |
| W2 | 술어 DSL 명세 + 사람용 문장 생성 규칙 | Opus | W1 |
| W3 | 증거 REF 문법 + 도메인별 `evidence[]` 요구 목록(uart/can/uds/dlt/la/psu) | Opus | W1 |
| W4 | `_xteam/core-test.md` 초안(세션·마커·검증기·판정·일괄 승인 API) | Opus + CORE 리드 | W1–W3 |
| W5 | 검증기 사례표 40+ (단위 시험 정본) | Sol | W2, W3 |
| W6 | Rust crate 조사(YAML, Markdown 표, calamine, JUnit, 템플릿, CEL) | Sol | — |
| W7 | `loom-test` SKILL.md + references(해석 예시, 판정 규칙표, 질문 조건) | Opus | W1–W3 |
| W8 | UI 테스트 화면 명세 → `_xteam/ui-test.md` | Opus | W4 |
| W9 | 보고서 템플릿(HTML/JUnit/JSON) 명세 + 샘플 | Sol | W4 |
| W10 | sim TC-1 환경 정의(SimPsu·bootshell·SimNode·가상 ECU DTC 주입) + 가짜 에이전트 명령열 | Sol | 3팀 M1 sim |
| W11 | freeze 변환 규칙(고정/비고정 표, 경고) | Opus | W1, D12 스키마 |
| W12 | 엑셀 매핑 형식 + 골든 샘플 3종 | Sol | W1, W6 |
| 구현(CORE) | 세션 엔진·검증기·판정 저장·보고서 API (CORE crate) | CORE | W4 |
| 구현(UI) | 테스트 화면 | UI | W8 |

## 13. 리스크
| 리스크 | 대응 |
|---|---|
| 에이전트가 TC를 잘못 해석(그럴듯한 오해) | 실행 전 계획 확인 모달(원문 문장 ↔ 단계 대조), `plan check` 정적 검증, 모호하면 `ask` |
| 증거는 맞는데 술어가 TC 의도와 다름 | 술어를 사람 문장으로 렌더링해 승인 화면에 표시. 사용자 판정 수정 경로 |
| 도메인 결과에 seq 핸들이 없음 | M0에서 `data.evidence[]`를 공통 계약으로(§15). 없으면 `check --auto`로 검증기가 찾음 |
| LA처럼 사후 생성 증거의 시간 창 판단 | 장비 시각 범위 + 정렬 오차 라벨, 창 판정은 `quality: host-approx` 표시 |
| 일괄 승인이 너무 넓음(안전) | 동작·대상·횟수·env·plan hash 바인딩, R4 제외, 계획 밖 쓰기는 개별 승인 |
| 긴 TC에서 승인 기한(A9) 충돌 | `max_test_duration` 별도 상한 요청 |
| 엑셀 TC 형식의 다양성 | 매핑 파일 + 에이전트 보조, 결정적 변환 결과를 사람 확인 |
| freeze 재실행 판정이 원본과 다름(타이밍) | timeout 상한만 고정, 차이는 보고서 비교 표에 표시 |

## 14. 사용자에게 물어볼 질문 (기본값)
| # | 질문 | 기본값 |
|---|---|---|
| Q1 | 사내 TC 엑셀의 열 구성·예시 파일을 받을 수 있나? | 일반 열(번호·절차·기대 결과·판정)로 매핑 샘플 작성 |
| Q2 | 해석 계획을 **매번** 사람이 확인? 같은 TC 재실행 시 생략? | 첫 실행·계획 변경 시만 확인, plan hash 같으면 일괄 승인만 |
| Q3 | 보고서를 소비하는 CI(Jenkins/GitLab 등)가 있나? | JUnit XML만 제공 |
| Q4 | TC 언어(한국어 위주?)와 판정 용어 | 한국어 UI, 판정 값은 영어 키 |
| Q5 | 사람 눈 판정(HMI 화면 등)이 많은가? | `manual` 단계로 처리 |
| Q6 | TC 파일 저장 위치(저장소 `tc/` vs 별도) | 프로젝트 `tc/` |

## 15. 공통 계약 변경 요청 / 타팀 의존성
| ID | 대상 | 요청 |
|---|---|---|
| TC-C1 | CORE | 테스트 세션 객체·`test.*` 이벤트(`session.start/end`, `step.begin/end`, `verdict`, `note`, `ask`), client↔세션 태깅으로 D3에 선택 필드 `test_session`, `test_step` 추가 |
| TC-C2 | CORE | 증거 검증기(저널 seq 조회·창·술어·gap 검사)와 판정 저장. pass 거부 종료 코드 4 `E_EVIDENCE_MISSING/MISMATCH/OUT_OF_WINDOW`를 D5 표에 추가 |
| TC-C3 | CORE | 일괄 승인: plan hash·동작·대상·횟수 바인딩, A9 확장(`max_test_duration`), R4 제외 |
| TC-C4 | CORE · 러너 | `loom_tc_plan: 1` ⊃ `loom_scenario: 1`. `expect` 술어 DSL을 러너와 공유, `loom run --test`가 같은 세션 엔진 사용 |
| TC-C5 | UART·CAN·UDS·탐색 (`_xteam/test-domains.md`) | 대기·조회 결과와 **쓰기 결과**(시간 기준점용: `psu.out.on`, `uart.tx`, `can.tx`, `uds.request`)의 `--json` `data.evidence[]`에 REF(`<domain>:<channel>@seq:…`)와 kind 포함. 예: `uart expect` 매치 seq, `can sig wait-for` 프레임 seq+값, `uds dtc list` 요청/응답 seq(`uds.tx`/`uds.rx`), `la wait` 캡처·행, `dlt wait-for` 로그 seq |
| TC-C6 | UI | `_xteam/ui-test.md`: §5 화면, 타임라인 단계 띠, 계획 검토 모달, 판정 수정(뷰어 전용), 기록 링크 `loom://records/...` |
| TC-C7 | UART | `expect` 정규식 문법을 술어 `regex`와 공유(Rust `regex`) |
| TC-C8 | CORE | 기록 불능(A12) 시 pass 판정도 중단 |
| TC-C9 | CORE | 검증기 평가 예산(식 깊이·대상 이벤트 수·시간)과 `inconclusive{cause:"eval_budget"}`, 테스트 쪽 큐·저장 상한(§9)을 C2 상한 표에 포함 |
| TC-C11 | CORE·메인 | `loom test end`/`loom run --test` 종료 코드: 11 `E_TEST_FAILED`(D5 신규), 8 `E_TEST_INCONCLUSIVE`, 6 `E_SESSION_ACTIVE`·`E_SESSION_PAUSED` |
| TC-C12 | UART·UDS·CAN·탐색 | `plan_summary{intents[]}`(대상·서비스·값 범위·횟수) — `plan check`가 일괄 승인 범위를 계산할 때 호출. 도메인 술어 이름은 CORE O3 목록 |
| TC-C10 | UI·CORE | 보고서 HTML 제공 경로 `/reports/<sess>`와 기록 링크 `loom://records/<session>?seq=a..b`가 UI 기록 탐색기를 연다 |

## 16. 결정 기록

| # | 쟁점 | Opus(r1-opus) | Sol(r1-sol) | **결정** | 이유 |
|---|---|---|---|---|---|
| T0 | **v2.0 방향 전환**(마스터 C1–C8) | — | — | EXPLORE 팀이 TEST 기획을 맡는다. 코어는 Rust, 사용자 화면은 웹 UI 앱 1급, TC마다 코드 없이 에이전트가 스킬로 실행, **증거 기반 판정** | 사용자 지시 C7·C8 |
| T1 | TC 정본 형식 | Markdown + YAML 머리말 `.tc.md`, 자연어만으로도 유효 | 엑셀은 calamine+csv, Gherkin은 선택 입력 | **`.tc.md` 정본**. 엑셀·CSV·Gherkin은 가져오기로 `.tc.md`를 만든다 | 사람이 쓰기 쉽고 diff·리뷰 가능. 외부 도구 형식은 다양해 매핑으로 흡수 |
| T2 | 해석 계획과 러너 | `loom_tc_plan: 1` ⊃ `loom_scenario: 1`, 엔진 하나 | 기록 고정 = 동사·인자·술어, 새 세션 재바인딩 | **계획 = 시나리오 상위 집합, 세션 엔진·검증기·보고서 하나**. 실행자만 에이전트 또는 러너 | 결정적 회귀와 대화형 실행이 같은 판정 규칙을 쓰게 함 |
| T3 | 판정 술어 | 구조화 비교 + JSON Pointer | 작은 비교 AST + Rust 시간·coverage 연산자, CEL 고급 후보, JSONPath는 누락 필드를 조용히 false | **비교 AST + 시간·hold·count 연산자**, 누락·타입 오류는 오류(→ inconclusive), 평가 예산, CEL은 M3 탈출구 | 정적 검증·사람 문장 생성·freeze 직렬화, 스파이크로 두 엔진 동일 결과 확인 |
| T4 | 증거 참조 | REF 문법, 결과 `data.evidence[]`에서 복사 | seq/path/expected/actual 오류 | **REF는 도메인이 결과에 넣어 주고 에이전트는 복사만**. 검증기가 존재·창·술어·gap 확인 | 환각 방지의 핵심: 에이전트가 seq·값을 만들 여지를 없앰 |
| T5 | JUnit 매핑 | inconclusive = `<error>` | 방언 여럿, inconclusive = skipped + 사유 권고 | **skipped + `inconclusive:` 사유(기본), `--junit-inconclusive=error` 선택**. 정본은 JSON·HTML | CI 소비자 호환 + 엄격 CI 옵션 |
| T6 | 일괄 사전 승인 | R2·R3 묶음, R4 제외, 횟수 상한 | — | **채택**. plan hash·동작·대상·횟수·env·프로파일 해시 바인딩, `max_test_duration`(A9 확장 요청) | 긴 TC에서 승인 피로 방지 + 범위 고정 |
| T7 | CORE/TEST 경계 | 세션·검증기·저장 = CORE, 형식·스킬·규칙·템플릿 = TEST | — | **채택**, `_xteam/core-test.md`로 제안 | CORE는 Rust 코어 소유(C1) |
| T8 | vehicle에서 단계별 질문 | R2+ 단계 전 확인 | — | **질문하지 않음**, 배지만 | B4: 실차 송신을 막을 이유 없음 |
| T9 | 보고서 기술 | minijinja 또는 askama | quick-junit + askama | **quick-junit + askama 고정 템플릿** | 빌드 시 타입 검사, 런타임 템플릿은 요구 생기면 |
| T10 | 증거 검증 범위 (R4 P1-1·P1-2) | 인용한 seq에 술어 평가 | 유리한 샘플만 골라 통과 가능 | **인용 = 범위+필터+술어, 검증기가 필터된 범위 전체를 재조회**, 관측 증거 origins 기본 device, request_id 짝, 같은 대상 사용자 쓰기 = uncertain, agent tx만 = 거부(R-8) | 환각 방지의 구멍 제거. CORE O3–O5와 같은 모델 |
| T11 | 시간 창 (P1-4·P1-5) | 단계 begin–end 창 | 대기 호출 시점에 판정이 좌우됨 | **assertion 창 = anchor + within_ms, 저널 기준**, 재시도마다 attempt 창, LA는 capture→host 오차 구간이 경계를 가로지르면 inconclusive | TC 문장("부팅 후 5 s")을 그대로 판정 |
| T12 | 승인·계획 변경 (P1-6·P1-7·P2-6) | 추가 단계만 재승인, 에이전트 위험 요약 | 기존 단계 변경·횟수 누락 | **위험 요약은 Loom classify/plan_summary 산출(finally·repeat·retry 포함), 계획 변경 = 전체 재승인, 계획 밖 쓰기 = amend 우선, freeze는 `--allow-unplanned`+needs_review** | 일괄 승인의 범위를 Loom이 계산하게 함 |
| T13 | 집계·종료 코드 (P1-8·P2-8) | 최악 판정 | 필수 미완료·임의 skip, CI 종료 코드 없음 | **자극 전용 = done, 필수 미완료 = incomplete, 사용자 필수 skip = 최대 inconclusive. 종료 코드 0/11(신규)/8/6** | CI 게이트를 판정과 독립으로 |
| T14 | v2.0 재개 후 C4.1 | — | — | 영향 없음(TEST는 장치 경로 무관). TC 예시의 DoIP 장비는 PC NIC + 미디어 컨버터 기준 | 메인 공지 05 |

