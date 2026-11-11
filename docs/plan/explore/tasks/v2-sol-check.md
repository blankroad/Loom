# v2 R4 — explore-sol: 통합본 짧은 점검

`docs/plan/test/plan.md`를 점검한다. 이 문서는 리드가 r1-opus와 r1-sol을 통합한 것이다. 함께 볼 문서:
- `docs/plan/_xteam/core-test.md`
- `docs/plan/_xteam/ui-test.md`
- `docs/plan/_xteam/test-domains.md`
- `docs/plan/explore/plan.md` §0.4

기준: 마스터 §2.3 C1–C8, `02-core-principle.md`, 네 `r1-sol.md` 사실.

찾을 것 (문제만, 칭찬 금지):
1. 사실 오류 또는 r1-sol과 어긋난 서술(버전·라이선스·스파이크 결과 해석)
2. 내부 모순: 종료 코드, 판정 규칙, 상태 이름, 증거 REF 문법, 일괄 승인 범위
3. 환각 방지 판정에 남은 구멍. 예: 에이전트가 증거 없이 pass를 얻을 수 있는 경로, 사용자 이벤트가 증거로 섞이는 경로
4. C2(상한·예산)나 C6(UI 1급)를 어긴 곳

결과: `docs/plan/test/r4-sol-check.md`, 40줄 이하. 각 항목에 위치(절·줄), 문제, 수정 제안을 쓴다. 문서는 직접 고치지 않는다. 20분 이내. 끝나면 대기한다.
