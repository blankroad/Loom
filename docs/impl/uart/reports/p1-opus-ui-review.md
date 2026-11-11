DONE

# P1 uart-opus: UI UART 패널 구현 검토 (T11w)

- 작성: uart-opus, 2026-11-11. docs/plan/02-core-principle.md 준수. 코드는 고치지 않았다(읽기만).
- 검토 대상:
  - **`ui/main` 브랜치 최신(dd8b0b9)**. uart worktree의 `ui/`(c1ed979)보다 SDK 파일 3개가 앞서 있어 `git show ui/main:…`로 읽었다.
  - 패널: `ui/src/panels/uart/{terminalPanel,tracePanel,uartState}.ts`.
  - SDK: `ui/src/sdk/{terminalView,actionGate}.ts`, `ui/src/shell/{prefixKeys.ts,TopBar.tsx}`.
  - mock: `ui/tools/mock/sim.ts`.
- 대조 기준:
  - worktree `crates/loom-uart/src/{kinds,commands}.rs`, `requests/ui-uart.md` `## uart 답변`.
  - 내 `loom-console::{sanitize,arbiter}`.
  - plan §5.2–5.10, M1 수용 (f2)(h).
- 줄 번호 표기: `TP` = terminalPanel.ts, `TV` = sdk/terminalView.ts, `US` = uartState.ts, `TR` = tracePanel.ts.

## 요약 (심각도 순)
| # | 문제 | 영향 | 심각도 |
|---|---|---|---|
| 1 | **자동 응답 방어 ① 정제기가 UI에 없다.** TV 4–5행 주석이 "server-side loom-console sanitizer is layer ①"이라고 잘못 가정한다. 장치 바이트가 그대로 `term.write`로 간다(TP 165) | 마우스 추적·포커스 보고 모드, C1 CSI, kitty 질의, OSC 기타가 xterm에 들어간다. ③ 게이트가 시간 창(100 ms)이라 타이핑 중 생긴 보고가 장치로 새어 나간다 | **M1 (f2) 불합격 위험** |
| 2 | `uart.input`의 `input_seq`가 **숫자**다(TP 69·84). 계약은 `{"type":"string"}`(commands.rs 264–270) | gateway 스키마 검증에서 `E_SCHEMA`. **사용자 키가 하나도 장치에 가지 않는다** | **M1 차단** |
| 3 | 입력 청크 상한이 서로 다르다. UI는 최대 64 KiB를 모아 보내고(TV 29), 큰 붙여넣기는 한 번에 보낸다(TP 84). 계약은 `data_hex` ≤ 8192자, 곧 4 KiB | 4 KiB가 넘는 붙여넣기·입력이 `E_SCHEMA` | 높음 |
| 4 | `uart.state` 필드 이름이 다르다. UI는 `status`를 읽고(US 88) 계약은 `state`다(kinds.rs 141) | 머리줄 포트 상태가 늘 `?`로 표시된다 | 높음 |
| 5 | `uart.input_ack`를 쓰지 않는다 | 키 전송 지연 `⌛`, 보류·폐기 이유가 화면에 나오지 않는다(§5.3·§5.8) | 중 |
| 6 | 연결이 STALLED이면 `uart.input`(Ctrl-C 포함)을 클라이언트가 막는다(ActionGate 58–60). hold는 operate 권한이 필요하다(TP 202). 서버는 hold를 watch 권한에도 허용한다(commands.rs `VIEWER_WATCH`) | 연결이 흔들릴 때 Ctrl-C를 보낼 길이 없다. 위험을 줄이는 동작인 hold가 watch 권한 사용자에게 막힌다 | 중 |
| 7 | gap·세대 경계에서 장식(decoration) 경로가 파서와 SGR을 리셋하지 않는다(TV 378–387) | gap 앞에서 반쯤 들어온 escape와 색이 gap 뒤 바이트를 오염시킨다 | 중 |
| 8 | 트레이스에 쓰기 상태 점진 갱신이 없다(TR 104 — `uart.data`만 읽음) | plan M1 ④ "트레이스 탭 최소 = 출처 행 + 쓰기 상태 점진 갱신" 미충족 | 중 |
| 9 | 패널 API 표면 | 장치를 여는 경로 0(아래 §5) | 합격 |

