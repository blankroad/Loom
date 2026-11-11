# test → core 요청 (2026-11-11, explore-lead — TEST 구현 착수)

메인 지시로 TEST가 증거 검증·술어 AST·.tc.md 파서·세션 CLI를 구현한다(계약 07의 "검증기는 P2(CORE)"를 앞당김). 소유 경계는 이렇게 제안한다. CORE 판단을 `## core 답변`에 남겨 달라.

| 부분 | 위치 | 소유 |
|---|---|---|
| 술어 AST·공통 술어 평가·역할 검사·판정 규칙·세션 결과/종료 코드 | `crates/loom-test-predicate`(순수 라이브러리) | TEST. **CORE 세션 엔진이 이 crate를 그대로 호출**(저널 창 재조회·pin·fence는 CORE) |
| .tc.md·plan YAML 파서, 정적 검사, 위험 요약(`RiskProvider` trait), plan hash | `crates/loom-test-format` | TEST |
| 세션 RPC 모듈(`test.*`)·`loom test` CLI 조각 | `crates/loom-test`(다음 라운드) — `Registry::for_core("test")` 사용 | TEST가 만들고 **CORE가 `loomd`·`loom`에 등록**. 예약 domain이라 CORE 승인 필요 |

요청:
1. 위 경계에 동의 여부, 그리고 `Registry::for_core("test")`를 TEST crate가 써도 되는지(안 되면 CORE가 얇은 래퍼 모듈을 소유하고 TEST 라이브러리를 호출).
2. workspace.dependencies 추가: `sha2`(plan hash·증거 digest), YAML crate 1종(`serde_yaml` 후계 — loom-test-format 보고서에서 선택), `pulldown-cmark`(.tc.md 표). 정확한 판은 보고서가 나오면 이 파일에 덧붙인다.
3. JCS(정렬 키 JSON) 정규화 함수가 `loom-proto`에 생기면 TEST는 그것으로 바꾼다(지금은 각 crate 단순판, 결과 동일성 시험으로 묶음).
4. TC-1 sim E2E(P1 관문)에 필요한 것: gateway 저널 연결, `loomd worker <kind>`, 세션 마커(`test.step_started/ended` + fence), 도메인 등록(uart·can·uds·psu). 준비되면 `docs/impl/core/STATUS.md`에 표시해 주면 TEST가 E2E를 붙인다.

## 추가 (2026-11-11, TEST P1 crate 완료 후)
- workspace.dependencies 정확한 판: `sha2 = "=0.11.1"`, `serde_norway = "=0.9.42"`(serde_yaml은 archived — 후계 fork), `pulldown-cmark = "=0.13.6"`, `regex = "=1.13.1"`(eth-filter·test-predicate 공통).
- `serde_norway`는 unsafe가 들어 있는 `unsafe-libyaml-norway`에 의존한다. loom-test-format이 토큰·깊이·크기 사전 검사로 입력을 제한한다. CORE가 다른 YAML crate를 정하면 TEST가 따라 바꾼다.
- 해시 규약: `canonical_json`(키 정렬·serde_json 직렬화·공백 없음) + sha256 소문자 hex. format·predicate 두 crate에 같은 구현이 있고 바이트 동일성 시험이 있다. `loom-proto`에 정본이 생기면 그것으로 바꾼다.
- 세션 엔진 연결 방법은 `docs/impl/test/reports/p1-opus-predicate.md` "CORE가 쓰는 방법"을 참고한다.
  - 승인 시: `Citation::parse` + `check(PointerSpec)` + `render()`
  - 검사 시: 창 전체 재조회 → `evaluate`
  - 판정 시: `judge_step`·`aggregate` → 종료 코드
  - 또는 `CommonVerifier`를 Registry `evidence_verifier`로 등록

## 추가 (2026-11-11, test 071f298 — 검증기 연결 후)
- 세션 엔진은 `PredicateEngine::from_registries(&[&Registry…])`(도메인 `EvidenceVerifier` + 모든 `PointerSpec`)로 만들어 `TestModule::with_services`에 넣는다. `TestModule::new()` 기본은 공통 술어만이다.
- R1에서 필요한 것:
  1. `CallCtx::now_mono_ns()` + mono 시각 커서 — fence 시각부터 정확히 스캔(지금은 상대 커서 + 1 s 여유).
  2. 단계 창 pin API — 지금 결과는 `pinned:false`(verified여도 보존 보호 없음).
  3. `SessionHooks::fence`(worker별 source fence) — **시작 fence가 없으면 PASS 불가**로 구현했다.
  4. `repeat`의 반복별 fence — 없으면 repeat > 1 단계는 거절.
  5. `loom-proto::canonical`이 오면 digest·plan hash를 그쪽으로 옮긴다.
- gap 이벤트 domain이 지금 `source`로 나와 TEST는 채널만으로 gap을 모은다(보수적). 표준 gap 발행기가 영향받은 domain·channel을 넣으면 그대로 맞춘다.
