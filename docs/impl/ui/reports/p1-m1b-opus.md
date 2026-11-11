docs/plan/02-core-principle.md 준수.

# UI P1 M1 보고 — ui-opus: 접속 흐름 · 새 채널 자동 표시 · 가리키기 · 자동화 브라우저

작성: ui-opus, 2026-11-11. 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui` (커밋 안 함, 직전 p1-m1 변경도 아직 커밋 전 상태 위에 쌓음).
근거: `docs/plan/ui/plan.md` §3.7·§4.1·§5.7·§0.4(V5), `docs/contract/01-processes-ipc.md` §1.3, `docs/contract/02-events.md` §2.5, `crates/loom-proto` `is_priority_kind`·`kinds::UI_FOCUS_REQUEST`.
리드 공지(플레이북 §6) 반영: 대화형 질문 없이 계약대로 정하고, 정한 것은 §3, 질문은 §4에 적었다.

## 0. 결과
- **`pnpm check` 전체 통과**: check:schema ok · typecheck 0 · lint 0 · **테스트 27개 파일 276개**(이번 추가 `src/shell/m1b.test.tsx` **23개**). `pnpm build` 통과.
- 지난 보고(p1-m1)에 적은 net/mock 실패 7건은 해당 담당이 고쳐 지금은 모두 통과한다.

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/shell/bootstrap.ts` (신규) | `#boot=<code>[&next=<route>]` 읽기 → **await 전에 `history.replaceState`로 fragment 제거**(성공·실패 모두) → `POST /api/v1/bootstrap {code}`(same-origin, `cache:no-store`, `referrerPolicy:no-referrer`, `redirect:error`) → 성공: `next` / need-auth 전에 기억한 딥링크(sessionStorage `loom.returnTo`, 1회용) / 기본 레이아웃. 실패: `#/need-auth?reason=expired\|network\|malformed`(사유는 고정 단어, 코드는 어디에도 안 남김). 코드 형식이 틀리면 요청 자체를 안 보냄. `next`는 앱 경로(`/p/…`·`/l/…`)만 허용 |
| `src/main.tsx` | bootstrap을 **WS 연결 전에** 실행, 결과대로 hash 이동 후 서비스 생성(쿠키 없는 WS는 role을 못 받으므로) |
| `src/shell/routes.ts` (신규, Layout.tsx에서 분리) | `PRESETS`·`parseRoute`(need-auth 사유 포함)·`slotHash`(딥링크 `#/p/<domain>/<panel>?props&at=`)·`sameSlot`(커서 `at` 무시 비교). Layout.tsx는 다시 내보냄 |
| `src/shell/ShellApp.tsx` | `NeedAuth{reason}` — 사유 문장(`코드가 만료됐거나 이미 쓰였습니다 — 터미널에서 \`loom ui\`를 다시 실행하세요.` 등), 이미 쿠키가 있어 연결되면 "화면으로 →". 인증 실패로 need-auth에 갈 때 현재 딥링크 기억. 하위 바에 탭 설정, 자격 표시에 "→ watch (자동화 브라우저)", `FocusPrompt` 탑재 |
| `src/store/channels.ts` (신규) | 채널 신호 읽기: `<domain>.state`·`<domain>.*_state`(쓰기 행 제외, core/display/record/source/test/run/ui 제외). channel = `ev.channel ?? payload.channel ?? target ?? interface`, 상태 = `state\|status\|phase\|link\|opened` → open/closed/unknown, 명시 여부. snapshot item 읽기. 도메인 → 기본 패널(uart.terminal·can.trace·uds.console·eth.packets·psu.gauge) |
| `src/store/shellControl.ts` | 채널 항목에 `state`·`addedLiveMs`·`genChangedMs`. `applyChannelSignal`: 채널 바에 **항상** 추가, 새로 열림(또는 닫힘→다시 열림)이면 `onChannelOpened` 알림. `noteSnapshotItems`(늦은 합류: 조용히 추가). **합류 유예 3 s**(§3-4). **자동화 브라우저**: `automation`·`mockServer`·`automationCapped` → 실제 데몬이면 `rights`를 watch로 낮춤 |
| `src/store/tabSettings.ts` (신규) | `auto_open_panels: on(기본)\|off`, `follow_agent: ask(기본)\|auto\|off` — localStorage, 저장 실패해도 이 페이지 동안 동작 |
| `src/store/focus.ts` (신규) | `ui.focus_request{panel, props, at, reason, actor}` 검증(패널 id 형식, props ≤ 16개·값 ≤ 256자·원시값만, `at` = `seq:N\|mark:X\|-Ns`, 이유 ≤ 500자). ask → 대기 1건, auto → 즉시(입력 중이면 보류 후 tick마다 재시도), off → 무시(횟수만). 같은 요청 2 s 안 → 합침(×N), 연 직후 같은 요청도 합침, 다른 요청은 최신으로 교체. **경로 이동·reveal만 하고 어떤 버튼도 누르지 않는다** |
| `src/shell/services.ts` | 토스트에 동작 버튼·수명·key(같은 key는 교체). `uiState.extraSlots`(레이아웃 옆 추가 패널 ≤ 4). `showSlot`(이미 보이면 그대로, 아니면 추가). 새 채널 → 토스트 + auto_open on이면 패널 추가 / off면 `[패널 열기]`. 가리키기: 패널이 이미 화면에 있으면 reveal만, 없으면 딥링크로 이동 후 reveal(마운트 기다려 100 ms × 20회 재시도, 못 찾으면 알림). 키 입력 시각 추적(입력 중 판정), `navigator.webdriver` → automation. 셸 구독 snapshot item을 채널 바로 |
| `src/sdk/context.ts` | 패널 구독 snapshot item도 채널 바로 전달 |
| `src/shell/FocusPrompt.tsx` (신규) | `에이전트 claude-1이 보여 주려 함 · CAN ch0 Trace · seq 5012로 이동 · 이유 …` [열기 ⏎] [이 탭은 자동으로 따라가기 ☐] [무시 Esc](입력칸 밖에서 Enter/Esc), 보류 중 표시, auto로 따라간 경우 안내 줄. 탭 설정 선택 상자 |
| `src/shell/ChannelBar.tsx` | 상태별 표시(닫힘은 취소선 + "(닫힘)"), 새 채널 10 s 강조, 세대 변경 10 s `gen2 ↻`, 칩 클릭 = 그 채널 패널 열기 |
| `src/shell/Layout.tsx` | 추가 패널을 레이아웃 뒤에 렌더(✕로 닫기) |
| `src/shell/Banners.tsx` | 자동화 브라우저 띠, 토스트 동작 버튼, 토스트별 타이머(다시 그려도 연장 안 됨) |
| `src/shell/shell.css` | 위 요소 |
| 시험 | `src/shell/m1b.test.tsx` 23개 |

