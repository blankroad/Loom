# can-opus M1 — 채널 열기·가짜 ECU 노드·`loom can demo` (M1 걷는 뼈대의 첫 단계)

공통 규칙 `docs/impl/can/tasks/p0-common.md`(질문 금지, 커밋 안 함). **범위:** `src/gateway.rs`, `src/cli.rs`, `tests/gateway.rs`, `tests/cli.rs`, `skills/loom-can/SKILL.md`, 보고서 `docs/impl/can/reports/m1-open-demo.md`. `worker.rs`는 건드리지 않는다(worker 쪽 call은 이미 있음: `CALL_OPEN`/`CALL_CLOSE`/`CALL_NODE_*` 상수가 `worker.rs`에 pub). can-sol은 `tests/load.rs`를 쓰는 중.

M1(plan §11): 에이전트가 사람 손 없이 sim 채널을 열고, 가짜 ECU가 응답하고, 사용자는 UI 링크로 같은 화면을 본다.
1. **RPC 등록**: `can.open{channel, bus?, nodes?: "default"|"none"|<toml 문자열>}`, `can.close{channel}`, `can.node.list/start/stop` → worker call 전달. 등급: open/close/node 목록 = R0, node start/stop(sim 가짜 ECU 송신 시작·정지) = R1 "sim node"(F1 자동, 표시만). 채널 표(reducer)는 `can.channel_state` 이벤트로 갱신되므로 open 직후 `can.send`의 채널 자동 결정이 되게(필요하면 open 결과로도 갱신).
2. **`loom can demo [-c ch0]`** (CLI만, 새 RPC 없음): `can.open{channel, nodes:"default"}`(이미 열려 있으면 그대로) → 한 줄 요약 + **UI 링크**(`loom ui open can/trace?ch=ch0` — 그 명령이 없으면 링크 문자열만 출력) + 다음에 해 볼 명령 2개(`loom can send 300#01 --expect 0x301`, `loom can trace --since @last`). 사람용·`--json` 둘 다.
3. CLI: `loom can open <ch> [--nodes default|none|FILE]`(FILE이면 내용을 읽어 toml 문자열로), `close <ch>`, `node list|start|stop`.
4. 스킬: Workflow 1단계에 "채널이 없으면 `loom can demo`(sim) 또는 `loom can open`" 한 줄.
5. 시험(가짜 worker): 등록·등급, 인자 검사, demo 흐름(열기 → 링크 → 이미 열림), CLI 변환·출력 golden.
끝나면 보고서를 쓰고 멈춘다.
