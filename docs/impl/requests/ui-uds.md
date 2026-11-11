# UI → UDS 요청 (P1 패널 구현 중, 2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `uds.console` 패널을 mock 위에서 먼저 만들었다(ui/main c1ed979, `ui/src/panels/uds/udsModel.ts`). 아래를 UDS 스키마로 확정해 주세요.
1. `uds.request{dry_run:true}` 결과 모양, 그리고 dry-run은 행·감사를 만들지 않는다는 확인(명령줄 미리보기를 150 ms 디바운스로 부름, 레이트 상한 알려 주세요).
2. NRC 종결 = `write_state:'failed'` + `nrc`·`nrc_name`인지, timeout 표시 필드(`timeout:true`?), `late`·`ok_suppressed`·가드 거부(`guard:'deny'`?) 필드 이름.
3. `uds.state` 필드: 세션 확신도(`confirmed|assumed|stale`), 보안 레벨, TP 상태, P2/P2*, lease 소유자 `owner_origin/actor`(사용자면 conn_id인지).
4. 명령줄 문법: M1은 hex만. `read F190`·`session ext` 같은 문법은 서버 파서(dry-run에 텍스트 전달)로 할지.
5. 시계 비교는 wall이 아니라 `core.time`(recv/send mono) 기준으로(uds-ui ui 의견과 같음).

## UDS 답변 (uds-lead, 2026-11-11 — P0 결정, 정본은 `crates/loom-uds` payload 타입·JSON Schema가 나오면 그것)
F1(기본 모두 자동)을 반영했다. 바뀌면 이 절을 고치고 CHANGELOG처럼 날짜를 남긴다.
1. **dry-run**: `uds.request{text|bytes_hex, target, dry_run:true}` → `{dry_run:true, target, bytes:{encoding:"hex",data}, sid, service, subfunction?, suppress_pos:bool, risk:"R0".."R4", reason, policy_action:"auto"|"confirm"|"approve"|"deny", block:null|"immutable_deny"|"hard_deny"|"default_blocked", parse_error?:{at, message}}`. **행·이벤트·감사·저널을 만들지 않는다**(확인). 레이트: 연결당 10회/s, 넘치면 `E_RATE_LIMIT`(exit 4) — UI 150 ms 디바운스면 걸리지 않는다.
2. **종결 표시**(`uds.result` payload, 같은 `request_id` 행 갱신): NRC = `write_state:"failed"` + `nrc`(u8) + `nrc_name`(ISO 이름, codec 표). timeout = 읽기는 `write_state:"failed"` + `timeout:true`, 쓰기·링크 끊김은 `write_state:"outcome_unknown"` + `timeout:true`(재전송 없음). 늦은 응답 = `uds.rx`에 `late:true` + 원래 `request_id`(행을 다시 열지 않음). SPRMIB 성공 = `write_state:"sent"` + `ok_suppressed:true`. 가드 거부 = `write_state:"failed"` + `block:{class, reason}`(F1 기본에서는 0x2A·0x86만 해당) — 별도 `guard` 필드는 없다. 정책 동작은 행에 `policy_action`으로 같이 둔다.
3. **`uds.state`**: `{target, generation, link:{state:"up"|"down"|"connecting", cause?}, session:{id:u8, name, confidence:"confirmed"|"assumed"|"stale"}, security:{level:u8|null, confidence}, tester_present:{active, interval_ms}|null, p2_ms, p2_star_ms, lease:{owner_origin:"agent"|"user"|"script", actor_id, client_id}|null}`. 사용자면 `actor_id` = 뷰어 계정 id, `client_id` = 연결 id(conn_id).
4. **명령줄 문법**: 서버 파서로 한다 — UI는 입력 텍스트를 dry-run `text`로 보내고 결과의 `bytes`·`parse_error`를 미리보기에 쓴다. M1 문법: hex(`22 F1 90`/`22F190`) + `read <DID>`, `write <DID> <hex>`, `session <default|ext|prog|hex>`, `tp`. 나머지는 P2.
5. **시계**: 동의. UDS 결과에 `t_send_mono_ns`·`t_first_rx_mono_ns`·`t_final_rx_mono_ns`(gateway mono)를 넣고, UI는 `core.time` 기준으로 비교한다. wall은 표시용.

### UDS 답변 개정 (2026-11-11, P1-a 정합 결과 — 위 1·2를 대체·보완)
- **행 이벤트**: `uds.tx`(송신) · `uds.pending`(0x78마다, 신규) · `uds.result`(종결)가 **같은 자기완결 `RowPayload`**를 쓴다(UI rowStore가 payload를 통째로 교체하므로). `uds.rx`는 응답 원문 기록용이며 `write_state`가 없고 행을 열지 않는다(늦은 응답 `late:true`도 여기).
- 응답 바이트는 `resp_bytes`(계약 02 `PayloadBytes{encoding:"hex",data}`) — 별도 hex 문자열 필드 없음. 요청 바이트는 `payload.bytes`.
- 실측은 `first_rx_ms`·`rtt_ms`, 한도는 `p2_ms`·`p2_star_ms`(state의 한도와 같은 이름·뜻). `pending_count`, `summary`, `decoded[]`, `dtcs[]` 포함. dry-run에 `did_name`·`summary` 추가.
- 정본 예시: `crates/loom-uds/fixtures/*.json`(Rust 타입에서 생성, 시험으로 고정). UI 쪽 차이 목록은 `docs/impl/requests/uds-ui.md` #1–#17.
