docs/plan/02-core-principle.md 준수.

# UI P0 보고 — ui-opus: 스토어 · 패널 SDK · 셸

작성: ui-opus, 2026-11-11. 작업 공간 `.worktrees/ui/ui` (브랜치 `ui/main`, **커밋 안 함**).
결과: **`pnpm check` 통과**(typecheck + lint + test 77개 — 내 테스트 51개 포함), `pnpm build` 통과(index 94.8 KB gzip). 리드 공지 `p0-proto-v1-update.md`(CORE 계약 v1)에 맞춰 수정 완료(§3).

## 1. 만든 파일 (전부 내 범위 안)

| 파일 | 내용 |
|---|---|
| `src/store/emitter.ts` | 버전 있는 emitter (모든 스토어의 바탕, `useSyncExternalStore`용) |
| `src/store/adapter.ts` | **wire 필드를 읽는 유일한 곳** — latch·activity·approval·worker_state·agent_cursor·gap(GapData/GapSummary)·heartbeat·viewers·safe_state list/run |
| `src/store/shellControl.ts` | ShellControlStore: 연결·hello(env·rights·viewers·latch)·latch·승인(서버가 보낼 때만)·record health·worker·채널·gap·오류. `computeLiveStatus` = LIVE/LAG/STALLED/LIVE-DEGRADED (heartbeat 1 s, 1.5 s 회색 점, 3 s STALLED → `writeLocked`), `hasRight`(RIGHT_RANK) |
| `src/store/rowStore.ts` | RowStore: `request_id` 키, `WRITE_STATE_RANK` 단조 갱신(같은 단계는 새 seq만 — UDS pending ×N), 첫 요청 시각 순 고정, 완료 행 상한 20,000(넘으면 95%까지 오래된 완료 행 퇴출 + `evicted`), 진행 중 행은 퇴출 없음·상한 10,000(넘으면 새 행 거절 + `rejected` 배너) |
| `src/store/activityStore.ts` | 링 5,000(덮어쓰기 + `dropped`), `core.agent_cursor`로 사용자 행 `⇢ 에이전트에게 전달됨` 판정 |
| `src/store/renderScheduler.ts` | rAF 루프(더러운 패널만, 없으면 잠듦), 프레임 데이터 예산 8 ms, 정상 rAF마다, 3프레임 연속 예산 초과 → 20 Hz 강등 + degraded 신호, 30번 연속 절반 이하 → 복구. 렌더 표본(stage) 링 1,024 |
| `src/store/stopController.ts` | STOP 상태기계: 낙관 "요청 중" → `core.latch` ack 시 "잠김" → 2 s 무응답 "확인 안 됨 — 재시도". WS 안 열림/RPC 실패 → `stopViaHttp` |
| `src/store/hooks.ts` | `useStoreVersion` |
| `src/store/testing/fakeNet.ts` | NetClient 테스트 대역 + 이벤트 빌더(계약 v1 모양) |
| `src/sdk/manifest.ts` | PanelManifest·TopicSpec·ActionSpec 타입 + `validateManifest`(id·키 중복·RPC 이름·rights·confirm·안전 방향에 확인 금지·메모리 ≤ 256 MiB·maxHz·budget) |
| `src/sdk/actionGate.ts` | ActionGate: `safetyDirection:'reduce'` = 모든 자격·확인 없음·STALLED에도 허용 / 그 외 rights 위계 검사(E_RIGHTS)·STALLED 잠금(E_STALLED)·confirm `enter_twice|card`(아니오 = E_CANCELLED, RPC 없음)·확인 뒤 재검사 |
| `src/sdk/storeMirror.ts` | StoreMirror(최근 N행 링, gap 표식 링 100, snapshotDone·headSeq·onAppend·project) |
| `src/sdk/context.ts` | PanelContext 구현 — 선언 안 된 topic·query·action은 `UndeclaredError`. 구독 이벤트를 RowStore로 흘림, act → WriteHandle(request_id로 행 추적), scheduler·clock·cursor·ui(reveal·openPanel·toast·mark) |
| `src/sdk/registry.ts`, `src/sdk/index.ts` | `registerPanel`(검증 후 등록, 중복 거부), 패널용 공개 SDK(출처 배지·쓰기 단계 칩 상수 포함) |
| `src/shell/services.ts` | NetClient → 스토어 배선(onState·onControl·activity 구독 1개·250 ms tick·가시성 보고), ToastStore, UiStateStore, `parseEvidenceRef`(TEST REF) |
| `src/shell/confirmHost.ts`, `ConfirmLayer.tsx` | 확인 카드(1 s 무장, 기본 포커스 취소, Esc 취소, enter_twice = 3 s 안 두 번째 Enter). 대기 상한 8(넘치면 즉시 거부) |
| `src/shell/TopBar.tsx` | env 배지(생략 금지)·LIVE/lag 배지·REC·보는 사람·에이전트 활동·승인 수(있을 때만)·**[■ STOP 전체]** |
| `src/shell/ChannelBar.tsx` | 채널 바 슬롯, 1 Hz 하트비트 시계 |
| `src/shell/Layout.tsx` | 프리셋 `m1-default`·`uart-only`·`can-uds` + 슬롯 최대화, 미등록 패널은 이름 자리표시, `PanelHost`(컨텍스트 생성·mount·ResizeObserver·dispose), 라우트 `#/l/<preset>`·`#/p/<domain>/<panel>?props`·`#/need-auth` |
| `src/shell/StopPanel.tsx` | STOP 상태 패널: 잠긴 것·취소 불가·출력 유지 + [비상 OFF]·복구 목록(`core.safe_state.list`) + [복구 송신 ⏎](`core.safe_state.run{ids}`, 항목별 결과·인터록 막힘), [해제…](operate + 확인 카드), 터미널 입력 중이 아니면 복구 버튼에 포커스 |
| `src/shell/ActivityStrip.tsx` | 하단 2줄 + 펼치기(50), 읽은 seq 범위, 전달됨 표식 |
| `src/shell/Banners.tsx` | 연결·STALLED·REC failed/degraded·worker 크래시·epoch·행 거절 배너, 토스트, **aria-live 하나**(승인·STOP·오류만) |
| `src/shell/Approvals.tsx` | 승인 대기열 + 일반 카드(서버가 `core.approval_requested`를 보낼 때만): 1 s 무장(승인만), **기본 포커스 거부**, Enter·Esc 비승인(Esc = 보류), 승인 = 클릭 또는 `Y`(자동 반복 무시), 코드 끝자리 일치, presence 필요 시 비활성(M2), `approval.display`는 텍스트 노드만으로 일반 렌더 |
| `src/shell/prefixKeys.ts` | `Ctrl-]` window capture: `x` STOP·`m` 마커·`A` 승인·`?` 도움말·`Ctrl-]` 두 번 = 리터럴(포커스 요소에 `loom:literal` 이벤트, detail.byte 0x1D). 2 s 타임아웃, 모디파이어만 누르면 대기 유지. `Ctrl-X` 미사용 |
| `src/shell/ShellApp.tsx`, `ShellContext.tsx`, `shell.css` | 셸 조립, `NeedAuth`(비밀 없음 안내), 도움말 |
| `src/panels/demo/demoPanel.ts`, `src/panels/index.ts` | 데모 패널 `demo.echo`: SDK만 import(I-UI1 lint 통과), 토픽 1개 행 표(`data-request-id`·`data-origin`, textContent만) + act 1개(마커). `#/p/demo/echo?domain=uart`로 연다 |
| `src/main.tsx` | `createNetClient('/rpc')` → `createShellServices` → `ShellApp` |
| 테스트 6개 | `store/rowStore.test.ts`, `store/shellControl.test.ts`, `store/stopController.test.ts`, `store/renderScheduler.test.ts`, `sdk/sdk.test.ts`, `shell/shell.test.tsx`(jsdom) |

