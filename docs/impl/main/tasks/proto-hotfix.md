docs/plan/02-core-principle.md 준수
# CORE 긴급 통합 수정 — main c4f70a3 MessagePack 시험

- 메인에서 workspace 시험은 실패하지만 proto 단독은 통과한다. loom-viz가 serde_json arbitrary_precision을 켜 숫자 직렬화 경로가 바뀌는 조합이 원인 후보다.
- core-sol은 CORE 소유 파일에서만 문제를 재현하고 최소 수정한다. unknown-field 계약·정수 정밀도·실제 worker IPC의 payload 왕복을 검증한다. 시험 생략·완화로 성공시키지 않는다.
- 다른 R1 변경과 독립된 수정 파일/패치를 보고한다. **커밋하지 않는다.** 메인이 검토한 최소 수정만 별도 통합 커밋으로 만들어 main을 녹색으로 복구한 후 R1을 이어간다.
- 진행 중인 CORE R1 코드·타팀 worktree를 덮어쓰지 않는다. 질문 창 금지. 한도에 걸린 리드는 22시 자동 재개 예정이며, main과 진행 가능한 sol 워커는 계속 작업한다.

## 19:43 판단

main은 P1 범위 밖인 P3 선행 d16f32d만 선택 revert했다. loom-viz의 전역 arbitrary_precision이 원인이며, 단순히 시험을 바꿔 통과시키면 MessagePack에 serde_json 내부 Number map이 섞이는 상호운용 영향이 남을 수 있다. UI 브랜치에 원본 구현을 보존한다. 현재 proto 시험 변경은 본선에 넣지 않는다. 분석과 최소 재현을 보고서에 남기고 CORE R1 통합 검토를 이어간다.
