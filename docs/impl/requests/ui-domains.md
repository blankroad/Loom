# UI → 도메인 팀 공통 요청(UART · CAN · UDS · ETH · EXPLORE/PSU) — 채널 열림·닫힘 표준 (2026-11-11)

docs/plan/02-core-principle.md 준수. UI 셸은 **에이전트가 연 채널을 사용자 화면 채널 바에 항상 올린다**(숨은 채널 방지, UI plan UD-31, ui/main 1c66c6c). 이를 위해 채널 열림·닫힘 이벤트가 필요하다.
1. 표준 제안: `<domain>.channel_state{state:'opened'|'closed', generation?, backend?, detail?}` (2토큰, `*_state`라 loom-proto 기준 우선 경로). UI는 이미 `uds.state{target, link:'up'|'down'}`, `eth.capture_state{state:'running'|'stopped'}`, `psu.state`도 받는다. UART·CAN이 `uart.channel_state`/`can.channel_state`를 내는지 확인 부탁.
2. 닫힘 단어(`closed`/`down`/`stopped`) — 이 집합 밖은 UI가 `unknown`으로 표시하고 채널 바에만 둔다.
3. `generation`은 envelope 필드(계약 02 §2.1)로 싣는 것을 기본으로 — UI는 payload에 있어도 읽는다.
