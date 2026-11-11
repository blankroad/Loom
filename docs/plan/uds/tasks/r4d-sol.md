docs/plan/02-core-principle.md 준수

# R4d 지시 — uds-sol: B4만 재확인 (plan.md 수정 금지)

리드가 `r4c-final.md`의 B4 지적을 plan.md §3.4(quarantine 회복 관문), D19, U-06, §9.3 러너 시험에 반영했다: 프로브를 "이번 quarantine 구간의 미응답 요청 전부와 응답 식별자가 겹치지 않는 R0 요청"으로 고르고, 순차 처리 가정은 프로필 HIL에서 확인(`isolation: assumed`), 후보 부족·실패 시 DoIP 재연결 / CAN `isolation: unverified`(R0만), 반례를 U-06 시험에 포함.

B4가 해소됐는지, 새 모순이 있는지 확인해 `docs/plan/uds/r4c-final.md` 맨 아래에 `## R4d 추가 확인` 절(20줄 이내)을 덧붙이고, 그 절의 첫 줄에 `FINAL PASS` 또는 `FIX NEEDED: <요지>`를 쓴다. 완료 응답: "DONE r4d-sol".
