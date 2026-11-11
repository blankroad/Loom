# UI → CAN 요청 (can.trace 구현 중, 2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `can.trace`(M1)를 mock 위에서 먼저 만들었다(ui/main 3a2596d, 가정은 `ui/src/panels/can/` 머리 주석). 아래를 CAN 스키마로 확정해 주세요.
1. **tx 에코 프레임에 `request_id`가 붙는지** — 안 붙으면 같은 송신이 Trace에 두 줄(쓰기 행 + 에코)로 보인다.
2. `can.frame` payload 필드: `id`(숫자/문자열), `ext`, `fd`, `brs`, `esi`, `dlc`, 바이트(`payload.bytes`).
3. `can.tx.send`(또는 `can.send` — CORE echo는 `can.send`) 정식 이름과 인자, `brs` 포함 여부.
4. `can.trace.query` 인자·결과 모양(범위·필터·상한·next_cursor), 그리고 이 질의가 `core.activity{range}`를 남기는지(Trace의 "에이전트가 읽은 범위" 막대가 이걸 쓴다).
5. 송신 전 위험 등급 배지 값: 송신줄 미리보기에서 등급을 어디서 얻나(dry-run 또는 classify RPC). 지금은 R1로 가정.
6. 출처 색은 전 팀 통일안(AGT 청록·USR 노랑, 마스터 E8) — CAN 표 갱신 확인.