---

## 1. 계약 대조 (UI가 읽는 것 ↔ `kinds.rs`·답변)
| kind / RPC | 필드 | 계약 | UI 현재 | 판정 |
|---|---|---|---|---|
| RPC `uart.input` | `input_seq` | string(u64 십진) | number (`++inputSeq`, TP 69·84) | **불일치 → E_SCHEMA** |
| RPC `uart.input` | `data_hex` | ≤ 8192자(4 KiB) | 최대 64 KiB 묶음, 큰 붙여넣기는 통째로 | **불일치** |
| RPC `uart.input` | `input_seq` 유일성 | 연결 단위로 ack를 대응 | 패널 인스턴스마다 1부터(TP 64). 같은 탭에 같은 채널 패널이 둘이면 겹친다 | 위험 |
| `uart.state` | `state` enum(소문자) | `state` | `p.status` (US 88) | **불일치** |
| `uart.config` | `baud`, `env` … | — | `baud`만 읽음. env는 `ctx.env` | 일치 |
| `uart.input_owner` | `owner`,`viewer`,`dirty`,`hold` | — | 읽음. `viewer === ctx.connId` 비교(US 115) | 일치 |
| `uart.input_owner` | `viewer_label` | 다른 viewer 표시 이름 | 안 씀. 다른 viewer를 raw `conn_id`로 표시(US 126) | 미사용 |
| `uart.input_owner` | `client` (owner=agent) | 에이전트 client_id | 안 씀. "입력권: 에이전트"만 표시(US 128) | 미사용 |
| `uart.lock` | `on, reason, ttl_ms, actor` | ttl_ms = 이벤트 시점 남은 시간 | 읽고 로컬로 깎음(US 120–121). `actor`는 표시 안 함 | 거의 일치 |
| `uart.agent_activity` | `kind, text, elapsed_ms, timeout_ms` | + `request_id`, `client` | 4개만 읽음 | 일치(추가 필드 미사용) |
| `uart.input_ack` | `input_seq, state, reason` | reason `other_writer`/`agent_lock`/`queue_full`/`takeover_timeout`/`not_open` | **처리 안 함** | 미사용 |
| `uart.input_suspect` | `pattern, query_seq` | 자동 응답 의심(감사) | 처리 안 함 | 미사용(차단 수 옆 표시 권장) |
| `uart.write` | `write_state` | — | 읽음(TP 170) | 일치 |
| `uart.write` | `final` | 종단 행이면 true | 안 씀 | 미사용(완료 표시에 쓸 것) |
| `uart.write` | `text`, `bytes`, `secret` | `bytes{base64}` 우선, 비밀값이면 `secret` | `bytes`가 없으면 `text`(TP 173–176). `secret`은 무시 | 거의 일치(비밀값은 `[secret N B]`로 표시 필요) |
| `uart.write` | `error{code}`, `deferred_ms` | — | 안 씀 | 미사용(실패 이유·대기 시간) |
| `uart.data` | `bytes{encoding,data}`, `len`, `rx_offset` | `raw_ref` 가능 | `raw_ref`면 `null`을 받고 **조용히 버린다**(TP 157, TR 107) | 위험(표시 누락을 숨김) |
| `uart.data` dir=tx | 송신 원문 | 트레이스 TX 행 = `uart.data dir=tx` | 그렇게 함(TR 108). 터미널은 tx를 그리지 않음(TP 163) | 일치 |
| `uart.expect_result`, `uart.mark`, `uart.signal`, `uart.transfer` | — | M1/M2 | 안 씀 | M2에서 확인 |
| RPC `uart.signal` | `kind` | M1 데몬은 `E_NOT_IMPLEMENTED`. 패널은 `uart.help` 가용성으로 숨김 | 팔레트 Break 버튼을 늘 표시(TP 37). `queries: []`라 help를 조회하지 않음 | 불일치(누르면 오류 토스트) |
| RPC `uart.hold` | 권한 | watch 허용(`VIEWER_WATCH`) | operate 필요(TP 202, uartActions 255) | UI가 더 엄격 |
| mock `sim.ts` | `uart.data`에 `len` 없음, tx에 `input_seq`, `uart.write`에 `summary`. `uart.state`·`input_ack` 미발행 | — | — | fixture를 계약에 맞게 갱신 필요 |

