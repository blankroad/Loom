# TEST 기획 R1 초안 — 통합 테스트 (explore-opus)

작성: explore-opus, 2026-11-10. 기준: `02-core-principle.md` → 마스터 §2.3 C1–C8(특히 C7·C8) → B → A → D, `test/brief.md`.
상태: 계획 초안. 구현 코드 없음. 불확실한 사실은 "미확인"이다(Sol이 확인 중). `core/plan.md`, `ui/plan.md`는 아직 없어서 브리프만 보고 경계를 제안한다.

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
  - id: S1   # 전원 사이클
    do: [{psu.out: {name: bench, state: "off"}}, {sleep: 2s}, {psu.out: {name: bench, state: "on"}}]
  - id: S2   # 부팅 프롬프트
    do: [{uart.expect: {port: dut, pattern: "login:", timeout: 30s}}]
    expect: {evidence: uart.match, where: {within_ms_of: {step: S1, event: psu.out.on, max: 30000}}}
  - id: S3   # PowerMode
    do: [{can.sig.wait-for: {channel: ch0, cond: "VehState.PowerMode == 2", timeout: 5s}}]
    expect: {evidence: can.signal, where: {name: VehState.PowerMode, op: "==", value: 2}}
  - id: S4   # DTC 읽기
    do: [{uds.dtc.list: {target: bcm, mask: 0x09}}]
    expect: {evidence: uds.response, where: {sid: 0x59, dtc_count: 0}}
    on_fail: {goto: S5}                  # 있으면 삭제 경로
  - id: S5   # 삭제 후 재확인 (R2)
    when: {step: S4, verdict: fail}
    do: [{uds.dtc.clear: {target: bcm}}, {uds.dtc.list: {target: bcm, mask: 0x09}}]
    expect: {evidence: uds.response, where: {sid: 0x59, dtc_count: 0}}
finally: [{psu.out: {name: bench, state: "off"}}]     # safe_state 동작
risk_summary: {R2: [S5 uds.dtc.clear ×1], R3: [S1 psu.out on ×1], R4: []}
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
- S4에서 가상 ECU가 DTC 1개(`B1234-00`)를 반환한다. 판정은 fail(증거 `uds:bcm@seq:18402`, `dtc_count=1`)이고 S5로 간다.
- `dtc.clear`는 일괄 승인 범위 안이다. 재조회 결과 `dtc_count=0`이므로 S5는 pass다.
- **사용자 개입 시연**: S3 대기 중 사용자가 UI에서 **일시정지**를 누르고 CAN IG 패널에서 프레임 1개를 직접 보낸 뒤 계속을 누른다. 에이전트의 다음 결과에 `others_since_last{count:1, origins:[user]}`가 붙는다. 스킬 규칙에 따라 에이전트는 S3 판정에 사용자 프레임이 섞였는지 확인하고 판정 메모에 적는다. 사용자 프레임 seq는 증거에서 제외된다.
- TC 판정: S4 fail은 계획상 분기(`on_fail: goto`)이므로 TC 결과는 **pass**다(§6.4 규칙 R-6). 보고서에는 S4 fail 사실이 그대로 남는다.

### TC-2 DLT 레벨 변경 + LA로 UART 물리 확인 (DLT + LA(Saleae) + UART)
- 원문: "CAM 컨텍스트를 debug로 올리고 콘솔에 `cam stat`을 보내면, DLT에 debug 로그가 2초 안에 나와야 하고, UART TX 선에 실제 `cam stat\r`이 115200으로 나가야 한다."
- 계획:
  - S1 `dlt.level`(R1) → expect `dlt.ctrl` 상태 `effective_verified`.
  - S2 `la.capture`(trigger uart_tx:fall, 2 s, async 디코드) + `uart.send`.
  - S3 `la.wait` → expect `la.decode` 행의 바이트열 = `63 61 6d 20 73 74 61 74 0d`, 그리고 `uart.tx` seq와 정렬 매치.
  - S4 `dlt.wait-for --level debug --ctx CAM --timeout 2s` → expect `dlt.log` 1건 이상.
