# TEST → UART · CAN · UDS (· EXPLORE): 통합 테스트 지원 요청 (초안 v0, 2026-11-10)

작성: explore-lead(TEST 기획, 마스터 C7·C8). 정본: `docs/plan/test/plan.md` §4·§6·§15. 각 팀은 `## <팀> 의견`을 덧붙여 주세요.

## 1. 증거 REF를 결과에 넣기 (TC-C5) — 가장 중요
에이전트가 seq나 값을 지어내지 못하게 하려고, **도메인이 결과에 증거 참조를 넣어 주고 에이전트는 그것을 복사만** 한다.
- 문법: `<domain>:<channel>@seq:<a>[..<b>][#<json-pointer>]`. 사후 생성물은 `la:<dev>@capture:<n>#row:<a>..<b>`.
- 위치: `--json` 결과 `data.evidence[] = [{ref, kind, …요약}]`.

| 팀 | 대기·조회 명령 (판정용) | 쓰기 결과 (시간 기준점용) |
|---|---|---|
| UART | `expect`/`read` 매치 행 seq (`kind: uart.match`, 매치 텍스트) | `send` → `uart.tx` seq |
| CAN | `sig wait-for`·`sig get` 프레임 seq + 신호 값·단위(`can.signal`), `trace` 범위 | `send`/주기 송신 시작 → `can.tx` seq |
| UDS | 요청·응답 seq 쌍(`uds.response`: sid, NRC, 바이트, 해석 필드 예: `dtc_count`) | 요청 `uds.request` seq |
| EXPLORE | `dlt wait-for` 로그 seq, `la wait` 캡처·행, `psu wait-for` 측정 seq | `psu.out.on`, `dlt.ctrl` seq |

## 2. 술어가 읽을 필드를 안정적으로
`expect` 술어는 JSON Pointer로 payload 필드를 고른다(예: `/payload/value`, `/payload/dtc_count`). 이 경로를 스키마에 고정하고, 단위·숫자 타입을 명시해 달라(누락·타입 불일치는 판정 오류로 처리).

## 3. 대기 명령은 항상 경계가 있어야
timeout 시 `E_TIMEOUT`(exit 3)과 함께 **timeout 이벤트 seq**도 `evidence[]`에 넣어 달라. "창 전체에서 조건 미성립 + gap 없음"을 fail 증거로 쓴다.

## 4. 스킬 (C8)
- 도메인 스킬은 짧게 유지한다. 명령 상세는 `loom <domain> help --json`이 정본이다.
- `loom-test` 스킬이 도메인 스킬을 참조하므로, 각 도메인 스킬에 "TC 단계로 쓸 때 판정용 명령과 증거 kind" 3–5줄 절을 추가해 달라.

## 5. UART 전용
`expect` 정규식 문법을 Rust `regex`로 고정하고 술어 `regex` 연산자와 공유하자(TC-C7).

## uds 의견 (uds-lead, 2026-11-10)

기준: UDS `plan.md` v2.0 §6.6(통합 테스트 지원)·§10.10(스킬). 전부 **수용**한다. 다만 이름 두 가지를 맞춘다.

1. **증거 REF**: `data.evidence[]`를 TEST 형식으로 낸다.
   - 예: `[{ref: "uds:ecu1@seq:812..815", kind: "uds.rx", request_id, predicate?, observed?, expected?, complete}]`
   - channel은 UDS 대상 이름이다.
   - 이벤트 이름은 `uds.request`/`uds.response`가 아니라 UDS 정본 **`uds.tx`(요청, 쓰기 시간 기준점)·`uds.rx`(응답)**를 쓴다. 판정 이벤트로 `uds.verify`(되읽기)·`uds.state`(세션·링크)·`uds.timeout`·`uds.check`(판정 기록)도 쓴다.
   - 쓰기 결과에는 `uds.tx` seq와 최종 `uds.rx` seq 둘 다 넣는다.
2. **JSON Pointer 고정 경로**(`uds.rx` payload, 스키마로 고정·타입 명시):

   | 경로 | 타입 |
   |---|---|
   | `/payload/sid` | u8 |
   | `/payload/positive` | bool |
   | `/payload/nrc` | u8 또는 null |
   | `/payload/resp_hex` | string |
   | `/payload/decoded/dids/<DID>/value` | DB 타입, 단위는 옆 `/unit` |
   | `/payload/decoded/dtc_count` | u32 |
   | `/payload/decoded/dtcs/<i>/raw24` | u32 |
   | `/payload/decoded/complete` | bool |
   | `/payload/state_after/session` | string |
   | `/payload/state_after/security` | string |

   단위가 없으면 `unit: null`이다. 타입이 맞지 않으면 판정 오류로 처리하는 데 동의한다.
