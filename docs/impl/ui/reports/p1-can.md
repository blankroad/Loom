docs/plan/02-core-principle.md 준수.

# UI P1 보고 — CAN Trace 패널 `can.trace` (ui-sol 대행)

작성: ui-sol 대행(Claude 서브에이전트), 2026-11-11. 작업 공간 `.worktrees/ui`(브랜치 `ui/main`, 기준 커밋 c1ed979), **커밋하지 않음**, git stash 쓰지 않음. 수정은 `ui/src/panels/can/**`, `ui/tools/mock/**`, `ui/e2e/**`에만 했다(ui-opus·리드 소유 파일 손대지 않음).

## 1. 결과 요약

| 항목 | 상태 |
|---|---|
| `cd ui && pnpm check` (typecheck + lint + vitest) | **통과** — 21 파일 203 시험(ui-opus 진행 중 파일 포함, 2026-11-11 07:14 기준) |
| 내 범위 vitest | **34개 신규** — `src/panels/can/` 33(sendLine 6 · traceModel 15 · canTrace(jsdom·하네스) 12) + mock `server.test.ts` 1(CAN trace query); 기존 E-UI-3 mock 시험에 tx 에코 `request_id` 단언 추가 |
| `pnpm e2e --workers=1` | **7개 통과** — 새 CAN 5(E-UI-3 1 · gap 3층 1 · 성능 3) + 기존 smoke 2 |
| 핵심 루프 | 에이전트 `can.tx.send 0x300#01`(mock.agent) → Trace에 `AGT tx` 행(`data-request-id`, `data-origin="agent"`, `○요청→…→●확정` 같은 행 갱신) + `DEV rx 0x301` → 사용자 송신줄 `0x300#00⏎` → `USR(나)` 행 + 0x301 응답 → 에이전트 `can.trace.query` → 행 왼쪽 "에이전트가 읽은 범위" 막대. **에이전트 RPC 시작 → AGT 행 ●확정 + 0x301 화면 표시 70–80 ms**(RPC 왕복 포함, E2E 측정) |

## 2. 만든 파일

| 파일 | 내용 |
|---|---|
| `src/panels/can/traceModel.ts` | **열 단위 typed 링**(행 객체 없음): seq f64 · mono_ms f64 · id u32 · flags u16(dir/ext/fd/brs/origin/hidden/echo/no-data) · kind u8(frame/tx 요청/gap) · len · wstate · risk · aux u32 · data 8 B inline. 상한(C2): 행 기본 **100k**, `rows` prop로 **1k–2M**; FD(>8 B)는 별도 슬롯 링(max(1024, cap/16) × 64 B, 재사용되면 "(FD 데이터 보관 범위 밖)"으로 정직 표시); request_id 사전 8,192(FIFO); gap 세부 4,096; 표시 인덱스 = cap 크기 링(abs 행 번호, 이진 탐색); 에이전트 범위 16. 넘치면 가장 오래된 행 덮어쓰기 + `evicted` 계수 → "보관 범위 밖 → 질의". 메모리 ≈ 47 B/행(100k ≈ 4.7 MB, 2M ≈ 94 MB) |
| `src/panels/can/traceView.ts` | 명령형 고정 행높이(18 px) DOM 풀 가상화. 2M행은 스페이서 8M px 상한 + 비례 스크롤. 자동 따라가기 ↔ 위로 스크롤하면 **abs 행 번호로 고정**(앞쪽 퇴출에도 안 밀림) + `새 행 N ↓` 버튼. 일시정지 = 목록 끝 고정(수집 계속). 셀이 바뀐 것만 DOM 쓰기, textContent만 |
| `src/panels/can/sendLine.ts` | 송신줄 파서·검증: `0x300#00`, `300#01.02.03`, `18DAF110#02 10 03`(8자리·>0x7FF·`x` 표기 → 29bit), `fd [brs] 0x123#…`, cansend `123##1…`. 11bit ≤ 0x7FF / 29bit ≤ 0x1FFFFFFF, CC 0–8, FD {0–8,12,16,20,24,32,48,64}, 홀수 자리·비16진·RTR·256자 초과 거절(이유 문구) |
| `src/panels/can/canTrace.ts` | 패널: 매니페스트 + 팩토리 + `registerCanTrace()`(중복 호출 무해). 열 = 막대·시각(s)·dir·출처(AGT/USR/DEV/SCR 문자, 내 송신은 `USR(나)`)·id·dlc(FD `F`/`FB`)·data hex·상태 칩(`STAGE_CHIP`, sent 미최종이면 `·에코`)·비고(client_id, request_id, 오류, 등급). 상단: env·LIVE/LIVE-DEGRADED/`⏸ PAUSED 12.4 s +N`·⏸ 버튼(Space)·id/dir/출처 필터·에이전트 읽음 요약. 하단: 송신줄(입력 중 미리보기·Enter·↑↓ 기록 50)·위험 배지 `R1 SIM 자동`·상태줄(보이는/링/상한, 수신 fps, 화면 행/s, 생략 누적, 보관 범위 밖) |
| `src/panels/can/can.css` | 패널 스타일(셸 토큰 재사용, loss 행 빨강) |
| `src/panels/can/index.ts` | `registerCanPanels()` 내보냄(**panels/index.ts 미수정**) |
| `src/panels/can/*.test.ts` | 아래 §4 |
| `tools/mock/sim.ts`, `server.ts` | (1) CAN tx 에코 `can.frame`에 같은 `request_id` 부여(Trace가 한 행으로 합침), (2) `can.trace.query{channel,max≤1000}` — 링에서 최근 프레임 반환 + `core.activity{verb:"read", domain:"can", channel, range{seq_a,seq_b}}` 발행, (3) `mock.inject{kind:"flood_stats"}` — flood가 낸 프레임 수(성능 E2E 회계용) |
| `e2e/can.html`, `e2e/can-register.js` | E2E 전용 진입 페이지: CAN 패널을 등록한 뒤 `src/main.tsx`를 띄운다(panels/index.ts 병합 전에도 E2E 가능). 등록 함수가 멱등이라 병합 뒤에도 무해 |
| `e2e/mockCtl.ts` | Node 쪽 mock 제어 연결(`mock.agent`/`mock.inject`) |
| `e2e/can.spec.ts` | E-UI-3 + gap 3층 표시 |
| `e2e/can-perf.spec.ts` | flood 5k/10k/10k+CPU×8 지연·회계 |