- 증거 예: `dlt:ecu1@seq:9120`(ctrl ack), `la:sal@capture:7#row:0..8`(디코드 행), `uart:dut@seq:18501`(tx), `dlt:ecu1@seq:9177`(debug 로그).
- 판정 주의: LA는 캡처 후 표시라 증거가 캡처 종료 뒤에 생긴다. 시간 창 검사는 **캡처의 장비 시각 범위**로 한다(정렬 오차 라벨 포함, `quality: host-approx`).

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
- **증거 검증기 절차**: (1) 참조한 seq가 이 세션 저널에 있다. (2) 그 이벤트 시각이 단계 창(begin–end, 허용 오차 기본 0) 안이다. 단 LA처럼 사후 생성 증거는 장비 시각 범위로 본다. (3) 계획의 `expect` 술어를 그 이벤트에 평가해 참이다. (4) "없음/시간 내" 주장은 창 안에 `record.gap`·`source.gap`이 없어야 한다. 하나라도 거짓이면 pass를 거부한다.

## 4. CLI / API 명세 초안 (+ `loom-test` 스킬)

D5 봉투·종료 코드·커서를 따른다. 모든 결과에 `others_since_last`가 붙는다.

| 명령 | 역할 | 비고 / 종료 코드 |
|---|---|---|
| `loom test import <xlsx\|csv> --map map.yaml [--sheet S] -o tc/` | 표 → `.tc.md` 여러 개 | 결정적 변환. 매핑 없는 열은 `notes`로 |
| `loom test plan check <plan.yaml>` | 정적 검증: 명령·인자 스키마, 등급 요약, timeout 필수, 술어 파싱, 증거 종류 존재 | 2 스키마 오류, 4 `default_blocked`/`immutable_deny` 포함 |
| `loom test plan show <plan.yaml>` | 사람용 요약(단계·등급·승인 묶음) | |
| `loom test start <tc.md> --plan <plan.yaml> [--wait-approval 300s]` | 계획 검토 요청 + 일괄 승인 대기 → 세션 시작 | 5 `E_APPROVAL_REQUIRED`, 3 승인 대기 timeout, 6 다른 세션 진행 중 |
| `loom test step <sess> <id> --begin \| --end` | 단계 마커 | 4 순서 위반(begin 없이 end) |
| `loom test check <sess> <id> [--evidence REF…] [--auto]` | 검증만(판정 저장 안 함). `--auto`는 창 안에서 술어를 만족하는 이벤트를 검증기가 찾아 제안 | 0 + 결과 표 |
| `loom test verdict <sess> <id> pass\|fail\|inconclusive\|blocked\|skipped --evidence REF… [--note T]` | 판정 기록 | **4 `E_EVIDENCE_MISSING`/`E_EVIDENCE_MISMATCH`/`E_OUT_OF_WINDOW`**(pass 거부) |
| `loom test evidence <sess> <id> --add REF [--label L]` | 보조 증거(첨부, 판정 근거 아님) | |
| `loom test note <sess> [<id>] TEXT` | 메모(에이전트 관찰, 사용자 개입 설명) | |
| `loom test ask <sess> --question T [--options a,b] --timeout 600s` | 사람에게 질문 → UI 대화상자, 답은 origin=user | 3 timeout |
| `loom test status <sess>` / `wait <sess> --timeout` | 진행 상태 / 사용자 개입·일시정지 해제 대기 | 6 `E_INTERRUPTED_BY_USER`(STOP) |
| `loom test end <sess> [--abort]` | 종료 → finally(A15) → TC 판정 계산 | |
| `loom test report <sess> --html --junit --json -o out/` | 보고서 | 8 기록 gap 포함 시 부분 표시 |
| `loom test freeze <sess> -o regress/X.scenario.yaml` | 결정적 시나리오로 고정(§6.5) | 비결정 단계 목록을 경고로 |

판정 거부 예:
```
$ loom test verdict ts-0042 S3 pass --evidence can:ch0@seq:17990
{"ok":false,"cmd":"test.verdict","error":{"code":"E_OUT_OF_WINDOW","message":"seq:17990 at t=-1.2s is before step S3 begin","hint":"cite evidence produced inside S3, or run 'loom test check ts-0042 S3 --auto'","retryable":true}}   # exit 4
```

**증거 참조 문법**(REF): `<domain>:<channel>@seq:<a>[..<b>][#<pointer>]`. 사후 생성물은 `la:<dev>@capture:<n>#row:<a>..<b>`. `<pointer>`는 JSON Pointer(RFC 6901, 예: `#/payload/value`)다. 도메인은 결과 `data.evidence[]`에 이 REF를 그대로 넣어 준다(§15 요청) — 에이전트는 seq를 지어내지 않고 **복사**한다.

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
- 단계마다 `loom test step <s> <id> --begin` → 명령 → `--end`. 계획에 없는 쓰기는 하지 않는다(필요하면 계획 변경 → 재승인).
- 대기는 항상 경계 있는 명령(`expect`, `wait-for`, `la wait`)과 계획의 timeout. `sleep`으로 결과를 기다리지 않는다.
- 결과마다 `others_since_last`를 확인. user 조작이 있으면 `loom test note`로 기록하고 그 이벤트를 증거에서 뺀다.

