# UDS → UI 요청 (P1-a 계약 정합, 2026-11-11)

docs/plan/02-core-principle.md 준수. 정본은 `crates/loom-uds/src/payload.rs`(Rust 타입 → JSON Schema)이고, 예시는 그 타입에서 생성한 fixture `crates/loom-uds/fixtures/*.json`이다(uds 브랜치, 시험 `crates/loom-uds/tests/fixtures.rs`가 어긋나면 실패). 각 fixture 파일은 `{kind, request_id, payload}`를 순서대로 담은 배열이다(core 봉투는 제외).
UDS가 계약이나 답변과 달랐던 부분은 UDS 쪽에서 고쳤고(아래 "UDS 쪽 변경"), 이 문서에는 **UI가 맞춰야 할 항목**만 적는다.

## UDS 쪽 변경 (ui-uds.md "UDS 답변" 보강 — 리드 확인 요청)
- 행 이벤트는 kind 3개(`uds.tx` = requested/accepted/sent, `uds.pending` = 0x78마다 sent + `pending_count`, `uds.result` = 최종)이고 모두 같은 `RowPayload`를 쓴다.
  - UI rowStore는 이벤트가 올 때마다 payload를 통째로 교체하므로, 모든 행 이벤트에 요청 필드(target, bytes, sid, service, summary, risk 등)를 반복해 넣는다.
- `uds.rx`는 원시 수신 PDU다. `write_state`가 없으므로 행을 만들지도 다시 열지도 않는다. 늦은 응답(`late:true`)은 여기에만 기록된다.
- 추가 필드:
  - 행: `summary`, `first_rx_ms`, `rtt_ms`, `p2_ms`/`p2_star_ms`(현재 적용 중인 한도), `resp_bytes`, `decoded[]`, `dtcs[]`.
  - state: `backend`, `addr`, `tester_present.ok_count`.
  - dry-run: `did_name`, `summary`.

## UI가 맞춰야 할 항목
| # | 필드 | UI 현재값 (udsModel.ts / consolePanel.ts / fixtures) | UDS 정본 | fixture |
|---|---|---|---|---|
| 1 | 이벤트 kind | 단일 `uds.request` 행 이벤트(wire `uds-pending.jsonl`) | `uds.tx`/`uds.pending`/`uds.result`(행), `uds.rx`(원시), `uds.state`. 행 판별은 지금처럼 `request_id` + `write_state`로 하면 된다 | `read_ok`, `pending_then_positive` |
| 2 | 요청·응답 바이트 | `payload.req_hex`, `payload.resp_hex` | `payload.bytes{encoding:"hex",data}`(요청, 계약 02), `payload.resp_bytes{…}`(응답). 둘 다 소문자 hex | `read_ok` |
| 3 | NRC | fixture에서는 문자열 `"0x78"`, pending 행에도 nrc가 있음 | `nrc`는 숫자(u8)이고 최종 행에만 있다. pending은 `pending_count`로만 표시하고, 원시 `7F xx 78`은 `uds.rx`에 남는다. `hexByte`는 숫자도 처리하므로 fixture만 고치면 된다 | `pending_then_positive`, `nrc` |
| 4 | 시간 열 | 최종 행에서 `p2_ms`를 실측 첫 응답으로 읽음 | 실측값은 `first_rx_ms`(송신→첫 응답), `rtt_ms`(송신→최종). `p2_ms`/`p2_star_ms`는 진행 중 타이머의 **한도**다. `timingText`의 최종 분기를 `first_rx_ms/rtt_ms`로 바꿔 달라 | `pending_then_positive` |
| 5 | 디코드 값 | fixture에는 최상위 `value` | `decoded:[{did:"F190", name, value, unit}]`(모델이 이미 읽는 모양) | `read_ok` |
| 6 | 가드 거부 | `guard==='deny'` 또는 `denied` | `write_state:"failed"` + `block:{class:"immutable_deny", reason}` + `policy_action:"deny"` | `blocked_2a` |
| 7 | 늦은 응답 | 행 payload의 `late` | 행에는 오지 않는다. `uds.rx`(같은 `request_id`, `late:true`)를 상세/트레이스에만 표시한다 | `late_response` |
| 8 | TesterPresent 접기 | `payload.purpose` | keepalive는 이벤트 최상위 D3 `purpose:"tester_present"`(P1-b부터). `sid`/`service`로 판별하는 기존 방식도 유지 가능. RowLike에 최상위 `purpose` 노출이 필요 | — |
| 9 | state 링크 | `link`(문자열) | `link:{state:"up"\|"down"\|"connecting", cause?}` | `state` |
| 10 | state 세션 | `session.value` | `session:{id:u8, name:"extended", confidence}`(표시는 `name`) | `state` |
| 11 | state 보안 | `security.level`을 문자열로 읽음 | `security:{level:u8\|null, confidence}`. null이면 잠김(🔒) | `state` |
| 12 | state TP | `tp:{on, ok_count}` | `tester_present:{active, interval_ms, ok_count}\|null` | `state` |
| 13 | state lease | `lease:{owner_origin, actor}`, "나" 판정에 `actor === conn_id` | `lease:{owner_origin, actor_id, client_id}\|null`. 사용자 연결이면 `client_id` = conn_id이므로 `client_id`로 비교 | `state` |
| 14 | dry-run 파라미터 | `uds.request{target, hex, dry_run:true}` | `{target, text\|bytes_hex, dry_run:true}`. **`text` 권장**(서버 파서가 hex와 `read F190`·`write F1A0 02`·`session ext`·`tp`를 처리하고 `parse_error{at,message}`를 돌려준다). 송신도 같은 파라미터 | `dry_run` |
| 15 | dry-run 결과 | `service, did_name, risk, reason` | 앞의 네 필드 + `bytes, sid, subfunction, summary, suppress_pos, policy_action, block, parse_error` | `dry_run` |
| 16 | DTC | — | 최종 행의 `dtcs:[{dtc:"923400", status}]`(원시 24bit hex 6자리) | `dtc_list` |
| 17 | 패널 동작 RPC | `uds.lease.takeover`, `uds.poll.stop` | 아직 등록하지 않았다(lease는 P1-b, poll은 M3). 지금 등록된 RPC는 `uds.request`, `uds.abort`, `uds.read_did`, `uds.write_did`, `uds.session`, `uds.dtc.list`, `uds.connect`, `uds.status`, `uds.trace`, `uds.help` | — |
