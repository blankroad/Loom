DONE v2.0
- 산출물: `docs/plan/test/plan.md`(TEST 기획 v2.0, 15절 + 핵심 루프 표 + §3.1 증거 검증 모델 + 결정 T0–T14). 입력은 `r1-opus.md`(구조·UX·스킬·예시 TC)·`r1-sol.md`(사실 확인·Rust 스파이크)·`r4-sol-check.md`(점검 16건 반영).
- 사람은 TC를 자연어나 `.tc.md`(엑셀·CSV·Gherkin 가져오기)로 쓰고, 에이전트가 해석 계획을 만들어 UI에서 확인·**일괄 승인**받은 뒤 `loom` 명령으로 실행한다. TC마다 코드를 짜지 않는다(C7).
- **증거 기반 판정**: 인용 = 범위 + 필터 + 술어이고, Loom 검증기가 **필터된 범위 전체를 저널에서 다시 읽어** 판정한다. 관측 증거는 device 출처만 인정하고, 자기 송신만으로는 pass할 수 없다. gap·사용자 개입·시각 오차가 있으면 inconclusive다. 증거 없는 PASS는 불가능하다.
- 시간 창은 TC 문장 기준(anchor + within_ms)이다. 위험 요약은 Loom이 산출하고, 계획이 바뀌면 전체를 다시 승인한다. 종료 코드는 0 passed / 11 failed(D5 신규 요청) / 8 inconclusive / 6 STOP이다.
- 술어는 작은 비교 AST + 시간·hold 연산자다(CEL은 M3 탈출구). 보고서는 quick-junit + askama HTML이다. Python은 쓰지 않는다.
- `loom-test` 스킬 초안(§4.1), UI 테스트 실행 화면 명세(`_xteam/ui-test.md`), 예시 TC 3개(TC-1이 sim M1 E2E)를 담았다.
- 합의: CORE `core-test.md`(O1–O9 수용 + 추가 요청 8개), UART·UDS `test-domains.md`(수용, 답 작성). **CAN 답 대기**, UI 의견 대기.
- 남은 결정 요청: D5 종료 코드 11, A9 확장(`max_test_duration`). 둘 다 메인·CORE 소관.
