docs/plan/02-core-principle.md 준수

# R4c 지시 — uds-sol: B1–B8 반영 확인 (아주 짧게, plan.md 수정 금지)

리드가 `docs/plan/uds/r4b-recheck.md`의 B1–B8을 `docs/plan/uds/plan.md`에 반영했다(주요 변경: §3.5 사용자 R1+ 요청 = 암묵 takeover, §2.0 t3/t4, §3.4 동기화 프로브 회복 관문, §4.1 `others_since_last.from_cursor`, §4.2 probe·approval wait M1·recover 행 삭제, §3.6 `uds.confirm` 단일화·`uds.recover` user 전용, §10.7 recover 순서, M3b 표기 통일, U-06/U-10/U-15/U-19, Q6, U2).

1. B1–B8 각각 해소 여부(Y/N + 위치 + 남은 문제 한 줄).
2. 이번 변경으로 생긴 새 모순이 있으면 목록(없으면 "없음").
3. 02 체크리스트 6개 항목 최종 판정.
4. 맨 위 한 줄: `FINAL PASS` 또는 `FIX NEEDED: <요지>`.

출력: `docs/plan/uds/r4c-final.md` (60줄 이내). 완료 응답: "DONE r4c-sol".
