docs/plan/02-core-principle.md 준수.

# UI P1 선행 — CAN Trace 패널 (ui-sol 대행 서브에이전트)

작성: ui-lead, 2026-11-11. 규칙은 `docs/impl/ui/tasks/p0-common.md`·`p0-proto-v1-update.md` 그대로(커밋 안 함, git stash 금지, 리드 소유 파일 수정 금지, `cd ui && pnpm check` 통과, 장시간 10분 이내). UI 내부 규약 추가: `ui/src/net/api.ts`의 onControl 주석(heartbeat 합성 이벤트 등).

## 근거
- `docs/plan/ui/plan.md` §0.1 E-UI-3, §0.2(CAN 종단 p95 150 / p99 300 ms, UI 구간 ≤ 50 ms, 정상 rAF·초과 시 20 Hz 강등), §5.1.3, §6.5(보관 하한 100k·상한 2M, 탭 예산 256 MiB), UD-02·UD-13
- 내용 명세: `docs/plan/_xteam/can-ui.md`(U-CAN-1·5·6·8·9 + `## ui 의견`)
- 패널 SDK: `ui/src/sdk/`(ui-opus 소유 — **읽기만**). 특히 `index.ts`(registerPanel·배지 상수), `rowTable.ts`·`virtualTable.ts`(ui-opus가 P1에서 만드는 중인 공통 표 — 쓸 수 있으면 재사용, 안 맞으면 패널 안에서 따로), `context.ts`(PanelContext), `testing/harness.ts`
- 네가 이미 만든 mock: `ui/tools/mock/`(CAN 0x300 → 0x301, `mock.inject flood_can`)

## 파일 범위
**만들 수 있는 곳**: `ui/src/panels/can/**`, 그리고 네 영역이던 `ui/tools/mock/**`, `ui/fixtures/**`, `ui/e2e/**`, `ui/src/net/**`(api.ts 제외). **수정 금지**: `ui/src/sdk/**`, `ui/src/store/**`, `ui/src/shell/**`, `ui/src/panels/{uart,uds,demo}/**`, `ui/src/panels/index.ts`, `ui/src/main.tsx`, 리드 소유 파일. ui-opus가 같은 worktree에서 UART·UDS 패널과 sdk를 고치고 있다. 패널 등록이 `panels/index.ts`를 건드려야 하면 **고치지 말고** `ui/src/panels/can/index.ts`에 `registerCanPanels()`를 내보내고 보고서에 "index.ts에 한 줄 추가 필요"라고 적어라(리드가 합친다).

## 만들 것 — `can.trace` (M1: Chrono·raw·송신줄·생략 행·에이전트 읽은 범위)
1. **보관**: 패널 안 **열 단위 typed 링**(ts_mono, seq, id, flags(dir·ext·fd·origin), dlc, data 오프셋/바이트 풀, request_id 사전) — 기본 100k행, 설정 상한 2M(데이터 예산 안). 넘치면 오래된 것 버림 + "보관 범위 밖 → 질의" 표식. 행 객체를 메인 스레드에 쌓지 않는다(Worker 링 projection은 M2).
2. **표시**: DOM 가상화(고정 행높이, `@tanstack/react-virtual` 또는 명령형 풀), 열 = 시각(s)·dir·출처 배지(문자 병기)·id·dlc·data hex·상태(TX 행 단계 칩 — `can.tx`의 `request_id`+`write_state`)·비고. 자동 따라가기(맨 아래) ↔ 사용자가 위로 스크롤하면 고정 + "새 행 N" 버튼. [⏸ 일시정지](수집 계속, 렌더만 정지 + `PAUSED +N`).
3. **정직성**: `display.gap`(로컬·서버) → 생략 합성 행 `⋯ N 프레임 화면 생략 (seq a–b) — 기록 정상|확인 안 됨`(record health가 ok일 때만 "기록 정상"), `source.gap`·`record.gap`은 빨강. 렌더 예산 초과 시 LIVE-DEGRADED(스케줄러 신호) + 표시 예산 2,000행/s, 넘는 행은 "화면 생략"으로 세되 링에는 보관.
4. **에이전트 읽은 범위 막대**: `core.activity`의 `range{seq_a, seq_b}`(domain can, 같은 채널) → 행 왼쪽 여백 막대.
5. **송신줄**: `0x300#00` / `18DAF110#02 10 03`(29bit) / FD 표기 입력, 검증(11/29bit, DLC 0–8, FD 길이 집합 0–8/12/16/20/24/32/48/64), Enter → `can.tx.send{channel, id, ext, fd, data_hex}` via `ctx.act`. 위험 등급 배지 표시(F1 기본 자동). STOP latch 중에도 사용자 송신은 허용(A3).
6. **필터**(M1 최소): id 목록·dir·origin 필터를 클라이언트에서. 서버 필터·필터식은 M2.
7. **성능 확인(10분 이내)**: mock `flood_can` 5k fps 정상 / 10k fps 고부하로 Playwright에서 렌더 proxy 지연(생성 시각→다음 rAF) p95를 재고, 5k에서 드롭 0·10k에서 gap 표시가 정확한지. 수치를 보고서에(합성·단일 머신이라는 한계 명시).
8. **E2E**: E-UI-3 — `mock.agent`로 에이전트 `can send 0x300#01` → Trace에 AGT tx 행(DOM `data-request-id`·`data-origin="agent"`) + DEV rx 0x301, 사용자 송신줄 `0x300#00⏎` → USR 행. 현재 레이아웃에 CAN 자리가 없으면 `#/p/can/trace?channel=ch0` 딥링크로 연다.
9. 단위 시험: 링 랩어라운드·상한, 송신줄 파서·검증, gap 합성 행, TX 행 단계 갱신.

## 보고
`docs/impl/ui/reports/p1-can.md` — 만든 파일, 시험(개수)·성능 수치, `panels/index.ts` 등 리드가 합칠 것, CAN 팀에 물을 것(payload 필드 등), 남은 것.
