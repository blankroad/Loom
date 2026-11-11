docs/plan/02-core-principle.md 준수.

# UI P1 M1 보고 — ui-opus: 브라우저 lag · 렌더 보고 · 뷰어 가시성 · SDK 보강

작성: ui-opus, 2026-11-11. 기준 ui/main `9e93ef2`(작업 중 HEAD `dd8b0b9`), 작업 공간 `.worktrees/ui/ui` (커밋 안 함).
근거: `docs/plan/ui/plan.md` §0.2·§3.5·§3.6·§5.4·§9.5, 생성 타입 `src/proto/gen/{metrics_report,viewer_presence,time_result}.ts`, `schema/v1/*.schema.json`.

## 0. 결과
- **내 범위 전부 통과**: `check:schema` ok, `typecheck` 0, `lint` 0, `build` ok. 새 시험 **27개**(`src/store/m1Lag.test.ts` 15 · `src/sdk/m1Sdk.test.ts` 8 · `src/shell/m1Shell.test.tsx` 4), 기존 시험 포함 내 영역 실패 0.
- **`pnpm check` 전체는 지금 빨강 — 내 파일 아님**: `src/net/mux.test.ts` 4건, `tools/mock/{fixtures,server}.test.ts` 3건(snapshot part 번호 0→1 기준 변경). 이 작업 공간에 다른 에이전트의 커밋 안 된 변경이 진행 중이다(`src/net/{wire,index}.ts`·`mux/core/budget.test.ts`·`testing/fixtures.ts`, `tools/mock/{hub,sim,gen-fixtures}.ts`, 새 `tools/mock/{eth,psu}.ts`). 작업 중 `tools/mock/eth.ts`의 타입 오류도 한때 보였다가 사라졌다. 그쪽 작업이 끝난 뒤 다시 돌려야 한다.

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/store/renderMetrics.ts` (신규) | **LagTracker**(패널별): 배치 수신 표식(첫·끝 seq, 첫 이벤트 `ts.mono_ns`·`clock_id`, 수신 시각, 개수) ≤ 256개(넘치면 최신 표식에 합침 — 가장 오래된 나이는 유지). draw가 돌려준 seq까지 표식 제거(seq가 없으면 draw 시작 전 수신분 제거). **lag = `mono_est(now) − ts.mono_ns`**(BigInt로 차를 먼저 구함, §3.5-4). 미결 없음 → lag 0 + `idleMs`(마지막 수신 이후). 시계 미동기이거나 이벤트 `clock_id`가 `core.time`의 것과 다르면(장치 HW 시계, §3.5-8) **수신 시점 기준 나이**로 대체하고 `basis:'recv'`로 표시 — CORE·WS 구간이 빠지므로 과소 보고, 과대 보고는 안 함. **LogHistogram**(0.05 ms~96 s, 옥타브당 8칸 ≈ 9 % 해상도, 고정 168칸). **RenderMetrics**(탭 하나): 패널 ≤ 64, 단계 `receive·decode·apply·draw·next_raf·ui`, 생략 수, 표본 모드 버퍼 ≤ 400/주기 |
| `src/store/viewerReport.ts` (신규) | **ViewerReporter**: 10 s마다 패널별 `core.metrics.report{kind:'ui.render'}`(≤ 16건/주기, 나머지는 `detail.reports_skipped`), 보고 뒤 히스토그램 초기화. 표본 모드면 `kind:'ui.render.sample'` 추가. `visibilitychange` → `net.setVisible(v)` 다음 `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}`. 숨김 동안 주기 보고 중단, 복귀 시 `returned{hiddenMs, events}` |
| `src/store/renderScheduler.ts` | `sink`(RenderMetrics)로 `draw`·`next_raf`(draw 끝 → 다음 프레임 시작, paint 대용) 기록 — 그린 다음 한 프레임을 더 돌려 next_raf를 잼(그 뒤 잠듦). **부하 규칙 일반화**: 패널이 `addWork(ms)`로 draw 밖 작업(ingest)을 알리면 1 s 창에서 draw+작업 > 300 ms/s → 강등, 복구는 싼 프레임 30회 **그리고** 부하 < 75 ms/s(CAN 패널이 갖고 있던 규칙을 그대로 옮김). 패널별 `onDegradedChange`, `invalidateAll()` |
| `src/store/shellControl.ts` | `setPanelFreshness`가 값 또는 **제공 함수**를 받음(250 ms tick마다 읽음 — 아무것도 안 와도 lag가 자라므로). `LiveStatus`에 `idleMs`·`clockUncertain`·`clockErrMs`·`serverLagMs`·`lagFromReceipt` 추가. **동기 불확실** = 보이는 패널 중 하나라도 시계 미동기, 또는 ±오차 > 보이는 패널 목표 중 최솟값. tick 알림은 화면 문구가 바뀔 때만(≤ 4 Hz) |
| `src/sdk/context.ts` | 배치 도착 시 `receive`(시계 기반)·`apply`(미러+RowStore 반영) 기록, LagTracker 수신; draw 감싸서 `ui` 단계(수신 → draw 끝)·렌더 seq. 패널 freshness를 제공 함수로 등록(**서버 `lag_ms` 사용 제거**, 툴팁용으로만 보관). **`ctx.scheduler.degraded`**(읽기 전용) + `onDegradedChange` + `reportWork(ms)` + `reportOmitted(n)`. **`ctx.latch`**{`on`,`by`,`origin`,`subscribe`}(on/by/origin이 바뀔 때만 호출, 패널 dispose 때 해제). deps에 `metrics`·`now`·`absNowMs`(없으면 개별 RenderMetrics) |
| `src/sdk/latchLine.ts` (신규) | `createLatchLine(ctx)` — `STOP — agent/script 쓰기 잠김(사용자 입력 가능)` 한 줄, latch 동안만 보임 |
| `src/panels/uart/terminalPanel.ts`, `uart/tracePanel.ts`, `uds/consolePanel.ts`, `can/canTrace.ts` | 루트 맨 위에 STOP 한 줄 |
| `src/panels/can/canTrace.ts` | **자체 강등 판단 제거**(overStreak·underStreak·selfDegraded·winWork·상수 4개) → `model.setDegraded(forceDegraded \|\| ctx.scheduler.degraded)`, ingest 시간은 매 프레임 `ctx.scheduler.reportWork`, 강등 변화 시 invalidate, 화면 생략 증가분은 `reportOmitted`로 렌더 보고에 |
| `src/sdk/storeMirror.ts` | 쓰이지 않게 된 `lastLagMs` 제거 |
| `src/shell/services.ts` | RenderMetrics·ViewerReporter 생성, scheduler sink 연결, 가시성 이벤트를 reporter로. 표본 모드: 옵션 `renderSample`, 없으면 `?render_sample=1` 또는 localStorage `loom.renderSample='1'` |
| `src/shell/Layout.tsx` | PanelHost가 `metrics`·`now`를 context에 전달 |
| `src/shell/TopBar.tsx` | 배지: `LIVE lag 37ms` / 미결 없으면 `LIVE idle 5.2s` / `· 동기 불확실`. 툴팁: 브라우저 lag(수신 시점 기준이면 그렇다고), 서버 배치 lag, 시계 ±오차 |
| `src/shell/Banners.tsx` | 복귀 줄 `숨김 중 12.3 s (4,210 이벤트) — 라이브로 이동`(누르면 닫고 모든 패널 다시 그림) |
| `src/sdk/testing/harness.ts` | 시험 시계를 context `now`·`absNowMs`로 전달 |
| `src/shell/shell.css` | 동기 불확실·패널 STOP 줄 |

## 2. 시험 (과제 5 대응)

| 요구 | 시험 | 결과 |
|---|---|---|
| lag 계산 | 미결 이벤트 `ts.mono_ns` 기준 250 ms, draw 후 lag 0 + idle(마지막 수신부터 2,034 ms), 받은 적 없으면 idle null, seq 없는 draw는 draw 시작 전 수신분만 제거, 256 표식 상한에서 가장 오래된 나이 유지, BigInt 정밀도(1.76e12 ms + 0.25 ms) | 통과 |
| idle 구분 | 셸 상태: 전부 idle → `idleMs`, 하나라도 미결 → null / 배지 `LIVE idle 5.2s` ↔ `LIVE lag 37ms`(배치 없이 tick만으로 숫자 갱신) / 하네스 패널 → 셸 배지까지(수신 120 ms 뒤 lag 120, draw 후 0 + idle 2,120) | 통과 |
| 시계 불확실 | 미동기 → `basis:'recv'`·`clockUncertain`, 다른 `clock_id` → 수신 기준, ±60 ms > 목표 45 ms → 불확실, ±3 ms → 아님, 배지에 "동기 불확실" | 통과 |
| metrics.report 주기·모양 | 타이머 1회 → 활동한 패널만 1건, **CORE JSON Schema(`schema/v1/metrics_report.schema.json`)를 ajv로 검증** + 생성 타입으로 타입 검사, `rendered_through_seq:'5'`, stages 5종(decode n=0), 다음 주기 0건(초기화) / 미결만 있어도 보고, 미동기 → `skew_err_ms:-1`·`clock_synced:false` / 16건 상한 + `reports_skipped` / 패널 64 상한 / 표본 모드 1/50(`50receive,100receive,50draw,100draw`, n=2, 스키마 통과) / 셸 경로로 실제 `core.metrics.report` 호출 | 통과 |
| presence 전환 | `setVisible:false` → presence 순서, **`viewer_presence.schema.json` 검증**, `{visible:false, rendered_through_seq:'41', oldest_pending_age_ms:990}`, 숨김 중 주기 보고 0, 복귀 `{hiddenMs:12300, events:30}` / 실제 `visibilitychange`로 FakeNet `visible=[false,true]`, 복귀 줄 표시·닫힘 | 통과 |
| degraded·latch 구독 | 20 ms draw 3회 → degraded + 콜백 `[true]`, 싼 프레임 → `[true,false]` / `reportWork(350)` → 1 s 창에서 강등 / `ctx.latch` 변화 때만 콜백 / uart.terminal·uart.trace·uds.console·can.trace에서 STOP 줄이 latch 동안만 보임 | 통과 |

## 3. 정의·정직성 메모
- **UI 구간(`ui`, 보고서의 상위 n/p50/p95/p99)** = 주 스레드 수신 → 그 배치를 덮은 draw 끝. plan §0.2의 `t_recv`는 Worker onmessage인데 net이 그 시각을 주 스레드에 넘기지 않아 **Worker → 주 스레드 postMessage 구간이 빠진다**(아래 요청 1). `detail.segment:'main_receipt_to_draw_end'`로 명시.
- `receive` 단계는 시계 기반 나이(CORE·배치·WS·Worker 포함)라 UI 전용 값이 아니다. `decode`는 net이 노출하지 않아 n=0, `detail.decode:'not_exposed_by_net'`.
- `next_raf`는 rAF 시작 시각 대용(§3.6: 스캔아웃 증명 아님).
- `ui` 히스토그램 표본 단위는 **배치**(이벤트 단위 아님). 이벤트 단위 표본은 표본 모드(1/50)가 담당.
- `skew_err_ms`는 스키마상 필수 number인데 미동기면 값이 없다(JSON에 Infinity 불가) → **-1 + `detail.clock_synced:false`**. 판정 쪽은 -1을 "동기 불확실"로 읽어야 한다(아래 CORE 질문 1).
- 셸 배지는 보이는 패널 lag 최댓값. 패널 `visible`은 아직 항상 true(레이아웃에 있으면 보임으로 간주). 탭 숨김은 브라우저가 rAF를 멈추므로 복귀 직후 lag가 크게 보이는 것이 사실 그대로다.
- 탭 단위 `rendered_through_seq` = 미결이 있는 패널 중 가장 낮은 렌더 seq(전부 그 이상은 그려짐), 미결이 없으면 가장 높은 렌더 seq. 필터 구독은 seq가 건너뛰므로 근사다.

## 4. 요청

### net (ui-sol, `src/net/**` — 수정 금지라 요청만)
1. **배치에 Worker 수신·디코드 시각**: `DeliveryItem`/`EventsBatch` 전달 시 `recv_ms`(Worker onmessage, `performance.timeOrigin + now` 기준)와 `decode_ms`를 함께 — 그러면 `ui` 구간을 §0.2 정의(`t_recv → t_draw`)대로 재고 `decode` 단계를 채운다.
2. **`setVisible(visible, extra?: Omit<ViewerPresence,'visible'>)`**: 지금은 net이 `{visible}`만 보내고 셸이 같은 RPC를 필드 채워 한 번 더 보낸다(둘 다 같은 포트 순서라 마지막 = 필드 있는 쪽). 재연결 직후 net이 보내는 `presence(false)`에도 필드를 넣으려면 net이 최근 값을 들고 있어야 한다.
3. 숨김 동안 데이터 credit 축소는 net 쪽(과제 3) — 현재 net 구현 상태를 모름. 복귀 줄의 "M 이벤트"는 **주 스레드에 도착한 수**라 credit을 줄이면 그만큼 작게 나온다(서버 쪽 생략은 display.gap으로 보임).

### CORE
1. `MetricsReport.skew_err_ms`: 미동기일 때 값 — `null` 허용 또는 -1 규약 확정.
2. `core.metrics.report`를 패널별로 여러 건(≤ 16/10 s) 보내는 방식이 괜찮은지, 아니면 배열 한 건(`reports[]`)을 원하는지.
3. presence의 `channel` 필드: 지금은 탭 전체(null)만 보냄. 채널별 presence가 필요하면 패널별로 보낼 수 있다.

### 리드
- 셸 tick이 패널 freshness 제공 함수를 250 ms마다 호출(패널 ≤ 64, 각 O(1)).
- CAN 패널 변경은 강등 판단 부분과 STOP 줄 한 줄만(과제 4 허용 범위). CAN 기존 시험 그대로 통과.

## 5. 남은 것
- 패널별 가시성(IntersectionObserver·최대화 상태)으로 `visible` 채우기.
- 원격(SSH 포워딩) 시 로컬 목표와 분리 보고(§3.5-7) — 원격 여부 신호가 hello에 생기면.
- 표본 모드의 시각은 `performance.now` 기준(`detail.t_base`). 하네스가 서버 수신 시각과 교차 확인하려면(§9.5) 데몬 mono 기준으로 바꿀지 시험 쪽과 정한다.