## 3. 판정 — 증거 인용 의무
- 판정은 `loom test verdict … --evidence <REF>`. REF는 명령 결과 `data.evidence[]`에서 **복사**한다. seq·값·시각을 직접 만들지 않는다.
- 증거가 없거나 검증이 거부되면 pass를 다시 시도하지 말고 `inconclusive` 또는 `fail`(위반 증거가 있을 때만).
- 창 안에 gap(`record.gap`/`source.gap`)이 있으면 "없음·시간 내" 주장은 `inconclusive`.
- 수치(시간·전류)는 검증기 계산값을 인용한다. 자신이 계산한 숫자를 판정 근거로 쓰지 않는다.

## 4. 실패·중단
- 계획의 `on_fail`을 따른다(기본: 그 TC 중단 → finally). 같은 단계 재시도는 계획에 `retry`가 있을 때 1회만.
- STOP(exit 6 `E_INTERRUPTED_BY_USER`)이면 즉시 멈추고 복구는 사용자에게 맡긴다(finally 자동 실행 안 됨).
- 장치 오류(exit 7)·`outcome_unknown` 쓰기는 다시 보내지 않는다. 상태를 읽어 확인하고 보고한다.

## 5. 사람에게 묻는 조건 (loom test ask)
TC 해석이 두 가지 이상 / R4 단계 / 계획 밖 쓰기가 필요 / fail이 장치 손상 가능성 / 같은 단계 2회 inconclusive /
사용자 개입이 판정에 영향 / 정책 거부(exit 4)로 계획 진행 불가 / env=vehicle에서 R2+ 단계 시작 전.

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
- 원문 해시(`tc`)와 계획 해시(`plan_hash`)를 둔다. 승인은 plan hash에 묶인다(D6). 계획을 바꾸면 추가된 R2+ 단계만 재승인한다.

### 6.3 술어 DSL — **구조화 비교(YAML) + JSON Pointer 필드 선택** 채택
| 후보 | 장점 | 단점 | 판단 |
|---|---|---|---|
| **구조화 비교** `{field, op, value}` + `all/any/not`, `within_ms_of`, `count` | 정적 검증·사람용 문장 자동 생성·freeze 직렬화가 쉬움. 튜링 완전 아님. UDS `expect` 스타일과 같은 계열 | 표현력 제한 | **채택(M1)** |
| CEL | 표현력, 표준, 안전한 평가 | Rust 구현 crate 성숙도 미확인(`cel-interpreter` 등 — Sol 확인), 사람이 읽기 어려움 | M3 이후 `expr:` 탈출구 후보 |
| JSONPath | 선택은 강력 | 비교·시간 술어 없음, 구현 간 차이 | 필드 선택은 더 단순한 JSON Pointer로 대체(serde_json `Value::pointer` 사용 가능) |
- 연산자: `== != < <= > >= in regex contains between`, 바이트열 `bytes_eq`(hex), 시간 `within_ms_of{step, event, max}`, 집계 `count/min/max` over evidence set.
- 정규식 엔진은 Rust `regex`(백트래킹 없음, ReDoS 안전) — 같은 문법을 UART `expect`와 공유한다(UART 팀과 확인 필요).

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

### 6.5 회귀 고정 `freeze`
- 출력: `loom_scenario: 1` + `kind: test` + 원 세션·계획 해시.
- **고정되는 것**: 실행한 명령과 해석된 인자(별칭→실 ID), 단계 순서·분기, `expect` 술어, timeout, 일괄 승인 요구 등급 요약, finally.
- **고정 안 되는 것**:
  - 자연어 판단(`manual` 단계로 남아 재실행 때 사람이 판정).
  - 에이전트가 계획 밖에서 한 탐색 조회(제외하고 목록으로 경고).
  - 실측 타이밍(재실행은 같은 timeout 상한으로 기다리고 시각은 새로 측정).
  - 장치가 준 값(`save:` 변수로 재획득).
  - 사용자 개입(재현하지 않음, 원본 보고서에만).
- 실행: `loom run regress/X.scenario.yaml --test` → 같은 세션 엔진·검증기·보고서. 에이전트 없이 CI·야간 회귀에 쓴다.