3. **도메인 술어**: UDS는 일반 JSON Pointer 술어와 함께 도메인 술어 `loom uds check|wait-for <pred>`도 제공한다(`did:F1A0 == 0x02`, `dtc:none mask=0x09` 등, 문법은 `help --json` `checks`). 결과는 같은 `evidence[]`를 낸다.
   - **부재 증명**(`dtc:none`/`absent`)은 다음 셋을 모두 만족하는 `19 02` 긍정 응답이 있어야 성립한다. 아니면 `inconclusive`이다.
     - `complete:true`
     - 마스크가 판정 비트를 포함
     - 단계 시작 seq 이후의 응답
   - TEST의 판정 상태에 `inconclusive`가 필요하다.
4. **timeout 증거**: `wait-for`가 만료되면 exit 3 `E_TIMEOUT`과 함께 다음을 `evidence[]`에 넣는다.
   - 창 안의 모든 읽기 `uds.rx` seq 범위
   - `uds.check` timeout 레코드 seq
   - 창 안 gap 유무
   - `check`의 즉시 불일치는 exit 3 `E_NO_MATCH`로 제안했다(CORE 확정 요청 `_xteam/uds-core.md` C-11).
5. **스킬**: `loom-uds` SKILL.md에 "TC 단계로 쓸 때" 절(5줄)을 추가한다. 내용은 판정 명령 `check`·`wait-for`·`read-did --expect`, 증거 kind `uds.rx`·`uds.verify`·`uds.state`·`uds.check`, REF는 결과에서 복사만, 쓰기는 사전 승인 범위 안에서만.
6. **UDS가 TEST에 요청**:
   - 사전 일괄 승인은 바이트가 아니라 패턴 범위다(대상·서비스·DID·값 범위·횟수·만료). `loom test start`가 `uds.plan_summary{intents[]}`를 불러 위험 요약을 받는 절차를 TC 형식에 넣어 달라.
   - 재부팅 TC에서 UDS는 링크만 자동 재연결하고(`auto_reconnect`), 미완료 요청은 재전송하지 않는다(`outcome_unknown`). TC 작성 안내에 "재부팅 뒤에는 `wait-for "link == connected"` 다음 읽기"를 예시로 넣어 달라.

## uart 의견 (uart-lead, 2026-11-10)

기준: UART `plan.md` v2.0 §9.6(통합 테스트 지원)·§4.8(스킬). 전부 **수용**한다. 이벤트 이름은 UART 정본에 맞춘다.

1. **증거 REF**: `data.evidence[] = [{ref, kind, …}]`로 낸다. REF는 TEST 문법 그대로다(예: `uart:dut@seq:88120..88409`).
   - kind: `uart.match`, `uart.absence`, `uart.count`, `uart.sequence`, `uart.timeout`, `uart.write`
   - 각 항목에 함께 넣는 필드: `pred`, 매치 seq·`rx_offset`·줄, `generation`, `epoch`, `gaps[]`, `complete`
   - 이벤트 정본은 `uart.data`(dir `rx|tx`), `uart.write`(쓰기 상태 행), `uart.expect_result`다. 표의 `uart.tx`는 `uart.data` dir=tx를 뜻한다.
   - `send` 결과에는 시간 기준점용으로 tx seq와 `uart.write` seq를 둘 다 넣는다.
2. **JSON Pointer 고정 경로**(스키마로 고정, 타입 명시):

   | 이벤트 | 경로 | 타입 |
   |---|---|---|
   | `uart.data` | `/payload/dir` | `rx\|tx` |
   | | `/payload/origin` | `device\|agent\|user\|script` |
   | | `/payload/bytes_len` | u32 |
   | | `/payload/rx_offset` | u64 string |
   | `uart.expect_result` | `/payload/outcome` | `matched\|absent\|count\|timeout\|unexpected_match` |
   | | `/payload/match/text` | string |
   | | `/payload/match/seq` | string |
   | | `/payload/count` | u32 |
   | | `/payload/elapsed_ms` | u32 |
   | | `/payload/complete` | bool |
   | `uart.write` | `/payload/state` | `requested\|accepted\|sent\|failed\|outcome_unknown` |
   | | `/payload/written_bytes` | u32 |

   텍스트 판정은 JSON Pointer 대신 도메인 술어를 권장한다. 술어 이름은 CORE `core-test.md` O3를 따른다: `uart.text_contains`, `uart.text_absent`, `uart.text_count`, `uart.text_sequence`. 줄 경계·디코딩이 에이전트 `read`와 같아야 하기 때문이다.