C2: 모든 컬렉션에 상한과 넘칠 때 정책을 코드 주석으로 적었다(행·활동·승인 256·gap 50·worker 64·채널 64·오류 20·토스트 5·확인 대기 8·렌더 표본 1,024·mirror 기본 2,000/최대 20,000).

## 2. 시험

| 범위 | 내용 |
|---|---|
| RowStore | 단계 진행, 늦은 낮은 단계 무시, 같은 단계 새 seq 갱신·옛 seq 무시, 첫 시각 순 고정, 완료 행만 퇴출·카운트, 진행 중 상한 거절, 배치 알림 1회 |
| 상태 배지 | 1.5 s 경계 회색 점, 3 s 경계 STALLED + 쓰기 잠금, 연결 끊김 = STALLED, LAG(목표 초과·2 s 심각), 숨은 패널 무시, LIVE-DEGRADED, STALLED 우선 |
| ActionGate | operate 행동(operate·approve 통과, watch 거부·RPC 없음), 안전 방향(watch·확인 없음·STALLED 중 허용), STALLED 잠금, enter_twice/card 아니오 = 취소·RPC 없음 |
| SDK | manifest 검증 오류들, 미선언 topic/query/action throw, 구독 1회·mirror·RowStore 연결, WriteHandle 단계 추적, dispose 시 구독 닫힘 |
| STOP | 요청 중 → ack 시 잠김, 2 s 무응답 → 확인 안 됨 → 재시도, ack 없는 latch는 잠김 아님, WS 끊김 → HTTP, RPC 실패 → HTTP. DOM: 버튼 상태 전이·2 s 타임아웃·watch 비활성 |
| 승인 카드 | 기본 포커스 거부, 무장 전 `Y` 무시, 무장 후 `Y` 승인, Enter·Esc 비승인·Esc 보류, 자동 반복 무시, XSS 문자열이 텍스트로만, `data-request-id`·`data-origin`, 코드 불일치 차단 |
| 접두키 | x/m/A/?, `Ctrl-]` 두 번 = 리터럴, 모디파이어 유지, `Ctrl-X` 미반응·기본 동작 막지 않음, 접두키 자체는 삼킴 |
| v1 정합 | `missing` 없는 latch는 ack 아님, hello latch·viewers·단일 rights, producer_range만 있는 gap은 stream seq null·count "unknown" 유지 |

