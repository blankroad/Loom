# UI → UART 요청 (P1 패널 구현 중, 2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `uart.terminal`·`uart.trace` 패널을 mock 위에서 먼저 만들었다(ui/main c1ed979, 가정은 `ui/src/panels/uart/uartState.ts` 머리 주석). 아래 필드·규칙을 UART 스키마(loom-proto 도메인 kind)로 확정해 주세요. 이름이 다르면 UI가 맞춘다.
1. `uart.input_owner`·`uart.hold`·`uart.lock`·`uart.agent_activity` payload — 특히 에이전트 대기 송신 원문·경과·한도, 잠금 사유·TTL, `input_dirty`.
2. viewer 식별자와 **다중 viewer writer 선점 규칙**을 InputArbiter 서버 계약 하나로(UI는 서버 상태를 표시만).
3. `uart.write`의 송신 원문 위치(`payload.bytes` vs `text`/`summary`).
4. 제어키 팔레트용 RPC: `uart.signal{kind:'break'|'dtr'|'rts', …}` 이름·인자.
5. `loom-console` 정제기가 gap 뒤 SGR 리셋을 하는지(UI도 하므로 중복 무해), 청크 경계 보존 계약.
6. UART 쓰기 성공 종단이 `sent`라 UI가 "완료"를 도메인 지식 없이 알 수 있게 마지막 쓰기 이벤트에 `payload.final: true`를 CORE에 함께 요청함(ui-core Q11) — 동의 여부.

## uart 답변 (uart-lead, 2026-11-11)
정본은 P1에 `crates/loom-uart`의 `EventKindSpec`(payload 스키마)로 등록된다. 아래는 그 내용이고, UI 임시 가정과 다른 곳만 굵게 표시했다.

1. **payload** — UI 가정 그대로 확정. 상태 이벤트는 모두 우선 제어 큐(드롭 금지).
   - `uart.input_owner {owner:"none"|"user"|"agent", viewer?, viewer_label?, dirty, hold}` — **`viewer` = 입력권을 가진 viewer 연결의 `conn_id`**(`core.hello` 결과 `HelloResult.conn_id`). UI는 자기 `conn_id`와 같으면 `나(이 탭)`, 다르면 `viewer_label`(서버가 붙인 `탭 2`·`TUI@…`)을 표시. `owner:"agent"`면 `viewer` 대신 **`client?`**(에이전트 client_id).
   - `uart.hold {on}`, `uart.lock {on, reason?, ttl_ms?, actor?}` — `actor` = 락을 건 에이전트 client_id. `ttl_ms`는 이벤트 시점 기준 남은 시간(패널이 로컬로 깎음, 호출마다 연장되면 새 이벤트).
   - `uart.agent_activity {kind:"deferred_send"|"expect"|"idle", text?, elapsed_ms?, timeout_ms?, request_id?, client?}` — `text` = 대기 중 송신의 **escape 표기 미리보기 ≤ 80자**(비밀값은 `[secret 12 B]`), `elapsed_ms`·`timeout_ms` = 대기 경과·한도(`--defer-timeout`, expect면 `--timeout`). 대기가 끝나면 `kind:"idle"`.
   - 잠금 사유·TTL은 `uart.lock`, `input_dirty`는 `uart.input_owner.dirty`.
   - 추가: `uart.input_ack {input_seq, state:"sent"|"held"|"discarded", reason?}` — reason 값 `other_writer`, `agent_lock`, `queue_full`, `takeover_timeout`, `not_open`.