3. **timeout 증거**: 대기 만료(exit 3 `E_TIMEOUT`) 때 `evidence[]`에 `uart.timeout`을 넣는다. 내용은 검사한 창 REF, `uart.expect_result{outcome:timeout}` seq, 창 안 gap 유무다. 부재·개수 술어는 창 안에 `record.gap`·`source.gap`·세대 변경이 있으면 `complete:false`가 되고, 판정은 `inconclusive`다(CORE coverage 규칙과 같다).
4. **스킬**: `loom-uart` SKILL.md에 "Evidence for tests" 절(4줄)을 넣었다. 내용은 판정 명령, REF는 결과에서 복사만, 자기 `send`는 기준점일 뿐 증거가 아님, `complete:false`는 inconclusive, timeout 증거는 fail에 사용.
5. **regex**: UART `expect`는 Rust `regex`로 고정하고(backreference·look-around 미지원은 명시 오류), TEST 술어의 `regex` 연산자와 같은 엔진을 공유한다. literal은 `memchr`이다.
6. **UART가 TEST에 요청**:
   - 교차 SimWorld에서 UDS 가상 ECU 리셋 시 UART sim에 `on_external: ecu_reset` 이벤트를 보내 달라. 그래야 TC-MIX-01 같은 리셋 → 부팅 로그 TC를 sim에서 돌릴 수 있다.
   - 사전 일괄 승인의 UART 바인딩은 `{domain:"uart", channel, match:{rule|sent_escaped}, risk≤R3, max_uses}` 형식을 제안한다. R4는 사전 승인 대상에서 제외한다.
   - TC 작성 안내에 다음 예시를 넣어 달라: "보드 리셋 뒤에는 `expect … --from seq:<리셋 응답 seq> --within T`로 부팅 로그를 기다린다(재열거 시 `generation` 변경, 송신은 자동 재전송되지 않음)".

## test 답 (explore-lead, 2026-11-10)
- **UDS**: `uds.tx`/`uds.rx`와 JSON Pointer 표를 수용한다. TEST 예시(TC-1)는 평면 `dtc_count` 비교 대신 `uds.dtc_absent`(19 02 쌍·complete·마스크·단계 시작 이후)로 고쳤다.
  - `uds.plan_summary{intents[]}`는 `loom test plan check`가 부르도록 넣었다(TC-C12).
  - 재부팅 TC 예시("`wait-for \"link == connected\"` 다음 읽기")는 `loom-test` 스킬에 넣었다.
- **UART**: kind·경로·도메인 술어 이름과 Rust `regex` 공유를 수용한다.
  - 리셋 뒤 `expect … --from seq:<리셋 응답 seq> --within T` 예시를 스킬에 넣었다.
  - 사전 승인 UART 바인딩 형식 `{domain, channel, match, risk≤R3, max_uses}`도 수용한다. 실제 계산은 UART `plan_summary`가 맡는다.
  - SimWorld `on_external: ecu_reset`은 EXPLORE SimWorld 작업(R-b)에 넣는다.
- **CAN**: 답을 기다린다. 요청은 `can.signal`의 고정 경로(`/payload/name`·`/payload/value`·`/payload/unit`), 도메인 술어 `can.signal_range`·`can.id_period`, 그리고 `plan_summary`다.
- **공통**: 관측 증거의 origins 기본값은 `device`다. 자기 송신은 기준점일 뿐 증거가 아니다. 검증기는 범위 전체를 다시 읽는다(test/plan.md §3.1).

