docs/plan/02-core-principle.md 준수.

# UI P0 — ui-sol: 네트워크 계층 · mock 데몬 · fixture · E2E 하네스

먼저 `docs/impl/ui/tasks/p0-common.md`. 계획 근거: `docs/plan/ui/plan.md` §3.3–3.6, §6.2, §6.5, §9.1–9.3, 당신의 `spikes/ui/sol/`.

## 당신의 파일 (이 밖은 수정 금지)
`ui/src/net/**`(단 `api.ts`는 읽기만), `ui/tools/mock/**`, `ui/fixtures/**`, `ui/e2e/**`

## 만들 것
1. **Net 계층** (`src/net/`) — `NetClient`(api.ts) 구현.
   - Web Worker(`net.worker.ts`)가 WS·디코드를 소유, 메인 스레드 프록시 `createNetClient(url)`(`src/net/index.ts`)가 `NetClient`를 구현. 핵심 로직은 Worker 없이도 시험 가능한 순수 모듈로 분리(rpc, mux, clock, classifier, budget).
   - RpcClient: id 매칭, 요청 64 KiB 상한 검사, timeout, **mutation 자동 재시도 금지**, 연결 끊김 시 진행 중 요청은 명시 오류.
   - hello → role/rights/env. 재연결 지수 백오프 0.5→8 s, 재연결 후 각 구독을 `after_cursor`+`stream_epoch`로 재개, 실패(epoch 변경 등)면 `onResync`.
   - SubscriptionMux: 같은 (filter, options)는 서버 구독 1개 + refcount. snapshot_part → snapshot_end → live 순서 보장, live는 `h` 이후만.
   - 분류: `isControlEvent`면 `onControl`로 **즉시 단건** 전달(배치 대기 없음). 데이터는 구독별로 프레임당 최대 1회 묶어 전달.
   - 디코드 대기 예산 8 MiB / 128 배치(먼저 도달) — 넘치면 해당 구독 데이터 배치를 버리고 **로컬 `display.gap{cause:"client_decode_budget"}` 합성 이벤트**를 그 구독에 전달(조용한 드롭 금지). 제어 이벤트는 예산 밖.
   - ClockSync: `core.time` 연결 직후 8회, 이후 5 s 주기·가시성 복귀·재연결 시. theta = ((d1−b0)+(d2−b3))/2, delta = (b3−b0)−(d2−d1), 최근 60 s 최소 delta 채택, clock_id/epoch 변경 시 폐기. BigInt로 ns 계산.
   - `setVisible` → `core.viewer.presence`, `stopViaHttp` → `POST /api/v1/stop`.
2. **mock 데몬** (`tools/mock/server.ts`, `pnpm mock`, 포트 7421, `/rpc` WS, `/healthz`, `POST /api/v1/stop`) — 임시 프로토콜로 말하는 가짜 `loomd`. CORE `loomd --fixture-replay`가 나오면 대체되므로 **작고 단순하게**.
   - hello(rights는 쿼리 `?rights=watch|operate`로 시험 가능하게, 기본 operate), core.subscribe(snapshot 분할 + live), 적응형 배치(≤ 40 ms / 200건 / 64 KiB, `max_batch_ms`, 조용한 뒤 첫 이벤트 즉시), heartbeat 1 s, core.time(d1/d2).
   - 시나리오: `fixtures/*.jsonl` 재생 — UART bootshell(에이전트 send → 에코·응답), CAN(0x300 → sim BCM 0x301 응답, 배경 프레임), UDS(read-did F190, 0x78 pending ×3 후 응답). 에이전트 동작은 mock이 스크립트로 흉내(타이머) + **mock 제어 RPC `mock.agent{...}`로 시험이 트리거**.
   - 쓰기 처리: `uart.input`, `can.tx.send`, `uds.request`, `uart.send` → origin=user(viewer 연결) 또는 agent(mock.agent) 이벤트를 `requested→accepted→sent→confirmed` 순으로 발행(publish-before-reply), STOP latch 중 agent 쓰기는 거부 + 이벤트.
   - `core.stop_all` → `core.latch{on:true,…, outputs_held:[psu 예시]}`, `core.stop.release`, `core.safe_state.list/run`, `core.mark`.
   - 결함 주입 `mock.inject{kind}`: `display_gap`, `record_gap`, `source_gap`, `stall_heartbeat{ms}`, `disconnect`, `flood_can{fps, seconds}`(≤ 10k fps), `flood_uart{bytes_per_s, seconds}`, `policy_confirm`(F1 기본 꺼짐인 승인을 켜서 `core.approval_requested` 발행).
3. **fixture** (`fixtures/`): 위 시나리오 JSONL + 정상·gap·`outcome_unknown`·늦은 합류(이벤트 1,000개 후 snapshot) 케이스.
4. **E2E 하네스** (`e2e/`): Playwright 브라우저 설치(`~/.cache/ms-playwright`, 프로젝트 밖) 후 스모크 1개 — 페이지 로드 → mock 연결. 셸 DOM은 ui-opus가 만드는 중이므로 `data-testid="loom-shell"` 존재 + `window.__loomNet`(dev 전용 노출, 당신이 index.ts에서 `import.meta.env.DEV`일 때만 설정) 상태가 `open`인지로 확인. 셸이 들어오면 리드가 시험을 확장.
5. 테스트: net 순수 모듈 단위 시험(순서·refcount·resync·예산 gap·시계 수식·재시도 금지), mock 시나리오 시험(Node에서 ws 클라이언트로 E-UI-1 흐름: subscribe → mock.agent uart send → 이벤트 순서·publish-before-reply).

## 보고서에 넣을 것
만든 파일, 시험 결과, 임시 프로토콜에서 애매했던 점(CORE 계약에 물어볼 것), 리드에게 요청할 설정·의존성 변경.