테스트가 실제 버그 1건을 잡아 고쳤다: 승인 카드의 거부 버튼을 무장 전 비활성으로 두어 기본 포커스가 가지 않았다 → 거부는 안전 방향이라 처음부터 활성.

## 3. 계약 v1 공지에 맞춘 곳
- `adapter.ts`: `GapData`(`seq_range{from,to}`, count 10진 문자열/"unknown", producer_range만이면 seq 지어내지 않음 — `producerOnly`), `GapSummary`(배치 `display_gap`), latch(`missing[]`·`persisted`·`approvals_cancelled[]`·`driver_uncancellable`), viewers `{subscribed, visible}`, 선택 필드 생략(null 아님) 처리.
- `shellControl.ts`: rights 단일 등급(`RIGHT_RANK`), hello의 `latch`·`viewers` 반영, 배치 `record_health` 제거(heartbeat로만).
- `storeMirror.ts`·`context.ts`·`services.ts`: `EventsBatch{sub, first, last, …}`, snapshot `events?`, 구독 `encoding: 'json'|'binary'`.
- STOP: **계약대로 operate 이상에서만 활성**, watch는 비활성 + 툴팁 "STOP은 operate 권한 필요(계약 v1 — CORE에 질의 중)". `Ctrl-] x`도 같은 규칙(토스트). hello 전(자격 모름)에는 활성 — 새 페이지에서 HTTP STOP을 막지 않기 위해.
- 테스트 대역(`fakeNet.ts`)을 v1 모양으로.

## 4. 임시 프로토콜에서 애매했던 점 (리드·CORE 확인 필요)

| # | 항목 | 내가 정한 것 | 확인 요청 |
|---|---|---|---|
| 1 | latch ack | v1에 `acked` 없음 → `missing`이 **있고 비어 있을 때만** ack. `missing` 생략이면 ack 아님(성공을 칠하지 않음) | CORE가 ack 완료 시 **`missing: []`를 생략하지 않고 보내는지**. 생략하면 STOP이 2 s 뒤 "확인 안 됨"으로 보인다 |
| 2 | hello `latch:true` | 이미 성립한 latch로 보고 "잠김" 표시(세부는 다음 `core.latch`/snapshot) | snapshot에 latch 리듀서 상태(잡·출력 유지)가 오는지 |
| 3 | `driver_uncancellable` 형 | 수(scalar)면 "드라이버에 넘어간 N건", 목록이면 그대로 | 타입 확정 |
| 4 | 안전 방향 행동(abort·비상 OFF)의 자격 | ActionGate는 계획대로 watch 허용. STOP만 계약대로 operate | CORE 답에 abort·비상 OFF도 포함해 달라 |
| 5 | `core.safe_state.list/run` 결과 모양 | `{items:[{id, domain, channel?, label, kind:'recover'|'emergency_off', blocked?}]}` / `{results:[{id, ok, blocked?, error?}]}` (adapter에 주석) | 계약·loom-proto에 없음 |
| 6 | 비상 OFF 경로 | `core.safe_state.list` 항목 중 `kind:'emergency_off'`이고 domain·channel이 출력 유지 장치와 같은 것을 `run{ids:[id]}` — 없으면 버튼 비활성 "경로 없음" | 비상 OFF를 safe_state에 넣을지, 도메인 RPC로 할지 |
| 7 | 승인 이벤트 payload | `approval_id, risk, requester, domain, channel, display, expires_wall_ns, presence_required, code_suffix` 가정 | F1로 기본 꺼짐이라 급하지 않음. 정책 fixture 만들 때 맞춤 |
| 8 | 승인 응답 | `core.approval.respond{approval_id, decision:'approve'|'deny'}` | scope 필드 |
| 9 | viewers 실시간 갱신 | hello 값 + (있으면) `core.viewers` 이벤트 payload를 같은 모양으로 | 실시간 이벤트가 계약에 없음 |
| 10 | `act()` 결과의 request_id | RPC 결과 `{request_id}` 가정, 없으면 WriteHandle.requestId = null | 쓰기 RPC 결과 공통 필드 |
| 11 | 패널 lag | P0은 배치 `lag_ms`(서버 측)를 패널 lag로 씀 | 브라우저 쪽 "미렌더 가장 오래된 이벤트 나이"는 Worker 링·시계가 붙은 뒤(P1) |
| 12 | 쿠키 없음 → `#/need-auth` | 연결이 `closed`이고 reason에 auth/401/unauthor가 있으면 이동 | net이 인증 실패를 어떤 reason으로 주는지 |