### 6.6 보고서
- **JSON(정본)**: 세션, TC/계획 해시, 단계별 판정·증거 REF·검증 결과·사용자 수정·메모, 재현 정보.
- **HTML**: `loomd`가 `/reports/<sess>`로 제공(UI에서 열람)하고 정적 내보내기도 한다. 증거 REF는 `loom://records/<session>?seq=a..b` 링크로 UI 기록 탐색기의 그 구간을 연다.
- **JUnit XML**: testsuite = TC, testcase = 단계, `<failure>`/`<skipped>`, properties에 증거 REF·plan hash. inconclusive는 `<error type="inconclusive">`로 둔다(CI 소비 도구별 표시 차이 미확인).
- **재현 정보**: Loom 버전·커밋, OS, env, 프로파일(`loom.toml`·policy) 해시, 장치 신원(포트 VID/PID, CAN 인터페이스 모델·드라이버 버전, UDS F189/F195 등 계획에 `collect_versions` 단계가 있을 때의 ECU SW 버전, Saleae 모델·Logic 2 버전), 시뮬레이터 seed.

## 7. 라이브러리 선택과 근거 (Rust, C1 — 버전은 Sol 확인 전이라 모두 미확인)
| 용도 | 후보 | 비고 |
|---|---|---|
| Markdown 파싱 | `pulldown-cmark` | 표 확장 지원(GFM tables) 확인 필요 |
| YAML | `serde_yaml`(보관 상태로 알려짐) → 대안 crate | **미확인**. 유지보수 상태 확인 후 결정 |
| 엑셀 가져오기 | `calamine`(xlsx/xls/ods 읽기) | docx는 결정적 파서 없이 에이전트가 읽어 `.tc.md` 작성 → 사람 확인 |
| 술어 평가 | 자체 구현(구조화 비교) + `regex` + serde_json `pointer` | CEL crate는 후속 검토 |
| HTML 템플릿 | `minijinja` 또는 `askama` | 미확인 |
| JUnit XML | `quick-junit` 또는 직접 생성 | 미확인 |
Python 사용 없음(C1).

## 8. HW / OS 지원
TEST 기능은 소프트웨어다. `loomd`가 도는 곳이면 동작한다: 실HW Windows x64·Linux x64(B1), 개발·CI macOS(sim). 보고서 HTML은 브라우저만 있으면 된다. 장치 지원 범위는 각 도메인 계획을 따른다. TC의 `equipment`가 이 호스트에 없으면 `blocked`다.

## 9. 시뮬레이션 & 테스트 전략
- **M1 E2E**: TC-1을 `env=sim`에서 실행한다. 장치는 SimPsu, UART pty `bootshell`, CAN SimNode(PowerMode 신호), UDS 가상 DoIP ECU(DTC 1개 주입)다. 실행자는 (a) 스크립트화한 가짜 에이전트(CI용 결정적 명령열), (b) 실제 Claude Code 수동 시연 둘 다로 한다.
- **검증기 단위 시험**: 표 기반으로 존재·창·술어·gap 조합 40+ 사례를 둔다. "없는 seq", "창 밖", "술어 거짓", "gap 중 absence 주장", "user 이벤트 인용" 거부를 반드시 넣는다.
- **환각 방지 회귀**: 가짜 에이전트가 지어낸 seq·값으로 pass를 시도하면 100% 거부되는지 본다.
- **freeze 왕복**: 세션 → freeze → `loom run --test` 결과가 같은 판정이어야 한다(manual 제외).
- **가져오기**: 샘플 xlsx 3종(열 순서·병합 셀·한국어 열 이름) → `.tc.md` 골든 비교.
- 3-OS CI, 모두 sim.

## 10. 안전 / 가드레일
- **일괄 사전 승인**: `test start` 시 계획의 R2·R3 단계를 하나의 뷰어 승인으로 묶는다. 바인딩은 plan hash·단계별 동작·대상·**횟수 상한**(예: `psu.out on ×5`)·env·프로파일 해시다. 사용 기한은 A9 확장이 필요하다: "세션 시작 10분 내 개시, 세션 중 유효, `max_test_duration` 기본 1 h"(§15 요청).
  - **R4는 묶지 않는다** — 그 단계 시작 때 매번 뷰어 승인(once).
  - `default_blocked`/`immutable_deny` 단계가 있으면 계획을 거부한다.
