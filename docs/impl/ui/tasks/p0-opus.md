docs/plan/02-core-principle.md 준수.

# UI P0 — ui-opus: 스토어 · 패널 SDK · 셸 화면

먼저 `docs/impl/ui/tasks/p0-common.md`. 계획 근거: `docs/plan/ui/plan.md` §0.4, §3.3–3.5, §4.2–4.3, §5.1.1, §5.1.5(F1: 정책으로 켰을 때만), §5.1.7, §5.2, §5.3–5.4, §5.9, §6.5.

## 당신의 파일 (이 밖은 수정 금지)
`ui/src/store/**`, `ui/src/sdk/**`, `ui/src/shell/**`, `ui/src/panels/**`(데모 패널만), `ui/src/main.tsx`

## 만들 것
1. **스토어** (`src/store/`, React 밖 순수 TS + `useSyncExternalStore` 훅)
   - ShellControlStore: 연결 상태·hello(env·rights)·latch·승인 목록(F1: 서버가 보낼 때만)·record health·heartbeat 신선도·뷰어 수·에이전트 활동 최신. **LIVE / LAG / STALLED / LIVE-DEGRADED** 계산(heartbeat 1 s, 1.5 s 회색 경고점, 3 s STALLED — STALLED는 쓰기 잠금 신호를 낸다).
   - RowStore: `request_id` 키 행, `WRITE_STATE_RANK`로 **단조 갱신**(늦게 온 낮은 단계 무시), 행 순서 고정(첫 요청 시각), 완료 행 상한(기본 20,000, 넘치면 오래된 완료 행부터 퇴출 + 퇴출 수), 진행 중 행은 퇴출 안 함.
   - ActivityStore: 링 5,000행.
   - RenderScheduler: rAF 루프, 패널별 invalidate, 프레임 데이터 렌더 예산 8 ms, 정상은 rAF마다, 예산 초과 패널은 20 Hz로 강등 + LIVE-DEGRADED 신호. 렌더 표본(stage)을 기록해 두는 훅(보고는 나중에).
2. **패널 SDK** (`src/sdk/`): `registerPanel(manifest, factory)`, manifest 검증, `PanelContext` 구현(plan §4.2 — 선언 안 된 topic·query·action은 throw), **ActionGate**(rights 검사, confirm 모드 none/enter_twice/card, `safetyDirection:'reduce'`는 watch도 허용·확인 없음), `StoreMirror`. NetClient는 주입받는다(api.ts 인터페이스만 의존).
3. **셸** (`src/shell/`, React)
   - 상단 바: env 배지(생략 금지, SIM/BENCH/VEHICLE 색), LIVE/lag 배지, REC, 보는 사람, 에이전트 활동, 승인 수(있을 때만), **[■ STOP 전체]**(모든 자격, 확인 없음, 낙관 표시 "STOP 요청 중…" → 서버 `core.latch{acked}` 시 "STOP — 잠김", 2 s 무응답 시 "STOP 확인 안 됨 — 재시도", WS 끊김이면 `stopViaHttp`).
   - 채널 바 슬롯, 1 Hz 하트비트 시계, 고정 레이아웃 프리셋 3개(`m1-default`, `uart-only`, `can-uds`) + 패널 최대화(빈 패널 자리에는 이름 표시).
   - STOP 상태 패널(§5.1.7): 잠긴 것·취소 불가·출력 유지 장치 + [비상 OFF]·복구 송신 목록(`core.safe_state.list`) + [복구 송신 ⏎] → `core.safe_state.run{ids}` 항목별 결과·인터록 막힘 표시. 해제 버튼(M1: operate + 확인 카드).
   - Activity 띠(하단 2줄 + 펼치기), 연결/STALLED/재연결/`REC failed`/worker 크래시 배너.
   - 승인 대기열 + 일반 승인 카드(정책으로 켰을 때만 렌더): 1 s 무장, 기본 포커스 거부, Enter·Esc 비승인, 승인 = 버튼 또는 `Y`, `approval.display` JSON을 일반 렌더러로(텍스트는 textContent).
   - 접두키 `Ctrl-]`(window capture 단계): `x` STOP, `m` 마커, `A` 승인 대기열, `?` 도움말, `Ctrl-]` 두 번 = 리터럴(패널에 전달). 웹에서 `Ctrl-X` STOP 미채택.
   - `/ui/#/need-auth` 화면(쿠키 없음 안내, 비밀 없음).
   - 접근성: 색만으로 상태 전달 금지, aria-live는 승인·STOP·오류만, DOM에 `data-request-id`·`data-env`·`data-origin`.
4. **데모 패널** (`src/panels/demo/`): SDK로 등록한 최소 패널 1개(구독 행 표시 + act 1개) — SDK가 lint 규칙(I-UI1) 안에서 동작함을 보이는 용도. 실제 UART/CAN/UDS 패널은 P1.
5. `src/main.tsx`: 셸 마운트. NetClient는 ui-sol이 `src/net/index.ts`에 `createNetClient(url)`을 만드는 중이므로, 그게 아직 없으면 **당신 쪽 테스트용 가짜(`src/store/testing/fakeNet.ts`)** 로 개발하고 main.tsx는 `createNetClient`를 import하는 형태로 두되 타입체크가 깨지면 보고서에 적는다.
6. 테스트(vitest, DOM은 `// @vitest-environment jsdom`): RowStore 단조성·퇴출, 상태 배지 임계, ActionGate 자격·확인 규칙, STOP 버튼 상태 전이, 승인 카드 키 규칙, 접두키.

## 보고서에 넣을 것
만든 파일, 시험 결과, 임시 프로토콜에서 애매했던 점, 리드에게 요청할 설정·의존성 변경, ui-sol 쪽 net과 붙일 때 확인할 점.
