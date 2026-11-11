docs/plan/02-core-principle.md 준수
# CAN 다음 작업 — 리드 한도 중 P1 우선순위

can-opus는 can.open/demo를 마치고 멈췄다. 리드는 22시까지 한도로 대기 중이다. 기존 load 작업을 마친 can-sol은 다음을 이어간다.

1. 기존 미커밋 변경과 보고서부터 확인해 겹치는 파일 편집을 피한다.
2. .worktrees/can/docs/impl/can/tasks/p1-features.md의 gateway/worker/cli 분리를 기존 리드 지시대로 구현한다. CLI는 장치 crate를 링크하지 않는다.
3. main만 따라가며 core/main 직접 병합 금지. CORE R1 API는 보고서/소스로 참고하되 새 API가 main에 오기 전 검증 불가능한 항목은 차단으로 기록한다.
4. 변경·검증 결과를 루트 docs/impl/can/reports/p1-features-sol.md에 보고하고 멈춘다. 커밋은 메인 검토 후 한다. 질문 창 금지.