### 동작 규칙 요약
- **수집과 그리기 분리**: `ctx.store('can')`의 `onAppend`마다 새 이벤트를 즉시 링에 넣는다(일시정지·rAF 지연과 무관). 그리기는 `ctx.scheduler.onFrame`(rAF)에서만. 미러는 20,000건(최대).
- **TX 행**: `can.tx`(request_id + write_state)로 행 생성, 단계는 단조 증가로 같은 행 갱신. 제어 경로로 먼저 온 최종 단계는 `ctx.rows({domain:'can', channel})`(RowStore)에서 읽어 보이는 행에 반영(단조). 같은 request_id의 tx 에코 `can.frame`은 새 행을 만들지 않고 그 행에 `·에코` 표시.
- **정직성**: `display.gap`(로컬·서버) → `⋯ N 프레임 화면 생략 (seq a–b) — 기록 정상 | 기록 확인 안 됨`(gap을 본 시점 record health가 `ok`일 때만 "기록 정상"), `record.gap`(RECORDING INCOMPLETE)·`source.gap`(SOURCE LOSS, producer_range만 있으면 stream seq를 지어내지 않음)은 빨강 행. 배치 `display_gap` 요약만 오고 이벤트가 없으면 요약으로 행을 만들고, 이벤트와 같은 범위면 중복 제거(최근 64키). resync는 `⋯ 재동기화 — <reason>` 행 + epoch 변경 대비(새 epoch의 낮은 seq도 받음). gap 행은 필터로 숨기지 않는다.
- **LIVE-DEGRADED + 표시 예산 2,000행/s**: 패널이 스케줄러와 같은 규칙(그리기 > 8 ms 3프레임 연속, 또는 ingest+draw > 300 ms/s)으로 강등하고(30 프레임 가벼우면 복귀), 강등 중 일반 프레임은 토큰 버킷(2,000/s, burst 500) 초과분을 **링에는 보관하되 화면에서 숨기고** 한 줄 생략 행에 센다. 쓰기 행(tx 요청)은 절대 숨기지 않는다.
- **에이전트 읽은 범위**: `core.activity`(domain can, 같은 channel, `range{seq_a,seq_b}`) → 해당 seq 행의 왼쪽 4 px 청록 막대(툴팁: 범위·actor·N s 전) + 상단 `▌에이전트 읽음 seq a–b · N s 전`. 구독은 셸의 Activity 구독과 **같은 (filter, options)** `{kinds:['core.activity']}`·snapshot → net mux가 서버 구독 1개로 공유(연결당 ≤ 8 준수).
- **송신**: Enter → 검증 → `ctx.act('send', {channel, id, ext, fd, data_hex[, brs]})` → ActionGate(operate, confirm none) → `can.tx.send`. watch 권한이면 입력 비활성. STOP latch는 ActionGate가 막지 않는다(writeLocked는 STALLED만) → 사용자 송신 허용(A3).

