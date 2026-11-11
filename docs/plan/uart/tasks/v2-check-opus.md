docs/plan/02-core-principle.md 준수 (최우선)

# v2.0 검증 — uart-opus (20분 내외, 수정 금지·점검만)
리드가 docs/plan/uart/plan.md 를 v2.0으로 통합했다(네 v2-opus.md와 uart-sol의 v2-sol.md 반영). 기준: docs/plan/00-master-plan.md §2.3 C1–C8(우선), docs/plan/04-v2-tasks.md, 02 원칙 체크리스트.
1. 02 체크리스트 6항목이 v2.0에서 성립하는지(특히 사용자 개입 경로가 웹 패널로 옮기며 빠진 곳, STOP·기록 장애 사용자 예외, 숨은 연결).
2. 내부 일관성: 절 번호 참조(§5.x는 새 번호, 구 TUI 규칙은 §5.15, 통합 테스트는 §9.6), §11 M1 수용 기준 ↔ §12 작업 ↔ §5 패널 명세 ↔ §2.0 E1, 이벤트 kind·RPC 이름(§3.9 ↔ §5 ↔ _xteam/uart-ui.md), M1/M2 배치 모순, TUI를 1차처럼 쓴 잔재.
3. docs/plan/_xteam/uart-ui.md 와 _xteam/uart-core.md 가 plan.md와 어긋나는 곳.
출력: docs/plan/uart/v2-check-opus.md, 맨 끝 "리드 수정 목록"(필수/권장, 각 항목에 plan.md 위치와 대체 문안). 짧게. 완료 시 "V2 CHECK DONE" 한 줄.