## 2. 시험 (과제 5 대응)

| 요구 | 시험 | 결과 |
|---|---|---|
| bootstrap 성공·fragment 제거 | 호출 순서 `replace → fetch(이미 제거됨)`, 주소 `/ui/`(코드 없음), POST 경로·옵션·본문 `{code}`, `next` 딥링크로 이동 / `next` 없으면 기억한 딥링크(1회 사용 후 삭제) → 없으면 기본 레이아웃 | 통과 |
| bootstrap 실패 | 410 → `#/need-auth?reason=expired`, 실패해도 fragment 제거, 결과·hash에 코드 없음 / fetch 예외 → `network` / 형식 오류 → 요청 안 함 / fragment 없으면 아무것도 안 함 / 외부 URL `next` 무시 / need-auth 화면 사유 문장, DOM에 코드 없음 | 통과 |
| 새 채널 토스트·채널 바·auto_open | on: 칩 `open`+강조, `에이전트가 uart dut2를 열었습니다 — 패널을 열었습니다`, 추가 패널 uart.terminal dut2 / off: 칩 있음, 패널 없음, `[패널 열기]` 누르면 추가 / 레이아웃에 이미 있는 채널은 중복 안 엶 / `uds.state{target, link:'up'}`·`eth.capture_state{running}` → uds.console·eth.packets / 같은 상태 반복은 토스트 1번 / 닫힘 "(닫힘)" / 다시 열림 + 세대 2 `gen2 ↻` + "다시 열었습니다" / 합류 유예 안의 psu.state는 바만, 유예 뒤 새 채널은 토스트 / snapshot item은 조용히 / 설정 저장·저장 실패 시에도 동작 | 통과 |
| 가리키기 ask/auto/off | ask: 문구 그대로, 누르기 전엔 이동 없음, [열기] → `#/p/can/trace?channel=ch0&at=seq%3A5012` + reveal `can:ch0@5012` / Enter·Esc / 이미 화면에 있으면 이동 없이 reveal / auto 즉시 + 안내 줄 / off 무시 | 통과 |
| 입력 중 보류 | auto + 입력 중 → 보류 표시, 입력 계속이면 그대로, 입력 끝나면 열림(이미 보이는 패널이라 reveal 7000만) | 통과 |
| 중복 합치기 | 2 s 안 같은 요청 ×2, 연 직후 같은 요청 합침, 2 s 뒤 새 요청 / [자동 따라가기] 체크 → 설정 auto + 열기 / 잘못된 패널 id 무시 | 통과 |
| webdriver 띠 | 실제 데몬: "자동화 브라우저 — 보기 전용", 자격 watch, operate 동작은 게이트에서 거부되고 RPC 안 나감, **STOP 버튼 활성** / mock 서버: "mock 서버라 쓰기 허용" / 일반 브라우저: 띠 없음 | 통과 |

