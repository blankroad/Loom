docs/plan/02-core-principle.md 준수.

# UI P1 M1 보고 — ui-opus: 수용 시험 결함 수정 (U-1–U-5, T08, C-6, C-8)

작성: ui-opus, 2026-11-11. 기준 ui/main `3e0f2ab`, 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui` (커밋 안 함). 수정은 `ui/src/**`(net·proto 제외)만. 시험·mock 파일은 손대지 않음.
근거: `docs/impl/ui/reports/p1-m1-accept.md` §3–§5.

## 0. 결과
- **`pnpm check` 통과**: check:schema ok · typecheck 0 · lint 0 · **31개 파일 325개 시험**(이번 추가 25개).
- **`pnpm e2e:mock --workers=1`**(3.0분): **41 통과 · 3 실패 · 1 건너뜀**(T07 CSP는 `test.fail` 예상 실패라 통과로 셈).
  - 이번에 고쳐진 것: **eth 수용 2**(U-1, 이전 실패 → 통과).
  - 남은 실패 3건은 모두 **시험·mock 쪽** — 원인과 근거는 §2. 요약: T02·T08 = mock 배경 시나리오의 에이전트 `uart.send`(15 s마다)가 시험 창에 끼어듦(시나리오를 끈 mock에서 두 시험 각 4회 = **8/8 통과**로 확인). T05 STALLED = U-3대로 입력칸이 이제 **비활성**이라 시험의 "Enter 후 거절 문구" 기대가 낡음.

## 1. 고친 것

| # | 내용 | 파일 | 시험 |
|---|---|---|---|
| **U-1** | 원인: 컴포넌트 규칙(`.vt-row{display:grid}` 등)이 브라우저 기본 `[hidden]{display:none}`보다 우선해 VirtualTable 풀의 숨긴 행(옛 `data-*`·문구)이 화면에 남음. **전역 `[hidden]{display:none !important}`** — VirtualTable·CAN 행/셀·배너 등 `hidden`을 쓰는 모든 곳에 적용. 패널 STOP 줄의 개별 규칙은 지움 | `src/shell/shell.css` | `src/shell/hiddenCss.test.ts`(3): 규칙 존재 + `!important` display로 되살리는 규칙 없음. **jsdom은 작성자 CSS와 무관하게 `[hidden]`을 숨겨 이 결함을 재현하지 못한다**(임시 시험으로 확인) — 그래서 스타일시트 내용을 고정하고, 실제 브라우저 확인은 e2e eth 수용 2(이제 통과). vitest가 CSS를 `?raw`로도 비워 주므로 파일을 Node로 읽음 |
| **T08** | UI 결함 아님(§2-1). 시험이 원하는 카드를 고를 수 있게 승인 카드에 `data-approval-id`·`data-domain`·`data-channel` 추가 | `src/shell/Approvals.tsx` | — |
| **U-2** | UART 상태줄 `[data-testid=uart-status]`에 마지막 송신의 `data-request-id`·`data-origin`·`data-state`. 흔들림 원인 1: 같은 요청의 tx 바이트(`uart.data dir:tx`)가 쓰기 행 뒤에 오면 단계가 지워졌음 → 같은 request_id면 단계·시각 유지. 원인 2(주 원인)는 mock 배경 시나리오(§2-1) | `src/panels/uart/terminalPanel.ts` | `uart.test.ts`(+1, 기존 1 보강) |
| **U-3** | **`ctx.writeLocked`**(셸 쓰기 잠금 = STALLED·미연결, 읽기 전용; 바뀌면 패널 다시 그림). CAN 송신줄·버튼 비활성 + 자리표시 `연결 STALLED — 쓰기 잠김`. 같은 규칙: UDS 명령줄·폴링·제어 가져오기(중단은 계속 활성), UART 터미널 읽기 전용 + 상태줄 `쓰기 잠김(STALLED)`, eth 쓰기 컨트롤, PSU 출력·설정(비상 OFF는 계속 활성) | `src/sdk/context.ts`, `panels/{can/canTrace,uds/consolePanel,uart/terminalPanel,eth/ethPanel,psu/gaugePanel}.ts` | `canTrace.test.ts`(+1: STALLED → 비활성 → heartbeat → 활성), `uds.test.ts`(+1) |
| **U-4** | VirtualTable: 스크롤이 맨 아래에서 벗어나도 **사용자 입력(휠·스크롤 키·포인터 누름/드래그·터치) 뒤 800 ms 안이거나 드래그 중일 때만** 스크롤백(`onScrollAway`). 레이아웃 변화(STOP 패널 열림·크기 변경)로 생긴 스크롤은 계속 꼬리를 따라감. 맨 아래로 오면 다시 따라감(이전과 같음) | `src/sdk/virtualTable.ts` | `src/sdk/virtualTable.test.ts`(3): 보기 높이 축소 → 따라감 유지·콜백 0, 휠/PageUp → 스크롤백, 드래그는 오래 걸려도 사용자, 입력 창 지난 뒤 스크롤은 따라감 |
| **7 (C-6)** | `core.activity` 어댑터를 **schema/v1 정본** `{client_id, method, summary, range{from,to}, waiting?}` 우선으로. CAN 답 `{action, domain, channel, range{seq_a,seq_b}}`(숫자 허용)·옛 `{actor, verb}`도 받음. `method`가 있으면 그것이 동사, domain은 `method` 접두로 보충, 범위는 작은 쪽부터, `waiting`은 `술어 · 기한` 문구(활동 줄 ⏳). CAN Trace "에이전트가 읽은 범위" 막대도 같은 규칙으로 | `src/store/adapter.ts`, `src/shell/ActivityStrip.tsx`, `src/panels/can/traceModel.ts` | `src/store/schemaAdapters.test.tsx` 4개(생성 타입 `ActivityPayload`로 만든 payload) |
| **8 (C-8)** | `core.safe_state.list`: **맨 배열**(replay) 또는 `{items}`, 항목 `description`→표시 문구, `method` 보관, `blocked:bool`(true면 `reason` 또는 "인터록") 또는 옛 문자열. `kind`는 schema에 없으므로: 명시 `kind` 우선 → 없으면 id·method·description에 `emergency`/`비상`이 있으면 비상 OFF 경로, 나머지는 복구 항목. 출력 유지 장치에 비상 OFF 항목이 없으면 버튼 비활성 + **"(경로 없음)"**(명령을 추측하지 않음). `run`: `{items[{outcome, reason, finally_blocked, request_id}]}` 우선, 옛 `{results[{ok}]}`도. 화면: `✓ 보냄`/`– 건너뜀: 사유`/`✗ 실패`, `finally 막힘`, `data-outcome`·`data-request-id` | `src/store/adapter.ts`, `src/shell/StopPanel.tsx` | 같은 파일 4개(생성 타입 `SafeStateItem`·`SafeStateRunResult`로 만든 값 + replay 모양으로 STOP 패널 렌더·복구 송신) |

## 2. 남은 e2e 실패 — 시험·mock 쪽 (시험 파일 수정 금지라 근거만)

1. **T08 `t08-rights.spec.ts:35` R2 카드**(전체 실행 실패, 단독 8회 중 2회 실패) — 남은 카드는 시험이 승인한 `can.send`가 아니라 **`ap-5 · uart.write 승인 필요`**(실패 시 페이지 스냅샷: "승인 대기열 (1)", "승인 요청 2건"). 원인: mock 기본 시나리오 `fixtures/scenarios/uart-bootshell.jsonl`이 에이전트 `uart.send`를 **3 s부터 30 s마다 + 18 s부터 30 s마다**(= 15 s 간격) 보내고, `policy_confirm`이 켜진 몇 초 사이에 그것이 걸리면 두 번째 승인이 정당하게 생긴다. UI는 실제 대기 승인을 보여 준 것.
2. **T02 `t02-agent-rows.spec.ts:47` UART 상태줄** — 기대 `AGT ▶ "uname -a⏎"`, 실제 `AGT ▶ "dmesg | tail -2⏎"`(같은 시나리오의 18 s 줄). 상태줄은 "마지막 송신"이므로 배경 송신이 덮는다.
   - 근거: `LOOM_MOCK_SCENARIOS=none`으로 띄운 mock에서 T02·T08을 `--repeat-each=4` → **8/8 통과**(`data-request-id` 비교 포함).
   - 제안(리드): ① 이 두 시험은 시나리오 없는 mock(또는 `mock.inject` 로 시나리오 일시정지 — 지금은 없음)에서, ② T08은 `page.locator('[data-testid=approval-card][data-domain=can]')`(이번에 추가한 속성)로 대상 카드를 고르고 "카드 0개" 대신 "이 approval_id 카드 사라짐"을 확인, ③ T02는 텍스트 대신 `data-request-id === r.request_id`가 될 때까지 기다리기(배경 송신이 끼면 여전히 흔들리므로 ①이 필요).
3. **T05 `t05-honesty.spec.ts:48` STALLED** — 기대 `can-send-msg` ~ `/STALLED|잠금|locked/`, 실제 `USR ▶ 0x300#07 → r-46 (requested)`. 원인: U-3으로 STALLED 중 입력칸이 **비활성**이 되어 Playwright `fill`이 활성화(STALLED 해제)까지 기다린 뒤 정상 송신했다. 시험 주석 자체가 "requested: disable it (report §5)". 제안: `await expect(input).toBeDisabled()` + `sends()` 불변 확인으로 바꾸고 fill/press/문구 확인은 지움.

그 밖: T07 CSP는 `test.fail`(CORE P1 L-3) 그대로. 실행 중 `[WebServer]` 스택 출력(mock 소켓 write 오류)이 한 번 보였으나 시험 결과와 무관.

## 3. 결정

- **U-5 결정: CORE fixture-replay 연결도 자동화 브라우저 강등을 유지한다(예외 없음). 예외는 지금처럼 `hello.capabilities`에 `mock`이 있는 경우뿐.**
  근거: ① 계약 01 §1.3 "Playwright·자동화 브라우저 세션은 watch만(bootstrap 쿠키 발급 안 함)"과 V5는 **데몬 종류를 가리지 않는다**. ② `--fixture-replay`는 출하되는 `loomd` 바이너리의 실행 모드다 — 데몬이 스스로 내세우는 능력 문자열 하나로 안전 기능을 끄면 실제 데몬에서도 같은 문자열로 우회될 수 있다. mock은 UI 팀 개발 도구(`ui/tools/mock`, 출하 안 함)라 예외가 그 경계 안에 머문다. ③ 시험 공백은 이미 다른 경로로 메워진다: 사용자 쓰기의 브라우저 동작은 mock e2e, echo-write 행 모양은 Node `src/net/contract.test.ts`(replay 녹취 고정). 따라서 replay e2e는 보기·에이전트 쓰기·자격 거부만 브라우저로 검증한다.
- U-1을 `.vt-row[hidden]` 한 줄이 아니라 **전역** 규칙으로: 같은 종류의 결함이 CAN(`.ct-grid`)·배너·새 패널에서도 생길 수 있어서.
- U-4 입력 창 800 ms: 휠 관성 스크롤(macOS)이 보통 이 안에서 시작되고, 드래그는 누르는 동안 계속 사용자 스크롤로 본다.

## 4. 물을 것·요청
- **CORE**: `SafeStateItem`에 `kind`(recover / emergency_off) 또는 그에 준하는 구분이 없으면 UI는 문구 추측(`emergency`/`비상`)에 기댄다 — 필드 추가 요청(이미 accept 보고 C-8에 있음). `blocked:true`일 때 사유 필드(`reason`?) 이름 확정.
- **CORE**: `ActivityPayload.waiting`의 키(`predicate`, `deadline`?) 확정 — 지금은 두 키를 읽고 나머지는 "대기 중".
- **리드(mock)**: 시나리오 일시정지 inject(`scenarios {on:false}`) 또는 e2e mock 기동 시 `LOOM_MOCK_SCENARIOS`를 시험별로 고르는 방법.
- net 요청 없음.
