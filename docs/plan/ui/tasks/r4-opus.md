docs/plan/02-core-principle.md 준수.

# R4 — ui-opus 최종 점검 (ui-sol은 샌드박스 경로 문제로 BLOCKED라 opus가 맡음)

대상: `docs/plan/ui/plan.md` (리드 통합본 v2.0). 출력: `docs/plan/ui/r4-check.md` (한국어, 100~250줄). `plan.md`는 고치지 말 것 — 리드가 반영한다.

점검 기준:
1. `docs/plan/02-core-principle.md` 체크리스트 6항목 — 각 항목 통과/미흡 + 근거 절 번호.
2. `docs/plan/00-brief.md` §4의 15절이 모두 있고 내용이 비어 있지 않은지.
3. `docs/plan/ui/brief.md` 범위 1~8 각각이 어디서 다뤄졌는지.
4. **내부 일관성**: 지연 수치(§0.2 ↔ §11 UI-T03), 메모리 상한(§6.5 ↔ 다른 절), M 배정(§1.2 ↔ §5 ↔ §11 ↔ §12), RPC·이벤트 이름(§4.5 ↔ 본문), 결정 기록(§16)이 본문과 모순 없는지.
5. 당신의 R1·R1b·R2 지적 중 반영이 빠지거나 잘못 반영된 것.
6. 타팀 명세(`_xteam/{can,uart,uds,explore}-ui.md`, `ui-test.md`, `core-ui.md`의 core 의견, `core-test.md`)와 아직 충돌하는 것.

형식: 문제마다 `| # | 심각도(높음/중간/낮음) | 위치 | 문제 | 수정 제안 |` 표. 마지막에 "DONE 가능 여부" 한 줄.
규칙은 `tasks/r1-common.md` §4. 끝나면 "완료: <경로>"만 답한다.
