docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R4 최종 점검 — explore-opus

리드가 R3 통합본 `docs/plan/explore/plan.md`를 썼다. 이것을 점검해 `docs/plan/explore/r4-check.md`로 보고한다. **plan.md는 직접 수정하지 말 것** (리드가 반영).

## 점검 기준
1. `docs/plan/02-core-principle.md` 체크리스트 6개 — 추천 5개 각각 실제로 만족하는가(형식이 아니라 내용). 특히 실시간 목표의 근거 표현, 고부하 정직성, M1 범위의 현실성, late viewer.
2. `docs/plan/explore/brief.md` 산출물 5개 항목(기준표, 평가표 열 전부, 상위 3–5개 각 a~j, 플러그인 구조, 착수 순서·질문).
3. `docs/plan/00-brief.md` §4의 15개 절 중 이 컴포넌트에 해당하는 것(범위, 시나리오, CLI, 뷰어, 라이브러리 근거, HW/OS, 시뮬레이션·테스트, 안전, 마일스톤, 작업 분해(담당·의존성), 리스크, 질문, 공통 계약 변경 요청)이 빠짐없이 있는가. 빠진 것(예: HW/OS 지원 매트릭스, 구현 작업 분해의 병렬 단위)이 있으면 무엇을 넣어야 하는지 구체 문장으로 제안.
4. 두 R1, 두 R2 리뷰(`r2-opus-review.md`, `r2-sol-review.md`)의 P0/P1 지적이 plan.md에 반영됐는가 — 누락 목록.
5. 세 팀 plan.md(`docs/plan/uart|can|uds/plan.md`)와 충돌하는 서술(이름, 등급 의미, origin, IPC, 파일 이름, 시점)이 있는가.
6. 내부 모순(표 점수 계산, 절 번호 참조, 결정 기록과 본문 불일치, TOML 예시 파싱 — python3 tomllib로 확인 가능).

§5.5(스코프/LA)의 장비 사실은 explore-sol이 별도 검증 중이므로 사실 검증은 생략하고 구조·원칙 적합성만 본다.

## 출력
- `docs/plan/explore/r4-check.md` (한국어, 150~300줄): 1) 반드시 고칠 것(P0/P1, 위치·수정 문장) 2) 권장 보완 3) 체크리스트 판정표 4) 세 팀과의 충돌 목록.
- 파일 쓰기는 `docs/plan/explore/r4-check.md`와 `spikes/explore/`만. 마지막 줄 `<!-- END R4 -->`, 화면에 "R4 DONE: <경로>" 한 줄.