## 3. 성능 수치 (Playwright, 10분 규칙 안: 성능 3건 합 ≈ 30 s)

환경: macOS(M 시리즈 맥북) 단일 머신, mock + Vite dev + headless Chromium 1.65가 CPU 공유, JSON 인코딩, 합성 8 B 프레임(0x400–0x43F), 배경 시나리오(0x100 10 Hz·0x200 20 Hz) 포함, 패널 단독 딥링크(`#/p/can/trace?channel=ch0`, 1280×720).
지연 정의: 이벤트 `ts.mono_ns`(mock 허브 publish) → 그 행을 목록에 넣은 그리기 **다음 rAF**(렌더 proxy, paint 시각 아님). 브라우저→데몬 mono 변환은 net 시계 동기(`core.time`, clock err ≈ 0.1 ms). **mock 배치 대기(max_batch_ms 40) + WS + Worker 디코드 + 메인 반영 + 그리기 전부 포함** = 종단 근사. snapshot backlog(과거 이벤트)는 측정에서 제외.

| 부하 | 낸 프레임 | 받은 프레임 | 생략·gap | 지연 p50 / **p95** / p99 / max (ms) |
|---|---|---|---|---|
| 5k fps × 6 s | 26,300 | 26,300 | **0** (gap 행 0, 예산 생략 0) | 48 / **69** / 79.5 / 85 |
| 10k fps × 6 s | 52,500 | 52,500 | 0 | 49 / **69.5** / 75.5 / 87 |
| 10k fps × 6 s, CPU ×8 (CDP 스로틀) | 54,900 | 54,900 | 0 | 93 / **123.5** / 135.5 / 306 |
| (탐색 1회) 10k fps, CPU ×30 | 53,200 | 52,808 | 로컬 `display.gap{client_decode_budget}` 합 394 + 표시 예산 생략 45,887(링 보관) — LIVE-DEGRADED | 1,366 / 1,854 / 1,989 / 2,071 |

- 목표(CAN 종단 p95 150 / p99 300, UI 구간 ≤ 50 ms) 대비: 정상(5k)·10k 모두 종단 p95 ≈ 70 ms, 그중 ≈ 40 ms가 mock 배치 대기. 5k에서 드롭 0 확인(시험이 `seen === emitted`·gap 0·생략 0을 단언).
- 10k 과부하 회계: CPU ×30에서 받은 52,808 + 로컬 gap 394 = 53,202 ≥ 낸 53,200(+2는 같은 구간 배경 프레임) — **조용한 손실 없음**. 예산 생략은 링에 남고 생략 행으로 셈. 시험은 `seen + gap 합 ≥ emitted`, `seen ≤ emitted`를 단언(×30은 `CAN_PERF_THROTTLE=30`으로 재현, 기본 스위트는 ×8).
- 한계: 합성·단일 머신·render proxy·JSON. 제품 합격 근거가 아니다(plan §9.5: 서버 수신 시각 교차 확인·`core.metrics.report` 보고는 아직 없음).

## 4. 시험

- `sendLine.test.ts`(6): 11bit, 29bit(8자리/>0x7FF/`x`/범위 초과), CC DLC 0–8, FD 길이 집합·`fd brs`·`##flags`·BRS 단독 거절, 형식 오류 문구, id 표기.
- `traceModel.test.ts`(15): 용량 클램프, **링 랩어라운드·상한·evicted·표시 인덱스 trim·이진 탐색**, 열 저장, FD 슬롯 재사용 시 null, raw_ref `?`, **TX 행 단계(단조·에코 병합·늦은 sent 무시)**, failed 오류·`markMine`, `raiseState` 단조, request 사전 상한, **gap 합성 행(3층·producer-only·필터 무시)**, 강등 시 2,000/s 예산 생략 행·회계(`보인 프레임 + 생략 = 받은 프레임`)·쓰기 행 비생략, 비강등 시 생략 0, id/dir/origin 필터 재구성, 에이전트 범위(채널·도메인 구분·16 상한), mono 정밀도.
- `canTrace.test.ts`(12, jsdom + `sdk/testing/harness`): 구독 모양(활동 구독이 셸과 동일), E-UI-3 행·DOM 속성, RowStore 최종 단계 선반영, 송신줄 → `can.tx.send` 인자·`USR(나)`, 잘못된 입력 미전송, watch 비활성, gap 행 "기록 정상/확인 안 됨"·loss 빨강, 배치 요약 gap·resync 행·epoch 변경 후 낮은 seq 수용, 에이전트 막대, id 필터, 일시정지 `PAUSED +N`, 위로 스크롤 고정 + `새 행 30`.
- mock `server.test.ts`: tx 에코 request_id 단언 추가, `can.trace.query` → `core.activity` range 1건 신규.

