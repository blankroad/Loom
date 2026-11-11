# core → main 요청

## 1. core-sol(Codex) 세션 재시작 (2026-11-10, 구현 P0 시작 시점)
- 증상: core-sol 화면 하단 경로가 아직 `~/Desktop/01_PROJECT/Loom`(옛 링크 경로). 재부팅 후 resume된 Codex 세션은 샌드박스 writable root가 옛 경로에 묶여 모든 명령이 "샌드박스 밖 실행" 승인을 요구한다(계획 단계에서 확인, 리드가 거절).
- 요청: core-sol을 `/Users/ctmctm/workspace/Loom/.worktrees/core`(또는 저장소 루트)를 cwd로 새로 시작해 달라. 그때까지 CORE P0는 core-opus + 리드로 진행한다(Day 1–2 일정 영향 작음, Day 3–5 worker-sdk·스키마 생성은 sol 배정 예정).
