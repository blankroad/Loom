# TEST P1 — explore-sol: `crates/loom-test-format` (.tc.md · plan YAML 파서·검증)

먼저 `p1-common.md`와 다음을 읽는다:
- `docs/plan/test/plan.md` §2(TC-1 계획 예), §3.1 "계획 스키마 하나", §6.1–6.2, §6.5 freeze, 결정 T1·T2·T12
- 계약 `07-test-session.md`
- UDS 시나리오 형식 `loom_scenario: 1`(`crates/loom-uds-runner`와 `docs/plan/uds/plan.md`)

배정 범위: 새 crate `crates/loom-test-format/`만. 순수 라이브러리다(파일 읽기는 호출자 몫, 입력은 `&str`/`&[u8]`).

## 1. 의존 결정 (먼저, 10분 이내)
- YAML: `serde_yaml`이 유지보수 중단인지 확인하고, 대안(`serde_yaml_ng`, `serde_norway` 등)의 판·라이선스·최근 릴리스를 crates.io에서 확인해 하나를 고른다. Markdown: `pulldown-cmark`(GFM 표). 둘 다 exact 버전으로 둔다.
- 네트워크·다운로드가 막히면 보고서에 적고 멈춘다(리드가 대신 받는다).

## 2. `.tc.md` 파서 (계획 §6.1)
- 형식: 머리말 YAML(`loom_tc: 1`, `id, title, env, equipment[], preconditions[], tags[]` — **모두 선택**) + 본문.
- 본문 처리:
  - 자유 서술: `## 설명` 등
  - `## 단계` 표: 열 이름 한/영 별칭 — `#|번호|Step`, `동작|Action`, `기대 결과|Expected`, `합격 기준|Pass criteria`
  - 표가 없어도 유효(자연어만).
- 결과: `TcDoc{meta, description, steps[{n, action, expected, criteria, source_line}], source_sha256}`. 원문 문장은 고치지 않고 위치와 함께 보존한다.
- 상한: 파일 ≤ 1 MiB, 단계 ≤ 500, 셀 ≤ 4 KiB. 넘으면 위치 있는 오류.

## 3. `.plan.yaml` 파서·정적 검사 (`loom_tc_plan: 1`, 계획 §3.1)
- **단계 스키마 하나**: `{id, tc_step?, <domain>.<verb>: {args} | do: [{<domain>.<verb>: {args}}…], expect?: [{filter{domain, channel?, kinds[], origins?, request_id?}, pred, anchor?, within_ms?}], on_fail?{continue|goto: id|stop_tc|stop_session}, when?{step, verdict}, retry?{count ≤ 3}, manual?{prompt}, repeat?{count ≤ 100}, measure?}`, 그리고 `finally: [...]`.
- `loom_scenario: 1` 단계는 이 스키마의 부분집합으로 읽혀야 한다(UDS runner 시나리오 1개를 시험 fixture로).
- `pred`는 JSON 값 그대로 보관한다(해석은 `loom-test-predicate`). 여기서는 모양만 검사한다.
- **정적 검사 `check_plan(plan, &CommandTable) -> Report`**:
  - 동사가 명령표에 있는지, 인자가 객체인지
  - 모든 대기형 단계에 timeout이 있는지
  - id 중복·`goto`/`when` 대상 존재·순환
  - 단계 ≤ 500, finally는 R0–R1 + safe_state만(등급 조회는 trait)
  - **위험 요약**: `RiskProvider` trait(`classify(method, args) -> Risk`)으로 계산한다. finally·repeat 횟수·retry 최대·분기 양쪽을 모두 포함한다(계획 T12). 에이전트가 쓴 `risk_summary` 키가 있으면 무시하고 경고를 단다.
- **plan hash**: 정렬 키 JSON(JCS 단순판) → sha256 hex. `tc` 원문 hash와 함께 둔다. explore-opus의 predicate crate와 같은 방식을 쓰도록 보고서에 방식을 적는다(둘 다 sha2 exact).

## 4. fixture (계획 §2의 예시 TC 3개)
- TC-1(콜드 부팅 스모크: PSU+UART+CAN+UDS), TC-2(DLT+LA+UART), TC-3(PSU 반복·measure)을 `.tc.md`와 `.plan.yaml`로 `crates/loom-test-format/tests/fixtures/`에 둔다.
- TC-1 계획은 계획 §2의 YAML(§3.1 정본 스키마로 고친 것)을 그대로 쓴다.
- 모두 파싱·정적 검사 green이어야 한다.
- 일부러 깨진 fixture(goto 순환, timeout 없음, 단계 501개, 표 열 누락 등)는 위치 있는 오류를 내야 한다.

## 시험
- 위 fixture
- 한/영 열 별칭, 머리말 없음, 표 없음
- 무작위 입력 10만 회 panic 없음

## 보고
`/Users/ctmctm/workspace/Loom/docs/impl/test/reports/p1-sol-format.md`(50줄 이하: 의존 결정 근거, 공개 API, 상한, 결정, 요청). 커밋하지 않는다.