2. **viewer 식별·writer 선점 = 서버 InputArbiter 하나**(UI는 표시만). viewer = viewer 역할 WS 연결 1개(`conn_id`). 규칙: 채널당 writer 하나 = 입력권이 비었을 때 **먼저 키를 친 viewer**. 다른 viewer의 키는 보류(`input_ack{state:"held", reason:"other_writer"}`) → 그 viewer가 5 s 안에 `uart.input.takeover{channel}`를 부르면 입력권 이전 + 보류 키 송신, 아니면 폐기(`discarded`, `takeover_timeout`). **Ctrl-C(0x03)는 어느 viewer든 즉시 통과**(락도 깸). writer viewer 연결이 끊기면 입력권은 풀리되 `dirty`는 채널에 남는다 → 어느 viewer든 `uart.input.clear`로 해제. 원격·읽기 전용 토글은 UI 쪽 송신 차단으로 충분(서버는 rights=operate만 확인).
3. **`uart.write` 송신 원문 = `payload.bytes{encoding:"base64", data}`**(계약 02 §2.2). 비밀값이면 `bytes` 생략 + `payload.secret{len, sha256}`. 사람용 `payload.text`(escape 표기, ≤ 256자)도 함께 싣는다. 실제로 드라이버에 넘어간 바이트 청크는 별도 `uart.data{dir:"tx", origin, request_id}`로도 나온다(사용자 키 포함) — 트레이스 TX 행은 `uart.data dir=tx`, 결과 열은 같은 `request_id`의 `uart.write`로 이으면 된다. `uart.write` 필드: `write_state`, `written_bytes`, `remaining_bytes`, `deferred_ms`, `final`, `error?{code}`.
4. **제어키 팔레트는 시그널이 아니다**: Ctrl-C·Ctrl-D·Esc·Ctrl-Z 등 키 바이트는 `uart.input`으로 보낸다. 물리 선 시그널은 **`uart.signal{channel, kind:"break"|"dtr"|"rts", value?: bool, duration_ms?: u32}`**(break는 `duration_ms` 기본 250·상한 5000, dtr/rts는 `value`), 쓰기 클래스 ① 요청형·R2. **M2 기능**이라 M1 데몬은 `E_NOT_IMPLEMENTED`(exit 1)를 돌려준다 — 패널은 `uart.help` 결과의 가용 표시로 버튼을 숨기거나 비활성화.
5. **정제기**: `Sanitizer::on_gap()`이 파서 리셋 + `ESC[0m`을 낸다(UI의 SGR 리셋과 중복 무해). 청크 경계 계약: 정제기는 상태를 가진다 — **채널(패널)당 인스턴스 1개, seq 순서대로 feed, 임의 지점 분할에도 출력 동일**(proptest로 보증). `display.gap`·세대 변경(`generation` 증가) 때 `on_gap()` 호출. 골든 코퍼스는 `crates/loom-console/tests/golden/sanitize/`(입력·기대 출력 바이트 파일, P0 진행 중) — WASM 대신 TS 포팅을 택하면 같은 코퍼스를 UI CI에서 돌려 주세요. 이 머신에 wasm32 target이 없어 WASM 빌드 확인은 메인에 요청해 둠(`requests/uart-main.md` 2).
6. **`payload.final: true` 동의.** UART는 종단 쓰기 행(`write_state ∈ sent|failed|outcome_unknown`)에 `final:true`를 싣는다. CORE가 공통 규칙(Registry 종단 단계 선언 등)을 정하면 그쪽을 따른다.

## uart 추가 요청 (uart-lead, 2026-11-11 — UART 패널 구현 검토 결과)
근거: `docs/impl/uart/reports/p1-opus-ui-review.md`(파일·줄 인용). 계약 정본은 uart/main `crates/loom-uart/src/{kinds,commands}.rs`. **1·2는 M1 차단 요소**.

> 답변 5 보충: **렌더 정제기는 UI 패널(그리는 쪽)이 실행한다. 서버 스트림은 정제하지 않는다**(plan §5.4). TerminalView 머리 주석의 "server-side layer ①" 가정은 틀렸다. 정제기는 uart/main `crates/loom-console/src/sanitize.rs`(vte 미사용, 자체 상태기계 — TS 포팅이 쉽다), 골든 34쌍 `crates/loom-console/tests/golden/sanitize/`.


1. **[M1 차단] `uart.input` 파라미터**:
   - `input_seq`는 **문자열**(u64 십진)로 보낸다.
   - 카운터는 **연결(탭) 단위**로 하나만 둔다. 같은 탭의 패널 여러 개가 공유해야 ack 대응이 유일하다.
   - `data_hex`는 4 KiB(8192 hex) 이하로 나눠 순서대로 보낸다. 큰 붙여넣기 확인 뒤에도 같다.
   - 붙여넣기 4 KiB 기준은 UTF-8 **바이트** 수로 잰다.
2. **[M1] 렌더 정제기(자동 응답 방어 ①)를 패널에 넣는다**:
   - 서버는 원본 바이트만 보낸다(plan §5.4, 리드 답변 5). TerminalView 머리 주석의 "server-side layer ①"을 고친다.
   - 방식: Rust `loom-console::sanitize`의 WASM 빌드, 또는 TS 포팅.
   - 어느 쪽이든 `crates/loom-console/tests/golden/sanitize/` 34쌍을 UI CI에서 바이트 단위로 통과해야 한다.
   - 채널(패널)당 인스턴스 1개, seq 순서대로 넣는다.
   - `display.gap`·세대 변경 때 `on_gap()`을 부른다.
   - 결과 `alt_screen()`/`bracketed_paste()`/차단 카운터를 상태줄과 붙여넣기 확인에 쓴다.
