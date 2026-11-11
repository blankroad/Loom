docs/plan/02-core-principle.md 준수

# R4e 지시 — uds-sol: B4 최종 확인 (plan.md 수정 금지)

리드가 R4d 지적을 반영해 plan.md §3.4 quarantine 규칙을 **응답 식별자 기반 격리**로 다시 썼다(미해결 집합 U, SID 겹침 = 차단(R0 포함), 해제 조건 = 늦은 응답 도착/DoIP 재연결/리셋 확인/검증 프로필 late_response_max_ms, 프로브는 생존 확인일 뿐, 겹침 상태 수신은 confirmed 반영 금지). D19, §9.3 러너 반례, U-06, §10.7 recover, 종료 코드 6(QUARANTINE), 스킬 exit 6 분기도 갱신했다.

B4가 해소됐는지와 새 모순 여부를 `docs/plan/uds/r4c-final.md` 맨 아래 `## R4e 추가 확인` 절(15줄 이내)로 덧붙이고, 첫 줄에 `FINAL PASS` 또는 `FIX NEEDED: <요지>`. 완료 응답: "DONE r4e-sol".