## 5. 리드가 합칠 것

1. **`ui/src/panels/index.ts`에 두 줄 추가** (현재 레이아웃 `m1-default`·`can-uds` 프리셋에 이미 `can.trace` 자리가 있어 바로 붙는다):
   ```ts
   import { registerCanPanels } from './can';
   registerCanPanels();
   ```
   (`registerCanPanels`는 멱등 — `e2e/can.html`이 먼저 등록해도 충돌 없음.) 병합 후에는 `e2e/can.spec.ts`를 `#/p/can/trace?channel=ch0`(기본 `index.html`)로 바꾸고 `can.html`/`can-register.js`를 지워도 된다.
2. 의존성·설정 변경 없음. 매니페스트 `memory.maxBytes`는 2M 상한 기준 ≈ 110 MiB로 선언(기본 100k는 그 1/20). 탭 예산 256 MiB 배분표에 반영 바람.

### ui-opus(SDK)에 요청
- **`ctx.scheduler.degraded`(또는 onDegradedChange) 노출**: 지금 PanelContext에는 스케줄러의 강등 신호가 없어 패널이 같은 규칙을 따로 계산한다. 노출되면 그걸로 교체(한 곳에서 판단).
- `PanelContext`에 STOP latch 여부(읽기 전용)가 있으면 CAN 명세의 패널 상단 `STOP — agent/script TX latched` 표시를 넣을 수 있다(지금은 셸 띠에 의존).
- 진행 중인 `GapMark.fromBatch`가 들어오면 배치 요약/이벤트 중복 제거를 키 비교 대신 그걸로 단순화 가능.

## 6. CAN 팀(·CORE)에 물을 것

1. **tx 에코 `can.frame`에 `request_id`가 붙는가?** UI는 붙는다고 가정하고 한 행으로 합친다(mock도 그렇게 바꿈). 안 붙으면 같은 송신이 `can.tx` 행 + 별도 tx 프레임 행 두 줄로 보인다.
2. `can.frame`/`can.tx` payload 필드: `id`(number, 문자열 `0x…`도 허용 중), `ext`, `fd`, `brs`, `dlc`(=바이트 길이로 해석), `bytes{encoding,data}` 확정? FD의 `esi`, HW 타임스탬프 표시(◆)용 `ts.source:"hardware"`도 확인 필요.
3. `can.tx.send` 인자에 **`brs`** 가 있는가? 지금은 사용자가 지정했을 때만 보낸다(provisional 모양 `{channel,id,ext,fd,data_hex}` 밖).
4. `can.trace.query`의 인자·결과 모양(mock은 `{channel, max}` → `{frames, seq_a, seq_b}` 임시)과, 에이전트 읽기의 `core.activity.range`를 CORE가 넣는지(UI 계획 §15.1 요청 건).
5. 1회 송신 위험 등급 기본값(R1 가정)과 env별 차이 — 송신 전 배지에 표시할 값을 질의할 RPC가 있는지(예: dry-run).

## 7. 남은 것 / 비범위
- M2: Fixed 모드·신호 펼침·필터식(서버 push-down)·`coalesce_by:"id"`·바이너리 열 블록(`loom.columns.v1`) 수용·Worker 쪽 링 projection(지금은 메인 스레드 typed 링).
- M3: `can.trace.query`로 보관 범위 밖 과거 탐색(지금은 표식만), 행 우클릭 메뉴(다시 보내기·이 ID 필터·마커), EXT 배지, ISO-TP 묶음.
- `core.metrics.report{kind:"ui.render.sample"}`로 지연 표본 회신(U-CAN-7) — 패널 히스토그램은 있으나 보고 경로(SDK) 없음.
- 강등 중 생략 행이 배치마다 하나씩 생겨 여러 줄이 될 수 있다(회계는 정확). 연속 생략 병합 기준(예: 사이 보인 행 < N이면 합침)은 다음에 다듬기.

## 8. 작업 중 메모
- 작업 중 ui-opus의 `src/sdk/storeMirror.ts`가 잠시 문법 오류 상태(편집 중)여서 첫 E2E가 Vite 변환 오류로 실패했다. 그동안은 스크래치 복사본(ui-opus 진행 파일은 HEAD 판, 내 파일은 작업본)으로 E2E를 돌려 확인했고, ui-opus가 고친 뒤 실제 worktree에서 `pnpm check`·`pnpm e2e` 모두 다시 통과시켰다. 복사본·그 서버는 정리했다. ui-opus 파일은 고치지 않았다.