## 2. 자동 응답 3중 방어 (plan §5.5)
| 층 | 상태 | 위치 | 비고 |
|---|---|---|---|
| ① 정제기 (`term.write` 직전) | **없음** | TP 165 `term.write(bytes)`에 원시 바이트. TV 4–5행 주석이 서버 정제를 가정 | plan §5.4·DR-V2: 서버 스트림은 원본이고, 정제는 그리는 쪽(WASM 또는 TS 포팅 + 골든)이다. 리드 답변 5번도 "채널(패널)당 인스턴스 1개"였다. 골든 34쌍은 `crates/loom-console/tests/golden/sanitize/`에 있다 |
| ② xterm 파서 핸들러 | **부분** | TV 247–277 | 아래 표 |
| ③ 사용자 의도 게이트 | **부분** | TV 26 `INTENT_WINDOW_MS = 100`, TV 164·279–290·308–316 | 시간 창 방식이다. plan은 "같은 이벤트 루프 턴"(동기 플래그)이다. 키를 친 뒤 100 ms 안에 생긴 에뮬레이터 보고는 통과한다 |

②가 등록한 것: DA1 `c`, DA2 `>c`, DA3 `=c`, DSR `n`, `?n`, DECRQM `$p`·`?$p`, `t`, XTVERSION `>q`, DCS `$q`·`+q`, OSC 4/10/11/12(`?` 포함일 때), OSC 0/1/2/8/52.

②에 빠진 것:
- **kitty 키보드 질의 `CSI ? u`.** xterm.js master에 `kittyKeyboardQuery` 응답이 있다 [확인: InputHandler.ts]. 고정 버전 6.0.0에 들어 있는지는 [미확인].
- 정제기 ①이 없어 막지 못하는 것:
  - 마우스 추적 DECSET `?9/1000/1002/1003/1005/1006/1015/1016`, 포커스 `?1004`.
  - C1 코드 포인트 U+009B(= CSI). 정제기는 이를 지우지만 xterm은 CSI로 해석한다.
  - DCS 기타(sixel 등), APC/PM/SOS, OSC 7/104/777 등.

**구체 위험 (f2)**:
- 장치가 `ESC[?1003h`(모든 마우스 이동 보고)를 켠다.
- 사용자가 키를 치며 마우스를 움직인다.
- 마지막 keydown 뒤 100 ms 안에 생긴 마우스 보고가 ③을 통과해 `uart.input`(origin=user)으로 장치에 들어간다.
- `others_since_last`에 가짜 사용자 개입이 남고, 입력권도 사용자에게 넘어간다.
- ①이 있으면 DECSET 자체가 지워져 생기지 않는다.

**결정(UART 쪽)**:
- ①은 UI 패널의 몫이다. 서버는 원본만 보낸다(plan 유지).
- UI가 WASM과 TS 포팅 중 무엇을 택하든 골든 34쌍 통과를 수용 조건으로 둔다.
- ③은 동기 게이트로 바꾸기를 요청한다. keydown/keypress/beforeinput/paste 처리 중에만 열고 같은 task가 끝나면 닫는다. IME `compositionend`는 한 task까지만 허용한다.