- **승인 범위 밖 쓰기**: 계획에 없는 R2+ 쓰기는 일반 경로대로 개별 승인을 받는다. 같은 세션 태그가 붙으므로 보고서에 "계획 밖 쓰기"로 표시된다.
- **env=vehicle**(B4): 같은 등급·경로를 쓴다. UI에 배지를 상시 표시하고, 스킬은 R2+ 단계 시작 전 사용자 확인을 묻는다(§4.1-5).
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
| **M1** 걷는 뼈대 | sim 위 **TC-1을 웹 UI 테스트 화면으로 실행**: 계획 검토·일괄 승인 → 단계 진행·실시간 증거 → 사용자 일시정지+직접 송신 → 에이전트가 `others_since_last`로 인지·메모 → 증거 검증 pass/fail → 사용자 판정 수정 1회 → HTML·JUnit 보고서 | ① 지어낸 seq로 pass 시도 거부 ② 단계 띠가 모든 레인에 표시 ③ 명령 → 화면 ≤ 200 ms(render ack) ④ 두 번째 브라우저 늦은 합류 시 같은 상태 ⑤ STOP 시 finally 미실행 + 복구 키 ⑥ 3-OS CI(가짜 에이전트) green |
| **M2** 실무 입력 | 엑셀 가져오기, `test ask`, `freeze` → `loom run --test`, TC-2(LA·DLT)·TC-3(repeat·measure) | freeze 왕복 동일 판정, xlsx 골든 3종, LA 사후 증거 창 검사 |
| **M3** 실HW·확장 | Windows/Linux 실HW에서 TC-1 변형(PCAN·실 UART), JUnit CI 연동, CEL 탈출구 검토, 다중 TC 묶음(suite) | 실HW 체크리스트 두 OS, 24 h 회귀 야간 실행 |

## 12. 작업 분해 (병렬 단위 · 담당 · 의존)
| ID | 작업 | 담당 | 의존 |
|---|---|---|---|
| T1 | `.tc.md`·`.plan.yaml` 스키마 명세 + fixture | Opus | — |
| T2 | 술어 DSL 명세 + 사람용 문장 생성 규칙 | Opus | T1 |
| T3 | 증거 REF 문법 + 도메인별 `evidence[]` 요구 목록(uart/can/uds/dlt/la/psu) | Opus | T1 |
| T4 | `_xteam/core-test.md` 초안(세션·마커·검증기·판정·일괄 승인 API) | Opus + CORE 리드 | T1–T3 |
| T5 | 검증기 사례표 40+ (단위 시험 정본) | Sol | T2, T3 |
| T6 | Rust crate 조사(YAML, Markdown 표, calamine, JUnit, 템플릿, CEL) | Sol | — |
| T7 | `loom-test` SKILL.md + references(해석 예시, 판정 규칙표, 질문 조건) | Opus | T1–T3 |
| T8 | UI 테스트 화면 명세 → `_xteam/ui-test.md` | Opus | T4 |
| T9 | 보고서 템플릿(HTML/JUnit/JSON) 명세 + 샘플 | Sol | T4 |
| T10 | sim TC-1 환경 정의(SimPsu·bootshell·SimNode·가상 ECU DTC 주입) + 가짜 에이전트 명령열 | Sol | 3팀 M1 sim |
| T11 | freeze 변환 규칙(고정/비고정 표, 경고) | Opus | T1, D12 스키마 |
| T12 | 엑셀 매핑 형식 + 골든 샘플 3종 | Sol | T1, T6 |
| 구현(CORE) | 세션 엔진·검증기·판정 저장·보고서 API (CORE crate) | CORE | T4 |
| 구현(UI) | 테스트 화면 | UI | T8 |

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
| TC-C5 | UART·CAN·UDS·탐색 | 대기·조회 결과 `--json`의 `data.evidence[]`에 REF(`<domain>:<channel>@seq:…`)와 kind 포함. 예: `uart expect` 매치 seq, `can sig wait-for` 프레임 seq+값, `uds dtc list` 요청/응답 seq, `la wait` 캡처·행, `dlt wait-for` 로그 seq |
| TC-C6 | UI | `_xteam/ui-test.md`: §5 화면, 타임라인 단계 띠, 계획 검토 모달, 판정 수정(뷰어 전용), 기록 링크 `loom://records/...` |
| TC-C7 | UART | `expect` 정규식 문법을 술어 `regex`와 공유(Rust `regex`) |
| TC-C8 | CORE | 기록 불능(A12) 시 pass 판정도 중단 |
