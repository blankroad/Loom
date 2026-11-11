docs/plan/02-core-principle.md 준수.

# UI P0 공통 지시 (ui-opus, ui-sol)

작성: ui-lead, 2026-11-10. 한국어로 보고.

## 읽을 것
1. `docs/impl/00-impl-playbook.md` (구현 규칙 — 특히 §3 역할, §4 품질, §5 운영)
2. `docs/plan/ui/plan.md` — **§0.4(v2.2 정합: F1 기본 모두 자동, E9, F2 eth)** 먼저, 그다음 자기 작업 관련 절
3. 작업 공간 코드: `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui/` — 특히 `src/proto/provisional.ts`(임시 wire 타입), `src/net/api.ts`(net ↔ store 경계 `NetClient`)

## 작업 공간·규칙
- **모든 코드 작업은 worktree `/Users/ctmctm/workspace/Loom/.worktrees/ui`** (브랜치 `ui/main`). 저장소 루트 체크아웃에서 코드 수정 금지.
- 패키지 관리자 pnpm(`cd ui && pnpm ...`). **`package.json`·`pnpm-lock.yaml`·`pnpm-workspace.yaml`·설정 파일(`tsconfig*`, `vite.config.ts`, `vitest.config.ts`, `eslint.config.js`, `playwright.config.ts`)과 `src/proto/**`, `src/net/api.ts`는 리드 소유** — 바꿔야 하면 보고서에 이유와 함께 요청. 이미 설치된 의존성: react 19, @tanstack/react-virtual, @xterm/xterm 6 + addon-webgl, uplot, ajv, ws, tsx, vitest 5 + jsdom, playwright 1.65, eslint 10 + typescript-eslint.
- **커밋하지 않는다**(리드가 검토 후 커밋). git stash 금지.
- 완료 기준: `cd ui && pnpm check`(typecheck + lint + test) 통과 + 자기 범위 테스트 추가.
- 무한 큐 금지(C2) — 모든 버퍼에 상한과 넘칠 때 정책을 코드 주석으로.
- 장시간 실행 10분 이내. Playwright 브라우저는 `~/.cache/ms-playwright`(설정에 있음).
- **대화형 질문 창(선택지 질문, AskUserQuestion 등)을 띄우지 말 것**(플레이북 §6). 메인·리드는 다른 에이전트의 창에 응답할 수 없다. 결정이 필요하면 계약·계획에 맞는 쪽으로 스스로 정하고 보고서에 "결정: … (근거 …)"로 적거나, 판단할 수 없으면 보고서에 질문을 쓰고 작업을 끝낸다.
- 경로는 항상 `/Users/ctmctm/workspace/Loom/...` 또는 상대 경로(옛 Desktop 경로 금지 — 권한 창이 뜬다).
- 다른 에이전트에게 herdr 명령 금지. 끝나면 `docs/impl/ui/reports/p0-<이름>.md`(저장소 루트 경로)에 보고서를 쓰고 "완료: <경로>"만 답한다.

## 임시 프로토콜 (CORE `loom-proto` 태그 `core-p0-d2` 전까지)
- 타입은 `src/proto/provisional.ts`만 쓴다. CORE 계약이 나오면 리드가 생성 타입으로 교체한다 — 그래서 **wire 필드 이름을 코드 곳곳에 흩뿌리지 말고** 어댑터 한 곳에서 읽는다.
- 임시 이벤트 kind (mock·fixture·셸이 공유):
  - UART: `uart.data`(rx/tx, payload `{encoding:'base64', data}`), `uart.write`(request_id + write_state), `uart.input_owner`(payload `{owner: 'none'|'user'|'agent', viewer?, dirty, hold}`)
  - CAN: `can.frame`(payload `{id, ext, fd, dlc, data_hex}`), `can.tx`(request_id + write_state + 프레임 필드)
  - UDS: `uds.request`(request_id + write_state + `{target, sid, service, summary, req_hex, resp_hex?, nrc?, nrc_name?, pending_count, rtt_ms?}`)
  - CORE: `core.latch`(payload `{on, by, origin, acked, jobs_stopped:[], approvals_cancelled, uncancellable:[], outputs_held:[{domain, channel, label, state}]}`), `core.activity`(payload `{actor, client_id, verb, domain, channel?, range?:{seq_a, seq_b}, summary}`), `core.approval_requested`/`core.approval_resolved`(F1: 기본 꺼짐 — 정책 fixture에서만 나옴), `core.agent_cursor`(payload `{client_id, from_seq, to_seq, others_count}`), `core.worker_state`, `display.gap`/`record.gap`/`source.gap`(payload `GapPayload`)
  - 알림: `core.snapshot_part`, `core.snapshot_end`, `core.events`, `core.heartbeat`
  - RPC: `hello`, `core.subscribe`, `core.unsubscribe`, `core.time`, `core.viewer.presence`, `core.metrics.report`, `core.stop_all`, `core.stop.release`, `core.safe_state.list`, `core.safe_state.run{ids}`, `core.mark{label}`, `core.approval.respond`, `uart.input{channel, data_hex, input_seq}`, `uart.input.yield|clear|takeover{channel}`, `uart.send`, `can.tx.send{channel, id, ext, fd, data_hex}`, `uds.request{target, hex, dry_run?}`, `uds.lease.takeover{target}`
- 출처 색(E8): AGT 청록 · USR 노랑 · DEV 회색 · SCR 보라 · EXT 보라 점선, 항상 문자 병기.