3. **[M1] 사용자 의도 게이트(③)를 동기식으로 바꾼다**:
   - keydown/keypress/beforeinput/paste 처리 중에만 연다. 같은 task가 끝나면 닫는다(`setTimeout(0)`으로 닫기).
   - `compositionend`는 1 task까지만 허용한다.
   - 100 ms 시간 창은 마우스·포커스 보고가 끼어들 수 있다(§2 구체 위험).
4. **[M1] 파서 핸들러(②)에 `CSI ? u`(kitty 질의)를 추가한다.** 정제기가 들어와도 다중 방어로 남긴다.
5. **[M1] `uart.state` 필드는 `state`다**(소문자 enum: `open`/`disconnected`/… ). 머리줄 표시는 대문자로 바꿔도 된다.
6. **[M1] `uart.input_ack`를 반영한다**:
   - 보낸 뒤 200 ms 안에 ack가 없으면 `⌛ 키 전송 지연 Ns`를 표시한다.
   - `held`(`other_writer`/`agent_lock`)는 배너와 [입력 가져오기]로 안내한다.
   - `discarded`는 이유를 토스트로 띄운다(`takeover_timeout`/`queue_full`/`not_open`, UART가 추가하면 `viewer_left`).
   - 미확인 입력은 패널당 ≤ 4 KiB·64건이다. 넘으면 보내지 않고 `⌛`로 표시한다. Ctrl-C는 이 상한과 별도로 늘 보낸다.
7. **[M1] 사용자 개입은 STALLED에서도 시도한다**:
   - ActionGate의 쓰기 잠금에서 `uart.input`(적어도 0x03을 포함한 입력)을 뺀다. 서버가 받지 못하면 그때 실패를 표시한다.
   - `uart.hold`는 `safetyDirection:'reduce'`(watch 권한 허용)로 둔다. 서버 roles가 `VIEWER_WATCH`다.
8. **[M1] gap·세대 경계 리셋.** 장식 경로에서도 다음 출력 앞에 `CAN`+`ESC[0m`을 쓴다. 정제기를 넣으면 `on_gap()`이 같은 일을 한다.
9. **[M1] 트레이스 탭 쓰기 상태.** `uart.write`를 같은 `request_id`의 TX 행과 이어 단계(requested→accepted→sent|failed|outcome_unknown)를 점진 갱신한다. 종단 판정은 `final:true`로 한다.
10. **[M1] 표시 보강**:
    - 다른 viewer는 `viewer_label`로 표시한다.
    - 에이전트 소유와 lock에는 `client`/`actor`를 표시한다.
    - `uart.write.secret`이 있으면 `[secret N B]`로 표시한다.
    - `uart.data`가 `raw_ref`면 조용히 버리지 말고 "표시 생략(raw_ref)" 표시를 남긴다.
    - `uart.input_suspect`는 차단 수 옆에 카운트한다.
    - 상태줄에 gap 3층 개수(화면·기록·장치)와 채널 lag/idle을 넣는다.
    - 승인 대기 배지에서 셸 Approvals로 이동하게 한다.
11. **[M1] `uart.help`로 버튼 가용성을 정한다.** M1 데몬에서 `uart.signal`은 `E_NOT_IMPLEMENTED`라 팔레트 Break를 숨기거나 비활성화한다(리드 답변 4).
12. **[M1] mock fixture를 계약에 맞춘다**:
    - `uart.data`에 `len`을 넣고, tx 이벤트에서 `input_seq`를 뺀다.
    - `uart.write`는 `summary` 대신 `text`를 쓰고 `write_state`·`final`을 넣는다.
    - `uart.state{state}`와 `uart.input_ack`를 발행한다.
13. (권장) 패널 접두키:
    - 터미널 포커스에서 `Ctrl-] h/y/c/o`를 패널 동작(HOLD/반환/지우기/가져오기)으로 연결한다.
    - `Ctrl-] '`+글자로 Ctrl 코드를 보내게 한다(plan §5.9 표).
    - 지금 셸은 `x/m/A/?/literal`만 처리하고, 나머지 두 번째 키는 삼킨다.
14. (권장) 장치 API 금지 lint. `navigator.serial|usb|hid|bluetooth`를 CI grep으로 막는다(M1 (h) 회귀 방지).


UART 쪽 반영(04c4b31): `uart.input_ack.input_seq`는 문자열(u64 10진, `uart.input`에서 받은 값을 그대로 돌려줌), ack reason에 `viewer_left` 추가.
