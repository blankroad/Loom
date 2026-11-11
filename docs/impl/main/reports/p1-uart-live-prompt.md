# 실제 브라우저 반례: UART 줄바꿈 없는 프롬프트

Opus foreground의 실제 Ego Lite 검증에서 부팅 DEV trace는 보이지만 `login:`이 표시되지 않았다. UART trace의 LineAssembler가 LF가 있는 행만 반환해, 실제 장치가 newline 없이 내보내는 login/password/shell 프롬프트가 UI에서 누락됐다.

메인 수정: assembler의 현재 partial을 즉시 한 RX 행으로 표시한다. 청크 추가 때 같은 행을 갱신하고 LF가 오면 그 행을 완결하므로 중복되지 않는다. 그 사이의 agent TX 행 순서는 보존한다. 기존 64KiB partial/20,000행+1,000축출 상한을 유지한다. source/record/display gap에서는 partial을 중단해 앞뒤 장치 바이트를 한 행으로 합치지 않는다.

검사: UART10 tests PASS (새 2개는 분할 login prompt→TX 개입→LF 완결의 stable row 및 같은batch gap전후 미결합), UI 전체387 PASS/schema/type/lint, build PASS. 로그 `/private/tmp/loom-uart-partial-{test,types,lint,build,ui-check}.log`. 실제 Ego UART 재검증은 Opus 진행 중이며 결과는 p1-opus-browser.md에 기록한다.

소유 ui/src/panels/uart/{tracePanel.ts,uart.test.ts}, 메인 직접 구현·자체 리뷰. Opus는 ui/e2e만 소유. 이 변경은 frontend만이며 통과한 native 30분 RSS binary의 알고리즘을 변경하지 않는다.
