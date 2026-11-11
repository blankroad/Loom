docs/plan/02-core-principle.md 준수

# R4f 지시 — uds-sol: U 해제 조건 확인 (plan.md 수정 금지)

R4e 지적 반영: §3.4 규칙1 해제 조건 = 완전한 최종 응답(0x78 제외)의 late 도착 / DoIP 재연결 / ECU 리셋 확인(DoIP·CAN 공통) / 검증 프로필 late_response_max_ms. `7F <SID> 78`·미완성 PDU는 U 유지. 규칙6 CAN 해제 조건 통일, D19와 §9.3 반례(늦은 7F 22 78) 갱신.

`docs/plan/uds/r4c-final.md` 맨 아래 `## R4f 추가 확인` 절(10줄 이내), 첫 줄 `FINAL PASS` 또는 `FIX NEEDED: <요지>`. 이번에는 B4와 이번 변경에 한정하고, 새로 범위를 넓히지 말 것(남는 사소한 개선은 P2로 한 줄만). 완료 응답: "DONE r4f-sol".
