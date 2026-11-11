docs/plan/02-core-principle.md 준수.

# UI P1 선행 — ui-opus: 공통 컴포넌트 + UDS 콘솔 + UART 터미널 패널

작성: ui-lead, 2026-11-11. 규칙은 `docs/impl/ui/tasks/p0-common.md`와 `p0-proto-v1-update.md` 그대로(커밋 안 함, 리드 소유 파일 수정 금지, `pnpm check` 통과). CORE gateway가 아직 없으므로 **fakeNet과 `tools/mock`(서브에이전트가 만드는 중) 위에서** 개발한다. mock이 아직 없으면 fakeNet으로 시험하고, mock 연동은 보고서에 남겨라.

## 근거 문서
- `docs/plan/ui/plan.md` §0.1(E-UI-1/2), §0.2(지연), §4.2, §5.1.2(UART), §5.1.4(UDS), §5.2(개입·입력 혼재·쓰기 단계 칩), §5.9, §6.5(메모리), §16.1(xterm 사실 확인)
- 내용 명세: `docs/plan/_xteam/uart-ui.md`(+ `## ui 의견`), `docs/plan/_xteam/uds-ui.md`(+ `## ui 의견`)

## 당신의 파일
`ui/src/sdk/**`(공통 컴포넌트 추가), `ui/src/panels/uart/**`, `ui/src/panels/uds/**`, `ui/src/panels/index.ts`, `ui/src/shell/**`(필요한 연결만), `ui/src/store/**`. **수정 금지**: `ui/src/net/**`, `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`, `ui/src/panels/can/**`, `ui/src/panels/eth/**`(서브에이전트 영역), 리드 소유 파일.

## 만들 것
1. **공통 RowTable**(sdk): `request_id` 키 RowStore를 그리는 가상화 표(`@tanstack/react-virtual` 또는 명령형 DOM 풀 — 선택 근거를 보고서에). 고정 행높이, 단계 칩(○◔◑●●●●?✗⌛?) + 위험 등급 배지(R0–R4, F1로 항상 표시) + 출처 배지(문자 병기), 행 위치 고정, 진행 중 행 경과 시간 표시, 행 펼침 상세(텍스트 노드만), 생략 행(`display.gap`) 표시, `data-request-id`·`data-origin`·`data-state`. UDS·CAN TX·DLT CTRL·TEST 단계가 재사용한다.
2. **공통 TerminalView**(sdk): xterm 6 + WebGL addon(실패·context lost 시 DOM fallback), **3중 방어 중 브라우저 쪽 2개**: parser hook으로 DA·DSR·DECRQSS·window report·**OSC 4/10/11/12 `?` 색 질의** 응답 삼킴, OSC 8 링크 비활성, clipboard addon 로드 금지 / **사용자 의도 게이트**(keydown·paste·composition 직후 짧은 창 안의 `onData`만 통과, 나머지 폐기 + 카운터). 접두키 리터럴은 셸이 보내는 `loom:literal` 이벤트로 받는다. Ctrl-C = 0x03(복사는 Ctrl-Shift-C/Cmd-C). 5 ms 묶음 송신, 0x03은 즉시. 수신·그리기 대기 2 MiB/채널 상한 — 넘치면 화면 생략 줄 + 파서·SGR 재동기화. gap 장식은 `registerDecoration`(제안 API: `allowProposedApi:true`)이 실패하면 gap 줄 삽입. readonly(watch) 모드 = 입력·자동 응답 송신 0.
3. **`uds.console` 패널** (M1, uds-ui 명세): 대상 바(대상별 연결·세션·보안·TP·P2, 제어 소유자 + [제어 가져오기 T] → `uds.lease.takeover`), 트랜잭션 표(RowTable, 결과 기호 ✓ ✗NRC ⏳ ⌛ ⌛? ⛔ late ok_suppressed, pending ×N 카운터와 경과/한도), 명령줄(hex 입력 → 150 ms 정지 시 `uds.request{dry_run:true}` 미리보기, Enter 송신 → `uds.request`, [중단 !], [폴링 정지]), **마지막 값 표**, TesterPresent 접기, 에이전트 확인 위치(`core.agent_cursor`). F1: 확인 카드는 서버가 요구할 때만.
4. **`uart.terminal` 패널** (M1, uart-ui 명세): TerminalView + 상단 상태(OPEN·baud·gen·env) + **입력권 칩**(`uart.input_owner`: 비어 있음/나/다른 뷰어/에이전트 잠금/에이전트 대기 "원문" n/m s/미완결 줄/HOLD/PAUSED) + 버튼(반환 `uart.input.yield`, 지우기 `.clear`, 가져오기 `.takeover`, HOLD) + 하단 상태줄(AGT ▶ 마지막 송신 + 단계, 에이전트 expect 대기, rx 속도, 차단 질의 수, REC) + 백로그/라이브 구분 장식 + [키 ▾] 제어키 팔레트(Ctrl-W/T/N, Break 등). 키 입력은 `uart.input{channel, data_hex, input_seq}`. `uart.trace`(최소 트레이스 모드: 시각·출처·내용, 링 20,000행)도 같은 폴더.
5. 레이아웃 `m1-default`에 두 패널 배치(CAN Trace 자리는 자리표시 유지 — 서브에이전트가 만든다).
6. 테스트: RowTable 갱신·고정 위치·gap 행, TerminalView 의도 게이트(키 없이 온 onData 폐기, 질의 응답 0건 — DA/DSR/OSC 11 `?` fixture), 0x03 즉시 송신, readonly 송신 0, UDS 행 `○→◔→◑ pending×1..5→●` 1행, 미리보기 디바운스, 입력권 칩 상태 전이.

## 보고
`docs/impl/ui/reports/p1-opus.md` — 만든 파일, 시험 결과, 명세에서 애매한 점(UART·UDS 팀에 물을 것), 리드 요청. 끝나면 "완료: <경로>"만.