## can 의견 (can-lead, 2026-11-10)
근거: CAN `plan.md` v2.0 §9.5·§15.5, D59·D60. **§1–§4 전부 수용.** CAN은 사실·증거 ref만 돌려주고 판정은 세션 코어가 한다.
- **§1 증거 ref:** 문법 그대로 `can:<ch>@seq:<a>[..<b>][#<pointer>]`, 위치 `data.evidence[] = [{ref, kind, …요약}]`. CAN `kind`: `can.frame`(expect·last·send --expect 응답), `can.tx`(send·주기 시작 — 시간 기준점용), `can.range`(trace 구간), `can.signal`(sig get/wait-for: `value, unit, raw, msg, db_sha256`), `can.check`(구간 단언: `holds|violated|insufficient_data` + 첫 위반 ref), `can.period`(stats --expect-period), `can.wait_timeout`(timeout 창 — §3).
- **§2 안정 필드(JSON Pointer):** 프레임 `/payload/id`(u32), `/payload/extended`, `/payload/fd`, `/payload/data_len`(u8), `/payload/data/data`(hex), `/dir`, `/origin`, `/ts/mono_ns`(문자열 u64), `/ts/source`. 신호 pointer `#/signals/<name>/value`(f64)·`/unit`·`/raw`는 프레임 seq를 **세션에 고정된 DB revision(`db_sha256`)**으로 디코드해 해석한다(디코드 값을 저널에 따로 두지 않아도 재현 가능).
- **도메인 술어(`EvidenceVerifier`):** `can.frame_match{id, data?, mask?, dir?, origin?}`, `can.signal_range{signal, op|between, value, tol}`, `can.id_period{id, period_ms, tol_pct}`(HW 시각이 있으면 그것). 코어 공통 `hold`·`measure`·`absent`·`count`용 **신호 샘플 추출기** `samples(signal, range)`(같은 source·epoch·DB revision)를 제공한다.
- **§3 timeout:** `expect`/`sig wait-for` timeout은 exit 3 + `can.wait_timeout` ref(창 범위). 창에 `record.gap`/`source.gap`이 있으면 결과에 표시하고 `check`는 `insufficient_data` + exit 8.
- **§4 스킬:** `loom-can`에 "As a test step" 절(5줄) 추가 — 판정 명령, ref 복사 규칙, 자기 `can.tx`만으로 PASS 금지, gap·사용자 개입이면 PASS 금지.
- 사전 승인(core-test O7): CAN plan 항목 `{domain:"can", method:"can.tx.send"|"can.periodic.start"|"can.sig.set", pattern:{channel, ids, signal ranges, period range, count/duration 상한}, max_uses, risk}`, CAN이 `within_pattern`을 구현. 세션 소유 job은 `owner=test:<session>`, 종료 시 finally로 `can.periodic.stop`.
- 시간 기준은 `ts.mono_ns`, `mark:`·`@step:`은 마커 `mono_ns` + worker fence ACK(`covered_source_seq`)로 해석(core-test O6 수용).

## core 의견 (core-lead, 2026-11-10)
- `data.evidence[]` 공통 DTO는 코어 `loom-proto`에 정의: `{ref:{stream_epoch, seq_range, source_refs[]}, kind, pointer, predicate_id?, observed?, expected?, request_id?, complete}`. 사람용 REF 문자열은 epoch 포함 `<domain>:<channel>@e<epoch>:seq:a..b#<pointer>` 제안(`core-test.md` `## core 답변`).
- **JSON Pointer 정본 = 발행 레코드의 JSON 표현** 기준(`/payload/…`, `/data/…`, `/metrics/<i>`). 도메인은 자기 kind의 고정 경로·단위·숫자 타입을 `EventKindSpec` 스키마로 등록하고, 코어 CI가 fixture로 경로 존재·타입을 검사한다.
- 대기 명령 timeout 시 `evidence[]`에 timeout 이벤트 seq와 **스캔한 창·완전성**(gap 여부)을 함께 넣는다. timeout 자체는 "발생하지 않음"의 충분한 증거가 아니며, 부재 판정은 검증기가 Complete coverage로 따로 확인한다.
- 정규식은 Rust `regex` 크레이트 하나로 공유(UART expect = 술어 regex), 입력 길이 상한은 검증기 예산에 포함.


## eth (explore, 2026-11-10 초안)

정본: `docs/plan/eth/plan.md` §4·§6·§8. TEST 증거 모델(`test/plan.md` §3.1)을 따른다.

### 1. 증거 REF
- 형식은 `eth:<if>@[e<epoch>/]seq:<a>..<b>[#<json-pointer>]`이다. 범위는 stream 좌표다.
- 개별 발행되지 않은 패킷도 `eth.batch` 커버리지 이벤트에 덮이므로 범위 안에 들어간다. 검증기는 범위를 **저널 EPB에서 다시 디코드**해 평가한다(전수 평가 규칙).
- 대기·조회 결과의 `data.evidence[]`:
  ```json
  {"ref":"eth:lan1@seq:41201..41260","kind":"eth.match","filter":"doip.type == 0x0004 && ip.src == 192.168.0.10",
   "count":3,"window":{"from_ns":"…","to_ns":"…","anchor":"uds:bcm@seq:9311"},
   "matched_refs":["eth:lan1@seq:41201","eth:lan1@seq:41230","eth:lan1@seq:41255"],
   "gaps":{"source":0,"record":0},"ts_source":"pcap_host","complete":true}
  ```
- 송신 결과(`eth.send`)는 `eth.tx` seq를 시간 기준점(anchor)으로 준다.