## 3. 입력 경로 (§5.3)
| 항목 | plan | UI 현재 | 판정 |
|---|---|---|---|
| onData 1회 = `uart.input` 1회 | 같은 프레임 안 연속 키는 2 ms 안에서만 합침 | 5 ms 묶음(TV 27·329) | 허용 범위(지연 +3 ms). 보고만 함 |
| Ctrl-C 우선 | 대기열을 건너뛰고 즉시, 자기 앞선 키 뒤 | 0x03이면 즉시 flush, 앞선 키가 먼저(TV 328) | 일치 |
| Ctrl-C가 STALLED 중에도 시도됨 | 사용자 개입은 막지 않는다(A3·A12) | ActionGate가 STALLED면 `E_STALLED`로 거절 | **불일치** — `input`(최소 0x03 포함)은 쓰기 잠금 예외로 |
| 미확인 입력 상한 | 사용자 4 KiB·64항목 + Ctrl-C 64 B·64항목(§3.13) | 송신 전 대기열 64 KiB(TV 29). 보낸 뒤 ack 추적 없음 | **불일치** — ack를 기준으로 미확인 ≤ 4 KiB·64건, Ctrl-C 별도 |
| ack 200 ms 지연 표시 | `⌛ 키 전송 지연 0.4s` | 없음 | 없음 |
| 보류·폐기 안내 | `held`/`discarded` + 이유 토스트, `other_writer`면 [입력 가져오기] | 없음(가져오기 버튼만 활성화) | 부분 |
| 붙여넣기 > 4 KiB 확인 | 모달 | `card` 확인(TP 81–86) | 있음. 단 기준이 `text.length`(UTF-16 단위)이고, 확인 뒤 **4 KiB 넘는 단일 RPC**를 보냄 → E_SCHEMA. 바이트 기준 판단 + 4 KiB 조각 순차 송신 필요 |
| 여러 줄 붙여넣기 확인(장치 bracketed paste 꺼짐) | §5.3 [제안] | 없음(정제기 ①의 `bracketed_paste()` 상태가 필요) | 없음 |
| IME | 조합 중에는 보내지 않고, 확정 바이트부터 입력권 | composition 이벤트가 게이트를 연다(TV 133). 보내는 것은 xterm에 맡김 | 일치 |
| PAUSED 중 키 | 보내지 않고 경고, Ctrl-C만 통과 | 그렇게 함(TV 317). 버린 키는 세지 않음, 칩에 안내 | 거의 일치 |
| 읽기 전용 | `disableStdin` | 그렇게 함(TV 107·170) | 일치 |
| 복사 / Ctrl-C | Ctrl-C = 0x03, 복사 = Ctrl-Shift-C / Cmd-C | 그렇게 함(TV 282–287) | 일치 |

## 4. 상태줄(§5.7) · gap · 다중 탭 입력권(§5.8)
**상태줄**:
- 있는 것:
  - 포트·baud·gen·env(머리줄).
  - 입력권 칩, 최근 송신과 단계, expect 대기.
  - rx 속도, 차단 수(질의·자동응답), 화면 생략 바이트, REC.
  - 1 Hz 갱신.
  - LIVE/LAG/STALLED 배지(셸 TopBar 전역).
  - vehicle 테두리(shell.css 29).
- 빠진 것:
  - **채널(패널) 단위 lag/idle.** 셸 배지는 전역이다.
  - **gap 3층 개수.** 화면 `display.gap`·기록 `record.gap`·장치 `source.gap`을 따로 세지 않고, `markGap` 줄만 남긴다.
  - **키 전송 지연 ⌛.**
  - **승인 대기 배지.** 셸 Approvals로 가는 링크가 패널에 없다.
  - **정제기 종류별 차단 수.** ①이 없어 없다.
  - **`uart.input_suspect` 표시.**

**gap·세대**:
- `mirror.gaps`가 `markGap`(장식)으로 연결되어 있다. 좋다.
- 그러나 장식 경로에서는 xterm 파서가 리셋되지 않는다. gap 직전에 잘린 `ESC [ 3 1`이 gap 뒤 바이트와 붙어 해석된다.
- 세대(`generation`) 변경 때도 리셋이 없다.
- 해결: 정제기 `on_gap()`과 같은 동작이 필요하다. 다음 `term.write` 앞에 `CAN`(0x18) + `ESC[0m`을 쓴다. TV의 fallback 경로에는 이미 있다.

**다중 탭 입력권**:
- 있는 것:
  - 자기 `conn_id` 비교로 "나 / 다른 뷰어"를 구분한다.
  - [가져오기] 버튼이 있다.
  - lock 칩과 남은 시간을 보여 준다.
