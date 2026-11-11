docs/plan/02-core-principle.md 준수

# R2 지시 — uds-sol: 교차 리뷰

1. 상대 초안 `docs/plan/uds/r1-opus.md`를 읽는다. 기준 문서: `docs/plan/02-core-principle.md`(최우선), `docs/plan/00-brief.md` §3·§4, `docs/plan/uds/brief.md`, `docs/plan/_xteam/can-uds-isotp.md`(리드 초안), `docs/plan/_xteam/viz-requirements.md`의 `## uds` 절.
2. `docs/plan/uds/r2-sol-review.md`에 아래 순서로 쓴다 (한국어, 간결하게, 절 번호로 위치 표시):
   - **A. 핵심 원칙 위반/미흡**: 02 체크리스트 6개 항목별로 상대 초안의 충족 여부. 특히 에이전트 전용 숨은 연결, 사용자 실시간 표시 경로가 빠진 곳.
   - **B. 틀린 사실**: 라이브러리 버전/라이선스/API, ISO 14229/13400/15765 내용, 타이밍 값. 근거(URL, 표준 조항, 스파이크 결과)와 함께.
   - **C. 빠진 것**: 브리프 §4 15개 절 기준 + UDS 브리프 "꼭 다룰 것" 기준.
   - **D. 더 나은 선택**: 상대안보다 당신 안(또는 제3안)이 나은 지점과 이유. 반대로 상대안이 더 나아서 당신이 양보할 지점도 명시.
   - **E. 합의 가능한 부분**: 그대로 plan.md에 넣어도 되는 결정 목록.
   - **F. 리드 결정 필요 쟁점**: 남은 의견 차이를 "쟁점 / 내 입장 / 상대 입장 / 판단 근거" 표로.
   - 리드의 xteam 초안 두 개에 대한 의견이 있으면 G 절에 짧게.
3. 자기 초안은 수정하지 않는다. 다른 파일 쓰지 말 것 (`docs/plan/uds/r2-sol-review.md`만).
4. 완료 응답: "DONE r2-sol" 한 줄.
