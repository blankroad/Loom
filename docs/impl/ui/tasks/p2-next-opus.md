docs/plan/02-core-principle.md 준수.

# UI M2 선행 — ui-opus: 수명주기 DOM 증가 확인 + 도킹 E2E

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md`(커밋 안 함, **질문 창 금지**, 경로 `/Users/ctmctm/workspace/Loom/...`). 기준 ui/main `a96dc47`(도킹 커밋).
**이번에는 `ui/e2e/**`도 수정 가능**(서브에이전트 작업 없음). ui-sol이 `ui/src/codec/**`를 작업 중 — 거기는 수정 금지. 그 밖 금지: `ui/src/net/**`, `ui/src/proto/**`, `ui/tools/**`(mock 변경이 필요하면 보고서에), 설정, Rust crate.

## 1. T10 DOM 노드 증가 확인
당신 보고(`p2-dock-opus.md` §0): T10 100회 열기/닫기 후 DOM 노드 1,137 → 2,156(상한 +2,000 안). 도킹 전에는 이만큼 늘지 않았다. **누수인지 확인**하라: 열고 닫기 횟수를 200·400으로 늘렸을 때 선형으로 늘면 누수 → 원인(dockview 그룹·탭 헤더·우리 PanelHost·ResizeObserver 등)을 찾아 고친다. 포화(일정 값에서 멈춤)면 근거를 보고서에. 결과에 맞춰 T10 상한을 조정할 필요가 있으면 근거와 함께 바꾼다(느슨하게만 바꾸지 말 것).
## 2. 도킹 E2E (`ui/e2e/m1/` 또는 새 `ui/e2e/m2/` — m1.config가 잡도록)
- 저장 → 새로고침 → `#/l/<저장 이름>` 복원(패널·props 같음)
- 탭 끌기(분할 이동) 후 **STOP 버튼 위치·활성 불변**
- 가리키기(`ui.focus_request`) 시 현재 배치 유지 + 대상 탭 활성화 + reveal
- 숨은 탭 → 그리기 중단, 다시 보이면 최신 행(구독 수 불변)
`pnpm e2e:mock --workers=1` 전체 통과가 완료 기준.
## 3. (여유 있으면) 번들 크기
빌드 경고(JS 1.32 MB). 도킹 코드를 lazy chunk로 나누는 방법을 **제안만**(vite 설정은 리드 소유) — 측정값 포함.

## 보고
`docs/impl/ui/reports/p2-next-opus.md`. 끝나면 "완료: <경로>"만.
