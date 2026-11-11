docs/plan/02-core-principle.md 준수.

# UI P1 선행 보고 — ui-opus: 공통 컴포넌트 + UDS 콘솔 + UART 터미널

작성: ui-opus, 2026-11-11. 작업 공간 `.worktrees/ui/ui` (커밋 안 함).
결과: **`pnpm check` 통과** — typecheck + lint + test **17개 파일 155개**(이번에 추가 41개: SDK 공통 16 · UDS 13 · UART 10 · 컨텍스트 2). `pnpm build` 통과. 레이아웃 `m1-default`에 UART 터미널·UDS 콘솔이 실제로 뜨고 CAN Trace 자리는 자리표시 그대로.

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/sdk/virtualTable.ts` (신규) | 프레임워크 없는 가상화 표: 고정 행높이, 행 요소 풀(보이는 행 + overscan만 그림), 꼬리 따라가기(맨 아래일 때만), 선택 행 상세 서랍(텍스트 노드만), 화살표·Esc 키, `role=grid` |
| `src/sdk/rowTable.ts` (신규) | **공통 RowTable** = request_id 행 프리셋: 고정 앞열(시각·출처 배지 문자 병기) + 도메인 열 + 고정 뒷열(**단계 칩** ○◔◑●●●●?✗⌛? + 진행 중 경과 초, **위험 등급 배지 R0–R4 항상 표시**(F1)). `display/record/source.gap` 표식을 seq 위치에 합성 행으로 끼움(`mergeGaps`). DOM `data-request-id`·`data-origin`·`data-state`·`data-gap` |
| `src/sdk/rowView.ts` (신규) | 패널용 읽기 전용 RowStore 뷰(도메인·채널·kind·조건 필터, 버전당 1회만 재계산) |
| `src/sdk/terminalView.ts` (신규) | **공통 TerminalView**(xterm 6 + WebGL addon, 실패·context lost 시 DOM). 브라우저 쪽 방어 ②③: parser hook으로 DA1/DA2/DA3·DSR/CPR·DECRQM·window ops·XTVERSION·DECRQSS·XTGETTCAP·**OSC 4/10/11/12 `?` 색 질의** 삼킴, OSC 8·52·0/1/2 무시, clipboard addon 미로드 / **사용자 의도 게이트**(keydown·paste·composition 뒤 100 ms 안의 onData만, 나머지 폐기+카운트). 5 ms 묶음, 0x03 즉시(대기분 먼저 flush). Ctrl-C = 0x03, 복사 Ctrl-Shift-C/Cmd-C. `loom:literal` 수신. 그리기 대기 **2 MiB/터미널** 상한 → 넘치면 화면에서 버리고 xterm이 따라잡으면 CAN(0x18)+SGR 리셋+반전 "화면 생략 N B" 줄. gap·백로그 구분선은 `registerDecoration`(allowProposedApi) → 실패 시 같은 방식의 줄 삽입. readonly = 키·리터럴·자동응답 송신 0. 4 KiB 초과 붙여넣기는 소유자에게 넘김(확인 카드). xterm이 열리지 않으면 예외 대신 패널 안에 이유를 표시 |
| `src/sdk/context.ts` | `ctx.rows(filter)`(자기 도메인만), `ctx.connId`, `ctx.recordHealth`, `ctx.agentCursor`. **query와 action에 같은 RPC가 선언되면 query는 `dry_run:true`일 때만 허용**(진짜 쓰기는 반드시 ActionGate) |
| `src/sdk/storeMirror.ts` | `after(seq)`(새 이벤트만, 뒤에서 스캔), `overwritten` 공개 |
| `src/sdk/index.ts` | 위 컴포넌트·헬퍼 공개 |
| `src/sdk/testing/harness.ts` (신규) | 패널을 FakeNet·실제 스토어·수동 rAF로 띄우는 시험 하네스(패널 시험이 I-UI1 import 규칙 안에 머물도록 sdk 안에 둠) |
| `src/store/rowStore.ts` | 행에 `firstWallMs`(표시 시각)·`firstSeenMs`/`stageSeenMs`(경과 카운터용 클라이언트 시계) 추가, 시계 주입 |
| `src/panels/uds/udsModel.ts`, `consolePanel.ts` (신규) | **`uds.console`** — 대상 바(링크·세션+확신도·보안·TP 카운터·P2/P2*·제어 소유자 + [제어 가져오기 T]), 트랜잭션 RowTable(대상·서비스·요약·결과 기호·p2/rtt 또는 경과/한도), TesterPresent 접기, **마지막 값 표**(LRU 256), 에이전트 확인 위치 `seq:N`, 명령줄(hex → 150 ms 뒤 dry-run 미리보기, Enter 송신, 미리보기 등급 R2+면 Enter 한 번 더), [중단 !](안전 방향), [폴링 정지], 단일 글자 키 `T`·`!`은 입력란 밖에서만 |
| `src/panels/uart/uartState.ts`, `terminalPanel.ts`, `tracePanel.ts` (신규) | **`uart.terminal`** — 상단(OPEN·baud·gen·env·**입력권 칩**·[반환][지우기][가져오기][HOLD][⏸][키 ▾]), TerminalView, 하단 상태줄(AGT ▶ 마지막 송신 + 단계 · 에이전트 expect 대기 · rx 속도 · 차단 질의/자동응답 수 · REC · 보기 전용). 키 → `uart.input{channel, data_hex, input_seq}`. watch면 터미널 readonly·버튼 비활성. **`uart.trace`** — 시각·출처·기호화 내용, 링 20,000행(1,000행 단위 퇴출 + 개수 표시), 줄 조립은 청크 경계·UTF-8 스트리밍, gap 뒤 반쯤 된 줄 버림. 터미널과 **같은 (filter, options)** 라 net이 서버 구독 하나를 공유(계약 v1 연결당 ≤ 8) |
| `src/panels/index.ts` | uart·uds 패널 등록 |
| `src/shell/Layout.tsx` | 패널 컨텍스트에 agent cursor 연결 |
| `src/shell/shell.css` | 표·단계·등급·결과·터미널·UDS 스타일 |
| 시험 | `src/sdk/p1.test.ts`, `src/panels/uds/uds.test.ts`, `src/panels/uart/uart.test.ts`, `src/sdk/sdk.test.ts`(+2) |

### RowTable을 명령형 DOM 풀로 만든 이유
패널은 `mount(el)`로 명령형 마운트되고 RenderScheduler의 프레임 콜백에서 그린다. `@tanstack/react-virtual`을 쓰면 패널마다 React 루트를 만들어 두 번째 렌더 루프와 매 데이터 프레임 reconcile 비용이 생긴다. 고정 높이 DOM 풀은 프레임당 일이 "보이는 행 수"뿐이고, S1 스파이크(28행 DOM viewport draw p95 ≈ 0.5 ms)와 같은 형태다. 셀 텍스트는 값이 바뀔 때만 다시 쓴다.

## 2. 시험 결과 (과제 6 항목 대응)

| 과제 요구 | 시험 | 결과 |
|---|---|---|
| RowTable 갱신·고정 위치·gap 행 | 같은 request_id는 DOM 1행·`data-state` 추종 / 앞 행이 갱신돼도 순서 고정 / 진행 중 경과 `◑보냄 3.1s`·R2·AGT / gap이 seq 사이에 `data-gap` 행 | 통과 |
| 의도 게이트 | 키 없이 온 onData 폐기·카운트, 의도 뒤에는 통과 | 통과 |
| 질의 응답 0건 | **대조 시험**: 훅 없는 xterm은 DA1·DA2·CPR·DECRQSS에 실제로 응답함을 먼저 확인 → TerminalView는 DA/DA2/DSR/DECRQSS/**OSC 11 `?`** 에 송신 0, 차단 카운트 ≥ 5 | 통과 |
| 0x03 즉시 | `a` 대기 중 0x03 → 타이머 없이 `6103` 한 번에 | 통과 |
| readonly 송신 0 | 키·리터럴 모두 0, readonlyDrops 증가 / UART 패널 watch: 팔레트 Ctrl-C도 0, 가져오기 비활성, "보기 전용" | 통과 |
| 그리기 상한 | 2 MiB 넘게 쓰면 droppedBytes > 0, pending ≤ 2 MiB, 이후 0으로 배수 | 통과 |
| UDS 1행 `○→◔→◑ pending×1..5→●` | 매 단계 DOM 행 1개, `pending ×i`, 마지막 `●확정`·`✓` | 통과 |
| 미리보기 디바운스 | 40 ms 간격 4타 → 150 ms 뒤 dry-run **1회**(`{target, hex:'22F190', dry_run:true}`), Enter → `uds.request{target, hex}` | 통과 |
| R2+ Enter 두 번 | 첫 Enter는 안내만, 두 번째에 송신 | 통과 |
| 입력권 칩 상태 전이 | 비어 있음 → 나 → 에이전트 대기 "reboot⏎" 3.2/10s → 나 → 비어 있음 / 다른 뷰어·미완결 줄·잠금 87s·HOLD·PAUSED 우선순위 | 통과 |
| 기타 | 대상 바(세션 ✓·TP ✓×4·제어 AGT), 마지막 값 `bcm:F190 VIN=…`, 상태줄 `AGT ▶ "uname -a⏎" ◑보냄`·`rx 11 B/s`, 팔레트 Ctrl-C → `uart.input{data_hex:'03', input_seq:1}`, LineAssembler 청크 경계, 기호화, dry-run 가드, rows() 도메인 제한, NRC 거짓 성공 방지, dry-run 행 숨김 | 통과 |

## 3. mock(`tools/mock`) 연동 — 소스로 확인한 차이 (브라우저 E2E는 아직 안 돌림)

| # | mock 동작 | 영향 | 조치 |
|---|---|---|---|
| M-1 | **dry-run 요청이 전체 파이프라인을 타고 `uds.request` 행을 발행**(`effect_unverified`, `dry_run:true`), 결과도 `{request_id, write_state}`뿐 | 입력할 때마다 표에 가짜 행이 쌓이고, 미리보기 문구(서비스·DID·등급)가 비게 됨 | 패널에서 `payload.dry_run===true` 행은 숨김(방어). **mock/계약 수정 요청**: uds-ui U3대로 dry-run은 행·감사 없이 `{bytes, service, did_name, risk, reason, needs, current, diff}` 반환 |
| M-2 | 지원 안 하는 SID에 **NRC 0x11을 `confirmed`로** 종결 | 결과 열이 ✓로 보이는 거짓 성공 | 결과 기호는 "최종 단계 + 0x78 아닌 NRC"면 ✗로 표시(방어, 시험 있음). **mock 수정 요청**: UDS 계획 §6.1대로 NRC = `failed` |
| M-3 | `uart.input_owner.viewer` = **client_id** | 내 칩은 `hello.conn_id`와 비교 → mock에서는 "나"가 "다른 뷰어"로 보임 | **결정 요청**: viewer 식별자를 conn_id로 할지 client_id로 할지. client_id면 hello 결과에 내 client_id를 돌려주거나 net이 노출해야 함 |
| M-4 | `uds.state` 미발행, `decoded` 없음 | 대상 바는 행에서 본 대상명만, 세션·보안 `?`; 마지막 값 표 비어 있음 | mock에 `uds.state`·`decoded` 추가 요청(우선순위 낮음) |
| M-5 | `uart.lock`·`uart.agent_activity`(deferred/expect)·`uart.state/config` 미발행 | 잠금·에이전트 대기·OPEN/baud 표시 없음 | mock에 추가 요청. 특히 **deferred send 대기**(E-UI-1 T3)는 핵심 시연 장면 |

## 4. 명세에서 애매한 점

### UART 팀에
1. `uart.input_owner`·`uart.hold`·`uart.lock`·`uart.agent_activity` payload 필드 확정(내 가정은 `uartState.ts` 머리 주석). 특히 에이전트 대기 원문·경과·한도, 잠금 사유·TTL.
2. viewer 식별자(M-3)와 다중 viewer writer 선점 규칙(이미 요청함).
3. `uart.write`에 송신 원문을 `bytes`로 넣는지 `text`로 넣는지(mock은 `bytes` + `summary`). 상태줄 표시는 둘 다 처리.
4. `uart.signal{kind:'break'}` 등 팔레트 동작의 RPC 이름·인자 확정.
5. 서버 정제기(`loom-console`)가 gap 뒤 SGR 리셋을 하는지 — 하면 브라우저 쪽 CAN+SGR 리셋은 중복이지만 무해.

### UDS 팀에
1. dry-run 결과 모양과 "행·감사 없음" 확인(M-1).
2. NRC 종결 = `failed`(M-2), timeout 표시 필드(`timeout:true`?), late·ok_suppressed·guard 거부 필드 이름(내 가정: `late`, `ok_suppressed`, `guard:'deny'`).
3. `uds.state` 필드(세션 확신도 `confirmed|assumed|stale`, lease `owner_origin/actor`) — lease actor가 사용자면 conn_id인지.
4. 명령줄 문법: M1은 hex만 지원(`read F190`·`session ext`는 "hex만 지원" 안내). 문법 확장은 서버 파서를 쓸지(dry-run에 텍스트를 넘겨 해석) 결정 요청.
5. R2+ "Enter 두 번"은 UI 실수 방지로만 넣었다(F1: 정책 확인은 서버가 요구할 때만 셸 카드). 사용자가 원치 않으면 끌 수 있게 할지.

## 5. 리드에게 요청
- **의존성 추가 요청(선택)**: `@xterm/addon-fit`(같은 xterm 6 계열 버전). 지금은 13 px 셀 크기로 cols/rows를 추정해 `resize`한다 — 폰트·DPR이 다르면 한두 칸 어긋날 수 있음.
- jsdom에 `matchMedia`가 없어 xterm `open()`이 실패한다. 시험 파일에서 각자 스텁했다(`p1.test.ts`, `uart.test.ts`). 공통 vitest setup 파일(`vitest.config.ts`의 `setupFiles`)로 옮기면 깔끔하다 — 설정 파일은 리드 소유라 요청만.
- CAN Trace(서브에이전트)가 RowTable을 TX 행에 재사용하려면 `createRowTable(host, {columns, detail, onNeedRender})` + `mergeGaps(ctx.rows({domain:'can'}).list(), mirror.gaps)`로 붙이면 된다.
- mock 수정 요청 M-1·M-2(거짓 성공·가짜 행 방지)는 우선순위 높음.

## 6. 남은 것
- 브라우저 E2E(mock 위 E-UI-1/2): 셸 + 두 패널을 Playwright로 열어 `data-request-id`·칩·상태줄 확인 — ui-sol/서브에이전트 하네스와 맞춰야 함.
- 패널 lag을 서버 `lag_ms` 대신 브라우저 쪽 "미렌더 가장 오래된 이벤트 나이"로(시계 정렬 연결 후).
- UDS M2 패널(DTC·세션·overrides·jobs·recover), UART 트레이스 hex·검색, Keyboard Lock(M2).