- 빠진 것:
  - 다른 viewer 이름을 `viewer_label`로 보여 주지 않는다(지금은 conn_id 원문).
  - `other_writer` 보류 배너("탭 2가 입력 중 — [입력 가져오기]")와 5 s 폐기 안내가 없다. input_ack를 쓰지 않아서다.
  - 에이전트 소유일 때 `client`를 보여 주지 않는다.
  - `canYield`가 "내 입력권"일 때만 켜진다(US 116). arbiter는 줄이 미완결이면 어느 viewer에게든 두 번 누르기 지우기를 허용한다. 작은 차이고, [지우기] 버튼으로 대체할 수 있다.

**백로그→라이브**:
- 구분선은 장식이다. 좋다.
- 구독에 `backlog:{lines, from_line_boundary}`가 없다. 지금은 `snapshot:'required'`에 mirrorRows 4000(이벤트 수)이다. 백로그 첫 줄이 줄 중간에서 시작할 수 있다.
- 이 옵션은 CORE 구독 계약 항목이다. 리드가 CORE에 확인할 것.

## 5. 패널 API 표면 (M1 (h))
- `ui/src` 전체에서 `navigator.serial`, `requestPort`, `navigator.usb`, `navigator.hid`, `navigator.bluetooth`, `SerialPort`가 **0건**이다(`git grep ui/main`).
- UART 패널 actions는 `uart.input`/`.yield`/`.clear`/`.takeover`/`hold`/`unhold`/`signal`뿐이다.
  - `uart.open`·`close`·`release` 같은 포트 수명 RPC가 없다.
  - 장치 접근은 gateway RPC로만 한다.
- **합격.**
- 권장: UI CI에 이 grep을 금지 규칙(lint)으로 넣으면 회귀를 막을 수 있다.

---

## ui-uart 추가 요청 초안
(리드 검토 후 `requests/ui-uart.md`에 붙인다. 근거는 `crates/loom-uart/src/{kinds,commands}.rs`와 이 보고서 §1–4.)

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

## UART 쪽 수정 제안
1. **`uart.input_ack.input_seq` 타입.**
   - `kinds.rs`는 string인데 내 arbiter의 `ArbiterEvent::payload()`는 숫자(u64)를 낸다.
   - 결정: 계약(string, u64 십진)이 정본이다.
   - arbiter의 `input_seq`를 문자열로 받고 내보내게 고친다. 또는 worker가 발행 전에 문자열로 바꾼다.
   - 나는 arbiter 쪽을 고치는 것을 권한다. 원본 문자열을 그대로 돌려줄 수 있다.
2. **`uart.input_ack.reason`에 `viewer_left`를 추가한다**(kinds.rs 205).
   - arbiter는 연결이 끊긴 viewer의 보류 키를 이 이유로 폐기한다.
   - 그 viewer는 이미 없지만, 같은 사용자의 다른 탭과 감사 기록이 이유를 볼 수 있어야 한다.
   - 대안: arbiter가 `takeover_timeout`으로 합친다. 정보가 줄어 권하지 않는다.
3. `uart.agent_activity.text`: kinds는 `maxLength 320`, 답변은 "≤ 80자"다.
   - arbiter는 80자(UTF-8 최대 320바이트)로 자르므로 둘은 맞다.
   - 스키마 설명에 "80 chars"를 적어 UI가 헷갈리지 않게 한다.
4. `requests/ui-uart.md` 답변 5에 한 줄을 추가한다: "정제기는 UI 패널(그리는 쪽)이 실행한다. 서버 스트림은 정제하지 않는다." UI 주석이 반대로 가정하고 있다.
5. `uart.input`의 `data_hex` maxLength 8192와 arbiter 보류 대기열(4 KiB)의 관계:
   - 4 KiB 붙여넣기 한 건이 다른 writer 때문에 보류되면 대기열이 가득 찬다. 이후 키는 `queue_full`이 된다.
   - 의도된 상한이므로 그대로 둔다. UI 요청 6의 미확인 상한(패널당 4 KiB)과 맞물린다.