## 5. 리드에게 요청 (설정·의존성)
- 의존성·설정 변경 요청 **없음**. `@testing-library`는 쓰지 않고 React 19 `act` + `createRoot`로 DOM 시험을 했다.
- 구독 상한(viewer 연결당 ≤ 8): 셸은 `core.activity` 구독 **1개**만 쓴다. 패널 구독은 같은 (filter, options)이면 net이 refcount로 합친다고 api.ts에 적혀 있어 SDK는 패널마다 구독을 따로 만들지 않고 net에 맡겼다. P1에서 패널이 늘면 "같은 domain·channel 합치기"를 SDK 쪽 토픽 정규화(필터 합집합 + 클라이언트 필터)로 넣어야 할 수 있다 — 결정 필요.
- `core.activity`를 제어 경로(`CONTROL_KIND_PREFIXES`)에 넣으면 셸의 구독 1개도 아낄 수 있다(빈도가 1 Hz 집계라 부담 작음) — proto 소유자 판단.

## 6. ui-sol net과 붙일 때 확인할 점
1. `core.heartbeat`: net이 heartbeat 알림을 control 이벤트(`kind:'core.heartbeat'`, payload = Heartbeat 필드)로 합성해 `onControl`로 준다 — 내 `readHeartbeat`가 `record_health`·`latch`를 그 payload에서 읽는다(소스로 확인, `wire.ts heartbeatEvent`). **이게 끊기면 셸은 3 s 뒤 STALLED + 쓰기 잠금**이 되므로 E2E에서 첫 확인 대상.
2. `onControl`로 오는 쓰기 최종 단계 이벤트도 RowStore에 넣는다. 같은 이벤트가 데이터 구독 배치로도 오면 RowStore가 같은 seq를 무시하므로 중복 표시는 없다(단조 + seq 비교).
3. `ConnState.open.hello`의 `rights`(단일 값)·`viewers`·`latch`를 셸이 그대로 쓴다.
4. `subscribe` 핸들러: `onSnapshotPart.events`가 생략될 수 있음을 처리했다. `onResync`가 오면 mirror에 resync gap을 넣고 snapshot 대기 상태로 돌린다.
5. `stopViaHttp`는 WS가 `open`이 아니거나 `core.stop_all` RPC가 실패할 때 부른다. RPC 타임아웃은 2 s로 넘긴다(`{timeoutMs: 2000}`).
6. `setVisible`은 `visibilitychange`마다 부른다.
7. 접두키 리터럴 0x1D는 포커스 요소에 `loom:literal` CustomEvent(bubbles, `detail.byte = 0x1d`)로 보낸다 — P1 터미널 패널이 이 이벤트를 받아 `uart.input`으로 보내야 한다.

## 7. 남은 것 (P1 이후)
- 실제 UART/CAN/UDS 패널(데모만 있음), Worker 링 projection과 `StoreMirror.project` 연결, 브라우저 쪽 lag 계산, `core.metrics.report` 렌더 보고 송신(표본은 스케줄러에 쌓이고 있음).
- 도킹·레이아웃 저장(M2), 가리키기 토스트(`ui.focus_request`), R3/R4·존재 확인(정책으로 켰을 때만, M2).
- Playwright E2E(ui-sol 하네스 위에서 셸 DOM 확인).