## 3. 계약대로 정한 것 (질문 대신)
1. **bootstrap 경로 `POST /api/v1/bootstrap`, 본문 `{code}`, 성공 = 2xx + Set-Cookie** — 과제 문구대로. 5xx는 `network`, 4xx는 모두 `expired`(만료·재사용·없는 코드를 구분하지 않음 — 코드 추측에 정보를 주지 않기 위해).
2. **딥링크 + bootstrap fragment 형식 `#boot=<code>&next=<url-encoded route>`** — 계약에 형식이 없어 정함. `loom ui --layout X` / `loom ui open …`(TTY)이 이 형식으로 열어야 원래 화면으로 돌아온다.
3. **자동화 브라우저 + mock 서버 = 쓰기 허용**(띠는 표시): 실제 데몬은 자동화 세션에 watch만 주므로(V5) UI도 watch로 낮춘다. mock(`hello.capabilities`에 `mock`)은 rights를 쿼리로 주는 개발 도구이고 e2e(`e2e/can.spec.ts` 송신)가 Playwright로 쓰기를 시험하므로 막지 않는다. 판단 근거는 서버가 mock이라는 사실뿐이라 실제 데몬에서는 우회되지 않는다.
4. **합류 유예 3 s**: 연결 직후 3 s 안에 처음 보이는 채널 중 명시적 열림(`opened`/`closed`/`opened:bool`/`*.channel_state`)이 아닌 상태 보고(예: psu.state 1 Hz, `uds.state{link:'up'}`)는 이 탭이 오기 전부터 있던 채널로 보고 채널 바에만 올린다. 유예 뒤 처음 보이는 채널은 상태 보고만으로도 "새로 씀"으로 보고 토스트한다.
5. **"현재 레이아웃의 빈 자리"**: M1 레이아웃은 고정 프리셋이라 빈 자리가 없다. 레이아웃에 같은 패널·채널이 있으면 그대로 두고, 없으면 레이아웃 옆 **추가 패널**(≤ 4, 오래된 것부터 빠짐, ✕로 닫기)로 연다. M2 도킹에서 탭 패널로 바꾼다.
6. 가리키기 [열기]: 대상 패널이 이미 화면(레이아웃·추가 패널·단일 패널 경로)에 있으면 **화면을 바꾸지 않고 reveal만**, 없으면 딥링크로 이동(레이아웃이 바뀜) 후 reveal. 채널 없는 패널이나 `mark:`/`-5s` 커서는 이동만 하고 reveal은 안 한다(해당 REF 형식이 없음).
7. 토스트 주어: 출처별 `에이전트가`/`사용자가`/`스크립트가`/`장치가`/`외부 도구가`.

