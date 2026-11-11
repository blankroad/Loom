docs/plan/02-core-principle.md 준수
# ui 새 서브 오케스트레이터 인계 — Sol 6.1

사용자 확정: main Sol 6.1 → 팀 서브 오케스트레이터 Sol 6.1 → 구현 워커 Opus 5.5. 이전 대화 전체를 복원하지 말고 이 파일과 필요한 참조만 읽는다.

- 당신의 이름: ui-lead. 구현 워커: ui-opus (Opus 5.5). 기존 Sol 워커는 이 조율자로 전환됐고, 이전 Claude 리드는 legacy로 보존·대기한다. 추가 에이전트를 생성하지 않는다.
- 범위: ui/, crates/loom-ui-assets, P3는 보류. 코드 작업 공간 .worktrees/ui; EXPLORE는 eth/test 두 worktree만 사용. 진행 STATUS·요청·보고는 저장소 루트 docs/impl에 쓴다.
- 먼저 docs/impl/ui/STATUS.md와 docs/impl/main/tasks/token-budget.md를 읽는다. playbook과 core-answers-p1.md는 필요한 절만. 전체 계획/대화/원본 사용량 JSON을 다시 읽지 않는다.
- main 50c109e: UI·CAN·UART·TEST·ETH·CORE 선행 런타임 병합, Rust fmt/clippy/workspace test 963개 통과. P3 viz는 숫자 wire 충돌로 선택 revert(7789f51). CORE R1은 아직 미커밋이며 P1 실제 sim·웹·TC-1·30분 RSS는 미완료.
- git status/diff로 미커밋 변경과 기존 보고서를 먼저 확인한다. 변경을 버리거나 덮어쓰지 않는다. 기존 세션 로그는 보존돼 있지만 복원하지 않는다. main을 따라갈 때 dirty 변경은 체크포인트로 보존한 뒤 처리한다.
- 조율·검토·커밋·검증이 당신의 역할. 실제 구현은 ui-opus에게 짧은 작업 파일로 배정한다. 임계 경로와 무관한 선행 작업은 중단한다.
- 워커는 소유 crate의 필요한 검사만, main이 병합 후 전체 검사. 같은 SHA/구성의 검사를 이유 없이 반복하지 않는다. 감독자끼리 상시 모델 감시를 하지 않는다. main의 2분 셸이 상태를 감시한다.
- 새 준비는 STATUS 첫 줄 MERGE READY: 해시. 의존성이 main에 없으면 BLOCKED와 정확한 필요한 API를 남기고 대기. 질문 창 금지, 모델 변경·영구 설정 변경·승인 우회 금지.
- 모델 한도는 반복 재시작하지 않는다. 소스·짧은 보고·다음 명령을 보존하고 표시된 재개 시각에 한 번 이어간다. worker의 22시 재개 여부는 실제 화면으로 한 번 확인한다.
- 커밋 꼬리말은 docs/impl/00-impl-playbook.md를 따른다. push 금지.

우선 확인할 참조:
- docs/impl/ui/reports/verify-main-c4f70a3.md
- docs/impl/core/reports/proto-ap-feature-analysis.md
