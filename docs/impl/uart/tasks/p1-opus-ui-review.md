docs/plan/02-core-principle.md 준수.
질문 창 금지: 대화형 질문을 띄우지 말고, 스스로 정해 보고서에 적거나 보고서에 질문을 쓰고 끝낸다(playbook §6, p0-common.md).

# P1 uart-opus: UI UART 패널 구현 검토 (T11w)

공통 규칙: `docs/impl/uart/tasks/p0-common.md`. 이번 작업은 **코드 수정 없음**(읽기 + 문서). `ui/`는 UI 팀 소유 — 고치지 말고 요청으로 쓴다.
보고서: `/Users/ctmctm/workspace/Loom/docs/impl/uart/reports/p1-opus-ui-review.md`.

## 배경
- 리드가 UI 질문 6개에 답했다: `/Users/ctmctm/workspace/Loom/docs/impl/requests/ui-uart.md` 끝 `## uart 답변`.
- 이벤트 kind·payload 정본: worktree `crates/loom-uart/src/kinds.rs`(리드 커밋). 명령·역할: `crates/loom-uart/src/commands.rs`.
- 네 정제기·InputArbiter: `crates/loom-console/src/{sanitize,arbiter}.rs`, 골든 `crates/loom-console/tests/golden/sanitize/`.
- UI 패널 구현: worktree `ui/src/panels/uart/`(UI 팀이 mock 위에서 먼저 만듦, 커밋 c1ed979 이후), 공통 `ui/src/**/TerminalView*`, mock 데몬·fixture.

## 할 일
1. **계약 대조**: UI 패널이 읽는 kind·필드(`uartState.ts` 등)와 `kinds.rs`·답변을 표로 대조 — 이름·타입·의미가 다른 곳, UI가 아직 안 쓰는 필드(`viewer`=conn_id 비교, `input_ack`, `final`, `uart.write.bytes` vs `uart.data dir=tx`).
2. **자동 응답 3중 방어(plan §5.5) 구현 상태**: ① 정제기(WASM 또는 TS 포팅 + 골든) 적용 위치, ② xterm 파서 핸들러 등록 목록, ③ 사용자 의도 게이트. 각각 있음/없음/부분 + 파일·줄. 없는 것은 M1 수용 (f2) 위험으로 표시.
3. **입력 경로(§5.3)**: onData 1회 = `uart.input` 1회, Ctrl-C 우선, 미확인 4 KiB·64항목 + Ctrl-C 64 B 상한, ack 200 ms 지연 표시, 붙여넣기 확인, IME.
4. **상태줄(§5.7)·gap 표시·다중 탭 입력권(§5.8)**: 빠진 항목.
5. **패널 API 표면 검사(M1 (h))**: UI 패널 코드에 장치 열기 경로(WebSerial 등)가 0인지.
6. 결과로 **UI 팀에 보낼 요청 초안**을 보고서 끝 `## ui-uart 추가 요청 초안`에 쓴다(리드가 검토 후 `requests/ui-uart.md`에 붙인다). 우리 쪽(`kinds.rs`·arbiter) 고칠 점이 나오면 따로 `## UART 쪽 수정 제안`.

## 끝나면
보고서 첫 줄 `DONE`. 대기.
