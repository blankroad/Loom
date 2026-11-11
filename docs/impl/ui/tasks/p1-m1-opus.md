docs/plan/02-core-principle.md 준수.

# UI P1 M1 — ui-opus: 렌더 지연 보고 · 뷰어 가시성 · 브라우저 lag · SDK 보강

작성: ui-lead, 2026-11-11. 기준 ui/main `9e93ef2`(main 761870a 병합, **wire 타입은 이제 `schema/v1` 생성 타입** — `src/proto/gen`, `src/proto/provisional.ts`는 별칭. `pnpm check`가 `check:schema`부터 돈다. 생성 파일 수정 금지). 규칙은 `p0-common.md` 그대로(커밋 안 함, 리드 소유 파일 수정 금지).

## 근거
`docs/plan/ui/plan.md` §0.2(지연 예산·UI 구간 목표), §3.5(시계·lag 정의), §3.6(렌더 보고), §5.4, UI-T03. 계약 `docs/contract/01-processes-ipc.md` §1.6·§1.8, 생성 타입 `src/proto/gen/viewer_presence.ts`·`metrics_report.ts`·`time_result.ts`.

## 만들 것
1. **브라우저 쪽 lag**: 패널 lag = 아직 그리지 못한 가장 오래된 이벤트의 나이(`net.clock()`의 theta로 데몬 mono 환산, BigInt). 받은 게 없으면 lag 0 + `idle N s`. 셸 배지 = 보이는 패널 lag 최댓값. 지금 배치 `lag_ms`(서버 측)를 쓰는 곳을 이것으로 교체(서버 lag는 툴팁에 함께). 시계 미동기·불확실성이 목표보다 크면 "동기 불확실" 표시.
2. **렌더 보고**: RenderScheduler 표본 → 10 s마다 `core.metrics.report`(생성 타입 모양 그대로) — 패널별 stage(receive/decode/apply/draw/next_raf) p50/p95/p99, `rendered_through_seq`, `oldest_pending_age_ms`, `clock_err_ms`, 생략 수. 표본 모드(1/50 이벤트의 seq·stage·시각)는 설정으로 켠다(시험 하네스용). 측정 자체가 부하를 만들지 않게 상한.
3. **뷰어 가시성**: `visibilitychange`마다 `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}`(net.setVisible이 이미 있으면 그 경로로 필드 채우기). 숨김 → 렌더 중단·데이터 credit 축소는 net 쪽, 복귀 시 "숨김 중 N s (M 이벤트) — 라이브로 이동" 한 줄.
4. **SDK 보강(CAN 패널 요청)**: `ctx.scheduler.degraded`(읽기 전용, 구독 가능) — CAN 패널이 따로 계산하던 강등 판단을 이것으로 교체(`src/panels/can/`의 해당 부분만 고치는 것은 허용), `ctx.latch`(STOP 상태 읽기 전용 + 구독) — CAN·UART·UDS 패널 상단에 `STOP — agent/script 쓰기 잠김(사용자 입력 가능)` 한 줄.
5. 시험: lag 계산(idle 구분, 시계 불확실), metrics.report 주기·모양(생성 타입으로 타입 검사), presence 전환, degraded·latch 구독.

## 파일 범위
`ui/src/store/**`, `ui/src/sdk/**`, `ui/src/shell/**`, `ui/src/panels/**`(위 4번에 필요한 범위). **수정 금지**: `ui/src/net/**`(필요한 net 변경은 보고서에 요청), `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`(서브에이전트 작업 중), `ui/src/proto/**`, 리드 소유 파일.

## 보고
`docs/impl/ui/reports/p1-m1-opus.md`. 끝나면 "완료: <경로>"만.
