# TEST P1 — explore-opus: `crates/loom-test-predicate` (술어 AST + 증거 검증 코어)

먼저 `p1-common.md`와 다음을 읽는다:
- `docs/plan/test/plan.md` §3.1, §6.3–6.4(R-1..R-9), 결정 T3·T10–T13
- 계약 `07-test-session.md` §7.2–7.4
- `crates/loom-domain-api/src/verify.rs`(Predicate·CoverageNeed·Coverage·EventRef·Verification·EvalBudget·EvidenceVerifier)
- `crates/loom-proto/src/evidence.rs`·`event.rs`
- 참고 구현: `spikes/explore/v2-evidence/`(hold·gap 10사례), `crates/loom-eth/src/gateway/evidence.rs`(eth 도메인 검증기 — 같은 원칙)

배정 범위: 새 crate `crates/loom-test-predicate/`만. 이 crate는 **순수 라이브러리**다(I/O 없음). CORE 세션 엔진이 저널에서 창 전체를 다시 읽어 `&[PublishedRecord]`와 `Coverage`를 넘겨주면, 이 crate가 판정한다. CORE가 그대로 가져다 쓸 수 있게 만든다.

## 1. 비교 AST (TEST 명세 = 정본)
- 노드:
  - 비교: `{field: <JSON Pointer>, op: == != < <= > >= in regex contains between bytes_eq, value}`
  - 논리: `all[] any[] not`
  - 단위: 문자열 `"5 s"`·`"150 ms"`, 숫자+단위 필드는 도메인 `PointerSpec.unit`과 대조
  - 타입: `U64` 문자열(계약 §6.1)도 숫자로 읽는다(eth `json_u64`와 같은 규칙).
- 파서: `serde_json::Value`(plan YAML에서 변환됨) → AST. 오류에 경로(`steps[2].expect[0].pred.where.all[1]`)·기대·실제를 붙인다.
- 타입 검사: 도메인이 등록한 `PointerSpec{path, unit, ty}` 목록을 넘겨받아 필드 존재·타입·단위를 검사한다(모르는 필드 = 오류).
- **사람용 문장 렌더링**(계획 승인 화면용, 한국어): 예 `VehState.PowerMode == 2` → "신호 VehState.PowerMode가 2", within → "S2 매치 후 5 s 안에".
- 상한: 깊이 32, 노드 512, `in` 원소 256, 정규식 1 KiB / size_limit 256 KiB(Rust `regex`).

## 2. 공통 술어 평가기 (계약 07 §7.3-5, TEST §3.1)
- 대상: `exists`, `absent`, `count{min,max}`, `order{a,b}`, `within_ms{anchor, max}`, `hold{min_ms, max_gap_ms=150}`(같은 source·신호·generation 연속 샘플), `measure{from,to}`(검증기가 `ts.mono_ns` 차이로 직접 계산).
- 각 술어의 `CoverageNeed`(Positive/Complete/Measure)를 정한다. Complete·Measure는 `Coverage.complete == false`면 `Inconclusive{cause}`.
- **전수 평가**: 넘겨받은 창의 모든 이벤트를 필터·AST로 평가한다. witness는 결과일 뿐 입력이 아니다.
- **역할 검사**:
  - 관측 증거 origins 기본 `["device"]`
  - `attributed_origin`(eth)·origin agent/script 송신만으로는 pass 불가(R-8)
  - 요청·응답은 `request_id`로 짝을 짓는다
  - 창 안에 같은 domain·channel·target 사용자 쓰기가 있으면 `Inconclusive{"user_intervention"}`
- 결과는 `loom_domain_api::Verification`이다. `EvalBudget`을 협력적으로 쓰고, 초과 시 `Inconclusive{"eval_budget"}`.
- digest: 일치 이벤트 집합(부재면 expect·창·필터·coverage)의 sha256. JCS는 serde_json을 정렬 키로 직렬화하는 단순판으로 하고, 결정을 보고서에 적는다. sha2 crate는 exact 버전으로 쓰고 CORE 요청 목록에 적는다.

## 3. 판정 규칙 엔진 (TEST R-1..R-9, 계약 §7.4)
- 단계 판정 함수: `(제안 판정, 증거 검사 결과들, 단계 종류{assert|stimulus|manual}, record_health) → 확정 판정 | 거절(E_EVIDENCE_MISSING|E_EVIDENCE_MISMATCH|E_OUT_OF_WINDOW|E_RECORDING_FAILED|E_PLAN_MISMATCH)`
  - 자극 전용 = `done`
  - 근거 없는 fail = `unsupported:true`
  - 사용자 판정 = 별도 타입
- 세션 결과 집계: `passed|failed|inconclusive|incomplete`(필수 단계 미판정, 사용자 필수 skip은 최대 inconclusive, `on_fail: goto` 대체 규칙) → **종료 코드 0/11/8/6**.

## 시험
- 표 기반 ≥ 60사례:
  - 존재·창·술어·gap·epoch 변경
  - user 개입, agent tx only, request_id 불일치
  - hold 끊김·간격 초과, measure 경계 ± 오차, 누락 필드 오류, 예산 초과
  - 판정 집계·종료 코드
- 스파이크 10사례를 이식한다.
- 무작위 AST JSON 10만 회 panic 없음.

## 보고
`/Users/ctmctm/workspace/Loom/docs/impl/test/reports/p1-opus-predicate.md`(50줄 이하: 공개 API, 상한 표, 결정, CORE가 이 crate를 쓰는 방법, 요청). 커밋하지 않는다.
