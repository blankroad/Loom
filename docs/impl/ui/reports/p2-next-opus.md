docs/plan/02-core-principle.md 준수.

# UI M2 선행 보고 — ui-opus: 수명주기 DOM 증가 확인 + 도킹 E2E (**중간 보고 — e2e 미실행**)

작성: ui-opus, 2026-11-11. 기준 ui/main `a96dc47`, 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui` (커밋 안 함). 사용량 한도로 여기서 멈춤 — **새 도킹 e2e는 리드 공지(Playwright 일시 중지) 뒤 아직 한 번도 돌리지 않았다.**

## 0. 상태
- 끝남: §1 누수 판정(+ T10 강화), CAN Trace reveal(가리키기 대상), DEV 훅, 도킹 e2e **작성**.
- 남음: `pnpm e2e:mock --workers=1` 전체(새 `e2e/m1/dock.spec.ts` 포함) 실행·수정, §3 번들 크기 제안.
- 단위: typecheck 0 · lint 0 · **35개 파일 366개 통과**.

## 1. T10 DOM 노드 증가 — 누수 아님 (근거)
| 회차 | CDP `Nodes` | 문서에 붙은 노드(TreeWalker) |
|---|---|---|
| 100 | 1,099 → 2,041 (+942) | — |
| 200 | 1,095 → 2,090 (+995) | — |
| 400 | 1,187 → 2,145 (+958) | — |
| 100 / 400 (계측 추가 후) | +1,035 / +1,053 | **740→740 / 736→740** |
- 증가가 회차와 무관(포화) → 회차당 누수 아님. 붙은 DOM은 그대로, 힙 스냅샷(전체 GC)에서 Detached DOM 0개(30·120회).
- 원인: CDP `Nodes`는 아직 쓸리지 않은 쓰레기 노드도 센다. 열린 CAN Trace가 프레임마다 셀 텍스트를 바꿔 같은 시점에서도 수백 개씩 흔들린다(같은 측정점에서 GC 1회 855 → 스냅샷 직후 1,383). 도킹 전에도 같았다(p1-fix 실행 943→1,852, +909).
- **T10 변경**(`e2e/m1/t10-lifecycle.spec.ts`): 회차 `T10_CYCLES`(기본 100), DOM 내역 로그, **새 엄격 조건 `attached ≤ base + 50`**(같은 패널이 열려 있으니 문서 트리 크기는 같아야 함 — 남은 패널 하나도 ≥ 100 노드). 기존 `Nodes ≤ base + 2,000`은 그대로(느슨하게 바꾸지 않음), 근거 주석 추가. 100회 실행 통과(attached 736→740).

## 2. 도킹 e2e (`e2e/m1/dock.spec.ts`, mock, **미실행**)
1. 저장 → 새로고침 → `#/l/e2e-dock` 복원: 패널·props 집합 동일.
2. 탭(`ch0`)을 UDS 그룹 오른쪽 가장자리로 끌기 → 배치 바뀜, **STOP 버튼 위치·활성 불변**. (dockview는 HTML5 DnD — Playwright `dragTo`가 안 먹으면 마우스 단계로 바꿔야 함)
3. 가리키기: ch0을 뒤 탭으로 만든 뒤 `ui.focus_request{at:seq:N}` → [열기] → 해시·패널 수 불변, ch0 활성, `.ct-row.ct-revealed[data-seq=N]` 보임.
4. 숨은 탭: `burst_can` 동안 ch0 그리기 0회 → 탭 클릭 후 > 0, 구독 수(conn_stats) 불변. (`burst_can` 인자 이름 `channel`/`count`는 mock 확인 필요)

## 3. 이번에 바꾼 코드
- `src/panels/can/traceView.ts`·`canTrace.ts`: **CAN Trace reveal** — `seq` 이상 첫 행으로 이동·표시(`ct-revealed`), `@req:`는 요청 행, 보관 범위보다 오래된 seq·아직 안 온 seq는 "처리 안 함"(셸이 찾지 못했다고 알림). 이전엔 UDS만 reveal을 처리해 "seq 5012로 이동"이 탭만 열었다. 시험 `src/panels/can/reveal.test.ts`(3).
- `src/shell/services.ts`: `handleControl`(제어 경로 처리기)을 서비스에 노출.
- `src/shell/dock/dockHost.ts`: `describe()`(열린 패널·instance id·보임 여부), 탭 요소 `data-props`. 시험 +1.
- `src/main.tsx`: **DEV 빌드 전용** `window.__loomShell`(control 주입, panels/snapshot/open, drawsSince, now). 프로덕션 빌드에는 없음.

## 4. 요청
- **mock(리드)**: `mock.inject focus_request{panel, props, at, reason}`(또는 에이전트 `ui.open`) — 그러면 e2e 3번이 DEV 훅 대신 와이어로 간다.
- 번들 크기(§3 과제): 측정·제안 미완.