## 4. 물을 것

### CORE
1. bootstrap: 경로(`/api/v1/bootstrap` vs plan §3.7의 `/bootstrap`), 본문 필드 이름(`code`), 응답 코드(성공 204? 만료 401/410?), `next` 같은 딥링크 동봉 형식을 `loom ui`가 쓰는지.
2. `ui.focus_request` payload 확정: `{panel, props, at, reason, actor}`로 가정. `actor`가 payload에 있는지 envelope `actor_id`만인지, `at` 문법(`seq:N|mark:X|-5s`), 2 s당 1회 제한을 서버도 하는지.
3. **늦은 합류 채널 목록**: 셸은 지금 패널 구독·셸 구독의 snapshot item과 우선 경로 상태 이벤트로만 채널을 안다. 레이아웃에 없는 도메인의 이미 열린 채널은 그 채널이 다음 상태 이벤트를 낼 때까지 바에 안 보인다. → 제어 구독 snapshot에 채널 목록(도메인·채널·상태·세대)을 넣어 주거나 `core.channels.list` RPC.
4. 계약 02 §2.5 kind 목록과 plan §5.7의 `core.ui_focus` 이름이 다르다 — UI는 `ui.focus_request`(loom-proto 상수)를 쓴다. plan 쪽 표기 정정 필요.

### 도메인 (UART · CAN · UDS · ETH · PSU)
1. 채널·대상 열림/닫힘 이벤트 이름과 payload: UI는 `<domain>.channel_state{state:'opened'|'closed', generation?}`를 표준으로 가정하고, `uds.state{target, link:'up'|'down'}`, `eth.capture_state{state:'running'|'stopped'}`, `psu.state`(상태 단어 없음)도 받는다. UART·CAN은 `uart.channel_state`/`can.channel_state`를 내는지(mock은 아직 안 냄).
2. 닫힘을 상태 단어로 줄지(`closed`/`down`/`stopped`) — 위 집합 밖의 단어는 `unknown`으로 보고 바에만 둔다.
3. 세대(`generation`)를 envelope에 싣는지 payload에 싣는지 — 둘 다 읽는다.

### 리드
- `src/proto/provisional.ts`의 `isControlEvent`는 `*_state`만 보고 loom-proto `is_priority_kind`의 `<domain>.state`·`*_error`·`.alert/.timeout/.late`를 놓친다(서버가 제어 구독으로 보내므로 동작에는 지장 없음, 리드 소유 파일이라 손대지 않음).
- e2e 담당에게: 자동화 띠가 mock에서도 보인다(쓰기는 허용). 스크린샷 비교가 있으면 영향.

## 5. net 요청 (ui-sol)
1. 이번 기능에 필요한 net 변경은 없다. bootstrap은 `fetch` 한 번이며 net 연결 전에 끝난다.
2. 인증 실패의 종료 사유: 셸은 `closed`의 `reason`에 `auth|401|unauthor`가 있으면 need-auth로 보낸다. 쿠키 없는 WS 업그레이드 거부(HTTP 401/403)를 net이 이 문자열로 알려 주는지 확인 부탁 — 재연결 루프 대신 `closed{reason:'unauthorized'}`로 멈추는 것이 맞다.

## 6. 남은 것
- 추가 패널 → M2 도킹의 탭 패널.
- 채널 칩 클릭 외에 "닫힌 채널 숨기기"는 하지 않음(숨은 채널 방지 원칙상 계속 보임). 상한 64를 넘으면 "+N 표시 안 됨".