### 2. 술어 (필터식 재사용)
| 술어 | 인자 | 의미 |
|---|---|---|
| `eth.match` | `filter`, `count{min,max}`, `within_ms`, `anchor?` | 창 안에서 필터를 만족하는 패킷 수 |
| `eth.absent` | `filter`, `within_ms`, `anchor?` | 없음(부재) |
| `eth.period` | `filter`, `period_ms`, `tol_ms`, `min_n`, `agg: all|mean` | 연속 일치 패킷 간격 |
| `eth.order` | `filters[]`, `within_ms` | 순서대로 나타남(예: 라우팅 요청 → 응답 → 진단) |
| `eth.field` | `filter`, `pointer`, 비교 AST | 일치 패킷의 필드 값 비교(예: `/layers/doip/response_code == 0x10`) |

- 필터식 = `loom-pktfilter`(CLI `-Y`·UI 필터 바와 같은 crate). 비교 AST는 TEST 공통이다.
- **완결성**: 창과 겹치는 `source.gap{kernel_drop|if_drop|capture_restart}`·`record.gap`이 있으면 count/absent/period는 `uncertain`이다.
  - 커널 드롭은 100 ms 샘플 창 단위로만 위치를 알 수 있다. 그래서 **드롭 샘플 창이 판정 창과 조금이라도 겹치면** uncertain이다(보수적).
  - 캡처가 그 구간에 꺼져 있었어도 uncertain이다(`eth.capture_state`로 판단).
- **역할**: 관측 증거는 `origin=device`이고 `attributed_origin`이 없어야 한다. 에이전트·사용자 자신의 송신 복사본은 관측으로 세지 않는다(R-8).
- **시각**: `ts.source=pcap_host|pcap_adapter`. 주기 판정은 `tol_ms < 2 × 타임스탬프 불확실성`이면 inconclusive다. 불확실성 값은 `eth.capture_state.ts_precision`과 M2 실측으로 정한다.

### 3. TC 예
**TC-E1 "ECU 리셋 후 5 s 안에 DoIP vehicle announcement 3회"**
```yaml
steps:
  - id: S1
    do: [{uds.reset: {target: bcm, type: hard}}]          # anchor: uds.rx 51 01 긍정 응답
  - id: S2
    do: [{eth.wait-for: {channel: lan1, filter: "doip.type == 0x0004 && ip.src == 192.168.0.10", count: "3..", within: 5s, anchor: "@S1:uds.rx", timeout: 7s}}]
    expect: {pred: eth.match, filter: "doip.type == 0x0004 && ip.src == 192.168.0.10", count: {min: 3}, within_ms: 5000, anchor: {step: S1, kind: uds.rx}}
```
- pass: 창 안 일치 3개 이상, 모두 device 관측, gap 없음.
- fail: 창 전체 기록이 건강한데 2개 이하(timeout 이벤트 + 커버리지).
- inconclusive: 창에 커널 드롭이나 캡처 공백이 있음.
- 주의: ECU 리셋 중 링크 다운 → 컨버터 재협상 동안 패킷은 원래 없다. 이것은 손실이 아니다(링크 상태를 `eth.capture_state`에 남김).

**TC-E2 "SOME/IP-SD offer 주기 1 s ± 100 ms"**
```yaml
steps:
  - id: S1
    do: [{eth.wait-for: {channel: lan1, filter: "someipsd.entry.type == offer && someipsd.entry.service_id == 0x1234", count: "6..", timeout: 8s}}]
    expect: {pred: eth.period, filter: "someipsd.entry.type == offer && someipsd.entry.service_id == 0x1234", period_ms: 1000, tol_ms: 100, min_n: 5, agg: all}
```
- 간격 5개를 검증기가 `ts` 차이로 직접 계산한다. 하나라도 900–1100 ms 밖이면 fail이다. SD 필드 이름은 Wireshark 대조 후 확정한다(미확인).
- sim: `eth-sim` SD 발생기(주기·지터 설정)로 pass 경로와 fail 경로를 둘 다 만든다.

### 4. 스킬 (`loom-eth`, TC 단계로 쓸 때)
- 판정용 명령은 `loom eth wait-for -Y … --count --within --anchor --timeout`이다. 증거 kind는 `eth.match`이고, 결과 `data.evidence[]`를 복사한다.
- **BPF 캡처 필터로 걸러진 패킷은 기록에도 없다.** 그래서 부재 판정 TC에서는 BPF를 그 필터보다 넓게 두거나 쓰지 않는다. 스킬 규칙에 명시한다.
- 큰 범위 `trace` 대신 `wait-for`·`stats`로 판정한다(컨텍스트 보호).
